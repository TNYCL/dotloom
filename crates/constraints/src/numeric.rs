//! Numeric backend for nonlinear/mixed components.
//!
//! Each iteration solves the equality-constrained least-squares step (scaled
//! variables `y = x / scale`):
//!
//! ```text
//! minimize   Σ w²·(aᵢ·Δ + rᵢ)² + w_s²·‖Δ + d‖²   (preferences, targets, stays)
//! subject to A·Δ = −r_hard                       (hard equalities + active inequalities)
//! ```
//!
//! Hard rules are exact constraints of the step (KKT system solved through its Schur
//! complement), never penalty terms, so preferences only choose among hard-feasible
//! points. Preference weights are bounded (strong 1, medium 0.1, weak 0.01, stay
//! 0.001) which keeps the Schur complement well conditioned. After each trial step
//! the point is projected back onto the hard manifold with minimum-norm Newton
//! corrections. Inequalities use a primal active set driven by the multipliers.
//! Hard equalities linear in a single unknown are eliminated first (presolve).

use std::collections::BTreeSet;

use nalgebra::{Cholesky, DMatrix, DVector};

use crate::{
    Problem, Relation, SolveOptions, Status, Strength, VarId,
    analysis::{Columns, EvalRow, dense, eval_rows, max_violation, rank_analysis},
    graph::Component,
    problem::STAY_WEIGHT,
};

/// Newton projection steps per trial point.
const PROJECTION_STEPS: usize = 8;

/// One least-squares preference row.
struct SoftRow {
    grad: Vec<(usize, f64)>,
    r: f64,
    w: f64,
}

pub(crate) struct NumericSolver {
    stay_w: Vec<f64>,
    rules: Vec<usize>,
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
}

fn is_hard(p: &Problem, r: &EvalRow) -> bool {
    p.rules.get(r.rule).is_some_and(crate::Rule::is_hard)
}

fn dot_sparse(grad: &[(usize, f64)], v: &DVector<f64>) -> f64 {
    grad.iter().map(|&(c, a)| a * v.get(c).copied().unwrap_or(0.0)).sum()
}

/// Cholesky of a symmetric matrix with escalating diagonal regularization relative to
/// its largest diagonal entry (handles redundant, rank-deficient rows).
fn regularized_cholesky(mut s: DMatrix<f64>, rel: f64) -> Option<Cholesky<f64, nalgebra::Dyn>> {
    let m = s.nrows();
    let max_diag = (0..m).map(|i| s[(i, i)].abs()).fold(0.0, f64::max).max(1e-300);
    let mut reg = rel * max_diag;
    for i in 0..m {
        s[(i, i)] += reg;
    }
    for _ in 0..5 {
        if let Some(c) = Cholesky::new(s.clone()) {
            return Some(c);
        }
        for i in 0..m {
            s[(i, i)] += reg * 999.0;
        }
        reg *= 1000.0;
    }
    None
}

/// Minimum-norm correction `δ = Aᵀ (A Aᵀ)⁻¹ b` for sparse rows.
fn min_norm(rows: &[&EvalRow], b: &DVector<f64>, n: usize) -> Option<DVector<f64>> {
    if rows.is_empty() {
        return Some(DVector::zeros(n));
    }
    let a = dense(rows, n);
    let aat = &a * a.transpose();
    let c = regularized_cholesky(aat, 1e-13)?;
    let y = c.solve(b);
    Some(a.transpose() * y)
}

impl NumericSolver {
    pub fn new(p: &Problem, comp: &Component, stay_ref: &[f64]) -> Self {
        Self {
            stay_w: Vec::new(),
            rules: comp.rules.clone(),
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
    fn presolve(&mut self, p: &Problem, x: &mut [f64], opts: &SolveOptions) {
        self.presolved = true;
        let mut fixed: BTreeSet<VarId> = BTreeSet::new();
        let mut determined_by: Vec<(VarId, usize)> = Vec::new();
        loop {
            let mut changed = false;
            for &ri in &self.rules {
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
                            self.presolve_conflict = Some(evidence);
                            return;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let remaining: Vec<VarId> = self.comp_vars.iter().copied().filter(|v| !fixed.contains(v)).collect();
        self.cols = Columns::new(p, &remaining);
        self.refresh_stay(p);
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
        out
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

    /// Solve the KKT step for the given hard rows. Returns `(Δ, λ)`.
    fn kkt(&self, hard: &[&EvalRow], soft: &[SoftRow], x: &[f64]) -> Option<(DVector<f64>, DVector<f64>)> {
        let n = self.cols.n();
        let ws2: Vec<f64> = (0..n).map(|c| self.stay_w2(c)).collect();
        let d = self.stay_offsets(x);
        let mut g = DVector::from_iterator(n, d.iter().zip(&ws2).map(|(d, w)| -d * w));
        let diag_only = soft.iter().all(|s| s.grad.len() <= 1);
        let mut hdiag = ws2.clone();
        let mut hdense =
            if diag_only { None } else { Some(DMatrix::<f64>::from_diagonal(&DVector::from_vec(ws2.clone()))) };
        for s in soft {
            let w2 = s.w * s.w;
            for &(c, a) in &s.grad {
                if let Some(gi) = g.get_mut(c) {
                    *gi -= w2 * a * s.r;
                }
                if let Some(h) = hdense.as_mut() {
                    for &(c2, a2) in &s.grad {
                        if let Some(e) = h.get_mut((c, c2)) {
                            *e += w2 * a * a2;
                        }
                    }
                } else if let Some(hd) = hdiag.get_mut(c) {
                    *hd += w2 * a * a;
                }
            }
        }
        let chol = match hdense {
            Some(h) => Some(Cholesky::new(h)?),
            None => None,
        };
        let hinv = |v: &DVector<f64>| -> DVector<f64> {
            match &chol {
                Some(c) => c.solve(v),
                None => DVector::from_iterator(n, v.iter().zip(&hdiag).map(|(a, h)| a / h)),
            }
        };
        let hg = hinv(&g);
        let m = hard.len();
        if m == 0 {
            return Some((hg, DVector::zeros(0)));
        }
        let at = dense(hard, n).transpose();
        let y = match &chol {
            Some(c) => c.solve(&at),
            None => {
                let mut y = at.clone();
                for (j, h) in hdiag.iter().enumerate() {
                    let mut row = y.row_mut(j);
                    row /= *h;
                }
                y
            }
        };
        let s = at.transpose() * &y;
        let b = DVector::from_iterator(m, hard.iter().map(|r| -r.scaled()));
        let rhs = DVector::from_iterator(m, hard.iter().map(|r| dot_sparse(&r.grad, &hg))) - b;
        let cs = regularized_cholesky(s, 1e-13)?;
        let lambda = cs.solve(&rhs);
        let delta = &hg - &y * &lambda;
        delta.iter().all(|v| v.is_finite()).then_some((delta, lambda))
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

    /// Damped minimum-norm Newton step on the hard rows. Returns the new violation
    /// or `None` when no damped step reduces it.
    fn newton_restore(&self, p: &Problem, x: &mut [f64], keys: &BTreeSet<(usize, usize)>, tol: f64) -> Option<f64> {
        let rows = eval_rows(p, &self.rules, &self.cols, x);
        let sel = Self::restoration_rows(p, &rows, keys, tol);
        let viol = sel.iter().map(|r| r.violation()).fold(0.0, f64::max);
        if !viol.is_finite() {
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
            let rows_new = eval_rows(p, &self.rules, &self.cols, x);
            let hard_new: Vec<EvalRow> = rows_new.iter().filter(|r| is_hard(p, r)).cloned().collect();
            let v_new = max_violation(&hard_new);
            if v_new.is_finite() && v_new < viol * (1.0 - 1e-4 * alpha) {
                return Some(v_new);
            }
            alpha *= 0.5;
        }
        x.copy_from_slice(&base);
        None
    }

    /// Project `x` back onto the hard manifold with damped Newton steps.
    fn project(&self, p: &Problem, x: &mut [f64], keys: &BTreeSet<(usize, usize)>, tol: f64) {
        for _ in 0..PROJECTION_STEPS {
            match self.newton_restore(p, x, keys, tol) {
                Some(v) if v > tol * 0.01 => {}
                _ => return,
            }
        }
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
            self.finish(&hard_rows, opts);
            return true;
        }
        self.iterations += 1;
        let tol = opts.tolerance;
        if hard_old > tol {
            // Restoration phase: reach the hard manifold first (minimum-norm, damped).
            let keys: BTreeSet<(usize, usize)> = BTreeSet::new();
            return match self.newton_restore(p, x, &keys, tol) {
                Some(v) => {
                    self.max_hard = v;
                    false
                }
                None => {
                    self.stalled = true;
                    let rows = eval_rows(p, &self.rules, &self.cols, x);
                    let hard: Vec<EvalRow> = rows.iter().filter(|r| is_hard(p, r)).cloned().collect();
                    self.finish(&hard, opts);
                    true
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
            let Some((delta, lambda)) = self.kkt(&a_rows, &soft, x) else { break };
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
            step = Some(delta);
            break;
        }
        self.active = active.clone();
        let Some(mut delta) = step else {
            self.stalled = true;
            self.finish(&hard_rows, opts);
            return true;
        };
        let maxabs = delta.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
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
            let rows_new = eval_rows(p, &self.rules, &self.cols, x);
            let hard_new_rows: Vec<EvalRow> = rows_new.iter().filter(|r| is_hard(p, r)).cloned().collect();
            let hard_new = max_violation(&hard_new_rows);
            let obj_new = self.objective(&self.soft_rows(p, &rows_new, x), x);
            let ok = hard_new <= tol && obj_new < obj_old;
            if ok {
                accepted = Some((hard_new, hard_new_rows, obj_new));
                break;
            }
            alpha *= 0.5;
        }
        match accepted {
            Some((hard_new, hard_new_rows, obj_new)) => {
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
                if hard_new <= tol && (moved <= 1e-12 || rel_gain <= 1e-9) {
                    self.finish(&hard_new_rows, opts);
                    return true;
                }
                false
            }
            None => {
                // Feasible and no projected trial step lowers the objective: optimal
                // within the line-search resolution.
                x.copy_from_slice(&x_old);
                self.finish(&hard_rows, opts);
                true
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
    pub fn redundant_rules(&self, p: &Problem, x: &[f64], opts: &SolveOptions) -> Vec<usize> {
        if !opts.analyze {
            return Vec::new();
        }
        let rows = eval_rows(p, &self.rules, &self.cols, x);
        let hard: Vec<&EvalRow> = rows.iter().filter(|r| is_hard(p, r) && r.relation == Relation::Eq).collect();
        let a = dense(&hard, self.cols.n());
        let r = DVector::from_iterator(hard.len(), hard.iter().map(|r| r.scaled()));
        let info = rank_analysis(&a, &r, opts.tolerance * 10.0);
        let mut rules: Vec<usize> = info.redundant_rows.iter().filter_map(|i| hard.get(*i).map(|r| r.rule)).collect();
        rules.sort_unstable();
        rules.dedup();
        rules
    }
}
