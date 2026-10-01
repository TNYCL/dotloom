//! Document model tests (DL-DOC).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use dotloom_document::{
    AnchorRef, Constraint, ConstraintId, DocError, Document, Entity, EntityId, Group, GroupId, LayerId, Limits,
    LineRef, PropValue, RefValue, RuleSpec, SCHEMA_VERSION, TypeId,
    builtin::{builtin_type_for, types},
};
use dotloom_geometry::{Affine, Circle, Point, Segment, Shape, ShapeKind, Vector};
use serde_json::json;

fn line(doc: &mut Document, a: (f64, f64), b: (f64, f64)) -> EntityId {
    let id = EntityId(doc.alloc_id());
    doc.insert_entity(
        Entity::new(id, builtin_type_for(ShapeKind::Line), LayerId(1))
            .with_geometry(Shape::Line(Segment::new(Point::new(a.0, a.1), Point::new(b.0, b.1)))),
    );
    id
}

fn wall(doc: &mut Document) -> EntityId {
    let id = EntityId(doc.alloc_id());
    doc.insert_entity(
        Entity::new(id, TypeId::new("acme.wall").unwrap(), LayerId(1))
            .with_prop("start", PropValue::Point(Point::new(0.0, 0.0)))
            .with_prop("end", PropValue::Point(Point::new(4000.0, 0.0)))
            .with_prop("thickness", PropValue::Number(200.0)),
    );
    id
}

fn door(doc: &mut Document, host: EntityId) -> EntityId {
    let id = EntityId(doc.alloc_id());
    doc.insert_entity(
        Entity::new(id, TypeId::new("acme.door").unwrap(), LayerId(1))
            .with_prop("host", PropValue::Ref(RefValue { entity: host }))
            .with_prop("offset", PropValue::Number(500.0))
            .with_prop("width", PropValue::Number(900.0)),
    );
    id
}

fn constraint(doc: &mut Document, rule: RuleSpec) -> ConstraintId {
    let id = ConstraintId(doc.alloc_id());
    doc.upsert_constraint(Constraint::new(id, rule));
    id
}

#[test]
fn json_roundtrip_is_semantically_equal() {
    let mut doc = Document::new();
    let a = line(&mut doc, (0.0, 0.0), (100.0, 0.0));
    let b = line(&mut doc, (100.0, 0.0), (100.0, 50.0));
    constraint(&mut doc, RuleSpec::Coincident { a: AnchorRef::new(a, "end"), b: AnchorRef::new(b, "start") });
    constraint(&mut doc, RuleSpec::Perpendicular { a: LineRef::of(a), b: LineRef::of(b) });
    let w = wall(&mut doc);
    door(&mut doc, w);
    doc.validate().unwrap();
    let json = doc.to_json_string().unwrap();
    let (back, notes) = Document::from_json_str(&json, &Limits::default()).unwrap();
    assert!(notes.is_empty());
    assert!(back.semantic_eq(&doc));
    assert_eq!(back.content_hash().unwrap(), doc.content_hash().unwrap());
    // Byte-identical re-serialization.
    assert_eq!(back.to_json_string().unwrap(), json);
}

#[test]
fn hash_ignores_insertion_order_but_not_draw_order() {
    let mut d1 = Document::new();
    let mut d2 = Document::new();
    // Same IDs, constraints inserted in different order.
    for d in [&mut d1, &mut d2] {
        line(d, (0.0, 0.0), (1.0, 0.0));
        line(d, (1.0, 0.0), (1.0, 1.0));
    }
    let r1 = RuleSpec::Horizontal { a: AnchorRef::new(EntityId(2), "start"), b: AnchorRef::new(EntityId(2), "end") };
    let r2 = RuleSpec::Vertical { a: AnchorRef::new(EntityId(3), "start"), b: AnchorRef::new(EntityId(3), "end") };
    d1.upsert_constraint(Constraint::new(ConstraintId(10), r1.clone()));
    d1.upsert_constraint(Constraint::new(ConstraintId(11), r2.clone()));
    d2.upsert_constraint(Constraint::new(ConstraintId(11), r2));
    d2.upsert_constraint(Constraint::new(ConstraintId(10), r1));
    assert_eq!(d1.content_hash().unwrap(), d2.content_hash().unwrap());
    // Draw order is semantic.
    d2.reorder(EntityId(3), 0).unwrap();
    assert_ne!(d1.content_hash().unwrap(), d2.content_hash().unwrap());
}

#[test]
fn negative_zero_is_normalized() {
    let mut d1 = Document::new();
    let mut d2 = Document::new();
    line(&mut d1, (0.0, 0.0), (1.0, 0.0));
    line(&mut d2, (-0.0, 0.0), (1.0, -0.0));
    assert!(d1.semantic_eq(&d2));
}

#[test]
fn reorder_keeps_ids() {
    let mut doc = Document::new();
    let a = line(&mut doc, (0.0, 0.0), (1.0, 0.0));
    let b = line(&mut doc, (0.0, 1.0), (1.0, 1.0));
    let c = line(&mut doc, (0.0, 2.0), (1.0, 2.0));
    doc.reorder(a, 2).unwrap();
    assert_eq!(doc.order(), &[b, c, a]);
    assert!(doc.entity(a).is_some());
    assert!(doc.reorder(EntityId(999), 0).is_err());
}

#[test]
fn unknown_fields_and_plugin_payloads_survive() {
    let j = json!({
        "schema": SCHEMA_VERSION,
        "layers": [{"id": 1, "name": "L", "futureLayerFlag": true}],
        "entities": [{
            "id": 2, "type": "vendor.gadget", "layer": 1,
            "props": {"size": 3.0},
            "data": {"vendor.gadget": {"nested": [1, 2, {"deep": "x"}]}},
            "futureEntityField": {"a": 1}
        }],
        "nextId": 3,
        "futureTopLevel": "keep me"
    });
    let (doc, _) = Document::from_json_value(j, &Limits::default()).unwrap();
    let out: serde_json::Value = serde_json::from_str(&doc.to_json_string().unwrap()).unwrap();
    assert_eq!(out["futureTopLevel"], "keep me");
    assert_eq!(out["layers"][0]["futureLayerFlag"], true);
    assert_eq!(out["entities"][0]["futureEntityField"]["a"], 1);
    assert_eq!(out["entities"][0]["data"]["vendor.gadget"]["nested"][2]["deep"], "x");
}

fn expect_err(j: serde_json::Value) -> DocError {
    match Document::from_json_value(j, &Limits::default()) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

#[test]
fn schema_version_handling() {
    assert!(matches!(
        expect_err(json!({"schema": SCHEMA_VERSION + 1, "nextId": 1})),
        DocError::FutureSchema { found, .. } if found == SCHEMA_VERSION + 1
    ));
    assert!(matches!(expect_err(json!({"schema": 0, "nextId": 1})), DocError::UnsupportedSchema(0)));
    assert!(matches!(expect_err(json!({"nextId": 1})), DocError::Malformed(_)));
}

#[test]
fn invariant_violations_are_rejected() {
    let base = |entities: serde_json::Value, extra: serde_json::Value| {
        let mut v =
            json!({"schema": SCHEMA_VERSION, "layers": [{"id": 1, "name": "L"}], "entities": entities, "nextId": 100});
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, x) in e {
                o.insert(k.clone(), x.clone());
            }
        }
        v
    };
    let line = |id: u64| json!({"id": id, "type": "dotloom.line", "layer": 1, "geometry": {"type": "line", "a": [0.0, 0.0], "b": [1.0, 0.0]}});
    // Duplicate entity id.
    assert!(matches!(
        expect_err(base(json!([line(5), line(5)]), json!({}))),
        DocError::Malformed(_) | DocError::DuplicateId(_)
    ));
    // ID used by two kinds.
    assert!(matches!(expect_err(base(json!([line(1)]), json!({}))), DocError::DuplicateId(_)));
    // ID not below nextId.
    assert!(matches!(expect_err(base(json!([line(500)]), json!({}))), DocError::InvalidValue(_)));
    // Unknown layer.
    let mut bad_layer = line(5);
    bad_layer["layer"] = json!(77);
    assert!(matches!(expect_err(base(json!([bad_layer]), json!({}))), DocError::UnknownLayer(_)));
    // Wrong geometry kind for a built-in type.
    let mut wrong = line(5);
    wrong["type"] = json!("dotloom.circle");
    assert!(matches!(expect_err(base(json!([wrong]), json!({}))), DocError::InvalidGeometry { .. }));
    // Singular transform.
    let mut singular = line(5);
    singular["transform"] = json!([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert!(matches!(expect_err(base(json!([singular]), json!({}))), DocError::InvalidValue(_)));
    // Broken reference property.
    let gadget = json!({"id": 6, "type": "vendor.gadget", "layer": 1, "props": {"host": {"ref": 42}}});
    assert!(matches!(expect_err(base(json!([gadget]), json!({}))), DocError::BrokenReference { .. }));
    // Missing built-in anchor in a constraint.
    let c = json!([{"id": 7, "rule": {"kind": "fixPoint", "a": {"entity": 5, "anchor": "center"}, "at": [0.0, 0.0]}}]);
    assert!(matches!(expect_err(base(json!([line(5)]), json!({"constraints": c}))), DocError::MissingAnchor { .. }));
    // Constraint on a missing entity.
    let c2 = json!([{"id": 7, "rule": {"kind": "radius", "circle": 55, "value": 3.0}}]);
    assert!(matches!(expect_err(base(json!([line(5)]), json!({"constraints": c2}))), DocError::BrokenReference { .. }));
    // Group cycle.
    let groups = json!([{"id": 8, "members": [], "children": [9]}, {"id": 9, "members": [], "children": [8]}]);
    assert!(matches!(expect_err(base(json!([line(5)]), json!({"groups": groups}))), DocError::GroupCycle(_)));
    // Entity in two groups.
    let groups2 = json!([{"id": 8, "members": [5]}, {"id": 9, "members": [5]}]);
    assert!(matches!(expect_err(base(json!([line(5)]), json!({"groups": groups2}))), DocError::MultipleParents(_)));
    // Negative distance.
    let c3 = json!([{"id": 7, "rule": {"kind": "distance", "a": {"entity": 5, "anchor": "start"}, "b": {"entity": 5, "anchor": "end"}, "value": -1.0}}]);
    assert!(matches!(expect_err(base(json!([line(5)]), json!({"constraints": c3}))), DocError::InvalidValue(_)));
    // Deep nesting.
    let mut deep = json!(1);
    for _ in 0..200 {
        deep = json!([deep]);
    }
    let mut g = json!({"id": 6, "type": "vendor.gadget", "layer": 1});
    g["data"] = json!({ "vendor.gadget": deep });
    assert!(Document::from_json_value(base(json!([g]), json!({})), &Limits::default()).is_err());
}

#[test]
fn limits_are_enforced() {
    let mut doc = Document::new();
    for i in 0..5 {
        line(&mut doc, (0.0, f64::from(i)), (1.0, f64::from(i)));
    }
    let tight = Limits { max_entities: 3, ..Limits::default() };
    assert!(doc.validate_all(&tight).iter().any(|e| matches!(e, DocError::LimitExceeded(_))));
}

#[test]
fn non_finite_props_are_rejected() {
    let mut doc = Document::new();
    let w = wall(&mut doc);
    if let Some(e) = doc.entity_mut(w) {
        e.props.insert("thickness".into(), PropValue::Number(f64::NAN));
    }
    assert!(doc.validate().is_err());
}

#[test]
fn copy_paste_remaps_internal_and_keeps_external_refs() {
    let mut doc = Document::new();
    let a = line(&mut doc, (0.0, 0.0), (100.0, 0.0));
    let b = line(&mut doc, (100.0, 0.0), (100.0, 50.0));
    let other = line(&mut doc, (0.0, 500.0), (10.0, 500.0));
    constraint(&mut doc, RuleSpec::Coincident { a: AnchorRef::new(a, "end"), b: AnchorRef::new(b, "start") });
    constraint(&mut doc, RuleSpec::FixPoint { a: AnchorRef::new(a, "start"), at: Point::new(0.0, 0.0) });
    // Constraint reaching outside the selection is not copied.
    constraint(&mut doc, RuleSpec::Parallel { a: LineRef::of(a), b: LineRef::of(other) });
    let w = wall(&mut doc);
    let d = door(&mut doc, w);
    let gid = GroupId(doc.alloc_id());
    doc.upsert_group(Group {
        id: gid,
        name: Some("pair".into()),
        members: vec![a, b],
        children: vec![],
        extra: Default::default(),
    });

    let clip = doc.copy(&[a, b, d]);
    assert_eq!(clip.entities.len(), 3);
    assert_eq!(clip.constraints.len(), 2);
    assert_eq!(clip.groups.len(), 1);

    let report = doc.paste(&clip, Vector::new(1000.0, 0.0), LayerId(1));
    doc.validate().unwrap();
    let na = report.entities[&a];
    let nb = report.entities[&b];
    let nd = report.entities[&d];
    assert!(na != a && nb != b && nd != d);
    // Internal constraint remapped to the new IDs.
    let coincident = doc
        .constraints()
        .find(|c| matches!(&c.rule, RuleSpec::Coincident { a: x, b: y } if x.entity == na && y.entity == nb));
    assert!(coincident.is_some());
    // Fixed point moved with the paste offset.
    assert!(doc.constraints().any(
        |c| matches!(&c.rule, RuleSpec::FixPoint { a: x, at } if x.entity == na && *at == Point::new(1000.0, 0.0))
    ));
    // Transform carries the offset.
    assert_eq!(doc.entity(na).unwrap().transform, Affine::translate(Vector::new(1000.0, 0.0)));
    // External host reference kept (wall exists in this document).
    assert_eq!(doc.entity(nd).unwrap().props["host"], PropValue::Ref(RefValue { entity: w }));
    assert!(report.dropped_refs.is_empty());
    // Group copied with remapped members.
    let ng = report.groups[0];
    assert_eq!(doc.group(ng).unwrap().members, vec![na, nb]);

    // Pasting into another document drops the dangling host reference and reports it.
    let mut other_doc = Document::new();
    let r2 = other_doc.paste(&doc.copy(&[d]), Vector::ZERO, LayerId(1));
    let nd2 = r2.entities[&d];
    assert_eq!(r2.dropped_refs, vec![(nd2, "host".to_owned())]);
    other_doc.validate().unwrap();
}

#[test]
fn dependents_and_removal() {
    let mut doc = Document::new();
    let w = wall(&mut doc);
    let d = door(&mut doc, w);
    let a = line(&mut doc, (0.0, 0.0), (1.0, 0.0));
    let c = constraint(&mut doc, RuleSpec::Horizontal { a: AnchorRef::new(a, "start"), b: AnchorRef::new(a, "end") });
    let deps = doc.dependents(w);
    assert_eq!(deps.entities, vec![d]);
    assert_eq!(doc.dependents(a).constraints, vec![c]);
    let gid = GroupId(doc.alloc_id());
    doc.upsert_group(Group { id: gid, name: None, members: vec![a], children: vec![], extra: Default::default() });
    let (removed, idx) = doc.remove_entity(a).unwrap();
    assert_eq!(removed.id, a);
    assert_eq!(idx, 2);
    assert!(doc.group(gid).unwrap().members.is_empty());
    // The constraint now dangles: validation catches it until the engine removes it.
    assert!(doc.validate().is_err());
    doc.remove_constraint(c);
    doc.validate().unwrap();
}

#[test]
fn builtin_types_need_matching_geometry() {
    let mut doc = Document::new();
    let id = EntityId(doc.alloc_id());
    doc.insert_entity(Entity::new(id, TypeId::new(types::CIRCLE).unwrap(), LayerId(1)));
    assert!(doc.validate().is_err());
    if let Some(e) = doc.entity_mut(id) {
        e.geometry = Some(Shape::Circle(Circle { center: Point::ORIGIN, radius: 5.0 }));
    }
    doc.validate().unwrap();
}

#[test]
fn ids_are_never_reused() {
    let mut doc = Document::new();
    let a = line(&mut doc, (0.0, 0.0), (1.0, 0.0));
    doc.remove_entity(a);
    let b = line(&mut doc, (0.0, 0.0), (1.0, 0.0));
    assert!(b.0 > a.0);
}
