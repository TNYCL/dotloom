# @dotloom/sdk

Framework-agnostic TypeScript SDK for [Dotloom](https://github.com/TNYCL/dotloom),
the open-source framework for constraint-driven 2D editors.

The Rust engine (compiled to WebAssembly) owns the document, the constraint solver
and the `.dotl` file format. In browsers it runs in a dedicated Web Worker; this
package talks to it through a small versioned message protocol.

```ts
import { DotloomEngine } from '@dotloom/sdk'

const engine = await DotloomEngine.create()
const report = await engine.apply([
  { op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [100, 0] } } },
])
const file = await engine.save() // Uint8Array (.dotl)
```

In Node.js (tests, tools, servers) the engine runs in the calling thread:

```ts
import { createNodeEngine } from '@dotloom/sdk/node'

const engine = await createNodeEngine()
```

* Units are millimetres, radians and seconds.
* Every edit is an atomic transaction; failed solves reject with a
  `DotloomError` (`code: 'solve'`) carrying diagnostics and nearest feasible values.
* Long solves can be cancelled with an `AbortSignal`; drag updates are coalesced.
* Importing the package has no side effects (SSR-safe).

Documentation: <https://tnycl.github.io/dotloom/>

Licensed under either of MIT or Apache-2.0 at your option. The WebAssembly modules
contain third-party crates and the Inter font (OFL-1.1); see `THIRD-PARTY-NOTICES.md`.
