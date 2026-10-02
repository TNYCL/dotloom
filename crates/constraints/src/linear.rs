//! Linear backend: kasuari (Cassowary) adapter.
//!
//! Variables and rows are scaled to O(1) before they reach kasuari (whose internal
//! zero test is an absolute 1e-8). Every free variable gets a "stay" preference
//! towards its previous value; stay weights differ slightly by stable column order so
//! that optimal ties are broken deterministically regardless of kasuari's internal
//! hash-map order.

use kasuari::{Constraint, Expression, RelationalOperator, Solver, Strength as KStrength, Term, Variable as KVar};

use crate::{
    Certainty, Diagnostic, DiagnosticKind, Problem, Relation, SolveOptions, Status, Strength,
    analysis::{Columns, dense, eval_rows, max_violation, rank_analysis},
    graph::Component,
};

/// Base kasuari strength of stay preferences (weak = 1.0).
pub(crate) const STAY_BASE: f64 = 0.01;

pub(crate) struct LinearOutcome {
    pub status: Status,
    pub diagnostics: Vec<Diagnostic>,
    pub max_hard: f64,
}

pub(crate) fn kstrength(s: Strength) -> KStrength {
    match s {
        Strength::Required => KStrength::REQUIRED,
        Strength::Strong => KStrength::STRONG,
        Strength::Medium => KStrength::MEDIUM,
        Strength::Weak => KStrength::WEAK,
    }
}

/// kasuari constraints for one rule, scaled.
pub(crate) fn rule_constraints(
    p: &Problem,
    rule_idx: usize,
    cols: &Columns,
    kvars: &[KVar],
    x: &[f64],
    strength: KStrength,
) -> Option<Vec<Constraint>> {
    let rule = p.rules.get(rule_idx)?;
    let fixed = |v| p.is_fixed(v);
    let mut out = Vec::with_capacity(rule.rows.len());
    for row in &rule.rows {
        let lf = row.expr.linear_form(x, &fixed)?;
        let sigma = if row.scale.is_finite() && row.scale > 0.0 { row.scale } else { 1.0 };
        let mut terms = Vec::with_capacity(lf.terms.len());
        for (v, c) in lf.terms {
            let col = cols.col.get(v.index()).copied().flatten()?;
            let s = cols.scales.get(col).copied().unwrap_or(1.0);
            terms.push(Term::new(*kvars.get(col)?, c * s / sigma));
        }
        let expr = Expression::new(terms, lf.constant / sigma);
        let op = match row.relation {
            Relation::Eq => RelationalOperator::Equal,
            Relation::Le => RelationalOperator::LessOrEqual,
        };
        out.push(Constraint::new(expr, op, strength));
    }
    Some(out)
}

/// Whether the given hard rules are jointly infeasible.
fn infeasible(p: &Problem, rules: &[usize], cols: &Columns, x: &[f64]) -> bool {
    let kvars: Vec<KVar> = (0..cols.n()).map(|_| KVar::new()).collect();
    let mut solver = Solver::new();
    for &ri in rules {
        let Some(cs) = rule_constraints(p, ri, cols, &kvars, x, KStrength::REQUIRED) else {
            continue;
        };
        for c in cs {
            if solver.add_constraint(c).is_err() {
                return true;
            }
        }
    }
    false
}

pub(crate) fn conflict_diagnostic(
    p: &Problem,
    rules: &[usize],
    certainty: Certainty,
    residual: Option<f64>,
    message: String,
) -> Diagnostic {
    let mut ids = Vec::new();
    let mut labels = Vec::new();
    let mut sources = Vec::new();
    let mut entities = Vec::new();
    for &ri in rules {
        if let Some(r) = p.rules.get(ri) {
            ids.push(r.id);
            labels.push(r.label.clone());
            sources.push(r.source.clone());
            entities.extend(r.entities.iter().copied());
        }
    }
    entities.sort_unstable();
    entities.dedup();
    Diagnostic {
        kind: if certainty == Certainty::Certain {
            DiagnosticKind::Conflict
        } else {
            DiagnosticKind::SuspectedConflict
        },
        certainty,
        rules: ids,
        labels,
        sources,
        entities,
        residual,
        message,
    }
}

/// Solve a linear component in place. On conflict, `x` is left unchanged.
pub(crate) fn solve_linear(
    p: &Problem,
    comp: &Component,
    x: &mut [f64],
    stay_ref: &[f64],
    opts: &SolveOptions,
) -> LinearOutcome {
    let cols = Columns::new(p, &comp.vars);
    let n = cols.n();
    let kvars: Vec<KVar> = (0..n).map(|_| KVar::new()).collect();
    let mut solver = Solver::new();
    let x_in = x.to_vec();

    // 1. Hard rules in problem order (structural rules first, edits last).
    let mut added_hard: Vec<usize> = Vec::new();
    for &ri in &comp.rules {
        let Some(rule) = p.rules.get(ri) else { continue };
        if !rule.is_hard() {
            continue;
        }
        let Some(cs) = rule_constraints(p, ri, &cols, &kvars, &x_in, KStrength::REQUIRED) else {
            continue;
        };
        let mut failed = false;
        for c in cs {
            if solver.add_constraint(c).is_err() {
                failed = true;
                break;
            }
        }
        added_hard.push(ri);
        if failed {
            // Deletion filter for a minimal infeasible subset (bounded).
            let mut set = added_hard.clone();
            if set.len() <= opts.conflict_search_limit {
                let mut i = 0;
                while i < set.len() {
                    if set.get(i) == Some(&ri) {
                        i += 1;
                        continue;
                    }
                    let mut trial = set.clone();
                    trial.remove(i);
                    if infeasible(p, &trial, &cols, &x_in) {
                        set = trial;
                    } else {
                        i += 1;
                    }
                }
            }
            let label = rule.label.clone();
            let d = conflict_diagnostic(
                p,
                &set,
                Certainty::Certain,
                None,
                format!(
                    "required rule `{label}` cannot be satisfied together with {} other rule(s)",
                    set.len().saturating_sub(1)
                ),
            );
            return LinearOutcome { status: Status::Conflicting, diagnostics: vec![d], max_hard: f64::INFINITY };
        }
    }

    // 2. Soft rules.
    for &ri in &comp.rules {
        let Some(rule) = p.rules.get(ri) else { continue };
        if rule.is_hard() {
            continue;
        }
        if let Some(cs) = rule_constraints(p, ri, &cols, &kvars, &x_in, kstrength(rule.strength)) {
            for c in cs {
                let _ = solver.add_constraint(c);
            }
        }
    }

    // 3. Targets and deterministic stays.
    for t in &p.targets {
        let Some(col) = cols.col.get(t.var.index()).copied().flatten() else { continue };
        let (Some(kv), Some(s)) = (kvars.get(col), cols.scales.get(col)) else { continue };
        let strength = if t.strength == Strength::Required { Strength::Strong } else { t.strength };
        let expr = Expression::new(vec![Term::new(*kv, 1.0)], -t.value / s);
        let _ = solver.add_constraint(Constraint::new(expr, RelationalOperator::Equal, kstrength(strength)));
    }
    for (col, var) in cols.vars.iter().enumerate() {
        let (Some(kv), Some(s)) = (kvars.get(col), cols.scales.get(col)) else { continue };
        let reference = stay_ref.get(var.index()).copied().unwrap_or(0.0);
        let mult = crate::problem::stay_factor(p.vars.get(var.index()).map_or(1.0, |v| v.stay));
        let w = STAY_BASE * mult * (1.0 + 0.5 * col as f64 / n.max(1) as f64);
        let expr = Expression::new(vec![Term::new(*kv, 1.0)], -reference / s);
        let _ = solver.add_constraint(Constraint::new(expr, RelationalOperator::Equal, KStrength::new(w)));
    }

    // 4. Read back and verify independently.
    read_back(&solver, &cols, &kvars, x);
    verify_linear(p, comp, &cols, x, &x_in, opts)
}

/// Copy kasuari's values into `x` (unscaled).
pub(crate) fn read_back(solver: &Solver, cols: &Columns, kvars: &[KVar], x: &mut [f64]) {
    for (col, var) in cols.vars.iter().enumerate() {
        let (Some(kv), Some(s)) = (kvars.get(col), cols.scales.get(col)) else { continue };
        if let Some(slot) = x.get_mut(var.index()) {
            *slot = solver.get_value(*kv) * s;
        }
    }
}

/// Verify a linear component's values against every hard row (independently of
/// kasuari) and, with `opts.analyze`, count degrees of freedom and redundant rules.
/// On failure `x` is restored from `x_in`.
pub(crate) fn verify_linear(
    p: &Problem,
    comp: &Component,
    cols: &Columns,
    x: &mut [f64],
    x_in: &[f64],
    opts: &SolveOptions,
) -> LinearOutcome {
    let n = cols.n();
    let rows = eval_rows(p, &comp.rules, cols, x);
    let hard: Vec<_> = rows.iter().filter(|r| p.rules.get(r.rule).is_some_and(crate::Rule::is_hard)).collect();
    let hard_owned: Vec<_> = hard.iter().map(|r| (*r).clone()).collect();
    let max_hard = max_violation(&hard_owned);
    if max_hard > opts.tolerance {
        x.copy_from_slice(x_in);
        return LinearOutcome {
            status: Status::NotConverged { suspected_conflict: false },
            diagnostics: vec![Diagnostic {
                kind: DiagnosticKind::NotConverged,
                certainty: Certainty::Suspected,
                rules: comp.rules.iter().filter_map(|ri| p.rules.get(*ri).map(|r| r.id)).collect(),
                labels: Vec::new(),
                sources: Vec::new(),
                entities: Vec::new(),
                residual: Some(max_hard),
                message: "linear solution failed independent verification".into(),
            }],
            max_hard,
        };
    }
    if !opts.analyze {
        return LinearOutcome { status: Status::Solved, diagnostics: Vec::new(), max_hard };
    }
    // Degrees of freedom count equality rows only: an inequality at its bound limits
    // motion in one direction but does not remove a degree of freedom.
    let binding: Vec<_> = hard.iter().copied().filter(|r| r.relation == Relation::Eq).collect();
    let a = dense(&binding, n);
    let r = nalgebra::DVector::from_iterator(binding.len(), binding.iter().map(|r| r.scaled()));
    let info = rank_analysis(&a, &r, opts.tolerance.max(1e-9) * 10.0);
    let dof = n.saturating_sub(info.rank);
    let mut diagnostics = Vec::new();
    if !info.redundant_rows.is_empty() {
        let mut rules: Vec<usize> =
            info.redundant_rows.iter().filter_map(|i| binding.get(*i).map(|r| r.rule)).collect();
        rules.sort_unstable();
        rules.dedup();
        let mut d = conflict_diagnostic(p, &rules, Certainty::Certain, None, "redundant rules (consistent)".into());
        d.kind = DiagnosticKind::Redundant;
        diagnostics.push(d);
    }
    LinearOutcome {
        status: if dof == 0 { Status::Solved } else { Status::Underconstrained { dof } },
        diagnostics,
        max_hard,
    }
}
