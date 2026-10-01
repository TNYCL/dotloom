//! Copy/paste with reference remapping.
//!
//! Policy (documented in `docs/file-format.md`):
//!
//! * Copied entities get new IDs; references between copied entities (properties,
//!   constraints, groups) are remapped to the new IDs.
//! * Constraints are copied only when *all* their entities are copied.
//! * A property reference to an entity outside the copied set is kept when that
//!   entity exists in the destination document (e.g. a pasted door keeps its host
//!   wall) and dropped otherwise; dropped references are reported.
//! * Groups are copied when all their members are copied.
//! * The paste offset is applied to entity transforms and to `fixPoint` targets.

use std::collections::{BTreeMap, BTreeSet};

use dotloom_geometry::{Affine, Vector};
use serde::{Deserialize, Serialize};

use crate::{Constraint, ConstraintId, Document, Entity, EntityId, Group, GroupId, LayerId, PropValue, RuleSpec};

/// Copied content, independent of the source document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Clipboard {
    /// Entities in draw order.
    pub entities: Vec<Entity>,
    /// Constraints among the copied entities.
    pub constraints: Vec<Constraint>,
    /// Groups whose members were all copied.
    pub groups: Vec<Group>,
}

/// Result of a paste.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PasteReport {
    /// Old → new entity IDs.
    pub entities: BTreeMap<EntityId, EntityId>,
    /// New constraint IDs.
    pub constraints: Vec<ConstraintId>,
    /// New group IDs.
    pub groups: Vec<GroupId>,
    /// `(new entity, property)` references dropped because the target is missing.
    pub dropped_refs: Vec<(EntityId, String)>,
}

impl Document {
    /// Copy entities (and the constraints/groups fully inside the selection).
    #[must_use]
    pub fn copy(&self, ids: &[EntityId]) -> Clipboard {
        let set: BTreeSet<EntityId> = ids.iter().copied().filter(|id| self.entity(*id).is_some()).collect();
        let entities: Vec<Entity> = self.entities().filter(|e| set.contains(&e.id)).cloned().collect();
        let constraints = self
            .constraints()
            .filter(|c| {
                let ents = c.rule.entities();
                !ents.is_empty() && ents.iter().all(|e| set.contains(e)) && c.owner.is_none_or(|o| set.contains(&o))
            })
            .cloned()
            .collect();
        let groups = self
            .groups()
            .filter(|g| !g.members.is_empty() && g.members.iter().all(|m| set.contains(m)))
            .cloned()
            .collect();
        Clipboard { entities, constraints, groups }
    }

    /// Paste clipboard content with new IDs, translated by `offset`. Entities whose
    /// layer does not exist here are placed on `fallback_layer`.
    pub fn paste(&mut self, clip: &Clipboard, offset: Vector, fallback_layer: LayerId) -> PasteReport {
        let mut report = PasteReport::default();
        for e in &clip.entities {
            let new = EntityId(self.alloc_id());
            report.entities.insert(e.id, new);
        }
        let t = Affine::translate(offset);
        for e in &clip.entities {
            let Some(&new_id) = report.entities.get(&e.id) else { continue };
            let mut n = e.clone();
            n.id = new_id;
            if self.layer(n.layer).is_none() {
                n.layer = fallback_layer;
            }
            n.transform = n.transform.then(t);
            let mut dropped = Vec::new();
            for (k, v) in &mut n.props {
                match v {
                    PropValue::Ref(r) => {
                        if let Some(m) = report.entities.get(&r.entity) {
                            r.entity = *m;
                        } else if self.entity(r.entity).is_none() {
                            dropped.push(k.clone());
                        }
                    }
                    PropValue::Anchor(a) => {
                        if let Some(m) = report.entities.get(&a.entity) {
                            a.entity = *m;
                        } else if self.entity(a.entity).is_none() {
                            dropped.push(k.clone());
                        }
                    }
                    _ => {}
                }
            }
            for k in dropped {
                n.props.remove(&k);
                report.dropped_refs.push((new_id, k));
            }
            self.insert_entity(n);
        }
        for c in &clip.constraints {
            let mut n = c.clone();
            if !n.rule.remap(&report.entities) {
                continue;
            }
            if let Some(o) = n.owner {
                n.owner = report.entities.get(&o).copied();
            }
            if let RuleSpec::FixPoint { at, .. } = &mut n.rule {
                *at += offset;
            }
            n.id = ConstraintId(self.alloc_id());
            report.constraints.push(n.id);
            self.upsert_constraint(n);
        }
        for g in &clip.groups {
            let mut n = g.clone();
            n.id = GroupId(self.alloc_id());
            n.members = g.members.iter().filter_map(|m| report.entities.get(m).copied()).collect();
            n.children.clear();
            report.groups.push(n.id);
            self.upsert_group(n);
        }
        report
    }
}
