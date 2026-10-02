# ADR-0001: Workspace layout and crate boundaries

Status: accepted (2026-10-02)

## Context

The brief requires a headless Rust engine, a replaceable wgpu renderer, a WASM bridge,
a framework-agnostic TypeScript SDK and an optional React editor, in one Cargo + pnpm
monorepo.

## Decision

Rust crates (`crates/*`, published as `dotloom-*`):

| Crate | Depends on | Must not depend on |
|---|---|---|
| `dotloom-geometry` | serde, robust, rstar | document, GPU, DOM |
| `dotloom-document` | geometry | GPU, DOM |
| `dotloom-constraints` | geometry, kasuari, nalgebra | document, GPU, DOM |
| `dotloom-scene` | geometry | GPU, DOM |
| `dotloom-engine` | geometry, document, constraints, scene | GPU, DOM |
| `dotloom-io` | engine | GPU, DOM |
| `dotloom-render` | scene, wgpu, lyon, fontdue | document, engine |
| `dotloom-wasm` | engine, io (no `png`) | render |
| `dotloom-render-web` | render (WebGPU + WebGL backends) | engine, document |
| `dotloom-cli` | io (and `dotloom-render` behind its `png` feature) | DOM |

Two additions to the brief's sketch keep the boundaries enforceable:

- `dotloom-scene` holds the public scene contract (display lists + binary delta
  encoding). The engine produces it and the renderer consumes it, so neither depends
  on the other.
- `dotloom-render-web` is a separate `cdylib` for the main-thread renderer instance.
  The engine WASM in the Worker never links wgpu, and the renderer instance never owns
  a document or solver.

Amended 2026-10-02 to match the implementation: PNG export lives in the CLI's
`png` feature (so `dotloom-io` stays GPU-free and is covered by the boundary check),
and there is no `dotloom` facade crate — Rust users depend on `dotloom-engine` and
`dotloom-io` directly.

TypeScript packages: `packages/sdk` (no React) and `packages/react`. Apps:
`apps/playground`, `apps/docs`. Examples live in `examples/*` and are built in CI.

## Consequences

- CI asserts the forbidden edges with `cargo tree` checks (`scripts/check-boundaries.mjs`).
- Two WASM binaries ship in the SDK package (engine + renderer).
