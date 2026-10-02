# Contributing to Dotloom

Thanks for helping. Bug reports, fixes, documentation and examples are welcome.
Larger features: open an issue first so the design (and, for boundary changes, an
ADR in `docs/adr/`) can be agreed before you invest time.

Please follow the [code of conduct](CODE_OF_CONDUCT.md). Report security issues
privately as described in [SECURITY.md](SECURITY.md), not in public issues.

## Setup

- Rust: the version pinned in `rust-toolchain.toml` (rustup installs it), with the
  `wasm32-unknown-unknown` target. The minimum supported version is in `Cargo.toml`
  (`rust-version`) and is checked in CI.
- `wasm-bindgen-cli` matching `Cargo.lock`:
  `cargo install wasm-bindgen-cli --version 0.2.129 --locked`.
- Node 24 and pnpm (`corepack enable`).

```sh
pnpm install
pnpm run build:wasm
pnpm run build
```

Every command works in Windows PowerShell, Linux and macOS shells.

### Windows

- Rust needs the MSVC toolchain: install "Desktop development with C++" from the
  Visual Studio Build Tools before `rustup`.
- Use PowerShell 5.1 or 7. Environment variables are set differently from POSIX
  shells (the tables below use the POSIX form):

  | POSIX shells | PowerShell |
  |---|---|
  | `DOTLOOM_E2E_BROWSERS=chromium pnpm run test:e2e` | `$env:DOTLOOM_E2E_BROWSERS = 'chromium'; pnpm run test:e2e` |
  | `DOTLOOM_UPDATE_PARITY=1 cargo test -p dotloom-wasm --test parity` | `$env:DOTLOOM_UPDATE_PARITY = '1'; cargo test -p dotloom-wasm --test parity` |

  Remove a variable afterwards with `Remove-Item Env:DOTLOOM_E2E_BROWSERS`.
- GPU tests (`crates/render/tests/headless.rs`) use Direct3D 12 or Vulkan. Without
  a GPU adapter they fail; set `DOTLOOM_ALLOW_NO_GPU=1` to skip them explicitly
  (they print `SKIPPED`, never a pass).
- `npx playwright install` downloads Chromium, Firefox and WebKit for Windows.
  Playwright's WebKit on Windows cannot composite resized WebGL2 canvases, so those
  tests skip with that reason; WebKit is fully covered on macOS in `compat.yml`.
- Benchmarks (`pnpm --filter @dotloomjs/e2e run bench`) open headed browser windows;
  keep the machine otherwise idle while they run.

## Checks

Run what CI runs before you push:

| Purpose | Command |
|---|---|
| Format, lint, typecheck (Rust and TypeScript) | `pnpm run check` |
| Rust tests | `cargo test --workspace --all-features` |
| SDK, React and example tests | `pnpm run test` |
| Browser tests (Chromium, Firefox, WebKit) | `pnpm run test:e2e` (`DOTLOOM_E2E_BROWSERS=chromium` to limit) |
| Documentation site | `pnpm run build:site` |
| Package consumer smoke test | `pnpm run smoke:packages` |
| Crate consumer smoke test | `pnpm run smoke:crates` |

`pnpm run format` fixes formatting. Build or test one thing at a time: parallel
Rust builds use a lot of memory (`.cargo/config.toml` caps cargo at four jobs).

## Rules that reviews enforce

The full list is in [`AGENTS.md`](AGENTS.md). The essentials:

- **Boundaries.** Geometry, document, constraints, scene, engine and io never depend
  on DOM, window, GPU or wgpu (`node scripts/check-boundaries.mjs`). The engine owns
  the only editable document copy; the renderer only reads scene output.
- **No panics on input.** No `unwrap`/`expect`/`panic!` in library code; return
  typed errors. `unsafe` only in the binding crates, with a `// SAFETY:` comment.
- **Determinism.** Sorted maps for anything serialized or compared; elementary
  functions through `dotloom_geometry::math` (clippy rejects `f64::sin` & co.).
- **TypeScript.** Strict mode, no `any` in public API, no React in the SDK, no
  browser globals at module top level.
- **Tests assert behaviour.** Solver results are validated with independent
  formulas. A missing GPU backend or browser is reported as skipped with a reason,
  never as passed. No retries that hide flaky tests.
- **Evidence.** A requirement in `docs/requirements.md` is done only with code, a
  test that ran and documentation; update `docs/status.md` when evidence changes.

## Fixtures and baselines

- **Native/WASM parity** (`tests/fixtures/parity/`): if a change alters results on
  purpose, run `DOTLOOM_UPDATE_PARITY=1 cargo test -p dotloom-wasm --test parity`,
  review the digest diff and explain it in the PR.
- **Visual baselines** (`tests/fixtures/visual/`) belong to Mesa lavapipe on CI
  Linux. To update them, run the `ci` workflow manually with
  "update-visual-baselines", download the `visual` artifact, look at every image and
  commit them. `DOTLOOM_VISUAL_PREVIEW=1 cargo test -p dotloom-cli --features png
  --test visual -- --ignored` renders the scenes on your own GPU to look at them.
- **Fuzzing**: `cargo +nightly fuzz run <target> fuzz/seeds/<target>` (targets in
  `fuzz/Cargo.toml`). Put a fixed crash input into `fuzz/regressions/<target>/`; the
  normal test suite replays it.
- **Benchmarks**: `docs/performance.md` describes the reference device and method.
  Do not compare numbers from other machines with it; CI numbers are not
  performance evidence.

## Pull requests

- Branch from `main`; keep PRs focused; describe what changed, why, and how it was
  verified. Fill in the PR template.
- Commit messages in English, imperative mood (`Fix …`, `Add …`).
- Required checks must pass on the latest commit before merging (squash merge).
- By contributing you agree that your contribution is licensed under
  MIT OR Apache-2.0, like the project.
