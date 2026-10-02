//! # dotloom-engine
//!
//! The headless Dotloom engine. It owns the only editable copy of a document and
//! runs every change through one pipeline:
//!
//! ```text
//! commands → working overlay → structural validation → solve (affected component)
//!          → independent hard-rule check → atomic commit → revision + events
//! ```
//!
//! A failed transaction leaves the document untouched. Undo/redo apply stored
//! before/after states without re-solving. Drags produce transient previews and a
//! single history entry. Plugin entity types are data ([`EntityTypeDef`]) compiled
//! from a typed expression language; no host code runs inside the solver.
//!
//! The engine has no DOM, window or GPU dependency.

mod apply;
mod build;
mod check;
mod command;
mod drag;
mod engine;
mod error;
pub mod eval;
mod history;
mod index;
pub mod lang;
mod migrate;
mod query;
pub mod registry;
mod scene_build;
mod solve;
mod validate;
mod view;

pub use command::{
    Command, CommandError, ConstraintPatch, ConstraintSpec, DeletePolicy, EditMode, EntityPatch, LayerPatch, NewEntity,
    ParamValue, SettingsPatch,
};
pub use dotloom_constraints as constraints;
pub use dotloom_document as document;
pub use dotloom_geometry as geometry;
pub use dotloom_scene as scene;
pub use drag::{DragPreview, DragSpec};
pub use engine::{ApplyOptions, CommitReport, Engine, EngineOptions, Event, PendingState, SolverStats, Transaction};
pub use error::{DiagnosticReport, EngineError, NearestValue, SolveFailure};
pub use migrate::migrate_entities;
pub use query::{Hit, SelectMode, Snap, SnapKind, SnapOptions, SnapQuery};
pub use registry::{EntityTypeDef, Registry, RegistryError};
pub use scene_build::THEME_DEFAULT;
pub use view::{DocView, Overlay};
