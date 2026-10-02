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
- npm: `@dotloomjs/sdk` and `@dotloomjs/react` under the owner's npm organization
  `dotloomjs`. The names are recorded in `packages/names.json`; `pnpm run check`
  verifies that the package manifests match it and that no source, test, workflow or
  document still uses another scope for them.
- Neither name is claimed as globally unique or legally cleared.

## Amendment (2026-10-02, 1.1.0)

The owner chose the npm organization `dotloomjs` instead of `dotloom`. 1.0.0 was
published only as a GitHub Release (tarballs named `@dotloomjs/*`, never on npm); from
1.1.0 the packages are `@dotloomjs/sdk` and `@dotloomjs/react`. The original text above
claimed that only `names.json` would change; in practice every import site used the
literal names, so the move touched them all and the check described above was added
to keep them consistent.
