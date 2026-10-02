# Platforms and browsers

## Renderer backends

The renderer uses wgpu with one set of shaders on both backends:

- **WebGPU** is tried first (`navigator.gpu`).
- **WebGL2** is used when WebGPU is not available, or when you ask for it:
  `Viewport.create(el, engine, { backends: ['webgl2'] })`.

Every attempt and its error are reported in `viewport.attempts`; there is no silent
Canvas2D fallback. If no backend works, the React editor shows an explanation and
keeps the panels (properties, objects, files) working without a canvas.

## Tested configurations

Browser tests (Playwright) run every renderer test once per backend. A backend a
browser does not offer is reported as **skipped with the reason**, never as passed.
Tests also probe the environment with plain WebGL/WebGPU code (no Dotloom); when a
problem reproduces there, the affected test is skipped with that evidence.

The product targets the current and previous major versions of desktop Chrome,
Edge, Firefox and Safari. This table lists what has actually been verified, with
which build and how; older majors are not verified yet.

| Browser (build) | WebGPU | WebGL2 | Evidence |
|---|---|---|---|
| Chrome 154, Windows 11, GeForce GTX 1060 | verified | verified | browser suite (Playwright Chromium) locally; reference benchmarks with the installed Chrome ([performance](./performance.md)) |
| Edge 154, Windows 11, GeForce GTX 1060 | verified | verified | reference benchmarks with the installed Edge (rendering, files, leak and stress runs) |
| Chromium (Playwright), Linux CI, SwiftShader | not available: a plain WebGPU device is destroyed right after creation; the viewport falls back to WebGL2 | verified | `ci` browser suite |
| Firefox 155 (Playwright build), Windows | verified in a window (benchmarks); headless builds return no WebGPU canvas context | verified | local suite and benchmarks |
| Firefox 155 (Playwright build), Linux CI | not exposed | verified (software WebGL, window under Xvfb) | `ci` browser suite |
| WebKit (Playwright build), Linux and macOS CI | not available | verified | `ci` and `compat` browser suites |
| WebKit (Playwright build), Windows | not available | verified, except resized canvases, which this build does not display (reproduced with plain WebGL) | local suite |
| **Safari 26.6.1**, macOS 15 (CI runner) | not exposed on the runner (`navigator.gpu` undefined) | verified: drawing, PNG export and the React playground through `safaridriver` | `compat` Safari smoke test |

Native rendering (PNG export, GPU pixel tests) is verified on Vulkan (NVIDIA locally,
Mesa lavapipe on Linux CI), Direct3D 12 (WARP on Windows CI) and Metal (macOS CI).
Visual baselines belong to lavapipe; see `CONTRIBUTING.md`.

## Engine and SDK

| Environment | Status |
|---|---|
| Rust (headless engine, CLI) | Windows, Linux, macOS (CI); minimum Rust 1.89 (checked in CI) |
| Node.js 24 | `@dotloom/sdk/node` (in-thread engine), tested in CI; other versions not verified |
| Browsers | module Web Workers, WebAssembly, ES2022 |
| SSR / build tools | importing the packages has no side effects; create engines only in the browser |

Results are bit-identical natively (Windows, Linux, macOS) and in WebAssembly: the
engine uses its own elementary functions instead of the platform math library, and
parity tests compare digests of commits, documents, scenes and exports.

## Device and context loss

WebGPU device loss and WebGL context loss are detected (also when nothing is being
drawn), the renderer is recreated on a fresh canvas and the full scene is requested
again from the engine. A backend whose device is lost within 5 s of creation (some
software adapters) is tried last, so the viewport moves on to the next backend
instead of retrying it. Repeated losses (three within 30 s) stop rendering with an
error event instead of looping. `viewport.whenStable()` resolves once rendering has
run on one backend without a loss for a moment (useful before screenshots).
