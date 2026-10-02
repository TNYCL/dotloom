# Plugins

A plugin has two layers:

1. **Model definitions** — serializable entity types. The engine (Rust) compiles and
   evaluates them: properties, derived values, anchors, drawing recipes, rules and
   migrations. They behave identically in the browser, in Node.js and in native
   builds. There is no `eval`, no script inside documents and no JavaScript callback
   during solving.
2. **Host code** — trusted TypeScript: tools, named commands, snap providers,
   panels, importers/exporters, storage adapters and rule templates.

```ts
import { type DotloomPlugin, PluginHost } from '@dotloom/sdk'

const shelves: DotloomPlugin = {
  id: 'acme.shelves',        // namespaced plugin ID
  version: '1.2.0',
  sdk: '^1.0.0',             // supported @dotloom/sdk range
  types: [shelfType],        // EntityTypeDef[]; type IDs use the plugin namespace (acme.*)
  tools: [() => placeShelfTool],
  commands: { addShelf: { label: 'Add shelf', run: (api, width) => ({ commands: [/* … */] }) } },
  snapProviders: [],
  panels: [],
  importers: [],
  exporters: [],
  constraintTemplates: [],
  activate(api) {
    const off = api.on('committed', () => {})  // removed automatically on disable
    return () => {}                            // cleanup (GPU resources, timers, DOM)
  },
}

const host = new PluginHost(editor.engine, editor.core)
await host.register(shelves)
```

## Lifecycle

| Step | Effect |
|---|---|
| `register` | validates the ID, the SDK range and the type namespace, registers types in the engine (conflicting type IDs are typed errors), enables the plugin. A failure leaves nothing registered. |
| `disable` | tools, commands, snap providers and listeners are removed; cleanups run. The plugin's entities stay in the document, drawn from their stored fallback and **read-only** (the inspector explains why). |
| `enable` | everything comes back; entities are editable again. |
| `unregister` | like `disable`, and the types are removed from the engine. Entities keep all their data and are saved unchanged; they are read-only until the plugin is registered again. |

Documents never contain plugin code and opening a document never downloads code.
If a document needs types that are not registered, `load()` reports them in
`missingPlugins`.

## Entity type definitions

```json
{
  "typeId": "acme.shelf",
  "version": 2,
  "label": "Shelf unit",
  "props": {
    "width": { "type": "number", "dim": "length", "default": "180cm", "label": "Inner width" },
    "w1":    { "type": "number", "dim": "length", "default": "60cm", "stay": "high" },
    "material": { "type": "enum", "values": ["oak", "birch"], "default": "oak" },
    "host": { "type": "ref", "target": "acme.wall", "onDelete": "cascade", "required": true }
  },
  "derived":  [{ "name": "x1", "expr": "w1" }],
  "anchors":  [{ "name": "divider1", "expr": "vec(x1, 0mm)", "kind": "vertex" }],
  "primitives": [
    { "kind": "polygon", "points": ["vec(0mm, 0mm)", "vec(width, 0mm)", "vec(width, 2m)", "vec(0mm, 2m)"], "style": { "fill": "#c8a27a40" } },
    { "kind": "text", "position": "vec(width / 2, 1m)", "content": "{material}", "height": "8cm", "halign": "center" }
  ],
  "constraints": [
    { "lhs": "w1 + w2 + w3", "op": "=", "rhs": "width", "label": "compartments fill the width" }
  ],
  "migrations": [{ "from": 1, "rename": { "w": "width" } }]
}
```

- **Properties**: `number` (with `dim`: `length`, `angle`, `time`, `scalar`; `min`,
  `max`; `solve: false` keeps it out of solving; `stay` priority), `point`, `bool`,
  `text`, `enum`, `ref` (`onDelete`: `cascade`, `clear`, `reject`).
- **Anchors** are points other rules, dimensions and snapping can use.
- **Primitives**: `line`, `polyline`, `polygon`, `circle`, `arc`, `text` (text
  content can interpolate properties with `{name}`).
- **Rules** (`constraints`) apply to every instance; they appear in diagnostics with
  their labels.
- **Migrations** run when a document contains an older type version (`rename`,
  `set`, `remove`). A document made with a *newer* type version opens read-only.

## Expression language

```text
expr    := term (('+' | '-') term)*
term    := unary (('*' | '/') unary)*
unary   := '-' unary | postfix
postfix := primary ('.' ident)*
primary := number[unit] | ident | ident '(' args ')' | '(' expr ')'
```

- Literal units: `mm cm m in ft deg rad ms s min h d`. Every expression is
  dimension-checked when the type is registered (adding a length to an angle is an
  error).
- Values are scalars or 2D vectors. Names: properties, derived values, `x`/`y` of
  points, referenced entities' properties and anchors (`host.start`,
  `host.end`), the time axis (`axis.origin`, `axis.mmPerSecond`).
- Functions: `min max clamp abs sqrt sin cos atan2 hypot vec x y len norm perp dot
  cross dist angle rotate lerp`.

Unsupported functions or equation classes are rejected with typed errors when the
type is registered. A new numeric primitive or solver backend needs a Rust extension
and a rebuild — the model language is intentionally closed.

## External plugins

The repository contains `examples/external-plugin`, a project **outside** the
workspace that installs the packed `@dotloom/sdk` and `@dotloom/react` tarballs and
defines its own entity type and tool using only public package entry points. CI
builds and tests it on every change; it is the reference for third-party plugins.
