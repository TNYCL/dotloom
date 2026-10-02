# Performance

Numbers measured on the project's reference device: AMD Ryzen 9 5900X, NVIDIA GeForce
GTX 1060 6 GB, Windows 11, a 1920 × 1080 display at 120 Hz, Chrome 154, Edge 154 and
Firefox 155. The full method, every browser/backend row and the raw result files are
in [`docs/performance.md`](https://github.com/TNYCL/dotloom/blob/main/docs/performance.md).

| Workload | Target | Measured |
|---|---|---|
| Pan and zoom over 10 000 visible shapes at 1080p | p95 frame ≤ 16.7 ms | every frame presented at 120 Hz (p95 8.4 ms, 0 dropped) on WebGPU and WebGL2 in Chrome, Edge and Firefox |
| Hit-test and snap on the same scene | p95 ≤ 8 ms | ≤ 1 ms round trip to the engine worker (mean 0.05–0.09 ms) |
| Editing a 200-variable sketch (linkages, polygons, tangent circles, plugins) | p95 ≤ 50 ms, hard rules hold | p95 23 ms, every hard rule satisfied |
| Opening a 10 000-object `.dotl` | ≤ 2 s | 0.1–0.19 s |
| 100 000 objects | opens, navigates, a long solve can be cancelled | opens in 1.1–1.3 s; cancellation takes effect within 0.1–0.4 s and leaves the document unchanged |
| Repeated open/close | no leak after warm-up | heap and renderer memory flat over 20 cycles |

Download size (brotli): engine 469 KiB, renderer 770 KiB. An editor starts in about
0.3 s on a cold page (both WebAssembly modules, the worker and the GPU device).

## What keeps it fast

- **Worker engine.** Solving, hit-testing and snapping run in a Web Worker; the main
  thread only renders and handles input.
- **Incremental, chunked rendering.** The renderer keeps GPU meshes per chunk of at
  most 256 items, culls chunks outside the view and re-tessellates only changed items
  or chunks whose level of detail changed. Frames are drawn on demand.
- **Spatial index.** Pointer queries never scan the whole document.
- **Sparse solver.** The numeric solver uses sparse factorizations ordered for the
  chain-like structure of sketches and includes constraint curvature, so it needs few
  iterations even when a linkage moves far.

## Measuring your own app

`Viewport.gpuIdle()` resolves when the GPU has finished the frames submitted so far
(useful for input → frame measurements), `Viewport.memoryStats()` reports renderer
memory, `engine.memory()` the engine's WebAssembly memory, and every commit carries
`CommitReport.solver` (iterations, attempts, variables, rules). Firefox reports GPU
completion late; measure presented frames with `requestAnimationFrame` there.

Memory grows with the document: about 2 KB of engine memory per simple object
(document, cached world-space geometry and indexes) plus the renderer's meshes.
