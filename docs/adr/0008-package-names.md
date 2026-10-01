# ADR-0008: Package names and namespaces

Status: accepted (2026-10-02)

## Context

On 2026-10-02 the names `dotloom` and `dotloom-*` returned 404 on crates.io and npm,
and the npm scope `@dotloom` did not exist. A 404 is not a reservation. No npm or
crates.io credentials were present on the development machine.

## Decision

- Crates: `dotloom` (facade), `dotloom-geometry`, `dotloom-document`,
  `dotloom-constraints`, `dotloom-scene`, `dotloom-engine`, `dotloom-io`,
  `dotloom-render`, `dotloom-cli`. The WASM binding crates are not published.
- npm: `@dotloom/sdk` and `@dotloom/react` under an npm organization `dotloom` that
  the owner must create. All import specifiers are defined in one place
  (`packages/names.json`); switching to a fallback scope changes only that file and
  the package manifests generated from it.
- Neither name is claimed as globally unique or legally cleared.
