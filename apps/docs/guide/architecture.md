# Architecture

```text
 Host UI (vanilla, React, …)
   │  commands, queries            ▲ events, scene deltas (binary)
   ▼                               │
 @dotloom/sdk  ──── versioned messages ────►  Engine (Rust → WASM) in a Web Worker
   │                                            geometry · document · constraints
   │ scene deltas                               solver · history · .dotl/SVG/DXF
   ▼
 Renderer (Rust/wgpu → WASM, main thread): WebGPU or WebGL2
```

## Crates

| Crate | Role | Depends on |
|---|---|---|
| `dotloom-geometry` | `f64` primitives, transforms, intersections, offsets, split/trim/extend, spatial index, tolerances, units | — |
| `dotloom-constraints` | rule expressions with derivatives, component analysis, linear (Cassowary) and numeric (KKT) backends, budgeted solve jobs | geometry |
| `dotloom-document` | entities, layers, groups, constraints, validation, canonical hashing, clipboard, schema migrations | geometry |
| `dotloom-scene` | the public scene contract: items, primitives, binary deltas | geometry |
| `dotloom-engine` | commands, transactions, plugin type compiler and evaluator, solving pipeline, independent checker, history, queries, drags | the above |
| `dotloom-io` | `.dotl` container, SVG and DXF import/export with loss reports | engine |
| `dotloom-render` | wgpu renderer (also headless PNG) | scene |
| `dotloom-cli` | `dotloom` command-line tool | engine, io, (render) |

`geometry`, `document`, `constraints`, `scene` and `engine` never depend on a DOM,
a window or a GPU; CI checks this boundary.

## One authoritative document

The engine owns the only editable document. Everything else — the renderer's
scene, React state, panel snapshots — is derived and keyed by the document
**revision**. Every change is a **transaction** of commands:

1. validate the commands against a copy-on-write working state,
2. solve the affected rules (only the connected component),
3. check every hard rule independently of the solver,
4. commit atomically: new revision, one undo entry, events, scene delta.

A failed transaction leaves the document exactly as it was. Results that arrive for
an older revision are rejected (`stale`).

## Worker protocol

Messages carry a request ID, the protocol version and the document revision; errors
carry a machine-readable code. Long solves run in budgeted steps with event-loop
yields in between, so cancellation (`AbortSignal`) takes effect during the solve,
and newer drag positions replace queued ones (only the newest is solved). A trapped
WebAssembly instance is reported as `crashed`; the host creates a new engine and
reopens the document (for example from autosave).

## Renderer

The renderer consumes only scene deltas. Items are grouped into order-preserving
chunks with bounding boxes (viewport culling, batched draws); meshes are cached per
item and re-tessellated only when the item changes or a curve is shown at a new
zoom level. Lines keep a constant screen width with analytic anti-aliasing; text
uses a signed-distance-field atlas of a bundled OFL font (Latin, Turkish, Greek,
Cyrillic, technical symbols). Shaders use only WebGL2-level features, so the WebGPU
and WebGL2 backends run the same code. GPU buffers use `f32` relative to local
origins; the model stays `f64`.

## Decisions

The architecture decisions are recorded in the repository
(`docs/adr/0001` … `0008`): crate boundaries, units, engine ownership and protocol,
the solver, the `.dotl` container, the plugin model, the renderer and package names.
