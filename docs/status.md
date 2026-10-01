# Status

Resume point for any contributor or agent. Keep it short; never store secrets here.

- Branch: `main` (bootstrap)
- Last updated: 2026-10-02
- Overall: **in progress — phase A/B**. Not a release candidate.

## Verified evidence

| What | How | Result |
|---|---|---|
| Geometry crate | `cargo test -p dotloom-geometry` (Windows 11, Rust 1.98.1) | 57 unit + 12 property (512 cases each) + 1 doctest passed |
| Geometry lint | `cargo clippy -p dotloom-geometry --all-targets -- -D warnings` | clean |

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

1. `dotloom-document`, `dotloom-constraints`, `dotloom-engine` (phase B/C).
2. GitHub repo + CI skeleton.

## Commands

```powershell
cargo test -p dotloom-geometry
cargo clippy -p dotloom-geometry --all-targets -- -D warnings
```
