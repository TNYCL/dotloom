//! Elementary curve pieces: line segments, circles, arcs and Bézier curves.

use core::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

use crate::{
    Aabb, Affine, GeoResult, GeometryError, LinearKind, Orientation, Point, Vector, error::finite, normalize_angle,
    orientation,
};

/// Relative tolerance used to classify transforms as similarities.
pub(crate) const SIMILARITY_REL: f64 = 1e-9;
/// Recursion cap for adaptive subdivision (2^16 pieces per curve at most).
const MAX_DEPTH: u32 = 16;

/// A straight line segment from `a` to `b`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Start point.
    pub a: Point,
    /// End point.
    pub b: Point,
}

impl Segment {
    /// Create a segment.
    #[must_use]
    pub const fn new(a: Point, b: Point) -> Self {
        Self { a, b }
    }

    /// Vector `b - a`.
    #[must_use]
    pub fn vector(self) -> Vector {
        self.b - self.a
    }

    /// Length.
    #[must_use]
    pub fn length(self) -> f64 {
        self.a.distance(self.b)
    }

    /// Unit direction, `None` when degenerate.
    #[must_use]
    pub fn direction(self) -> Option<Vector> {
        self.vector().normalize()
    }

    /// Point at parameter `t` (`0 → a`, `1 → b`).
    #[must_use]
    pub fn point_at(self, t: f64) -> Point {
        self.a.lerp(self.b, t)
    }

    /// Midpoint.
    #[must_use]
    pub fn midpoint(self) -> Point {
        self.a.midpoint(self.b)
    }

    /// Unclamped parameter of the orthogonal projection of `p` on the line.
    /// Degenerate segments return `0`.
    #[must_use]
    pub fn line_param(self, p: Point) -> f64 {
        let v = self.vector();
        let len2 = v.length_sq();
        if len2 > 0.0 && len2.is_finite() { (p - self.a).dot(v) / len2 } else { 0.0 }
    }

    /// Closest point on the segment and its parameter.
    #[must_use]
    pub fn closest(self, p: Point) -> (f64, Point) {
        let t = self.line_param(p).clamp(0.0, 1.0);
        (t, self.point_at(t))
    }

    /// Distance from `p` to the segment.
    #[must_use]
    pub fn distance_to_point(self, p: Point) -> f64 {
        self.closest(p).1.distance(p)
    }

    /// Bounding box.
    #[must_use]
    pub fn bbox(self) -> Aabb {
        Aabb::from_corners(self.a, self.b)
    }

    /// Reversed segment.
    #[must_use]
    pub const fn reversed(self) -> Self {
        Self::new(self.b, self.a)
    }

    /// Transformed segment (any affine transform is exact for segments).
    #[must_use]
    pub fn transform(self, t: Affine) -> Self {
        Self::new(t.apply(self.a), t.apply(self.b))
    }
}

/// A full circle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    /// Center.
    pub center: Point,
    /// Radius (> 0 for a valid circle).
    pub radius: f64,
}

impl Circle {
    /// Create a validated circle.
    pub fn new(center: Point, radius: f64) -> GeoResult<Self> {
        if !center.is_finite() {
            return Err(GeometryError::NonFinite("circle center"));
        }
        finite(radius, "circle radius")?;
        if radius <= 0.0 {
            return Err(GeometryError::Degenerate("circle radius must be > 0"));
        }
        Ok(Self { center, radius })
    }

    /// Point at angle `a` (radians).
    #[must_use]
    pub fn point_at_angle(self, a: f64) -> Point {
        self.center + Vector::from_angle(a) * self.radius
    }

    /// Circumference.
    #[must_use]
    pub fn length(self) -> f64 {
        TAU * self.radius
    }

    /// Bounding box.
    #[must_use]
    pub fn bbox(self) -> Aabb {
        Aabb::from_corners(self.center, self.center).inflate(self.radius.abs())
    }

    /// Closest point on the circle; the center maps to angle 0.
    #[must_use]
    pub fn closest(self, p: Point) -> (f64, Point) {
        let a = match (p - self.center).normalize() {
            Some(v) => normalize_angle(v.angle()),
            None => 0.0,
        };
        (a, self.point_at_angle(a))
    }

    /// Distance from `p` to the circle line.
    #[must_use]
    pub fn distance_to_point(self, p: Point) -> f64 {
        (p.distance(self.center) - self.radius).abs()
    }

    /// Apply a transform; only similarities keep a circle a circle.
    pub fn transform(self, t: Affine) -> GeoResult<Self> {
        match t.linear_kind(SIMILARITY_REL) {
            LinearKind::Similarity { scale, .. } => Self::new(t.apply(self.center), self.radius * scale),
            LinearKind::General => Err(GeometryError::UnsupportedTransform {
                shape: "circle",
                reason: "non-uniform scale or shear turns a circle into an ellipse",
            }),
            LinearKind::Singular => Err(GeometryError::SingularTransform { determinant: t.determinant() }),
        }
    }
}

/// A circular arc: `center + radius·(cos θ, sin θ)` for `θ = start + sweep·t`, `t ∈ [0, 1]`.
///
/// `sweep > 0` is counter-clockwise. `|sweep| ≤ 2π`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Arc {
    /// Center.
    pub center: Point,
    /// Radius (> 0).
    pub radius: f64,
    /// Start angle in radians.
    pub start: f64,
    /// Signed sweep in radians.
    pub sweep: f64,
}

impl Arc {
    /// Create a validated arc.
    pub fn new(center: Point, radius: f64, start: f64, sweep: f64) -> GeoResult<Self> {
        if !center.is_finite() {
            return Err(GeometryError::NonFinite("arc center"));
        }
        finite(radius, "arc radius")?;
        finite(start, "arc start angle")?;
        finite(sweep, "arc sweep")?;
        if radius <= 0.0 {
            return Err(GeometryError::Degenerate("arc radius must be > 0"));
        }
        if sweep == 0.0 {
            return Err(GeometryError::Degenerate("arc sweep must be non-zero"));
        }
        Ok(Self { center, radius, start, sweep: sweep.clamp(-TAU, TAU) })
    }

    /// Arc through three points `a → m → b`.
    pub fn from_three_points(a: Point, m: Point, b: Point) -> GeoResult<Self> {
        if !(a.is_finite() && m.is_finite() && b.is_finite()) {
            return Err(GeometryError::NonFinite("arc points"));
        }
        let o = orientation(a, m, b);
        if o == Orientation::Collinear {
            return Err(GeometryError::Degenerate("arc points are collinear"));
        }
        let center = circumcenter(a, m, b).ok_or(GeometryError::Degenerate("arc points are collinear"))?;
        let radius = center.distance(a);
        let sa = (a - center).angle();
        let sb = (b - center).angle();
        let ccw = o == Orientation::CounterClockwise;
        let mut sweep = normalize_angle(sb - sa);
        if !ccw {
            sweep -= TAU;
        }
        if sweep == 0.0 {
            return Err(GeometryError::Degenerate("arc start and end coincide"));
        }
        Self::new(center, radius, sa, sweep)
    }

    /// Arc between `p0` and `p1` with DXF-style bulge (`tan(sweep/4)`, positive = CCW).
    pub fn from_bulge(p0: Point, p1: Point, bulge: f64) -> GeoResult<Self> {
        finite(bulge, "bulge")?;
        if bulge == 0.0 {
            return Err(GeometryError::Degenerate("zero bulge is a straight segment"));
        }
        let chord = p1 - p0;
        let c = chord.length();
        if c == 0.0 || !c.is_finite() {
            return Err(GeometryError::Degenerate("bulge arc with coincident endpoints"));
        }
        let sweep = 4.0 * bulge.atan();
        let left = chord.perp() / c;
        let h = (c * 0.5) * (1.0 - bulge * bulge) / (2.0 * bulge);
        let center = p0.midpoint(p1) + left * h;
        let radius = center.distance(p0);
        Self::new(center, radius, (p0 - center).angle(), sweep)
    }

    /// Bulge value of this arc (`tan(sweep/4)`).
    #[must_use]
    pub fn bulge(self) -> f64 {
        (self.sweep / 4.0).tan()
    }

    /// End angle (`start + sweep`).
    #[must_use]
    pub fn end_angle(self) -> f64 {
        self.start + self.sweep
    }

    /// Point at parameter `t`.
    #[must_use]
    pub fn point_at(self, t: f64) -> Point {
        self.center + Vector::from_angle(self.start + self.sweep * t) * self.radius
    }

    /// Start point.
    #[must_use]
    pub fn start_point(self) -> Point {
        self.point_at(0.0)
    }

    /// End point.
    #[must_use]
    pub fn end_point(self) -> Point {
        self.point_at(1.0)
    }

    /// Midpoint along the arc.
    #[must_use]
    pub fn mid_point(self) -> Point {
        self.point_at(0.5)
    }

    /// Unit tangent at `t` in the direction of travel.
    #[must_use]
    pub fn tangent_at(self, t: f64) -> Vector {
        let v = Vector::from_angle(self.start + self.sweep * t).perp();
        if self.sweep >= 0.0 { v } else { -v }
    }

    /// Arc length.
    #[must_use]
    pub fn length(self) -> f64 {
        self.radius * self.sweep.abs()
    }

    /// Parameter for absolute angle `a` if it lies on the arc (with `eps` radians slack).
    #[must_use]
    pub fn param_of_angle(self, a: f64, eps: f64) -> Option<f64> {
        let d = if self.sweep >= 0.0 { normalize_angle(a - self.start) } else { normalize_angle(self.start - a) };
        let span = self.sweep.abs();
        if d <= span + eps {
            Some((d / span).min(1.0))
        } else if TAU - d <= eps {
            Some(0.0)
        } else {
            None
        }
    }

    /// Closest point on the arc and its parameter.
    #[must_use]
    pub fn closest(self, p: Point) -> (f64, Point) {
        if let Some(v) = (p - self.center).normalize()
            && let Some(t) = self.param_of_angle(v.angle(), 0.0)
        {
            return (t, self.point_at(t));
        }
        let (s, e) = (self.start_point(), self.end_point());
        if p.distance_sq(s) <= p.distance_sq(e) { (0.0, s) } else { (1.0, e) }
    }

    /// Distance from `p` to the arc.
    #[must_use]
    pub fn distance_to_point(self, p: Point) -> f64 {
        self.closest(p).1.distance(p)
    }

    /// Exact bounding box.
    #[must_use]
    pub fn bbox(self) -> Aabb {
        let mut b = Aabb::from_corners(self.start_point(), self.end_point());
        for k in 0..4 {
            let a = f64::from(k) * PI * 0.5;
            if self.param_of_angle(a, 0.0).is_some() {
                b = b.include(self.center + Vector::from_angle(a) * self.radius);
            }
        }
        b
    }

    /// Reversed arc (same points, opposite direction).
    #[must_use]
    pub fn reversed(self) -> Self {
        Self { start: self.start + self.sweep, sweep: -self.sweep, ..self }
    }

    /// Sub-arc between parameters `t0` and `t1`.
    #[must_use]
    pub fn subarc(self, t0: f64, t1: f64) -> Self {
        Self { start: self.start + self.sweep * t0, sweep: self.sweep * (t1 - t0), ..self }
    }

    /// Apply a similarity transform.
    pub fn transform(self, t: Affine) -> GeoResult<Self> {
        match t.linear_kind(SIMILARITY_REL) {
            LinearKind::Similarity { scale, reflected } => {
                let start_dir = t.apply_vector(Vector::from_angle(self.start));
                let sweep = if reflected { -self.sweep } else { self.sweep };
                Self::new(t.apply(self.center), self.radius * scale, start_dir.angle(), sweep)
            }
            LinearKind::General => Err(GeometryError::UnsupportedTransform {
                shape: "arc",
                reason: "non-uniform scale or shear turns an arc into an elliptical arc",
            }),
            LinearKind::Singular => Err(GeometryError::SingularTransform { determinant: t.determinant() }),
        }
    }

    /// Append a polyline approximation (excluding the start point) to `out`.
    pub fn flatten_into(self, tol: f64, out: &mut Vec<Point>) {
        let n = arc_segments(self.radius, self.sweep.abs(), tol);
        for i in 1..=n {
            out.push(self.point_at(f64::from(i) / f64::from(n)));
        }
    }
}

/// Number of chords so that the sagitta stays below `tol`.
pub(crate) fn arc_segments(radius: f64, sweep: f64, tol: f64) -> u32 {
    let tol = tol.max(radius * 1e-12).max(1e-12);
    if radius <= tol {
        return 1;
    }
    // sagitta = r (1 - cos(θ/2)) ≤ tol  ⇒  θ ≤ 2 acos(1 - tol/r)
    let max_step = 2.0 * (1.0 - tol / radius).clamp(-1.0, 1.0).acos();
    if max_step <= 0.0 || !max_step.is_finite() {
        return 4096;
    }
    let n = (sweep / max_step).ceil();
    if n.is_finite() { n.clamp(1.0, 4096.0) as u32 } else { 4096 }
}

/// Circumcenter of a triangle, `None` when collinear.
#[must_use]
pub fn circumcenter(a: Point, b: Point, c: Point) -> Option<Point> {
    let ab = b - a;
    let ac = c - a;
    let d = 2.0 * ab.cross(ac);
    if d == 0.0 || !d.is_finite() {
        return None;
    }
    let ab2 = ab.length_sq();
    let ac2 = ac.length_sq();
    let ux = (ac.y * ab2 - ab.y * ac2) / d;
    let uy = (ab.x * ac2 - ac.x * ab2) / d;
    let p = a + Vector::new(ux, uy);
    p.is_finite().then_some(p)
}

/// Quadratic Bézier curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QuadBez {
    /// Start point.
    pub p0: Point,
    /// Control point.
    pub p1: Point,
    /// End point.
    pub p2: Point,
}

/// Cubic Bézier curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CubicBez {
    /// Start point.
    pub p0: Point,
    /// First control point.
    pub p1: Point,
    /// Second control point.
    pub p2: Point,
    /// End point.
    pub p3: Point,
}

impl QuadBez {
    /// Elevate to an exactly equivalent cubic.
    #[must_use]
    pub fn to_cubic(self) -> CubicBez {
        let c1 = self.p0 + (self.p1 - self.p0) * (2.0 / 3.0);
        let c2 = self.p2 + (self.p1 - self.p2) * (2.0 / 3.0);
        CubicBez { p0: self.p0, p1: c1, p2: c2, p3: self.p2 }
    }
}

impl CubicBez {
    /// Evaluate at `t`.
    #[must_use]
    pub fn eval(self, t: f64) -> Point {
        let mt = 1.0 - t;
        let a = mt * mt * mt;
        let b = 3.0 * mt * mt * t;
        let c = 3.0 * mt * t * t;
        let d = t * t * t;
        Point::new(
            a * self.p0.x + b * self.p1.x + c * self.p2.x + d * self.p3.x,
            a * self.p0.y + b * self.p1.y + c * self.p2.y + d * self.p3.y,
        )
    }

    /// First derivative at `t`.
    #[must_use]
    pub fn deriv(self, t: f64) -> Vector {
        let mt = 1.0 - t;
        let d0 = self.p1 - self.p0;
        let d1 = self.p2 - self.p1;
        let d2 = self.p3 - self.p2;
        d0 * (3.0 * mt * mt) + d1 * (6.0 * mt * t) + d2 * (3.0 * t * t)
    }

    /// Second derivative at `t`.
    #[must_use]
    pub fn deriv2(self, t: f64) -> Vector {
        let dd0 = (self.p2 - self.p1) - (self.p1 - self.p0);
        let dd1 = (self.p3 - self.p2) - (self.p2 - self.p1);
        dd0 * (6.0 * (1.0 - t)) + dd1 * (6.0 * t)
    }

    /// Split at `t` (de Casteljau).
    #[must_use]
    pub fn split(self, t: f64) -> (Self, Self) {
        let p01 = self.p0.lerp(self.p1, t);
        let p12 = self.p1.lerp(self.p2, t);
        let p23 = self.p2.lerp(self.p3, t);
        let p012 = p01.lerp(p12, t);
        let p123 = p12.lerp(p23, t);
        let m = p012.lerp(p123, t);
        (Self { p0: self.p0, p1: p01, p2: p012, p3: m }, Self { p0: m, p1: p123, p2: p23, p3: self.p3 })
    }

    /// Sub-curve between `t0` and `t1`.
    #[must_use]
    pub fn subsegment(self, t0: f64, t1: f64) -> Self {
        if t1 <= t0 {
            return Self { p0: self.eval(t0), p1: self.eval(t0), p2: self.eval(t0), p3: self.eval(t0) };
        }
        let (_, right) = self.split(t0);
        let u = if t0 < 1.0 { (t1 - t0) / (1.0 - t0) } else { 1.0 };
        right.split(u.clamp(0.0, 1.0)).0
    }

    /// Exact bounding box (extrema of the derivative).
    #[must_use]
    pub fn bbox(self) -> Aabb {
        let mut b = Aabb::from_corners(self.p0, self.p3);
        let roots_x = deriv_roots(self.p0.x, self.p1.x, self.p2.x, self.p3.x);
        let roots_y = deriv_roots(self.p0.y, self.p1.y, self.p2.y, self.p3.y);
        for t in roots_x.into_iter().chain(roots_y).flatten() {
            if (0.0..=1.0).contains(&t) {
                b = b.include(self.eval(t));
            }
        }
        b
    }

    /// Bounding box of the control polygon (cheap, conservative).
    #[must_use]
    pub fn hull_bbox(self) -> Aabb {
        Aabb::from_points([self.p0, self.p1, self.p2, self.p3])
    }

    /// Maximum distance of the control points from the chord (flatness bound).
    #[must_use]
    pub fn flatness(self) -> f64 {
        let chord = Segment::new(self.p0, self.p3);
        chord.distance_to_point(self.p1).max(chord.distance_to_point(self.p2))
    }

    /// Append a polyline approximation (excluding the start point) to `out`.
    pub fn flatten_into(self, tol: f64, out: &mut Vec<Point>) {
        let tol = tol.max(1e-12);
        flatten_rec(self, tol, 0, out);
    }

    /// Arc length with adaptive Gauss–Legendre quadrature (relative accuracy ≈ `rel`).
    #[must_use]
    pub fn length(self, rel: f64) -> f64 {
        length_rec(self, gauss_len(self), rel.max(1e-14), 0)
    }

    /// Closest point (parameter, point).
    ///
    /// Global search by branch-and-bound subdivision (the control-polygon box is a
    /// lower bound for every sub-curve), followed by Newton polishing. This cannot
    /// get trapped in a local minimum the way pure sampling + Newton can.
    #[must_use]
    pub fn closest(self, p: Point) -> (f64, Point) {
        let size = self.hull_bbox().size().length();
        let eps = (size * 1e-12).max(1e-300);
        let mut best = (f64::INFINITY, 0.0);
        for (t, q) in [(0.0, self.p0), (1.0, self.p3)] {
            let d = q.distance_sq(p);
            if d < best.0 {
                best = (d, t);
            }
        }
        closest_bb(self, 0.0, 1.0, p, eps, 0, &mut best);
        let (best_d, best_t) = best;
        let mut t = best_t;
        for _ in 0..12 {
            let b = self.eval(t);
            let d1 = self.deriv(t);
            let d2 = self.deriv2(t);
            let f = (b - p).dot(d1);
            let fp = d1.dot(d1) + (b - p).dot(d2);
            if fp.abs() < 1e-300 || !fp.is_finite() {
                break;
            }
            let nt = (t - f / fp).clamp(0.0, 1.0);
            if (nt - t).abs() < 1e-15 {
                t = nt;
                break;
            }
            t = nt;
        }
        let refined = self.eval(t);
        if refined.distance_sq(p) <= best_d { (t, refined) } else { (best_t, self.eval(best_t)) }
    }

    /// Transform (any affine transform is exact for Bézier curves).
    #[must_use]
    pub fn transform(self, t: Affine) -> Self {
        Self { p0: t.apply(self.p0), p1: t.apply(self.p1), p2: t.apply(self.p2), p3: t.apply(self.p3) }
    }

    /// Reversed curve.
    #[must_use]
    pub const fn reversed(self) -> Self {
        Self { p0: self.p3, p1: self.p2, p2: self.p1, p3: self.p0 }
    }
}

fn closest_bb(c: CubicBez, t0: f64, t1: f64, p: Point, eps: f64, depth: u32, best: &mut (f64, f64)) {
    let lb = c.hull_bbox().distance_to_point(p);
    if lb * lb > best.0 {
        return;
    }
    if depth >= 48 || c.flatness() <= eps {
        let chord = Segment::new(c.p0, c.p3);
        let (u, _) = chord.closest(p);
        let t = t0 + (t1 - t0) * u;
        let q = c.eval(u);
        let d = q.distance_sq(p);
        if d < best.0 {
            *best = (d, t);
        }
        return;
    }
    let (l, r) = c.split(0.5);
    let tm = 0.5 * (t0 + t1);
    let mid = l.p3.distance_sq(p);
    if mid < best.0 {
        *best = (mid, tm);
    }
    // Visit the nearer half first for better pruning.
    if l.hull_bbox().distance_to_point(p) <= r.hull_bbox().distance_to_point(p) {
        closest_bb(l, t0, tm, p, eps, depth + 1, best);
        closest_bb(r, tm, t1, p, eps, depth + 1, best);
    } else {
        closest_bb(r, tm, t1, p, eps, depth + 1, best);
        closest_bb(l, t0, tm, p, eps, depth + 1, best);
    }
}

fn flatten_rec(c: CubicBez, tol: f64, depth: u32, out: &mut Vec<Point>) {
    if depth >= MAX_DEPTH || c.flatness() <= tol {
        out.push(c.p3);
        return;
    }
    let (l, r) = c.split(0.5);
    flatten_rec(l, tol, depth + 1, out);
    flatten_rec(r, tol, depth + 1, out);
}

/// Roots in [0,1] of the derivative of a 1D cubic Bézier.
fn deriv_roots(p0: f64, p1: f64, p2: f64, p3: f64) -> [Option<f64>; 2] {
    // B'(t)/3 = a t² + b t + c
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let scale = a.abs().max(b.abs()).max(c.abs());
    if scale == 0.0 {
        return [None, None];
    }
    if a.abs() <= 1e-12 * scale {
        if b.abs() <= 1e-12 * scale {
            return [None, None];
        }
        return [Some(-c / b), None];
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return [None, None];
    }
    let sq = disc.sqrt();
    // Numerically stable quadratic formula.
    let q = -0.5 * (b + b.signum() * sq);
    let r1 = q / a;
    let r2 = if q != 0.0 { c / q } else { -b / (2.0 * a) };
    [Some(r1), Some(r2)]
}

const GL_X: [f64; 8] = [
    -0.960_289_856_497_536_2,
    -0.796_666_477_413_626_7,
    -0.525_532_409_916_329,
    -0.183_434_642_495_649_8,
    0.183_434_642_495_649_8,
    0.525_532_409_916_329,
    0.796_666_477_413_626_7,
    0.960_289_856_497_536_2,
];
const GL_W: [f64; 8] = [
    0.101_228_536_290_376_3,
    0.222_381_034_453_374_5,
    0.313_706_645_877_887_3,
    0.362_683_783_378_362,
    0.362_683_783_378_362,
    0.313_706_645_877_887_3,
    0.222_381_034_453_374_5,
    0.101_228_536_290_376_3,
];

fn gauss_len(c: CubicBez) -> f64 {
    GL_X.iter().zip(GL_W.iter()).map(|(x, w)| w * c.deriv(0.5 * (x + 1.0)).length()).sum::<f64>() * 0.5
}

fn length_rec(c: CubicBez, whole: f64, rel: f64, depth: u32) -> f64 {
    let (l, r) = c.split(0.5);
    let (ll, rl) = (gauss_len(l), gauss_len(r));
    let halves = ll + rl;
    if depth >= 12 || (halves - whole).abs() <= rel * halves.max(1e-300) {
        halves
    } else {
        length_rec(l, ll, rel, depth + 1) + length_rec(r, rl, rel, depth + 1)
    }
}

/// One elementary curve piece. Shapes decompose into these for intersections,
/// trimming, hit-testing and tessellation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Curve {
    /// Straight segment.
    Line(Segment),
    /// Circular arc.
    Arc(Arc),
    /// Full circle (closed, parameter = angle / 2π).
    Circle(Circle),
    /// Cubic Bézier (quadratics are elevated exactly).
    Cubic(CubicBez),
}

impl Curve {
    /// Point at normalized parameter `t ∈ [0,1]`.
    #[must_use]
    pub fn point_at(&self, t: f64) -> Point {
        match *self {
            Self::Line(s) => s.point_at(t),
            Self::Arc(a) => a.point_at(t),
            Self::Circle(c) => c.point_at_angle(t * TAU),
            Self::Cubic(c) => c.eval(t),
        }
    }

    /// Start point (circles start at angle 0).
    #[must_use]
    pub fn start(&self) -> Point {
        self.point_at(0.0)
    }

    /// End point.
    #[must_use]
    pub fn end(&self) -> Point {
        self.point_at(1.0)
    }

    /// Whether the curve is closed on itself.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Circle(_))
    }

    /// Bounding box.
    #[must_use]
    pub fn bbox(&self) -> Aabb {
        match *self {
            Self::Line(s) => s.bbox(),
            Self::Arc(a) => a.bbox(),
            Self::Circle(c) => c.bbox(),
            Self::Cubic(c) => c.bbox(),
        }
    }

    /// Closest point `(t, point)`.
    #[must_use]
    pub fn closest(&self, p: Point) -> (f64, Point) {
        match *self {
            Self::Line(s) => s.closest(p),
            Self::Arc(a) => a.closest(p),
            Self::Circle(c) => {
                let (a, q) = c.closest(p);
                (a / TAU, q)
            }
            Self::Cubic(c) => c.closest(p),
        }
    }

    /// Distance to `p`.
    #[must_use]
    pub fn distance_to_point(&self, p: Point) -> f64 {
        self.closest(p).1.distance(p)
    }

    /// Length.
    #[must_use]
    pub fn length(&self) -> f64 {
        match *self {
            Self::Line(s) => s.length(),
            Self::Arc(a) => a.length(),
            Self::Circle(c) => c.length(),
            Self::Cubic(c) => c.length(1e-12),
        }
    }

    /// Append flattened points (excluding the start point) to `out`.
    pub fn flatten_into(&self, tol: f64, out: &mut Vec<Point>) {
        match *self {
            Self::Line(s) => out.push(s.b),
            Self::Arc(a) => a.flatten_into(tol, out),
            Self::Circle(c) => {
                if let Ok(a) = Arc::new(c.center, c.radius, 0.0, TAU) {
                    a.flatten_into(tol, out);
                }
            }
            Self::Cubic(c) => c.flatten_into(tol, out),
        }
    }

    /// Unit tangent at `t` (None where the derivative vanishes).
    #[must_use]
    pub fn tangent_at(&self, t: f64) -> Option<Vector> {
        match *self {
            Self::Line(s) => s.direction(),
            Self::Arc(a) => Some(a.tangent_at(t)),
            Self::Circle(_) => Some(Vector::from_angle(t * TAU).perp()),
            Self::Cubic(c) => c.deriv(t).normalize(),
        }
    }

    /// Kind name for diagnostics.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Line(_) => "line",
            Self::Arc(_) => "arc",
            Self::Circle(_) => "circle",
            Self::Cubic(_) => "cubic",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_from_three_points_ccw_and_cw() {
        let a = Arc::from_three_points(Point::new(1.0, 0.0), Point::new(0.0, 1.0), Point::new(-1.0, 0.0)).unwrap();
        assert!(a.center.distance(Point::ORIGIN) < 1e-12);
        assert!((a.radius - 1.0).abs() < 1e-12);
        assert!((a.sweep - PI).abs() < 1e-12);
        let b = Arc::from_three_points(Point::new(1.0, 0.0), Point::new(0.0, -1.0), Point::new(-1.0, 0.0)).unwrap();
        assert!((b.sweep + PI).abs() < 1e-12);
        assert!(b.mid_point().distance(Point::new(0.0, -1.0)) < 1e-12);
        assert!(Arc::from_three_points(Point::ORIGIN, Point::new(1.0, 1.0), Point::new(2.0, 2.0)).is_err());
    }

    #[test]
    fn bulge_roundtrip() {
        for bulge in [0.25, 1.0, -0.5, 2.0, -3.0] {
            let p0 = Point::new(2.0, 1.0);
            let p1 = Point::new(5.0, -3.0);
            let a = Arc::from_bulge(p0, p1, bulge).unwrap();
            assert!(a.start_point().distance(p0) < 1e-9, "bulge {bulge}");
            assert!(a.end_point().distance(p1) < 1e-9, "bulge {bulge}");
            assert!((a.bulge() - bulge).abs() < 1e-9);
        }
        // Semicircle: bulge 1, center at chord midpoint.
        let s = Arc::from_bulge(Point::ORIGIN, Point::new(2.0, 0.0), 1.0).unwrap();
        assert!(s.center.distance(Point::new(1.0, 0.0)) < 1e-12);
        // CCW from (0,0) to (2,0) goes below the chord.
        assert!(s.mid_point().y < 0.0);
    }

    #[test]
    fn arc_bbox_includes_extrema() {
        let a = Arc::new(Point::ORIGIN, 1.0, -0.1, 0.2 + PI / 2.0).unwrap();
        let b = a.bbox();
        assert!((b.max.x - 1.0).abs() < 1e-12);
        assert!((b.max.y - 1.0).abs() < 1e-12);
    }

    #[test]
    fn circle_rejects_nonuniform_scale() {
        let c = Circle::new(Point::ORIGIN, 2.0).unwrap();
        assert!(matches!(c.transform(Affine::scale(2.0, 1.0)), Err(GeometryError::UnsupportedTransform { .. })));
        let r = c.transform(Affine::scale(3.0, 3.0)).unwrap();
        assert!((r.radius - 6.0).abs() < 1e-12);
    }

    #[test]
    fn arc_reflection_flips_sweep() {
        let a = Arc::new(Point::ORIGIN, 1.0, 0.0, PI / 2.0).unwrap();
        let m = Affine::mirror(Point::ORIGIN, Vector::new(1.0, 0.0)).unwrap();
        let r = a.transform(m).unwrap();
        assert!(r.end_point().distance(Point::new(0.0, -1.0)) < 1e-12);
        assert!(r.sweep < 0.0);
    }

    #[test]
    fn cubic_length_of_straight_line() {
        let c = CubicBez {
            p0: Point::ORIGIN,
            p1: Point::new(1.0, 0.0),
            p2: Point::new(2.0, 0.0),
            p3: Point::new(3.0, 0.0),
        };
        assert!((c.length(1e-12) - 3.0).abs() < 1e-10);
    }

    #[test]
    fn quarter_circle_cubic_length() {
        // Standard approximation constant; true arc length differs by < 1e-3.
        let k = 0.552_284_749_830_793_4;
        let c = CubicBez {
            p0: Point::new(1.0, 0.0),
            p1: Point::new(1.0, k),
            p2: Point::new(k, 1.0),
            p3: Point::new(0.0, 1.0),
        };
        assert!((c.length(1e-12) - PI / 2.0).abs() < 1e-3);
    }

    #[test]
    fn cubic_closest_matches_dense_sampling() {
        let c = CubicBez {
            p0: Point::ORIGIN,
            p1: Point::new(1.0, 3.0),
            p2: Point::new(4.0, -2.0),
            p3: Point::new(5.0, 1.0),
        };
        let p = Point::new(2.3, 1.7);
        let (_, q) = c.closest(p);
        let mut best = f64::INFINITY;
        for i in 0..=100_000 {
            best = best.min(c.eval(f64::from(i) / 100_000.0).distance(p));
        }
        assert!(q.distance(p) <= best + 1e-9);
    }

    #[test]
    fn flatten_respects_tolerance() {
        let a = Arc::new(Point::ORIGIN, 100.0, 0.0, PI).unwrap();
        let mut pts = vec![a.start_point()];
        a.flatten_into(0.01, &mut pts);
        for w in pts.windows(2) {
            let mid = w[0].midpoint(w[1]);
            assert!(100.0 - mid.distance(Point::ORIGIN) <= 0.01 + 1e-12);
        }
    }
}
