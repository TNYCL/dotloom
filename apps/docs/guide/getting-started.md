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

```sh
npm install @dotloom/sdk
# optional React editor
npm install @dotloom/react react react-dom
```

> Package names are defined in one place in the repository (`packages/names.json`).
> If you consume Dotloom before it is published to npm, install the packed tarballs
> (`pnpm pack`) or use the workspace.

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
