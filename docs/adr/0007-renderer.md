# ADR-0007: Renderer

Status: accepted (2026-10-02)

## Decision

- wgpu, one code path. The web renderer tries WebGPU first, then WebGL2 — or exactly
  the backend the host requests. The selected backend is reported; failures produce
  typed init errors. There is no Canvas2D fallback.
- Shaders use only features available in WebGL2 (no compute, no storage buffers), so
  both backends run the same pipelines.
- Lines/curves are flattened on the CPU in `f64` with a zoom-dependent tolerance and
  expanded to screen-constant-width quads in the vertex shader. Fills use lyon
  tessellation in local coordinates.
- Precision: vertices are `f32` relative to a batch origin; the view transform is
  built in `f64` and only the small residual offset reaches the GPU.
- Text: glyph atlas generated from an embedded OFL-licensed font (subset covering
  Latin, Latin Extended-A/B incl. Turkish, Greek, Cyrillic) rasterized by fontdue into
  a signed distance field. No complex shaping (Arabic/Indic) — documented limitation.
  Hosts can register additional fonts.
- Overlays (selection, snap markers, previews) are a separate layer in screen units.
- Native PNG export uses the same renderer with an offscreen target.
