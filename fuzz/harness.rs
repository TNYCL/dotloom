// Fuzz harness (DL-TEST-10), shared by the cargo-fuzz targets (`fuzz/fuzz_targets/`)
// and the stable replay test (`crates/cli/tests/fuzz_replay.rs`) through `include!`,
// so the committed seeds and every input that once crashed keep being checked by
// the normal test suite.
//
// Contract: any input is handled without panicking, without hanging and within the
// configured limits. Untrusted inputs: `.dotl` containers, document JSON, SVG and
// DXF files, plugin type definitions, transactions sent through the SDK and scene
// buffers.

#[allow(unused_imports)]
use dotloom_engine::{
    ApplyOptions, Command, Engine, EntityTypeDef, NewEntity, Transaction,
    document::{Document, Limits},
    scene::SceneDelta,
};
#[allow(unused_imports)]
use dotloom_io::{
    dotl::{DotlLimits, read_dotl},
    dxf::import_dxf,
    import_into,
    svg::import_svg,
};

/// Plugin types used by the transaction target.
const FLOORPLAN: &str = include_str!("../tests/fixtures/plugins/floorplan.json");
const SHELF: &str = include_str!("../tests/fixtures/plugins/shelf.json");

/// An engine with plugin types, entities and constraints to run transactions on.
#[allow(dead_code)]
fn engine_with_content() -> Engine {
    let mut e = Engine::default();
    for text in [FLOORPLAN, SHELF] {
        let defs: Vec<EntityTypeDef> = match serde_json::from_str::<serde_json::Value>(text) {
            Ok(v @ serde_json::Value::Array(_)) => serde_json::from_value(v).unwrap_or_default(),
            Ok(v) => serde_json::from_value(v).map(|d| vec![d]).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        for d in defs {
            let _ = e.register_type(d, "fuzz");
        }
    }
    let setup = r#"[
        {"op":"createEntity","id":1000,"entity":{"geometry":{"type":"line","a":[0,0],"b":[100,0]}}},
        {"op":"createEntity","id":1001,"entity":{"geometry":{"type":"line","a":[100,0],"b":[100,80]}}},
        {"op":"createEntity","id":1002,"entity":{"geometry":{"type":"circle","center":[50,40],"radius":20}}},
        {"op":"createEntity","id":1003,"entity":{"type":"floorplan.wall","props":{"start":[0,200],"end":[3000,200]}}},
        {"op":"createEntity","id":1004,"entity":{"type":"floorplan.door","props":{"host":{"ref":1003}}}},
        {"op":"createEntity","id":1005,"entity":{"type":"shelf.unit"}},
        {"op":"addConstraint","constraint":{"rule":{"kind":"coincident","a":{"entity":1000,"anchor":"end"},"b":{"entity":1001,"anchor":"start"}}}},
        {"op":"addConstraint","constraint":{"rule":{"kind":"length","line":{"from":{"entity":1000,"anchor":"start"},"to":{"entity":1000,"anchor":"end"}},"value":100}}},
        {"op":"addConstraint","constraint":{"rule":{"kind":"radius","circle":1002,"value":20}}}
    ]"#;
    if let Ok(commands) = serde_json::from_str::<Vec<Command>>(setup) {
        let _ = e.apply(Transaction::new("setup", commands), ApplyOptions::default());
    }
    e
}

/// `.dotl` bytes: open, load into an engine, encode the scene.
#[allow(dead_code)]
pub fn dotl(data: &[u8]) {
    if let Ok((file, _)) = read_dotl(data, &DotlLimits::default()) {
        let mut e = Engine::default();
        if e.load(file.document).is_ok() {
            let _ = e.full_scene().encode();
        }
    }
}

/// Document JSON: parse, migrate, validate, load.
#[allow(dead_code)]
pub fn document_json(data: &[u8]) {
    if let Ok((doc, _)) = Document::from_json_slice(data, &Limits::default()) {
        let mut e = Engine::default();
        if e.load(doc).is_ok() {
            let _ = e.full_scene().encode();
        }
    }
}

/// SVG text: import and commit into an engine.
#[allow(dead_code)]
pub fn svg(data: &[u8]) {
    let Ok(text) = core::str::from_utf8(data) else { return };
    if let Ok(imp) = import_svg(text) {
        let mut e = Engine::default();
        let layers: Vec<_> = imp.layers.into_iter().map(|n| (n, None)).collect();
        let _ = import_into(&mut e, "svg", &layers, imp.entities);
    }
}

/// DXF bytes: import and commit into an engine.
#[allow(dead_code)]
pub fn dxf(data: &[u8]) {
    if let Ok(imp) = import_dxf(data) {
        let mut e = Engine::default();
        let _ = import_into(&mut e, "dxf", &imp.layers, imp.entities);
    }
}

/// A JSON command list applied to an engine with content, then undone.
#[allow(dead_code)]
pub fn transaction(data: &[u8]) {
    let Ok(commands) = serde_json::from_slice::<Vec<Command>>(data) else { return };
    let mut e = engine_with_content();
    if e.apply(Transaction::new("fuzz", commands), ApplyOptions::default()).is_ok() {
        let _ = e.full_scene().encode();
        let _ = e.undo(ApplyOptions::default());
        let _ = e.redo(ApplyOptions::default());
    }
}

/// A plugin type definition: register it, create an entity, draw it.
#[allow(dead_code)]
pub fn plugin(data: &[u8]) {
    let Ok(def) = serde_json::from_slice::<EntityTypeDef>(data) else { return };
    let mut e = Engine::default();
    let type_id = def.type_id.clone();
    if e.register_type(def, "fuzz").is_ok() {
        let create = Command::CreateEntity { id: None, entity: NewEntity { type_id: Some(type_id), ..NewEntity::default() } };
        let _ = e.apply(Transaction::new("fuzz", vec![create]), ApplyOptions::default());
        let _ = e.full_scene().encode();
    }
}

/// Scene buffers: decoding never panics; re-encoding is stable after one round.
#[allow(dead_code)]
pub fn scene(data: &[u8]) {
    if let Ok(d) = SceneDelta::decode(data) {
        let once = d.encode();
        let twice = SceneDelta::decode(&once).map(|d| d.encode());
        assert_eq!(twice.ok().as_deref(), Some(once.as_slice()), "scene encoding is not stable");
    }
}
