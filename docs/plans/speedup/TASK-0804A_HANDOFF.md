# TASK-0804A Handoff: Cells Demand-Driven Speed Blocker

Date: 2026-06-25
Updated: 2026-06-27

Status: historically this was unfinished and explicitly postponed. As of the
2026-06-27 refresh, the focused Cells runtime/currentness/input gates now pass
on fresh reports. This is still not a default-switch completion claim and does
not replace the full native GPU handoff gate set.

This file exists so another AI can start from the real evidence instead of
repeating the same blind debugging loop.

## Short Summary

TASK-0804A is the Cells native demand-driven performance blocker. The strict
gate is `verify-native-gpu-idle-wake --example cells`, rolled into
`verify-demand-driven-render-loop` and then into
`verify-unified-architecture-all`.

The original handoff root cause was correct: the fix had to be generic runtime
architecture work rather than a Cells-level workaround. Current evidence shows
the practical runtime pieces are now in place: indexed `List/find`, demand-
current `cells.value` / `cells.error`, currentness barriers for selected reads,
formula fanout/range dependency updates, cycle safety, and bounded
materialization. Future work should preserve those contracts and focus on any
fresh native scroll/frame/present evidence that still misses the full handoff
target.

## Current Gate State

Primary reports:

- `target/reports/native-gpu/idle-wake-cells.json`
- `target/reports/native-gpu/idle-wake-custom-projects.json`
- `target/reports/native-gpu/demand-driven-render-loop.json`
- `target/reports/unified/unified-architecture-all.json`

Fresh 2026-06-27 focused Cells state:

- `verify-native-cells-interaction-speed --profile release` passes with
  `interaction_latency_ms_p95=9.087524`,
  `interaction_latency_ms_max=15.308088`, `logical_cell_count=2600`,
  `materialized_cell_count_max=240`, `rendered_cell_count=210`,
  `formula_evaluated_cell_count_max=4`, and
  `formula_recomputed_field_count_max=6`.
- The latest local `verify-native-cells-visible-click-e2e --profile release`
  rerun selects the expected cells and exposes the expected formula/value text
  for `64` real OS click targets, with `simple_source_click_count=64`,
  `generic_fallback_count=0`, and zero unbounded click-to-present outliers, but
  still narrowly fails the strict full input-wake budget:
  `input_wake_to_formula_visible_ms_p95=16.99711600000228` against `16.7ms`.
- `verify-native-gpu-idle-wake --example cells --idle-ms 1500` passes with
  `post_idle_input_to_present_ms=35.344134`.
- `verify-demand-driven-render-loop --check-existing` passes after refreshing
  its four idle-wake children.
- The latest `verify-unified-architecture-all --check-existing` no longer
  fails because of demand-driven/Cells status. It fails because 17 previously
  passing child reports have stale `xtask` `binary_hash` values after the local
  rebuild.

Do not use the stale 2026-06-25 paragraphs below as current failure evidence
without rerunning the reports. They remain useful as historical root-cause
context and as a warning against returning to renderer/report micro-fixes.

Current unified aggregate state after the latest U10 refresh:

- `target/reports/unified/unified-architecture-all.json` is still `fail`.
- `checked_report_count=18`.
- `passed_report_count=17`.
- `failed_report_count=1`.
- `schema_valid_report_count=17`.
- The only unified aggregate blocker is
  `target/reports/native-gpu/demand-driven-render-loop.json`.

The demand-driven aggregate may show stale child-report fingerprints after any
new `xtask` or worktree change. Do not interpret stale report noise as a new
root cause. Regenerate the child reports before using demand-driven aggregate
details as current evidence.

## Commands

Focused Cells gate:

```bash
cargo build -q -p xtask
target/debug/xtask verify-native-gpu-idle-wake --example cells --idle-ms 1500 --report target/reports/native-gpu/idle-wake-cells.json
target/debug/xtask verify-report-schema target/reports/native-gpu/idle-wake-cells.json
```

Demand-driven aggregate:

```bash
target/debug/xtask verify-native-gpu-idle-wake --example counter --idle-ms 1500 --report target/reports/native-gpu/idle-wake-counter.json
target/debug/xtask verify-native-gpu-idle-wake --example todomvc --idle-ms 1500 --report target/reports/native-gpu/idle-wake-todomvc.json
target/debug/xtask verify-native-gpu-idle-wake --example cells --idle-ms 1500 --report target/reports/native-gpu/idle-wake-cells.json
target/debug/xtask verify-native-gpu-idle-wake --custom-project-fixture target/fixtures/native-gpu/custom-projects.json --idle-ms 1500 --report target/reports/native-gpu/idle-wake-custom-projects.json
target/debug/xtask verify-demand-driven-render-loop --check-existing --report target/reports/native-gpu/demand-driven-render-loop.json
target/debug/xtask verify-report-schema target/reports/native-gpu/demand-driven-render-loop.json
```

Deep Cells startup/runtime-state probe:

```bash
cargo build -q -p boon_native_playground
/usr/bin/time -f 'elapsed=%E cpu=%P maxrss_kb=%M exit=%x' timeout 60s \
  target/debug/boon_native_playground \
  --role layout-proof \
  --code-file examples/cells.bn \
  --report target/artifacts/native-gpu/layout-proof-cells-root-cause.json
```

## Accepted Root-Cause Evidence

The most important accepted measurement is the Cells layout-proof root-cause
run after fixing the report-generation hash bottleneck:

- Command:
  `timeout 60s target/debug/boon_native_playground --role layout-proof --code-file examples/cells.bn --report target/artifacts/native-gpu/layout-proof-cells-root-cause.json`
- Result: `status=pass`.
- Wall time: about `40.78s`.
- CPU: about `99%`.
- Max RSS: about `118612 KB`.
- `runtime_init_ms=37309.590100999994`.
- `runtime_document_summary_ms=24.035957`.
- `generic_total_ms=37189.763702000004`.
- `initialize_generic_derived_ms=32192.184642999997`.
- `initialize_indexed_reset_sources_ms=4924.46496`.
- Dominant field: `cells.value`, `count=2600`,
  `total_ms=28408.06217599999`, `avg_ms=10.926177759999996`,
  `changed_read_count=5196`.
- Reset-source initialization recomputes `cells.address` for 2600 rows
  (`4694.196477ms`) and `cells.default_formula` for 2600 rows
  (`230.192856ms`).

Interpretation:

Cells creates 2600 rows. Each row has derived `address`, `default_formula`,
`value`, and `error`. Runtime startup eagerly enumerates all startup recompute
keys, roughly `2600 * 4 = 10400`, and evaluates them. Formula values call
`cell_value()`, which uses `List/find(cells, field: address, value:
target_address)`. Startup therefore behaves like a full spreadsheet
interpreter pass with list lookup and dependency bookkeeping.

This is engine/runtime work. It is not WGPU, parser, layout, JSON report
writing, or simple route lookup.

## Cells Interaction Evidence

Earlier Cells click evidence initially looked like a full document relower
problem:

- `post_idle_input_to_present_ms=959.196572`, budget `120ms`.
- `layout_ms=667.005312`, `total_ms=736.874921`.
- `runtime_ms=11.634521`, `route_ms=19.077434`,
  `before_summary_ms=10.872884`.
- Frame method:
  `render-patch-state-delta-and-full-runtime-backed-layout-recompute`.
- `document_eval_lower_ms=321.441103` for `347` retained entries.

That specific full-relower problem was partially fixed. Later Cells evidence
showed:

- Frame method changed to
  `render-patch-state-delta-and-paint-space-patch`.
- `document_eval_lower_ms=0.0`.
- `layout_frame_clone_ms=0.0`.
- `retained_layout_frame_reuse_without_clone=true`.
- `render_scene_patch_applied=true`.
- `render_scene_patch_operation_count=1`.
- `render_scene_patch_rejection=null`.
- End-to-end input latency improved to the 150-260ms range in later runs, but
  still missed the `120ms` budget.

Useful late accepted Cells measurement:

- `post_idle_input_to_present_ms=156.066706`.
- `post_idle_source_replace_to_present_ms=198.703886`.
- Preview idle CPU about `0.6654%`.
- Combined idle CPU about `1.3307%`.
- Compact ack `round_trip_ms=82`.
- Stage timings:
  `window_ms=0.139593`,
  `before_summary_ms=0.001849`,
  `route_ms=0.059589`,
  `runtime_ms=11.441561`,
  `layout_ms=36.011677`,
  `total_ms=47.6962`.

Interpretation:

Route lookup is no longer the active Cells click bottleneck. Full document
relower and layout-frame clone are no longer the active Cells click bottleneck.
The remaining interaction gap is stacked runtime/formula/list cost,
retained-layout/patch-proof cost, and native render/present/readback tail.

## Idle CPU Evidence

Idle CPU was a real blocker for a while. It is no longer the primary active
Cells problem after the accessibility publish cache.

Before the cache:

- Render/event thread passive idle polling dominated.
- `hook_poll` average about `7130.394us`.
- Last idle `hook_poll` sample about `15897us`.
- Preview idle CPU around `8-10%`.

After retaining the accessibility host cache:

- Idle `hook_poll` average improved to about `284.457us`.
- Last idle `hook_poll` sample improved to about `489us`.
- Preview idle CPU around `0.6624%`.
- The remaining blocker became post-idle input-to-present latency.

Do not spend the next TASK-0804A attempt on idle CPU unless a fresh report
shows it regressed.

## Killed Or Non-Finishing Attempts

Do not repeat these without new evidence:

- Full document relower optimization for Cells selection after reports show
  `document_eval_lower_ms=0.0`.
- Blind click-route/report/layout microchanges.
- Verifier wait tweaks as a substitute for engine work.
- JSON/report hashing work as the main explanation. The 527 MiB debug binary
  hashing issue was fixed by streaming/external `sha256sum` use while keeping
  exact SHA-256 validation.
- Naive lazy summary/startup materialization. It proved that normal row-field
  reads can recompute generic derived indexed fields on demand, but coupling
  document summary serialization back into recomputation was too fragile and
  caused a default-stack `cells_scenario_runs_and_detects_cycle` stack overflow.
- Ad hoc `PASSIVE_INPUT_POLL_INTERVAL` increases. They may reduce CPU but risk
  real input lag and no longer target the dominant Cells blocker.

## Kept Improvements

These changes were useful and should not be casually reverted:

- Equality-style retained patching for Cells selected state.
- Render-scene retag sidecar for identity-only selected-style state.
- Compact operator-host input ack path for verifier hot-path response size.
- Idle poll substep instrumentation.
- Preview accessibility host cache.
- Narrow preview operator-host snapshot.
- Source-event node hint fast path.
- Direct node/source route proof.
- Verifier progress and wait hardening.
- Layout-proof progress markers and exact streaming hash fix.

## Required Future Direction

The next serious TASK-0804A attempt should start with engine architecture, not
micro-optimizations:

1. Add compiled per-list dependency metadata over dense row slots.
2. Track formula dependencies explicitly: direct cell address dependencies,
   range dependencies, and invalidation from `A0`-style references.
3. Replace `List/find(cells, field: address, value: target_address)` hot lookup
   with an indexed address-to-row path when the compiler can prove the list and
   key field.
4. Initialize only fields required by indexed hold resets, visible windows,
   active formulas, or explicit runtime bridge requirements.
5. Avoid eager startup recompute of all `cells.value` and `cells.error` rows.
6. Replace reset-source ordered-prefix scans with compiled indexed reset-source
   initialization.
7. Lazily or dependency-drive `cells.value`, but do it in runtime/engine
   machinery with cycle safety, not in document summary serialization.
8. Batch startup cache invalidation so global lookup caches are not cleared per
   row mutation.
9. Keep default-stack cycle tests passing.
10. Only after startup/runtime-state is fixed, return to the remaining
    interaction path: retained layout/patch proof and native render/present
    tail.

## Candidate Acceptance Criteria

Do not mark TASK-0804A complete until all of these are true on fresh reports:

- `target/debug/xtask verify-native-gpu-idle-wake --example cells --idle-ms 1500 --report target/reports/native-gpu/idle-wake-cells.json` passes.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/idle-wake-cells.json` passes.
- `target/debug/xtask verify-demand-driven-render-loop --check-existing --report target/reports/native-gpu/demand-driven-render-loop.json` passes after all children are regenerated.
- `target/debug/xtask verify-unified-architecture-all --check-existing --report target/reports/unified/unified-architecture-all.json` no longer fails because of demand-driven render loop.
- Cells startup/layout-proof no longer spends tens of seconds in
  `initialize_generic_derived_ms` / `cells.value`.
- The default-stack Cells cycle scenario remains safe.
- No Cells-specific hardcoded shortcut, fixture reduction, example-name branch,
  source-text branch, or hidden fallback was added.

## Good Files To Read First

- `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
  around the 2026-06-25 TASK-0804A entries.
- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`
  around `TASK-0804A`, `TASK-0804B/C`, and the Cells layout-proof root-cause
  measurement.
- `docs/architecture/LIST_MODEL.md`.
- `docs/architecture/RUNTIME_MODEL.md`.
- `docs/architecture/DELTA_PROTOCOL.md`.
- `docs/architecture/NATIVE_GPU_PIPELINE.md`.
- `crates/boon_runtime/src/lib.rs`.
- `crates/boon_native_playground/src/main.rs`.
- `examples/cells.bn`.

## 2026-06-26 Manual Cells Input Regression Note

After a release playground launch of `boon_native_playground --role desktop
--example cells`, manual user testing still reported that Cells do not react to
mouse clicks at all. The focused Cells tests and release interaction-speed
verifier may pass, but that is not sufficient evidence that the visible
playground input path works. Treat the real native preview click path,
app-window coordinate provenance, and Cells source-event routing as still
unresolved until a fresh app-owned native proof matches manual behavior.

## 2026-06-27 Manual Cells Regression Still Active

Manual user testing again reported that Cells still does not react to mouse
clicks. Keep this as an open manual-behavior blocker separate from the current
non-Cells unified-goal slices: the next TASK-0804A return must first prove the
real visible native click path, selected-cell visual state, and formula/input
sync with app-owned host events and WGPU/readback evidence.

## 2026-06-27 Real Click Follow-Up

Fresh app-owned native evidence now proves the visible-click route works in the
isolated Weston harness, but it still does not meet the 60 FPS budget:

- `verify-native-cells-visible-click-e2e --profile release` still fails.
- All 32 measured samples select the expected cell and expose the expected
  formula/value text.
- `simple_source_click_count=32`, `generic_fallback_count=0`.
- The verifier now keeps surface-readback hashes and external render-proof
  hashes in separate domains and requires a newer native input wake count, so a
  stale frame/proof cannot satisfy the current click.
- The speed gate uses visible-surface WGPU readback for proof and skips
  offscreen render-hook readback work. The hot report path is compacted:
  `report_json_ms` is about `0.03ms`.
- Current failing timings are still well above budget:
  `click_to_formula_visible_ms_p95=66.327663`,
  `click_to_present_ms_p95=60.388395`,
  `input_wake_to_formula_visible_ms_p95=53.36054`, and
  `input_wake_to_present_ms_p95=48.725027`.

Interpretation:

- For the visible click path, the active blocker is no longer `List/find`,
  formula evaluation, generic fallback routing, or proof JSON construction.
- The first measured states repeatedly miss the render-scene cache and spend
  about `27-28ms` lowering/render-scene-building selected/focus/hover variants.
- Warm cache-hit frames still spend roughly `18-22ms` from native input wake to
  present, mostly stacked input poll delay, small retained render work, and
  queue-to-present pacing.
- The next real fix should make selected/focus/hover/caret state a retained
  render-scene patch or GPU overlay instead of a distinct full render-scene
  cache key. After that, reduce native input wake-to-dirty-poll and
  queue-to-present tail. Do not try to pass this by weakening proof domains or
  hiding cold samples.

## One-Sentence Warning

If the next attempt does not either reduce remaining retained render-scene
misses/present tail or preserve the already-added demand-current/indexed Cells
runtime work, it is probably not solving TASK-0804A.

## 2026-06-27 Retained Input Overlay Patch Follow-Up

Implemented a generic retained render-scene sidecar for native input overlay
state. Focus/selected/hover/caret changes now build a
`RenderScenePatchOperation::ReplaceNodeEntries` patch for touched nodes and
apply it to an unoverlaid cached base scene. This is not Cells-specific and does
not change Boon source semantics.

Fresh release evidence:

- `cargo fmt --check` passes.
- `cargo test -q -p boon_native_playground input_overlay_render_scene_patch_matches_full_overlay_lowering`
  passes.
- `cargo check -p boon_native_playground -p xtask -p boon_native_app_window`
  passes with existing warnings.
- `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  still fails.

Latest report facts:

- `target_count=32`, `timing_sample_count_complete=true`.
- `render_scene_patch_source=native_input_overlay` for 131 render-hook samples.
- `input_overlay_patch_built=22`, `render_scene_cache_misses=22`,
  `render_scene_cache_hits=109`.
- Patch construction is no longer the tail after retaining the text column
  measurer: `input_overlay_render_scene_patch_build_ms p95=0.239567`.
- Render-hook work is mostly under frame budget now:
  `total_before_report_json_ms p50=1.796412`, `p95=13.033252`, `max=22.969747`.
- E2E timing still fails:
  `click_to_formula_visible_ms_p95=46.517619`,
  `click_to_present_ms_p95=42.348184`,
  `input_wake_to_formula_visible_ms_p95=39.491071999999534`,
  `input_wake_to_present_ms_p95=31.402876999999535`.

Interpretation:

- The next blocker is no longer full render-scene lowering for every click and
  no longer runtime `List/find`/formula work.
- Remaining work is native scheduling and proof tail: input wake-to-dirty poll,
  frame/present pacing, and visible readback/formula-visible observation still
  push p95 well above 16.7ms even when the render hook itself is usually below
  budget.

## 2026-06-27 Overlay Lookup / Touched-Only Patch Follow-Up

Implemented the next retained-render reduction for native input overlay state.
The render hook now extracts a small selected-address overlay lookup from the
layout proof and no longer deep-clones the full layout proof for every render.
First-time focus/selected/hover/caret patches are built from touched display
items only, then checked against full overlay lowering by
`input_overlay_render_scene_patch_matches_full_overlay_lowering`.

Fresh evidence:

- `cargo fmt --check` passes.
- `cargo test -q -p boon_native_playground input_overlay_render_scene_patch_matches_full_overlay_lowering`
  passes.
- `verify-native-cells-visible-click-e2e --profile release` still fails.

Latest report facts:

- `click_to_formula_visible_ms_p95=43.134555`.
- `click_to_present_ms_p95=35.381584000000004`.
- `input_wake_to_formula_visible_ms_p95=35.677989000000736`.
- `input_wake_to_present_ms_p95=27.739979999999377`.
- First-time overlay `render_frame_cache_ms` is down to roughly
  `0.008-0.017ms`.
- First-time overlay `render_scene_cache_ms` still costs roughly
  `6.75-10.05ms` because patch composition still clones/applies against a full
  base `RenderScene`.
- Warm render-hook samples are usually around `1.6-1.8ms`, but full
  accessibility snapshot work remains about `2.7-4.9ms` and queue-to-present is
  still about `7.4-10.9ms`.

Next real fix:

- Make WGPU consume retained `RenderPatch`/overlay state directly, or make
  render-scene patch composition avoid full base-scene clones.
- Add semantic/accessibility patch updates rather than rebuilding the whole
  AccessKit tree for every selection/value change.
- Keep the current visible-surface WGPU readback and input-wake proof domains;
  do not pass by hiding cold samples or skipping accessibility correctness.

## 2026-06-27 Direct Document Render-Scene Patch Encode Follow-Up

Implemented the first WGPU-side retained patch-consumption slice. Native GPU now
accepts a base `DocumentRenderScene` plus a
`RenderScenePatchOperation::ReplaceNodeEntries` patch and converts the effective
scene without cloning/applying a full patched document render scene in the
playground hot path. The Cells native input overlay path uses this direct encode
route when the interaction verifier is using visible-surface proof.

Fresh evidence:

- `cargo fmt --check` passes.
- `cargo test -q -p boon_native_gpu document_render_scene_patch_conversion_matches_materialized_apply`
  passes.
- `cargo test -q -p boon_native_playground input_overlay_render_scene_patch_matches_full_overlay_lowering`
  passes.
- `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  still fails.

Latest report facts:

- `target_count=32`, `timing_sample_count_complete=true`.
- All 131 render-hook samples report
  `visible_surface_metrics.render_scene_source=document-render-scene-patch`.
- `input_overlay_render_scene_patch_direct_encode=true` for all 131 render-hook
  samples.
- Render-hook p95 is now low enough that it is no longer the main tail:
  `total_before_report_json_ms p50=4.056361`, `p95=4.315259`, `max=5.014209`.
- Patch/base-scene work is bounded:
  `render_scene_cache_ms p95=2.457225`, `encode_scene_ms p95=1.809274`.
- E2E timing still fails:
  `click_to_formula_visible_ms_p95=38.528216`,
  `click_to_present_ms_p95=34.204679000000006`,
  `input_wake_to_formula_visible_ms_p95=32.03031799999981`,
  `input_wake_to_present_ms_p95=20.480467000000317`.
- Remaining per-click tail includes `input_wake_to_dirty_poll_ms p95=6.456552`,
  poll `total_ms p95=5.444331999999999`,
  accessibility snapshot `p95=3.005728`, and interactive readback
  `p95=3.000009`.

Interpretation:

- The active blocker has moved past render-scene materialization. Continue with
  native event-driven wake/poll latency, retained semantic/accessibility
  patches, visible readback/formula-visible observation, and present pacing.
- Do not re-open runtime `List/find`/formula startup as the click-path blocker
  unless fresh evidence regresses those counters.

## 2026-06-27 Runtime Exact Lookup Dependency / Route-Key Follow-Up

Implemented the useful generic runtime slice from the external review without
hardcoding Cells. `List/find` over a text field now records an exact
`list_lookup_text` read key in addition to the existing broad list/column read
keys, so the runtime can diagnose and later narrow invalidation for
spreadsheet-style address lookups. The broad column key is intentionally still
retained for correctness until old/new lookup-value invalidation is in place.

Also removed the route-cache-key fingerprinting tail for already cached preview
snapshots with embedded source intents. Focus-overlay and scroll guards still
force a fresh key when the cached hit route table is not valid.

Fresh evidence:

- `cargo fmt --check` passes.
- `cargo check -q -p boon_runtime` passes.
- `cargo check -q -p boon_native_playground` passes with existing warnings.
- `cargo test -q -p boon_runtime list_index_find_uses_text_lookup_index`
  passes.
- `cargo test -q -p boon_runtime cells_value_and_error_are_demand_current_at_startup`
  passes.
- `cargo test -q -p boon_runtime pure_boon_cells_fanout_recomputes_from_generic_read_index`
  passes.
- `cargo test -q -p boon_native_playground preview_route_cache_key_uses_snapshot_key_with_embedded_source_intents`
  passes.
- `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  still fails.

Latest release visible-click report:

- `status="fail"`.
- `target_count=32`, `timing_sample_count_complete=true`.
- `simple_source_click_count=32`, `generic_fallback_count=0`.
- Route lookup is no longer a meaningful hot path in this run:
  `route_table_lookup_ms p95=0.0`, max about `0.148ms`.
- Recent simple-source native click handling is under budget:
  native input samples are about `0.4-2.9ms`, with runtime list scans at `0`.
- Runtime currentness is also not the long pole for the measured clicks:
  samples show `click_to_runtime_current_observed_ms` around `3.8ms`.
- End-to-end visual timing still misses:
  `click_to_formula_visible_ms_p95=39.145115`,
  `click_to_present_ms_p95=33.369455`,
  `input_wake_to_formula_visible_ms_p95=30.68494099999896`,
  `input_wake_to_present_ms_p95=19.453144999999495`.

Important failed check:

- `cargo test -q -p boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json`
  still fails on the pre-existing assertion that the typed click route should
  focus the formula bar without proof hit/source JSON. Do not treat the new
  isolated route-key unit test as proof that this older typed-click path is
  fixed.

Current interpretation:

- The external runtime diagnosis was the right architecture direction, but this
  dirty tree already contains the main demand-current/indexed Cells runtime
  work. The new exact lookup dependency key improves observability and provides
  the next correctness-preserving invalidation hook.
- The active TASK-0804A blocker is now visible-present/formula-visible latency,
  not full-grid formula recomputation, unindexed Cells address lookup, route
  fingerprinting, or render-scene materialization.
- Continue with native wake-to-present scheduling, retained semantic/
  accessibility patches, readback/proof latency, and the still-broken
  typed-click formula-bar focus path.

## 2026-06-27 Typed Focus / Readback Freshness Follow-Up

Implemented the next generic visible-click fix. Typed route-table click focus
now counts typed focus state as sufficient evidence for the retained focus
overlay instead of requiring source-intent proof JSON. This closes the older
`preview_hover_and_click_use_typed_route_table_without_proof_hit_json` unit
failure without adding any Cells-specific branch.

The native render-loop report now carries sampled and presented input
generations plus pending surface-readback state. The Cells visible-click
verifier rejects stale frames and pending readbacks for the current native input
generation, and its probes record wake-to-queue, queue-to-present, and
present-to-readback-report timings.

Fresh focused evidence:

- `cargo fmt --check` passes.
- `cargo check -q -p boon_native_app_window` passes.
- `cargo check -q -p xtask` passes with existing native GPU warnings.
- `cargo check -q -p boon_native_playground` passes with existing warnings.
- `cargo test -q -p boon_runtime list_index_find_uses_text_lookup_index`
  passes.
- `cargo test -q -p boon_runtime cells_value_and_error_are_demand_current_at_startup`
  passes.
- `cargo test -q -p boon_runtime pure_boon_cells_fanout_recomputes_from_generic_read_index`
  passes.
- `cargo test -q -p boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json`
  now passes.
- `cargo test -q -p boon_native_playground cells_formula_bar_click_accepts_text_edit`
  passes.
- `cargo test -q -p boon_native_playground input_overlay_render_scene_patch_matches_full_overlay_lowering`
  passes.
- `cargo test -q -p boon_native_gpu document_render_scene_patch_conversion_matches_materialized_apply`
  passes.

Fresh release visible-click report:

- `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  still fails.
- `target_count=32`, `timing_sample_count_complete=true`.
- `simple_source_click_count=32`, `generic_fallback_count=0`.
- The report shows no stale input-generation proofs:
  `stale_for_latest_input=false` for all sampled generation probes.
- Current failing timing:
  `click_to_formula_visible_ms_p95=50.302772`,
  `click_to_formula_visible_ms_max=54.387388`,
  `click_to_present_ms_p95=45.886718`,
  `click_to_present_ms_max=46.516721000000004`,
  `input_wake_to_formula_visible_ms_p95=44.54610000000063`, and
  `input_wake_to_present_ms_p95=35.46416599999975`.
- Render-hook work is not the active p95 blocker:
  `total_with_report_json_ms p95=2.562514`,
  `render_scene_cache_ms p95=0.28221399999999996`,
  `encode_scene_ms p95=2.19228`, and `report_json_ms p95=0.051379`.
- The visible tail is now mostly native scheduling/present/readback:
  `input_wake_to_dirty_poll_ms p95=22.25784300000032`,
  `wake_to_queue_ms p95=26.727210000000017`,
  `queue_to_present_ms p95=9.90306100000089`, and
  `present_to_readback_report_ms p95=4.312976000001072`.

Current interpretation:

- The pasted external review remains right for the larger TASK-0804A engine
  direction, but this dirty tree has already covered the practical runtime
  pieces that were still relevant to the measured click path: indexed
  `List/find`, demand-current Cells startup, and generic formula fanout guards.
- The manual "click a cell and formula bar updates" bug is now covered at
  unit/focused-verifier level, but the full release E2E gate is still not
  60 FPS. Do not claim completion.
- The next high-value slice is not another route/runtime micro-optimization.
  It is native wake scheduling and proof latency: avoid 20ms-class
  wake-to-dirty-poll tails, reduce present pacing, and move toward small-region
  or evented app-owned readback/proof for the formula-bar area.

## 2026-06-27 Generic Projection Index and Wake-Span Attribution Follow-Up

Implemented one remaining useful generic runtime slice from the external review:
document/summary `List/find` projection reads now use the existing text lookup
index instead of `find_list_index_by_textlike` row scans. This affects
`store.selected_input` and any other root list projection lowered as
`List/find(...)`; it is not Cells-specific and does not add a new index type.

Additional runtime guards now prove:

- `cells.value` and `cells.error` stay out of startup recompute.
- reset-source startup initializes `cells.address` and `cells.default_formula`
  through batch fast paths and does not pull in `cells.value` / `cells.error`.
- selected-input document values use the indexed projection path after counters
  are reset immediately before the read.
- formula fanout and range formulas update through the generic read dependency
  index without full-grid recompute.
- default-stack cycle detection still passes.

The native render-loop report and Cells visible-click verifier now carry
sub-spans for wake-to-poll, poll-to-dirty, dirty-to-render, render-hook-to-queue,
queue-to-present, and readback-report latency.

Fresh evidence:

- `cargo fmt --check` passes.
- `cargo test -q -p boon_ir --lib indexed_derived_startup_recompute_is_ir_semantic_not_path_heuristic`
  passes.
- `cargo test -q -p boon_runtime cells_selected_input_document_state_values_use_indexed_list_find_projection`
  passes.
- `cargo test -q -p boon_runtime cells_indexed_reset_sources_use_batch_fast_paths`
  passes.
- `cargo test -q -p boon_runtime cells_value_and_error_are_demand_current_at_startup`
  passes.
- `cargo test -q -p boon_runtime pure_boon_cells_fanout_recomputes_from_generic_read_index`
  passes.
- `cargo test -q -p boon_runtime pure_boon_cells_range_formula_updates_from_member_change`
  passes.
- `cargo test -q -p boon_runtime cells_scenario_runs_and_detects_cycle`
  passes.
- `cargo test -q -p boon_runtime cells_window_document_summary_keeps_selected_projection_current`
  passes.
- `cargo test -q -p boon_runtime root_currentness_barrier` passes.
- `cargo test -q -p boon_native_app_window elapsed_delta_ms_only_reports_forward_time`
  passes.
- `RUSTFLAGS='-Awarnings' cargo check -q -p boon_runtime` passes.
- `RUSTFLAGS='-Awarnings' cargo check -q -p xtask` passes.
- `target/debug/xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  still fails.

Latest release visible-click report:

- `target_count=32`, `timing_sample_count_complete=true`.
- `simple_source_click_count=32`, `generic_fallback_count=0`.
- `click_to_formula_visible_ms_p95=52.666079`,
  `click_to_formula_visible_ms_max=60.145939`.
- `click_to_present_ms_p95=49.247813`,
  `input_wake_to_formula_visible_ms_p95=42.34009900000026`,
  `input_wake_to_present_ms_p95=40.199767000000065`.
- New p95 sub-spans:
  `input_wake_to_poll_started_ms=21.28910899999937`,
  `poll_started_to_dirty_poll_ms=3.837776000000304`,
  `dirty_poll_to_render_started_ms=0.0425070000001142`,
  `surface_acquired_to_render_hook_completed_ms=3.709313999999722`,
  `render_hook_to_queue_ms=0.30691400000068825`,
  `queue_to_present_ms=11.235940999999912`,
  `present_to_readback_report_ms=4.549282999999377`.

Interpretation:

- The remaining strict failure is not runtime formula/list lookup. The first
  long pole is native input wake-to-poll start, followed by present pacing and
  readback-report proof latency.
- Do not retry the reverted motion-only coalescing experiment without new
  evidence; it worsened the gate and did not catch follow-up input generations.
- The next high-value fix is event-loop wake scheduling: make app-window input
  wake the render worker immediately instead of waiting a 16-22ms class interval,
  then reduce queue-to-present and readback proof latency.

## 2026-06-27 External Review Reconciliation and Current Gate State

The pasted external review is accurate for the historical root cause, but this
dirty tree already contains the practical generic runtime pieces it asks for.
Do not restart from runtime `List/find`, demand-current startup, or formula
fanout unless fresh counters regress those paths.

Current verified runtime coverage:

- `List/find` uses the existing text lookup index, and focused tests prove index
  hits plus zero scan rows.
- `cells.value` and `cells.error` stay demand-current at startup.
- `cells.address` and `cells.default_formula` reset-source startup use batch
  fast paths and do not pull in `cells.value` / `cells.error`.
- selected-input document values stay current through the indexed `List/find`
  projection path.
- formula fanout and `sum(A0:A3)` range updates recompute through the generic
  read dependency index without full-grid recompute.
- default-stack cycle detection still passes.
- the typed-route native focus regression that matched "clicking a cell does
  nothing" is covered by
  `preview_hover_and_click_use_typed_route_table_without_proof_hit_json`.

Fresh commands that passed:

- `cargo fmt --check`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime list_index_find_uses_text_lookup_index`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime cells_value_and_error_are_demand_current_at_startup`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime cells_indexed_reset_sources_use_batch_fast_paths`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime pure_boon_cells_fanout_recomputes_from_generic_read_index`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime pure_boon_cells_range_formula_updates_from_member_change`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime cells_selected_input_document_state_values_use_indexed_list_find_projection`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime cells_scenario_runs_and_detects_cycle`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime root_currentness_barrier`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json`
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_native_app_window input_event_wake_elapsed_ms_uses_generation_timeline`

Idle-wake refresh after restoring the passive native input poll fallback to
`100ms`:

- Counter passes: `post_idle_input_to_present_ms=12.815526`,
  `post_idle_source_replace_to_present_ms=11.337169`,
  `combined_idle_cpu_percent_p95=1.3314128405628287`.
- TodoMVC passes: `post_idle_input_to_present_ms=100.456937`,
  `post_idle_source_replace_to_present_ms=32.767068`,
  `combined_idle_cpu_percent_p95=0.6657590666245364`.
- Custom projects pass with the required fixture argument:
  `post_idle_input_to_present_ms=13.09092`,
  `post_idle_source_replace_to_present_ms=11.659951`,
  `combined_idle_cpu_percent_p95=0.6643575458573793`.
- Cells still fails: `post_idle_input_to_present_ms=130.95367399999998`,
  `post_idle_source_replace_to_present_ms=139.718995`,
  `combined_idle_cpu_percent_p95=0.6622233291358749`.
- `verify-demand-driven-render-loop --check-existing` now fails only because
  `target/reports/native-gpu/idle-wake-cells.json` has `status=fail`.

Current conclusion:

- Cells idle CPU is no longer the idle-wake blocker.
- Cells runtime/list/formula startup is not the current measured blocker.
- The remaining smoke-gate failure is Cells native post-idle latency, and the
  stricter TASK-0804A target remains the dedicated 60 FPS visible-click/scroll
  verifier. The next patch should attack native scheduling/proof/layout latency
  for Cells, not Boon-level styling hacks or another generic runtime micro-loop.

## 2026-06-27 Retained Focus Overlay Clone Removal

Implemented the useful native/UI slice from the latest review pass. The Cells
selection-proxy focus path still patches retained layout state for correctness,
but it no longer clones the full `layout_proof` JSON blob for every focused
cell/textinput overlay update. The patch uses field-level borrows inside
`PreviewSharedRenderState`, so the existing retained-frame tests keep proving
formula text and caret correctness while the hot click path avoids the repeated
multi-millisecond proof copy.

Fresh evidence:

- `cargo fmt --check` passes.
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_native_playground cells_formula_cell_focus_uses_formula_text_and_arrow_aliases_move_caret`
  passes.
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_native_playground cells_click_selection_updates_formula_bar_and_selected_style`
  passes.
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json`
  passes.
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime list_index_find_uses_text_lookup_index_for_runtime_list_ref`
  passes.
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime cells_value_and_error_are_demand_current_at_startup`
  passes.
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_runtime pure_boon_cells_fanout_recomputes_from_generic_read_index`
  passes.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`
  passes.
- `target/debug/xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  now passes.

Latest release visible-click report:

- `status="pass"`.
- `target_count=32`, `timing_sample_count_complete=true`.
- `simple_source_click_count=32`, `generic_fallback_count=0`.
- `click_to_formula_visible_ms_p95=26.565813`,
  `click_to_formula_visible_ms_max=27.557629`.
- `input_wake_to_formula_visible_ms_p95=15.85446200000115`,
  `input_wake_to_formula_visible_ms_max=15.94123800000034`.
- The former source-input long pole moved:
  `focus_overlay_ms p95=0.011954`, down from about `4ms` before this patch.
- Other current p95 sub-spans:
  `poll_started_to_dirty_poll_ms=3.627520000000004`,
  `surface_acquired_to_render_hook_completed_ms=3.5450999999993655`,
  `queue_to_present_ms=8.92986000000019`.

Remaining blocker:

- `target/debug/xtask verify-native-gpu-idle-wake --example cells --idle-ms 1500 --report target/reports/native-gpu/idle-wake-cells.json`
  still fails as a separate smoke gate with
  `post_idle_input_to_present_ms=140.93864200000002`,
  `post_idle_source_replace_to_present_ms=139.899255`, and
  `combined_idle_cpu_percent_p95=0.0`.
- Do not report TASK-0804A as fully complete until the idle-wake smoke gate,
  scroll proof gate, and aggregate demand-driven checks are refreshed and pass.

## 2026-06-27 Native Input Provenance / Idle-Wake Recovery

Implemented the useful native-verifier/input fixes from the latest review pass
without adding a Cells-specific shortcut.

What changed:

- The Linux `app_window` mouse path now records the Wayland surface/window id
  on enter and motion events, so mouse provenance is attached to the same
  native window that later receives button or wheel events.
- The Weston human-like test driver now round-trips after click-only and
  async-input button release sequences, so button requests are not lost when the
  helper exits immediately after flushing.
- `verify-native-gpu-idle-wake` now exercises the real native driver path before
  post-idle input proof instead of using the operator-host source-event shortcut
  as the primary click proof.
- The two-window idle-wake probe records the live preview/dev child evidence,
  then terminates only the dev child in isolated Weston before sending the
  preview click. This avoids the headless-overlap case where the dev window
  receives the proof click after the two-window idle sample has already been
  captured.
- The post-idle proof waits for a quiet render-loop span after dev removal
  before taking the click baseline, so the dev-window teardown frame is not
  mistaken for a click response.
- The post-idle native click proof now mirrors the dedicated visible-click
  verifier: move-only pointer preposition, app-owned mouse-position proof,
  quiet wait, then a measured button-only click. The readback baseline is taken
  after preposition, so hover/preposition changes cannot satisfy the click
  proof.

Fresh evidence:

- `cargo fmt --check` passes.
- `RUSTFLAGS='-Awarnings' cargo check -q -p boon_native_app_window -p boon_native_playground`
  passes.
- `cargo build -q -p xtask` passes.
- `target/debug/xtask verify-native-gpu-idle-wake --example cells --idle-ms 1500 --report target/reports/native-gpu/idle-wake-cells.json`
  passes.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/idle-wake-cells.json`
  passes.
- `target/debug/xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  passes.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`
  passes.
- Refreshed `counter`, `todomvc`, `cells`, and fixture-based
  `custom-projects` idle-wake reports are schema-valid.
- `target/debug/xtask verify-demand-driven-render-loop --check-existing --report target/reports/native-gpu/demand-driven-render-loop.json`
  passes.
- `BOON_NATIVE_GPU_PREVIEW_E2E_ISOLATED=1 target/debug/xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`
  passes, and its schema check passes.

Latest report facts:

- Cells idle-wake:
  `post_idle_input_to_present_ms=35.867035`,
  `post_idle_source_replace_to_present_ms=124.486139`, and
  `combined_idle_cpu_percent_p95=1.3290407628518937` on the first successful
  rerun after the preposition/button-only verifier patch. The proof used the
  native Weston button-only driver and did not use
  `fallback_host_event_probe`.
- Release visible-click:
  `target_count=32`, `simple_source_click_count=32`,
  `generic_fallback_count=0`,
  `click_to_formula_visible_ms_p95=25.920653`,
  `click_to_formula_visible_ms_max=26.708242`,
  `input_wake_to_formula_visible_ms_p95=16.390971999999238`, and
  `input_wake_to_formula_visible_ms_max=19.820090999999593`.
- Demand-driven aggregate: `status="pass"`.
- Cells scroll-speed isolated Weston report: `status="pass"`,
  `logical_cell_count=2600`, `materialized_cell_count_max=336`,
  `instance_count_visible=160`, `graph_rebuild_count=0`,
  `layout_rebuild_scope="visible-plus-overscan-delta"`,
  `app_owned_window_vertical_wheel_input=true`, and
  `app_owned_window_horizontal_wheel_input=true`.
- The isolated scroll report still records software-adapter wall-clock timing:
  `scroll_frame_ms_p95=20.967003` and
  `wall_clock_frame_budget_pass=false`; that timing is explicitly marked
  software-adapter budget-exempt in the report and should not be used as the
  production GPU 60 FPS proof.

Current interpretation:

- The manual "Cells click does nothing" path was primarily a native input/
  verifier provenance problem in the current tree, not a fresh proof that the
  runtime `List/find` or formula fanout work had regressed.
- The historical external review remains the right architectural warning:
  preserve the generic indexed lookup, demand-current startup, batch reset
  initialization, currentness barriers, and formula dependency guards. Do not
  reintroduce Boon-level styling hacks or example-name branches.
- TASK-0804A is still not a blanket production claim until the full native GPU
  handoff gate set and a non-exempt production-adapter scroll/interaction proof
  are refreshed. The concrete blocker moved from "Cells ignores clicks" and
  failing idle-wake to finishing production-grade scroll/frame evidence.

## 2026-06-27 External Review Follow-Up

Re-audited the pasted external TASK-0804A review against the dirty tree with an
independent read-only subagent and local focused tests. The six runtime
architecture items from the review are already implemented here: generic
indexed `List/find`, demand-current `cells.value` / `cells.error`, currentness
barriers for selected reads, sparse formula fanout/range invalidation with
cycle safety, batched reset-source initialization, and virtualized/materialized
Cells counters.

Implemented the remaining useful hardening from that review: the
`cells-interaction-speed-*` report check now requires the spreadsheet-style
evidence directly. It fails if `logical_cell_count` drops below `2600`, if
materialized/rendered counts become missing or unbounded, or if sparse formula
evaluation/recompute counters disappear or exceed their caps.

Fresh evidence after this hardening:

- `cargo fmt --all -- --check` passes.
- `RUSTFLAGS='-Awarnings' cargo check -q -p xtask` passes.
- Focused runtime tests for indexed `List/find`, demand-current startup, batch
  reset-source initialization, selected-input currentness, formula fanout,
  range invalidation, cycle safety, and root currentness pass.
- `target/debug/xtask verify-native-gpu-headed-scenario --example cells --report target/reports/native-gpu/headed-scenario-cells.json`
  passes.
- `target/debug/xtask verify-native-cells-interaction-speed --profile release --report target/reports/native-gpu/cells-interaction-speed-release.json`
  passes under the stricter counter gate with `interaction_latency_ms_p95=9.087524`
  and `interaction_latency_ms_max=15.308088`.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/headed-scenario-cells.json target/reports/native-gpu/cells-interaction-speed-release.json`
  passes.

The remaining latest visible-click E2E miss is not the historical runtime bug:
the report shows `list_find_rows_scanned=0`, no generic fallback clicks, runtime
currentness observed in about `4.3ms`, and app-owned visible proof. The p95
overage is a native wake/render/proof tail of roughly `0.3ms` over the strict
`16.7ms` input-wake-to-visible budget.
