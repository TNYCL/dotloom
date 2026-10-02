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

All shortcuts are configurable: `new EditorCore(engine, viewport, { shortcuts: { undo: ['Mod+u'] } })`.

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
