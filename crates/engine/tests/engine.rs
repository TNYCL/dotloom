//! End-to-end engine tests (DL-CMD, DL-SOLVE, DL-EXAMPLE, DL-PLUGIN).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use dotloom_engine::{
    ApplyOptions, Command, ConstraintSpec, DeletePolicy, DragSpec, EditMode, Engine, EngineError, EngineOptions,
    EntityPatch, EntityTypeDef, Event, NewEntity, ParamValue, PendingState, Transaction,
    constraints::{DiagnosticKind, Status},
    document::{
        AnchorRef, Cmp, Constraint, Document, EntityId, LineRef, ParamRef, PropValue, RefValue, RuleSpec, Term, TypeId,
    },
    geometry::{Affine, Point, Segment, Shape, Vector, units::TimeAxis},
};

fn shelf_def() -> EntityTypeDef {
    serde_json::from_str(include_str!("../../../tests/fixtures/plugins/shelf.json")).unwrap()
}

fn floorplan_defs() -> Vec<EntityTypeDef> {
    serde_json::from_str(include_str!("../../../tests/fixtures/plugins/floorplan.json")).unwrap()
}

fn timeline_def() -> EntityTypeDef {
    serde_json::from_str(include_str!("../../../tests/fixtures/plugins/timeline.json")).unwrap()
}

fn tx(label: &str, commands: Vec<Command>) -> Transaction {
    Transaction::new(label, commands)
}

fn apply(e: &mut Engine, commands: Vec<Command>) -> Result<dotloom_engine::CommitReport, EngineError> {
    e.apply(tx("test", commands), ApplyOptions::default())
}

fn create(e: &mut Engine, ne: NewEntity) -> EntityId {
    let r = apply(e, vec![Command::CreateEntity { id: None, entity: ne }]).unwrap();
    r.created[0]
}

fn line(e: &mut Engine, a: (f64, f64), b: (f64, f64)) -> EntityId {
    create(
        e,
        NewEntity {
            geometry: Some(Shape::Line(Segment::new(Point::new(a.0, a.1), Point::new(b.0, b.1)))),
            ..NewEntity::default()
        },
    )
}

fn add_constraint(e: &mut Engine, rule: RuleSpec) -> Result<dotloom_engine::CommitReport, EngineError> {
    apply(
        e,
        vec![Command::AddConstraint {
            id: None,
            constraint: ConstraintSpec { rule, strength: Default::default(), enabled: true, label: None, source: None },
        }],
    )
}

fn set(
    e: &mut Engine,
    id: EntityId,
    param: &str,
    value: f64,
    mode: EditMode,
) -> Result<dotloom_engine::CommitReport, EngineError> {
    apply(e, vec![Command::SetParams { values: vec![ParamValue { entity: id, param: param.into(), value }], mode }])
}

fn p(e: &Engine, id: EntityId, name: &str) -> f64 {
    e.params_of(id)[name]
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// ---------------------------------------------------------------------------
// Shelf acceptance (DL-EXAMPLE-1)

fn shelf() -> (Engine, EntityId, dotloom_engine::document::ConstraintId) {
    let mut e = Engine::default();
    e.register_type(shelf_def(), "shelf").unwrap();
    let s = create(&mut e, NewEntity { type_id: Some(TypeId::new("shelf.unit").unwrap()), ..NewEntity::default() });
    assert!(close(p(&e, s, "width"), 1800.0) && close(p(&e, s, "w1"), 600.0));
    let lock = add_constraint(&mut e, RuleSpec::Fix { param: ParamRef::prop(s, "w1"), value: 600.0 })
        .unwrap()
        .created_constraints[0];
    (e, s, lock)
}

#[test]
fn creating_with_explicit_props_adapts_defaults_but_not_explicit_values() {
    let mut e = Engine::default();
    e.register_type(shelf_def(), "shelf").unwrap();
    // Only the width is given: the default compartments adapt to it.
    let mut props = std::collections::BTreeMap::new();
    props.insert("width".to_string(), PropValue::Number(1200.0));
    let s = create(
        &mut e,
        NewEntity { type_id: Some(TypeId::new("shelf.unit").unwrap()), props, ..NewEntity::default() },
    );
    let (w1, w2, w3) = (p(&e, s, "w1"), p(&e, s, "w2"), p(&e, s, "w3"));
    assert!(close(p(&e, s, "width"), 1200.0));
    assert!(close(w1 + w2 + w3, 1200.0) && close(w2, w3) && w2 >= 400.0 - 1e-6, "{w1} {w2} {w3}");
    // Explicit values that contradict the rules are still rejected.
    let mut bad = std::collections::BTreeMap::new();
    bad.insert("width".to_string(), PropValue::Number(1200.0));
    bad.insert("w1".to_string(), PropValue::Number(600.0));
    bad.insert("w2".to_string(), PropValue::Number(600.0));
    let err = apply(
        &mut e,
        vec![Command::CreateEntity {
            id: None,
            entity: NewEntity { type_id: Some(TypeId::new("shelf.unit").unwrap()), props: bad, ..NewEntity::default() },
        }],
    )
    .unwrap_err();
    assert!(matches!(err, EngineError::Solve { .. }), "{err:?}");
}

#[test]
fn shelf_180_160_130_with_undo_redo_and_unlock() {
    let (mut e, s, lock) = shelf();
    // 160 cm → 60/50/50.
    let r = set(&mut e, s, "width", 1600.0, EditMode::Exact).unwrap();
    assert!(r.status.is_acceptable());
    assert!(close(p(&e, s, "w1"), 600.0) && close(p(&e, s, "w2"), 500.0) && close(p(&e, s, "w3"), 500.0));
    let rev = e.revision();
    let hash = e.document().content_hash().unwrap();
    let _ = e.take_events();

    // 130 cm violates hard rules: rejected, document unchanged, bound 140 cm explained.
    let err = set(&mut e, s, "width", 1300.0, EditMode::Exact).unwrap_err();
    let EngineError::Solve { failure } = &err else { panic!("expected solve failure, got {err:?}") };
    assert_eq!(failure.status, Status::Conflicting);
    let d = failure.diagnostics.iter().find(|d| d.kind == DiagnosticKind::Conflict).expect("conflict diagnostic");
    assert!(d.constraints.contains(&lock), "{d:?}");
    assert!(d.edits.iter().any(|(id, param)| *id == s && param == "width"));
    assert!(!d.templates.is_empty());
    assert_eq!(failure.nearest.len(), 1);
    assert!(close(failure.nearest[0].feasible, 1400.0), "{:?}", failure.nearest);
    assert_eq!(e.revision(), rev);
    assert_eq!(e.document().content_hash().unwrap(), hash);
    assert!(e.take_events().is_empty(), "no events for failed transactions");

    // Unlock the left compartment: 130 cm becomes feasible.
    apply(&mut e, vec![Command::RemoveConstraint { id: lock }]).unwrap();
    set(&mut e, s, "width", 1300.0, EditMode::Exact).unwrap();
    let (w1, w2, w3) = (p(&e, s, "w1"), p(&e, s, "w2"), p(&e, s, "w3"));
    assert!(close(w1 + w2 + w3, 1300.0) && close(w2, w3) && w2 >= 400.0 - 1e-6);

    // Undo twice: back to 160 cm with the lock; redo restores the unlock.
    e.undo(ApplyOptions::default()).unwrap();
    e.undo(ApplyOptions::default()).unwrap();
    assert!(close(p(&e, s, "width"), 1600.0) && close(p(&e, s, "w2"), 500.0));
    assert!(e.document().constraint(lock).is_some());
    e.redo(ApplyOptions::default()).unwrap();
    assert!(e.document().constraint(lock).is_none());
    assert!(close(p(&e, s, "width"), 1600.0));
    // A new change clears the redo branch.
    set(&mut e, s, "width", 1500.0, EditMode::Exact).unwrap();
    assert!(!e.can_redo());
    assert!(matches!(e.redo(ApplyOptions::default()), Err(EngineError::NothingToRedo)));
}

#[test]
fn shelf_prefer_mode_clamps_to_the_feasible_bound() {
    let (mut e, s, _) = shelf();
    set(&mut e, s, "width", 1300.0, EditMode::Prefer).unwrap();
    assert!(close(p(&e, s, "width"), 1400.0), "{}", p(&e, s, "width"));
}

#[test]
fn stale_revision_is_rejected() {
    let (mut e, s, _) = shelf();
    let rev = e.revision();
    let err = e
        .apply(
            tx(
                "x",
                vec![Command::SetParams {
                    values: vec![ParamValue { entity: s, param: "width".into(), value: 1700.0 }],
                    mode: EditMode::Exact,
                }],
            ),
            ApplyOptions { expected_revision: Some(rev - 1) },
        )
        .unwrap_err();
    assert!(matches!(err, EngineError::Stale { .. }));
    assert_eq!(e.revision(), rev);
}

// ---------------------------------------------------------------------------
// Geometric constraints and transactions (DL-CMD)

#[test]
fn moving_a_constrained_line_drags_its_neighbour_and_fixed_points_hold() {
    let mut e = Engine::default();
    let a = line(&mut e, (0.0, 0.0), (100.0, 0.0));
    let b = line(&mut e, (100.0, 0.0), (100.0, 50.0));
    add_constraint(&mut e, RuleSpec::Coincident { a: AnchorRef::new(a, "end"), b: AnchorRef::new(b, "start") })
        .unwrap();
    add_constraint(&mut e, RuleSpec::Perpendicular { a: LineRef::of(a), b: LineRef::of(b) }).unwrap();
    add_constraint(&mut e, RuleSpec::Length { line: LineRef::of(b), value: 50.0 }).unwrap();
    // Move line a: line b follows (coincident) and stays perpendicular with length 50.
    apply(
        &mut e,
        vec![Command::Transform {
            ids: vec![a],
            transform: Affine::translate(Vector::new(10.0, 20.0)),
            policy: Default::default(),
        }],
    )
    .unwrap();
    let ea = e.evaluate(a).unwrap();
    let eb = e.evaluate(b).unwrap();
    assert!(ea.anchor("end").unwrap().distance(Point::new(110.0, 20.0)) < 1e-6);
    assert!(eb.anchor("start").unwrap().distance(ea.anchor("end").unwrap()) < 1e-6);
    assert!((eb.anchor("start").unwrap().distance(eb.anchor("end").unwrap()) - 50.0).abs() < 1e-6);
    // Pin the start of a: moving a is now rejected and nothing changes.
    add_constraint(&mut e, RuleSpec::FixPoint { a: AnchorRef::new(a, "start"), at: Point::new(10.0, 20.0) }).unwrap();
    let rev = e.revision();
    let r = apply(
        &mut e,
        vec![Command::Transform {
            ids: vec![a],
            transform: Affine::translate(Vector::new(5.0, 0.0)),
            policy: Default::default(),
        }],
    );
    assert!(matches!(r, Err(EngineError::Solve { .. })), "{r:?}");
    assert_eq!(e.revision(), rev);
}

#[test]
fn transaction_is_atomic() {
    let mut e = Engine::default();
    let a = line(&mut e, (0.0, 0.0), (100.0, 0.0));
    let before = e.document().content_hash().unwrap();
    // Second command fails: the first must not be applied.
    let r = apply(
        &mut e,
        vec![
            Command::UpdateEntity {
                id: a,
                patch: EntityPatch { name: Some("renamed".into()), ..EntityPatch::default() },
            },
            Command::UpdateEntity { id: EntityId(9999), patch: EntityPatch::default() },
        ],
    );
    assert!(r.is_err());
    assert_eq!(e.document().content_hash().unwrap(), before);
}

#[test]
fn delete_cascades_constraints_or_rejects() {
    let mut e = Engine::default();
    let a = line(&mut e, (0.0, 0.0), (100.0, 0.0));
    let b = line(&mut e, (100.0, 0.0), (100.0, 50.0));
    let c = add_constraint(&mut e, RuleSpec::Coincident { a: AnchorRef::new(a, "end"), b: AnchorRef::new(b, "start") })
        .unwrap()
        .created_constraints[0];
    let r = apply(&mut e, vec![Command::Delete { ids: vec![a], policy: DeletePolicy::Reject }]);
    assert!(r.is_err());
    let r = apply(&mut e, vec![Command::Delete { ids: vec![a], policy: DeletePolicy::Cascade }]).unwrap();
    assert_eq!(r.removed_constraints, vec![c]);
    assert!(e.document().constraint(c).is_none());
    e.document().validate().unwrap();
    // Undo restores entity and constraint together.
    e.undo(ApplyOptions::default()).unwrap();
    assert!(e.document().entity(a).is_some() && e.document().constraint(c).is_some());
    e.document().validate().unwrap();
}

#[test]
fn events_follow_commits() {
    let mut e = Engine::default();
    let _ = e.take_events();
    let a = line(&mut e, (0.0, 0.0), (1.0, 0.0));
    let ev = e.take_events();
    assert!(ev.iter().any(|x| matches!(x, Event::Committed { entities, .. } if entities.contains(&a))));
    assert!(ev.iter().any(|x| matches!(x, Event::HistoryChanged { can_undo: true, .. })));
}

#[test]
fn history_limit_drops_oldest() {
    let mut e = Engine::new(EngineOptions { history_entries: 3, ..EngineOptions::default() });
    for i in 0..6 {
        line(&mut e, (0.0, f64::from(i)), (1.0, f64::from(i)));
    }
    let mut n = 0;
    while e.undo(ApplyOptions::default()).is_ok() {
        n += 1;
    }
    assert_eq!(n, 3);
    assert_eq!(e.document().entity_count(), 3);
}

#[test]
fn pending_solve_can_be_cancelled_without_changes() {
    let (mut e, s, _) = shelf();
    let before = e.document().content_hash().unwrap();
    let id = e
        .begin_apply(
            tx(
                "w",
                vec![Command::SetParams {
                    values: vec![ParamValue { entity: s, param: "width".into(), value: 1600.0 }],
                    mode: EditMode::Exact,
                }],
            ),
            ApplyOptions::default(),
        )
        .unwrap();
    assert!(e.has_pending());
    // Another mutation is refused while busy.
    assert!(matches!(e.undo(ApplyOptions::default()), Err(EngineError::Busy { .. })));
    assert!(e.cancel_pending());
    assert_eq!(e.document().content_hash().unwrap(), before);
    // Stepping a finished/cancelled job reports that nothing is active.
    assert!(e.step_pending(1).is_err());
    let _ = id;
    // A fresh begin + steps commits.
    e.begin_apply(
        tx(
            "w",
            vec![Command::SetParams {
                values: vec![ParamValue { entity: s, param: "width".into(), value: 1600.0 }],
                mode: EditMode::Exact,
            }],
        ),
        ApplyOptions::default(),
    )
    .unwrap();
    loop {
        match e.step_pending(1).unwrap() {
            PendingState::Running { .. } => {}
            PendingState::Done(r) => {
                r.unwrap();
                break;
            }
        }
    }
    assert!(close(p(&e, s, "w2"), 500.0));
}

// ---------------------------------------------------------------------------
// Drag (DL-CMD-4/5)

#[test]
fn drag_previews_then_commits_one_entry_or_cancels() {
    let mut e = Engine::default();
    let a = line(&mut e, (0.0, 0.0), (100.0, 0.0));
    add_constraint(&mut e, RuleSpec::Length { line: LineRef::of(a), value: 100.0 }).unwrap();
    add_constraint(&mut e, RuleSpec::FixPoint { a: AnchorRef::new(a, "start"), at: Point::ORIGIN }).unwrap();
    let rev = e.revision();
    e.begin_drag(DragSpec::Anchor { entity: a, anchor: "end".into() }).unwrap();
    let mut last = None;
    for k in 1..=5 {
        let (pre, delta) = e.drag_to(Point::new(100.0, f64::from(k) * 20.0)).unwrap();
        assert!(pre.accepted, "{pre:?}");
        assert!(delta.preview && !delta.upserts.is_empty());
        last = Some(delta);
    }
    // Nothing committed during the drag.
    assert_eq!(e.revision(), rev);
    let r = e.end_drag(true).unwrap().unwrap();
    assert_eq!(r.revision, rev + 1);
    let end = e.evaluate(a).unwrap().anchor("end").unwrap();
    assert!((end.distance(Point::ORIGIN) - 100.0).abs() < 1e-6, "length kept: {end:?}");
    // The end moved towards the last pointer position (100, 100) on the circle.
    assert!((end.y / end.x - 1.0).abs() < 1e-3, "{end:?}");
    let _ = last;
    // One undo restores the pre-drag state.
    e.undo(ApplyOptions::default()).unwrap();
    assert!(e.evaluate(a).unwrap().anchor("end").unwrap().distance(Point::new(100.0, 0.0)) < 1e-9);
    // Cancel leaves everything as it was.
    let rev = e.revision();
    e.begin_drag(DragSpec::Move { ids: vec![a], from: Point::new(50.0, 0.0) }).unwrap();
    let _ = e.drag_to(Point::new(60.0, 10.0)).unwrap();
    assert!(e.end_drag(false).unwrap().is_none());
    assert_eq!(e.revision(), rev);
    // Preview items are re-emitted from the committed state.
    let delta = e.take_scene_delta();
    assert!(delta.upserts.iter().any(|i| i.id == a.0 && i.flags & dotloom_engine::scene::flags::PREVIEW == 0));
}

#[test]
fn rejected_drag_positions_keep_the_last_valid_preview() {
    let mut e = Engine::default();
    let a = line(&mut e, (0.0, 0.0), (100.0, 0.0));
    add_constraint(&mut e, RuleSpec::FixPoint { a: AnchorRef::new(a, "start"), at: Point::ORIGIN }).unwrap();
    add_constraint(&mut e, RuleSpec::FixPoint { a: AnchorRef::new(a, "end"), at: Point::new(100.0, 0.0) }).unwrap();
    e.begin_drag(DragSpec::Move { ids: vec![a], from: Point::new(50.0, 0.0) }).unwrap();
    // Fully fixed line: moving it is impossible (the move is a strong preference,
    // the fixes are hard): the preview stays where the hard rules allow.
    let (pre, _) = e.drag_to(Point::new(70.0, 30.0)).unwrap();
    assert!(pre.accepted);
    assert!(e.end_drag(true).unwrap().is_some());
    assert!(e.evaluate(a).unwrap().anchor("start").unwrap().distance(Point::ORIGIN) < 1e-6);
}

// ---------------------------------------------------------------------------
// Floor plan (DL-EXAMPLE-2)

fn floorplan() -> (Engine, EntityId, EntityId) {
    let mut e = Engine::default();
    for d in floorplan_defs() {
        e.register_type(d, "floorplan").unwrap();
    }
    let wall = create(
        &mut e,
        NewEntity {
            type_id: Some(TypeId::new("floorplan.wall").unwrap()),
            props: [
                ("start".to_owned(), PropValue::Point(Point::ORIGIN)),
                ("end".to_owned(), PropValue::Point(Point::new(4000.0, 0.0))),
            ]
            .into_iter()
            .collect(),
            ..NewEntity::default()
        },
    );
    let door = create(
        &mut e,
        NewEntity {
            type_id: Some(TypeId::new("floorplan.door").unwrap()),
            props: [
                ("host".to_owned(), PropValue::Ref(RefValue { entity: wall })),
                ("offset".to_owned(), PropValue::Number(2500.0)),
            ]
            .into_iter()
            .collect(),
            ..NewEntity::default()
        },
    );
    (e, wall, door)
}

#[test]
fn door_follows_wall_and_slides_when_the_wall_shrinks() {
    let (mut e, wall, door) = floorplan();
    let hinge = e.evaluate(door).unwrap().anchor("hinge").unwrap();
    assert!(hinge.distance(Point::new(2500.0, 0.0)) < 1e-9);
    // Rotate the wall end: the door stays on the wall.
    set(&mut e, wall, "end.y", 3000.0, EditMode::Exact).unwrap();
    let ev = e.evaluate(door).unwrap();
    let h = ev.anchor("hinge").unwrap();
    let dir = Point::new(4000.0, 3000.0).to_vector() / 5000.0;
    assert!(h.distance(Point::ORIGIN + dir * p(&e, door, "offset")) < 1e-6);
    // Shorten the wall below offset + width: the door slides (offset decreases).
    set(&mut e, wall, "end.y", 0.0, EditMode::Exact).unwrap();
    set(&mut e, wall, "end.x", 3000.0, EditMode::Exact).unwrap();
    let off = p(&e, door, "offset");
    assert!(off + p(&e, door, "width") <= 3000.0 + 1e-6, "door must fit: offset {off}");
    // The pinned attempt keeps the wall start and the door width: only the offset moved.
    assert!(e.evaluate(wall).unwrap().anchor("start").unwrap().distance(Point::ORIGIN) < 1e-9);
    assert!(close(p(&e, door, "width"), 900.0));
    // With the wall start anchored, a wall too short for the door (≥ 60 cm) conflicts.
    add_constraint(&mut e, RuleSpec::FixPoint { a: AnchorRef::new(wall, "start"), at: Point::ORIGIN }).unwrap();
    let rev = e.revision();
    let err = set(&mut e, wall, "end.x", 500.0, EditMode::Exact).unwrap_err();
    assert!(matches!(err, EngineError::Solve { .. }), "{err:?}");
    assert_eq!(e.revision(), rev);
}

#[test]
fn deleting_a_wall_deletes_its_door_and_undo_restores_both() {
    let (mut e, wall, door) = floorplan();
    let r = apply(&mut e, vec![Command::Delete { ids: vec![wall], policy: DeletePolicy::Cascade }]).unwrap();
    assert!(r.deleted.contains(&door));
    e.document().validate().unwrap();
    e.undo(ApplyOptions::default()).unwrap();
    assert!(e.document().entity(door).is_some());
    e.document().validate().unwrap();
}

#[test]
fn save_load_roundtrip_keeps_references_and_behaviour() {
    let (mut e, wall, door) = floorplan();
    let json = e.document().to_json_string().unwrap();
    let mut e2 = Engine::default();
    for d in floorplan_defs() {
        e2.register_type(d, "floorplan").unwrap();
    }
    let (doc, _) = Document::from_json_str(&json, &Default::default()).unwrap();
    e2.load(doc).unwrap();
    assert!(e2.document().semantic_eq(e.document()));
    assert_eq!(e2.evaluate(door).unwrap().anchor("hinge"), e.evaluate(door).unwrap().anchor("hinge"));
    set(&mut e2, wall, "end.x", 3000.0, EditMode::Exact).unwrap();
    assert!(p(&e2, door, "offset") + p(&e2, door, "width") <= 3000.0 + 1e-6);
}

#[test]
fn old_wall_versions_are_migrated_on_load() {
    let mut e = Engine::default();
    for d in floorplan_defs() {
        e.register_type(d, "floorplan").unwrap();
    }
    let j = serde_json::json!({
        "schema": 1, "layers": [{"id": 1, "name": "L"}], "nextId": 3,
        "entities": [{"id": 2, "type": "floorplan.wall", "layer": 1, "props": {"start": [0, 0], "end": [1000, 0], "thick": 150.0}}]
    });
    let (doc, _) = Document::from_json_value(j, &Default::default()).unwrap();
    e.load(doc).unwrap();
    let w = e.document().entity(EntityId(2)).unwrap();
    assert_eq!(w.type_version, 2);
    assert_eq!(w.props.get("thickness"), Some(&PropValue::Number(150.0)));
    assert!(!w.props.contains_key("thick"));
}

// ---------------------------------------------------------------------------
// Missing / disabled plugins (DL-PLUGIN-5)

#[test]
fn missing_plugin_entities_are_read_only_and_preserved() {
    let (mut e, wall, door) = floorplan();
    let ev = e.evaluate(wall).unwrap();
    // Store a fallback representation (what a saving host would do).
    let fallback: Vec<Shape> = ev.shapes().collect();
    apply(
        &mut e,
        vec![Command::UpdateEntity { id: wall, patch: EntityPatch { hidden: Some(false), ..EntityPatch::default() } }],
    )
    .unwrap();
    let mut doc = e.document().clone();
    doc.entity_mut(wall).unwrap().fallback = Some(fallback);
    let mut bare = Engine::default();
    bare.load(doc.clone()).unwrap();
    // Read-only: content edits are refused, payload kept on save.
    let err = apply(
        &mut bare,
        vec![Command::SetParams {
            values: vec![ParamValue { entity: wall, param: "thickness".into(), value: 300.0 }],
            mode: EditMode::Exact,
        }],
    );
    assert!(matches!(err, Err(EngineError::Command { .. })), "{err:?}");
    assert!(bare.document().semantic_eq(&doc));
    let ev = bare.evaluate(wall).unwrap();
    assert!(ev.read_only.is_some());
    assert!(!ev.drawables.is_empty(), "fallback representation is drawn");
    let scene = bare.full_scene();
    let item = scene.upserts.iter().find(|i| i.id == wall.0).unwrap();
    assert!(item.flags & dotloom_engine::scene::flags::READONLY != 0);
    // Deleting is still allowed.
    apply(&mut bare, vec![Command::Delete { ids: vec![door, wall], policy: DeletePolicy::Cascade }]).unwrap();
}

#[test]
fn plugin_registration_errors_are_typed() {
    let mut e = Engine::default();
    let mut bad = shelf_def();
    bad.constraints[0].rhs = "90deg".into();
    assert!(matches!(e.register_type(bad, "x"), Err(EngineError::Plugin { .. })));
    let mut reserved = shelf_def();
    reserved.type_id = TypeId::new("dotloom.shelf").unwrap();
    assert!(e.register_type(reserved, "x").is_err());
    e.register_type(shelf_def(), "x").unwrap();
    assert!(e.register_type(shelf_def(), "x").is_err(), "duplicate type id");
    let mut unknown_fn = timeline_def();
    unknown_fn.type_id = TypeId::new("timeline.other").unwrap();
    unknown_fn.derived[0].expr = "eval(start)".into();
    assert!(e.register_type(unknown_fn, "x").is_err());
}

// ---------------------------------------------------------------------------
// Timeline (DL-EXAMPLE-3)

#[test]
fn timeline_order_gap_and_locked_times() {
    let mut e = Engine::default();
    e.register_type(timeline_def(), "timeline").unwrap();
    apply(
        &mut e,
        vec![Command::SetSettings {
            patch: dotloom_engine::SettingsPatch {
                time_axis: Some(TimeAxis::new(0.0, 0.1).unwrap()),
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let block = |e: &mut Engine, start_h: f64, dur_h: f64| {
        create(
            e,
            NewEntity {
                type_id: Some(TypeId::new("timeline.block").unwrap()),
                props: [
                    ("start".to_owned(), PropValue::Number(start_h * 3600.0)),
                    ("duration".to_owned(), PropValue::Number(dur_h * 3600.0)),
                ]
                .into_iter()
                .collect(),
                ..NewEntity::default()
            },
        )
    };
    let a = block(&mut e, 0.0, 2.0);
    let b = block(&mut e, 3.0, 1.0);
    // b starts at least 30 minutes after a ends: b.start − a.start − a.duration ≥ 1800 s.
    add_constraint(
        &mut e,
        RuleSpec::Linear {
            terms: vec![
                Term { coef: 1.0, param: ParamRef::prop(b, "start") },
                Term { coef: -1.0, param: ParamRef::prop(a, "start") },
                Term { coef: -1.0, param: ParamRef::prop(a, "duration") },
            ],
            op: Cmp::Ge,
            rhs: 1800.0,
        },
    )
    .unwrap();
    // Lock a's start.
    add_constraint(&mut e, RuleSpec::Fix { param: ParamRef::prop(a, "start"), value: 0.0 }).unwrap();
    // Making a longer pushes b later (gap kept).
    set(&mut e, a, "duration", 3.0 * 3600.0, EditMode::Exact).unwrap();
    assert!(p(&e, b, "start") >= 3.5 * 3600.0 - 1e-6, "{}", p(&e, b, "start"));
    assert!(close(p(&e, a, "start"), 0.0));
    // Moving b earlier shortens a (a's duration is free): the rules still hold.
    set(&mut e, b, "start", 3.0 * 3600.0, EditMode::Exact).unwrap();
    assert!(close(p(&e, a, "duration"), 2.5 * 3600.0), "{}", p(&e, a, "duration"));
    // Lock a's duration too: now b cannot move before a's end + gap.
    add_constraint(&mut e, RuleSpec::Fix { param: ParamRef::prop(a, "duration"), value: 2.5 * 3600.0 }).unwrap();
    // Moving b before a's end + gap is rejected; with prefer it clamps.
    assert!(set(&mut e, b, "start", 3600.0, EditMode::Exact).is_err());
    set(&mut e, b, "start", 3600.0, EditMode::Prefer).unwrap();
    assert!(close(p(&e, b, "start"), 3.0 * 3600.0), "{}", p(&e, b, "start"));
    // Time → view mapping is explicit: block a ends at x = 2.5 h × 0.1 mm/s.
    let fin = e.evaluate(a).unwrap().anchor("finish").unwrap();
    assert!(close(fin.x, 2.5 * 3600.0 * 0.1));
    // Durations have a minimum of 5 minutes (type template).
    assert!(set(&mut e, b, "duration", 60.0, EditMode::Exact).is_err());
}

// ---------------------------------------------------------------------------
// Queries

#[test]
fn hit_test_and_snapping() {
    let mut e = Engine::default();
    let a = line(&mut e, (0.0, 0.0), (100.0, 0.0));
    let b = line(&mut e, (50.0, -50.0), (50.0, 50.0));
    let hits = e.hit_test(Point::new(30.0, 1.0), 2.0);
    assert_eq!(hits.first().map(|h| h.entity), Some(a));
    assert!(e.hit_test(Point::new(30.0, 10.0), 2.0).is_empty());
    use dotloom_engine::{SnapKind, SnapQuery};
    let q =
        |pt: Point| SnapQuery { point: pt, radius: 5.0, options: Default::default(), exclude: vec![], previous: None };
    let s = e.snap(&q(Point::new(98.0, 2.0))).unwrap();
    assert_eq!(s.kind, SnapKind::Endpoint);
    assert_eq!(s.point, Point::new(100.0, 0.0));
    let s = e.snap(&q(Point::new(51.0, 1.0))).unwrap();
    assert_eq!(s.kind, SnapKind::Intersection);
    assert!(s.point.distance(Point::new(50.0, 0.0)) < 1e-9);
    // Hysteresis: a weaker candidate near the previous snap keeps the previous one.
    let prev = e.snap(&q(Point::new(99.0, 0.5))).unwrap();
    let mut q2 = q(Point::new(96.0, 0.5));
    q2.previous = Some(prev.clone());
    assert_eq!(e.snap(&q2), Some(prev));
    // Grid snaps: halfway between two grid points the previous one is kept, but at
    // (or clearly nearer to) the next grid point the snap moves on.
    let grid = |pt: Point, prev: Option<dotloom_engine::Snap>| SnapQuery {
        point: pt,
        radius: 10.0,
        options: dotloom_engine::SnapOptions {
            endpoint: false,
            midpoint: false,
            center: false,
            quadrant: false,
            intersection: false,
            anchor: false,
            nearest: false,
            grid: true,
            grid_spacing: Some(10.0),
        },
        exclude: vec![],
        previous: prev,
    };
    let g1 = e.snap(&grid(Point::new(410.0, 410.0), None)).unwrap();
    assert_eq!(g1.point, Point::new(410.0, 410.0));
    let g2 = e.snap(&grid(Point::new(415.0, 415.0), Some(g1.clone()))).unwrap();
    assert_eq!(g2.point, g1.point, "kept while ambiguous");
    let g3 = e.snap(&grid(Point::new(420.0, 420.0), Some(g2))).unwrap();
    assert_eq!(g3.point, Point::new(420.0, 420.0), "moves on when clearly closer");
    // Excluding an entity removes its snaps.
    let mut q3 = q(Point::new(51.0, 1.0));
    q3.exclude = vec![b];
    assert_ne!(e.snap(&q3).map(|s| s.kind), Some(SnapKind::Intersection));
    let sel = e.select_in_rect(
        dotloom_engine::geometry::Aabb::from_corners(Point::new(-1.0, -1.0), Point::new(101.0, 1.0)),
        dotloom_engine::SelectMode::Window,
    );
    assert_eq!(sel, vec![a]);
}

#[test]
fn scene_deltas_are_incremental() {
    let mut e = Engine::default();
    let a = line(&mut e, (0.0, 0.0), (100.0, 0.0));
    let full = e.full_scene();
    assert!(full.reset && full.upserts.iter().any(|i| i.id == a.0));
    assert!(e.take_scene_delta().is_empty());
    let b = line(&mut e, (0.0, 10.0), (100.0, 10.0));
    let d = e.take_scene_delta();
    assert_eq!(d.upserts.iter().map(|i| i.id).collect::<Vec<_>>(), vec![b.0]);
    apply(&mut e, vec![Command::Delete { ids: vec![a], policy: DeletePolicy::Cascade }]).unwrap();
    let d = e.take_scene_delta();
    assert_eq!(d.removals, vec![a.0]);
    // Encoded deltas decode to the same content.
    let bytes = d.encode();
    assert_eq!(dotloom_engine::scene::SceneDelta::decode(&bytes).unwrap(), d);
}

#[test]
fn constraint_on_unknown_anchor_is_rejected() {
    let mut e = Engine::default();
    let a = line(&mut e, (0.0, 0.0), (100.0, 0.0));
    let r = add_constraint(&mut e, RuleSpec::FixPoint { a: AnchorRef::new(a, "center"), at: Point::ORIGIN });
    assert!(matches!(r, Err(EngineError::Command { .. })));
    let _ = Constraint::new(dotloom_engine::document::ConstraintId(1), RuleSpec::Radius { circle: a, value: 1.0 });
}
