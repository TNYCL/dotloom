# Documents and commands

## The document

A document (`document.json` inside a `.dotl` file) contains:

- **entities** — stable numeric IDs, a namespaced type (`dotloom.line`,
  `acme.shelf`), a layer, a transform, built-in geometry or typed properties,
  optional style, name, lock and visibility, and opaque plugin `data`;
- **layers** (order, visibility, lock, colour) and **groups**;
- **constraints** — rules between parameters and anchors, with a strength;
- **settings** — display unit, grid, time axis, title.

IDs never change when objects are reordered and are never reused. Copy/paste remaps
references between copied objects; references to objects outside the copy are kept.

## Transactions

```ts
const report = await engine.apply({
  label: 'Add door',
  commands: [
    { op: 'createEntity', entity: { type: 'floorplan.door', props: { host: { ref: wallId }, offset: 1000 } } },
    { op: 'addConstraint', constraint: { rule: { kind: 'fix', param: { entity: wallId, prop: 'thickness' }, value: 200 } } },
  ],
})
report.revision // new revision
report.status   // { status: 'solved' } | { status: 'underconstrained', dof } | …
```

All commands in a transaction succeed together or not at all. Commands:

| Command | Purpose |
|---|---|
| `createEntity`, `updateEntity`, `delete` (`cascade`/`reject`), `reorder` | objects |
| `setParams` (`exact` / `prefer`) | numeric parameters (`width`, `a.x`, `radius`, …) |
| `transform` (`strict` / `convert`) | move, rotate, scale |
| `addConstraint`, `updateConstraint`, `removeConstraint` | rules |
| `addLayer`, `updateLayer`, `removeLayer`, `moveLayer` | layers |
| `group`, `ungroup`, `paste` | structure |
| `split`, `trim`, `extend` | curve editing |
| `setSettings` | units, grid, time axis, title |

`exact` edits must be met exactly; `prefer` edits are met when the rules allow and
otherwise the nearest feasible values are used. When explicitly given values are
impossible, the error carries the rules involved and, for linear rules, the
nearest feasible values (`error.details.failure.nearest`).

New plugin objects: properties you pass are exact; omitted properties start from
their defaults and may adapt to the type's rules.

## Undo and redo

`engine.undo()` / `engine.redo()` restore committed before/after states without
solving again. A new change after undo clears the redo branch. History has an entry
and memory limit; a single change larger than the limit clears history and reports
`undoAvailable: false`.

## Drags

```ts
await engine.beginDrag({ kind: 'anchor', entity: id, anchor: 'end' })
await engine.dragTo([1500, 0])   // preview: scene delta marked as preview
await engine.endDrag(true)       // one history entry (false = cancel)
```

Each drag position is solved in up to three attempts: the dragged anchor exactly on
the pointer with everything else pinned; exactly on the pointer with everything free
(by stay priority); and finally the nearest feasible position. Previews never change
the document revision.

## Events

`committed`, `historyChanged`, `selectionChanged` and `pluginsChanged` are delivered
after the commit completes. Calling the engine from a listener queues a new request;
commits never nest.

## Queries

`hitTest`, `selectInRect` (window or crossing), `snap` (endpoint, midpoint, center,
quadrant, intersection, anchor, nearest, grid; screen-space radius, priority and
hysteresis), `entityInfo` (anchors, parameters, read-only reason, measured value),
`analyze` (status and diagnostics of the whole document) and `verify` (independent
check of every hard rule against the stored values).
