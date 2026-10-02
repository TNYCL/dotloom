//! The compatibility manifest (`compatibility.json`) matches the Rust side: crate
//! version, worker protocol, scene format, `.dotl` container, document schema,
//! renderer binding protocol and toolchains. The TypeScript side is checked by
//! `packages/sdk/test/compatibility.test.ts`.
#![allow(clippy::unwrap_used)] // test helpers

use std::path::PathBuf;

use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap()
}

#[test]
fn manifest_matches_the_rust_code() {
    let m: Value = serde_json::from_str(&text("compatibility.json")).unwrap();
    assert_eq!(m["release"], env!("CARGO_PKG_VERSION"));
    assert_eq!(m["packages"]["crates"], env!("CARGO_PKG_VERSION"));
    let f = &m["formats"];
    assert_eq!(f["workerProtocol"], dotloom_wasm::PROTOCOL_VERSION);
    assert_eq!(f["sceneFormat"]["version"], dotloom_engine::scene::SCENE_FORMAT_VERSION);
    assert_eq!(f["sceneFormat"]["magic"].as_str().unwrap().as_bytes(), dotloom_engine::scene::MAGIC);
    assert_eq!(f["dotlContainer"], dotloom_io::dotl::FORMAT_VERSION);
    assert_eq!(f["documentSchema"]["current"], dotloom_engine::document::SCHEMA_VERSION);
    assert_eq!(f["documentSchema"]["oldestReadable"], dotloom_engine::document::OLDEST_SUPPORTED_SCHEMA);
    // The renderer binding constant is compiled for wasm32 only: compare the source.
    let render_web = text("crates/render-web/src/lib.rs");
    let rp = f["renderProtocol"].as_u64().unwrap();
    assert!(render_web.contains(&format!("pub const RENDER_PROTOCOL: u32 = {rp};")), "render protocol {rp}");
    let t = &m["toolchains"];
    assert!(text("Cargo.toml").contains(&format!("rust-version = \"{}\"", t["rustMsrv"].as_str().unwrap())));
    assert!(text("rust-toolchain.toml").contains(&format!("channel = \"{}\"", t["rustPinned"].as_str().unwrap())));
}
