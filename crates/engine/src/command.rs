//! Commands and their application to a transaction overlay.

use std::collections::{BTreeMap, BTreeSet};

use dotloom_document::{
    Clipboard, Color, Constraint, ConstraintId, Entity, EntityId, Group, GroupId, Layer, LayerId, PropValue, RuleSpec,
    StrengthSpec, Style, TypeId,
    builtin::{builtin_shape_kind, is_builtin, set_geometry_param, types},
};
use dotloom_geometry::{
    Affine, GeometryError, ModelTolerance, Point, Shape, TransformPolicy, Vector,
    edit::CurveEnd,
    units::{LengthUnit, TimeAxis},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    DocView, Overlay,
    eval::{Ctx, evaluate, plugin_for},
    registry::{OnDelete, PropDef, Registry},
};

/// How numeric edits are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditMode {
    /// The value is a hard requirement of this transaction (rejected if infeasible).
    #[default]
    Exact,
    /// The value is a strong preference (the solver may deviate to satisfy rules).
    Prefer,
}

/// A numeric parameter assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamValue {
    /// Entity.
    pub entity: EntityId,
    /// Parameter (`width`, `a.x`, `start.x`, `r`, ...).
    pub param: String,
    /// Value in canonical units.
    pub value: f64,
}

/// Data for a new entity.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEntity {
    /// Type.
    #[serde(rename = "type")]
    pub type_id: Option<TypeId>,
    /// Layer (default: first unlocked visible layer).
    #[serde(default)]
    pub layer: Option<LayerId>,
    /// Geometry (built-in types).
    #[serde(default)]
    pub geometry: Option<Shape>,
    /// Properties (missing ones get defaults).
    #[serde(default)]
    pub props: BTreeMap<String, PropValue>,
    /// Transform.
    #[serde(default)]
    pub transform: Option<Affine>,
    /// Name.
    #[serde(default)]
    pub name: Option<String>,
    /// Style.
    #[serde(default)]
    pub style: Option<Style>,
    /// Opaque payloads.
    #[serde(default)]
    pub data: BTreeMap<String, Value>,
}

/// Partial update of an entity. `props` entries set to `null` remove the property.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityPatch {
    /// New geometry.
    #[serde(default)]
    pub geometry: Option<Shape>,
    /// Property changes.
    #[serde(default)]
    pub props: BTreeMap<String, Option<PropValue>>,
    /// New transform.
    #[serde(default)]
    pub transform: Option<Affine>,
    /// New name (`""` clears).
    #[serde(default)]
    pub name: Option<String>,
    /// New style.
    #[serde(default)]
    pub style: Option<Style>,
    /// New layer.
    #[serde(default)]
    pub layer: Option<LayerId>,
    /// Lock state.
    #[serde(default)]
    pub locked: Option<bool>,
    /// Visibility.
    #[serde(default)]
    pub hidden: Option<bool>,
    /// Payload changes (`null` removes).
    #[serde(default)]
    pub data: BTreeMap<String, Option<Value>>,
}

impl EntityPatch {
    fn only_meta(&self) -> bool {
        self.geometry.is_none() && self.props.is_empty() && self.transform.is_none() && self.data.is_empty()
    }
}

/// Constraint content (the engine assigns or validates the ID).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintSpec {
    /// Rule.
    pub rule: RuleSpec,
    /// Strength.
    #[serde(default)]
    pub strength: StrengthSpec,
    /// Enabled.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Label.
    #[serde(default)]
    pub label: Option<String>,
    /// Source.
    #[serde(default)]
    pub source: Option<String>,
}

fn yes() -> bool {
    true
}

/// Partial constraint update.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintPatch {
    /// New rule.
    #[serde(default)]
    pub rule: Option<RuleSpec>,
    /// New strength.
    #[serde(default)]
    pub strength: Option<StrengthSpec>,
    /// Enable/disable.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// New label.
    #[serde(default)]
    pub label: Option<String>,
}

/// Layer update.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerPatch {
    /// Name.
    #[serde(default)]
    pub name: Option<String>,
    /// Visible.
    #[serde(default)]
    pub visible: Option<bool>,
    /// Locked.
    #[serde(default)]
    pub locked: Option<bool>,
    /// Color (`null` keeps; use `"#00000000"` for none).
    #[serde(default)]
    pub color: Option<Color>,
}

/// Settings update.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    /// Display unit.
    #[serde(default)]
    pub display_unit: Option<LengthUnit>,
    /// Grid spacing (mm).
    #[serde(default)]
    pub grid_spacing: Option<f64>,
    /// Time axis (`null` keeps).
    #[serde(default)]
    pub time_axis: Option<TimeAxis>,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
}

/// What to do with constraints and referencing entities when deleting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeletePolicy {
    /// Delete dependent constraints and apply each reference's `onDelete` policy.
    #[default]
    Cascade,
    /// Fail if anything depends on the deleted entities.
    Reject,
}

/// One command of a transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    /// Create an entity (with a reserved or freshly allocated ID).
    CreateEntity {
        /// Reserved ID (see `Engine::reserve_ids`).
        #[serde(default)]
        id: Option<EntityId>,
        /// Content.
        entity: NewEntity,
    },
    /// Patch an entity.
    UpdateEntity {
        /// Entity.
        id: EntityId,
        /// Changes.
        patch: EntityPatch,
    },
    /// Set numeric parameters through the solver.
    SetParams {
        /// Values.
        values: Vec<ParamValue>,
        /// Exact (hard) or preferred.
        #[serde(default)]
        mode: EditMode,
    },
    /// Transform entities (move/rotate/scale/mirror).
    Transform {
        /// Entities.
        ids: Vec<EntityId>,
        /// World transform to apply.
        transform: Affine,
        /// Policy for shapes that cannot represent the result.
        #[serde(default)]
        policy: TransformPolicy,
    },
    /// Delete entities.
    Delete {
        /// Entities.
        ids: Vec<EntityId>,
        /// Dependency policy.
        #[serde(default)]
        policy: DeletePolicy,
    },
    /// Move an entity in draw order.
    Reorder {
        /// Entity.
        id: EntityId,
        /// New index (0 = bottom).
        index: usize,
    },
    /// Add a constraint.
    AddConstraint {
        /// Reserved ID.
        #[serde(default)]
        id: Option<ConstraintId>,
        /// Content.
        constraint: ConstraintSpec,
    },
    /// Update a constraint.
    UpdateConstraint {
        /// Constraint.
        id: ConstraintId,
        /// Changes.
        patch: ConstraintPatch,
    },
    /// Remove a constraint.
    RemoveConstraint {
        /// Constraint.
        id: ConstraintId,
    },
    /// Add a layer.
    AddLayer {
        /// Reserved ID.
        #[serde(default)]
        id: Option<LayerId>,
        /// Name.
        name: String,
    },
    /// Update a layer.
    UpdateLayer {
        /// Layer.
        id: LayerId,
        /// Changes.
        patch: LayerPatch,
    },
    /// Remove an empty layer.
    RemoveLayer {
        /// Layer.
        id: LayerId,
    },
    /// Move a layer in the stack.
    MoveLayer {
        /// Layer.
        id: LayerId,
        /// New index (0 = bottom).
        index: usize,
    },
    /// Group entities.
    Group {
        /// Reserved ID.
        #[serde(default)]
        id: Option<GroupId>,
        /// Members.
        members: Vec<EntityId>,
        /// Name.
        #[serde(default)]
        name: Option<String>,
    },
    /// Dissolve a group (members stay).
    Ungroup {
        /// Group.
        id: GroupId,
    },
    /// Paste clipboard content.
    Paste {
        /// Content.
        clipboard: Clipboard,
        /// Offset.
        offset: Vector,
    },
    /// Split a line/arc/polyline at a point (circles need `at2`).
    Split {
        /// Entity.
        id: EntityId,
        /// Split point (world).
        at: Point,
        /// Second point for circles.
        #[serde(default)]
        at2: Option<Point>,
    },
    /// Trim the piece of an entity between cutting edges around `pick`.
    Trim {
        /// Entity.
        id: EntityId,
        /// Cutting entities.
        cutters: Vec<EntityId>,
        /// Pick point (world).
        pick: Point,
    },
    /// Extend an end of a line/arc/polyline to boundaries.
    Extend {
        /// Entity.
        id: EntityId,
        /// Which end.
        end: CurveEnd,
        /// Boundary entities.
        boundaries: Vec<EntityId>,
    },
    /// Update document settings.
    SetSettings {
        /// Changes.
        patch: SettingsPatch,
    },
}

/// Command errors (the transaction is rejected, nothing changes).
#[derive(Debug, Clone, PartialEq, Error, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "camelCase")]
#[non_exhaustive]
pub enum CommandError {
    /// Unknown entity/constraint/layer/group.
    #[error("{what} does not exist")]
    NotFound {
        /// Object.
        what: String,
    },
    /// The entity cannot be edited (plugin missing/disabled/newer, locked).
    #[error("{what} is read-only: {reason}")]
    ReadOnly {
        /// Object.
        what: String,
        /// Reason.
        reason: String,
    },
    /// Invalid input.
    #[error("invalid {what}: {reason}")]
    Invalid {
        /// What.
        what: String,
        /// Why.
        reason: String,
    },
    /// Deletion refused because of dependents.
    #[error("cannot delete {what}: {reason}")]
    HasDependents {
        /// What.
        what: String,
        /// Dependents.
        reason: String,
    },
    /// Geometry operation failed.
    #[error("geometry: {0}")]
    Geometry(String),
}

impl From<GeometryError> for CommandError {
    fn from(e: GeometryError) -> Self {
        Self::Geometry(e.to_string())
    }
}

/// Side results of applying commands.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyNotes {
    /// Entities created (in order).
    pub created: Vec<EntityId>,
    /// Constraints created.
    pub created_constraints: Vec<ConstraintId>,
    /// Entities deleted (including cascades).
    pub deleted: Vec<EntityId>,
    /// Constraints removed implicitly (cascade, split/trim).
    pub removed_constraints: Vec<ConstraintId>,
    /// Explicit numeric edits `(entity, param, value, exact)`.
    #[serde(skip)]
    pub edits: Vec<(EntityId, String, f64, bool)>,
    /// Entities whose content changed.
    #[serde(skip)]
    pub touched: BTreeSet<EntityId>,
    /// Constraints added/changed.
    #[serde(skip)]
    pub touched_constraints: BTreeSet<ConstraintId>,
    /// Messages (dropped references, ...).
    pub notes: Vec<String>,
}

fn not_found(what: impl ToString) -> CommandError {
    CommandError::NotFound { what: what.to_string() }
}

fn invalid(what: impl Into<String>, reason: impl Into<String>) -> CommandError {
    CommandError::Invalid { what: what.into(), reason: reason.into() }
}

/// Applies commands to an overlay.
pub(crate) struct Applier<'o, 'd> {
    pub ov: &'o mut Overlay<'d>,
    pub registry: &'o Registry,
    pub reserved: &'o BTreeSet<u64>,
    pub notes: ApplyNotes,
}

impl Applier<'_, '_> {
    fn ctx(&self) -> Ctx<'_> {
        Ctx { view: &*self.ov, registry: self.registry }
    }

    fn take_id(&mut self, wanted: Option<u64>) -> Result<u64, CommandError> {
        match wanted {
            None => Ok(self.ov.alloc_id()),
            Some(id) => {
                let free = self.reserved.contains(&id) || id >= self.ov.next_id;
                let unused = self.ov.entity(EntityId(id)).is_none()
                    && self.ov.constraint(ConstraintId(id)).is_none()
                    && self.ov.layer(LayerId(id)).is_none()
                    && self.ov.group(GroupId(id)).is_none();
                if !(free && unused) {
                    return Err(invalid("id", format!("{id} is not a reserved, unused id")));
                }
                if id >= self.ov.next_id {
                    self.ov.next_id = id.saturating_add(1);
                }
                Ok(id)
            }
        }
    }

    /// The entity must exist and be editable (plugin available, not locked).
    fn editable(&self, id: EntityId, meta_only: bool) -> Result<&Entity, CommandError> {
        let e = self.ov.entity(id).ok_or_else(|| not_found(id))?;
        if meta_only {
            return Ok(e);
        }
        if !is_builtin(&e.type_id)
            && let Err(r) = plugin_for(self.registry, e)
        {
            return Err(CommandError::ReadOnly { what: id.to_string(), reason: format!("{r:?}") });
        }
        if e.locked {
            return Err(CommandError::ReadOnly { what: id.to_string(), reason: "entity is locked".into() });
        }
        if self.ov.layer(e.layer).is_some_and(|l| l.locked) {
            return Err(CommandError::ReadOnly { what: id.to_string(), reason: "layer is locked".into() });
        }
        Ok(e)
    }

    fn default_layer(&self) -> Result<LayerId, CommandError> {
        self.ov
            .layers()
            .iter()
            .rev()
            .find(|l| l.visible && !l.locked)
            .or_else(|| self.ov.layers().last())
            .map(|l| l.id)
            .ok_or_else(|| invalid("layer", "document has no layers"))
    }

    /// Validate properties against the type schema and fill defaults.
    fn check_props(
        &self,
        type_id: &TypeId,
        props: &mut BTreeMap<String, PropValue>,
        fill: bool,
    ) -> Result<(), CommandError> {
        if is_builtin(type_id) {
            if type_id.as_str() == types::DIMENSION {
                for (k, v) in props.iter() {
                    if !v.is_finite() {
                        return Err(invalid(format!("prop {k}"), "not finite"));
                    }
                }
            }
            return Ok(());
        }
        let Some(entry) = self.registry.enabled(type_id) else {
            return Err(CommandError::ReadOnly {
                what: type_id.to_string(),
                reason: "plugin type is not registered or disabled".into(),
            });
        };
        let def = &entry.def;
        for (k, v) in props.iter() {
            let Some(pd) = def.props.get(k) else {
                return Err(invalid(format!("prop {k}"), format!("not defined by {type_id}")));
            };
            let ok = match (pd, v) {
                (PropDef::Number { .. }, PropValue::Number(n)) => n.is_finite(),
                (PropDef::Point { .. }, PropValue::Point(p)) => p.is_finite(),
                (PropDef::Bool { .. }, PropValue::Bool(_)) => true,
                (PropDef::Text { max_len, .. }, PropValue::Text(t)) => t.chars().count() <= max_len.unwrap_or(10_000),
                (PropDef::Enum { values, .. }, PropValue::Text(t)) => values.contains(t),
                (PropDef::Ref { target, .. }, PropValue::Ref(r)) => match self.ov.entity(r.entity) {
                    None => false,
                    Some(t) => target.as_ref().is_none_or(|tt| *tt == t.type_id),
                },
                _ => false,
            };
            if !ok {
                return Err(invalid(format!("prop {k}"), format!("value {} does not match the schema", v.kind_name())));
            }
        }
        if fill {
            for (k, pd) in &def.props {
                if props.contains_key(k) {
                    continue;
                }
                let v = match pd {
                    PropDef::Number { dim, default, .. } => match default {
                        Some(l) => {
                            Some(PropValue::Number(l.value(dim.dim()).map_err(|e| invalid(format!("prop {k}"), e))?))
                        }
                        None => Some(PropValue::Number(0.0)),
                    },
                    PropDef::Point { default, .. } => {
                        Some(PropValue::Point(default.map_or(Point::ORIGIN, |[x, y]| Point::new(x, y))))
                    }
                    PropDef::Bool { default, .. } => Some(PropValue::Bool(default.unwrap_or(false))),
                    PropDef::Text { default, .. } => Some(PropValue::Text(default.clone().unwrap_or_default())),
                    PropDef::Enum { values, default, .. } => {
                        default.clone().or_else(|| values.first().cloned()).map(PropValue::Text)
                    }
                    PropDef::Ref { required, .. } => {
                        if *required {
                            return Err(invalid(format!("prop {k}"), "required reference is missing"));
                        }
                        None
                    }
                };
                if let Some(v) = v {
                    props.insert(k.clone(), v);
                }
            }
        }
        Ok(())
    }

    fn record_param_changes(&mut self, before: &Entity, after: &Entity, exact: bool) {
        let ctx = self.ctx();
        let names: Vec<String> = if is_builtin(&after.type_id) {
            after
                .geometry
                .as_ref()
                .map(|g| dotloom_document::builtin::geometry_params(g).into_iter().map(|(n, _, _)| n).collect())
                .unwrap_or_default()
        } else {
            plugin_for(self.registry, after)
                .map(|d| d.params.iter().map(|p| p.name.clone()).collect())
                .unwrap_or_default()
        };
        let same_shape = match (&before.geometry, &after.geometry) {
            (Some(a), Some(b)) => {
                dotloom_document::builtin::geometry_params(a).len()
                    == dotloom_document::builtin::geometry_params(b).len()
            }
            _ => true,
        };
        let mut edits = Vec::new();
        for n in names {
            let (b, a) = (crate::eval::param_value(ctx, before, &n), crate::eval::param_value(ctx, after, &n));
            if let Some(a) = a
                && (b != Some(a) || !same_shape)
            {
                edits.push((after.id, n, a, exact));
            }
        }
        // Transform changes move every anchor: pin all parameters.
        if before.transform != after.transform {
            for (id, n, v, _) in self.all_params(after) {
                if !edits.iter().any(|e| e.0 == id && e.1 == n) {
                    edits.push((id, n, v, exact));
                }
            }
        }
        self.notes.edits.extend(edits);
    }

    fn all_params(&self, e: &Entity) -> Vec<(EntityId, String, f64, bool)> {
        let ctx = self.ctx();
        let names: Vec<String> = if is_builtin(&e.type_id) {
            e.geometry
                .as_ref()
                .map(|g| dotloom_document::builtin::geometry_params(g).into_iter().map(|(n, _, _)| n).collect())
                .unwrap_or_default()
        } else {
            plugin_for(self.registry, e).map(|d| d.params.iter().map(|p| p.name.clone()).collect()).unwrap_or_default()
        };
        names.into_iter().filter_map(|n| crate::eval::param_value(ctx, e, &n).map(|v| (e.id, n, v, true))).collect()
    }

    /// Apply one command.
    pub fn apply(&mut self, cmd: Command) -> Result<(), CommandError> {
        match cmd {
            Command::CreateEntity { id, entity } => self.create(id, entity),
            Command::UpdateEntity { id, patch } => self.update(id, patch),
            Command::SetParams { values, mode } => self.set_params(values, mode),
            Command::Transform { ids, transform, policy } => self.transform(&ids, transform, policy),
            Command::Delete { ids, policy } => self.delete(&ids, policy),
            Command::Reorder { id, index } => {
                self.ov.entity(id).ok_or_else(|| not_found(id))?;
                self.ov.order_ops.push(crate::view::OrderOp::Move(id, index));
                self.notes.touched.insert(id);
                Ok(())
            }
            Command::AddConstraint { id, constraint } => {
                let cid = ConstraintId(self.take_id(id.map(|c| c.0))?);
                let c = Constraint {
                    id: cid,
                    rule: constraint.rule,
                    strength: constraint.strength,
                    enabled: constraint.enabled,
                    label: constraint.label,
                    source: constraint.source.or_else(|| Some("user".into())),
                    owner: None,
                    extra: BTreeMap::new(),
                };
                self.check_constraint(&c)?;
                self.ov.put_constraint(c);
                self.notes.created_constraints.push(cid);
                self.notes.touched_constraints.insert(cid);
                Ok(())
            }
            Command::UpdateConstraint { id, patch } => {
                let mut c = self.ov.constraint(id).cloned().ok_or_else(|| not_found(id))?;
                if let Some(r) = patch.rule {
                    c.rule = r;
                }
                if let Some(s) = patch.strength {
                    c.strength = s;
                }
                if let Some(e) = patch.enabled {
                    c.enabled = e;
                }
                if let Some(l) = patch.label {
                    c.label = (!l.is_empty()).then_some(l);
                }
                self.check_constraint(&c)?;
                self.ov.put_constraint(c);
                self.notes.touched_constraints.insert(id);
                Ok(())
            }
            Command::RemoveConstraint { id } => {
                self.ov.constraint(id).ok_or_else(|| not_found(id))?;
                self.ov.delete_constraint(id);
                self.notes.touched_constraints.insert(id);
                Ok(())
            }
            Command::AddLayer { id, name } => {
                if name.chars().count() > 256 {
                    return Err(invalid("layer name", "too long"));
                }
                let lid = LayerId(self.take_id(id.map(|l| l.0))?);
                self.ov.layers_mut().push(Layer::new(lid, name));
                Ok(())
            }
            Command::UpdateLayer { id, patch } => {
                let layers = self.ov.layers_mut();
                let l = layers.iter_mut().find(|l| l.id == id).ok_or_else(|| not_found(id))?;
                if let Some(n) = patch.name {
                    l.name = n;
                }
                if let Some(v) = patch.visible {
                    l.visible = v;
                }
                if let Some(v) = patch.locked {
                    l.locked = v;
                }
                if let Some(c) = patch.color {
                    l.color = (c.components()[3] != 0).then_some(c);
                }
                let members: Vec<EntityId> = self
                    .ov
                    .entity_ids()
                    .into_iter()
                    .filter(|e| self.ov.entity(*e).is_some_and(|x| x.layer == id))
                    .collect();
                self.notes.touched.extend(members);
                Ok(())
            }
            Command::RemoveLayer { id } => {
                if self.ov.entity_ids().iter().any(|e| self.ov.entity(*e).is_some_and(|x| x.layer == id)) {
                    return Err(CommandError::HasDependents {
                        what: id.to_string(),
                        reason: "layer is not empty".into(),
                    });
                }
                let layers = self.ov.layers_mut();
                let pos = layers.iter().position(|l| l.id == id).ok_or_else(|| not_found(id))?;
                if layers.len() == 1 {
                    return Err(invalid("layer", "a document needs at least one layer"));
                }
                layers.remove(pos);
                Ok(())
            }
            Command::MoveLayer { id, index } => {
                let layers = self.ov.layers_mut();
                let pos = layers.iter().position(|l| l.id == id).ok_or_else(|| not_found(id))?;
                let l = layers.remove(pos);
                let i = index.min(layers.len());
                layers.insert(i, l);
                let all = self.ov.entity_ids();
                self.notes.touched.extend(all);
                Ok(())
            }
            Command::Group { id, members, name } => {
                if members.is_empty() {
                    return Err(invalid("group", "no members"));
                }
                for m in &members {
                    self.ov.entity(*m).ok_or_else(|| not_found(m))?;
                }
                // Members leave their previous groups.
                for gid in self.ov.group_ids() {
                    if let Some(g) = self.ov.group(gid).cloned()
                        && g.members.iter().any(|m| members.contains(m))
                    {
                        let mut g2 = g;
                        g2.members.retain(|m| !members.contains(m));
                        self.ov.groups.insert(gid, Some(g2));
                    }
                }
                let gid = GroupId(self.take_id(id.map(|g| g.0))?);
                self.ov
                    .groups
                    .insert(gid, Some(Group { id: gid, name, members, children: Vec::new(), extra: BTreeMap::new() }));
                Ok(())
            }
            Command::Ungroup { id } => {
                self.ov.group(id).ok_or_else(|| not_found(id))?;
                for gid in self.ov.group_ids() {
                    if let Some(g) = self.ov.group(gid).cloned()
                        && g.children.contains(&id)
                    {
                        let mut g2 = g;
                        g2.children.retain(|c| *c != id);
                        self.ov.groups.insert(gid, Some(g2));
                    }
                }
                self.ov.groups.insert(id, None);
                Ok(())
            }
            Command::Paste { clipboard, offset } => self.paste(clipboard, offset),
            Command::Split { id, at, at2 } => self.split(id, at, at2),
            Command::Trim { id, cutters, pick } => self.trim(id, &cutters, pick),
            Command::Extend { id, end, boundaries } => self.extend(id, end, &boundaries),
            Command::SetSettings { patch } => {
                if let Some(g) = patch.grid_spacing
                    && !(g.is_finite() && g > 0.0)
                {
                    return Err(invalid("grid spacing", "must be > 0"));
                }
                let title = patch.title.clone();
                let s = self.ov.settings_mut();
                if let Some(u) = patch.display_unit {
                    s.display_unit = u;
                }
                if let Some(g) = patch.grid_spacing {
                    s.grid.spacing = g;
                }
                if let Some(t) = patch.time_axis {
                    TimeAxis::new(t.origin_s, t.mm_per_second)?;
                    s.time_axis = Some(t);
                }
                if let Some(t) = title {
                    s.extra.insert("title".into(), Value::String(t));
                }
                // Dimensions and time-based entities depend on settings.
                let all = self.ov.entity_ids();
                self.notes.touched.extend(all);
                Ok(())
            }
        }
    }

    fn create(&mut self, id: Option<EntityId>, ne: NewEntity) -> Result<(), CommandError> {
        let type_id = match (&ne.type_id, &ne.geometry) {
            (Some(t), _) => t.clone(),
            (None, Some(g)) => dotloom_document::builtin::builtin_type_for(g.kind()),
            (None, None) => return Err(invalid("entity", "needs a type or geometry")),
        };
        if let Some(kind) = builtin_shape_kind(&type_id) {
            let g = ne.geometry.as_ref().ok_or_else(|| invalid("entity", "built-in type needs geometry"))?;
            if g.kind() != kind {
                return Err(invalid("geometry", format!("{type_id} needs {} geometry", kind.name())));
            }
            g.validate()?;
        } else if is_builtin(&type_id) && type_id.as_str() != types::DIMENSION {
            return Err(invalid("type", format!("unknown built-in type {type_id}")));
        }
        let layer = match ne.layer {
            Some(l) => {
                let lay = self.ov.layer(l).ok_or_else(|| not_found(l))?;
                if lay.locked {
                    return Err(CommandError::ReadOnly { what: l.to_string(), reason: "layer is locked".into() });
                }
                l
            }
            None => self.default_layer()?,
        };
        let mut props = ne.props;
        self.check_props(&type_id, &mut props, true)?;
        let eid = EntityId(self.take_id(id.map(|e| e.0))?);
        let mut e = Entity::new(eid, type_id, layer);
        e.type_version = self.registry.get(&e.type_id).map_or(1, |t| t.compiled.def.version);
        e.geometry = ne.geometry;
        e.props = props;
        e.data = ne.data;
        if let Some(t) = ne.transform {
            if t.inverse().is_err() {
                return Err(invalid("transform", "singular"));
            }
            e.transform = t;
        }
        e.name = ne.name.filter(|n| !n.is_empty());
        if let Some(s) = ne.style {
            s.validate().map_err(|x| invalid("style", x.to_string()))?;
            e.style = s;
        }
        // New entities are pinned where they were created.
        let pins = self.all_params(&e);
        self.ov.put_entity(e);
        self.notes.edits.extend(pins);
        self.notes.created.push(eid);
        self.notes.touched.insert(eid);
        Ok(())
    }

    fn update(&mut self, id: EntityId, patch: EntityPatch) -> Result<(), CommandError> {
        // Unlocking/hiding is allowed on locked entities; content edits are not.
        let meta_only = patch.only_meta() && patch.layer.is_none() && patch.style.is_none();
        let before = self.editable(id, meta_only)?.clone();
        let mut e = before.clone();
        if let Some(g) = patch.geometry {
            if let Some(kind) = builtin_shape_kind(&e.type_id)
                && g.kind() != kind
            {
                return Err(invalid("geometry", format!("{} needs {} geometry", e.type_id, kind.name())));
            }
            g.validate()?;
            e.geometry = Some(g);
        }
        let mut set = BTreeMap::new();
        for (k, v) in patch.props {
            match v {
                Some(v) => {
                    set.insert(k, v);
                }
                None => {
                    e.props.remove(&k);
                }
            }
        }
        self.check_props(&e.type_id, &mut set, false)?;
        e.props.extend(set);
        for (k, v) in patch.data {
            match v {
                Some(v) => {
                    e.data.insert(k, v);
                }
                None => {
                    e.data.remove(&k);
                }
            }
        }
        if let Some(t) = patch.transform {
            if t.inverse().is_err() || !t.is_finite() {
                return Err(invalid("transform", "singular or non-finite"));
            }
            e.transform = t;
        }
        if let Some(n) = patch.name {
            e.name = (!n.is_empty()).then_some(n);
        }
        if let Some(s) = patch.style {
            s.validate().map_err(|x| invalid("style", x.to_string()))?;
            e.style = s;
        }
        if let Some(l) = patch.layer {
            self.ov.layer(l).ok_or_else(|| not_found(l))?;
            e.layer = l;
        }
        if let Some(l) = patch.locked {
            e.locked = l;
        }
        if let Some(h) = patch.hidden {
            e.hidden = h;
        }
        self.record_param_changes(&before, &e, true);
        self.ov.put_entity(e);
        self.notes.touched.insert(id);
        Ok(())
    }

    fn set_params(&mut self, values: Vec<ParamValue>, mode: EditMode) -> Result<(), CommandError> {
        for pv in values {
            let before = self.editable(pv.entity, false)?.clone();
            if !pv.value.is_finite() {
                return Err(invalid(format!("{}.{}", pv.entity, pv.param), "not finite"));
            }
            let mut e = before.clone();
            if is_builtin(&e.type_id) {
                let g = e.geometry.as_mut().ok_or_else(|| invalid("entity", "has no geometry"))?;
                set_geometry_param(g, &pv.param, pv.value).map_err(|x| invalid(pv.param.clone(), x.to_string()))?;
            } else {
                let def = plugin_for(self.registry, &e)
                    .map_err(|r| CommandError::ReadOnly { what: pv.entity.to_string(), reason: format!("{r:?}") })?;
                if let Some((base, axis)) = pv.param.rsplit_once('.') {
                    let Some(PropValue::Point(mut p)) = e.props.get(base).cloned() else {
                        return Err(invalid(pv.param.clone(), "not a point property"));
                    };
                    match axis {
                        "x" => p.x = pv.value,
                        "y" => p.y = pv.value,
                        _ => return Err(invalid(pv.param.clone(), "unknown component")),
                    }
                    e.props.insert(base.to_owned(), PropValue::Point(p));
                } else if matches!(def.def.props.get(&pv.param), Some(PropDef::Number { .. })) {
                    e.props.insert(pv.param.clone(), PropValue::Number(pv.value));
                } else {
                    return Err(invalid(pv.param.clone(), "not a numeric property"));
                }
            }
            if mode == EditMode::Prefer {
                // Keep the old value in the overlay; the solver moves towards the target.
                self.notes.edits.push((pv.entity, pv.param.clone(), pv.value, false));
                self.notes.touched.insert(pv.entity);
                continue;
            }
            self.notes.edits.push((pv.entity, pv.param.clone(), pv.value, true));
            self.ov.put_entity(e);
            self.notes.touched.insert(pv.entity);
        }
        Ok(())
    }

    fn transform(&mut self, ids: &[EntityId], t: Affine, policy: TransformPolicy) -> Result<(), CommandError> {
        if !t.is_finite() || t.inverse().is_err() {
            return Err(invalid("transform", "singular or non-finite"));
        }
        for id in ids {
            let before = self.editable(*id, false)?.clone();
            let mut e = before.clone();
            let mut kind_changed = false;
            match e.geometry.clone() {
                Some(g) if is_builtin(&e.type_id) => {
                    // The world transform `t` expressed in local coordinates.
                    let local_t = e.transform.then(t).then(e.transform.inverse()?);
                    match g.transform(local_t, TransformPolicy::Strict) {
                        Ok(ng) => e.geometry = Some(ng),
                        Err(err) => {
                            let similarity =
                                matches!(local_t.linear_kind(1e-9), dotloom_geometry::LinearKind::Similarity { .. });
                            if matches!(g, Shape::Rect(_)) && similarity {
                                // Rotated rectangles keep their semantics in the entity transform.
                                e.transform = e.transform.then(t);
                            } else if policy == TransformPolicy::Convert {
                                let converted = g.transform(e.transform.then(t), TransformPolicy::Convert)?;
                                e.type_id = dotloom_document::builtin::builtin_type_for(converted.kind());
                                e.geometry = Some(converted);
                                e.transform = Affine::IDENTITY;
                                kind_changed = true;
                            } else {
                                return Err(err.into());
                            }
                        }
                    }
                }
                _ => e.transform = e.transform.then(t),
            }
            self.record_param_changes(&before, &e, true);
            self.ov.put_entity(e);
            if kind_changed {
                self.drop_anchor_constraints(*id);
            }
            self.notes.touched.insert(*id);
        }
        Ok(())
    }

    /// Remove constraints that reference an entity whose kind changed.
    fn drop_anchor_constraints(&mut self, id: EntityId) {
        for cid in self.ov.constraint_ids() {
            if self.ov.constraint(cid).is_some_and(|c| c.rule.entities().contains(&id)) {
                self.ov.delete_constraint(cid);
                self.notes.removed_constraints.push(cid);
                self.notes.notes.push(format!("{cid} removed: {id} changed shape kind"));
            }
        }
    }

    fn delete(&mut self, ids: &[EntityId], policy: DeletePolicy) -> Result<(), CommandError> {
        let mut queue: Vec<EntityId> = ids.to_vec();
        let mut doomed: BTreeSet<EntityId> = BTreeSet::new();
        for id in ids {
            self.editable(*id, true)?;
            if self.ov.layer(self.ov.entity(*id).map_or(LayerId(0), |e| e.layer)).is_some_and(|l| l.locked) {
                return Err(CommandError::ReadOnly { what: id.to_string(), reason: "layer is locked".into() });
            }
        }
        let all_ids = self.ov.entity_ids();
        while let Some(id) = queue.pop() {
            if !doomed.insert(id) {
                continue;
            }
            // Referencing entities.
            for other in &all_ids {
                if doomed.contains(other) {
                    continue;
                }
                let Some(o) = self.ov.entity(*other) else { continue };
                let refs: Vec<String> =
                    o.props.iter().filter(|(_, v)| v.referenced_entity() == Some(id)).map(|(k, _)| k.clone()).collect();
                if refs.is_empty() {
                    continue;
                }
                if policy == DeletePolicy::Reject {
                    return Err(CommandError::HasDependents {
                        what: id.to_string(),
                        reason: format!("{other} references it"),
                    });
                }
                let rule = if o.type_id.as_str() == types::DIMENSION || is_builtin(&o.type_id) {
                    OnDelete::Cascade
                } else {
                    plugin_for(self.registry, o)
                        .ok()
                        .and_then(|d| {
                            refs.iter().find_map(|k| match d.def.props.get(k) {
                                Some(PropDef::Ref { on_delete, required, .. }) => {
                                    Some(if *required && *on_delete == OnDelete::Clear {
                                        OnDelete::Cascade
                                    } else {
                                        *on_delete
                                    })
                                }
                                _ => None,
                            })
                        })
                        .unwrap_or(OnDelete::Cascade)
                };
                match rule {
                    OnDelete::Cascade => queue.push(*other),
                    OnDelete::Reject => {
                        return Err(CommandError::HasDependents {
                            what: id.to_string(),
                            reason: format!("{other} references it"),
                        });
                    }
                    OnDelete::Clear => {
                        let mut o2 = o.clone();
                        for k in &refs {
                            o2.props.remove(k);
                        }
                        self.ov.put_entity(o2);
                        self.notes.touched.insert(*other);
                        self.notes.notes.push(format!("{other}: reference cleared"));
                    }
                }
            }
        }
        // Constraints.
        for cid in self.ov.constraint_ids() {
            let Some(c) = self.ov.constraint(cid) else { continue };
            let hit =
                c.rule.entities().iter().any(|e| doomed.contains(e)) || c.owner.is_some_and(|o| doomed.contains(&o));
            if hit {
                if policy == DeletePolicy::Reject {
                    return Err(CommandError::HasDependents {
                        what: "entities".into(),
                        reason: format!("{cid} references them"),
                    });
                }
                self.ov.delete_constraint(cid);
                self.notes.removed_constraints.push(cid);
            }
        }
        // Groups.
        for gid in self.ov.group_ids() {
            if let Some(g) = self.ov.group(gid).cloned()
                && g.members.iter().any(|m| doomed.contains(m))
            {
                let mut g2 = g;
                g2.members.retain(|m| !doomed.contains(m));
                if g2.members.is_empty() && g2.children.is_empty() {
                    self.ov.groups.insert(gid, None);
                } else {
                    self.ov.groups.insert(gid, Some(g2));
                }
            }
        }
        for id in &doomed {
            self.ov.delete_entity(*id);
            self.notes.deleted.push(*id);
            self.notes.touched.insert(*id);
        }
        Ok(())
    }

    fn paste(&mut self, clip: Clipboard, offset: Vector) -> Result<(), CommandError> {
        if !(offset.x.is_finite() && offset.y.is_finite()) {
            return Err(invalid("offset", "not finite"));
        }
        let mut map = BTreeMap::new();
        for e in &clip.entities {
            map.insert(e.id, EntityId(self.ov.alloc_id()));
        }
        let fallback = self.default_layer()?;
        let t = Affine::translate(offset);
        for e in &clip.entities {
            let Some(&nid) = map.get(&e.id) else { continue };
            let mut n = e.clone();
            n.id = nid;
            if self.ov.layer(n.layer).is_none_or(|l| l.locked) {
                n.layer = fallback;
            }
            n.transform = n.transform.then(t);
            let mut dropped = Vec::new();
            for (k, v) in &mut n.props {
                match v {
                    PropValue::Ref(r) => match map.get(&r.entity) {
                        Some(m) => r.entity = *m,
                        None if self.ov.entity(r.entity).is_none() => dropped.push(k.clone()),
                        None => {}
                    },
                    PropValue::Anchor(a) => match map.get(&a.entity) {
                        Some(m) => a.entity = *m,
                        None if self.ov.entity(a.entity).is_none() => dropped.push(k.clone()),
                        None => {}
                    },
                    _ => {}
                }
            }
            for k in dropped {
                n.props.remove(&k);
                self.notes.notes.push(format!("{nid}: reference `{k}` dropped (target not in this document)"));
            }
            if !is_builtin(&n.type_id) && plugin_for(self.registry, &n).is_ok() {
                let mut p = n.props.clone();
                self.check_props(&n.type_id.clone(), &mut p, true)?;
                n.props = p;
            }
            let pins = self.all_params(&n);
            self.ov.put_entity(n);
            self.notes.edits.extend(pins);
            self.notes.created.push(nid);
            self.notes.touched.insert(nid);
        }
        for c in &clip.constraints {
            let mut n = c.clone();
            if !n.rule.remap(&map) {
                continue;
            }
            if let RuleSpec::FixPoint { at, .. } = &mut n.rule {
                *at += offset;
            }
            n.owner = n.owner.and_then(|o| map.get(&o).copied());
            n.id = ConstraintId(self.ov.alloc_id());
            self.notes.created_constraints.push(n.id);
            self.notes.touched_constraints.insert(n.id);
            self.ov.put_constraint(n);
        }
        for g in &clip.groups {
            let members: Vec<EntityId> = g.members.iter().filter_map(|m| map.get(m).copied()).collect();
            if members.is_empty() {
                continue;
            }
            let gid = GroupId(self.ov.alloc_id());
            self.ov.groups.insert(
                gid,
                Some(Group { id: gid, name: g.name.clone(), members, children: Vec::new(), extra: BTreeMap::new() }),
            );
        }
        Ok(())
    }

    fn check_constraint(&self, c: &Constraint) -> Result<(), CommandError> {
        if !c.rule.values_valid() {
            return Err(invalid("constraint", format!("{} has invalid values", c.rule.kind_name())));
        }
        for e in c.rule.entities() {
            self.ov.entity(e).ok_or_else(|| not_found(e))?;
        }
        let ctx = self.ctx();
        for a in c.rule.anchors() {
            if evaluate(ctx, a.entity).anchor(&a.anchor).is_none() {
                return Err(invalid("constraint", format!("{} has no anchor `{}`", a.entity, a.anchor)));
            }
        }
        for p in c.rule.params() {
            let name = match &p.slot {
                dotloom_document::ParamSlot::Geom(n) | dotloom_document::ParamSlot::Prop(n) => n,
            };
            let e = self.ov.entity(p.entity).ok_or_else(|| not_found(p.entity))?;
            if crate::eval::param_value(ctx, e, name).is_none() {
                return Err(invalid("constraint", format!("{} has no numeric parameter `{name}`", p.entity)));
            }
        }
        Ok(())
    }

    fn world_shape(&self, id: EntityId) -> Result<Shape, CommandError> {
        let e = self.ov.entity(id).ok_or_else(|| not_found(id))?;
        let g = e.geometry.as_ref().ok_or_else(|| invalid("entity", format!("{id} has no editable geometry")))?;
        Ok(g.transform(e.transform, TransformPolicy::Convert)?)
    }

    fn set_world_geometry(&mut self, id: EntityId, world: Shape) -> Result<(), CommandError> {
        let before = self.editable(id, false)?.clone();
        let mut e = before.clone();
        let inv = e.transform.inverse()?;
        let local = world.transform(inv, TransformPolicy::Strict)?;
        e.type_id = dotloom_document::builtin::builtin_type_for(local.kind());
        e.geometry = Some(local);
        let anchors_before = evaluate(self.ctx(), id).anchors;
        self.ov.put_entity(e.clone());
        // Constraints on anchors that moved or disappeared are removed.
        let after = evaluate(self.ctx(), id);
        let moved: BTreeSet<String> = anchors_before
            .iter()
            .filter(|a| {
                after.anchor(&a.name).is_none_or(|p| p.distance(a.point) > 1e-9 * (1.0 + a.point.to_vector().length()))
            })
            .map(|a| a.name.clone())
            .collect();
        for cid in self.ov.constraint_ids() {
            let Some(c) = self.ov.constraint(cid) else { continue };
            let refs_moved = c.rule.anchors().iter().any(|a| a.entity == id && moved.contains(&a.anchor))
                || (!c.rule.params().is_empty() && c.rule.params().iter().any(|p| p.entity == id))
                || (c.rule.entities().contains(&id) && c.rule.anchors().is_empty() && c.rule.params().is_empty());
            if refs_moved {
                self.ov.delete_constraint(cid);
                self.notes.removed_constraints.push(cid);
                self.notes.notes.push(format!("{cid} removed: its anchor on {id} changed"));
            }
        }
        let pins = self.all_params(&e);
        self.notes.edits.extend(pins);
        self.notes.touched.insert(id);
        Ok(())
    }

    fn split(&mut self, id: EntityId, at: Point, at2: Option<Point>) -> Result<(), CommandError> {
        self.editable(id, false)?;
        let world = self.world_shape(id)?;
        let tol = ModelTolerance::DEFAULT;
        let pieces: Vec<Shape> = match (&world, at2) {
            (Shape::Circle(c), Some(b)) => {
                dotloom_geometry::edit::split_circle(*c, at, b, tol)?.into_iter().map(Shape::Arc).collect()
            }
            _ => dotloom_geometry::edit::split_at(&world, at, tol)?,
        };
        let mut it = pieces.into_iter();
        let first = it.next().ok_or_else(|| invalid("split", "no pieces"))?;
        let base = self.ov.entity(id).cloned().ok_or_else(|| not_found(id))?;
        self.set_world_geometry(id, first)?;
        for p in it {
            let nid = EntityId(self.ov.alloc_id());
            let mut n = base.clone();
            n.id = nid;
            n.transform = Affine::IDENTITY;
            n.type_id = dotloom_document::builtin::builtin_type_for(p.kind());
            n.geometry = Some(p);
            let pins = self.all_params(&n);
            self.ov.put_entity(n);
            self.notes.edits.extend(pins);
            self.notes.created.push(nid);
            self.notes.touched.insert(nid);
        }
        Ok(())
    }

    fn trim(&mut self, id: EntityId, cutters: &[EntityId], pick: Point) -> Result<(), CommandError> {
        self.editable(id, false)?;
        let world = self.world_shape(id)?;
        let ctx = self.ctx();
        let cut: Vec<Shape> =
            cutters.iter().filter(|c| **c != id).flat_map(|c| evaluate(ctx, *c).shapes().collect::<Vec<_>>()).collect();
        let pieces = dotloom_geometry::edit::trim(&world, &cut, pick, ModelTolerance::DEFAULT)?;
        let base = self.ov.entity(id).cloned().ok_or_else(|| not_found(id))?;
        let mut it = pieces.into_iter();
        match it.next() {
            None => return self.delete(&[id], DeletePolicy::Cascade),
            Some(first) => self.set_world_geometry(id, first)?,
        }
        for p in it {
            let nid = EntityId(self.ov.alloc_id());
            let mut n = base.clone();
            n.id = nid;
            n.transform = Affine::IDENTITY;
            n.type_id = dotloom_document::builtin::builtin_type_for(p.kind());
            n.geometry = Some(p);
            let pins = self.all_params(&n);
            self.ov.put_entity(n);
            self.notes.edits.extend(pins);
            self.notes.created.push(nid);
            self.notes.touched.insert(nid);
        }
        Ok(())
    }

    fn extend(&mut self, id: EntityId, end: CurveEnd, boundaries: &[EntityId]) -> Result<(), CommandError> {
        self.editable(id, false)?;
        let world = self.world_shape(id)?;
        let ctx = self.ctx();
        let bounds: Vec<Shape> = boundaries
            .iter()
            .filter(|b| **b != id)
            .flat_map(|b| evaluate(ctx, *b).shapes().collect::<Vec<_>>())
            .collect();
        let out = dotloom_geometry::edit::extend(&world, end, &bounds, ModelTolerance::DEFAULT)?;
        self.set_world_geometry(id, out)
    }
}
