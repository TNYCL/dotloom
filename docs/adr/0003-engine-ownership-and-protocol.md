# ADR-0003: Engine ownership, Worker hosting and protocol

Status: accepted (2026-10-02)

## Decision

- The Rust engine is the single editable source of truth. It runs as WASM inside a
  dedicated module Web Worker. The main thread runs UI and the renderer WASM.
- Hosts talk to the engine through the TypeScript SDK. The SDK sends versioned
  messages (`protocol: 1`) with `requestId`, `documentId`, `expectedRevision`.
  Responses carry `revision`, a typed `error.code` on failure and capabilities on init.
- Scene updates travel as a binary `SceneDelta` (`dotloom-scene` encoding) in a
  transferred `ArrayBuffer`; the renderer decodes it in WASM. Each delta carries the
  revision it was produced for; previews are a separate transient layer.
- Long operations (solves) run as budgeted `step` calls; the worker yields to the
  event loop between steps (`MessageChannel` macrotask) so `cancel` messages and newer
  drag updates are processed. A job bound to an older revision cannot commit.
- Drag updates are coalesced (latest wins) while a step is in flight.
- No SharedArrayBuffer, no COOP/COEP headers and no WASM threads are required.
- An in-thread transport (same API, no Worker) exists for Node tests and tooling; the
  browser default is the Worker transport.

Versions are independent: protocol version (messages), package version (npm/crate
SemVer), document schema version (`.dotl`).
