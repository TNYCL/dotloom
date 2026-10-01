//! Read access to a document or a transaction's working state.

use std::collections::BTreeMap;

use dotloom_document::{
    Constraint, ConstraintId, Document, Entity, EntityId, Group, GroupId, Layer, LayerId, Settings,
};

/// Read-only view of document content.
pub trait DocView {
    /// Entity by ID.
    fn entity(&self, id: EntityId) -> Option<&Entity>;
    /// Constraint by ID.
    fn constraint(&self, id: ConstraintId) -> Option<&Constraint>;
    /// Layer by ID.
    fn layer(&self, id: LayerId) -> Option<&Layer>;
    /// Group by ID.
    fn group(&self, id: GroupId) -> Option<&Group>;
    /// Settings.
    fn settings(&self) -> &Settings;
}

impl DocView for Document {
    fn entity(&self, id: EntityId) -> Option<&Entity> {
        Document::entity(self, id)
    }
    fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        Document::constraint(self, id)
    }
    fn layer(&self, id: LayerId) -> Option<&Layer> {
        Document::layer(self, id)
    }
    fn group(&self, id: GroupId) -> Option<&Group> {
        Document::group(self, id)
    }
    fn settings(&self) -> &Settings {
        &self.settings
    }
}

/// Copy-on-write overlay over a document used while a transaction runs.
#[derive(Debug, Clone)]
pub struct Overlay<'a> {
    /// Base document.
    pub base: &'a Document,
    /// Changed entities (`None` = deleted).
    pub entities: BTreeMap<EntityId, Option<Entity>>,
    /// Changed constraints.
    pub constraints: BTreeMap<ConstraintId, Option<Constraint>>,
    /// Changed groups.
    pub groups: BTreeMap<GroupId, Option<Group>>,
    /// Replaced layer list.
    pub layers: Option<Vec<Layer>>,
    /// Replaced settings.
    pub settings: Option<Settings>,
    /// Draw-order operations.
    pub order_ops: Vec<OrderOp>,
    /// ID allocator.
    pub next_id: u64,
}

/// Owned overlay content (without the base borrow), kept by pending commits.
#[derive(Debug, Clone, Default)]
pub struct OverlayData {
    entities: BTreeMap<EntityId, Option<Entity>>,
    constraints: BTreeMap<ConstraintId, Option<Constraint>>,
    groups: BTreeMap<GroupId, Option<Group>>,
    layers: Option<Vec<Layer>>,
    settings: Option<Settings>,
    order_ops: Vec<OrderOp>,
    next_id: u64,
}

impl<'a> Overlay<'a> {
    /// Detach the changes from the base document.
    #[must_use]
    pub fn into_data(self) -> OverlayData {
        OverlayData {
            entities: self.entities,
            constraints: self.constraints,
            groups: self.groups,
            layers: self.layers,
            settings: self.settings,
            order_ops: self.order_ops,
            next_id: self.next_id,
        }
    }

    /// Re-attach detached changes to the same base document.
    #[must_use]
    pub fn from_data(base: &'a Document, d: OverlayData) -> Self {
        Self {
            base,
            entities: d.entities,
            constraints: d.constraints,
            groups: d.groups,
            layers: d.layers,
            settings: d.settings,
            order_ops: d.order_ops,
            next_id: d.next_id,
        }
    }
}

/// Draw-order change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderOp {
    /// Move to index.
    Move(EntityId, usize),
}

impl<'a> Overlay<'a> {
    /// Fresh overlay.
    #[must_use]
    pub fn new(base: &'a Document) -> Self {
        Self {
            base,
            entities: BTreeMap::new(),
            constraints: BTreeMap::new(),
            groups: BTreeMap::new(),
            layers: None,
            settings: None,
            order_ops: Vec::new(),
            next_id: base.next_id(),
        }
    }

    /// Allocate an ID.
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Entity for modification (copied into the overlay on first write).
    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        if !self.entities.contains_key(&id) {
            let e = self.base.entity(id)?.clone();
            self.entities.insert(id, Some(e));
        }
        self.entities.get_mut(&id).and_then(Option::as_mut)
    }

    /// Insert or replace an entity.
    pub fn put_entity(&mut self, e: Entity) {
        self.entities.insert(e.id, Some(e));
    }

    /// Delete an entity.
    pub fn delete_entity(&mut self, id: EntityId) {
        self.entities.insert(id, None);
    }

    /// Insert or replace a constraint.
    pub fn put_constraint(&mut self, c: Constraint) {
        self.constraints.insert(c.id, Some(c));
    }

    /// Delete a constraint.
    pub fn delete_constraint(&mut self, id: ConstraintId) {
        self.constraints.insert(id, None);
    }

    /// Layers (copied on first write).
    pub fn layers_mut(&mut self) -> &mut Vec<Layer> {
        self.layers.get_or_insert_with(|| self.base.layers().to_vec())
    }

    /// Current layers.
    #[must_use]
    pub fn layers(&self) -> &[Layer] {
        self.layers.as_deref().unwrap_or(self.base.layers())
    }

    /// Settings for modification.
    pub fn settings_mut(&mut self) -> &mut Settings {
        self.settings.get_or_insert_with(|| self.base.settings.clone())
    }

    /// All constraint IDs visible in the overlay (base ∪ new − deleted).
    #[must_use]
    pub fn constraint_ids(&self) -> Vec<ConstraintId> {
        let mut ids: Vec<ConstraintId> = self
            .base
            .constraints()
            .map(|c| c.id)
            .filter(|id| !matches!(self.constraints.get(id), Some(None)))
            .collect();
        ids.extend(
            self.constraints
                .iter()
                .filter(|(id, c)| c.is_some() && self.base.constraint(**id).is_none())
                .map(|(id, _)| *id),
        );
        ids.sort_unstable();
        ids
    }

    /// All entity IDs visible in the overlay.
    #[must_use]
    pub fn entity_ids(&self) -> Vec<EntityId> {
        let mut ids: Vec<EntityId> =
            self.base.order().iter().copied().filter(|id| !matches!(self.entities.get(id), Some(None))).collect();
        ids.extend(
            self.entities.iter().filter(|(id, e)| e.is_some() && self.base.entity(**id).is_none()).map(|(id, _)| *id),
        );
        ids
    }

    /// All groups visible in the overlay.
    #[must_use]
    pub fn group_ids(&self) -> Vec<GroupId> {
        let mut ids: Vec<GroupId> =
            self.base.groups().map(|g| g.id).filter(|id| !matches!(self.groups.get(id), Some(None))).collect();
        ids.extend(
            self.groups.iter().filter(|(id, g)| g.is_some() && self.base.group(**id).is_none()).map(|(id, _)| *id),
        );
        ids.sort_unstable();
        ids
    }
}

impl DocView for Overlay<'_> {
    fn entity(&self, id: EntityId) -> Option<&Entity> {
        match self.entities.get(&id) {
            Some(e) => e.as_ref(),
            None => self.base.entity(id),
        }
    }
    fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        match self.constraints.get(&id) {
            Some(c) => c.as_ref(),
            None => self.base.constraint(id),
        }
    }
    fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers().iter().find(|l| l.id == id)
    }
    fn group(&self, id: GroupId) -> Option<&Group> {
        match self.groups.get(&id) {
            Some(g) => g.as_ref(),
            None => self.base.group(id),
        }
    }
    fn settings(&self) -> &Settings {
        self.settings.as_ref().unwrap_or(&self.base.settings)
    }
}
