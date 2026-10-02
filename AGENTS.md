# AGENTS.md — working on Dotloom

Dotloom is an open-source framework for constraint-driven 2D editors: a headless Rust
engine (geometry, document, constraints, `.dotl` files), a wgpu renderer, a WebAssembly
bridge, a framework-agnostic TypeScript SDK and an optional React editor.

Read these before writing code:

- `docs/requirements.md` — requirement IDs (`DL-*`) and their evidence.
- `docs/status.md` — current branch/commit, what is verified, what is next.
- `docs/adr/` — architecture decisions. Change an ADR before changing a boundary.

## 1. Machine safety (non-negotiable)

Parallel Rust builds can exhaust the development machine's memory.

- Run at most **one** build/test command at a time (cargo, wasm-bindgen, pnpm build,
  Playwright). Never start a second build while one is running, never fan out several
  compiling agents.
- `.cargo/config.toml` caps cargo at `jobs = 4`. Do not raise it.
- Prefer the narrowest command: `cargo test -p dotloom-geometry`, not `--workspace`,
  until the final pre-push check.
- Do not create extra git worktrees or extra `target` directories.

## 2. Architecture boundaries

```text
Host / React UI  ->  TypeScript SDK  ->  versioned protocol  ->  Rust/WASM engine (Worker)
                                                                 -> document + geometry + constraints
                                                                 -> committed deltas / previews / scene
                                                                 -> Rust/wgpu renderer (main thread)
```

- `dotloom-geometry`, `dotloom-document`, `dotloom-constraints`, `dotloom-engine`,
  `dotloom-scene` and `dotloom-io` must build without DOM,
  window, GPU or wgpu. CI checks this with `cargo tree`.
- The Rust engine owns the only editable copy of a document. TypeScript, React and the
  renderer hold derived views keyed by revision.
- Camera, viewport and panel layout are view state, never document geometry.
- GPU-flattened paths are never the source of truth for measurement.
- The renderer consumes `dotloom-scene` output only; it never reads `Document`.

## 3. Code rules

### Rust

- Edition 2024. `cargo fmt` clean, `cargo clippy --workspace --all-targets -- -D warnings`
  clean.
- Library code must not panic on any input: no `unwrap`/`expect`/`panic!` outside tests
  (workspace lints enforce this). Return typed errors.
- `unsafe` is forbidden outside the WASM/render binding crates, and every block there
  needs a `// SAFETY:` comment.
- Canonical geometry is `f64`; GPU buffers are `f32` relative to a local origin.
- Model, solver, tessellation and screen-snap tolerances are distinct types/values; do
  not reuse one for another.
- Deterministic output: use `BTreeMap`/sorted vectors for anything hashed, serialized or
  compared. Never rely on hash-map iteration order.

### TypeScript

- `strict` mode, no unchecked `any` in public API, no engine internals in public types.
- The SDK must not import React. Browser-only globals are touched lazily inside
  functions, never at module top level (SSR/build environments import the SDK).
- Asset URLs (WASM, Worker, fonts) use `new URL('./x', import.meta.url)` with an explicit
  override option; they must work from an installed tarball and under `/dotloom/`.

## 4. Testing and evidence

- Tests must assert real behaviour. No empty passes, no tests that copy the
  implementation, no `continue-on-error` on required gates, no silent skips.
- If a required GPU backend or browser is not available, the test fails or is reported
  as *not run* — it is never counted as passing.
- Solver tests validate final geometry with independent formulas, not only the solver's
  own residual.
- A requirement is "done" only when `docs/requirements.md` links real code, a test that
  ran, and documentation. Update `docs/status.md` when evidence changes.

## 5. Commands (same entry points locally and in CI)

| Purpose | Command |
|---|---|
| Format + lint + typecheck | `pnpm run check` |
| Rust tests | `cargo test --workspace --all-features` |
| WASM build | `pnpm run build:wasm` |
| SDK/React unit tests | `pnpm run test` |
| Browser E2E | `pnpm run test:e2e` |
| Docs site build | `pnpm run build:site` |
| Package smoke | `pnpm run smoke:packages` |

All scripts are Node or cargo based and run on Windows PowerShell, Linux and macOS.

## 6. Git

- Commit author: `TNYCL <tunaycl@outlook.com>`. Do **not** add `Co-Authored-By` trailers
  or "Generated with" lines for AI tools to commits or PR descriptions.
- Feature branches + PRs into `main`; merge only after required checks pass on the
  latest commit. Never force-push `main`.
- Commit messages, PR descriptions, code and docs are in English.

## 7. Security

- `.dotl` files never execute code and never trigger network fetches.
- Imported SVG/DXF are untrusted input: bounded parsers, no script/external entity/
  network reference reaches the DOM.
- Never print or commit secrets. Workflows use read-only default tokens; publish/deploy
  permissions live only in the job that needs them; actions are pinned to full SHAs.
