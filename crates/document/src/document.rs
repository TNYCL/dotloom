//! The document container.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

use crate::{
    Constraint, ConstraintId, DocError, Entity, EntityId, Group, GroupId, Layer, LayerId, Meta, SCHEMA_VERSION,
    Settings,
};

/// A Dotloom document: the single editable source of truth (owned by the engine).
///
/// Entities are kept in a map keyed by stable ID plus a separate draw order, so
/// reordering never changes identity. IDs for every object kind come from one
/// monotonically increasing allocator (`next_id`) and are never reused.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DocumentRepr")]
pub struct Document {
    /// Metadata.
    pub meta: Meta,
    /// Settings.
    pub settings: Settings,
    layers: Vec<Layer>,
    entities: BTreeMap<EntityId, Entity>,
    order: Vec<EntityId>,
    groups: BTreeMap<GroupId, Group>,
    constraints: BTreeMap<ConstraintId, Constraint>,
    next_id: u64,
    /// Unknown top-level fields preserved on save.
    pub extra: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentRepr {
    schema: u32,
    #[serde(default)]
    meta: Meta,
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    layers: Vec<Layer>,
    #[serde(default)]
    entities: Vec<Entity>,
    #[serde(default)]
    groups: Vec<Group>,
    #[serde(default)]
    constraints: Vec<Constraint>,
    next_id: u64,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl TryFrom<DocumentRepr> for Document {
    type Error = DocError;

    fn try_from(r: DocumentRepr) -> Result<Self, DocError> {
        if r.schema != SCHEMA_VERSION {
            return Err(DocError::Malformed(format!(
                "schema {} must be migrated before deserialization (current {SCHEMA_VERSION})",
                r.schema
            )));
        }
        let mut entities = BTreeMap::new();
        let mut order = Vec::with_capacity(r.entities.len());
        for e in r.entities {
            order.push(e.id);
            if entities.insert(e.id, e).is_some() {
                return Err(DocError::DuplicateId(order.last().map(ToString::to_string).unwrap_or_default()));
            }
        }
        let mut groups = BTreeMap::new();
        for g in r.groups {
            let id = g.id;
            if groups.insert(id, g).is_some() {
                return Err(DocError::DuplicateId(id.to_string()));
            }
        }
        let mut constraints = BTreeMap::new();
        for c in r.constraints {
            let id = c.id;
            if constraints.insert(id, c).is_some() {
                return Err(DocError::DuplicateId(id.to_string()));
            }
        }
        let mut seen_layers = BTreeSet::new();
        for l in &r.layers {
            if !seen_layers.insert(l.id) {
                return Err(DocError::DuplicateId(l.id.to_string()));
            }
        }
        Ok(Self {
            meta: r.meta,
            settings: r.settings,
            layers: r.layers,
            entities,
            order,
            groups,
            constraints,
            next_id: r.next_id,
            extra: r.extra,
        })
    }
}

struct OrderedEntities<'a>(&'a Document);

impl Serialize for OrderedEntities<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.0.order.iter().filter_map(|id| self.0.entities.get(id)))
    }
}

impl Serialize for Document {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(8 + self.extra.len()))?;
        m.serialize_entry("schema", &SCHEMA_VERSION)?;
        m.serialize_entry("meta", &self.meta)?;
        m.serialize_entry("settings", &self.settings)?;
        m.serialize_entry("layers", &self.layers)?;
        m.serialize_entry("entities", &OrderedEntities(self))?;
        m.serialize_entry("groups", &self.groups.values().collect::<Vec<_>>())?;
        m.serialize_entry("constraints", &self.constraints.values().collect::<Vec<_>>())?;
        m.serialize_entry("nextId", &self.next_id)?;
        // Unknown top-level fields from newer producers are written back unchanged.
        for (k, v) in &self.extra {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

/// What references an entity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependents {
    /// Constraints that reference the entity.
    pub constraints: Vec<ConstraintId>,
    /// Entities whose properties reference the entity.
    pub entities: Vec<EntityId>,
    /// Groups that contain the entity.
    pub groups: Vec<GroupId>,
}

impl Document {
    /// Empty document with one default layer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            meta: Meta::default(),
            settings: Settings::default(),
            layers: vec![Layer::new(LayerId(1), "Layer 1")],
            entities: BTreeMap::new(),
            order: Vec::new(),
            groups: BTreeMap::new(),
            constraints: BTreeMap::new(),
            next_id: 2,
            extra: BTreeMap::new(),
        }
    }

    /// Allocate a fresh ID (shared by all object kinds).
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Next ID that will be allocated.
    #[must_use]
    pub const fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Raise the allocator so that `id` is never handed out again.
    pub fn reserve_id(&mut self, id: u64) {
        if id >= self.next_id {
            self.next_id = id.saturating_add(1);
        }
    }

    // --- entities -------------------------------------------------------------

    /// Number of entities.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Entity by ID.
    #[must_use]
    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    /// Mutable entity by ID (engine use; callers must re-validate).
    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.entities.get_mut(&id)
    }

    /// Entities in draw order.
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.order.iter().filter_map(|id| self.entities.get(id))
    }

    /// Draw order.
    #[must_use]
    pub fn order(&self) -> &[EntityId] {
        &self.order
    }

    /// Insert an entity at the top of the draw order (or replace one with the same
    /// ID in place).
    pub fn insert_entity(&mut self, e: Entity) {
        self.reserve_id(e.id.0);
        if !self.entities.contains_key(&e.id) {
            self.order.push(e.id);
        }
        self.entities.insert(e.id, e);
    }

    /// Insert an entity at a draw-order index.
    pub fn insert_entity_at(&mut self, e: Entity, index: usize) {
        self.reserve_id(e.id.0);
        let id = e.id;
        if self.entities.insert(id, e).is_none() {
            let i = index.min(self.order.len());
            self.order.insert(i, id);
        }
    }

    /// Remove an entity (does not touch constraints or references; see
    /// [`Document::dependents`]). Returns the entity and its draw-order index.
    pub fn remove_entity(&mut self, id: EntityId) -> Option<(Entity, usize)> {
        let e = self.entities.remove(&id)?;
        let idx = self.order.iter().position(|x| *x == id).unwrap_or(self.order.len());
        if idx < self.order.len() {
            self.order.remove(idx);
        }
        for g in self.groups.values_mut() {
            g.members.retain(|m| *m != id);
        }
        Some((e, idx))
    }

    /// Move an entity to a draw-order index. IDs never change.
    pub fn reorder(&mut self, id: EntityId, index: usize) -> Result<(), DocError> {
        let pos = self.order.iter().position(|x| *x == id).ok_or(DocError::UnknownEntity(id))?;
        self.order.remove(pos);
        let i = index.min(self.order.len());
        self.order.insert(i, id);
        Ok(())
    }

    /// Draw-order index of an entity.
    #[must_use]
    pub fn order_index(&self, id: EntityId) -> Option<usize> {
        self.order.iter().position(|x| *x == id)
    }

    // --- layers ---------------------------------------------------------------

    /// Layers in display order.
    #[must_use]
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Layer by ID.
    #[must_use]
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// Mutable layer by ID.
    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Insert or replace a layer (appended when new).
    pub fn upsert_layer(&mut self, l: Layer) {
        self.reserve_id(l.id.0);
        if let Some(slot) = self.layers.iter_mut().find(|x| x.id == l.id) {
            *slot = l;
        } else {
            self.layers.push(l);
        }
    }

    /// Insert a layer at an index.
    pub fn insert_layer_at(&mut self, l: Layer, index: usize) {
        self.reserve_id(l.id.0);
        let i = index.min(self.layers.len());
        self.layers.insert(i, l);
    }

    /// Remove a layer. Fails while entities still use it.
    pub fn remove_layer(&mut self, id: LayerId) -> Result<(Layer, usize), DocError> {
        if self.entities.values().any(|e| e.layer == id) {
            return Err(DocError::InvalidValue(format!("layer {id} is not empty")));
        }
        let pos = self.layers.iter().position(|l| l.id == id).ok_or(DocError::UnknownLayer(id))?;
        Ok((self.layers.remove(pos), pos))
    }

    // --- groups ---------------------------------------------------------------

    /// Groups.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.values()
    }

    /// Group by ID.
    #[must_use]
    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.get(&id)
    }

    /// Insert or replace a group.
    pub fn upsert_group(&mut self, g: Group) {
        self.reserve_id(g.id.0);
        self.groups.insert(g.id, g);
    }

    /// Remove a group (members stay in the document).
    pub fn remove_group(&mut self, id: GroupId) -> Option<Group> {
        let g = self.groups.remove(&id)?;
        for other in self.groups.values_mut() {
            other.children.retain(|c| *c != id);
        }
        Some(g)
    }

    /// Group that directly contains an entity.
    #[must_use]
    pub fn group_of(&self, id: EntityId) -> Option<GroupId> {
        self.groups.values().find(|g| g.members.contains(&id)).map(|g| g.id)
    }

    // --- constraints ----------------------------------------------------------

    /// Constraints.
    pub fn constraints(&self) -> impl Iterator<Item = &Constraint> {
        self.constraints.values()
    }

    /// Number of constraints.
    #[must_use]
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    /// Constraint by ID.
    #[must_use]
    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints.get(&id)
    }

    /// Insert or replace a constraint.
    pub fn upsert_constraint(&mut self, c: Constraint) {
        self.reserve_id(c.id.0);
        self.constraints.insert(c.id, c);
    }

    /// Remove a constraint.
    pub fn remove_constraint(&mut self, id: ConstraintId) -> Option<Constraint> {
        self.constraints.remove(&id)
    }

    // --- queries --------------------------------------------------------------

    /// Everything that references `id` (O(n); the engine keeps an index).
    #[must_use]
    pub fn dependents(&self, id: EntityId) -> Dependents {
        Dependents {
            constraints: self
                .constraints
                .values()
                .filter(|c| c.rule.entities().contains(&id) || c.owner == Some(id))
                .map(|c| c.id)
                .collect(),
            entities: self
                .entities
                .values()
                .filter(|e| e.id != id && e.referenced_entities().any(|r| r == id))
                .map(|e| e.id)
                .collect(),
            groups: self.groups.values().filter(|g| g.members.contains(&id)).map(|g| g.id).collect(),
        }
    }

    /// Whether any ID of any kind equals `raw`.
    #[must_use]
    pub fn id_in_use(&self, raw: u64) -> bool {
        self.entities.contains_key(&EntityId(raw))
            || self.constraints.contains_key(&ConstraintId(raw))
            || self.groups.contains_key(&GroupId(raw))
            || self.layers.iter().any(|l| l.id.0 == raw)
    }
}
