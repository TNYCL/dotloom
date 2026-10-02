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

The product targets the **current and previous major versions** of desktop Chrome,
Edge, Firefox and Safari. On 2026-10-02 these are Chrome 154/153, Edge 154/153,
Firefox 157/156 and Safari 27/26. Two kinds of evidence are kept apart:

- **Released browsers** (the builds users run), driven through their own WebDriver
  servers by `tests/e2e/webdriver/smoke.mjs`: drawing with each backend selected
  explicitly (lines, a circle, Turkish text), PNG export and the React playground.
  Chrome, Edge and Firefox are downloaded from the vendors by
  `tests/e2e/webdriver/install.mjs`; the six Linux jobs and the Safari job run in
  `compat.yml` on every pull request and on `main`.
- **Engine families** (Playwright's Chromium, Firefox and WebKit builds) run the full
  browser suite: tools, files, recovery, accessibility, device loss and pixel tests.

A backend a browser does not offer is reported as **not available with the reason**,
never as passed.

### Released browsers

| Browser | OS, GPU | WebGPU | WebGL2 | Evidence |
|---|---|---|---|---|
| Chrome 154.0.8037.58 (installed) | Windows 11, GTX 1060 | verified | verified | WebDriver smoke ([results](https://github.com/TNYCL/dotloom/tree/main/docs/compat/2026-10-02/windows-gtx1060)); reference benchmarks ([performance](./performance.md)) |
| Chrome 153.0.8010.52 (Chrome for Testing) | Windows 11, GTX 1060 | verified | verified | WebDriver smoke |
| Edge 154.0.4258.48 (installed) | Windows 11, GTX 1060 | verified | verified | WebDriver smoke; reference benchmarks |
| Chrome 154.0.8037.92 and 153.0.8010.52 | Linux CI, SwiftShader | verified | verified | `compat` real-browser jobs ([results](https://github.com/TNYCL/dotloom/tree/main/docs/compat/2026-10-02/ci-run-36980003137)) |
| Edge 154.0.4258.53 | Linux CI, SwiftShader | verified | verified | `compat` real-browser job |
| Edge 153.0.4234.48 | Linux CI, SwiftShader | not available: the software device was lost, the viewport moved on | verified | `compat` real-browser job |
| Firefox 157.0 and 156.0.1 | Linux CI, Mesa (Xvfb) | not exposed on Linux | verified | `compat` real-browser jobs |
| Safari 26.6.1 (previous major) | macOS 15 CI runner | not exposed on the runner (`navigator.gpu` undefined) | verified | `compat` Safari job |
| Safari 27 (current major) | — | not verified | not verified | no Mac with Safari 27 is available; hosted runners ship 26.6.x (issue #13) |
| Firefox 157/156 on Windows or macOS, Edge 153 on Windows, Safari WebGPU on Mac hardware | — | not verified | not verified | no such device or install available here |

### Engine families (full suite)

| Build | WebGPU | WebGL2 | Evidence |
|---|---|---|---|
| Chromium 153 (Playwright), Windows, GTX 1060 | verified | verified | local suite |
| Chromium 153 (Playwright), Linux CI, SwiftShader | not available: a plain WebGPU device is destroyed right after creation in this build | verified | `ci` browser suite |
| Firefox 155 (Playwright build), Windows | verified in a window (benchmarks); headless builds return no WebGPU canvas context | verified | local suite and benchmarks |
| Firefox 155 (Playwright build), Linux CI | not exposed | verified (software WebGL, window under Xvfb) | `ci` browser suite |
| WebKit 26.6 (Playwright build), Linux and macOS CI | not available | verified | `ci` and `compat` browser suites |
| WebKit 26.6 (Playwright build), Windows | not available | verified, except resized canvases, which this build does not display (reproduced with plain WebGL) | local suite |

Native rendering (PNG export, GPU pixel tests) is verified on Vulkan (NVIDIA locally,
Mesa lavapipe on Linux CI), Direct3D 12 (WARP on Windows CI) and Metal (macOS CI).
Visual baselines belong to lavapipe; see `CONTRIBUTING.md`.

## Engine and SDK

| Environment | Status |
|---|---|
| Rust (headless engine, CLI) | Windows, Linux, macOS (CI); minimum Rust 1.89 (checked in CI) |
| Node.js 24 | `@dotloomjs/sdk/node` (in-thread engine), tested in CI; other versions not verified |
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
