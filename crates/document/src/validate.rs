//! Structural invariants.
//!
//! [`Document::validate`] checks everything that can be checked without plugin
//! definitions: IDs, references, groups, layers, built-in geometry and anchors,
//! numeric sanity and size limits. The engine adds registry-aware checks (plugin
//! anchors and property schemas) on top.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    AnchorRef, DocError, Document, EntityId, GroupId, ParamRef, ParamSlot, PropValue, RuleSpec,
    builtin::{builtin_shape_kind, get_geometry_param, is_builtin, types},
};

/// Size limits applied by validation (and by the `.dotl` reader).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum entities.
    pub max_entities: usize,
    /// Maximum constraints.
    pub max_constraints: usize,
    /// Maximum layers.
    pub max_layers: usize,
    /// Maximum groups.
    pub max_groups: usize,
    /// Maximum properties per entity.
    pub max_props: usize,
    /// Maximum length of any string field in bytes.
    pub max_string: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entities: 1_000_000,
            max_constraints: 1_000_000,
            max_layers: 10_000,
            max_groups: 100_000,
            max_props: 1_000,
            max_string: 100_000,
        }
    }
}

/// Built-in anchors of an entity, if it is a built-in geometry entity.
fn builtin_anchor_exists(doc: &Document, a: &AnchorRef) -> Option<bool> {
    let e = doc.entity(a.entity)?;
    if !is_builtin(&e.type_id) || e.type_id.as_str() == types::DIMENSION {
        return None;
    }
    Some(e.geometry.as_ref().is_some_and(|g| g.anchor(&a.anchor).is_some()))
}

fn check_anchor(doc: &Document, a: &AnchorRef, out: &mut Vec<DocError>) {
    match doc.entity(a.entity) {
        None => out.push(DocError::BrokenReference { from: "constraint".into(), target: a.entity.to_string() }),
        Some(_) => {
            if builtin_anchor_exists(doc, a) == Some(false) {
                out.push(DocError::MissingAnchor { entity: a.entity, anchor: a.anchor.clone() });
            }
        }
    }
}

fn check_param(doc: &Document, p: &ParamRef, out: &mut Vec<DocError>) {
    let Some(e) = doc.entity(p.entity) else {
        out.push(DocError::BrokenReference { from: "constraint".into(), target: p.entity.to_string() });
        return;
    };
    match &p.slot {
        ParamSlot::Geom(name) => {
            let ok = e.geometry.as_ref().is_some_and(|g| get_geometry_param(g, name).is_some());
            if !ok {
                out.push(DocError::MissingParam { entity: p.entity, param: name.clone() });
            }
        }
        ParamSlot::Prop(name) => {
            if !matches!(e.props.get(name), Some(PropValue::Number(_))) {
                out.push(DocError::MissingParam { entity: p.entity, param: name.clone() });
            }
        }
    }
}

fn check_circle(doc: &Document, id: EntityId, out: &mut Vec<DocError>) {
    match doc.entity(id) {
        None => out.push(DocError::BrokenReference { from: "constraint".into(), target: id.to_string() }),
        Some(e) if is_builtin(&e.type_id) => {
            let ok = matches!(e.type_id.as_str(), types::CIRCLE | types::ARC);
            if !ok {
                out.push(DocError::InvalidValue(format!("{id} is not a circle or arc")));
            }
        }
        // Plugin entities are checked by the engine against their definition.
        Some(_) => {}
    }
}

impl Document {
    /// First invariant violation, if any.
    pub fn validate(&self) -> Result<(), DocError> {
        match self.validate_all(&Limits::default()).into_iter().next() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// All invariant violations (bounded to the first 1000).
    #[must_use]
    pub fn validate_all(&self, limits: &Limits) -> Vec<DocError> {
        let mut out = Vec::new();
        self.check_limits(limits, &mut out);
        self.check_ids(&mut out);
        self.check_entities(limits, &mut out);
        self.check_groups(&mut out);
        self.check_constraints(&mut out);
        out.truncate(1000);
        out
    }

    fn check_limits(&self, l: &Limits, out: &mut Vec<DocError>) {
        if self.entity_count() > l.max_entities {
            out.push(DocError::LimitExceeded(format!("{} entities > {}", self.entity_count(), l.max_entities)));
        }
        if self.constraint_count() > l.max_constraints {
            out.push(DocError::LimitExceeded(format!(
                "{} constraints > {}",
                self.constraint_count(),
                l.max_constraints
            )));
        }
        if self.layers().len() > l.max_layers {
            out.push(DocError::LimitExceeded(format!("{} layers > {}", self.layers().len(), l.max_layers)));
        }
        if self.groups().count() > l.max_groups {
            out.push(DocError::LimitExceeded(format!("too many groups (> {})", l.max_groups)));
        }
        if self.meta.title.len() > l.max_string {
            out.push(DocError::LimitExceeded("title too long".into()));
        }
    }

    fn check_ids(&self, out: &mut Vec<DocError>) {
        // One allocator for all kinds: IDs must be unique across kinds and below next_id.
        let mut seen: BTreeMap<u64, &'static str> = BTreeMap::new();
        let mut add = |raw: u64, kind: &'static str, out: &mut Vec<DocError>| {
            if raw >= self.next_id() {
                out.push(DocError::InvalidValue(format!("{kind} id {raw} is not below nextId {}", self.next_id())));
            }
            if let Some(prev) = seen.insert(raw, kind) {
                out.push(DocError::DuplicateId(format!("{raw} used by {prev} and {kind}")));
            }
        };
        for l in self.layers() {
            add(l.id.0, "layer", out);
        }
        for e in self.entities() {
            add(e.id.0, "entity", out);
        }
        for g in self.groups() {
            add(g.id.0, "group", out);
        }
        for c in self.constraints() {
            add(c.id.0, "constraint", out);
        }
        let order: BTreeSet<EntityId> = self.order().iter().copied().collect();
        if order.len() != self.order().len() || order.len() != self.entity_count() {
            out.push(DocError::BadOrder("draw order is not a permutation of the entities".into()));
        }
    }

    fn check_entities(&self, l: &Limits, out: &mut Vec<DocError>) {
        for e in self.entities() {
            if self.layer(e.layer).is_none() {
                out.push(DocError::UnknownLayer(e.layer));
            }
            if !e.transform.is_finite() || e.transform.inverse().is_err() {
                out.push(DocError::InvalidValue(format!("{} has a non-finite or singular transform", e.id)));
            }
            if let Some(kind) = builtin_shape_kind(&e.type_id) {
                match &e.geometry {
                    None => out.push(DocError::InvalidGeometry { entity: e.id, reason: "missing geometry".into() }),
                    Some(g) if g.kind() != kind => out.push(DocError::InvalidGeometry {
                        entity: e.id,
                        reason: format!(
                            "type {} requires {} geometry, found {}",
                            e.type_id,
                            kind.name(),
                            g.kind().name()
                        ),
                    }),
                    Some(g) => {
                        if let Err(err) = g.validate() {
                            out.push(DocError::InvalidGeometry { entity: e.id, reason: err.to_string() });
                        }
                    }
                }
            } else if let Some(g) = &e.geometry
                && let Err(err) = g.validate()
            {
                out.push(DocError::InvalidGeometry { entity: e.id, reason: err.to_string() });
            }
            if e.props.len() > l.max_props {
                out.push(DocError::LimitExceeded(format!("{} has more than {} properties", e.id, l.max_props)));
            }
            for (k, v) in &e.props {
                if k.len() > 256 {
                    out.push(DocError::LimitExceeded(format!("{} property name too long", e.id)));
                }
                if !v.is_finite() {
                    out.push(DocError::InvalidValue(format!("{}.{k} is not finite", e.id)));
                }
                if let PropValue::Text(t) = v
                    && t.len() > l.max_string
                {
                    out.push(DocError::LimitExceeded(format!("{}.{k} text too long", e.id)));
                }
                match v {
                    PropValue::Ref(r) if self.entity(r.entity).is_none() => {
                        out.push(DocError::BrokenReference {
                            from: format!("{}.{k}", e.id),
                            target: r.entity.to_string(),
                        });
                    }
                    PropValue::Anchor(a) => {
                        if self.entity(a.entity).is_none() {
                            out.push(DocError::BrokenReference {
                                from: format!("{}.{k}", e.id),
                                target: a.entity.to_string(),
                            });
                        } else if builtin_anchor_exists(self, a) == Some(false) {
                            out.push(DocError::MissingAnchor { entity: a.entity, anchor: a.anchor.clone() });
                        }
                    }
                    _ => {}
                }
            }
            if e.name.as_ref().is_some_and(|n| n.len() > l.max_string) {
                out.push(DocError::LimitExceeded(format!("{} name too long", e.id)));
            }
            if let Err(err) = e.style.validate() {
                out.push(err);
            }
            for f in e.fallback.iter().flatten() {
                if let Err(err) = f.validate() {
                    out.push(DocError::InvalidGeometry { entity: e.id, reason: format!("fallback: {err}") });
                }
            }
        }
    }

    fn check_groups(&self, out: &mut Vec<DocError>) {
        let mut entity_parent: BTreeMap<EntityId, GroupId> = BTreeMap::new();
        let mut group_parent: BTreeMap<GroupId, GroupId> = BTreeMap::new();
        for g in self.groups() {
            for m in &g.members {
                if self.entity(*m).is_none() {
                    out.push(DocError::BrokenReference { from: g.id.to_string(), target: m.to_string() });
                }
                if entity_parent.insert(*m, g.id).is_some() {
                    out.push(DocError::MultipleParents(m.to_string()));
                }
            }
            for c in &g.children {
                if self.group(*c).is_none() {
                    out.push(DocError::UnknownGroup(*c));
                }
                if group_parent.insert(*c, g.id).is_some() {
                    out.push(DocError::MultipleParents(c.to_string()));
                }
            }
        }
        // Cycle detection: walk parents; each group has at most one parent.
        for g in self.groups() {
            let mut cur = g.id;
            let mut steps = 0usize;
            while let Some(p) = group_parent.get(&cur) {
                if *p == g.id || steps > group_parent.len() {
                    out.push(DocError::GroupCycle(g.id));
                    break;
                }
                cur = *p;
                steps += 1;
            }
        }
    }

    fn check_constraints(&self, out: &mut Vec<DocError>) {
        for c in self.constraints() {
            if !c.rule.values_valid() {
                out.push(DocError::InvalidValue(format!("{} ({}) has invalid values", c.id, c.rule.kind_name())));
            }
            if let Some(owner) = c.owner
                && self.entity(owner).is_none()
            {
                out.push(DocError::BrokenReference { from: c.id.to_string(), target: owner.to_string() });
            }
            for a in c.rule.anchors() {
                check_anchor(self, a, out);
            }
            for p in c.rule.params() {
                check_param(self, p, out);
            }
            match &c.rule {
                RuleSpec::PointOnCircle { circle, .. }
                | RuleSpec::Radius { circle, .. }
                | RuleSpec::TangentLineCircle { circle, .. } => check_circle(self, *circle, out),
                RuleSpec::Concentric { a, b }
                | RuleSpec::EqualRadius { a, b }
                | RuleSpec::TangentCircles { a, b, .. } => {
                    check_circle(self, *a, out);
                    check_circle(self, *b, out);
                }
                RuleSpec::Expression { entity, .. } if self.entity(*entity).is_none() => {
                    out.push(DocError::BrokenReference { from: c.id.to_string(), target: entity.to_string() });
                }
                _ => {}
            }
        }
    }
}
