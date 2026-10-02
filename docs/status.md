# Status

Resume point for any contributor or agent. Keep it short; never store secrets here.

- Branch: `feat/viewport` (PR #8: SDK viewport, editor tools, browser E2E)
- Last updated: 2026-10-02
- Overall: **in progress — phases C/D (SDK, renderer, interaction)**. Not a release candidate.

## Merged so far (main)

| PR | Content |
|---|---|
| #1–#3 | workspace, geometry, constraints, document |
| #4 | scene contract + engine |
| #5 | `.dotl`, SVG/DXF, CLI |
| #6 | WASM binding + SDK engine client (Worker protocol) |
| #7 | wgpu renderer + browser binding, CLI PNG |

## Verified evidence (latest runs)

| What | How | Result |
|---|---|---|
| Rust workspace | `cargo test --workspace --all-features` (Windows 11, Rust 1.98.1) | all passed (render GPU tests on Vulkan, GTX 1060) |
| Rust lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` + wasm32 clippy for `dotloom-wasm`, `dotloom-render-web` | clean |
| GPU tests in CI | `rust test (ubuntu-24.04)` with Mesa lavapipe, `DOTLOOM_ALLOW_NO_GPU` unset (a missing adapter fails) | passed on PR #7 |
| SDK | `pnpm --filter @dotloom/sdk test` (Vitest, real WASM in Node) | 30 passed |
| Browser E2E (local, Windows) | `tests/e2e`, Playwright 1.63 against the built package | Chromium 16/16 (WebGPU + WebGL2), Firefox 11 passed / 5 skipped (no WebGPU canvas context), WebKit 10 passed / 6 skipped (no `navigator.gpu`; resized WebGL2 canvases not composited — reproduced with plain WebGL) |
| WASM sizes | `node scripts/build-wasm.mjs` (wasm-release, before wasm-opt) | engine 2035 KiB, renderer 2807 KiB |

## Environment facts (2026-10-02)

- Windows 11, Ryzen 9 5900X (12C/24T), 24 GB RAM, NVIDIA GTX 1060 6 GB
  (driver 32.0.15.8266), 1920×1080.
- Rust 1.98.1 (+ wasm32-unknown-unknown), Node 24.19.0, pnpm 12.8.1 (corepack shim in
  `%APPDATA%\npm`), wasm-bindgen-cli 0.2.129, git 2.55, gh (user `TNYCL`).
- Playwright browsers: Chromium 153 (channel `chromium`), Firefox 155, WebKit 26.6.
- Safari is not available on Windows: the real-Safari smoke must run on a macOS runner.

## Known issues / decisions to remember

- WebKit (Playwright build on Windows) never composites WebGL2 contexts created with
  `antialias: false` and does not show resized WebGL2 canvases. `dotloom-render-web`
  pre-creates the context (`antialias: true`) and uses an sRGB surface on WebGL2
  (draw-based present). The resize case is detected by a plain-WebGL probe and skipped.
- Chromium's emulated device scale factor does not update `devicePixelContentBoxSize`;
  the viewport cross-checks it against `devicePixelRatio`.
- WebGPU device loss is asynchronous: the viewport polls `lost()` (500 ms) and
  recovers on a fresh canvas; WebGL restores on `webglcontextrestored` or after 1.5 s.
- `fontdue` reads only the legacy `kern` table; Inter has GPOS kerning → no kerning.

## External access blockers

- npm: not logged in on this machine; `@dotloom` org does not exist yet.
- crates.io: no token on this machine.
- Real Safari: needs a macOS runner (GitHub-hosted macOS is free for public repos).

## Next

1. Merge PR #8 after CI (browser matrix on Linux).
2. Storage (file open/download, IndexedDB autosave + recovery, adapter) and host
   plugin lifecycle in the SDK.
3. `@dotloom/react` editor, vanilla example, playground, examples (shelf, floorplan,
   timeline, external plugin from tarballs).
4. Docs site (VitePress + TypeDoc + rustdoc) and Pages deploy; release workflow.
5. Performance harness and reference measurements; fuzz targets; MSRV 1.88 check.

## Commands

```powershell
cargo test --workspace --all-features
node scripts/build-wasm.mjs           # engine + renderer WASM into packages/sdk/src/wasm
pnpm --filter @dotloom/sdk run build  # dist/
pnpm --filter @dotloom/sdk test
cd tests/e2e; npx playwright test     # DOTLOOM_E2E_BROWSERS=chromium to limit
```
