# Requirements and evidence

Status values:

- `planned` — not implemented yet.
- `dev` — implemented, not yet verified by a test run.
- `local` — implemented and verified by tests that ran locally.
- `ci` — verified by a test that runs in GitHub Actions on `main` (`ci.yml`: Linux
  Rust/TS/browser; `compat.yml`: Windows and macOS Rust, macOS WebKit and Safari;
  `pages.yml`: the published site). Runs are linked in `status.md`.
- `hw` — verified on the reference hardware (see `docs/performance.md`).
- `ext` — blocked on external access (account/device); see `status.md`.

A file existing or a function signature is never evidence. Evidence = code + a test
that ran + documentation.

## DL-CORE — dependency boundaries and headless use

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-CORE-1 | geometry/document/constraints/engine/scene/io build without DOM/window/GPU/wgpu | ci | core manifests have no wgpu/web-sys/js-sys/wasm-bindgen dependency; `scripts/check-boundaries.mjs` (all targets, all features, renderer canary must be flagged) and `cargo check` of the six core crates for `wasm32-unknown-unknown` without bindings, both in `pnpm run check` and the `rust fmt + clippy` job; ADR-0001, `guide/architecture.md` |
| DL-CORE-2 | engine is usable headless from Rust (create, edit, solve, save) | ci | `crates/engine` (no DOM/GPU deps); `crates/engine/tests/engine.rs` |
| DL-CORE-3 | CI checks the dependency boundary (`cargo tree` deny-list) | ci | `scripts/check-boundaries.mjs` (`cargo tree -e normal --all-features --target all` deny-list of 14 GPU/DOM/window crates; fails if the canary `dotloom-render` is not flagged) run by `scripts/check.mjs rust` locally and in CI |
| DL-CORE-4 | single authoritative document copy; views keyed by revision | ci | engine owns `Document`; SDK tracks `DotloomEngine.revision`; renderer `SceneCache::revision`; `packages/sdk/test/engine.test.ts` (scene deltas before results) |

## DL-GEO — geometry, precision, transforms, spatial queries

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-GEO-1 | primitives: point, segment, polyline (bulge arcs), rect, circle, arc, quad/cubic path, polygon, text; dimension geometry | ci | `crates/geometry/src/{curve,shape,dimension}.rs`; unit tests |
| DL-GEO-2 | translate/rotate/scale, local/world transforms, inverse, bounding boxes | ci | `affine.rs`, `aabb.rs`; `transform_inverse_roundtrip`, `bbox_contains_samples` |
| DL-GEO-3 | hit-test, point–curve distance, window/crossing selection, visibility query | ci | `Shape::hit/intersects_rect/inside_rect`; `crossing_vs_window_selection`; cubic `closest` branch-and-bound |
| DL-GEO-4 | semantic anchors (endpoint/mid/center/vertex/quadrant/corner/insert/centroid) + intersections | ci | `Shape::anchors`; `intersect.rs` tests + `intersection_points_lie_on_both_curves` |
| DL-GEO-5 | split/trim/extend for line/arc/circle/polyline classes, capability errors otherwise | ci | `edit.rs` + `trim_pieces_lie_on_target` |
| DL-GEO-6 | length/angle/radius measurement from exact `f64` model | ci | `Curve::length`, `dimension.rs` tests |
| DL-GEO-7 | spatial index avoids full scans on pointer queries | ci | `spatial.rs` + `spatial_index_matches_bruteforce` |
| DL-GEO-8 | `f64` canonical model; GPU `f32` relative to local origin | ci | `crates/render/src/{tess,cache}.rs` (f32 relative to item/chunk origins, chunks ≤ 1e5 mm); `view.rs::large_coordinates_stay_precise`, `tess::line_becomes_one_instance_relative_to_origin` |
| DL-GEO-9 | explicit results for NaN/Inf, zero length, coincident lines, big coords, tiny shapes, singular transforms | ci | `non_finite_inputs_are_rejected`, `far_from_origin_intersections_keep_relative_accuracy`, `tiny_shapes_are_handled`, `degenerate_inputs_never_panic` |
| DL-GEO-10 | unit ADR (mm/rad/s), mm/cm/m/in/ft conversions tested; timeline time axis explicit; mixed dimensions rejected | ci | ADR-0002, `units.rs` tests |
| DL-GEO-11 | model/solver/tessellation/screen tolerances are separate concepts | ci | `tolerance.rs` (model, tessellation, screen tolerances); solver tolerance `SolveOptions::tolerance` (relative to row scales); independent check tolerance `CHECK_REL_TOL` (`check.rs`); `the_independent_checker_rejects_what_a_loose_solver_accepts` |
| DL-GEO-12 | exact orientation predicates for critical decisions | ci | `orientation()` (robust), `orientation_is_exact_for_nearly_collinear_points` |
| DL-GEO-13 | shape-breaking transforms keep semantics: circle/arc non-uniform scale → capability error or explicit conversion; anchors/dimensions tested | ci | `circle_nonuniform_scale_policy` (geometry); engine `shape_breaking_transforms_keep_rules_and_dimensions_meaningful`: strict non-uniform scale of a circle gives a capability error, a similarity keeps the radius rule and the radial dimension, a non-uniform line scale updates its linear dimension, `Convert` gives a path with the removed rule and the unresolved dimension reported, undo restores all |

## DL-DOC — document model

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-DOC-1 | stable entity IDs, namespaced type IDs, type/schema versions, typed properties, transform, layers, groups, anchor refs, constraint IDs | ci | `crates/document/src/{ids,model,value,constraint}.rs`; `json_roundtrip_is_semantically_equal` |
| DL-DOC-2 | reordering does not change IDs | ci | `Document::reorder`; `reorder_keeps_ids`, `ids_are_never_reused` |
| DL-DOC-3 | clone/copy-paste remaps internal references; external reference policy documented | ci | `clipboard.rs`; `copy_paste_remaps_internal_and_keeps_external_refs`; policy in `guide/documents.md` (copy/paste) |
| DL-DOC-4 | group cycles, missing anchors, invalid references rejected before commit | ci | `validate.rs`; `invariant_violations_are_rejected` (group cycles, missing built-in anchors, broken refs); engine `rules_on_the_wrong_geometry_class_or_missing_anchors_are_rejected_before_commit` (missing built-in and plugin anchors rejected at command validation, document unchanged) |
| DL-DOC-5 | deleting entities: explicit effect on constraints, no orphans | ci | `DeletePolicy` cascade/reject + `onDelete` per reference; `delete_cascades_constraints_or_rejects`, `deleting_a_wall_deletes_its_door_and_undo_restores_both` |
| DL-DOC-6 | derived geometry/cache separate from canonical data | ci | `eval.rs` (`EvalCache`, derived plugin geometry never stored except explicit `fallback`) |
| DL-DOC-7 | unknown plugin payloads preserved opaquely | ci | `Entity::data`, `extra` maps; `unknown_fields_and_plugin_payloads_survive` |
| DL-DOC-8 | normalized canonical hash/snapshot (order independent) | ci | `canonical.rs`; `hash_ignores_insertion_order_but_not_draw_order`, `negative_zero_is_normalized` |
| DL-DOC-9 | geometry-only edits do not copy/re-render the whole document (measured) | hw | engine test `one_edit_in_a_large_document_touches_one_scene_item` (10k entities: one changed entity, one scene upsert, delta < 0.1 % of the full scene); render cache test `chunks_split_by_count_and_reuse_unchanged_chunks`; measured in Chrome/Firefox: commit p95 0.8–2 ms, 120-byte delta, 1 item tessellated, 1 chunk rebuilt (`files.bench.ts`, `docs/performance.md`) |

## DL-CMD — commands, previews, cancellation, history

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-CMD-1 | pipeline validate → working state → solve → validate invariants → atomic commit → revision/event | ci | `engine.rs` (`begin_apply` → `Overlay` → `validate_overlay` → `solve::plan/finish` → `commit`) |
| DL-CMD-2 | failed command leaves the document unchanged | ci | `transaction_is_atomic`, `shelf_180_160_130_with_undo_redo_and_unlock` (hash unchanged) |
| DL-CMD-3 | multi-change atomic transactions | ci | `Transaction`; `transaction_is_atomic` |
| DL-CMD-4 | drag = transient previews; pointer-up = one history entry | ci | `drag.rs`; `drag_previews_then_commits_one_entry_or_cancels`; SDK `editor.test.ts` "drag-move commits one undoable step; capture loss cancels" |
| DL-CMD-5 | Escape / pointer cancel / focus loss cancel policy | ci | `packages/sdk/src/editor/{core,dom}.ts` (Escape, pointercancel, lostpointercapture, window blur → `cancel`); `editor.test.ts` "drag-move … capture loss cancels" |
| DL-CMD-6 | undo/redo applies committed before/after without re-solving | ci | `history.rs` (`Change::apply`) |
| DL-CMD-7 | new change after undo clears redo branch | ci | shelf test (`can_redo` false after new edit) |
| DL-CMD-8 | constraints, properties, plugin payloads undone in the same transaction | ci | `Change` covers entities/constraints/groups/layers/settings; delete+undo test |
| DL-CMD-9 | events after commit; reentrant callbacks cannot nest commits | ci | `engine.test.ts` "reentrant calls from listeners are queued, not nested"; events posted after commit in `host.ts::flush` |
| DL-CMD-10 | history memory limit + large-operation policy | ci | `History` (entries + byte budget; oversize clears history, `undoAvailable: false`); `history_limit_drops_oldest` |
| DL-CMD-11 | request ID + expected revision; stale results rejected | ci | `protocol.ts` (request `id`, `expectedRevision`, `revision` in every result); `engine.test.ts` stale test; browser: `tests/e2e/specs/editor.spec.ts` "stale revisions are rejected through the worker" |
| DL-CMD-12 | public API exposes no mutable engine internals | ci | `Engine::document()` is read-only; edits only via `Transaction` |

## DL-SOLVE — constraints

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-SOLVE-1 | linear rules: fixed, equal, sum/difference, equal spacing, min/max, constant ratio, prioritized preferences | ci | `rules.rs` (`fix`, `equal`, `linear`, `ratio`, `at_least/at_most`, `equal_spacing`); `tests/solver.rs::equal_spacing_ratio_and_bounds`, `soft_rules_only_apply_inside_hard_set`, shelf tests |
| DL-SOLVE-2 | geometric rules: coincident, horizontal, vertical, fixed point, equal length, distance, parallel, perpendicular, angle, concentric, line–circle and circle–circle tangency; anchor/geometry class checks | ci | `rules.rs`; `tests/solver.rs` (independent formula checks per rule); engine class checks: rules on the wrong geometry class become `unsupported` with the reason (`rules_on_the_wrong_geometry_class_or_missing_anchors_are_rejected_before_commit`, `unsupported_rules_without_rows_are_reported_not_skipped`) |
| DL-SOLVE-3 | unsupported equation classes return typed `unsupported` | ci | `Rule::unsupported`; `unsupported_rule_reports_unsupported`, `unsupported_rules_without_rows_are_reported_not_skipped` (hard and soft) |
| DL-SOLVE-4 | Cassowary-class incremental adapter for linear components (kasuari) | ci | `linear.rs` (kasuari); incremental drag sessions `session.rs` (`LinearSession`: edit variables for the drag target, incremental add/remove, independent verification, fallback to a full solve); `tests/session.rs` (property test against full solves, hard/soft targets, infeasible positions, structural changes); engine `linear_drags_are_solved_incrementally_and_match_fresh_drags`; ADR-0004 §12; 15–67× faster (`examples/drag_session.rs`, `docs/performance.md`) |
| DL-SOLVE-5 | numeric backend: scaled residuals, analytic Jacobian (checked numerically), damping, warm start, explicit stop criteria | ci | `numeric.rs`, `expr.rs`; `tests/jacobian.rs` (256 random configs × 22 builders) |
| DL-SOLVE-6 | shared variable/constraint graph, component classification, no backend oscillation | ci | `graph.rs` + unit tests |
| DL-SOLVE-7 | hard constraints exact (not penalties); fixed vars eliminated; equality/inequality feasibility checked; soft only within hard feasible set | ci | ADR-0004 §4; `drag_target_respects_hard_rules_numeric`, `equal_spacing_ratio_and_bounds` |
| DL-SOLVE-8 | drag target / locks / preferences priority; stay near previous solution; branch preservation documented | ci | ADR-0004 §9/§11 (pinned, unpinned and relaxed attempts, stay multipliers, branch keeping); `distance_and_fixed_point`; engine `feasible_drag_targets_are_met_exactly` (door slides before it shrinks), `rejected_drag_positions_keep_the_last_valid_preview`; `drag_target_respects_hard_rules_numeric` |
| DL-SOLVE-9 | statuses: solved, underconstrained, conflicting (with evidence), not-converged, cancelled, unsupported; suspected vs certain conflicts | ci | `solution.rs`; `constant_conflict_is_certain`, `linear_conflict_inside_mixed_component_is_certain`, `impossible_nonlinear_is_suspected_not_certain`, `budget_exhaustion_keeps_input_values` |
| DL-SOLVE-10 | structured diagnostics (rule ID, source label, residual, entities); no auto-removal of user locks; budget exhaustion keeps last valid document | ci | `Diagnostic`/`DiagnosticReport` (rule and constraint IDs, labels, sources, residual, entities, nearest values); `shelf_130_is_rejected_with_certain_minimal_conflict`; engine `an_exhausted_iteration_budget_rejects_the_change_and_keeps_the_document`, `the_independent_checker_rejects_what_a_loose_solver_accepts`; locks are never removed by the solver (locked parameters are constants) |
| DL-SOLVE-11 | real cancellation: budgeted steps + event-loop yield; stale revision cannot commit; E2E timeout/cancel test | ci | `host.ts` budgeted `step` loop with macrotask yields + cancel; `engine.test.ts` "cancels a long solve between steps"; browser E2E `editor.spec.ts` "a long solve in the worker is cancelled for real" (Chromium/Firefox/WebKit, Windows) |

## DL-RENDER — renderer

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-RENDER-1 | wgpu renderer for geometry, styles, text, grid, dimensions, selection and snap overlays | ci | `crates/render`; GPU pixel tests `crates/render/tests/headless.rs` (Vulkan GTX 1060 locally, Mesa lavapipe in CI); browser pixel tests `tests/e2e/specs/render.spec.ts` |
| DL-RENDER-2 | consumes the public scene contract only | ci | `dotloom-render` depends only on `dotloom-scene`/`dotloom-geometry` (no engine/document); input is `SceneDelta` |
| DL-RENDER-3 | WebGPU preferred, WebGL2 separately validated; capability detection; explicit init errors; explicit fallback | ci | `dotloom-render-web` explicit `webgpu`/`webgl2`; `Viewport` tries backends in order and reports `attempts`; `tests/e2e/specs/render.spec.ts` runs each backend separately (Chromium: both; Firefox/WebKit: WebGL2, WebGPU reported as skipped with reason) |
| DL-RENDER-4 | pan/zoom, devicePixelRatio, resize without double-DPI errors | ci | `view.rs` tests; `tests/e2e/specs/render.spec.ts` "device pixel ratio 2 …", "pan, zoom and resize …" (WebKit-Windows resize skipped: plain WebGL repro) |
| DL-RENDER-5 | viewport culling, dirty caches, batched draws | ci | `cache.rs` order-preserving chunks/runs, per-item mesh cache, LOD; tests `runs_preserve_stacking`, `culling_and_lod_rebuilds`, `chunks_split_by_count_and_reuse_unchanged_chunks`; headless `culling_hides_offscreen_items…` |
| DL-RENDER-6 | stable screen-size overlays | ci | `overlay.rs::markers_have_constant_screen_size`; screen-px line widths in `shaders.wgsl` |
| DL-RENDER-7 | text with licensed font; Unicode incl. Turkish tested | ci | Inter 4.1 subset (OFL, `crates/render/assets`), outlines via `read-fonts` and an own coverage rasterizer (`text.rs`: `outlines_match_the_glyph_boxes_of_the_font` for every simple and composite glyph, `rasterizer_coverage_area_and_winding`, Turkish/Greek/Cyrillic glyphs); pair kerning shared with `dotloom-geometry` (`line_layout_matches_harfbuzz_kerning`: widths equal HarfBuzz for Latin and Turkish strings; `headless_text_metrics_match_the_font`: every advance equals the font); headless and browser Turkish text ink checks; visual baseline `text-turkish` |
| DL-RENDER-8 | device/context loss, recreation, dispose; GPU/worker resources released | ci | `Viewport` loss watchdog/restore; `tests/e2e/specs/render.spec.ts` "recovers from GPU device/context loss" (WebGPU `device.destroy`, WebGL `WEBGL_lose_context`), "dispose releases the canvas"; `Renderer::dispose` |
| DL-RENDER-9 | no silent Canvas2D fallback; WebGL2 path not dependent on compute/storage | ci | no Canvas2D path exists; `shaders.wgsl` uses only WebGL2-level features; device limits = `downlevel_webgl2_defaults` on both backends; no base-instance draws |

## DL-INPUT — selection, pointer, snapping, keyboard

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-INPUT-1 | tools: select/multi/box, line/polyline/rect/circle/arc/path, move/rotate/scale, copy/delete, layer/group ops, dimension, grid/snap, split/trim/extend | ci | `packages/sdk/src/editor/tools/*`; `editor.test.ts` (drawing, selection, manipulation, split/trim/extend, against the real engine); React `LayersPanel` (add, visibility, lock, active layer) and group shortcuts tested; browser `editor.spec.ts` |
| DL-INPUT-2 | tool state machines: idle/start/preview/commit/cancel; capture loss, Escape, leave, focus | ci | tool state machines (`toolState`: idle/press/preview/drag/marquee/typing); `editor.test.ts` |
| DL-INPUT-3 | text fields keep keyboard input; global shortcuts do not fire there | ci | `dom.ts::isEditableTarget` (keyboard read only from the focused editor); browser `editor.spec.ts` "typing in a form field never triggers editor shortcuts" |
| DL-INPUT-4 | snapping with screen tolerance, priority, hysteresis; snaps never commit hard-rule violations | ci | engine snap priority + hysteresis (`query.rs`, `HYSTERESIS`), `engine.rs::hit_test_and_snapping`; `editor.test.ts` "snaps to existing endpoints within the screen radius"; snaps are proposals committed through the solver |
| DL-INPUT-5 | configurable snap options, grid spacing, units, shortcuts | ci | `EditorCoreOptions` (`snap`, `snapRadiusPx`, `snapEnabled`, `shortcuts`, `unitScale`), `viewport.setGrid`, document `displayUnit`/`gridSpacing`; `editor.test.ts` "configuration (DL-INPUT-5)" (remapped shortcuts replace defaults, snap kinds off, snap radius, grid spacing), unit formatting tests, browser keyboard test with `1,7 m`; `guide/tools.md` "Configuration" |

## DL-PLUGIN — extensions

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-PLUGIN-1 | extension points: entity type, geometry/anchor recipe, tool, command, snap provider, constraint template, inspector/panel, import/export, storage adapter | ci | `DotloomPlugin` (types, tools, commands, snap providers, panels, importers/exporters, storage adapters, constraint templates) + engine `EntityTypeDef` (geometry/anchor recipes); tests: `plugins.test.ts` (registration, tools, commands, snap providers, importers/exporters, storage adapters, panels), React `ConstraintsPanel` plugin templates (`editor.test.tsx`), `DotloomEditor` mounts plugin panels by `forTypes` and autosaves to plugin storage (`dotloom-editor.test.tsx`); `guide/plugins.md` |
| DL-PLUGIN-2 | entity definition: namespaced typeId, schema version, typed props, anchors, primitive recipes, constraint templates, migrations | ci | `EntityTypeDef` (`registry.rs`, `lang.rs`, `migrate.rs`); `type_id_grammar`, `plugin_registration_errors_are_typed`, `old_wall_versions_are_migrated_on_load`, `entities_from_a_newer_plugin_version_open_read_only`, `dimension_errors`; SDK namespace/conflict test; `guide/plugins.md` |
| DL-PLUGIN-3 | model definitions evaluated in Rust via typed expression AST; no eval; typed errors for unsupported functions | ci | `lang.rs`, `registry.rs`; `dimension_errors`, `plugin_registration_errors_are_typed` |
| DL-PLUGIN-4 | lifecycle register/enable/disable/dispose; type ID conflicts and version mismatches explicit; listener/GPU cleanup | ci | `PluginHost` register/enable/disable/unregister/dispose, `onDispose`, auto-removed listeners, idempotent panel unmount; `plugins.test.ts` (lifecycle; "lists tools, runs onDispose and listener cleanups, mounts panels once, dispose() releases everything"; conflicts and SDK range mismatches); newer type versions read-only (engine test). Plugins have no direct GPU access: their objects render through the scene, and resources a plugin owns are released through `onDispose` (tested) |
| DL-PLUGIN-5 | documents never carry executable code or fetch remote code; missing plugins keep payloads + standard representation; non-editable explained | ci | `eval::ReadOnly`; `missing_plugin_entities_are_read_only_and_preserved`; fixture `floorplan-plugin-0.1.0.dotl` (`plugin_objects_open_read_only_without_the_plugin_and_keep_their_data`); React "read-only objects of a disabled plugin are explained", missing-plugin banner (`dotloom-editor.test.tsx`) |
| DL-PLUGIN-6 | external plugin works from published packages without private imports | ci | `examples/external-plugin` (outside the pnpm workspace; imports only `@dotloomjs/sdk`, `@dotloomjs/sdk/node`, `@dotloomjs/react`; `exports` maps block deep imports); `scripts/smoke-packages.mjs` (packs, installs outside the repo, `npm test`, builds at `/` and `/dotloom/`); browser `tests/e2e/external/external.spec.ts`; `ci` packages job and `release.yml` |

## DL-SDK — WASM protocol, Worker, TypeScript, React

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-SDK-1 | strict TS, no unchecked `any`, generated bindings separate from ergonomic API | ci | `tsconfig.base.json` (strict, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`); generated bindings in `src/wasm/*` behind `host.ts`/`viewport.ts` |
| DL-SDK-2 | API: init/dispose, load/save, commands/transactions, selection, camera, tool/plugin registration, constraints, events, export, diagnostics | ci | `DotloomEngine`, `Viewport`, `EditorCore`, `createEditor`; covered by `engine.test.ts`, `editor.test.ts`, browser E2E |
| DL-SDK-3 | protocol/API/schema versions separated | ci | `PROTOCOL_VERSION` (worker), `RENDER_PROTOCOL` (renderer), `capabilities.schema/formatVersion/sceneFormat`; mismatches rejected (`engine.test.ts` protocol tests) |
| DL-SDK-4 | messages carry request ID, document/revision, error code, capabilities | ci | `protocol.ts` messages (id, revision, typed `ErrorCode`, capabilities at init) |
| DL-SDK-5 | batching/backpressure; stale drags dropped; transferable ownership | ci | drag coalescing in `host.ts`; pointer-move coalescing in `EditorCore`; transferables for scene deltas/files; tests "coalesces queued drag updates", "coalesces pointer moves" |
| DL-SDK-6 | tests: init/dispose races, stale responses, worker crash, reopen | ci | `engine.test.ts` (dispose races, stale, protocol errors), `crash.test.ts`, browser `editor.spec.ts` "worker crash is reported and a new engine reopens the saved document" |
| DL-SDK-7 | React snapshot/subscription model without full-tree re-render | ci | `editor/store.ts`, React hooks on `useSyncExternalStore` (`useEditorState`, `usePointer`, `useRevisionQuery`); "pointer moves re-render only the status bar cursor" (`editor.test.tsx`); `guide/react.md` |
| DL-SDK-8 | vanilla example without React | ci | `examples/vanilla` (`createEditor`, plain DOM toolbar, `toolText`); browser `site.spec.ts` "vanilla example: toolbar, drawing, undo/redo, save, SVG export and open without React" in the `ci` docs job and `pages.yml`; `guide/getting-started.md` |
| DL-SDK-9 | SSR/build import safety; documented support boundary | ci | `index.ts` has no top-level browser access (renderer loaded lazily); Node tests import the package entry; support boundary in `guide/getting-started.md` and `guide/platforms.md` |
| DL-SDK-10 | WASM/Worker/font assets work from npm install and under `/dotloom/` | ci | `new URL(…, import.meta.url)` + `workerUrl`/`engineWasmUrl`/`renderWasmUrl` overrides; font embedded in the renderer WASM; tarball install outside the repo served at `/` and built for `/dotloom/` (`external.spec.ts`: all WASM/JS from the site path, text renders); workspace site under `/dotloom/` (`site.spec.ts`, PRs and Pages); `guide/getting-started.md` "Bundlers" |

## DL-FILE — `.dotl`, migration, storage, SVG/DXF/PNG, CLI

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-FILE-1 | ZIP container: `manifest.json`, `document.json`, `assets/`; ADR | ci | ADR-0005; `crates/io/src/{zip,dotl}.rs`; `crates/io/tests/io.rs` `dotl_roundtrip_is_semantic_and_deterministic` |
| DL-FILE-2 | semantic round-trip | ci | `crates/io/tests/io.rs` `dotl_roundtrip_is_semantic_and_deterministic`, `loaded_file_round_trips_through_engine`; SDK `engine.test.ts` "round-trips .dotl" |
| DL-FILE-3 | tested migrations from older schema versions; clear error for future major | ci | `document/src/migrate.rs` (schema 1 is the first public schema; a future schema is an error); checked-in files written by 0.1.0 that every later version must open (`tests/fixtures/dotl/*-0.1.0.dotl`, `crates/io/tests/fixtures.rs`); `future-schema.dotl`/`future-format.dotl` rejected; plugin type migrations `old_wall_versions_are_migrated_on_load`; `guide/versioning.md` |
| DL-FILE-4 | unknown metadata/plugin fields preserved | ci | unknown ZIP entries kept (`unknown_entries`), `document.rs::unknown_fields_and_plugin_payloads_survive` |
| DL-FILE-5 | validation: duplicate IDs, broken refs, missing assets, size/count limits | ci | `crates/io/tests/io.rs` `dotl_rejects_bad_containers`, `dotl_assets_are_verified`; `document.rs::invariant_violations_are_rejected` |
| DL-FILE-6 | bounded ZIP parser: traversal, bomb, entry count, corrupt container | ci | `crates/io/tests/io.rs` `dotl_path_traversal_and_bombs_are_rejected`, `dotl_rejects_bad_containers` (own bounded ZIP reader) |
| DL-FILE-7 | browser open/download; IndexedDB autosave + recovery | ci | `storage.ts` (`pickFile`, `readFile`, `downloadBytes`, `IndexedDbStorage`, `Autosave`); browser: "IndexedDB autosave survives a reload and is offered for recovery", "unsaved work is offered for recovery after a reload", "command palette exports SVG; save and reopen a .dotl file", Ctrl+S/Ctrl+O test, "IndexedDB failure (QuotaExceededError / blocked) is reported and editing goes on" (Chromium, Firefox, WebKit); `guide/storage.md` |
| DL-FILE-8 | native atomic save (temp + replace); failed save keeps the old file | ci | `crates/io/tests/io.rs` `atomic_save_replaces_or_keeps_the_old_file` |
| DL-FILE-9 | async storage adapter for host storage | ci | async `StorageAdapter` (read/meta/write/remove/list), `MemoryStorage`, `IndexedDbStorage`, plugin-contributed adapters; tests: SDK autosave/recovery, React "a host-supplied adapter receives autosaves; its failures are shown and recover", plugin storage used by `DotloomEditor`; `guide/storage.md` |
| DL-FILE-10 | SVG import/export with support matrix; scripts/external refs never reach the DOM | ci | `crates/io/tests/io.rs` `svg_export_import_roundtrip_and_report`, `svg_import_is_safe_and_reports_unsupported_content`; fixture `tests/fixtures/svg/unsupported.svg` (`svg_fixture_reports_every_loss_and_imports_no_active_content`); SVG parsed in Rust, never inserted into the DOM; support matrix in `guide/formats.md` |
| DL-FILE-11 | ASCII DXF LINE/LWPOLYLINE(bulge)/CIRCLE/ARC/TEXT/layers; versions + unit policy; loss report for unsupported entities | ci | `crates/io/tests/io.rs` `dxf_fixtures_from_an_independent_writer` (ezdxf R12/R2000/R2018), `dxf_export_roundtrip`, `dxf_rejects_binary_and_garbage`; exports read back with ezdxf in CI (`dxf export read back with ezdxf` job, `tests/fixtures/dxf/readback.py`); `guide/formats.md` |
| DL-FILE-12 | PNG export with size/background/scale | ci | native: `dotloom convert x.png --width --background` (`cli.rs::png_export_renders_the_drawing`, lavapipe in CI); browser: `Viewport.exportPng` from the command palette (`playground.spec.ts` decodes the PNG and finds the drawing) |
| DL-FILE-13 | loss/warning reports visible to hosts | ci | `ConversionReport` returned by CLI `--json` and SDK `exportSvg/exportDxf/importSvg/importDxf` |
| DL-FILE-14 | CLI inspect/validate/convert, `--json`, exit codes; PNG capability boundary | ci | `crates/cli/tests/cli.rs` (inspect/validate/convert, `--json`, exit codes 0–4, PNG capability error without the feature) |

## DL-UI — reference editor

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-UI-1 | canvas, toolbar, layer/object list, property and constraint panels | ci | `DotloomEditor.tsx`, `Toolbar`, `LayersPanel`, `ObjectsPanel`, `Inspector`, `ConstraintsPanel`, `PluginPanels`; React tests (toolbar, layers, objects, inspector, constraints); browser "room planner example renders…"; `guide/react.md` |
| DL-UI-2 | numeric input with explicit units | ci | Inspector `NumberField` with explicit unit (`units.ts`); React "160 cm gives 60/50/50…", "invalid input shows a message…; Turkish UI"; browser keyboard test (`1,7 m` gives 170 cm) |
| DL-UI-3 | constraint lock/enable/disable; conflict and convergence messages | ci | Inspector locks; `ConstraintsPanel` enable/disable/remove, templates, conflict and non-convergence messages; React shelf test (lock/unlock, conflict, nearest 140 cm) and "explains a non-converged file, toggles and removes rules, applies plugin templates"; browser shelf flow |
| DL-UI-4 | command search, shortcuts, undo/redo, open/save, export | ci | `CommandPalette`, `actions.ts`, undo/redo, Ctrl+K/S/O; React "command palette filters, navigates and runs"; browser palette SVG/DXF/PNG export, Ctrl+S save (status "Saved"), Ctrl+O open |
| DL-UI-5 | light/dark themes, design tokens, translation keys (en, tr) | ci | design tokens (`--dl-*`) and `[data-theme='dark']`; `i18n.tsx` (en, tr; tool texts from the SDK); React "the system theme follows the OS preference and its changes", "every English message has a Turkish translation with the same placeholders"; browser dark theme pixels and Turkish UI |
| DL-UI-6 | keyboard-accessible controls, visible focus, non-canvas editing path | ci | `:focus-visible` outline, dialog focus trap/restore, keyboard listbox, labelled controls; browser "keyboard-only: reach an object through the list and edit it", visible focus check (2 px outline), axe-core WCAG 2.1 A/AA scan (light, dark, palette open) with no violations |
| DL-UI-7 | dirty state, autosave status, recovery | ci | status bar: saving/autosaved/unsaved/saved and autosave failures (alert); `RecoveryDialog`; browser recovery after reload, "Saved" after Ctrl+S, IndexedDB failures; React host-adapter failure and recovery |
| DL-UI-8 | empty doc, corrupt file, missing plugin, GPU init failure, long computation + cancel states | ci | `dotloom-editor.test.tsx` (full editor in jsdom with the real engine and renderer module): GPU start failure with every backend attempt, corrupt file, missing plugin, busy state with Cancel; browser empty-drawing note (`playground.spec.ts`); `guide/troubleshooting.md` |

## DL-EXAMPLE — examples

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-EXAMPLE-1 | shelf configurator: 180→160 gives 60/50/50; 130 rejected with 140 bound; unlock; undo/redo | ci | `tests/fixtures/plugins/shelf.json`; engine `shelf_180_160_130_with_undo_redo_and_unlock`; `examples/shelf-configurator`; React shelf test; browser `playground.spec.ts` shelf flow |
| DL-EXAMPLE-2 | floor planner: wall/door entities, door on wall, dimensions, layers, snapping, wall change/delete integrity, save/load | ci | `examples/plugins/src/floorplan.ts` (wall/door, tools, layers, dimension); `examples.test.ts` (corner drag and dimension, door slides or conflict, delete and save/load, tools, closing on the first corner by endpoint snap, layer assignment and locking); engine door/wall tests; `examples/floorplan` |
| DL-EXAMPLE-3 | timeline: start/duration/end, order/equality/min gap, explicit time→view mapping, locked times respected | ci | `examples/plugins/src/timeline.ts` (start/duration, `x0`/`x1` from the explicit time axis, order/gap/equal-duration/lock rules); `examples.test.ts` (pushes, locked release, 30-minute gap held, minimum duration, equal durations); engine `timeline_order_gap_and_locked_times`, `linear_drags_are_solved_incrementally_and_match_fresh_drags` |
| DL-EXAMPLE-4 | vanilla integration without React | ci | `examples/vanilla` (see DL-SDK-8); browser interaction test on every PR |
| DL-EXAMPLE-5 | external plugin project installed from packages outside the monorepo | ci | see DL-PLUGIN-6 |

## DL-TEST — test strategy

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-TEST-1 | geometry unit/property tests | ci | `crates/geometry/tests/properties.rs` (512 cases/property) |
| DL-TEST-2 | constraint tests per rule type, under/over-determined, mixed, priorities, non-convergence, budget cancel | ci | `crates/constraints/tests/solver.rs` (22 tests) |
| DL-TEST-3 | independent validation of final geometry | ci | closed-form checks in `tests/solver.rs` and `tests/session.rs`; engine independent checker (`check.rs`) on every commit: `the_independent_checker_rejects_what_a_loose_solver_accepts`, `verify_measures_rules_of_an_opened_file_independently` |
| DL-TEST-4 | transaction/history tests | ci | `crates/engine/tests/engine.rs` (atomicity, undo/redo, history limits), SDK undo/redo tests |
| DL-TEST-5 | file fixtures: round-trip, migration, unknown plugin, corrupt ZIP/JSON, missing asset, loss reports | ci | `tests/fixtures/dotl/` (0.1.0 drawing and plugin files; truncated, not-a-zip, corrupt JSON, missing document, missing asset, future format/schema) read by `crates/io/tests/fixtures.rs`; `tests/fixtures/svg/unsupported.svg`; `crates/io/tests/io.rs` (round-trip, bombs, traversal, assets, DXF/SVG loss reports); the fixtures also seed the fuzz targets |
| DL-TEST-6 | native/WASM parity on normalized output | ci | bit-identical digests of commit reports, documents, full scenes, hit-test/snap and SVG/DXF exports over 8 corpus cases: `crates/wasm/tests/parity.rs` (native; Linux in `ci`, Windows/macOS in `compat`) and `packages/sdk/test/parity.test.ts` (WASM) against `tests/fixtures/parity/expected.json`; deterministic `libm` math (ADR-0002) |
| DL-TEST-7 | SDK public type tests, protocol, lifecycle, error mapping, asset loading | ci | `packages/sdk/test/*.test.ts` (30 tests: protocol, lifecycle, error mapping, editor) |
| DL-TEST-8 | browser E2E user flows through reopen | ci | browser flows in `tests/e2e/specs/editor.spec.ts` (draw, select, delete, undo; crash and reopen; autosave, reload, recovery) and `playground.spec.ts` (shelf, room planner, timeline, save/reopen, recovery, keyboard-only) |
| DL-TEST-9 | visual regression with fixed font/backend/environment | ci | `crates/cli/tests/visual.rs` against reviewed Mesa lavapipe baselines (`tests/fixtures/visual/*.png`, fixed font and backend) in the `rust test (ubuntu-24.04)` job; update procedure in CONTRIBUTING |
| DL-TEST-10 | bounded fuzz with reproducible seeds; findings become fixtures | ci | 7 cargo-fuzz targets (`fuzz/`), nightly bounded runs with recorded seeds (`nightly.yml`, no crash so far); every test run replays seeds, fixtures and `fuzz/regressions/` plus deterministic mutations (`crates/cli/tests/fuzz_replay.rs`); `fuzz/regressions/README.md` |
| DL-TEST-11 | package consumer tests from real tarballs/crates | ci | `scripts/smoke-packages.mjs` (npm tarballs installed outside the repo, tested, built at `/` and `/dotloom/`, run in a browser) and `scripts/smoke-crates.mjs` (crates packaged and verified, a consumer outside the repo builds and runs against them); both in the `ci` packages job and `release.yml` |
| DL-TEST-12 | resource lifecycle tests | hw | dispose tests (engine, viewport/canvas, GPU buffers destroyed in `Renderer::dispose`); 20-cycle open/close leak check (`tests/e2e/bench/files.bench.ts`, DL-PERF-7) |
| DL-TEST-13 | native Windows/Linux/macOS; Chromium/Firefox/WebKit; WebGPU and WebGL2 selected separately; real Safari smoke | ci | `ci.yml`: Rust tests on Linux (Vulkan lavapipe), Playwright Chromium/Firefox/WebKit suites on Linux; `compat.yml`: Rust tests on Windows and macOS, MSRV, WebKit on macOS, released browsers through WebDriver (Chrome 154/153, Edge 154/153, Firefox 157/156 on Linux; Safari 26.6.1 on macOS) with each backend selected explicitly; reference device: Chrome 153/154 and Edge 154 WebGPU+WebGL2 (`docs/compat/2026-10-02/`); support table in `guide/platforms.md`. Safari 27 (current major) and Safari WebGPU on Mac hardware are not verified (no device; issue #13) |
| DL-TEST-14 | released Chrome, Edge and Firefox: current and previous major verified separately from engine-family builds | ci | `tests/e2e/webdriver/{install,smoke}.mjs` (vendor downloads, WebDriver, each backend selected explicitly); `compat.yml` real-browser jobs: Chrome 154.0.8037.92 and 153.0.8010.52, Edge 154.0.4258.53 and 153.0.4234.48, Firefox 157.0 and 156.0.1 (Linux); reference device Chrome 154.0.8037.58/153.0.8010.52 and Edge 154.0.4258.48 with WebGPU (`docs/compat/2026-10-02/`); `guide/platforms.md` |
| DL-TEST-15 | released Safari: current and previous major | ext | Safari 26.6.1 (previous major) verified by the `compat` Safari job; Safari 27 (current major) and Safari WebGPU need a Mac with Safari 27 — none available, hosted runners ship 26.6.x (issue #13) |

## DL-PERF — performance

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-PERF-1 | reference device + method recorded before measuring | hw | `docs/performance.md` (device, browsers, display 1080p@120 Hz, build profile, warm-up and timer rules) |
| DL-PERF-2 | 10k shapes 1080p pan/zoom p95 ≤ 16.7 ms | hw | presented-frame p95 8.4 ms, 0 dropped (Chrome/Edge WebGPU+WebGL2, Firefox WebGPU+WebGL2); input→GPU-done p95 ≤ 9.4 ms (Chromium); `tests/e2e/bench/render.bench.ts`, `tests/e2e/harness/bench.ts`; `docs/performance.md` |
| DL-PERF-3 | hit-test/snap p95 ≤ 8 ms on the same scene | hw | worker round trip p95 ≤ 1 ms, mean ≤ 0.09 ms; `render.bench.ts`; `docs/performance.md` |
| DL-PERF-4 | 200-variable geometric corpus p95 ≤ 50 ms, hard constraints satisfied | hw | gated corpus p95 23.2 ms via SDK/WASM, 0 violations; `scripts/bench/solver-corpus.mjs`, `scripts/bench/solver.mjs`, `crates/engine/examples/solver_corpus.rs`; solver tests `far_preference_on_a_circle_converges_in_few_iterations`, `bent_chain_typed_end_position_is_exact_and_fast`; ADR-0004 |
| DL-PERF-5 | open 10k-object `.dotl` ≤ 2 s (excl. WASM init) | hw | median 101–187 ms, max 188 ms; `tests/e2e/bench/files.bench.ts`; `docs/performance.md` |
| DL-PERF-6 | 100k stress: open/navigate without crash, real cancel | hw | open 1.1–1.3 s, 120 frames without error, 16 000-variable solve cancelled (86–435 ms), revision unchanged; `files.bench.ts` |
| DL-PERF-7 | repeated open/close: no sustained leak after warm-up | hw | 20 cycles: heap growth ≤ +0.7 % (Chromium, after `gc()`), renderer WASM memory constant (all); Firefox heap not measurable; `files.bench.ts` |
| DL-PERF-8 | WASM size, cold start, glyph cache, peak file memory reported | hw | `docs/performance.md` (raw/gzip/brotli, cold 239–423 ms, atlas 4+4 MiB, peaks 10k/100k); `scripts/bench/wasm-size.mjs`, `crates/wasm/examples/memory_profile.rs`, `Viewport.memoryStats()`, `engine.memory()` |

## DL-CI — repository, Actions, deployment

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-CI-1 | public repo TNYCL/dotloom | ci | https://github.com/TNYCL/dotloom (public) |
| DL-CI-2 | same command entry points locally and in CI; Windows guide | ci | root scripts and `scripts/check.mjs` used by CI (`check.mjs rust`, `check.mjs ts`, `pnpm test`, `pnpm build`, `build-wasm.mjs`, `build-site.mjs`, `smoke-packages.mjs`, `smoke-crates.mjs`, `cargo test --workspace --all-features`, `npx playwright test`); Windows section in CONTRIBUTING (MSVC, PowerShell environment variables, GPU tests, WebKit limits) |
| DL-CI-3 | `ci.yml`, `compat.yml`, `nightly.yml`, `pages.yml`, `release.yml` | ci | `.github/workflows/{ci,compat,nightly,pages,release}.yml` |
| DL-CI-4 | timeouts, caches keyed by toolchain/platform/lockfile, concurrency cancel, limited retention | ci | every job has `timeout-minutes`; every artifact has `retention-days` (including the Pages artifact); `Swatinem/rust-cache` (rustc version, OS, `Cargo.lock`) and the `setup-node` pnpm cache; `concurrency` cancels superseded runs (ci, compat, nightly; pages and release never cancel) |
| DL-CI-5 | main protected by required checks; actual API result reported | ci | branch protection on `main` read back from the API (21 required checks incl. the six released-browser jobs, strict, admins included, linear history, no force pushes or deletions, conversation resolution); `status.md` |
| DL-CI-6 | fork PRs without secrets; no `pull_request_target` on contributor code; read-only default token; actions pinned to SHAs; dependency updates | ci | no `pull_request_target`; `permissions: contents: read` at the top of every workflow, write permissions only in the deploy/release jobs; all actions pinned to full SHAs; `persist-credentials: false`; secrets used only in release publish steps; Dependabot for Actions, Cargo (root, fuzz) and npm (root, external plugin) |

## DL-OSS — license, docs, packaging, stable API

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| DL-OSS-1 | MIT + Apache-2.0 texts, SPDX metadata, third-party notices, asset rights | ci | `LICENSE-MIT`, `LICENSE-APACHE` (root, every published crate, checked by `pnpm run check`, and both npm packages); SPDX `MIT OR Apache-2.0` in every manifest; `THIRD-PARTY-NOTICES.md` generated from `cargo metadata` (checked for freshness) and shipped in `@dotloomjs/sdk` and the CLI archives; Inter font under OFL-1.1 (`crates/render/assets/OFL.txt`); cargo-deny license allow-list (nightly) |
| DL-OSS-2 | docs: README, getting started, API reference, plugin guide, constraints model, file format spec, format matrix, platform support, performance, migration, troubleshooting, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, CHANGELOG, templates | ci | README, `guide/` (getting started, architecture, documents, constraints, tools, plugins, react, storage, file format, formats matrix, platforms, performance, versioning and migration, troubleshooting, CLI, examples, API), TypeDoc and rustdoc on the site, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, CHANGELOG, issue/PR templates; site built and checked on every PR and deployed by `pages.yml` |
| DL-OSS-3 | public examples built/tested; badges linked to real workflows | ci | examples built and started in a browser on every PR (`site.spec.ts`), vanilla UI interaction test, example plugins tested with Vitest; README badges link to `ci.yml`, `compat.yml`, `pages.yml` |
| DL-OSS-4 | namespace ownership verified; import paths defined in one place | ext | crates.io: the eight `dotloom-*` crates are owned by the project since their 1.1.0 publish. npm: the scope `@dotloomjs` must be created by the owner (registry: 'Scope not found'). Names are recorded in `packages/names.json` and checked against every manifest and import by `pnpm run check` (ADR-0008) |
| DL-OSS-5 | cargo package / npm pack tested in clean consumers | ci | see DL-TEST-11; the published `v1.1.0` release was checked from its public URLs (`scripts/verify-release.mjs`, also a `release.yml` job), and a fresh Cargo project builds and runs against the crates downloaded from crates.io |
| DL-OSS-6 | compatibility manifest (engine/SDK lockstep, schema/protocol independent) | ci | `compatibility.json` (engine/SDK lockstep version, protocol, render protocol, schema, format version) checked by `crates/wasm/tests/compatibility.rs` and `packages/sdk/test/compatibility.test.ts`; `guide/versioning.md` |
| DL-OSS-7 | SemVer stable release; npm/crates.io/GitHub Release | ext | SemVer stable releases `v1.0.0` (GitHub only) and `v1.1.0` (`release.yml` run 36995447580 from `main@ef0f517`: verified tag, CLI archives for Linux/Windows/macOS, crates, npm tarballs, `SHA256SUMS`, `release-manifest.json`, verified from its public URLs). **crates.io: all eight `dotloom-*` crates 1.1.0 are published** (completed by the publish mode, run 36998435514, after crates.io's new-crate rate limit) and a fresh project builds and runs against them from crates.io. **npm: not published** — the registry answers 'Scope not found' for `@dotloomjs` (the owner's npm organization does not exist yet; issue #10) |
