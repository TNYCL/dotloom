//! # dotloom-document
//!
//! The Dotloom document model: stable IDs, namespaced entity types with versions,
//! typed properties, transforms, layers, groups, anchor/parameter references and
//! constraint descriptions, plus structural validation, canonical hashing,
//! copy/paste remapping and schema migration.
//!
//! This crate is pure data and validation; it has no solver, DOM or GPU dependency.
//!
//! ```
//! use dotloom_document::{Document, Entity, EntityId, LayerId, builtin::builtin_type_for};
//! use dotloom_geometry::{Point, Segment, Shape, ShapeKind};
//!
//! let mut doc = Document::new();
//! let id = EntityId(doc.alloc_id());
//! doc.insert_entity(
//!     Entity::new(id, builtin_type_for(ShapeKind::Line), LayerId(1))
//!         .with_geometry(Shape::Line(Segment::new(Point::new(0.0, 0.0), Point::new(100.0, 0.0)))),
//! );
//! assert!(doc.validate().is_ok());
//! let json = doc.to_json_string().unwrap();
//! let (back, _) = Document::from_json_str(&json, &Default::default()).unwrap();
//! assert!(back.semantic_eq(&doc));
//! ```

pub mod builtin;
mod canonical;
mod clipboard;
mod constraint;
mod document;
mod error;
mod ids;
mod migrate;
mod model;
mod validate;
mod value;

pub use canonical::fnv1a64;
pub use clipboard::{Clipboard, PasteReport};
pub use constraint::{Cmp, Constraint, LineRef, ParamRef, ParamSlot, RuleSpec, StrengthSpec, Term};
pub use document::{Dependents, Document};
pub use error::DocError;
pub use ids::{ConstraintId, EntityId, GroupId, LayerId, TypeId};
pub use migrate::{MAX_JSON_DEPTH, MigrationNote, OLDEST_SUPPORTED_SCHEMA, SCHEMA_VERSION, migrate, schema_of};
pub use model::{Entity, GridSettings, Group, Layer, Meta, Settings};
pub use validate::Limits;
pub use value::{AnchorRef, Color, PropValue, RefValue, Style};
