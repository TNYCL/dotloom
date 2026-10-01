//! Dependency index: constraints per entity and reverse property references.

use std::collections::{BTreeMap, BTreeSet};

use dotloom_document::{Constraint, ConstraintId, Document, Entity, EntityId};

/// Which constraints and entities depend on an entity.
#[derive(Debug, Clone, Default)]
pub struct DepIndex {
    constraints: BTreeMap<EntityId, BTreeSet<ConstraintId>>,
    referrers: BTreeMap<EntityId, BTreeSet<EntityId>>,
}

fn constraint_entities(c: &Constraint) -> Vec<EntityId> {
    let mut v = c.rule.entities();
    if let Some(o) = c.owner {
        v.push(o);
    }
    v
}

impl DepIndex {
    /// Build from a document.
    #[must_use]
    pub fn build(doc: &Document) -> Self {
        let mut s = Self::default();
        for e in doc.entities() {
            s.add_entity(e);
        }
        for c in doc.constraints() {
            s.add_constraint(c);
        }
        s
    }

    /// Index an entity's outgoing references.
    pub fn add_entity(&mut self, e: &Entity) {
        for t in e.referenced_entities() {
            self.referrers.entry(t).or_default().insert(e.id);
        }
    }

    /// Remove an entity's outgoing references.
    pub fn remove_entity(&mut self, e: &Entity) {
        for t in e.referenced_entities() {
            if let Some(s) = self.referrers.get_mut(&t) {
                s.remove(&e.id);
                if s.is_empty() {
                    self.referrers.remove(&t);
                }
            }
        }
    }

    /// Index a constraint.
    pub fn add_constraint(&mut self, c: &Constraint) {
        for e in constraint_entities(c) {
            self.constraints.entry(e).or_default().insert(c.id);
        }
    }

    /// Remove a constraint.
    pub fn remove_constraint(&mut self, c: &Constraint) {
        for e in constraint_entities(c) {
            if let Some(s) = self.constraints.get_mut(&e) {
                s.remove(&c.id);
                if s.is_empty() {
                    self.constraints.remove(&e);
                }
            }
        }
    }

    /// Constraints referencing an entity.
    pub fn constraints_of(&self, id: EntityId) -> impl Iterator<Item = ConstraintId> + '_ {
        self.constraints.get(&id).into_iter().flatten().copied()
    }

    /// Entities whose properties reference `id`.
    pub fn referrers_of(&self, id: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        self.referrers.get(&id).into_iter().flatten().copied()
    }
}
