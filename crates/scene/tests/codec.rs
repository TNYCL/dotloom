//! Scene codec round-trip and robustness tests.

use dotloom_scene::{
    DecodeError, Primitive, SceneDelta, SceneItem, Stroke,
    geometry::{
        Aabb, Arc, Circle, HAlign, Path, PathEl, Point, Polygon, Polyline, Rect, Segment, Shape, Text, VAlign, Vector,
    },
};
use proptest::prelude::*;

fn sample() -> SceneDelta {
    let shapes = vec![
        Shape::Line(Segment::new(Point::new(0.0, 0.0), Point::new(1e9, -3.5))),
        Shape::Polyline(Polyline {
            points: vec![Point::ORIGIN, Point::new(1.0, 2.0), Point::new(3.0, 0.0)],
            bulges: vec![0.0, 0.5],
            closed: false,
        }),
        Shape::Rect(Rect { origin: Point::new(-1.0, -2.0), width: 3.0, height: 4.0 }),
        Shape::Circle(Circle { center: Point::new(5.0, 5.0), radius: 2.0 }),
        Shape::Arc(Arc { center: Point::ORIGIN, radius: 1.0, start: 0.25, sweep: -1.5 }),
        Shape::Path(Path {
            elements: vec![
                PathEl::MoveTo(Point::ORIGIN),
                PathEl::QuadTo(Point::new(1.0, 1.0), Point::new(2.0, 0.0)),
                PathEl::CubicTo(Point::new(3.0, 1.0), Point::new(4.0, -1.0), Point::new(5.0, 0.0)),
                PathEl::LineTo(Point::new(5.0, -3.0)),
                PathEl::Close,
            ],
        }),
        Shape::Polygon(Polygon {
            outer: vec![Point::ORIGIN, Point::new(10.0, 0.0), Point::new(10.0, 10.0)],
            holes: vec![vec![Point::new(1.0, 1.0), Point::new(2.0, 1.0), Point::new(2.0, 2.0)]],
        }),
    ];
    let mut prims: Vec<Primitive> = shapes
        .into_iter()
        .map(|shape| Primitive::Shape {
            shape,
            stroke: Some(Stroke { color: 0x112233ff, width: 1.5, dash: vec![4.0, 2.0] }),
            fill: Some(0x00ff0080),
        })
        .collect();
    prims.push(Primitive::Text {
        text: Text {
            position: Point::new(1.0, 1.0),
            content: "Ölçü 1200 mm — ğüşıİç".into(),
            height: 3.5,
            rotation: 0.3,
            halign: HAlign::Center,
            valign: VAlign::Middle,
        },
        color: 0xff,
    });
    prims.push(Primitive::Arrow {
        tip: Point::new(2.0, 3.0),
        direction: Vector::new(1.0, 0.0),
        size: 3.0,
        color: 0xff,
    });
    SceneDelta {
        revision: 42,
        preview: true,
        reset: false,
        upserts: vec![SceneItem {
            id: 7,
            layer: 2,
            bbox: Aabb::from_corners(Point::ORIGIN, Point::new(10.0, 10.0)),
            flags: 3,
            prims,
        }],
        removals: vec![1, 2, 3],
        order: Some(vec![7, 9, 11]),
    }
}

#[test]
fn roundtrip_all_primitives() {
    let d = sample();
    let bytes = d.encode();
    assert_eq!(SceneDelta::decode(&bytes).unwrap(), d);
    let empty = SceneDelta::default();
    assert_eq!(SceneDelta::decode(&empty.encode()).unwrap(), empty);
}

#[test]
fn rejects_bad_headers() {
    assert_eq!(SceneDelta::decode(b"XXXX"), Err(DecodeError::BadMagic));
    let mut b = sample().encode();
    b[4] = 99;
    assert_eq!(SceneDelta::decode(&b), Err(DecodeError::Version(99)));
    let mut t = sample().encode();
    t.push(0);
    assert_eq!(SceneDelta::decode(&t), Err(DecodeError::Trailing));
}

#[test]
fn huge_counts_do_not_allocate() {
    let mut b = SceneDelta::default().encode();
    // Overwrite the upsert count with u32::MAX.
    b[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(SceneDelta::decode(&b), Err(DecodeError::Count(_))));
}

proptest! {
    #[test]
    fn truncated_or_mutated_input_never_panics(cut in 0usize..2000, flip in 0usize..2000, byte in any::<u8>()) {
        let b = sample().encode();
        let cut = cut.min(b.len());
        let _ = SceneDelta::decode(&b[..cut]);
        let mut m = b.clone();
        let i = flip % m.len();
        m[i] = byte;
        let _ = SceneDelta::decode(&m);
    }
}
