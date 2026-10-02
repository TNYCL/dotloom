//! Sparse symmetric factorizations for the numeric backend.
//!
//! Sketch Jacobians are very sparse (a row touches two to eight variables) and their
//! row graphs are chain- or mesh-like. Matrices are assembled sparsely, ordered with
//! reverse Cuthill–McKee and factored in envelope (skyline) storage, so a solve
//! costs roughly `size · bandwidth²` instead of a dense cubic factorization.
//!
//! - [`NormalFactor`]: Cholesky of the normal equations `A·diag(d)·Aᵀ` (minimum-norm
//!   corrections and steps with a diagonal objective Hessian).
//! - [`KktFactor`]: `L·D·Lᵀ` of the quasi-definite KKT matrix `[H Aᵀ; A −εI]`
//!   (steps with a general sparse Hessian, e.g. including constraint curvature),
//!   with an inertia check and iterative refinement.
//!
//! Assembly, ordering and factorization are deterministic (fixed summation order,
//! index tie-breaks), which keeps native and WebAssembly results bit-identical.
//! Indexing in the factor/solve loops relies on the envelope invariants built in
//! [`Envelope::new`] (`first[k] ≤ k`, `start` prefix sums), never on input data.

use crate::analysis::EvalRow;

/// Lower triangle of a symmetric matrix: row `i` holds `(j, value)` with `j ≤ i`,
/// sorted by `j`, duplicates summed in insertion order.
type Lower = Vec<Vec<(usize, f64)>>;

fn merge_rows(lower: &mut Lower) {
    for row in lower.iter_mut() {
        row.sort_by_key(|e| e.0);
        let mut merged: Vec<(usize, f64)> = Vec::with_capacity(row.len());
        for &(j, v) in row.iter() {
            match merged.last_mut() {
                Some(last) if last.0 == j => last.1 += v,
                _ => merged.push((j, v)),
            }
        }
        *row = merged;
    }
}

/// Envelope storage of a symmetrically permuted matrix.
#[derive(Debug, Clone)]
struct Envelope {
    /// `perm[k]` = original index at position `k`.
    perm: Vec<usize>,
    /// `pos[i]` = position of original index `i`.
    pos: Vec<usize>,
    /// First column (position) of row `k`'s envelope.
    first: Vec<usize>,
    /// Offset of row `k`'s envelope in `vals`; `start[k + 1] − 1` is the diagonal.
    start: Vec<usize>,
    /// Row-wise envelope values (columns `first[k]..=k`).
    vals: Vec<f64>,
}

impl Envelope {
    fn new(lower: &Lower, perm: Vec<usize>) -> Self {
        let size = lower.len();
        let mut pos = vec![0usize; size];
        for (k, &i) in perm.iter().enumerate() {
            if let Some(p) = pos.get_mut(i) {
                *p = k;
            }
        }
        let place = |i: usize, j: usize| {
            let (pi, pj) = (pos.get(i).copied().unwrap_or(0), pos.get(j).copied().unwrap_or(0));
            (pi.max(pj), pi.min(pj))
        };
        let mut first: Vec<usize> = (0..size).collect();
        for (i, row) in lower.iter().enumerate() {
            for &(j, _) in row {
                let (hi, lo) = place(i, j);
                if let Some(f) = first.get_mut(hi) {
                    *f = (*f).min(lo);
                }
            }
        }
        let mut start = Vec::with_capacity(size + 1);
        let mut total = 0usize;
        for (k, f) in first.iter().enumerate() {
            start.push(total);
            total += k - f + 1;
        }
        start.push(total);
        let mut vals = vec![0.0; total];
        for (i, row) in lower.iter().enumerate() {
            for &(j, v) in row {
                let (hi, lo) = place(i, j);
                let (Some(&f), Some(&st)) = (first.get(hi), start.get(hi)) else { continue };
                if let Some(e) = vals.get_mut(st + lo - f) {
                    *e += v;
                }
            }
        }
        Self { perm, pos, first, start, vals }
    }

    fn size(&self) -> usize {
        self.perm.len()
    }

    /// Diagonal entry at position `k`.
    fn diag_mut(&mut self, k: usize) -> Option<&mut f64> {
        let e = self.start.get(k + 1)?.checked_sub(1)?;
        self.vals.get_mut(e)
    }

    /// Row `k`'s envelope as a slice (columns `first[k]..=k`).
    fn row(&self, k: usize) -> &[f64] {
        let (Some(&a), Some(&b)) = (self.start.get(k), self.start.get(k + 1)) else { return &[] };
        self.vals.get(a..b).unwrap_or(&[])
    }

    /// In-place Cholesky `L·Lᵀ`; `false` on a non-positive pivot.
    fn cholesky(&mut self) -> bool {
        for k in 0..self.perm.len() {
            let (fk, sk) = (self.first[k], self.start[k]);
            for j in fk..k {
                let fj = self.first[j];
                let lo = fk.max(fj);
                let dot = dot(&self.row(k)[lo - fk..j - fk], &self.row(j)[lo - fj..j - fj]);
                let rj = self.row(j);
                let djj = rj[rj.len() - 1];
                let e = &mut self.vals[sk + j - fk];
                *e = (*e - dot) / djj;
            }
            let rk = self.row(k);
            let (head, diag) = rk.split_at(rk.len() - 1);
            let d = diag[0] - dot(head, head);
            if !(d > 0.0 && d.is_finite()) {
                return false;
            }
            self.vals[sk + k - fk] = d.sqrt();
        }
        true
    }

    /// In-place `L·D·Lᵀ` (unit `L`, `D` on the diagonal slots). `positive(i)` gives
    /// the required pivot sign of original index `i`; `false` when a pivot has the
    /// wrong sign or is negligible relative to `scale`.
    fn ldlt(&mut self, positive: &dyn Fn(usize) -> bool, scale: f64) -> bool {
        let tiny = 1e-14 * scale;
        // Row k of L·D (v_j = L_kj·D_j), rebuilt per row.
        let mut v: Vec<f64> = Vec::new();
        for k in 0..self.perm.len() {
            let (fk, sk) = (self.first[k], self.start[k]);
            v.clear();
            for j in fk..k {
                let fj = self.first[j];
                let lo = fk.max(fj);
                // v_j = a_kj − Σ_{l<j} v_l·L_jl
                let a_kj = self.vals[sk + j - fk];
                let s = dot(&v[lo - fk..], &self.row(j)[lo - fj..j - fj]);
                v.push(a_kj - s);
            }
            let mut d = self.vals[sk + k - fk];
            for (off, vj) in v.iter().enumerate() {
                let rj = self.row(fk + off);
                let l = vj / rj[rj.len() - 1];
                d -= vj * l;
                self.vals[sk + off] = l;
            }
            let want_positive = positive(self.perm[k]);
            if !d.is_finite() || d.abs() <= tiny || (d > 0.0) != want_positive {
                return false;
            }
            self.vals[sk + k - fk] = d;
        }
        true
    }

    fn permute(&self, b: &[f64]) -> Vec<f64> {
        self.perm.iter().map(|&i| b.get(i).copied().unwrap_or(0.0)).collect()
    }

    fn unpermute(&self, z: &[f64]) -> Vec<f64> {
        let mut y = vec![0.0; z.len()];
        for (k, &i) in self.perm.iter().enumerate() {
            if let (Some(slot), Some(v)) = (y.get_mut(i), z.get(k)) {
                *slot = *v;
            }
        }
        y
    }

    /// Forward substitution with the strictly lower part of `L` (unit diagonal).
    fn forward_unit(&self, z: &mut [f64]) {
        for k in 0..self.size() {
            let fk = self.first[k];
            let rk = self.row(k);
            z[k] -= dot(&rk[..rk.len() - 1], &z[fk..k]);
        }
    }

    /// Backward substitution with the strictly lower part of `L` transposed.
    fn backward_unit(&self, z: &mut [f64]) {
        for k in (0..self.size()).rev() {
            let fk = self.first[k];
            let rk = self.row(k);
            let zk = z[k];
            for (zl, lkl) in z[fk..k].iter_mut().zip(&rk[..rk.len() - 1]) {
                *zl -= lkl * zk;
            }
        }
    }

    /// Solve with a Cholesky factor.
    fn solve_cholesky(&self, b: &[f64]) -> Vec<f64> {
        let mut z = self.permute(b);
        for k in 0..self.size() {
            let fk = self.first[k];
            let rk = self.row(k);
            let (head, diag) = rk.split_at(rk.len() - 1);
            z[k] = (z[k] - dot(head, &z[fk..k])) / diag[0];
        }
        for k in (0..self.size()).rev() {
            let fk = self.first[k];
            let rk = self.row(k);
            let (head, diag) = rk.split_at(rk.len() - 1);
            let zk = z[k] / diag[0];
            z[k] = zk;
            for (zl, lkl) in z[fk..k].iter_mut().zip(head) {
                *zl -= lkl * zk;
            }
        }
        self.unpermute(&z)
    }

    /// Solve with an `L·D·Lᵀ` factor.
    fn solve_ldlt(&self, b: &[f64]) -> Vec<f64> {
        let mut z = self.permute(b);
        self.forward_unit(&mut z);
        for (k, zk) in z.iter_mut().enumerate() {
            let rk = self.row(k);
            *zk /= rk[rk.len() - 1];
        }
        self.backward_unit(&mut z);
        self.unpermute(&z)
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Factored `A·diag(d)·Aᵀ + reg·I`.
#[derive(Debug)]
pub(crate) struct NormalFactor {
    env: Envelope,
}

impl NormalFactor {
    /// Assemble and factor `A·diag(d)·Aᵀ`, with a diagonal regularization relative
    /// to the largest diagonal entry that escalates while the factorization fails
    /// (redundant, rank-deficient rows). `n` is the number of columns.
    pub fn new(rows: &[&EvalRow], d: &[f64], n: usize, rel_reg: f64) -> Option<Self> {
        let m = rows.len();
        let mut by_col: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        for (i, r) in rows.iter().enumerate() {
            for &(c, a) in &r.grad {
                if let Some(list) = by_col.get_mut(c) {
                    list.push((i, a));
                }
            }
        }
        let mut lower: Lower = vec![Vec::new(); m];
        for (c, list) in by_col.iter().enumerate() {
            let dc = d.get(c).copied().unwrap_or(1.0);
            for (k, &(i, ai)) in list.iter().enumerate() {
                for &(j, aj) in list.iter().take(k + 1) {
                    if let Some(row) = lower.get_mut(i.max(j)) {
                        row.push((i.min(j), ai * dc * aj));
                    }
                }
            }
        }
        merge_rows(&mut lower);
        let max_diag = lower
            .iter()
            .enumerate()
            .filter_map(|(i, row)| row.last().filter(|e| e.0 == i).map(|e| e.1.abs()))
            .fold(0.0, f64::max)
            .max(1e-300);
        let perm = rcm(&lower);
        let base = Envelope::new(&lower, perm);
        let mut reg = rel_reg * max_diag;
        for _ in 0..6 {
            let mut env = base.clone();
            for k in 0..m {
                if let Some(e) = env.diag_mut(k) {
                    *e += reg;
                }
            }
            if env.cholesky() {
                return Some(Self { env });
            }
            reg = if reg > 0.0 { reg * 1000.0 } else { 1e-13 * max_diag };
        }
        None
    }

    /// Solve `S·y = b`.
    pub fn solve(&self, b: &[f64]) -> Vec<f64> {
        self.env.solve_cholesky(b)
    }
}

/// `L·D·Lᵀ` of the quasi-definite KKT matrix `K = [H Aᵀ; A −εI]` (`n` primal and
/// `m` dual unknowns).
#[derive(Debug)]
pub(crate) struct KktFactor {
    env: Envelope,
    /// Lower triangle of `K` without the `−εI` block (iterative refinement target).
    lower: Lower,
}

impl KktFactor {
    /// Factor `[H Aᵀ; A −εI]`. `h` lists entries of `H` as `(i, j, value)` with
    /// `i ≥ j` (duplicates are summed); `rows` are the rows of `A`. Fails (`None`)
    /// when the inertia is not `(n, m)` — `H` is not positive definite on the null
    /// space of `A` — or a pivot vanishes; callers then regularize `H`.
    ///
    /// The ordering is reverse Cuthill–McKee with every dual unknown moved after its
    /// primal neighbours, so dual pivots are only as small as `−ε` for rows that
    /// depend on earlier rows (redundant rules). `ε` is relative to the largest entry.
    pub fn new(n: usize, h: &[(usize, usize, f64)], rows: &[&EvalRow]) -> Option<Self> {
        let m = rows.len();
        let mut lower: Lower = vec![Vec::new(); n + m];
        for &(i, j, v) in h {
            if let Some(row) = lower.get_mut(i.max(j)) {
                row.push((i.min(j), v));
            }
        }
        for (r, row) in rows.iter().enumerate() {
            if let Some(out) = lower.get_mut(n + r) {
                for &(c, a) in &row.grad {
                    out.push((c, a));
                }
            }
        }
        merge_rows(&mut lower);
        let scale = lower.iter().flat_map(|row| row.iter().map(|e| e.1.abs())).fold(0.0, f64::max).max(1e-300);
        // Duals after their primal neighbours.
        let order = rcm(&lower);
        let mut placed = vec![false; n + m];
        let mut perm = Vec::with_capacity(n + m);
        let mut pending: Vec<Vec<usize>> = vec![Vec::new(); n + m];
        let mut missing: Vec<usize> = (0..n + m)
            .map(|i| if i >= n { lower.get(i).map_or(0, |row| row.iter().filter(|e| e.0 < n).count()) } else { 0 })
            .collect();
        for (i, row) in lower.iter().enumerate().skip(n) {
            for &(j, _) in row {
                if let Some(p) = pending.get_mut(j) {
                    p.push(i);
                }
            }
        }
        let mut stack = Vec::new();
        for &i in &order {
            if i >= n {
                if missing.get(i).copied().unwrap_or(0) == 0 && !placed[i] {
                    placed[i] = true;
                    perm.push(i);
                }
                continue;
            }
            if placed[i] {
                continue;
            }
            placed[i] = true;
            perm.push(i);
            stack.clear();
            stack.extend(pending.get(i).into_iter().flatten().copied());
            for &d in &stack {
                if let Some(c) = missing.get_mut(d) {
                    *c = c.saturating_sub(1);
                    if *c == 0 && !placed[d] {
                        placed[d] = true;
                        perm.push(d);
                    }
                }
            }
        }
        for (i, p) in placed.iter().enumerate() {
            if !p {
                perm.push(i);
            }
        }
        let eps = 1e-12 * scale;
        let mut env = Envelope::new(&lower, perm);
        for r in 0..m {
            if let Some(k) = env.pos.get(n + r).copied()
                && let Some(e) = env.diag_mut(k)
            {
                *e -= eps;
            }
        }
        if !env.ldlt(&|i| i < n, scale) {
            return None;
        }
        Some(Self { env, lower })
    }

    /// Solve `K·z = rhs` with two steps of iterative refinement against `K` without
    /// the `−εI` regularization.
    pub fn solve(&self, rhs: &[f64]) -> Vec<f64> {
        let mut z = self.env.solve_ldlt(rhs);
        for _ in 0..2 {
            let mut r: Vec<f64> = rhs.to_vec();
            for (i, row) in self.lower.iter().enumerate() {
                for &(j, v) in row {
                    let (zi, zj) = (z.get(i).copied().unwrap_or(0.0), z.get(j).copied().unwrap_or(0.0));
                    if let Some(ri) = r.get_mut(i) {
                        *ri -= v * zj;
                    }
                    if i != j
                        && let Some(rj) = r.get_mut(j)
                    {
                        *rj -= v * zi;
                    }
                }
            }
            let c = self.env.solve_ldlt(&r);
            for (zi, ci) in z.iter_mut().zip(&c) {
                *zi += ci;
            }
        }
        z
    }
}

/// Reverse Cuthill–McKee ordering of the symmetric pattern given by its lower
/// triangle. Deterministic: ties break on degree, then index.
fn rcm(lower: &Lower) -> Vec<usize> {
    let m = lower.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); m];
    for (i, row) in lower.iter().enumerate() {
        for &(j, _) in row {
            if i != j {
                if let Some(a) = adj.get_mut(i) {
                    a.push(j);
                }
                if let Some(a) = adj.get_mut(j) {
                    a.push(i);
                }
            }
        }
    }
    for a in &mut adj {
        a.sort_unstable();
        a.dedup();
    }
    let degree: Vec<usize> = adj.iter().map(Vec::len).collect();
    let mut seen = vec![false; m];
    let mut order = Vec::with_capacity(m);
    let mut by_degree: Vec<usize> = (0..m).collect();
    by_degree.sort_by_key(|&i| (degree[i], i));
    for &root in &by_degree {
        if seen[root] {
            continue;
        }
        let root = peripheral(&adj, &degree, root);
        seen[root] = true;
        let mut head = order.len();
        order.push(root);
        while head < order.len() {
            let v = order[head];
            head += 1;
            let mut next: Vec<usize> = adj[v].iter().copied().filter(|&u| !seen[u]).collect();
            next.sort_by_key(|&u| (degree[u], u));
            for u in next {
                seen[u] = true;
                order.push(u);
            }
        }
    }
    order.reverse();
    order
}

/// Pseudo-peripheral node: repeat BFS from the farthest, lowest-degree node until
/// the eccentricity stops growing.
fn peripheral(adj: &[Vec<usize>], degree: &[usize], start: usize) -> usize {
    let mut node = start;
    let mut ecc = 0;
    for _ in 0..8 {
        let (far, e) = bfs_far(adj, degree, node);
        if e <= ecc {
            break;
        }
        ecc = e;
        node = far;
    }
    node
}

fn bfs_far(adj: &[Vec<usize>], degree: &[usize], start: usize) -> (usize, usize) {
    let mut level = vec![usize::MAX; adj.len()];
    level[start] = 0;
    let mut queue = vec![start];
    let mut head = 0;
    let mut best = (start, 0usize);
    while head < queue.len() {
        let v = queue[head];
        head += 1;
        let lv = level[v];
        if lv > best.1 || (lv == best.1 && (degree[v], v) < (degree[best.0], best.0)) {
            best = (v, lv);
        }
        for &u in &adj[v] {
            if level[u] == usize::MAX {
                level[u] = lv + 1;
                queue.push(u);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Relation;
    use nalgebra::{DMatrix, DVector};

    fn row(grad: Vec<(usize, f64)>) -> EvalRow {
        EvalRow { rule: 0, row: 0, relation: Relation::Eq, value: 0.0, scale: 1.0, grad }
    }

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f64) / f64::from(u32::MAX)
        }
    }

    fn random_rows(rng: &mut Lcg, m: usize, n: usize) -> Vec<EvalRow> {
        (0..m)
            .map(|i| {
                let mut g: Vec<(usize, f64)> = (0..3)
                    .map(|k| ((i * 7 + k * 13 + (rng.next() * 5.0) as usize) % n, rng.next() * 2.0 - 1.0))
                    .collect();
                g.sort_by_key(|e| e.0);
                g.dedup_by_key(|e| e.0);
                row(g)
            })
            .collect()
    }

    fn dense_of(rows: &[EvalRow], n: usize) -> DMatrix<f64> {
        let mut a = DMatrix::<f64>::zeros(rows.len(), n);
        for (i, r) in rows.iter().enumerate() {
            for &(c, v) in &r.grad {
                a[(i, c)] += v;
            }
        }
        a
    }

    #[test]
    fn normal_factor_matches_dense_products() {
        let mut rng = Lcg(12345);
        let (m, n) = (40, 70);
        let rows = random_rows(&mut rng, m, n);
        let refs: Vec<&EvalRow> = rows.iter().collect();
        let d: Vec<f64> = (0..n).map(|_| 0.5 + rng.next()).collect();
        let b: Vec<f64> = (0..m).map(|_| rng.next() - 0.5).collect();
        let f = NormalFactor::new(&refs, &d, n, 0.0).expect("factor");
        let y = f.solve(&b);
        let a = dense_of(&rows, n);
        let s = &a * DMatrix::from_diagonal(&DVector::from_vec(d)) * a.transpose();
        let back = &s * DVector::from_vec(y);
        for (i, bi) in b.iter().enumerate() {
            assert!((back[i] - bi).abs() < 1e-9, "row {i}: {} vs {bi}", back[i]);
        }
    }

    #[test]
    fn normal_factor_regularizes_redundant_rows() {
        // Two identical rows: S is singular; the regularized solve stays finite
        // and satisfies the consistent right-hand side.
        let rows = [row(vec![(0, 1.0), (1, -1.0)]), row(vec![(0, 1.0), (1, -1.0)])];
        let refs: Vec<&EvalRow> = rows.iter().collect();
        let f = NormalFactor::new(&refs, &[1.0, 1.0], 2, 1e-13).expect("factor");
        let y = f.solve(&[1.0, 1.0]);
        assert!(y.iter().all(|v| v.is_finite()));
        let delta0 = y[0] + y[1];
        assert!((2.0 * delta0 - 1.0).abs() < 1e-6, "A·Aᵀ·y = b: {delta0}");
    }

    #[test]
    fn chain_ordering_has_a_narrow_envelope() {
        // Rows of a chain given in shuffled order: RCM recovers bandwidth 1.
        let n = 60;
        let mut order: Vec<usize> = (0..n - 1).collect();
        order.reverse();
        order.rotate_left(17);
        let rows: Vec<EvalRow> = order.iter().map(|&i| row(vec![(i, 1.0), (i + 1, -1.0)])).collect();
        let refs: Vec<&EvalRow> = rows.iter().collect();
        let f = NormalFactor::new(&refs, &vec![1.0; n], n, 0.0).expect("factor");
        let width = (0..f.env.size()).map(|k| k - f.env.first[k]).max().unwrap_or(0);
        assert_eq!(width, 1);
    }

    #[test]
    fn kkt_factor_solves_an_equality_constrained_quadratic() {
        // minimize ½ΔᵀHΔ − gᵀΔ  s.t.  AΔ = b, with H indefinite but positive definite
        // on the null space of A; compare with the dense KKT solution.
        let mut rng = Lcg(777);
        let (m, n) = (12, 30);
        let rows = random_rows(&mut rng, m, n);
        let refs: Vec<&EvalRow> = rows.iter().collect();
        let mut h = Vec::new();
        let mut hd = DMatrix::<f64>::zeros(n, n);
        for i in 0..n {
            let v = 1.0 + rng.next();
            h.push((i, i, v));
            hd[(i, i)] += v;
            if i > 0 {
                let c = 0.3 * (rng.next() - 0.5);
                h.push((i, i - 1, c));
                hd[(i, i - 1)] += c;
                hd[(i - 1, i)] += c;
            }
        }
        let g: Vec<f64> = (0..n).map(|_| rng.next() - 0.5).collect();
        let b: Vec<f64> = (0..m).map(|_| rng.next() - 0.5).collect();
        let f = KktFactor::new(n, &h, &refs).expect("factor");
        let rhs: Vec<f64> = g.iter().chain(&b).copied().collect();
        let z = f.solve(&rhs);
        let a = dense_of(&rows, n);
        let mut k = DMatrix::<f64>::zeros(n + m, n + m);
        k.view_mut((0, 0), (n, n)).copy_from(&hd);
        k.view_mut((n, 0), (m, n)).copy_from(&a);
        k.view_mut((0, n), (n, m)).copy_from(&a.transpose());
        let exact = k.lu().solve(&DVector::from_vec(rhs)).expect("dense solve");
        for (i, (zi, ei)) in z.iter().zip(exact.iter()).enumerate() {
            assert!((zi - ei).abs() < 1e-8 * (1.0 + ei.abs()), "unknown {i}: {zi} vs {ei}");
        }
    }

    #[test]
    fn kkt_factor_rejects_wrong_inertia() {
        // H negative definite along the null space of A: no minimizer.
        let rows = [row(vec![(0, 1.0)])];
        let refs: Vec<&EvalRow> = rows.iter().collect();
        let h = [(0, 0, 1.0), (1, 1, -1.0)];
        assert!(KktFactor::new(2, &h, &refs).is_none());
    }
}
