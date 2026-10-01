//! # dotloom-scene
//!
//! The public scene contract between the Dotloom engine and renderers.
//!
//! The engine turns documents into [`SceneItem`]s (world-space shapes with resolved
//! styles, keyed by entity ID) and ships incremental [`SceneDelta`]s. Renderers
//! consume deltas and never read documents, so any renderer that understands this
//! crate can replace the default wgpu renderer.
//!
//! Shapes are exact (`f64`, curves kept as curves); renderers flatten them with a
//! zoom-dependent tolerance. Deltas have a compact, versioned binary encoding
//! ([`SceneDelta::encode`]) suitable for transfer between a Worker and the main
//! thread without JSON parsing.

mod codec;

pub use codec::{DecodeError, MAGIC, SCENE_FORMAT_VERSION};
pub use dotloom_geometry as geometry;
use dotloom_geometry::{Aabb, Point, Segment, Shape, Text, Vector};
use serde::{Deserialize, Serialize};

/// Packed RGBA color (`0xRRGGBBAA`).
pub type Rgba = u32;

/// Stroke style (screen-constant width).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Color.
    pub color: Rgba,
    /// Width in CSS pixels.
    pub width: f32,
    /// Dash pattern in CSS pixels (empty = solid).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dash: Vec<f32>,
}

/// One drawable primitive in world coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "prim", rename_all = "camelCase")]
pub enum Primitive {
    /// A shape with optional stroke and fill (fill applies to regions only).
    Shape {
        /// Geometry (world coordinates).
        shape: Shape,
        /// Stroke.
        stroke: Option<Stroke>,
        /// Fill color.
        fill: Option<Rgba>,
    },
    /// Text in world units.
    Text {
        /// Text geometry.
        text: Text,
        /// Color.
        color: Rgba,
    },
    /// Filled arrow head (dimension lines); `size` in model units.
    Arrow {
        /// Tip.
        tip: Point,
        /// Unit direction pointing at the tip.
        direction: Vector,
        /// Length in model units.
        size: f64,
        /// Color.
        color: Rgba,
    },
}

/// Item state flags.
pub mod flags {
    /// Selected.
    pub const SELECTED: u8 = 1;
    /// Shown from a transient preview (drag), not committed.
    pub const PREVIEW: u8 = 2;
    /// Locked (entity or layer).
    pub const LOCKED: u8 = 4;
    /// Hovered.
    pub const HOVER: u8 = 8;
    /// Read-only (plugin missing or disabled; drawn from its fallback).
    pub const READONLY: u8 = 16;
    /// Has a constraint problem.
    pub const PROBLEM: u8 = 32;
}

/// Everything drawn for one entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneItem {
    /// Entity ID.
    pub id: u64,
    /// Index of the entity's layer (bottom = 0).
    pub layer: u32,
    /// Bounding box of all primitives (world).
    pub bbox: Aabb,
    /// [`flags`] bit set.
    pub flags: u8,
    /// Primitives in drawing order.
    pub prims: Vec<Primitive>,
}

/// Incremental scene update.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneDelta {
    /// Document revision the delta brings the scene to.
    pub revision: u64,
    /// The delta shows transient preview state on top of `revision`.
    pub preview: bool,
    /// Drop every item before applying.
    pub reset: bool,
    /// New or changed items.
    pub upserts: Vec<SceneItem>,
    /// Removed item IDs.
    pub removals: Vec<u64>,
    /// Complete draw order (entity IDs bottom to top) when it changed.
    pub order: Option<Vec<u64>>,
}

impl SceneDelta {
    /// Whether the delta changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.reset && self.upserts.is_empty() && self.removals.is_empty() && self.order.is_none()
    }
}

/// Snap/handle marker kinds drawn by overlays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerKind {
    /// Selection grip.
    Handle,
    /// Endpoint snap.
    Endpoint,
    /// Midpoint snap.
    Midpoint,
    /// Center snap.
    Center,
    /// Intersection snap.
    Intersection,
    /// Point on curve.
    Nearest,
    /// Grid point.
    Grid,
    /// Quadrant / vertex / other anchors.
    Anchor,
}

/// A marker in world coordinates, drawn at constant screen size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Position.
    pub at: Point,
    /// Kind.
    pub kind: MarkerKind,
}

/// Interaction overlay (selection grips, snap marker, marquee, guides), set by the
/// host UI directly on the renderer; it is not part of the document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Overlay {
    /// Markers.
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// Selection rectangle (world).
    #[serde(default)]
    pub marquee: Option<Aabb>,
    /// Whether the marquee is a crossing (dashed) selection.
    #[serde(default)]
    pub crossing: bool,
    /// Construction guides (world).
    #[serde(default)]
    pub guides: Vec<Segment>,
    /// In-progress drawing preview shapes (world), drawn with the preview style.
    #[serde(default)]
    pub sketch: Vec<Shape>,
}
