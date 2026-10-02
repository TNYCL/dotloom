# Status

Resume point for any contributor or agent. Keep it short; never store secrets here.

- Branch: `docs/npm-published` (npm packages published)
- Last updated: 2026-10-02
- Overall: **1.1.0 released** (tag `v1.1.0` on `main@ef0f517`,
  https://github.com/TNYCL/dotloom/releases/tag/v1.1.0), on **npm** (`@dotloomjs/sdk`,
  `@dotloomjs/react`) and **crates.io** (all eight `dotloom-*` crates). Open: Safari 27
  is not verified (no Mac; issue #13), so the goal is not complete.

## Merged (main)

| PR | Content |
|---|---|
| #1–#3 | workspace, geometry, constraints, document |
| #4 | scene contract + engine |
| #5 | `.dotl`, SVG/DXF, CLI |
| #6 | WASM binding + SDK engine client (Worker protocol) |
| #7 | wgpu renderer + browser binding, CLI PNG |
| #8 | SDK viewport, editor core/tools, storage, plugins, React editor, playground, examples, docs site, Pages, package smoke |
| #9 | performance (DL-PERF), determinism/parity, compatibility matrix, fuzzing, visual tests, release workflow |
| #17 | single-edit measurement (DL-DOC-9), platform table, nightly fixes |
| #18 | incremental linear drags, rule class checks, plugin panels/storage in the editor, UI state tests, accessibility scan, file fixtures, crate consumer, third-party notices, CI hygiene |
| #21 | released-browser matrix (Chrome, Edge, Firefox current and previous major; Safari) through WebDriver; requirement evidence for every row |
| #22 | load-error title, technical details disclosure, unmount test, released-browser support table |
| #24 | release 1.0.0: lockstep version, CHANGELOG, install from the GitHub Release, `publish` mode for partial releases |
| #25 | release verification from public URLs (`scripts/verify-release.mjs`, job in `release.yml`), status after 1.0.0 |
| #26 | pair kerning shared by renderer and engine (checked against HarfBuzz); outlines via `read-fonts` and an own rasterizer; fontdue/ttf-parser removed (closed #15, #16) |
| #27 | engine memory −26 % per object (cached evaluations drop spare capacity; part of #14) |
| #23 | `@types/node` 24.9.2 → 24.19.0 (Dependabot) |
| #29 | registry publishing in the GitHub environment `release` |
| #30 | npm scope `@dotloomjs`, version 1.1.0 |
| #31, #32 | release publish mode: complete partial publishes (rate limits, real exit codes, local tarball paths) |

## Verified evidence (latest runs)

| What | Run | Result |
|---|---|---|
| `ci` / `compat` / `pages` on the released commit `main@36b5e63` | 36981837890 / 36981837919 / 36981837985 | all green (21 required checks incl. six released-browser jobs) |
| `release` dry run on `main@36b5e63` (release candidate) | 36981855431 | green |
| `release` for tag `v1.1.0` | 36995447580 | GitHub Release published and verified from its URLs; 5 crates published before crates.io's new-crate rate limit; npm `404 Scope not found` |
| `release` publish mode for `v1.1.0` | 36997278172, 36998435514 | remaining crates published (all eight on crates.io); npm still `Scope not found` |
| `release` publish mode for `v1.1.0` (npm) | 36999757823 | `@dotloomjs/sdk` and `@dotloomjs/react` 1.1.0 published with provenance after the owner created the npm organization `dotloomjs`; byte-identical to the release tarballs |
| npm consumer | `examples/external-plugin` copied outside the repository, `npm install` from registry.npmjs.org | Node tests 3/3, build at the root and under `/dotloom/`, `tests/e2e/external` 2/2 in Chromium |
| crates.io consumer | fresh Cargo project with `dotloom-engine = "=1.1.0"`, `dotloom-io = "=1.1.0"` from crates.io | builds and runs (solve, save, reopen, SVG export) |
| `release` for tag `v1.0.0` | 36982429946 | GitHub Release published with 3 CLI archives, 8 crates, 2 npm tarballs, `SHA256SUMS`, `release-manifest.json`; npm and crates.io recorded as not published (no credentials) |
| Published release checked from its public URLs (Windows 11) | `node scripts/verify-release.mjs v1.0.0` | checksums match; npm tarballs installed by URL into a fresh project and used from Node (engine 1.0.0, solve, save, reopen); Windows CLI `dotloom 1.0.0` inspects a 0.1.0 fixture |
| `ci` on `main@480ad97` (lint incl. boundaries/notices/licenses, Rust tests + visual regression on Linux/lavapipe, ezdxf readback, WASM, TS, browser E2E Chromium/Firefox/WebKit, site checks incl. examples, package + crate consumers) | 36979044165 | all green |
| `compat` on `main@480ad97` (Rust tests Windows/macOS, MSRV 1.89, WebKit macOS, Safari 26.6.1 smoke) | 36979044103 | all green |
| `pages` on `main@480ad97` (build, site checks under `/dotloom/`, deploy, live smoke) | 36979044228 | green; https://tnycl.github.io/dotloom/ |
| `nightly` on `main@9bf37b2` (7 fuzz targets × 60 s with recorded seeds, cargo-deny, solver benchmark) | 36973303755 | green, no crash |
| `release` dry run on `main@b9ed771` (packages, crates, CLI archives for Linux/Windows/macOS, notes) | 36972188938 | green, nothing published |
| Reference-device performance (Windows 11, Ryzen 9 5900X, GTX 1060) | `docs/performance.md`, `docs/perf/2026-10-02/` | DL-PERF-1…8 met |
| Released browsers in CI (WebDriver; `compat` on PR #21) | 36980003137 | Chrome 154.0.8037.92/153.0.8010.52, Edge 154.0.4258.53/153.0.4234.48, Firefox 157.0/156.0.1 (Linux), Safari 26.6.1 (macOS): WebGL2 and playground passed everywhere, WebGPU where offered (`docs/compat/2026-10-02/ci-run-36980003137/`) |
| Released browsers on the reference device (WebGPU + WebGL2 + playground) | `docs/compat/2026-10-02/windows-gtx1060/` | Chrome 153.0.8010.52, Chrome 154.0.8037.58, Edge 154.0.4258.48 passed |

Branch protection on `main` (read back from the API): 21 required checks (incl. the six released-browser jobs), strict
(up to date), enforced for admins, linear history, no force pushes, no deletions,
conversation resolution required.

## Environment facts (2026-10-02)

- Windows 11 Home 26200, Ryzen 9 5900X (12C/24T), 24 GB RAM, NVIDIA GTX 1060 6 GB
  (driver 32.0.15.8266), display 1920×1080 at 120 Hz.
- Rust 1.98.1 (+ wasm32-unknown-unknown), Node 24.19.0, pnpm 12.8.1,
  wasm-bindgen-cli 0.2.129, gh (user `TNYCL`).
- Installed browsers: Chrome 154.0.8037.58, Edge 154.0.4258.48. Playwright builds:
  Chromium 153, Firefox 155, WebKit 26.6.
- Current majors on 2026-10-02: Chrome 154, Edge 154, Firefox 157, Safari 27
  (previous: 153, 153, 156, 26). GitHub-hosted macOS runners ship Safari 26.6.x.

## External access blockers (concrete user actions)

| Blocked | Why | What the owner needs to do |
|---|---|---|
| Safari 27 (current major) and Safari WebGPU on real Mac hardware (issue #13) | no Mac; hosted runners have Safari 26.6.x without WebGPU | on a Mac with Safari 27: `sudo safaridriver --enable`, serve the harness and playground, `node tests/e2e/webdriver/smoke.mjs --browser safari --webgpu required` |

Once the secrets exist, run the `release` workflow manually with `publish: v1.0.0`:
it publishes the release's own npm tarballs and the crates (repackaged from the tag
and compared byte-for-byte with the release's `.crate` files), skips versions that
are already in a registry and records the result on the release. 1.0.0 was a GitHub-only
release under `@dotloom/*`; from 1.1.0 the npm scope is `@dotloomjs`.

## Known issues / decisions to remember

- Firefox reports GPU completion late (WebGL fence ≈ 40 ms, WebGPU work-done
  ≈ 100 ms after submission); frame time there is measured with presented frames.
- Linux CI has no GPU: Chromium's SwiftShader WebGPU device is destroyed right after
  creation, so WebGPU is verified on the reference device (all browsers) and WebGL2
  everywhere.
- Playwright WebKit on Windows does not composite resized WebGL2 canvases
  (reproduced with plain WebGL) — that test is skipped there with the reason.
- Text uses the default font's pair kerning (shared tables, checked against
  HarfBuzz); outlines come from `read-fonts` (no advisory exceptions in cargo-deny).
- Engine memory ≈ 1.5 KB per simple object after load (was ≈ 2 KB; issue #14 tracks
  further reductions, e.g. not caching anchors of built-in shapes).
- Dependabot ignores major TypeScript (7 breaks the toolchain) and `@types/node`
  (types follow the Node 24 runtime) updates.

## Next

1. Safari 27 smoke test on a Mac (issue #13): `node tests/e2e/webdriver/smoke.mjs` with
   `safaridriver` (see `apps/docs/guide/platforms.md`).
2. Further engine memory work (issue #14, optional).

## Commands

```powershell
pnpm run check                        # format, lint, typecheck, boundaries, notices
cargo test --workspace --all-features
node scripts/build-wasm.mjs           # engine + renderer WASM into packages/sdk/src/wasm
pnpm run build; pnpm run test
cd tests/e2e; npx playwright test     # $env:DOTLOOM_E2E_BROWSERS = 'chromium' to limit
pnpm run smoke:packages; pnpm run smoke:crates
pnpm run verify:release -- v1.0.0     # a published release, from its public URLs
pnpm --filter @dotloomjs/e2e run bench  # reference-device benchmarks (headed browsers)
node tests/e2e/webdriver/install.mjs chrome previous --dir browsers/chrome-previous
node tests/e2e/webdriver/smoke.mjs --info browsers/chrome-previous/browser.json
```
