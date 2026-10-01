//! Undo/redo history of committed changes.
//!
//! Each entry stores the before/after state of every touched object. Undo and
//! redo apply those states directly — the solver is never re-run to "re-derive"
//! history. A new commit after an undo clears the redo branch. Memory is bounded
//! by entry count and an estimated byte budget; the oldest entries are dropped
//! first, and a single change larger than the budget clears the history (the
//! commit result reports `undoAvailable: false`).

use dotloom_document::{Constraint, ConstraintId, Document, Entity, EntityId, Group, GroupId, Layer, Settings};

/// An entity with its draw-order index.
pub type PlacedEntity = (Entity, usize);

/// Before/after state of one entity.
pub type EntityChange = (EntityId, Option<PlacedEntity>, Option<PlacedEntity>);

/// Before/after state of one transaction.
#[derive(Debug, Clone, Default)]
pub struct Change {
    /// `(id, before (entity, order index), after)`.
    pub entities: Vec<EntityChange>,
    /// Constraints.
    pub constraints: Vec<(ConstraintId, Option<Constraint>, Option<Constraint>)>,
    /// Groups.
    pub groups: Vec<(GroupId, Option<Group>, Option<Group>)>,
    /// Layer list before/after.
    pub layers: Option<(Vec<Layer>, Vec<Layer>)>,
    /// Settings before/after.
    pub settings: Option<(Settings, Settings)>,
}

fn entity_bytes(e: &Entity) -> usize {
    let g = e.geometry.as_ref().map_or(0, |g| {
        g.flatten(dotloom_geometry::FlattenTolerance(f64::INFINITY)).iter().map(|f| f.points.len() * 16).sum::<usize>()
            + 64
    });
    256 + g + e.props.len() * 48 + e.data.len() * 128
}

impl Change {
    /// Rough memory estimate.
    #[must_use]
    pub fn estimated_bytes(&self) -> usize {
        let e: usize = self
            .entities
            .iter()
            .map(|(_, b, a)| {
                b.as_ref().map_or(0, |x| entity_bytes(&x.0)) + a.as_ref().map_or(0, |x| entity_bytes(&x.0))
            })
            .sum();
        e + self.constraints.len() * 256
            + self.groups.len() * 128
            + self.layers.as_ref().map_or(0, |(a, b)| (a.len() + b.len()) * 96)
    }

    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
            && self.constraints.is_empty()
            && self.groups.is_empty()
            && self.layers.is_none()
            && self.settings.is_none()
    }

    /// Apply the `after` (redo) or `before` (undo) side to a document.
    pub fn apply(&self, doc: &mut Document, forward: bool) {
        let pick =
            |b: &Option<(Entity, usize)>, a: &Option<(Entity, usize)>| if forward { a.clone() } else { b.clone() };
        for (id, _, _) in &self.entities {
            doc.remove_entity(*id);
        }
        let mut inserts: Vec<(Entity, usize)> = self.entities.iter().filter_map(|(_, b, a)| pick(b, a)).collect();
        inserts.sort_by_key(|(_, i)| *i);
        for (e, i) in inserts {
            doc.insert_entity_at(e, i);
        }
        for (id, b, a) in &self.constraints {
            match if forward { a } else { b } {
                Some(c) => doc.upsert_constraint(c.clone()),
                None => {
                    doc.remove_constraint(*id);
                }
            }
        }
        for (id, b, a) in &self.groups {
            match if forward { a } else { b } {
                Some(g) => doc.upsert_group(g.clone()),
                None => {
                    doc.remove_group(*id);
                }
            }
        }
        if let Some((b, a)) = &self.layers {
            // Layer removal requires an empty layer, so restoring the whole list is safe.
            doc.set_layers(if forward { a.clone() } else { b.clone() });
        }
        if let Some((b, a)) = &self.settings {
            doc.settings = if forward { a.clone() } else { b.clone() };
        }
    }

    /// Entities touched by the change.
    pub fn entity_ids(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.entities.iter().map(|(id, _, _)| *id)
    }
}

/// One history entry.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Label.
    pub label: String,
    /// Change.
    pub change: Change,
    /// Estimated size.
    pub bytes: usize,
}

/// Bounded undo/redo stacks.
#[derive(Debug, Clone)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    bytes: usize,
    /// Maximum entries.
    pub max_entries: usize,
    /// Byte budget.
    pub max_bytes: usize,
}

impl Default for History {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), bytes: 0, max_entries: 500, max_bytes: 64 << 20 }
    }
}

impl History {
    /// History with limits.
    #[must_use]
    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self { max_entries, max_bytes, ..Self::default() }
    }

    /// Record a committed change; returns whether it can be undone.
    pub fn push(&mut self, label: String, change: Change) -> bool {
        self.redo.clear();
        self.recount();
        let bytes = change.estimated_bytes();
        if bytes > self.max_bytes {
            self.undo.clear();
            self.bytes = 0;
            return false;
        }
        self.undo.push(Entry { label, change, bytes });
        self.bytes += bytes;
        while self.undo.len() > self.max_entries || self.bytes > self.max_bytes {
            if self.undo.is_empty() {
                break;
            }
            let old = self.undo.remove(0);
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        true
    }

    fn recount(&mut self) {
        self.bytes = self.undo.iter().map(|e| e.bytes).sum();
    }

    /// Pop the entry to undo.
    pub fn take_undo(&mut self) -> Option<Entry> {
        let e = self.undo.pop()?;
        self.bytes = self.bytes.saturating_sub(e.bytes);
        Some(e)
    }

    /// Pop the entry to redo.
    pub fn take_redo(&mut self) -> Option<Entry> {
        self.redo.pop()
    }

    /// Put an undone entry on the redo stack.
    pub fn push_redo(&mut self, e: Entry) {
        self.redo.push(e);
    }

    /// Put a redone entry back on the undo stack (keeps the redo stack).
    pub fn push_undo_keep_redo(&mut self, e: Entry) {
        self.bytes += e.bytes;
        self.undo.push(e);
    }

    /// Whether undo is possible.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether redo is possible.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Label of the next undo.
    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    /// Label of the next redo.
    #[must_use]
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Estimated bytes held.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Clear everything.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }
}
