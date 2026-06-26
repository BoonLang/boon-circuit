# TASK-0804A Handoff: Cells Demand-Driven Speed Blocker

Date: 2026-06-25

Status: unfinished and explicitly postponed. This is not a pass, not a default
switch unblocker, and not evidence that Cells is fast enough.

This file exists so another AI can start from the real evidence instead of
repeating the same blind debugging loop.

## Short Summary

TASK-0804A is the Cells native demand-driven performance blocker. The strict
gate is `verify-native-gpu-idle-wake --example cells`, rolled into
`verify-demand-driven-render-loop` and then into
`verify-unified-architecture-all`.

The task should stay postponed until other independent unified tasks are done,
or until someone is ready to implement a real engine/runtime architecture fix:
compiled/indexed dependency scheduling, formula dependency tracking, and
virtualized row materialization. Small verifier, JSON, route-lookup, WGPU, or
proof-publication tweaks have already been explored heavily and are unlikely to
finish the task.

## Current Gate State

Primary reports:

- `target/reports/native-gpu/idle-wake-cells.json`
- `target/reports/native-gpu/idle-wake-custom-projects.json`
- `target/reports/native-gpu/demand-driven-render-loop.json`
- `target/reports/unified/unified-architecture-all.json`

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

## One-Sentence Warning

If the next attempt does not reduce the eager `cells.value` / indexed
reset-source startup work or replace it with a proven dependency scheduler, it
is probably not solving TASK-0804A.
