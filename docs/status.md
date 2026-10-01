# Status

Resume point for any contributor or agent. Keep it short; never store secrets here.

- Branch: `feat/engine` (scene + engine)
- Last updated: 2026-10-02
- Overall: **in progress — phase A/B**. Not a release candidate.

## Verified evidence

| What | How | Result |
|---|---|---|
| Workspace tests | `cargo test --workspace` (Windows 11, Rust 1.98.1) | geometry 57+12 property+doc, constraints 7+22+1 Jacobian property+doc, document 8+13+doc, scene 4, engine 3+21 — all passed |
| Workspace lint | `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| CI | PRs #1–#3 merged with green `rust fmt + clippy` and `rust test (ubuntu-24.04)` | see GitHub Actions |

## Environment facts (2026-10-02)

- Windows 11, Ryzen 9 5900X (12C/24T), 24 GB RAM, NVIDIA GTX 1060 6 GB
  (driver 32.0.15.8266), 1920×1080.
- Rust 1.98.1 (+ wasm32-unknown-unknown), Node 24.19.0, pnpm 12.8.1 (corepack shim in
  `%APPDATA%\npm`), wasm-bindgen-cli 0.2.129, git 2.55, gh (user `TNYCL`).
- Browsers: Chrome 154, Edge 154 installed; Firefox not installed locally; Safari not
  available on Windows (real Safari smoke must run on a macOS runner).

## External access blockers

- npm: not logged in on this machine; `@dotloom` org does not exist yet.
- crates.io: no token on this machine.

## Next

1. `dotloom-io`: `.dotl` container, SVG/DXF import/export, CLI.
2. `dotloom-wasm` binding + TypeScript SDK (Worker protocol), then wgpu renderer.

## Commands

```powershell
cargo test -p dotloom-geometry
cargo clippy -p dotloom-geometry --all-targets -- -D warnings
```
