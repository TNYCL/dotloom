//! Problem description handed to the solver.

use serde::{Deserialize, Serialize};

use crate::{Expr, VarId};

/// Constraint strength. `Required` rules are hard: a solution violating them is
/// never accepted. The others are preferences ordered by priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strength {
    /// Weak preference.
    Weak,
    /// Medium preference.
    Medium,
    /// Strong preference (drag targets use this).
    Strong,
    /// Hard rule.
    #[default]
    Required,
}

impl Strength {
    /// Least-squares weight for the numeric backend (not squared). Hard rules are
    /// constraints, not weights; the bounded 10× ladder keeps the KKT system well
    /// conditioned (ADR-0004).
    #[must_use]
    pub const fn weight(self) -> f64 {
        match self {
            Self::Required => f64::INFINITY,
            Self::Strong => 1.0,
            Self::Medium => 0.1,
            Self::Weak => 0.01,
        }
    }
}

/// Weight of the implicit "stay near the previous valid value" preference. It is
/// below every user strength so any explicit preference wins over staying put.
pub const STAY_WEIGHT: f64 = 1e-3;

/// Relation of a row: `expr = 0` or `expr ≤ 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Relation {
    /// `expr = 0`.
    Eq,
    /// `expr ≤ 0`.
    Le,
}

/// One scalar residual of a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// Residual expression.
    pub expr: Expr,
    /// Relation.
    pub relation: Relation,
    /// Characteristic magnitude of the residual (mm for lengths, 1 for angles).
    /// Tolerances are relative to it.
    pub scale: f64,
}

/// A rule: one or more rows with shared metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Stable rule ID (the document constraint ID or a synthetic ID).
    pub id: u64,
    /// Rows.
    pub rows: Vec<Row>,
    /// Strength.
    pub strength: Strength,
    /// Human readable label used in diagnostics (`"distance 50 mm"`).
    pub label: String,
    /// Source label (`"user"`, `"edit"`, `"plugin:acme.wall"`, ...).
    pub source: String,
    /// Related entity IDs for diagnostics.
    pub entities: Vec<u64>,
    /// When set, the rule cannot be expressed; components containing it report
    /// `unsupported` instead of being solved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported: Option<String>,
}

impl Rule {
    /// Create a rule with default metadata.
    #[must_use]
    pub fn new(id: u64, rows: Vec<Row>, strength: Strength) -> Self {
        Self {
            id,
            rows,
            strength,
            label: String::new(),
            source: String::new(),
            entities: Vec::new(),
            unsupported: None,
        }
    }

    /// Builder: label.
    #[must_use]
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = l.into();
        self
    }

    /// Builder: source.
    #[must_use]
    pub fn source(mut self, s: impl Into<String>) -> Self {
        self.source = s.into();
        self
    }

    /// Builder: related entities.
    #[must_use]
    pub fn entities(mut self, e: Vec<u64>) -> Self {
        self.entities = e;
        self
    }

    /// Variables referenced by any row.
    #[must_use]
    pub fn vars(&self) -> Vec<VarId> {
        let mut v: Vec<VarId> = self.rows.iter().flat_map(|r| r.expr.vars()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Whether this is a hard rule.
    #[must_use]
    pub fn is_hard(&self) -> bool {
        self.strength == Strength::Required
    }
}

/// A solver variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    /// Current (starting) value.
    pub value: f64,
    /// Fixed variables are constants for this solve (locks, `fix` rules on params).
    #[serde(default)]
    pub fixed: bool,
    /// Characteristic magnitude used to scale steps (mm for lengths, 1 for angles).
    pub scale: f64,
    /// Diagnostic label (`"e12.width"`).
    #[serde(default)]
    pub label: String,
}

impl Variable {
    /// Free variable with scale 1.
    #[must_use]
    pub fn new(value: f64) -> Self {
        Self { value, fixed: false, scale: 1.0, label: String::new() }
    }

    /// Builder: scale.
    #[must_use]
    pub fn scale(mut self, s: f64) -> Self {
        self.scale = s;
        self
    }

    /// Builder: fixed flag.
    #[must_use]
    pub fn fixed(mut self, f: bool) -> Self {
        self.fixed = f;
        self
    }

    /// Builder: label.
    #[must_use]
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = l.into();
        self
    }
}

/// A desired value for a variable (drag target, typed preference). Targets are never
/// hard; hard edits are expressed as `Required` rules.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// Variable.
    pub var: VarId,
    /// Desired value.
    pub value: f64,
    /// Priority (must not be `Required`; treated as `Strong` if it is).
    pub strength: Strength,
}

/// A complete solve request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    /// Variables indexed by [`VarId`].
    pub vars: Vec<Variable>,
    /// Rules in priority order of insertion (structural rules first, edit rules last).
    pub rules: Vec<Rule>,
    /// Targets.
    pub targets: Vec<Target>,
}

impl Problem {
    /// Add a variable and return its ID.
    pub fn add_var(&mut self, v: Variable) -> VarId {
        let id = VarId(u32::try_from(self.vars.len()).unwrap_or(u32::MAX));
        self.vars.push(v);
        id
    }

    /// Current values.
    #[must_use]
    pub fn values(&self) -> Vec<f64> {
        self.vars.iter().map(|v| v.value).collect()
    }

    /// Whether `v` is fixed.
    #[must_use]
    pub fn is_fixed(&self, v: VarId) -> bool {
        self.vars.get(v.index()).is_none_or(|x| x.fixed)
    }
}

/// Solver options.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SolveOptions {
    /// Relative residual tolerance for hard rows: `|r| ≤ tol · row.scale`.
    pub tolerance: f64,
    /// Maximum Gauss–Newton iterations per numeric component.
    pub max_iterations: u32,
    /// Maximum number of rules examined when minimizing a linear conflict set.
    pub conflict_search_limit: usize,
    /// Compute degrees of freedom and redundancy (rank analysis) for accepted
    /// solutions. Interactive previews may skip it; commits keep it on.
    pub analyze: bool,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self { tolerance: 1e-9, max_iterations: 100, conflict_search_limit: 64, analyze: true }
    }
}
