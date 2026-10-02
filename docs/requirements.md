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
| DL-CORE-2 | engine is usable headless from Rust (create, edit, solve, save) | local | `crates/engine` (no DOM/GPU deps); `crates/engine/tests/engine.rs` |
| DL-CORE-3 | CI checks the dependency boundary (`cargo tree` deny-list) | planned | |
| DL-CORE-4 | single authoritative document copy; views keyed by revision | local | engine owns `Document`; SDK tracks `DotloomEngine.revision`; renderer `SceneCache::revision`; `packages/sdk/test/engine.test.ts` (scene deltas before results) |

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
| DL-GEO-8 | `f64` canonical model; GPU `f32` relative to local origin | local | `crates/render/src/{tess,cache}.rs` (f32 relative to item/chunk origins, chunks ≤ 1e5 mm); `view.rs::large_coordinates_stay_precise`, `tess::line_becomes_one_instance_relative_to_origin` |
| DL-GEO-9 | explicit results for NaN/Inf, zero length, coincident lines, big coords, tiny shapes, singular transforms | local | `non_finite_inputs_are_rejected`, `far_from_origin_intersections_keep_relative_accuracy`, `tiny_shapes_are_handled`, `degenerate_inputs_never_panic` |
| DL-GEO-10 | unit ADR (mm/rad/s), mm/cm/m/in/ft conversions tested; timeline time axis explicit; mixed dimensions rejected | local | ADR-0002, `units.rs` tests |
| DL-GEO-11 | model/solver/tessellation/screen tolerances are separate concepts | local | `tolerance.rs`; solver tolerance in constraints crate (pending) |
| DL-GEO-12 | exact orientation predicates for critical decisions | local | `orientation()` (robust), `orientation_is_exact_for_nearly_collinear_points` |
| DL-GEO-13 | shape-breaking transforms keep semantics: circle/arc non-uniform scale → capability error or explicit conversion; anchors/dimensions tested | local (geometry) | `circle_nonuniform_scale_policy`; constraint-anchor part pending |

## DL-DOC — document model

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-DOC-1 | stable entity IDs, namespaced type IDs, type/schema versions, typed properties, transform, layers, groups, anchor refs, constraint IDs | local | `crates/document/src/{ids,model,value,constraint}.rs`; `json_roundtrip_is_semantically_equal` |
| DL-DOC-2 | reordering does not change IDs | local | `Document::reorder`; `reorder_keeps_ids`, `ids_are_never_reused` |
| DL-DOC-3 | clone/copy-paste remaps internal references; external reference policy documented | local (docs page pending) | `clipboard.rs` (policy in module docs); `copy_paste_remaps_internal_and_keeps_external_refs` |
| DL-DOC-4 | group cycles, missing anchors, invalid references rejected before commit | local (structural); plugin anchors via engine pending | `validate.rs`; `invariant_violations_are_rejected` |
| DL-DOC-5 | deleting entities: explicit effect on constraints, no orphans | local | `DeletePolicy` cascade/reject + `onDelete` per reference; `delete_cascades_constraints_or_rejects`, `deleting_a_wall_deletes_its_door_and_undo_restores_both` |
| DL-DOC-6 | derived geometry/cache separate from canonical data | local | `eval.rs` (`EvalCache`, derived plugin geometry never stored except explicit `fallback`) |
| DL-DOC-7 | unknown plugin payloads preserved opaquely | local | `Entity::data`, `extra` maps; `unknown_fields_and_plugin_payloads_survive` |
| DL-DOC-8 | normalized canonical hash/snapshot (order independent) | local | `canonical.rs`; `hash_ignores_insertion_order_but_not_draw_order`, `negative_zero_is_normalized` |
| DL-DOC-9 | geometry-only edits do not copy/re-render the whole document (measured) | planned | |

## DL-CMD — commands, previews, cancellation, history

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-CMD-1 | pipeline validate → working state → solve → validate invariants → atomic commit → revision/event | local | `engine.rs` (`begin_apply` → `Overlay` → `validate_overlay` → `solve::plan/finish` → `commit`) |
| DL-CMD-2 | failed command leaves the document unchanged | local | `transaction_is_atomic`, `shelf_180_160_130_with_undo_redo_and_unlock` (hash unchanged) |
| DL-CMD-3 | multi-change atomic transactions | local | `Transaction`; `transaction_is_atomic` |
| DL-CMD-4 | drag = transient previews; pointer-up = one history entry | local (engine) | `drag.rs`; `drag_previews_then_commits_one_entry_or_cancels` |
| DL-CMD-5 | Escape / pointer cancel / focus loss cancel policy | local | `packages/sdk/src/editor/{core,dom}.ts` (Escape, pointercancel, lostpointercapture, window blur → `cancel`); `editor.test.ts` "drag-move … capture loss cancels" |
| DL-CMD-6 | undo/redo applies committed before/after without re-solving | local | `history.rs` (`Change::apply`) |
| DL-CMD-7 | new change after undo clears redo branch | local | shelf test (`can_redo` false after new edit) |
| DL-CMD-8 | constraints, properties, plugin payloads undone in the same transaction | local | `Change` covers entities/constraints/groups/layers/settings; delete+undo test |
| DL-CMD-9 | events after commit; reentrant callbacks cannot nest commits | local | `engine.test.ts` "reentrant calls from listeners are queued, not nested"; events posted after commit in `host.ts::flush` |
| DL-CMD-10 | history memory limit + large-operation policy | local | `History` (entries + byte budget; oversize clears history, `undoAvailable: false`); `history_limit_drops_oldest` |
| DL-CMD-11 | request ID + expected revision; stale results rejected | local | `protocol.ts` (request `id`, `expectedRevision`, `revision` in every result); `engine.test.ts` stale test; browser: `tests/e2e/specs/editor.spec.ts` "stale revisions are rejected through the worker" |
| DL-CMD-12 | public API exposes no mutable engine internals | local | `Engine::document()` is read-only; edits only via `Transaction` |

## DL-SOLVE — constraints

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-SOLVE-1 | linear rules: fixed, equal, sum/difference, equal spacing, min/max, constant ratio, prioritized preferences | local (crate) | `rules.rs` (`fix`, `equal`, `linear`, `ratio`, `at_least/at_most`, `equal_spacing`); `tests/solver.rs::equal_spacing_ratio_and_bounds`, `soft_rules_only_apply_inside_hard_set`, shelf tests |
| DL-SOLVE-2 | geometric rules: coincident, horizontal, vertical, fixed point, equal length, distance, parallel, perpendicular, angle, concentric, line–circle and circle–circle tangency; anchor/geometry class checks | local (crate); class checks pending (engine) | `rules.rs`; `tests/solver.rs` (independent formula checks per rule) |
| DL-SOLVE-3 | unsupported equation classes return typed `unsupported` | local (crate) | `Rule::unsupported`; `unsupported_rule_reports_unsupported` |
| DL-SOLVE-4 | Cassowary-class incremental adapter for linear components (kasuari) | local (one-shot); incremental drag sessions pending | `linear.rs`; shelf tests, determinism test |
| DL-SOLVE-5 | numeric backend: scaled residuals, analytic Jacobian (checked numerically), damping, warm start, explicit stop criteria | local (crate) | `numeric.rs`, `expr.rs`; `tests/jacobian.rs` (256 random configs × 22 builders) |
| DL-SOLVE-6 | shared variable/constraint graph, component classification, no backend oscillation | local (crate) | `graph.rs` + unit tests |
| DL-SOLVE-7 | hard constraints exact (not penalties); fixed vars eliminated; equality/inequality feasibility checked; soft only within hard feasible set | local (crate) | ADR-0004 §4; `drag_target_respects_hard_rules_numeric`, `equal_spacing_ratio_and_bounds` |
| DL-SOLVE-8 | drag target / locks / preferences priority; stay near previous solution; branch preservation documented | local (crate); engine drag sessions pending | ADR-0004 §9; `distance_and_fixed_point` |
| DL-SOLVE-9 | statuses: solved, underconstrained, conflicting (with evidence), not-converged, cancelled, unsupported; suspected vs certain conflicts | local (crate) | `solution.rs`; `constant_conflict_is_certain`, `linear_conflict_inside_mixed_component_is_certain`, `impossible_nonlinear_is_suspected_not_certain`, `budget_exhaustion_keeps_input_values` |
| DL-SOLVE-10 | structured diagnostics (rule ID, source label, residual, entities); no auto-removal of user locks; budget exhaustion keeps last valid document | local (crate); engine part pending | `Diagnostic`; `shelf_130_is_rejected_with_certain_minimal_conflict` |
| DL-SOLVE-11 | real cancellation: budgeted steps + event-loop yield; stale revision cannot commit; E2E timeout/cancel test | local | `host.ts` budgeted `step` loop with macrotask yields + cancel; `engine.test.ts` "cancels a long solve between steps"; browser E2E `editor.spec.ts` "a long solve in the worker is cancelled for real" (Chromium/Firefox/WebKit, Windows) |

## DL-RENDER — renderer

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-RENDER-1 | wgpu renderer for geometry, styles, text, grid, dimensions, selection and snap overlays | local | `crates/render`; GPU pixel tests `crates/render/tests/headless.rs` (Vulkan GTX 1060 locally, Mesa lavapipe in CI); browser pixel tests `tests/e2e/specs/render.spec.ts` |
| DL-RENDER-2 | consumes the public scene contract only | local | `dotloom-render` depends only on `dotloom-scene`/`dotloom-geometry` (no engine/document); input is `SceneDelta` |
| DL-RENDER-3 | WebGPU preferred, WebGL2 separately validated; capability detection; explicit init errors; explicit fallback | local | `dotloom-render-web` explicit `webgpu`/`webgl2`; `Viewport` tries backends in order and reports `attempts`; `tests/e2e/specs/render.spec.ts` runs each backend separately (Chromium: both; Firefox/WebKit: WebGL2, WebGPU reported as skipped with reason) |
| DL-RENDER-4 | pan/zoom, devicePixelRatio, resize without double-DPI errors | local | `view.rs` tests; `tests/e2e/specs/render.spec.ts` "device pixel ratio 2 …", "pan, zoom and resize …" (WebKit-Windows resize skipped: plain WebGL repro) |
| DL-RENDER-5 | viewport culling, dirty caches, batched draws | local | `cache.rs` order-preserving chunks/runs, per-item mesh cache, LOD; tests `runs_preserve_stacking`, `culling_and_lod_rebuilds`, `chunks_split_by_count_and_reuse_unchanged_chunks`; headless `culling_hides_offscreen_items…` |
| DL-RENDER-6 | stable screen-size overlays | local | `overlay.rs::markers_have_constant_screen_size`; screen-px line widths in `shaders.wgsl` |
| DL-RENDER-7 | text with licensed font; Unicode incl. Turkish tested | local | Inter 4.1 subset (OFL, `crates/render/assets`); `text.rs` tests (Turkish/Greek/Cyrillic glyphs); headless + browser Turkish text ink checks |
| DL-RENDER-8 | device/context loss, recreation, dispose; GPU/worker resources released | local | `Viewport` loss watchdog/restore; `tests/e2e/specs/render.spec.ts` "recovers from GPU device/context loss" (WebGPU `device.destroy`, WebGL `WEBGL_lose_context`), "dispose releases the canvas"; `Renderer::dispose` |
| DL-RENDER-9 | no silent Canvas2D fallback; WebGL2 path not dependent on compute/storage | local | no Canvas2D path exists; `shaders.wgsl` uses only WebGL2-level features; device limits = `downlevel_webgl2_defaults` on both backends; no base-instance draws |

## DL-INPUT — selection, pointer, snapping, keyboard

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-INPUT-1 | tools: select/multi/box, line/polyline/rect/circle/arc/path, move/rotate/scale, copy/delete, layer/group ops, dimension, grid/snap, split/trim/extend | local (tools); layer panel UI pending (React) | `packages/sdk/src/editor/tools/*`; `editor.test.ts` (16 tests against the real engine); browser `editor.spec.ts` |
| DL-INPUT-2 | tool state machines: idle/start/preview/commit/cancel; capture loss, Escape, leave, focus | local | tool state machines (`toolState`: idle/press/preview/drag/marquee/typing); `editor.test.ts` |
| DL-INPUT-3 | text fields keep keyboard input; global shortcuts do not fire there | local | `dom.ts::isEditableTarget` (keyboard read only from the focused editor); browser `editor.spec.ts` "typing in a form field never triggers editor shortcuts" |
| DL-INPUT-4 | snapping with screen tolerance, priority, hysteresis; snaps never commit hard-rule violations | local | engine snap priority + hysteresis (`query.rs`, `HYSTERESIS`), `engine.rs::hit_test_and_snapping`; `editor.test.ts` "snaps to existing endpoints within the screen radius"; snaps are proposals committed through the solver |
| DL-INPUT-5 | configurable snap options, grid spacing, units, shortcuts | planned | |

## DL-PLUGIN — extensions

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-PLUGIN-1 | extension points: entity type, geometry/anchor recipe, tool, command, snap provider, constraint template, inspector/panel, import/export, storage adapter | planned | |
| DL-PLUGIN-2 | entity definition: namespaced typeId, schema version, typed props, anchors, primitive recipes, constraint templates, migrations | planned | |
| DL-PLUGIN-3 | model definitions evaluated in Rust via typed expression AST; no eval; typed errors for unsupported functions | local | `lang.rs`, `registry.rs`; `dimension_errors`, `plugin_registration_errors_are_typed` |
| DL-PLUGIN-4 | lifecycle register/enable/disable/dispose; type ID conflicts and version mismatches explicit; listener/GPU cleanup | planned | |
| DL-PLUGIN-5 | documents never carry executable code or fetch remote code; missing plugins keep payloads + standard representation; non-editable explained | local (engine) | `eval::ReadOnly`; `missing_plugin_entities_are_read_only_and_preserved` |
| DL-PLUGIN-6 | external plugin works from published packages without private imports | planned | |

## DL-SDK — WASM protocol, Worker, TypeScript, React

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-SDK-1 | strict TS, no unchecked `any`, generated bindings separate from ergonomic API | local | `tsconfig.base.json` (strict, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`); generated bindings in `src/wasm/*` behind `host.ts`/`viewport.ts` |
| DL-SDK-2 | API: init/dispose, load/save, commands/transactions, selection, camera, tool/plugin registration, constraints, events, export, diagnostics | local | `DotloomEngine`, `Viewport`, `EditorCore`, `createEditor`; covered by `engine.test.ts`, `editor.test.ts`, browser E2E |
| DL-SDK-3 | protocol/API/schema versions separated | local | `PROTOCOL_VERSION` (worker), `RENDER_PROTOCOL` (renderer), `capabilities.schema/formatVersion/sceneFormat`; mismatches rejected (`engine.test.ts` protocol tests) |
| DL-SDK-4 | messages carry request ID, document/revision, error code, capabilities | local | `protocol.ts` messages (id, revision, typed `ErrorCode`, capabilities at init) |
| DL-SDK-5 | batching/backpressure; stale drags dropped; transferable ownership | local | drag coalescing in `host.ts`; pointer-move coalescing in `EditorCore`; transferables for scene deltas/files; tests "coalesces queued drag updates", "coalesces pointer moves" |
| DL-SDK-6 | tests: init/dispose races, stale responses, worker crash, reopen | local | `engine.test.ts` (dispose races, stale, protocol errors), `crash.test.ts`, browser `editor.spec.ts` "worker crash is reported and a new engine reopens the saved document" |
| DL-SDK-7 | React snapshot/subscription model without full-tree re-render | planned | |
| DL-SDK-8 | vanilla example without React | planned | |
| DL-SDK-9 | SSR/build import safety; documented support boundary | local (tests); docs pending | `index.ts` has no top-level browser access (renderer module loaded lazily); Node tests import the package entry |
| DL-SDK-10 | WASM/Worker/font assets work from npm install and under `/dotloom/` | dev | E2E harness consumes the built package through Vite (`base: ./`); tarball install and `/dotloom/` Pages path pending |

## DL-FILE — `.dotl`, migration, storage, SVG/DXF/PNG, CLI

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-FILE-1 | ZIP container: `manifest.json`, `document.json`, `assets/`; ADR | local | ADR-0005; `crates/io/src/{zip,dotl}.rs`; `crates/io/tests/io.rs` `dotl_roundtrip_is_semantic_and_deterministic` |
| DL-FILE-2 | semantic round-trip | local | `crates/io/tests/io.rs` `dotl_roundtrip_is_semantic_and_deterministic`, `loaded_file_round_trips_through_engine`; SDK `engine.test.ts` "round-trips .dotl" |
| DL-FILE-3 | tested migrations from older schema versions; clear error for future major | local (framework); no older document schema exists yet | `document/src/migrate.rs` (schema 1 is the first public schema; future schema → `FutureSchema` error, tested in `document.rs`); plugin type migrations `engine.rs::old_wall_versions_are_migrated_on_load` |
| DL-FILE-4 | unknown metadata/plugin fields preserved | local | unknown ZIP entries kept (`unknown_entries`), `document.rs::unknown_fields_and_plugin_payloads_survive` |
| DL-FILE-5 | validation: duplicate IDs, broken refs, missing assets, size/count limits | local | `crates/io/tests/io.rs` `dotl_rejects_bad_containers`, `dotl_assets_are_verified`; `document.rs::invariant_violations_are_rejected` |
| DL-FILE-6 | bounded ZIP parser: traversal, bomb, entry count, corrupt container | local | `crates/io/tests/io.rs` `dotl_path_traversal_and_bombs_are_rejected`, `dotl_rejects_bad_containers` (own bounded ZIP reader) |
| DL-FILE-7 | browser open/download; IndexedDB autosave + recovery | planned | |
| DL-FILE-8 | native atomic save (temp + replace); failed save keeps the old file | local | `crates/io/tests/io.rs` `atomic_save_replaces_or_keeps_the_old_file` |
| DL-FILE-9 | async storage adapter for host storage | planned | |
| DL-FILE-10 | SVG import/export with support matrix; scripts/external refs never reach the DOM | local (support matrix doc pending) | `crates/io/tests/io.rs` `svg_export_import_roundtrip_and_report`, `svg_import_is_safe_and_reports_unsupported_content` (SVG parsed in Rust, never inserted into the DOM) |
| DL-FILE-11 | ASCII DXF LINE/LWPOLYLINE(bulge)/CIRCLE/ARC/TEXT/layers; versions + unit policy; loss report for unsupported entities | local; CI validation of exports with ezdxf pending | `crates/io/tests/io.rs` `dxf_fixtures_from_an_independent_writer` (ezdxf-generated R12/R2000/R2018 fixtures), `dxf_export_roundtrip`, `dxf_rejects_binary_and_garbage` |
| DL-FILE-12 | PNG export with size/background/scale | local (native CLI); browser export pending | `dotloom convert x.png --width --background` via `dotloom-render` headless; `cli.rs::png_export_renders_the_drawing` (CI on lavapipe) |
| DL-FILE-13 | loss/warning reports visible to hosts | local | `ConversionReport` returned by CLI `--json` and SDK `exportSvg/exportDxf/importSvg/importDxf` |
| DL-FILE-14 | CLI inspect/validate/convert, `--json`, exit codes; PNG capability boundary | local | `crates/cli/tests/cli.rs` (inspect/validate/convert, `--json`, exit codes 0–4, PNG capability error without the feature) |

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
| DL-EXAMPLE-1 | shelf configurator: 180→160 gives 60/50/50; 130 rejected with 140 bound; unlock; undo/redo | local (engine with shared plugin JSON); UI example pending | `tests/fixtures/plugins/shelf.json`; `shelf_180_160_130_with_undo_redo_and_unlock` |
| DL-EXAMPLE-2 | floor planner: wall/door entities, door on wall, dimensions, layers, snapping, wall change/delete integrity, save/load | planned | |
| DL-EXAMPLE-3 | timeline: start/duration/end, order/equality/min gap, explicit time→view mapping, locked times respected | planned | |
| DL-EXAMPLE-4 | vanilla integration without React | planned | |
| DL-EXAMPLE-5 | external plugin project installed from packages outside the monorepo | planned | |

## DL-TEST — test strategy

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-TEST-1 | geometry unit/property tests | local | `crates/geometry/tests/properties.rs` (512 cases/property) |
| DL-TEST-2 | constraint tests per rule type, under/over-determined, mixed, priorities, non-convergence, budget cancel | local | `crates/constraints/tests/solver.rs` (22 tests) |
| DL-TEST-3 | independent validation of final geometry | local (solver tests); engine commit validator pending | closed-form checks in `tests/solver.rs` |
| DL-TEST-4 | transaction/history tests | local | `crates/engine/tests/engine.rs` (atomicity, undo/redo, history limits), SDK undo/redo tests |
| DL-TEST-5 | file fixtures: round-trip, migration, unknown plugin, corrupt ZIP/JSON, missing asset, loss reports | planned | |
| DL-TEST-6 | native/WASM parity on normalized output | planned | |
| DL-TEST-7 | SDK public type tests, protocol, lifecycle, error mapping, asset loading | local | `packages/sdk/test/*.test.ts` (30 tests: protocol, lifecycle, error mapping, editor) |
| DL-TEST-8 | browser E2E user flows through reopen | local (partial) | browser flows in `tests/e2e/specs/editor.spec.ts` (draw → select → delete → undo, crash → reopen); autosave/recovery flow pending |
| DL-TEST-9 | visual regression with fixed font/backend/environment | planned | |
| DL-TEST-10 | bounded fuzz with reproducible seeds; findings become fixtures | planned | |
| DL-TEST-11 | package consumer tests from real tarballs/crates | planned | |
| DL-TEST-12 | resource lifecycle tests | local (partial) | dispose tests (engine, viewport/canvas, GPU buffers destroyed in `Renderer::dispose`); leak measurement pending |
| DL-TEST-13 | native Windows/Linux/macOS; Chromium/Firefox/WebKit; WebGPU and WebGL2 selected separately; real Safari smoke | local (Windows: Chromium WebGPU+WebGL2, Firefox WebGL2, WebKit WebGL2); CI Linux matrix pending; real Safari `ext` | `tests/e2e/playwright.config.ts`; results summary in `status.md` |

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
