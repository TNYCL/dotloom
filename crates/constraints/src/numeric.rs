//! Numeric backend for nonlinear/mixed components.
//!
//! Each optimization iteration solves the equality-constrained quadratic step
//! (scaled variables `y = x / scale`):
//!
//! ```text
//! minimize   ½ Δᵀ (H + Σ λᵢ ∇²cᵢ) Δ − gᵀΔ   (preferences, targets, stays + curvature)
//! subject to A·Δ = −r_hard                  (hard equalities + active inequalities)
//! ```
//!
//! Hard rules are exact constraints of the step, never penalty terms, so preferences
//! only choose among hard-feasible points. Preference weights are bounded (strong 1,
//! medium 0.1, weak 0.01, stay 0.001). The curvature of the hard rows (`∇²cᵢ`, by
//! finite differences of their exact gradients, weighted with the multipliers of the
//! previous step) makes the step a sequential-quadratic-programming step, which
//! converges superlinearly on curved constraint manifolds where a Gauss–Newton
//! step only creeps. After each trial step the point is projected back onto the
//! hard manifold with minimum-norm Newton corrections. Inequalities use a primal
//! active set driven by the multipliers.
//!
//! Two phases precede the optimization:
//!
//! - **Presolve** eliminates hard equalities linear in a single unknown.
//! - **Approach.** Such single-unknown equalities that are *violated* at the start
//!   (typed edits, hard drag targets) are first followed as strong preferences from
//!   the current, feasible configuration — the optimization phase moves along the
//!   hard manifold towards them — and only then enforced exactly. Jumping straight
//!   to the new value would start the restoration far from the manifold, where
//!   Newton steps on chains and linkages overshoot and crawl; following the
//!   manifold also keeps the configuration from flipping to a distant branch.
//!
//! Restoration (reaching the hard manifold from an infeasible point) uses damped
//! minimum-norm Gauss–Newton steps with an Armijo test on `Σ violation²` (a smooth
//! merit: the maximum violation would reject good steps that trade a little error
//! between rows).
//!
//! Linear algebra is sparse (see [`crate::sparse`]): normal equations for
//! minimum-norm corrections and diagonal-Hessian steps, a quasi-definite KKT
//! factorization when the Hessian has off-diagonal terms.

use std::collections::{BTreeMap, BTreeSet};

use nalgebra::DVector;

use crate::{
    Problem, Relation, SolveOptions, Status, Strength, VarId,
    analysis::{Columns, EvalRow, dense, eval_rows, eval_values, max_violation, rank_analysis},
    graph::Component,
    problem::STAY_WEIGHT,
    sparse::{KktFactor, NormalFactor},
};

/// Newton projection steps per trial point.
const PROJECTION_STEPS: usize = 8;
/// Relative finite-difference step for row Hessians (scaled variables).
const FD_STEP: f64 = 1e-7;
/// Approach phase ends when the deferred rows are within this scaled residual…
const APPROACH_TOL: f64 = 1e-6;
/// …or when a step lowers the objective by less than this fraction.
const APPROACH_GAIN: f64 = 1e-3;

/// Quadratic model of the objective: gradient term `g`, diagonal and off-diagonal
/// (lower-triangle) Hessian entries.
type Model = (DVector<f64>, Vec<f64>, Vec<(usize, usize, f64)>);

/// One least-squares preference row.
struct SoftRow {
    grad: Vec<(usize, f64)>,
    r: f64,
    w: f64,
}

/// Approach phase state: violated single-unknown hard equalities followed as
/// strong preferences before they are enforced.
struct Approach {
    /// Rule indices (into `Problem::rules`).
    deferred: Vec<usize>,
}

pub(crate) struct NumericSolver {
    stay_w: Vec<f64>,
    /// Rules solved in the current phase.
    rules: Vec<usize>,
    /// All rules of the component.
    all_rules: Vec<usize>,
    comp_vars: Vec<VarId>,
    cols: Columns,
    stay_ref: Vec<f64>,
    presolved: bool,
    pub presolve_conflict: Option<Vec<usize>>,
    trust: f64,
    pub iterations: u32,
    pub done: Option<Status>,
    pub max_hard: f64,
    stalled: bool,
    active: BTreeSet<(usize, usize)>,
    /// Multipliers of the last optimization step, by `(rule, row)`.
    lambda: BTreeMap<(usize, usize), f64>,
    approach: Option<Approach>,
    /// The approach phase ran (or was skipped) already.
    approach_done: bool,
    /// Redundant rules found by the final rank analysis.
    redundant: Vec<usize>,
}

fn is_hard(p: &Problem, r: &EvalRow) -> bool {
    p.rules.get(r.rule).is_some_and(crate::Rule::is_hard)
}

fn dot_sparse(grad: &[(usize, f64)], v: &DVector<f64>) -> f64 {
    grad.iter().map(|&(c, a)| a * v.get(c).copied().unwrap_or(0.0)).sum()
}

/// `diag(d)·Aᵀ·y` for sparse rows (`d = 1` when `None`).
fn transpose_mul(rows: &[&EvalRow], y: &[f64], n: usize, d: Option<&[f64]>) -> DVector<f64> {
    let mut out = DVector::zeros(n);
    for (r, yi) in rows.iter().zip(y) {
        for &(c, a) in &r.grad {
            if let Some(o) = out.get_mut(c) {
                *o += a * yi;
            }
        }
    }
    if let Some(d) = d {
        for (o, di) in out.iter_mut().zip(d) {
            *o *= di;
        }
    }
    out
}

/// Minimum-norm correction `δ = Aᵀ (A Aᵀ)⁻¹ b` for sparse rows.
fn min_norm(rows: &[&EvalRow], b: &DVector<f64>, n: usize) -> Option<DVector<f64>> {
    if rows.is_empty() {
        return Some(DVector::zeros(n));
    }
    let f = NormalFactor::new(rows, &vec![1.0; n], n, 1e-13)?;
    let y = f.solve(b.as_slice());
    Some(transpose_mul(rows, &y, n, None))
}

/// Sum of squared violations of the hard rows (restoration merit).
fn merit(p: &Problem, rows: &[EvalRow]) -> f64 {
    rows.iter()
        .filter(|r| is_hard(p, r))
        .map(|r| if r.value.is_finite() { r.violation() * r.violation() } else { f64::INFINITY })
        .sum()
}

impl NumericSolver {
    pub fn new(p: &Problem, comp: &Component, stay_ref: &[f64]) -> Self {
        Self {
            stay_w: Vec::new(),
            rules: comp.rules.clone(),
            all_rules: comp.rules.clone(),
            comp_vars: comp.vars.clone(),
            cols: Columns::new(p, &comp.vars),
            stay_ref: stay_ref.to_vec(),
            presolved: false,
            presolve_conflict: None,
            trust: 1.0,
            iterations: 0,
            done: None,
            max_hard: f64::INFINITY,
            stalled: false,
            active: BTreeSet::new(),
            lambda: BTreeMap::new(),
            approach: None,
            approach_done: false,
            redundant: Vec::new(),
        }
    }

    fn refresh_stay(&mut self, p: &Problem) {
        self.stay_w = self
            .cols
            .vars
            .iter()
            .map(|v| STAY_WEIGHT * crate::problem::stay_factor(p.vars.get(v.index()).map_or(1.0, |x| x.stay)))
            .collect();
    }

    /// Eliminate unknowns fixed by hard equalities linear in a single unknown.
    ///
    /// On the first presolve, such equalities that are violated by the starting
    /// point are deferred to the approach phase instead (when the component has
    /// anything else to solve), unless enforcing them right away already shows a
    /// contradiction.
    fn presolve(&mut self, p: &Problem, x: &mut [f64], opts: &SolveOptions) {
        self.presolved = true;
        let first = !self.approach_done;
        self.approach_done = true;
        let mut trial = x.to_vec();
        let fixed = match self.eliminate(p, &self.rules, &mut trial, opts) {
            Ok(fixed) => fixed,
            Err(evidence) => {
                self.presolve_conflict = Some(evidence);
                return;
            }
        };
        let deferred = if first { self.violated_fixes(p, x, opts) } else { Vec::new() };
        let fixed = if !deferred.is_empty() && deferred.len() < self.rules.len() {
            self.rules.retain(|r| !deferred.contains(r));
            self.approach = Some(Approach { deferred });
            match self.eliminate(p, &self.rules, x, opts) {
                Ok(fixed) => fixed,
                Err(evidence) => {
                    self.presolve_conflict = Some(evidence);
                    return;
                }
            }
        } else {
            x.copy_from_slice(&trial);
            fixed
        };
        let remaining: Vec<VarId> = self.comp_vars.iter().copied().filter(|v| !fixed.contains(v)).collect();
        self.cols = Columns::new(p, &remaining);
        self.refresh_stay(p);
        // Columns changed: start the phase with a Gauss–Newton step.
        self.lambda.clear();
    }

    /// Hard rules made only of single-unknown linear equalities that `x` violates.
    fn violated_fixes(&self, p: &Problem, x: &[f64], opts: &SolveOptions) -> Vec<usize> {
        let is_fixed = |v: VarId| p.is_fixed(v);
        self.rules
            .iter()
            .copied()
            .filter(|&ri| {
                let Some(rule) = p.rules.get(ri) else { return false };
                if !rule.is_hard() || rule.rows.is_empty() {
                    return false;
                }
                let mut violated = false;
                for row in &rule.rows {
                    if row.relation != Relation::Eq {
                        return false;
                    }
                    match row.expr.linear_form(x, &is_fixed) {
                        Some(lf) if lf.terms.len() == 1 => {
                            let scale = if row.scale > 0.0 { row.scale } else { 1.0 };
                            violated |= (row.expr.eval(x) / scale).abs() > opts.tolerance;
                        }
                        _ => return false,
                    }
                }
                violated
            })
            .collect()
    }

    /// Fix every unknown determined by a single-unknown hard equality (repeatedly),
    /// writing the values into `x`. Returns the fixed unknowns, or the rules of a
    /// contradiction (a row without unknowns left that is violated).
    fn eliminate(
        &self,
        p: &Problem,
        rules: &[usize],
        x: &mut [f64],
        opts: &SolveOptions,
    ) -> Result<BTreeSet<VarId>, Vec<usize>> {
        let mut fixed: BTreeSet<VarId> = BTreeSet::new();
        let mut determined_by: Vec<(VarId, usize)> = Vec::new();
        loop {
            let mut changed = false;
            for &ri in rules {
                let Some(rule) = p.rules.get(ri) else { continue };
                if !rule.is_hard() {
                    continue;
                }
                for row in &rule.rows {
                    if row.relation != Relation::Eq {
                        continue;
                    }
                    let is_fixed = |v: VarId| p.is_fixed(v) || fixed.contains(&v);
                    let Some(lf) = row.expr.linear_form(x, &is_fixed) else { continue };
                    if let [(v, c)] = lf.terms.as_slice() {
                        if c.abs() < 1e-300 {
                            continue;
                        }
                        let value = -lf.constant / c;
                        if !value.is_finite() {
                            continue;
                        }
                        if let Some(slot) = x.get_mut(v.index()) {
                            *slot = value;
                        }
                        fixed.insert(*v);
                        determined_by.push((*v, ri));
                        changed = true;
                    } else if lf.terms.is_empty() {
                        let scale = if row.scale > 0.0 { row.scale } else { 1.0 };
                        if (lf.constant / scale).abs() > opts.tolerance {
                            let vars = row.expr.vars();
                            let mut evidence: Vec<usize> =
                                determined_by.iter().filter(|(dv, _)| vars.contains(dv)).map(|(_, r)| *r).collect();
                            evidence.push(ri);
                            evidence.sort_unstable();
                            evidence.dedup();
                            return Err(evidence);
                        }
                    }
                }
            }
            if !changed {
                return Ok(fixed);
            }
        }
    }

    /// Leave the approach phase: enforce the deferred rules exactly.
    fn end_approach(&mut self) {
        if self.approach.take().is_some() {
            self.rules = self.all_rules.clone();
            self.presolved = false;
        }
    }

    fn soft_rows(&self, p: &Problem, rows: &[EvalRow], x: &[f64]) -> Vec<SoftRow> {
        let mut out = Vec::new();
        for r in rows {
            let Some(rule) = p.rules.get(r.rule) else { continue };
            if rule.is_hard() {
                continue;
            }
            let res = r.scaled();
            if r.relation == Relation::Le && res <= 0.0 {
                continue;
            }
            out.push(SoftRow { grad: r.grad.clone(), r: res, w: rule.strength.weight() });
        }
        for t in &p.targets {
            let Some(c) = self.cols.col.get(t.var.index()).copied().flatten() else { continue };
            let s = self.cols.scales.get(c).copied().unwrap_or(1.0);
            let xv = x.get(t.var.index()).copied().unwrap_or(0.0);
            let strength = if t.strength == Strength::Required { Strength::Strong } else { t.strength };
            out.push(SoftRow { grad: vec![(c, 1.0)], r: (xv - t.value) / s, w: strength.weight() });
        }
        if let Some(a) = &self.approach {
            // Gradients only when the caller evaluated its rows with gradients.
            let with_grad = rows.iter().any(|r| !r.grad.is_empty());
            let deferred =
                if with_grad { eval_rows(p, &a.deferred, &self.cols, x) } else { eval_values(p, &a.deferred, x) };
            for r in deferred {
                out.push(SoftRow { grad: r.grad, r: r.value / r.scale, w: Strength::Strong.weight() });
            }
        }
        out
    }

    /// Largest scaled residual of the deferred rows.
    fn approach_residual(&self, p: &Problem, x: &[f64]) -> f64 {
        self.approach
            .as_ref()
            .map_or(0.0, |a| eval_values(p, &a.deferred, x).iter().map(EvalRow::violation).fold(0.0, f64::max))
    }

    /// Stay offsets `d_c = (x − x_prev) / scale`.
    fn stay_offsets(&self, x: &[f64]) -> DVector<f64> {
        DVector::from_iterator(
            self.cols.n(),
            self.cols.vars.iter().enumerate().map(|(c, v)| {
                let s = self.cols.scales.get(c).copied().unwrap_or(1.0);
                let xv = x.get(v.index()).copied().unwrap_or(0.0);
                (xv - self.stay_ref.get(v.index()).copied().unwrap_or(xv)) / s
            }),
        )
    }

    fn stay_w2(&self, c: usize) -> f64 {
        let w = self.stay_w.get(c).copied().unwrap_or(STAY_WEIGHT);
        w * w
    }

    /// Preference objective (soft rows + stays).
    fn objective(&self, soft: &[SoftRow], x: &[f64]) -> f64 {
        let s: f64 = soft.iter().map(|s| s.w * s.w * s.r * s.r).sum();
        s + self.stay_offsets(x).iter().enumerate().map(|(c, d)| self.stay_w2(c) * d * d).sum::<f64>()
    }

    /// Scaled gradient of one row at `x` (same convention as `eval_rows`).
    fn row_grad(&self, p: &Problem, rule: usize, row: usize, x: &[f64]) -> Vec<(usize, f64)> {
        let Some(r) = p.rules.get(rule).and_then(|r| r.rows.get(row)) else { return Vec::new() };
        let scale = if r.scale.is_finite() && r.scale > 0.0 { r.scale } else { 1.0 };
        r.expr
            .eval_dual(x)
            .g
            .iter()
            .filter_map(|&(v, g)| {
                let c = self.cols.col.get(v as usize).copied().flatten()?;
                let s = self.cols.scales.get(c).copied().unwrap_or(1.0);
                Some((c, g * s / scale))
            })
            .collect()
    }

    /// `Σ λᵢ ∇²cᵢ` over the given rows (scaled variables), as symmetric lower-triangle
    /// entries. Row Hessians come from forward differences of the exact gradients.
    fn curvature(&self, p: &Problem, hard: &[&EvalRow], x: &[f64]) -> Vec<(usize, usize, f64)> {
        let mut acc: BTreeMap<(usize, usize), f64> = BTreeMap::new();
        let mut xp = x.to_vec();
        for r in hard {
            let Some(&l) = self.lambda.get(&(r.rule, r.row)) else { continue };
            if l == 0.0 || !l.is_finite() {
                continue;
            }
            for &(c, _) in &r.grad {
                let Some(v) = self.cols.vars.get(c) else { continue };
                let s = self.cols.scales.get(c).copied().unwrap_or(1.0);
                let i = v.index();
                let Some(&x0) = x.get(i) else { continue };
                if let Some(slot) = xp.get_mut(i) {
                    *slot = x0 + FD_STEP * s;
                }
                let gp = self.row_grad(p, r.rule, r.row, &xp);
                if let Some(slot) = xp.get_mut(i) {
                    *slot = x0;
                }
                // Column c of the row Hessian: (∇c(x + h·e_c) − ∇c(x)) / h.
                let mut col: BTreeMap<usize, f64> = gp.into_iter().collect();
                for &(c2, g0) in &r.grad {
                    *col.entry(c2).or_default() -= g0;
                }
                for (c2, dg) in col {
                    let hv = l * dg / FD_STEP;
                    if !hv.is_finite() || hv == 0.0 {
                        continue;
                    }
                    // Symmetrize: average (c2, c) and (c, c2).
                    let k = (c2.max(c), c2.min(c));
                    *acc.entry(k).or_default() += if c2 == c { hv } else { 0.5 * hv };
                }
            }
        }
        acc.into_iter().map(|((i, j), v)| (i, j, v)).collect()
    }

    /// Objective gradient term `g` and Hessian (diagonal part + off-diagonal soft
    /// couplings) of the step's quadratic model.
    fn model(&self, soft: &[SoftRow], x: &[f64]) -> Model {
        let n = self.cols.n();
        let mut hdiag: Vec<f64> = (0..n).map(|c| self.stay_w2(c)).collect();
        let d = self.stay_offsets(x);
        let mut g = DVector::from_iterator(n, d.iter().zip(&hdiag).map(|(d, w)| -d * w));
        let mut off = Vec::new();
        for s in soft {
            let w2 = s.w * s.w;
            for (k, &(c, a)) in s.grad.iter().enumerate() {
                if let Some(gi) = g.get_mut(c) {
                    *gi -= w2 * a * s.r;
                }
                if let Some(hd) = hdiag.get_mut(c) {
                    *hd += w2 * a * a;
                }
                for &(c2, a2) in s.grad.iter().take(k) {
                    if c2 != c {
                        off.push((c.max(c2), c.min(c2), w2 * a * a2));
                    } else if let Some(hd) = hdiag.get_mut(c) {
                        *hd += 2.0 * w2 * a * a2;
                    }
                }
            }
        }
        (g, hdiag, off)
    }

    /// Solve the step for the given hard rows. Returns `(Δ, λ)`; rows without
    /// unknowns (fully presolved) get `λ = 0`.
    fn kkt(&self, p: &Problem, hard: &[&EvalRow], soft: &[SoftRow], x: &[f64]) -> Option<(DVector<f64>, DVector<f64>)> {
        let live: Vec<&EvalRow> = hard.iter().copied().filter(|r| !r.grad.is_empty()).collect();
        // Curvature first; when it makes the model non-convex on the feasible
        // directions, fall back to the Gauss–Newton model for this step.
        let curved = if self.lambda.is_empty() { None } else { self.kkt_rows(p, &live, soft, x, true) };
        let (delta, lambda_live) = match curved {
            Some(s) => s,
            None => self.kkt_rows(p, &live, soft, x, false)?,
        };
        let mut it = lambda_live.iter();
        let lambda = DVector::from_iterator(
            hard.len(),
            hard.iter().map(|r| if r.grad.is_empty() { 0.0 } else { it.next().copied().unwrap_or(0.0) }),
        );
        Some((delta, lambda))
    }

    fn kkt_rows(
        &self,
        p: &Problem,
        hard: &[&EvalRow],
        soft: &[SoftRow],
        x: &[f64],
        use_curvature: bool,
    ) -> Option<(DVector<f64>, DVector<f64>)> {
        let n = self.cols.n();
        let m = hard.len();
        let (g, hdiag, off) = self.model(soft, x);
        let curv = if use_curvature { self.curvature(p, hard, x) } else { Vec::new() };
        if use_curvature && curv.is_empty() {
            return None;
        }
        let b = DVector::from_iterator(m, hard.iter().map(|r| -r.scaled()));
        if off.is_empty() && curv.is_empty() {
            // Diagonal Hessian: sparse Schur complement A·H⁻¹·Aᵀ.
            let dinv: Vec<f64> = hdiag.iter().map(|h| 1.0 / h).collect();
            let hg = DVector::from_iterator(n, g.iter().zip(&dinv).map(|(gi, di)| gi * di));
            if m == 0 {
                return Some((hg, DVector::zeros(0)));
            }
            let rhs = DVector::from_iterator(m, hard.iter().map(|r| dot_sparse(&r.grad, &hg))) - &b;
            let f = NormalFactor::new(hard, &dinv, n, 1e-13)?;
            let lambda = DVector::from_vec(f.solve(rhs.as_slice()));
            let delta = &hg - transpose_mul(hard, lambda.as_slice(), n, Some(&dinv));
            return delta.iter().all(|v| v.is_finite()).then_some((delta, lambda));
        }
        // General sparse Hessian: quasi-definite KKT system. Indefinite curvature in
        // the range of Aᵀ is removed with ρ·AᵀA (same step); indefiniteness on the
        // null space with a shift δ·I.
        let mut base: Vec<(usize, usize, f64)> = hdiag.iter().enumerate().map(|(c, &h)| (c, c, h)).collect();
        base.extend(off.iter().copied());
        base.extend(curv.iter().copied());
        let max_h = base.iter().filter(|e| e.0 == e.1).map(|e| e.2.abs()).fold(0.0, f64::max).max(1e-300);
        let mut ata: Vec<(usize, usize, f64)> = Vec::new();
        let mut max_a: f64 = 0.0;
        for r in hard {
            for (k, &(c, a)) in r.grad.iter().enumerate() {
                for &(c2, a2) in r.grad.iter().take(k + 1) {
                    ata.push((c.max(c2), c.min(c2), a * a2));
                    if c2 == c {
                        max_a = max_a.max(a * a);
                    }
                }
            }
        }
        let max_a = max_a.max(1e-300);
        let attempts: &[(f64, f64)] =
            if use_curvature { &[(0.0, 0.0), (10.0, 0.0)] } else { &[(0.0, 0.0), (0.0, 1e-10), (0.0, 1e-6)] };
        for &(rho_rel, shift_rel) in attempts {
            let rho = rho_rel * max_h / max_a;
            let shift = shift_rel * max_h;
            let mut h = base.clone();
            if rho > 0.0 {
                h.extend(ata.iter().map(|&(i, j, v)| (i, j, rho * v)));
            }
            if shift > 0.0 {
                h.extend((0..n).map(|c| (c, c, shift)));
            }
            let Some(f) = KktFactor::new(n, &h, hard) else { continue };
            // g + ρ·Aᵀ·b keeps the minimizer of the augmented model unchanged.
            let mut gr = g.clone();
            if rho > 0.0 {
                gr += transpose_mul(hard, b.as_slice(), n, None) * rho;
            }
            let rhs: Vec<f64> = gr.iter().chain(b.iter()).copied().collect();
            let z = f.solve(&rhs);
            let delta = DVector::from_iterator(n, z.iter().take(n).copied());
            let lambda = DVector::from_iterator(m, z.iter().skip(n).copied());
            if delta.iter().chain(lambda.iter()).all(|v| v.is_finite()) {
                return Some((delta, lambda));
            }
        }
        None
    }

    fn apply(&self, x: &mut [f64], base: &[f64], delta: &DVector<f64>, alpha: f64) {
        for (c, v) in self.cols.vars.iter().enumerate() {
            let s = self.cols.scales.get(c).copied().unwrap_or(1.0);
            if let (Some(slot), Some(old), Some(d)) = (x.get_mut(v.index()), base.get(v.index()), delta.get(c)) {
                *slot = old + alpha * d * s;
            }
        }
    }

    /// Hard rows to satisfy: equalities, the given active inequalities and every
    /// violated inequality.
    fn restoration_rows<'a>(
        p: &Problem,
        rows: &'a [EvalRow],
        keys: &BTreeSet<(usize, usize)>,
        tol: f64,
    ) -> Vec<&'a EvalRow> {
        rows.iter()
            .filter(|r| {
                is_hard(p, r)
                    && (r.relation == Relation::Eq || keys.contains(&(r.rule, r.row)) || r.violation() > tol * 0.01)
            })
            .collect()
    }

    /// Damped minimum-norm Newton (Gauss–Newton) step on the hard rows with an
    /// Armijo test on `Σ violation²`. Returns the new maximum violation, or `None`
    /// when no damped step reduces the merit.
    fn newton_step(&self, p: &Problem, x: &mut [f64], keys: &BTreeSet<(usize, usize)>, tol: f64) -> Option<f64> {
        let rows = eval_rows(p, &self.rules, &self.cols, x);
        let sel = Self::restoration_rows(p, &rows, keys, tol);
        let viol = sel.iter().map(|r| r.violation()).fold(0.0, f64::max);
        let m0 = merit(p, &rows);
        if !viol.is_finite() || !m0.is_finite() {
            return None;
        }
        if viol <= tol * 0.01 {
            return Some(viol);
        }
        let b = DVector::from_iterator(sel.len(), sel.iter().map(|r| -r.scaled()));
        let corr = min_norm(&sel, &b, self.cols.n())?;
        let base = x.to_vec();
        let mut alpha = 1.0;
        for _ in 0..20 {
            self.apply(x, &base, &corr, alpha);
            let rows_new = eval_values(p, &self.rules, x);
            let m_new = merit(p, &rows_new);
            if m_new.is_finite() && m_new <= (1.0 - 2e-4 * alpha) * m0 {
                let hard_new: Vec<EvalRow> = rows_new.into_iter().filter(|r| is_hard(p, r)).collect();
                return Some(max_violation(&hard_new));
            }
            alpha *= 0.5;
        }
        x.copy_from_slice(&base);
        None
    }

    /// Project `x` back onto the hard manifold with damped Newton steps.
    fn project(&self, p: &Problem, x: &mut [f64], keys: &BTreeSet<(usize, usize)>, tol: f64) {
        for _ in 0..PROJECTION_STEPS {
            match self.newton_step(p, x, keys, tol) {
                Some(v) if v > tol * 0.01 => {}
                _ => return,
            }
        }
    }

    /// Finish the current phase: the approach phase hands over to exact
    /// enforcement, the final phase records the status.
    fn finish_phase(&mut self, hard_rows: &[EvalRow], opts: &SolveOptions) -> bool {
        if self.approach.is_some() {
            self.end_approach();
            return false;
        }
        self.finish(hard_rows, opts);
        true
    }

    /// One iteration. Returns `true` when finished.
    pub fn iterate(&mut self, p: &Problem, x: &mut [f64], opts: &SolveOptions) -> bool {
        if self.done.is_some() {
            return true;
        }
        if !self.presolved {
            self.presolve(p, x, opts);
            if self.presolve_conflict.is_some() {
                self.done = Some(Status::Conflicting);
                return true;
            }
        }
        let rows = eval_rows(p, &self.rules, &self.cols, x);
        let hard_rows: Vec<EvalRow> = rows.iter().filter(|r| is_hard(p, r)).cloned().collect();
        let hard_old = max_violation(&hard_rows);
        self.max_hard = hard_old;
        if self.cols.n() == 0 || self.iterations >= opts.max_iterations {
            return self.finish_phase(&hard_rows, opts);
        }
        self.iterations += 1;
        let tol = opts.tolerance;
        if hard_old > tol {
            // Restoration: reach the hard manifold first (damped Gauss–Newton).
            return match self.newton_step(p, x, &BTreeSet::new(), tol) {
                Some(v) => {
                    self.max_hard = v;
                    false
                }
                None => {
                    self.stalled = true;
                    let rows = eval_rows(p, &self.rules, &self.cols, x);
                    let hard: Vec<EvalRow> = rows.iter().filter(|r| is_hard(p, r)).cloned().collect();
                    self.finish_phase(&hard, opts)
                }
            };
        }
        let soft = self.soft_rows(p, &rows, x);
        let obj_old = self.objective(&soft, x);

        // Active set over hard inequalities (multiplier driven).
        let tol_act = opts.tolerance.max(1e-12);
        let mut active: BTreeSet<(usize, usize)> = hard_rows
            .iter()
            .filter(|r| r.relation == Relation::Le && (r.scaled() > -tol_act || self.active.contains(&(r.rule, r.row))))
            .map(|r| (r.rule, r.row))
            .collect();
        let mut step = None;
        for _ in 0..(2 * hard_rows.len() + 2) {
            let a_rows: Vec<&EvalRow> =
                hard_rows.iter().filter(|r| r.relation == Relation::Eq || active.contains(&(r.rule, r.row))).collect();
            let Some((delta, lambda)) = self.kkt(p, &a_rows, &soft, x) else { break };
            let release = a_rows
                .iter()
                .zip(lambda.iter())
                .filter(|(r, l)| r.relation == Relation::Le && **l < -1e-14)
                .min_by(|a, b| a.1.total_cmp(b.1))
                .map(|(r, _)| (r.rule, r.row));
            if let Some(key) = release {
                active.remove(&key);
                continue;
            }
            let add = hard_rows
                .iter()
                .filter(|r| r.relation == Relation::Le && !active.contains(&(r.rule, r.row)))
                .map(|r| (r, r.scaled() + dot_sparse(&r.grad, &delta)))
                .filter(|(_, l)| *l > tol_act)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(r, _)| (r.rule, r.row));
            if let Some(key) = add {
                active.insert(key);
                continue;
            }
            self.lambda = a_rows.iter().zip(lambda.iter()).map(|(r, l)| ((r.rule, r.row), *l)).collect();
            step = Some(delta);
            break;
        }
        self.active = active.clone();
        let Some(mut delta) = step else {
            self.stalled = true;
            return self.finish_phase(&hard_rows, opts);
        };
        let maxabs = delta.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        if maxabs <= 1e-14 {
            // Stationary: nothing left to improve.
            return self.finish_phase(&hard_rows, opts);
        }
        if maxabs > self.trust {
            delta *= self.trust / maxabs;
        }

        // Line search; each trial point is projected back onto the hard manifold.
        let x_old = x.to_vec();
        let mut alpha = 1.0;
        let mut accepted = None;
        for _ in 0..12 {
            self.apply(x, &x_old, &delta, alpha);
            self.project(p, x, &active, tol);
            let rows_new = eval_values(p, &self.rules, x);
            let hard_new = rows_new
                .iter()
                .filter(|r| is_hard(p, r))
                .map(|r| if r.value.is_finite() { r.violation() } else { f64::INFINITY })
                .fold(0.0, f64::max);
            let obj_new = self.objective(&self.soft_rows(p, &rows_new, x), x);
            if hard_new <= tol && obj_new < obj_old {
                accepted = Some((hard_new, obj_new));
                break;
            }
            alpha *= 0.5;
        }
        match accepted {
            Some((hard_new, obj_new)) => {
                self.max_hard = hard_new;
                if alpha == 1.0 {
                    self.trust = (self.trust * 2.0).min(1e6);
                }
                let moved = self
                    .cols
                    .vars
                    .iter()
                    .enumerate()
                    .map(|(c, v)| {
                        let s = self.cols.scales.get(c).copied().unwrap_or(1.0);
                        ((x.get(v.index()).copied().unwrap_or(0.0) - x_old.get(v.index()).copied().unwrap_or(0.0)) / s)
                            .abs()
                    })
                    .fold(0.0, f64::max);
                let rel_gain = if obj_old > 1e-300 { (obj_old - obj_new) / obj_old } else { 0.0 };
                if self.approach.is_some() && (self.approach_residual(p, x) <= APPROACH_TOL || rel_gain < APPROACH_GAIN)
                {
                    // Hand over once close, or once a step stops making progress (the
                    // remaining gap is left to exact enforcement).
                    self.end_approach();
                    return false;
                }
                if hard_new <= tol && (moved <= 1e-12 || rel_gain <= 1e-9) {
                    let rows = eval_rows(p, &self.rules, &self.cols, x);
                    let hard: Vec<EvalRow> = rows.into_iter().filter(|r| is_hard(p, r)).collect();
                    return self.finish_phase(&hard, opts);
                }
                false
            }
            None => {
                // Feasible and no projected trial step lowers the objective: optimal
                // within the line-search resolution.
                x.copy_from_slice(&x_old);
                self.finish_phase(&hard_rows, opts)
            }
        }
    }

    fn finish(&mut self, hard_rows: &[EvalRow], opts: &SolveOptions) {
        let n = self.cols.n();
        let max_hard = max_violation(hard_rows);
        self.max_hard = max_hard;
        let eq_rows: Vec<&EvalRow> = hard_rows.iter().filter(|r| r.relation == Relation::Eq).collect();
        if max_hard <= opts.tolerance {
            if opts.analyze {
                let a = dense(&eq_rows, n);
                let r = DVector::from_iterator(eq_rows.len(), eq_rows.iter().map(|r| r.scaled()));
                let info = rank_analysis(&a, &r, opts.tolerance * 10.0);
                let dof = n.saturating_sub(info.rank);
                let mut redundant: Vec<usize> =
                    info.redundant_rows.iter().filter_map(|i| eq_rows.get(*i).map(|r| r.rule)).collect();
                redundant.sort_unstable();
                redundant.dedup();
                self.redundant = redundant;
                self.done = Some(if dof == 0 { Status::Solved } else { Status::Underconstrained { dof } });
            } else {
                self.done = Some(Status::Solved);
            }
        } else {
            let a = dense(&eq_rows, n);
            let r = DVector::from_iterator(eq_rows.len(), eq_rows.iter().map(|r| r.scaled()));
            let info = rank_analysis(&a, &r, opts.tolerance * 10.0);
            self.done =
                Some(Status::NotConverged { suspected_conflict: self.stalled || !info.inconsistent_rows.is_empty() });
        }
    }

    /// Rules flagged as inconsistent: presolve contradictions, rank-analysis
    /// evidence, violated inequalities and violated rows without unknowns.
    pub fn inconsistent_rules(&self, p: &Problem, x: &[f64], opts: &SolveOptions) -> Vec<usize> {
        if let Some(c) = &self.presolve_conflict {
            return c.clone();
        }
        let rows = eval_rows(p, &self.rules, &self.cols, x);
        let hard: Vec<&EvalRow> = rows.iter().filter(|r| is_hard(p, r) && r.relation == Relation::Eq).collect();
        let a = dense(&hard, self.cols.n());
        let r = DVector::from_iterator(hard.len(), hard.iter().map(|r| r.scaled()));
        let info = rank_analysis(&a, &r, opts.tolerance * 10.0);
        let mut rules: Vec<usize> =
            info.inconsistent_rows.iter().filter_map(|i| hard.get(*i).map(|r| r.rule)).collect();
        rules.extend(
            rows.iter()
                .filter(|r| is_hard(p, r) && r.relation == Relation::Le && r.violation() > opts.tolerance)
                .map(|r| r.rule),
        );
        rules.extend(
            rows.iter()
                .filter(|r| is_hard(p, r) && r.grad.is_empty() && r.violation() > opts.tolerance)
                .map(|r| r.rule),
        );
        rules.sort_unstable();
        rules.dedup();
        rules
    }

    /// Rules flagged as redundant by the final rank analysis.
    pub fn redundant_rules(&self, _p: &Problem, _x: &[f64], opts: &SolveOptions) -> Vec<usize> {
        if !opts.analyze {
            return Vec::new();
        }
        self.redundant.clone()
    }
}
