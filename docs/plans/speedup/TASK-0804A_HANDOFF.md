# TASK-0804A Handoff: Cells Demand-Driven Speed Blocker

Date: 2026-06-25
Updated: 2026-06-28

Status: historically this was unfinished and explicitly postponed. As of the
2026-06-27 refresh, the focused Cells runtime/currentness/input gates now pass
on fresh reports. This is still not a default-switch completion claim and does
not replace the full native GPU handoff gate set.

2026-06-28 manual observation update: do not treat the 2026-06-27 focused
passes as proof that visible Cells selection is fully fixed. A fresh human
observation reports that clicking/focusing a cell may reveal the cell-local
formula/value state correctly, while the main formula text input above the
grid remains visually stale. That symptom points more specifically at
`store.selected_input` currentness, bound text-input sync, or retained
formula-bar patching than at raw mouse delivery. Future Cells verification must
measure this exact visible transition: click a cell whose formula differs from
the previous selected cell, then prove both runtime
`store.selected_input.editing_text` and the formula-bar text-input pixels/bound
text update to the expected formula within the interaction budget.

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
  `native_input_overlay_recovery_count=0`, and zero unbounded click-to-present outliers, but
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
- `simple_source_click_count=32`, `native_input_overlay_recovery_count=0`.
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
- `simple_source_click_count=32`, `native_input_overlay_recovery_count=0`.
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
- `simple_source_click_count=32`, `native_input_overlay_recovery_count=0`.
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
- `simple_source_click_count=32`, `native_input_overlay_recovery_count=0`.
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
- `simple_source_click_count=32`, `native_input_overlay_recovery_count=0`.
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
  `native_input_overlay_recovery_count=0`,
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

## 2026-06-27 Native Render Architecture Follow-Up

Implemented a generic WGPU retained-quad draw-call coalescing pass in
`boon_native_gpu`. The previous retained renderer already reused retained chunk
uploads, but still submitted one draw per retained quad batch. On Cells visible
selection changes that meant roughly `331` retained chunks and `552` draw calls
for a one-cell/formula-bar update even though only two retained chunks missed.

The new renderer keeps the existing per-retained-chunk upload/cache identity but
merges adjacent same-texture, same-upload-ring-generation byte ranges into one
draw range. This is not a Cells branch and does not change Boon semantics.

Fresh evidence:

- `cargo test -p boon_native_gpu coalesced_quad_draw_ranges_merge_only_adjacent_compatible_batches`
  passed.
- `cargo fmt --all -- --check` passed.
- `cargo build -q -p xtask` passed with existing native GPU dead-code warnings.
- `target/debug/xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  passed with `target_count=64`, `simple_source_click_count=64`,
  `native_input_overlay_recovery_count=0`, `input_wake_to_formula_visible_ms_p95=16.67486799999915`,
  `click_to_formula_visible_ms_p95=28.815178`, one bounded driver-to-wake
  outlier, and zero unbounded outliers.
- The visible-click render-loop proof reports `draw_calls=9`,
  `retained_chunk_count=331`, `retained_chunk_hit_count=329`,
  `retained_chunk_miss_count=2`, and
  `render_hook_to_queue_ms=0.12736299999960465`.
- `target/debug/xtask verify-native-gpu-headed-scenario --example cells --report target/reports/native-gpu/headed-scenario-cells.json`
  passed.
- `target/debug/xtask verify-native-cells-interaction-speed --profile release --report target/reports/native-gpu/cells-interaction-speed-release.json`
  passed after the headed child refresh.
- `target/debug/xtask verify-wgpu-retained-arenas --report target/reports/native-gpu/wgpu-retained-arenas.json`
  passed after refreshing the native/browser world-scene children.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json target/reports/native-gpu/cells-interaction-speed-release.json target/reports/native-gpu/wgpu-retained-arenas.json`
  passed.

Remaining architecture options:

- Keep the input-wake resample idea as the next robustness option. A read-only
  input-path review found a possible press/release sampling boundary in the
  app-window loop: when a release arrives just after input sampling, the loop
  can wait for the next turn. The current p95 passes, but the report still has
  a bounded max outlier, so a generation-aware immediate re-sample remains
  useful if the gate flakes.
- A deeper retained renderer would retain a frame texture or chunk atlas and
  redraw only dirty chunks. The current fix reduces draw-call overhead while
  still clearing/drawing the visible scene every frame.
- Runtime/list/currentness work should not resume unless counters regress:
  latest visible-click evidence still shows targeted patch paths, zero
  `List/find` scans, no full summary scan, and no generic click fallback.

## 2026-06-29 Formula-Bar Visible Click Follow-Up

User observation: focusing a cell appeared to resolve the cell formula, but the
main formula input above the Cells grid could remain visually stale. The old
visible-click report was too easy to trust because it asserted runtime/frame
text and accepted retained-bound sync without forcing the formula-bar text nodes
into the direct render-scene patch.

Implemented fix:

- cached native surface size from resize callbacks to avoid an extra hot-loop
  `size_scale().await` before input polling;
- expanded retained bound text-input sync to include selection-dependent
  formula-bar bindings;
- added retained bound text-update nodes to the native input-overlay
  render-scene patch so the cached base scene is reused but formula-bar
  text/address chunks are replaced.

Fresh focused evidence:

- `cargo fmt --all`: pass.
- Focused `boon_native_playground` tests for retained content revision,
  focus-only formula-bar sync, and real-window click/formula-bar sync: pass.
- `target/debug/xtask verify-native-cells-visible-click-e2e --profile release --address B0 --expected-formula '=add(A0,A1)' --report target/reports/native-gpu/cells-visible-click-e2e-b0-current.json`:
  pass.
- The refreshed B0 report has `input_wake_to_formula_visible_ms=15.365523999999825`,
  `click_to_formula_visible_ms=22.013412`, `render_scene_cache_hit=true`,
  `render_scene_cache_ms=0.28293300000000005`,
  `input_overlay_render_scene_patch_touched_node_count=4`, zero runtime
  scans/recompute/root materialization, and formula-bar text
  `=add(A0,A1)`.
- `target/debug/xtask verify-native-cells-visible-click-e2e --profile release --address A2 --expected-formula 15 --report target/reports/native-gpu/cells-visible-click-e2e-a2-current.json`:
  pass.
- The refreshed A2 report has `formula_bar_text="15"`,
  `input_wake_to_formula_visible_ms=14.410942000000432`,
  `click_to_formula_visible_ms=21.336541`, zero runtime
  scans/recompute/root materialization, and `selected_address="A2"`.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-a2-current.json`:
  pass.

Do not mark TASK-0804A complete from this alone. The remaining acceptance still
needs the full multi-target Cells interaction-speed, scroll-speed, preview E2E,
headed, schema aggregate, and 60 FPS report refreshes. Also harden the verifier
with a cropped formula-bar pixel/readback artifact or equivalent app-owned
pixel inventory.

Follow-up status:

- `target/debug/xtask verify-native-gpu-headed-scenario --example cells --report target/reports/native-gpu/headed-scenario-cells.json`:
  pass.
- `target/debug/xtask verify-native-cells-interaction-speed --profile release --report target/reports/native-gpu/cells-interaction-speed-release.json`:
  pass after refreshing the headed child report, with
  `interaction_latency_ms_p95=8.572534999999998`,
  `interaction_latency_ms_max=13.825261`, `logical_cell_count=2600`,
  `rendered_cell_count=210`, `materialized_cell_count_max=240`,
  and `formula_evaluated_cell_count_max=4`.
- `BOON_NATIVE_GPU_PREVIEW_E2E_ISOLATED=1 target/debug/xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`:
  still fails. The run proves real-window wheel delivery, two native child
  windows, installed app-window wheel adapter, and per-window input provenance,
  but the isolated launch proof reports
  `isolated Weston native launch did not prove real-window wheel delivery for this native scroll run`.
- Manual COSMIC workspace launch is separately blocked by
  `org.freedesktop.DBus.Error.ServiceUnknown`. Do not treat that as a Cells
  click/formula-bar runtime failure without a fresh manual launch path.

Update:

- The scroll-speed verifier contract was corrected so the isolated scroll step
  checks wheel-delivery evidence directly instead of inheriting an unrelated
  shared desktop supervisor status failure.
- `BOON_NATIVE_GPU_PREVIEW_E2E_ISOLATED=1 target/debug/xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`:
  pass.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/scroll-speed-cells.json`:
  pass.
- Fresh scroll evidence has `evidence_tier="real-window"`,
  `real_wheel_input=true`, vertical and horizontal real-window wheel input both
  true, no per-step failures, and the nested isolated shared-launch proof still
  honestly records its unrelated `status="fail"` while exposing
  `driver_pass=true`, `desktop_pass=true`, `measured_loop_pass=true`,
  `real_os_events_observed=true`, `driver_effect_observed=true`, and
  `wheel_events=4`.

## 2026-06-30 Cells Formula-Bar Headed Proof and Speed Refresh

Status: refreshed the exact user-observed Cells click/focus symptom in the
native headed and interaction-speed gates. The top formula input above the
Cells grid is now covered by app-owned native events plus WGPU/readback
evidence; the final headed C0 click asserts both runtime
`/store/selected_input/editing_text` and the retained formula-bar text node as
`=sum(A0:A2)`.

Implemented fix:

- Added a configurable headed-scenario timeout to the native preview role and
  xtask wrapper. Cells uses a longer runner timeout/hold because all steps were
  already passing but the old fixed 12s runner timeout could fail at the final
  formula-bar assertion.
- Prefer the already-current `store.selected_input` summary when refreshing
  focused Cells text instead of recursively scanning visible list summaries
  first. This keeps the generic selected-input/currentness path hot without
  adding a Cells Boon workaround or shrinking the grid.

Fresh evidence:

- `RUSTFLAGS='-Awarnings' cargo check -q -p xtask -p boon_native_playground`:
  pass.
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path -- --nocapture`:
  pass.
- `RUSTFLAGS='-Awarnings' cargo test -q -p boon_native_playground cells_focus_only_route_syncs_formula_bar_text -- --nocapture`:
  pass.
- `RUSTFLAGS='-Awarnings' cargo run -q -p xtask -- verify-native-gpu-headed-scenario --example cells --report target/reports/native-gpu/headed-scenario-cells.json`:
  pass with `visual_capture_method="app-owned-wgpu-readback-with-visible-cursor-overlay"`,
  `cursor_visible=true`, `key_hud_visible=true`, scenario timeout `20000ms`,
  and final formula-bar retained text `=sum(A0:A2)`.
- `RUSTFLAGS='-Awarnings' cargo run -q -p xtask -- verify-native-cells-interaction-speed --profile release --report target/reports/native-gpu/cells-interaction-speed-release.json`:
  pass with `requested_event_count=64`, `interaction_latency_ms_p95=13.40666`,
  `interaction_latency_ms_max=16.604776`, `workflow_coverage_pass=true`,
  `selection_formula_full_layout_count=0`, `logical_cell_count=2600`,
  `materialized_cell_count_max=240`, `rendered_cell_count=210`,
  `formula_evaluated_cell_count_max=4`, and
  `formula_recomputed_field_count_max=6`.
- `RUSTFLAGS='-Awarnings' cargo run -q -p xtask -- verify-report-schema target/reports/native-gpu/headed-scenario-cells.json target/reports/native-gpu/cells-interaction-speed-release.json`:
  pass.

Remaining caution:

- This clears the current native headed formula-bar proof and 64-event release
  interaction-speed report, but the larger unified goal still needs the broader
  native GPU aggregate/readiness refreshes and the non-Cells BYTES/MachinePlan
  phases. Do not treat a manual desktop observation as evidence unless it is
  backed by a fresh app-owned report.

## 2026-06-30 Focused B0 Correctness Restored, Budget Still Misses

Latest focused B0 work restored the proof boundary after the manual formula-bar
and selected-cell concerns:

- The top formula input and grid selected paint now both pass in app-owned WGPU
  readback for B0.
- The verifier now refreshes pre-click selected address/formula text after
  calibration/preposition, so it no longer compares a B0 click against stale
  initial A0 metadata when the immediate pre-click selected cell is C3.
- The render-scene patch now preserves per-node draw order and no longer uses
  the broad 243-visible-cell replacement path for ordinary selected-state
  changes. Latest focused report has
  `input_overlay_render_scene_patch_touched_node_count=5` and
  `input_overlay_render_scene_patch_build_ms=0.321596`.

Fresh focused command:

- `target/debug/xtask verify-native-cells-visible-click-e2e --profile release --address B0 --expected-formula '=add(A0,A1)' --report target/reports/native-gpu/cells-visible-click-e2e-b0-selected-current.json`

Latest status:

- Still fail, but only on the strict timing budget:
  `input_wake_to_formula_visible_ms=18.68868500000008 > 16.7`.
- Correctness fields pass:
  `sample_status="pass"`, `visual_formula_probe.status="pass"`,
  `selected_cell_visual_pass=true`, selected address `B0`, formula text
  `=add(A0,A1)`.
- Remaining timing shape:
  native input `total_ms=7.122748`,
  `selection_proxy_refresh_ms=4.33797`,
  render hook `total_before_report_json_ms=2.439897`,
  `present_call_ms=8.124693`.

Next return should focus on native scheduling and indexed retained selection
state. Do not reintroduce the broad visible selectable-node render patch unless
previous/current selected identity is unavailable; that path fixed correctness
but made the patch cost too high.

Follow-up update:

- Retained selection state is now indexed enough for the focused B0 hot path.
  Latest report has native input `total_ms=2.430272`,
  `selection_proxy_refresh_ms=0.324137`,
  `selected_overlay_patch_ms=0.32217`,
  `selection_proxy_text_refresh_ms=0.001298`, and
  `selection_focus_overlay_state_ms=0.0005589999999999999`.
- Render hook remains bounded:
  `input_overlay_render_scene_patch_build_ms=0.313894`,
  `total_before_report_json_ms=2.53656`.
- The focused gate still fails on timing:
  `input_wake_to_formula_visible_ms=26.32301499999994 > 16.7`.
- Remaining timing shape now points at native loop scheduling/present:
  `input_wake_to_dirty_poll_ms=14.202577000000929`,
  `present_call_ms=9.149435`.

Next return should inspect native app-window input generation attribution and
dirty-poll scheduling before doing more retained UI micro-optimizations.

## 2026-06-30 Layout-Derived Formula-Bar Proof and Offscreen Timing Probe

Follow-up tightened the focused B0 visible-click verifier instead of trusting
hardcoded crop coordinates:

- `cells_visual_formula_probe_from_readback` now derives the top formula-bar
  address label and text-input crop bounds from the app-owned layout artifact.
  Latest proof resolves the formula input as `x=80,y=8,width=832,height=30`.
- The formula-bar visual proof still passes with app-owned WGPU readback and
  selected-cell crop proof; the main text input above the grid is therefore
  covered by the report, not just retained-frame metadata.
- Focused unit/schema checks passed:
  `cargo test -q -p xtask cells_visual_formula_probe_requires_expected_formula_bar_text_value -- --nocapture`,
  `cargo test -q -p boon_native_app_window requested_animation_can_repaint_existing_scheduler_only_content -- --nocapture`,
  and `cargo xtask verify-report-schema` for the new reports.

Architecture probe:

- Enabling the existing `app-owned-offscreen-copy-to-present` path can pass the
  focused B0 gate in one run:
  `input_wake_to_formula_visible_ms=15.412932999999612`,
  `input_wake_to_dirty_poll_ms=3.1144829999993817`, and
  `render_target_kind="app-owned-offscreen-copy-to-present"`.
- Making that path the demand-driven Preview default is not sufficient by
  itself. A fresh default run still failed with
  `input_wake_to_formula_visible_ms=29.39201099999991`,
  `input_wake_to_dirty_poll_ms=13.959326999999575`, and
  `present_call_ms=12.364903`, even though the report confirmed
  `render_target_kind="app-owned-offscreen-copy-to-present"` and the visual
  formula-bar proof passed.

Current conclusion:

- Do not spend the next iteration on more Cells formula/runtime micro-tuning for
  this focused B0 miss. Runtime/source work is around 2.5-2.7ms and retained
  render-patch work is bounded. The remaining miss is in native scheduling and
  surface presentation timing: wake-to-poll can still consume most of a frame,
  and `frame.present()` can block for another 9-12ms.
- Next return should either make app-window input wake preempt the sleeping/
  presenting loop more directly, or split render preparation/presentation so a
  late input event can be sampled and committed before the next visible present.

## 2026-06-30 Immediate Present Mode and Stale Readback Skip

Focused B0 work moved the remaining miss from Cells formula/runtime into the
native app-window presentation policy and fixed the current single-click gate.

What changed:

- `low_latency_present_mode` now prefers non-vsync modes before Mailbox:
  `Immediate`, then `AutoNoVsync`, then `Mailbox`, then `Fifo`.
- Demand-driven Preview still uses the app-owned offscreen copy-to-present path
  when the surface supports `COPY_DST`.
- The native loop now records `post_present_stale_readback_skip` and skips
  finishing interactive readback for a frame if a newer input generation arrived
  during/after present. The report exposes
  `last_interactive_surface_readback_skipped_for_stale_input` so this is honest
  diagnostic behavior, not hidden proof loss.

Evidence:

- `cargo check -q -p boon_native_app_window`: pass.
- `cargo test -q -p boon_native_app_window present_mode -- --nocapture`: pass.
- `cargo test -q -p boon_native_app_window input_resample_counters_distinguish_inline_and_deferred_turns -- --nocapture`:
  pass.
- Failed comparison before present-mode reorder:
  `target/reports/native-gpu/cells-visible-click-e2e-b0-post-present-skip.json`
  had correct visual/runtime proof but failed timing with
  `input_wake_to_formula_visible_ms=20.466770000000903`,
  `present_mode="Mailbox"`, `present_call_ms=14.461043`, and
  `queue_to_present_ms=14.462157000000843`.
- Passing focused report:
  `target/reports/native-gpu/cells-visible-click-e2e-b0-present-mode.json`
  has `status=pass`, selected address `B0`, formula text `=add(A0,A1)`,
  `input_wake_to_formula_visible_ms=15.465542000000823`,
  `click_to_formula_visible_ms=21.444083`, `present_mode="Immediate"`,
  `present_call_ms=9.673265`, `queue_to_present_ms=9.673957000000884`,
  `input_wake_to_poll_started_ms=0.28535199999987526`, and
  `input_wake_to_dirty_poll_ms=2.9513510000006136`.
- The same report proves the top formula text input crop changed at
  `x=80,y=8,width=832,height=30` with app-owned WGPU readback, and
  `verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-b0-present-mode.json`
  passes.

Remaining caution:

- This fixes the focused B0 click/formula-bar gate in one fresh run. It does not
  complete TASK-0804A by itself. The broader Cells 60 FPS verifier, scroll gate,
  native GPU aggregate, and BYTES/MachinePlan default-path gates still need a
  fresh sweep before claiming the whole task or unified goal is complete.
- Manual testing should use the latest release playground binary. A stale
  visible playground can still contradict the app-owned report if it was
  launched before these native-loop changes.

## 2026-06-30 Broader Cells Gate Refresh and Manual Formula-Bar Caution

Fresh child reports now unblock the demand-driven aggregate, but manual testing
still raised a formula-bar concern that must stay visible.

Evidence:

- `target/debug/xtask verify-native-gpu-headed-scenario --example cells --report target/reports/native-gpu/headed-scenario-cells.json`:
  pass.
- `target/debug/xtask verify-native-cells-interaction-speed --profile release --report target/reports/native-gpu/cells-interaction-speed-release.json`:
  pass with `interaction_latency_ms_p95=9.128491`,
  `interaction_latency_ms_max=14.988123000000002`,
  `selection_formula_full_layout_count=0`, `logical_cell_count=2600`,
  `materialized_cell_count_max=240`, `rendered_cell_count=210`,
  `formula_evaluated_cell_count_max=4`, and
  `formula_recomputed_field_count_max=6`.
- `target/debug/xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`:
  pass, but the isolated run used a software Vulkan adapter. The report keeps
  `scroll_frame_ms_p95=19.23486`, `wall_clock_frame_budget_pass=false`, and
  `software_adapter_wall_clock_budget_exempt=true`; this is not production GPU
  wall-clock proof.
- Refreshed `verify-native-gpu-idle-wake` reports for counter, todomvc, cells,
  and custom projects all pass.
- `target/debug/xtask verify-demand-driven-render-loop --check-existing --report target/reports/native-gpu/demand-driven-render-loop.json`:
  pass after the idle-wake child report refresh.
- `cargo xtask verify-report-schema` passes for the refreshed headed scenario,
  Cells interaction-speed, Cells scroll-speed, idle-wake children, and
  demand-driven aggregate reports.

Manual caution:

- The app-owned readback for the focused B0 run visibly shows the top formula
  input updated to `=add(A0,A1)` and the selected address updated to `B0`.
  However, manual testing reported that focusing a cell appears to reveal the
  formula somewhere while the main text input above the grid does not visibly
  change. Treat that as an unresolved manual/live-playground mismatch until a
  freshly launched release playground reproduces the same app-owned frame path
  and the verifier includes stronger visible text/OCR-style proof for the
  formula input, not just retained text metadata plus crop-hash changes.

## 2026-06-30 Retained Binding Sync Replaces Formula-Bar Geometry Patch

The latest native playground slice removes the remaining coordinate-based
formula-bar address mutation from the focus/render overlay path. Formula-bar
address and formula input updates now come from the retained document binding
sync, and the render hook uses the retained bound-sync node list as WGPU
render-scene patch targets. This keeps the selected-cell/formula-bar path on the
generic retained binding architecture instead of Cells-specific geometry.

Evidence:

- `cargo fmt --package boon_native_playground`: pass.
- `cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path -- --nocapture`:
  pass. The test now asserts that the formula-bar text input node is included in
  retained text update nodes, so the direct WGPU render-scene patch replaces the
  old glyphs instead of only changing runtime or retained frame state.
- `cargo test -q -p boon_native_playground cells_input_overlay_render_scene_patch_updates_stale_selected_cell_primitives -- --nocapture`:
  pass.
- `target/debug/xtask verify-native-cells-visible-click-e2e --profile release --address B0 --expected-formula '=add(A0,A1)' --report target/reports/native-gpu/cells-visible-click-e2e-b0-current.json`:
  expected fail on timing only. Runtime state and app-owned visible proof pass:
  selected address `B0`, formula-bar text `=add(A0,A1)`,
  `visual_formula_probe.status=pass`, formula input crop changed, selected-cell
  crop changed, `runtime_work_contract.status=pass`, and
  `retained_update_contract.status=pass`.
- `target/debug/xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-b0-current.json`:
  pass.

Remaining blocker:

- The focused visible-click report still fails the strict 60 FPS timing budget:
  `input_wake_to_formula_visible_ms_p95=17.582675999999992` against the
  `16.7ms` budget, and `click_to_formula_visible_ms_p95=24.012844`. The report
  points away from formula/runtime work: the measured native input click path is
  about `2.505137ms`, the render hook is about `2.665924ms`, runtime scans and
  recomputes are zero, while `input_wake_to_dirty_poll_ms` is about `3.232466ms`
  and `present_call_ms` is about `11.336246ms`. The next TASK-0804A work should
  target wake scheduling and present/copy-to-present latency rather than another
  Cells Boon workaround.
