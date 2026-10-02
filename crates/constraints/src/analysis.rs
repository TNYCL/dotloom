//! Row evaluation, residual checks and rank analysis shared by the backends.

use nalgebra::{DMatrix, DVector};

use crate::{Problem, Relation, VarId};

/// One evaluated row in scaled coordinates.
#[derive(Debug, Clone)]
pub(crate) struct EvalRow {
    /// Index of the rule in `Problem::rules`.
    pub rule: usize,
    /// Index of the row within the rule.
    pub row: usize,
    /// Relation.
    pub relation: Relation,
    /// Raw residual value (model units).
    pub value: f64,
    /// Row scale.
    pub scale: f64,
    /// Gradient w.r.t. *scaled* free variables: `(column, ∂(f/σ)/∂y)`.
    pub grad: Vec<(usize, f64)>,
}

impl EvalRow {
    /// Residual relative to the row scale.
    pub fn scaled(&self) -> f64 {
        self.value / self.scale
    }

    /// Violation relative to the row scale (`Le` rows: only the positive part).
    pub fn violation(&self) -> f64 {
        match self.relation {
            Relation::Eq => self.scaled().abs(),
            Relation::Le => self.scaled().max(0.0),
        }
    }
}

/// Column mapping of a component's free variables.
#[derive(Debug, Clone)]
pub(crate) struct Columns {
    /// Free variables in column order.
    pub vars: Vec<VarId>,
    /// `col[var] = Some(column)`.
    pub col: Vec<Option<usize>>,
    /// Variable scales per column.
    pub scales: Vec<f64>,
}

impl Columns {
    pub fn new(p: &Problem, vars: &[VarId]) -> Self {
        let mut col = vec![None; p.vars.len()];
        let mut scales = Vec::with_capacity(vars.len());
        for (i, v) in vars.iter().enumerate() {
            if let Some(slot) = col.get_mut(v.index()) {
                *slot = Some(i);
            }
            let s = p.vars.get(v.index()).map_or(1.0, |x| x.scale);
            scales.push(if s.is_finite() && s > 0.0 { s } else { 1.0 });
        }
        Self { vars: vars.to_vec(), col, scales }
    }

    pub fn n(&self) -> usize {
        self.vars.len()
    }
}

/// Evaluate all rows of the given rules.
pub(crate) fn eval_rows(p: &Problem, rules: &[usize], cols: &Columns, x: &[f64]) -> Vec<EvalRow> {
    let mut out = Vec::new();
    for &ri in rules {
        let Some(rule) = p.rules.get(ri) else { continue };
        for (k, row) in rule.rows.iter().enumerate() {
            let d = row.expr.eval_dual(x);
            let scale = if row.scale.is_finite() && row.scale > 0.0 { row.scale } else { 1.0 };
            let grad =
                d.g.iter()
                    .filter_map(|&(v, g)| {
                        let c = cols.col.get(v as usize).copied().flatten()?;
                        let s = cols.scales.get(c).copied().unwrap_or(1.0);
                        Some((c, g * s / scale))
                    })
                    .collect();
            out.push(EvalRow { rule: ri, row: k, relation: row.relation, value: d.v, scale, grad });
        }
    }
    out
}

/// Rows of the given rules evaluated without gradients (`grad` empty): line-search
/// trial points only need residuals.
pub(crate) fn eval_values(p: &Problem, rules: &[usize], x: &[f64]) -> Vec<EvalRow> {
    let mut out = Vec::new();
    for &ri in rules {
        let Some(rule) = p.rules.get(ri) else { continue };
        for (k, row) in rule.rows.iter().enumerate() {
            let scale = if row.scale.is_finite() && row.scale > 0.0 { row.scale } else { 1.0 };
            out.push(EvalRow {
                rule: ri,
                row: k,
                relation: row.relation,
                value: row.expr.eval(x),
                scale,
                grad: Vec::new(),
            });
        }
    }
    out
}

/// Largest hard violation among `rows` (relative to row scale); NaN counts as ∞.
pub(crate) fn max_violation(rows: &[EvalRow]) -> f64 {
    rows.iter().map(|r| if r.value.is_finite() { r.violation() } else { f64::INFINITY }).fold(0.0, f64::max)
}

/// Dense matrix from sparse rows.
pub(crate) fn dense(rows: &[&EvalRow], n: usize) -> DMatrix<f64> {
    let mut m = DMatrix::zeros(rows.len(), n);
    for (i, r) in rows.iter().enumerate() {
        for &(c, g) in &r.grad {
            if let Some(e) = m.get_mut((i, c)) {
                *e += g;
            }
        }
    }
    m
}

/// Rank analysis of the hard Jacobian at a point.
#[derive(Debug, Clone)]
pub(crate) struct RankInfo {
    /// Numerical rank.
    pub rank: usize,
    /// Rows participating in a dependency that the residual violates (conflict
    /// evidence), or in a consistent dependency (redundancy), as row indices.
    pub inconsistent_rows: Vec<usize>,
    /// Rows in consistent dependencies.
    pub redundant_rows: Vec<usize>,
}

/// Relative singular-value threshold for rank decisions.
pub(crate) const RANK_REL_TOL: f64 = 1e-9;

/// SVD-based rank and left-null-space analysis of `a` with residual `r`.
pub(crate) fn rank_analysis(a: &DMatrix<f64>, r: &DVector<f64>, tol: f64) -> RankInfo {
    let (m, n) = a.shape();
    if m == 0 || n == 0 {
        return RankInfo {
            rank: 0,
            inconsistent_rows: Vec::new(),
            redundant_rows: if n == 0 { (0..m).collect() } else { Vec::new() },
        };
    }
    // SVD of the m×n matrix. nalgebra returns thin U (m×k) with k = min(m, n);
    // the left null space beyond k is captured by projecting r out of range(U_r).
    let svd = a.clone().svd(true, false);
    let smax = svd.singular_values.iter().copied().fold(0.0, f64::max);
    let thresh = smax * RANK_REL_TOL * (m.max(n) as f64);
    let rank = svd.singular_values.iter().filter(|s| **s > thresh).count();
    let Some(u) = svd.u else {
        return RankInfo { rank, inconsistent_rows: Vec::new(), redundant_rows: Vec::new() };
    };
    if rank >= m {
        return RankInfo { rank, inconsistent_rows: Vec::new(), redundant_rows: Vec::new() };
    }
    // Basis of range(A) = columns of U with significant singular values.
    let mut idx: Vec<usize> = (0..svd.singular_values.len()).filter(|i| svd.singular_values[*i] > thresh).collect();
    idx.sort_unstable();
    let ur = u.select_columns(&idx);
    // Component of r outside range(A): r⊥ = r − U_r U_rᵀ r.
    let proj = &ur * (ur.transpose() * r);
    let r_perp = r - proj;
    // Rows in the left null space: a row participates in a dependency when its unit
    // vector has a component outside range(A).
    let mut dependent = Vec::new();
    for i in 0..m {
        let row_norm2: f64 = (0..ur.ncols()).map(|k| ur[(i, k)] * ur[(i, k)]).sum();
        if 1.0 - row_norm2 > 1e-6 {
            dependent.push(i);
        }
    }
    let perp_norm = r_perp.norm();
    if perp_norm > tol {
        let inconsistent = dependent.iter().copied().filter(|i| r_perp[*i].abs() > perp_norm * 1e-6).collect();
        RankInfo { rank, inconsistent_rows: inconsistent, redundant_rows: Vec::new() }
    } else {
        RankInfo { rank, inconsistent_rows: Vec::new(), redundant_rows: dependent }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_and_inconsistency() {
        // Rows: x = 1, x = 2 (inconsistent dependency), y = 0.
        let a = DMatrix::from_row_slice(3, 2, &[1.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        let r = DVector::from_vec(vec![-1.0, -2.0, 0.0]);
        let info = rank_analysis(&a, &r, 1e-9);
        assert_eq!(info.rank, 2);
        assert_eq!(info.inconsistent_rows, vec![0, 1]);
        // Consistent duplicate: x = 1 twice.
        let r2 = DVector::from_vec(vec![-1.0, -1.0, 0.0]);
        let info2 = rank_analysis(&a, &r2, 1e-9);
        assert!(info2.inconsistent_rows.is_empty());
        assert_eq!(info2.redundant_rows, vec![0, 1]);
    }
}
