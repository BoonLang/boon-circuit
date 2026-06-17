# TASK-0804 Root-Flush Resolution Plan

## Purpose

This file is the resumption plan for `TASK-0804A` and `TASK-0804B`. It turns
the remaining NovyWave interaction-speed blocker into a controlled sequence of
measurements, correctness contracts, and implementation slices.

The current problem is not renderer upload, BYTES payload storage, LIST
representation, JSON report writing, bridge file IO, or full row
materialization. The remaining slow path is CPU runtime root-flush fanout from
cursor-class changes. A real `selected_timeline_cursor_value` change updates
`cursor_position` and then wakes bridge/page roots plus visible selected-row
list views. The bad click class remains `194` dependent visits, `32`
dependent enqueues, and `38` dirty pops.

This plan keeps `TASK-0804A` as the historical postponed investigation and uses
`TASK-0804B` as the future resumption umbrella. Do not unpostpone `TASK-0804A`;
an explicit user resume activates `TASK-0804B` only.

## Current Evidence To Preserve

- Latest checklist state: `TASK-1001` and `TASK-1002` are done; `TASK-0804A`
  is postponed historical evidence; `TASK-0804B` is postponed future
  resumption work.
- Latest canonical speed refresh still misses the strict `16.700ms`
  click/input budget, with `click_to_cursor.p95` and `input_to_visible.p95`
  around `18ms` in the latest recorded refresh.
- The report still names
  `root_flush_dirty_scheduler_plus_root_list_materialization`.
- Slow clicks are the cursor-class crossing path, not false-positive no-op
  cursor samples. Fast classes have about `26` or `28` dependent visits; slow
  samples remain `194/32/38`.
- Root-list materialization is already field-only for the hot NovyWave rows:
  `full_eval_row_count=0` and `row_materialize_ms=0`.
- Hot visible list work remains `selected_signal_lane_rows` and
  `selected_cursor_pair_rows`; this work is real and must not be blindly
  skipped.
- Hot internal bridge/page roots include
  `bridge_request_descriptor`, `bridge_cursor_values_page_ref`,
  `bridge_cursor_values`, `bridge_cursor_values_label`,
  `bridge_waveform_page(_ref)`, `bridge_signal_page(_ref)`,
  `bridge_hierarchy_page(_ref)`, and `bridge_file_stats(_ref)`.
- Renderer upload is not the current measured culprit: post-interaction upload
  remains around `3360` bytes, with zero staging wraps and zero quad-cache
  evictions in the recorded probe.
- Previous dead ends must not be retried without new evidence: row-clean
  caches, readiness-set heuristics, direct list-ref alias narrowing,
  structured pure-root deferral bolted onto the dirty queue, reference-only
  list equality, root-value-cache clone deferral, broad same-shape in-place
  updates, persistent function caches, per-step `List/find_value` memoization,
  and container swaps.

## Resolution Strategy

The remaining work should start with correctness and identity, then remove
fanout, then optimize the residual visible list work.

1. Lock the current baseline and add no-behavior counters before changing
   scheduling behavior.
2. Define a currentness contract so deferred internal roots cannot be read
   stale by evaluators, summaries, assertions, or row/list-view evaluation.
3. Split stable bridge/page identity from cursor-hot telemetry where the
   current model makes pure internal roots change only because labels,
   page refs, or request descriptors carry volatile UI state.
4. Add a generic demand/currentness frontier before dirty enqueue for roots
   that are proven safe to keep demand-current instead of eager-current.
5. Only after the `194/32/38` graph shape moves, optimize the residual
   field-only list-view eval/diff path.

The default order is identity-contract work before aggressive demand deferral.
After `0804R-01`, use the decision table below instead of judgment:

| Evidence from `0804R-01` | Next step |
| --- | --- |
| Candidate roots are mostly value-changing, would hide semantic deltas, carry bridge identity, or feed visible row/list fields. | Run `0804R-03` before `0804R-04`. |
| At least 80% of simulated-suppressed candidate enqueues are `currentness_only`, first demanded through audited barriers, and would not hide semantic deltas. | `0804R-04` may run before `0804R-03`. |
| The diagnostic cannot report first demand context, read kind, changed keys, semantic-delta impact, and root-list visibility impact. | Stop and improve diagnostics before implementation. |

Meaningful movement is defined relative to the `0804R-00` canonical baseline:
either strict click/input p95 passes under `16.700ms`, or a named p95/root-list
bucket improves by at least `10%` and `1.0ms` with no click/input p95
regression greater than `0.5ms` or `5%`. Graph movement means the slow
`194/32/38` class occurrence count drops by at least `25%`, or p95 dependent
visits/enqueues/pops drop by at least `10%` with minimum absolute reductions
of `10` visits, `3` enqueues, and `3` pops.

## Plan-Local Status Rules

These plan-local tasks are not directly picked by the master checklist until
`TASK-0804B` is explicitly unpostponed.

Status values:

- `blocked`: cannot start while `TASK-0804B` is postponed.
- `pending`: ready after `TASK-0804B` is unpostponed.
- `in_progress`: actively being implemented.
- `done`: acceptance and verification passed.
- `killed`: reverted or intentionally stopped by kill criteria.
- `superseded`: replaced by another task ID.

All `0804R-*` tasks inherit the `TASK-0804B` source plans unless a task
explicitly narrows them.

Activation protocol:

1. Keep `TASK-0804A` postponed.
2. Change `TASK-0804B` to `in_progress` in the master checklist.
3. Change `0804R-00` to `pending` or `in_progress` in this file.
4. Keep later `0804R-*` tasks `blocked` until their dependencies are done.
5. Append matching progress-log entries in both files.

## 0804R-00 Baseline And Evidence Lock

Status: done
Depends on: `TASK-1001`, `TASK-1002`, explicit user resume of `TASK-0804B`

Goal: capture a current, non-stale baseline before any implementation slice.

Implementation requirements:

- Run the canonical speed gate with profiling env vars explicitly cleared.
- Run one root-demand diagnostic and one dirty-frontier diagnostic.
- Extract click/input p95s, root-flush p95s, graph counts, top root work,
  top list roots, renderer upload counters, and bridge scenario status.
- Record the current git commit/worktree status and report fingerprints in the
  checklist progress log.

Verification commands:

```bash
git status --short
env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
BOON_PROFILE_ROOT_DEMAND=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-root-demand.json
BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-dirty-frontier.json
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
cargo xtask verify-report-schema
jq -e '.runtime_dirty_frontier_cause_summary.click.demand_classification_counts | length > 0' target/diagnostics/native-gpu/novywave-interaction-speed-root-demand.json
jq -e '.runtime_dirty_frontier_cause_summary.click.top_frontier_edges | length > 0' target/diagnostics/native-gpu/novywave-interaction-speed-dirty-frontier.json
```

Acceptance:

- The canonical report either reproduces the root-flush/list-view slow path or
  documents a new dominant cause.
- The diagnostic reports expose `candidate_unobserved_source_free_pure`,
  observed/list-view blockers, and per-click graph counts.
- Renderer upload counters remain separated from click/input timing.
- Because `cargo xtask verify-report-schema` scans `target/reports`, not
  `target/diagnostics`, every diagnostic report required by this plan has an
  explicit `jq -e` field check.

Kill criteria:

- If the current report no longer points at root flush or selected-row list
  work, stop implementation and write a replacement diagnosis before editing
  runtime code.
- If the reports are stale or have mismatched worktree fingerprints, rerun
  them before drawing conclusions.

## 0804R-01 No-Behavior Candidate-Demand Simulation

Status: in_progress
Depends on: `0804R-00`

Goal: measure whether candidate internal roots can be deferred safely before
actually changing dirty enqueue behavior.

Implementation requirements:

- Add env-gated diagnostics only; do not change runtime semantics.
- Simulate suppressing candidate unobserved source-free pure root enqueues.
- Count which simulated-suppressed roots are demanded later in the same
  interaction and in which context: evaluator, state summary, document/window
  summary, sparse-value summary, assertion, root-list evaluation, or observed
  projection.
- Track a simulated-suppressed set and exclude the root's own eager dirty-pop
  materialization from "later demanded" counts.
- Report for each top candidate: first demand context, demand read kind,
  changed read keys, whether an eager semantic delta would have been hidden,
  and whether a visible root-list field depended on it.
- Split candidate roots by:
  - currentness-only propagation.
  - must-publish semantic delta.
  - stable bridge/page identity roots.
  - cursor-hot telemetry roots.
  - roots that feed visible row/list fields.
- Keep canonical reports free of heavy diagnostics unless explicitly enabled.

Verification commands:

```bash
cargo fmt -p boon_runtime -p boon_native_playground -p xtask
cargo check -p boon_runtime -p boon_native_playground -p xtask
env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
BOON_PROFILE_ROOT_DEMAND=1 BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json
cargo xtask verify-report-schema
jq -e '.runtime_dirty_frontier_cause_summary.click.candidate_defer_probe.scope == "no_behavior_change_simulated_suppression_for_candidate_unobserved_source_free_pure"' target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json
jq -e '.runtime_dirty_frontier_cause_summary.click.candidate_defer_probe.top_roots | length > 0' target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json
```

Acceptance:

- The diagnostic report says how many candidate roots would have been skipped,
  how many were later demanded, how many changed, first demand context, demand
  read kind, changed read keys, hidden-semantic-delta impact, and root-list
  visibility impact.
- The report distinguishes bridge/page identity churn from visible cursor/list
  work.
- Canonical click/input p95 does not regress by more than `0.5ms` or `5%`
  relative to `0804R-00`.

Kill criteria:

- Revert or keep strictly diagnostic-only if the counters add measurable
  overhead to the canonical report.
- Do not implement deferral from this task alone if demanded candidate roots
  are mostly value-changing and evaluator-demanded; in that case proceed to
  the bridge/page identity split first.

## 0804R-02 Currentness And Stale-Read Contract

Status: blocked
Depends on: `0804R-01`

Goal: define and test the correctness contract required before any root can be
kept demand-current instead of eager-current.

Implementation requirements:

- Identify every read path that can observe a deferred root:
  `eval_identifier`, `eval_path`, `root_derived_boon_value`,
  runtime state summaries, sparse summaries, document/window summaries,
  assertions, root-list materialization, and bridge proof queries.
- Add a read-path table in this file or the checklist mapping every root read
  API to exactly one of:
  `ensure_root_current` barrier, eager-only/non-deferred exemption, or
  impossible/unreachable with test evidence.
- Introduce or identify a single audited `ensure_root_current`-style API.
  Every direct scalar/cache/root-derived read path must either call it or be
  listed as an explicit non-deferred exemption.
- Add or update focused tests proving that deferred internal roots are
  queryable, current when demanded, and do not publish render patches unless
  observed or semantically required.
- Deferred roots must clear or mark stale stored scalar/cache entries so a
  later read cannot accidentally see a previous value.
- Observed roots, semantic deltas, assertions, bridge stale-response guards,
  and visible row/list fields remain eager or are protected by an explicit
  currentness barrier.

Verification commands:

```bash
cargo fmt -p boon_runtime
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_derived_ -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib structured_root_ -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture
cargo check -p boon_runtime
```

Acceptance:

- There is a documented helper or invariant for "ensure this root is current
  before returning it to evaluator/summary/assertion code".
- The read-path table covers every direct root scalar/cache/summary read
  identified in implementation.
- Tests fail if a deferred pure root can be read stale.
- Tests fail if an observed root or semantic delta is hidden by deferral.

Kill criteria:

- Stop and redesign if the runtime has too many untracked read paths to make
  stale reads auditable.
- Do not proceed to demand deferral until this contract exists.

## 0804R-03 Bridge/Page Identity Split

Status: blocked
Depends on: `0804R-01`, `0804R-02`

Goal: separate stable bridge/page/blob/request identity from cursor-hot
telemetry so cursor movement does not make broad bridge/page roots look
semantically changed unless the underlying bridge input really changed.

Implementation requirements:

- Keep Boon syntax unchanged.
- Do not hardcode NovyWave filenames, fixture rows, or example-specific bridge
  branches.
- Treat labels, UI cursor text, row-local page refs, debug descriptors, and
  telemetry as volatile presentation data unless the bridge payload semantics
  require them.
- Split identity into three layers:
  artifact/page payload identity, real request input key, and
  presentation/freshness metadata.
- Keep artifact/page payload identity deterministic across replay:
  bridge schema version, file/blob identity, page kind, signal/scope, zoom/pan
  or requested range, and payload digest must remain explicit.
- Keep response generation and response/request fingerprint as stale-response
  guards. Do not silently fold them into stable payload identity.
- Cursor-value requests may include cursor/range information only where that
  cursor is a real bridge input, not where it is just a display label or row
  annotation.
- Stale response rejection must remain at least as strict as today.
- BYTES sidecars remain the payload path for real VCD/GHW bytes; do not move
  payload data back into text summaries.

Verification commands:

```bash
cargo fmt -p boon_runtime -p boon_bridge -p boon_native_playground -p xtask
cargo test -p boon_bridge --lib -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_derived_ -- --nocapture
cargo check -p boon_runtime -p boon_bridge -p boon_native_playground -p xtask
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
cargo xtask verify-report-schema
```

Acceptance:

- Bridge/page request identity is deterministic across replay.
- Stale responses are still rejected.
- Focused tests cover cursor-only, label-only, pan/zoom, stale-response, and
  replay-determinism cases.
- Cursor-hot UI telemetry can change without perturbing stable file/page/blob
  identity unless real bridge inputs changed.
- The canonical speed report satisfies the meaningful-movement threshold from
  the Resolution Strategy section or passes the strict click/input p95 budget.

Kill criteria:

- Revert if bridge proof coverage drops, stale response rejection weakens, or
  public bridge hashes change without a deliberate schema/version migration.
- Revert if the speed report does not move the graph class or named buckets
  and the change adds bridge/API complexity.

## 0804R-04 Demand/Currentness Frontier Before Dirty Enqueue

Status: blocked
Depends on: `0804R-02`; run after `0804R-03` unless the `0804R-01` decision
table selects demand-frontier-first

Goal: avoid eager dirty enqueue/materialization for internal roots that only
need to become current when demanded.

Implementation requirements:

- Implement a generic runtime/compiler frontier, not a NovyWave branch.
- Classify candidate roots before enqueue into:
  `currentness_only`, `must_publish_semantic_delta`, `bridge_identity`,
  `cursor_telemetry`, and `visible_list_dependency`.
- Suppress eager enqueue only for `currentness_only` candidates. Other classes
  remain eager unless their own tests prove a barrier-safe replacement.
- Candidate roots may be skipped only if their values are protected by the
  `0804R-02` currentness contract.
- A later demand read must call the currentness barrier and refresh the root
  exactly once for that interaction/generation.
- Dirty read keys for observed/list dependents must still propagate.
- Root-list visible rows must not see stale page refs or cursor values.
- Keep diagnostic counters for skipped, demanded, refreshed, changed, and
  published roots.

Verification commands:

```bash
cargo fmt -p boon_runtime -p boon_native_playground -p xtask
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_derived_ -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib structured_root_ -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture
cargo check -p boon_runtime -p boon_native_playground -p xtask
BOON_PROFILE_ROOT_DEMAND=1 BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-demand-frontier.json
env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
cargo xtask verify-report-schema
jq -e '.runtime_dirty_frontier_cause_summary.click.candidate_defer_probe.root_count >= 0' target/diagnostics/native-gpu/novywave-interaction-speed-demand-frontier.json
```

Acceptance:

- Only `currentness_only` candidate enqueue count drops. Semantic-delta,
  bridge-identity, cursor-telemetry, and visible-list candidates remain eager
  unless separately proven barrier-safe.
- The slow `194/32/38` graph class satisfies the graph-movement threshold from
  the Resolution Strategy section, disappears, or is no longer the top p95
  cause with evidence.
- Click/input p95 passes the strict budget or root-flush buckets drop enough
  to justify a follow-up list-view slice.
- No stale evaluator/summary/assertion read is possible for deferred roots.

Kill criteria:

- Revert if `194/32/38` remains unchanged and no named bucket improves.
- Revert if work moves into layout/list materialization and final p95 regresses.
- Revert if correctness requires hiding semantic deltas or weakening observed
  root behavior.

## 0804R-05 Root List-View Field Frontier

Status: blocked
Depends on: one completed graph/identity slice: `0804R-03` or `0804R-04`;
that slice must satisfy the strict p95 budget or the meaningful-movement
threshold from the Resolution Strategy section.

Goal: after graph fanout moves, reduce the remaining visible row/list
field-only eval/diff cost for `selected_signal_lane_rows` and
`selected_cursor_pair_rows` through generic LIST/root-list infrastructure.

Implementation requirements:

- Do not start this before at least one graph/identity slice has moved the
  `194/32/38` class or root-flush buckets.
- Preserve current full fallback behavior on list length changes, row identity
  uncertainty, branch selector changes, reordered rows, missing fields, or
  changed row shape.
- Use field-level read keys and current LIST row-index/selection storage modes
  from `TASK-1002`.
- Skip field projection/diff work only when the compiler/runtime can prove the
  row field is clean for the current source row and environment.
- Add tests where a deferred/currentness-protected root feeds
  `selected_signal_lane_rows` or `selected_cursor_pair_rows` and must force the
  correct field-cache miss or currentness refresh.
- Keep user function cache keys semantic and deterministic; do not key on
  hidden NovyWave names.

Verification commands:

```bash
cargo fmt -p boon_runtime
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave -- --nocapture
cargo check -p boon_runtime -p boon_native_playground -p xtask
env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
cargo xtask verify-report-schema
```

Acceptance:

- Hot list counters move: `selected_signal_lane_rows.eval_ms`,
  `selected_signal_lane_rows.diff_ms`,
  `selected_cursor_pair_rows.eval_ms`, or
  `selected_cursor_pair_rows.diff_ms` satisfy the meaningful-movement
  threshold from the Resolution Strategy section.
- `full_eval_row_count` stays `0` and `row_materialize_ms` stays `0.0` in the
  click aggregate unless the log records an expected fallback case.
- Final click/input p95 passes, or the remaining blocker is documented with a
  new task and fresh evidence.

Kill criteria:

- Revert if field cache misses stay unchanged and p95 does not improve.
- Revert if a row/list optimization breaks nested row fields, row refs,
  list selections, source rebinding, or bridge scenario proof.

## 0804R-06 Final Verification And Closeout

Status: blocked
Depends on: `0804R-03`, `0804R-04`, `0804R-05` closeout matrix below

Goal: prove the remaining speed task is genuinely resolved or document the
exact unresolved blocker with no hidden unfinished work.

Verification commands:

```bash
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib
cargo test -p boon_bridge --lib -- --nocapture
cargo check -p boon_runtime -p boon_bridge -p boon_native_playground -p xtask
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
cargo xtask verify-report-schema
git diff --check
```

Closeout matrix:

| Prior task state | Closeout action |
| --- | --- |
| `0804R-03` or `0804R-04` passes strict p95 and bridge proof. | `0804R-05` may be skipped; run final verification and mark `TASK-0804B` done. |
| `0804R-03`/`0804R-04` moves graph/root buckets but p95 still fails and list-view buckets dominate. | Run `0804R-05` before final verification. |
| `0804R-03` and `0804R-04` are both killed or superseded. | Do not run `0804R-05`; create `0804R-07` / `TASK-0804C` replacement diagnosis. |
| `0804R-05` is killed after graph movement. | Create `0804R-07` / `TASK-0804C` with the exact residual blocker and keep `TASK-0804B` postponed or superseded. |

Acceptance:

- Canonical NovyWave interaction-speed gate passes the strict click/input
  budget, or the checklist records a narrow remaining blocker with a new task
  and exact evidence.
- Bridge proof remains pass.
- Renderer upload remains solved and separated from click/input timing.
- `TASK-0804A` and `TASK-0804B` are updated consistently in the master
  checklist: either done, superseded, or still postponed with the remaining
  blocker recorded.

Kill criteria:

- Do not mark either task done merely because one sub-bucket improved.
- Do not claim native-present latency is solved from the current
  interaction-speed gate alone; that gate does not include per-click GPU
  present/readback timing.

## 0804R-07 Replacement Diagnosis / TASK-0804C Draft

Status: blocked
Depends on: `0804R-06` closeout matrix

Goal: reserve a concrete follow-up path if the plan proves the current
root-flush hypothesis wrong or insufficient.

Requirements:

- Add a new `TASK-0804C` entry to the master checklist only if `0804R-06`
  cannot close `TASK-0804B`.
- The new task must include status, dependencies, acceptance, verification,
  rollback/stop condition, exact report paths, and the measured residual
  culprit.
- The new task must say whether `TASK-0804B` is postponed, superseded, or
  blocked by the replacement task.

## Rules For Updating This File

- Append a short dated log under this section whenever any `0804R-*` task is
  started, killed, completed, or superseded.
- Keep each log entry tied to exact commands and report paths.
- If a future slice changes task order, record the reason and the measurement
  that forced the change.
- Never delete killed experiments; keep the reason so later runs do not repeat
  them blindly.
- When implementation resumes, update the master checklist and this file in
  the same change.

## Progress Log

- 2026-06-17: Plan created from the latest `TASK-0804A`/`TASK-0804B`
  checklist evidence. No runtime code changed. Tasks remain inactive until the
  user explicitly resumes `TASK-0804B`.
- 2026-06-17: `TASK-0804B` explicitly resumed by `/goal` objective. Activated
  `0804R-00` baseline/evidence lock only; `TASK-0804A` remains postponed and
  later `0804R-*` tasks remain blocked until dependencies are satisfied.
- 2026-06-17: Completed `0804R-00` baseline/evidence lock. Commands:
  `git status --short`;
  `env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`;
  `BOON_PROFILE_ROOT_DEMAND=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-root-demand.json`;
  `BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-dirty-frontier.json`;
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`; and the two required diagnostic
  `jq -e` checks. The first schema run caught a stale role-artifact hash after
  diagnostics rewrote the shared artifact; rerunning the canonical speed report
  before schema fixed the freshness mismatch.
- 2026-06-17: `0804R-00` baseline evidence: canonical speed report
  `target/reports/native-gpu/novywave-interaction-speed.json` remains
  `status=fail` with `click_to_cursor.p95=18.995379ms`,
  `input_to_visible.p95=18.995379ms`, `runtime_apply.p95=11.735027ms`,
  `runtime_step_apply.p95=9.513186ms`, and
  `layout_rebuild.p95=4.709477ms`. Cause remains
  `root_flush_dirty_scheduler_plus_root_list_materialization`.
  Click graph counts are `visits=3536`, `enqueues=600`, `pops=792`;
  aggregate click root work is `root_flush_ms=124.119652`,
  `dirty_scheduler_ms=46.682550`, and
  `root_materialization_ms=73.578293`.
- 2026-06-17: `0804R-00` diagnostics: root-demand report
  `target/diagnostics/native-gpu/novywave-interaction-speed-root-demand.json`
  exposes `candidate_unobserved_source_free_pure` with `24` candidate roots,
  `552` simulated defer enqueues, `552` changed materializations, and `512`
  demand reads. The largest candidate is
  `store.bridge_cursor_values_page_ref` with `64` simulated enqueues and `64`
  demand reads. Demand classes are
  `candidate_unobserved_source_free_pure=3568 visits/576 enqueues`,
  `blocked_observed_downstream=304/96`, `blocked_observed_root=240/80`, and
  `blocked_list_view=704/64`. Dirty-frontier report
  `target/diagnostics/native-gpu/novywave-interaction-speed-dirty-frontier.json`
  exposes ranked frontier edges. Bridge proof
  `target/reports/novywave-bridge-scenario.json` is `status=pass`,
  `measurement_mode=proof`.
- 2026-06-17: `0804R-00` renderer/list evidence: renderer remains separated
  from the current slow path with post-interaction upload `3360` bytes, `3`
  dirty ranges, `3` queue writes, `0` staging wraps, and `0` quad-cache
  evictions. Dominant list remains `selected_signal_lane_rows` with
  `eval_ms=20.513292`, `user_function_body_ms=12.373484`,
  `full_eval_row_count=0`, and `row_materialize_ms=0.0`. Started `0804R-01`
  after baseline acceptance passed; no runtime behavior changed yet.
