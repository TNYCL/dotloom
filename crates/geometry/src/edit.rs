//! Split, trim and extend for lines, arcs, circles and line/arc polylines.
//!
//! Supported classes (anything else returns [`GeometryError::UnsupportedOperation`]):
//!
//! | operation | line | arc | circle | open polyline | closed polyline | path/polygon/rect/text |
//! |-----------|------|-----|--------|---------------|-----------------|------------------------|
//! | split     | yes  | yes | two points | yes       | yes (opens it)  | no |
//! | trim      | yes  | yes | yes    | yes           | no              | no |
//! | extend    | yes  | yes | n/a    | end segments  | n/a             | no |

use core::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::{
    Aabb, Arc, Circle, Curve, GeoResult, GeometryError, ModelTolerance, Point, Polyline, Segment, Shape,
    intersect::intersect, normalize_angle,
};

/// Which end of an open curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CurveEnd {
    /// The start point.
    Start,
    /// The end point.
    End,
}

/// An open chain of line/arc pieces with a global parameter `u ∈ [0, n]`.
#[derive(Debug, Clone)]
struct Chain {
    pieces: Vec<Curve>,
    /// Original shape kind for rebuilding.
    origin: ChainOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ChainOrigin {
    Line,
    Arc,
    Polyline,
}

impl Chain {
    fn from_shape(s: &Shape, op: &'static str) -> GeoResult<Self> {
        match s {
            Shape::Line(l) => Ok(Self { pieces: vec![Curve::Line(*l)], origin: ChainOrigin::Line }),
            Shape::Arc(a) => Ok(Self { pieces: vec![Curve::Arc(*a)], origin: ChainOrigin::Arc }),
            Shape::Polyline(p) if !p.closed => Ok(Self { pieces: s.curves(), origin: ChainOrigin::Polyline }),
            other => Err(GeometryError::UnsupportedOperation { operation: op, shape: other.kind().name() }),
        }
    }

    fn n(&self) -> f64 {
        self.pieces.len() as f64
    }

    fn closest_u(&self, p: Point) -> f64 {
        let mut best = (f64::INFINITY, 0.0);
        for (i, c) in self.pieces.iter().enumerate() {
            let (t, q) = c.closest(p);
            let d = q.distance_sq(p);
            if d < best.0 {
                best = (d, i as f64 + t);
            }
        }
        best.1
    }

    fn point_at_u(&self, u: f64) -> Point {
        let (i, t) = self.split_u(u);
        self.pieces.get(i).map_or(Point::ORIGIN, |c| c.point_at(t))
    }

    fn split_u(&self, u: f64) -> (usize, f64) {
        let n = self.pieces.len();
        if n == 0 {
            return (0, 0.0);
        }
        let u = u.clamp(0.0, n as f64);
        let i = (u.floor() as usize).min(n - 1);
        (i, u - i as f64)
    }

    /// Sub-chain between global parameters.
    fn sub(&self, u0: f64, u1: f64) -> Vec<Curve> {
        let mut out = Vec::new();
        let (i0, t0) = self.split_u(u0);
        let (i1, t1) = self.split_u(u1);
        for i in i0..=i1 {
            let Some(c) = self.pieces.get(i) else { continue };
            let a = if i == i0 { t0 } else { 0.0 };
            let b = if i == i1 { t1 } else { 1.0 };
            if b - a <= 1e-15 {
                continue;
            }
            out.push(sub_curve(c, a, b));
        }
        out
    }

    fn rebuild(&self, pieces: Vec<Curve>) -> Option<Shape> {
        if pieces.is_empty() {
            return None;
        }
        if pieces.len() == 1
            && let Some(c) = pieces.first()
        {
            match (*c, self.origin) {
                (Curve::Line(l), ChainOrigin::Line) => return Some(Shape::Line(l)),
                (Curve::Arc(a), ChainOrigin::Arc) => return Some(Shape::Arc(a)),
                _ => {}
            }
        }
        Some(Shape::Polyline(curves_to_polyline(&pieces)))
    }
}

fn sub_curve(c: &Curve, a: f64, b: f64) -> Curve {
    match *c {
        Curve::Line(s) => Curve::Line(Segment::new(s.point_at(a), s.point_at(b))),
        Curve::Arc(arc) => Curve::Arc(arc.subarc(a, b)),
        Curve::Circle(ci) => Arc::new(ci.center, ci.radius, a * TAU, (b - a) * TAU).map_or(*c, Curve::Arc),
        Curve::Cubic(cb) => Curve::Cubic(cb.subsegment(a, b)),
    }
}

fn curves_to_polyline(pieces: &[Curve]) -> Polyline {
    let mut points = Vec::with_capacity(pieces.len() + 1);
    let mut bulges = Vec::with_capacity(pieces.len());
    if let Some(f) = pieces.first() {
        points.push(f.start());
    }
    for c in pieces {
        points.push(c.end());
        bulges.push(match c {
            Curve::Arc(a) => a.bulge(),
            _ => 0.0,
        });
    }
    if bulges.iter().all(|b| *b == 0.0) {
        bulges.clear();
    }
    Polyline { points, bulges, closed: false }
}

/// Split a shape at the point on it closest to `at`.
///
/// Lines, arcs and open polylines yield two shapes; a closed polyline yields one open
/// polyline that starts and ends at the split point. Circles need two points: see
/// [`split_circle`].
pub fn split_at(shape: &Shape, at: Point, tol: ModelTolerance) -> GeoResult<Vec<Shape>> {
    if let Shape::Polyline(p) = shape
        && p.closed
    {
        return split_closed_polyline(shape, p, at);
    }
    let chain = Chain::from_shape(shape, "split")?;
    let u = chain.closest_u(at);
    let p = chain.point_at_u(u);
    if p.distance(chain.point_at_u(0.0)) <= tol.at_scale(p.to_vector().length())
        || p.distance(chain.point_at_u(chain.n())) <= tol.at_scale(p.to_vector().length())
    {
        return Err(GeometryError::InvalidArgument("split point coincides with an endpoint"));
    }
    let a = chain.rebuild(chain.sub(0.0, u));
    let b = chain.rebuild(chain.sub(u, chain.n()));
    Ok(a.into_iter().chain(b).collect())
}

fn split_closed_polyline(shape: &Shape, p: &Polyline, at: Point) -> GeoResult<Vec<Shape>> {
    let pieces = shape.curves();
    if pieces.is_empty() {
        return Err(GeometryError::Degenerate("empty polyline"));
    }
    let chain = Chain { pieces, origin: ChainOrigin::Polyline };
    let u = chain.closest_u(at);
    let mut curves = chain.sub(u, chain.n());
    curves.extend(chain.sub(0.0, u));
    let _ = p;
    Ok(vec![Shape::Polyline(curves_to_polyline(&curves))])
}

/// Split a circle into two arcs at the points closest to `a` and `b`.
pub fn split_circle(c: Circle, a: Point, b: Point, tol: ModelTolerance) -> GeoResult<[Arc; 2]> {
    let (aa, pa) = c.closest(a);
    let (ab, pb) = c.closest(b);
    if pa.distance(pb) <= tol.at_scale(c.radius) {
        return Err(GeometryError::InvalidArgument("split points coincide"));
    }
    let s1 = normalize_angle(ab - aa);
    Ok([Arc::new(c.center, c.radius, aa, s1)?, Arc::new(c.center, c.radius, ab, TAU - s1)?])
}

/// Intersection parameters of `chain` with all cutter shapes, sorted and deduplicated.
fn cut_params(chain: &Chain, cutters: &[Shape], tol: ModelTolerance) -> Vec<f64> {
    let mut us = Vec::new();
    for (i, piece) in chain.pieces.iter().enumerate() {
        for cutter in cutters {
            for cc in cutter.curves() {
                let r = intersect(piece, &cc, tol);
                if r.overlap {
                    continue;
                }
                for p in r.points {
                    us.push(i as f64 + p.t_a);
                }
            }
        }
    }
    us.sort_by(f64::total_cmp);
    us.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    us
}

/// Trim: remove the piece of `target` between the cutting intersections that
/// surround `pick`. Returns the remaining pieces (possibly none).
pub fn trim(target: &Shape, cutters: &[Shape], pick: Point, tol: ModelTolerance) -> GeoResult<Vec<Shape>> {
    if let Shape::Circle(c) = target {
        return trim_circle(*c, cutters, pick, tol).map(|a| vec![Shape::Arc(a)]);
    }
    let chain = Chain::from_shape(target, "trim")?;
    let n = chain.n();
    let cuts: Vec<f64> = cut_params(&chain, cutters, tol).into_iter().filter(|u| *u > 1e-9 && *u < n - 1e-9).collect();
    if cuts.is_empty() {
        return Err(GeometryError::NoBoundary("no cutting edge intersects the target"));
    }
    let up = chain.closest_u(pick);
    let lo = cuts.iter().copied().filter(|u| *u <= up).fold(0.0, f64::max);
    let hi = cuts.iter().copied().filter(|u| *u > up).fold(n, f64::min);
    let mut out = Vec::new();
    if lo > 0.0 {
        out.extend(chain.rebuild(chain.sub(0.0, lo)));
    }
    if hi < n {
        out.extend(chain.rebuild(chain.sub(hi, n)));
    }
    Ok(out)
}

fn trim_circle(c: Circle, cutters: &[Shape], pick: Point, tol: ModelTolerance) -> GeoResult<Arc> {
    let curve = Curve::Circle(c);
    let mut angles: Vec<f64> = Vec::new();
    for cutter in cutters {
        for cc in cutter.curves() {
            let r = intersect(&curve, &cc, tol);
            if !r.overlap {
                angles.extend(r.points.iter().map(|p| normalize_angle(p.t_a * TAU)));
            }
        }
    }
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    if angles.len() < 2 {
        return Err(GeometryError::NoBoundary("trimming a circle needs two intersections"));
    }
    let (pa, _) = c.closest(pick);
    // Gap containing the pick angle: from the last angle ≤ pa to the first > pa (cyclic).
    let lo = angles.iter().copied().rev().find(|a| *a <= pa);
    let hi = angles.iter().copied().find(|a| *a > pa);
    let (gap_start, gap_end) = match (lo, hi) {
        (Some(l), Some(h)) => (l, h),
        _ => (angles.last().copied().unwrap_or(0.0), angles.first().copied().unwrap_or(0.0)),
    };
    // Keep from gap_end CCW to gap_start.
    let sweep = normalize_angle(gap_start - gap_end);
    Arc::new(c.center, c.radius, gap_end, if sweep == 0.0 { TAU } else { sweep })
}

/// Extend one end of a line/arc/open polyline to the nearest boundary.
pub fn extend(target: &Shape, end: CurveEnd, boundaries: &[Shape], tol: ModelTolerance) -> GeoResult<Shape> {
    match target {
        Shape::Line(l) => extend_line(*l, end, boundaries, tol).map(Shape::Line),
        Shape::Arc(a) => extend_arc(*a, end, boundaries, tol).map(Shape::Arc),
        Shape::Polyline(p) if !p.closed && p.points.len() >= 2 => {
            let last = p.segment_count().saturating_sub(1);
            let idx = if end == CurveEnd::Start { 0 } else { last };
            let seg = p.segment(idx).ok_or(GeometryError::Degenerate("polyline segment"))?;
            let mut q = p.clone();
            match seg {
                Curve::Line(l) => {
                    let e = extend_line(l, end, boundaries, tol)?;
                    if end == CurveEnd::Start {
                        if let Some(f) = q.points.first_mut() {
                            *f = e.a;
                        }
                    } else if let Some(l) = q.points.last_mut() {
                        *l = e.b;
                    }
                }
                Curve::Arc(a) => {
                    let e = extend_arc(a, end, boundaries, tol)?;
                    if let Some(b) = q.bulges.get_mut(idx) {
                        *b = e.bulge();
                    }
                    if end == CurveEnd::Start {
                        if let Some(f) = q.points.first_mut() {
                            *f = e.start_point();
                        }
                    } else if let Some(l) = q.points.last_mut() {
                        *l = e.end_point();
                    }
                }
                _ => {
                    return Err(GeometryError::UnsupportedOperation { operation: "extend", shape: "polyline segment" });
                }
            }
            Ok(Shape::Polyline(q))
        }
        other => Err(GeometryError::UnsupportedOperation { operation: "extend", shape: other.kind().name() }),
    }
}

fn reach(target: Aabb, boundaries: &[Shape]) -> f64 {
    let all = boundaries.iter().fold(target, |b, s| b.union(s.bbox()));
    all.size().length() * 2.0 + 1.0
}

fn extend_line(l: Segment, end: CurveEnd, boundaries: &[Shape], tol: ModelTolerance) -> GeoResult<Segment> {
    let dir = l.direction().ok_or(GeometryError::Degenerate("zero-length line"))?;
    let (from, dir) = match end {
        CurveEnd::End => (l.b, dir),
        CurveEnd::Start => (l.a, -dir),
    };
    let far = from + dir * reach(l.bbox(), boundaries);
    let ray = Curve::Line(Segment::new(from, far));
    let min_t = tol.at_scale(from.to_vector().length()) / from.distance(far).max(f64::MIN_POSITIVE);
    let mut best: Option<(f64, Point)> = None;
    for b in boundaries {
        for bc in b.curves() {
            for p in intersect(&ray, &bc, tol).points {
                if p.t_a > min_t && best.is_none_or(|(t, _)| p.t_a < t) {
                    best = Some((p.t_a, p.point));
                }
            }
        }
    }
    let (_, p) = best.ok_or(GeometryError::NoBoundary("no boundary in the extension direction"))?;
    Ok(match end {
        CurveEnd::End => Segment::new(l.a, p),
        CurveEnd::Start => Segment::new(p, l.b),
    })
}

fn extend_arc(a: Arc, end: CurveEnd, boundaries: &[Shape], tol: ModelTolerance) -> GeoResult<Arc> {
    let circle = Curve::Circle(Circle { center: a.center, radius: a.radius });
    let dirsign = a.sweep.signum();
    let end_angle = match end {
        CurveEnd::End => a.end_angle(),
        CurveEnd::Start => a.start,
    };
    let max_extra = TAU - a.sweep.abs();
    let eps = tol.at_scale(a.radius) / a.radius;
    let mut best: Option<f64> = None;
    for b in boundaries {
        for bc in b.curves() {
            let r = intersect(&circle, &bc, tol);
            if r.overlap {
                continue;
            }
            for p in r.points {
                let ang = p.t_a * TAU;
                // Angular distance travelled beyond the end in the extension direction.
                let extra = match end {
                    CurveEnd::End => normalize_angle((ang - end_angle) * dirsign),
                    CurveEnd::Start => normalize_angle((end_angle - ang) * dirsign),
                };
                if extra > eps && extra < max_extra - eps && best.is_none_or(|e| extra < e) {
                    best = Some(extra);
                }
            }
        }
    }
    let extra = best.ok_or(GeometryError::NoBoundary("no boundary along the arc"))?;
    Ok(match end {
        CurveEnd::End => Arc { sweep: a.sweep + dirsign * extra, ..a },
        CurveEnd::Start => Arc { start: a.start - dirsign * extra, sweep: a.sweep + dirsign * extra, ..a },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::PI;

    fn tol() -> ModelTolerance {
        ModelTolerance::DEFAULT
    }

    fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Shape {
        Shape::Line(Segment::new(Point::new(ax, ay), Point::new(bx, by)))
    }

    #[test]
    fn split_line() {
        let parts = split_at(&line(0.0, 0.0, 10.0, 0.0), Point::new(4.0, 1.0), tol()).unwrap();
        assert_eq!(parts, vec![line(0.0, 0.0, 4.0, 0.0), line(4.0, 0.0, 10.0, 0.0)]);
        assert!(split_at(&line(0.0, 0.0, 10.0, 0.0), Point::new(0.0, 0.0), tol()).is_err());
    }

    #[test]
    fn split_arc_keeps_circle() {
        let a = Shape::Arc(Arc::new(Point::ORIGIN, 2.0, 0.0, PI).unwrap());
        let parts = split_at(&a, Point::new(0.0, 5.0), tol()).unwrap();
        assert_eq!(parts.len(), 2);
        for p in &parts {
            let Shape::Arc(x) = p else { unreachable!() };
            assert!((x.radius - 2.0).abs() < 1e-12);
            assert!((x.sweep - PI / 2.0).abs() < 1e-12);
        }
    }

    #[test]
    fn trim_middle_of_line() {
        let target = line(0.0, 0.0, 10.0, 0.0);
        let cutters = [line(3.0, -1.0, 3.0, 1.0), line(7.0, -1.0, 7.0, 1.0)];
        let r = trim(&target, &cutters, Point::new(5.0, 0.2), tol()).unwrap();
        assert_eq!(r, vec![line(0.0, 0.0, 3.0, 0.0), line(7.0, 0.0, 10.0, 0.0)]);
        let end = trim(&target, &cutters, Point::new(9.0, 0.0), tol()).unwrap();
        assert_eq!(end, vec![line(0.0, 0.0, 7.0, 0.0)]);
        assert!(matches!(
            trim(&target, &[line(20.0, -1.0, 20.0, 1.0)], Point::new(5.0, 0.0), tol()),
            Err(GeometryError::NoBoundary(_))
        ));
    }

    #[test]
    fn trim_circle_between_two_lines() {
        let c = Shape::Circle(Circle::new(Point::ORIGIN, 1.0).unwrap());
        let cutter = line(-2.0, 0.0, 2.0, 0.0);
        let r = trim(&c, &[cutter], Point::new(0.0, 1.0), tol()).unwrap();
        let Shape::Arc(a) = &r[0] else { unreachable!() };
        // Upper half removed: lower half remains.
        assert!(a.mid_point().distance(Point::new(0.0, -1.0)) < 1e-9);
        assert!((a.sweep.abs() - PI).abs() < 1e-9);
    }

    #[test]
    fn extend_line_to_boundary() {
        let l = line(0.0, 0.0, 2.0, 0.0);
        let close = |s: &Shape, ax: f64, bx: f64| {
            let Shape::Line(l) = s else { return false };
            l.a.distance(Point::new(ax, 0.0)) < 1e-12 && l.b.distance(Point::new(bx, 0.0)) < 1e-12
        };
        let r = extend(&l, CurveEnd::End, &[line(5.0, -1.0, 5.0, 1.0)], tol()).unwrap();
        assert!(close(&r, 0.0, 5.0), "{r:?}");
        let r2 = extend(&l, CurveEnd::Start, &[line(-3.0, -1.0, -3.0, 1.0), line(5.0, -1.0, 5.0, 1.0)], tol()).unwrap();
        assert!(close(&r2, -3.0, 2.0), "{r2:?}");
        assert!(extend(&l, CurveEnd::End, &[line(-3.0, -1.0, -3.0, 1.0)], tol()).is_err());
    }

    #[test]
    fn extend_arc_to_line() {
        let a = Shape::Arc(Arc::new(Point::ORIGIN, 1.0, 0.0, PI / 4.0).unwrap());
        let r = extend(&a, CurveEnd::End, &[line(0.0, 0.0, 0.0, 3.0)], tol()).unwrap();
        let Shape::Arc(x) = r else { unreachable!() };
        assert!((x.sweep - PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn polyline_trim_and_split() {
        let pl =
            Shape::Polyline(Polyline::open(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0)]));
        let parts = split_at(&pl, Point::new(10.0, 5.0), tol()).unwrap();
        assert_eq!(parts.len(), 2);
        let Shape::Polyline(first) = &parts[0] else { unreachable!() };
        assert_eq!(first.points.last().copied(), Some(Point::new(10.0, 5.0)));
        let r = trim(&pl, &[line(5.0, -1.0, 5.0, 1.0)], Point::new(1.0, 0.0), tol()).unwrap();
        assert_eq!(r.len(), 1);
        let Shape::Polyline(rest) = &r[0] else { unreachable!() };
        assert_eq!(rest.points.first().copied(), Some(Point::new(5.0, 0.0)));
    }

    #[test]
    fn closed_polyline_split_opens_it() {
        let sq = Shape::Polyline(Polyline::closed(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
        ]));
        let r = split_at(&sq, Point::new(0.5, 0.0), tol()).unwrap();
        let Shape::Polyline(p) = &r[0] else { unreachable!() };
        assert!(!p.closed);
        assert_eq!(p.points.first(), p.points.last());
        assert!((r[0].length() - 4.0).abs() < 1e-12);
    }

    #[test]
    fn unsupported_classes_report_capability_errors() {
        let rect = Shape::Rect(crate::Rect { origin: Point::ORIGIN, width: 1.0, height: 1.0 });
        assert!(matches!(split_at(&rect, Point::ORIGIN, tol()), Err(GeometryError::UnsupportedOperation { .. })));
    }
}
