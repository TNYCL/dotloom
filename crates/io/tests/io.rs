//! File format tests (DL-FILE).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use dotloom_engine::{
    ApplyOptions, Command, ConstraintSpec, Engine, EntityTypeDef, NewEntity, Transaction,
    document::{AnchorRef, Document, EntityId, PropValue, RefValue, RuleSpec, TypeId},
    geometry::{Arc, Circle, HAlign, Path, PathEl, Point, Polyline, Rect, Segment, Shape, Text, VAlign},
};
use dotloom_io::{
    DotlError, DotlFile, DotlLimits, LossKind,
    dotl::{add_asset, read_dotl, save_atomic, write_dotl},
    dxf::{export_dxf, import_dxf},
    import_into,
    svg::{SvgExportOptions, export_svg, import_svg},
    zip::{Archive, ZipLimits, write},
};

fn floorplan_defs() -> Vec<EntityTypeDef> {
    serde_json::from_str(include_str!("../../../tests/fixtures/plugins/floorplan.json")).unwrap()
}

fn engine_with_content() -> Engine {
    let mut e = Engine::default();
    for d in floorplan_defs() {
        e.register_type(d, "floorplan").unwrap();
    }
    let shapes = vec![
        Shape::Line(Segment::new(Point::new(0.0, 0.0), Point::new(100.0, 0.0))),
        Shape::Circle(Circle { center: Point::new(50.0, 50.0), radius: 20.0 }),
        Shape::Arc(Arc::new(Point::new(200.0, 0.0), 30.0, 0.0, 1.5).unwrap()),
        Shape::Polyline(Polyline {
            points: vec![Point::new(0.0, 100.0), Point::new(50.0, 100.0), Point::new(100.0, 100.0)],
            bulges: vec![0.0, 1.0],
            closed: false,
        }),
        Shape::Rect(Rect { origin: Point::new(300.0, 0.0), width: 40.0, height: 20.0 }),
        Shape::Text(Text {
            position: Point::new(10.0, 150.0),
            content: "Ölçü ğüşıİç".into(),
            height: 5.0,
            rotation: 0.3,
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        }),
        Shape::Path(Path {
            elements: vec![
                PathEl::MoveTo(Point::new(0.0, 200.0)),
                PathEl::CubicTo(Point::new(10.0, 220.0), Point::new(30.0, 220.0), Point::new(40.0, 200.0)),
            ],
        }),
    ];
    let mut cmds: Vec<Command> = shapes
        .into_iter()
        .map(|s| Command::CreateEntity { id: None, entity: NewEntity { geometry: Some(s), ..NewEntity::default() } })
        .collect();
    let wall_id = e.reserve_ids(1)[0];
    cmds.push(Command::CreateEntity {
        id: Some(EntityId(wall_id)),
        entity: NewEntity {
            type_id: Some(TypeId::new("floorplan.wall").unwrap()),
            props: [
                ("start".to_owned(), PropValue::Point(Point::new(0.0, 400.0))),
                ("end".to_owned(), PropValue::Point(Point::new(3000.0, 400.0))),
            ]
            .into_iter()
            .collect(),
            ..NewEntity::default()
        },
    });
    cmds.push(Command::CreateEntity {
        id: None,
        entity: NewEntity {
            type_id: Some(TypeId::new("floorplan.door").unwrap()),
            props: [("host".to_owned(), PropValue::Ref(RefValue { entity: EntityId(wall_id) }))].into_iter().collect(),
            ..NewEntity::default()
        },
    });
    e.apply(Transaction::new("content", cmds), ApplyOptions::default()).unwrap();
    let line = EntityId(e.document().order()[0].0);
    e.apply(
        Transaction::new(
            "c",
            vec![Command::AddConstraint {
                id: None,
                constraint: ConstraintSpec {
                    rule: RuleSpec::Horizontal { a: AnchorRef::new(line, "start"), b: AnchorRef::new(line, "end") },
                    strength: Default::default(),
                    enabled: true,
                    label: None,
                    source: None,
                },
            }],
        ),
        ApplyOptions::default(),
    )
    .unwrap();
    e
}

// ---------------------------------------------------------------------------
// .dotl

#[test]
fn dotl_roundtrip_is_semantic_and_deterministic() {
    let e = engine_with_content();
    let mut file = DotlFile::new(e.document().clone());
    file.view = Some(serde_json::json!({"camera": {"x": 1.0, "y": 2.0, "zoom": 3.0}}));
    let asset = add_asset(&mut file, "png", vec![0x89, b'P', b'N', b'G', 1, 2, 3]);
    let first = file.document.order()[0];
    file.document.entity_mut(first).unwrap().props.insert("image".into(), PropValue::Text(format!("asset:{asset}")));
    file.unknown_entries.insert("future/thing.json".into(), b"{\"x\":1}".to_vec());
    let bytes = write_dotl(&file).unwrap();
    assert_eq!(write_dotl(&file).unwrap(), bytes, "writing is deterministic");
    let (back, report) = read_dotl(&bytes, &DotlLimits::default()).unwrap();
    assert!(back.document.semantic_eq(&file.document));
    assert_eq!(back.view, file.view);
    assert_eq!(back.assets, file.assets);
    assert_eq!(back.unknown_entries, file.unknown_entries);
    assert!(report.warnings.iter().any(|w| w.contains("future/thing.json")));
    assert_eq!(report.plugins.len(), 2);
    // Re-saving keeps everything.
    assert_eq!(write_dotl(&back).unwrap(), bytes);
}

#[test]
fn dotl_rejects_bad_containers() {
    let e = engine_with_content();
    let good = write_dotl(&DotlFile::new(e.document().clone())).unwrap();
    let lim = DotlLimits::default();
    // Truncated.
    assert!(read_dotl(&good[..good.len() / 2], &lim).is_err());
    // Not a zip.
    assert!(matches!(read_dotl(b"hello", &lim), Err(DotlError::Zip(_))));
    // Missing document.json.
    let only_manifest = write(&[(
        "manifest.json".into(),
        br#"{"format":"dotloom","formatVersion":1,"schemaVersion":1,"producer":{"name":"x","version":"1"}}"#.to_vec(),
    )])
    .unwrap();
    assert!(matches!(read_dotl(&only_manifest, &lim), Err(DotlError::MissingEntry("document.json"))));
    // Future container version.
    let future = write(&[
        (
            "manifest.json".into(),
            br#"{"format":"dotloom","formatVersion":9,"schemaVersion":1,"producer":{"name":"x","version":"9"}}"#
                .to_vec(),
        ),
        ("document.json".into(), b"{}".to_vec()),
    ])
    .unwrap();
    assert!(matches!(read_dotl(&future, &lim), Err(DotlError::FutureFormat { found: 9, .. })));
    // Future document schema.
    let future_doc = write(&[
        (
            "manifest.json".into(),
            br#"{"format":"dotloom","formatVersion":1,"schemaVersion":7,"producer":{"name":"x","version":"9"}}"#
                .to_vec(),
        ),
        ("document.json".into(), br#"{"schema":7,"nextId":1}"#.to_vec()),
    ])
    .unwrap();
    let err = read_dotl(&future_doc, &lim).unwrap_err();
    assert!(err.to_string().contains("newer than supported"), "{err}");
    // Corrupt JSON.
    let bad_json = write(&[
        (
            "manifest.json".into(),
            br#"{"format":"dotloom","formatVersion":1,"schemaVersion":1,"producer":{"name":"x","version":"1"}}"#
                .to_vec(),
        ),
        ("document.json".into(), b"{\"schema\":1,".to_vec()),
    ])
    .unwrap();
    assert!(matches!(read_dotl(&bad_json, &lim), Err(DotlError::Document(_))));
    // Duplicate entity IDs.
    let dup = write(&[
        (
            "manifest.json".into(),
            br#"{"format":"dotloom","formatVersion":1,"schemaVersion":1,"producer":{"name":"x","version":"1"}}"#
                .to_vec(),
        ),
        (
            "document.json".into(),
            br#"{"schema":1,"layers":[{"id":1,"name":"L"}],"nextId":9,"entities":[
                {"id":5,"type":"dotloom.point","layer":1,"geometry":{"type":"point","at":[0,0]}},
                {"id":5,"type":"dotloom.point","layer":1,"geometry":{"type":"point","at":[1,0]}}]}"#
                .to_vec(),
        ),
    ])
    .unwrap();
    assert!(matches!(read_dotl(&dup, &lim), Err(DotlError::Document(_))));
}

#[test]
fn dotl_path_traversal_and_bombs_are_rejected() {
    // Patch a valid archive's entry name to "../evil" (same length as "aa/evil").
    let mut z = write(&[("aa/evil".into(), b"x".to_vec())]).unwrap();
    let pat = b"aa/evil";
    let mut patched = 0;
    for i in 0..z.len() - pat.len() {
        if &z[i..i + pat.len()] == pat {
            z[i..i + 3].copy_from_slice(b"../");
            patched += 1;
        }
    }
    assert_eq!(patched, 2, "local and central headers");
    assert!(Archive::parse(&z, ZipLimits::default()).is_err());
    let bomb = write(&[("document.json".into(), vec![b' '; 8 << 20])]).unwrap();
    assert!(read_dotl(&bomb, &DotlLimits::default()).is_err());
}

#[test]
fn dotl_assets_are_verified() {
    let e = engine_with_content();
    let mut file = DotlFile::new(e.document().clone());
    let id = file.document.order()[0];
    file.document
        .entity_mut(id)
        .unwrap()
        .props
        .insert("image".into(), PropValue::Text("asset:assets/missing.png".into()));
    assert!(matches!(write_dotl(&file), Err(DotlError::Asset(..))));
    // A manifest checksum mismatch is detected on read.
    let mut ok = DotlFile::new(e.document().clone());
    let path = add_asset(&mut ok, "bin", vec![1, 2, 3, 4]);
    let bytes = write_dotl(&ok).unwrap();
    let mut tampered = bytes.clone();
    // Assets are stored (incompressible) — flip the payload byte "\x04".
    let pos = tampered.windows(4).position(|w| w == [1, 2, 3, 4]).unwrap();
    tampered[pos + 3] = 9;
    let err = read_dotl(&tampered, &DotlLimits::default()).unwrap_err();
    assert!(matches!(err, DotlError::Zip(_) | DotlError::Asset(..)), "{err:?} for {path}");
}

#[test]
fn atomic_save_replaces_or_keeps_the_old_file() {
    let dir = std::env::temp_dir().join(format!("dotloom-atomic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("plan.dotl");
    save_atomic(&target, b"first").unwrap();
    save_atomic(&target, b"second").unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"second");
    // Renaming over a non-empty directory fails: the directory and its content survive,
    // and no temporary file is left behind.
    let blocked = dir.join("blocked.dotl");
    std::fs::create_dir_all(blocked.join("inner")).unwrap();
    assert!(save_atomic(&blocked, b"x").is_err());
    assert!(blocked.join("inner").is_dir());
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// SVG

#[test]
fn svg_export_import_roundtrip_and_report() {
    let mut e = engine_with_content();
    let (svg, report) = export_svg(&mut e, &SvgExportOptions::default());
    assert!(svg.starts_with("<?xml"));
    assert_eq!(report.count(LossKind::Constraints), 1);
    assert_eq!(report.count(LossKind::PluginGeometry), 2);
    let imp = import_svg(&svg).unwrap();
    // 7 built-in shapes + wall polygon + door line and arc (as separate shapes).
    assert!(imp.entities.len() >= 9, "{}", imp.entities.len());
    let mut target = Engine::default();
    import_into(&mut target, "import", &imp.layers.iter().map(|n| (n.clone(), None)).collect::<Vec<_>>(), imp.entities)
        .unwrap();
    // The circle comes back as a circle with the same center/radius.
    let circle = target
        .document()
        .entities()
        .find_map(|x| match &x.geometry {
            Some(Shape::Circle(c)) => Some(*c),
            _ => None,
        })
        .unwrap();
    assert!(circle.center.distance(Point::new(50.0, 50.0)) < 1e-4 && (circle.radius - 20.0).abs() < 1e-4);
    // Text content survives (UTF-8).
    assert!(
        target.document().entities().any(|x| matches!(&x.geometry, Some(Shape::Text(t)) if t.content == "Ölçü ğüşıİç"))
    );
    // The bulge polyline came back with its semicircle (as a Bézier path; reported).
    assert!(imp.report.count(LossKind::Approximated) >= 1);
}

#[test]
fn svg_import_is_safe_and_reports_unsupported_content() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100mm" height="50mm" viewBox="0 0 200 100">
      <script>alert(1)</script>
      <style>.a{stroke:red}</style>
      <foreignObject><div xmlns="http://www.w3.org/1999/xhtml">x</div></foreignObject>
      <image href="https://example.com/x.png" width="10" height="10"/>
      <use xlink:href="#a"/>
      <g id="walls" transform="translate(10 10)">
        <line x1="0" y1="0" x2="100" y2="0" stroke="#ff0000" stroke-width="2"/>
        <rect x="0" y="10" width="20" height="10" rx="2" fill="url(#grad)"/>
        <ellipse cx="50" cy="50" rx="10" ry="5"/>
        <path d="M0 80 h10 v10 a5 5 0 0 1 10 0 q5 -5 10 0 t10 0 c 1 1 2 2 3 3 s 4 4 5 5 z"/>
      </g>
    </svg>"##;
    let imp = import_svg(svg).unwrap();
    let r = &imp.report;
    assert!(r.losses.iter().any(|l| l.what.contains("script")));
    assert!(r.losses.iter().any(|l| l.kind == LossKind::ExternalReference));
    assert!(r.losses.iter().any(|l| l.what == "foreignObject"));
    assert!(r.count(LossKind::Approximated) >= 2, "{r:?}");
    assert_eq!(imp.layers, vec!["Imported".to_owned(), "walls".to_owned()]);
    // viewBox 200 units over 100 mm: 0.5 mm per unit, Y flipped.
    let line = imp.entities.iter().find_map(|(_, e)| match &e.geometry {
        Some(Shape::Line(l)) => Some(*l),
        _ => None,
    });
    let l = line.unwrap();
    assert!(l.a.distance(Point::new(5.0, -5.0)) < 1e-9, "{l:?}");
    assert!(l.b.distance(Point::new(55.0, -5.0)) < 1e-9);
    // DTDs (entity expansion) are rejected outright.
    let dtd = r#"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "aaaaaaaa"><!ENTITY b "&a;&a;&a;&a;">]><svg xmlns="http://www.w3.org/2000/svg">&b;</svg>"#;
    assert!(import_svg(dtd).is_err());
    assert!(import_svg("<html/>").is_err());
}

// ---------------------------------------------------------------------------
// DXF

#[test]
fn dxf_fixtures_from_an_independent_writer() {
    let r12 = import_dxf(include_bytes!("../../../tests/fixtures/dxf/r12_basic.dxf")).unwrap();
    assert_eq!(r12.version.as_deref(), Some("AC1009"));
    assert!(r12.report.losses.iter().any(|l| l.kind == LossKind::Units), "R12 has no $INSUNITS");
    assert!(r12.layers.iter().any(|(n, c)| n == "WALLS" && c.is_some()));
    let text = r12.entities.iter().find_map(|(_, e)| match &e.geometry {
        Some(Shape::Text(t)) => Some(t.clone()),
        _ => None,
    });
    assert_eq!(text.unwrap().content, "Ölçü ğüşıİç", "cp1252 + \\U+ escapes decoded");
    assert!(r12.report.losses.iter().any(|l| l.what == "INSERT"), "block references are reported");

    let cm = import_dxf(include_bytes!("../../../tests/fixtures/dxf/r2000_cm.dxf")).unwrap();
    assert_eq!(cm.unit, dotloom_engine::geometry::units::LengthUnit::Centimetre);
    // LINE (0,0)-(100,0) cm = 1000 mm.
    assert!(
        cm.entities.iter().any(|(_, e)| matches!(&e.geometry, Some(Shape::Line(l)) if (l.b.x - 1000.0).abs() < 1e-9))
    );
    // LWPOLYLINE with a semicircle bulge on its second segment.
    assert!(
        cm.entities.iter().any(|(_, e)| matches!(&e.geometry, Some(Shape::Polyline(p)) if p.bulges == vec![0.0, 1.0]))
    );
    for unsupported in ["ELLIPSE", "SPLINE", "HATCH", "INSERT"] {
        assert!(cm.report.losses.iter().any(|l| l.what == unsupported), "{unsupported} must be reported");
    }
    assert!(cm.report.count(LossKind::Text) >= 1, "MTEXT formatting reported");

    let r2018 = import_dxf(include_bytes!("../../../tests/fixtures/dxf/r2018_mm.dxf")).unwrap();
    assert_eq!(r2018.version.as_deref(), Some("AC1032"));
    assert!(
        r2018.entities.iter().any(|(_, e)| matches!(&e.geometry, Some(Shape::Text(t)) if t.content == "Ölçü ğüşıİç"))
    );

    // OCS extrusion (0,0,-1): X mirrored.
    let m = import_dxf(include_bytes!("../../../tests/fixtures/dxf/r2000_mirrored.dxf")).unwrap();
    let c = m.entities.iter().find_map(|(_, e)| match &e.geometry {
        Some(Shape::Circle(c)) => Some(*c),
        _ => None,
    });
    assert!(c.unwrap().center.distance(Point::new(-10.0, 5.0)) < 1e-9);
    let a = m.entities.iter().find_map(|(_, e)| match &e.geometry {
        Some(Shape::Arc(a)) => Some(*a),
        _ => None,
    });
    // OCS arc 0°..90° around (10,5) mirrored: from (-14,5) to (-10,9).
    let a = a.unwrap();
    assert!(a.start_point().distance(Point::new(-14.0, 5.0)) < 1e-9, "{:?}", a.start_point());
    assert!(a.end_point().distance(Point::new(-10.0, 9.0)) < 1e-9, "{:?}", a.end_point());
}

#[test]
fn dxf_export_roundtrip() {
    let mut e = engine_with_content();
    let (dxf, report) = export_dxf(&mut e);
    assert!(dxf.contains("AC1009"));
    assert_eq!(report.count(LossKind::PluginGeometry), 2);
    assert!(report.count(LossKind::Approximated) >= 1, "Bézier flattened");
    let back = import_dxf(dxf.as_bytes()).unwrap();
    assert_eq!(back.unit, dotloom_engine::geometry::units::LengthUnit::Millimetre);
    let arc = back.entities.iter().find_map(|(_, e)| match &e.geometry {
        Some(Shape::Arc(a)) => Some(*a),
        _ => None,
    });
    let a = arc.unwrap();
    assert!(a.center.distance(Point::new(200.0, 0.0)) < 1e-9 && (a.sweep - 1.5).abs() < 1e-9);
    assert!(
        back.entities
            .iter()
            .any(|(_, e)| matches!(&e.geometry, Some(Shape::Polyline(p)) if p.bulges == vec![0.0, 1.0]))
    );
    assert!(
        back.entities.iter().any(|(_, e)| matches!(&e.geometry, Some(Shape::Text(t)) if t.content == "Ölçü ğüşıİç"))
    );
}

#[test]
fn dxf_rejects_binary_and_garbage() {
    assert!(import_dxf(b"AutoCAD Binary DXF\r\n\x1a\x00").is_err());
    assert!(import_dxf(b"not a number\nSECTION\n").is_err());
    let empty = import_dxf(b"").unwrap();
    assert!(empty.entities.is_empty());
}

#[test]
fn loaded_file_round_trips_through_engine() {
    let e = engine_with_content();
    let bytes = write_dotl(&DotlFile::new(e.document().clone())).unwrap();
    let (file, _) = read_dotl(&bytes, &DotlLimits::default()).unwrap();
    let mut e2 = Engine::default();
    for d in floorplan_defs() {
        e2.register_type(d, "floorplan").unwrap();
    }
    e2.load(file.document).unwrap();
    let a: Document = e.document().clone();
    assert!(e2.document().semantic_eq(&a));
}

#[test]
fn dxf_export_reports_text_it_cannot_represent() {
    // R12 TEXT has one line and \U+XXXX escapes cover the Basic Multilingual Plane
    // only: both changes must appear in the report, nothing silently.
    let mut e = Engine::default();
    let text = |content: &str, y: f64| Command::CreateEntity {
        id: None,
        entity: NewEntity {
            geometry: Some(Shape::Text(Text {
                position: Point::new(0.0, y),
                content: content.into(),
                height: 5.0,
                rotation: 0.0,
                halign: HAlign::Left,
                valign: VAlign::Baseline,
            })),
            ..NewEntity::default()
        },
    };
    e.apply(
        Transaction::new("t", vec![text("iki\nsatır", 0.0), text("emoji 😀 ve ğ", 20.0), text("düz", 40.0)]),
        ApplyOptions::default(),
    )
    .unwrap();
    let (dxf, report) = export_dxf(&mut e);
    assert!(dxf.contains(r"iki sat\U+0131r"), "{dxf}");
    assert!(dxf.contains(r"emoji ? ve \U+011F"), "{dxf}");
    assert!(dxf.contains(r"d\U+00FCz"));
    assert_eq!(report.count(LossKind::Text), 2, "{report:?}");
    let back = import_dxf(dxf.as_bytes()).unwrap();
    let contents: Vec<String> = back
        .entities
        .iter()
        .filter_map(|(_, e)| match &e.geometry {
            Some(Shape::Text(t)) => Some(t.content.clone()),
            _ => None,
        })
        .collect();
    assert!(contents.contains(&"iki satır".to_owned()) && contents.contains(&"düz".to_owned()), "{contents:?}");
}
