//! Residual builders for the supported rule classes.
//!
//! Linear rules: fix, equal, linear relation (sum/difference/ratio/min/max with
//! constant coefficients), equal spacing.
//!
//! Geometric rules: coincident, horizontal, vertical, fixed point, distance
//! (point–point, signed point–line), point on line/circle, length, equal length,
//! parallel, perpendicular, angle, concentric, radius, equal radius, line–circle and
//! circle–circle tangency.
//!
//! Every builder returns residual rows; the solver decides linearity from the
//! expressions themselves. Residuals of direction rules are normalized (sine/cosine
//! of angles) so their scale is 1 regardless of segment length.

use crate::{Expr, PointExpr, Relation, Row, VarId};

fn eq(expr: Expr, scale: f64) -> Row {
    Row { expr, relation: Relation::Eq, scale }
}

fn le(expr: Expr, scale: f64) -> Row {
    Row { expr, relation: Relation::Le, scale }
}

/// Linear comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cmp {
    /// `=`.
    Eq,
    /// `≤`.
    Le,
    /// `≥`.
    Ge,
}

/// `v = value`.
#[must_use]
pub fn fix(v: Expr, value: f64, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(v, Expr::c(value)), scale)]
}

/// `a = b`.
#[must_use]
pub fn equal(a: Expr, b: Expr, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(a, b), scale)]
}

/// `Σ cᵢ·eᵢ (cmp) rhs`.
#[must_use]
pub fn linear(terms: &[(f64, Expr)], cmp: Cmp, rhs: f64, scale: f64) -> Vec<Row> {
    let sum = terms.iter().fold(Expr::c(0.0), |acc, (c, e)| Expr::add(acc, Expr::mul(Expr::c(*c), e.clone())));
    let lhs = Expr::sub(sum, Expr::c(rhs));
    match cmp {
        Cmp::Eq => vec![eq(lhs, scale)],
        Cmp::Le => vec![le(lhs, scale)],
        Cmp::Ge => vec![le(Expr::neg(lhs), scale)],
    }
}

/// `a = k · b` (constant ratio).
#[must_use]
pub fn ratio(a: Expr, b: Expr, k: f64, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(a, Expr::mul(Expr::c(k), b)), scale)]
}

/// `a ≥ min`.
#[must_use]
pub fn at_least(a: Expr, min: f64, scale: f64) -> Vec<Row> {
    vec![le(Expr::sub(Expr::c(min), a), scale)]
}

/// `a ≤ max`.
#[must_use]
pub fn at_most(a: Expr, max: f64, scale: f64) -> Vec<Row> {
    vec![le(Expr::sub(a, Expr::c(max)), scale)]
}

/// Consecutive differences are equal: `v₁−v₀ = v₂−v₁ = …`.
#[must_use]
pub fn equal_spacing(values: &[Expr], scale: f64) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut iter = values.windows(2);
    let Some(first) = iter.next() else { return rows };
    let d0 = Expr::sub(first[1].clone(), first[0].clone());
    for w in iter {
        rows.push(eq(Expr::sub(Expr::sub(w[1].clone(), w[0].clone()), d0.clone()), scale));
    }
    rows
}

/// Equal values: `v₀ = v₁ = …`.
#[must_use]
pub fn all_equal(values: &[Expr], scale: f64) -> Vec<Row> {
    values.windows(2).map(|w| eq(Expr::sub(w[1].clone(), w[0].clone()), scale)).collect()
}

/// Coincident points.
#[must_use]
pub fn coincident(p: &PointExpr, q: &PointExpr, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(p.x.clone(), q.x.clone()), scale), eq(Expr::sub(p.y.clone(), q.y.clone()), scale)]
}

/// Same Y.
#[must_use]
pub fn horizontal(p: &PointExpr, q: &PointExpr, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(p.y.clone(), q.y.clone()), scale)]
}

/// Same X.
#[must_use]
pub fn vertical(p: &PointExpr, q: &PointExpr, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(p.x.clone(), q.x.clone()), scale)]
}

/// Point fixed at a location.
#[must_use]
pub fn fix_point(p: &PointExpr, at: (f64, f64), scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(p.x.clone(), Expr::c(at.0)), scale), eq(Expr::sub(p.y.clone(), Expr::c(at.1)), scale)]
}

fn dx(p: &PointExpr, q: &PointExpr) -> Expr {
    Expr::sub(q.x.clone(), p.x.clone())
}

fn dy(p: &PointExpr, q: &PointExpr) -> Expr {
    Expr::sub(q.y.clone(), p.y.clone())
}

fn dist(p: &PointExpr, q: &PointExpr) -> Expr {
    Expr::hypot(dx(p, q), dy(p, q))
}

/// `|q − p| = d`.
#[must_use]
pub fn distance(p: &PointExpr, q: &PointExpr, d: f64, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(dist(p, q), Expr::c(d)), scale)]
}

/// `|q − p| (cmp) d`.
#[must_use]
pub fn distance_cmp(p: &PointExpr, q: &PointExpr, cmp: Cmp, d: f64, scale: f64) -> Vec<Row> {
    let r = Expr::sub(dist(p, q), Expr::c(d));
    match cmp {
        Cmp::Eq => vec![eq(r, scale)],
        Cmp::Le => vec![le(r, scale)],
        Cmp::Ge => vec![le(Expr::neg(r), scale)],
    }
}

/// Signed distance from `p` to the line `a → b` (positive on the left).
#[must_use]
pub fn signed_line_distance(p: &PointExpr, a: &PointExpr, b: &PointExpr) -> Expr {
    // cross(b − a, p − a) / |b − a|
    let cross = Expr::sub(Expr::mul(dx(a, b), dy(a, p)), Expr::mul(dy(a, b), dx(a, p)));
    Expr::div(cross, dist(a, b))
}

/// Signed point–line distance equals `d` (sign selects the side).
#[must_use]
pub fn point_line_distance(p: &PointExpr, a: &PointExpr, b: &PointExpr, signed_d: f64, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(signed_line_distance(p, a, b), Expr::c(signed_d)), scale)]
}

/// Point lies on the infinite line through `a`, `b`.
#[must_use]
pub fn point_on_line(p: &PointExpr, a: &PointExpr, b: &PointExpr, scale: f64) -> Vec<Row> {
    vec![eq(signed_line_distance(p, a, b), scale)]
}

/// Point lies on the circle `(c, r)`.
#[must_use]
pub fn point_on_circle(p: &PointExpr, c: &PointExpr, r: Expr, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(dist(c, p), r), scale)]
}

/// Segment length.
#[must_use]
pub fn length(a: &PointExpr, b: &PointExpr, len: f64, scale: f64) -> Vec<Row> {
    distance(a, b, len, scale)
}

/// Equal segment lengths.
#[must_use]
pub fn equal_length(a1: &PointExpr, b1: &PointExpr, a2: &PointExpr, b2: &PointExpr, scale: f64) -> Vec<Row> {
    vec![eq(Expr::sub(dist(a1, b1), dist(a2, b2)), scale)]
}

fn cross_dot(a1: &PointExpr, b1: &PointExpr, a2: &PointExpr, b2: &PointExpr) -> (Expr, Expr, Expr) {
    let (ux, uy) = (dx(a1, b1), dy(a1, b1));
    let (vx, vy) = (dx(a2, b2), dy(a2, b2));
    let cross = Expr::sub(Expr::mul(ux.clone(), vy.clone()), Expr::mul(uy.clone(), vx.clone()));
    let dot = Expr::add(Expr::mul(ux.clone(), vx.clone()), Expr::mul(uy.clone(), vy.clone()));
    let norm = Expr::mul(Expr::hypot(ux, uy), Expr::hypot(vx, vy));
    (cross, dot, norm)
}

/// Parallel (or anti-parallel) segments: `sin(angle) = 0`.
#[must_use]
pub fn parallel(a1: &PointExpr, b1: &PointExpr, a2: &PointExpr, b2: &PointExpr) -> Vec<Row> {
    let (cross, _, norm) = cross_dot(a1, b1, a2, b2);
    vec![eq(Expr::div(cross, norm), 1.0)]
}

/// Perpendicular segments: `cos(angle) = 0`.
#[must_use]
pub fn perpendicular(a1: &PointExpr, b1: &PointExpr, a2: &PointExpr, b2: &PointExpr) -> Vec<Row> {
    let (_, dot, norm) = cross_dot(a1, b1, a2, b2);
    vec![eq(Expr::div(dot, norm), 1.0)]
}

/// Signed angle from segment 1 to segment 2 equals `theta` (radians). The
/// residual is the wrapped angle difference, smooth everywhere except at ±π.
#[must_use]
pub fn angle(a1: &PointExpr, b1: &PointExpr, a2: &PointExpr, b2: &PointExpr, theta: f64) -> Vec<Row> {
    let (cross, dot, _) = cross_dot(a1, b1, a2, b2);
    let (s, c) = libm::sincos(theta);
    // angle(φ − θ) = atan2(sinφ cosθ − cosφ sinθ, cosφ cosθ + sinφ sinθ), scaled by |u||v|.
    let y = Expr::sub(Expr::mul(cross.clone(), Expr::c(c)), Expr::mul(dot.clone(), Expr::c(s)));
    let x = Expr::add(Expr::mul(dot, Expr::c(c)), Expr::mul(cross, Expr::c(s)));
    vec![eq(Expr::atan2(y, x), 1.0)]
}

/// Concentric circles/arcs.
#[must_use]
pub fn concentric(c1: &PointExpr, c2: &PointExpr, scale: f64) -> Vec<Row> {
    coincident(c1, c2, scale)
}

/// Radius equals `value`.
#[must_use]
pub fn radius(r: Expr, value: f64, scale: f64) -> Vec<Row> {
    fix(r, value, scale)
}

/// Equal radii.
#[must_use]
pub fn equal_radius(r1: Expr, r2: Expr, scale: f64) -> Vec<Row> {
    equal(r1, r2, scale)
}

/// Line `a → b` tangent to circle `(c, r)`; `side` (±1) keeps the circle on its
/// current side of the line.
#[must_use]
pub fn tangent_line_circle(a: &PointExpr, b: &PointExpr, c: &PointExpr, r: Expr, side: f64, scale: f64) -> Vec<Row> {
    let d = signed_line_distance(c, a, b);
    vec![eq(Expr::sub(Expr::mul(Expr::c(side.signum()), d), r), scale)]
}

/// Tangency between two circles.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CircleTangency {
    /// Circles touch from outside: `|c₂ − c₁| = r₁ + r₂`.
    External,
    /// One circle inside the other: `|c₂ − c₁| = sign·(r₁ − r₂)`; `sign` keeps which
    /// circle is the outer one.
    Internal {
        /// +1 when circle 1 is the outer circle, −1 otherwise.
        sign: f64,
    },
}

/// Circle–circle tangency.
#[must_use]
pub fn tangent_circles(
    c1: &PointExpr,
    r1: Expr,
    c2: &PointExpr,
    r2: Expr,
    kind: CircleTangency,
    scale: f64,
) -> Vec<Row> {
    let d = dist(c1, c2);
    let target = match kind {
        CircleTangency::External => Expr::add(r1, r2),
        CircleTangency::Internal { sign } => Expr::mul(Expr::c(sign.signum()), Expr::sub(r1, r2)),
    };
    vec![eq(Expr::sub(d, target), scale)]
}

/// Point expression of an arc/circle point at an angle: `c + r·(cos a, sin a)`.
#[must_use]
pub fn polar_point(cx: Expr, cy: Expr, r: Expr, angle: Expr) -> PointExpr {
    PointExpr {
        x: Expr::add(cx, Expr::mul(r.clone(), Expr::cos(angle.clone()))),
        y: Expr::add(cy, Expr::mul(r, Expr::sin(angle))),
    }
}

/// Convenience: variable expression.
#[must_use]
pub const fn v(id: VarId) -> Expr {
    Expr::Var(id)
}
