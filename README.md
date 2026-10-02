# Dotloom

**Open-source framework for constraint-driven 2D editors.**

[![ci](https://github.com/TNYCL/dotloom/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/TNYCL/dotloom/actions/workflows/ci.yml)
[![compat](https://github.com/TNYCL/dotloom/actions/workflows/compat.yml/badge.svg?branch=main)](https://github.com/TNYCL/dotloom/actions/workflows/compat.yml)
[![pages](https://github.com/TNYCL/dotloom/actions/workflows/pages.yml/badge.svg?branch=main)](https://github.com/TNYCL/dotloom/actions/workflows/pages.yml)

Dotloom lets you build editors where drawings follow rules: a shelf whose
compartments always fill its width, a floor plan whose doors stay in their walls,
a schedule whose blocks keep their order. It is

- a **Rust engine** — geometry, documents, transactions and undo, linear and
  nonlinear constraint solving, `.dotl` project files, SVG/DXF import and export —
  that runs headless or in the browser through WebAssembly in a Web Worker;
- a **wgpu renderer** for WebGPU and WebGL2 (and native GPUs for PNG export);
- a **framework-agnostic TypeScript SDK** with tools, snapping, storage and plugins;
- an optional **React editor** with panels, inspector, command palette, themes and
  English/Turkish messages.

**Try it:** [playground](https://tnycl.github.io/dotloom/playground/) ·
[examples](https://tnycl.github.io/dotloom/guide/examples) ·
[documentation](https://tnycl.github.io/dotloom/)

> **Status:** `1.0.0`, the first stable release. The packages and the CLI are
> attached to the [GitHub Release](https://github.com/TNYCL/dotloom/releases/tag/v1.0.0);
> they are not on the npm and crates.io registries yet. See
> [`docs/status.md`](docs/status.md) for what is verified and what is open, and
> [`docs/requirements.md`](docs/requirements.md) for the evidence behind every
> requirement.

## Use it

Install the packages from the release (npm, pnpm and yarn accept tarball URLs):

```sh
npm install https://github.com/TNYCL/dotloom/releases/download/v1.0.0/dotloom-sdk-1.0.0.tgz https://github.com/TNYCL/dotloom/releases/download/v1.0.0/dotloom-react-1.0.0.tgz
```

Or build them from this repository (bash or PowerShell):

```sh
git clone https://github.com/TNYCL/dotloom
cd dotloom
pnpm install
pnpm run build:wasm
pnpm run build
pnpm --filter @dotloom/sdk pack --pack-destination "$PWD/dist-packages"
pnpm --filter @dotloom/react pack --pack-destination "$PWD/dist-packages"
# in your project: install both tarballs in one command
npm install ../dotloom/dist-packages/dotloom-sdk-1.0.0.tgz ../dotloom/dist-packages/dotloom-react-1.0.0.tgz
```

A complete editor without a framework:

```ts
import { createEditor } from '@dotloom/sdk'

const editor = await createEditor(document.getElementById('editor')!)
await editor.engine.apply([
  { op: 'createEntity', entity: { geometry: { type: 'rect', origin: [0, 0], width: 1200, height: 800 } } },
])
await editor.viewport.fit()
editor.core.setTool('line')
```

The React editor:

```tsx
import { DotloomEditor } from '@dotloom/react'
import '@dotloom/react/styles.css'

export const App = () => <div style={{ height: '100vh' }}><DotloomEditor theme="system" locale="tr" /></div>
```

Your own object types are plugins — typed properties, anchors, drawing recipes and
rules written as expressions:

```ts
import { PluginHost } from '@dotloom/sdk'

const plugins = new PluginHost(editor.engine, editor.core)
await plugins.register({
  id: 'acme.furniture',
  version: '1.0.0',
  types: [{
    typeId: 'acme.table', version: 1,
    props: { width: { type: 'number', dim: 'length', default: '120cm' } },
    constraints: [{ lhs: 'width', op: '>=', rhs: '60cm' }],
    // anchors, render recipes, …
  }],
})
```

See [Getting started](https://tnycl.github.io/dotloom/guide/getting-started),
[Plugins](https://tnycl.github.io/dotloom/guide/plugins) and the
[external plugin example](examples/external-plugin), which is built and tested
from the packed tarballs outside this repository in CI.

## Repository

| Path | What |
|---|---|
| `crates/geometry`, `document`, `constraints`, `scene`, `engine`, `io` | headless Rust core (no DOM, window or GPU dependency — checked in CI) |
| `crates/render`, `render-web`, `wasm`, `cli` | wgpu renderer, browser bindings, WebAssembly engine binding, `dotloom` CLI |
| `packages/sdk`, `packages/react` | `@dotloom/sdk`, `@dotloom/react` |
| `apps/docs`, `apps/playground` | documentation site (VitePress, TypeDoc, rustdoc) and the reference editor |
| `examples/` | vanilla, shelf configurator, floor plan, timeline, external plugin |
| `tests/e2e`, `tests/fixtures`, `fuzz` | browser tests and benchmarks, fixtures and baselines, fuzz targets |
| `docs/` | requirements, status, ADRs, performance |

## Develop

Requirements: Rust (version from `rust-toolchain.toml`, plus `wasm32-unknown-unknown`),
`wasm-bindgen-cli` 0.2.129, Node 24 and pnpm. The same commands run locally
(Windows PowerShell, Linux, macOS) and in CI:

| Purpose | Command |
|---|---|
| Format, lint, typecheck | `pnpm run check` |
| Rust tests | `cargo test --workspace --all-features` |
| WebAssembly build | `pnpm run build:wasm` |
| Packages | `pnpm run build` |
| SDK / React / example tests | `pnpm run test` |
| Browser tests | `pnpm run test:e2e` |
| Documentation site | `pnpm run build:site` |
| Package consumer smoke test | `pnpm run smoke:packages` |
| Crate consumer smoke test | `pnpm run smoke:crates` |
| Reference-device benchmarks | `pnpm --filter @dotloom/e2e run bench` |

Read [`AGENTS.md`](AGENTS.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md) before
changing code.

## Platforms

Chrome, Edge and Firefox on WebGPU and WebGL2, Safari (WebGL2; WebGPU where Safari
exposes it), Node.js for headless use, and the Rust crates on Windows, Linux and
macOS. The [support table](https://tnycl.github.io/dotloom/guide/platforms) lists
which combination is verified by which test.

## Performance

On the reference device (Ryzen 9 5900X, GTX 1060, 1080p at 120 Hz): 10 000 shapes
pan and zoom without a dropped frame, hit-testing and snapping answer in under a
millisecond, editing a 200-variable sketch takes 23 ms at p95, and a 10 000-object
file opens in about 0.1 s. Method and all numbers: [`docs/performance.md`](docs/performance.md).

## Security

`.dotl` files never execute code or fetch from the network; SVG and DXF imports are
bounded and sanitized. Report vulnerabilities privately as described in
[`SECURITY.md`](SECURITY.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.

Third-party assets: the renderer embeds a subset of Inter (SIL Open Font License
1.1, see `crates/render/assets/`).
