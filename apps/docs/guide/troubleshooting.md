# Troubleshooting

## “no renderer backend available”

`viewport.attempts` lists each backend and why it failed.

- **WebGPU: navigator.gpu missing** — the browser or platform does not expose
  WebGPU; WebGL2 is tried next.
- **WebGL2 is not available** — hardware acceleration may be disabled in the browser
  settings, or the GPU is block-listed. Check `chrome://gpu` / `about:support`.
- In headless test environments, use a software adapter (SwiftShader in Chromium,
  Mesa in Firefox under Xvfb).

## The canvas stays empty

- The container must have a size (`height: 100%` needs sized parents).
- With Vite, exclude `@dotloomjs/sdk` (and `@dotloomjs/react`) from `optimizeDeps`;
  pre-bundling breaks the `new URL(…, import.meta.url)` asset paths.
- When serving under a sub-path, build with the right `base` or pass explicit
  `workerUrl`, `engineWasmUrl` and `renderWasmUrl`.

## A change is rejected

The error (`code: 'solve'`) lists the rules that would break (`labels`), the
parameters you edited and, for linear rules, the nearest feasible values. Unlock or
disable a rule, or use the suggested value. Nothing is changed by a rejected edit.

## “stale”

A request was made for a revision that is no longer current (another change
committed first). Read the new state and try again.

## The engine crashed

`engine.on('crash', …)` fires and every call rejects with `crashed`. Create a new
engine and reopen the document — the React editor's autosave offers recovery.

## Missing plugins

`load()` reports `missingPlugins`. Those objects are shown read-only from their
stored representation and saved unchanged. Register the plugin and reopen the file
to edit them.

## Slow solving

Very large connected rule systems take longer; the status bar shows a **Cancel**
button (`AbortSignal` in the SDK). Split independent parts — the solver works per
connected component.
