# Requirements and evidence

Status values:

- `planned` — not implemented yet.
- `dev` — implemented, not yet verified by a test run.
- `local` — implemented and verified by tests that ran locally.
- `ci` — verified in GitHub Actions on `main` (run linked in `status.md`).
- `hw` — verified on the reference hardware (see `docs/performance.md`).
- `ext` — blocked on external access (account/device); see `status.md`.

A file existing or a function signature is never evidence. Evidence = code + a test
that ran + documentation.

## DL-CORE — dependency boundaries and headless use

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-CORE-1 | geometry/document/constraints/engine/scene/io build without DOM/window/GPU/wgpu | planned | |
| DL-CORE-2 | engine is usable headless from Rust (create, edit, solve, save) | planned | |
| DL-CORE-3 | CI checks the dependency boundary (`cargo tree` deny-list) | planned | |
| DL-CORE-4 | single authoritative document copy; views keyed by revision | planned | |

## DL-GEO — geometry, precision, transforms, spatial queries

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-GEO-1 | primitives: point, segment, polyline (bulge arcs), rect, circle, arc, quad/cubic path, polygon, text; dimension geometry | local | `crates/geometry/src/{curve,shape,dimension}.rs`; unit tests |
| DL-GEO-2 | translate/rotate/scale, local/world transforms, inverse, bounding boxes | local | `affine.rs`, `aabb.rs`; `transform_inverse_roundtrip`, `bbox_contains_samples` |
| DL-GEO-3 | hit-test, point–curve distance, window/crossing selection, visibility query | local | `Shape::hit/intersects_rect/inside_rect`; `crossing_vs_window_selection`; cubic `closest` branch-and-bound |
| DL-GEO-4 | semantic anchors (endpoint/mid/center/vertex/quadrant/corner/insert/centroid) + intersections | local | `Shape::anchors`; `intersect.rs` tests + `intersection_points_lie_on_both_curves` |
| DL-GEO-5 | split/trim/extend for line/arc/circle/polyline classes, capability errors otherwise | local | `edit.rs` + `trim_pieces_lie_on_target` |
| DL-GEO-6 | length/angle/radius measurement from exact `f64` model | local | `Curve::length`, `dimension.rs` tests |
| DL-GEO-7 | spatial index avoids full scans on pointer queries | local | `spatial.rs` + `spatial_index_matches_bruteforce` |
| DL-GEO-8 | `f64` canonical model; GPU `f32` relative to local origin | planned | renderer part pending |
| DL-GEO-9 | explicit results for NaN/Inf, zero length, coincident lines, big coords, tiny shapes, singular transforms | local | `non_finite_inputs_are_rejected`, `far_from_origin_intersections_keep_relative_accuracy`, `tiny_shapes_are_handled`, `degenerate_inputs_never_panic` |
| DL-GEO-10 | unit ADR (mm/rad/s), mm/cm/m/in/ft conversions tested; timeline time axis explicit; mixed dimensions rejected | local | ADR-0002, `units.rs` tests |
| DL-GEO-11 | model/solver/tessellation/screen tolerances are separate concepts | local | `tolerance.rs`; solver tolerance in constraints crate (pending) |
| DL-GEO-12 | exact orientation predicates for critical decisions | local | `orientation()` (robust), `orientation_is_exact_for_nearly_collinear_points` |
| DL-GEO-13 | shape-breaking transforms keep semantics: circle/arc non-uniform scale → capability error or explicit conversion; anchors/dimensions tested | local (geometry) | `circle_nonuniform_scale_policy`; constraint-anchor part pending |

## DL-DOC — document model

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-DOC-1 | stable entity IDs, namespaced type IDs, type/schema versions, typed properties, transform, layers, groups, anchor refs, constraint IDs | planned | |
| DL-DOC-2 | reordering does not change IDs | planned | |
| DL-DOC-3 | clone/copy-paste remaps internal references; external reference policy documented | planned | |
| DL-DOC-4 | group cycles, missing anchors, invalid references rejected before commit | planned | |
| DL-DOC-5 | deleting entities: explicit effect on constraints, no orphans | planned | |
| DL-DOC-6 | derived geometry/cache separate from canonical data | planned | |
| DL-DOC-7 | unknown plugin payloads preserved opaquely | planned | |
| DL-DOC-8 | normalized canonical hash/snapshot (order independent) | planned | |
| DL-DOC-9 | geometry-only edits do not copy/re-render the whole document (measured) | planned | |

## DL-CMD — commands, previews, cancellation, history

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-CMD-1 | pipeline validate → working state → solve → validate invariants → atomic commit → revision/event | planned | |
| DL-CMD-2 | failed command leaves the document unchanged | planned | |
| DL-CMD-3 | multi-change atomic transactions | planned | |
| DL-CMD-4 | drag = transient previews; pointer-up = one history entry | planned | |
| DL-CMD-5 | Escape / pointer cancel / focus loss cancel policy | planned | |
| DL-CMD-6 | undo/redo applies committed before/after without re-solving | planned | |
| DL-CMD-7 | new change after undo clears redo branch | planned | |
| DL-CMD-8 | constraints, properties, plugin payloads undone in the same transaction | planned | |
| DL-CMD-9 | events after commit; reentrant callbacks cannot nest commits | planned | |
| DL-CMD-10 | history memory limit + large-operation policy | planned | |
| DL-CMD-11 | request ID + expected revision; stale results rejected | planned | |
| DL-CMD-12 | public API exposes no mutable engine internals | planned | |

## DL-SOLVE — constraints

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-SOLVE-1 | linear rules: fixed, equal, sum/difference, equal spacing, min/max, constant ratio, prioritized preferences | planned | |
| DL-SOLVE-2 | geometric rules: coincident, horizontal, vertical, fixed point, equal length, distance, parallel, perpendicular, angle, concentric, line–circle and circle–circle tangency; anchor/geometry class checks | planned | |
| DL-SOLVE-3 | unsupported equation classes return typed `unsupported` | planned | |
| DL-SOLVE-4 | Cassowary-class incremental adapter for linear components (kasuari) | planned | |
| DL-SOLVE-5 | numeric backend: scaled residuals, analytic Jacobian (checked numerically), damping, warm start, explicit stop criteria | planned | |
| DL-SOLVE-6 | shared variable/constraint graph, component classification, no backend oscillation | planned | |
| DL-SOLVE-7 | hard constraints exact (not penalties); fixed vars eliminated; equality/inequality feasibility checked; soft only within hard feasible set | planned | |
| DL-SOLVE-8 | drag target / locks / preferences priority; stay near previous solution; branch preservation documented | planned | |
| DL-SOLVE-9 | statuses: solved, underconstrained, conflicting (with evidence), not-converged, cancelled, unsupported; suspected vs certain conflicts | planned | |
| DL-SOLVE-10 | structured diagnostics (rule ID, source label, residual, entities); no auto-removal of user locks; budget exhaustion keeps last valid document | planned | |
| DL-SOLVE-11 | real cancellation: budgeted steps + event-loop yield; stale revision cannot commit; E2E timeout/cancel test | planned | |

## DL-RENDER — renderer

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-RENDER-1 | wgpu renderer for geometry, styles, text, grid, dimensions, selection and snap overlays | planned | |
| DL-RENDER-2 | consumes the public scene contract only | planned | |
| DL-RENDER-3 | WebGPU preferred, WebGL2 separately validated; capability detection; explicit init errors; explicit fallback | planned | |
| DL-RENDER-4 | pan/zoom, devicePixelRatio, resize without double-DPI errors | planned | |
| DL-RENDER-5 | viewport culling, dirty caches, batched draws | planned | |
| DL-RENDER-6 | stable screen-size overlays | planned | |
| DL-RENDER-7 | text with licensed font; Unicode incl. Turkish tested | planned | |
| DL-RENDER-8 | device/context loss, recreation, dispose; GPU/worker resources released | planned | |
| DL-RENDER-9 | no silent Canvas2D fallback; WebGL2 path not dependent on compute/storage | planned | |

## DL-INPUT — selection, pointer, snapping, keyboard

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-INPUT-1 | tools: select/multi/box, line/polyline/rect/circle/arc/path, move/rotate/scale, copy/delete, layer/group ops, dimension, grid/snap, split/trim/extend | planned | |
| DL-INPUT-2 | tool state machines: idle/start/preview/commit/cancel; capture loss, Escape, leave, focus | planned | |
| DL-INPUT-3 | text fields keep keyboard input; global shortcuts do not fire there | planned | |
| DL-INPUT-4 | snapping with screen tolerance, priority, hysteresis; snaps never commit hard-rule violations | planned | |
| DL-INPUT-5 | configurable snap options, grid spacing, units, shortcuts | planned | |

## DL-PLUGIN — extensions

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-PLUGIN-1 | extension points: entity type, geometry/anchor recipe, tool, command, snap provider, constraint template, inspector/panel, import/export, storage adapter | planned | |
| DL-PLUGIN-2 | entity definition: namespaced typeId, schema version, typed props, anchors, primitive recipes, constraint templates, migrations | planned | |
| DL-PLUGIN-3 | model definitions evaluated in Rust via typed expression AST; no eval; typed errors for unsupported functions | planned | |
| DL-PLUGIN-4 | lifecycle register/enable/disable/dispose; type ID conflicts and version mismatches explicit; listener/GPU cleanup | planned | |
| DL-PLUGIN-5 | documents never carry executable code or fetch remote code; missing plugins keep payloads + standard representation; non-editable explained | planned | |
| DL-PLUGIN-6 | external plugin works from published packages without private imports | planned | |

## DL-SDK — WASM protocol, Worker, TypeScript, React

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-SDK-1 | strict TS, no unchecked `any`, generated bindings separate from ergonomic API | planned | |
| DL-SDK-2 | API: init/dispose, load/save, commands/transactions, selection, camera, tool/plugin registration, constraints, events, export, diagnostics | planned | |
| DL-SDK-3 | protocol/API/schema versions separated | planned | |
| DL-SDK-4 | messages carry request ID, document/revision, error code, capabilities | planned | |
| DL-SDK-5 | batching/backpressure; stale drags dropped; transferable ownership | planned | |
| DL-SDK-6 | tests: init/dispose races, stale responses, worker crash, reopen | planned | |
| DL-SDK-7 | React snapshot/subscription model without full-tree re-render | planned | |
| DL-SDK-8 | vanilla example without React | planned | |
| DL-SDK-9 | SSR/build import safety; documented support boundary | planned | |
| DL-SDK-10 | WASM/Worker/font assets work from npm install and under `/dotloom/` | planned | |

## DL-FILE — `.dotl`, migration, storage, SVG/DXF/PNG, CLI

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-FILE-1 | ZIP container: `manifest.json`, `document.json`, `assets/`; ADR | planned | |
| DL-FILE-2 | semantic round-trip | planned | |
| DL-FILE-3 | tested migrations from older schema versions; clear error for future major | planned | |
| DL-FILE-4 | unknown metadata/plugin fields preserved | planned | |
| DL-FILE-5 | validation: duplicate IDs, broken refs, missing assets, size/count limits | planned | |
| DL-FILE-6 | bounded ZIP parser: traversal, bomb, entry count, corrupt container | planned | |
| DL-FILE-7 | browser open/download; IndexedDB autosave + recovery | planned | |
| DL-FILE-8 | native atomic save (temp + replace); failed save keeps the old file | planned | |
| DL-FILE-9 | async storage adapter for host storage | planned | |
| DL-FILE-10 | SVG import/export with support matrix; scripts/external refs never reach the DOM | planned | |
| DL-FILE-11 | ASCII DXF LINE/LWPOLYLINE(bulge)/CIRCLE/ARC/TEXT/layers; versions + unit policy; loss report for unsupported entities | planned | |
| DL-FILE-12 | PNG export with size/background/scale | planned | |
| DL-FILE-13 | loss/warning reports visible to hosts | planned | |
| DL-FILE-14 | CLI inspect/validate/convert, `--json`, exit codes; PNG capability boundary | planned | |

## DL-UI — reference editor

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-UI-1 | canvas, toolbar, layer/object list, property and constraint panels | planned | |
| DL-UI-2 | numeric input with explicit units | planned | |
| DL-UI-3 | constraint lock/enable/disable; conflict and convergence messages | planned | |
| DL-UI-4 | command search, shortcuts, undo/redo, open/save, export | planned | |
| DL-UI-5 | light/dark themes, design tokens, translation keys (en, tr) | planned | |
| DL-UI-6 | keyboard-accessible controls, visible focus, non-canvas editing path | planned | |
| DL-UI-7 | dirty state, autosave status, recovery | planned | |
| DL-UI-8 | empty doc, corrupt file, missing plugin, GPU init failure, long computation + cancel states | planned | |

## DL-EXAMPLE — examples

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-EXAMPLE-1 | shelf configurator: 180→160 gives 60/50/50; 130 rejected with 140 bound; unlock; undo/redo | planned | |
| DL-EXAMPLE-2 | floor planner: wall/door entities, door on wall, dimensions, layers, snapping, wall change/delete integrity, save/load | planned | |
| DL-EXAMPLE-3 | timeline: start/duration/end, order/equality/min gap, explicit time→view mapping, locked times respected | planned | |
| DL-EXAMPLE-4 | vanilla integration without React | planned | |
| DL-EXAMPLE-5 | external plugin project installed from packages outside the monorepo | planned | |

## DL-TEST — test strategy

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-TEST-1 | geometry unit/property tests | local | `crates/geometry/tests/properties.rs` (512 cases/property) |
| DL-TEST-2 | constraint tests per rule type, under/over-determined, mixed, priorities, non-convergence, budget cancel | planned | |
| DL-TEST-3 | independent validation of final geometry | planned | |
| DL-TEST-4 | transaction/history tests | planned | |
| DL-TEST-5 | file fixtures: round-trip, migration, unknown plugin, corrupt ZIP/JSON, missing asset, loss reports | planned | |
| DL-TEST-6 | native/WASM parity on normalized output | planned | |
| DL-TEST-7 | SDK public type tests, protocol, lifecycle, error mapping, asset loading | planned | |
| DL-TEST-8 | browser E2E user flows through reopen | planned | |
| DL-TEST-9 | visual regression with fixed font/backend/environment | planned | |
| DL-TEST-10 | bounded fuzz with reproducible seeds; findings become fixtures | planned | |
| DL-TEST-11 | package consumer tests from real tarballs/crates | planned | |
| DL-TEST-12 | resource lifecycle tests | planned | |
| DL-TEST-13 | native Windows/Linux/macOS; Chromium/Firefox/WebKit; WebGPU and WebGL2 selected separately; real Safari smoke | planned | |

## DL-PERF — performance

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-PERF-1 | reference device + method recorded before measuring | planned | |
| DL-PERF-2 | 10k shapes 1080p pan/zoom p95 ≤ 16.7 ms | planned | |
| DL-PERF-3 | hit-test/snap p95 ≤ 8 ms on the same scene | planned | |
| DL-PERF-4 | 200-variable geometric corpus p95 ≤ 50 ms, hard constraints satisfied | planned | |
| DL-PERF-5 | open 10k-object `.dotl` ≤ 2 s (excl. WASM init) | planned | |
| DL-PERF-6 | 100k stress: open/navigate without crash, real cancel | planned | |
| DL-PERF-7 | repeated open/close: no sustained leak after warm-up | planned | |
| DL-PERF-8 | WASM size, cold start, glyph cache, peak file memory reported | planned | |

## DL-CI — repository, Actions, deployment

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-CI-1 | public repo TNYCL/dotloom | planned | |
| DL-CI-2 | same command entry points locally and in CI; Windows guide | planned | |
| DL-CI-3 | `ci.yml`, `compat.yml`, `nightly.yml`, `pages.yml`, `release.yml` | planned | |
| DL-CI-4 | timeouts, caches keyed by toolchain/platform/lockfile, concurrency cancel, limited retention | planned | |
| DL-CI-5 | main protected by required checks; actual API result reported | planned | |
| DL-CI-6 | fork PRs without secrets; no `pull_request_target` on contributor code; read-only default token; actions pinned to SHAs; dependency updates | planned | |

## DL-OSS — license, docs, packaging, stable API

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-OSS-1 | MIT + Apache-2.0 texts, SPDX metadata, third-party notices, asset rights | dev | `LICENSE-MIT`, `LICENSE-APACHE`, workspace `license` |
| DL-OSS-2 | docs: README, getting started, API reference, plugin guide, constraints model, file format spec, format matrix, platform support, performance, migration, troubleshooting, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, CHANGELOG, templates | planned | |
| DL-OSS-3 | public examples built/tested; badges linked to real workflows | planned | |
| DL-OSS-4 | namespace ownership verified; import paths defined in one place | planned | |
| DL-OSS-5 | cargo package / npm pack tested in clean consumers | planned | |
| DL-OSS-6 | compatibility manifest (engine/SDK lockstep, schema/protocol independent) | planned | |
| DL-OSS-7 | SemVer stable release; npm/crates.io/GitHub Release | planned | |
