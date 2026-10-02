//! Native/WASM parity (DL-TEST-6), native half.
//!
//! Runs `tests/fixtures/parity/cases.json` through the same binding layer the
//! WebAssembly build exposes (`WasmEngine`, string/byte API) and compares FNV-1a
//! digests of every output — commit reports, final document, full scene, hit-test,
//! snap, SVG and DXF export — with `tests/fixtures/parity/expected.json`. The WASM
//! half (`packages/sdk/test/parity.test.ts`) compares the same file, so a pass on
//! both sides means bit-identical results natively (Windows, Linux, macOS in CI) and
//! in WebAssembly.
//!
//! `DOTLOOM_UPDATE_PARITY=1 cargo test -p dotloom-wasm --test parity` rewrites the
//! expected digests (review the diff: a change means results changed).

use std::collections::BTreeMap;
use std::path::PathBuf;

use dotloom_wasm::WasmEngine;
use serde_json::Value;

/// Solver iterations per `step` call (same as the WASM half).
const BUDGET: u32 = 50;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/parity").join(name)
}

fn fnv1a(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn apply(e: &mut WasmEngine, tx: &Value) -> String {
    let started = e.begin_apply(&tx.to_string(), None);
    assert!(started.is_ok(), "begin_apply failed for {tx}");
    loop {
        let r = e.step(BUDGET);
        if !r.contains("\"state\":\"running\"") {
            return r;
        }
    }
}

fn run(case: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut e = WasmEngine::new();
    // The binding installs a browser panic hook; tests keep the default one.
    let _ = std::panic::take_hook();
    if let Some(types) = case["types"].as_array().filter(|t| !t.is_empty()) {
        let r = e.register_types(&Value::Array(types.clone()).to_string(), "parity");
        assert!(r.is_ok(), "register_types");
    }
    for tx in case["setup"].as_array().into_iter().flatten() {
        out.insert(format!("{:03}-setup", out.len()), fnv1a(apply(&mut e, tx).as_bytes()));
    }
    for tx in case["edits"].as_array().into_iter().flatten() {
        out.insert(format!("{:03}-edit", out.len()), fnv1a(apply(&mut e, tx).as_bytes()));
    }
    let doc = e.document_json().unwrap_or_default();
    out.insert("document".into(), fnv1a(doc.as_bytes()));
    out.insert("scene".into(), fnv1a(&e.full_scene()));
    for (i, q) in case["hit"].as_array().into_iter().flatten().enumerate() {
        let f = |k: usize| q[k].as_f64().unwrap_or(0.0);
        out.insert(format!("hit-{i}"), fnv1a(e.hit_test(f(0), f(1), f(2)).as_bytes()));
    }
    for (i, q) in case["snap"].as_array().into_iter().flatten().enumerate() {
        out.insert(format!("snap-{i}"), fnv1a(e.snap(&q.to_string()).unwrap_or_default().as_bytes()));
    }
    out.insert("svg".into(), fnv1a(e.export_svg(None, None, None).unwrap_or_default().as_bytes()));
    out.insert("dxf".into(), fnv1a(e.export_dxf().as_bytes()));
    out
}

#[test]
fn native_results_match_the_shared_digests() {
    let cases: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(fixture("cases.json")).unwrap()).unwrap();
    let actual: BTreeMap<String, BTreeMap<String, String>> =
        cases.iter().map(|c| (c["name"].as_str().unwrap_or("?").to_owned(), run(c))).collect();
    let path = fixture("expected.json");
    if std::env::var("DOTLOOM_UPDATE_PARITY").as_deref() == Ok("1") {
        std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&actual).unwrap())).unwrap();
        return;
    }
    let expected: BTreeMap<String, BTreeMap<String, String>> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut diffs = Vec::new();
    for (case, outputs) in &expected {
        for (key, digest) in outputs {
            let got = actual.get(case).and_then(|o| o.get(key));
            if got != Some(digest) {
                diffs.push(format!("{case}/{key}: expected {digest}, got {got:?}"));
            }
        }
    }
    assert_eq!(actual.len(), expected.len(), "case count");
    assert!(diffs.is_empty(), "{} outputs differ:\n{}", diffs.len(), diffs.join("\n"));
}
