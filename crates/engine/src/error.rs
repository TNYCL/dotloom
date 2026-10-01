//! Engine errors and solve diagnostics as reported to hosts.

use dotloom_constraints::{Certainty, DiagnosticKind, Status};
use dotloom_document::{ConstraintId, EntityId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::CommandError;

/// A solver diagnostic mapped back to document objects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    /// Category.
    pub kind: DiagnosticKind,
    /// Certain or suspected.
    pub certainty: Certainty,
    /// Document constraints involved.
    pub constraints: Vec<ConstraintId>,
    /// Plugin template rules involved `(entity, label)`.
    pub templates: Vec<(EntityId, String)>,
    /// Typed edits involved `(entity, parameter)`.
    pub edits: Vec<(EntityId, String)>,
    /// Entities involved.
    pub entities: Vec<EntityId>,
    /// Rule labels.
    pub labels: Vec<String>,
    /// Largest residual (model units / radians).
    pub residual: Option<f64>,
    /// Developer message (UIs format their own text from the fields).
    pub message: String,
}

/// Nearest feasible value for an edit that could not be satisfied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NearestValue {
    /// Entity.
    pub entity: EntityId,
    /// Parameter.
    pub param: String,
    /// Requested value.
    pub requested: f64,
    /// Closest value that satisfies every hard rule.
    pub feasible: f64,
}

/// Why solving rejected a transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SolveFailure {
    /// Aggregate status.
    pub status: Status,
    /// Diagnostics.
    pub diagnostics: Vec<DiagnosticReport>,
    /// Nearest feasible values of the requested edits (linear conflicts).
    pub nearest: Vec<NearestValue>,
}

/// Engine errors. A failed request never changes the document.
#[derive(Debug, Clone, PartialEq, Error, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[non_exhaustive]
pub enum EngineError {
    /// `expectedRevision` does not match.
    #[error("stale request: expected revision {expected}, document is at {actual}")]
    Stale {
        /// Expected.
        expected: u64,
        /// Actual.
        actual: u64,
    },
    /// Another interaction (drag or pending solve) is in progress.
    #[error("engine busy: {reason}")]
    Busy {
        /// Reason.
        reason: String,
    },
    /// A command was rejected.
    #[error("{error}")]
    Command {
        /// Error.
        error: CommandError,
    },
    /// The resulting document would violate an invariant.
    #[error("invalid result: {message}")]
    Invalid {
        /// Explanation.
        message: String,
    },
    /// Hard rules could not be satisfied.
    #[error("constraints not satisfied: {:?}", failure.status)]
    Solve {
        /// Details.
        failure: SolveFailure,
    },
    /// The independent check found a violated hard rule after solving.
    #[error("independent check failed for {what}: residual {residual:e} > {tolerance:e}")]
    Validation {
        /// Rule.
        what: String,
        /// Residual.
        residual: f64,
        /// Tolerance.
        tolerance: f64,
    },
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// Nothing to undo.
    #[error("nothing to undo")]
    NothingToUndo,
    /// Nothing to redo.
    #[error("nothing to redo")]
    NothingToRedo,
    /// No interaction of the requested kind is active.
    #[error("no active {what}")]
    NotActive {
        /// What.
        what: String,
    },
    /// Loading a document failed.
    #[error("cannot load document: {message}")]
    Load {
        /// Explanation.
        message: String,
    },
    /// Plugin registration failed.
    #[error("plugin error: {message}")]
    Plugin {
        /// Explanation.
        message: String,
    },
}

impl From<CommandError> for EngineError {
    fn from(error: CommandError) -> Self {
        Self::Command { error }
    }
}

impl EngineError {
    /// Stable machine-readable code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Stale { .. } => "stale",
            Self::Busy { .. } => "busy",
            Self::Command { .. } => "command",
            Self::Invalid { .. } => "invalid",
            Self::Solve { .. } => "solve",
            Self::Validation { .. } => "validation",
            Self::Cancelled => "cancelled",
            Self::NothingToUndo => "nothingToUndo",
            Self::NothingToRedo => "nothingToRedo",
            Self::NotActive { .. } => "notActive",
            Self::Load { .. } => "load",
            Self::Plugin { .. } => "plugin",
        }
    }

    /// Solve status if this is a solve failure.
    #[must_use]
    pub fn solve_status(&self) -> Option<Status> {
        match self {
            Self::Solve { failure } => Some(failure.status),
            _ => None,
        }
    }
}
