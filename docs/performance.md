# Performance (DL-PERF)

Measured on the reference device below with the commands at the end of this page.
Hosted CI runners have no GPU and are not used for performance gates (they check
correctness only). The benchmark runs write raw results to `bench/` (not versioned);
the results behind this page are kept in [`perf/2026-10-02/`](perf/2026-10-02/).

## Reference device and method (DL-PERF-1)

| | |
|---|---|
| CPU | AMD Ryzen 9 5900X, 12 cores / 24 threads |
| GPU | NVIDIA GeForce GTX 1060 6 GB, driver 32.0.15.8266 (WebGPU via Dawn/D3D12, WebGL2 via ANGLE/D3D11) |
| RAM | 24 GB |
| OS | Windows 11 Home 10.0.26200 |
| Display | 1920 × 1080, 120 Hz (`CurrentRefreshRate` 119), scaling 100 % (`devicePixelRatio` 1) |
| Browsers | Google Chrome 154.0.8037.58, Microsoft Edge 154.0.4258.48 (installed browsers driven by Playwright `channel`), Firefox 155.0 (Playwright's Firefox build; no release Firefox is installed) |
| Browser mode | headed windows (real compositor and refresh; Firefox exposes WebGPU only with a window) |
| Build | WASM: `wasm-release` profile (`opt-level = "s"`, fat LTO, one codegen unit, `panic = "abort"`), wasm-bindgen 0.2.129, rustc 1.98.1; native: `release` profile (thin LTO); Node 24.19.0 |
| Code | branch `feat/perf` on top of `9c09010` |

Rules applied to every measurement:

- A warm-up phase is excluded (60 frames for rendering, setup transactions for the
  solver corpus); fixtures are generated deterministically (seeded).
- Each renderer backend is selected explicitly (no fallback), so a WebGPU row is a
  WebGPU measurement.
- `performance.now()` is coarsened by browsers without cross-origin isolation (0.1 ms
  in Chromium, 1 ms in Firefox). For sub-millisecond queries the mean over all
  samples is reported next to percentiles.

## Results

| ID | Workload | Target | Result (worst browser/backend) | |
|---|---|---|---|---|
| DL-PERF-2 | 10 000 visible shapes, 1080p pan/zoom | p95 frame ≤ 16.7 ms | presented-frame p95 8.4 ms, 0 dropped of 600 at 120 Hz (all six); input → GPU done p95 9.4 ms (Chromium WebGL2) | met |
| DL-PERF-3 | hit-test and snap on the same scene | p95 ≤ 8 ms | worker round trip p95 ≤ 1 ms (timer resolution), mean ≤ 0.09 ms, max 2 ms | met |
| DL-PERF-4 | 200-variable geometric editing corpus | p95 ≤ 50 ms, hard constraints satisfied | 23.2 ms through the SDK on WASM (Node); 0 violations, 0 rejected edits | met |
| DL-PERF-5 | open a 10 000-object `.dotl` | ≤ 2 s (excl. WASM init) | median 101–187 ms, max 188 ms | met |
| DL-PERF-6 | 100 000-object stress | open and navigate without crash; real cancel | open 1.1–1.3 s, 120 navigation frames without error; a running 16 000-variable solve cancelled in 86–435 ms, document unchanged | met |
| DL-PERF-7 | repeated open/close | no sustained leak after warm-up | JS heap growth −6.5 %…+0.7 % (cycles 16–20 vs 6–10, Chromium); renderer WASM memory unchanged over 20 cycles | met (heap not measurable in Firefox) |
| DL-PERF-8 | WASM size, cold start, glyph cache, peak file memory | report | see below | reported |

### DL-PERF-2: rendering 10 000 shapes at 1080p

Fixture (`tests/e2e/harness/bench.ts`): 10 000 shapes on a 100 × 100 grid of 100 mm
cells — 40 % lines, 20 % rectangles, 20 % circles, 10 % arcs, 10 % five-point
polylines, every fifth shape filled — with the grid overlay on. The camera path has
600 frames: a circular pan at the fitted view (all 10 000 shapes visible: 30–40
chunks, 27 000–35 000 line segments and 1 500–2 000 fill triangles drawn), a zoom to
10× and back (5–40 chunks visible), and a zoom out to 0.25× and back.

Two measurements on the same path:

- **Presented frames**: the path driven by `requestAnimationFrame`, one camera change
  and render per frame; intervals between frames. This is what a user sees.
- **Input → GPU done**: per frame, the camera change, the render call and a wait
  until the GPU reports the submitted work complete (`Viewport.gpuIdle()`: WebGPU
  `onSubmittedWorkDone`, WebGL2 fence polling between tasks — an upper bound).

| Browser | Backend | presented p50 / p95 | dropped (> 25 ms) | input → GPU done p95 | render call CPU p95 |
|---|---|---|---|---|---|
| Chrome 154 | WebGPU | 8.3 / 8.4 ms | 0 / 600 | 4.3 ms | 0.3 ms |
| Chrome 154 | WebGL2 | 8.3 / 8.4 ms | 0 / 600 | 9.4 ms | 0.5 ms |
| Edge 154 | WebGPU | 8.3 / 8.4 ms | 0 / 600 | 4.6 ms | 0.3 ms |
| Edge 154 | WebGL2 | 8.3 / 8.4 ms | 0 / 600 | 9.4 ms | 0.5 ms |
| Firefox 155 | WebGPU | 8.34 / 8.34 ms | 0 / 600 | not measurable¹ | 1 ms |
| Firefox 155 | WebGL2 | 8.34 / 8.34 ms | 0 / 600 | not measurable¹ | 2 ms |

¹ Firefox reports WebGL fence and WebGPU work-done completion late (≈ 40 ms and
≈ 100 ms after submission even with 100 shapes, while it presents every 120 Hz
frame), so this method does not measure frame time there; presented frames do.

The synchronous loop shows isolated outliers (max 54–174 ms) on Chromium that do not
coincide with tessellation work (`chunksRebuilt = 0`) and do not appear in the
presented-frame runs; they are scheduling stalls of the polling loop. A plain WebGL2
animation without Dotloom (`tests/e2e/bench/plain-webgl.bench.ts`) is the
environment baseline: 0 dropped frames in every browser.

### DL-PERF-3: hit-test and snap

500 seeded points, half at the fitted view and half zoomed in 5×, 6 px query radius;
each `engine.hitTest` and `engine.snap` is a full worker round trip.

| Browser / backend | hit-test mean / p95 / max | snap mean / p95 / max |
|---|---|---|
| Chrome WebGPU | 0.050 / 0.1 / 0.8 ms | 0.054 / 0.1 / 0.8 ms |
| Chrome WebGL2 | 0.052 / 0.1 / 0.9 ms | 0.052 / 0.1 / 0.6 ms |
| Edge WebGPU | 0.048 / 0.1 / 1.0 ms | 0.063 / 0.1 / 0.8 ms |
| Edge WebGL2 | 0.053 / 0.1 / 0.9 ms | 0.055 / 0.1 / 0.7 ms |
| Firefox WebGPU | 0.084 / 1 / 1 ms | 0.080 / 1 / 1 ms |
| Firefox WebGL2 | 0.068 / 1 / 2 ms | 0.092 / 1 / 1 ms |

### DL-PERF-4: solver corpus

Corpus: `scripts/bench/solver-corpus.mjs` (seeded), about 200 solver variables per
document, every document consistent by construction and every edit feasible. Each
edit is one committed transaction (rank analysis on, as for user commits); hard
constraints are re-checked independently after every edit (`engine.verify()`).

| Case | Variables / rules | Edits | WASM p50 / p95 | native p95 |
|---|---|---|---|---|
| `linkage-drag`: bent 50-link chain, end dragged along an arc (≈ 50 mm per frame) | 200 / 100 | 25 | 14.4 / 16.7 ms | 9.7 ms |
| `linkage-typed`: same chain, typed end positions up to 400 mm away | 200 / 100 | 25 | 18.2 / 53.3 ms | 45.2 ms |
| `polygon-40`: closed equal-sided 40-gon, vertex edits | 160 / 81 | 25 | 8.3 / 14.0 ms | 13.9 ms |
| `rectangles-12`: 12 rectangles, equal widths, spacings | 192 / 117 | 25 | 7.6 / 7.9 ms | 5.3 ms |
| `circles-40`: chain of 40 tangent equal circles + 20 tangent lines, radius edits | 200 / 139 | 25 | 11.0 / 28.1 ms | 23.6 ms |
| `mixed-sketch`: lines, circles, parallel/length/radius rules | 176 / 82 | 25 | 0.3 / 4.0 ms | 3.2 ms |
| `shelves-50`: 50 plugin shelves, equal widths (linear backend) | 200 / 50 | 25 | 8.0 / 8.4 ms | 5.3 ms |
| **gated corpus (175 edits)** | | | **8.3 / 23.2 ms** | 16.3 ms |
| `linkage-straight-jump` (stress, not gated) | 200 / 100 | 25 | 29.1 / 53.9 ms | 31.3 ms |

WASM numbers are measured through the SDK (`scripts/bench/solver.mjs`, Node 24,
in-thread transport: protocol and JSON included); native numbers with
`crates/engine/examples/solver_corpus.rs`. The stress case types the end of a nearly
straight chain 500–2500 mm inwards: shortening a straight chain is singular to first
order (it must buckle), which every local method handles with many small steps; it is
reported but excluded from the gate because it is not an interactive edit.

The numeric backend reached this through measured changes (ADR-0004): sparse linear
algebra (the dense predecessor spent 75 % of a solve in `A·Aᵀ` and its Cholesky),
constraint curvature in the step model (a 40-gon and a tangent-circle chain no longer
exhaust the 100-iteration budget), an approach phase for violated typed values and
hard drag targets, and value-only line-search evaluations. The first version of the
corpus (dense algebra, no curvature, the straight-chain case still gated) measured
p95 0.97 s natively and 1.8 s on WASM.

### DL-PERF-5 and DL-PERF-6: opening files, 100 000-object stress

`.dotl` of the 10 000-shape fixture: 80 317 bytes. Opening = `engine.load(bytes)` in a
fresh editor whose WASM modules are already initialized, `fit`, first frame finished
on the GPU.

| Browser / backend | 10k open median / max | load / first frame (median) | 100k open | 100k navigation p95² | cancel latency |
|---|---|---|---|---|---|
| Chrome WebGPU | 105 / 126 ms | 77 / 29 ms | 1.29 s | 37.6 ms | 86 ms |
| Chrome WebGL2 | 101 / 126 ms | 75 / 27 ms | 1.34 s | 29.6 ms | 96 ms |
| Edge WebGPU | 105 / 128 ms | 77 / 28 ms | 1.28 s | 33.2 ms | 97 ms |
| Edge WebGL2 | 104 / 127 ms | 79 / 25 ms | 1.31 s | 30.6 ms | 102 ms |
| Firefox WebGPU | 187 / 188 ms | 100 / 85 ms | 1.33 s | 132 ms¹ | 435 ms |
| Firefox WebGL2 | 112 / 121 ms | 96 / 16 ms | 1.09 s | 48 ms¹ | 144 ms |

² Input → GPU done over 120 pan/zoom frames of the 100 000-shape scene (no target:
the brief asks for navigation without a crash, not a frame rate). ¹ Includes the late
completion report described under DL-PERF-2.

Cancel: a transaction creating a 4000-link chain with its end typed far away (16 000
variables) is sent with an `AbortSignal` that fires after 100 ms. In every run the
request rejected with `cancelled`, the document revision was unchanged, no renderer
loss or engine crash was reported, and a hit-test right after answered in ≤ 0.5 ms.

### Single edits in a large document (DL-DOC-9)

50 exact edits of one line each in the loaded 10 000-shape document
(`files.bench.ts`, `edit10k`):

| Browser / backend | commit p50 / p95 | scene delta | renderer work per edit |
|---|---|---|---|
| Chrome 154 WebGPU | 0.5 / 0.8 ms | ≤ 120 bytes (full scene 1 093 028 bytes) | 1 item tessellated, 1 chunk rebuilt |
| Firefox 155 WebGL2 | 1 / 2 ms (1 ms timer) | ≤ 120 bytes | 1 item tessellated, 1 chunk rebuilt |

The engine commits through a copy-on-write overlay and re-emits only changed
entities (`crates/engine/tests/engine.rs`,
`one_edit_in_a_large_document_touches_one_scene_item`); the renderer re-tessellates
only the changed item and rebuilds only its chunk (`crates/render/src/cache.rs`
tests). Raw: `perf/2026-10-02/edit10k.json`.

### DL-PERF-7: repeated open/close

20 cycles of: create an editor (engine worker + renderer), open the 10 000-shape
`.dotl`, draw, dispose. Chrome and Edge run with `--enable-precise-memory-info` and
`--js-flags=--expose-gc`; the heap is read after `gc()`. Cycles 1–5 are warm-up.

| Browser / backend | JS heap, cycles 16–20 vs 6–10 | renderer WASM memory over 20 cycles |
|---|---|---|
| Chrome WebGPU | −6.5 % | 32.4 MiB constant |
| Chrome WebGL2 | +0.5 % | 34.5 MiB constant |
| Edge WebGPU | +0.7 % | 32.4 MiB constant |
| Edge WebGL2 | +0.4 % | 34.5 MiB constant |
| Firefox (both) | not measurable (no heap API) | constant |

Engine workers are terminated on dispose; their memory is released by the browser
with the worker (not observable from the page).

### DL-PERF-8: size, start-up and memory

| | Engine | Renderer |
|---|---|---|
| `.wasm` raw | 2154 KiB | 2813 KiB |
| gzip −9 | 656 KiB | 1014 KiB |
| brotli q11 | 469 KiB | 770 KiB |

`node scripts/bench/wasm-size.mjs`. The renderer includes the shader translator for
WebGL2 (naga), the tessellator and the embedded font subset.

- Cold start (page script → editor ready: worker spawn, both modules fetched,
  compiled and instantiated, GPU adapter and device): 305–358 ms in Chrome/Edge,
  239–423 ms in Firefox. A second editor on the same page: 41–104 ms (Chromium),
  103–297 ms (Firefox).
- Glyph cache: one 2048 × 2048 single-channel SDF atlas, 4 MiB on the GPU plus a
  4 MiB CPU copy in the renderer's WASM memory.
- Peak memory (WebAssembly linear memory never shrinks, so its size after an
  operation is the operation's high-water mark): opening the 10 000-object file —
  engine 21.8 MiB, renderer 32.4–34.8 MiB, GPU buffers 2.6 MiB; opening the 100 000
  object file — engine 199.6 MiB, renderer 102.4–119.7 MiB, GPU 26 MiB.
- Native heap profile of opening 100 000 objects
  (`cargo run --release -p dotloom-wasm --example memory_profile`): document 661
  B/entity, engine state after load 2023 B/entity (document, evaluation cache with
  world-space anchors and drawables, spatial and dependency indexes), parse peak
  155 MiB, overall peak 242 MiB (full scene encoding for the renderer).

Memory work done while measuring: documents of the current schema are deserialized
directly instead of through a JSON tree (parse peak 214 → 155 MiB at 100 000
objects), the WASM binding moves the opened document into the engine instead of
keeping a second copy (engine peak in the browser 265 → 200 MiB), and built-in anchor
names are static strings.

### Incremental linear drags (DL-SOLVE-4)

During a drag, a problem whose components are all linear is re-solved by one
`LinearSession` (Cassowary edit variables on the drag target) instead of from
scratch. A chain of boxes (x₀ fixed, xᵢ₊₁ = xᵢ + wᵢ, 50 ≤ wᵢ ≤ 400, a weak width
preference) with its end dragged through 200 pointer positions, native release
build on the reference device
(`cargo run --release -p dotloom-constraints --example drag_session -- 50 100 200`):

| Boxes | Variables | Full solve p50 / p95 | Incremental p50 / p95 | Speed-up (p50) | Max difference |
|---|---|---|---|---|---|
| 50 | 101 | 1.71 / 2.17 ms | 0.11 / 0.19 ms | 15× | 1.1e-11 mm |
| 100 | 201 | 6.99 / 9.60 ms | 0.20 / 0.49 ms | 36× | 3.3e-11 mm |
| 200 | 401 | 37.7 / 50.7 ms | 0.56 / 1.71 ms | 67× | 1.2e-10 mm |

Every position was also solved from scratch; the session never fell back and its
values match the full solve to the last digits shown. Correctness is tested in
`crates/constraints/tests/session.rs` (property test against full solves, hard and
soft targets, infeasible positions, structural changes) and
`crates/engine/tests/engine.rs::linear_drags_are_solved_incrementally_and_match_fresh_drags`.

## Not measured

- Safari: no macOS device is available locally, so there are no Safari performance
  numbers. (Safari correctness checks on a hosted macOS runner are tracked in
  `docs/status.md`; hosted runners give no performance numbers either.)
- Playwright's WebKit on Windows has no GPU acceleration comparable to Safari and is
  not used for performance.
- Release Firefox is not installed; the Firefox rows use Playwright's Firefox 155
  build.
- Firefox exposes no JS heap size, so the leak check there covers renderer WASM
  memory only.

## Reproduce

```text
pnpm run build:wasm && pnpm run build
node scripts/bench/solver-corpus.mjs --out bench/solver-corpus.json
node scripts/bench/solver.mjs --json bench/solver-wasm.json
cargo run --release -p dotloom-engine --example solver_corpus -- bench/solver-corpus.json
node scripts/bench/wasm-size.mjs --json bench/wasm-size.json
cargo run --release -p dotloom-wasm --example memory_profile -- 100000
cargo run --release -p dotloom-constraints --example drag_session -- 50 100 200
pnpm --filter @dotloom/e2e run bench          # rendering, queries, files, stress, leak
```

`DOTLOOM_BENCH_PROJECTS=chrome-webgpu` limits the browser/backend projects;
`DOTLOOM_BENCH_HEADLESS=1` runs without windows.
