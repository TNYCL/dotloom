# @dotloomjs/react

Optional React editor for [Dotloom](https://github.com/TNYCL/dotloom), the
open-source framework for constraint-driven 2D editors. It builds on
[`@dotloomjs/sdk`](../sdk/README.md): the Rust engine runs in a Web Worker and the
wgpu renderer draws with WebGPU or WebGL2.

```tsx
import { DotloomEditor } from '@dotloomjs/react'
import '@dotloomjs/react/styles.css'

export function App() {
  return <DotloomEditor plugins={[]} locale="en" theme="system" style={{ height: '100vh' }} />
}
```

`<DotloomEditor>` is the complete reference editor: canvas, toolbar, layer and
object lists, inspector with unit-aware numeric input, constraints panel with
conflict messages, command palette, undo/redo, open/save/export, autosave with
recovery, light/dark themes and English/Turkish messages.

To build your own UI, compose the panels (`Toolbar`, `LayersPanel`,
`ObjectsPanel`, `Inspector`, `ConstraintsPanel`, `StatusBar`, `CommandPalette`)
inside an `EditorProvider` and read state with the hooks (`useEditorState`,
`useDocument`, `useRevisionQuery`, …). Hooks subscribe to slices of editor state,
so pointer moves and drag previews do not re-render the whole tree.

React 19 is a peer dependency.

Documentation: <https://tnycl.github.io/dotloom/guide/react.html>

Licensed under either of MIT or Apache-2.0 at your option.
