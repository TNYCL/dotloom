# API reference

- [TypeScript API (`@dotloomjs/sdk`, `@dotloomjs/react`)](/api/ts/index.html) — generated
  with TypeDoc from the published type declarations.
- [Rust API](/api/rust/dotloom_engine/index.html) — rustdoc of the published crates:
  [`dotloom_geometry`](/api/rust/dotloom_geometry/index.html),
  [`dotloom_constraints`](/api/rust/dotloom_constraints/index.html),
  [`dotloom_document`](/api/rust/dotloom_document/index.html),
  [`dotloom_scene`](/api/rust/dotloom_scene/index.html),
  [`dotloom_engine`](/api/rust/dotloom_engine/index.html),
  [`dotloom_io`](/api/rust/dotloom_io/index.html),
  [`dotloom_render`](/api/rust/dotloom_render/index.html).

## Main TypeScript entry points

| Export | What it is |
|---|---|
| `createEditor(container, options)` | engine + viewport + tools + input in one call |
| `DotloomEngine` | asynchronous engine client (Worker): transactions, undo/redo, files, queries, drags, plugins |
| `createNodeEngine()` (`@dotloomjs/sdk/node`) | in-thread engine for Node.js |
| `Viewport` | wgpu canvas: camera, overlays, themes, grid, PNG export, loss recovery |
| `EditorCore`, `bindDom` | DOM-free tool/selection/snapping/shortcut logic and its DOM binding |
| `PluginHost`, `DotloomPlugin` | plugin lifecycle and extension points |
| `Autosave`, `IndexedDbStorage`, `MemoryStorage`, `StorageAdapter` | storage |
| `decodeSceneDelta`, `SceneStore` | the scene contract for custom renderers |
| `formatLength`, `parseLength`, `formatDuration`, … | display units |
| `DotloomEditor`, panels, hooks (`@dotloomjs/react`) | the reference editor |

## Versions

Three versions are independent: the **package** version (semver), the Worker
**protocol** version (`PROTOCOL_VERSION`, checked when an engine starts) and the
document **schema** version (in every file). `engine.capabilities` reports the
engine version, protocol, schema, `.dotl` format and scene format versions.
