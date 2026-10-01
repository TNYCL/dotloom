//! Property tests for geometry invariants (DL-GEO, DL-TEST).
//!
//! Seeds are reproducible: proptest persists failures under `proptest-regressions/`.

use dotloom_geometry::{
    Aabb, Affine, Arc, Circle, CubicBez, Curve, FlattenTolerance, ModelTolerance, Point, Segment, Shape, SpatialIndex,
    Vector, edit, intersect::intersect,
};
use proptest::prelude::*;

fn coord() -> impl Strategy<Value = f64> {
    -1000.0..1000.0f64
}

fn point() -> impl Strategy<Value = Point> {
    (coord(), coord()).prop_map(|(x, y)| Point::new(x, y))
}

fn segment() -> impl Strategy<Value = Segment> {
    (point(), point()).prop_map(|(a, b)| Segment::new(a, b))
}

fn circle() -> impl Strategy<Value = Circle> {
    (point(), 0.1..500.0f64).prop_map(|(c, r)| Circle { center: c, radius: r })
}

fn arc() -> impl Strategy<Value = Arc> {
    (point(), 0.1..500.0f64, -7.0..7.0f64, prop_oneof![-6.2..-0.01f64, 0.01..6.2f64])
        .prop_map(|(c, r, s, w)| Arc::new(c, r, s, w).unwrap_or(Arc { center: c, radius: r, start: s, sweep: w }))
}

fn cubic() -> impl Strategy<Value = CubicBez> {
    (point(), point(), point(), point()).prop_map(|(p0, p1, p2, p3)| CubicBez { p0, p1, p2, p3 })
}

fn curve() -> impl Strategy<Value = Curve> {
    prop_oneof![
        segment().prop_map(Curve::Line),
        circle().prop_map(Curve::Circle),
        arc().prop_map(Curve::Arc),
        cubic().prop_map(Curve::Cubic),
    ]
}

fn on_curve_tol(c: &Curve) -> f64 {
    // Tolerance proportional to the size of the scene, as documented.
    1e-6 * (1.0 + c.bbox().size().length())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, .. ProptestConfig::default() })]

    #[test]
    fn intersection_points_lie_on_both_curves(a in curve(), b in curve()) {
        let r = intersect(&a, &b, ModelTolerance::DEFAULT);
        if !r.overlap {
            for p in &r.points {
                prop_assert!(a.distance_to_point(p.point) <= on_curve_tol(&a) + on_curve_tol(&b), "{a:?} {b:?} {p:?}");
                prop_assert!(b.distance_to_point(p.point) <= on_curve_tol(&a) + on_curve_tol(&b), "{a:?} {b:?} {p:?}");
                prop_assert!(a.point_at(p.t_a).distance(p.point) <= on_curve_tol(&a) * 10.0 + on_curve_tol(&b));
            }
        }
    }

    #[test]
    fn intersection_is_symmetric_for_exact_classes(
        a in prop_oneof![segment().prop_map(Curve::Line), circle().prop_map(Curve::Circle), arc().prop_map(Curve::Arc)],
        b in prop_oneof![segment().prop_map(Curve::Line), circle().prop_map(Curve::Circle), arc().prop_map(Curve::Arc)],
    ) {
        let ab = intersect(&a, &b, ModelTolerance::DEFAULT);
        let ba = intersect(&b, &a, ModelTolerance::DEFAULT);
        prop_assert_eq!(ab.overlap, ba.overlap);
        prop_assert_eq!(ab.points.len(), ba.points.len(), "{:?} vs {:?}", ab, ba);
        let tol = 1e-6 * (1.0 + a.bbox().union(b.bbox()).size().length());
        for p in &ab.points {
            prop_assert!(ba.points.iter().any(|q| q.point.distance(p.point) <= tol), "{ab:?} {ba:?}");
        }
    }

    #[test]
    fn transform_inverse_roundtrip(
        p in point(), angle in -7.0..7.0f64, sx in 0.01..100.0f64, sy in 0.01..100.0f64, t in point(), flip in any::<bool>()
    ) {
        let m = Affine::rotate(angle)
            .then(Affine::scale(if flip { -sx } else { sx }, sy))
            .then(Affine::translate(t.to_vector()));
        let inv = m.inverse().unwrap();
        let back = inv.apply(m.apply(p));
        prop_assert!(back.distance(p) <= 1e-9 * (1.0 + p.to_vector().length()) * (sx.max(sy) / sx.min(sy)));
    }

    #[test]
    fn bbox_contains_samples(c in curve()) {
        let b = c.bbox().inflate(1e-9 * (1.0 + c.bbox().size().length()));
        for i in 0..=64 {
            let q = c.point_at(f64::from(i) / 64.0);
            prop_assert!(b.contains_point(q), "{c:?} {q:?} {b:?}");
        }
    }

    #[test]
    fn flatten_stays_within_tolerance(c in prop_oneof![arc().prop_map(Curve::Arc), cubic().prop_map(Curve::Cubic)]) {
        let tol = 0.05;
        let mut pts = vec![c.start()];
        c.flatten_into(tol, &mut pts);
        for w in pts.windows(2) {
            let mid = w[0].midpoint(w[1]);
            prop_assert!(c.distance_to_point(mid) <= tol * 1.01 + 1e-9, "{c:?}");
        }
    }

    #[test]
    fn transformed_shapes_keep_measurements(
        c in circle(), angle in -7.0..7.0f64, s in 0.01..50.0f64, t in point()
    ) {
        let shape = Shape::Circle(c);
        let m = Affine::rotate(angle).then(Affine::scale(s, s)).then(Affine::translate(t.to_vector()));
        let out = shape.transform(m, dotloom_geometry::TransformPolicy::Strict).unwrap();
        let Shape::Circle(o) = out else { unreachable!() };
        prop_assert!((o.radius - c.radius * s).abs() <= 1e-9 * o.radius.max(1.0));
        prop_assert!(o.center.distance(m.apply(c.center)) <= 1e-9 * (1.0 + o.center.to_vector().length()));
    }

    #[test]
    fn spatial_index_matches_bruteforce(
        boxes in prop::collection::vec((point(), 0.0..50.0f64, 0.0..50.0f64), 0..200),
        q in (point(), 0.0..300.0f64, 0.0..300.0f64),
        probe in point(), radius in 0.0..100.0f64,
    ) {
        let items: Vec<(usize, Aabb)> = boxes.iter().enumerate()
            .map(|(i, (p, w, h))| (i, Aabb::from_corners(*p, *p + Vector::new(*w, *h))))
            .collect();
        let idx = SpatialIndex::bulk_load(items.clone());
        let r = Aabb::from_corners(q.0, q.0 + Vector::new(q.1, q.2));
        let mut brute: Vec<usize> = items.iter().filter(|(_, b)| b.intersects(r)).map(|(k, _)| *k).collect();
        brute.sort_unstable();
        prop_assert_eq!(idx.query_rect(r), brute);
        let mut near: Vec<usize> = idx.query_point(probe, radius);
        near.sort_unstable();
        let mut brute_near: Vec<usize> = items.iter().filter(|(_, b)| b.distance_to_point(probe) <= radius).map(|(k, _)| *k).collect();
        brute_near.sort_unstable();
        prop_assert_eq!(near, brute_near);
    }

    #[test]
    fn trim_pieces_lie_on_target(target in segment(), c1 in segment(), c2 in segment(), pick in 0.0..1.0f64) {
        let shape = Shape::Line(target);
        let pick_point = target.point_at(pick);
        if let Ok(pieces) = edit::trim(&shape, &[Shape::Line(c1), Shape::Line(c2)], pick_point, ModelTolerance::DEFAULT) {
            let tol = 1e-9 * (1.0 + target.bbox().size().length());
            let total: f64 = pieces.iter().map(Shape::length).sum();
            prop_assert!(total <= target.length() + tol);
            for p in pieces {
                let Shape::Line(l) = p else { unreachable!() };
                prop_assert!(target.distance_to_point(l.a) <= tol && target.distance_to_point(l.b) <= tol);
            }
        }
    }

    #[test]
    fn degenerate_inputs_never_panic(a in point(), r in -1.0..1.0f64, w in -1.0..1.0f64) {
        let _ = Arc::new(a, r, 0.0, w);
        let _ = Circle::new(a, r);
        let seg = Curve::Line(Segment::new(a, a));
        let _ = intersect(&seg, &seg, ModelTolerance::DEFAULT);
        let _ = Shape::Line(Segment::new(a, a)).flatten(FlattenTolerance(0.0));
        let _ = Arc::from_three_points(a, a, a);
    }
}

#[test]
fn far_from_origin_intersections_keep_relative_accuracy() {
    // 1e9 mm = 1000 km offset; model tolerance scales relatively.
    let o = Vector::new(1e9, -1e9);
    let a = Curve::Line(Segment::new(Point::new(0.0, 0.0) + o, Point::new(2.0, 2.0) + o));
    let b = Curve::Line(Segment::new(Point::new(0.0, 2.0) + o, Point::new(2.0, 0.0) + o));
    let r = intersect(&a, &b, ModelTolerance::DEFAULT);
    assert_eq!(r.points.len(), 1);
    assert!(r.points[0].point.distance(Point::new(1.0, 1.0) + o) < 1e-6);
}

#[test]
fn tiny_shapes_are_handled() {
    let c = Circle::new(Point::ORIGIN, 1e-9).unwrap();
    let r = intersect(
        &Curve::Circle(c),
        &Curve::Line(Segment::new(Point::new(-1.0, 0.0), Point::new(1.0, 0.0))),
        ModelTolerance { abs: 1e-15, rel: 1e-12 },
    );
    assert_eq!(r.points.len(), 2);
}

#[test]
fn non_finite_inputs_are_rejected() {
    assert!(Shape::Circle(Circle { center: Point::new(f64::INFINITY, 0.0), radius: 1.0 }).validate().is_err());
    assert!(Arc::new(Point::ORIGIN, f64::NAN, 0.0, 1.0).is_err());
    assert!(Affine { m: [f64::NAN; 6] }.inverse().is_err());
    let s = Shape::Line(Segment::new(Point::ORIGIN, Point::new(1.0, 0.0)));
    assert!(
        s.transform(Affine { m: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0] }, dotloom_geometry::TransformPolicy::Strict).is_err()
    );
}
