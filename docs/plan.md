# Dotloom implementation plan

Phases are an execution order, not delivery milestones. The product is complete only
when every requirement in [`requirements.md`](requirements.md) has implementation, a
test that ran and documentation, and the completion conditions in the brief hold.

| Phase | Scope | Main outputs |
|---|---|---|
| A | Environment, repo, ADRs, risk checks | workspace, toolchain pins, licenses, ADR-0001…0008, CI skeleton, wgpu WebGPU/WebGL2 + Worker/WASM spike in the real crates |
| B | Geometry + document | `dotloom-geometry`, `dotloom-document`, transactions/history in `dotloom-engine`, `.dotl` round-trip, property corpus, native/WASM parity harness |
| C | Constraints | `dotloom-constraints`: graph + components, kasuari linear backend, hierarchical Gauss–Newton backend, hard/soft semantics, diagnostics, budgeted stepping, shelf acceptance |
| D | Renderer + interaction | `dotloom-scene`, `dotloom-render` (wgpu, lyon, SDF text), `dotloom-render-web`, SDK tools/snapping/previews, browser flows |
| E | SDK + extensibility | protocol v1, Worker host, plugin registry (model defs + TS tools/panels), vanilla + React bindings, external consumer, domain examples |
| F | Files + editor | SVG/DXF/PNG, migrations, autosave/recovery, React editor panels, a11y, support matrices |
| G | Product verification | perf on the reference device, OS/browser/backend matrix, package consumers, docs + Pages, release |

## Working rules

- One build at a time (`AGENTS.md` §1).
- Every phase lands through PRs into `main` with green required checks.
- `docs/status.md` is updated whenever evidence changes; it is the resume point.
- Scope is not narrowed silently: unfinished items stay open in `requirements.md`.

## Key technical choices (details in ADRs)

- Units: model length = millimetre, angle = radian, time = second (ADR-0002).
- Engine owns the document; Worker hosts the engine WASM; main thread hosts the
  renderer WASM (ADR-0003).
- Solver: component classification; linear components → kasuari (Cassowary);
  geometric/mixed components → hierarchical (lexicographic) Gauss–Newton with hard
  constraints as exact equality rows (ADR-0004).
- `.dotl` = ZIP container with `manifest.json`, `document.json`, `assets/` (ADR-0005).
- Plugins: serializable entity definitions evaluated in Rust via a typed expression AST;
  trusted TypeScript for tools and panels (ADR-0006).
- Renderer: wgpu with runtime WebGPU → WebGL2 selection, screen-space line expansion,
  SDF glyph atlas with an embedded OFL font (ADR-0007).
