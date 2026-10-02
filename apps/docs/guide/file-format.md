# `.dotl` file format

Format version **1**, document schema **1**. File names typically look like
`ev-plani.dotl` or `raf-tasarimi.dotl`.

## Container

A `.dotl` file is a ZIP archive (stored or deflate entries):

```text
manifest.json            format, formatVersion, producer, schemaVersion, plugins[], assets[]
document.json            the document (schema-versioned JSON)
view.json                optional view state (camera, panels) — never geometry
assets/<sha256>.<ext>    binary assets referenced from the document as "asset:<sha256>.<ext>"
```

`manifest.json`:

```json
{
  "format": "dotloom",
  "formatVersion": 1,
  "schemaVersion": 1,
  "producer": { "name": "dotloom", "version": "0.1.0" },
  "plugins": [{ "typeId": "floorplan.wall", "version": 2 }],
  "assets": [{ "path": "assets/9f2c….png", "mediaType": "image/png", "size": 1234, "sha256": "9f2c…" }]
}
```

Writing is deterministic (same document → same bytes). Unknown ZIP entries and
unknown JSON fields are preserved when a file is loaded and saved again.

## `document.json`

```json
{
  "schema": 1,
  "meta": { "title": "ev-plani" },
  "settings": { "displayUnit": "metre", "grid": { "spacing": 100, "majorEvery": 10 } },
  "layers": [{ "id": 1, "name": "Default" }],
  "entities": [
    { "id": 2, "type": "dotloom.line", "layer": 1, "geometry": { "type": "line", "a": [0, 0], "b": [4000, 0] } },
    { "id": 3, "type": "floorplan.door", "typeVersion": 1, "layer": 1, "props": { "host": { "ref": 2 }, "offset": 500, "width": 900 } }
  ],
  "groups": [],
  "constraints": [{ "id": 4, "rule": { "kind": "fix", "param": { "entity": 3, "prop": "width" }, "value": 900 }, "label": "door width" }],
  "nextId": 5
}
```

Coordinates are millimetres with Y up; angles radians; times seconds. Points are
`[x, y]` arrays.

## What is not stored

Renderer caches, WebAssembly state, solver iterations and the undo history are
never written. Opening a file never runs code and never fetches anything.

## Validation and limits

Readers verify, before anything is shown:

- ZIP: no absolute or `..` paths, no duplicate names, UTF-8 names, at most 10 000
  entries, 64 MiB per entry, 256 MiB in total, compression ratio ≤ 1024;
- JSON: nesting ≤ 128; up to 1 000 000 entities and constraints, 10 000 layers,
  100 000 groups, 1 000 properties per entity, 100 000 characters per string;
- document invariants: unique IDs, valid references and anchors, no group cycles,
  finite numbers, asset hashes matching their content.

## Migration

Older document schemas are migrated step by step with tests; a file from a newer
major schema is rejected with a clear error (`FutureSchema`). Plugin types migrate
their own instances (`migrations` in the type definition).

## Missing plugins

Entities whose type is not registered keep their payload and are drawn from the
stored `fallback` representation; they are read-only until the plugin is available.
Their data is written back unchanged.

## Clipboard and references

- Copied entities get new IDs; references between copied entities (properties,
  constraints, groups) are remapped to the new IDs.
- Constraints and groups are copied only when *all* their entities are copied.
- A property reference to an entity outside the copy is kept when that entity
  exists in the destination document (a pasted door keeps its host wall) and
  dropped otherwise; dropped references are reported.
- The paste offset is applied to entity transforms and to `fixPoint` targets.
