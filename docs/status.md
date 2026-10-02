# Status

Resume point for any contributor or agent. Keep it short; never store secrets here.

- Branch: `feat/perf` (draft PR #9: performance work, DL-PERF)
- Last updated: 2026-10-02
- Overall: **in progress — performance done on the reference device; compatibility
  matrix, fuzzing, parity, release and repository documents remain.** Not a release
  candidate.

## Merged (main)

| PR | Content |
|---|---|
| #1–#3 | workspace, geometry, constraints, document |
| #4 | scene contract + engine |
| #5 | `.dotl`, SVG/DXF, CLI |
| #6 | WASM binding + SDK engine client (Worker protocol) |
| #7 | wgpu renderer + browser binding, CLI PNG |
| #8 | SDK viewport, editor core/tools, storage/autosave, plugins, React editor, playground, examples, docs site, Pages workflow, package consumer smoke |

GitHub Pages is enabled (workflow source). Run 36961922289 on `main@363b04d` built the
site, deployed it and smoke-tested the live URL: https://tnycl.github.io/dotloom/
(guide, playground, examples, TypeDoc and rustdoc all answer 200).

## Verified evidence (latest runs)

| What | How | Result |
|---|---|---|
| CI on `main@363b04d` | `ci` run 36961922318: rust fmt+clippy+boundaries, rust tests (Linux, lavapipe GPU), wasm build, TS lint/test/build, browser E2E Chromium/Firefox/WebKit (Linux), package consumer | all green |
| Rust workspace (local) | `cargo test --workspace --all-features` (Windows 11, Rust 1.98.1) | all passed |
| Rust lint (local) | clippy `-D warnings` (all targets/features) + wasm32 clippy (`dotloom-wasm`, `dotloom-render-web`) | clean |
| TS unit (local) | SDK 37, React 9, example plugins 7 (Vitest, real WASM in Node) | all passed |
| Browser E2E (local, Windows) | `cd tests/e2e; npx playwright test` | 50 passed, 16 skipped (reasons recorded per test: WebKit/Firefox WebGPU, WebKit resize compositing) |
| Performance (reference device) | `docs/performance.md`; raw results `docs/perf/2026-10-02/` | DL-PERF-1…8 met/reported |

Linux CI: Chromium WebGPU tests are skipped because plain WebGPU on a clean page also
fails there (`Device was destroyed`, SwiftShader, no GPU); WebGPU is verified locally
on the GTX 1060 (Chrome, Edge, Firefox headed).

## Environment facts (2026-10-02)

- Windows 11 Home 26200, Ryzen 9 5900X (12C/24T), 24 GB RAM, NVIDIA GTX 1060 6 GB
  (driver 32.0.15.8266), display 1920×1080 at 120 Hz.
- Rust 1.98.1 (+ wasm32-unknown-unknown), Node 24.19.0, pnpm 12.8.1,
  wasm-bindgen-cli 0.2.129, gh (user `TNYCL`).
- Installed browsers: Google Chrome 154.0.8037.58, Microsoft Edge 154.0.4258.48 (used
  through Playwright `channel`). Playwright builds: Chromium 153, Firefox 155, WebKit
  26.6. No release Firefox, no Safari.

## Known issues / decisions to remember

- Firefox reports GPU completion late (WebGL fence ≈ 40 ms, WebGPU work-done
  ≈ 100 ms after submission); frame time there is measured with presented frames.
- Firefox exposes WebGPU only in headed mode (Playwright build); benchmarks run headed.
- Native and WASM solver runs can differ in the last bits (platform `sin`/`cos` vs
  Rust's libm on wasm32) → iteration counts differ slightly. Parity work (DL-TEST-6)
  should route transcendental functions through `libm` in the core crates.
- Engine memory ≈ 2 KB per simple object (document 661 B, evaluation cache with
  world-space anchors/drawables, indexes); 100k objects peak at ≈ 200 MiB WASM.
- WebKit (Playwright build on Windows) does not composite resized WebGL2 canvases;
  detected by a plain-WebGL probe and skipped.
- `fontdue` reads only the legacy `kern` table; Inter has GPOS kerning → no kerning.

## External access blockers

- npm: not logged in on this machine; the `@dotloom` org does not exist yet.
- crates.io: no token on this machine.
- Real Safari: needs a macOS machine/runner (GitHub-hosted macOS is free for public
  repositories) — not yet set up.

## Next

1. Merge PR #9 after CI.
2. `compat.yml`: Rust tests on Windows/Linux/macOS, MSRV build (1.89), macOS WebKit +
   Safari smoke; branch protection on `main`.
3. Native/WASM parity tests (DL-TEST-6, libm), visual regression baselines
   (DL-TEST-9), bounded fuzz targets with seeds + `nightly.yml` (DL-TEST-10), ezdxf
   readback of DXF exports in CI.
4. Repository documents: README update, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT,
   CHANGELOG, issue/PR templates; requirements pass over all `planned` rows.
5. `release.yml` (GitHub Release with packages/tarballs); npm/crates.io publishing
   once access exists.

## Commands

```powershell
pnpm run check                        # format + lint + typecheck
cargo test --workspace --all-features
node scripts/build-wasm.mjs           # engine + renderer WASM into packages/sdk/src/wasm
pnpm run build; pnpm run test
cd tests/e2e; npx playwright test     # DOTLOOM_E2E_BROWSERS=chromium to limit
pnpm --filter @dotloom/e2e run bench  # reference-device benchmarks (headed browsers)
```
