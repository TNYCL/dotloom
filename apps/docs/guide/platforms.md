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

| Browser | WebGPU | WebGL2 | Where |
|---|---|---|---|
| Chromium (Windows, GPU) | tested | tested | local runs |
| Chromium (Linux, SwiftShader) | environment does not provide a working adapter (plain-WebGPU probe) | tested | CI |
| Firefox | not exposed to canvases in the tested builds | tested (Linux CI with software WebGL under Xvfb; Windows locally) | CI, local |
| WebKit (Playwright build) | not available | tested on Linux CI; the Windows build does not display non-antialiased or resized WebGL canvases (reproduced with plain WebGL) | CI, local |
| Safari (macOS) | — | — | real-Safari smoke test pending: Playwright's WebKit is not Safari |

Native rendering (PNG export, GPU tests) runs on Vulkan, DX12 and Metal adapters and
on Mesa lavapipe in CI.

## Engine and SDK

| Environment | Status |
|---|---|
| Rust (headless engine, CLI) | Windows, Linux, macOS (CI matrix); MSRV 1.89 |
| Node.js ≥ 22 | `@dotloom/sdk/node` (in-thread engine), tested in CI |
| Browsers | module Web Workers, WebAssembly, ES2022 |
| SSR / build tools | importing the packages has no side effects; create engines only in the browser |

## Device and context loss

WebGPU device loss and WebGL context loss are detected (also when nothing is being
drawn), the renderer is recreated on a fresh canvas and the full scene is requested
again from the engine. Repeated losses (three within 30 s) stop rendering with an
error event instead of looping.
