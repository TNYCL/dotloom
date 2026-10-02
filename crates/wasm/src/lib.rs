//! WebAssembly binding of the Dotloom engine (runs inside a Web Worker).
//!
//! The surface is intentionally small and data-oriented: JSON strings for commands,
//! queries and results; `Uint8Array` for `.dotl` files and binary scene deltas.
//! The TypeScript SDK wraps it in a typed, versioned protocol — application code
//! never calls this module directly.

#![allow(unsafe_code)] // wasm-bindgen generated glue only; this file has no unsafe blocks.

use std::collections::BTreeMap;

use dotloom_engine::{
    ApplyOptions, DragSpec, Engine, EngineError, EngineOptions, EntityTypeDef, PendingState, SelectMode, SnapQuery,
    Transaction,
    document::{Document, EntityId, Limits, TypeId},
    geometry::{Aabb, Point},
};
use dotloom_io::{
    DotlFile, DotlLimits,
    dotl::{read_dotl, write_dotl},
    dxf, import_into, svg,
};
use serde::Serialize;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

/// Protocol version of the worker messages built on top of this binding.
pub const PROTOCOL_VERSION: u32 = 1;

fn err(e: &EngineError) -> JsValue {
    let v = json!({ "code": e.code(), "message": e.to_string(), "details": e });
    JsValue::from_str(&v.to_string())
}

fn err_msg(code: &str, message: impl ToString) -> JsValue {
    JsValue::from_str(&json!({ "code": code, "message": message.to_string() }).to_string())
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| json!({ "code": "serialize", "message": e.to_string() }).to_string())
}

fn opts(expected_revision: Option<f64>) -> ApplyOptions {
    ApplyOptions { expected_revision: expected_revision.filter(|r| r.is_finite() && *r >= 0.0).map(|r| r as u64) }
}

/// Engine instance.
#[wasm_bindgen]
#[derive(Debug)]
pub struct WasmEngine {
    engine: Engine,
    file: Option<DotlFile>,
    preview: Vec<u8>,
}

#[wasm_bindgen]
impl WasmEngine {
    /// Create an engine with an empty document.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self { engine: Engine::new(EngineOptions::default()), file: None, preview: Vec::new() }
    }

    /// Versions and capabilities.
    #[must_use]
    pub fn capabilities(&self) -> String {
        json!({
            "engineVersion": env!("CARGO_PKG_VERSION"),
            "protocol": PROTOCOL_VERSION,
            "schema": dotloom_engine::document::SCHEMA_VERSION,
            "formatVersion": dotloom_io::dotl::FORMAT_VERSION,
            "sceneFormat": dotloom_engine::scene::SCENE_FORMAT_VERSION,
            "import": ["dotl", "svg", "dxf"],
            "export": ["dotl", "svg", "dxf"],
        })
        .to_string()
    }

    /// Current revision.
    #[must_use]
    pub fn revision(&self) -> f64 {
        self.engine.revision() as f64
    }

    /// Register plugin type definitions (JSON object or array).
    pub fn register_types(&mut self, defs_json: &str, plugin: &str) -> Result<String, JsValue> {
        let v: Value = serde_json::from_str(defs_json).map_err(|e| err_msg("plugin", e))?;
        let defs: Vec<EntityTypeDef> = match v {
            Value::Array(_) => serde_json::from_value(v),
            other => serde_json::from_value(other).map(|d| vec![d]),
        }
        .map_err(|e| err_msg("plugin", e))?;
        let mut ids = Vec::new();
        for d in defs {
            ids.push(d.type_id.to_string());
            self.engine.register_type(d, plugin).map_err(|e| err(&e))?;
        }
        Ok(to_json(&ids))
    }

    /// Unregister a plugin type.
    pub fn unregister_type(&mut self, type_id: &str) -> Result<(), JsValue> {
        let t = TypeId::new(type_id).map_err(|e| err_msg("plugin", e))?;
        self.engine.unregister_type(&t).map_err(|e| err(&e))
    }

    /// Enable or disable a plugin type.
    pub fn set_type_enabled(&mut self, type_id: &str, enabled: bool) -> Result<(), JsValue> {
        let t = TypeId::new(type_id).map_err(|e| err_msg("plugin", e))?;
        self.engine.set_type_enabled(&t, enabled).map_err(|e| err(&e))
    }

    /// Start a new empty document.
    pub fn new_document(&mut self) -> Result<f64, JsValue> {
        self.file = None;
        self.engine.load(Document::new()).map(|r| r as f64).map_err(|e| err(&e))
    }

    /// Load a `.dotl` file. Returns the load report JSON.
    pub fn load_dotl(&mut self, bytes: &[u8]) -> Result<String, JsValue> {
        let (file, report) = read_dotl(bytes, &DotlLimits::default()).map_err(|e| err_msg("file", e))?;
        let revision = self.engine.load(file.document.clone()).map_err(|e| err(&e))?;
        let missing: Vec<String> = report
            .plugins
            .iter()
            .filter(|p| self.engine.registry().enabled(&p.type_id).is_none())
            .map(|p| p.type_id.to_string())
            .collect();
        let out = json!({
            "revision": revision,
            "migrations": report.migrations.iter().map(|m| json!({"from": m.from, "message": m.message})).collect::<Vec<_>>(),
            "plugins": report.plugins,
            "missingPlugins": missing,
            "warnings": report.warnings,
            "view": file.view,
        });
        self.file = Some(file);
        Ok(out.to_string())
    }

    /// Load a document from its JSON form.
    pub fn load_json(&mut self, doc_json: &str) -> Result<f64, JsValue> {
        let (doc, _) = Document::from_json_str(doc_json, &Limits::default()).map_err(|e| err_msg("file", e))?;
        self.file = None;
        self.engine.load(doc).map(|r| r as f64).map_err(|e| err(&e))
    }

    /// Save as `.dotl`, with optional view state JSON.
    pub fn save_dotl(&self, view_json: Option<String>) -> Result<Vec<u8>, JsValue> {
        let mut f = self.file.clone().unwrap_or_else(|| DotlFile::new(Document::new()));
        f.document = self.engine.document().clone();
        // Fallback representations let viewers without a plugin display its entities.
        let ids: Vec<EntityId> = f.document.order().to_vec();
        for id in ids {
            let is_plugin = f.document.entity(id).is_some_and(|e| e.type_id.namespace() != "dotloom");
            if !is_plugin {
                continue;
            }
            if let Some(ev) = self.engine_eval(id)
                && ev.read_only.is_none()
                && let Some(e) = f.document.entity_mut(id)
            {
                e.fallback = Some(ev.shapes().collect());
            }
        }
        f.view = match view_json {
            Some(v) => Some(serde_json::from_str(&v).map_err(|e| err_msg("file", e))?),
            None => f.view,
        };
        write_dotl(&f).map_err(|e| err_msg("file", e))
    }

    fn engine_eval(&self, id: EntityId) -> Option<dotloom_engine::eval::Evaluated> {
        let ctx = dotloom_engine::eval::Ctx { view: self.engine.document(), registry: self.engine.registry() };
        self.engine.document().entity(id)?;
        Some(dotloom_engine::eval::evaluate(ctx, id))
    }

    /// Document JSON.
    pub fn document_json(&self) -> Result<String, JsValue> {
        self.engine.document().to_json_string().map_err(|e| err_msg("file", e))
    }

    /// Reserve fresh IDs (JSON array of numbers).
    pub fn reserve_ids(&mut self, n: u32) -> String {
        to_json(&self.engine.reserve_ids(n as usize))
    }

    /// Start a transaction (solve runs in [`WasmEngine::step`]).
    pub fn begin_apply(&mut self, tx_json: &str, expected_revision: Option<f64>) -> Result<f64, JsValue> {
        let tx: Transaction =
            serde_json::from_str(tx_json).map_err(|e| err_msg("protocol", format!("invalid transaction: {e}")))?;
        self.engine.begin_apply(tx, opts(expected_revision)).map(|id| id as f64).map_err(|e| err(&e))
    }

    /// Run the pending transaction for at most `budget` solver iterations.
    /// Returns `{"state":"running"}` or `{"state":"done","ok":…}`.
    pub fn step(&mut self, budget: u32) -> String {
        match self.engine.step_pending(budget) {
            Ok(PendingState::Running { id }) => json!({ "state": "running", "id": id }).to_string(),
            Ok(PendingState::Done(Ok(report))) => json!({ "state": "done", "ok": true, "report": report }).to_string(),
            Ok(PendingState::Done(Err(e))) => json!({ "state": "done", "ok": false, "error": { "code": e.code(), "message": e.to_string(), "details": e } }).to_string(),
            Err(e) => json!({ "state": "done", "ok": false, "error": { "code": e.code(), "message": e.to_string(), "details": e } }).to_string(),
        }
    }

    /// Cancel the pending transaction.
    pub fn cancel_pending(&mut self) -> bool {
        self.engine.cancel_pending()
    }

    /// Whether a transaction is pending.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        self.engine.has_pending()
    }

    /// Undo.
    pub fn undo(&mut self, expected_revision: Option<f64>) -> Result<f64, JsValue> {
        self.engine.undo(opts(expected_revision)).map(|r| r as f64).map_err(|e| err(&e))
    }

    /// Redo.
    pub fn redo(&mut self, expected_revision: Option<f64>) -> Result<f64, JsValue> {
        self.engine.redo(opts(expected_revision)).map(|r| r as f64).map_err(|e| err(&e))
    }

    /// Binary scene delta since the last call.
    pub fn take_scene_delta(&mut self) -> Vec<u8> {
        self.engine.take_scene_delta().encode()
    }

    /// Binary full scene.
    pub fn full_scene(&mut self) -> Vec<u8> {
        self.engine.full_scene().encode()
    }

    /// Pending events (JSON array).
    pub fn take_events(&mut self) -> String {
        to_json(&self.engine.take_events())
    }

    /// Replace the selection (JSON array of IDs).
    pub fn set_selection(&mut self, ids_json: &str) -> Result<(), JsValue> {
        let ids: Vec<u64> = serde_json::from_str(ids_json).map_err(|e| err_msg("protocol", e))?;
        self.engine.set_selection(&ids.into_iter().map(EntityId).collect::<Vec<_>>());
        Ok(())
    }

    /// Selection (JSON array).
    #[must_use]
    pub fn selection(&self) -> String {
        to_json(&self.engine.selection())
    }

    /// Hit test.
    pub fn hit_test(&mut self, x: f64, y: f64, radius: f64) -> String {
        to_json(&self.engine.hit_test(Point::new(x, y), radius))
    }

    /// Area selection; `crossing` selects touching entities.
    pub fn select_in_rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, crossing: bool) -> String {
        let mode = if crossing { SelectMode::Crossing } else { SelectMode::Window };
        to_json(&self.engine.select_in_rect(Aabb::from_corners(Point::new(x0, y0), Point::new(x1, y1)), mode))
    }

    /// Snap query.
    pub fn snap(&mut self, query_json: &str) -> Result<String, JsValue> {
        let q: SnapQuery = serde_json::from_str(query_json).map_err(|e| err_msg("protocol", e))?;
        Ok(to_json(&self.engine.snap(&q)))
    }

    /// Begin a drag.
    pub fn begin_drag(&mut self, spec_json: &str) -> Result<(), JsValue> {
        let spec: DragSpec = serde_json::from_str(spec_json).map_err(|e| err_msg("protocol", e))?;
        self.engine.begin_drag(spec).map_err(|e| err(&e))
    }

    /// Move the drag; returns the preview status JSON. The preview scene delta is
    /// fetched with [`WasmEngine::take_preview`].
    pub fn drag_to(&mut self, x: f64, y: f64) -> Result<String, JsValue> {
        let (preview, delta) = self.engine.drag_to(Point::new(x, y)).map_err(|e| err(&e))?;
        self.preview = delta.encode();
        Ok(to_json(&preview))
    }

    /// Binary preview delta of the last `drag_to`.
    pub fn take_preview(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.preview)
    }

    /// End the drag (commit or cancel). Returns the commit report JSON or `null`.
    pub fn end_drag(&mut self, commit: bool) -> Result<String, JsValue> {
        self.engine.end_drag(commit).map(|r| to_json(&r)).map_err(|e| err(&e))
    }

    /// Entity details: evaluated anchors, parameters, constraints, read-only state.
    pub fn entity_info(&mut self, id: f64) -> Result<String, JsValue> {
        let eid = EntityId(id as u64);
        let entity = self
            .engine
            .document()
            .entity(eid)
            .cloned()
            .ok_or_else(|| err_msg("notFound", format!("{eid} does not exist")))?;
        let ev = self.engine.evaluate(eid).ok_or_else(|| err_msg("notFound", format!("{eid} does not exist")))?;
        let params: BTreeMap<String, f64> = self.engine.params_of(eid);
        Ok(json!({
            "entity": entity,
            "anchors": ev.anchors,
            "bbox": ev.bbox,
            "params": params,
            "readOnly": ev.read_only,
            "error": ev.error,
            "measured": ev.measured,
            "constraints": self.engine.constraints_of(eid),
        })
        .to_string())
    }

    /// Analyse all constraints (status + diagnostics JSON).
    #[must_use]
    pub fn analyze(&self) -> String {
        let (status, diags) = self.engine.analyze();
        json!({ "status": status, "diagnostics": diags }).to_string()
    }

    /// Hard rules violated by the stored values (JSON array).
    #[must_use]
    pub fn verify(&self) -> String {
        let v: Vec<Value> = self
            .engine
            .verify()
            .into_iter()
            .map(|(w, r, t)| json!({ "rule": w, "residual": r, "tolerance": t }))
            .collect();
        to_json(&v)
    }

    /// Export SVG: `{ "svg": string, "report": … }`.
    pub fn export_svg(
        &mut self,
        foreground: Option<String>,
        background: Option<String>,
        margin: Option<f64>,
    ) -> Result<String, JsValue> {
        let mut o = svg::SvgExportOptions::default();
        if let Some(c) = foreground {
            o.foreground = dotloom_engine::document::Color::parse(&c).map_err(|e| err_msg("protocol", e))?;
        }
        if let Some(c) = background {
            o.background = Some(dotloom_engine::document::Color::parse(&c).map_err(|e| err_msg("protocol", e))?);
        }
        if let Some(m) = margin {
            o.margin = m;
        }
        let (s, report) = svg::export_svg(&mut self.engine, &o);
        Ok(json!({ "svg": s, "report": report }).to_string())
    }

    /// Export DXF: `{ "dxf": string, "report": … }`.
    pub fn export_dxf(&mut self) -> String {
        let (s, report) = dxf::export_dxf(&mut self.engine);
        json!({ "dxf": s, "report": report }).to_string()
    }

    /// Import SVG into the document as one undoable transaction.
    pub fn import_svg(&mut self, text: &str) -> Result<String, JsValue> {
        let imp = svg::import_svg(text).map_err(|e| err_msg("import", e))?;
        let layers: Vec<_> = imp.layers.iter().map(|n| (n.clone(), None)).collect();
        let commit = import_into(&mut self.engine, "Import SVG", &layers, imp.entities).map_err(|e| err(&e))?;
        Ok(json!({ "report": imp.report, "commit": commit }).to_string())
    }

    /// Import DXF into the document as one undoable transaction.
    pub fn import_dxf(&mut self, bytes: &[u8]) -> Result<String, JsValue> {
        let imp = dxf::import_dxf(bytes).map_err(|e| err_msg("import", e))?;
        let commit = import_into(&mut self.engine, "Import DXF", &imp.layers, imp.entities).map_err(|e| err(&e))?;
        Ok(json!({ "report": imp.report, "commit": commit, "version": imp.version, "unit": imp.unit }).to_string())
    }

    /// Copy entities to clipboard JSON.
    pub fn copy(&self, ids_json: &str) -> Result<String, JsValue> {
        let ids: Vec<u64> = serde_json::from_str(ids_json).map_err(|e| err_msg("protocol", e))?;
        Ok(to_json(&self.engine.document().copy(&ids.into_iter().map(EntityId).collect::<Vec<_>>())))
    }

    /// Registered plugin types (JSON).
    #[must_use]
    pub fn plugin_types(&self) -> String {
        let v: Vec<Value> = self
            .engine
            .registry()
            .types()
            .map(
                |(t, e)| json!({ "typeId": t, "enabled": e.enabled, "plugin": e.plugin, "definition": e.compiled.def }),
            )
            .collect();
        to_json(&v)
    }

    /// Undo/redo availability (JSON).
    #[must_use]
    pub fn history_state(&self) -> String {
        json!({ "canUndo": self.engine.can_undo(), "canRedo": self.engine.can_redo(), "bytes": self.engine.history_bytes() }).to_string()
    }

    /// Test hook: trap the WebAssembly instance (simulates an engine crash so hosts
    /// can verify crash reporting and recovery). Never called by the SDK itself.
    #[allow(clippy::panic)]
    pub fn debug_trap(&self) {
        panic!("debug_trap: simulated engine crash");
    }
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::new()
    }
}
