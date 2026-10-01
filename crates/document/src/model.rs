//! Entities, layers, groups, metadata and settings.

use std::collections::BTreeMap;

use dotloom_geometry::{
    Affine, Shape,
    units::{LengthUnit, TimeAxis},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Color, EntityId, GroupId, LayerId, PropValue, Style, TypeId};

fn one() -> u32 {
    1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one(v: &u32) -> bool {
    *v == 1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(v: &bool) -> bool {
    !*v
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_identity(a: &Affine) -> bool {
    a.is_identity()
}

fn yes() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(v: &bool) -> bool {
    *v
}

/// A document entity.
///
/// Built-in types (`dotloom.*`) carry canonical `geometry`. Plugin types carry typed
/// `props`; their drawable geometry is *derived* by the type definition and is not
/// stored, except for the optional `fallback` representation that lets viewers
/// without the plugin display (not edit) the entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    /// Stable identifier.
    pub id: EntityId,
    /// Namespaced type.
    #[serde(rename = "type")]
    pub type_id: TypeId,
    /// Version of the type's schema the properties follow.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub type_version: u32,
    /// Layer membership.
    pub layer: LayerId,
    /// Local-to-world transform.
    #[serde(default, skip_serializing_if = "is_identity")]
    pub transform: Affine,
    /// Canonical geometry (built-in types) in local coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<Shape>,
    /// Typed properties (canonical units).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, PropValue>,
    /// Opaque, namespaced plugin payloads preserved verbatim.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, Value>,
    /// Optional user-visible name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Style overrides.
    #[serde(default, skip_serializing_if = "Style::is_default")]
    pub style: Style,
    /// Locked against interactive editing (not a solver lock).
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Hidden from rendering and picking.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// Standard representation for viewers that lack the plugin (derived snapshot,
    /// world coordinates). Never used for editing or measurement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Vec<Shape>>,
    /// Unknown fields from newer producers, preserved on save.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Entity {
    /// New entity with default metadata.
    #[must_use]
    pub fn new(id: EntityId, type_id: TypeId, layer: LayerId) -> Self {
        Self {
            id,
            type_id,
            type_version: 1,
            layer,
            transform: Affine::IDENTITY,
            geometry: None,
            props: BTreeMap::new(),
            data: BTreeMap::new(),
            name: None,
            style: Style::default(),
            locked: false,
            hidden: false,
            fallback: None,
            extra: BTreeMap::new(),
        }
    }

    /// Builder: geometry.
    #[must_use]
    pub fn with_geometry(mut self, s: Shape) -> Self {
        self.geometry = Some(s);
        self
    }

    /// Builder: property.
    #[must_use]
    pub fn with_prop(mut self, k: impl Into<String>, v: PropValue) -> Self {
        self.props.insert(k.into(), v);
        self
    }

    /// Entities referenced by properties.
    pub fn referenced_entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.props.values().filter_map(PropValue::referenced_entity)
    }
}

/// A layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// Identifier.
    pub id: LayerId,
    /// Display name.
    pub name: String,
    /// Visible.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub visible: bool,
    /// Locked against editing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Default stroke color for entities on this layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Unknown fields preserved.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Layer {
    /// New visible, unlocked layer.
    #[must_use]
    pub fn new(id: LayerId, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), visible: true, locked: false, color: None, extra: BTreeMap::new() }
    }
}

/// A group of entities and nested groups. Each entity and group has at most one
/// parent group; nesting is acyclic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// Identifier.
    pub id: GroupId,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Member entities.
    #[serde(default)]
    pub members: Vec<EntityId>,
    /// Nested groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<GroupId>,
    /// Unknown fields preserved.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Document metadata.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    /// Title.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Unknown fields preserved.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Grid settings (presentation defaults stored with the document).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridSettings {
    /// Grid spacing in mm.
    pub spacing: f64,
    /// Major line every N minor lines.
    pub major_every: u32,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self { spacing: 10.0, major_every: 10 }
    }
}

/// Document settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Unit shown to users and used by importers by default (storage is always mm).
    #[serde(default)]
    pub display_unit: LengthUnit,
    /// Grid defaults.
    #[serde(default)]
    pub grid: GridSettings,
    /// Optional time axis for timeline documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_axis: Option<TimeAxis>,
    /// Unknown fields preserved.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            display_unit: LengthUnit::Millimetre,
            grid: GridSettings::default(),
            time_axis: None,
            extra: BTreeMap::new(),
        }
    }
}
