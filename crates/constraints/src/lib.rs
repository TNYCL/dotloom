//! # dotloom-constraints
//!
//! Constraint solving for Dotloom (ADR-0004).
//!
//! * Rules are rows of scalar [`Expr`] residuals with exact derivatives.
//! * The problem is split into connected components. Purely linear components go to
//!   **kasuari** (a maintained Cassowary implementation); nonlinear/mixed components
//!   go to a damped Gauss–Newton backend that solves hard rules as exact equality
//!   constraints and preferences as least squares inside the hard-feasible set.
//! * Results distinguish `solved`, `underconstrained`, `conflicting` (with evidence),
//!   `notConverged` (with or without a suspected conflict), `cancelled` and
//!   `unsupported`, and carry structured [`Diagnostic`]s.
//! * [`SolveJob`] runs in budgeted steps so hosts can cancel between steps.
//!
//! This crate has no document, DOM or GPU dependency.
//!
//! ```
//! use dotloom_constraints::{Problem, Rule, Strength, Variable, Expr, rules, solve, SolveOptions, Status};
//!
//! // Shelf: total = w1 + w2 + w3, w1 locked at 600 mm, w2 = w3 ≥ 400 mm.
//! let mut p = Problem::default();
//! let total = p.add_var(Variable::new(1800.0).scale(1000.0));
//! let w1 = p.add_var(Variable::new(600.0).scale(1000.0));
//! let w2 = p.add_var(Variable::new(600.0).scale(1000.0));
//! let w3 = p.add_var(Variable::new(600.0).scale(1000.0));
//! let v = |id| Expr::Var(id);
//! p.rules.push(Rule::new(1, rules::linear(&[(1.0, v(w1)), (1.0, v(w2)), (1.0, v(w3)), (-1.0, v(total))], rules::Cmp::Eq, 0.0, 1000.0), Strength::Required));
//! p.rules.push(Rule::new(2, rules::equal(v(w2), v(w3), 1000.0), Strength::Required));
//! p.rules.push(Rule::new(3, rules::at_least(v(w2), 400.0, 1000.0), Strength::Required));
//! p.rules.push(Rule::new(4, rules::at_least(v(w3), 400.0, 1000.0), Strength::Required));
//! p.rules.push(Rule::new(5, rules::fix(v(w1), 600.0, 1000.0), Strength::Required));
//! p.rules.push(Rule::new(6, rules::fix(v(total), 1600.0, 1000.0), Strength::Required));
//! let s = solve(&p, &SolveOptions::default());
//! assert!(s.accepted());
//! assert!((s.values[w2.index()] - 500.0).abs() < 1e-6);
//! ```

mod analysis;
mod expr;
mod graph;
mod job;
mod linear;
mod numeric;
mod problem;
pub mod rules;
mod solution;

pub use expr::{Dual, Expr, LinearForm, PointExpr, VarId};
pub use graph::{Component, components};
pub use job::{Progress, SolveJob, solve};
pub use problem::{Problem, Relation, Row, Rule, STAY_WEIGHT, SolveOptions, Strength, Target, Variable};
pub use solution::{Backend, Certainty, ComponentReport, Diagnostic, DiagnosticKind, Solution, Status};
