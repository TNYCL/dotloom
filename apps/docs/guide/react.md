# React editor

`@dotloomjs/react` is optional. It renders a complete editor and exports every part
so you can compose your own layout.

```tsx
import { DotloomEditor } from '@dotloomjs/react'
import '@dotloomjs/react/styles.css'

<DotloomEditor
  plugins={[shelves]}            // DotloomPlugin[]
  locale="tr"                    // 'en' | 'tr' | any locale with your messages
  messages={{ 'panel.layers': 'Katmanlar' }}
  theme="system"                 // 'light' | 'dark' | 'system'
  autosave                       // IndexedDB autosave + recovery (default)
  name="raf-tasarimi.dotl"
  onReady={(h) => h.viewport.fit()}
>
  <MyPanel />                    {/* extra sidebar content, inside the editor context */}
</DotloomEditor>
```

## What it includes

- canvas (WebGPU/WebGL2) with grid, snapping markers, selection grips;
- toolbar with built-in and plugin tools (aria-pressed, shortcuts in titles);
- **Properties**: name, layer, lock, visibility, every numeric parameter with its
  unit (typed as `160`, `1,6 m`, `63 in`), a lock button per value, inline solver
  feedback with the nearest allowed value; enum/bool/text properties; read-only
  explanations for missing or disabled plugins; document settings when nothing is
  selected;
- **Rules**: status (solved, degrees of freedom, conflicts with labels), enable,
  disable and remove rules of the selection, add rules from templates;
- **Layers** and a virtualized, keyboard-navigable **Objects** list — every object
  is reachable without the canvas;
- command palette (Ctrl+K), file open (`.dotl`, SVG, DXF), save, export (SVG, DXF,
  PNG), undo/redo;
- status bar: prompts, typed value, cursor position in display units, snap/grid
  toggles, zoom, solving indicator with **Cancel**, autosave state;
- dialogs and banners: recovery of unsaved work, failed changes with the broken
  rules, unreadable files, missing plugins, graphics initialisation failure (the
  panels keep working without a canvas), empty drawing.

## Hooks and state

Components subscribe through `useSyncExternalStore` to exactly what they render:

```ts
const tool = useEditorState((s) => s.tool)          // re-renders on tool change only
const pointer = usePointer()                         // only for cursor read-outs
const doc = useDocument()                            // refetched per committed revision
const info = useEntityInfo(id)
const analysis = useAnalysis()
```

Dragging an object produces preview frames without changing the revision, so it
does not re-render the panels.

## Theming

All colours and sizes are CSS custom properties on `.dl-editor` (`--dl-bg`,
`--dl-text`, `--dl-accent`, `--dl-border`, `--dl-sidebar-width`, …). Dark values
apply with `data-theme="dark"`. Canvas colours follow the theme preset.

## Accessibility

Controls are keyboard reachable with visible focus; the objects list supports
arrows, Home/End, Enter/Space and Shift/Ctrl extension; dialogs move focus inside
and restore it; status messages use live regions; shortcuts never fire while you
type in a field.

## Translations

`en` and `tr` are included. Pass `messages` to override any key (`tool.line`,
`inspector.nearest`, …); unknown locales fall back to English.
