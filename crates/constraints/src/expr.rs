//! Scalar expression trees with exact derivatives (forward-mode automatic
//! differentiation on sparse gradients).
//!
//! Every constraint row is an [`Expr`]. Built-in geometric rules and plugin
//! expressions compile to the same representation, so one Jacobian implementation
//! serves both. Gradients are exact (not finite differences); tests compare them
//! with central differences.

use core::fmt;

use serde::{Deserialize, Serialize};

/// Index of a solver variable within a [`crate::Problem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VarId(pub u32);

impl VarId {
    /// Index into value vectors.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A scalar expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "camelCase")]
pub enum Expr {
    /// Constant.
    Const(f64),
    /// Variable.
    Var(VarId),
    /// `a + b`.
    Add(Box<Expr>, Box<Expr>),
    /// `a - b`.
    Sub(Box<Expr>, Box<Expr>),
    /// `a * b`.
    Mul(Box<Expr>, Box<Expr>),
    /// `a / b`.
    Div(Box<Expr>, Box<Expr>),
    /// `-a`.
    Neg(Box<Expr>),
    /// `sin a`.
    Sin(Box<Expr>),
    /// `cos a`.
    Cos(Box<Expr>),
    /// `sqrt a` (domain `a ≥ 0`; negative inputs evaluate to NaN).
    Sqrt(Box<Expr>),
    /// `|a|`.
    Abs(Box<Expr>),
    /// `atan2(y, x)`.
    Atan2(Box<Expr>, Box<Expr>),
    /// `hypot(x, y)`.
    Hypot(Box<Expr>, Box<Expr>),
    /// `min(a, b)`.
    Min(Box<Expr>, Box<Expr>),
    /// `max(a, b)`.
    Max(Box<Expr>, Box<Expr>),
}

/// Value plus sparse gradient, sorted by variable index.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dual {
    /// Value.
    pub v: f64,
    /// Non-zero partial derivatives `(var, ∂/∂var)`, sorted by var.
    pub g: Vec<(u32, f64)>,
}

fn merge(a: &[(u32, f64)], ka: f64, b: &[(u32, f64)], kb: f64) -> Vec<(u32, f64)> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        match (a.get(i), b.get(j)) {
            (Some(&(va, da)), Some(&(vb, db))) if va == vb => {
                out.push((va, da * ka + db * kb));
                i += 1;
                j += 1;
            }
            (Some(&(va, da)), Some(&(vb, _))) if va < vb => {
                out.push((va, da * ka));
                i += 1;
            }
            (Some(_), Some(&(vb, db))) => {
                out.push((vb, db * kb));
                j += 1;
            }
            (Some(&(va, da)), None) => {
                out.push((va, da * ka));
                i += 1;
            }
            (None, Some(&(vb, db))) => {
                out.push((vb, db * kb));
                j += 1;
            }
            (None, None) => break,
        }
    }
    out
}

fn scale(a: &[(u32, f64)], k: f64) -> Vec<(u32, f64)> {
    a.iter().map(|&(v, d)| (v, d * k)).collect()
}

// `add`/`sub`/... are simplifying *constructors* taking two expressions (no `self`
// receiver); implementing the operator traits instead would hide the constant folding.
#[allow(clippy::should_implement_trait, clippy::redundant_guards)]
impl Expr {
    /// Constant expression.
    #[must_use]
    pub const fn c(v: f64) -> Self {
        Self::Const(v)
    }

    /// Variable expression.
    #[must_use]
    pub const fn var(v: VarId) -> Self {
        Self::Var(v)
    }

    fn as_const(&self) -> Option<f64> {
        if let Self::Const(v) = self { Some(*v) } else { None }
    }

    /// Simplifying constructor for `a + b`.
    #[must_use]
    pub fn add(a: Self, b: Self) -> Self {
        match (a.as_const(), b.as_const()) {
            (Some(x), Some(y)) => Self::Const(x + y),
            (Some(x), None) if x == 0.0 => b,
            (None, Some(y)) if y == 0.0 => a,
            _ => Self::Add(Box::new(a), Box::new(b)),
        }
    }

    /// Simplifying constructor for `a - b`.
    #[must_use]
    pub fn sub(a: Self, b: Self) -> Self {
        match (a.as_const(), b.as_const()) {
            (Some(x), Some(y)) => Self::Const(x - y),
            (None, Some(y)) if y == 0.0 => a,
            (Some(x), None) if x == 0.0 => Self::neg(b),
            _ => Self::Sub(Box::new(a), Box::new(b)),
        }
    }

    /// Simplifying constructor for `a * b`.
    #[must_use]
    pub fn mul(a: Self, b: Self) -> Self {
        match (a.as_const(), b.as_const()) {
            (Some(x), Some(y)) => Self::Const(x * y),
            (Some(x), _) | (_, Some(x)) if x == 0.0 => Self::Const(0.0),
            (Some(x), None) if x == 1.0 => b,
            (None, Some(y)) if y == 1.0 => a,
            _ => Self::Mul(Box::new(a), Box::new(b)),
        }
    }

    /// Simplifying constructor for `a / b`.
    #[must_use]
    pub fn div(a: Self, b: Self) -> Self {
        match (a.as_const(), b.as_const()) {
            (Some(x), Some(y)) => Self::Const(x / y),
            (None, Some(y)) if y == 1.0 => a,
            _ => Self::Div(Box::new(a), Box::new(b)),
        }
    }

    /// Simplifying constructor for `-a`.
    #[must_use]
    pub fn neg(a: Self) -> Self {
        match a {
            Self::Const(x) => Self::Const(-x),
            Self::Neg(inner) => *inner,
            other => Self::Neg(Box::new(other)),
        }
    }

    /// `sin a`.
    #[must_use]
    pub fn sin(a: Self) -> Self {
        a.as_const().map_or_else(|| Self::Sin(Box::new(a)), |x| Self::Const(libm::sin(x)))
    }

    /// `cos a`.
    #[must_use]
    pub fn cos(a: Self) -> Self {
        a.as_const().map_or_else(|| Self::Cos(Box::new(a)), |x| Self::Const(libm::cos(x)))
    }

    /// `sqrt a`.
    #[must_use]
    pub fn sqrt(a: Self) -> Self {
        a.as_const().map_or_else(|| Self::Sqrt(Box::new(a)), |x| Self::Const(x.sqrt()))
    }

    /// `|a|`.
    #[must_use]
    pub fn abs(a: Self) -> Self {
        a.as_const().map_or_else(|| Self::Abs(Box::new(a)), |x| Self::Const(x.abs()))
    }

    /// `atan2(y, x)`.
    #[must_use]
    pub fn atan2(y: Self, x: Self) -> Self {
        match (y.as_const(), x.as_const()) {
            (Some(a), Some(b)) => Self::Const(libm::atan2(a, b)),
            _ => Self::Atan2(Box::new(y), Box::new(x)),
        }
    }

    /// `hypot(x, y)`.
    #[must_use]
    pub fn hypot(x: Self, y: Self) -> Self {
        match (x.as_const(), y.as_const()) {
            (Some(a), Some(b)) => Self::Const(libm::hypot(a, b)),
            _ => Self::Hypot(Box::new(x), Box::new(y)),
        }
    }

    /// `min(a, b)`.
    #[must_use]
    pub fn min(a: Self, b: Self) -> Self {
        match (a.as_const(), b.as_const()) {
            (Some(x), Some(y)) => Self::Const(x.min(y)),
            _ => Self::Min(Box::new(a), Box::new(b)),
        }
    }

    /// `max(a, b)`.
    #[must_use]
    pub fn max(a: Self, b: Self) -> Self {
        match (a.as_const(), b.as_const()) {
            (Some(x), Some(y)) => Self::Const(x.max(y)),
            _ => Self::Max(Box::new(a), Box::new(b)),
        }
    }

    /// Evaluate with variable values `x` (missing variables evaluate to NaN).
    #[must_use]
    pub fn eval(&self, x: &[f64]) -> f64 {
        match self {
            Self::Const(v) => *v,
            Self::Var(v) => x.get(v.index()).copied().unwrap_or(f64::NAN),
            Self::Add(a, b) => a.eval(x) + b.eval(x),
            Self::Sub(a, b) => a.eval(x) - b.eval(x),
            Self::Mul(a, b) => a.eval(x) * b.eval(x),
            Self::Div(a, b) => a.eval(x) / b.eval(x),
            Self::Neg(a) => -a.eval(x),
            Self::Sin(a) => libm::sin(a.eval(x)),
            Self::Cos(a) => libm::cos(a.eval(x)),
            Self::Sqrt(a) => a.eval(x).sqrt(),
            Self::Abs(a) => a.eval(x).abs(),
            Self::Atan2(y, xx) => libm::atan2(y.eval(x), xx.eval(x)),
            Self::Hypot(a, b) => libm::hypot(a.eval(x), b.eval(x)),
            Self::Min(a, b) => a.eval(x).min(b.eval(x)),
            Self::Max(a, b) => a.eval(x).max(b.eval(x)),
        }
    }

    /// Evaluate value and exact gradient.
    ///
    /// Non-smooth points use a deterministic one-sided derivative: `|0|' = 0`,
    /// `sqrt'(0) = 0`, `hypot'(0, 0) = (1, 0)`, ties of `min/max` take the first argument.
    #[must_use]
    pub fn eval_dual(&self, x: &[f64]) -> Dual {
        match self {
            Self::Const(v) => Dual { v: *v, g: Vec::new() },
            Self::Var(v) => Dual { v: x.get(v.index()).copied().unwrap_or(f64::NAN), g: vec![(v.0, 1.0)] },
            Self::Add(a, b) => {
                let (a, b) = (a.eval_dual(x), b.eval_dual(x));
                Dual { v: a.v + b.v, g: merge(&a.g, 1.0, &b.g, 1.0) }
            }
            Self::Sub(a, b) => {
                let (a, b) = (a.eval_dual(x), b.eval_dual(x));
                Dual { v: a.v - b.v, g: merge(&a.g, 1.0, &b.g, -1.0) }
            }
            Self::Mul(a, b) => {
                let (a, b) = (a.eval_dual(x), b.eval_dual(x));
                Dual { v: a.v * b.v, g: merge(&a.g, b.v, &b.g, a.v) }
            }
            Self::Div(a, b) => {
                let (a, b) = (a.eval_dual(x), b.eval_dual(x));
                let inv = 1.0 / b.v;
                Dual { v: a.v * inv, g: merge(&a.g, inv, &b.g, -a.v * inv * inv) }
            }
            Self::Neg(a) => {
                let a = a.eval_dual(x);
                Dual { v: -a.v, g: scale(&a.g, -1.0) }
            }
            Self::Sin(a) => {
                let a = a.eval_dual(x);
                Dual { v: libm::sin(a.v), g: scale(&a.g, libm::cos(a.v)) }
            }
            Self::Cos(a) => {
                let a = a.eval_dual(x);
                Dual { v: libm::cos(a.v), g: scale(&a.g, -libm::sin(a.v)) }
            }
            Self::Sqrt(a) => {
                let a = a.eval_dual(x);
                let v = a.v.sqrt();
                let k = if v > 0.0 { 0.5 / v } else { 0.0 };
                Dual { v, g: scale(&a.g, k) }
            }
            Self::Abs(a) => {
                let a = a.eval_dual(x);
                let s = if a.v > 0.0 {
                    1.0
                } else if a.v < 0.0 {
                    -1.0
                } else {
                    0.0
                };
                Dual { v: a.v.abs(), g: scale(&a.g, s) }
            }
            Self::Atan2(y, xx) => {
                let (y, xx) = (y.eval_dual(x), xx.eval_dual(x));
                let r2 = y.v * y.v + xx.v * xx.v;
                if r2 > 0.0 {
                    Dual { v: libm::atan2(y.v, xx.v), g: merge(&y.g, xx.v / r2, &xx.g, -y.v / r2) }
                } else {
                    Dual { v: 0.0, g: Vec::new() }
                }
            }
            Self::Hypot(a, b) => {
                let (a, b) = (a.eval_dual(x), b.eval_dual(x));
                let v = libm::hypot(a.v, b.v);
                if v > 0.0 { Dual { v, g: merge(&a.g, a.v / v, &b.g, b.v / v) } } else { Dual { v, g: a.g } }
            }
            Self::Min(a, b) => {
                let (a, b) = (a.eval_dual(x), b.eval_dual(x));
                if a.v <= b.v { a } else { b }
            }
            Self::Max(a, b) => {
                let (a, b) = (a.eval_dual(x), b.eval_dual(x));
                if a.v >= b.v { a } else { b }
            }
        }
    }

    /// Variables referenced by the expression (sorted, unique).
    #[must_use]
    pub fn vars(&self) -> Vec<VarId> {
        let mut out = Vec::new();
        self.collect_vars(&mut out);
        out.sort_unstable();
        out.dedup();
        out
    }

    fn collect_vars(&self, out: &mut Vec<VarId>) {
        match self {
            Self::Const(_) => {}
            Self::Var(v) => out.push(*v),
            Self::Neg(a) | Self::Sin(a) | Self::Cos(a) | Self::Sqrt(a) | Self::Abs(a) => a.collect_vars(out),
            Self::Add(a, b)
            | Self::Sub(a, b)
            | Self::Mul(a, b)
            | Self::Div(a, b)
            | Self::Atan2(a, b)
            | Self::Hypot(a, b)
            | Self::Min(a, b)
            | Self::Max(a, b) => {
                a.collect_vars(out);
                b.collect_vars(out);
            }
        }
    }

    /// Affine form `Σ cᵢ·xᵢ + k` if the expression is exactly linear in the variables
    /// whose `fixed[i]` is false (fixed variables are folded into the constant).
    #[must_use]
    pub fn linear_form(&self, x: &[f64], fixed: &dyn Fn(VarId) -> bool) -> Option<LinearForm> {
        match self {
            Self::Const(v) => Some(LinearForm { terms: Vec::new(), constant: *v }),
            Self::Var(v) => {
                if fixed(*v) {
                    Some(LinearForm { terms: Vec::new(), constant: x.get(v.index()).copied().unwrap_or(f64::NAN) })
                } else {
                    Some(LinearForm { terms: vec![(*v, 1.0)], constant: 0.0 })
                }
            }
            Self::Add(a, b) => Some(a.linear_form(x, fixed)?.combine(&b.linear_form(x, fixed)?, 1.0)),
            Self::Sub(a, b) => Some(a.linear_form(x, fixed)?.combine(&b.linear_form(x, fixed)?, -1.0)),
            Self::Neg(a) => Some(a.linear_form(x, fixed)?.scaled(-1.0)),
            Self::Mul(a, b) => {
                let (la, lb) = (a.linear_form(x, fixed)?, b.linear_form(x, fixed)?);
                if la.terms.is_empty() {
                    Some(lb.scaled(la.constant))
                } else if lb.terms.is_empty() {
                    Some(la.scaled(lb.constant))
                } else {
                    None
                }
            }
            Self::Div(a, b) => {
                let (la, lb) = (a.linear_form(x, fixed)?, b.linear_form(x, fixed)?);
                (lb.terms.is_empty() && lb.constant != 0.0).then(|| la.scaled(1.0 / lb.constant))
            }
            // Non-linear operators are linear only when fully constant.
            other => {
                let vars = other.vars();
                if vars.iter().all(|v| fixed(*v)) {
                    Some(LinearForm { terms: Vec::new(), constant: other.eval(x) })
                } else {
                    None
                }
            }
        }
    }

    /// Replace every variable by the expression returned by `f` (constant folding is
    /// applied by the simplifying constructors).
    #[must_use]
    pub fn substitute(&self, f: &dyn Fn(VarId) -> Self) -> Self {
        match self {
            Self::Const(v) => Self::Const(*v),
            Self::Var(v) => f(*v),
            Self::Add(a, b) => Self::add(a.substitute(f), b.substitute(f)),
            Self::Sub(a, b) => Self::sub(a.substitute(f), b.substitute(f)),
            Self::Mul(a, b) => Self::mul(a.substitute(f), b.substitute(f)),
            Self::Div(a, b) => Self::div(a.substitute(f), b.substitute(f)),
            Self::Neg(a) => Self::neg(a.substitute(f)),
            Self::Sin(a) => Self::sin(a.substitute(f)),
            Self::Cos(a) => Self::cos(a.substitute(f)),
            Self::Sqrt(a) => Self::sqrt(a.substitute(f)),
            Self::Abs(a) => Self::abs(a.substitute(f)),
            Self::Atan2(a, b) => Self::atan2(a.substitute(f), b.substitute(f)),
            Self::Hypot(a, b) => Self::hypot(a.substitute(f), b.substitute(f)),
            Self::Min(a, b) => Self::min(a.substitute(f), b.substitute(f)),
            Self::Max(a, b) => Self::max(a.substitute(f), b.substitute(f)),
        }
    }

    /// Number of nodes (used for complexity limits).
    #[must_use]
    pub fn size(&self) -> usize {
        match self {
            Self::Const(_) | Self::Var(_) => 1,
            Self::Neg(a) | Self::Sin(a) | Self::Cos(a) | Self::Sqrt(a) | Self::Abs(a) => 1 + a.size(),
            Self::Add(a, b)
            | Self::Sub(a, b)
            | Self::Mul(a, b)
            | Self::Div(a, b)
            | Self::Atan2(a, b)
            | Self::Hypot(a, b)
            | Self::Min(a, b)
            | Self::Max(a, b) => 1 + a.size() + b.size(),
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Const(v) => write!(f, "{v}"),
            Self::Var(v) => write!(f, "x{}", v.0),
            Self::Add(a, b) => write!(f, "({a} + {b})"),
            Self::Sub(a, b) => write!(f, "({a} - {b})"),
            Self::Mul(a, b) => write!(f, "({a} * {b})"),
            Self::Div(a, b) => write!(f, "({a} / {b})"),
            Self::Neg(a) => write!(f, "-{a}"),
            Self::Sin(a) => write!(f, "sin({a})"),
            Self::Cos(a) => write!(f, "cos({a})"),
            Self::Sqrt(a) => write!(f, "sqrt({a})"),
            Self::Abs(a) => write!(f, "abs({a})"),
            Self::Atan2(a, b) => write!(f, "atan2({a}, {b})"),
            Self::Hypot(a, b) => write!(f, "hypot({a}, {b})"),
            Self::Min(a, b) => write!(f, "min({a}, {b})"),
            Self::Max(a, b) => write!(f, "max({a}, {b})"),
        }
    }
}

/// Affine form `Σ cᵢ·xᵢ + constant`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LinearForm {
    /// Coefficients (sorted by var, no duplicates after normalization).
    pub terms: Vec<(VarId, f64)>,
    /// Constant term.
    pub constant: f64,
}

impl LinearForm {
    fn scaled(mut self, k: f64) -> Self {
        for t in &mut self.terms {
            t.1 *= k;
        }
        self.constant *= k;
        self
    }

    fn combine(mut self, o: &Self, k: f64) -> Self {
        for &(v, c) in &o.terms {
            if let Some(t) = self.terms.iter_mut().find(|t| t.0 == v) {
                t.1 += c * k;
            } else {
                self.terms.push((v, c * k));
            }
        }
        self.constant += o.constant * k;
        self.terms.sort_by_key(|t| t.0);
        self.terms.retain(|t| t.1 != 0.0);
        self
    }
}

/// A 2D point as a pair of expressions.
#[derive(Debug, Clone, PartialEq)]
pub struct PointExpr {
    /// X expression.
    pub x: Expr,
    /// Y expression.
    pub y: Expr,
}

impl PointExpr {
    /// Point from two expressions.
    #[must_use]
    pub const fn new(x: Expr, y: Expr) -> Self {
        Self { x, y }
    }

    /// Point from two variables.
    #[must_use]
    pub const fn vars(x: VarId, y: VarId) -> Self {
        Self { x: Expr::Var(x), y: Expr::Var(y) }
    }

    /// Constant point.
    #[must_use]
    pub const fn constant(x: f64, y: f64) -> Self {
        Self { x: Expr::Const(x), y: Expr::Const(y) }
    }

    /// Apply an affine transform `[a b c d e f]` (SVG convention).
    #[must_use]
    pub fn transformed(&self, m: [f64; 6]) -> Self {
        let [a, b, c, d, e, f] = m;
        if m == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
            return self.clone();
        }
        Self {
            x: Expr::add(
                Expr::add(Expr::mul(Expr::c(a), self.x.clone()), Expr::mul(Expr::c(c), self.y.clone())),
                Expr::c(e),
            ),
            y: Expr::add(
                Expr::add(Expr::mul(Expr::c(b), self.x.clone()), Expr::mul(Expr::c(d), self.y.clone())),
                Expr::c(f),
            ),
        }
    }

    /// Evaluate.
    #[must_use]
    pub fn eval(&self, x: &[f64]) -> (f64, f64) {
        (self.x.eval(x), self.y.eval(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(i: u32) -> Expr {
        Expr::Var(VarId(i))
    }

    /// Central-difference gradient for validation.
    fn numeric_grad(e: &Expr, x: &[f64]) -> Vec<f64> {
        (0..x.len())
            .map(|i| {
                let h = 1e-6 * (1.0 + x[i].abs());
                let mut xp = x.to_vec();
                let mut xm = x.to_vec();
                xp[i] += h;
                xm[i] -= h;
                (e.eval(&xp) - e.eval(&xm)) / (2.0 * h)
            })
            .collect()
    }

    #[test]
    fn dual_matches_central_differences() {
        let e = Expr::add(
            Expr::mul(Expr::sin(v(0)), Expr::hypot(v(1), v(2))),
            Expr::div(Expr::atan2(v(2), Expr::sub(v(0), Expr::c(3.0))), Expr::add(Expr::sqrt(v(1)), Expr::c(2.0))),
        );
        let x = [0.7, 2.3, -1.1];
        let d = e.eval_dual(&x);
        assert!((d.v - e.eval(&x)).abs() < 1e-15);
        let num = numeric_grad(&e, &x);
        for (i, n) in num.iter().enumerate() {
            let a = d.g.iter().find(|g| g.0 == i as u32).map_or(0.0, |g| g.1);
            assert!((a - n).abs() < 1e-7 * (1.0 + n.abs()), "var {i}: {a} vs {n}");
        }
    }

    #[test]
    fn linear_form_detection() {
        let e = Expr::sub(Expr::add(Expr::mul(Expr::c(2.0), v(0)), v(1)), Expr::div(v(2), Expr::c(4.0)));
        let lf = e.linear_form(&[0.0; 3], &|_| false).unwrap();
        assert_eq!(lf.terms, vec![(VarId(0), 2.0), (VarId(1), 1.0), (VarId(2), -0.25)]);
        let nl = Expr::mul(v(0), v(1));
        assert!(nl.linear_form(&[1.0, 2.0], &|_| false).is_none());
        // Becomes linear when one factor is fixed.
        let lf2 = nl.linear_form(&[1.0, 2.0], &|id| id == VarId(1)).unwrap();
        assert_eq!(lf2.terms, vec![(VarId(0), 2.0)]);
        // hypot of fixed vars folds to a constant.
        let h = Expr::hypot(v(0), v(1));
        assert_eq!(h.linear_form(&[3.0, 4.0], &|_| true).unwrap().constant, 5.0);
    }

    #[test]
    fn simplification() {
        assert_eq!(Expr::add(Expr::c(1.0), Expr::c(2.0)), Expr::c(3.0));
        assert_eq!(Expr::mul(Expr::c(1.0), v(3)), v(3));
        assert_eq!(Expr::mul(Expr::c(0.0), v(3)), Expr::c(0.0));
        assert_eq!(Expr::neg(Expr::neg(v(1))), v(1));
    }

    #[test]
    fn non_smooth_points_are_deterministic() {
        let h = Expr::hypot(v(0), v(1)).eval_dual(&[0.0, 0.0]);
        assert_eq!(h.v, 0.0);
        assert_eq!(h.g, vec![(0, 1.0)]);
        let s = Expr::sqrt(v(0)).eval_dual(&[0.0]);
        assert_eq!(s.g, vec![(0, 0.0)]);
    }
}
