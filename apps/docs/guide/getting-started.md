# Getting started

Dotloom has three layers you can use separately:

| Layer | Package | Runs in |
|---|---|---|
| Engine (geometry, documents, rules, files) | Rust crates `dotloom-*`, compiled to WebAssembly inside `@dotloom/sdk` | Rust programs, Node.js, a Web Worker |
| SDK (protocol, renderer, tools, storage, plugins) | `@dotloom/sdk` | browsers, Node.js |
| Reference editor | `@dotloom/react` | React 19 |

Units are always **millimetres**, **radians** and **seconds** in the model. Display
units (cm, m, in, ft) only change how values are shown and typed.

## Install

::: warning Not on npm yet
Dotloom `0.1.0` is a pre-release and is not published to npm or crates.io yet.
Build the packages from the repository and install the tarballs:

```sh
git clone https://github.com/TNYCL/dotloom
cd dotloom
pnpm install
pnpm run build:wasm
pnpm run build
pnpm --filter @dotloom/sdk pack --pack-destination "$PWD/dist-packages"
pnpm --filter @dotloom/react pack --pack-destination "$PWD/dist-packages"
```

In your project, install both tarballs in one command (React is optional):

```sh
npm install ../dotloom/dist-packages/dotloom-sdk-0.1.0.tgz ../dotloom/dist-packages/dotloom-react-0.1.0.tgz
npm install react react-dom   # only for @dotloom/react
```
:::

Once published, the packages will be `@dotloom/sdk` and `@dotloom/react`
(names are defined in one place, `packages/names.json`).

## A complete editor without React

```ts
import { createEditor } from '@dotloom/sdk'

const editor = await createEditor(document.getElementById('editor')!)

// Draw something through a transaction (atomic, undoable).
await editor.engine.apply([
  { op: 'createEntity', entity: { geometry: { type: 'rect', origin: [0, 0], width: 1200, height: 800 } } },
])
await editor.viewport.fit()

// Tools: select, pan, line, polyline, rect, circle, arc, path, text, dimension,
// move, rotate, scale, split, trim, extend — or your own.
editor.core.setTool('line')
```

`createEditor` starts the engine in a Web Worker, creates the wgpu canvas inside
the container (WebGPU first, then WebGL2), registers the built-in tools and binds
pointer, wheel, touch and keyboard input. The container needs a size.

See the [vanilla example](/examples/vanilla/) for a page that builds its own toolbar.

## The React editor

```tsx
import { DotloomEditor } from '@dotloom/react'
import '@dotloom/react/styles.css'

export function App() {
  return (
    <div style={{ height: '100vh' }}>
      <DotloomEditor theme="system" locale="tr" />
    </div>
  )
}
```

The editor includes the canvas, toolbar, layers and objects lists, a properties
panel with unit-aware input, a rules panel, a command palette (Ctrl+K), file
open/save/export, autosave with recovery, light/dark themes and English/Turkish
messages. See [React editor](./react.md).

## Headless (Node.js)

```ts
import { createNodeEngine } from '@dotloom/sdk/node'

const engine = await createNodeEngine()
await engine.apply([{ op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [100, 0] } } }])
const bytes = await engine.save() // .dotl
```

Or use the Rust crates directly (`dotloom-engine`, `dotloom-io`) and the `dotloom`
[command-line tool](./cli.md).

## Bundlers

The SDK loads its Worker and WebAssembly files with `new URL('…', import.meta.url)`,
which Vite, webpack 5, Rollup and esbuild understand. With Vite, exclude the
packages from dependency pre-bundling so the asset URLs stay relative to the
package:

```ts
// vite.config.ts
export default defineConfig({
  worker: { format: 'es' },
  optimizeDeps: { exclude: ['@dotloom/sdk', '@dotloom/react'] },
})
```

If you serve the files elsewhere (a CDN, a sub-path), pass explicit URLs:
`DotloomEngine.create({ workerUrl, engineWasmUrl })` and
`Viewport.create(container, engine, { renderWasmUrl })`.

Importing `@dotloom/sdk` has no side effects and touches no browser globals, so it
is safe in server-side rendering; create engines and viewports only in the browser.
