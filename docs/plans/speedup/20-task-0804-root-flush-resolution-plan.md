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

- Pre-activation checklist state to preserve: `TASK-1001` and `TASK-1002`
  are done; `TASK-0804A` is postponed historical evidence; `TASK-0804B`
  was postponed future resumption work until the explicit 2026-06-17 `/goal`
  activation. Current progress is tracked in this file and the master
  checklist progress log.
- Audited canonical speed refresh still misses the strict `16.700ms`
  click/input budget. After `0804R-05` correctness-only control and a
  measurement-audit refresh, `click_to_cursor.p95` and
  `input_to_visible.p95` are `18.020024ms`; two immediate canonical repeats
  recorded `18.226649ms` and `18.070569ms`.
- Measurement interpretation must stay precise: this gate measures deterministic
  app-owned input through runtime/layout/shared render-state update. The current
  `input_to_visible` field aliases the click-loop summary and is not
  per-interaction WGPU present/readback proof.
- The report still names
  `root_flush_dirty_scheduler_plus_root_list_materialization`.
- Slow clicks are the cursor-class crossing path, not false-positive no-op
  cursor samples. Before `0804R-03`, slow samples were `194/32/38`; after
  `0804R-05` correctness-only control, p95 graph counts are `75/17/24`.
- Root-list materialization is already field-only for the hot NovyWave rows:
  `full_eval_row_count=0` and `row_materialize_ms=0`.
- Hot visible list work remains `selected_signal_lane_rows` and
  `selected_cursor_pair_rows`; this work is real and must not be blindly
  skipped. The killed `0804R-05` clean-prevalidated-hit micro-optimization
  proved that local field-cache counter movement can regress final p95 and
  shift work into worse root-list materialization.
- Hot internal bridge/page roots originally included
  `bridge_request_descriptor`, `bridge_cursor_values_page_ref`,
  `bridge_cursor_values`, `bridge_cursor_values_label`,
  `bridge_waveform_page(_ref)`, `bridge_signal_page(_ref)`,
  `bridge_hierarchy_page(_ref)`, and `bridge_file_stats(_ref)`. After
  `0804R-03`, cursor movement no longer churns the stable non-cursor
  hierarchy/signal/waveform page identities; remaining bridge churn is mostly
  cursor-values and visible selected-row list work.
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

- `blocked`: cannot start while `TASK-0804B` is postponed, while dependencies
  are incomplete, or while a decision gate explicitly says the task is not
  selected yet. The reason must be recorded in this file.
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

Status: done
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
jq -e '.runtime_dirty_frontier_cause_summary.click.candidate_defer_probe as $p | $p.scope == "no_behavior_change_simulated_suppression_for_candidate_unobserved_source_free_pure" and $p.root_count > 0 and $p.simulated_defer_enqueue_count > 0 and ($p.later_demand_read_count | type) == "number" and ($p.hidden_semantic_delta_count | type) == "number" and ($p.visible_root_list_dependency_count | type) == "number" and ($p.visible_root_list_changed_dependency_count | type) == "number" and ($p.root_list_evaluation_dependency_count | type) == "number" and ($p.classification_counts | type) == "object" and ($p.classification_counts | has("currentness_only") and has("must_publish_semantic_delta") and has("bridge_identity") and has("cursor_telemetry") and has("visible_list_dependency"))' target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json
jq -e '.runtime_dirty_frontier_cause_summary.click.candidate_defer_probe.top_roots | length > 0 and all(.[]; (.root_path | type) == "string" and (.simulated_defer_enqueue_count | type) == "number" and (.later_demand_read_count | type) == "number" and (.changed_read_keys | type) == "array" and (.would_hide_semantic_delta | type) == "boolean" and (.visible_root_list_dependency | type) == "boolean" and (.visible_root_list_changed_dependency_count | type) == "number" and (.root_list_evaluation_dependency_count | type) == "number" and (.classification as $c | ["currentness_only","must_publish_semantic_delta","bridge_identity","cursor_telemetry","visible_list_dependency"] | index($c) != null) and (if .later_demand_read_count > 0 then (.first_demand | type) == "object" and (.first_demand.context as $c | ["evaluator","state_summary","document_window_summary","sparse_value_summary","assertion","root_list_evaluation","observed_projection"] | index($c) != null) and (.first_demand.read_kind | type) == "string" and (.first_demand.read_key | type) == "string" else (.first_demand == null or (.first_demand | type) == "object") end))' target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json
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

Status: done
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

R02 read-path audit table:

| Read path | Category | Contract |
| --- | --- | --- |
| `eval_identifier`, `eval_path` root reads | `ensure_root_current` barrier | They route through `runtime_scalar_boon_value`, `root_derived_boon_value`, or `structured_root_child_boon_value`; those helpers own the barrier. |
| `runtime_scalar_json`, `runtime_scalar_boon_value` | `ensure_root_current` barrier | Direct scalar/json root reads call `ensure_root_current` before reading stored root values. |
| `structured_root_child_boon_value` | `ensure_root_current` barrier | Child-path reads refresh the deferred matching parent or direct root before reading nested JSON. |
| `root_derived_boon_value`, `root_pure_derived_boon_value`, `root_non_list_derived_boon_value` | `ensure_root_current` barrier | The root itself is refreshed first; root-value-cache hits validate dependency read sets through `ensure_root_reads_current`. Demand-refresh changed reads invalidate dependent root/function/list-view/list-map caches. |
| `root_list_derived_boon_value` for list-view roots | `eager-only/non-deferred exemption` | List-view roots remain eager in R02. R04 must not put `DerivedValueKind::ListView` roots into `deferred_dirty_roots` without a separate list-view materialization contract and tests. |
| Root `root_value_cache` hits | `ensure_root_current` barrier | Cache hits validate recorded root reads, including `RootChild` dependencies by demanding the child path before the parent fallback. A direct demand refresh invalidates other root caches whose reads overlap the refreshed root's changed reads. |
| `function_value_cache` hits | `ensure_root_current` barrier | Cache hits validate recorded reads through `ensure_root_reads_current`; demand refresh changed reads invalidate overlapping function cache entries. |
| `root_list_view_field_cache` hits and previous-pass field reuse | `ensure_root_current` barrier | Cache hits validate recorded reads through `ensure_root_reads_current`; normal dirty-read guards and demand-refresh invalidation keep overlapping entries from returning stale fields. |
| `root_list_map_output_cache` hits | `ensure_root_current` barrier | Cache hits validate recorded reads through `ensure_root_reads_current`; demand-refresh changed reads invalidate overlapping map-output entries. |
| `state_summary`, `runtime_value_summaries`, `document_state_summary`, `document_state_summary_for_window`, `generic_summary_with_limits` | `ensure_root_current` barrier | Sparse summaries demand requested paths through root-derived/scalar helpers. Full document/state summaries also refresh stored root paths before reporting them. This is a correctness guarantee, not a speed claim for future R04 summary behavior. |
| Scenario assertions through `assert_generic_step_expectations` and `root_textlike_for_assertion` | `ensure_root_current` barrier | Assertion reads clear relevant summary root caches and demand through root-derived/scalar helpers. |
| Root-list materialization evaluator reads | `ensure_root_current` barrier | Row/list field evaluation reaches root values through the same evaluator/scalar/root-derived helpers; list-view roots themselves stay eager-only in R02. |
| `runtime_scalar_number_for_numeric_guard` | `ensure_root_current` barrier | Numeric guard reads refresh roots before checking stability intervals. |
| Projection selector/storage reads | `ensure_root_current` barrier | `runtime_scalar_json` refreshes selector roots; `projection_storage_list_name` refreshes projection storage roots before reading stored list names or evaluating fallback statements. |
| Boolean source-route context reads through `derived_bool_value` | `ensure_root_current` barrier | Bool route context refreshes roots before direct bool/count reads. |
| Source-route text context reads, root-text transform current comparisons, lower `GenericCircuitRuntime` root text/bool helpers, raw mutation/current-comparison helpers | `eager-only/non-deferred exemption` | These paths still use direct storage and are not eligible for demand deferral in R04. Roots feeding these guards must remain eager until the source-route callback/storage APIs are made mutable and barrier-aware. |
| Bridge proof queries through summaries/assertions | `ensure_root_current` barrier | Proof queries that use runtime summaries or assertion reads are barrier-protected. |
| Bridge stale-response guard inputs that use direct source-route/storage helpers | `eager-only/non-deferred exemption` | These guards remain eager-only and must not be deferred by R04 until their inputs are read through barrier-aware APIs. |
| Runtime initialization reads | `impossible/unreachable with test evidence` | They run before deferred dirty roots exist, so they cannot observe production deferred-dirty state. |
| Test-only storage corruption helpers | `impossible/unreachable with test evidence` | They are not production demand-read paths; R02 uses them only to create stale storage/cache states that prove the barrier refreshes before returning values. |

Verification commands:

```bash
cargo fmt -p boon_runtime
RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_currentness_barrier -- --nocapture
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
- Existing observed-root and semantic-delta behavior remains eager in R02.
  R04 must add deferral-specific tests before any observed or semantic-delta
  root can be deferred.

Kill criteria:

- Stop and redesign if the runtime has too many untracked read paths to make
  stale reads auditable.
- Do not proceed to demand deferral until this contract exists.

## 0804R-03 Bridge/Page Identity Split

Status: done
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

Status: blocked; not selected after `0804R-03` because the fresh diagnostic
still reports `currentness_only=0`
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

Status: killed
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

Status: done; negative closeout because `0804R-05` was killed and
`TASK-0804C` now owns the remaining blocker
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

Status: done; added `TASK-0804C` to the master checklist
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

Outcome:

- `TASK-0804B` is superseded by `TASK-0804C`.
- `TASK-0804C` is `pending` in the master checklist and owns the next
  implementation attempt: a generic compiled row/field dependency frontier for
  root list-view evaluation/currentness, not another NovyWave-specific
  field-cache patch.

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
- 2026-06-17: Completed `0804R-01` as diagnostic-only runtime/reporting
  infrastructure. Code changes are limited to gated candidate-demand
  instrumentation in `crates/boon_runtime/src/lib.rs` and report aggregation in
  `crates/boon_native_playground/src/main.rs`; dirty enqueue and materialize
  behavior did not change. Commands: `cargo fmt -p boon_runtime -p
  boon_native_playground -p xtask`; `cargo check -p boon_runtime -p
  boon_native_playground -p xtask`; `BOON_PROFILE_ROOT_DEMAND=1
  BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json`
  (expected failing exit with `status=fail` due the existing click/input p95
  budget blockers, report written); `env -u BOON_PROFILE_ROOT_DEMAND -u
  BOON_PROFILE_DIRTY_FRONTIER cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/reports/native-gpu/novywave-interaction-speed.json` (expected failing
  exit with diagnostics disabled and the same p95 blockers, report written);
  `timeout 240 cargo xtask
  verify-novywave-bridge-scenario --report
  target/reports/novywave-bridge-scenario.json`; `cargo xtask
  verify-report-schema`; enriched diagnostic `jq -e` checks; canonical
  no-diagnostic/no-regression `jq -e` checks; and `git diff --check`.
- 2026-06-17: `0804R-01` evidence from
  `target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json`:
  candidate probe is `enabled=true` with `24` candidate roots, `552` simulated
  defer enqueues, `552` materializations, `552` changed materializations, `512`
  later demand reads, `248` hidden semantic-delta materializations, and `336`
  aggregate visible/list dependency hits, split as `208` changed-read list
  dependencies and `128` root-list-evaluation demand dependencies.
  Classification counts are not
  currentness-only: `currentness_only=0` enqueues, `bridge_identity=472`,
  `cursor_telemetry=264`, `must_publish_semantic_delta=248`, and
  `visible_list_dependency=208`. The top root
  `store.bridge_cursor_values_page_ref` has first demand context
  `root_list_evaluation`, first read kind `runtime_scalar_boon_value`,
  visible list root `store.selected_signal_lane_rows`, `64` later demand
  reads, and changed read keys including page digest, request fingerprint,
  input digest, and status.
- 2026-06-17: `0804R-01` canonical overhead check passed. The refreshed
  canonical report `target/reports/native-gpu/novywave-interaction-speed.json`
  has candidate probe `enabled=false`, `root_count=0`, and p95s
  `click_to_cursor=18.653014ms`, `input_to_visible=18.653014ms`,
  `runtime_apply=12.355368ms`, `runtime_step_apply=9.997917ms`, and
  `layout_rebuild=4.940346ms`, within the no-regression threshold relative to
  the `0804R-00` baseline. The slow graph remains `194/32/38` at p95, with
  `root_flush.p95=5.810345ms`, `dirty_scheduler.p95=2.370330ms`, and
  `root_materialization.p95=3.296347ms`. The root-list cause remains
  `root_flush_dirty_scheduler_plus_root_list_materialization`; aggregate list
  counters show `eval_ms=35.999336`, `diff_ms=30.612126`,
  `full_eval_row_count=0`, and `row_materialize_ms=0.0`; dominant list remains
  `selected_signal_lane_rows` with `eval_ms=19.894158`.
- 2026-06-17: `0804R-01` decision: do not implement demand deferral from this
  evidence. Candidate roots are value-changing, bridge/page/cursor identity
  roots and visible list dependencies, not mostly currentness-only. Per the
  decision table, proceed to `0804R-02` currentness/stale-read contract before
  behavior changes; `0804R-03` bridge/page identity split remains the likely
  first behavior slice after the contract. `0804R-04` demand/currentness
  frontier is not selected first.
- 2026-06-17: Completed `0804R-02` currentness/stale-read contract in
  `crates/boon_runtime/src/lib.rs`. The runtime now has a single audited
  `ensure_root_current` barrier plus `ensure_root_reads_current` for cached
  read-set validation. The barrier resolves deferred structured child reads to
  the deferred parent root when needed, clears the refreshed root cache/guards
  before materialization, and uses the materialization changed-read set to
  invalidate overlapping dependent root-value, function, root-list-view-field,
  and root-list-map-output caches. No Boon syntax, NovyWave source, fixture,
  hardcoded filename, report schema, or budget changed.
- 2026-06-17: `0804R-02` focused regressions added:
  `root_currentness_barrier_refreshes_deferred_scalar_before_cached_reads_and_summaries`
  proves evaluator, assertion, and sparse-summary reads refresh a deliberately
  stale deferred pure root; `root_currentness_barrier_invalidates_other_cached_dependents_after_direct_refresh`
  proves a direct scalar demand refresh invalidates other cached dependents
  before they can stale-hit; and
  `root_currentness_barrier_refreshes_deferred_structured_parent_before_child_read`
  proves nested child reads refresh the deferred structured parent before
  reading JSON. Post-slice review then found a `RootChild` cache dependency
  gap, so `root_currentness_barrier_checks_root_child_cache_dependencies` was
  added to prove cached dependents demand `root.child`, not just the parent
  root.
- 2026-06-17: `0804R-02` verification passed: `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_currentness_barrier -- --nocapture`
  (`4 passed`); `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_derived_ -- --nocapture`
  (`6 passed`); `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib structured_root_ -- --nocapture`
  (`3 passed`); `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`
  (`1 passed`); `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`
  (`19 passed`); and `cargo check -p boon_runtime`.
- 2026-06-17: `0804R-02` subagent review results: correctness reviewer
  `019ed6a1-88e1-7e41-ba9a-87f80e95cc12` and performance reviewer
  `019ed6a1-e64e-7580-aaf4-2c0cf8b1ab0b` both flagged the missing
  demand-refresh changed-read invalidation; the implementation was extended
  before R02 was marked done. They also flagged list-view-root deferral and
  source-route text/storage reads as not barrier-safe yet, so the read-path
  table marks those as eager-only/non-deferred exemptions. Docs reviewer
  `019ed6a2-3450-7192-be27-d033da2adf21` required the explicit read-path
  table and clarified that R02 is a correctness contract, not a speed closeout.
  Post-slice correctness reviewer `019ed6ac-1811-7892-8bb2-e3cef2990f51`
  then found that `RootChild` read-set validation checked only the parent root;
  `ensure_root_reads_current` now checks both `root.child` and the parent
  fallback, and the new `RootChild` regression passed. Docs reviewer
  `019ed6ac-4f7f-71b1-8a4b-6e35ba715269` found no blocking docs issue.
  Performance reviewer `019ed6ac-2a25-7431-95d4-76b52ecb9272` agreed R03 is
  the right next slice but required a post-R02 canonical speed refresh before
  behavior changes.
- 2026-06-17: Post-R02 canonical no-diagnostic speed refresh completed with
  `env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`.
  The command exited with the expected failing gate status because the known
  strict latency budgets remain; `cargo xtask verify-report-schema` passed.
  Report `target/reports/native-gpu/novywave-interaction-speed.json` has
  `status=fail`, `candidate_defer_probe.enabled=false`, candidate root count
  `0`, `click_to_cursor.p95=19.806244ms`,
  `input_to_visible.p95=19.806244ms`, `runtime_apply.p95=12.656522ms`,
  `runtime_step_apply.p95=10.357947ms`,
  `runtime_state_summary.p95=1.017862ms`,
  `layout_rebuild.p95=4.696317ms`, root-flush p95 `6.113463ms`,
  dirty-scheduler p95 `2.649652ms`, root-materialization p95 `3.324309ms`,
  and the slow graph remains `194/32/38`. Renderer post-interaction upload
  remains separated at `3360` bytes, `3` queue writes, `0` staging wraps, and
  `0` quad-cache evictions. Next task is `0804R-03` bridge/page identity
  split; `0804R-04` remains blocked/not selected first by the `0804R-01`
  decision table.
- 2026-06-17: Completed `0804R-03` bridge/page identity split. The behavior
  slice was committed at `74ca85f` and changed only
  `examples/novywave/RUN.bn` plus focused runtime tests in
  `crates/boon_runtime/src/lib.rs`. The Boon source now separates full
  cursor-aware request freshness/stale guards from stable non-cursor page
  identity roots (`bridge_page_request_*` and `bridge_page_response_*`).
  Cursor-only movement keeps hierarchy/signal/waveform page identity stable,
  while cursor-values remains cursor-aware because cursor is a real payload
  input.
- 2026-06-17: `0804R-03` correctness verification: `cargo fmt -p
  boon_runtime -p boon_bridge -p boon_native_playground -p xtask`;
  `cargo test -p boon_bridge --lib -- --nocapture` (`12 passed`);
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib
  root_derived_ -- --nocapture` (`6 passed`); `cargo check -p
  boon_runtime -p boon_bridge -p boon_native_playground -p xtask`;
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report
  target/reports/novywave-bridge-scenario.json`; and `cargo xtask
  verify-report-schema` all passed. Focused `boon_runtime` tests passed for:
  `novywave_cursor_click_splits_page_identity_from_cursor_freshness`,
  `novywave_hover_label_updates_do_not_change_bridge_page_identity`,
  `novywave_pan_zoom_and_format_are_real_page_identity_inputs`,
  `novywave_bridge_page_identity_is_deterministic_across_replay`,
  `novywave_waveform_metadata_drives_selected_file_and_timeline_window`, and
  `novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them`.
  The broad `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib
  novywave -- --nocapture` command still fails one pre-existing test,
  `novywave_selected_visible_items_model_group_headers_and_collapse`; the same
  focused failure was reproduced on parent commit `7557f82`, so it is not an
  `0804R-03` regression. It remains a final verification blocker for later
  closeout work.
- 2026-06-17: `0804R-03` fresh diagnostic report
  `target/diagnostics/native-gpu/novywave-interaction-speed-bridge-identity.json`
  exited with the expected failing gate status but wrote current diagnostic
  evidence. Candidate roots dropped from `24` to `10`, simulated defer
  enqueues from `552` to `296`, later demand reads from `512` to `240`, and
  hidden semantic-delta materializations from `248` to `184`. The diagnostic
  still reports `currentness_only=0`, so `0804R-04` remains not selected.
  Remaining top work is dominated by `store.selected_signal_lane_rows`,
  `store.selected_cursor_pair_rows`, and cursor-values roots.
- 2026-06-17: `0804R-03` fresh canonical no-diagnostic speed report
  `target/reports/native-gpu/novywave-interaction-speed.json` was generated at
  `git_commit=74ca85f`, `binary_hash=9b694ccbba130ad1dfbc506ce0f800982a4031eecf1a6f44c29a4eded1c17a76`,
  and clean `worktree_fingerprint=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
  The gate still fails only the strict click/input p95 budgets:
  `click_to_cursor.p95=17.701393ms` and
  `input_to_visible.p95=17.701393ms` against `16.700ms`.
  Meaningful movement passed relative to both the post-R02 baseline and the
  original `0804R-00` graph shape: root-flush p95 dropped from
  `6.113463ms` to `3.506412ms`, dirty-scheduler p95 from `2.649652ms` to
  `1.181580ms`, root-materialization p95 from `3.324309ms` to
  `2.207607ms`, and the slow click graph class from `194/32/38` to
  `80/17/23` (`16` samples). Aggregate click graph counts dropped from
  `3536/600/792` to `1632/344/536`. Renderer upload remains separated:
  post-interaction upload `3360` bytes, `3` queue writes, `0` staging wraps,
  and `0` quad-cache evictions. Per the closeout matrix, `0804R-05` is now
  the next pending task because p95 still fails and visible list-view buckets
  dominate.
- 2026-06-17: `0804R-03` subagent review effects: correctness review required
  more than the initial cursor-only test, so label-only hover, pan/zoom/format,
  page-level stale rejection, replay determinism, and row page-ref semantics
  tests were added before marking the task done. Performance review required
  fresh canonical and diagnostic reports for `74ca85f`; stale reports were not
  used for the keep/kill decision. Docs review required the status to remain
  `in_progress` until current evidence existed and required expected failing
  speed gates to be recorded as failing budget gates, not as passed
  verification.
- 2026-06-17: `0804R-05` was attempted and killed. The correctness fixes found
  during the slice are kept: root list-view commits now publish changed reads;
  list-view roots are materialized/currentness-checked before exposing a
  `ListRef`; inherited `root_stack` is threaded through root list-view
  materialization; row-field names keep precedence over same-named global list
  roots; and field-cache entries whose `Root` or `RootChild` reads overlap the
  active root stack are rejected. The extra clean-prevalidated-hit fast path
  that skipped `ensure_root_reads_current` was reverted because the current
  R05 repeat report
  `target/diagnostics/native-gpu/novywave-interaction-speed-r05-repeat.json`
  was current and non-diagnostic but regressed `click_to_cursor.p95` and
  `input_to_visible.p95` to `24.929800ms`, `runtime_step_apply.p95` to
  `15.958209ms`, root-flush p95 to `8.222271ms`, and click aggregate root-list
  work to `eval_ms=99.689672`, `full_eval_row_count=94`, and
  `row_materialize_ms=2.012348`. The fast path violates the R05 no-regression
  and field-only invariants and must not be retried without a different proof.
- 2026-06-17: `0804R-05` correctness-only control after reverting that fast path
  passed focused verification: `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib
  root_list_view_ -- --nocapture` (`24 passed`);
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_
  -- --nocapture` (`81 passed`);
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib
  user_function_cache_ -- --nocapture` (`3 passed`);
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave
  -- --nocapture` (`25 passed`);
  `cargo check -p boon_runtime -p boon_bridge -p boon_native_playground
  -p xtask`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report
  target/reports/novywave-bridge-scenario.json`; canonical speed
  `env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/reports/native-gpu/novywave-interaction-speed.json`; and `cargo xtask
  verify-report-schema` after moving the stale repeat artifact under
  `target/diagnostics`.
- 2026-06-17: `0804R-05` first correctness-only control speed report
  `target/reports/native-gpu/novywave-interaction-speed.json` still exits
  `status=fail`: `click_to_cursor.p95=18.365383ms`,
  `input_to_visible.p95=18.365383ms`, `runtime_apply.p95=11.584193ms`,
  `runtime_step_apply.p95=9.657368ms`, `runtime_state_summary.p95=0.897571ms`,
  and `layout_rebuild.p95=4.659514ms`. Root p95s are
  `source_action_root_flush=4.279103ms`,
  `source_action_root_dirty_scheduler=1.389330ms`, and
  `source_action_root_materialization=2.703999ms`; graph p95 is
  `75/17/24`. The click aggregate stays field-only
  (`full_eval_row_count=0`, `row_materialize_ms=0.0`) but does not move the
  intended hot list buckets: `selected_signal_lane_rows eval_ms=33.297496` and
  `diff_ms=29.203688`; `selected_cursor_pair_rows eval_ms=10.224008` and
  `diff_ms=8.852834`. Bridge proof remains `status=pass`.
- 2026-06-17: Measurement audit refreshed the canonical speed evidence after the
  doc update. `target/reports/native-gpu/novywave-interaction-speed.json` is
  current for `worktree_fingerprint=a3f1581db192b7b5ad28af13eabf5295b9ab5eb936daaf63a8ad2497b6637469`,
  `binary_hash=075f458396be1587a7c5a15eddf073853a0c2e4f1fa069731f99c58e8d8ca09c`,
  and `playground_binary_hash=b01b57ee05de6df734c618148452c09b2a578e16a703bb02124131a7e4643ba1`.
  It still exits `status=fail` only on strict click/input latency:
  `click_to_cursor.p95=18.020024ms`,
  `input_to_visible.p95=18.020024ms`, `runtime_apply.p95=11.305099ms`,
  `runtime_step_apply.p95=9.319520ms`, and graph p95 `75/17/24`.
  Two immediate canonical repeats with
  `env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER` wrote
  `target/diagnostics/native-gpu/novywave-interaction-speed-measurement-audit-1.json`
  and
  `target/diagnostics/native-gpu/novywave-interaction-speed-measurement-audit-2.json`;
  both matched the same worktree/xtask/release-playground hashes and measured
  `click/input.p95=18.226649ms` and `18.070569ms`. This is enough to trust the
  post-revert failed-latency class, but future decisions on small p95 deltas must
  use repeats or stronger harness evidence.
- 2026-06-17: Post-slice subagents agreed on the closeout. Correctness review
  `019ed722-341c-7173-9d9d-377afe3fabd3` found no blocker to keeping the
  correctness fixes, with the documented gap that inherited-stack list-view
  currentness is covered by focused unit probes rather than a full production
  end-to-end scenario. Performance review
  `019ed722-35a9-7202-906b-f094692c536b` said R05 acceptance is not met and the
  fast path must be recorded as killed. Plan/status review
  `019ed722-3714-7821-9503-d18069a4d2df` required `0804R-05=killed`,
  `0804R-06=done` as negative closeout, `0804R-07=done`, and
  `TASK-0804B=superseded` by new `TASK-0804C`.
- 2026-06-17: `0804R-06` and `0804R-07` are complete as negative closeout, not
  as speed success. The plan proved insufficient after a real graph movement
  slice and a killed list-view frontier slice. The replacement task is
  `TASK-0804C` in the master checklist. It owns the measured residual blocker:
  generic engine-level root-list currentness/materialization still routes click
  updates through eager source-action root flush and repeated field-only
  list-view evaluation. `TASK-0804B` is superseded, while `TASK-0804A` remains
  postponed historical evidence.
- 2026-06-17: Measurement-review subagents
  `019ed72f-a3b6-7a80-8291-d4ce0543ad8b`,
  `019ed72f-ba1e-7210-8a33-cf48785fe03a`, and
  `019ed72f-d0cc-7ec1-bb37-d2576d337d76` agreed that the current report
  freshness protections are meaningful, the R05 kill is defensible because the
  regression was large and violated field-only invariants, and the current
  verifier is an app hot-path oracle rather than live-present proof. Before
  keeping or killing future optimizations on sub-millisecond or otherwise small
  p95 movement, run repeat canonical reports or first harden the verifier with
  more samples, ambient-load metadata, and per-interaction render/present timing
  if the claim depends on visual presentation.
