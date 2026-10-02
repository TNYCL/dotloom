# Tools and input

`EditorCore` turns pointer and keyboard input into commands. It is independent of
the DOM (the `bindDom` function connects a viewport's events), so tools can be
tested in Node.js.

## Built-in tools

| Tool | Key | Notes |
|---|---|---|
| Select | V | click, Shift/Ctrl-click, window (left→right) and crossing (right→left) selection, drag to move, drag grips to reshape |
| Pan | H | also middle mouse, Space+drag, two-finger touch |
| Line | L | chains; Shift = horizontal/vertical; type a length (`1200`) or length and angle (`1200,30`) |
| Polyline | P | Enter or double-click finishes, click the first point to close, Backspace removes a point |
| Rectangle | R | click-click or drag; type `width,height` |
| Circle | C | center, radius (typed or clicked) |
| Arc | A | start, point on arc, end |
| Path | B | click for straight segments, drag for smooth curve handles |
| Text | T | click, type, Enter (Shift+Enter: new line) |
| Dimension | D | two points (associative when snapped to anchors), then the line position |
| Move / Rotate / Scale | M / O / S | base point, then target; typed `dx,dy`, degrees or factor |
| Split / Trim / Extend | K / X / E | pick on curves; trim and extend use the selection or everything visible |

Typed values use the document's display unit (`1,5 m` and `120 cm` also work).

## State machines

Every tool moves through `idle → start → preview → commit | cancel`. Escape,
lost pointer capture, window blur and tool changes cancel cleanly; a drag that is
cancelled leaves no history entry. Pointer moves are coalesced: only the newest
position is processed, so a slow solve never builds up a backlog.

## Snapping

Snap candidates (endpoint, midpoint, center, quadrant, intersection, anchor,
nearest, grid) are searched within a radius in **screen pixels** and ranked by
priority, then distance. Hysteresis keeps the previous snap while it stays close
unless a better kind appears or a same-kind candidate is clearly closer. Alt
disables snapping. A snap is only a proposal: committing still runs the solver and
the independent check, so a snap can never commit a broken rule.

## Keyboard

Shortcuts are read only from the focused editor surface — never from text fields —
so typing in a panel cannot delete drawing content.

| Action | Keys |
|---|---|
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z, Ctrl+Y |
| Delete | Delete, Backspace |
| Copy / cut / paste | Ctrl+C / Ctrl+X / Ctrl+V |
| Select all | Ctrl+A |
| Group / ungroup | Ctrl+G / Ctrl+Shift+G |
| Nudge | arrows (Shift = ×10 grid) |
| Zoom to fit | Shift+F, Home |
| Grid / snap | F7 / F3 |
| Command palette (React editor) | Ctrl+K |

## Configuration

Snapping, grid, units and shortcuts are options of the editor core (and of
`<DotloomEditor core={...}>` in React). Everything below is covered by tests in
`packages/sdk/test/editor.test.ts` ("configuration").

```ts
import { EditorCore } from '@dotloom/sdk'

const core = new EditorCore(engine, viewport, {
  // Snap kinds (all on by default): endpoint, midpoint, center, quadrant,
  // intersection, anchor, nearest, grid.
  snap: { nearest: false, quadrant: false },
  snapRadiusPx: 6, // search radius in CSS pixels (default 10)
  snapEnabled: true, // F3 toggles it at run time
  // Replace the keys of an action; `Mod` is Ctrl on Windows/Linux and ⌘ on macOS.
  shortcuts: { delete: ['Delete'], undo: ['Mod+z'], redo: ['Mod+Shift+z'] },
  unitScale: 10, // typed values are centimetres (model units are millimetres)
})

viewport.setGrid({ spacing: 25, majorEvery: 4 }) // grid snapping uses this spacing
```

The document's display unit and grid spacing (`setSettings { displayUnit,
gridSpacing }`) are stored in the `.dotl` file; the React inspector edits them under
"Document". Holding Alt while pointing disables snapping for that move.

## Your own tools

```ts
import type { Tool } from '@dotloom/sdk'

const stamp: Tool = {
  id: 'acme.stamp',
  label: 'Stamp',
  shortcut: 'y',
  async pointerDown(ctx, p) {
    await ctx.apply({ commands: [{ op: 'createEntity', entity: { geometry: { type: 'circle', center: p.point, radius: 50 } } }] })
  },
}
editor.core.registerTool(stamp)
```

Tools receive the snapped point, the raw world point, screen coordinates and
modifiers, and can draw previews with `ctx.setOverlay({ sketch, guides, markers })`.
