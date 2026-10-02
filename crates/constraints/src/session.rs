//! Incremental linear solving for interactive drags (DL-SOLVE-4).
//!
//! A drag re-solves the same problem for every pointer move; between moves only the
//! drag target and, for moved entities, a few constants change. For problems whose
//! rule components are all linear, a [`LinearSession`] keeps one kasuari (Cassowary)
//! solver per component alive instead of rebuilding it:
//!
//! * the rows of the *moving* rules (the drag target) are bound to Cassowary edit
//!   variables, so a pointer move is a `suggest_value` — a dual-simplex
//!   re-optimization from the previous basis;
//! * any other row, preference, target or stay whose coefficients changed is
//!   removed and re-added (Cassowary's incremental add/remove); unchanged ones stay;
//! * every result is verified against all hard rows independently of kasuari,
//!   exactly like a full solve.
//!
//! Whatever the session cannot decide on its own — a structural change, a hard row it
//! could not meet, a numeric component, a constant rule that is violated — yields
//! `None`, and the caller runs a full [`crate::SolveJob`], which also produces the
//! exact diagnostics (minimal conflict sets and so on).
//!
//! A required moving row is held by a *strong* edit variable. When the verified
//! result meets it, the result is an optimum of the full problem too: the strong
//! error term is zero there and the remaining objective (preferences and stays) is
//! the same, so session and full solve agree.

use kasuari::{Constraint, Expression, RelationalOperator, Solver, Strength as KStrength, Term, Variable as KVar};

use crate::{
    Backend, ComponentReport, Problem, Relation, Solution, SolveOptions, Status, Strength, VarId,
    analysis::Columns,
    graph::{Component, components},
    job::solve_trivial,
    linear::{STAY_BASE, kstrength, read_back, verify_linear},
};

/// A scaled linear row as handed to kasuari: `Σ cᵢ·yᵢ + k (= | ≤) 0` at a strength.
#[derive(Debug, Clone, PartialEq)]
struct Spec {
    terms: Vec<(usize, f64)>,
    constant: f64,
    relation: Relation,
    strength: f64,
}

/// One kasuari constraint and the spec it was built from.
#[derive(Debug)]
struct Slot {
    spec: Spec,
    constraint: Constraint,
}

/// A moving row: `Σ cᵢ·yᵢ − e = 0` (required) with `e` an edit variable.
#[derive(Debug)]
struct Edit {
    aux: KVar,
    terms: Vec<(usize, f64)>,
    link: Constraint,
    value: f64,
}

/// The parts of an edit spec that can change between moves.
#[derive(Debug, Clone, PartialEq)]
struct EditSpec {
    terms: Vec<(usize, f64)>,
    value: f64,
    strength: f64,
}

struct LinearPart {
    comp: Component,
    cols: Columns,
    kvars: Vec<KVar>,
    solver: Solver,
    slots: Vec<Slot>,
    edits: Vec<Edit>,
}

enum Part {
    Trivial(Component),
    Linear(Box<LinearPart>),
}

impl core::fmt::Debug for Part {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Trivial(c) => f.debug_tuple("Trivial").field(c).finish(),
            Self::Linear(p) => f
                .debug_struct("Linear")
                .field("component", &p.comp)
                .field("constraints", &p.slots.len())
                .field("edits", &p.edits.len())
                .finish_non_exhaustive(),
        }
    }
}

/// Everything that must be equal for two problems to share a session.
#[derive(Debug, PartialEq)]
struct Signature {
    vars: Vec<(bool, u64, u64)>,
    rules: Vec<(Strength, Vec<VarId>, Vec<Relation>, bool)>,
    targets: Vec<(VarId, Strength)>,
}

fn signature(p: &Problem) -> Signature {
    Signature {
        vars: p.vars.iter().map(|v| (v.fixed, v.scale.to_bits(), crate::stay_factor(v.stay).to_bits())).collect(),
        rules: p
            .rules
            .iter()
            .map(|r| {
                let free: Vec<VarId> = r.vars().into_iter().filter(|v| !p.is_fixed(*v)).collect();
                (r.strength, free, r.rows.iter().map(|row| row.relation).collect(), r.unsupported.is_some())
            })
            .collect(),
        targets: p.targets.iter().map(|t| (t.var, t.strength)).collect(),
    }
}

/// Scaled linear form of one row in column coordinates.
fn row_form(p: &Problem, rule: usize, row: usize, cols: &Columns, x: &[f64]) -> Option<(Vec<(usize, f64)>, f64)> {
    let r = p.rules.get(rule)?.rows.get(row)?;
    let lf = r.expr.linear_form(x, &|v| p.is_fixed(v))?;
    let sigma = if r.scale.is_finite() && r.scale > 0.0 { r.scale } else { 1.0 };
    let mut terms = Vec::with_capacity(lf.terms.len());
    for (v, c) in lf.terms {
        let col = cols.col.get(v.index()).copied().flatten()?;
        let s = cols.scales.get(col).copied().unwrap_or(1.0);
        terms.push((col, c * s / sigma));
    }
    Some((terms, lf.constant / sigma))
}

fn constraint(spec: &Spec, kvars: &[KVar]) -> Option<Constraint> {
    let mut terms = Vec::with_capacity(spec.terms.len());
    for &(col, c) in &spec.terms {
        terms.push(Term::new(*kvars.get(col)?, c));
    }
    let op = match spec.relation {
        Relation::Eq => RelationalOperator::Equal,
        Relation::Le => RelationalOperator::LessOrEqual,
    };
    Some(Constraint::new(Expression::new(terms, spec.constant), op, KStrength::new(spec.strength)))
}

fn link(terms: &[(usize, f64)], aux: KVar, kvars: &[KVar]) -> Option<Constraint> {
    let mut t = Vec::with_capacity(terms.len() + 1);
    for &(col, c) in terms {
        t.push(Term::new(*kvars.get(col)?, c));
    }
    t.push(Term::new(aux, -1.0));
    Some(Constraint::new(Expression::new(t, 0.0), RelationalOperator::Equal, KStrength::REQUIRED))
}

impl LinearPart {
    /// Wanted constraints, in the same order as a full linear solve adds them: hard
    /// rows, preferences, targets, stays — and the moving rows separately.
    fn specs(&self, p: &Problem, moving: &[usize], x: &[f64]) -> Option<(Vec<Spec>, Vec<EditSpec>)> {
        let mut slots = Vec::new();
        let mut edits = Vec::new();
        for hard_pass in [true, false] {
            for &ri in &self.comp.rules {
                let rule = p.rules.get(ri)?;
                if rule.is_hard() != hard_pass {
                    continue;
                }
                let is_moving = moving.binary_search(&ri).is_ok();
                for (k, row) in rule.rows.iter().enumerate() {
                    let (terms, constant) = row_form(p, ri, k, &self.cols, x)?;
                    if is_moving {
                        if row.relation != Relation::Eq {
                            return None;
                        }
                        let strength = if rule.is_hard() { Strength::Strong } else { rule.strength };
                        edits.push(EditSpec { terms, value: -constant, strength: kstrength(strength).value() });
                    } else {
                        let strength = if hard_pass { KStrength::REQUIRED } else { kstrength(rule.strength) };
                        slots.push(Spec { terms, constant, relation: row.relation, strength: strength.value() });
                    }
                }
            }
        }
        for t in &p.targets {
            let Some(col) = self.cols.col.get(t.var.index()).copied().flatten() else { continue };
            let s = self.cols.scales.get(col).copied().unwrap_or(1.0);
            let strength = if t.strength == Strength::Required { Strength::Strong } else { t.strength };
            slots.push(Spec {
                terms: vec![(col, 1.0)],
                constant: -t.value / s,
                relation: Relation::Eq,
                strength: kstrength(strength).value(),
            });
        }
        let n = self.cols.n();
        for (col, var) in self.cols.vars.iter().enumerate() {
            let s = self.cols.scales.get(col).copied().unwrap_or(1.0);
            let reference = x.get(var.index()).copied().unwrap_or(0.0);
            let mult = crate::stay_factor(p.vars.get(var.index()).map_or(1.0, |v| v.stay));
            let w = STAY_BASE * mult * (1.0 + 0.5 * col as f64 / n.max(1) as f64);
            slots.push(Spec { terms: vec![(col, 1.0)], constant: -reference / s, relation: Relation::Eq, strength: w });
        }
        Some((slots, edits))
    }

    fn new(p: &Problem, comp: Component, moving: &[usize], x: &[f64]) -> Option<Self> {
        let cols = Columns::new(p, &comp.vars);
        let kvars: Vec<KVar> = (0..cols.n()).map(|_| KVar::new()).collect();
        let mut part = Self { comp, cols, kvars, solver: Solver::new(), slots: Vec::new(), edits: Vec::new() };
        let (slots, edits) = part.specs(p, moving, x)?;
        for spec in slots {
            let c = constraint(&spec, &part.kvars)?;
            let required = spec.strength >= KStrength::REQUIRED.value();
            if part.solver.add_constraint(c.clone()).is_err() && required {
                return None;
            }
            part.slots.push(Slot { spec, constraint: c });
        }
        for e in edits {
            let aux = KVar::new();
            let l = link(&e.terms, aux, &part.kvars)?;
            part.solver.add_constraint(l.clone()).ok()?;
            part.solver.add_edit_variable(aux, KStrength::new(e.strength)).ok()?;
            part.solver.suggest_value(aux, e.value).ok()?;
            part.edits.push(Edit { aux, terms: e.terms, link: l, value: e.value });
        }
        Some(part)
    }

    /// Bring the solver to the wanted constraints. `None` leaves the solver
    /// inconsistent with the problem (the session is then unusable).
    fn update(&mut self, p: &Problem, moving: &[usize], x: &[f64]) -> Option<()> {
        let (slots, edits) = self.specs(p, moving, x)?;
        if slots.len() != self.slots.len() || edits.len() != self.edits.len() {
            return None;
        }
        for (cur, want) in self.slots.iter_mut().zip(slots) {
            if cur.spec == want {
                continue;
            }
            let c = constraint(&want, &self.kvars)?;
            self.solver.remove_constraint(&cur.constraint).ok()?;
            self.solver.add_constraint(c.clone()).ok()?;
            *cur = Slot { spec: want, constraint: c };
        }
        for (cur, want) in self.edits.iter_mut().zip(edits) {
            if cur.terms != want.terms {
                let l = link(&want.terms, cur.aux, &self.kvars)?;
                self.solver.remove_constraint(&cur.link).ok()?;
                self.solver.add_constraint(l.clone()).ok()?;
                cur.link = l;
                cur.terms = want.terms;
            }
            if cur.value.to_bits() != want.value.to_bits() {
                self.solver.suggest_value(cur.aux, want.value).ok()?;
                cur.value = want.value;
            }
        }
        Some(())
    }
}

/// An incremental solve session for repeated solves of one linear problem whose
/// constants change (see the module documentation).
#[derive(Debug)]
pub struct LinearSession {
    signature: Signature,
    moving: Vec<usize>,
    parts: Vec<Part>,
    opts: SolveOptions,
    broken: bool,
}

impl LinearSession {
    /// Build a session for `problem`. `moving` are indices into `problem.rules` of
    /// the rules whose constants change from solve to solve (drag targets); their
    /// rows must be equalities. Returns `None` when the problem has a nonlinear
    /// component, an unsupported rule, a non-finite value, or required rules that
    /// already contradict each other — a full solve handles those.
    #[must_use]
    pub fn new(problem: &Problem, moving: &[usize], opts: SolveOptions) -> Option<Self> {
        if problem.vars.iter().any(|v| !v.value.is_finite()) || problem.rules.iter().any(|r| r.unsupported.is_some()) {
            return None;
        }
        let mut moving = moving.to_vec();
        moving.sort_unstable();
        moving.dedup();
        let x = problem.values();
        let mut parts = Vec::new();
        for comp in components(problem) {
            match comp.backend {
                Backend::Numeric => return None,
                Backend::Trivial => parts.push(Part::Trivial(comp)),
                Backend::Linear => parts.push(Part::Linear(Box::new(LinearPart::new(problem, comp, &moving, &x)?))),
            }
        }
        Some(Self { signature: signature(problem), moving, parts, opts, broken: false })
    }

    /// Whether the session can no longer be used (a constraint could not be
    /// re-added); build a new one.
    #[must_use]
    pub fn is_broken(&self) -> bool {
        self.broken
    }

    /// Solve `problem`, which must have the structure of the problem the session
    /// was built from (same variables, fixed flags, rules and targets; constants,
    /// coefficients and reference values may differ). Returns an accepted,
    /// independently verified solution, or `None` when a full solve is needed.
    pub fn resolve(&mut self, problem: &Problem) -> Option<Solution> {
        if self.broken || signature(problem) != self.signature {
            return None;
        }
        let x_in = problem.values();
        if x_in.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut x = x_in.clone();
        let mut reports = Vec::with_capacity(self.parts.len());
        let mut diagnostics = Vec::new();
        let mut iterations = 0;
        for part in &mut self.parts {
            match part {
                Part::Trivial(comp) => {
                    let (status, d, max_hard) = solve_trivial(problem, comp, &mut x, &self.opts);
                    if !status.is_acceptable() {
                        return None;
                    }
                    diagnostics.extend(d);
                    reports.push(report(problem, comp, Backend::Trivial, status, 0, max_hard));
                }
                Part::Linear(lp) => {
                    if lp.update(problem, &self.moving, &x_in).is_none() {
                        self.broken = true;
                        return None;
                    }
                    read_back(&lp.solver, &lp.cols, &lp.kvars, &mut x);
                    let out = verify_linear(problem, &lp.comp, &lp.cols, &mut x, &x_in, &self.opts);
                    if !out.status.is_acceptable() {
                        return None;
                    }
                    iterations += 1;
                    diagnostics.extend(out.diagnostics);
                    reports.push(report(problem, &lp.comp, Backend::Linear, out.status, 1, out.max_hard));
                }
            }
        }
        let status = reports.iter().fold(Status::Solved, |acc, r| acc.combine(r.status));
        Some(Solution { values: x, status, components: reports, diagnostics, iterations })
    }
}

fn report(
    p: &Problem,
    comp: &Component,
    backend: Backend,
    status: Status,
    iterations: u32,
    max_hard: f64,
) -> ComponentReport {
    ComponentReport {
        vars: comp.vars.clone(),
        rules: comp.rules.iter().filter_map(|ri| p.rules.get(*ri).map(|r| r.id)).collect(),
        backend,
        status,
        iterations,
        max_hard_residual: max_hard,
    }
}
