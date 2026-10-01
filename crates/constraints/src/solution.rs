//! Solve results, statuses and structured diagnostics.

use serde::{Deserialize, Serialize};

use crate::VarId;

/// Which backend handled a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Backend {
    /// No rules: targets/stays only, or constant rules.
    Trivial,
    /// kasuari (Cassowary) for purely linear components.
    Linear,
    /// Hierarchical damped Gauss–Newton for nonlinear/mixed components.
    Numeric,
}

/// Status of one component (or the aggregate solve).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Status {
    /// All hard rules hold and no degrees of freedom remain.
    Solved,
    /// All hard rules hold; `dof` independent motions remain. This is a valid state.
    Underconstrained {
        /// Remaining degrees of freedom.
        dof: usize,
    },
    /// Proven contradiction between hard rules (see diagnostics for the evidence).
    Conflicting,
    /// The solver stopped without satisfying hard rules. This is *not* a proof of
    /// infeasibility; `suspected_conflict` marks a stationary point with residual left.
    NotConverged {
        /// The hard residual stopped decreasing at a non-zero value.
        suspected_conflict: bool,
    },
    /// The job was cancelled before completion.
    Cancelled,
    /// A rule could not be expressed by the supported equation classes.
    Unsupported,
}

impl Status {
    /// Whether the resulting values satisfy all hard rules and may be committed.
    #[must_use]
    pub const fn is_acceptable(self) -> bool {
        matches!(self, Self::Solved | Self::Underconstrained { .. })
    }

    fn rank(self) -> u8 {
        match self {
            Self::Solved => 0,
            Self::Underconstrained { .. } => 1,
            Self::NotConverged { .. } => 3,
            Self::Conflicting => 4,
            Self::Unsupported => 5,
            Self::Cancelled => 6,
        }
    }

    /// Combine two statuses (worst wins; DOF add up).
    #[must_use]
    pub fn combine(self, o: Self) -> Self {
        match (self, o) {
            (Self::Underconstrained { dof: a }, Self::Underconstrained { dof: b }) => {
                Self::Underconstrained { dof: a + b }
            }
            (Self::NotConverged { suspected_conflict: a }, Self::NotConverged { suspected_conflict: b }) => {
                Self::NotConverged { suspected_conflict: a || b }
            }
            _ => {
                if o.rank() > self.rank() {
                    o
                } else {
                    self
                }
            }
        }
    }
}

/// Diagnostic category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticKind {
    /// Hard rules contradict each other (certain).
    Conflict,
    /// Hard rules could not be satisfied; likely but not proven contradictory.
    SuspectedConflict,
    /// Iteration budget exhausted.
    NotConverged,
    /// Rules are redundant (consistent duplicates); informational.
    Redundant,
    /// Rule class not supported.
    Unsupported,
    /// A preference could not be met because harder rules prevent it.
    PreferenceUnmet,
    /// The job was cancelled.
    Cancelled,
}

/// Certainty of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Certainty {
    /// Proven (e.g. Cassowary required-failure, all-constant rule violated).
    Certain,
    /// Heuristic evidence (stationary point of the residual).
    Suspected,
}

/// A structured diagnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// Category.
    pub kind: DiagnosticKind,
    /// Certainty.
    pub certainty: Certainty,
    /// Rule IDs involved.
    pub rules: Vec<u64>,
    /// Rule labels (same order).
    pub labels: Vec<String>,
    /// Rule sources (same order).
    pub sources: Vec<String>,
    /// Related entity IDs (deduplicated).
    pub entities: Vec<u64>,
    /// Largest absolute residual among the rules, in model units.
    pub residual: Option<f64>,
    /// English message for developers; UIs build their own text from the fields.
    pub message: String,
}

/// Report of one solved component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentReport {
    /// Free variables of the component.
    pub vars: Vec<VarId>,
    /// Rule IDs of the component.
    pub rules: Vec<u64>,
    /// Backend used.
    pub backend: Backend,
    /// Status.
    pub status: Status,
    /// Iterations used (numeric backend) or 1.
    pub iterations: u32,
    /// Largest hard residual relative to its row scale.
    pub max_hard_residual: f64,
}

/// Complete solve result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Solution {
    /// Values for every variable (unchanged input values when not acceptable).
    pub values: Vec<f64>,
    /// Aggregate status.
    pub status: Status,
    /// Per-component reports.
    pub components: Vec<ComponentReport>,
    /// Diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Total iterations.
    pub iterations: u32,
}

impl Solution {
    /// Whether the values may be committed.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.status.is_acceptable()
    }
}
