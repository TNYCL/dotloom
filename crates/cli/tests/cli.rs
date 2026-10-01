//! CLI behaviour and exit codes (DL-FILE-14).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use std::{path::PathBuf, process::Command};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dotloom"))
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dotloom-cli-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn json(out: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn convert_dxf_to_dotl_then_validate_inspect_and_export() {
    let dotl = tmp("from-dxf.dotl");
    let out = bin().args(["--json", "convert"]).arg(fixtures().join("dxf/r2000_cm.dxf")).arg(&dotl).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v = json(&out);
    assert!(v["import"]["losses"].as_array().unwrap().iter().any(|l| l["what"] == "INSERT"));

    let out = bin().args(["--json", "validate"]).arg(&dotl).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out)["valid"], true);

    let out = bin().args(["--json", "inspect"]).arg(&dotl).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v = json(&out);
    assert!(v["entities"].as_u64().unwrap() >= 6);
    assert!(v["layers"].as_array().unwrap().iter().any(|l| l == "WALLS"));

    for ext in ["svg", "dxf"] {
        let target = tmp(&format!("out.{ext}"));
        let out = bin().args(["--json", "convert"]).arg(&dotl).arg(&target).output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        assert!(std::fs::metadata(&target).unwrap().len() > 100);
    }
}

#[test]
fn validate_reports_violated_rules_with_exit_1() {
    // A document whose stored geometry violates a horizontal constraint.
    let doc = serde_json::json!({
        "schema": 1, "nextId": 10, "layers": [{"id": 1, "name": "L"}],
        "entities": [{"id": 2, "type": "dotloom.line", "layer": 1, "geometry": {"type": "line", "a": [0, 0], "b": [10, 5]}}],
        "constraints": [{"id": 3, "rule": {"kind": "horizontal", "a": {"entity": 2, "anchor": "start"}, "b": {"entity": 2, "anchor": "end"}}}]
    });
    let manifest = serde_json::json!({"format": "dotloom", "formatVersion": 1, "schemaVersion": 1, "producer": {"name": "test", "version": "0"}});
    let bytes = dotloom_io::zip::write(&[
        ("manifest.json".into(), serde_json::to_vec(&manifest).unwrap()),
        ("document.json".into(), serde_json::to_vec(&doc).unwrap()),
    ])
    .unwrap();
    let path = tmp("violated.dotl");
    std::fs::write(&path, bytes).unwrap();
    let out = bin().args(["--json", "validate"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["valid"], false);
    assert_eq!(v["violations"][0]["rule"], "c3");
    assert_eq!(v["error"]["code"], 1);
}

#[test]
fn plugins_flag_enables_plugin_rules() {
    // A shelf whose stored widths break the "sum = width" template.
    let doc = serde_json::json!({
        "schema": 1, "nextId": 10, "layers": [{"id": 1, "name": "L"}],
        "entities": [{"id": 2, "type": "shelf.unit", "layer": 1, "props": {"width": 1800.0, "w1": 600.0, "w2": 600.0, "w3": 500.0, "height": 2000.0}}]
    });
    let manifest = serde_json::json!({"format": "dotloom", "formatVersion": 1, "schemaVersion": 1, "producer": {"name": "test", "version": "0"}});
    let bytes = dotloom_io::zip::write(&[
        ("manifest.json".into(), serde_json::to_vec(&manifest).unwrap()),
        ("document.json".into(), serde_json::to_vec(&doc).unwrap()),
    ])
    .unwrap();
    let path = tmp("shelf.dotl");
    std::fs::write(&path, bytes).unwrap();
    // Without the plugin the rules cannot be checked: reported, not failed.
    let out = bin().args(["--json", "validate"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out)["missingPlugins"][0], "shelf.unit");
    // With the plugin the violated template is found.
    let out = bin()
        .args(["--json", "--plugins"])
        .arg(fixtures().join("plugins/shelf.json"))
        .arg("validate")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        json(&out)["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["rule"].as_str().unwrap().contains("fill the inner width"))
    );
}

#[test]
fn exit_codes() {
    let bad = tmp("bad.dotl");
    std::fs::write(&bad, b"not a zip").unwrap();
    assert_eq!(bin().args(["validate"]).arg(&bad).output().unwrap().status.code(), Some(1));
    assert_eq!(bin().args(["validate", "does-not-exist.dotl"]).output().unwrap().status.code(), Some(3));
    assert_eq!(bin().args(["frobnicate"]).output().unwrap().status.code(), Some(2));
    let png = tmp("x.png");
    let out = bin().args(["--json", "convert"]).arg(fixtures().join("dxf/r12_basic.dxf")).arg(&png).output().unwrap();
    assert_eq!(out.status.code(), Some(4), "PNG without the png feature is a capability error");
    assert_eq!(json(&out)["error"]["code"], 4);
}
