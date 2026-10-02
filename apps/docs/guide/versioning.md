# Versions and migration

## What is versioned

| Version | Where | Policy |
|---|---|---|
| Packages and crates | npm / crates.io | semver; `0.x` minor releases may break APIs, documented in the changelog |
| Worker protocol | `PROTOCOL_VERSION` | checked when an engine starts; SDK and engine WebAssembly ship together in `@dotloom/sdk` |
| Renderer binding | `RENDER_PROTOCOL` | checked when the renderer module loads |
| `.dotl` container | `manifest.formatVersion` | readers reject unknown majors |
| Document schema | `document.json` `schema` | older schemas are migrated step by step; newer majors are rejected with a clear error |
| Scene contract | `SCENE_FORMAT_VERSION` | binary deltas carry it; custom renderers check it |
| Plugin types | `typeVersion` per entity | type migrations upgrade old instances; newer instances open read-only |

## Upgrading

1. Read the changelog for the new version.
2. Update `@dotloom/sdk` and `@dotloom/react` together (they are released together).
3. Files written by older versions open and are migrated in memory; saving writes
   the current schema. Keep a copy if older applications must still read them.
4. Plugins declare the SDK range they support (`sdk: '^0.1.0'`); an incompatible
   range is reported when the plugin is registered.

## Deprecations

APIs are marked `@deprecated` in the type declarations and rustdoc at least one
minor release before removal.
