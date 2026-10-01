//! Curve/curve intersections.
//!
//! * Segment/segment decisions use exact orientation predicates; the reported point
//!   is computed in floating point.
//! * Line/circle and circle/circle use numerically stable closed forms; near-tangent
//!   cases within the model tolerance report a single tangent point.
//! * Arcs reuse the circle results filtered by their angular span.
//! * Bézier intersections flatten to a fine polyline, intersect chords, then refine
//!   the parameters with Newton iterations on the exact curves.

use core::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::{Arc, Circle, CubicBez, Curve, ModelTolerance, Orientation, Point, Segment, Vector, orientation};

/// One intersection point with the parameters on both curves.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Intersection {
    /// Intersection point.
    pub point: Point,
    /// Parameter on the first curve.
    pub t_a: f64,
    /// Parameter on the second curve.
    pub t_b: f64,
}

/// Result of intersecting two curves.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Intersections {
    /// Isolated intersection points (for overlaps: the overlap endpoints).
    pub points: Vec<Intersection>,
    /// The curves share a stretch of positive length (collinear or co-circular overlap).
    pub overlap: bool,
}

impl Intersections {
    fn none() -> Self {
        Self::default()
    }

    fn swapped(mut self) -> Self {
        for p in &mut self.points {
            core::mem::swap(&mut p.t_a, &mut p.t_b);
        }
        self
    }
}

/// Intersect two curves.
#[must_use]
pub fn intersect(a: &Curve, b: &Curve, tol: ModelTolerance) -> Intersections {
    if !a.bbox().inflate(tol.abs).intersects(b.bbox().inflate(tol.abs)) {
        return Intersections::none();
    }
    let mut r = match (*a, *b) {
        (Curve::Line(s1), Curve::Line(s2)) => segment_segment(s1, s2, tol),
        (Curve::Line(s), Curve::Circle(c)) => line_circle(s, c, tol, Span::Full),
        (Curve::Circle(c), Curve::Line(s)) => line_circle(s, c, tol, Span::Full).swapped(),
        (Curve::Line(s), Curve::Arc(a2)) => line_circle(s, circle_of(a2), tol, Span::Arc(a2)),
        (Curve::Arc(a1), Curve::Line(s)) => line_circle(s, circle_of(a1), tol, Span::Arc(a1)).swapped(),
        (Curve::Circle(c1), Curve::Circle(c2)) => circle_circle(c1, Span::Full, c2, Span::Full, tol),
        (Curve::Circle(c1), Curve::Arc(a2)) => circle_circle(c1, Span::Full, circle_of(a2), Span::Arc(a2), tol),
        (Curve::Arc(a1), Curve::Circle(c2)) => circle_circle(circle_of(a1), Span::Arc(a1), c2, Span::Full, tol),
        (Curve::Arc(a1), Curve::Arc(a2)) => {
            circle_circle(circle_of(a1), Span::Arc(a1), circle_of(a2), Span::Arc(a2), tol)
        }
        (Curve::Cubic(c), other) => cubic_curve(c, &other, tol),
        (other, Curve::Cubic(c)) => cubic_curve(c, &other, tol).swapped(),
    };
    dedup(&mut r.points, tol);
    r
}

#[derive(Clone, Copy)]
enum Span {
    Full,
    Arc(Arc),
}

impl Span {
    /// Parameter for a point at absolute angle `ang` or `None` when outside.
    fn param(self, ang: f64, radius: f64, tol: ModelTolerance) -> Option<f64> {
        match self {
            Self::Full => Some(crate::normalize_angle(ang) / TAU),
            Self::Arc(a) => a.param_of_angle(ang, tol.at_scale(radius) / radius.max(f64::MIN_POSITIVE)),
        }
    }
}

fn circle_of(a: Arc) -> Circle {
    Circle { center: a.center, radius: a.radius }
}

fn segment_segment(s1: Segment, s2: Segment, tol: ModelTolerance) -> Intersections {
    let o1 = orientation(s1.a, s1.b, s2.a);
    let o2 = orientation(s1.a, s1.b, s2.b);
    let o3 = orientation(s2.a, s2.b, s1.a);
    let o4 = orientation(s2.a, s2.b, s1.b);

    let d1 = s1.vector();
    let d2 = s2.vector();
    let l1 = d1.length_sq();
    let l2 = d2.length_sq();

    if o1 == Orientation::Collinear && o2 == Orientation::Collinear {
        // Collinear (or degenerate) segments: compute the overlap on s1's parameter.
        if l1 == 0.0 && l2 == 0.0 {
            return if s1.a.distance(s2.a) <= tol.abs { single(s1.a, 0.0, 0.0) } else { Intersections::none() };
        }
        if l1 == 0.0 {
            let t = s2.line_param(s1.a);
            return if (0.0..=1.0).contains(&t) { single(s1.a, 0.0, t) } else { Intersections::none() };
        }
        let ta = s1.line_param(s2.a);
        let tb = s1.line_param(s2.b);
        let lo = ta.min(tb).max(0.0);
        let hi = ta.max(tb).min(1.0);
        let eps = tol.at_scale(l1.sqrt()) / l1.sqrt();
        if hi < lo - eps {
            return Intersections::none();
        }
        let param_on_2 = |p: Point| s2.line_param(p).clamp(0.0, 1.0);
        if hi - lo <= eps {
            let p = s1.point_at(lo.clamp(0.0, 1.0));
            return single(p, lo.clamp(0.0, 1.0), param_on_2(p));
        }
        let p_lo = s1.point_at(lo);
        let p_hi = s1.point_at(hi);
        return Intersections {
            points: vec![
                Intersection { point: p_lo, t_a: lo, t_b: param_on_2(p_lo) },
                Intersection { point: p_hi, t_a: hi, t_b: param_on_2(p_hi) },
            ],
            overlap: true,
        };
    }

    let crosses = o1 != o2 && o3 != o4;
    if !crosses {
        // Near misses within tolerance (endpoint touching a segment).
        return near_touch(s1, s2, tol);
    }
    let denom = d1.cross(d2);
    if denom == 0.0 {
        return near_touch(s1, s2, tol);
    }
    let w = s2.a - s1.a;
    let t = (w.cross(d2) / denom).clamp(0.0, 1.0);
    let u = (w.cross(d1) / denom).clamp(0.0, 1.0);
    // Average both evaluations for a symmetric result.
    let p = s1.point_at(t).midpoint(s2.point_at(u));
    single(p, t, u)
}

fn near_touch(s1: Segment, s2: Segment, tol: ModelTolerance) -> Intersections {
    let mut out = Intersections::none();
    for (p, on_first) in [(s1.a, true), (s1.b, true), (s2.a, false), (s2.b, false)] {
        let (other, own) = if on_first { (s2, s1) } else { (s1, s2) };
        let (t_other, q) = other.closest(p);
        if q.distance(p) <= tol.at_scale(p.to_vector().length()) {
            let t_own = own.line_param(p).clamp(0.0, 1.0);
            let (t_a, t_b) = if on_first { (t_own, t_other) } else { (t_other, t_own) };
            out.points.push(Intersection { point: p, t_a, t_b });
        }
    }
    out
}

fn single(point: Point, t_a: f64, t_b: f64) -> Intersections {
    Intersections { points: vec![Intersection { point, t_a, t_b }], overlap: false }
}

/// Intersections of the *infinite* line through `s` with a circle: `(line_t, angle)`.
pub(crate) fn line_circle_raw(s: Segment, c: Circle, tol: ModelTolerance) -> Vec<(f64, f64)> {
    let d = s.vector();
    let dl = d.length();
    if dl == 0.0 || !dl.is_finite() {
        return Vec::new();
    }
    let u = d / dl;
    // Foot of the perpendicular from the center.
    let along = (c.center - s.a).dot(u);
    let foot = s.a + u * along;
    let dist = foot.distance(c.center);
    let r = c.radius;
    let ttol = tol.at_scale(r.max(dist));
    if dist > r + ttol {
        return Vec::new();
    }
    let h2 = r * r - dist * dist;
    let mk = |off: f64| {
        let p = foot + u * off;
        ((along + off) / dl, (p - c.center).angle())
    };
    if h2 <= 0.0 || (r - dist).abs() <= ttol {
        return vec![mk(0.0)];
    }
    let h = h2.sqrt();
    vec![mk(-h), mk(h)]
}

fn line_circle(s: Segment, c: Circle, tol: ModelTolerance, span: Span) -> Intersections {
    let len = s.length();
    let eps = if len > 0.0 { tol.at_scale(len) / len } else { 0.0 };
    let mut out = Intersections::none();
    for (t, ang) in line_circle_raw(s, c, tol) {
        if t < -eps || t > 1.0 + eps {
            continue;
        }
        if let Some(tc) = span.param(ang, c.radius, tol) {
            let t = t.clamp(0.0, 1.0);
            out.points.push(Intersection { point: s.point_at(t), t_a: t, t_b: tc });
        }
    }
    out
}

/// Intersection angles of two full circles: `(angle_on_1, angle_on_2)`.
/// Returns `None` for coincident circles.
pub(crate) fn circle_circle_raw(c1: Circle, c2: Circle, tol: ModelTolerance) -> Option<Vec<(f64, f64)>> {
    let dv = c2.center - c1.center;
    let d = dv.length();
    let ttol = tol.at_scale(c1.radius.max(c2.radius).max(d));
    if d <= ttol {
        return if (c1.radius - c2.radius).abs() <= ttol { None } else { Some(Vec::new()) };
    }
    if d > c1.radius + c2.radius + ttol || d < (c1.radius - c2.radius).abs() - ttol {
        return Some(Vec::new());
    }
    let u = dv / d;
    let a = (c1.radius * c1.radius - c2.radius * c2.radius + d * d) / (2.0 * d);
    let h2 = c1.radius * c1.radius - a * a;
    let base = c1.center + u * a;
    let ang = |p: Point| ((p - c1.center).angle(), (p - c2.center).angle());
    let tangent = (d - (c1.radius + c2.radius)).abs() <= ttol || (d - (c1.radius - c2.radius).abs()).abs() <= ttol;
    if h2 <= 0.0 || tangent {
        return Some(vec![ang(base)]);
    }
    let h = h2.sqrt();
    let n: Vector = u.perp();
    Some(vec![ang(base + n * h), ang(base - n * h)])
}

fn circle_circle(c1: Circle, s1: Span, c2: Circle, s2: Span, tol: ModelTolerance) -> Intersections {
    let Some(raw) = circle_circle_raw(c1, c2, tol) else {
        return coincident_circles(c1, s1, s2, tol);
    };
    let mut out = Intersections::none();
    for (a1, a2) in raw {
        if let (Some(t1), Some(t2)) = (s1.param(a1, c1.radius, tol), s2.param(a2, c2.radius, tol)) {
            out.points.push(Intersection { point: c1.point_at_angle(a1), t_a: t1, t_b: t2 });
        }
    }
    out
}

fn coincident_circles(c: Circle, s1: Span, s2: Span, tol: ModelTolerance) -> Intersections {
    // Co-circular curves: report overlap plus the span endpoints that lie on the other.
    let mut out = Intersections { points: Vec::new(), overlap: false };
    let ends = |s: Span| match s {
        Span::Full => Vec::new(),
        Span::Arc(a) => vec![a.start, a.end_angle()],
    };
    for ang in ends(s1) {
        if let (Some(t1), Some(t2)) = (s1.param(ang, c.radius, tol), s2.param(ang, c.radius, tol)) {
            out.points.push(Intersection { point: c.point_at_angle(ang), t_a: t1, t_b: t2 });
        }
    }
    for ang in ends(s2) {
        if let (Some(t1), Some(t2)) = (s1.param(ang, c.radius, tol), s2.param(ang, c.radius, tol)) {
            out.points.push(Intersection { point: c.point_at_angle(ang), t_a: t1, t_b: t2 });
        }
    }
    out.overlap = match (s1, s2) {
        (Span::Full, _) | (_, Span::Full) => true,
        (Span::Arc(a1), Span::Arc(a2)) => {
            // Overlap if a midpoint of either lies on the other.
            s2.param((a1.mid_point() - c.center).angle(), c.radius, tol).is_some()
                || s1.param((a2.mid_point() - c.center).angle(), c.radius, tol).is_some()
        }
    };
    if !out.overlap {
        out.points.truncate(out.points.len().min(2));
    }
    out
}

/// Flattened cubic with parameter values, fine enough for intersection seeding.
fn cubic_samples(c: CubicBez, tol: f64) -> Vec<(f64, Point)> {
    let mut out = vec![(0.0, c.p0)];
    sample_rec(c, 0.0, 1.0, tol, 0, &mut out);
    out
}

fn sample_rec(c: CubicBez, t0: f64, t1: f64, tol: f64, depth: u32, out: &mut Vec<(f64, Point)>) {
    if depth >= 14 || c.flatness() <= tol {
        out.push((t1, c.p3));
        return;
    }
    let (l, r) = c.split(0.5);
    let tm = 0.5 * (t0 + t1);
    sample_rec(l, t0, tm, tol, depth + 1, out);
    sample_rec(r, tm, t1, tol, depth + 1, out);
}

fn cubic_curve(c: CubicBez, other: &Curve, tol: ModelTolerance) -> Intersections {
    let size = c.hull_bbox().union(other.bbox()).size().length().max(1e-300);
    let flat_tol = size * 1e-4;
    let a_samples = cubic_samples(c, flat_tol);
    let mut out = Intersections::none();
    match *other {
        Curve::Cubic(d) => {
            let b_samples = cubic_samples(d, flat_tol);
            for wa in a_samples.windows(2) {
                let sa = Segment::new(wa[0].1, wa[1].1);
                for wb in b_samples.windows(2) {
                    let sb = Segment::new(wb[0].1, wb[1].1);
                    let r = segment_segment(sa, sb, ModelTolerance { abs: flat_tol, rel: 0.0 });
                    for p in r.points {
                        let s0 = wa[0].0 + (wa[1].0 - wa[0].0) * p.t_a;
                        let t0 = wb[0].0 + (wb[1].0 - wb[0].0) * p.t_b;
                        if let Some((s, t)) = refine_cubic_cubic(c, d, s0, t0, tol) {
                            out.points.push(Intersection { point: c.eval(s).midpoint(d.eval(t)), t_a: s, t_b: t });
                        }
                    }
                }
            }
        }
        _ => {
            for wa in a_samples.windows(2) {
                let sa = Curve::Line(Segment::new(wa[0].1, wa[1].1));
                let r = intersect(&sa, other, ModelTolerance { abs: flat_tol, rel: 0.0 });
                for p in r.points {
                    let s0 = wa[0].0 + (wa[1].0 - wa[0].0) * p.t_a;
                    if let Some((s, q, t)) = refine_cubic_curve(c, other, s0, tol) {
                        out.points.push(Intersection { point: q, t_a: s, t_b: t });
                    }
                }
            }
        }
    }
    out
}

/// Newton on `dist(B(s), other)` using the closest-point projection onto `other`.
fn refine_cubic_curve(c: CubicBez, other: &Curve, s0: f64, tol: ModelTolerance) -> Option<(f64, Point, f64)> {
    let mut s = s0;
    for _ in 0..30 {
        let p = c.eval(s);
        let (_, q) = other.closest(p);
        let (t_on, _) = other.closest(q);
        let n = match other.tangent_at(t_on) {
            Some(tan) => tan.perp(),
            None => break,
        };
        let f = (p - q).dot(n);
        let fp = c.deriv(s).dot(n);
        if fp.abs() < 1e-300 {
            break;
        }
        let ns = (s - f / fp).clamp(0.0, 1.0);
        if (ns - s).abs() < 1e-15 {
            s = ns;
            break;
        }
        s = ns;
    }
    let p = c.eval(s);
    let (t, q) = other.closest(p);
    (p.distance(q) <= tol.at_scale(p.to_vector().length()) * 10.0 + 1e-9).then_some((s, p.midpoint(q), t))
}

fn refine_cubic_cubic(a: CubicBez, b: CubicBez, s0: f64, t0: f64, tol: ModelTolerance) -> Option<(f64, f64)> {
    let (mut s, mut t) = (s0, t0);
    for _ in 0..30 {
        let f = a.eval(s) - b.eval(t);
        let da = a.deriv(s);
        let db = b.deriv(t);
        // Solve [da, -db] [ds, dt]^T = -f
        let det = da.x * (-db.y) - (-db.x) * da.y;
        if det.abs() < 1e-300 {
            break;
        }
        let ds = (-f.x * (-db.y) - (-db.x) * (-f.y)) / det;
        let dt = (da.x * (-f.y) - (-f.x) * da.y) / det;
        s = (s + ds).clamp(0.0, 1.0);
        t = (t + dt).clamp(0.0, 1.0);
        if ds.abs() < 1e-15 && dt.abs() < 1e-15 {
            break;
        }
    }
    let p = a.eval(s);
    (p.distance(b.eval(t)) <= tol.at_scale(p.to_vector().length()) * 10.0 + 1e-9).then_some((s, t))
}

fn dedup(points: &mut Vec<Intersection>, tol: ModelTolerance) {
    let mut out: Vec<Intersection> = Vec::with_capacity(points.len());
    for p in points.drain(..) {
        let dup = out.iter().any(|q| q.point.distance(p.point) <= tol.at_scale(p.point.to_vector().length()) * 10.0);
        if !dup {
            out.push(p);
        }
    }
    out.sort_by(|a, b| a.t_a.total_cmp(&b.t_a));
    *points = out;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Point;

    fn tol() -> ModelTolerance {
        ModelTolerance::DEFAULT
    }

    fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Curve {
        Curve::Line(Segment::new(Point::new(ax, ay), Point::new(bx, by)))
    }

    #[test]
    fn crossing_segments() {
        let r = intersect(&line(0.0, 0.0, 2.0, 2.0), &line(0.0, 2.0, 2.0, 0.0), tol());
        assert_eq!(r.points.len(), 1);
        assert!(r.points[0].point.distance(Point::new(1.0, 1.0)) < 1e-12);
        assert!((r.points[0].t_a - 0.5).abs() < 1e-12);
    }

    #[test]
    fn parallel_segments_do_not_intersect() {
        let r = intersect(&line(0.0, 0.0, 2.0, 0.0), &line(0.0, 1.0, 2.0, 1.0), tol());
        assert!(r.points.is_empty() && !r.overlap);
    }

    #[test]
    fn collinear_overlap() {
        let r = intersect(&line(0.0, 0.0, 4.0, 0.0), &line(2.0, 0.0, 6.0, 0.0), tol());
        assert!(r.overlap);
        assert_eq!(r.points.len(), 2);
        assert!(r.points[0].point.distance(Point::new(2.0, 0.0)) < 1e-12);
        assert!(r.points[1].point.distance(Point::new(4.0, 0.0)) < 1e-12);
    }

    #[test]
    fn touching_endpoint() {
        let r = intersect(&line(0.0, 0.0, 2.0, 0.0), &line(2.0, 0.0, 2.0, 5.0), tol());
        assert_eq!(r.points.len(), 1);
        assert!(r.points[0].point.distance(Point::new(2.0, 0.0)) < 1e-12);
    }

    #[test]
    fn line_circle_two_and_tangent() {
        let c = Curve::Circle(Circle::new(Point::ORIGIN, 1.0).unwrap());
        let r = intersect(&line(-2.0, 0.0, 2.0, 0.0), &c, tol());
        assert_eq!(r.points.len(), 2);
        let t = intersect(&line(-2.0, 1.0, 2.0, 1.0), &c, tol());
        assert_eq!(t.points.len(), 1);
        assert!(t.points[0].point.distance(Point::new(0.0, 1.0)) < 1e-9);
    }

    #[test]
    fn circle_circle_cases() {
        let a = Curve::Circle(Circle::new(Point::ORIGIN, 1.0).unwrap());
        let b = Curve::Circle(Circle::new(Point::new(1.0, 0.0), 1.0).unwrap());
        let r = intersect(&a, &b, tol());
        assert_eq!(r.points.len(), 2);
        for p in &r.points {
            assert!((p.point.distance(Point::ORIGIN) - 1.0).abs() < 1e-12);
            assert!((p.point.distance(Point::new(1.0, 0.0)) - 1.0).abs() < 1e-12);
        }
        let ext = Curve::Circle(Circle::new(Point::new(2.0, 0.0), 1.0).unwrap());
        assert_eq!(intersect(&a, &ext, tol()).points.len(), 1);
        let same = intersect(&a, &a, tol());
        assert!(same.overlap);
    }

    #[test]
    fn arc_filters_by_span() {
        let upper = Curve::Arc(Arc::new(Point::ORIGIN, 1.0, 0.0, core::f64::consts::PI).unwrap());
        let r = intersect(&line(-2.0, 0.5, 2.0, 0.5), &upper, tol());
        assert_eq!(r.points.len(), 2);
        let r2 = intersect(&line(-2.0, -0.5, 2.0, -0.5), &upper, tol());
        assert!(r2.points.is_empty());
    }

    #[test]
    fn cubic_line_refined() {
        let c = Curve::Cubic(CubicBez {
            p0: Point::new(0.0, 0.0),
            p1: Point::new(1.0, 2.0),
            p2: Point::new(2.0, -2.0),
            p3: Point::new(3.0, 0.0),
        });
        let l = line(-1.0, 0.0, 4.0, 0.0);
        let r = intersect(&c, &l, tol());
        assert_eq!(r.points.len(), 3, "{r:?}");
        for p in &r.points {
            assert!(p.point.y.abs() < 1e-9);
            assert!(c.point_at(p.t_a).distance(p.point) < 1e-9);
        }
    }

    #[test]
    fn cubic_cubic() {
        let a = Curve::Cubic(CubicBez {
            p0: Point::new(0.0, 0.0),
            p1: Point::new(1.0, 1.0),
            p2: Point::new(2.0, 1.0),
            p3: Point::new(3.0, 0.0),
        });
        let b = Curve::Cubic(CubicBez {
            p0: Point::new(0.0, 0.6),
            p1: Point::new(1.0, 0.0),
            p2: Point::new(2.0, 0.0),
            p3: Point::new(3.0, 0.6),
        });
        let r = intersect(&a, &b, tol());
        assert_eq!(r.points.len(), 2, "{r:?}");
        for p in &r.points {
            assert!(a.point_at(p.t_a).distance(b.point_at(p.t_b)) < 1e-8);
        }
    }
}
