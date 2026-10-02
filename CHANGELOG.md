# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) (pre-1.0: minor versions may break).
Engine crates, `@dotloom/sdk` and `@dotloom/react` are versioned in lockstep; file
and protocol versions are listed in [`compatibility.json`](compatibility.json).

## [Unreleased]

The first stable release (`1.0.0`) is in preparation; the workspace still carries
the preview version `0.1.0`. Nothing is published to npm or crates.io yet.

### Added

- Rust engine: geometry (lines, polylines with bulges, rectangles, circles, arcs,
  Béziers, polygons with holes, text, dimensions), documents with layers, groups,
  stable IDs and plugin entities, atomic transactions with undo/redo history,
  spatial index, hit-testing, window/crossing selection and snapping.
- Constraint solving: linear components with Cassowary (kasuari), kept alive
  between pointer moves while dragging (incremental edit-variable updates);
  nonlinear and mixed components with a sparse SQP solver (constraint curvature, approach phase
  for typed values and drags, active-set inequalities); hard rules are exact,
  preferences are weighted; diagnostics for conflicts, redundancy and degrees of
  freedom; budgeted, cancellable solving; an independent check before every commit.
- `.dotl` project files (ZIP container, schema migrations, assets, unknown entries
  preserved), SVG and DXF import/export with loss reports, PNG export, `dotloom`
  CLI (inspect, validate, convert).
- wgpu renderer for WebGPU and WebGL2 with chunked incremental meshes, level of
  detail, SDF text (Turkish and other Latin, Greek, Cyrillic), grid, overlays,
  selection and hover, device-loss recovery.
- `@dotloom/sdk`: engine in a Web Worker with a versioned protocol, viewport,
  editor core with tools (select, pan, line, polyline, rect, circle, arc, path, text,
  dimension, move, rotate, scale, split, trim, extend), snapping, unit-aware input,
  keyboard shortcuts, clipboard, plugin host, storage adapters and autosave with
  recovery, Node entry point.
- `@dotloom/react`: complete editor with toolbar, layers and objects lists,
  inspector, rules panel, plugin panels, command palette, dialogs, status bar with
  autosave state and failures, light/dark/system themes, English and Turkish;
  checked with axe-core (WCAG 2.1 A/AA).
- Examples: vanilla, shelf configurator, floor plan, timeline, external plugin
  consumed from package tarballs; playground and documentation site on GitHub
  Pages.
- Tests: Rust unit/property tests, browser tests in Chromium, Firefox and WebKit on
  both backends, released-browser smoke tests (Chrome, Edge and Firefox current and
  previous major; Safari), native/WASM parity digests, visual regression baselines,
  DXF read-back with ezdxf, checked-in `.dotl`/SVG fixtures, fuzz targets with
  corpus replay, npm package and crate consumer tests; reference-device benchmarks.
- Licensing: MIT OR Apache-2.0 in every package and crate, generated
  `THIRD-PARTY-NOTICES.md` (Rust crates and the Inter font) shipped with the SDK
  and the CLI.
