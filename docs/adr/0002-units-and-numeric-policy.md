# ADR-0002: Units, angles and numeric policy

Status: accepted (2026-10-02)

## Decision

- Canonical length unit: **millimetre**. Documents store a *display* unit
  (`mm`, `cm`, `m`, `in`, `ft`) used by UIs and importers; stored coordinates are mm.
- Angles: **radians**, counter-clockwise from +X. UIs may display degrees.
- Time (timelines): **seconds**. Time never becomes a coordinate implicitly; a
  `TimeAxis { origin_s, mm_per_second }` maps it to model X.
- `Quantity { value, dim }` carries exponents of length/angle/time; adding values of
  different dimensions is an error (`DimensionMismatch`). Expression evaluation in
  plugin definitions uses the same dimension check.
- Canonical model and all measurement/solver math use `f64`. GPU buffers use `f32`
  relative to a per-batch local origin; the camera transform is composed in `f64`.
  Rendered/flattened data is never used for measurement.
- Four separate tolerance concepts:
  - `ModelTolerance { abs: 1e-6 mm, rel: 1e-12 }` — equality of model values.
  - `SolverTolerance` (constraints crate) — residual acceptance, scaled per residual.
  - `FlattenTolerance` — max chord deviation for tessellation, derived from zoom.
  - `ScreenTolerance { css_px }` — pick/snap radius, converted with the view scale.
- Critical orientation decisions use Shewchuk's adaptive predicate (`robust` crate).
  Everything else uses explicit relative tolerances, documented at each use.
- Elementary functions (`sin`, `cos`, `atan2`, `hypot`, `tan`, `acos`, …) go through
  `dotloom_geometry::math` (pure-Rust `libm`) in every core crate; `f64::sin` & co.
  are disallowed by clippy (`clippy.toml`). Platform math libraries differ in the
  last bit between Windows, Linux, macOS and WebAssembly, which changed solver
  trajectories (iteration counts) between the native and WASM builds. With `libm`
  the same input gives bit-identical commits, documents, scenes and exports
  everywhere; `crates/wasm/tests/parity.rs` and `packages/sdk/test/parity.test.ts`
  check this against shared digests (`tests/fixtures/parity/`).

## Consequences

Importers convert units explicitly (DXF `$INSUNITS`; SVG user units at 96 dpi unless
the root size says otherwise) and report the chosen policy in the import report.
