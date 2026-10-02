# Status

Resume point for any contributor or agent. Keep it short; never store secrets here.

- Branch: `feat/browser-versions` (PR #21: real-browser matrix, requirement evidence)
- Last updated: 2026-10-02
- Overall: **feature-complete and verified in CI; not released.** Every row of
  `docs/requirements.md` has code, a test that runs and documentation, except the
  rows marked `ext` (external accounts/devices, below). No stable release yet: npm
  and crates.io publishing and namespace ownership need the owner's accounts.

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

## Verified evidence (latest runs)

| What | Run | Result |
|---|---|---|
| `ci` on `main@480ad97` (lint incl. boundaries/notices/licenses, Rust tests + visual regression on Linux/lavapipe, ezdxf readback, WASM, TS, browser E2E Chromium/Firefox/WebKit, site checks incl. examples, package + crate consumers) | 36979044165 | all green |
| `compat` on `main@480ad97` (Rust tests Windows/macOS, MSRV 1.89, WebKit macOS, Safari 26.6.1 smoke) | 36979044103 | all green |
| `pages` on `main@480ad97` (build, site checks under `/dotloom/`, deploy, live smoke) | 36979044228 | green; https://tnycl.github.io/dotloom/ |
| `nightly` on `main@9bf37b2` (7 fuzz targets × 60 s with recorded seeds, cargo-deny, solver benchmark) | 36973303755 | green, no crash |
| `release` dry run on `main@b9ed771` (packages, crates, CLI archives for Linux/Windows/macOS, notes) | 36972188938 | green, nothing published |
| Reference-device performance (Windows 11, Ryzen 9 5900X, GTX 1060) | `docs/performance.md`, `docs/perf/2026-10-02/` | DL-PERF-1…8 met |
| Real browsers on the reference device (WebDriver: WebGPU + WebGL2 + playground) | `docs/compat/2026-10-02/` | Chrome 153.0.8010.52, Chrome 154.0.8037.58, Edge 154.0.4258.48 passed |

Branch protection on `main` (read back from the API): 15 required checks, strict
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
| npm `@dotloom/sdk`, `@dotloom/react` (issue #10) | not logged in; the `@dotloom` org does not exist | create the npm org `dotloom` (free for public packages), then either add an automation token as the repository secret `NPM_TOKEN` or configure trusted publishing for `TNYCL/dotloom` / `release.yml` |
| crates.io `dotloom-*` (issue #11) | no crates.io token | create a crates.io API token (scope `publish-new`, `publish-update`) and add it as the repository secret `CARGO_REGISTRY_TOKEN` |
| Namespace ownership (DL-OSS-4) | names were unclaimed on 2026-10-02 but are owned only after the first publish | covered by the two steps above |
| Safari 27 (current major) and Safari WebGPU on real Mac hardware (issues #12, #13) | no Mac; hosted runners have Safari 26.6.x without WebGPU | run `node tests/e2e/webdriver/smoke.mjs --browser safari` on a Mac with Safari 27 (`safaridriver --enable` once) |

A stable GitHub Release is deliberately held back until the npm scope is secured, so
that the release artifacts, the npm tarballs and the crates carry the same names and
checksums (`release.yml` publishes all of them from one verified tag).

## Known issues / decisions to remember

- Firefox reports GPU completion late (WebGL fence ≈ 40 ms, WebGPU work-done
  ≈ 100 ms after submission); frame time there is measured with presented frames.
- Linux CI has no GPU: Chromium's SwiftShader WebGPU device is destroyed right after
  creation, so WebGPU is verified on the reference device (all browsers) and WebGL2
  everywhere.
- Playwright WebKit on Windows does not composite resized WebGL2 canvases
  (reproduced with plain WebGL) — that test is skipped there with the reason.
- `fontdue` reads only the legacy `kern` table (no kerning, issue #15) and its
  `ttf-parser` is unmaintained (issue #16, cargo-deny exception with reason).
- Engine memory ≈ 2 KB per simple object (issue #14).
- Dependabot ignores major TypeScript (7 breaks the toolchain) and `@types/node`
  (types follow the Node 24 runtime) updates.

## Next

1. Merge PR #21 after CI; add the six real-browser jobs to the required checks once
   they are green on `main`.
2. With npm/crates.io access: bump the lockstep version to `1.0.0` (first stable
   SemVer release; 0.x versions are previews), add the CHANGELOG section, tag `v1.0.0`,
   run `release.yml`, verify the published packages from a clean consumer, update
   DL-OSS-4/7.

## Commands

```powershell
pnpm run check                        # format, lint, typecheck, boundaries, notices
cargo test --workspace --all-features
node scripts/build-wasm.mjs           # engine + renderer WASM into packages/sdk/src/wasm
pnpm run build; pnpm run test
cd tests/e2e; npx playwright test     # $env:DOTLOOM_E2E_BROWSERS = 'chromium' to limit
pnpm run smoke:packages; pnpm run smoke:crates
pnpm --filter @dotloom/e2e run bench  # reference-device benchmarks (headed browsers)
node tests/e2e/webdriver/install.mjs chrome previous --dir browsers/chrome-previous
node tests/e2e/webdriver/smoke.mjs --info browsers/chrome-previous/browser.json
```
