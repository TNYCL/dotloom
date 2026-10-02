//! Checked-in `.dotl` and SVG fixtures (DL-TEST-5, DL-FILE-1…10).
//!
//! The files in `tests/fixtures/dotl/` were written by Dotloom 0.1.0 and are never
//! regenerated: every later version must still open them the same way. Missing
//! fixtures are created by running the tests with `DOTLOOM_WRITE_FIXTURES=1`;
//! existing files are left alone (delete one on purpose to rewrite it).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use std::path::PathBuf;

use dotloom_engine::{
    ApplyOptions, Command, ConstraintSpec, Engine, EntityTypeDef, NewEntity, Transaction,
    document::{AnchorRef, EntityId, LineRef, PropValue, RefValue, RuleSpec, TypeId},
    eval::ReadOnly,
    geometry::{Arc, Circle, HAlign, Point, Polyline, Segment, Shape, Text, VAlign},
};
use dotloom_io::{
    DotlError, DotlFile, DotlLimits, LossKind,
    dotl::{add_asset, read_dotl, write_dotl},
    svg::import_svg,
    zip::write,
};
use serde_json::{Value, json};

fn dir(sub: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(sub)
}

/// Read a fixture, writing it first when it is missing and writing is enabled.
fn fixture(name: &str, make: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    let path = dir("dotl").join(name);
    if !path.exists() {
        assert!(
            std::env::var("DOTLOOM_WRITE_FIXTURES").as_deref() == Ok("1"),
            "{} is missing; run with DOTLOOM_WRITE_FIXTURES=1 to create it",
            path.display()
        );
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, make()).unwrap();
    }
    std::fs::read(&path).unwrap()
}

fn floorplan_defs() -> Vec<EntityTypeDef> {
    serde_json::from_str(include_str!("../../../tests/fixtures/plugins/floorplan.json")).unwrap()
}

fn apply(e: &mut Engine, cmds: Vec<Command>) -> Vec<EntityId> {
    e.apply(Transaction::new("fixture", cmds), ApplyOptions::default()).unwrap().created
}

fn create(shape: Shape) -> Command {
    Command::CreateEntity { id: None, entity: NewEntity { geometry: Some(shape), ..NewEntity::default() } }
}

/// A drawing with every built-in kind, rules, a dimension, an asset, view state and
/// forward-compatible extras (an unknown container entry and manifest field).
fn drawing() -> Vec<u8> {
    let mut e = Engine::default();
    let ids = apply(
        &mut e,
        vec![
            create(Shape::Line(Segment::new(Point::new(0.0, 0.0), Point::new(1000.0, 0.0)))),
            create(Shape::Line(Segment::new(Point::new(1000.0, 0.0), Point::new(1000.0, 600.0)))),
            create(Shape::Circle(Circle { center: Point::new(500.0, 300.0), radius: 120.0 })),
            create(Shape::Arc(Arc::new(Point::new(1500.0, 0.0), 200.0, 0.0, 2.0).unwrap())),
            create(Shape::Polyline(Polyline {
                points: vec![Point::new(0.0, 800.0), Point::new(400.0, 800.0), Point::new(800.0, 800.0)],
                bulges: vec![0.0, 0.5],
                closed: false,
            })),
            create(Shape::Text(Text {
                position: Point::new(0.0, 1000.0),
                content: "Ölçü planı — İğdır".into(),
                height: 50.0,
                rotation: 0.0,
                halign: HAlign::Left,
                valign: VAlign::Baseline,
            })),
        ],
    );
    let (a, b) = (ids[0], ids[1]);
    let rule = |rule| Command::AddConstraint {
        id: None,
        constraint: ConstraintSpec { rule, strength: Default::default(), enabled: true, label: None, source: None },
    };
    apply(
        &mut e,
        vec![
            rule(RuleSpec::Coincident { a: AnchorRef::new(a, "end"), b: AnchorRef::new(b, "start") }),
            rule(RuleSpec::Length { line: LineRef::of(a), value: 1000.0 }),
            rule(RuleSpec::Perpendicular { a: LineRef::of(a), b: LineRef::of(b) }),
            Command::CreateEntity {
                id: None,
                entity: NewEntity {
                    type_id: Some(TypeId::new("dotloom.dimension").unwrap()),
                    props: [
                        ("kind".to_owned(), PropValue::Text("linear".into())),
                        ("a".to_owned(), PropValue::Anchor(AnchorRef::new(a, "start"))),
                        ("b".to_owned(), PropValue::Anchor(AnchorRef::new(a, "end"))),
                    ]
                    .into_iter()
                    .collect(),
                    ..NewEntity::default()
                },
            },
        ],
    );
    let mut file = DotlFile::new(e.document().clone());
    let png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 0];
    let path = add_asset(&mut file, "png", png);
    let first = file.document.order()[0];
    file.document.entity_mut(first).unwrap().props.insert("image".into(), PropValue::Text(format!("asset:{path}")));
    file.view = Some(json!({ "camera": { "center": [500.0, 400.0], "scale": 0.5 } }));
    file.unknown_entries.insert("extensions/acme.json".into(), br#"{"note":"kept by older readers"}"#.to_vec());
    file.manifest_extra.insert("acme".into(), json!({ "license": "internal" }));
    write_dotl(&file).unwrap()
}

/// Floor-plan plugin objects with a stored fallback representation.
fn plugin_drawing() -> Vec<u8> {
    let mut e = Engine::default();
    for d in floorplan_defs() {
        e.register_type(d, "floorplan").unwrap();
    }
    let wall = EntityId(e.reserve_ids(1)[0]);
    apply(
        &mut e,
        vec![
            Command::CreateEntity {
                id: Some(wall),
                entity: NewEntity {
                    type_id: Some(TypeId::new("floorplan.wall").unwrap()),
                    props: [
                        ("start".to_owned(), PropValue::Point(Point::ORIGIN)),
                        ("end".to_owned(), PropValue::Point(Point::new(4000.0, 0.0))),
                    ]
                    .into_iter()
                    .collect(),
                    ..NewEntity::default()
                },
            },
            Command::CreateEntity {
                id: None,
                entity: NewEntity {
                    type_id: Some(TypeId::new("floorplan.door").unwrap()),
                    props: [("host".to_owned(), PropValue::Ref(RefValue { entity: wall }))].into_iter().collect(),
                    ..NewEntity::default()
                },
            },
        ],
    );
    let mut doc = e.document().clone();
    for id in doc.order().to_vec() {
        let shapes: Vec<Shape> = e.evaluate(id).unwrap().shapes().collect();
        doc.entity_mut(id).unwrap().fallback = Some(shapes);
    }
    write_dotl(&DotlFile::new(doc)).unwrap()
}

fn manifest(extra: Value) -> Vec<u8> {
    let mut m = json!({
        "format": "dotloom", "formatVersion": 1, "schemaVersion": 1,
        "producer": { "name": "dotloom", "version": "0.1.0" }, "plugins": [], "assets": []
    });
    if let (Some(m), Some(x)) = (m.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            m.insert(k.clone(), v.clone());
        }
    }
    serde_json::to_vec_pretty(&m).unwrap()
}

const EMPTY_DOC: &str = r#"{"schema":1,"layers":[{"id":1,"name":"Layer 1"}],"entities":[],"nextId":2}"#;

fn container(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    write(&entries.into_iter().map(|(n, d)| (n.to_owned(), d)).collect::<Vec<_>>()).unwrap()
}

#[test]
fn drawing_written_by_0_1_0_opens_and_keeps_everything() {
    let bytes = fixture("drawing-0.1.0.dotl", drawing);
    let (file, report) = read_dotl(&bytes, &DotlLimits::default()).unwrap();
    assert!(report.migrations.is_empty());
    let doc = &file.document;
    let kinds: Vec<&str> = doc.order().iter().map(|id| doc.entity(*id).unwrap().type_id.as_str()).collect();
    for k in ["dotloom.line", "dotloom.circle", "dotloom.arc", "dotloom.polyline", "dotloom.text", "dotloom.dimension"]
    {
        assert!(kinds.contains(&k), "{k} in {kinds:?}");
    }
    assert_eq!(doc.constraints().count(), 3);
    // Text and Unicode survive.
    let text = doc.order().iter().find_map(|id| match &doc.entity(*id)?.geometry {
        Some(Shape::Text(t)) => Some(t.content.clone()),
        _ => None,
    });
    assert_eq!(text.as_deref(), Some("Ölçü planı — İğdır"));
    // Asset, view state and forward-compatible extras are kept.
    assert_eq!(file.assets.len(), 1);
    assert_eq!(file.view.as_ref().and_then(|v| v.pointer("/camera/scale")), Some(&json!(0.5)));
    assert!(file.unknown_entries.contains_key("extensions/acme.json"));
    assert!(report.warnings.iter().any(|w| w.contains("extensions/acme.json")));
    assert_eq!(file.manifest_extra.get("acme"), Some(&json!({ "license": "internal" })));
    // The engine opens it, every hard rule holds, and the dimension measures 1000.
    let mut e = Engine::default();
    e.load(file.document.clone()).unwrap();
    assert!(e.verify().is_empty(), "{:?}", e.verify());
    let dim = doc.order().iter().copied().find(|id| doc.entity(*id).unwrap().type_id.as_str() == "dotloom.dimension");
    assert!((e.evaluate(dim.unwrap()).unwrap().measured.unwrap() - 1000.0).abs() < 1e-9);
    // Re-saving keeps the content (hash) and the extras.
    let again = write_dotl(&file).unwrap();
    let (file2, _) = read_dotl(&again, &DotlLimits::default()).unwrap();
    assert_eq!(file2.document.content_hash().unwrap(), file.document.content_hash().unwrap());
    assert_eq!(file2.unknown_entries, file.unknown_entries);
    assert_eq!(file2.manifest_extra, file.manifest_extra);
    assert_eq!(file2.assets, file.assets);
}

#[test]
fn plugin_objects_open_read_only_without_the_plugin_and_keep_their_data() {
    let bytes = fixture("floorplan-plugin-0.1.0.dotl", plugin_drawing);
    let (file, report) = read_dotl(&bytes, &DotlLimits::default()).unwrap();
    let required: Vec<String> = report.plugins.iter().map(|p| p.type_id.to_string()).collect();
    assert_eq!(required, ["floorplan.door", "floorplan.wall"]);
    let mut bare = Engine::default();
    bare.load(file.document.clone()).unwrap();
    for id in file.document.order() {
        let ev = bare.evaluate(*id).unwrap();
        assert!(matches!(ev.read_only, Some(ReadOnly::MissingPlugin { .. })), "{:?}", ev.read_only);
        // The stored fallback is what gets drawn.
        assert!(!ev.drawables.is_empty());
    }
    // Saved again without the plugin: the plugin payload is byte-for-byte the same JSON.
    let resaved = write_dotl(&DotlFile::new(bare.document().clone())).unwrap();
    let (file2, _) = read_dotl(&resaved, &DotlLimits::default()).unwrap();
    for id in file.document.order() {
        assert_eq!(file2.document.entity(*id).unwrap().props, file.document.entity(*id).unwrap().props);
    }
    // With the plugin registered, the objects are editable again.
    let mut full = Engine::default();
    for d in floorplan_defs() {
        full.register_type(d, "floorplan").unwrap();
    }
    full.load(file.document.clone()).unwrap();
    assert!(file.document.order().iter().all(|id| full.evaluate(*id).unwrap().read_only.is_none()));
}

#[test]
fn damaged_and_future_files_are_rejected_with_specific_errors() {
    let limits = DotlLimits::default();
    let good = fixture("drawing-0.1.0.dotl", drawing);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("truncated.dotl", good[..good.len() / 2].to_vec()),
        (
            "corrupt-document-json.dotl",
            container(vec![
                ("manifest.json", manifest(json!({}))),
                ("document.json", b"{\"schema\":1,\"layers\":[".to_vec()),
            ]),
        ),
        ("missing-document.dotl", container(vec![("manifest.json", manifest(json!({})))])),
        (
            "missing-asset.dotl",
            container(vec![
                (
                    "manifest.json",
                    manifest(json!({ "assets": [{
                        "path": "assets/00.png", "mediaType": "image/png", "size": 4,
                        "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
                    }] })),
                ),
                ("document.json", EMPTY_DOC.as_bytes().to_vec()),
            ]),
        ),
        (
            "future-format.dotl",
            container(vec![
                ("manifest.json", manifest(json!({ "formatVersion": 99 }))),
                ("document.json", EMPTY_DOC.as_bytes().to_vec()),
            ]),
        ),
        (
            "future-schema.dotl",
            container(vec![
                ("manifest.json", manifest(json!({ "schemaVersion": 99 }))),
                ("document.json", EMPTY_DOC.replace("\"schema\":1", "\"schema\":99").into_bytes()),
            ]),
        ),
        ("not-a-zip.dotl", b"%PDF-1.7 this is not a dotl file".to_vec()),
    ];
    for (name, make) in cases {
        let bytes = fixture(name, || make.clone());
        let err = read_dotl(&bytes, &limits).expect_err(name);
        let ok = match name {
            "truncated.dotl" | "not-a-zip.dotl" => matches!(err, DotlError::Zip(_)),
            "corrupt-document-json.dotl" => {
                matches!(&err, DotlError::Document(m) if m.contains("EOF") || m.contains("parse"))
            }
            "missing-document.dotl" => matches!(err, DotlError::MissingEntry("document.json")),
            "missing-asset.dotl" => {
                matches!(&err, DotlError::Asset(p, m) if p == "assets/00.png" && m.contains("missing"))
            }
            "future-format.dotl" => matches!(err, DotlError::FutureFormat { found: 99, .. }),
            "future-schema.dotl" => matches!(&err, DotlError::Document(m) if m.contains("99")),
            _ => false,
        };
        assert!(ok, "{name}: unexpected error {err:?}");
    }
}

#[test]
fn svg_fixture_reports_every_loss_and_imports_no_active_content() {
    let svg = std::fs::read_to_string(dir("svg").join("unsupported.svg")).unwrap();
    let imp = import_svg(&svg).unwrap();
    // The line, rectangle, circle and text of the plan are imported.
    assert!(imp.entities.len() >= 4, "{:?}", imp.entities);
    assert!(imp.layers.contains(&"plan".to_owned()));
    let r = &imp.report;
    assert!(r.losses.iter().any(|l| l.kind == LossKind::Unsupported && l.what.contains("script")), "{r:?}");
    assert!(r.losses.iter().any(|l| l.what == "foreignObject"), "{r:?}");
    assert!(r.count(LossKind::ExternalReference) >= 2, "image and use: {r:?}");
    assert!(r.count(LossKind::Style) >= 1, "style sheet, gradient or filter: {r:?}");
    let model = format!("{:?}", imp.entities);
    assert!(!model.contains("alert") && !model.contains("example.com") && !model.contains("onclick"));
}
