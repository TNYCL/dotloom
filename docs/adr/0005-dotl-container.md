# ADR-0005: `.dotl` container format

Status: accepted (2026-10-02)

## Decision

A `.dotl` file is a ZIP archive (stored or deflate entries only):

```text
manifest.json            format, formatVersion, producer, schemaVersion, plugins[], assets[]
document.json            canonical document (schema-versioned JSON)
view.json                optional view state (camera, panels) — never geometry
assets/<sha256>.<ext>    binary assets referenced from document.json
```

- `manifest.format` = `"dotloom"`, `formatVersion` = container version (1).
- `schemaVersion` = document schema (integer major). Older majors are migrated by
  explicit, tested migration steps; an unknown future major is rejected with a clear
  error; unknown fields are preserved.
- Plugin requirements list `typeId` + version. Missing plugins never block opening:
  their entities keep opaque payloads and the stored fallback representation.
- Not stored: renderer caches, WASM runtime state, solver iterations, undo history
  (history is session-only).
- Reader limits (callers may lower them): 256 MiB total uncompressed, 64 MiB per
  entry, 10 000 entries, compression ratio ≤ 200, no absolute or `..` paths, no
  duplicate names, UTF-8 names only, JSON nesting ≤ 128, ≤ 1 000 000 entities.
- Native save writes a temp file in the target directory, flushes it, then atomically
  replaces the target; on failure the old file is untouched.
- The file never contains executable code and opening it never fetches anything.
