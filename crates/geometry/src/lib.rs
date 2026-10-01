//! # dotloom-geometry
//!
//! Headless 2D geometry for Dotloom: `f64` primitives, affine transforms, exact
//! orientation predicates, curve intersections, split/trim/extend, dimension
//! geometry, dimensioned quantities and a bounding-box spatial index.
//!
//! This crate has no DOM, window or GPU dependency.
//!
//! ```
//! use dotloom_geometry::{Point, Segment, Shape, Curve, intersect, ModelTolerance};
//!
//! let a = Curve::Line(Segment::new(Point::new(0.0, 0.0), Point::new(2.0, 2.0)));
//! let b = Curve::Line(Segment::new(Point::new(0.0, 2.0), Point::new(2.0, 0.0)));
//! let hits = intersect::intersect(&a, &b, ModelTolerance::DEFAULT);
//! assert_eq!(hits.points[0].point, Point::new(1.0, 1.0));
//! ```

mod aabb;
mod affine;
mod curve;
pub mod dimension;
pub mod edit;
mod error;
pub mod intersect;
mod point;
mod shape;
pub mod spatial;
mod tolerance;
pub mod units;

pub use aabb::Aabb;
pub use affine::{Affine, LinearKind};
pub use curve::{Arc, Circle, CubicBez, Curve, QuadBez, Segment, circumcenter};
pub use error::{GeoResult, GeometryError};
pub use point::{Orientation, Point, Vector, normalize_angle, normalize_angle_signed, orientation};
pub use shape::{
    Anchor, AnchorKind, FlatPath, HAlign, MAX_SHAPE_POINTS, MAX_TEXT_CHARS, Path, PathEl, PointShape, Polygon,
    Polyline, Rect, Shape, ShapeKind, SubPath, Text, TransformPolicy, VAlign, arc_to_cubics, point_in_ring,
    ring_signed_area,
};
pub use spatial::SpatialIndex;
pub use tolerance::{FlattenTolerance, ModelTolerance, ScreenTolerance};
