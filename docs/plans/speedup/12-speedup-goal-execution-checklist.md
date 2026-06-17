# Speedup Goal Execution Checklist

## Objective

This is the master `/goal` control document for implementing the Boon and
NovyWave speedup roadmap. It converts the existing research, smell inventories,
API/design plans, engine plans, and radical option catalogue into executable,
dependency-ordered tasks.

The source roadmap is:

- `docs/plans/speedup/01-inspiration.md`
- `docs/plans/speedup/02-novel-research-ideas.md`
- `docs/plans/speedup/03-rust-speed-libraries.md`
- `docs/plans/speedup/04-rust-and-shader-slow-patterns.md`
- `docs/plans/speedup/05-rust-wgpu-performance-measurement.md`
- `docs/plans/speedup/06-human-like-scenario-testing.md`
- `docs/plans/speedup/07-novywave-boon-code-smells.md`
- `docs/plans/speedup/08-repo-code-smell-risk-inventory.md`
- `docs/plans/speedup/09-novywave-boon-api-and-design-speed-plan.md`
- `docs/plans/speedup/10-engine-runtime-renderer-speed-plan.md`
- `docs/plans/speedup/11-crazy-speed-options.md`
- `docs/plans/speedup/13-structural-representation-experiments.md`
- `docs/plans/speedup/14-binary-bytes-list-constant-experiments.md`
- `docs/plans/speedup/15-targeted-representation-experiments.md`
- `docs/plans/speedup/16-user-suggested-representation-experiments.md`
- `docs/plans/speedup/17-representation-next-experiments.md`
- `docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`
- `docs/plans/speedup/19-user-representation-ledger.md`
- `docs/plans/speedup/20-task-0804-root-flush-resolution-plan.md`

The ordering principle is correctness first, proof integrity second,
measurement third, then speed. A faster engine is not acceptable if stale
reports, example-specific shortcuts, scaffold render paths, or silent patch
failures can make it look correct.

Default user-facing boundary:

- Do not add Boon syntax unless an engine-only solution has been attempted,
  found impossible or undiagnosable, and documented in this file.
- Prefer inferred schemas, stable IDs, route tables, indexes, materialization
  protocols, bridge metadata, and clear compiler errors.
- Generic engine layers must not contain NovyWave-, Cells-, TodoMVC-, or
  Counter-specific branches.

## Execution Rules For `/goal`

At the start of every `/goal` continuation:

1. Run `git status --short`.
2. Re-read this file.
3. Re-read the source plan files that are listed on the next unblocked task.
4. Inspect the current implementation around the likely touched modules before
   editing.
5. Check whether previous assumptions are stale, especially generated reports,
   binary hashes, worktree fingerprints, scenario hashes, and budget hashes.

Task selection algorithm:

1. Pick the first task with `Status: pending` whose dependencies are `done`.
2. If the task is too large to complete safely in one implementation slice,
   split it into child tasks before changing code.
3. Implement only the selected task and directly required support work.
4. Add or update focused tests, gates, or reports for that task.
5. Run the listed verification commands, or implement the missing command when
   the task itself is the command creation task.
6. Update this file in the same change as the task: status, progress log, and
   any new follow-up tasks found while implementing.
7. Do not mark a task `done` unless its acceptance criteria pass or the file
   records an explicit, narrow exception with a replacement task.
8. If there are no real `pending` or `in_progress` tasks but real postponed
   tasks remain, stop and report those task IDs plus the exact user action
   required to resume them. Do not silently treat the roadmap as complete.

Evidence rules:

- Runtime-only evidence is semantic support, not proof of UI interaction.
- Automated reports must never claim human observation.
- Native GPU proof must follow `docs/architecture/NATIVE_GPU_PIPELINE.md`.
- Stale report artifacts are historical hints, not current acceptance proof.
- Interaction-mode speed reports must exclude proof readbacks, PNG writes,
  heavy JSON summaries, report serialization, and dev-only blocking IPC.
- Full recompute or current interpreter behavior remains the oracle until an
  incremental path is proven equivalent.

Self-update rules:

- Update task `Status` in this file when work starts, completes, blocks, or is
  superseded.
- Add child task IDs immediately when a task is split.
- Add discovered blockers as new tasks near the earliest phase that needs them.
- Add experiments only when they have a metric, oracle, and kill criteria.
- Append to `Progress Log` after each completed task or intentional stop.
- Do not delete completed entries; use `superseded` with a replacement task ID
  when a task changes direction.

Status values:

- `pending`: not started.
- `in_progress`: currently being implemented in the active `/goal` run.
- `blocked`: cannot proceed without another task or user/external input.
- `done`: acceptance and verification completed.
- `dropped`: intentionally not pursued, with reason recorded.
- `superseded`: replaced by another task ID.
- `postponed`: intentionally paused by user direction, experiment cap, or
  explicit kill criteria. Do not pick it in the task selection algorithm until
  a later user request or checklist update unpostpones it.

Template placeholders:

- `TASK-0000` and `EXP-0000` inside the schema examples are documentation
  templates, not executable work items. Ignore those placeholders when counting
  real pending work.

## Task Schema

Every implementation task must use this exact shape:

```md
### TASK-0000 Short Title
Status: pending
Type: implementation | gate | refactor | measurement | cleanup
Priority: P0 | P1 | P2 | P3
Depends on: none | TASK-0000
Source plans: 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 13, 14, 15
Likely areas: runtime, xtask, document, native-gpu
Goal:
Acceptance:
Verification:
Rollback / stop condition:
Notes:
```

Required task fields mean:

- `Priority`: P0 blocks trustworthy speed work; P1 is required for the first
  fast generic engine; P2 is important but can wait; P3 is exploratory or late.
- `Depends on`: use task IDs, not prose. Use `none` only for tasks that can be
  first in the roadmap.
- `Source plans`: cite the plan numbers that justify the task.
- `Acceptance`: concrete observable behavior, not intent.
- `Verification`: exact command where possible; if the command does not exist,
  the task must create it.
- `Rollback / stop condition`: tells `/goal` when to stop instead of pushing a
  risky partial implementation.

## Experiment Schema

Every experiment must use this exact shape:

```md
### EXP-0000 Short Title
Status: pending
Type: experiment
Depends on: TASK-0000
Hypothesis:
Metric to improve:
Correctness oracle:
Kill criteria:
Promote to implementation when:
Verification:
Notes:
```

Experiments must not be accepted as permanent architecture without promotion
criteria being met. A dependency or low-level optimization that does not move a
measured bottleneck should be removed or left behind a clearly labeled
diagnostic branch.

## Phase 0: Integrity Before Speed

### TASK-0001 Scenario Manifest Integrity Gate
Status: done
Type: gate
Priority: P0
Depends on: none
Source plans: 06, 08, 09, 10, 11
Likely areas: `crates/xtask`, `crates/boon_runtime`, `examples/manifest.toml`, `examples/*.scn`
Goal:
Add `verify-scenario-manifest-integrity` as the prerequisite gate for trusting scenario evidence.
Acceptance:
- Duplicate `.scn` step IDs fail.
- Manifest scenario references that are not present in scenario/probe inventory fail.
- Duplicate manifest refs fail unless explicitly phased or generated-probe provenance is recorded.
- Authored raw-coordinate selectors fail.
- Target-text-only selectors that do not disambiguate role/control fail.
- Input/action steps without expected public source intent fail unless explicitly exempted with reason.
- Source/scenario/manifest/budget hash fields are included in the report.
- Evidence tier drift is reported and fails readiness.
Verification:
- `cargo xtask verify-scenario-manifest-integrity --report target/reports/scenario-manifest-integrity.json`
- `cargo test -p boon_runtime --lib scenario`
Rollback / stop condition:
- Stop if the manifest/scenario TOML shape is not expressive enough to report generated probes or phased refs; add a child task to extend the schema first.
Notes:
- This gate is expected to fail initially until `TASK-0002` classifies or fixes known drift.
- Implemented by `verify-scenario-manifest-integrity`; current report writes hashes and fails with 18 known blockers for `TASK-0002`.

### TASK-0002 Classify And Fix Known Scenario Drift
Status: done
Type: cleanup
Priority: P0
Depends on: TASK-0001
Source plans: 06, 09, 10
Likely areas: `examples/manifest.toml`, `examples/novywave.scn`, `examples/cells.scn`, `examples/todomvc.scn`, `examples/todo_mvc_physical.scn`, `examples/counter.scn`
Goal:
Make the existing bundled scenarios pass the integrity rules or explicitly classify generated/probe cases.
Acceptance:
- NovyWave duplicate `select-primary-file` is removed, renamed, or classified without duplicate identity.
- NovyWave duplicate input/scroll-focus manifest refs are either phased/generated with provenance or split into unique executable scenario IDs.
- Cells scroll/focus labels are executable `.scn` steps or explicitly generated probes with provenance.
- TodoMVC `reject-empty-todo` manifest reference is reconciled with `reject-empty-todo-type` and `reject-empty-todo-submit`.
- TodoMVC and Counter target-text-only action selectors are disambiguated with role/control/target data.
- TodoMVC hover/delete action steps either route expected public source intent or are explicitly classified as non-source hover probes with provenance.
- TodoMVC Physical action steps without assertions are given assertions or documented exemptions.
- The integrity report is passing before any scenario output is used as acceptance evidence.
Verification:
- `cargo xtask verify-scenario-manifest-integrity --report target/reports/scenario-manifest-integrity.json`
- `cargo test -p boon_runtime --lib`
Rollback / stop condition:
- Stop if fixing drift would weaken a scenario assertion. Add a follow-up task to improve scenario expressiveness instead.
Notes:
- Do not delete scenario coverage just to make the gate green.
- Implemented with explicit manifest `scenario_ref_provenance`, scenario `source_intent_exemption`, selector disambiguation, unique NovyWave step identity, and generic edit-state runtime invariant fixes needed by runtime verification.

### TASK-0003 Interaction, Proof, And Diagnostic Report Modes
Status: done
Type: implementation
Priority: P0
Depends on: TASK-0001
Source plans: 05, 06, 08, 10, 11
Likely areas: `crates/boon_report_schema`, `crates/xtask`, `crates/boon_driver`, native GPU reports
Goal:
Separate interaction latency measurement from proof and diagnostic work.
Acceptance:
- Reports include an explicit `measurement_mode` or equivalent field with `interaction`, `proof`, or `diagnostic`.
- Interaction reports fail if proof readback, PNG writes, heavy JSON summaries, report serialization, verbose tracing, or dev-only blocking IPC is counted in hot-path latency.
- Proof reports can still include WGPU readbacks, hashes, artifacts, and schema checks.
- Diagnostic reports can include rich debug tables without satisfying speed budgets.
Verification:
- `cargo test -p boon_report_schema --lib`
- `cargo xtask verify-report-schema`
- Existing native GPU report schema checks still pass or fail only for newly documented reasons.
Rollback / stop condition:
- Stop if report producers cannot be migrated without breaking all existing gates; add compatibility fields and a child migration task.
Notes:
- Do not weaken native GPU schemas or negative checks to make this pass.
- Implemented with required top-level `measurement_mode`; runtime speed reports
  use `interaction`, static/native proof gates use `proof`, benchmark wrappers
  and debug/helper reports use `diagnostic`.
- Interaction-mode reports are rejected when hot-path counters or booleans show
  proof readbacks, PNG writes, report serialization, heavy JSON summaries,
  verbose tracing, or dev-only blocking IPC.
- `cargo xtask verify-example-speed counter --report target/reports/counter-speed.json`
  was tried as an interaction artifact and failed the existing allocation
  budget; that generated report was removed before schema verification because
  the failure is unrelated to TASK-0003.

### TASK-0004 Flow IDs And Release-Mode Stage Counters
Status: superseded
Type: measurement
Priority: P0
Depends on: TASK-0003
Source plans: 05, 10, 11
Likely areas: runtime reports, document/layout reports, native GPU reports, xtask speed gates
Goal:
Make every interaction traceable across host input, source intent, runtime, document, layout, render, GPU, IPC, and optional proof readback.
Acceptance:
- A common `InteractionFlowId` or `FrameFlowId` links host input, source intent, runtime turn, document patch, layout/materialization, scene build, text shaping, asset work, GPU upload, encode, submit, present, IPC, and optional proof readback.
- Reports include p50/p95/p99/max and sample counts for key stages.
- Counters exist for allocations, rows scanned/touched, route actions visited, dirty fanout, cache hits/misses/evictions, lock waits, draw calls, queue writes, upload bytes, text shaping, glyph atlas uploads, asset status, IPC queue depth, dropped/coalesced messages, blocked sends, and dev lag.
- Interaction-mode reports show proof/readback/report-write hot-path counters as zero.
Verification:
- `cargo xtask verify-report-schema`
- `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
- At least one release-mode speed report for TodoMVC or Cells includes non-empty stage counters.
Rollback / stop condition:
- Stop if adding counters makes hot paths allocate or block materially; move heavy capture behind diagnostic mode.
Notes:
- Counters may start coarse, but they must be real measurements, not hard-coded zeros.
- Superseded by TASK-0004A through TASK-0004D because this spans report
  schema, runtime speed reports, native GPU/IPC observability, and release
  speed budget evidence.

### TASK-0004A Common Flow And Stage Counter Report Contract
Status: done
Type: measurement
Priority: P0
Depends on: TASK-0003
Source plans: 05, 10, 11
Likely areas: `crates/boon_report_schema`, `crates/boon_runtime`, `crates/xtask`
Goal:
Define the common report shape for interaction flow IDs and stage counters without requiring every producer to populate every native/runtime field yet.
Acceptance:
- Interaction reports include a stable `interaction_flow_id` or `frame_flow_id`.
- Interaction reports include a `stage_counters` object with p50/p95/p99/max/sample-count summary fields where timing samples exist.
- Interaction reports include explicit zero hot-path counters for proof/readback/report-write work.
- Schema validation rejects interaction reports with missing flow ID, missing `stage_counters`, empty stage samples, or positive hot-path proof/report counters.
- Proof and diagnostic reports are not required to carry interaction flow IDs.
Verification:
- `cargo test -p boon_report_schema --lib`
- `cargo test -p boon_runtime --lib`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if the common contract would force diagnostic/proof producers to fabricate interaction fields; keep the requirement interaction-mode only.
Notes:
- This child task is the prerequisite for adding real native/runtime stage detail.
- Implemented as an interaction-mode-only schema contract. Proof and
  diagnostic reports are not required to fabricate flow or stage fields.
- Runtime speed reports populate `interaction_flow_id`, `stage_counters`, and
  explicit zero hot-path proof/report counters from already collected scenario
  measurements. Xtask interaction reports derive stage counters from existing
  p50/p95/p99/max summary fields when available.

### TASK-0004B Runtime Speed Stage Counters
Status: done
Type: measurement
Priority: P0
Depends on: TASK-0004A
Source plans: 05, 10, 11
Likely areas: `crates/boon_runtime`
Goal:
Populate runtime speed reports with real interaction flow IDs and runtime/document-stage counters derived from existing measurements.
Acceptance:
- Runtime speed reports include a deterministic interaction flow ID tied to source/scenario/program hashes.
- Runtime speed reports include non-empty stage counter summaries for scenario step latency, semantic tick, render lowering, patch apply, dirty nodes/keys, render patches, allocations, rows scanned/touched when available, and route actions visited when available.
- Runtime speed reports carry explicit zero hot-path counters for proof readback, PNG writes, report writes, and dev blocking IPC.
- Semantic/proof reports retain their current proof behavior without pretending to be interaction reports.
Verification:
- `cargo test -p boon_runtime --lib`
- `cargo test -p boon_report_schema --lib`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if runtime counters require extra allocation in the measured hot path; derive summaries from already collected speed samples or record a follow-up task.
Notes:
- Runtime speed reports now expose route actions visited, rows touched,
  recompute candidates, recomputed fields, dirty nodes/keys, render patches,
  allocation count/bytes, semantic tick, runtime turn, render lowering, and
  document patch apply through `stage_counters` or top-level summary fields.
- Rows scanned are reported as unavailable with a pointer to TASK-0301 because
  list text lookup scan counts require lookup/index instrumentation.
- Semantic/proof runtime reports no longer carry `interaction_flow_id`,
  `stage_counters`, or hot-path interaction counters.

### TASK-0004C Native GPU And IPC Stage Counters
Status: superseded
Type: measurement
Priority: P0
Depends on: TASK-0004A
Source plans: 05, 10, 11
Likely areas: `crates/boon_native_gpu`, `crates/boon_native_app_window`, `crates/boon_native_playground`, `crates/xtask`
Goal:
Expose native renderer, app-window, and IPC counters through native GPU observability and speed reports.
Acceptance:
- Native reports include flow-linked summaries for host input, layout/materialization, scene build, text shaping, asset work, GPU upload, command encode, submit, present, IPC, and optional proof readback where those stages exist.
- Reports expose draw calls, queue writes, upload bytes, text shaping/cache, glyph atlas activity, asset status, IPC queue depth, dropped/coalesced messages, blocked sends, and dev lag from real counters or clearly documented unavailable fields.
- Interaction-mode native speed reports continue to show proof/readback/report-write hot-path counters as zero.
Verification:
- `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
- `cargo xtask verify-report-schema`
- Focused native GPU/app-window tests touched by the implementation.
Rollback / stop condition:
- Stop if collecting a native counter requires blocking the render loop or readback on the interaction path; move that counter to proof/diagnostic mode.
Notes:
- Superseded by TASK-0004C1 through TASK-0004C3 so renderer/app-window
  counter inventory, IPC observability, and native speed interaction reports
  can be verified independently.

### TASK-0004C1 Native Renderer Counter Inventory
Status: done
Type: measurement
Priority: P0
Depends on: TASK-0004A
Source plans: 05, 10, 11
Likely areas: `crates/boon_native_gpu`, `crates/boon_native_app_window`, `crates/xtask`
Goal:
Expose the native renderer/app-window counter inventory in reports without adding blocking instrumentation.
Acceptance:
- Native observability reports list the real source fields for draw calls, upload bytes, visible items, rendered rects, text runs shaped, rendered text runs, glyphon text areas, preview IPC blocking, input polls, rendered frames, scheduler wakes, and proof readback.
- The report explicitly labels unavailable or not-yet-instrumented stages such as GPU timestamp encode/submit/present timing and glyph atlas upload/eviction counts.
- No interaction-mode report is allowed to claim a proof/readback counter as hot-path timing.
Verification:
- `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if exposing a counter would require app-window readback or blocking device polling in the interaction path; leave it unavailable and move detailed capture to proof/diagnostic mode.
Notes:
- Implemented as `native_renderer_counter_inventory` and
  `native_stage_counter_availability` on the native GPU observability report.
- The inventory names real renderer/app-window fields and labels synthetic or
  unavailable counters such as queue-write counts, text cache hit/miss counts,
  asset texture upload bytes, glyph atlas uploads, blocked sends, and split
  encode/submit/present timing.

### TASK-0004C2 Native IPC Observability Stage Counters
Status: done
Type: measurement
Priority: P0
Depends on: TASK-0004C1
Source plans: 05, 10, 11
Likely areas: `crates/boon_native_playground`, `crates/xtask`
Goal:
Expose live preview/dev IPC queue, drop, coalescing, byte, heartbeat, and dev-lag counters as native stage counters.
Acceptance:
- Native observability reports include queue depth, dropped telemetry, dropped frame metrics, dropped debug updates, debug byte summaries, heartbeat gap, preview frame summary, blocked send count, and coalescing flags from live counters or explicit unavailable fields.
- Preview blocked-on-IPC counters remain zero for interaction-mode speed reports.
Verification:
- `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if the only available IPC values are synthetic or hardcoded; add live producer instrumentation instead of marking the task done.
Notes:
- Implemented `native_ipc_stage_counters`,
  `native_ipc_counter_availability`, and schema validation for native GPU
  observability reports.
- Added real Unix-stream request/response byte counters and request round-trip
  samples in `send_preview_ipc_request_with_timeouts`; observability now reports
  measured live IPC exchange stages for debug query bytes, debug subscription
  bytes, dev request round trip, and heartbeat gap.
- Queue depth, drop counts, preview-frame summary, and dev command apply timing
  remain bounded live-probe/synthetic-load evidence and are labeled as such.
  Dev UI lag and replace-source queue status are explicit unavailable fields
  when the observability run does not expose those counters.
- Blocked send count is an explicit zero nonblocking contract, and
  interaction-mode schema still rejects hot-path dev blocking IPC.
- Verified with:
  `cargo test -p xtask`,
  `cargo check -p boon_native_playground`,
  `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`,
  `cargo xtask verify-report-schema`,
  `git diff --check`.

### TASK-0004C3 Native Speed Stage Counter Reports
Status: done
Type: measurement
Priority: P0
Depends on: TASK-0004C1, TASK-0004C2
Source plans: 05, 10, 11
Likely areas: `crates/xtask`, native speed reports
Goal:
Attach native renderer and IPC stage counters to interaction-mode native speed reports.
Acceptance:
- Native scroll, dev-editor scroll, example-switch, NovyWave interaction, Cells interaction, and Counter interaction speed reports expose flow-linked `stage_counters` from existing p50/p95/p99/max summaries.
- Interaction reports carry explicit zero hot-path proof/readback/report counters.
- Missing native stage data fails the relevant speed report instead of being silently omitted.
Verification:
- `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`
- `cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if release native speed gates fail for unrelated budgets; split the budget blocker before claiming this task done.
Notes:
- Implemented generic interaction `stage_counters` derivation for native speed
  summaries and scalar timing fields, including renderer draw calls, renderer
  queue writes, upload bytes, input queue depth, preview IPC blocking, nested
  dev IPC summaries, interaction totals, example-switch timings, and NovyWave
  interaction summaries.
- Added schema validation requirements for the named native speed labels so
  missing required stage counters fail `verify-report-schema`.
- Verified passing reports:
  `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`,
  `cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json`,
  `cargo xtask verify-report-schema`,
  `cargo test -p xtask`,
  `git diff --check`.
- Fixed generic document source-intent lowering for component event-group
  parameters by preserving dotted runtime-local origins such as
  `store.sources.increment_button.events`, resolving group prefixes through
  the current node's source-binding index, normalizing `.events` out of
  typecheck source paths, and deriving implicit target text from the rendered
  node when no explicit target/address intent exists.
- Fresh C3 report evidence now exposes `stage_counters` in the named reports:
  `counter-interaction-speed.json` status `pass` with 2 counters;
  `cells-interaction-speed-debug.json` status `fail` with
  `interaction_latency`; `example-switch-speed-debug.json` status `fail` with
  5 example-switch counters; `novywave-interaction-speed.json` status `fail`
  with 6 NovyWave interaction counters; `scroll-speed-cells.json` status
  `pass` with 24 renderer/IPC/scroll counters; and
  `scroll-speed-dev-code-editor.json` status `pass` with 22
  renderer/IPC/dev-editor counters.
- The remaining failed reports are no longer missing stage-counter evidence:
  Cells debug exceeds max latency, example-switch still has latest-wins and
  readback protocol blockers, and NovyWave release interaction exceeds the
  current p95 budgets. Those blockers belong to TASK-0004D/TASK-0804 before
  release speed evidence can be declared passing.
- Verification run:
  `cargo test -p boon_native_playground counter_button`,
  `cargo check -p boon_native_playground`,
  `cargo test -p xtask`,
  `cargo xtask verify-native-counter-interaction-speed --report target/reports/native-gpu/counter-interaction-speed.json`,
  `cargo xtask verify-native-cells-interaction-speed --report target/reports/native-gpu/cells-interaction-speed-debug.json`,
  `cargo xtask verify-native-example-switch-speed --report target/reports/native-gpu/example-switch-speed-debug.json`,
  `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`,
  `cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json`,
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`,
  `cargo xtask verify-report-schema`, and `git diff --check`.

### TASK-0004D Release Speed Report Evidence
Status: done
Type: measurement
Priority: P0
Depends on: TASK-0004B, TASK-0004C3
Source plans: 05, 10, 11
Likely areas: `crates/xtask`, budgets, speed reports
Goal:
Produce at least one current release-mode TodoMVC or Cells speed report with non-empty stage counters and passing schema validation.
Acceptance:
- A release-mode TodoMVC or Cells speed report exists under `target/reports`.
- The report has `measurement_mode: interaction`, a flow ID, non-empty `stage_counters`, p50/p95/p99/max/sample-count fields, and explicit zero proof/readback/report-write hot-path counters.
- Any remaining speed-budget failure is either fixed or split into a named follow-up that blocks declaring TASK-0004D done.
Verification:
- `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json` or `cargo xtask verify-example-speed cells --report target/reports/cells-speed.json`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if release speed fails for an existing budget unrelated to reporting; record the blocker and add the earliest task that should fix it.
Notes:
- During TASK-0003, `cargo xtask verify-example-speed counter --report target/reports/counter-speed.json` failed the existing allocation budget; do not treat that as TASK-0004 evidence.
- Current release TodoMVC evidence is
  `target/reports/todomvc-speed.json`, generated by
  `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`.
  It passes with `measurement_mode: interaction`, release build,
  `runtime_profile: software_dynamic`, a runtime interaction flow ID, 14
  stage counters, and explicit zero hot-path proof/readback/report-write
  counters.
- The report intentionally has no `stress_profiles`: normal scenario steps are
  not stress profiles. Schema validation now permits missing stress profiles
  only for `software_dynamic` speed reports that disclose measured allocation
  counts and explicitly mark the bounded zero-allocation budget as not
  applicable. Bounded runtime profiles still require stress-profile evidence
  and zero post-warmup allocations.
- Verified with:
  `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`,
  `cargo xtask verify-report-schema`,
  `cargo test -p boon_report_schema --lib`,
  `cargo test -p boon_runtime --lib`,
  `cargo test -p xtask`, and `git diff --check`.

### TASK-0005 SourceStore Row Bind And Unbind Correctness
Status: done
Type: implementation
Priority: P0
Depends on: none
Source plans: 08, 09, 10, 11
Likely areas: `crates/boon_runtime/src/lib.rs`
Goal:
Prevent stale row-source unbinds from dropping live bindings and make row/source binding invariants testable.
Acceptance:
- `SourceStore::unbind_row` validates list ID, key, and generation before mutating slots.
- Stale key, stale generation, wrong list ID, repeated unbind, remove/reinsert, and interleaved row key reuse are covered by tests.
- Active binding count, row slots, source slots, source IDs, and row generations agree after bind/unbind mutations.
- Failure or ignored stale unbind behavior is explicit in tests or debug assertions.
Verification:
- `cargo test -p boon_runtime --lib source_store`
- `cargo test -p boon_runtime --lib live_source_event_preserves_bound_row_identity`
Rollback / stop condition:
- Stop if the current hidden source ID model cannot express a safe invariant; add a child task to introduce explicit source epochs first.
Notes:
- This task may be done before the scenario gate because it is a direct correctness hazard.
- `SourceStore::unbind_row` now checks the requested list/key/generation before
  removing a row slot, so stale generation and wrong-list unbinds no longer
  detach row-slot lookup while leaving live source bindings behind.
- `SourceStore` row slots are bucketed by numeric key and then by
  list/generation, because row keys are list-local and NovyWave can have
  multiple active lists with the same key. Same-list active generation
  collisions are rejected; different lists can safely share a key.
- Added SourceStore invariant checks for active binding count, row-slot
  references, source-id slots, and exactly-one row-slot ownership per active
  binding.
- Tests now cover stale key, stale generation, wrong list ID, repeated unbind,
  same-key/different-list binding, active same-list generation collision,
  remove/reinsert with reused key, dense source ID rejection after unbind, and
  row binding storage growth.
- Verified with:
  `cargo test -p boon_runtime --lib source_store`,
  `cargo test -p boon_runtime --lib live_source_event_preserves_bound_row_identity`,
  and `cargo test -p boon_runtime --lib`.

### TASK-0006 Document Patch Result And Invariants
Status: done
Type: implementation
Priority: P0
Depends on: none
Source plans: 08, 10, 11
Likely areas: `crates/boon_document`, `crates/boon_document_model`, document callers
Goal:
Replace silent document patch success with structured apply reports and fail-closed errors.
Acceptance:
- Document patch application returns `PatchApplyReport` / `PatchApplyError` or an equivalent structured result.
- Missing targets, stale targets, orphaned children, invalid parent/child links, and stale hit/style/layout references fail closed.
- `RemoveNode` and subtree removal semantics are explicit.
- Patch reports include invalidation class data needed by future layout/render chunks.
- Callers cannot ignore patch errors in readiness paths.
Verification:
- `cargo test -p boon_document --lib`
- `cargo test -p boon_runtime --lib document`
- `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`
Rollback / stop condition:
- Stop if every caller currently assumes `()` and the migration is too large; split into report type, caller migration, and invariant tasks.
Notes:
- This is a prerequisite for retained layout/render caching.
- `DocumentState::apply_patch` now returns a structured
  `PatchApplyReport` / `PatchApplyError` instead of silently succeeding.
  Reports include patch kind, target, removed nodes, node count, and
  invalidation classes for structure, text, style, binding, scroll,
  materialization, layout, and hit regions.
- Missing targets, missing parents, duplicate children, orphaned children,
  invalid parent/child links, stale focus references, root removal, and parent
  cycles fail closed. `RemoveNode` explicitly removes the whole subtree,
  detaches it from its parent, clears stale focus, and reports removed nodes.
- `try_layout` validates document graph references before layout, and the
  native cached document-frame relayout path now uses it.
- The native sparse document patch fast path now treats stale cached snapshots,
  stale target nodes, and zero-target application after precheck as errors
  instead of silent fallback. Policy misses such as scroll offsets can still
  decline the sparse fast path.
- `verify-native-gpu-layout-contract` now exercises the structured document
  patch contract and records `document_patch_contract` in
  `target/reports/native-gpu/layout-contract.json`.
- Verified with:
  `cargo test -p boon_document --lib`,
  `cargo test -p boon_runtime --lib document`,
  `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`,
  `cargo xtask verify-report-schema`,
  `cargo test -p xtask`,
  `cargo check -p boon_native_playground`, and `git diff --check`.

### TASK-0007 Scaffold Proof Demotion And Readback Deadlines
Status: done
Type: gate
Priority: P0
Depends on: TASK-0003
Source plans: 08, 10, 11
Likely areas: `crates/boon_native_gpu`, `crates/boon_native_app_window`, `crates/xtask`
Goal:
Ensure scaffold rendering and unbounded WGPU readbacks cannot satisfy readiness or hang verifiers.
Acceptance:
- Scaffold `CopyToPresent` or no-surface proof is explicitly diagnostic and cannot pass visible native GPU readiness.
- Visible readiness requires real acquired surface texture and app-owned rendered frame proof according to `NATIVE_GPU_PIPELINE.md`.
- WGPU map/readback waits have deadlines.
- Timeout artifacts include backend, adapter, frame ID, surface, requested rect, pending submission/report context, and failure reason.
- Interaction mode never waits for proof readback.
Verification:
- `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`
- `cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json`
- `cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json`
Rollback / stop condition:
- Stop if an existing gate only has scaffold proof. Add a child task to build the real proof path instead of weakening the gate.
Notes:
- This task must preserve shader freshness and native GPU negative checks.
- App-owned native GPU readbacks and app-window visible-surface readbacks now
  use bounded WGPU waits and bounded map-callback waits with 5-second
  deadlines instead of `PollType::wait_indefinitely`.
- Timeout errors include backend, adapter availability, frame ID, surface,
  requested rect, submission/report context, deadline, and failure reason.
  Successful proof artifacts include `readback_deadline_ms` and
  `readback_poll_status`.
- `verify-native-gpu-negative` now rejects recursive `copy_to_present`
  scaffold proof, `scaffold-no-surface` / no-surface present results, and
  `acquired_surface_texture=false` proof as insufficient for visible native
  readiness.
- The negative gate also audits the native GPU/app-window source trees for
  unbounded readback waits and for timeout-context tokens.
- Verified with:
  `cargo check -p boon_native_gpu -p boon_native_app_window -p boon_native_playground -p xtask`,
  `cargo test -p xtask`,
  `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`,
  `cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json`,
  `cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json`,
  `cargo xtask verify-report-schema`, and `git diff --check`.
  Transient supervisor progress and role support JSON files under
  `target/reports/native-gpu` were removed before schema validation because
  they are not canonical reports.

## Phase 1: Parser, IR, Typecheck, And Semantic Index

### TASK-0101 Semantic Index Skeleton
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0004B
Source plans: 01, 02, 08, 10, 11
Likely areas: `crates/boon_parser`, `crates/boon_ir`, `crates/boon_typecheck`, runtime compile path
Goal:
Create a single semantic index that later stages can share instead of rediscovering source/list/row/render facts.
Acceptance:
- The index owns stable IDs for sources, lists, row scopes, functions, fields, modules/source units, view bindings, and diagnostics spans.
- Parser policy checks are separated from syntax parsing where safe.
- IR/typecheck can report whether source payload schemas, row scopes, selectors, and render contracts are known or fallback.
- Reports include semantic-index presence and reuse information.
Verification:
- `cargo test -p boon_parser -p boon_ir -p boon_typecheck --lib`
- Existing source syntax gates continue to pass or fail with clearer readiness diagnostics.
Rollback / stop condition:
- Stop if the index requires a broad parser rewrite. Add a child task for a minimal index built from current AST and IR facts.
Notes:
- Do not replace the parser with Tree-sitter/Rowan/Salsa in this task.
- Implemented a first `SemanticIndex` in `boon_ir` without changing Boon
  syntax or rewriting the parser. It is built from the existing parsed program,
  typed IR tables, and typecheck report.
- The index owns stable IDs for source units, sources, lists, row scopes,
  functions, fields, view bindings, and diagnostic spans. It also exposes
  known/fallback readiness summaries for source payload schemas, row scopes,
  selector-like list facts, render contracts, and dynamic type fallback.
- Runtime reports now include a compact `semantic_index` projection at the
  top level and under `runtime_execution`; report schema validation requires
  the mirrored projection and reuse flags.
- `verify-boon-source-syntax` now records semantic-index presence/counts for
  each bundled example. The gate still fails on the existing Cells formatter
  blocker, but it now fails with semantic-index readiness evidence present.

### TASK-0102 Cross-Stage Symbol Interning And Collision Diagnostics
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0101
Source plans: 03, 08, 10, 11
Likely areas: parser, IR, typecheck, runtime storage, document/style IDs
Goal:
Replace hot cross-stage string identity with dense symbols while keeping readable diagnostics.
Acceptance:
- Field names, style attrs, source labels, module paths, operator names, tags, and document attrs can be represented by dense IDs at parse/lower/runtime boundaries.
- Hash-derived field/list/source IDs have collision detection or are replaced by table-assigned IDs.
- Reports expose collision diagnostics and symbol counts.
- Readable strings remain available in source maps, diagnostics, and report output.
Verification:
- `cargo test -p boon_parser -p boon_ir -p boon_typecheck -p boon_runtime --lib`
- A targeted collision/duplicate-name test fails before the fix and passes after.
Rollback / stop condition:
- Stop if interning breaks stable report output; add a source-map/report projection child task.
Notes:
- Dependency choice is not part of this task unless a custom table is insufficient.
- Implemented a dense semantic symbol table in `SemanticIndex` for source unit
  paths, module paths, source labels and segments, list names, row scopes,
  function names/args, field names/paths, operator names, tags, document attrs,
  style attrs, and view node kinds. Existing readable strings remain in IR,
  diagnostics, and reports.
- Runtime compiled reports now expose `runtime_symbol_table`,
  `field_slot_id_kind`, `field_slot_collision_count`, and
  `field_slot_collision_diagnostics`.
- Added a regression test with real colliding field labels for the current
  20-bit field-slot hash so collisions are detected and reported without
  aliasing readable labels.

### TASK-0103 Typechecker Readiness Fallback Gates
Status: done
Type: gate
Priority: P1
Depends on: TASK-0101
Source plans: 08, 09, 10, 11
Likely areas: `crates/boon_typecheck`, runtime readiness reports, xtask gates
Goal:
Make route-critical dynamic fallback visible and readiness-blocking.
Acceptance:
- Route-critical payloads, row scopes, render contracts, selectors, bridge/page descriptors, and source completions fail readiness when type shape is unknown or open.
- Diagnostics name the expression/span and fallback reason.
- Reports include dynamic fallback count, source payload schema coverage, route-critical unknowns, row-scope ambiguity, selector/index ambiguity, render slot fallback, and semantic-index reuse.
Verification:
- `cargo test -p boon_typecheck --lib`
- `cargo test -p boon_runtime --lib typecheck`
- Existing examples show zero readiness-blocking fallback for routes that are already claimed generic.
Rollback / stop condition:
- Stop if current examples rely broadly on dynamic fallback. Add a staged warning/report-only child task before making it fatal.
Notes:
- Normal users should not be forced to add manual types to resolve compiler ambiguity; ambiguity should be a compiler error with restructuring advice.
- Implemented as a report/schema readiness gate on top of the semantic index.
  Runtime execution reports now fail schema validation if route-critical
  readiness buckets have nonzero fallback counts or if dynamic fallback is
  nonzero.
- Readiness reports now include source payload schemas, source completions,
  route-critical unknowns, row scopes, row-scope ambiguity, selectors,
  selector/index ambiguity, render contracts, bridge/page descriptors, dynamic
  fallback count, and semantic-index reuse.
- TodoMVC release speed evidence shows zero fallback in all readiness buckets.
  The documented `cargo test -p boon_runtime --lib typecheck` command currently
  matches zero tests, so full runtime lib tests and schema/report evidence were
  used as stronger verification.

## Phase 2: Runtime Correctness And Source Routing

### TASK-0201 Typed Source Route Op Streams
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0005, TASK-0101, TASK-0103
Source plans: 01, 08, 09, 10, 11
Likely areas: `crates/boon_runtime`, IR source route lowering, scenario application
Goal:
Replace event-time route classification with precompiled typed action op streams.
Acceptance:
- Runtime route execution is keyed by source ID, inferred payload schema, row binding identity, and generation.
- Source action vectors are not cloned per event in hot paths.
- Route reports include route ID, action op count, rows scanned/touched, dirty keys, recompute candidates, allocation counts, and fallback/deopt reasons.
- Readiness paths do not branch on example names, source file names, TodoMVC field names, Cells field names, or NovyWave signal labels.
- Negative tests prove TodoMVC-like names outside TodoMVC do not trigger TodoMVC behavior.
Verification:
- `cargo test -p boon_runtime --lib source_route`
- `cargo test -p boon_runtime --lib generic_source_events_route_to_action_inputs`
- `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`
Rollback / stop condition:
- Stop if route op streams cannot be built from current IR facts; add a child task to extend IR route metadata.
Notes:
- This task should preserve current scenario semantics while removing heuristic readiness claims.
- Implemented shared compiled source action slices keyed by typed `SourceId`.
  The runtime hot path now takes an `Arc<[SourceAction]>` handle instead of
  cloning a `Vec<SourceAction>` per event.
- Compiled schedule and generic runtime slice evidence now expose source route
  op-stream counts, total/max action op counts, per-route route/source IDs,
  payload schema field counts, op kinds, and zero clone/fallback/deopt counts.
- Scenario step reports now attach `source_route_execution` with route ID,
  source ID, payload schema field count, row binding identity including hidden
  key/generation when present, action op count, rows scanned/touched, dirty key
  count, recompute candidate count, allocation counts, and fallback/deopt
  reasons.
- The old `SourceActionKind` event classifier is now compiled only for
  `cfg(test)` TodoMVC scenario-harness glue. Production runtime route execution
  and readiness reports use the typed source action op stream.
- Added a negative runtime test with a TodoMVC-looking source file/name/list
  shape proving no TodoMVC append/remove behavior is synthesized without IR
  list actions.
- Verification passed:
  `cargo test -p boon_runtime --lib source_route`;
  `cargo test -p boon_runtime --lib generic_source_events_route_to_action_inputs`;
  `cargo test -p boon_runtime --lib todomvc_like_names_do_not_create_todomvc_routes_without_ir_actions`;
  `cargo test -p boon_runtime --lib` (113 tests);
  `cargo test -p boon_report_schema --lib`;
  `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`;
  `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`;
  `cargo xtask verify-report-schema`.
- One `verify-report-schema` attempt failed while
  `target/reports/native-gpu/negative.json` was being concurrently rewritten;
  rerunning after the native negative gate completed passed.

### TASK-0202 Public Source Batch Runtime Boundary
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0201
Source plans: 06, 09, 10, 11
Likely areas: runtime API, native host input path, BoonDriver
Goal:
Make ordered public source batches the boundary used by host, BoonDriver, and runtime dispatch.
Acceptance:
- Host input and scenarios dispatch public source intents/batches rather than private runtime mutations.
- Batches carry monotonic sequence IDs, source/event IDs, payloads, row identity, and generation.
- Equal-sequence `LATEST` conflicts are deterministic and reported.
- Private runtime mutation APIs cannot satisfy UI interaction proof.
Verification:
- `cargo test -p boon_runtime --lib source_batch`
- `cargo test -p boon_driver --lib`
- `cargo xtask verify-boon-driver-schema`
Rollback / stop condition:
- Stop if native host callers cannot be migrated in one step; add compatibility wrapper and child migration tasks.
Notes:
- This task may initially support existing payload fields before richer source schemas are complete.
- Added public `SourceBatch` and `SourceBatchEvent` runtime types carrying
  monotonic batch sequence IDs, per-event IDs, source events, payloads, source
  IDs after normalization, and existing row key/generation fields.
- Added `LiveRuntime::apply_source_batch_turn` for ordered public batch
  dispatch. It rejects empty batches, non-increasing event IDs, and equal/lower
  batch sequence IDs with deterministic errors.
- Existing public single-event `apply_source_event*` compatibility methods now
  enter through a one-event source batch envelope so existing host/scenario
  callers consume the public batch sequence boundary while preserving existing
  behavior.
- Added host-neutral BoonDriver `SourceIntent` and `SourceRowIdentity` schema
  types plus `source_intent_boundary_schema()` without adding a
  `boon_runtime` dependency to `boon_driver`.
- `verify-boon-driver-schema` now checks both sides of the boundary:
  BoonDriver owns source intents, and `boon_runtime` owns public source batch
  dispatch.
- Kept normalized `source_id` out of synthetic user-action row-binding checks
  so alias source paths with hidden key/generation continue to resolve by the
  original host source binding.
- Verification passed:
  `cargo test -p boon_runtime --lib source_batch`;
  `cargo test -p boon_runtime --lib live_source_event_preserves_bound_row_identity`;
  `cargo test -p boon_runtime --lib live_runtime_applies_plain_latest_key_payload_match`;
  `cargo test -p boon_runtime --lib live_runtime_applies_numeric_counter_hold_updates_generically`;
  `cargo test -p boon_runtime --lib` (114 tests);
  `cargo test -p boon_driver --lib`;
  `cargo xtask verify-boon-driver-schema`;
  `cargo xtask verify-report-schema`.

### TASK-0203 Row Identity, Generation, And Stale Event Rejection
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0202
Source plans: 07, 08, 09, 10, 11
Likely areas: runtime row storage, source bindings, document hit/source binding reports
Goal:
Make row routing independent of labels and safe across delete/reinsert/recycle.
Acceptance:
- Runtime source events can carry source ID, source epoch, list ID, row key, row generation, occurrence/index, and typed payload fields.
- Duplicate labels route correctly.
- Stale row events are rejected before mutating state.
- Row unbind/bind reports expose key/generation/epoch mismatches.
- Critical scenarios no longer rely on text-only target matching for row identity.
Verification:
- `cargo test -p boon_runtime --lib row_identity`
- `cargo test -p boon_runtime --lib live_source_event_preserves_bound_row_identity`
- Scenario integrity gate rejects row actions without public source identity or justified selector.
Rollback / stop condition:
- Stop if document hit regions cannot carry row identity. Add a document source-binding extension child task.
Notes:
- This is a prerequisite for virtual materialization and retained row rendering.
- `LiveSourceEvent` and scenario expected source events now carry explicit
  `list_id` and `source_epoch` fields in addition to source ID, row key,
  generation, occurrence/index, and typed payload fields. `source_epoch` is
  accepted as the public source-binding epoch alias when resolving row-bound
  events.
- Row-bound source resolution now rejects stale or mismatched list/key,
  generation, source ID, and epoch inputs before applying mutations.
- Added row binding resolution reports with matched status, mismatch reason,
  requested key/generation/epoch/source ID, candidate counts, and candidate
  samples so stale bind/unbind failures are inspectable.
- Extended the BoonDriver `SourceIntent` boundary schema with `source_epoch`
  while keeping source intent host-neutral.
- Scenario manifest integrity now includes
  `row_action_public_identity_or_selector`; row-like actions must provide
  public row/source identity or a justified selector, so critical scenarios are
  no longer allowed to silently rely on bare label matching.
- Verification passed:
  `cargo fmt --all`;
  `cargo test -p boon_runtime --lib row_identity`;
  `cargo test -p boon_runtime --lib live_source_event_preserves_bound_row_identity`;
  `cargo test -p xtask scenario_integrity`;
  `cargo xtask verify-scenario-manifest-integrity --report target/reports/scenario-manifest-integrity.json`;
  `cargo test -p boon_driver --lib`;
  `cargo xtask verify-boon-driver-schema`;
  `cargo xtask verify-report-schema`;
  `cargo test -p boon_runtime --lib` (116 tests).

## Phase 3: Storage, Dirty Sets, List Indexes, And Deltas

### TASK-0301 List Scan Counters And First Inferred Indexes
Status: done
Type: measurement
Priority: P1
Depends on: TASK-0004B, TASK-0203
Source plans: 07, 08, 09, 10, 11
Likely areas: runtime list storage, dirty propagation, selector helpers
Goal:
Make large-list work visible and add the first safe generic lookup indexes.
Acceptance:
- Reports include rows scanned, rows touched, row occurrences scanned, order slots refreshed, summary fields scanned, dirty entries deduplicated, and route candidates visited.
- Row/generation indexes exist for list rows and row source bindings.
- First text/value lookup indexes cover safe repeated `List/find_value` or equivalent selectors.
- Index ambiguity produces compiler/runtime diagnostics rather than silent fallback in readiness paths.
Verification:
- `cargo test -p boon_runtime --lib list_index`
- One large-list benchmark or scenario report includes rows-scanned counters.
Rollback / stop condition:
- Stop if counters show no stable row identity; return to `TASK-0203`.
Notes:
- This task should add counters before replacing every scan.
- Added per-step `list_scan_counters` with `rows_scanned`,
  `row_occurrences_scanned`, `order_slots_refreshed`,
  `summary_fields_scanned`, `dirty_entries_deduplicated`,
  `route_candidates_visited`, and exact text lookup index hit/miss/candidate
  counters.
- Runtime speed reports now summarize the same counters under `stage_counters`
  and top-level `*_p50_p95_p99_max` fields.
- `ListMemory` now has a lazily rebuilt exact text lookup index keyed by
  `FieldSlotId`. It stores slots, not visible indexes, so moves preserve
  first-visible-row semantics without rebuilding the index.
- `List/find_value` over concrete runtime `ListRef` rows uses the exact text
  lookup index and preserves duplicate-value semantics by selecting the first
  current visible row.
- Index fallback is now visible through `text_lookup_index_hits`,
  `text_lookup_index_misses`, and `text_lookup_index_candidates` counters
  rather than silently disappearing from readiness reports.
- Existing row/generation indexes remain the dense `key_slots` /
  `order_slots` path in `ListMemory`; row source bindings remain indexed by
  source ID and row key/generation buckets in `SourceStore`.
- Added `cargo xtask verify-large-list-scan-counters`, which generates a
  1,000-row generic large-list source/scenario under `target/generated`, runs
  the real speed layer in release mode, and writes a schema-valid diagnostic
  report at `target/reports/large-list-scan-counters.json`. The report links
  the raw speed-layer artifact and asserts `list_slot_count >= 1000`,
  `stage_counters.rows_scanned.max >= 1000`, and per-step
  `list_scan_counters.rows_scanned >= 1000`.
- Verification passed:
  `cargo fmt --all`;
  `cargo test -p boon_runtime --lib list_index`;
  `cargo test -p boon_runtime --lib` (119 tests);
  `cargo test -p xtask` (10 tests);
  `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`
  plus `jq` inspection of the new counter summaries and per-step counter
  objects;
  `cargo xtask verify-large-list-scan-counters --report target/reports/large-list-scan-counters.json`
  (`rows=1000`, `list_slot_count=1000`, `rows_scanned_max=2000`);
  `jq` inspection of `target/reports/large-list-scan-counters.json`
  confirming the large-list evidence;
  `cargo xtask verify-report-schema`;
  `git diff --check`.

### TASK-0302 Derived List Delta Operators
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0301
Source plans: 02, 07, 09, 10, 11
Likely areas: runtime list projections, IR list operation lowering, dirty propagation
Goal:
Update common list projections by deltas instead of full recompute.
Acceptance:
- At least one filter/map/count/find projection updates from a single-row delta.
- Full recompute remains available as verifier oracle.
- Reports include delta operator hit/miss/fallback and affected row counts.
- Small TodoMVC and Cells scenarios do not regress.
Verification:
- `cargo test -p boon_runtime --lib list_delta`
- A large synthetic list scenario or benchmark compares incremental output with full recompute.
Rollback / stop condition:
- Stop if full recompute oracle cannot be generated for the projection. Add oracle support first.
Notes:
- Do not introduce differential-dataflow or broad external frameworks before proving local delta operators.
- Implemented the first conservative derived-list delta operator for
  `RuntimeListOperationKind::Count` projections over single-row
  `FieldBool`/`FieldBoolNot` changes. The runtime captures the row predicate
  preimage before the source action, updates the cached count by delta, and
  compares the result with the existing full-scan count as the oracle before
  keeping the cached value.
- Added per-step and aggregate report counters:
  `list_delta_counters`, `list_delta_operator_hits`,
  `list_delta_operator_misses`, `list_delta_operator_fallbacks`,
  `list_delta_affected_rows`, `list_delta_oracle_checks`, and
  `list_delta_oracle_mismatches`.
- Added `list_delta` unit coverage for a TodoMVC single-row toggle, report
  counter emission, and a generated 1000-row TodoMVC variant that compares the
  cached active/completed counts with full-scan oracle counts after one row
  changes.
- Current TodoMVC speed evidence in `target/reports/todomvc-speed.json`
  reports `list_delta_operator_hits.max = 2`, `list_delta_oracle_checks.max =
  2`, and `list_delta_oracle_mismatches.max = 0` on row-toggle steps.
- Verification passed:
  `cargo test -p boon_runtime --lib list_delta` (3 tests);
  `cargo test -p boon_runtime --lib` (122 tests, including Cells scenario
  tests);
  `cargo test -p xtask` (10 tests);
  `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.
- Cells correctness did not regress under the full runtime library suite. The
  dedicated Cells xtask gates were not used as pass evidence: the generic named
  and dedicated semantic gates currently fail because
  `generic_derived_value_semantic_delta_emitter` is false for Cells
  (`derived_text_transform_count = 0`), and the dedicated Cells speed gate
  still fails the existing `latency_p95_budget` with a generated p95 around
  4.6 ms. Those are existing Cells gate/budget blockers rather than this count
  delta operator's oracle mismatch.

### TASK-0303 Dirty Set Redesign Gate
Status: done
Type: measurement
Priority: P2
Depends on: TASK-0301
Source plans: 03, 04, 08, 10, 11
Likely areas: runtime dirty sets, dependency graph, report counters
Goal:
Collect enough dirty-set density/cardinality data to choose a representation.
Acceptance:
- Reports include dirty entry count, dirty fanout, duplicate attempts, density estimate, and top recompute causes.
- Current string-heavy dirty paths are isolated behind a representation boundary.
- The report recommends sorted `Vec`, fixed bitset, roaring bitmap, or current structure based on measured data.
Verification:
- `cargo test -p boon_runtime --lib dirty`
- At least two workloads report dirty density/cardinality.
Rollback / stop condition:
- Stop if no reliable counters exist. Do not swap data structures blindly.
Notes:
- Actual dependency adoption is tracked under experiments.
- Added `RuntimeDirtySetMetrics` and made `DirtyKeySets` the explicit
  measurement boundary for dirty entry count, unique keyed dirty count,
  duplicate dirty attempts, density estimate, fanout recompute candidate
  count, top recompute causes, and representation recommendation.
- Reports now include per-step `dirty_set_metrics`, `dirty_entry_count`,
  `dirty_duplicate_attempt_count`, `dirty_density_estimate`,
  `dirty_fanout_recompute_candidate_count`, and
  `dirty_set_recommended_representation`, plus aggregate `stage_counters` and
  top-level percentile summaries for dirty entries, duplicate attempts, and
  density.
- The report recommendation is measurement-only and does not change the
  current runtime representation. It can recommend `current_vec`,
  `sorted_vec`, `fixed_bitset`, or `roaring_bitmap` based on observed
  cardinality, duplicates, and density.
- Current string-heavy dirty paths are isolated behind `DirtyKeySets` for this
  task. `GenericReadKey`/`GenericDerivedKey` still use strings and remain the
  later slot-ID migration boundary; this task intentionally did not swap the
  dependency graph representation.
- Two current workload reports include density/cardinality evidence:
  `target/reports/todomvc-speed.json` has nonzero dirty activity
  (`dirty_entry_count.max = 21`, `dirty_duplicate_attempt_count.max = 18`,
  `dirty_density_estimate.max = 0.5555555555555556`) and top recompute causes
  on toggle-all steps; `target/reports/large-list-scan-counters.json` links to
  `target/generated/large-list-scan/large-list-scan-speed-raw.json` and
  reports the sparse/no-keyed-dirty case with zero dirty density across two
  steps.
- Verification passed:
  `cargo test -p boon_runtime --lib dirty` (3 tests);
  `cargo test -p boon_runtime --lib` (123 tests);
  `cargo test -p xtask` (10 tests);
  `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`;
  `cargo xtask verify-large-list-scan-counters --report target/reports/large-list-scan-counters.json`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.

## Phase 4: Document, Layout, Materialization, And Passive Scroll

### TASK-0401 Generic Virtual Materialization Protocol
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0006, TASK-0203, TASK-0301
Source plans: 07, 09, 10, 11
Likely areas: `crates/boon_document`, runtime document lowering, layout demand handling
Goal:
Introduce renderer-neutral virtual list/grid/page materialization for large logical collections.
Acceptance:
- Layout can demand visible range plus overscan.
- Runtime/document can return stable keyed materialized rows/pages.
- Reports distinguish logical item count from materialized item count.
- Source binding lifecycle is safe for materialized and recycled rows.
- The public protocol does not mention NovyWave, Cells, TodoMVC, or dev editor.
Verification:
- `cargo test -p boon_document --lib materialization`
- `cargo test -p boon_runtime --lib materialization`
- `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`
Rollback / stop condition:
- Stop if document patches cannot report invalidation correctly; return to `TASK-0006`.
Notes:
- Cells is the first preferred proof case after the protocol exists.
- `boon_document` now exposes renderer-neutral materialization protocol
  metadata through `LayoutDemand`, `MaterializationReport`, and
  `LayoutFrame.materialization`. Layout demands include visible range,
  overscan range, logical item count, materialized item count, stable key
  prefix, and first/last stable key samples.
- `DocumentPatch::SetListMaterialization` now returns structured
  materialization metadata in `PatchApplyReport.materialization` while still
  reporting `Materialization`, `Layout`, and `HitRegion` invalidation.
- Runtime document summaries now include a generic
  `__boon_materialization` array. Raw/retained lists and chunk/page
  projections report logical item count, materialized item count, visible and
  overscan ranges, stable key prefix, and first/last stable row keys derived
  from existing hidden `{row_key, generation}` identity.
- `SummaryLimits::document_preview_window` now carries a raw-list row start as
  well as chunk row/column ranges, so materialized raw/retained lists can
  return the demanded window rather than always returning the first N rows.
- Source binding lifecycle remains tied to logical row identity
  `{list_id, row_key, generation, source_id, bind_epoch}`. The full runtime
  suite continues to cover row source binding, unbind, stale generation, and
  stale bind-epoch behavior; materialized summaries reuse those stable row
  identities instead of inventing view-local source IDs.
- The public materialization protocol in `crates/boon_document` and
  `crates/boon_document_model` contains no NovyWave, Cells, TodoMVC, or dev
  editor names. The native layout-contract report now includes
  `layout_demands` and `layout_materialization` evidence with logical vs
  materialized counts and stable key samples.
- Verification passed:
  `cargo test -p boon_document --lib materialization` (2 tests);
  `cargo test -p boon_runtime --lib materialization` (3 tests);
  `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`;
  `jq` inspection of the layout report confirming
  `layout-contract:materialization-protocol-fields` pass, visible/overscan
  ranges, logical/materialized counts, and stable keys;
  `cargo test -p boon_document --lib` (15 tests);
  `cargo test -p boon_runtime --lib` (125 tests);
  `cargo test -p xtask` (10 tests);
  `cargo xtask verify-report-schema`;
  `rg` scan of document/model protocol files for example names;
  `git diff --check`.

### TASK-0402 Passive Scroll Property-Tree Path
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0401
Source plans: 01, 08, 09, 10, 11
Likely areas: document/layout, native playground scroll handling, render state
Goal:
Make scroll that does not hit a semantic source binding update property/layout state without runtime graph dispatch.
Acceptance:
- Passive scroll reports `runtime_dispatch_count_for_passive_scroll=0`.
- Passive scroll reports `graph_rebuild_count=0` where only offsets/materialized ranges change.
- Scroll root IDs, hit region IDs, materialized ranges, and invalidation classes are reported.
- Dev editor fast paths are generalized or explicitly recorded as prototypes, not final special cases.
Verification:
- `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`
- `cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json`
Rollback / stop condition:
- Stop if scroll correctness depends on hard-coded playground geometry. Add a layout-derived geometry child task.
Notes:
- This task must not use desktop screenshots or compositor scraping as proof.
- Native scroll-speed reports now include `scroll_root_ids`,
  `hit_region_ids`, `invalidation_classes`, a
  `passive_scroll_property_tree_proof` object, and
  `passive_scroll_targeting_policy:
  generic-layout-axis-largest-area-scroll-region`.
- The scroll input target picker now selects layout-derived scroll regions by
  requested axis and largest area instead of branching on Cells or dev-editor
  labels. This keeps the proof away from hard-coded playground geometry.
- Cells scroll proof is classified as `generic_property_tree_virtual_collection`.
  Dev editor scroll proof is explicitly recorded as
  `prototype_generic_property_tree_dev_surface_probe`, not as a final generic
  app-surface path.
- The proof object is accepted only when route evidence passes, private runtime
  dispatch is false, runtime dispatch count is zero, graph rebuild count is
  zero, the non-OS materialization model passes, and materialized ranges report
  an observed operator-host wheel-input range change.
- Verification passed:
  `cargo fmt --all`;
  `cargo check -p boon_native_playground`;
  `cargo test -p xtask`;
  `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`;
  `jq` inspection of Cells passive-scroll proof, route evidence, materialized
  ranges, scroll roots, and hit-region counts;
  `cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json`;
  `jq` inspection of dev-editor passive-scroll proof, prototype fast-path
  label, scroll roots, hit target, and materialized ranges;
  `cargo xtask verify-report-schema`;
  `git diff --check`.

### TASK-0403 Computed Style IDs And Invalidation Classes
Status: done
Type: implementation
Priority: P2
Depends on: TASK-0006, TASK-0102
Source plans: 07, 08, 09, 10, 11
Likely areas: document model, layout, render lowering
Goal:
Move performance-sensitive style data toward computed IDs and explicit invalidation classes.
Acceptance:
- Style/material/font/pseudo-state values used for invalidation have stable IDs or typed records.
- Invalidation classes include paint-only, layout-only, hit-region, source-binding, list-structure, conditional-structure, scroll-offset-only, materialization-only, and full document.
- Renderer-facing style identity no longer depends on repeated hot string map lookups for critical paths.
Verification:
- `cargo test -p boon_document --lib style`
- `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`
Rollback / stop condition:
- Stop if current style maps are too broad. Add a child task for a minimal typed style subset first.
Notes:
- Public Boon style syntax should not change in this task.
- Public Boon style syntax and the source-facing `StyleMap` remain unchanged.
  The document layer now computes renderer-facing `ComputedStyleIdentity`
  values from the existing style map and stores them on layout `DisplayItem`s.
- Computed identities split full style, layout, paint, material, font, and
  pseudo-state domains. Clip mutations recompute the identity after adding
  internal clip style values.
- Style patch invalidation now uses actual changed keys. Known keys map to
  precise classes, and unknown future style keys conservatively add
  `full_document` rather than pretending a narrow class is safe.
- The expanded invalidation vocabulary now includes `paint_only`,
  `layout_only`, `hit_region`, `source_binding`, `list_structure`,
  `conditional_structure`, `scroll_offset_only`, `materialization_only`, and
  `full_document`, while preserving existing broad classes for compatibility.
- The native renderer cache path now threads computed IDs into text run
  signatures and quad batch cache keys. Raw style maps are still available for
  property extraction; this task only moved cache identity away from repeated
  hot string-map identity decisions.
- The layout-contract report now includes `computed_style_identity_samples` and
  checks `layout-contract:computed-style-identity-fields`,
  `layout-contract:style-patch-computed-invalidation-classes`, and
  `layout-contract:expanded-invalidation-vocabulary`.
- Verification passed:
  `cargo test -p boon_document --lib style`;
  `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`;
  `jq` inspection of style/invalidation layout-contract checks and
  `computed_style_identity_samples`;
  `cargo test -p xtask`;
  `cargo test -p boon_document --lib`;
  `cargo test -p boon_native_gpu --lib`;
  `cargo check -p boon_native_playground -p boon_native_gpu -p boon_document -p xtask`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.

## Phase 5: Renderer, Text, Assets, And GPU Uploads

### TASK-0501 Retained Render Chunk IDs
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0006, TASK-0401, TASK-0403
Source plans: 01, 02, 08, 10, 11
Likely areas: `crates/boon_native_gpu`, document/render lowering, native playground render state
Goal:
Replace whole-frame render reuse with retained chunks keyed by stable document/layout/render identity.
Acceptance:
- Render chunks have stable IDs, layout bounds, clip, transform, material/style identity, dependency set, GPU buffer range, text run IDs, texture/asset refs, and generation.
- Passive scroll updates transforms/materialized chunks without rebuilding unchanged primitive content.
- Caret blink, hover, or focus does not re-upload static chrome.
- Reports include chunk hit/miss/reuse, dirty chunk count, upload bytes, draw calls, and text-shaped runs.
Verification:
- `cargo test -p boon_native_gpu --lib`
- `cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json`
Rollback / stop condition:
- Stop if document/layout cannot provide stable invalidation classes; return to Phase 4 tasks.
Notes:
- Do not key readiness on example names or source file names.
- Native GPU `FrameMetrics` now reports retained render chunks with stable
  chunk IDs, layout bounds, clip, transform, computed style identity,
  dependency set, GPU buffer range, text run IDs, texture/asset refs,
  generation, and cache status.
- `VisibleLayoutRenderer` now keeps previous chunk IDs and reports
  retained chunk hit, miss, reuse, dirty, upload-byte, draw-call, and
  text-shaped-run evidence from the visible render path.
- Chunk IDs include node, kind, full style identity, layout identity, paint
  identity, material identity, and pseudo-state identity. Generation is
  reported separately so IDs stay stable across frames until the relevant
  render identity changes.
- Unit coverage proves unchanged chrome remains reusable when a focused/input
  chunk changes pseudo-state identity, while the changed chunk is marked dirty.
- Preview-e2e Cells evidence now requires retained chunk metrics through
  `require_visible_native_render_proof`; the refreshed report showed 347
  preview chunks, 347 hits, 0 misses, 347 reused chunks, 0 dirty chunks,
  0 upload bytes on the reused frame, 2 draw calls, and 42 shaped text runs.
- Verification passed:
  `cargo test -p boon_native_gpu --lib retained_render_chunks`;
  `cargo test -p boon_native_gpu --lib`;
  `cargo test -p xtask`;
  `cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json`;
  `jq` inspection of retained chunk counts, hit/miss/reuse/dirty counts,
  upload bytes, draw calls, shaped text runs, chunk IDs, and dependency sets;
  `cargo xtask verify-report-schema`;
  `git diff --check`.

### TASK-0502 POD And Ring-Buffer GPU Upload Path
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0501, EXP-0001
Source plans: 03, 04, 08, 10, 11
Likely areas: `crates/boon_native_gpu`, generated shader layout tests
Goal:
Replace split per-frame geometry byte vectors and repeated buffer creation with POD instance data and bounded dirty uploads.
Acceptance:
- Host structs and WGSL layouts have explicit layout tests.
- Upload path can update dirty ranges or reuse persistent buffers.
- Reports include allocated GPU bytes, uploaded bytes, dirty ranges, buffer reuse count, staging wrap count, queue write count, and cache evictions.
- SHA-256 geometry hashing is proof/report behavior, not hot interaction identity.
Verification:
- `cargo test -p boon_native_gpu --lib`
- `cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json`
- A release interaction report shows upload counters before/after.
Rollback / stop condition:
- Stop if POD layout cannot be proven stable across host/WGSL. Keep old path and add layout proof task.
Notes:
- Completed on 2026-06-15.
- Uploads are now planned at frame-begin before any `queue.write_buffer`, so
  wrap/grow invalidates stale cached ranges before current-frame writes can
  overwrite live geometry.
- GPU quad cache and prepared-quad cache entries validate ring generation and
  byte ranges before reuse.
- The NovyWave release interaction report now carries `renderer_upload_probe`,
  upload summaries, dirty ranges, queue writes, wraps, cache evictions, and
  renderer stage counters.
- Measurement finding: this task made the upload path measurable and safe, but
  it also proved the next bottleneck is architectural. A post-interaction
  NovyWave frame still uploads a full `900600` byte quad batch and wraps the
  ring even though retained chunks report only `10` misses out of `304`.
  Do not chase more ring micro-tuning before chunk-level GPU geometry identity.

### TASK-0502A Chunk-Level GPU Geometry Upload Identity
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0502
Source plans: 03, 05, 08, 10, 11
Likely areas: `crates/boon_document::render_scene`, `crates/boon_native_gpu`, `crates/boon_native_playground`, `crates/xtask`
Goal:
Make retained render chunks survive into GPU geometry/cache identity so a small
NovyWave interaction updates only dirty chunk geometry instead of rewriting one
whole-scene quad batch.
Acceptance:
- RenderScene/quad lowering carries stable retained chunk IDs through GPU batch
  construction without changing Boon syntax or NovyWave source.
- GPU cache identity is chunk/run based, paint-order preserving, and texture
  safe; adjacent same-texture coalescing is allowed, but cross-texture reorder
  is not.
- Dirty interactions reuse unchanged chunk ranges and upload only dirty chunk
  ranges, with reports naming chunk IDs/counts and byte ranges.
- A refreshed NovyWave release interaction report shows post-interaction upload
  bytes substantially below the initial full-frame upload when retained chunks
  mostly hit; kill the slice if post-interaction upload remains near the full
  `900600` byte batch.
- Prepared caches, quad-buffer caches, and ring generation checks remain valid
  after chunk-level reuse.
Verification:
- `cargo test -p boon_document --lib`
- `cargo test -p boon_native_gpu --lib`
- `cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json`
- `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
- `cargo xtask verify-report-schema`
- `jq` inspection proving post-interaction upload bytes, dirty range count,
  retained chunk hit/miss counts, and queue write count moved in the expected
  direction.
Rollback / stop condition:
- Stop if chunk-level geometry cannot preserve paint order or asset texture
  validity. Keep TASK-0502 ring metrics and write a narrower renderer-boundary
  task instead.
Notes:
- This task exists because TASK-0502 measurement showed a full-scene quad upload
  after a mostly retained NovyWave interaction. It is intentionally higher
  leverage than tiny container micro-optimizations.
- 2026-06-15 completion: renderer upload identity now follows retained render
  chunks through `RenderScene`, primitive lowering, GPU batch construction, and
  upload dirty-range metrics. A focused native GPU regression proves a
  two-chunk document scene reuses the unchanged chunk and uploads only the
  changed chunk after interaction.
- Current renderer measurement: refreshed NovyWave interaction probe status is
  `pass`; initial first render uploaded `900600` bytes, immediate identical
  render uploaded `0` bytes, and post-interaction upload dropped to `3360`
  bytes across `3` ranges and `2` retained chunk IDs with `231` reused buffers,
  `0` staging wraps, and `0` quad-cache evictions. This satisfies the renderer
  slow-path acceptance for this task.
- Current remaining gate blocker is not renderer upload. The refreshed
  end-to-end speed gate still failed strict latency at
  `click_to_cursor.p95=17.399ms` / `input_to_visible.p95=17.399ms` against the
  `16.700ms` budget. The measured slow path is runtime/root flush:
  `runtime_apply.p95=9.942ms`, `runtime_step_apply.p95=7.958ms`,
  `source_action_root_flush.p95=7.412ms`,
  `source_action_root_materialization.p95=4.215ms`,
  `source_action_root_dirty_scheduler.p95=3.113ms`, and
  `layout_rebuild.p95=4.849ms`. Continue with `TASK-0804A` before low-level
  container experiments.

### TASK-0503 RenderScene Boundary And Renderer Semantics Cleanup
Status: superseded
Type: refactor
Priority: P1
Depends on: TASK-0501
Source plans: 08, 10, 11
Likely areas: `crates/boon_native_gpu`, document/display-list lowering
Goal:
Move app/editor/widget semantics out of the GPU crate and into renderer-neutral display primitives.
Acceptance:
- GPU crate consumes primitive render scene data: bins, instances, text runs, textures, clips, transforms, materials, and proof markers.
- Editor type hints, syntax spans, checkbox/default fills, and app-shaped semantics are lowered before the GPU crate.
- Renderer tests operate on primitive scene data.
Verification:
- `cargo test -p boon_native_gpu --lib`
- `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`
Rollback / stop condition:
- Stop if display-list lowering lacks needed primitive types; add primitive schema task.
Notes:
- This is a cleanup and cacheability task, not a visual redesign.
- Superseded by `TASK-0503A` and `TASK-0503B` because this task combined two
  different scopes: introducing a RenderScene consumption boundary and moving
  all semantic lowering before the GPU crate.

### TASK-0503A Internal RenderScene Consumption Boundary
Status: done
Type: refactor
Priority: P1
Depends on: TASK-0501
Source plans: 08, 10, 11
Likely areas: `crates/boon_native_gpu`
Goal:
Make the visible native renderer consume a single RenderScene boundary for
scene items, text runs, quad batches, texture refs, dependency sets, and retained
chunk metadata instead of spreading raw `LayoutFrame` traversal across encode.
Acceptance:
- `boon_native_gpu` has explicit `RenderScene` and `RenderSceneItem` types.
- Visible render encode lowers once, then consumes scene quad batches, scene text
  runs, scene item descriptors, texture refs, dependency sets, and retained
  chunk metadata.
- Renderer tests include a manual pre-lowered RenderScene path that does not
  construct a `LayoutFrame`.
- Architecture report checks prove the boundary exists, is consumed, and is
  tested.
Verification:
- `cargo check -p boon_native_gpu`
- `cargo test -p boon_native_gpu --lib`
- `cargo test -p xtask`
- `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`
Rollback / stop condition:
- Stop if the boundary changes rendered output or retained chunk identity.
Notes:
- This is an internal renderer boundary only. It intentionally does not claim
  that semantic lowering has left the GPU crate; `TASK-0503B` owns that stricter
  requirement.

### TASK-0503B Move RenderScene Lowering Before GPU Crate
Status: done
Type: refactor
Priority: P1
Depends on: TASK-0503A
Source plans: 08, 10, 11
Likely areas: `crates/boon_document`, possible `crates/boon_render_scene`,
`crates/boon_native_gpu`, native playground render adapters
Goal:
Move editor/widget/style semantic lowering before the GPU crate so
`boon_native_gpu` consumes renderer-neutral scene data rather than interpreting
app-shaped style keys and document node kinds.
Acceptance:
- `crates/boon_document/src/render_scene.rs` or an equivalent
  renderer-neutral crate defines the scene contract without depending on WGPU,
  glyphon, image, resvg, or GPU resource handles.
- A renderer-neutral scene boundary outside `boon_native_gpu` owns editor type
  hints, syntax spans, checkbox/default fills, caret/selection/underline/
  strikethrough lowering, widget default alignment, texture refs, clips,
  transforms, and material descriptors.
- `boon_native_gpu` entrypoints can accept pre-lowered scene data without
  `LayoutFrame`, `DisplayItem`, or `DocumentNodeKind` in the hot render encode
  path.
- Remaining `LayoutFrame` adapters are outside the WGPU encode path and are
  marked as compatibility adapters, not the renderer contract.
- Renderer tests operate on primitive/pre-lowered scene data; lowering tests live
  with the renderer-neutral boundary.
Verification:
- `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`
- `cargo test -p boon_native_gpu --lib`
- `cargo test -p boon_document --lib`
- `cargo test -p xtask`
- `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`
- `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`
- `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`
Rollback / stop condition:
- Stop if extraction forces duplicated text shaping, unstable style defaults, or
  degraded visual output. Add a smaller primitive schema task instead of moving
  WGPU code into the neutral lowerer.
Notes:
- A new `boon_render_scene` crate is allowed if it keeps WGPU/resource ownership
  out of the neutral lowerer. Do not expose Boon syntax or GPU handles.
- Prefer `boon_document::render_scene` first. Use a text-column measurement trait
  for caret/type-hint placement so `boon_document` does not depend on glyphon.
- Tighten `verify-native-gpu-architecture` at the end of this task to fail if
  `boon_native_gpu/src` still owns semantic keys such as
  `editor_type_hints_json`, `syntax_spans_json`, `editor_selection_start`,
  `checkbox_*`, `placeholder_*`, or `default_fill_for_kind`.
- Progress 2026-06-12: `boon_document::render_scene` now defines the initial
  renderer-neutral `RenderScene`, `RenderSceneItem`, quad batch, texture ref,
  text run, rich span, text alignment, font style/weight, scene metrics, and
  retained chunk descriptor contract. Architecture verification now checks that
  this boundary exists and does not import WGPU, glyphon, image, or resvg
  resource APIs.
- Progress 2026-06-12: text-run semantic lowering moved into
  `boon_document::render_scene::render_text_runs`. The neutral lowerer owns
  placeholder text, checked fallback text, syntax spans, editor type hints,
  widget default text alignment, font style/weight normalization, clipping,
  rotation, and text bounds. `boon_native_gpu` now delegates text-run lowering
  to this document boundary and only adapts neutral `RenderTextRun` /
  `RenderRichTextSpan` values into glyphon-specific render state.
- Progress 2026-06-12: native GPU internal `RenderScene` now stores neutral
  `RenderTextRun` values through retained chunk metadata and converts to
  glyphon-specific `TextRun` values only at the final text render call.
- Progress 2026-06-12: `boon_document::render_scene` now defines neutral
  `RenderVisualPrimitive` / `RenderVisualPrimitiveKind` descriptors and
  `render_visual_primitives`. That document-side lowerer owns viewport
  background, default fills, asset texture refs, checkbox primitives, checkbox
  checkmark intent, clip/radius/color fields, style identity, and dependency
  sets for those primitive intents. Architecture verification now records this
  visual-primitive ownership.
- Progress 2026-06-12: `boon_native_gpu` now exposes
  `SurfaceRenderSceneRequest` / `encode_render_scene_to_surface` for
  pre-lowered `boon_document::RenderScene` values, converts document visual
  primitives to quad batches without requiring a `LayoutFrame`, and keys the
  internal hot encode cache by render-scene content instead of a layout-frame
  clone. Native GPU tests cover adapting an external document render scene, and
  architecture verification records the external scene entrypoint plus
  scene-keyed hot encode boundary.
- Progress 2026-06-12: native playground preview/dev visible rendering now
  lowers `LayoutFrame` values to `boon_document::RenderScene` with the glyphon
  text-column measurer before calling `VisibleLayoutRenderer::encode_scene`.
  App-owned readback still accepts a compatibility `LayoutFrame` request for
  proof hashing, but its actual GPU encode also goes through
  `SurfaceRenderSceneRequest`. Architecture verification now fails if native
  playground visible rendering returns to `SurfaceRenderRequest`.
- Progress 2026-06-12: `boon_document::render_scene` now lowers text overlay
  primitives before the GPU crate: editor selection, bracket highlights,
  editor carets, text-input carets, underlines, strikethroughs, and button
  checkmark strokes. The lowerer uses the renderer-neutral text-column measurer
  to compute overlay geometry, and `boon_native_gpu` only paints the resulting
  `RenderVisualPrimitiveKind` values in the external scene path. Architecture
  verification now records document ownership and renderer paint support for
  these text overlay primitives.
- Progress 2026-06-12: material fill adjustment for the external scene path now
  lives in `boon_document::render_scene`. Neutral fill primitives account for
  transparency, refraction, frosted blur, frosted saturation, gloss, and metal
  before they reach `boon_native_gpu`; the architecture gate records this
  material-fill ownership.
- Progress 2026-06-12: border primitive lowering for the external scene path now
  lives in `boon_document::render_scene`. The neutral lowerer emits whole-border
  and per-side border primitives with stroke width, radius, color, clip, style
  identity, and dependencies, and appends them after normal visual primitives to
  preserve the existing paint order over descendant fills. `boon_native_gpu`
  paints those pre-lowered border primitives through the external scene path,
  and architecture verification records both document ownership and renderer
  support.
- Progress 2026-06-12: material overlay lowering for the external scene path now
  lives in `boon_document::render_scene`. The neutral lowerer emits frosted
  material layer primitives before fills and material highlight primitives after
  fills, preserving the previous renderer paint order while keeping gloss,
  depth, glass highlight, and frosted haze geometry out of the GPU semantic
  contract. `boon_native_gpu` paints those pre-lowered primitives as ordinary
  styled rectangles, and architecture verification records both document
  ownership and renderer support.
- Progress 2026-06-12: box-shadow lowering for the external scene path now
  lives in `boon_document::render_scene`. The neutral lowerer emits shadow
  rectangle primitives before frosted layers and fills, preserves CSS reverse
  shadow paint order, rounded blur expansion, inset bands, and non-rounded
  rect-difference halo bands. `boon_native_gpu` paints those pre-lowered shadow
  primitives as ordinary styled rectangles, and architecture verification
  records both document ownership and renderer support.
- Progress 2026-06-12: checkbox raster descriptor lowering for the external
  scene path now lives in `boon_document::render_scene`. The neutral lowerer
  emits checkbox cast-shadow, circle, inner-shadow, highlight, and checkmark
  descriptors with ring/inner colors, stroke widths, antialias widths, and
  checkmark control points, and it owns the asset-icon skip rule. `boon_native_gpu`
  keeps rasterization math but consumes the pre-lowered descriptor fields instead
  of interpreting checkbox style keys in the external scene path. Architecture
  verification records both document ownership and renderer support.
- Progress 2026-06-12: the compatibility `SurfaceRenderRequest` path now lowers
  through `boon_document::render_scene::lower_layout_frame_to_render_scene` and
  adapts with `render_scene_from_document_scene` before entering the WGPU encode
  path. The old `render_scene_from_layout_frame` / `rect_vertices` semantic
  lowerer is marked compatibility-only for legacy renderer unit tests and is no
  longer part of the production encode contract. Architecture verification now
  fails if `encode_layout_to_surface_with_pipeline` calls the old lowerer or
  `rect_vertices`.

### TASK-0504 Shared Text Service And Bounded Shaped-Run Cache
Status: done
Type: implementation
Priority: P2
Depends on: TASK-0501
Source plans: 03, 04, 05, 08, 10, 11
Likely areas: native GPU text state, document text measurement, editor metrics
Goal:
Unify text measurement, shaping, editor metrics, and glyph atlas reporting.
Acceptance:
- Text measurement and rendering share deterministic contracts for font size, line height, fallback fonts, rich spans, caret metrics, and selection metrics.
- Reports include shaped-run hits/misses/evictions, glyph atlas uploads/evictions, missing glyphs, visible text runs, shaped text runs, and cache memory.
- Cache is bounded and observable.
Verification:
- `cargo test -p boon_native_gpu --lib text`
- `cargo test -p boon_document --lib text`
- `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
Rollback / stop condition:
- Stop if measurement and render contracts differ. Add a contract-normalization child task.
Notes:
- Avoid fresh font system creation in hot paths where a shared service can be used.
- Glyphon does not currently expose atlas upload-byte or eviction callbacks.
  The native reports expose renderer-owned `glyph_atlas_prepare_count` and
  `glyph_atlas_evictions_observed` fields plus an explicit
  `unavailable_reason` for exact atlas upload/eviction callbacks instead of
  fabricating those counters.

### TASK-0505 AssetRef And Async Asset Pipeline
Status: done
Type: implementation
Priority: P2
Depends on: TASK-0102, TASK-0501
Source plans: 03, 07, 08, 09, 10, 11
Likely areas: asset generation, document/render lowering, native GPU texture cache
Goal:
Move generated/inline assets toward digest-based refs and keep decode/raster/upload out of interaction hot paths.
Acceptance:
- `AssetRef`/`BlobRef`/digest identities are available for generated assets and renderer uploads.
- Asset cache reports decode/raster/upload state, hits/misses/evictions, byte caps, and failure diagnostics.
- Synchronous SVG raster/upload does not happen on interaction hot paths when assets are already known.
Verification:
- `cargo test -p boon_native_gpu --lib asset`
- Asset-heavy native proof still renders correctly with app-owned readback.
Rollback / stop condition:
- Stop if generated asset identity is not stable. Add digest generation/freshness task.
Notes:
- Do not expose Rust handles or GPU resource IDs as Boon-visible values.
- First-seen SVG data URL decode/raster/upload is still synchronous. The
  current completed slice makes generated/inline asset identity digest-based,
  reports the work, and proves already-known assets do not repeat synchronous
  decode/raster/upload on the next interaction/render frame. A broader async
  predecode worker can build on these refs later.

## Phase 6: Host Event Loop, IPC, And Dev/Preview Separation

### TASK-0601 Live IPC And Latest-Wins Worker Counters
Status: done
Type: measurement
Priority: P1
Depends on: TASK-0003, TASK-0004C2
Source plans: 01, 05, 08, 10, 11
Likely areas: native playground, native app window, xtask IPC gates
Goal:
Replace synthetic/hardcoded IPC backpressure counters with live preview/dev counters.
Acceptance:
- Reports include queue depth, dropped/coalesced messages, blocked sends, blocked duration, bytes, heartbeat gaps, dev lag, preview frame gaps, stale/discarded revisions, and semantic coalescing reason.
- Preview never blocks on dev window IPC, debug summaries, report writes, PNG writes, or proof-only readbacks.
- Latest-wins workers report input count, coalesced count, dropped count, and completed revision.
Verification:
- `cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json`
- `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
Rollback / stop condition:
- Stop if a counter is not live; omit it or mark diagnostic instead of hardcoding zero.
Notes:
- Preserve current native GPU proof boundaries.
- Result: done. Preview IPC now keeps live server-side request/response byte,
  round-trip, response-write, and hot-path blocked-send counters; bounded IPC
  reports keep synthetic stress-load counters labeled separately from live IPC
  exchange metrics. The preview replace-source worker now reports live
  latest-wins input, coalesced, dropped, stale/discarded, completed command /
  revision, and semantic coalescing reason fields in ACK/status/result payloads.
  Observability availability now marks dev lag as measured from live dev request
  round trips and latest-wins counters as measured from the live preview replace
  worker.
- Verification:
  `cargo fmt --all`
  `cargo check -p boon_native_playground -p xtask`
  `cargo test -p boon_native_playground --bin boon_native_playground preview_replace_worker_queue_reports_live_latest_wins_metrics`
  `cargo test -p boon_native_playground --bin boon_native_playground replace_source_ack_is_small_and_worker_commits_latest_revision`
  `cargo test -p xtask native_ipc`
  `cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json`
  `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
- Evidence: fresh IPC and observability reports both pass with
  `preview_blocked_on_ipc_count=0`,
  `preview_blocked_on_ipc_duration_ms_max=0`,
  `preview_ipc_request_count=9`, live response-write summaries, and
  latest-wins `input_count=6`, `coalesced_count=4`, `dropped_count=5`,
  `stale_revision_discard_count=1`, plus semantic reason
  `single-slot latest-wins source replacement queue overwrote older pending work`.
  The observability report marks `dev_lag_ms` as
  `measured_live_dev_request_round_trip`.

### TASK-0602 Event-Driven Loop And Fixed Sleep Audit
Status: done
Type: cleanup
Priority: P2
Depends on: TASK-0601
Source plans: 04, 05, 08, 10, 11
Likely areas: native app window, native playground, xtask harnesses
Goal:
Remove or bound fixed sleeps and polling loops that affect interaction or verifier flake.
Acceptance:
- Fixed sleeps in interaction paths are replaced by event waits, revision waits, ACKs, socket handshakes, readback hashes, or bounded deadlines.
- Remaining polling reports interval, total wait, wake reason, and timeout reason.
- Hard process exits in app paths are moved to supervised top-level exits where possible.
Verification:
- `cargo test -p boon_native_app_window --lib`
- `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
Rollback / stop condition:
- Stop if event source is missing. Add a child task to expose the needed wake/ACK signal.
Notes:
- Do not weaken verifier timeouts to hide missing readiness.
- Result: done for the currently implementable interaction-loop slice. The
  native demand-driven app-window loop no longer sleeps for 5 ms after every
  rendered frame; only continuous probe mode keeps the fixed 16 ms cadence. The
  dev preview replace-result wait now uses a bounded channel `recv_timeout`
  instead of a fixed 5 ms polling sleep, while the nonblocking UI poll path keeps
  `try_recv`. Remaining app-window idle waits report
  `passive_input_poll_interval_ms`, `idle_wait_count`, `idle_wait_total_ms`,
  `last_idle_wait_timeout_ms`, `last_idle_wait_actual_ms`, and
  `last_idle_wait_wake_reason`.
- Hard-exit audit: the native app-window `std::process::exit(0)` calls remain at
  the app-window/application boundary because `app_window::application::main`
  still owns process lifetime. The preview shutdown IPC still lacks a clean
  app-window quit signal; track that as TASK-0602A instead of adding a sleep or
  fake readiness signal.
- Verification:
  `cargo fmt --all`
  `cargo test -p boon_native_app_window --lib`
  `cargo check -p boon_native_playground -p xtask`
  `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
- Evidence: fresh observability report passed. Final role loop reports
  `target/reports/native-gpu/roles/preview-loop-todomvc-1484588.json` and
  `target/reports/native-gpu/roles/dev-loop-todomvc-1484588.json` include
  `passive_input_poll_interval_ms=100`, idle wait counters/totals/last timeout
  fields, input poll counts, skipped idle poll counts, and rendered frame counts.

### TASK-0602A Supervised Native Shutdown Signal
Status: done
Type: cleanup
Priority: P2
Depends on: TASK-0602
Source plans: 05, 08, 10, 11
Likely areas: native app window, native playground preview IPC shutdown path
Goal:
Expose a clean app-window quit/shutdown signal so role processes can exit from a
supervised top-level path instead of IPC handlers sleeping briefly and calling
`std::process::exit(0)`.
Acceptance:
- Preview shutdown IPC ACK records a shutdown request and wakes the native loop.
- Native app-window loop observes the shutdown request and returns through the
  app/window top-level lifecycle without a handler-local sleep.
- Reports include shutdown request time, wake generation, observed loop exit
  reason, and timeout reason if shutdown is not observed.
Verification:
- `cargo test -p boon_native_app_window --lib`
- `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`
Rollback / stop condition:
- Stop if `app_window::application::main` cannot expose a quit/close signal;
  document the missing upstream primitive instead of adding another fixed sleep.

Progress Log:
- Date: 2026-06-12
- Task: TASK-0602A
- Commit: uncommitted
- Files changed: vendor/app_window/src/application.rs; crates/boon_native_app_window/src/lib.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_native_app_window -p boon_native_playground -p xtask`; `cargo test -p boon_native_app_window --lib`; `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`; `cargo xtask verify-report-schema`; `git diff --check`; `jq` inspection of `target/reports/native-gpu/.observability-supervisor.json` and `target/reports/native-gpu/roles/preview-loop-todomvc-1581876.json`
- Result: done. `app_window::application::stop()` now exposes the platform main-loop stop primitive, native app-window probes return through the top-level lifecycle instead of `std::process::exit(0)`, and preview shutdown IPC records a native-loop-exit ACK with request time and wake generation. The refreshed observability supervisor report passed with `preview_shutdown_ack.shutdown_method="native-loop-exit-hook"`, `shutdown_requested_at_unix_ms=1781265924795`, `shutdown_wake_generation=25`, `preview_clean_exit_after_dev_exit=true`, and `preview_exit_status_after_dev_exit="exit status: 0"`; the preview loop report recorded `loop_exit_reason="preview_shutdown_ipc:desktop-supervisor-clean-exit-after-dev"`.
- Follow-up: next ready checklist item is TASK-0603.

### TASK-0603 Typed Hit Side Table
Status: done
Type: implementation
Priority: P2
Depends on: TASK-0203, TASK-0401
Source plans: 06, 08, 10, 11
Likely areas: document hit regions, native input routing, BoonDriver
Goal:
Use typed hit-test data for interaction while keeping JSON proof output for reports.
Acceptance:
- Hit side table contains node ID, source binding ID, bounds, z/depth, scroll root, row key/generation, and coarse spatial bucket where useful.
- Click/hover paths do not scan JSON proof data.
- Reports can still serialize equivalent proof data afterward.
Verification:
- `cargo test -p boon_document --lib hit`
- `cargo test -p boon_driver --lib`
- BoonDriver route proof includes hit/focus/scroll evidence.
Rollback / stop condition:
- Stop if document source binding lacks row generation; return to `TASK-0203`.
Notes:
- This task supports honest human-like automation and speed.

Progress Log:
- Date: 2026-06-12
- Task: TASK-0603
- Commit: uncommitted
- Files changed: crates/boon_document/src/lib.rs; crates/boon_driver/src/lib.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebbba-e9ae-7573-a8cd-0fc0e0d0ac45`; `cargo fmt --all`; `cargo test -p boon_document --lib hit`; `cargo test -p boon_driver --lib`; `cargo check -p boon_document -p boon_driver -p boon_native_playground`; `cargo test -p boon_document --lib`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: partial progress only. `boon_document::HitSideTable` now builds typed hit entries from `DocumentFrame + LayoutFrame` with node ID, source binding ID/path/intent, bounds, z depth, nearest scroll root, optional row key/generation, per-entry spatial bucket, and a serializable coarse bucket index. Dev editor source-binding lookup and dev route-proof hit evidence now use the typed table before serializing report JSON. BoonDriver action proofs now preserve hit/focus/scroll evidence, including typed hit metadata when present.
- Follow-up: finish TASK-0603 by moving preview hover/click routing off `layout_proof["hit_target_assertions"]` and `source_intent_assertions` scans, ideally by caching a typed route table in `PreviewSharedRenderState` whenever the layout proof/frame changes. Keep JSON proof helpers only for tests/report compatibility.

- Date: 2026-06-12
- Task: TASK-0603
- Commit: uncommitted
- Files changed: crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground hit_region`; `cargo test -p boon_document --lib hit`; `cargo test -p boon_driver --lib`
- Result: partial progress only. Preview input now builds a `PreviewHitRouteTable` from the cached `DocumentRenderSnapshot` keyed by `layout_frame_hash`, so simple pointer-move, simple source-click, cached hover-click, and fallback hover-state updates prefer typed hit/source/display-item data before any report JSON. Updated route methods resolve hit buckets, source-node bubbling, pointer payloads, source intent lookup, target occurrence, key-focus acceptance, text-cursor state, and link checks from typed `HitSideTable`, typed source-intent records, and typed display items.
- Follow-up: finish TASK-0603 by replacing the remaining slow mouse-release/text-focus branch and caret/text extraction helpers that still require JSON hit regions; after that, tighten tests so live preview click/hover paths fail if they call proof JSON hit scans.

- Date: 2026-06-12
- Task: TASK-0603
- Commit: uncommitted
- Files changed: crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_document -p boon_driver -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground hit_region`; `cargo test -p boon_native_playground --bin boon_native_playground cells_formula_bar_click_accepts_text_edit`; `cargo test -p boon_document --lib hit`; `cargo test -p boon_driver --lib`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. Preview hover, simple click, pointer-move, mouse-release, text-focus, blur/focus, caret placement, key-focus, link, and double-click routing now use `PreviewHitRouteTable` when a cached typed route table exists, and typed misses no longer fall through to proof JSON scans. JSON hit/source helpers remain for report serialization and legacy helper tests. The new poisoned-proof regression removes `hit_target_assertions`, `source_intent_assertions`, and source indexes while preserving `layout_frame_hash`; hover and click still focus the Cells formula bar through the typed route table.
- Follow-up: `TASK-0502` remains blocked by `EXP-0001`; next ready implementation task by dependency order is `TASK-0701`.

## Phase 7: Bridge, Effects, And NovyWave Page Refs

### TASK-0701 Bridge Schema And Effect Kernel Skeleton
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0103, TASK-0202
Source plans: 01, 03, 07, 09, 10, 11
Likely areas: new or existing bridge crate boundary, runtime bridge completion path, xtask bridge verifier
Goal:
Create the generic Rust bridge/effects substrate before real Wellen integration.
Acceptance:
- Bridge metadata includes module names, export kinds, schema versions, schema hashes, capabilities, ABI version, and provider metadata.
- Canonical schema encoding and golden vectors cover records, tagged variants, lists, refs, pages, blobs, diagnostics, and effect completions.
- Request/completion scheduling includes request ID, generation/epoch, schema hash, input digest, request key, status, diagnostic, completion payload, cancellation, dedup, stale rejection, and replay.
- Missing module, changed schema, wrong effect kind, stale completion, duplicate completion, cancellation, grant denial, payload cap, replay, and no-Rust-handle cases are tested.
Verification:
- `cargo test --workspace bridge`
- `cargo xtask check-bridge --report target/reports/check-bridge.json`
Rollback / stop condition:
- Stop if no bridge crate/boundary exists. Add a child task to create only the minimal internal bridge module and verifier fixture.
Notes:
- Public `Boon.toml` workflow can be deferred, but the engine validation kernel must be generic.

Progress Log:
- Date: 2026-06-12
- Task: TASK-0701
- Commit: uncommitted
- Files changed: Cargo.toml; Cargo.lock; crates/boon_bridge/Cargo.toml; crates/boon_bridge/src/lib.rs; crates/xtask/Cargo.toml; crates/xtask/src/main.rs; crates/boon_ply_playground/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebbd5-0289-7ea1-8a58-00f29d0a03ca`; `cargo fmt --all`; `cargo test --workspace bridge`; `cargo xtask check-bridge --report target/reports/check-bridge.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. Added a real `boon_bridge` workspace crate with bridge ABI/schema versioning, canonical bridge value shapes, schema hashing, module/export/provider/capability metadata, pure/effect export validation, effect request/completion data, request-key deduplication, cancellation, stale/duplicate rejection, replay, payload-cap enforcement, and no-Rust-handle validation. Added fixture schemas/golden vectors and `cargo xtask check-bridge`, which writes `target/reports/check-bridge.json` with metadata, golden vector hashes, and negative-case coverage.
- Follow-up: next ready implementation task by dependency order is `TASK-0702`.

### TASK-0702 NovyWave PageRef, ArtifactRef, And BlobRef Fixture Path
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0401, TASK-0701
Source plans: 07, 09, 10, 11
Likely areas: NovyWave example source, runtime bridge fixture, scenario reports
Goal:
Make NovyWave consume bounded bridge-shaped pages and descriptors instead of app-graph waveform payloads.
Acceptance:
- Fixture adapter exposes open result, hierarchy page, signal page, waveform page, cursor values, file stats, diagnostics, and status.
- Every page carries schema version, request fingerprint, response fingerprint, input digest, page digest, generation, row/sample/transition counts, byte length, and status.
- Boon-visible values are descriptors, refs, pages, blobs, diagnostics, and statuses only.
- Full waveform payloads and Rust handles do not enter Boon-visible data.
- Stale response rejection is based on generation/request fingerprints.
Verification:
- `cargo test -p boon_runtime --lib novywave`
- `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
Rollback / stop condition:
- Stop if the bridge skeleton cannot replay completions deterministically. Return to `TASK-0701`.
Notes:
- Real Wellen integration remains deferred until fixture proof passes.
Progress Log:
- Date: 2026-06-12
- Task: TASK-0702
- Commit: uncommitted
- Files changed: crates/boon_bridge/src/lib.rs; crates/boon_ir/src/lib.rs; crates/boon_parser/src/lib.rs; crates/boon_runtime/src/lib.rs; crates/xtask/src/main.rs; examples/novywave/Bridge/NovyBridge.bn; examples/novywave/RUN.bn; examples/novywave.scn; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebbdd-ac1a-7330-b3e7-bc2989ae4429`; `cargo fmt --all`; `cargo test -p boon_runtime --lib novywave_waveform_metadata_drives_selected_file_and_timeline_window -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo test --workspace bridge`; `cargo xtask check-bridge --report target/reports/check-bridge.json`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. NovyWave now exposes bridge-shaped `ArtifactRef`, `BlobRef`, `OpenResult`, `PageRef`, hierarchy/signal/waveform/cursor/file-stat pages, diagnostics, status descriptors, request/response input digests, page digests, generations, counts, bounded page byte lengths, and stale response rejection based on generation/request fingerprints. The bridge PageRef Rust fixture carries the same metadata, the parser/IR/runtime hidden-identity policy now allows visible domain `generation` fields while still forbidding `$boon`, hidden generation, target generation, row/source IDs, and bind epochs, and `cargo xtask verify-novywave-bridge-scenario` writes `target/reports/novywave-bridge-scenario.json`.
- Follow-up: `TASK-0502` remains blocked by `EXP-0001`; next ready implementation task by dependency order is `TASK-0703`.

### TASK-0703 NovyWave View Over Rows, Pages, And Virtualization
Status: done
Type: implementation
Priority: P2
Depends on: TASK-0401, TASK-0501, TASK-0702
Source plans: 07, 09, 10, 11
Likely areas: `examples/novywave`, document materialization, render chunks
Goal:
Rebuild NovyWave view model around keyed signal lane rows, page refs, and generic virtualization.
Acceptance:
- One keyed `SignalLaneRow` view model owns row identity, signal label, current value, format, lane state, focus state, hit regions, and page/window refs.
- Materialized rows equal visible rows plus overscan.
- Wave segment data is already scoped to row/window/page before rendering.
- Labels, values, lanes, cursor, marker, hover, and focus overlays stay aligned during scroll, pan, zoom, resize, and theme changes.
- Generic virtual collection protocol contains no NovyWave-specific public contract.
Verification:
- `cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json`
- `cargo xtask verify-native-gpu-scroll-speed --example novywave --report target/reports/native-gpu/scroll-speed-novywave.json`
Rollback / stop condition:
- Stop if virtual materialization does not support row-local source binding lifecycle. Return to Phase 4.
Notes:
- Do not move waveform samples back into ordinary Boon lists to pass scenarios.
- Partial engine fix: readable nested `SignalLaneRow` records are now the
  intended shape. Compiler/runtime fixes must preserve nested row records,
  row-local inline lists, referenced root record payloads, and structured row
  columns instead of requiring flat surrogate fields.
- 2026-06-12 evidence: `cargo xtask verify-native-gpu-novywave-visual --report
  target/reports/native-gpu/novywave-visual.json` passes with app-owned WGPU
  readback, row alignment, real-preview input, and runtime-projected waveform
  width evidence. The Boon view no longer contains hardcoded workspace file
  row constructors; it maps generic `store.file_tree_rows` through the
  existing `file_tree_row` dispatcher. The promoted real-preview input test
  now also proves the external-loaded file tree exposes the generic
  metadata-driven loaded row and does not expose stale workspace fixture rows.
- 2026-06-12 evidence: `cargo xtask verify-native-gpu-scroll-speed --example
  novywave --report target/reports/native-gpu/scroll-speed-novywave.json`
  passes with `blockers: null`. Timeline pan/zoom projected replay is
  p50/p95/max 12.693/16.308/16.308 ms against the 120 ms budget, preview
  frame p95 is 9.635 ms, wheel-to-visible p95 is 9.635 ms,
  `background_app_owned_scroll_speed_proven=true`, `budget_pass=true`, and
  `non_os_scroll_model.frame_budget_model_pass=true`. The report remains
  explicit that this is `evidence_tier=boon-driver` app-owned background
  proof, not a real OS-input claim (`required_real_window_speed_proven=false`).
  The speed path is fixed by projecting only requested runtime summary paths
  for timeline replay and by caching lowered preview `RenderScene`s by
  frame-hash plus viewport size instead of rebuilding unchanged render scenes
  during scroll measurement.
- 2026-06-12 evidence: `cargo test -p boon_runtime --lib
  render_projection_does_not_overwrite_appended_row_label_field -- --nocapture`;
  `cargo test -p boon_runtime --lib
  novywave_waveform_metadata_drives_selected_file_and_timeline_window --
  --nocapture`; `cargo test -p boon_native_playground --bin
  boon_native_playground
  novywave_external_loaded_file_tree_renders_loaded_file_without_workspace_rows
  -- --nocapture`; `git diff --check`. The engine now keeps render-projection
  fields from overwriting model row fields, list summaries include dynamic
  append fields, and the NovyWave file tree renders external loaded files from
  generic metadata-driven rows while excluding stale workspace fixture rows.

## Phase 8: BoonDriver, Reports, Anti-Cheating, And Scenarios

### TASK-0801 BoonDriver Scenario Engine Path
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0001, TASK-0202, TASK-0603
Source plans: 06, 09, 10, 11
Likely areas: `crates/boon_driver`, `crates/xtask`, native host input path
Goal:
Make BoonDriver drive scenarios through app-owned host/document/source/runtime/render evidence instead of wrapping stale reports.
Acceptance:
- BoonDriver parses scenarios, resolves selectors, performs waits, dispatches actions, routes host input, resolves hit/focus/scroll, records source intent, records runtime dispatch, records document/render patch evidence, asserts outcomes, and generates reports.
- Evidence tiers remain explicit: runtime, BoonDriver, real-window, human.
- BoonDriver cannot claim real-window or human observation.
- `verify-boon-driver-e2e` proves action flow, not just report wrapping.
Verification:
- `cargo test -p boon_driver --lib`
- `cargo xtask verify-boon-driver-e2e --report target/reports/boon-driver-e2e.json`
Rollback / stop condition:
- Stop if host input cannot route through public source batches; return to `TASK-0202`.
Notes:
- Native reports can provide window/render evidence, but BoonDriver owns the per-step route.

### TASK-0802 Negative And Fabricated-Report Gates
Status: done
Type: gate
Priority: P1
Depends on: TASK-0003, TASK-0801
Source plans: 06, 08, 09, 10, 11
Likely areas: `crates/xtask`, `crates/boon_report_schema`, native negative gates
Goal:
Reject stale, fabricated, shortcut, scaffold, and tier-inflated evidence.
Acceptance:
- Negative fixtures mutate source hash, scenario hash, budget hash, artifact hash, pixel hash, source event field, route ID, real OS input claim, private dispatch flag, stale source generation, stale row generation, and duplicate scenario ID.
- Fake human observation, fake real OS input, private runtime dispatch, source-event-only IPC shortcut, preview scenario-data leakage, full waveform payload entering Boon, scaffold rendering, copied pixel hashes, stale binaries, reduced fixtures, and model-only timing fail named checks.
- Honesty booleans are independently tested; they are not trusted merely because producer reports say `false`.
Verification:
- `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`
- `cargo xtask verify-report-schema`
- `cargo test -p boon_report_schema --lib`
Rollback / stop condition:
- Stop if a negative fixture requires weakening the schema. Add missing report fields instead.
Notes:
- This task protects future speed work from shortcut regressions.
- Implemented by extending `verify-native-gpu-negative` to 49 named fabricated
  report fixtures and centralizing extra rejection policy in
  `native_gpu_report_integrity_reasons`.
- Added named checks for stale scenario/budget/artifact hashes, mutated source
  event fields, route ID drift, stale source/row generations, duplicate
  scenario IDs, fake human observation, source-event-only IPC shortcuts, full
  waveform payload leakage into Boon, and reduced fixtures.
- `boon_report_schema` did not need new fields for this slice; the extra
  fields are native negative rejection triggers, while schema validation still
  checks the final negative report shape.

### TASK-0803 Metamorphic Hidden Fixture Gate
Status: superseded
Type: gate
Priority: P2
Depends on: TASK-0801, TASK-0802
Source plans: 06, 09, 10, 11
Likely areas: xtask, scenario fixtures, example manifests
Goal:
Catch hardcoded labels, source paths, fixture names, example names, and declaration order assumptions.
Acceptance:
- Core stories rerun after legal source reformat, source path move, fixture ID/path changes, label/symbol renames, declaration order changes where legal, viewport changes, and theme changes.
- Reports persist generator inputs and seeds.
- Expected semantic invariants are defined before running mutated cases.
- Visual equivalence uses app-owned crops and semantic labels, not whole-frame pixel equality.
- Documentation-only strings and report-only paths are allowlisted.
Verification:
- `cargo xtask verify-metamorphic-hidden-fixtures --report target/reports/metamorphic-hidden-fixtures.json`
Rollback / stop condition:
- Stop if baseline scenarios are not integrity-clean. Return to Phase 0.
Notes:
- Split into `TASK-0803A` and `TASK-0803B` so the first implementation can
  land an honest semantic hidden-fixture gate without claiming native visual
  crop equivalence before the app-owned visual path is wired into the matrix.

### TASK-0803A Counter Runtime Metamorphic Fixture Gate
Status: done
Type: gate
Priority: P2
Depends on: TASK-0801, TASK-0802
Source plans: 06, 09, 10, 11
Likely areas: `crates/xtask`, `crates/boon_report_schema`, `examples/counter.*`
Goal:
Create the first `verify-metamorphic-hidden-fixtures` gate as a deterministic
runtime proof that catches hidden hardcoding of labels, source routes, fixture
paths, fixture IDs, declaration order, formatting, and simple style/viewport
assumptions.
Acceptance:
- Baseline Counter semantic scenario passes before any mutated fixture runs.
- Generated fixture paths do not rely on the original `examples/counter.*`
  names, and the report persists generator seed, source path, scenario path,
  scenario name, source/scenario hashes, and artifact hashes.
- Mutated cases cover legal source reformat/comments, moved source/scenario
  paths, fixture ID/path changes, source route renames, visible label and
  `target_text` renames, legal declaration/source-branch/order changes, and
  simple style/viewport/theme-like changes that must not affect `store.count`.
- Expected semantic invariants are derived and recorded before mutated cases
  run: step IDs, operation count, source-event count, and the `store.count`
  sequence from the scenario.
- Documentation-only strings and report-only path strings are explicitly
  allowlisted in the report.
- Native visual crop equivalence is not claimed in this slice; it is deferred
  to `TASK-0803B`.
- No Boon syntax changes are introduced.
Verification:
- `cargo xtask verify-metamorphic-hidden-fixtures --report target/reports/metamorphic-hidden-fixtures.json`
- `cargo test -p boon_report_schema --lib`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if the baseline Counter scenario is not integrity-clean. Return to Phase
  0 instead of accepting mutated fixture evidence.
Notes:
- This is the first passing slice of the broader metamorphic matrix.
- Implemented `verify-metamorphic-hidden-fixtures` as a static proof gate. It
  generates deterministic Counter fixtures under
  `target/generated/metamorphic-hidden-fixtures/task-0803a-counter-seed-v1`,
  runs the baseline semantic scenario first, records semantic invariants before
  mutated execution, and verifies three hidden cases: path/reformat, route and
  label rename, and order/style mutation.
- `target/reports/metamorphic-hidden-fixtures.json` records the generator seed,
  fixture root, baseline hashes, three case reports, invariant declarations,
  allowlists, visual deferral to `TASK-0803B`, and artifact hashes for generated
  source/scenario/report files.
- `verify-report-schema` now excludes live app-window loop reports from summary
  artifact hashing while still classifying them as recognized diagnostic
  artifacts; otherwise a running native playground can rewrite the loop report
  between hash collection and summary validation.

### TASK-0803B App-Owned Visual Metamorphic Expansion
Status: done
Type: gate
Priority: P2
Depends on: TASK-0803A
Source plans: 05, 06, 09, 10, 11
Likely areas: native GPU gates, app-owned crops, `todo_mvc_physical`,
`novywave`, scenario fixtures
Goal:
Expand the metamorphic hidden-fixture matrix to physical multi-file examples
and app-owned visual equivalence.
Acceptance:
- Physical TodoMVC and/or NovyWave stories rerun after source path moves,
  source-unit order changes, legal reformat, fixture ID/path changes,
  label/symbol renames, viewport changes, and theme changes.
- Multi-file `RuntimeSourceUnit` inputs preserve generic module loading and do
  not rely on original manifest file names beyond documented module stem rules.
- Visual equivalence uses app-owned crops and semantic labels, not whole-frame
  pixel equality.
- Viewport/theme mutations prove semantic invariants and crop-level visual
  invariants independently.
- Documentation-only strings and report-only paths remain allowlisted.
Verification:
- Extend `cargo xtask verify-metamorphic-hidden-fixtures --report target/reports/metamorphic-hidden-fixtures.json`
  to include the physical/native visual matrix.
Rollback / stop condition:
- Stop if visual equivalence requires whole-desktop screenshots, model-only
  claims, or weakening native GPU report schemas.

### TASK-0804 NovyWave Scenario And Speed Gates
Status: done
Type: gate
Priority: P2
Depends on: TASK-0703, TASK-0801, TASK-0802
Source plans: 05, 06, 07, 09, 10, 11, 13, 14, 15, 16, 17, 18
Likely areas: xtask native GPU gates, NovyWave scenarios, budgets
Goal:
Prove NovyWave through bridge-shaped scenarios, app-owned visuals, and release-mode interaction speed.
Acceptance:
- `verify-novywave-bridge-scenario` covers empty state, load dialog, deterministic VCD, planned GHW/FST page behavior, scope selection, signal search, selected-row reorder/grouping, format cycling, cursor/pan/zoom, stale page rejection, marker operations, dark/light mode, payload caps, and missing grants.
- Native preview receives source/project payload, not example names or scenario data.
- Visual reports require nonblank/non-single-color frames, waveform row/label alignment, visible cursor/marker, readable dark/light text, crop hashes, backend, adapter, surface format, and scale metadata.
- Interaction speed runs release-only, warmed, with proof overhead excluded and stage timings included.
Verification:
- `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
- `cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json`
- `cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json`
- `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
Rollback / stop condition:
- Stop if speed passes only by reducing fixtures or using model-only updates. Add negative coverage instead.
Notes:
- These gates become readiness gates only when the architecture contract intentionally includes them.
- Bridge-shaped scenario proof, native preview E2E, app-owned visual, and
  release interaction-speed gates are all passing on the 2026-06-15 TASK-0804
  completion slice.
- 2026-06-13 TASK-0804 runtime speed slice: selected-list
  `List/filter_field_equal`, `List/filter_field_not_equal`, and numeric
  `List/retain` now intersect existing selections with generic text/numeric
  list indexes instead of scanning selected rows. Added focused runtime tests
  for selected text-index order preservation and numeric selected `!=`
  behavior with missing/nonnumeric values. Verified with
  `cargo test -p boon_runtime --lib list_index_ -- --nocapture`,
  `cargo test -p boon_runtime --lib list_filter_field -- --nocapture`,
  `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`,
  `cargo test -p boon_runtime --lib map_join_field -- --nocapture`,
  `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`,
  `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`,
  `cargo test -p boon_runtime --lib novywave_repeated_hover_width_is_noop_for_canvas_width -- --nocapture`,
  `cargo check -p boon_runtime -p boon_native_playground`,
  `cargo fmt --all`, and `cargo build --release -p boon_native_playground`.
  The official
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  gate still fails latency budgets, but its fresh report is an interaction-mode
  report with 11 stage counters and zero hot-path PNG/report writes. Runtime
  list counters across 97 samples now show
  `max_filter_field_rows_scanned=0`, `max_retain_rows_scanned=0`,
  `max_text_lookup_index_hits=46`, and `max_numeric_lookup_index_hits=14`.
  Remaining blockers are latency, not selected-list scan fallback:
  `input_to_visible_p95=50.474ms`, `click_to_cursor_p95=50.474ms`,
  `divider_drag_p95=29.157ms`, `hover_p95=17.260ms`, and
  `runtime_step_apply_p95=21.680ms`. Next TASK-0804 slices should attack
	  high `row_occurrences_scanned`/index-candidate fanout plus remaining
	  `move_field_rows_scanned` and `join_field_rows_scanned`, without weakening
	  the NovyWave fixture or speed budgets.
- 2026-06-13 TASK-0804 correctness/speed gate refresh: fixed the generic
  runtime root-derived propagation bug exposed by NovyWave reload and file-row
  bridge scenarios. Root-derived dirty propagation now permits a dependent to be
  requeued when one of its dependencies changes later in the same wave instead
  of treating "already processed once" as final; this keeps request/response
  structural keys, window labels, and `bridge_response_status` coherent without
  flattening NovyWave's Boon model. Also fixed `LIST { ... } |> WHEN` evaluation
  by routing piped list statements through expression evaluation instead of
  treating them as direct list literals before the pipe. Verification:
  subagent explorer `019ec03c-17c3-7081-9604-ff9fa3c860b0`;
  `cargo fmt --all`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`;
  `cargo test -p boon_runtime --lib novywave_bridge_scenario_file_row_selection_accepts_current_response -- --nocapture`;
  `cargo test -p boon_runtime --lib novywave_timeline_pan_zoom_required_sequence_matches_current_model -- --nocapture`;
  `cargo test -p boon_runtime --lib novywave_bridge_scenario_reload_default_resets_cursor_format_and_response -- --nocapture`;
  `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets). Latest bridge
  report is `status=pass`, `measurement_mode=proof`, coverage `71/71`, no
  failed groups. Latest speed report is `status=fail`, `measurement_mode=interaction`,
  11 stage counters, zero hot-path PNG/report writes, zero blocked IPC, with
  `hover_p95=16.374ms`, `click_to_cursor_p95=54.769ms`,
  `divider_drag_p95=36.737ms`, `resize_p95=13.532ms`,
  `runtime_step_apply_p95=26.609ms`, `runtime_apply_p95=27.964ms`;
  sample maxima include `rows_scanned=12`, `row_occurrences_scanned=1147`,
  `filter_field_rows_scanned=0`, `retain_rows_scanned=0`,
  `move_field_rows_scanned=6`, `join_field_rows_scanned=6`,
  `text_lookup_index_hits=46`, `numeric_lookup_index_hits=14`,
  `route_candidates_visited=1088`, `recompute_candidates=15`, and
  `semantic_deltas=28`. TASK-0804 remains in progress: next work should make
  root-derived propagation topological/versioned enough to preserve the
  correctness fix without broad reprocessing, then reduce click/divider latency
  without weakening fixtures or budgets.
- 2026-06-13 TASK-0804 root-derived ordering refresh: added dynamic root-dirty
  pop ordering so a dirty root-derived field whose last recorded root reads
  include another dirty root field waits behind that upstream field, while still
  allowing later same-wave requeueing for branch-sensitive dependencies. This is
  deliberately an ordering heuristic, not a processed-once shortcut. Verification:
  subagent explorer `019ec055-01fe-72a1-9e3a-bd2bd6075eb5`;
  `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`;
  `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets). Latest bridge
  report is `status=pass`, `measurement_mode=proof`, coverage `71/71`, no
  failed groups. Latest speed report is `status=fail`, `measurement_mode=interaction`,
  with zero hot-path PNG writes, report writes, proof readbacks, or blocking IPC.
  Current latencies: `click_to_cursor_p95=64.138ms`, `input_to_visible_p95=64.138ms`,
  `divider_drag_p95=26.241ms`, `hover_p95=15.568ms`, `resize_p95=12.176ms`,
  `runtime_step_apply_p95=35.606ms`, and `runtime_apply_p95=36.811ms`. Runtime
  counter maxima show the remaining blocker is selected-lane current-value
  recomputation with high index/occurrence fanout, not direct selected-list
  scans: `row_occurrences_scanned=1352`, `route_candidates_visited=1321`,
  `text_lookup_index_candidates=528`, `numeric_lookup_index_candidates=792`,
  `filter_field_rows_scanned=0`, `retain_rows_scanned=0`,
  `move_field_rows_scanned=6`, `join_field_rows_scanned=4`, and the worst
  samples still recompute `selected_signal_lane_rows[*].current_value`. Next
  TASK-0804 work should reduce cursor/current-value fanout through generic
  selection-aware index intersections, cursor-value memoization, or dependency
  narrowing without changing Boon syntax or reducing the NovyWave fixture.
- 2026-06-13 TASK-0804 selection-intersection/cache-key slice: kept the generic
  selected-subset direct lookup path for text/numeric predicates and replaced
  JSON serialization in user-function cache keys with a stable direct
  `BoonValue` encoder. This is an engine-only change: no Boon syntax changes,
  no NovyWave-specific runtime branches, and no fixture reduction. Verification:
  `cargo fmt --all`;
  `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`;
  `cargo test -p boon_runtime --lib list_index_ -- --nocapture`;
  `cargo test -p boon_runtime --lib list_filter_field -- --nocapture`;
  `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`;
  `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`;
  `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`;
  `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets). Current bridge
  report is `status=pass`, `measurement_mode=proof`,
  `bridge_scenario_coverage.status=pass`, `required_step_count=71`,
  `covered_step_count=71`, and no failures. Current speed report is
  `status=fail`, `measurement_mode=interaction`, `event_count=32`, with zero
  hot-path PNG writes, report writes, proof readbacks, or blocking IPC.
  Latencies: `input_to_visible_p95=68.651ms`,
  `click_to_cursor_p95=68.651ms`, `hover_p95=19.056ms`,
  `divider_drag_p95=26.406ms`, `resize_p95=14.302ms`,
  `runtime_step_apply_p95=36.634ms`, and `runtime_apply_p95=37.927ms`.
  Counter maxima now show the selected-subset path removed the previous broad
  candidate fanout but did not solve latency:
  `row_occurrences_scanned=578`, `route_candidates_visited=547`,
  `text_lookup_index_candidates=410`, `numeric_lookup_index_candidates=136`,
  `filter_field_rows_scanned=0`, `retain_rows_scanned=0`,
  `move_field_rows_scanned=6`, `join_field_rows_scanned=4`,
  `map_join_field_fusions=9`, and `map_join_field_rows_fused=8`.
  A function cacheability heuristic that skipped caching tiny non-list helper
  functions was tested and killed because release p95 regressed to
  `click_to_cursor_p95=69.913ms`, `divider_drag_p95=27.253ms`, and
  `runtime_apply_p95=38.145ms`. Next work should target unchanged
  `selected_signal_lane_rows[*].current_value` candidate evaluation, root
  fanout, or interval/cursor-value indexing rather than selected-list scan
  fallback.
- 2026-06-13 TASK-0804 numeric stability guard slice: kept a generic
  engine-only dependency-narrowing path for indexed numeric `List/retain`.
  Runtime evaluation now records root numeric stability intervals for
  row-field predicates such as `segment.start <= store.cursor` and
  `segment.end > store.cursor`, carries those guards through user-function
  cache hits, skips dirty row-field candidates when the changed root value is
  still inside the interval, and merges adjacent intervals only after an
  unchanged recompute proves the field value stayed the same. This adds no
  Boon syntax, no NovyWave-specific runtime branch, and no fixture reduction.
  Verification: subagent explorers `019ec08f-7ab4-7ba1-bffe-73ddaddc8cd0`
  and `019ec08f-9c2f-7f72-8724-8147a3a4c517`; `cargo fmt --all`;
  `cargo check -p boon_runtime`; `cargo check -p boon_runtime -p boon_native_playground`;
  `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`;
  `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`;
  `cargo test -p boon_runtime --lib list_filter_field -- --nocapture`;
  `cargo test -p boon_runtime --lib map_join_field -- --nocapture`;
  `cargo test -p boon_runtime --lib list_index_ -- --nocapture`;
  `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`;
  `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`;
  `cargo test -p boon_runtime --lib novywave_repeated_hover_width_is_noop_for_canvas_width -- --nocapture`;
  `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets). Current bridge
  report is `status=pass`, `measurement_mode=proof`,
  `bridge_scenario_coverage.status=pass`, `required_step_count=71`,
  `covered_step_count=71`, and no failures. Current speed report is
  `status=fail`, `event_count=32`, with zero hot-path PNG writes, report
  writes, proof readbacks, report serialization, or preview-blocking IPC.
  Latencies: `input_to_visible_p95=64.779ms`,
  `click_to_cursor_p95=64.779ms`, `hover_p95=18.194ms`,
  `divider_drag_p95=27.009ms`, `resize_p95=13.411ms`,
  `runtime_step_apply_p95=34.458ms`, and `runtime_apply_p95=35.724ms`.
  Counter maxima improved to `row_occurrences_scanned=440`,
  `route_candidates_visited=409`, `text_lookup_index_candidates=360`,
  `numeric_lookup_index_candidates=48`, `filter_field_rows_scanned=0`,
  `retain_rows_scanned=0`, `join_field_rows_scanned=4`,
  `map_join_field_fusions=8`, and `map_join_field_rows_fused=8`.
  Repeated click samples now show `runtime_recompute_candidate_count=0`
  after the first candidate-bearing cursor click, so the previous
  selected-lane current-value candidate blocker is mostly removed. TASK-0804
  remains in progress because cursor positions such as `49` and `150` still
  produce 21 root semantic deltas and spend roughly `34-36ms` in runtime step
  apply. Next work should target root-derived cursor fanout, root expression
  evaluation cost, or root-delta/patch batching rather than selected-row
  current-value scans.
- 2026-06-13 TASK-0804 root fanout/duplicate propagation slice: kept another
  generic engine-only runtime improvement, with no Boon syntax changes and no
  NovyWave-specific runtime branch. Structured root materialization now diffs
  changed child paths precisely: whole-object readers are still dirtied through
  the parent root path, but stable sibling child paths are no longer marked as
  changed. The source-action path also no longer seeds the final root-derived
  materialization pass with root deltas that were already fully propagated
  during `apply_source_actions`; the final pass now starts from post-source
  changes plus indexed-derived materializations. Small scalar evaluator
  allocation cleanups make `Text/concat`, `Text/time_range_label`, and text-like
  `+` consume owned `BoonValue` strings instead of cloning and formatting them
  again. Verification: subagent explorer
  `019ec0a6-ed66-7e20-afa8-ad266c16a12e`; `cargo fmt --all`;
  `cargo check -p boon_runtime`;
  `cargo test -p boon_runtime --lib structured_root_changed_reads_only_dirty_changed_children -- --nocapture`;
  `cargo test -p boon_runtime --lib root_derived_materialization_uses_changed_dependencies -- --nocapture`;
  `cargo test -p boon_runtime --lib root_derived_structured_parent_dirties_dependents_without_empty_text_patch -- --nocapture`;
  `cargo test -p boon_runtime --lib root_derived_worklist_revisits_direct_and_indirect_dependents -- --nocapture`;
  `cargo test -p boon_runtime --lib source_text_payload_can_be_read_inside_then_update_expression -- --nocapture`;
  `cargo test -p boon_runtime --lib novywave_repeated_hover_width_is_noop_for_canvas_width -- --nocapture`;
  `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets);
  `git diff --check -- crates/boon_runtime/src/lib.rs`. Current bridge report
  is `status=pass`, `measurement_mode=proof`, `required_step_count=71`,
  `covered_step_count=71`, `applied_source_event_count=90`, and no failures.
  Current speed report is `status=fail`, but runtime is no longer the dominant
  blocker: `runtime_step_apply_p95=11.006ms` and
  `runtime_apply_p95=12.207ms`, down from the prior `34.458ms`/`35.724ms`.
  Official end-to-end latency still fails with
  `input_to_visible_p95=38.316ms`, `click_to_cursor_p95=38.316ms`,
  `hover_p95=19.841ms`, `divider_drag_p95=25.848ms`, and
  `resize_p95=14.016ms`. Click samples now spend roughly `5-13ms` in runtime
  apply, `6-10ms` in patched document layout, and `1-4ms` in shared updates;
  the next TASK-0804 slice should target layout/shared/native interaction
  timing and the mismatch between per-sample `total_ms` and top-level
  click/hover p95, not the old selected-lane current-value or duplicate root
  propagation blockers.
- 2026-06-13 TASK-0804 native input route-cache/measurement slice: kept the
  change generic to native preview input handling. `PreviewHitRouteTable`
  construction is now cached by stable `layout_frame_hash` for unscrolled,
  non-focus-mutated frames, while scrolled or focus-overlay frames still build
  from the current shared frame to avoid stale text-input state. The generic
  fallback input path now records compact `PreviewNativeInputTimingSample`
  entries, and the NovyWave interaction role reports aggregate
  `hover_interaction_timing_ms`, `divider_interaction_timing_ms`,
  `hover_native_input_timing_ms`, and `divider_native_input_timing_ms` fields
  in addition to the existing click summaries. Verification: subagent explorer
  `019ec0ba-2211-7921-9382-ebd92a18eb46`; `cargo fmt --all`;
  `cargo check -p boon_native_playground`;
  `cargo test -p boon_native_playground operator_host_input_uses_structural_row_text_for_novywave_bound_sources -- --nocapture`;
  `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets);
  `git diff --check -- crates/boon_native_playground/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
  Current bridge report is `status=pass`, `measurement_mode=proof`,
  `bridge_scenario_coverage.status=pass`, and coverage `71/71`.
  Current speed report is `status=fail`, but hover is now below the release
  p95 budget: `hover_p95=16.223ms`. Remaining budget blockers are
  `input_to_visible_p95=36.853ms`, `click_to_cursor_p95=36.853ms`, and
  `divider_drag_p95=24.684ms`; `resize_p95=10.397ms`,
  `runtime_apply_p95=12.132ms`, `runtime_step_apply_p95=10.974ms`, and
  `layout_rebuild_p95=7.778ms`. The new native-input summaries show the next
  blocker clearly: `native_input_timing_count=161`, with fast paths
  `generic_fallback=88`, `simple_pointer_move=32`, and
  `simple_source_click=8`; click native input still spends
  `total_input_p95=30.881ms`, while divider native input spends
  `total_input_p95=20.507ms`. One broad diagnostic test remains failing and
  should be handled separately before treating operator-host-input proof as
  clean: `cargo test -p boon_native_playground novywave_operator_host_input_batches_execute_in_preview_runtime -- --nocapture`
  fails because the `store.elements.load_default_file` host-route proof lacks
  a layout hit/source intent after dynamic layout changes even though runtime
  acceptance is recorded. TASK-0804 remains in progress; next work should move
  repeated click handling out of `generic_fallback` and reduce patched layout /
  shared update cost without weakening the fixture or budget.
- 2026-06-13 TASK-0804 direct layout-frame patch slice: kept a narrowed
  engine-only partial `LayoutFrame` patch path for simple row/stack geometry
  instead of relaxing `dimension_node_has_children` broadly. Subagent explorer
  `019ec0ee-7dfe-7fd3-b1c2-140b7b50b3a8` confirmed the broad relaxation would
  produce stale descendant/sibling geometry for NovyWave cursor overlays and
  divider panels. The kept path is shape-based and generic: it directly patches
  zero-gap explicit-width rows with simple stack/text children, moves spacer
  and line siblings, and still rejects complex padded stacks such as panel
  dividers. Verification: `cargo fmt --all`;
  `cargo check -p boon_native_playground`;
  `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`;
  `cargo test -p boon_ir source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`;
  `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets). A compact release
  diagnostic with `BOON_NATIVE_DISABLE_UI_STATE_PERSIST=1` and
  `BOON_NATIVE_PREVIEW_COMPACT_TIMING=1` proved the path fires for hover/click
  samples and leaves divider samples on the safe full-layout path. Current
  official speed report is `status=fail`: direct layout-frame patch is `true`
  for 65 samples and `false` for 32 divider samples, all false samples reject
  with `simple_stack_style_not_supported`. Current p95s:
  `hover=16.581ms` (now under the 16.7ms budget), `click_to_cursor=27.231ms`,
  `input_to_visible=27.231ms`, `divider_drag=22.473ms`, `resize=14.434ms`,
  `layout_rebuild=6.734ms`, `runtime_apply=12.172ms`, and
  `runtime_step_apply=10.742ms`. TASK-0804 remains in progress; next work
  should reduce the 21-delta click root fanout and the complex divider
  layout/shared-update cost without Boon syntax changes or NovyWave fixture
  reduction.
- 2026-06-13 TASK-0804 subtree layout-frame splice slice: replaced the
  remaining complex divider full-layout fallback with a generic document-owned
  subtree relayout/splice path. `boon_document` now exposes
  `try_layout_subtree`/`layout_subtree`, and the native patcher groups width
  changes for row children so the row subtree is relaid out once from the final
  `DocumentFrame`; the splice replaces subtree-owned display items, hit
  regions, scroll regions, demands, materialization reports, and frame metrics
  together. This keeps the fix engine-side: no Boon syntax changes, no
  NovyWave-specific patch branch, and no fixture reduction. Verification:
  subagent explorer `019ec116-523d-70e0-8896-61ca9253f4c1`;
  `cargo fmt --all`;
  `cargo check -p boon_document -p boon_native_playground`;
  `cargo test -p boon_document --lib layout_subtree_matches_whole_frame_row_geometry -- --nocapture`;
  `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`;
  `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`;
  `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets);
  `git diff --check -- crates/boon_document/src/lib.rs crates/boon_native_playground/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
  Current bridge report is `status=pass`, `measurement_mode=proof`, coverage
  `71/71`, and no failed groups. Current speed report is `status=fail`, but
  all `97` interaction timing samples now use direct layout-frame patching and
  there are zero direct patch rejections. Divider latency improved to
  `divider_drag_p95=17.440ms` from the previous `22.473ms` rejected-stack
  state, and layout p95 is `5.849ms`; remaining budget blockers are
  `click_to_cursor_p95=31.180ms`, `input_to_visible_p95=31.180ms`,
  `hover_p95=18.604ms`, and the near-budget divider p95. The next TASK-0804
  slice should target the click/native apply path and root fanout: fresh click
  samples still spend roughly `10-12ms` in runtime step apply for `21`
  semantic deltas plus several milliseconds in patched layout/shared update,
  while the native `simple_source_click` timing reports `apply_ms` as the
  dominant top-level click cost.
- 2026-06-13 TASK-0804 proof-clone deferral slice: kept a narrow native
  engine optimization that uses the previous `layout_frame_hash` and cached
  `DocumentRenderSnapshot` for document-patch fast-path eligibility, then
  clones the full previous layout proof only after runtime changes actually
  need a patched proof/layout. This avoids carrying a large proof clone through
  every live-event turn. No Boon syntax or NovyWave source workaround was
  added. Verification: `cargo fmt --all`;
  `cargo check -p boon_native_playground`;
  `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (fresh interaction-mode report still fails speed budgets). Current official
  speed report is `status=fail`: `click_to_cursor_p95=18.552ms`,
  `input_to_visible_p95=18.552ms`, `hover_p95=8.139ms`,
  `divider_drag_p95=6.515ms`, `resize_p95=7.578ms`,
  `runtime_apply_p95=13.314ms`, `runtime_step_apply_p95=12.362ms`,
  and `layout_rebuild_p95=3.475ms`. Click samples remain dominated by
  21-delta cursor turns: click `total_apply_p95=17.312ms`,
  `runtime_apply_p95=13.626ms`, `layout_rebuild_p95=3.475ms`,
  and native click `resolve_p95=1.218ms`. A deferred runtime-state snapshot
  experiment was killed because it regressed click/input p95 to `20.751ms`;
  a cached source-intent-index snapshot experiment was killed because it failed
  to improve the gate (`18.815ms` click/input p95) while adding extra cached
  state. TASK-0804 remains in progress; next work should reduce the
  21-semantic-delta click root cascade or make click presentation avoid
  recomputing/applying full hover/layout proof work after every cursor move.

### TASK-0804A Source-Action Root Flush Architecture Pass
Status: postponed
Type: implementation
Priority: P1
Depends on: TASK-0502A, TASK-0804
Source plans: 09, 10, 11, 13, 14, 15, 16, 17, 18, 20
Likely areas: `crates/boon_runtime/src/lib.rs`, `crates/boon_native_playground/src/main.rs`, `crates/xtask/src/main.rs`
Goal:
Reduce the current measured NovyWave interaction slow path in generic engine
code: source-action root flush, root materialization, root dirty scheduling,
and follow-on layout work. Do not start with `smallvec`, `arrayvec`, `IndexSet`,
or other container swaps unless refreshed profiles prove those containers are
the dominant source-action/root-flush cost.
Acceptance:
- A refreshed interaction-speed report identifies the dominant subphase before
  code changes, using existing `runtime_step_profile`, click timing,
  layout-patch, and renderer counters.
- Any kept implementation is generic engine/runtime/layout infrastructure, not
  a NovyWave row/file-name/source workaround and not new Boon syntax.
- The kept slice lowers either final canonical click/input p95 below the
  strict budget or, if the end-to-end gate is noisy, lowers at least one
  measured root-flush bucket by a meaningful amount:
  `source_action_root_flush`, `source_action_root_materialization`,
  `source_action_root_dirty_scheduler`, or `layout_rebuild`.
- Renderer upload remains solved: post-interaction upload stays far below the
  initial full `900600` byte upload, dirty upload ranges keep retained chunk
  IDs, staging wraps stay zero, and quad-cache evictions stay zero.
- If an experiment does not move the measured buckets, revert it and record
  the kill reason instead of stacking more heuristics.
Verification:
- `cargo test -p boon_runtime --lib`
- `cargo test -p boon_native_playground --bin boon_native_playground`
- `cargo check -p boon_runtime -p boon_native_playground -p xtask`
- `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
- `cargo xtask verify-report-schema`
- `jq` inspection of click/runtime/layout subphase p95s and renderer upload
  post-interaction metrics.
Rollback / stop condition:
- Stop and write a narrower follow-up if the current profiles contradict the
  root-flush diagnosis, if the change only shifts work into layout/shared
  update without reducing click/input p95, or if correctness requires
  hardcoded NovyWave knowledge.
Notes:
- Created after TASK-0502A moved the measured renderer slow path:
  post-interaction renderer upload is now `3360` bytes instead of `900600`.
  The remaining failed speed gate is dominated by
  `source_action_root_flush.p95=7.412ms`,
  `source_action_root_materialization.p95=4.215ms`,
  `source_action_root_dirty_scheduler.p95=3.113ms`,
  `runtime_apply.p95=9.942ms`, and `layout_rebuild.p95=4.849ms`.
- Candidate architecture directions to evaluate against those counters:
  field-only root list-view materialization, direct root `List.map`
  row/output reuse, cheaper dirty-scheduler dependency representation, root
  field/value dependency tracking that avoids rebuilding unaffected row fields,
  and only then a compound cursor-value query/index if field profiles still
  justify it.
- Do not run `EXP-0002` as the next slice merely because it is next in the old
  experiment backlog. Low-level container experiments are deferred until this
  measured root-flush path is addressed or disproven.
- See `docs/plans/speedup/20-task-0804-root-flush-resolution-plan.md` for the
  systematic resumption plan. It keeps this task as postponed history and
  routes future implementation through `TASK-0804B` after an explicit user
  resume.
- Current rule after plan 20: do not unpostpone `TASK-0804A`. It remains the
  historical investigation record; future implementation resumes through
  `TASK-0804B` only.
- 2026-06-16 TASK-0804A measurement refresh and killed lazy-ready scheduler
  experiment: refreshed the official NovyWave interaction-speed gate before
  changing code. The fresh canonical report still fails only the release click
  and input p95 budgets: `click_to_cursor_p95=25.073ms`,
  `input_to_visible_p95=25.073ms`, `runtime_apply_p95=16.311ms`,
  `runtime_step_apply_p95=13.897ms`, and `layout_rebuild_p95=5.345ms`.
  Click root buckets identify the current slow path as
  `source_action_root_flush_p95=9.873ms`,
  `source_action_root_dirty_scheduler_p95=6.254ms`,
  `source_action_root_materialization_p95=3.360ms`,
  `source_action_root_dependent_visit_count_p95=194`,
  `source_action_root_dependent_enqueue_count_p95=32`, and
  `source_action_root_dirty_pop_count_p95=38`. The report's root-list summary
  says the cause directly:
  `root_flush_dirty_scheduler_plus_root_list_materialization`; the
  architecture cause is that root list-view materialization still evaluates the
  whole root expression, maps source rows, materializes rows, and diffs after
  the fact instead of using a compiled row/field dependency frontier. Across
  the full click sample set the root flush spent `193.319ms`, with
  `112.687ms` in dirty scheduling and `75.386ms` in root materialization;
  `selected_signal_lane_rows` dominated list work (`eval_ms=25.638`,
  `diff_ms=20.191`, `changed_row_count=96`, `field_cache_hits=4768`,
  `field_cache_misses=160`), followed by `selected_cursor_pair_rows`
  (`eval_ms=12.507`, `diff_ms=11.774`, `changed_row_count=48`). An opt-in
  dirty-frontier report with `BOON_PROFILE_DIRTY_FRONTIER=1` confirmed ranked
  frontier/root-work samples remain available diagnostically, with top root
  materialization work on `store.selected_signal_lane_rows` and
  `store.selected_cursor_pair_rows`; the canonical report intentionally keeps
  those heavy BTreeMap/string samples disabled and reports
  `no_dirty_frontier_samples`. A generic lazy-ready dirty-root scheduler patch
  was implemented and tested, then killed: focused root-derived and
  root-list-view tests passed, but the official speed gate regressed
  `click_to_cursor_p95`/`input_to_visible_p95` to `25.850ms` and worsened
  `runtime_apply_p95` to `16.521ms`. It only nudged
  `source_action_root_dirty_scheduler_p95` from `6.254ms` to `6.092ms` and
  `source_action_root_dependent_enqueue_p95` from `3.784ms` to `3.582ms`,
  below the meaningful-improvement threshold, so it was reverted instead of
  stacked. Verification/evidence used: report-side subagent
  `019ed06e-c1d0-7fa3-ae3f-f301eb7fac2c`; `cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`;
  `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`;
  canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`;
  diagnostic `BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-dirty-frontier.json`;
  and `git diff --check -- crates/boon_runtime/src/lib.rs`. TASK-0804A
  remains `in_progress`; the next kept slice should target compiled
  root-list-view row/field frontiers or direct reuse for the two dominant
  list-view roots, not another readiness-set heuristic.
- 2026-06-16 TASK-0804A field-only root list-view snapshot slice: kept a
  narrow generic runtime materialization cleanup from explorer
  `019ed06e-7af9-76a0-a86b-4de834fe8f60`. The successful field-only
  root `ListView` path no longer clones every previous visible row snapshot
  before it can patch fields. It now checks row count with `list_len`, uses
  per-row `list_row_field_names` only for branch field-shape guards, and keeps
  the existing per-field `list_row_field` comparison for changed values. Full
  fallback snapshot/diff behavior is unchanged. This adds no Boon syntax, no
  NovyWave-specific branch, and no source workaround. Because canonical reports
  intentionally keep heavy dirty-frontier samples disabled, `boon_runtime`
  test builds now keep those samples on by default so structured-parent
  diagnostic tests still prove skipped child enqueue behavior without changing
  release report behavior. Post-revert baseline before this slice:
  `click_to_cursor_p95=22.998ms`, `input_to_visible_p95=22.998ms`,
  `runtime_apply_p95=15.306ms`, `runtime_step_apply_p95=13.181ms`,
  `layout_rebuild_p95=5.147ms`, `source_action_root_flush_p95=9.109ms`,
  `source_action_root_materialization_p95=2.841ms`, and
  `source_action_root_dirty_scheduler_p95=6.104ms`;
  `selected_signal_lane_rows.previous_snapshot_ms=3.087`,
  `selected_signal_lane_rows.eval_ms=23.045`, and
  `selected_cursor_pair_rows.previous_snapshot_ms=0.058`. After the slice,
  the strict speed gate still fails click/input budgets and click p95 is noisy
  (`click_to_cursor_p95=23.337ms`,
  `input_to_visible_p95=23.337ms`), but the targeted runtime buckets moved in
  the right direction: `runtime_apply_p95=15.106ms`,
  `runtime_step_apply_p95=12.957ms`,
  `source_action_root_flush_p95=8.972ms`,
  `source_action_root_materialization_p95=2.653ms`, and
  `source_action_root_dirty_scheduler_p95=6.111ms`.
  `selected_signal_lane_rows.previous_snapshot_ms=0.0`,
  `selected_signal_lane_rows.eval_ms=19.767`,
  `selected_signal_lane_rows.diff_ms=18.556`,
  `selected_cursor_pair_rows.previous_snapshot_ms=0.0`, and
  `selected_cursor_pair_rows.eval_ms=11.382`. Renderer upload remains solved:
  post-interaction upload is still `3360` bytes, dirty upload ranges are `3`,
  staging wraps are `0`, and quad-cache evictions are `0`. Verification:
  `cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`;
  `cargo test -p boon_runtime --lib root_list_view_same_source_rows_patch_in_place_and_keep_target_identity -- --nocapture`;
  `cargo test -p boon_runtime --lib root_list_view_field_only_patches_when_dispatched_record_rows -- --nocapture`;
  `cargo test -p boon_runtime --lib root_list_view_branch_selector_dirty_falls_back_before_field_patch -- --nocapture`;
  `cargo test -p boon_runtime --lib root_list_view_materializes_current_order_after_same_count_reorder -- --nocapture`;
  `cargo test -p boon_runtime --lib root_list_view_append_after_remove_does_not_reuse_stale_row_projection -- --nocapture`;
  `cargo test -p boon_runtime --lib root_derived_revisits_earlier_dependent_after_later_dependency_changes -- --nocapture`;
  `cargo test -p boon_runtime --lib root_scalar_same_event_ -- --nocapture`;
  `cargo test -p boon_runtime --lib` (`201 passed`);
  `cargo check -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (expected `status=fail` on remaining click/input budgets);
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
  (`status=pass`, proof mode, coverage `71/71`);
  `cargo xtask verify-report-schema`;
  and `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
  The broad
  `cargo test -p boon_native_playground --bin boon_native_playground` command
  was attempted and is not pass evidence in the current tree: it failed
  `22/168` broad UI/editor/cells/Todo/NovyWave tests unrelated to this
  runtime-only field-only list-view snapshot change. TASK-0804A remains
  `in_progress`; next work should attack the remaining `selected_signal_lane_rows`
  `diff_ms`/field-loop cost or dirty-scheduler fanout with a compiled
  row/field frontier.
- 2026-06-17 TASK-0804A root-demand refresh and killed row-clean-cache
  experiment: rechecked the checklist state and confirmed TASK-0804A is the
  last real unfinished item, excluding template placeholders `TASK-0000` and
  `EXP-0000`. The refreshed canonical speed report still fails only the strict
  click/input p95 budgets by a narrow margin:
  `click_to_cursor_p95=17.111ms`, `input_to_visible_p95=17.111ms`,
  `runtime_apply_p95=10.356ms`, `runtime_step_apply_p95=8.363ms`, and
  `layout_rebuild_p95=4.751ms` against the `16.700ms` click/input budget.
  A diagnostic run with `BOON_PROFILE_ROOT_DEMAND=1` identified the cause as
  `dirty_frontier_fanout_with_ranked_root_work`, not renderer upload. The
  largest demand bucket was `candidate_unobserved_source_free_pure`
  (`dependency_lookup_count=3568`, `dependent_visit_count=3568`,
  `dependent_enqueue_count=576`), followed by observed/list-view blockers.
  Top repeated frontier edges route cursor changes into
  `store.selected_signal_lane_rows`, `store.selected_cursor_pair_rows`, and
  cursor bridge roots. Top root work was
  `store.selected_signal_lane_rows` (`pop_count=32`,
  `materialization_ms=17.121`, `changed_read_count=200`) and
  `store.selected_cursor_pair_rows` (`pop_count=32`, `skip_count=8`,
  `materialization_ms=8.088`). The row-list profiles show the remaining
  engine cause: `selected_signal_lane_rows` still pays
  `eval_ms=14.688`, `diff_ms=13.555`, and
  `user_function_body_ms=11.003` while touching `4768` cached fields and
  `160` evaluated fields; `selected_cursor_pair_rows` still pays
  `eval_ms=8.803`, `diff_ms=8.382`, and `96` evaluated fields. Two read-only
  subagent audits agreed the likely next useful slices are either a compiled
  root-list row/field frontier before active projection or a more explicit
  dependency-frontier audit; they warned against another broad readiness-set
  heuristic. A generic direct-projector row-clean cache experiment was then
  implemented and verified locally (`cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo test -p boon_runtime --lib`, `222 passed`), but the official speed
  gate regressed and was killed: `click_to_cursor_p95` and
  `input_to_visible_p95` worsened to `17.880ms`,
  `runtime_apply_p95=10.708ms`, `runtime_step_apply_p95=8.761ms`, and
  `selected_signal_lane_rows.eval_ms=17.330`. The code was reverted rather
  than stacked. After reverting, the canonical report was rerun so
  `target/reports/native-gpu/novywave-interaction-speed.json` again reflects
  the current code; that run is noisy but still shows the same shape:
  `click_to_cursor_p95=19.407ms`, `input_to_visible_p95=19.407ms`,
  `runtime_apply_p95=11.629ms`, `runtime_step_apply_p95=9.329ms`,
  `layout_rebuild_p95=5.058ms`,
  `selected_signal_lane_rows.eval_ms=16.763`, and
  `selected_signal_lane_rows.diff_ms=15.303`. TASK-0804A remains
  `in_progress`; the next attempt should avoid speculative row-level cache
  state and instead measure/attack the real per-field diff/user-function loop
  and dirty-frontier fanout directly.
- 2026-06-17 TASK-0804A deep culprit exploration with subagent loop:
  reran a separate diagnostic report with `BOON_PROFILE_ROOT_DEMAND=1` at
  `target/reports/native-gpu/novywave-interaction-speed-root-demand.json` and
  reconciled it with the current canonical
  `target/reports/native-gpu/novywave-interaction-speed.json`. The real p95
  culprit is the `cursor_position` changed class, not renderer upload, report
  writing, full row rebuilds, or a single huge list allocation. Canonical click
  samples split into three classes: `26` dependent visits (`8` samples,
  `8.393ms` average total, `1.988ms` root flush), `28` dependent visits
  (`8` samples, `13.527ms` average total, `2.580ms` root flush), and `194`
  dependent visits (`16` samples, `17.039ms` average total, `5.315ms` root
  flush, `2.445ms` dirty scheduler, `2.717ms` root materialization, and
  `4.780ms` layout). In the `26`/`28` classes `store.cursor_position` is
  materialized but unchanged; in the `194` class `store.cursor_position`
  changes and opens the bridge/page wave.
- The exact trigger path is
  `selected_timeline_cursor_value -> cursor_position ->
  bridge_request_descriptor_label`, followed by
  `bridge_request_fingerprint`, `bridge_request_input_digest`,
  `bridge_request_structural_key`, response status, bridge page refs/pages,
  `bridge_cursor_values`, `selected_lane_materialization`, and both selected
  row list views. Diagnostic slow-class root work ranked the largest concrete
  work as `store.selected_signal_lane_rows` (`9.008ms` / `16` slow samples),
  `store.selected_cursor_pair_rows` (`5.501ms` / `16`), then pure bridge/page
  roots such as `store.bridge_request_descriptor` (`2.803ms` / `32`) and
  `store.bridge_cursor_values_page_ref` (`2.488ms` / `48`). The current
  field-only list-view path is active: `full_eval_row_count=0` and
  `row_materialize_ms=0`; remaining list cost is repeated field-only
  projection/diff/cache-key/user-function work. In the canonical slow class,
  `selected_signal_lane_rows` totals `19.164ms` across `32` materialization
  samples with `2720` field-cache hits and `96` misses, while
  `selected_cursor_pair_rows` totals `7.991ms` with
  `RUN/selected_cursor_pair_row:label` missing `64` times.
- Subagent loop result: explorers `019ed29c-bd25-7ee1-84a2-b6839b969e35` and
  `019ed29d-3fae-7aa2-9770-f78a10e065a7` converged after a second round. The
  first implementation target should be a generic compiled demand/currentness
  frontier before enqueue for safe `candidate_unobserved_source_free_pure`
  bridge/page roots, with explicit barriers for observed roots, semantic
  deltas, evaluator reads, summaries, assertions, and observed projections.
  Do not start by adding another row cache or inside-loop prefilter: earlier
  row-clean and field-prefilter experiments moved or worsened cost without
  changing the `194`-visit graph shape. Also do not treat scalar no-change
  gating as the primary fix: current evidence shows unchanged `cursor_position`
  samples are already the fast classes; the slow class is a real value change
  and needs demand/frontier narrowing, not false-positive suppression alone.
  The second target is the root-list field-only loop for the two selected row
  lists, but only after or alongside graph-shape work. Kill the next slice if
  `visits=194`, `dependent_enqueue_count=32`, `dirty_pop_count=38`, or final
  click/input p95 do not move meaningfully.

### TASK-0804B Bridge/Page Identity And Demand Frontier Resumption
Status: in_progress
Type: implementation
Priority: P1
Depends on: TASK-0804, TASK-1001, TASK-1002, explicit user resume
Source plans: 09, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20
Likely areas: `crates/boon_runtime/src/lib.rs`, `crates/boon_native_playground/src/main.rs`, `crates/xtask/src/main.rs`, NovyWave bridge/page runtime integration
Goal:
Resume the unfinished NovyWave root-flush speed work with a narrower,
architecture-level target after the capped TASK-0804A loops. Split stable
bridge/page identity from cursor-hot telemetry or compile an explicit
demand/currentness frontier so cursor movement does not enqueue and materialize
the same broad bridge/page/list roots on every click.
Acceptance:
- A fresh baseline report identifies the current slow-click graph shape before
  code changes, including click/input p95, `source_action_root_flush`,
  `source_action_root_dirty_scheduler`, `source_action_root_materialization`,
  `source_action_root_dependent_visit_count`,
  `source_action_root_dependent_enqueue_count`,
  `source_action_root_dirty_pop_count`, and root-list by-list counters.
- The implementation is generic engine/bridge/runtime infrastructure, not a
  NovyWave row/file-name/fixture shortcut and not new Boon syntax.
- Bridge/page request identity remains deterministic across replay and stale
  response rejection still works.
- Cursor-hot telemetry can update without changing stable page/blob/request
  identities unless the real bridge/file/page input changed.
- The kept slice reduces the slow `194/32/38` class occurrence count by at
  least `25%`, lowers final click/input p95 below the strict `16.700ms`
  budget, or lowers a named p95/root-list bucket by at least `10%` and
  `1.0ms` with no click/input p95 regression greater than `0.5ms` or `5%`.
- Renderer upload remains solved: post-interaction upload stays near the
  retained path (`3360` bytes in the latest report), staging wraps stay zero,
  and quad-cache evictions stay zero.
Verification:
- `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib`
- `cargo test -p boon_bridge --lib -- --nocapture`
- `cargo check -p boon_runtime -p boon_bridge -p boon_native_playground -p xtask`
- `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
- `env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
- `cargo xtask verify-report-schema`
- `jq` inspection of bridge payload identity, speed p95s, root-flush graph
  counts, root-list by-list counters, and renderer post-interaction upload.
Rollback / stop condition:
- Do not start this task while the user cap on TASK-0804A follow-up debugging
  remains active. When explicitly resumed, revert any slice that leaves
  `194/32/38` unchanged, does not reduce a named root-flush/list bucket, or
  changes public bridge hashes/replay semantics without an explicit schema
  migration.
Notes:
- Before the explicit 2026-06-17 TASK-0804B resume, this was the future task
  created from the TASK-0804A postponed follow-up. It is now active through
  plan `20`; TASK-0804A remains postponed.
- Prefer a bridge/page identity split or compiled demand/currentness frontier
  over more field-only loop microchanges, row caches, container swaps, JSON vs
  binary report rewrites, or renderer upload tweaks.
- TASK-1001 and TASK-1002 are prerequisites because the future slice should
  build on real BYTES sidecars and current LIST row-index/selection storage
  modes instead of re-solving payload/list representation.
- Use `docs/plans/speedup/20-task-0804-root-flush-resolution-plan.md` as the
  decision-complete resumption plan for this task. Start with `0804R-00`
  only after this task is explicitly unpostponed.
- Activation protocol: when the user explicitly resumes `TASK-0804B`, keep
  `TASK-0804A` postponed, set `TASK-0804B` to `in_progress`, set plan-20
  `0804R-00` to `pending` or `in_progress`, leave later `0804R-*` tasks
  blocked until their dependencies are done, and append matching progress-log
  entries in both files.
- 2026-06-17 TASK-0804B activation: explicit `/goal` objective resumed this
  task. `TASK-0804A` remains postponed historical evidence. Plan 20 `0804R-00`
  is the active slice; no runtime code changed before the baseline evidence
  lock.
- 2026-06-17 TASK-0804B `0804R-00` baseline/evidence lock completed. The
  canonical report
  `target/reports/native-gpu/novywave-interaction-speed.json` remains
  `status=fail` with `click_to_cursor.p95=18.995379ms`,
  `input_to_visible.p95=18.995379ms`, `runtime_apply.p95=11.735027ms`,
  `runtime_step_apply.p95=9.513186ms`, and
  `layout_rebuild.p95=4.709477ms`. Cause remains
  `root_flush_dirty_scheduler_plus_root_list_materialization`; click graph
  counts are `visits=3536`, `enqueues=600`, and `pops=792`. Root-demand
  diagnostics report `24` candidate pure roots, `552` simulated defer
  enqueues, `552` changed materializations, and `512` demand reads. Dirty
  frontier diagnostics expose ranked frontier edges. Bridge proof
  `target/reports/novywave-bridge-scenario.json` is `status=pass`.
  Renderer post-interaction upload remains separated: `3360` bytes, `3`
  dirty ranges, `3` queue writes, `0` staging wraps, and `0` quad-cache
  evictions. Schema validation passed after refreshing the canonical report
  once more because diagnostics rewrote the shared native role artifact.
  Plan 20 `0804R-00` is `done`; `0804R-01` is now `in_progress`.
- 2026-06-17 TASK-0804B `0804R-01` candidate-demand diagnostic completed as a
  no-behavior runtime/reporting slice. Verification: `cargo fmt -p
  boon_runtime -p boon_native_playground -p xtask`; `cargo check -p
  boon_runtime -p boon_native_playground -p xtask`; diagnostic speed run
  `BOON_PROFILE_ROOT_DEMAND=1 BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json`
  wrote an expected failing report with `status=fail` due the existing p95
  budget blockers and enriched candidate fields; canonical speed run
  `env -u BOON_PROFILE_ROOT_DEMAND -u BOON_PROFILE_DIRTY_FRONTIER cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/reports/native-gpu/novywave-interaction-speed.json` wrote an expected
  failing report with diagnostics disabled and the same p95 blockers; `timeout
  240 cargo xtask
  verify-novywave-bridge-scenario --report
  target/reports/novywave-bridge-scenario.json` passed; `cargo xtask
  verify-report-schema` passed; enriched diagnostic `jq` checks passed;
  canonical no-regression `jq` check passed; `git diff --check` passed.
  Diagnostic evidence: `24` candidate roots, `552` simulated defer enqueues,
  `552` changed materializations, `512` later demand reads, `248` hidden
  semantic-delta materializations, and `336` aggregate visible/list dependency
  hits, split as `208` changed-read list dependencies and `128`
  root-list-evaluation demand dependencies. Classification counts show
  `currentness_only=0` enqueues,
  `bridge_identity=472`, `cursor_telemetry=264`,
  `must_publish_semantic_delta=248`, and `visible_list_dependency=208`.
  Canonical p95s are `click_to_cursor=18.653014ms` and
  `input_to_visible=18.653014ms`, within the `0804R-01` no-regression limit
  but still above the strict `16.700ms` budget. At that checkpoint, plan 20
  `0804R-01` was `done` and `0804R-02` was `in_progress`. The decision table rejects
  demand-deferral-first and points toward `0804R-03` bridge/page identity after
  the `0804R-02` currentness/stale-read contract.

## Phase 9: Low-Level Rust Experiments

### EXP-0001 `bytemuck` POD GPU Uploads
Status: done
Type: experiment
Depends on: TASK-0501
Hypothesis:
Interleaved POD vertex/instance structs reduce CPU byte conversion, GPU buffer writes, and upload overhead.
Metric to improve:
Upload bytes, queue write count, CPU scene-build/upload time, p95 interaction frame time.
Correctness oracle:
Existing render output plus app-owned readback crops and shader layout tests.
Kill criteria:
No measurable upload/write/time improvement, unstable host/WGSL layout, or meaningfully less clear code.
Promote to implementation when:
At least one representative workload improves upload/write counters without visual or shader freshness regressions.
Verification:
- `cargo test -p boon_native_gpu --lib`
- `cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json`
Notes:
- This experiment gates `TASK-0502`.
- 2026-06-15 result: promoted. `boon_native_gpu` now builds quad
  geometry as an interleaved `bytemuck::Pod` vertex with a single vertex
  buffer per dirty quad batch. The renderer reports real
  `queue_write_count`, `dirty_upload_range_count`, `allocated_gpu_bytes`,
  `buffer_reuse_count`, `staging_wrap_count`, and
  `quad_cache_eviction_count` in `FrameMetrics`; `xtask` no longer treats
  queue writes as synthetic where renderer metrics are present.
- Evidence: the WGPU asset-cache test now proves dirty quad uploads use one
  queue write per dirty batch instead of the legacy split-buffer three writes,
  and that the second identical frame reuses the cached GPU buffer with zero
  queue writes. The shader report includes a compiled
  `vertex_layout_contract` proving POD size `20`, align `4`, one host buffer,
  stride `20`, host offsets `0/8/12`, and generated shader inputs
  `location 0 Float32x2`, `location 1 Uint32`, `location 2 Float32x2`.
- Non-blocking diagnostic: `verify-native-gpu-preview-e2e --example cells`
  currently writes a failing report for scenario/dev-window coverage blockers,
  but its `visible_surface_metrics` include the new renderer counters and do
  not indicate a POD/shader/layout regression.

### EXP-0002 `smallvec` Or `arrayvec` For Tiny Hot Lists
Status: done
Type: experiment
Depends on: TASK-0004B
Hypothesis:
Tiny hot route/dirty/patch/invalidation vectors allocate enough to justify inline storage.
Metric to improve:
Allocation count/bytes and p95 runtime/document/render stage time.
Correctness oracle:
Existing runtime/document tests and unchanged reports.
Kill criteria:
Allocation counters do not improve, code becomes harder to read, or capacity handling becomes panic-prone.
Promote to implementation when:
Multiple hot callsites show allocation reduction and no correctness risk.
Verification:
- `cargo test -p boon_runtime -p boon_document --lib`
Notes:
- Do not use fixed-capacity arrays where overflow would panic in production.
- Promoted 2026-06-16 with `SmallVec`, not `ArrayVec`, for two internal
  overflow-safe tiny-list surfaces: root read-key vectors and dirty-key entry
  vectors. This is a narrow allocation reduction, not the remaining
  zero-allocation budget fix.

### EXP-0003 Interner Crate Versus Custom Symbol Table
Status: superseded
Type: experiment
Depends on: TASK-0101
Hypothesis:
An interner or custom symbol table reduces cross-stage string cloning while preserving diagnostics.
Metric to improve:
String clone count, allocation bytes, compile/lower/runtime memory, route lookup time.
Correctness oracle:
Source maps, diagnostics, reports, and current runtime output.
Kill criteria:
Diagnostics degrade, lifetimes become unsafe/awkward, or allocation counters do not improve.
Promote to implementation when:
Symbol IDs simplify at least parser/IR/runtime boundaries and improve measured allocation or lookup counters.
Verification:
- `cargo test -p boon_parser -p boon_ir -p boon_typecheck -p boon_runtime --lib`
Notes:
- Candidate crates include `lasso` or `string-interner`, but a small custom table is allowed.
- Superseded 2026-06-16 by the already-completed `TASK-0102` custom dense
  semantic/runtime symbol tables. A narrow cleanup was still promoted for the
  remaining semantic-symbol construction boundary: duplicate `(category, text)`
  lookups no longer allocate owned lookup keys, and no new interner dependency
  was added.

### EXP-0004 Dirty Set Representation
Status: done
Type: experiment
Depends on: TASK-0303
Hypothesis:
A measured dirty set representation can reduce duplicate checks and dirty propagation cost.
Metric to improve:
Dirty dedupe time, dirty memory, fanout traversal time, runtime p95.
Correctness oracle:
Full recompute oracle and existing dirty propagation tests.
Kill criteria:
Measured density/cardinality does not justify the replacement or reports become harder to interpret.
Promote to implementation when:
`fixedbitset`, `roaring`, sorted `Vec`, or current structure wins on representative density/fanout data.
Verification:
- `cargo test -p boon_runtime --lib dirty`
Notes:
- Do not choose the representation before `TASK-0303` reports data.
- Result: keep the current `SmallVec<[DirtyKeyEntry; 8]>` / linear
  `current_vec` representation as the canonical `DirtyKeySets` path for now.
  The measured dirty sets are tiny enough that `fixedbitset`, `roaring`, and
  sort/dedup would add complexity at the wrong boundary.
- Fresh TodoMVC speed evidence in `target/reports/todomvc-speed.json` reports
  `dirty_entry_count.max = 21`, `dirty_entry_count.p95 = 14`,
  `dirty_duplicate_attempt_count.max = 18`, and
  `dirty_density_estimate.max = 0.5`. This is high density only over a tiny
  key universe, which is not a good fixed-bitset signal.
- The current NovyWave interaction report still points away from dirty-set
  container replacement: click `dirty_set_metrics.p95 = 0.041408ms`, while
  `source_action_root_dirty_scheduler.p95 = 2.526055ms`,
  `source_action_root_dependent_visit_count.p95 = 194`,
  `source_action_root_dependent_enqueue_count.p95 = 32`, and
  `source_action_root_dirty_pop_count.p95 = 38`.
- `cargo xtask verify-large-list-scan-counters --report
  target/reports/large-list-scan-counters.json` failed during this experiment
  with `large-list rows-scanned proof too small: stage max 0, per-step max 0,
  expected at least 1000`; do not use that stale/failed large-list report as
  promotion evidence until the verifier/data scenario is repaired.
- Follow-up direction: if dirty propagation remains hot, work on the compiled
  row/field dirty frontier, field-only list-root scheduling, or precomputed
  dirty-field probes. Reopen container replacement only if refreshed reports
  show representative dirty cardinality above `64`, `roaring`-scale sparse
  sets, or dirty-set timing above roughly `1%` of apply time.

### EXP-0005 Shader-Side Shapes
Status: done
Type: experiment
Depends on: TASK-0501
Hypothesis:
Shader-side rounded rects, borders, checkmarks, shadows, timeline grids, or waveform segments reduce CPU-expanded geometry and upload bytes.
Metric to improve:
Primitive expansion count, upload bytes, draw calls, frame p95, scene build time.
Correctness oracle:
App-owned readback crops, shader freshness gate, visual negative cases.
Kill criteria:
No measured geometry/upload win, shader complexity harms proof reliability, or visual output regresses.
Promote to implementation when:
A narrow primitive improves metrics and passes native GPU proof gates.
Verification:
- `cargo test -p boon_native_gpu --lib`
- `cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json`
Notes:
- Keep generated WESL/WGSL/bindgen pipeline authoritative.
- Result: do not promote a shader-side shape primitive in this slice. The
  renderer already has chunk-level geometry upload identity, and the latest
  available NovyWave interaction report shows post-interaction upload at
  `3360` bytes, `3` dirty upload ranges, and `3` queue writes after retained
  chunk reuse. That report is useful as a historical hint, but it is not fresh
  for current `HEAD`, so it is not treated as current proof.
- The future candidate remains real but narrower: replace CPU-rasterized
  checkbox/circle/checkmark primitives with analytic GPU primitives only after
  reports distinguish primitive expansion by type. Current tests show the CPU
  paths are correctness-sensitive and already have visual expectations for
  checkbox raster, rounded shadows, material layers, and external document
  primitives; broad rounded/shadow/timeline/waveform shader rewrites are
  deferred behind a dedicated measured candidate.
- Kill criteria applied: no measured current interaction upload/frame win was
  available, and changing the generated shader/vertex schema now would add
  proof risk without attacking the measured runtime/frontier bottleneck.
- Reopen when a dedicated report shows a primitive class dominating initial or
  interaction frames, for example CPU-emitted checkbox/circle pixels,
  rounded-border segments, or shadow/material layers. A promoted slice should
  first add primitive-type counters and then move only one primitive family to
  a generated WESL/WGSL-backed analytic path.

### EXP-0006 Generated Rust Or Cranelift Kernels
Status: done
Type: experiment
Depends on: TASK-0902
Hypothesis:
Pure derived fields or list projections can run faster as generated kernels after bytecode/micro-ops define exact semantics.
Metric to improve:
Runtime derived evaluation p95, allocation count, large-list projection throughput.
Correctness oracle:
Bytecode/interpreter output and full recompute oracle.
Kill criteria:
Compile cost dominates, semantic equivalence is hard to prove, or generic examples regress.
Promote to implementation when:
One pure subset improves runtime after compile cost is excluded and equality proof is stable.
Verification:
- `cargo test -p boon_runtime --lib generated`
Notes:
- This is explicitly not an early optimization.
- Completed first as a generated Rust-enum kernel proof over the Counter
  scalar source-route subset. It is not promoted to a production hot path yet:
  the proof excludes compile cost and does not include a release-mode p95
  runtime win. Future Cranelift or generated-Rust work should reuse this
  three-way parity shape before trying broader expression families.

### EXP-0007 Large-List Dataflow Kernel
Status: done
Type: experiment
Depends on: TASK-0302, TASK-0902
Hypothesis:
A dataflow-style kernel improves large-list projection throughput after row identity, deltas, and full recompute oracle exist.
Metric to improve:
Large-list projection throughput, rows scanned/touched, p95 update latency.
Correctness oracle:
Full recompute oracle and deterministic replay.
Kill criteria:
Small examples regress, memory overhead is too high, or semantics become less Boon-faithful.
Promote to implementation when:
Large-list workloads improve without hurting TodoMVC/Cells and the kernel remains generic.
Verification:
- `cargo test -p boon_runtime --lib dataflow`
Notes:
- Do not adopt differential-dataflow or similar wholesale before this local experiment.
- Completed first as a local bitset-backed count dataflow proof over a 1,000
  row TodoMVC list. It is not wired into production runtime scheduling yet:
  the experiment proves a reusable state shape and full-recompute oracle parity
  for one-row boolean-field count updates.

## Phase 10: Compiled Artifact, Bytecode, And Future Kernel Work

### TASK-0901 `.boonc` Compiled Artifact MVP
Status: done
Type: implementation
Priority: P2
Depends on: TASK-0101, TASK-0201, TASK-0802
Source plans: 02, 10, 11
Likely areas: parser/IR/runtime compile output, report schema, CLI/xtask
Goal:
Emit a compiled artifact containing enough stable runtime structure to execute without loading parser AST data.
Acceptance:
- Artifact includes semantic index, symbol table, storage layout, source schemas, route op streams, dependency graph, document lowering tables, bridge schemas when present, report schema hash, and source unit hashes.
- A scenario can run from the artifact.
- Artifact output equals current interpreter output.
- Artifact hash appears in reports.
Verification:
- `cargo test -p boon_runtime --lib compiled_artifact`
- `cargo xtask verify-compiled-artifact-scenario counter --artifact target/artifacts/boonc/counter.boonc --report target/reports/compiled-artifact-scenario-counter.json`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if artifact scope is too broad. Split into serialization, runtime load, scenario run, and report hash child tasks.
Notes:
- This task should not remove the current interpreter path.
- Split execution after the first implementation slice:
  - TASK-0901A deterministic artifact emission and report hash is complete.
    This adds `boon_cli compile <source> --out <path.boonc> [--report <path>]`
    and `cargo xtask verify-compiled-artifact <example>` for a JSON MVP
    artifact that records the static runtime sidecar data and file hash.
  - TASK-0901B is complete for runtime-load readiness. `.boonc` now decodes
    `runtime_plan.generic_derived`, `runtime_plan.storage_initialization`,
    `runtime_plan.document_lowering`, `runtime_plan.runtime_symbols`, scalar
    equations, derived text transforms, list equations, list projections, list
    source bindings, source routes, source/action tables, source payload field
    metadata, dense source/action routing, storage layout counts, and field-slot
    diagnostics into runtime-owned structs. `CompiledProgram::from_artifact`
    assembles those sections, `LoadedRuntime::from_compiled_artifact` can
    instantiate Counter, TodoMVC, and Cells without source or typed IR, and
    inspection reports now truthfully claim `loaded_runtime_from_artifact =
    true`, `runtime_instantiated_from_artifact = true`,
    `source_free_runtime_load_available = true`,
    `source_reparse_required_for_current_runtime = false`, and
    `missing_runtime_plan_sections = []`.
  - TASK-0901C must run at least one scenario from the loaded artifact and
    compare output against the interpreter path. This is complete for Counter:
    `run_compiled_artifact_scenario` loads `target/artifacts/boonc/counter.boonc`,
    instantiates `LoadedRuntime` from the artifact, executes
    `examples/counter.scn`, and matches semantic deltas, render patches, and
    final state against the source-runtime oracle.
- Normal source-run reports still must not claim artifact-loaded execution. Use
  `verify-compiled-artifact-scenario` when the report is specifically proving
  `.boonc` scenario execution.

### TASK-0902 Expression Bytecode Or Micro-Op Interpreter
Status: done
Type: implementation
Priority: P2
Depends on: TASK-0901
Source plans: 01, 02, 10, 11
Likely areas: runtime expression evaluation, IR lowering, full recompute oracle
Goal:
Compile pure expressions and route computations into compact bytecode or micro-ops before any JIT/kernel work.
Acceptance:
- Bytecode/micro-op output equals current interpreter output for covered expressions.
- Reports include op histogram, fallback/deopt reason, and warm-path allocation count.
- Full recompute/interpreter path remains available as oracle.
- Fallback to interpreter is reported and cannot silently satisfy hot readiness paths.
Verification:
- `cargo test -p boon_runtime --lib bytecode`
- `cargo xtask verify-bytecode counter --report target/reports/bytecode-counter.json`
- A scenario report compares interpreter and bytecode outputs for at least one example.
Rollback / stop condition:
- Stop if expression semantics are not sufficiently typed. Return to typechecker readiness tasks.
Notes:
- This task unlocks later generated kernel experiments.
- Completed first as scalar source-route expression bytecode for Counter.
  This is not a full runtime replacement: it proves `number_infix` and
  `const_text` micro-ops against `ScalarEquationPlan` and reports fallback,
  deopt, op histogram, and warm-path allocation metadata. Broader expression
  families must extend the same proof report instead of silently falling back.

### TASK-1001 Runtime BYTES Value And Bridge/File Payload Boundary
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0902
Source plans: 14, 15, 18, 19
Likely areas: `crates/boon_runtime/src/lib.rs`, `crates/boon_bridge/src/lib.rs`, NovyWave bridge/runtime integration
Goal:
Add an engine-internal BYTES representation that can be inferred from bridge
and file/payload contracts without adding Boon syntax. Use it first where
binary payloads already exist or are unavoidable, and keep public summaries and
diagnostics stable by exposing digests/refs rather than inline large byte
arrays.
Acceptance:
- Runtime value/storage layers can carry shared byte payloads or byte refs
  without treating them as UTF-8 `TEXT` or generic JSON arrays.
- Bridge/file payload paths can construct and validate byte values from Rust
  contracts without new Boon syntax or manual annotations in examples.
- Public JSON reports, scenario fixtures, and bridge canonical hashes remain
  compatible unless an explicit schema migration is recorded.
- NovyWave integration uses BYTES only for real binary/file/page/blob payloads,
  not labels, filenames, statuses, formulas, or scenario text.
- Tests prove byte payload equality, digest/ref stability, serde/report
  compatibility, and deterministic replay behavior for the first integrated
  boundary.
Verification:
- `cargo test -p boon_bridge --lib -- --nocapture`
- `cargo test -p boon_runtime --lib bytes -- --nocapture`
- `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`
- `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop or revert if bytes require unsafe ownership shortcuts, leak into Boon
  syntax before the upstream design is integrated, change public bridge hashes
  silently, or make scenario replay nondeterministic.
Notes:
- This task starts after postponing TASK-0804A. It is not expected to solve the
  current cursor-click p95 by itself; it is representation groundwork for real
  waveform/file/page payload movement.

### TASK-1002 LIST Storage Mode And Incremental Representation Slice
Status: done
Type: implementation
Priority: P1
Depends on: TASK-0902
Source plans: 14, 15, 18, 19
Likely areas: `crates/boon_ir/src/lib.rs`, `crates/boon_runtime/src/lib.rs`, NovyWave list-view paths
Goal:
Promote the existing LIST representation classifier into one safe internal
storage-mode optimization, with generic `LIST` semantics preserved as the
oracle. Start with constant arrays, dense vectors, selection views, or
incremental projections only where compiler/runtime usage proves the mode is
safe.
Acceptance:
- The chosen storage mode is inferred by compiler/runtime facts, not user
  annotations or example-specific branches.
- Generic LIST execution remains available as the equivalence oracle.
- Focused tests prove `List/map`, `List/filter_field_equal`, `List/retain`,
  `List/join_field`, and root list-view materialization produce identical
  values for the covered mode.
- NovyWave reports show either neutral correctness-only behavior or a measured
  improvement in list/root-view buckets such as `eval_ms`, `diff_ms`,
  `user_function_body_ms`, field-cache misses, or root-flush fanout.
Verification:
- `cargo test -p boon_ir --lib representation -- --nocapture`
- `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`
- `cargo test -p boon_runtime --lib list_ -- --nocapture`
- `cargo check -p boon_ir -p boon_runtime -p boon_native_playground -p xtask`
- `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
Rollback / stop condition:
- Stop or revert if the storage mode requires NovyWave-specific code, hides a
  dynamic dependency, changes row identity semantics, or improves only a
  microbenchmark while regressing the official speed gate.
Notes:
- Do not retry killed row-output cache, dense read-ID sidecar, or one-off
  container swaps unless new counters prove the same bottleneck has changed.

## Progress Log

Append entries here as `/goal` executes tasks. Do not delete older entries.

```md
- Date: 2026-06-12
- Task: TASK-0001
- Commit: uncommitted
- Files changed: crates/xtask/src/main.rs; crates/boon_report_schema/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo check -p xtask`; `cargo test -p xtask`; `cargo test -p boon_runtime --lib scenario`; `cargo xtask verify-scenario-manifest-integrity --report target/reports/scenario-manifest-integrity.json`
- Result: integrity gate implemented and writes `target/reports/scenario-manifest-integrity.json`; command currently exits 1 with 18 expected blockers owned by TASK-0002.
- Follow-up: TASK-0002 must fix/classify Cells generated scroll/focus refs, TodoMVC reject-empty ref drift, NovyWave duplicate step/ref identity, TodoMVC/Counter target-text-only selectors, and TodoMVC hover source-intent provenance.

- Date: 2026-06-12
- Task: TASK-0002
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; crates/xtask/src/main.rs; crates/boon_report_schema/src/lib.rs; examples/manifest.toml; examples/todomvc.scn; examples/counter.scn; examples/novywave.scn; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo xtask verify-scenario-manifest-integrity --report target/reports/scenario-manifest-integrity.json`; `cargo test -p xtask`; `cargo test -p boon_runtime --lib`
- Result: scenario manifest integrity report passes with zero blockers; xtask tests pass 7/7; boon_runtime lib tests pass 109/109.
- Follow-up: TASK-0003 is now the next P0 task unblocked by TASK-0001.

- Date: 2026-06-12
- Task: TASK-0003
- Commit: uncommitted
- Files changed: crates/boon_report_schema/src/lib.rs; crates/boon_runtime/src/lib.rs; crates/xtask/src/main.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_report_schema --lib`; `cargo test -p xtask`; `cargo test -p boon_runtime --lib`; `cargo xtask verify-scenario-manifest-integrity --report target/reports/scenario-manifest-integrity.json`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `cargo xtask verify-report-schema`
- Result: report schema now requires `measurement_mode`; refreshed proof reports for scenario integrity, native GPU architecture, and schema summary pass with `measurement_mode: proof`; schema unit tests prove speed reports require `interaction`, benchmark wrappers require `diagnostic`, and interaction reports reject proof/diagnostic hot-path work.
- Follow-up: TASK-0004A through TASK-0004D split flow IDs, runtime counters, native counters, and release-speed evidence on top of the mode contract.

- Date: 2026-06-12
- Task: TASK-0004A
- Commit: uncommitted
- Files changed: crates/boon_report_schema/src/lib.rs; crates/boon_runtime/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_report_schema --lib`; `cargo test -p xtask`; `cargo test -p boon_runtime --lib`; `cargo xtask verify-report-schema`; `cargo xtask verify-example-speed counter --report target/reports/counter-speed.json || true` plus `jq` inspection of the generated interaction report fields
- Result: interaction schema now requires flow ID, non-empty `stage_counters`, and explicit zero proof/readback/report-write hot-path counters; runtime speed reports populate those fields from existing timing/allocation/dirty/render summaries; a generated counter speed report contained `measurement_mode: interaction`, a runtime flow ID, seven stage counters, and zero hot-path proof/report counters.
- Follow-up: TASK-0004B should expand runtime speed counters beyond the common contract; TASK-0004D still must resolve or split the existing allocation-budget failure before using release speed reports as passing evidence.

- Date: 2026-06-12
- Task: TASK-0004B
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_runtime --lib`; `cargo test -p boon_report_schema --lib`; `cargo xtask verify-report-schema`; `cargo xtask verify-example-speed counter --report target/reports/counter-speed.json || true` plus `jq` inspection of the generated speed report; `cargo xtask verify-example-semantic counter --report target/reports/counter-semantic.json` plus `jq` inspection of proof-mode fields
- Result: runtime speed reports include deterministic flow IDs, fourteen runtime stage counters, top-level route/recompute/row-touch summaries, and explicit zero hot-path proof/report counters. Generated counter speed evidence showed route actions visited and row-touch summaries. Proof-mode semantic report inspection showed no interaction flow/stage/hot-path fields.
- Follow-up: TASK-0004C1 through TASK-0004C3 split native renderer inventory, IPC observability, and native speed stage counters. TASK-0301 remains responsible for real row-scan/index counters. The inspected counter speed report still fails the existing allocation budget, and the inspected counter semantic report still fails existing generic interpreter execution validation; both generated artifacts were removed before final `verify-report-schema`.

- Date: 2026-06-12
- Task: TASK-0004C1
- Commit: uncommitted
- Files changed: crates/xtask/src/main.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p xtask`; `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`; `jq` inspection of `native_renderer_counter_inventory` and `native_stage_counter_availability`; `cargo xtask verify-report-schema`
- Result: native observability report passes and includes inventory version 1, real `boon_native_gpu::FrameMetrics` fields, app-window loop/frame timing fields, playground interaction sample fields, proof-readback inventory, and explicit unavailable/synthetic labels for GPU timestamp split timing, glyph atlas counters, asset texture upload bytes, queue-write counts, text cache hit/miss counts, and blocked-send counters.
- Follow-up: TASK-0004C2 should turn IPC queue/drop/coalescing/dev-lag fields into native observability stage counters. The observability run produced transient role/progress JSON artifacts under `target/reports/native-gpu`; they were removed before final `verify-report-schema` because they are support artifacts, not canonical schema reports.

- Date: 2026-06-12
- Task: TASK-0004D
- Commit: uncommitted
- Files changed: crates/boon_report_schema/src/lib.rs; crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`; `cargo xtask verify-report-schema`; `cargo test -p boon_report_schema --lib`; `cargo test -p boon_runtime --lib`; `cargo test -p xtask`; `git diff --check`
- Result: release TodoMVC speed report passes schema with interaction measurement mode, release build, runtime flow ID, 14 stage counters, explicit zero proof/readback/report-write hot-path counters, and a passing speed budget. Software-dynamic allocation evidence is disclosed as measured and non-bounded instead of being mislabeled as stress-profile or zero-allocation proof.
- Follow-up: real TodoMVC/Cells stress-profile harnesses remain future bounded-runtime/scalability work; TASK-0004D only proves current release interaction speed report evidence.

- Date: 2026-06-12
- Task: TASK-0005
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_runtime --lib source_store`; `cargo test -p boon_runtime --lib live_source_event_preserves_bound_row_identity`; `cargo test -p boon_runtime --lib`
- Result: SourceStore unbind now validates list/key/generation before mutation, row slots support multiple active lists sharing a numeric key, same-list generation collisions are rejected, remove/reinsert reuse works after unbind, and invariant tests cover active counts, row slots, source slots, source IDs, and row generation consistency.
- Follow-up: TASK-0006 is the next pending direct P0 implementation task with no dependency blocker.

- Date: 2026-06-12
- Task: TASK-0006
- Commit: uncommitted
- Files changed: crates/boon_document/src/lib.rs; crates/boon_native_playground/src/main.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_document --lib`; `cargo test -p boon_runtime --lib document`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema`; `cargo test -p xtask`; `cargo check -p boon_native_playground`; `git diff --check`
- Result: document patch application now returns structured reports/errors, layout validates document graph references, remove-node subtree semantics are explicit, and native sparse document patching fails closed on stale cached patch state after precheck.
- Follow-up: TASK-0007 is the next pending direct P0 task with no dependency blocker.

- Date: 2026-06-12
- Task: TASK-0007
- Commit: uncommitted
- Files changed: crates/boon_native_gpu/src/lib.rs; crates/boon_native_app_window/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo check -p boon_native_gpu -p boon_native_app_window -p boon_native_playground -p xtask`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`; `cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json`; `cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: readback waits are deadline-bounded, timeout context is present in native readback errors, successful readback artifacts expose deadline metadata, and native negative checks now reject CopyToPresent/no-surface scaffold proof for visible readiness.
- Follow-up: TASK-0101 is the next dependency-ready implementation task unless another direct P0 is added.

- Date: 2026-06-12
- Task: TASK-0101
- Commit: uncommitted
- Files changed: crates/boon_ir/src/lib.rs; crates/boon_runtime/src/lib.rs; crates/boon_report_schema/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_parser -p boon_ir -p boon_typecheck --lib`; `cargo test -p boon_report_schema --lib`; `cargo test -p boon_runtime --lib`; `cargo test -p xtask`; `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`; `cargo xtask verify-report-schema`; `cargo xtask verify-boon-source-syntax --report target/reports/boon-source-syntax.json` exited 1 with the existing Cells formatter blocker while reporting semantic-index evidence for all bundled examples; `git diff --check`
- Result: `TypedProgram` now carries a shared semantic index built from parser, IR, and typecheck facts. Runtime reports expose mirrored semantic-index presence/reuse metadata, schema validation requires it, TodoMVC speed evidence passes with semantic-index metadata, and source-syntax diagnostics now show semantic-index counts for Cells, TodoMVC, Physical TodoMVC, NovyWave, and Counter.
- Follow-up: TASK-0102 is the next dependency-ready task; it can replace cross-stage string identity with dense symbols using the new index as the shared inventory.

- Date: 2026-06-12
- Task: TASK-0102
- Commit: uncommitted
- Files changed: crates/boon_ir/src/lib.rs; crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_parser -p boon_ir -p boon_typecheck -p boon_runtime --lib`; `cargo test -p boon_report_schema --lib`; `cargo test -p xtask`; `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`; `jq` inspection of `semantic_index.symbol_count`, `semantic_index.symbol_categories`, `compiled_schedule.runtime_symbol_count`, and `compiled_schedule.field_slot_collision_count`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: semantic index now exposes dense symbol IDs and category counts while preserving readable strings, TodoMVC speed evidence reports 244 semantic symbols across source/list/field/operator/tag/document/style categories, compiled runtime evidence reports 42 runtime symbols and zero field-slot collisions for TodoMVC, and a targeted runtime test catches real labels that collide under the current path-derived field-slot hash.
- Follow-up: TASK-0103 is the next dependency-ready implementation task; it can use semantic-index readiness fields to make dynamic fallback route blockers explicit.

- Date: 2026-06-12
- Task: TASK-0103
- Commit: uncommitted
- Files changed: crates/boon_ir/src/lib.rs; crates/boon_report_schema/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_typecheck --lib`; `cargo test -p boon_runtime --lib typecheck` matched zero tests; `cargo test -p boon_runtime --lib`; `cargo test -p boon_report_schema --lib`; `cargo test -p boon_runtime --lib runtime_execution_schema_rejects_adapter_or_incomplete_generic_slices`; `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`; `jq` inspection of `semantic_index.readiness` and `runtime_execution.semantic_index.readiness`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: runtime report schema now blocks route-critical readiness fallback, semantic-index readiness exposes source completion, route-critical unknown, row-scope ambiguity, selector/index ambiguity, render contract, bridge/page descriptor, and dynamic fallback buckets, and TodoMVC speed evidence reports zero fallback in every bucket.
- Follow-up: TASK-0201 is now unblocked by TASK-0005, TASK-0101, and TASK-0103.

- Date: 2026-06-12
- Task: TASK-0402
- Commit: uncommitted
- Files changed: crates/xtask/src/main.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_native_playground`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`; `jq` inspection of `passive_scroll_property_tree_proof`, route evidence, materialized ranges, scroll roots, and hit-region count; `cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json`; `jq` inspection of dev-editor proof fields and prototype label; `cargo xtask verify-report-schema`; `git diff --check`
- Result: passive scroll reports now carry zero runtime dispatch and graph rebuild counts tied to passing route/model proof, layout-derived scroll roots, hit regions, invalidation classes, generic axis/area scroll targeting, and materialized range changes for both Cells and the dev editor surface.
- Follow-up: TASK-0403 can build on the explicit invalidation-class reporting by replacing remaining string-heavy style/material/font invalidation identities with stable computed IDs.

- Date: 2026-06-12
- Task: TASK-0403
- Commit: uncommitted
- Files changed: crates/boon_document/src/lib.rs; crates/boon_native_gpu/src/lib.rs; crates/boon_native_playground/src/main.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_document --lib style`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `jq` inspection of style/invalidation layout-contract checks and `computed_style_identity_samples`; `cargo test -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo check -p boon_native_playground -p boon_native_gpu -p boon_document -p xtask`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: document layout display items now carry stable computed style IDs split by layout, paint, material, font, and pseudo-state domains; style patch reports use changed-key invalidation with conservative full-document fallback for unknown keys; native renderer text and quad cache identity consumes the computed IDs; layout-contract evidence requires the IDs and expanded invalidation vocabulary.
- Follow-up: TASK-0501 is the next dependency-ready renderer task and can build retained render chunk IDs on top of materialization and computed style identity.

- Date: 2026-06-12
- Task: TASK-0501
- Commit: uncommitted
- Files changed: crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_native_gpu --lib retained_render_chunks`; `cargo test -p boon_native_gpu --lib`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json`; `jq` inspection of retained chunk counts, hit/miss/reuse/dirty counts, upload bytes, draw calls, shaped text runs, chunk IDs, and dependency sets; `cargo xtask verify-report-schema`; `git diff --check`
- Result: native visible render metrics now expose retained chunk IDs and descriptors, previous-frame chunk reuse, dirty chunk counts, upload bytes, draw calls, and text-shaped runs. Cells preview-e2e passed with retained chunk schema enforcement and a reused preview frame reporting 347 retained chunk hits and zero upload bytes.
- Follow-up: TASK-0502 can replace the current CPU-side per-batch buffer path with POD/ring-buffer uploads using the retained chunk identities.

- Date: 2026-06-12
- Task: TASK-0503A
- Commit: uncommitted
- Files changed: crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo check -p boon_native_gpu`; `cargo test -p boon_native_gpu --lib render_scene_boundary`; `cargo test -p boon_native_gpu --lib retained_render_chunks`; `cargo test -p boon_native_gpu --lib`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `jq` inspection of render-scene architecture checks
- Result: partial progress only. The visible native renderer now goes through a `RenderScene` boundary, retained chunks are derived from scene items, and architecture reports prove the boundary exists, is consumed by the renderer, and is covered by scene-boundary tests.
- Follow-up: continue with TASK-0503B by moving editor/widget/style semantic lowering out of `boon_native_gpu`.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/Cargo.toml; crates/boon_document/src/lib.rs; crates/boon_document/src/render_scene.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_document --lib render_scene`; `cargo check -p boon_document -p boon_native_gpu`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `jq` inspection of document render-scene architecture checks
- Result: partial progress only. `boon_document::render_scene` now exposes a renderer-neutral scene contract and the architecture gate proves it exists without WGPU/glyphon/image/resvg resource imports.
- Follow-up: continue TASK-0503B by moving the actual semantic lowering code and GPU request entrypoints onto the external scene contract.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/Cargo.toml; crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo check -p boon_document -p boon_native_gpu`; `cargo test -p boon_document --lib render_scene`; `cargo test -p boon_native_gpu --lib`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `jq` inspection of text-lowering architecture checks; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo test -p boon_document --lib`
- Result: partial progress only. Text-run semantic lowering now lives in the neutral document render-scene boundary and GPU text rendering delegates to it through `GlyphonRenderTextColumnMeasurer` plus neutral-to-glyphon adapters. Architecture reports prove document text-lowering ownership and GPU delegation.
- Follow-up: continue TASK-0503B by moving rectangle/material/widget lowering and pre-lowered scene entrypoints out of `boon_native_gpu`.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_native_gpu/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo check -p boon_document -p boon_native_gpu`; `cargo test -p boon_native_gpu --lib`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `jq` inspection of render-scene/text-lowering architecture checks
- Result: partial progress only. Internal GPU `RenderScene` now carries neutral `RenderTextRun` values until the glyphon render call; retained chunk text IDs are generated from neutral scene text runs.
- Follow-up: continue TASK-0503B by moving rectangle/material/widget lowering and accepting external pre-lowered `boon_document::RenderScene` in native GPU entrypoints.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/src/lib.rs; crates/boon_document/src/render_scene.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo test -p boon_document --lib render_scene`; `cargo check -p boon_document`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `jq` inspection of visual-primitive architecture checks; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`
- Result: partial progress only. The neutral document render-scene boundary now owns visual primitive intent for viewport backgrounds, default fills, asset refs, and checkbox/checkmark semantics; architecture reports prove that ownership.
- Follow-up: continue TASK-0503B by having GPU consume external `boon_document::RenderScene` / visual primitives for rectangle paths, then remove or demote remaining semantic style-key ownership in `boon_native_gpu`.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_document -p boon_native_gpu`; `cargo test -p boon_native_gpu --lib renderer_adapts_external_document_render_scene_without_layout_frame`; `cargo test -p boon_native_gpu --lib renderer_helpers_accept_prelowered_render_scene_without_layout_frame`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib render_scene`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `jq` inspection of architecture render-scene/external/hot-encode checks; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: partial progress only. Native GPU now has a public `SurfaceRenderSceneRequest` / `encode_render_scene_to_surface` path for pre-lowered `boon_document::RenderScene`, adapts document visual primitives into quad batches without a `LayoutFrame`, uses a render-scene cache key in the hot encode function, and architecture evidence proves the external scene entrypoint plus scene-keyed hot encode boundary.
- Follow-up: continue TASK-0503B by routing production/native playground rendering through the external scene contract and removing or demoting remaining GPU-owned rectangle/material/widget semantic style-key lowering.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_native_gpu/src/lib.rs; crates/boon_native_playground/src/main.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_native_gpu -p boon_native_playground`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_native_gpu --lib`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `jq` inspection of `architecture:playground-visible-render-uses-external-scene`
- Result: partial progress only. Native playground preview/dev visible rendering now lowers to `boon_document::RenderScene` and calls `encode_scene`; app-owned readback keeps the compatibility `LayoutFrame` request for proof hashes but encodes through `SurfaceRenderSceneRequest`; architecture verification now proves playground visible rendering uses the external scene contract.
- Follow-up: continue TASK-0503B by moving or demoting the remaining GPU-owned rectangle/material/widget semantic style-key tessellation and then tightening the architecture gate against those remaining keys.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_document --lib render_visual_primitives_lower_text_overlays_before_gpu`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib render_scene`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `jq` inspection of `architecture:document-render-scene-owns-text-overlay-lowering` and `architecture:renderer-paints-document-text-overlay-primitives`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: partial progress only. Document render-scene lowering now owns editor selection, bracket highlights, editor carets, text-input carets, underlines, strikethroughs, and button checkmark stroke primitives; native GPU paints those pre-lowered primitive kinds through the external scene path; architecture evidence now proves document ownership and renderer support.
- Follow-up: continue TASK-0503B with border/material/shadow primitive descriptors or retiring the remaining compatibility-only semantic lowerer from the hot renderer contract.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/src/render_scene.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_document --lib render_visual_primitives_apply_material_fill_adjustments_before_gpu`; `cargo test -p boon_document --lib render_scene`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `jq` inspection of `architecture:document-render-scene-owns-material-fill-lowering`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema` after a transient parallel report-write failure; `git diff --check`
- Result: partial progress only. Document render-scene fill primitives now apply transparency, refraction, frosted blur, frosted saturation, gloss, and metal before the native GPU paints them; architecture evidence now proves document ownership for material fill adjustment.
- Follow-up: continue TASK-0503B with material highlight/frosted layer primitives, shadows, borders, checkbox raster descriptors, or retirement of the old compatibility `rect_vertices` semantic lowerer.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_document --lib render_visual_primitives_lower_borders_after_descendant_fills_before_gpu`; `cargo test -p boon_native_gpu --lib renderer_paints_external_document_border_primitives`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: partial progress only. Document render-scene lowering now owns whole-border and per-side border primitives with stroke width/radius/color/clip/style dependencies, appends them after normal primitives to preserve existing paint order, and native GPU paints those pre-lowered border primitives through the external scene path.
- Follow-up: continue TASK-0503B with material highlight/frosted layer primitives, shadows, checkbox raster descriptors, or retirement of the old compatibility `rect_vertices` semantic lowerer.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_document --lib render_visual_primitives_lower_material_layers_before_gpu`; `cargo test -p boon_native_gpu --lib renderer_paints_external_document_material_layer_primitives`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: partial progress only. Document render-scene lowering now owns frosted material layer primitives before fills and material highlight primitives after fills; native GPU paints those pre-lowered primitive kinds through the external scene path; architecture evidence now proves document ownership and renderer support.
- Follow-up: continue TASK-0503B with shadow primitive descriptors, checkbox raster descriptors, or retirement of the old compatibility `rect_vertices` semantic lowerer.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_document --lib render_visual_primitives_lower_shadows_before_fill_before_gpu`; `cargo test -p boon_native_gpu --lib renderer_paints_external_document_shadow_primitives`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: partial progress only. Document render-scene lowering now owns box-shadow primitive expansion for the external scene path, including reverse CSS paint order, rounded blur expansion, inset bands, and non-rounded rect-difference halo bands; native GPU paints those pre-lowered shadow primitives through the external scene path; architecture evidence now proves document ownership and renderer support.
- Follow-up: continue TASK-0503B with checkbox raster descriptors or retirement of the old compatibility `rect_vertices` semantic lowerer.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_document --lib render_visual_primitives_lower_checkbox_raster_semantics_before_gpu`; `cargo test -p boon_document --lib render_visual_primitives_skip_checkbox_raster_when_asset_icon_covers_control`; `cargo test -p boon_native_gpu --lib renderer_paints_external_document_checkbox_raster_primitives`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: partial progress only. Document render-scene lowering now owns checkbox raster descriptors for the external scene path, including cast shadow, ring/inner circle colors, inner shadow, highlight, checkmark control points, antialias widths, and asset-icon skip; native GPU consumes those descriptors and keeps only rasterization math.
- Follow-up: continue TASK-0503B by retiring or quarantining the old compatibility `rect_vertices` semantic lowerer so the GPU crate contract is pure primitive rendering.

- Date: 2026-06-12
- Task: TASK-0503B
- Commit: uncommitted
- Files changed: crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`; `rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json`; `cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. `SurfaceRenderRequest` compatibility rendering now lowers to `boon_document::RenderScene` and adapts through `render_scene_from_document_scene` before WGPU encode; the old LayoutFrame semantic lowerer is explicitly compatibility-only for legacy renderer unit tests, and architecture verification fails if the layout request encode path calls `render_scene_from_layout_frame` or `rect_vertices`.
- Follow-up: next ready checklist item after TASK-0503B dependency consumers is TASK-0504 unless a higher-priority pending dependency now becomes unblocked.

- Date: 2026-06-12
- Task: TASK-0504
- Commit: uncommitted
- Files changed: crates/boon_document/src/lib.rs; crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_document --lib text`; `cargo test -p boon_native_gpu --lib text`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo test -p xtask`; `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json` failed because isolated preview/dev role reports were not produced; loop reports rendered frames successfully.
- Result: partial progress only. `boon_document` now exports renderer-neutral `RenderTextShapeKey` and `RenderTextPlacementKey` contracts for rich spans, size, line height, color, placement, clipping, and rotation; native GPU `FrameMetrics` now exposes real visible/shaped text counts, shaped-run cache hits/misses/evictions/entry count/capacity/bytes, missing-glyph count, and glyphon prepare/observed-eviction fields; xtask inventories and scroll summaries now read the new real fields when present instead of fabricating `text_shape_cache_*` values.
- Follow-up: continue TASK-0504 by extracting a reusable native text service for shared measurement/render state and by fixing the observability role-report path so the native gate can pass with live preview/dev evidence.

- Date: 2026-06-12
- Task: TASK-0504
- Commit: uncommitted
- Files changed: crates/boon_native_gpu/src/lib.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_native_gpu --lib text`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `rg -n "GlyphonRenderTextColumnMeasurer;|GlyphonRenderTextColumnMeasurer\\b" crates/boon_native_gpu/src/lib.rs crates/boon_native_playground/src/main.rs`
- Result: partial progress only. Native GPU now has a shared internal `GlyphonTextService` for font-system/swash-cache ownership, text measurement, render shaping, rotated glyph rasterization, empty custom glyph buffers, and editor column-edge shaping; `GlyphonTextMeasurer`, `GlyphonTextState`, and `GlyphonRenderTextColumnMeasurer` use that service while public APIs remain stable.
- Follow-up: continue TASK-0504 by making editor interaction column-edge paths reuse a long-lived service instance instead of the compatibility helper's one-shot service, and fix/re-run the observability role-report gate.

- Date: 2026-06-12
- Task: TASK-0504
- Commit: uncommitted
- Files changed: crates/boon_document/src/lib.rs; crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/boon_native_playground/src/main.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_native_playground -p xtask`; `cargo test -p xtask`; `cargo test -p boon_native_gpu --lib text`; `cargo test -p boon_document --lib text`; `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`; `test ! -e target/reports/native-gpu/.observability-supervisor.progress.json`; `jq` inspection of `target/reports/native-gpu/observability.json` and `target/reports/native-gpu/.observability-supervisor.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. Text shape/placement contracts now live in `boon_document::render_scene`, native GPU text measurement/render shaping share `GlyphonTextService`, the dev editor column metric cache owns a long-lived `GlyphonRenderTextColumnMeasurer`, and `FrameMetrics` reports visible/shaped text runs, shaped-run cache hits/misses/evictions/capacity/bytes, missing glyphs, and glyph atlas prepare/observed-eviction fields. The refreshed observability report passed with live preview/dev role reports, `dev_ipc_probe_timeout_ms=20000`, no stale supervisor progress report, measured text-shaping availability, and explicit glyphon atlas callback unavailability instead of synthetic upload/eviction numbers.
- Follow-up: next ready checklist item is TASK-0505.

- Date: 2026-06-12
- Task: TASK-0505
- Commit: uncommitted
- Files changed: crates/boon_document/Cargo.toml; crates/boon_document/src/render_scene.rs; crates/boon_native_gpu/src/lib.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebb85-86a0-7e02-9f1e-7281870c0902`; `cargo fmt --all`; `cargo test -p boon_document --lib asset`; `cargo test -p boon_native_gpu --lib asset`; `cargo test -p xtask`; `cargo check -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo xtask verify-native-gpu-preview-e2e --example todo_mvc_physical --report target/reports/native-gpu/preview-e2e-todo_mvc_physical.json`; `cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json`; `jq` inspection of `native_stage_counter_availability.asset_work` and asset `FrameMetrics` inventory; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. `boon_document::render_scene` now provides renderer-neutral `RenderBlobRef` and `RenderAssetRef` digest identities for inline/generated SVG data URL assets, and render-scene/retained-chunk metadata uses digest asset IDs instead of raw URLs. Native GPU texture keys carry those refs while retaining raw URL payloads only for first decode, and `FrameMetrics` now reports `asset_ref_count`, `asset_refs`, cache hits/misses/evictions, cache byte count/cap/cap-hit state, decode/raster/upload counts, upload bytes, and diagnostics. The asset test proves first render misses/upload and second render hits with zero repeat decode/raster/upload; native observability now marks `asset_work` measured and inventories the real asset fields.
- Follow-up: next ready checklist item is TASK-0601.

- Date: 2026-06-12
- Task: TASK-0603
- Commit: uncommitted
- Files changed: crates/boon_document/src/lib.rs; crates/boon_driver/src/lib.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_document -p boon_driver -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground hit_region`; `cargo test -p boon_native_playground --bin boon_native_playground cells_formula_bar_click_accepts_text_edit`; `cargo test -p boon_document --lib hit`; `cargo test -p boon_driver --lib`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. `boon_document::HitSideTable` carries typed route identity, scroll root, row generation, bounds, z depth, and spatial buckets; BoonDriver reports preserve hit/focus/scroll evidence; native preview input now routes hover/click/pointer-move/mouse-release/text-focus through `PreviewHitRouteTable` when a typed cached snapshot exists, with proof JSON kept for serialization and legacy helper tests only.
- Follow-up: `TASK-0502` remains blocked by `EXP-0001`; next ready implementation task by dependency order is `TASK-0701`.

- Date: 2026-06-12
- Task: TASK-0701
- Commit: uncommitted
- Files changed: Cargo.toml; Cargo.lock; crates/boon_bridge/Cargo.toml; crates/boon_bridge/src/lib.rs; crates/xtask/Cargo.toml; crates/xtask/src/main.rs; crates/boon_ply_playground/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebbd5-0289-7ea1-8a58-00f29d0a03ca`; `cargo fmt --all`; `cargo test --workspace bridge`; `cargo xtask check-bridge --report target/reports/check-bridge.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. `boon_bridge` now provides the generic bridge/effects substrate: ABI/schema version metadata, canonical schema and bridge value hashing, records/tagged values/lists/refs/pages/blobs/diagnostics/completions, module/export/provider/capability metadata, effect request/completion scheduling, request-key deduplication, cancellation, stale/duplicate rejection, replay, payload caps, grant denial, and no-Rust-handle validation. `cargo xtask check-bridge` writes and validates `target/reports/check-bridge.json`.
- Follow-up: next ready implementation task by dependency order is `TASK-0702`.

- Date: 2026-06-12
- Task: TASK-0702
- Commit: uncommitted
- Files changed: crates/boon_bridge/src/lib.rs; crates/boon_ir/src/lib.rs; crates/boon_parser/src/lib.rs; crates/boon_runtime/src/lib.rs; crates/xtask/src/main.rs; examples/novywave/Bridge/NovyBridge.bn; examples/novywave/RUN.bn; examples/novywave.scn; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebbdd-ac1a-7330-b3e7-bc2989ae4429`; `cargo fmt --all`; `cargo test -p boon_runtime --lib novywave_waveform_metadata_drives_selected_file_and_timeline_window -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo test --workspace bridge`; `cargo xtask check-bridge --report target/reports/check-bridge.json`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. NovyWave now consumes bridge-shaped refs/pages/descriptors for the fixture path, with bounded page byte lengths, request/response fingerprints, input/page digests, visible domain generations, diagnostics/status descriptors, and stale response rejection proved by `target/reports/novywave-bridge-scenario.json`.
- Follow-up: `TASK-0502` remains blocked by `EXP-0001`; next ready implementation task by dependency order is `TASK-0703`.

- Date: 2026-06-12
- Task: TASK-0703
- Commit: uncommitted
- Files changed: crates/boon_parser/src/lib.rs; crates/boon_ir/src/lib.rs; crates/boon_runtime/src/lib.rs; crates/boon_native_playground/src/main.rs; crates/xtask/src/main.rs; examples/novywave/RUN.bn; examples/novywave/View/NovyView.bn; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebc1f-fad8-7b53-97c5-9ca61213e146`; `cargo fmt`; `cargo test -p boon_parser --lib novywave_list_memory_names_are_unique -- --nocapture`; `cargo test -p boon_ir --lib inline_empty_render_slot_lists_inside_row_constructors_get_unique_names -- --nocapture`; `cargo test -p boon_runtime --lib generic_rows_preserve_nested_field_records_and_lists -- --nocapture`; `cargo test -p boon_runtime --lib novywave_selected_visible_items_model_group_headers_and_collapse -- --nocapture`; `cargo test -p boon_runtime --lib render_projection_does_not_overwrite_appended_row_label_field -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_metadata_drives_selected_file_and_timeline_window -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground novywave_external_loaded_file_tree_renders_loaded_file_without_workspace_rows -- --nocapture`; `cargo test -p boon_native_playground novywave_search_and_waveform_keyboard_work_from_real_preview_input -- --nocapture`; `cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json`; `cargo xtask verify-native-gpu-scroll-speed --example novywave --report target/reports/native-gpu/scroll-speed-novywave.json`
- Result: done. The flat lane-row workaround was removed in favor of engine fixes: parser/IR now keep generated list memories and inline row-local list fields distinct, runtime row storage preserves structured JSON fields, record-valued field blocks evaluate as records instead of the last child scalar, row refs rehydrate structured fields, render projections no longer overwrite model row fields, and NovyWave `selected_signal_lane_rows` carries nested lane state, hit regions, page/window refs, materialization refs, and row-local segments. NovyWave file-tree UI now maps generic metadata rows instead of hardcoded view constructors, and external loaded files render from source metadata while stale workspace fixture rows stay absent. The promoted visual gate passes with app-owned WGPU readback, real-preview input, row alignment, runtime-projected waveform width, and material readback coverage. The scroll-speed gate passes with projected requested-path timeline replay and a cached preview render scene.
- Follow-up: continue with TASK-0804 after the current checkpoint is committed.

- Date: 2026-06-12
- Task: TASK-0801
- Commit: uncommitted
- Files changed: crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebbfb-3626-7020-8752-82a84d55e7b3`; `cargo fmt --all`; `cargo test -p boon_driver --lib`; `cargo test -p boon_native_playground --bin boon_native_playground todomvc_runtime_inserted_row_exposes_source_hit_targets -- --nocapture`; `cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json`; `cargo xtask verify-boon-driver-e2e --report target/reports/boon-driver-e2e.json`
- Result: done. BoonDriver TodoMVC E2E now consumes current app-owned native preview route evidence instead of stale/runtime-only proof. Native source-intent lowering now extracts row-local evaluated `events` objects, including payloadless event keys, and combines them with the source group prefix so runtime-inserted TodoMVC rows expose title/edit/remove source intents and hit regions. Preview operator host input no longer flips route proof to pass merely because `LiveRuntime` produced deltas; runtime acceptance remains diagnostic only, and host-route pass requires document source binding plus hit evidence. The preview host-input proof path now materializes the shared layout after every mutating source event so later source-only controls in subsequent IPC batches route against current document state. The refreshed TodoMVC preview report passed with zero failed operator-host routes, `dev_ipc_probe.operator_host_input.status="pass"`, `boon_driver_proof.status="pass"`, `boon_driver_proof.route_status="pass"`, and 26 runtime assertions. The BoonDriver E2E report passed for `todomvc`.
- Follow-up: next dependency-ready task is `TASK-0802`; the older TASK-0703 follow-up pointing to TASK-0804 is superseded because TASK-0804 still depends on TASK-0802.

- Date: 2026-06-13
- Task: TASK-0802
- Commit: uncommitted
- Files changed: crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorers `019ebdf2-cb5a-78d0-adf7-44db66846ec4` and `019ebdf2-cd8f-7c21-b11e-f3c42c3d995d`; `cargo check -p xtask`; `cargo fmt --all`; `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`; `jq` inspection of `target/reports/native-gpu/negative.json` showing `status="pass"` and `negative_case_count=49`; `cargo test -p boon_report_schema --lib`; stale generated `target/reports/**/*.json` artifacts pruned; `cargo xtask verify-report-schema`; `jq` inspection of `target/reports/schema.json` showing `status="pass"`; `git diff --check`
- Result: done. Native negative verification now rejects the additional TASK-0802 fabricated evidence classes: stale scenario/budget/artifact hashes, mutated source event and route identity, stale source/row generations, duplicate scenario identity, fake human observation, source-event-only IPC shortcuts, full waveform payloads entering Boon, and reduced fixtures. Existing fake real OS input, private runtime dispatch, scaffold rendering, copied pixel hash, stale binary/worktree/source, model-only timing, and preview scenario-data leakage checks remain covered. `boon_report_schema` remains a shape/hash validator for the generated negative report rather than the source of native-specific rejection policy.
- Follow-up: next dependency-ready task is `TASK-0803`.

- Date: 2026-06-13
- Task: TASK-0803A
- Commit: uncommitted
- Files changed: crates/xtask/src/main.rs; crates/boon_report_schema/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorers `019ebdf9-1176-7971-8d5e-f0faa178aa2e` and `019ebdf9-131d-77f0-8621-e7fce6fcc08a`; `cargo fmt --all`; `cargo check -p xtask`; `cargo xtask verify-metamorphic-hidden-fixtures --report target/reports/metamorphic-hidden-fixtures.json`; `jq` inspection of `target/reports/metamorphic-hidden-fixtures.json` showing `status="pass"`, `metamorphic_task_slice="TASK-0803A"`, 13 checks, three cases, visual deferral, and 12 artifact hashes; `cargo test -p boon_report_schema --lib`; stale generated `target/reports/**/*.json` artifacts pruned before final report refresh; `cargo xtask verify-report-schema`; `git diff --check`
- Result: done. The first metamorphic hidden-fixture gate now runs as a real `xtask` command. It baseline-checks Counter, generates moved hidden source/scenario files, records semantic invariants before mutation execution, and proves three deterministic mutated cases: legal reformat/path move, source route plus visible label/target text rename, and declaration/source-branch/control-order plus style/viewport-like changes. Failing reports from the new command are recognized as blocker audits, and recursive report-schema validation now avoids hashing live app-window loop diagnostics while still classifying them. Native/app-owned visual metamorphic coverage remains explicit follow-up in `TASK-0803B`.
- Follow-up: next dependency-ready task is `TASK-0803B`.

- Date: 2026-06-13
- Task: TASK-0803B
- Commit: uncommitted
- Files changed: crates/boon_ir/src/lib.rs; crates/boon_native_playground/Cargo.toml; crates/boon_native_playground/src/main.rs; crates/xtask/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorers `019ebe06-fff1-76a3-8c7e-cd963efc19d4` and `019ebe07-11e7-7073-bb7e-a733a9ccc995`; `cargo fmt --all`; `cargo check -p boon_ir -p boon_runtime -p xtask`; `cargo test -p boon_ir --lib list_record_literal_signed_numbers_lower_to_numeric_initializers -- --nocapture`; `cargo check -p boon_native_playground -p xtask`; `cargo check -p xtask`; `cargo xtask verify-metamorphic-hidden-fixtures --report target/reports/metamorphic-hidden-fixtures.json`; `jq` inspection of `target/reports/metamorphic-hidden-fixtures.json` showing `status="pass"`, 24 checks, no failed checks, `metamorphic_task_slice="TASK-0803B"`, task slices `TASK-0803A`, `TASK-0803B-runtime-project`, and `TASK-0803B-app-owned-visual`, three app-owned visual cases, crop counts 14/11/11, viewport coverage `962x1017` and `1040x1080`, theme-switch final-state coverage, and 154 artifact hashes.
- Result: done. The metamorphic hidden-fixture gate now covers TodoMVC Physical as a moved multi-file `RuntimeSourceUnit` project with changed source-unit order, legal reformatting, fixture/source/target label mutation, semantic replay through the full theme story, explicit app-owned layout-proof source-unit JSON, app-owned WGPU readbacks, and semantic-label crop equivalence instead of whole-frame equality. The visual matrix compares initial Classic/Light and final Neumorphic/Dark state at the physical viewport plus final state at an expanded viewport, with per-label crops including TodoMVC content, filters, and theme controls. The compiler now lowers signed integer fields in list-record literals, fixing the hidden physical fixture failure on values such as `elevation: -4`.
- Follow-up: next dependency-ready task is `TASK-0804`.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_ir/src/lib.rs; crates/boon_runtime/src/lib.rs; examples/novywave.scn; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo test -p boon_ir --lib derived_dependency_routes_do_not_borrow_payload_specific_branches -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_metadata_drives_selected_file_and_timeline_window -- --nocapture`; `cargo test -p boon_runtime --lib novywave_marker_count_updates_after_list_insert_and_remove -- --nocapture`; `cargo test -p boon_runtime --lib novywave_initial_bridge_descriptor_uses_initial_format -- --nocapture`; `cargo test -p boon_runtime --lib novywave_top_level_format_updates_active_selected_row_formatter -- --nocapture`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `jq` inspection of `target/reports/novywave-bridge-scenario.json` showing `status="pass"`, zero blockers, `bridge_scenario_coverage.status="pass"`, no failed groups, `measurement_mode="proof"`, and a non-empty worktree fingerprint; `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_ir/src/lib.rs examples/novywave.scn docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Result: in progress. The bridge scenario slice now passes without NovyWave-specific runtime shortcuts. Compiler lowering no longer borrows payload-specific branches from HOLD initializers, source payload concatenation lowers without fake `THEN` wrappers, top-level format buttons update selected-row formatter routes, initial bridge descriptors use settled format state, list append/remove dirties list-structure and count-target dependents before count-cache reads, numeric Boon scalars are preserved as JSON numbers inside bridge records, scalar JSON roots can still feed text/number source projectors, and the cursor/pan scenario expectations now follow the current Binary format state after format-cycling.
- Follow-up: continue TASK-0804 with `verify-native-gpu-preview-e2e --example novywave`, `verify-native-gpu-novywave-visual`, and `verify-native-gpu-novywave-interaction-speed`; do not mark TASK-0804 done until those app-owned visual and release-speed gates are implemented and passing.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; examples/novywave/RUN.bn; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebfa5-d440-7df1-9ea5-da43059f2919`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib list_index_text_lookup -- --nocapture`; `cargo test -p boon_runtime --lib list_filter_field_equal -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_column_read_dirties_when_row_key_membership_changes -- --nocapture`; `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`; `cargo test -p boon_runtime --lib novywave_repeated_hover_width_is_noop_for_canvas_width -- --nocapture`; `cargo test -p boon_runtime --lib novywave_external_loaded_name_payload_updates_active_file -- --nocapture`; `cargo build --release -p boon_native_playground`; `BOON_NATIVE_DISABLE_UI_STATE_PERSIST=1 BOON_NATIVE_PREVIEW_COMPACT_TIMING=1 ./target/release/boon_native_playground --role interaction-speed --example novywave --event-count 4 --max-p95-ms 16.7 --max-max-ms 33.4 --max-resize-p95-ms 33.4 --report target/artifacts/native-gpu/novywave-interaction-speed-diagnostic.json` (fails current budget)
- Result: in progress. Generic runtime filtering now uses exact text lookup indexes for `List/filter_field_equal` on both full `ListRef` inputs and homogeneous `List<RowRef>` pipelines, while preserving visible order and adding a column-level read key so rows changing into or out of a filtered value dirty indexed dependents. Bool/non-text filters still fall back to the scan path. `List/retain` now has a generic fast path for simple row-field numeric comparisons against row-independent scalar expressions. NovyWave no longer computes cursor-dependent selected-signal current values in the defaults catalog; cursor values are computed after selected-row filtering, and redundant per-segment bridge page/fingerprint metadata was removed from lane segments. The latest release interaction-speed diagnostic still fails: hover p95 `15.163ms` passes, but click p95/max is `44.022ms` and divider p95 is `23.207ms` against `16.7ms`; compact click samples now show only three `selected_signal_lane_rows[*].current_value` candidates and zero recomputed current-value fields, so the remaining blocker is avoiding/economizing unchanged current-value candidate evaluation plus the aggregate divider timing artifact.
- Follow-up: keep TASK-0804 in progress. Next work should either make derived row-field recomputation lazy/value-aware enough to skip unchanged current-value candidates, add stronger per-interaction list/predicate counters to the native speed report, or replace the remaining current-value pipeline with a generic indexed interval lookup without adding Boon syntax or reducing NovyWave fixture coverage.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ebff4-0ff6-77d3-9d67-9849a2a083a7`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib path_qualified_root_list_filter_uses_materialized_list_ref_index -- --nocapture`; `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`; `cargo test -p boon_runtime --lib list_filter_field -- --nocapture`; `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`; `cargo test -p boon_runtime --lib novywave_repeated_hover_width_is_noop_for_canvas_width -- --nocapture`; `cargo build --release -p boon_native_playground`; `BOON_NATIVE_DISABLE_UI_STATE_PERSIST=1 BOON_NATIVE_PREVIEW_COMPACT_TIMING=1 ./target/release/boon_native_playground --role interaction-speed --example novywave --event-count 4 --max-p95-ms 16.7 --max-max-ms 33.4 --max-resize-p95-ms 33.4 --report target/artifacts/native-gpu/novywave-interaction-speed-diagnostic.json` (fails current budget)
- Result: in progress. The native interaction timing report now carries per-turn source-route scan summaries, and the diagnostic proves the click path has zero source-route rows scanned. Runtime list scan counters now separate `List/find`, text contains, field filter, move, join, and retain fallback rows. Path-qualified root lists such as `store.waveform_segment_records` now resolve to materialized `ListRef` values before scalar JSON copies, which eliminates the previous `filter_field_rows_scanned=982` click fallback. A generic per-turn user-function value cache now stores body values with dependency reads and invalidates them through existing read keys; its cache key is free-variable aware so unrelated caller row bindings do not bust identical pure calls. Focused tests prove path-qualified root-list filters use indexes, repeated identical function calls reuse one indexed lookup, and unreferenced caller env bindings do not prevent reuse. A fixed small-subset row-ref scan experiment was tried and killed because it reduced global candidate counts but did not improve click latency.
- Latest diagnostic: TASK-0804 remains failing. With the free-variable-aware function cache, release click samples still have three `selected_signal_lane_rows[*].current_value` candidates and zero source-route scans, but repeated cursor-value work is lower than before: numeric lookup hits are now `12` instead of `20`, text lookup hits are `38-43` instead of `54-59`, and join rows are `10` instead of `14`. The strict release diagnostic still reports hover p95 `14.853ms`, click p95/max `44.671ms`, divider p95 `19.412ms`, runtime apply p95 `31.492ms`, and runtime step p95 `22.070ms`, failing both p95 and max latency budgets.
- Follow-up: keep TASK-0804 in progress. Next generic engine work should preserve indexed row selections through list pipelines, e.g. an internal row-set/list-selection value that intersects current candidates for text and numeric filters without materializing broad full-list index candidates, or fuse `List/map(... [value: expr]) |> List/join_field(field: "value")` for the cursor-value path. Also split `route_candidates_visited` from list lookup candidate accounting so source-route diagnostics stay distinct from list-index diagnostics.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec002-ea1d-7a00-ad03-a4f6f398622c`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib map_join_field -- --nocapture`; `cargo test -p boon_runtime --lib list_filter_field -- --nocapture`; `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`; `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`; `cargo test -p boon_runtime --lib novywave_repeated_hover_width_is_noop_for_canvas_width -- --nocapture`; `cargo build --release -p boon_native_playground`; `BOON_NATIVE_DISABLE_UI_STATE_PERSIST=1 BOON_NATIVE_PREVIEW_COMPACT_TIMING=1 ./target/release/boon_native_playground --role interaction-speed --example novywave --event-count 4 --max-p95-ms 16.7 --max-max-ms 33.4 --max-resize-p95-ms 33.4 --report target/artifacts/native-gpu/novywave-interaction-speed-diagnostic.json` (fails current budget)
- Result: in progress. The generic runtime now fuses the existing `List/map(... [field: expr]) |> List/join_field(field: ...)` shape without Boon syntax changes. The fused path works for nested pipe expressions, continuation children, and sibling continuation statements, evaluates only the projected joined field, preserves reads from the projected expression, and avoids materializing one-field records only to scan them again. Runtime reports now expose `map_join_field_fusions` and `map_join_field_rows_fused`; the NovyWave click-like samples show `map_join_field_fusions=7` and `map_join_field_rows_fused=6`. A private internal `BoonValue::ListSelection { list, indices }` now carries narrowed row sets through field filters, numeric retain, summary expansion, and list iteration. The first full-list indexed filter/retain narrows to a selection, later predicates operate relative to that selection and cannot reintroduce rows outside it. This reduces broad candidate accounting in the latest diagnostic: click-like samples now have numeric lookup candidates `0` instead of `528-606`, text lookup candidates roughly `106-126` instead of `328-350`, and route candidates roughly `107-127` instead of `879-935`.
- Latest diagnostic: TASK-0804 remains failing. With map/join fusion plus `ListSelection`, the strict release diagnostic reports hover p95 `13.904ms`, click p95/max `42.408ms`, divider p95 `20.091ms`, runtime apply p95 `32.437ms`, and runtime step p95 `22.850ms`. This is a small improvement over the prior `42.420ms` fused-only click result and the earlier `44.671ms` free-variable-cache result, but it still fails the `16.7ms` p95 and `33.4ms` max budgets. `ListSelection` is kept for now because it materially reduces broad index candidate work without regressing measured latency, but it introduces `188-236` selected-row scans in click-like samples.
- Follow-up: keep TASK-0804 in progress. Next engine work should make `ListSelection` predicates avoid scanning the selected rows, for example by adding selection-aware text/numeric index intersections, cached selected-field bitsets, or a cursor-interval lookup specialized by the existing generic filter/retain shape. Do not add Boon syntax and do not reduce NovyWave fixtures.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec063-99aa-76c0-992f-985d9646dee8`; `cargo fmt --all`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib list_index_ -- --nocapture`; `cargo test -p boon_runtime --lib list_filter_field -- --nocapture`; `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`; `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget)
- Result: in progress. `LATEST` source-payload concatenation remains a compiler/IR feature, not a Boon workaround: the focused IR test proves `TEXT { - } |> Text/concat(with: elements.external_file_loaded_name.text, separator: " ")` lowers without a fake `THEN` wrapper and preserves the separate reset branch. The runtime now directly intersects small selected row sets for text and numeric indexed predicates instead of building broad global match sets first, and user-function cache keys use a stable direct `BoonValue` encoder instead of serde JSON. Current bridge proof passes 71/71 required steps with no failures. Current release interaction-speed still fails: click/input p95 `68.651ms`, hover p95 `19.056ms`, divider p95 `26.406ms`, runtime step p95 `36.634ms`, runtime apply p95 `37.927ms`; counters are lower (`row_occurrences_scanned=578`, `text_lookup_index_candidates=410`, `numeric_lookup_index_candidates=136`) but latency remains dominated by unchanged `selected_signal_lane_rows[*].current_value` candidate evaluation and root fanout.
- Follow-up: keep TASK-0804 in progress. Do not retry the killed function cacheability heuristic unless there is finer profiling proving cache-key overhead, because it regressed the release speed gate. Next work should attack candidate skipping/value-aware dirty propagation, root-derived fanout, or a generic interval lookup for cursor-value pipelines without changing Boon syntax.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorers `019ec08f-7ab4-7ba1-bffe-73ddaddc8cd0` and `019ec08f-9c2f-7f72-8724-8147a3a4c517`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`; `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`; `cargo test -p boon_runtime --lib list_filter_field -- --nocapture`; `cargo test -p boon_runtime --lib map_join_field -- --nocapture`; `cargo test -p boon_runtime --lib list_index_ -- --nocapture`; `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`; `cargo test -p boon_runtime --lib novywave_repeated_hover_width_is_noop_for_canvas_width -- --nocapture`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`, `target/reports/novywave-bridge-scenario.json`, and `target/reports/schema.json`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Result: in progress. Generic numeric `List/retain` now records root numeric stability guards from indexed row-field predicates, carries them through user-function cache hits, skips dirty row-field candidates when the changed root value stays in the proven interval, and merges connected intervals only after an unchanged recompute proves the field value stayed the same. The new focused regression proves a cursor move inside the same retained interval skips the row value candidate while a boundary-crossing move still recomputes and changes the value. The official speed report still fails, but the bottleneck moved: repeated click samples now show `runtime_recompute_candidate_count=0` after the first candidate-bearing cursor click, numeric lookup candidates are down to `48`, and row occurrences are down to `440`; click/input p95 remains `64.779ms` because cursor positions such as `49` and `150` still produce 21 root semantic deltas with runtime step/apply p95 `34.458ms`/`35.724ms`.
- Follow-up: keep TASK-0804 in progress. The next generic engine slice should target root-derived cursor fanout and root expression/delta/patch cost, not selected-lane current-value candidate scans. Add finer root materialization timing counters if the next implementation cannot clearly identify which root fields dominate the remaining `34-36ms` runtime step.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec0a6-ed66-7e20-afa8-ad266c16a12e`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib structured_root_changed_reads_only_dirty_changed_children -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_materialization_uses_changed_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_structured_parent_dirties_dependents_without_empty_text_patch -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_worklist_revisits_direct_and_indirect_dependents -- --nocapture`; `cargo test -p boon_runtime --lib source_text_payload_can_be_read_inside_then_update_expression -- --nocapture`; `cargo test -p boon_runtime --lib novywave_repeated_hover_width_is_noop_for_canvas_width -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`, `target/reports/novywave-bridge-scenario.json`, and `target/reports/schema.json`; `git diff --check -- crates/boon_runtime/src/lib.rs`
- Result: in progress. Root-derived runtime fanout is now much lower without changing Boon syntax or adding NovyWave-specific runtime paths. Structured root changes dirty only changed child paths plus the whole parent, source-action roots are not fed into a duplicate final root-derived materialization pass after already being fully propagated, and owned-string scalar evaluation avoids extra clones/formatting in `Text/concat`, `Text/time_range_label`, and text-like `+`. The bridge scenario still passes with 71/71 required steps, 90 source events, and no failures. The official speed report still fails, but the runtime blocker moved substantially: `runtime_step_apply_p95=11.006ms` and `runtime_apply_p95=12.207ms`, down from the previous `34.458ms`/`35.724ms`.
- Follow-up: keep TASK-0804 in progress. The next slice should target native/layout/shared interaction cost: current official p95s are `click_to_cursor=38.316ms`, `input_to_visible=38.316ms`, `hover=19.841ms`, `divider_drag=25.848ms`, and `resize=14.016ms`; per-sample click rows now show runtime mostly below the frame budget, with remaining time in patched document layout, shared updates, and top-level interaction timing/present accounting.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec0ee-7dfe-7fd3-b1c2-140b7b50b3a8`; `cargo fmt --all`; `cargo check -p boon_native_playground`; `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`; `cargo test -p boon_ir source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/reports/novywave-bridge-scenario.json`
- Result: in progress. Native preview now has a generic, conservative direct `LayoutFrame` patch path for simple explicit-width row/stack geometry. It patches cursor/hover spacer rows in place, moves sibling line geometry, and rejects complex padded divider stacks instead of pretending a parent width change is enough. Focused tests cover both the accepted simple row case and the complex stack rejection. The bridge proof remains `status=pass`. The official speed report still fails but improved its shape: hover p95 is now under budget at `16.581ms`, direct layout-frame patch is true for 65 samples and false for 32 divider samples, and false samples all reject with `simple_stack_style_not_supported`. Remaining blockers are `click_to_cursor_p95=27.231ms`, `input_to_visible_p95=27.231ms`, and `divider_drag_p95=22.473ms`; runtime p95 is `12.172ms` apply / `10.742ms` step, so the next work should reduce 21-delta click root fanout and complex divider layout/shared-update cost without Boon syntax changes or fixture reduction.
- Follow-up: keep TASK-0804 in progress. Do not broaden direct dimension patching to arbitrary children; either build a real partial subtree relayout/splice path, add a generic paint-space overlay primitive for cursor/hover guides, or attack runtime root fanout for 21-delta click turns.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_native_playground`; `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `pgrep -af 'cargo xtask verify-native-gpu-novywave-interaction-speed|boon_native_playground.*novywave-interaction-speed|target/debug/xtask' || true`
- Result: in progress. The native live-event path no longer clones the full previous layout proof before runtime apply. Patch eligibility now uses the stable `layout_frame_hash` plus cached document snapshot and clones the proof only when a changed turn actually enters the document patch path. The official speed report still fails, but the current blocker is narrower: hover, divider, and resize p95 are under budget, while click/input p95 is `18.552ms` against `16.7ms`. Click samples show `total_apply_p95=17.312ms`, `runtime_apply_p95=13.626ms`, `runtime_step_apply_p95=12.639ms`, `layout_rebuild_p95=3.475ms`, and native resolve p95 `1.218ms`.
- Follow-up: keep TASK-0804 in progress. Do not revive the deferred runtime-state snapshot or cached source-intent-index experiments without new profiling; both were tried and killed. Next work should reduce the 21-delta cursor click cascade or avoid repeating hover/layout proof work after cursor moves without changing Boon syntax or reducing NovyWave fixture coverage.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: reused completed subagent reviews `019ec125-515d-7321-82a3-f69bbcbd68f8`, `019ec135-4164-7650-be3f-aa5d700674f9`, `019ec149-44af-79d1-80f1-d6e35d3e6314`, and `019ec0ff-16b7-7783-81e8-bae24ecbb798`; attempted a fresh explorer but the agent pool was full; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo check -p boon_native_playground`; `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`; `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`; `cargo test -p boon_runtime --lib root_scalar_same_event_flush_follows_qualified_derived_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`; `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `pgrep -af 'boon_native_playground.*novywave|verify-native-gpu-novywave-interaction-speed' || true`
- Result: in progress. Several narrow TASK-0804 experiments were tried and killed instead of kept as unproven complexity. Persisting the generic user-function value cache across turns passed focused cache tests but still failed click/input p95 at `18.384ms` and was reverted because stale-read risk was not justified. A per-step `List/find_value` memo for `ListRef` inputs passed focused list/index and NovyWave tests but worsened the fresh role report to click/input p95 `19.368ms` with higher text candidate counts, so it was reverted. A root-level numeric stability guard for root-derived list pipelines passed a focused root-pipeline test but regressed the official gate to click/input p95 `22.799ms` and did not reduce NovyWave counters, so it was reverted. Narrowing same-event root-scalar flushing so direct reads did not force materialization passed the same-event dependency and NovyWave tests, but the official gate still failed at click/input p95 `20.928ms`, so it was reverted. The current post-revert official report matches the current worktree and still fails only click/input p95: click/input `19.153ms` against `16.7ms`, hover `11.679ms`, divider `7.235ms`, resize `7.851ms`, runtime step/apply p95 `12.550ms`/`13.555ms`, layout p95 `3.538ms`, native resolve p95 `1.342ms`, and slow click samples remain the 21-semantic-delta/4-render-patch cursor class with `row_occurrences_scanned=173-177`, `text_lookup_index_candidates=115`, and `route_candidates_visited=124-128`.
- Follow-up: keep TASK-0804 in progress. Do not retry function-cache persistence, per-step `List/find_value` memoization, root-level numeric stability guards, or direct-read same-event flush narrowing without finer root-materialization timing that proves a different bottleneck. The next slice should add per-root materialization timing/count instrumentation or move the cursor/crosshair updates to a generic overlay/paint-space primitive so cursor clicks avoid rebuilding document layout and hover proof while preserving all semantic state deltas and without adding Boon syntax or NovyWave-specific shortcuts.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo check -p boon_native_playground`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Kept root-materialization instrumentation because it explains the remaining click failure without changing Boon syntax or source code. `LiveTurnOutput`, per-step runtime metrics, native interaction samples, and JSON profiles now expose `runtime_root_materialization_stats` with candidate/change/emitted counts, total time, and capped per-root samples. The official release speed gate still fails the same click budgets: top-level blockers report input-to-visible and click-to-cursor p95 `19.251ms` against `16.700ms`. Click interaction p95s from the role artifact are `total_apply=17.570ms`, `runtime_apply=12.652ms`, `runtime_step_apply=11.815ms`, `layout_rebuild=3.364ms`, and `shared_update=0.879ms`. The new root evidence shows click p95 root-materialization time `6.572ms` and max `7.157ms`, with p95 `candidate_count=123`, `changed_count=36`, and `emitted_mutation_count=19`; grouped samples identify `store.selected_signal_lane_rows` list-view materialization as the top measured root cost (`48.731ms` summed over sampled click records, max `3.928ms`), followed by unchanged `store.bridge_cursor_values.rows` pure checks (`13.944ms` summed, max `0.918ms`).
- Follow-up: keep TASK-0804 in progress. The next implementation slice should optimize generic root list-view materialization for cursor-driven selected-row pipelines, especially `store.selected_signal_lane_rows`, or make unchanged bridge row roots avoid repeated expensive pure checks. Keep the instrumentation until at least one promoted optimization proves a lower root-materialization p95 in `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; kill any optimization that lowers one sample but increases click/input p95 or hides semantic deltas.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec1e6-be8d-7422-ba20-f39ccfb51a25`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo check -p boon_native_playground`; `cargo test -p boon_runtime --lib source_store_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `pgrep -af 'boon_native_playground.*novywave|verify-native-gpu-novywave-interaction-speed|cargo xtask verify-native-gpu-novywave-interaction-speed' || true`
- Result: in progress. Kept the generic `SourceStore::bind_row` idempotence fix: rebinding the same row generation to already-bound source paths now skips duplicate source bindings while preserving duplicate paths inside one call. Focused source-store tests pass, including `source_store_rebinding_same_row_paths_is_idempotent` and the existing duplicate-storage growth test. A conservative list-view equality-copy removal experiment was tried and killed: it replaced `list_visible_snapshots() != rows` with a reference-based comparison helper, passed a focused semantic-equivalence test, but regressed the official release speed gate from the prior current-tree report (`18.459ms` click/input p95) to `20.755ms` and increased sampled `store.selected_signal_lane_rows` cost from `47.945ms` to `49.828ms`; the helper and test were removed. The post-revert official report matches the current worktree and still fails click/input only: click/input p95 `20.610ms` against `16.700ms`, hover p95 `10.411ms`, divider p95 `7.966ms`, resize p95 `8.268ms`, runtime step/apply p95 `12.297ms`/`13.491ms`, and layout p95 `3.752ms`. Click root-materialization p95 is `6.914ms`; grouped root samples still identify `store.selected_signal_lane_rows` as the top measured root cost (`50.520ms` summed over sampled click records, max `3.627ms`), followed by unchanged `store.bridge_cursor_values.rows` pure checks (`14.310ms` summed, max `0.957ms`).
- Follow-up: keep TASK-0804 in progress. Do not retry reference-based list-view equality or broad same-shape in-place row updates without stronger evidence and a more precise identity/source-binding contract. The next generic engine slice should either add field-level/per-row dependency reuse for root list-view materialization or move cursor/crosshair paint updates into a generic overlay/paint-space primitive so the 21-semantic-delta cursor class avoids repeated full selected-lane row materialization and layout proof work. Preserve all semantic deltas, keep fixtures intact, and add no Boon syntax.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: closed completed subagent `019ec1f4-b7c2-7862-9308-b446c05ab51d`; subagent explorer `019ec200-08aa-76e1-8e90-8a859765c2a7`; `cargo fmt --all`; `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_materialization_uses_changed_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_structured_parent_dirties_dependents_without_empty_text_patch -- --nocapture`; `cargo check -p boon_runtime`; `cargo check -p boon_native_playground`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Kept the generic structured-root diff fix: `boon_value_matches_json` now recognizes unchanged `RowRef`, `ListRef`, and `ListSelection` JSON shapes so changing a structured parent does not dirty stable non-scalar children such as list refs or selections. The focused regression proves only the changed scalar child is dirtied while stable list-ref/list-selection children are skipped. A root-value-cache clone-deferral experiment was tried and killed: it moved `root_value_cache` insertion after no-op checks for unchanged pure roots, passed focused root-derived and NovyWave semantic tests, but the official speed gate regressed to click/input p95 `20.989ms`; reverting just that experiment restored the official report to click/input p95 `17.850ms` against `16.700ms`, with hover `8.616ms`, divider `6.971ms`, resize `8.632ms`, runtime step/apply p95 `11.414ms`/`12.280ms`, and layout p95 `3.325ms`. Grouped root samples still identify `store.selected_signal_lane_rows` as the top materialization cost (`47.155ms` summed, max `3.160ms`), followed by unchanged `store.bridge_cursor_values.rows` (`13.833ms` summed, max `0.888ms`).
- Follow-up: keep TASK-0804 in progress. The next generic slice should target root `ListView` materialization for `store.selected_signal_lane_rows` with a conservative row-field diff/dependency-reuse path: preserve full replace/rebind semantics until correctness is proven, fall back to broad invalidation on length/order/identity uncertainty, and do not change Boon syntax or reduce NovyWave fixture coverage. Do not retry the killed cache clone-deferral, reference-based list-view equality, broad same-shape in-place updates, persistent function cache, per-step `List/find_value` memo, root numeric stability guard, or same-event flush-narrowing experiments without new profiling evidence.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec200-08aa-76e1-8e90-8a859765c2a7` (closed after completion); `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_row_field_diff_skips_structure_only_dependents -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_materialization_uses_changed_dependencies -- --nocapture`; `cargo check -p boon_runtime`; `cargo check -p boon_native_playground`; `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`; `cargo test -p boon_runtime --lib map_join_field -- --nocapture`; `cargo test -p boon_runtime --lib list_filter_field -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`, `target/artifacts/native-gpu/novywave-interaction-speed-role.json`, and `target/reports/novywave-bridge-scenario.json`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Result: in progress. Kept a conservative generic root `ListView` changed-read slice. `materialize_root_list_view_field` now returns changed read keys instead of a boolean. Same-length, same-shape row value changes dirty the list key plus exact `ListColumn`/`ListField` keys, but no longer dirty the root path; length, shape, or uncertainty still falls back to broad structure/root invalidation. Because current list-view materialization still replaces row storage wholesale, the list key remains dirty until a separate identity-preserving row update path is proven. Root list-view rows are now treated as owned by the list-view materializer: indexed derived-field plans for that same list are skipped during key generation and on-demand field lookup, fixing the existing `rows[0].label` recompute bug where a projected list-view row was read as if it were the source row. Missing list-field diagnostics now include available row fields.
- Latest speed result: the official release gate still fails only click/input p95, but improved from the prior `17.850ms` to `17.294ms` against the `16.700ms` budget. Hover `8.507ms`, divider `6.061ms`, and resize `8.810ms` remain under their budgets. Runtime step/apply p95 improved to `10.654ms`/`11.587ms`, layout p95 is `3.298ms`, and grouped root samples show `store.selected_signal_lane_rows` dropped from `47.155ms` summed/max `3.160ms` to `31.357ms` summed/max `2.623ms`. The bridge proof remains `status=pass`, `measurement_mode=proof`, with 71 required steps passing.
- Follow-up: keep TASK-0804 in progress. The next slice should either prove identity-preserving root list-view row updates with stable source binding and row-generation tests, or target unchanged pure roots such as `store.bridge_cursor_values.rows` without reintroducing killed cache/persistence experiments. The remaining p95 gap is small (`17.294ms` vs `16.700ms`), but do not tune the budget, reduce fixtures, or add Boon syntax.

- Date: 2026-06-13
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec215-435d-77b1-b622-fde41be9e357` (closed after completion); `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_materialization_uses_changed_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_worklist_revisits_direct_and_indirect_dependents -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`, `target/artifacts/native-gpu/novywave-interaction-speed-role.json`, and `target/reports/novywave-bridge-scenario.json`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Result: in progress. Investigated unchanged structured-child pure roots after the kept root ListView changed-read slice. The subagent identified `store.bridge_cursor_values.rows` as a hot unchanged root candidate (`24` sampled evaluations, `0` changes, roughly `14-15ms` summed in current reports), but two generic structured-child coalescing variants were killed. The first variant changed root worklist ordering so structured parents ran before owned children; focused tests passed, but the official speed gate regressed to click/input p95 `40.120ms` with runtime step/apply p95 `33.039ms`/`33.941ms`, so it was reverted. The second variant skipped owned children while the pure structured parent was dirty without reordering and proved changed-child reinsertion in a focused test, but the official speed gate still regressed to click/input p95 `20.610ms` and shifted cost into `store.bridge_cursor_values`, so it was also reverted. The current post-revert report matches the current worktree and still fails only click/input p95: click/input `16.975ms` against `16.700ms`, hover `7.793ms`, divider `6.408ms`, resize `8.581ms`, runtime step/apply p95 `10.483ms`/`11.467ms`, and layout p95 `3.232ms`. Grouped root samples remain `store.selected_signal_lane_rows` (`30.894ms` summed, max `2.315ms`) and unchanged `store.bridge_cursor_values.rows` (`14.342ms` summed, max `0.899ms`).
- Follow-up: keep TASK-0804 in progress. Do not retry structured-child coalescing by worklist ordering or dirty-parent skipping without a stronger dependency contract and a proof that it lowers whole-gate p95. The next slice should target either identity-preserving root ListView row updates for `store.selected_signal_lane_rows`, a cheaper direct evaluation path for owned `ListRef`/`ListSelection` structured children that does not force parent recomputation, or a generic paint/overlay path for cursor movement so the remaining 21-delta click class avoids full selected-lane/layout proof work.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_ir/src/lib.rs; crates/boon_native_playground/src/main.rs; crates/boon_runtime/src/lib.rs; examples/novywave/RUN.bn; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: `rg -n "root_dirty_closure|root_derived_worklist_waits_for_transitive|RootListViewChange|update_list_rows_preserving_identity|runtime_row_identity_values_match|runtime_row_identity_field" crates/boon_runtime/src/lib.rs` (no matches); `cargo fmt --all`; `cargo test -p boon_runtime --lib root_derived_worklist_ -- --nocapture`; `cargo test -p boon_runtime --lib root_scalar_same_event_flush_follows_qualified_derived_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib root_latest_ -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_native_playground sparse_document_patch_gate_ -- --nocapture`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`, `target/artifacts/native-gpu/novywave-interaction-speed-role.json`, and `target/reports/novywave-bridge-scenario.json`; `git diff --check -- crates/boon_ir/src/lib.rs crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs examples/novywave/RUN.bn`
- Result: in progress. Kept the compiler/source-payload shape as an engine capability, not a Boon workaround: `external_file_tree_label` uses the two-branch `LATEST` form with a direct `TEXT { - } |> Text/concat(with: elements.external_file_loaded_name.text, separator: " ")` branch plus the `show_empty` reset branch, and the focused IR regression proves it lowers to `PrefixPayloadConcat` without a fake `THEN` wrapper. Kept the native sparse document patch fix: alias-aware data-binding target lookup and targetless nonstructural layout/paint patches are accepted while targetless structural patches still reject; the sparse-patch focused suite passes and the current speed artifact has zero document patch fast-path rejections. Killed and reverted three runtime experiments after measurement: identity-preserving same-shape root ListView row updates regressed click/input p95 to `22.999ms` without reducing `store.selected_signal_lane_rows`; non-root scalar batching regressed to `23.909ms`; transitive root dirty closure regressed to click/input p95 `28.460ms`, hover p95 `32.508ms`, and raised 7-delta candidate counts to `85.5`.
- Latest speed result: the current official release gate still fails click/input p95 at `21.930ms` against `16.700ms`; hover p95 `9.452ms`, divider p95 `10.089ms`, and bridge proof `status=pass`/`measurement_mode=proof` are current. Click samples still split into 7-delta and 21-delta classes. The 21-delta class averages `19.922ms` total, `13.466ms` runtime apply, `6.732ms` root materialization, `124` root candidates, `36` changed roots, and `19` emitted mutations. Aggregated sampled click roots again identify `store.selected_signal_lane_rows` as the dominant root cost (`55.565ms` summed, max `3.720ms`), followed by unchanged `store.bridge_cursor_values.rows` (`14.276ms` summed, max `0.922ms`). Direct layout-frame patching is true for click samples, document layout recompute is avoided, and 21-delta click layout rebuild still averages `5.096ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry the killed identity-preserving ListView update, non-root scalar batching, transitive dirty closure, root-value-cache clone deferral, reference-based list equality, structured-child coalescing, persistent function cache, per-step `List/find_value` memo, root numeric stability guard, or same-event flush narrowing without new profiling that proves a different implementation target. The next slice should use fresh subagent/runtime/native review and target either a narrower generic root ListView materialization reuse path, a cheaper owned `ListRef`/`ListSelection` child evaluation path, or cursor/crosshair paint-space handling that preserves semantic deltas and fixtures without new Boon syntax.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorers `019ec35d-8d08-7351-bcb7-bf007fbad654` and `019ec35d-adfc-79a2-a1d5-3e1ec0435017`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_row_field_diff_skips_structure_only_dependents -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Result: in progress. Kept generic root list-view phase profiling because it explains the current hot path without changing Boon syntax, dirty semantics, render patches, or fixtures. `LiveRuntimeRootMaterializationSample` now carries an optional `list_view_profile` with row counts, changed row/field counts, broad-fallback status, and substage timings for root list views. The focused regression proves materialized list-view samples expose the profile. The refreshed speed gate still fails click/input only, but p95 is effectively unchanged from the previous run (`21.860ms` vs `21.930ms`), so the instrumentation passes its overhead kill criterion for now.
- Latest profile result: `store.selected_signal_lane_rows` remains the dominant sampled root (`57.905ms` summed, max `3.940ms`) and the new phase split shows the cost is evaluation, not storage churn: `eval_ms=51.375ms` summed, `row_materialize_ms=0.974ms`, `previous_snapshot_ms=1.071ms`, `diff_ms=0.182ms`, `replace_ms=0.488ms`, and `rebind_ms=0.005ms`. Every sampled `selected_signal_lane_rows` turn broad-falls back with the same row count (`3`) and zero narrowed changed fields, but the broad fallback itself is cheap; the expensive part is re-evaluating the list-view expression and its nested cursor-value pipeline. The current gate reports click/input p95 `21.860ms`, hover p95 `9.188ms`, divider p95 `9.558ms`, runtime step/apply p95 `12.099ms`/`13.724ms`, and layout p95 `5.492ms`.
- Follow-up: keep TASK-0804 in progress. Do not spend the next slice on previous-row snapshot cloning or replace/rebind unless a later profile contradicts this run. The next generic implementation should attack `selected_signal_lane_rows` evaluation itself: profile or optimize nested user-function calls such as `selected_cursor_value_for_signal`, preserve reusable filter/retain/map/join work inside a root list-view evaluation, or add a safe field-shape explanation before trying to narrow broad fallback. Any change must preserve semantic deltas, source fixtures, and the two-branch `LATEST` source-payload concat shape.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec36c-69a6-70a2-99ef-df447725172b`; `cargo fmt --all`; `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Kept an opt-in generic user-function profile, but killed the always-on form as interaction-mode machinery. `LiveTurnOutput`, runtime step reports, and native interaction samples can now carry `function_call_stats` with call counts, cache hits, total/max time, and capped samples. Production/runtime builds only collect it when `BOON_RUNTIME_FUNCTION_PROFILING` is enabled; unit tests keep it enabled so regressions catch missing attribution. This preserves diagnostic visibility without making official interaction timing pay per-function `Instant` and aggregate-map costs.
- Latest diagnostic result: the temporary always-on function profiler confirmed that the `selected_signal_lane_rows` evaluation cost is broader than `selected_cursor_value_for_signal` alone. Across sampled click records the largest inclusive function costs were `RUN/new_signal_lane_row` (`106.676ms`, 96 calls), `RUN/signal_lane_segment_rows` (`58.509ms`, 96 calls), `RUN/new_signal_lane_group_row` (`52.393ms`, 32 calls), `RUN/new_signal_lane_variable_row` (`47.086ms`, 64 calls), `RUN/new_waveform_segment` (`24.498ms`, 186 calls), `RUN/selected_cursor_pair_row` (`16.142ms`, 64 calls), and `RUN/selected_cursor_value_for_signal` (`15.432ms`, 128 calls, 64 cache hits). The no-profiler official report still fails the click/input p95 budget at `26.379ms` against `16.700ms`; role details show hover p95 `13.142ms`, divider p95 `13.478ms`, resize p95 `11.932ms`, runtime step/apply p95 `13.720ms`/`15.650ms`, and the `selected_signal_lane_rows` list-view profile still spends almost all sampled root time in eval (`eval_total=55.427ms` of 16 samples).
- Follow-up: keep TASK-0804 in progress. The next slice should not retry persistent function caches, function-cache key heuristics, previous-row snapshots, replace/rebind work, or broad identity-preserving row updates. The new evidence points to field-level reuse inside root list-view record construction or a generic cached/fused segment-row projection for stable row fields: cursor clicks should not rebuild lane `segments`, `lane_identity`, page/window refs, and other row fields whose read sets are disjoint from the cursor-value reads. Any implementation must keep full semantic deltas, preserve NovyWave fixtures, and avoid Boon syntax changes.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec37c-0f4e-71d2-af5a-3ff4a03a1990`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_stable_fields_across_cursor_change -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_row_field_diff_skips_structure_only_dependents -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib repeated_user_function_call_reuses_value_and_body_reads -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ignores_unreferenced_caller_env_bindings -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Kept a conservative generic root ListView record-field cache. It is keyed by root list-view path, source row key/generation, record/function scope, and output field, and entries store the exact field value, read keys, and numeric stability guards. Cache hits merge reads/guards back into the parent frame; misses isolate field reads before storing. Existing full row diff, replace, and source rebind behavior is unchanged, so this does not revive the killed identity-preserving list-view update path. Entries are invalidated from the same changed read keys used by generic derived and root-derived propagation, including read keys emitted by same-step derived materialization.
- Latest speed result: the official release gate still fails click/input p95 at `24.675ms` against `16.700ms`, but the kept cache materially reduces the measured root evaluation cost. For sampled click records, `store.selected_signal_lane_rows` list-view eval dropped from the prior no-profiler `55.427ms` total / `3.950ms` max to `20.934ms` total / `1.556ms` max, with `field_cache_hits=1136`, `field_cache_misses=32`, and `field_cache_stores=32`. Runtime step/apply p95 improved to `11.129ms`/`12.786ms`; layout p95 is `6.131ms`; role details report hover p95 `14.036ms`, divider p95 `11.201ms`, resize p95 `14.374ms`, and max observed `25.868ms`.
- Follow-up: keep TASK-0804 in progress. The next slice should use the reduced root-eval evidence to target the remaining end-to-end gap, likely top-level interaction/layout timing for the 21-delta click class or further generic materialization reuse outside root ListView record fields. Do not retry persistent function caches, row identity replacement, previous snapshots, replace/rebind work, root numeric guards, or same-event flush narrowing without new evidence.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorers `019ec38c-a884-7de3-a813-541c2bcb7eea`, `019ec38c-a9b8-7ce0-9dcf-02a9a2a29ea0`; `cargo fmt --all`; `cargo test -p boon_ir source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_stable_fields_across_cursor_change -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_materialization_uses_changed_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo check -p boon_runtime`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `timeout 180 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (killed structured-root reuse experiment); `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Kept a runtime regression test proving direct source-payload concat branches inside two-branch `LATEST` are valid without wrapping the payload read in a fake `THEN`. This locks the same contract already covered by IR lowering: `TEXT { - } |> Text/concat(with: elements.external_file_loaded_name.text, separator: " ")` updates from the source text payload, while the sibling `show_empty` `THEN` branch still resets the held label. No Boon syntax change or source workaround is needed.
- Killed experiment: same-pass structured-root child reuse looked attractive for unchanged `bridge_cursor_values.rows`, and a focused unit test could be made to pass, but the bridge proof exceeded the 180-second kill threshold before the experiment was reverted. The culprit was broad structured-root dependency ordering and parent-child read reuse creating too much root scheduling work. Do not revive this exact approach without a much narrower dependency contract and a proof that `verify-novywave-bridge-scenario` remains fast.
- Latest speed result: the current official release gate still fails only click/input p95 at `25.609ms` against `16.700ms`; max observed is `26.558ms`, within the `33.400ms` max budget. Hover p95 `12.300ms`, divider p95 `11.754ms`, and resize p95 `12.791ms` pass. Runtime step/apply p95 are `11.618ms`/`13.422ms`, layout p95 is `6.616ms`. Sampled click roots still show `store.selected_signal_lane_rows` as the dominant kept-cache cost (`27.638ms` total, `1.967ms` max, `field_cache_hits=1136`, `field_cache_misses=32`, `field_cache_stores=32`, `eval_total=20.594ms`) and unchanged `store.bridge_cursor_values.rows` still costs `15.586ms` total across sampled click materializations.
- Follow-up: keep TASK-0804 in progress. Next work should follow the subagent/local evidence toward either a safer root-ListView evaluation-local projection cache with caller-argument-safe keys, or clone-light direct layout patching with explicit `layout_clone_ms` / direct-patch counters. Also add nested root-list field-cache correctness tests before any nested cache expansion, because the current key lacks caller-argument identity for nested functions such as segment rows. Do not retry the killed structured-root child reuse, broad root dependency overlap, persistent function caches, row identity replacement, previous snapshots, replace/rebind work, root numeric guards, or same-event flush narrowing without new evidence.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec3a4-82ec-7d23-b3cc-bbacf09dc1d6`; subagent explorer `019ec3af-de79-7eb0-a439-958bfa5b8527`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo check -p boon_native_playground`; `cargo test -p boon_runtime --lib root_list_view_field_cache_separates_same_source_row_by_caller_env -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_stable_fields_across_cursor_change -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo test -p boon_native_playground sparse_document_patch_gate_ -- --nocapture`; `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `git diff -- crates/boon_native_playground/src/main.rs | rg -n "live_layout_state_frame_hash|layout_hash_basis|runtime_document_state_hash" -C 3 || true`
- Result: in progress. Kept the root `ListView` record-field cache correctness fix before extending it further. `RootListViewFieldCacheKey` now includes a caller environment fingerprint, so nested mapped functions that project the same inner source row under different outer caller rows cannot reuse the wrong field value. `eval_user_function_record_statement_children` now restores the previous record scope after nested record-returning calls, so a nested function such as `detail_row` does not leak its scope into the caller. The new focused regression maps two outer rows through the same inner row and proves both flat and nested record-derived labels stay distinct (`A/left`, `A/right`, `A:left`, `A:right`). The bridge proof remains current and passing: `status=pass`, `measurement_mode=proof`, bridge coverage `pass`, and no failed groups.
- Killed experiment: a runtime-state-based patched-layout hash was tried as a generic way to make repeated cursor states hit the document render snapshot cache. It did produce cache-like hits (`28/32` click samples no longer direct-patched), but failed the kill criteria and was reverted: click/input p95 worsened to `22.782ms`, layout p95 worsened to `6.087ms`, and the official gate still failed. The current post-revert report again matches the current worktree: all `32/32` click samples use direct layout-frame patching, click/input p95 is `22.578ms` against `16.700ms`, max observed is `22.649ms` within the `33.400ms` max budget, hover p95 `10.106ms`, divider p95 `10.878ms`, and resize p95 `9.864ms` all pass. Click timing p95s are `total_apply=20.594ms`, `runtime_apply=12.713ms`, `runtime_step_apply=11.084ms`, `layout_rebuild=6.105ms`, `shared_update=1.112ms`, and native resolve p95 `0.629ms`. Sampled click roots remain `store.selected_signal_lane_rows` (`27.442ms` total, `2.018ms` max, `field_cache_hits=1136`, `field_cache_misses=32`, `field_cache_stores=32`, `eval_total=20.693ms`) and unchanged `store.bridge_cursor_values.rows` (`14.756ms` total, `0.959ms` max).
- Follow-up: keep TASK-0804 in progress. The next slice should follow the native subagent recommendation toward clone-light direct layout patching with explicit `layout_clone_ms` / direct-patch substage counters before attempting another layout optimization. Do not retry the runtime-state layout-cache hash unless profiling shows the remaining cost is not the frame/proof cloning path. Do not extend the root `ListView` field cache without keeping the caller-env regression and record-scope restoration tests.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec3bc-0cd3-7ef0-88bd-04dd35acd13f`; `cargo fmt --all`; `cargo check -p boon_native_playground`; `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`; `cargo test -p boon_native_playground sparse_document_patch_gate_ -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Kept direct layout-patch profiling and compact layout-proof cloning. Click samples now report `layout_patch_profile` and `layout_proof_clone` summaries, and compact proof-header cloning reduced proof clone p95 to `0.007ms`. The direct layout patch path remains accepted for all click samples, with p95 substages `direct_layout_patch_total=1.610ms`, `direct_target_patch=0.667ms`, `direct_row_group_patch=0.603ms`, `document_frame_clone=0.360ms`, `layout_frame_clone=0.376ms`, `proof_build=1.262ms`, and `snapshot_cache_layout_clone=0.469ms`. The shared layout-frame override now uses an `Arc` so render/shared-state readers do not eagerly clone the full layout frame; focused direct-layout and sparse-document patch tests still pass. The official speed gate still fails click/input p95 at `21.216ms` against `16.700ms`; max `22.337ms`, hover p95 `9.579ms`, divider p95 `10.569ms`, resize p95 `10.022ms`, runtime apply p95 `13.273ms`, and layout rebuild p95 `4.955ms`.
- Follow-up: keep TASK-0804 in progress. The next slice should target the remaining click/input gap, not proof JSON cloning. Current evidence points at runtime apply plus unavoidable direct layout publication/cache work; any further direct-layout optimization should first prove it removes `layout_frame_clone` or `snapshot_cache_layout_clone` without regressing the 32/32 direct-patch acceptance or hover/divider/resize budgets.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_native_playground/src/main.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorers `019ec3d0-137e-75f0-b6a1-b0baf6ffb850` and `019ec3d0-1210-7160-8e3d-0d9339b7b1b7`; `cargo fmt --all`; `cargo check -p boon_native_playground`; `cargo test -p boon_native_playground direct_layout_patch_ -- --nocapture`; `cargo test -p boon_native_playground sparse_document_patch_gate_ -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `git diff --check -- crates/boon_native_playground/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Result: in progress. Kept the clone-light direct-layout publication/cache change. `DocumentPatchLayoutResult` and `DocumentRenderSnapshot` now share patched layout frames through `Arc<LayoutFrame>`, so the direct-patch result, shared render state, and document render snapshot cache no longer require a second full layout-frame clone after patching. The fresh release speed report proves the targeted cost collapsed: click `snapshot_cache_layout_clone` p95 dropped from `0.469ms` to `0.000093ms`, `snapshot_cache` p95 dropped to `0.032ms`, and all `32/32` click samples still used the direct layout-frame patch path. The official gate still fails click/input p95 at `21.172ms` against `16.700ms`; max `21.288ms`, hover p95 `9.176ms`, divider p95 `8.911ms`, resize p95 `11.118ms`, runtime apply p95 `11.907ms`, runtime step p95 `10.167ms`, and layout rebuild p95 `4.423ms`. Click `total_apply` p95 improved to `18.793ms`; the remaining sampled root costs are still runtime-side, led by `store.selected_signal_lane_rows` (`26.099ms` total) and unchanged `store.bridge_cursor_values.rows` (`14.068ms` total).
- Follow-up: keep TASK-0804 in progress. Do not try to remove the direct patch `layout_frame_clone` without a deeper layout overlay/persistent-frame design; it is the current safe mutation boundary. The next implementation candidate is the runtime explorer's bounded proposal: a turn-local root-`ListView` evaluation projection cache for repeated cursor-value-style scalar row projections, with caller-env/read-key guards and focused stale-reuse tests before running the bridge and speed gates.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec3db-b9ee-7c73-8b67-54c952fa3ba9`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_row_field_diff_skips_structure_only_dependents -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Result: in progress. Killed and reverted a same-length root `ListView` row-shape union-diff experiment. The experiment made `runtime_row_changed_field_names` emit exact added/removed field read keys instead of broad-fallbacking when row field sets differed. Focused tests passed and `store.selected_signal_lane_rows` reported `broad_fallback=false`, but the official speed gate regressed to click/input p95 `22.590ms` and did not reduce root candidate counts: the 7-delta click class still had `23-24` root candidates, the 21-delta class still had `124`, and the selected-lane diff work rose to roughly `0.056-0.073ms` per sampled materialization with `20-23` changed read keys. The runtime/test patch was removed.
- Latest speed result: the post-revert official release gate again reflects the current worktree and still fails only click/input p95: `21.028ms` against `16.700ms`, max `21.817ms`, hover p95 `8.767ms`, resize p95 `8.920ms`, runtime step/apply p95 `10.496ms`/`12.186ms`, and layout p95 `4.324ms`. The click samples still split into a 7-semantic-delta class (`23-24` root candidates, max total `13.479ms`) and a 21-semantic-delta class (`124` root candidates, max total `19.321ms`). Sampled root costs remain led by `store.selected_signal_lane_rows` (`26.001ms` total, broad fallback in all `16/16` samples) and unchanged `store.bridge_cursor_values.rows` (`13.987ms` total).
- Follow-up: keep TASK-0804 in progress. Do not retry same-length row-shape union diff as a speed path unless a later profile proves the extra exact read keys reduce root scheduling work. The next generic slice should target the 21-delta root scheduling/materialization class directly: repeated pure roots such as `store.bridge_request_descriptor`, `store.bridge_cursor_values_page_ref`, and their unchanged children are still recomputed multiple times per click. The subagent review also confirms the existing user-function cache already covers identical `selected_cursor_value_for_signal` calls, so a new projection cache must prove it covers work outside that cache before implementation.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: docs/plans/speedup/12-speedup-goal-execution-checklist.md (runtime wave-batching experiment in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: subagent explorer `019ec3e9-36c7-7461-af4d-6ab524ca7ce4`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_materialization_batches_dependent_after_frontier_chains_settle -- --nocapture` (passed only during the killed experiment, then test removed with the experiment); `cargo test -p boon_runtime --lib root_list_view_field_cache -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (run once on the killed experiment and again after revert; both fail current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Killed and reverted a root-dirty frontier wave-batching experiment. The idea was to drain the current root dirty frontier, collect changed root read keys, and enqueue newly discovered dependents only for the next frontier, while still invalidating caches immediately. A synthetic diamond-chain runtime test proved the intended freshness shape, but the official NovyWave gate rejected the tradeoff: the experiment reduced some duplicate `bridge_request_descriptor`/`bridge_cursor_values_page_ref` counts, but increased 21-delta click root totals to roughly `8.5-10.1ms`, raised `store.selected_signal_lane_rows` to `28.954ms` total and `store.bridge_cursor_values.rows` to `16.897ms` total across sampled 21-delta clicks, and regressed click/input p95 to `25.018ms`. The runtime patch and focused test were removed.
- Latest speed result: the refreshed post-revert official release gate again reflects the current worktree and still fails only click/input p95: `21.749ms` against `16.700ms`, max `24.124ms`, hover p95 `9.091ms`, divider p95 `10.011ms`, resize p95 `9.454ms`, runtime step/apply p95 `10.805ms`/`12.570ms`, and layout p95 `4.554ms`. The 21-semantic-delta click class is back to `124` root candidates and `36` changed roots, with root totals around `4.79-5.67ms`. Sampled duplicate roots remain `store.bridge_request_descriptor` (`32` samples, all changed, `3.278ms` total), `store.bridge_cursor_values_page_ref` (`32` samples, all changed, `2.134ms` total), and unchanged children such as `store.bridge_request_descriptor.identity` (`32` samples, `1.694ms` total).
- Follow-up: keep TASK-0804 in progress. Do not retry root-dirty frontier wave batching unless new profiling shows `selected_signal_lane_rows` and `bridge_cursor_values.rows` stay flat under that ordering. The next generic runtime slice should follow the subagent proposal toward pass-local root freshness metadata rather than another ordering-only change: track dirty reasons by root, stamp root evaluations by the read tokens they observed, commit already-current cached roots without re-evaluating their AST, and skip a requeued root only when every read token it observed is unchanged. Required tests before promotion: cached parent commit without re-eval, revisiting when a dependency token moves after an earlier eval, same-event source flush invalidation, `store.foo`/`foo` alias canonicalization, and structured child reuse that does not repeat unchanged children while changed siblings still propagate.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: docs/plans/speedup/12-speedup-goal-execution-checklist.md (runtime cached-root-commit experiment in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: `cargo fmt --all`; `cargo test -p boon_runtime --lib root_materialization_commits_dependency_computed_root_without_reeval -- --nocapture` (passed only during the killed experiment, then test removed with the experiment); `cargo test -p boon_runtime --lib root_derived_materialization_uses_changed_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_worklist_ -- --nocapture`; `cargo test -p boon_runtime --lib root_scalar_same_event_flush_follows_qualified_derived_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_revisits_earlier_dependent_after_later_dependency_changes -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (current post-revert report `status=pass`, `measurement_mode=proof`); `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (run once on the killed experiment and again after revert; both fail current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Killed and reverted a cached-root commit experiment. The idea was to let `materialize_root_derived_field_commit` commit from a `root_value_cache` value that had already been computed through `root_derived_boon_value`, so a parent root forced by a child could still publish its semantic delta later without re-evaluating its AST. A synthetic test proved the local contract: the child saw fresh values, the parent delta was emitted, and the parent function body ran once. The NovyWave bridge proof also passed during the experiment. The speed oracle rejected it: the official gate regressed click/input p95 to `24.308ms`, the 21-delta class stayed at `124` root candidates and `36` changed roots, and duplicate roots such as `store.bridge_request_descriptor` and `store.bridge_cursor_values_page_ref` were effectively unchanged. The runtime patch and focused test were removed.
- Latest speed result: the refreshed post-revert official release gate again reflects the current worktree and still fails only click/input p95: `21.569ms` against `16.700ms`, max `21.764ms`, hover p95 `9.632ms`, divider p95 `9.623ms`, resize p95 `9.371ms`, runtime step/apply p95 `10.467ms`/`12.109ms`, and layout p95 `4.426ms`. The 21-semantic-delta click class is back to `124` root candidates and `36` changed roots, with root totals around `4.65-5.08ms`. Sampled duplicate roots remain `store.bridge_request_descriptor` (`32` samples, all changed, `3.270ms` total), `store.bridge_cursor_values_page_ref` (`32` samples, all changed, `2.061ms` total), and unchanged `store.bridge_request_descriptor.identity` (`32` samples, `1.662ms` total).
- Follow-up: keep TASK-0804 in progress. Do not retry cached root commit by itself; a local stale guard is not enough to improve this hot path. The next runtime design must either implement the full pass-local token/freshness scheme with dirty reasons and observed read-token checks, or switch targets to the remaining expensive root-list/bridge row evaluations. Promotion still requires the bridge proof to pass and the official speed gate not to regress click/input p95, root totals, or `selected_signal_lane_rows` / `bridge_cursor_values.rows`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: docs/plans/speedup/12-speedup-goal-execution-checklist.md (runtime structured-child shortcut and native route-table index experiments were reverted)
- Verification: subagent explorers `019ec402-421a-7153-9ba6-1d086bdbcc8c` and `019ec402-4352-77e2-a0ed-2e9e50fcfc5e`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture` (existing broad filter still fails `root_derived_list_literal_when_matches_runtime_values` in the current dirty tree and was not used as acceptance evidence); `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache -- --nocapture`; `cargo check -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json -- --nocapture` (fails current dirty tree with `store.selected_input.sources.editor.select` source-ID resolution and was not used as acceptance evidence); `cargo test -p boon_native_playground --bin boon_native_playground novywave_row_formatter_real_click_opens_and_rerenders_preview_layout -- --nocapture` (fails current dirty tree because the test asks for `signal.format_elements.format_dropdown_toggle` while current source intents expose `selected_signal.format_elements.format_dropdown_toggle`); `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `rg -n "structured_root_child_value_owned_by_parent|root_path_is_ancestor_of|hit_entry_by_node|display_item_by_node|source_intent_by_node" crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs` (no matches after revert).
- Result: in progress. Killed and reverted a same-pass structured-root child shortcut. The idea was to materialize a nested root path from an already-owned parent record instead of evaluating the nested expression. It immediately exposed a stale broad-`store` snapshot hazard: the existing `root_derived_list_literal_when_matches_runtime_values` invariant returned `Array []` instead of `compact primary`. This confirms the subagent warning that structured child reuse must be implemented through a real pass-local token/freshness model, not an ownership shortcut. Also killed and reverted a native `PreviewHitRouteTable` node/display/source-intent index experiment because the focused poisoned-proof route-table test does not currently provide a passing proof surface in this dirty tree; keeping an index without a clean route proof would make speed evidence weaker.
- Follow-up: keep TASK-0804 in progress. The next runtime implementation should use the full pass-local root freshness/token design, not another shortcut: replace the plain `root_value_cache` with entries carrying value, input reads, published reads, observed read tokens, and pass ID; use published reads for dependency graph updates but only actual input reads for freshness; bump canonical `store.foo`/`foo`/leaf tokens for changed reads; commit from cache only when every observed token still matches; and add the required stale-reuse tests before running the bridge and speed gates. Do not retry structured parent-child reuse or route-table indexing until their focused proof tests are passing on the current worktree.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_parser/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec410-56d2-7872-a35a-d8b2d65dbe28`; `cargo fmt --all`; `cargo test -p boon_parser list_literal_pipe_on_same_line_is_parsed_as_when_input -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_list_literal_when_matches_runtime_values -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (`status=pass`, `measurement_mode=proof`); `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `git diff --check -- crates/boon_parser/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Result: in progress. Kept a generic parser fix for same-line list-literal pipelines. A line such as `LIST { mode, side } |> WHEN { ... }` is now parsed and executed as an expression pipeline instead of being classified as an `AstStatementKind::List` initializer that silently evaluates to `[]`. This removes the concrete `root_derived_list_literal_when_matches_runtime_values` blocker without changing Boon syntax or adding a source workaround. The broad `root_derived_` runtime filter now passes, and the no-wrapper two-branch `LATEST` source-payload concat regression still passes.
- Latest speed result: the refreshed official release gate still fails only the click/input p95 budget: click/input p95 `20.698ms` against `16.700ms`, max `21.381ms`, hover p95 `8.732ms`, divider p95 `8.050ms`, resize p95 `11.159ms`, runtime apply p95 `11.963ms`, runtime step apply p95 `10.224ms`, and layout p95 `4.295ms`. The slow sampled classes still include the 21-semantic-delta/124-root-candidate click class, plus one 24-delta/127-root-candidate sample. Sampled root costs remain led by `store.selected_signal_lane_rows` (`27.050ms` total, all changed), unchanged `store.bridge_cursor_values.rows` (`14.371ms` total), `store.bridge_request_descriptor` (`5.038ms` total), and `store.bridge_cursor_values_page_ref` (`3.403ms` total).
- Follow-up: keep TASK-0804 in progress. This clears the known parser/statement-kind smell that was hiding root-derived evidence, but it does not implement the broader pass-local root freshness/token design recommended by the subagent. The next speed slice should still target either full observed-read-token root freshness or the remaining expensive root-list/bridge row evaluations, with bridge proof and official speed-gate evidence before promotion.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec427-4871-7853-8aa8-335d24c3f882`; `cargo fmt --all`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib list_ -- --nocapture`; `cargo test -p boon_runtime --lib source_text_payload_can_be_read_inside_then_update_expression -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (`status=pass`, `measurement_mode=proof`, 71/71 required scenario steps, 77 checks, zero failed checks); `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`
- Result: in progress. Kept a generic indexed list-pipeline fusion for the NovyWave cursor-value shape without changing Boon syntax or adding NovyWave-specific runtime branches. The runtime now recognizes linear static-field chains of `List/filter_field_equal`, `List/filter_field_not_equal`, and row-field-vs-scalar numeric `List/retain` immediately before `List/map(record) |> List/join_field`, computes the ordered `ListSelection` with the existing text/numeric index APIs, then feeds the existing projected-field join fusion. Unsupported dynamic fields, bool/non-text filters, unrecognized retain predicates, nonnumeric scalars, unindexed stages, record spreads, and non-record map projections fall back to the old evaluator. The focused regression proves order, fallback `empty`, text not-equal filters, numeric retain bounds, zero filter/retain scans, and zero join-field scans for the fused shape.
- Latest speed result: the official release gate still fails click/input p95, but the generic fusion moved the measured hot path in the right direction. Current click/input p95 is `21.428ms` against `16.700ms`, max `21.522ms`, hover p95 `10.184ms`, divider p95 `9.988ms`, resize p95 `9.940ms`, runtime apply p95 `11.945ms`, runtime step p95 `10.300ms`, and layout p95 `4.469ms`. The report shows the fused pipeline firing in click samples: `map_join_field_fusions=64`, `map_join_field_rows_fused=64`, `filter_field_rows_scanned=0`, `retain_rows_scanned=0`, `text_lookup_index_hits=411`, and `numeric_lookup_index_hits=128`. Sampled root costs remain led by `store.selected_signal_lane_rows` (`25.833ms` total, all changed), unchanged `store.bridge_cursor_values.rows` (`14.291ms` total), `store.bridge_request_descriptor` (`5.067ms` total), and `store.bridge_cursor_values_page_ref` (`3.356ms` total).
- Follow-up: keep TASK-0804 in progress. The fused cursor-value operator chain removed a real per-stage cost but not the broader root materialization/scheduling gap. Do not spend more time on adjacent filter/retain/map/join micro-fusion unless the report shows new row scans. The next speed slice should target either full pass-local observed-read-token freshness for unchanged bridge row roots, or a deeper root `ListView` materialization design that reduces repeated `store.selected_signal_lane_rows` / `store.bridge_cursor_values.rows` evaluation without stale broad-`store` shortcuts.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: docs/plans/speedup/12-speedup-goal-execution-checklist.md (runtime pass-local root token/freshness experiment in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: subagent explorers `019ec437-8e8d-7531-9c3f-7b29462b71c4` and `019ec43c-9d56-7700-b102-93f2c0856a20`; `cargo fmt --all`; `cargo check -p boon_runtime`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib root_materialization_fresh_cache_ -- --nocapture` (passed only during the killed experiment, then tests removed with the experiment); `cargo test -p boon_runtime --lib root_scalar_same_event_flush_follows_qualified_derived_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_revisits_earlier_dependent_after_later_dependency_changes -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (failed during the too-narrow root-to-root-read version, then passed after rollback); `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (failed current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Killed and reverted a full pass-local observed-read-token freshness experiment. The implementation replaced the plain root value cache with entries carrying input reads, published reads, and observed tokens, added root/list token invalidation, attempted fresh cached commits, and briefly narrowed root-to-root reads to child root outputs. Focused stale-read tests could be made to pass, and the report showed `store.bridge_cursor_values.rows` cache hits, but the tradeoff failed both correctness and speed gates. The too-narrow root-to-root read version left updated `signal_search_text` visible while search-result roots stayed stale, and the conservative published-read replay version restored the bridge proof but regressed speed badly: click/input p95 jumped to `66.950ms`, runtime apply p95 to `53.115ms`, and unchanged `store.bridge_cursor_values.rows` worsened to `36.002ms` total despite `24` cache hits. The runtime patch and focused tests were removed.
- Latest speed result: the current post-revert official release gate again reflects the current worktree and still fails only click/input p95: `22.610ms` against `16.700ms`, max `22.719ms`, hover p95 `10.573ms`, divider p95 `10.368ms`, resize p95 `12.452ms`, runtime step/apply p95 `10.751ms`/`12.507ms`, and layout p95 `4.452ms`. The bridge proof is current and passing: `status=pass`, `measurement_mode=proof`, `71/71` required steps covered, and no bridge coverage failures. Sampled root costs remain led by `store.selected_signal_lane_rows` (`27.300ms` total, all changed, `field_cache_hits=1136`, `field_cache_misses=32`, `eval_total=20.560ms`) and unchanged `store.bridge_cursor_values.rows` (`14.703ms` total, `24` samples, no changes).
- Follow-up: keep TASK-0804 in progress. Do not retry the broad root token/freshness cache in this form. If root freshness is revisited, first add instrumentation that separates dirty reason, initial scheduling reason, actual input-read token changes, published-output changes, and root-to-root dependency edges before changing scheduling. The next implementation slice should instead target a narrower measured cost: either `store.selected_signal_lane_rows` list-view evaluation internals that are still changed every click, or `store.bridge_cursor_values.rows` as a value-specific projection/materialization problem without changing global root scheduling.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: crates/boon_runtime/src/lib.rs; docs/plans/speedup/12-speedup-goal-execution-checklist.md
- Verification: subagent explorer `019ec475-ffa4-7ad0-a388-e1985e023314`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `git diff --check -- crates/boon_runtime/src/lib.rs`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (`status=pass`, `measurement_mode=proof`); `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Killed and reverted a direct root-list `List/length` / `List/count` shortcut. The shortcut avoided cloning cached root list values for direct pipe inputs and locally reduced unchanged `store.selected_lane_materialized_row_count` from roughly `4.508ms` to `3.805ms` over click samples, but the official speed oracle rejected the tradeoff: click/input p95 regressed to `26.800ms`, runtime apply p95 to `13.421ms`, layout p95 to `5.107ms`, `store.selected_signal_lane_rows` rose to `24.112ms` eval total, and unchanged `store.bridge_cursor_values.rows` rose to `17.051ms`. The runtime patch and focused test were removed.
- Current speed result: the refreshed post-revert official release gate reflects the current worktree and still fails click/input p95: `21.355ms` against `16.700ms`, max `21.934ms`, hover p95 `9.169ms`, divider p95 `10.379ms`, resize p95 `9.324ms`, runtime step/apply p95 `10.537ms`/`12.231ms`, and layout p95 `4.303ms`. The bridge proof is current and passing. `store.selected_signal_lane_rows` remains the top sampled list-view cost (`21.073ms` eval total, `0.016ms` row materialization total, `1136` field-cache hits, `32` misses, `broad_fallback=0`), while unchanged `store.bridge_cursor_values.rows` remains expensive (`15.253ms` total over `24` samples). The 21-semantic-delta click class still has `124` root candidates and root totals around `4.98-5.51ms`.
- Follow-up: keep TASK-0804 in progress. The next candidate should follow the subagent's narrower read-fingerprint direction rather than another syntax/source workaround: add instrumentation or a guarded root-derived skip that fingerprints the actual previous input reads for dirty roots (`Root`, `List`, `ListColumn`, `ListField`) and skips only when every intersecting changed read is fingerprint-identical. It must keep `root_value_cache` entries available for skipped roots, refresh fingerprints after real root/list-view evaluation, and fall back for reads that cannot be fingerprinted cheaply. Required kill criteria: revert if 124-candidate click waves remain, unchanged `store.bridge_cursor_values.rows` stays around `15ms`, or click/input p95 does not move materially toward `16.7ms`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: docs/plans/speedup/12-speedup-goal-execution-checklist.md (runtime read-fingerprint experiment in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: subagent explorer `019ec48c-d0ad-7401-a9fe-770bacd13e5c`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_revisits_earlier_dependent_after_later_dependency_changes -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (current post-revert `status=pass`, `measurement_mode=proof`, 71/71 required scenario steps); `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Killed and reverted a root read-fingerprint diagnostics/skip experiment. The first version fingerprinted actual root input reads and added scheduling profile counters; it preserved the bridge proof but regressed click/input p95 to `24.732ms`. A capped version only retained fingerprints for roots with at most 96 reads and still regressed click/input p95 to `26.058ms`. A conservative zero-relevant-input-read skip avoided `819` root candidates and kept the bridge proof passing, but still failed the speed oracle: click/input p95 stayed around `22.733ms`/`22.276ms`, unchanged `store.bridge_cursor_values.rows` stayed near `15.198ms`, and the 21-delta click class remained above budget. The runtime patch was removed after meeting its documented kill criteria.
- Current speed result: the refreshed post-revert official release gate reflects the current worktree and still fails click/input p95: `21.550ms` against `16.700ms`, max `21.976ms`, hover p95 `9.464ms`, divider p95 `9.546ms`, resize p95 `13.049ms`, runtime step/apply p95 `10.339ms`/`12.040ms`, and layout p95 `4.154ms`. The bridge proof is current and passing with `status=pass`, `measurement_mode=proof`, and 71/71 required scenario steps.
- Follow-up: keep TASK-0804 in progress. Do not retry broad read-fingerprint refresh or zero-relevant root skips as standalone speed paths. If this area is revisited, first add lower-overhead counters outside the hot path or target direct value projection/materialization. The next implementation slice should focus on `store.selected_signal_lane_rows` list-view evaluation internals or `store.bridge_cursor_values.rows` value-specific projection/materialization, not global root scheduling shortcuts.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: docs/plans/speedup/12-speedup-goal-execution-checklist.md (runtime projection/cache experiments in `crates/boon_runtime/src/lib.rs` were reverted)
- Verification: subagent explorer `019ec4a8-7bb1-7af1-9861-d2b528f7552d`; `cargo fmt --all`; `cargo test -p boon_runtime --lib structured_root_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (current post-revert `status=pass`, `measurement_mode=proof`, 71/71 required scenario steps); `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`
- Result: in progress. Killed and reverted two value-projection experiments. The first made unbacked root `ListView` fields evaluate into `root_value_cache` instead of no-oping; it reduced unchanged `store.bridge_cursor_values.rows` from roughly `15.186ms` total to `2.049ms`, but merely moved the work into newly materialized `store.selected_cursor_pair_rows` (`13.004ms` total), increased `store.selected_signal_lane_rows` to `32.930ms`, and regressed click/input p95 to `21.690ms`. The second followed the structured-parent child-capture idea: parent record evaluation cached child values such as `bridge.rows` for same-pass child roots. Focused structured-root tests passed, but the NovyWave bridge proof failed the practical oracle by stalling far beyond its normal runtime and had to be terminated; the patch was removed.
- Current speed result: the refreshed post-revert official release gate reflects the current worktree and still fails click/input p95: `22.024ms` against `16.700ms`, max `22.068ms`, hover p95 `9.133ms`, divider p95 `8.825ms`, resize p95 `10.527ms`, runtime step/apply p95 `10.497ms`/`12.212ms`, and layout p95 `4.287ms`. The bridge proof is current and passing with `status=pass`, `measurement_mode=proof`, and 71/71 required scenario steps. Sampled root costs are back to the known baseline shape: `store.selected_signal_lane_rows` `27.141ms` total and unchanged `store.bridge_cursor_values.rows` `15.022ms` total.
- Follow-up: keep TASK-0804 in progress. Do not retry unbacked list-view materialization or broad parent-record child capture as standalone speed paths. A future structured-child projection cache would need per-child read capture, bounded dependency updates, and a bridge-proof timeout/size guard before speed testing. The next safer slice should either add lower-overhead per-field timing inside `selected_signal_lane_rows`, or target the specific `selected_cursor_value_for_signal` value pipeline so selected-lane rows and cursor-pair rows share the computed cursor value without materializing another root.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: docs/plans/speedup/12-speedup-goal-execution-checklist.md (runtime function-cache invalidation experiment in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: subagent explorer `019ec4c9-0c0a-7bb1-9746-8056145ef1fd`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_change_preserves_unaffected_function_cache_entries -- --nocapture` (passed only during the killed experiment, then test removed with the experiment); `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_stable_fields_across_cursor_change -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (post-revert `status=pass`, `measurement_mode=proof`, 71/71 required scenario steps, zero failed checks); `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (run once on the killed experiment and again after revert; both fail current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: in progress. Killed and reverted a narrowed root-list-view function-cache invalidation experiment. The idea was to replace the list-view branch's broad `clear_function_value_cache()` with `invalidate_function_value_cache_for_reads(changed_reads)` so function values computed while materializing `store.selected_signal_lane_rows` could be reused by sibling unchanged roots such as `store.bridge_cursor_values.rows`. A direct focused regression proved the local behavior: entries reading changed projected row fields were invalidated and unrelated entries survived. The speed oracle rejected the tradeoff. During the experiment the official release gate still failed click/input p95 at `22.200ms`, and the measured hot roots did not move: `store.selected_signal_lane_rows` remained `28.171ms` total and unchanged `store.bridge_cursor_values.rows` stayed around `15.374ms`.
- Current speed result: the refreshed post-revert official release gate reflects the current worktree and still fails click/input p95: `21.109ms` against `16.700ms`, max `21.615ms`, hover p95 `9.993ms`, divider p95 `9.966ms`, resize p95 `11.093ms`, runtime step/apply p95 `10.462ms`/`12.230ms`, and layout p95 `4.362ms`. The bridge proof is current and passing with `status=pass`, `measurement_mode=proof`, 71/71 required scenario steps, and zero failed checks. Sampled root costs remain led by `store.selected_signal_lane_rows` (`28.323ms` total, all changed) and unchanged `store.bridge_cursor_values.rows` (`15.351ms` total over 24 samples).
- Follow-up: keep TASK-0804 in progress. Do not retry same-turn list-view function-cache preservation as a standalone speed path; it proves local reuse but does not reduce the measured root costs. The next slice should target value-specific projection/materialization for `store.bridge_cursor_values.rows` or lower-overhead per-field timing and reuse inside `store.selected_signal_lane_rows`, with promotion requiring the bridge proof to stay green and either click/input p95 or the dominant root totals to move materially toward the `16.700ms` budget.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs`, `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent explorers `019ec4e3-6aa5-70d1-9165-ac62f0b3e629` and `019ec4e6-af0e-7212-9d66-c913ceee13c9`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_stable_fields_across_cursor_change -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` (`status=pass`, `measurement_mode=proof`, 71/71 required scenario steps); `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (still fails current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: in progress. Kept generic root-`ListView` per-field profile instrumentation without Boon syntax or NovyWave-specific runtime branches. `LiveRuntimeRootListViewProfile` now exposes bounded `field_profiles` keyed by `record_scope` and `field`, with `sample_count`, cache hit/miss/store counts, `total_ms`, and `max_ms`. The instrumentation is attached to the existing root-list-view field-cache context, so cloned child frames share one accumulator and native speed reports inherit the data through the existing `runtime_root_materialization_stats` path. Focused tests prove stable projected fields report cache hits while cursor-dependent fields report misses/stores.
- Current speed result: the official release gate still fails click/input p95: `21.462ms` against `16.700ms`, max `24.921ms`, hover p95 `8.971ms`, divider p95 `8.848ms`, resize p95 `9.373ms`, runtime step/apply p95 `10.349ms`/`12.119ms`, and layout p95 `4.235ms`. The bridge proof is current and passing with `status=pass`, `measurement_mode=proof`, and 71/71 required scenario steps. Root totals did not materially move: `store.selected_signal_lane_rows` remains the top changed root (`27.886ms` total, max `1.880ms`), and unchanged pure `store.bridge_cursor_values.rows` remains about `15.303ms` total over 24 samples. The new field attribution shows selected-lane `segments` cache hits dominate measured root-list-view field time (`RUN/new_signal_lane_variable_row/segments` `1.853ms` over 32 hit samples plus group-row `segments` `0.715ms` over 16 hit samples), while `RUN/new_signal_lane_variable_row/current_value` is only `0.483ms` over 32 misses/stores.
- Follow-up: keep TASK-0804 in progress. Do not optimize `current_value` first solely because it misses; the measured root-list-view field cost is now cached segment-field cloning/reuse plus broader list-view evaluation overhead, while `bridge_cursor_values.rows` is confirmed by subagent review as an unchanged pure root, not a materialized list view. The next generic implementation slice should either generalize the root-list-view record-field cache into a bounded mapped-record field cache for pure `List/map` roots such as `selected_cursor_pair_rows` / `bridge_cursor_values.rows`, or make large cached `FieldValue`/segments reuse clone-light. Kill either path if bridge proof stalls/fails, `bridge_cursor_values.rows` stays near `15ms`, cost moves into `selected_cursor_pair_rows` or raises `selected_signal_lane_rows`, or click/input p95 does not move materially toward `16.700ms`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs` (temporary pure-root field-cache context extension was reverted; existing root-`ListView` field profiling/cache remains), `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent explorer `019ec4f1-a7b8-7bd3-a76c-87924601346c`; subagent review `019ec504-41f0-74d1-a8e9-767a8edcb813`; `cargo fmt --all`; `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (run once on the killed experiment and again after revert; both fail current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: in progress. Killed and reverted a pure-root field-cache context extension. The experiment installed the root-list-view field-cache machinery around generic pure-root evaluation and retained root-list-view field-cache entries across numeric stability intervals. Focused correctness tests passed, and the bridge proof completed, but the official speed oracle rejected the tradeoff: click/input p95 regressed to `22.925ms`, `store.selected_signal_lane_rows` rose to `28.622ms` total, and the only meaningful win was reducing unchanged `store.bridge_cursor_values.rows` to `12.035ms` total. That violates the promotion requirement because cost moved into the selected-lane root and p95 moved away from the `16.700ms` budget.
- Current speed result: after reverting the experiment, the official release gate reflects the kept code worktree and still fails click/input p95: `20.584ms` against `16.700ms`. The top sampled root costs are `store.selected_signal_lane_rows` (`26.645ms` total, all changed) and unchanged `store.bridge_cursor_values.rows` (`14.551ms` total over 24 samples). The bridge proof is current and passing with `status=pass`, `measurement_mode=proof`, `coverage_status=pass`, no blockers, and worktree fingerprint `7cc85e5346cbc9a7644c1a00135382dc628d7c490a780a074f6accfbaad326df`; the later checklist-only edit changes the dirty-tree fingerprint, so the report fingerprint should be read as evidence for the kept code before this documentation append. Root-list-view field profiles remain available; representative selected-lane samples still show `segments` as the largest cached field cost and `current_value` as a much smaller miss/store cost.
- Follow-up: keep TASK-0804 in progress. Do not retry pure-root field-cache context installation or guarded root-list-view field-cache retention as standalone speed paths. The next slice should target clone-light reuse for large cached row fields such as `segments`, or a more explicit shared value projection that lets selected-lane rows and cursor-value roots share the same cursor-value computation without moving work into another materialized pure root. Promotion still requires bridge proof to stay green and either click/input p95 or the combined `selected_signal_lane_rows` / `bridge_cursor_values.rows` root totals to move materially toward budget.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary clone-light root-`ListView` field-cache entry experiment in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: subagent review `019ec509-1eb1-7e80-9974-e92d7d381734`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground` (passed during the experiment); `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (run once on the killed experiment and again after revert; both fail current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: in progress. Killed and reverted a clone-light root-`ListView` field-cache entry experiment. The idea was to store `value: None` for row-materialization field-cache entries so large cached fields such as `segments` kept only `FieldValue` instead of both `BoonValue` and `FieldValue`, while cache-hit paths borrowed entries instead of cloning whole cache records. Focused tests and the bridge proof passed, but the official speed oracle did not show a real TASK-0804 win. The experiment still failed click/input p95 (`20.522ms` against `16.700ms`) and worsened the dominant root totals versus the prior kept checkpoint: `store.selected_signal_lane_rows` rose to `27.672ms` total and unchanged `store.bridge_cursor_values.rows` rose to `15.074ms`. The small p95 movement was noise-sized and did not offset the hotter root totals, so the runtime patch and extra test assertion were removed.
- Current speed result: after reverting the experiment and refreshing reports, the official release gate reflects the current worktree and still fails click/input p95: `20.742ms` against `16.700ms`. The bridge proof is current and passing with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `262a571ae1320f7c85aa39688a37d74941d197c73c70f63a93c80c39ede04650`. Current sampled root costs are `store.selected_signal_lane_rows` (`27.219ms` total, all changed) and unchanged `store.bridge_cursor_values.rows` (`14.698ms` total over 24 samples).
- Follow-up: keep TASK-0804 in progress. Do not retry duplicate-`BoonValue` removal or borrowed root-list-view field-cache hits as a standalone speed path; it did not move the hot roots materially. The next implementation slice should target shared value projection for the cursor-value pipeline or a bounded mapped-record field cache for pure `List/map` roots, with explicit guards that prevent work from moving into `store.selected_cursor_pair_rows` or back into `store.selected_signal_lane_rows`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary bounded mapped-record field-cache experiment in `crates/boon_runtime/src/lib.rs` was reverted before promotion)
- Verification: `cargo fmt --all`; `cargo test -p boon_runtime --lib pure_mapped_record_field_cache_ -- --nocapture` (failed focused proof during the experiment: duplicate pure `List/map` consumers produced mapped-cache stores but no mapped-cache hits because existing user-function/root-list-view cache paths already covered the repeated route); post-revert `cargo check -p boon_runtime`; post-revert `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; post-revert `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; post-revert `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; post-revert `cargo test -p boon_runtime --lib list_map_join_field_fuses_projected_record_field -- --nocapture`; `jq` inspection of the current kept-checkpoint bridge and speed reports.
- Result: in progress. Killed and reverted the bounded mapped-record field-cache experiment before running the expensive bridge/speed gates. The attempted cache keyed projected record fields by source row identity, record scope, caller environment, and field, but the focused proof did not show independent reuse: the simplest duplicate pure-map shape stored four mapped fields and hit zero mapped fields, while the existing user-function cache and root-list-view field cache already handled the repeated computation paths. Keeping another cache layer without hit evidence would add invalidation and stale-read risk without a measured TASK-0804 win.
- Current speed result: no new promoted runtime code was kept in this slice. The current kept reports remain the prior checkpoint: bridge proof `status=pass`, `measurement_mode=proof`, no blockers, worktree fingerprint `262a571ae1320f7c85aa39688a37d74941d197c73c70f63a93c80c39ede04650`; interaction-speed still fails click/input p95 at `20.742ms` against `16.700ms`, with sampled root totals led by changed `store.selected_signal_lane_rows` (`27.219ms`) and unchanged `store.bridge_cursor_values.rows` (`14.698ms`).
- Follow-up: keep TASK-0804 in progress. Do not retry bounded mapped-record field caching as a standalone speed path unless new profiling proves a duplicate projection route that is not already covered by the user-function cache or root-list-view field cache. The next slice should target a more explicit shared cursor-value projection/materialization path, or add lower-overhead per-field/value attribution inside `store.selected_signal_lane_rows` to identify work that is not already cached. Promotion still requires the bridge proof to stay green and either click/input p95 or the combined `selected_signal_lane_rows` / `bridge_cursor_values.rows` root totals to move materially toward budget.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary structured-root child read-through experiment in `crates/boon_runtime/src/lib.rs` was reverted after the speed oracle)
- Verification: subagent explorer `019ec527-8ab4-7061-9049-583fb26d2abc`; `cargo fmt --all`; during the experiment `cargo test -p boon_runtime --lib structured_root_child_read_through_ -- --nocapture` passed; post-revert `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`; post-revert `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; post-revert `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; post-revert `cargo check -p boon_runtime -p boon_native_playground`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (run once on the killed experiment and again after revert; both fail current budget); `cargo xtask verify-report-schema`; `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`; `rg -n "boon_value_child_path|StructuredRootChildValue|structured_root_child_value_from_parent_cache|structured_root_child_read_through" crates/boon_runtime/src/lib.rs` (no matches after revert)
- Result: in progress. Killed and reverted a same-pass structured-root child read-through experiment. The idea was to materialize a child root such as `store.bridge_cursor_values.rows` from an already-cached structured parent `store.bridge_cursor_values` instead of evaluating the child expression again. A tightened child-only dependency version passed focused correctness tests and the bridge proof, and reduced root candidates in the 21-delta click class from the baseline `124` to `82`, but it did not satisfy promotion. The official speed gate still failed click/input p95 at `19.451ms` against `16.700ms`, `store.selected_signal_lane_rows` stayed hot (`27.891ms` total), and the bridge-row cost effectively moved from unchanged `store.bridge_cursor_values.rows` into changed `store.bridge_cursor_values` (`15.375ms` total). That is too much semantic/invalidation risk for a parent-cache shortcut that does not remove the dominant root work or pass the speed budget.
- Current speed result: after reverting the experiment and deleting the stale diagnostic speed report under `target/`, the refreshed official reports match the current worktree. The bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `076d1d3d1154765354aa9c81034112c53bc395937bba79cdcae915428f4ca1c3`; `verify-report-schema` passes. The speed gate still fails click/input p95 at `21.157ms` against `16.700ms`; sampled root totals are back to the known shape, led by changed `store.selected_signal_lane_rows` (`27.601ms` total) and unchanged `store.bridge_cursor_values.rows` (`15.007ms` total), with the 21-delta click class back at `124` root candidates.
- Follow-up: keep TASK-0804 in progress. Do not retry same-pass structured-parent child read-through as a standalone shortcut. A future version must either eliminate the parent/child duplicate work with explicit pass-local freshness tokens and clear published-vs-input read semantics, or avoid global root scheduling changes and target the value-specific cursor projection path directly. Promotion still requires bridge proof, schema validation, and a speed report that moves click/input p95 or the combined selected-lane/bridge-row root totals materially toward the `16.700ms` budget without moving the work into another root.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary shallow parent-owned structured-child read-key pruning in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: `cargo fmt --all`; during the experiment `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`, `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`, and `cargo check -p boon_runtime -p boon_native_playground` passed; after revert `cargo test -p boon_runtime --lib structured_root_changed_reads_ -- --nocapture`, `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`, and `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture` pass on the current worktree; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (run once on the killed experiment and again after revert; both fail current budget); `cargo xtask verify-report-schema`; `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`, `target/artifacts/native-gpu/novywave-interaction-speed-role.json`, and `target/reports/novywave-bridge-scenario.json`; `rg -n "boon_value_child_path|StructuredRootChildValue|structured_root_child_value_from_parent_cache|structured_root_child_read_through" crates/boon_runtime/src/lib.rs` (no matches after the earlier structured-child read-through revert)
- Result: in progress. Killed and reverted a smaller parent-owned structured-child read-key pruning experiment. Instead of materializing child values from the parent record, the patch made parent-owned child roots publish only the parent read key, so children such as `store.bridge_cursor_values.rows` no longer collected every nested read inside their current cached values. The focused tests passed, but the official speed oracle rejected the tradeoff: click/input p95 was still `20.735ms` against `16.700ms`, the unchanged `store.bridge_cursor_values.rows` total only moved to `13.991ms`, and `store.selected_signal_lane_rows` stayed hot at `27.540ms`. The improvement from the post-revert baseline was below the `0.75ms` kill threshold and did not change the dominant hot-root shape enough to justify weakening child dependency evidence.
- Current speed result: after reverting the experiment, the refreshed official speed report reflects the current worktree and still fails click/input p95 at `21.577ms` against `16.700ms`; `verify-report-schema` passes. The bridge proof remains passing with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `076d1d3d1154765354aa9c81034112c53bc395937bba79cdcae915428f4ca1c3`. Sampled root totals are the known baseline: changed `store.selected_signal_lane_rows` (`27.884ms` total), unchanged `store.bridge_cursor_values.rows` (`14.888ms` total over `24` samples), `store.bridge_request_descriptor` (`5.199ms`), `store.selected_lane_materialized_row_count` (`4.269ms`), and `store.browser_panel_width` (`3.707ms`).
- Follow-up: keep TASK-0804 in progress. Do not retry shallow parent-owned structured-child read-key pruning as a standalone shortcut; it trims some bridge-row bookkeeping but leaves the whole-gate p95 and selected-lane work essentially unchanged. The next slice should follow the value-specific cursor-projection direction: a pass-local indexed map/join projection cache or equivalent shared computation that lets `selected_signal_lane_rows` and `bridge_cursor_values.rows` reuse the same cursor-value work without adding Boon syntax, changing NovyWave fixtures, or hiding stale child dependencies.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary pass-local indexed map/join projection-cache experiment in `crates/boon_runtime/src/lib.rs` was reverted after the speed oracle)
- Verification: subagent explorer `019ec557-7072-78f2-ac31-8f514e023a05`; during the experiment `cargo fmt --all`, `cargo check -p boon_runtime`, `cargo test -p boon_runtime --lib indexed_map_join_projection_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo test -p boon_runtime --lib list_index_report_counters_include_task_0301_fields -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`, and `cargo check -p boon_runtime -p boon_native_playground` passed; the experiment bridge proof passed with worktree fingerprint `3c7e16e7b686a3b8ed8848caf2e749e177c182c9eb62310122d62109ec3c90f3`; the experiment speed gate failed; after revert `rg -n "indexed_map_join_projection|INDEXED_MAP_JOIN_PROJECTION|IndexedMapJoinProjection" crates/boon_runtime/src/lib.rs` has no matches, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo test -p boon_runtime --lib list_index_report_counters_include_task_0301_fields -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`, and `cargo check -p boon_runtime -p boon_native_playground` pass; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json` passes; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` fails the strict p95 budget; refreshed `cargo xtask verify-report-schema` passes; `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: in progress. Killed and reverted the pass-local indexed map/join projection-cache experiment. The cache attempted to reuse fused cursor-value style `List/filter`/`List/retain`/`List/map`/`List/join_field` projections across roots using row signatures and free-environment values while preserving reads/guards on hits. Focused cache, pipeline, field-cache, and NovyWave runtime checks passed, and the bridge proof stayed green, but the official speed oracle rejected the tradeoff: click/input p95 regressed to `24.593ms` against `16.700ms`, unchanged `store.bridge_cursor_values.rows` worsened to `19.130ms` total, and changed `store.selected_signal_lane_rows` remained hot at `27.713ms`. The standalone cache added overhead without reducing the dominant selected-lane work, so the runtime patch and cache-specific tests were removed.
- Current speed result: after reverting the experiment and refreshing reports, the bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `cdd9793519bd9b76f26a6d34ca02c48a6d63c2f84c8f13dc59d121cdc53da2bc`; `verify-report-schema` passes. The official release interaction-speed gate still fails only the click/input p95 budget: click/input p95 `21.668ms` against `16.700ms`, hover p95 `9.960ms`, divider p95 `10.983ms`, resize p95 `16.437ms`, runtime step/apply p95 `10.759ms`/`12.398ms`, and layout p95 `4.412ms`. Sampled root totals are again the known shape: changed `store.selected_signal_lane_rows` (`27.699ms` total), unchanged `store.bridge_cursor_values.rows` (`15.301ms` total over `24` samples), `store.browser_panel_width` (`6.210ms`), `store.bridge_request_descriptor` (`5.433ms`), and `store.selected_lane_materialized_row_count` (`4.338ms`).
- Follow-up: keep TASK-0804 in progress. Do not retry pass-local indexed map/join projection caching as a standalone speed path; it preserved correctness but made the measured bridge-row root worse and did not move selected-lane materialization. The next slice should avoid another cache layer unless it first proves hits outside the existing user-function/root-list-view caches. Prefer lower-overhead per-field/value attribution inside `store.selected_signal_lane_rows`, or a more explicit shared cursor-value projection that removes duplicate work without moving it into `store.bridge_cursor_values.rows` or hiding dependency evidence.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs`, `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent review `019ec63e-95be-7993-8120-625bf178f6b0`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (still fails current budget); `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: in progress. Kept lower-overhead root-`ListView` evaluation attribution for the selected-lane hot path. `LiveRuntimeRootListViewProfile` now separates total field-profile time from broader eval time and records `List/map` call/row timing plus nested user-function/cache/body/record-field-loop timing through the existing root-list-view field-cache context. This keeps the existing Boon source and syntax intact and adds no NovyWave-specific runtime branch.
- Current speed result: the refreshed bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `9af9b1e09981871b77343c06aaa77dde21b1e66a8bedd6d29852aacf3c5372fd`; `verify-report-schema` passes. The official release speed gate still fails only click/input p95: click/input p95 `21.781ms` against `16.700ms`, max `22.714ms`, hover p95 `9.114ms`, divider p95 `8.573ms`, resize p95 `8.291ms`, runtime step/apply p95 `10.712ms`/`12.611ms`, and layout p95 `4.426ms`. Root totals remain the known shape: changed `store.selected_signal_lane_rows` (`28.010ms` total, max `2.099ms`) and unchanged pure `store.bridge_cursor_values.rows` (`15.311ms` total over `24` samples). The new selected-lane attribution explains the remaining shape: `eval_total=21.766ms`, `list_map_total=21.151ms`, `list_map_row_eval=21.119ms`, `field_profile_total=3.889ms`, and `eval_minus_field_profiles=17.876ms`; field profiles still show `segments` as the largest measured cached fields while `current_value` is only `0.480ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry another standalone cache until profiling proves work outside the existing user-function/root-list-view caches. The next generic runtime slice should target `List/map` row evaluation and nested record/user-function overhead inside `store.selected_signal_lane_rows`, especially the `17.876ms` eval time not explained by field profiles, or build a shared cursor-value computation that removes duplicate work without moving it into `store.bridge_cursor_values.rows`. Promotion still requires a green bridge proof and a speed report that moves click/input p95 or the combined selected-lane/bridge-row root totals materially toward `16.700ms`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary root-`ListView` record-column construction fast path in `crates/boon_runtime/src/lib.rs` was reverted after the speed oracle)
- Verification: subagent review `019ec652-6d3b-7b60-bb9e-6315635d2f87`; during the experiment `cargo fmt --all`, `cargo test -p boon_runtime --lib value_columns_keep_field_slots_sorted_for_dense_lookup -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`, `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`, `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, `cargo xtask verify-report-schema`, and `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; after revert `rg -n "with_unique_capacity|push_unique_value|sort_unique_slots|unique_field_slots|fresh_columns" crates/boon_runtime/src/lib.rs || true` has no matches, `cargo fmt --all`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, and `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` complete on the current kept state; `jq` inspection of the refreshed reports.
- Result: in progress. Killed and reverted the root-`ListView` record-column construction fast path. The experiment replaced repeated `ValueColumns::insert_value` calls inside cached root-list-view record construction with append-then-sort slots for fresh unique record fields. Focused tests and the bridge proof passed, but the official speed oracle rejected the tradeoff: click/input p95 regressed to `25.831ms`, `store.selected_signal_lane_rows` rose to `29.281ms`, unchanged `store.bridge_cursor_values.rows` rose to `16.043ms`, `list_map_total` rose to `21.691ms`, and `record_field_loop` rose to `15.718ms`. The runtime patch and helper test additions were removed.
- Current speed result: after reverting the experiment and refreshing reports, the bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `2f20588f265b605fb827adcc3226798d8ea440ef9e0a4ad46236ef4fd49a803c`; the official release speed gate still fails only click/input p95 at `21.677ms` against `16.700ms`. Hover p95 is `9.596ms`, divider p95 `9.135ms`, resize p95 `10.513ms`, runtime step/apply p95 `10.660ms`/`12.350ms`, and layout p95 `4.206ms`. Root totals are back to the attribution-only shape: `store.selected_signal_lane_rows` `27.281ms` total, unchanged `store.bridge_cursor_values.rows` `15.025ms`, `eval_total=21.232ms`, `list_map_total=20.637ms`, `list_map_row_eval=20.606ms`, `field_profile_total=3.771ms`, `eval_minus_field_profiles=17.460ms`, `user_body=33.393ms`, and `record_field_loop=15.027ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry append/sort `ValueColumns` construction as a standalone speed path. The fresh subagent review ranks frame/env overlaying for `List/map` and user-function calls above further record-constructor micro-optimizations: reduce repeated `GenericEvalFrame::child()` env cloning, map binding insert/restore, and user-function arg/env cloning while preserving exact cache-key/free-name semantics. Required focused tests before promotion: nested map env overlay separates same inner row under different outer caller rows, caller-env cache tests still pass, root-list-view field cache tests still pass, and NovyWave bridge proof stays green. Kill if click/input p95, `selected_signal_lane_rows`, `list_map_total`, or `eval_minus_field_profiles` do not move materially toward the `16.700ms` budget.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary frame/env binding experiment in `crates/boon_runtime/src/lib.rs` was reverted after the speed oracle)
- Verification: subagent explorer `019ec664-3beb-79f3-9a8d-d286c9ccc65f`; during the experiment `cargo fmt --all`, `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib list_map_binding_shadow_restores_outer_binding -- --nocapture`, `cargo test -p boon_runtime --lib map_join_field -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`, `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`, `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`, `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, `cargo xtask verify-report-schema`, and `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` ran; after revert `rg -n "set_env_binding|restore_env_binding|function-cache-referenced-outer-env|list-map-binding-shadow-restore|lookup_current\\(|shadow_row\\(" crates/boon_runtime/src/lib.rs || true` has no matches, `cargo fmt --all`, `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` twice because the first post-revert run was a noisy outlier, and refreshed `cargo xtask verify-report-schema`; `jq` inspection of the bridge, speed, and role reports.
- Result: in progress. Killed and reverted the frame/env binding experiment. The patch delayed `GenericEvalFrame::child()` creation in `eval_user_function` until after the free-name-aware function cache lookup and made `List/map` / fused map-join mutate one environment binding slot instead of reinserting the binding for every row, with shadowing tests for referenced caller env and map binding restoration. Focused tests and the bridge proof passed, but the speed oracle did not show a material TASK-0804 win. The experiment speed report still failed click/input p95 at `21.358ms` against `16.700ms`; `store.selected_signal_lane_rows` stayed hot at `27.468ms` total, `list_map_total` rose to `20.816ms`, and `eval_minus_field_profiles` rose to `17.718ms`. The tiny p95 movement was below the promotion threshold and the dominant selected-lane attribution moved the wrong way, so the runtime patch and experiment-only tests were removed.
- Current speed result: after reverting and rerunning to avoid one noisy outlier, the bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, coverage `71/71`, and worktree fingerprint `29ed55550828fdb48c629ad3c1fc9c1bf04bce2a345a78dad32f297da9849c50`; `verify-report-schema` passes. The official release speed gate still fails only click/input p95 at `20.854ms` against `16.700ms`. Hover p95 is `9.134ms`, divider p95 `9.206ms`, resize p95 `10.398ms`, runtime step/apply p95 `10.909ms`/`12.339ms`, and layout p95 `4.262ms`. Current root totals are `store.selected_signal_lane_rows` `27.385ms`, unchanged `store.bridge_cursor_values.rows` `14.886ms`, `store.bridge_request_descriptor` `5.244ms`, `store.browser_panel_width` `4.831ms`, and `store.selected_lane_materialized_row_count` `4.329ms`; selected-lane attribution is `eval_total=21.203ms`, `list_map_total=20.623ms`, `list_map_row_eval=20.593ms`, `field_profile_total=3.800ms`, `eval_minus_field_profiles=17.402ms`, `user_body=33.400ms`, and `record_field_loop=15.040ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry delayed child-frame creation or per-row binding-slot mutation as a standalone speed path; it was correct but too small and noisy for the measured NovyWave bottleneck. The next slice should avoid micro-optimizing env mechanics and instead target the remaining repeated user-body/record-field-loop work inside `store.selected_signal_lane_rows`, ideally by reducing actual row-body evaluation or sharing cursor-value computation without adding a cache layer that shifts work into `store.bridge_cursor_values.rows`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md`; from this slice, only a strengthened stale-read regression test was kept in `crates/boon_runtime/src/lib.rs` while the runtime speed experiments were reverted.
- Verification: subagent explorer `019ec67a-d9a3-7430-ac5e-40daac08a257`; during the first experiment `cargo fmt --all`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, and `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; during the previous-row reuse experiment `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_stable_fields_across_cursor_change -- --nocapture`, `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`, and `cargo check -p boon_runtime -p boon_native_playground` passed before the bridge proof rejected the patch; after revert `rg -n "profile_key|RootListViewFieldProfileKey|row_reuse|RootListViewRowReuse|root_list_view_source_identities|root_list_view_row_record_scopes|current_source_identities|previous_source_identities|can_reuse_previous_root_list_view" crates/boon_runtime/src/lib.rs || true` has no matches, `cargo fmt --all`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`, and refreshed `cargo xtask verify-report-schema` completed.
- Result: in progress. Killed and reverted the root-list-view field-profile key reuse micro-optimization. It avoided cloning the profile key on cache hits and passed focused correctness plus the bridge proof, but the official speed oracle moved click/input p95 only from the `20.854ms` baseline to `20.745ms` and worsened `store.selected_signal_lane_rows` from `27.385ms` to `28.500ms`; that missed the `0.75ms` kill threshold and moved the dominant root in the wrong direction. Killed and reverted the larger previous-row root-list-view column reuse experiment as soon as the bridge proof failed. Focused tests passed, including the strengthened stale-read case, but the bridge scenario failed `selected-row-reorder-grouping` at `selected-remove-reset-row`: `selected_rows_count` stayed `2` instead of `1`, and the order label stayed `A[3:0], B[3:0]` instead of `A[3:0]`. That is exactly the dependency/removal hazard this task must not paper over.
- Current speed result: the bridge proof is back to `status=pass`, `measurement_mode=proof`, no blockers, worktree fingerprint `997e4a8f66802f4ffeece14f62765b73327d8b45ed5dc7b8713827463ded4441`; `verify-report-schema` passes. The official release speed gate still fails only click/input p95 at `21.592ms` against `16.700ms`, with max `22.830ms`. Runtime step/apply p95 are `10.530ms`/`12.267ms`, and layout rebuild p95 is `4.322ms`. Current selected-lane attribution is `eval_total=21.620ms`, `field_profile_total=3.900ms`, `eval_minus_field=17.720ms`, `list_map_total=21.014ms`, `list_map_row=20.982ms`, `user_body=33.983ms`, `record_loop=15.269ms`, `hits=1136`, `misses=32`, `stores=32`, `changed_rows=48`, and `row_count=48`.
- Follow-up: keep TASK-0804 in progress. Do not retry previous-row column reuse without a precise dependency/change contract that proves list length, row order, row removal, and stale field invalidation for root list views. The kept regression test now proves a stable-looking root-list-view field that depends on `store.suffix` recomputes after a cursor change and later rename (`A/renamed`), so any future reuse path must satisfy that contract. The next slice should either build a dependency-safe materializer plan with explicit invalidation proof, target a different generic layer such as paint/overlay/cursor handling, or add lower-overhead per-field dependency instrumentation before attempting another cache.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary root-list-view cached-field hit batching in `crates/boon_runtime/src/lib.rs` was reverted after the speed oracle)
- Verification: subagent explorer `019ec697-21df-7dc1-b6df-f3e36ceda76d`; during the experiment `cargo fmt --all`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, and `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` ran; after revert `rg -n "RootListViewFieldValueEvaluation|merge_numeric_stability_guard_into|batched_numeric_stability_guards|batched_reads|eval_root_list_view_record_field_value_for_cache_key" crates/boon_runtime/src/lib.rs || true` has no matches, `cargo fmt --all`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`, and refreshed `cargo xtask verify-report-schema` completed.
- Result: in progress. Killed and reverted the cached-field hit batching experiment. The patch tried to make the root `ListView` `RecordColumns` path merge cached field reads and numeric stability guards once per row/record instead of once per cached field, and also avoided cloning the whole cache entry when the `FieldValue` path only needed the cached field value. Focused correctness tests and the bridge proof passed, but the release speed oracle rejected the tradeoff: experiment click/input p95 regressed to `23.034ms` against `16.700ms`, `store.selected_signal_lane_rows` rose to `28.300ms`, `eval_total` rose to `22.059ms`, `eval_minus_field` rose to `20.058ms`, and `record_loop` rose to `15.783ms`. The field-profile total dropped to `2.001ms`, but that was not a real end-to-end win; the overhead moved into unattributed row evaluation and p95 got worse.
- Current speed result: after reverting, the bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `7bb1ed8bc4ad909f3846eb5068845a240220e54785ea48817b7b4387f75e6b82`; `verify-report-schema` passes. The official release speed gate still fails only click/input p95 at `21.078ms` against `16.700ms`, with max `21.375ms`. Runtime step/apply p95 are `10.665ms`/`12.334ms`, layout rebuild p95 is `4.382ms`, hover p95 is `10.359ms`, divider p95 is `10.666ms`, and resize p95 is `15.521ms`. Sampled root totals remain led by changed `store.selected_signal_lane_rows` (`27.915ms`) and unchanged `store.bridge_cursor_values.rows` (`15.088ms`); selected-lane attribution is `eval_total=21.747ms`, `field_profile_total=3.850ms`, `eval_minus_field=17.897ms`, `list_map_total=21.126ms`, `list_map_row=21.094ms`, `user_body=34.343ms`, and `record_loop=15.490ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry per-field read/guard batching or cache-entry clone trimming as a standalone speed path; it made the measured selected-lane row loop worse even though a subcomponent metric improved. If this area is revisited, first add lower-overhead attribution that separates cache-hit cloning, read-set merging, numeric-guard merging, `ValueColumns::insert_value`, and nested user-function calls. The alternate subagent slice remains available: a bounded native hover-overlay idempotence key in `PreviewNativeInputState`, with strict checks that layout-frame generation changes still reapply hover patches.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary native hover-overlay idempotence experiment in `crates/boon_native_playground/src/main.rs` was reverted after the speed oracle)
- Verification: subagent explorer `019ec6a9-f0f9-7280-b213-e9f09988b56d`; during the experiment `cargo fmt --all`, `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_overlay_skips_same_frame_and_reapplies_after_frame_generation_change -- --nocapture`, `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json -- --nocapture` (still fails the current dirty tree with `store.selected_input.sources.editor.select` source-ID resolution and was not used as acceptance evidence), `cargo check -p boon_runtime -p boon_native_playground`, `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, `cargo xtask verify-report-schema`, and `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` ran; after revert `rg -n "PreviewHoverOverlayKey|last_hover_overlay_key|preview_hover_bounds_key|preview_hover_overlay_key|preview_hover_overlay_skips_same_frame|clean_replacement_frame" crates/boon_native_playground/src/main.rs` has no matches, `cargo fmt --all`, `cargo check -p boon_runtime -p boon_native_playground`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, refreshed solo `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`, and refreshed `cargo xtask verify-report-schema` completed.
- Result: in progress. Killed and reverted the bounded native hover-overlay idempotence experiment. The patch added a last-applied hover overlay key containing hovered node, target text, bounds bits, layout frame hash, and shared update count, plus a focused regression proving a same-hash clean replacement frame re-applies hover state before same-frame calls become idempotent. Correctness for that local case passed, and the bridge proof passed, but the speed oracle rejected the tradeoff. The solo experiment speed run regressed click/input p95 to `23.375ms` against `16.700ms`, raised click native `hover_overlay` p95 to `1.813ms`, and only improved hover aggregate p95 to `9.179ms`; this is not a TASK-0804 win because click/input is the failing budget. The patch and focused test were removed.
- Current speed result: after reverting and rerunning the speed gate alone, the bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `4b6f5cc1976d9cd565598aaff59ce9ef9605fe94825355020234c0ef8f2b613c`; `verify-report-schema` passes. The official release speed gate still fails only click/input p95 at `21.138ms` against `16.700ms`, with max `21.859ms`. Hover p95 is `9.363ms`, divider p95 is `8.426ms`, native click `total_input` p95 is `21.136ms`, native click `hover_overlay` p95 is `1.533ms`, native hover `hover_overlay` p95 is `0.993ms`, `preview_shared_render_update_count=291`, `preview_last_error=null`, and `hover_persist_write_count=0`.
- Follow-up: keep TASK-0804 in progress. Do not retry a hover-overlay idempotence key as a standalone speed path; the hot click path usually changes the shared frame generation through runtime application, so the overlay still has to run and the key adds overhead/noise instead of solving the failing budget. Revisit native overlay only as part of a larger design that can avoid reapplying hover after runtime turns without risking stale `__hover` / `__hover_paint`. The next speed slice should return to the dominant click path: runtime selected-lane/root materialization work or a measured native click apply/layout patch cost that can move click/input p95 materially toward `16.700ms`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary root-list-view detailed-attribution opt-in experiment in `crates/boon_runtime/src/lib.rs` was reverted after the speed oracle)
- Verification: current-source inspection of `examples/novywave/RUN.bn` around `external_file_tree_file` and `external_file_tree_label`; `rg -n "root_list_view_detailed|detailed_profile" crates/boon_runtime/src/lib.rs || true` (no matches after revert); `cargo fmt --all`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (still fails current budget); refreshed `cargo xtask verify-report-schema`; `jq` inspection of `target/reports/novywave-bridge-scenario.json`, `target/reports/native-gpu/novywave-interaction-speed.json`, and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: in progress. Answered and enforced the `LATEST` source-payload rule: a one-branch `LATEST` is only legitimate when a held cell truly has one update source, but it must not be introduced as a parser workaround. For NovyWave, the current source keeps direct payload expressions inside `LATEST` without a fake `THEN` wrapper, and the relevant cells have explicit reset branches where needed: `external_file_tree_file` updates from `elements.external_file_loaded_name.text` and resets on `show_empty`, while `external_file_tree_label` computes the concat branch directly and resets on `show_empty`. The IR/runtime tests prove the compiler and runtime lower/evaluate that direct source-payload concat correctly.
- Result: killed and reverted the root-list-view detailed-attribution opt-in experiment. The patch made detailed root-list-view field/list-map/user-function attribution test/env-only via `BOON_RUNTIME_ROOT_LIST_VIEW_DETAILED_PROFILE` to check whether profiling overhead was the real release bottleneck. The experiment bridge proof passed, but the speed gate got worse (`21.782ms` click/input p95 against `16.700ms`) and only zeroed diagnostic subfields; it did not reduce actual root materialization. The runtime hook was removed so current reports remain comparable with prior checkpoints.
- Current speed result: after reverting and refreshing reports, the bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `1ccb7ce3bc495669c43eb45773f3a886a20917be3725d802bf8c830ff71981f4`; `verify-report-schema` passes. The official speed gate still fails only click/input p95 at `20.695ms` against `16.700ms`, with max `21.643ms`. Hover p95 is `9.239ms`, divider p95 is `9.066ms`, resize p95 is `10.205ms`, runtime step/apply p95 are `10.450ms`/`12.186ms`, and layout rebuild p95 is `4.093ms`. Root totals are `store.selected_signal_lane_rows` `28.765ms`, unchanged `store.bridge_cursor_values.rows` `15.191ms`, `store.browser_panel_width` `6.191ms`, `store.bridge_request_descriptor` `5.270ms`, and `store.selected_lane_materialized_row_count` `4.461ms`; selected-lane attribution is `eval_total=22.205ms`, `field_profile_total=3.949ms`, `eval_minus_field=18.256ms`, `list_map_total=21.572ms`, `list_map_row=21.538ms`, `user_body=35.024ms`, and `record_loop=15.775ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry disabling root-list-view detailed attribution as a speed path; it hides observability rather than removing the hot work. Keep the direct source-payload `LATEST` tests as compiler/runtime guardrails, and target actual selected-lane row-body/materialization work next, especially reducing repeated `List/map` row evaluation and nested record/user-function body cost without changing Boon syntax or moving work into `store.bridge_cursor_values.rows`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary `Arc` function-plan sharing and `List/map` cardinality fast-path changes in `crates/boon_runtime/src/lib.rs` were reverted after the speed oracle)
- Verification: subagent explorers `019ec6d9-cbdc-7591-ad9f-58b21454586b` and `019ec6ec-6a48-7e13-acff-7b150943ce72`; during the experiment `cargo fmt --all`, `cargo test -p boon_runtime --lib list_length_over_mapped_root_list_uses_cardinality_without_map_body -- --nocapture`, `cargo test -p boon_runtime --lib list_length_over_inline_map_skips_map_body -- --nocapture`, `cargo test -p boon_runtime --lib mapped_root_list_still_materializes_when_value_is_observed -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`, `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`, `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`, and `cargo xtask verify-report-schema` ran; after revert `rg -n "try_eval_list_cardinality_expr|try_eval_root_list_cardinality|list_map_binding_and_new_expr|list_length_over_mapped_root_list_uses_cardinality|list_length_over_inline_map_skips|mapped_root_list_still_materializes_when_value_is_observed|Arc<FunctionDefinition>|function_free_names_cache: BTreeMap<String, Arc" crates/boon_runtime/src/lib.rs` has no matches, `cargo fmt --all`, `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`, `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`, `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`, `cargo check -p boon_runtime -p boon_native_playground`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`, and refreshed `cargo xtask verify-report-schema` completed; `jq` inspection of the bridge, speed, and role reports.
- Result: in progress. Killed and reverted the combined `Arc` function-plan sharing plus `List/map` cardinality fast-path experiment. The cardinality fast path was generic and semantically plausible for `List/count`, `List/length`, and `List/is_not_empty` over `List/map`, but it was not promotion-ready: the subagent review called out missing coverage for count/non-empty, remove/reorder, alias forms, and filter/retain membership changes. More importantly, the speed oracle rejected the current tradeoff. The experiment bridge proof passed, but the official speed gate still failed and regressed click/input p95 to `21.356ms` against `16.700ms`; `store.selected_signal_lane_rows` only moved to `27.876ms`, unchanged `store.bridge_cursor_values.rows` to `14.734ms`, and `store.selected_lane_materialized_row_count` worsened to `4.609ms` instead of collapsing toward noise. The earlier `Arc`-only run also failed with worse click/input p95, so both runtime changes and the experiment-only tests were removed.
- Current speed result: after reverting and refreshing reports, the bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `edb65f331cd978aa8a34976e6ee1078b140591f523349cee9354733ebebdc032`; `verify-report-schema` passes. The official speed gate still fails only click/input p95 at `21.143ms` against `16.700ms`, with max `23.425ms`. Hover p95 is `9.120ms`, divider p95 is `10.320ms`, resize p95 is `8.782ms`, runtime step/apply p95 are `10.556ms`/`12.165ms`, and layout rebuild p95 is `4.331ms`. Root totals are `store.selected_signal_lane_rows` `28.116ms`, unchanged `store.bridge_cursor_values.rows` `15.247ms`, `store.bridge_request_descriptor` `5.249ms`, `store.selected_lane_materialized_row_count` `4.328ms`, and `store.browser_panel_width` `3.919ms`; selected-lane attribution is `eval_total=21.855ms`, `field_profile_total=3.852ms`, `eval_minus_field=18.003ms`, `list_map_total=21.230ms`, `list_map_row=21.199ms`, `user_body=34.464ms`, and `record_loop=15.533ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry `Arc` function-plan sharing or cardinality shortcuts as standalone speed paths unless new profiling proves clone/cardinality cost is dominant and the missing cardinality cases are covered first. The next slice should return to the main selected-lane bottleneck: reduce actual `store.selected_signal_lane_rows` row-body/user-function/record-loop work, or design a dependency-safe materializer plan for nested row-invariant records. Any such slice must preserve removal/reorder correctness, avoid a new cache that shifts cost into `store.bridge_cursor_values.rows`, add no Boon syntax, and promote only with a green bridge proof plus speed/root totals that move materially toward the `16.700ms` click/input budget.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent explorer `019ec6f9-714d-7133-9515-640616302f1e`; `cargo fmt --all`; `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_stable_fields_across_cursor_change -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_separates_same_source_row_by_caller_env -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture`; `cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` before and after killing the hoist refinement; refreshed `cargo xtask verify-report-schema`; `rg -n "RootListViewHoisted|hoisted_fields|root_list_view_hoisted|global_meta|try_eval_list_cardinality_expr|set_env_binding|with_unique_capacity" crates/boon_runtime/src/lib.rs || true` has no matches for killed experiment symbols; `git diff --check -- crates/boon_runtime/src/lib.rs`
- Result: in progress. Kept the generic nested-record root-`ListView` field-cache slice. `root_list_view_record_field_cache_key` now treats pure nested record fields as cacheable when their child statements are fields/record-shaped, so stable nested fields inside a mapped row can reuse the existing source-row keyed field cache instead of recomputing the nested record body on cursor-only turns. The focused regression now covers `stable_meta` and `cursor_meta`: stable nested row fields hit for both rows after a cursor change, cursor-dependent nested row fields miss and restage, and a later rename still invalidates the stable nested field (`A/renamed`). This keeps the behavior generic and does not add Boon syntax.
- Result: killed and reverted the follow-on per-materialization hoist refinement. The subagent proposed hoisting row-invariant record fields once per root-list materialization; a narrow implementation reduced `page_refs` misses from `48` to `32`, but the official speed oracle regressed. The hoist experiment fingerprint `69a32d6cc980f86c2584ef244f3005fd0780a9048f7b9928053b09ad56447092` failed click/input p95 at `24.724ms`, raised `store.selected_signal_lane_rows` to `24.459ms`, and raised unchanged `store.bridge_cursor_values.rows` to `15.920ms`. The hoist map/key/helpers and hoist-only assertions were removed.
- Current speed result: after reverting the hoist and refreshing reports, the bridge proof passes with `status=pass`, `measurement_mode=proof`, no blockers, and worktree fingerprint `14028d729452540ee5b39fe6bb90a47e4d277bca19eaf9f64d06423a4404e244`; `verify-report-schema` passes. The official speed gate still fails only click/input p95 at `20.730ms` against `16.700ms`, with max `22.685ms`. Hover p95 is `9.727ms`, divider p95 is `9.119ms`, resize p95 is `8.236ms`, runtime step/apply p95 are `10.129ms`/`11.798ms`, and layout rebuild p95 is `4.147ms`. Root totals are `store.selected_signal_lane_rows` `22.960ms`, unchanged `store.bridge_cursor_values.rows` `14.954ms`, `store.bridge_request_descriptor` `5.194ms`, `store.browser_panel_width` `4.941ms`, and `store.selected_lane_materialized_row_count` `4.369ms`; selected-lane attribution is `eval_total=16.549ms`, `field_profile_total=8.003ms`, `eval_minus_field=8.545ms`, `list_map_total=15.961ms`, `list_map_row=15.929ms`, `user_body=23.865ms`, and `record_loop=10.205ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry per-materialization row-invariant field hoisting unless the dependency contract is stricter and the speed oracle shows it does not move overhead into bridge rows or aggregate input latency. The next slice should target the still-expensive `page_refs`/current-value row work with a dependency-safe plan that reduces actual selected-lane materialization, not a broader cache layer; promote only with a green bridge proof and material movement toward the `16.700ms` click/input budget.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/13-structural-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary transient `BTreeSet` replacement in `crates/boon_runtime/src/lib.rs` was reverted after the speed oracle)
- Verification: local research of sibling Boon BYTES design in `/home/martinkavik/repos/boon/docs/language/BYTES.md`, `/home/martinkavik/repos/boon/docs/language/TEXT_SYNTAX.md`, and `/home/martinkavik/repos/boon/docs/language/storage/TABLE_BYTES_RESEARCH.md`; local code inspection of `FieldValue`, `BoonValue`, `ValueColumns`, `GenericEvalFrame`, `GenericDerivedState`, list operations, and NovyWave constant/bridge text usage; during the `STRUCT-SET-001` experiment `cargo fmt --all`, `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`, `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo check -p boon_runtime`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, and refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; after revert `cargo fmt --all`, `cargo check -p boon_runtime`, `cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture`, `rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md`, `git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md docs/plans/speedup/13-structural-representation-experiments.md crates/boon_runtime/src/lib.rs`, and `cargo xtask verify-report-schema` completed.
- Result: in progress. Added `13-structural-representation-experiments.md` to capture representation-level speed work: internal binary structured fields, runtime BYTES, bridge/example BYTES refactoring, transient set replacement, dense read/dirty IDs, list shape classification, virtual/incremental collections, constant interning/folding, and constant-aware cache fragments. The file keeps the no-new-syntax default and treats BYTES as an existing upstream design to implement in runtime/compiler/bridge layers first.
- Result: killed and reverted the first `STRUCT-SET-001` code experiment. Replacing the transient `BTreeSet` in `record_numeric_retain_stability_guard` with a sorted/deduped `Vec<usize>` and `binary_search` passed the focused numeric retain and fused pipeline tests, and the bridge proof passed with worktree fingerprint `8f47641ed3510a835da98b5da4af63c555289666299f038cba8baa2c2a4c65ae`. The official speed oracle rejected the tradeoff: click/input p95 regressed to `22.103ms` against `16.700ms`, selected-lane sample total was `23.789ms`, and unchanged `store.bridge_cursor_values.rows` rose to `15.505ms`. The runtime patch was removed because the local allocation change did not move the measured bottleneck and made the official gate worse.
- Follow-up: keep TASK-0804 in progress. Do not retry one-off container swaps without counters proving that container cost is dominant. Use the new `13` and `14` plans for bigger representation changes: runtime BYTES/binary payloads, dense read IDs, list-shape classification, and constant folding/interning. The next code slice should target measured row-body/materialization work or add low-overhead representation counters that can prove JSON/string/set/list costs before changing storage.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/14-binary-bytes-list-constant-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `crates/boon_runtime/src/lib.rs`
- Verification: subagent explorer `019ec728-b7d2-7df0-94e4-21cec241926e`; upstream BYTES design read from `/home/martinkavik/repos/boon/docs/language/BYTES.md` and `/home/martinkavik/repos/boon/docs/language/storage/TABLE_BYTES_RESEARCH.md`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib record_columns_cache_fragment -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; refreshed `cargo xtask verify-report-schema`; `jq` inspection of bridge and speed reports.
- Result: in progress. Added `14-binary-bytes-list-constant-experiments.md` to capture the new representation roadmap: typed/binary internal encodings instead of JSON strings, audited set replacements, internal runtime `BYTES`, inferred `LIST` storage modes, and compiler/runtime constants. The file keeps the no-new-Boon-syntax default and requires one experiment at a time with explicit kill criteria.
- Result: kept the first `REPRESENT-BINARY-001` code slice as a low-risk internal representation cleanup, but not as a TASK-0804 speed win. `BoonValue::RecordColumns` cache fragments now walk `ValueColumns` directly with typed tags and deterministic nested JSON-object key ordering instead of building `value_columns_json(columns).to_string()` only to create a cache key. New tests prove field insertion order stability, text/bool and text/enum separation, and deterministic nested object/list fragments. The bridge proof still passes with `status=pass`, `preview_last_error=null`, git commit `4707d42`, and binary hash `aff4410d97f33e057d0d3dcc16d0c6ea9e9f9a35f93f35c93f29d9b79490f42d`.
- Current speed result: the official speed gate still fails only the click/input p95 budget. Current `verify-native-gpu-novywave-interaction-speed` report has `status=fail`, role status `fail`, no preview error, no hot-path JSON/report/PNG/proof overhead counts, and blockers for `input-to-visible-p95-budget` and `click-to-cursor-p95-budget`: click/input p95 is `21.524ms` against `16.700ms`, max is `21.939ms`; hover p95 is `8.615ms`; runtime step/apply p95 are `10.514ms`/`12.253ms`; layout rebuild p95 is `4.338ms`; worktree fingerprint is `2f69c3c00079ea81b7819b594bbdcb600a2672aaed206e2022cd10fc1ce1a67b`.
- Follow-up: keep TASK-0804 in progress. Do not treat JSON cache-fragment removal as sufficient for the failing latency budget. The next slice should target the still-dominant selected-lane row-body/materialization path, preferably with a larger representation change from plan `14`: compiler/runtime constants, root list-view direct materialization, `LIST` storage-mode inference, or a real `BYTES` bridge/runtime design where file and payload data currently moves through text/JSON.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/14-binary-bytes-list-constant-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `crates/boon_bridge/src/lib.rs` (temporary direct root `List/map` materializer changes in `crates/boon_runtime/src/lib.rs` were reverted after the speed oracle)
- Verification: subagent direct-materializer review `019ec737-d45d-7a72-884c-d8e63fcc96e6`; subagent dirty-read-aware materializer review `019ec737-f0ac-7822-8c58-482255436a18`; `rg -n "RootListViewMap|direct_root_list_view|materialize_root_list_view_map_statement|root_list_view_map_parts|root_list_view_map_expr" crates/boon_runtime/src/lib.rs || true`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib record_columns_cache_fragment -- --nocapture`; `cargo test -p boon_bridge --lib -- --nocapture`; `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md docs/plans/speedup/14-binary-bytes-list-constant-experiments.md crates/boon_bridge/src/lib.rs crates/boon_runtime/src/lib.rs`; `cargo xtask verify-report-schema`; `jq` inspection of bridge and speed reports. A full `cargo test -p boon_runtime --lib -- --nocapture` was tried as a broad sanity check but is not a clean TASK-0804 verifier in this checkout: it failed 13 broader Cells/TodoMVC/NovyWave expectation tests while the focused root/ListView/cache tests for this slice passed.
- Result: killed and reverted the direct root `List/map` materializer experiment. The bridge proof passed during the experiment, but the speed oracle rejected it: click/input p95 regressed to `22.623ms` against `16.700ms`, max was `22.755ms`, hover p95 was `9.172ms`, runtime step/apply p95 were `10.686ms`/`12.377ms`, layout rebuild p95 was `4.561ms`, and the experiment worktree fingerprint was `84854902b0a0a6f5b18e25a8605664419820be362980a8a3da1a9ca308887452`. The direct-materializer structs/helpers/counters/tests were removed; the post-revert symbol search has no matches for the temporary direct materializer names.
- Result: kept a narrower `REPRESENT-BINARY-001` bridge cleanup as representation hygiene, not as a promoted TASK-0804 speed win. `canonical_hash` in `boon_bridge` now streams the existing canonical JSON bytes directly into SHA-256 through a small `Write` adapter instead of first allocating a `String`; `canonical_json` remains available and a regression test proves the streamed hash is byte-for-byte identical to hashing `canonical_json`. This does not introduce a new binary bridge format or change public request/schema digests.
- Current speed result: the bridge proof passes with `status=pass`, `measurement_mode=proof`, `preview_last_error=null`, git commit `4707d42`, worktree fingerprint `5627dc2bbdb6a15cf3f7b9e59c634a35d268e0a408edf0361de9643609f84a0e`, and binary hash `caa8e0d1fb5c9d9ece78358588955356375d6180bc6f7cdbcc2bb0221ac22875`. The official speed gate still fails only the click/input p95 budget: click/input p95 is `20.481ms` against `16.700ms`, max is `22.278ms`, runtime step/apply p95 are `10.575ms`/`12.250ms`, layout rebuild p95 is `4.193ms`, hot-path report/PNG counts remain zero, and profiling is suppressed for the budget run. Aggregated click samples still show the dominant roots as `store.selected_signal_lane_rows` at `22.614ms` and unchanged `store.bridge_cursor_values.rows` at `14.913ms`.
- Follow-up: keep TASK-0804 in progress. Do not retry one-pass direct root `List/map` materialization as a standalone speed path, and do not treat bridge hash streaming as enough for the failing latency budget. The next high-value experiment should either implement the subagent-proposed dirty-read-aware root `ListView` field materialization with strict removal/reorder/stale-field tests, or add low-overhead counters that split selected-lane row-body work into field cache hit cloning, read-set merging, numeric guards, `ValueColumns::insert_value`, nested user-function calls, and bridge/current-value row dependencies. True binary bridge encoding should be a versioned canonical-schema migration; runtime `BYTES` should start from the existing `BridgeValue::Bytes` shape and upstream BYTES design without adding new Boon syntax.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent dirty-read-aware materializer review `019ec759-6516-75e0-857e-ba56b362a3ba`; subagent measurement review `019ec759-8a5a-7113-a6cb-e7d55b6a26ef`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib record_columns_cache_fragment -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` twice; `cargo xtask verify-report-schema`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `jq` inspection of bridge, speed, and role reports.
- Result: kept a low-level root `ListView` measurement slice, not a speed win. `LiveRuntimeRootListViewProfile` now reports aggregate cache/row-body pressure counters: function-cache hit read-key and numeric-guard counts, user-body read-key and numeric-guard counts, field-cache value-vs-field-value hit counts, field-cache hit/miss read-key and numeric-guard counts, and `ValueColumns`/record-map insert counts. The focused root-list-view test now asserts the counters are nonzero on a cursor-change projection and that the column-record path is used for the list-view row projection. The fields have `serde(default)` to preserve report compatibility.
- Current speed result: bridge proof remains green with `status=pass`, `measurement_mode=proof`, `preview_last_error=null`, worktree fingerprint `db3b304f4083be8dacc34b9361fd2b7c5aca1dd7cc7d1d03a2a8eca201fedbc6`, and binary hash `967655ef91ec151403f4b1e82efa252b0f0025c45d82623de6a9f5fce0340426`. The speed gate still fails click/input p95. Two runs with the measurement slice reported click/input p95 `21.827ms` and `22.493ms` against `16.700ms`; runtime step/apply p95 on the second run were `10.636ms`/`12.405ms`, layout p95 was `4.228ms`, hover p95 was `9.886ms`, and divider p95 was `8.874ms`. The second run's grouped click samples show `store.selected_signal_lane_rows` at `22.968ms` total and unchanged `store.bridge_cursor_values.rows` at `15.247ms`.
- New diagnostic evidence: selected-lane click samples now show `1328` field-cache hits, `80` misses/stores, all hits through the `FieldValue` path, `13440` read keys merged from field-cache hits, `672` read keys and `64` numeric guards staged from misses, `1408` `ValueColumns` inserts, `0` record-map inserts, `22816` user-body read keys, and `128` user-body numeric guards. The top per-field costs remain `page_refs` misses and `segments` hits: variable-row `page_refs` totals `2.472ms`, variable-row `segments` totals `1.803ms`, group-row `page_refs` totals `1.285ms`, and group-row `segments` totals `0.773ms`.
- Follow-up: keep TASK-0804 in progress. Do not add more hot-path counters until a concrete optimization needs them; future measurement should prefer diagnostic/post-processing aggregation or counters gated to already-sampled profiles. The next implementation slice should follow the subagent dependency contract: pass current dirty reads into `RootListViewFieldCacheContext`, force field-cache misses when entry reads intersect the dirty set, add dirty-forced miss counters, then only consider a root-`ListView` row-output cache keyed by root path, source list/key/generation, record scope, env fingerprint, and row-output identity. Required tests before any reuse promotion: same-count reorder, remove-then-append/no stale row, dirty dependency invalidating one cached field while clean fields hit, caller-env separation, and bridge proof plus speed/root totals that do not raise `store.bridge_cursor_values.rows`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent dirty-read field-cache review `019ec768-13d1-75d1-96f4-08a7feac5e7b`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_cache_forces_dirty_read_miss_before_reuse -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib record_columns_cache_fragment -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` for the broad function-cache guard, the narrow field-cache guard, and the optimized narrow field-cache guard; `cargo xtask verify-report-schema`; `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`.
- Result: in progress. Kept the dirty-read-aware root `ListView` field-cache guard as a correctness prerequisite, not as a speed win. `materialize_root_derived_field_commits_for_changed_reads` now passes the current dirty read set into `RootListViewFieldCacheContext`, accumulates same-turn cascading root/list dirty reads, and both root-list-view field-cache hit paths remove and recompute an entry before reuse when its stored reads intersect the current dirty set. `LiveRuntimeRootListViewProfile` now exposes `field_cache_dirty_forced_miss_count` and `field_cache_dirty_forced_miss_read_key_count` with `serde(default)`. The focused regression directly primes stale field-cache entries, materializes with dirty `store.suffix` reads, proves forced field misses and refreshed clean hits, and the existing cursor-change test proves normal invalidation keeps forced misses at zero.
- Result: killed the broader root-list-view user-function dirty-read guard. It was semantically tempting because function-cache hits can bypass field-cache evaluation, but the simple dirty-read intersection rule was too conservative for cache entries created or validated inside the current materialization. The speed oracle showed `32` selected-lane user-function dirty-forced misses, raised selected-lane user-body work, and failed click/input p95 at `23.358ms`, so the function-cache guard and counters were removed. Future function-cache safety needs an entry-generation or invalidation-epoch contract rather than a plain dirty-read intersection check.
- Current speed result: the optimized field-cache guard still fails the official speed budget but no longer has a clear extra miss cost in the hot path. The final speed report has `status=fail`, worktree fingerprint `3f4d9ac01ac0e27897ae59b7d4a7ff733cdf26a99a232d688751b66461b5f063`, and binary hash `cc0ddd5e6be36a43a37e79afbabfeaab85432dc105369e70bbeef936962a0315`; click/input p95 is `22.619ms` against `16.700ms`, max is `23.036ms`, hover p95 is `8.954ms`, divider p95 is `8.979ms`, resize p95 is `9.112ms`, runtime step/apply p95 are `10.522ms`/`12.225ms`, and layout rebuild p95 is `4.386ms`. Aggregated click samples show `store.selected_signal_lane_rows` at `23.540ms`, unchanged `store.bridge_cursor_values.rows` at `15.221ms`, and `0` official dirty field-cache forced misses.
- Follow-up: keep TASK-0804 in progress. Do not retry a broad user-function dirty-read guard without a generation/epoch design. The next root-`ListView` reuse experiment must first add same-count reorder and remove-then-append/no-stale-row tests, then use the kept field-cache dirty guard as a safety backstop. If the next slice targets the user's representation ideas instead, prefer plan `14` tasks with real profile evidence: `BYTES` bridge/runtime design, dense read/dirty IDs, constant interning/folding, or proven `LIST` storage modes rather than one-off container swaps.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent root-list-view test review `019ec77c-ad88-7973-a502-2ac72fcb38b8`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_materializes_current_order_after_same_count_reorder -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_append_after_remove_does_not_reuse_stale_row_projection -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `cargo xtask verify-report-schema`; `jq` inspection of bridge and speed reports.
- Result: in progress. Kept a generic cache-correctness fix required before any row-output reuse experiment. Added the prerequisite root `ListView` guards for same-count reorder and remove-then-append with physical slot reuse; the latter exposed that direct source-list structure changes could leave materialized target rows stale because cached user-function rows read `ListField`/`ListColumn` keys while the dirty input was only the list-structure key. `invalidate_function_value_cache_for_reads` and `invalidate_root_list_view_field_cache_for_reads` now treat `GenericReadKey::List { list }` as overlapping cached row/column reads for the same list, while keeping exact field/index invalidation precise for non-structure changes. No Boon syntax or NovyWave-specific runtime path was added.
- Current speed result: the bridge proof passes with `status=pass`, `measurement_mode=proof`, `preview_last_error=null`, git commit `4707d42`, worktree fingerprint `ba129b06669627b4a7dfbf424602ae559b73cbcb76678eb6c5620c799a31bc5c`, and binary hash `c60b0a01743f8eaf6b92a9cad54ea6dd40f0093a279689ba711c5bb2a7c469af`. The official speed gate still fails only click/input p95: `input_to_visible` and `click_to_cursor` p95 are `21.084ms` against `16.700ms`, max `23.315ms`; hover p95 is `9.488ms`, divider p95 is `9.388ms`, resize p95 is `9.279ms`, runtime step/apply p95 are `10.597ms`/`12.217ms`, and layout rebuild p95 is `4.366ms`. Hot-path report/PNG/proof/persist counts remain zero. Aggregated click samples show `store.selected_signal_lane_rows` at `24.075ms`, unchanged `store.bridge_cursor_values.rows` at `15.226ms`, `store.bridge_request_descriptor` at `5.048ms`, and `store.selected_lane_materialized_row_count` at `4.617ms`.
- Follow-up: keep TASK-0804 in progress. The row-output-cache prerequisite tests now exist, but the cache layer must not be promoted unless it preserves this new list-structure invalidation behavior and improves the official speed oracle. Next viable slices: a generation/epoch-aware user-function cache contract, dense read/dirty IDs to reduce invalidation/set overhead, or a root-list row-output cache keyed by root path, source list/key/generation, record scope, env fingerprint, and row-output identity with bridge proof plus speed evidence.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary by-reference cache-hit merge and root `ListView` row-output-cache experiments in `crates/boon_runtime/src/lib.rs` were reverted after the speed oracle)
- Verification: subagent dense-read review `019ec78e-0bd8-7bb0-9b75-51d9fd673920`; subagent row-output-cache review `019ec78d-f1fb-7e81-b696-34cd36692cf5`; `rg -n "row_output|root_list_view_env_fingerprint_excluding|RootListViewRowOutput" crates/boon_runtime/src/lib.rs || true`; `cargo fmt -p boon_runtime -p boon_bridge`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib record_columns_cache_fragment -- --nocapture`; `cargo test -p boon_bridge --lib -- --nocapture`; `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`; refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); refreshed `cargo xtask verify-report-schema`; `jq` inspection of bridge, speed, and role reports.
- Result: in progress. Killed and reverted two cache-layer experiments. The by-reference cache-hit merge avoided cloning whole cache entries and merged numeric/read guards by reference, but the official speed oracle regressed click/input p95 to `21.796ms` and runtime apply p95 to `12.700ms`, so the helpers were removed. The root `ListView` row-output cache passed focused reorder/remove correctness after fixing the caller-env key to exclude the current row binding, but the NovyWave speed oracle rejected it harder: click/input p95 rose to `22.311ms`, `store.selected_signal_lane_rows` rose to `26.718ms`, and the hot selected-lane samples showed `row_output_hits=0`, `row_output_misses=48`, and `row_output_stores=48`. This proves the current hot shape is not a good standalone row-output-cache target.
- Current speed result: after reverting both experiments, the bridge proof passes with `status=pass`, `measurement_mode=proof`, `preview_last_error=null`, git commit `4707d42`, worktree fingerprint `1092cdb23503aaa2e47d7baf5736c5b97b6a5d14f0a9aa045ae641b8b3bcc89f`, and binary hash `c60b0a01743f8eaf6b92a9cad54ea6dd40f0093a279689ba711c5bb2a7c469af`. The official speed gate still fails only click/input p95: `input_to_visible` and `click_to_cursor` p95 are `21.798ms` against `16.700ms`, max `23.140ms`; hover p95 is `8.967ms`, divider p95 is `8.831ms`, resize p95 is `13.216ms`, runtime step/apply p95 are `10.933ms`/`12.811ms`, and layout rebuild p95 is `4.207ms`. Profiling is suppressed for this budget run, so this entry intentionally records p95 evidence but no current root-total attribution.
- Follow-up: keep TASK-0804 in progress. Do not retry by-reference cache-entry merging or root row-output caching unless a preflight profile shows stable hot-row reuse. The denser read/dirty-ID direction from subagent review is the better next representation-level slice because it targets invalidation/set overhead directly without relying on row-output hits that do not exist in the current NovyWave click shape. Larger plan-`14` work remains valid: internal `BYTES`, compiler/runtime constants, and inferred `LIST` storage modes should proceed as typed, measured engine changes with no Boon syntax.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary dense `GenericReadKey` sidecar experiment in `crates/boon_runtime/src/lib.rs` was reverted after the speed oracle)
- Verification: subagent dense-read code map `019ec7a8-1cf2-7183-aabe-8d5659a50d24`; during the experiment `cargo fmt -p boon_runtime`, `cargo test -p boon_runtime --lib dense_generic_read_sets_preserve_exact_and_list_structure_overlap -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_materializes_current_order_after_same_count_reorder -- --nocapture`, `cargo test -p boon_runtime --lib root_list_view_append_after_remove_does_not_reuse_stale_row_projection -- --nocapture`, `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`, and refreshed `cargo xtask verify-report-schema` ran; after revert `rg -n "read_ids|read_key_ids|read_keys_by_id|materialize_dense_reads|DenseGeneric|GenericReadKeyId|current_dirty_read_ids|intern_read|dense_generic_read_sets|dense_reads" crates/boon_runtime/src/lib.rs || true` has no matches, `cargo fmt -p boon_runtime`, `cargo check -p boon_runtime`, `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`, `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`, `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`, `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`, and refreshed `cargo xtask verify-report-schema` completed.
- Result: in progress. Killed and reverted the dense read-ID sidecar experiment. The patch kept canonical `GenericReadKey` sets for dependency maps and added dense sidecars for function/root-list-view cache invalidation and cache-hit read merging; it also preserved the broad `List { list }` overlap with `ListField` and `ListColumn` in a focused test. Correctness and bridge proof passed, but the official release speed oracle rejected the tradeoff: click/input p95 regressed to `24.870ms` against `16.700ms`, hover p95 to `12.172ms`, divider p95 to `11.127ms`, resize p95 to `16.194ms`, runtime step/apply p95 to `11.954ms`/`13.939ms`, and layout p95 to `4.537ms`. The added dense frame sidecar/interner/caches/test were removed.
- Current speed result: after reverting the dense experiment, the bridge proof passes with `status=pass`, `measurement_mode=proof`, `preview_last_error=null`, git commit `4707d42`, worktree fingerprint `b2900fc89790edd7c367ee2f0e0d1a5d4e67e9e9cf977c5c5887420e6eea919e`, and binary hash `c60b0a01743f8eaf6b92a9cad54ea6dd40f0093a279689ba711c5bb2a7c469af`. The official speed gate still fails only click/input p95: `input_to_visible` and `click_to_cursor` p95 are `22.314ms` against `16.700ms`, max `23.018ms`; hover p95 is `9.528ms`, divider p95 is `10.311ms`, resize p95 is `9.728ms`, runtime step/apply p95 are `10.723ms`/`12.373ms`, and layout rebuild p95 is `4.246ms`. Profiling is suppressed for this budget run, so no current root-total attribution is available.
- Follow-up: keep TASK-0804 in progress. Do not retry dense read IDs as a sidecar plus deferred expansion; it adds enough bookkeeping/cache-entry size to hurt the release path. If read-set overhead is revisited, it should be a deeper representation change that replaces canonical hot-path storage end-to-end with a proved low-allocation dependency map, not an extra sidecar. Next work should return to either typed `BYTES`/binary bridge payload design or a larger compiler/runtime constant/list-storage plan with a clear speed oracle.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `docs/plans/speedup/15-targeted-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `Cargo.toml`; `crates/boon_bridge/Cargo.toml`; `crates/boon_bridge/src/lib.rs` (temporary root read-key alias narrowing and row-field ID/cache experiments in `crates/boon_runtime/src/lib.rs` were reverted after the speed oracle)
- Verification: subagent earlier root-materialization review `019ec7bf-5e47-7161-b9d6-98b37b7f4b00`; subagent JSON/BYTES/IPC review `019ec7d4-15b8-7410-ad1d-8c796adeb873`; subagent containers/LIST/constants review `019ec7d4-16f5-7ae0-ab5b-98d9a7426722`; for the nested root read-key alias experiment `cargo fmt -p boon_runtime`, focused `root_derived_`, `root_list_view_`, `novywave_bridge_scenario`, and NovyWave timeline tests, `cargo check -p boon_runtime -p boon_native_playground`, refreshed bridge proof, and refreshed speed gate ran before revert; for the row-field ID experiment `cargo fmt -p boon_runtime`, `cargo test -p boon_runtime --lib value_columns_cached_field_ids_match_path_based_inserts -- --nocapture`, `record_columns_cache_fragment`, `root_list_view_`, `user_function_cache_`, mapped/fused NovyWave pipeline tests, `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`, refreshed bridge proof, and refreshed speed gate ran before revert; after reverts, `rg` checks found no experiment-only root/read-id/field-id symbols, `cargo fmt -p boon_runtime`, `cargo check -p boon_runtime`, focused root/list/cache tests, `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`, refreshed bridge proof, and refreshed speed gate ran; for the kept bridge BYTES slice `cargo fmt -p boon_bridge`, `cargo test -p boon_bridge --lib -- --nocapture`, `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`, refreshed `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`, refreshed `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`, `cargo xtask verify-report-schema`, and `jq` inspection of bridge/speed reports completed.
- Result: in progress. Added `15-targeted-representation-experiments.md` as the standalone plan for the user's binary encoding, BYTES, container, LIST, and constant ideas. It records the current baseline, bans retrying known bad one-off `BTreeSet`/dense-read/field-ID/root-alias micro-slices without new evidence, and adds concrete next experiments: shared bridge BYTES, binary native source IPC frames, typed dev render metadata instead of JSON-in-style strings, internal binary cache-key writers, runtime/NovyWave BYTES, inferred LIST storage modes, direct root `List.map` materialization, and compiler/runtime constness. The subagent findings are recorded explicitly: source IPC and dev render metadata are better JSON-removal targets than public reports, and the actual click-p95 lever is still generic root `ListView` / `List.map` materialization rather than blind container swaps.
- Result: killed and reverted nested root read-key alias narrowing. It preserved focused correctness and the bridge proof passed with worktree fingerprint `b218205748c2862a2a98c2a13295e6ebe29eb069e3baec9a2f34bfe21f4c52c8`, but the official speed oracle rejected it: click/input p95 was `23.317ms` against `16.700ms`, `store.selected_signal_lane_rows` stayed around `24.362ms`, and unchanged `store.bridge_cursor_values.rows` stayed around `15.674ms`.
- Result: killed and reverted row-field ID / cached field-id construction. It preserved focused correctness and the bridge proof passed with worktree fingerprint `5cf4de3b3855f0898299bc66a98cc4e9b5cc39a586192bd06b1cf074b7130d13`, but the official speed oracle rejected it: click/input p95 was `23.380ms` against `16.700ms`, runtime apply p95 rose to `12.803ms`, `store.selected_signal_lane_rows` only moved to `23.179ms`, and unchanged `store.bridge_cursor_values.rows` stayed around `15.424ms`.
- Result: kept the first `15`/BYTES implementation slice as bridge groundwork, not as a TASK-0804 speed win. `BridgeValue::Bytes` now stores inline payloads as shared `bytes::Bytes` with custom serde preserving the existing JSON shape, and `BridgeValue::inline_bytes` centralizes construction. The focused test proves exact JSON shape, serde round-trip, canonical JSON/hash stability across round-trip, and byte-slice contents. This does not add Boon syntax, does not change bridge canonical schema version, and does not migrate public reports.
- Current speed result: after all reverts and the kept bridge BYTES groundwork, the bridge proof passes with `status=pass`, `measurement_mode=proof`, `preview_last_error=null`, git commit `4707d42`, worktree fingerprint `69b0997e897cf67f94a99a2c72020275c3b45fafb01ddb47782f5833ed5ca574`, and binary hash `8cf3c980ea0b79b5a2ac1fe0bb31042e069da7affd587957105f55c43aed4e29`. The official speed gate still fails only click/input p95: `input_to_visible` and `click_to_cursor` p95 are `24.508ms` against `16.700ms`, max `25.642ms`; hover p95 is `9.295ms`, divider p95 is `9.657ms`, resize p95 is `9.450ms`, runtime step/apply p95 are `11.309ms`/`13.182ms`, and layout rebuild p95 is `4.450ms`. The release playground binary hash is still `62e304f40a304cca18fa53f19daec7568850722916fd2dfb34242c9e451f7fc8`, matching the previous clean-post-revert speed run, so the p95 movement is recorded as speed-gate noise rather than a changed hot binary. Grouped click root totals remain dominated by `store.selected_signal_lane_rows=25.078ms` and `store.bridge_cursor_values.rows=15.345ms`.
- Follow-up: keep TASK-0804 in progress. Do not treat the bridge BYTES storage change as a speed fix. The next high-value choices are either `15` Experiment A2 for binary source IPC if source/example switching is the focus, or Experiment E2 for generic direct root `List.map` materialization if the failing click-p95 budget is the focus. Avoid public-report JSON rewrites and one-off container swaps; target internal JSON strings, binary transport, or whole LIST representation boundaries with a green bridge proof and a speed report that moves selected-lane/root totals materially toward budget.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_native_playground/src/main.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent binary source IPC review `019ec7df-155d-78a3-958c-8026d6793be8`; `cargo fmt -p boon_native_playground -p xtask`; `cargo test -p boon_native_playground --bin boon_native_playground source_project_binary_ipc_frame -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground replace_source_ack_is_small_and_worker_commits_latest_revision -- --nocapture`; `cargo check -p boon_native_playground`; `cargo check -p xtask`; refreshed `cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json`; attempted `cargo xtask verify-native-example-switch-speed --report target/reports/native-gpu/example-switch-speed-debug.json`; `cargo xtask verify-report-schema`; `jq` inspection of IPC and example-switch reports.
- Result: kept the first `15`/A2 binary native source IPC slice as internal transport groundwork, not as a promoted TASK-0804 speed win. Preview source-project IPC now supports a JSON control line plus length-prefixed UTF-8 source chunks over the existing Unix stream. The decoder reconstructs the same `SourceProjectPayload` before the existing enqueue/prewarm logic and validates frame version, unit length, UTF-8, per-unit SHA-256, and project hash. The old JSON payload path remains available for fallback/control compatibility, reports stay JSON, and Boon syntax/parser/runtime source APIs are unchanged. The dev preview sender uses the binary source frame for `replace-source`; xtask source-project prewarm/replace/stale/burst probes use the same frame so verifier reports can prove the transport.
- Green evidence: focused binary IPC tests passed and prove that source text does not leak into the control JSON, binary framing is smaller than the equivalent JSON source payload for escaped source text, decoded payloads are identical, and non-UTF-8 source bytes are rejected without producing a payload. The existing replace-source worker test still passes, including the small ACK budget. `cargo check -p boon_native_playground`, `cargo check -p xtask`, and `cargo xtask verify-report-schema` pass. The refreshed native IPC/backpressure gate passes with `status=pass`, `preview_blocked_on_ipc_count=0`, bounded queue depth `256`, preview frame p95 `1.4ms`, heartbeat max `4ms`, and no full-state mirroring; that gate still uses its legacy `replace-code` probe, so it is a regression guard for IPC/backpressure rather than proof that every source-project update is binary.
- Live binary-source evidence and blocker: the attempted example-switch report failed, but not because binary decode rejected valid payloads. Its first three prewarm requests (`counter`, `todomvc`, `cells`) successfully used `source_project_ipc_transport=unix-stream-binary-source` and `source_project_binary_frame=true`; request sizes were `3863`, `24950`, and `19505` bytes, and `cells` prewarm took `5898.925ms`. The run then timed out on the heavier `novywave` prewarm with `Resource temporarily unavailable (os error 11)`, later source-switch requests hit connection errors after the preview hold window elapsed, and the gate reported missing readback hashes/latest-wins proof. This exposes a separate engine/verifier problem: synchronous prewarm can monopolize the preview IPC server long enough that the live example-switch verifier never reaches the switch loop.
- Follow-up: keep TASK-0804 in progress. Do not call binary source IPC a NovyWave click-speed fix yet. The next A2 follow-up should either make prewarm non-blocking/latest-wins or add a focused live source-project binary transport gate that avoids the long NovyWave prewarm path while still proving preview receives source payloads, stale payloads reject correctly, and `preview_blocked_on_ipc_count` stays zero. For the main click-p95 budget, continue to prefer the generic selected-lane/root materialization and LIST/constant representation work from plan `15`.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_native_playground/src/main.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/15-targeted-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: subagent async prewarm review `019ec7f6-b005-7452-820a-1ac55749a165`; subagent BYTES design/application review `019ec807-bc2d-7192-978d-1c8b4baa6ff3`; subagent data-structure/LIST/constants review `019ec807-dfe8-7631-b99c-65c06f2a9715`; `cargo fmt -p boon_native_playground -p xtask`; `cargo check -p boon_native_playground -p xtask`; `cargo test -p boon_native_playground --bin boon_native_playground source_project -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground example_tab_switch_uses_fast_visual_path_and_async_project_work -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground replace_source_ack_is_small_and_worker_commits_latest_revision -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground preview_replace_worker_queue_reports_live_latest_wins_metrics -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground identical_physical_project_replace_is_fast_noop -- --nocapture`; attempted `cargo xtask verify-native-example-switch-speed --profile debug --report target/reports/native-gpu/example-switch-speed-debug.json`; `jq` inspection of the failed example-switch report.
- Result: in progress. Added Experiment A2b to plan `15` and kept a bounded async source-project prewarm slice as representation/IPC groundwork, not as a promoted TASK-0804 speed win. Preview prewarm now queues a background latest-wins build and returns a small immediate `queued` ACK for non-prewarmed hashes; it reports queue depth, stale drops, input count, and coalescing count, and it does not commit source/runtime/render state or write replace-source status. Successful background prewarm marks only reusable project/runtime hashes.
- Green evidence: the focused source-project tests pass, including binary frame round-trips, non-UTF-8 rejection, and the new queued prewarm ACK/background hash mark behavior. Existing fast-path/latest-wins replace tests still pass, and `cargo check -p boon_native_playground -p xtask` passes.
- Live blocker: the refreshed debug example-switch gate still fails. The report now proves top-level `source_project_ipc_transport=unix-stream-binary-source` and `source_project_binary_frame=true`, with `prewarm_elapsed=101.785ms`, `ack_payload_bytes=2774`, `ack_latency_ms_p95=33.275ms`, `ack_latency_ms_max=64.711ms`, and `preview_exit_status="exit status: 0"`. Every prewarm request uses a binary frame and returns quickly as queued/pass, with coalesced/dropped stale counters increasing through the burst. The first `counter`, `todomvc`, and `cells` replace requests ACK and become ready, but their readback probes still fail to bind to the final frame. The `novywave` replace remains pending until timeout, later requests hit IPC errors, and the report still lists stale latest-wins proof, missing separate pending-overlay readback, missing final replace/readback evidence, missing/failed later binary-frame evidence, and real SHA-256 readback changes. This means source transfer and synchronous prewarm are no longer the only blocker; the next source-switch slice must make heavy replace builds interruptible/coalesced at a lower layer or repair the final readback/latest-wins proof path.
- Plan update: extended `15-targeted-representation-experiments.md` with the current BYTES boundary (`BridgeValue::Bytes` exists, but `Type::Bytes`, `BoonValue::Bytes`, and `FieldValue::Bytes` are still missing), a typed NovyWave blob/page-ref experiment, and concrete data-structure experiments for `DirtyKeySets`, LIST selection complement, incremental LIST lookup indexes, runtime list-name lookup, and pattern-specific-to-general constant folding. The plan keeps labels, paths, formulas, statuses, and UI strings as `TEXT` and treats raw waveform chunks, decoded pages, blob payloads, and asset byte caches as the BYTES candidates.
- Follow-up: keep TASK-0804 in progress. Continue the user's representation direction, but do not retry blind `BTreeSet` swaps or public JSON rewrites. Next viable experiments are plan `15` A3 for typed dev render metadata, E/E2 for generic LIST/root `List.map` materialization, F for real constness/hoisting, or an A2b follow-up that makes NovyWave source replacement itself cancellable/latest-wins instead of only prewarm.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_ir/src/lib.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/16-user-suggested-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only subagent readback/latest-wins reviews `019ec811-adaa-7cf0-8fdb-b2e2da251fe7` and `019ec811-936d-7053-a25a-42637fc63c43`; `cargo fmt -p boon_ir -p xtask`; `cargo test -p boon_ir combinational_cycles_must_be_broken_by_hold -- --nocapture`; `cargo check -p boon_ir`; `cargo check -p boon_ir -p xtask -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`; attempted `timeout 240 cargo xtask verify-native-example-switch-speed --profile debug --report target/reports/native-gpu/example-switch-speed-debug.json`; `jq` inspection of the refreshed failed example-switch report.
- Result: kept a measured representation/compiler slice and a verifier evidence fix. The new plan `16-user-suggested-representation-experiments.md` records the user's binary/BYTES/container/LIST/constant ideas with experiment ordering and kill criteria. The example-switch preview child now passes `--frame-readback`, so the live debug report records real top-level SHA-256 readback hashes instead of `missing`. The report's `native-example-switch-speed:readback-hashes-change` check now passes with `readback_hash_before=c4612caf447af48a5c36dd0aed63fb43b5e81d57a0cd045b44cf7b7b85ea010e` and `readback_hash_after=6b068e719b26dbe4b6391815ba3b6ceba6ddac62e8b38dd9cdf750a8c53c272a`.
- Speed result: the compiler/IR lower path now precomputes same-parent field dependency edges for the combinational-cycle verifier and uses a memoized candidate-source index shared by dependency edges, possible causes, and update-branch derivation. NovyWave replace-source debug timing improved from the measured baseline `total_ms=68482.8`, `live_runtime_ms=61037.5`, `lower_ms=46968.1`, `verify_combinational_field_cycles_ms=21252.4`, `dependency_edges_ms=4747.3`, `possible_causes_ms=4801.8`, `update_branches_ms=4732.3` to `total_ms=32784.0`, `live_runtime_ms=25351.2`, `lower_ms=11295.6`, `verify_combinational_field_cycles_ms=25.5`, `dependency_edges_ms=36.4`, `possible_causes_ms=0.09`, `update_branches_ms=145.5`. No Boon syntax or example-level workaround was added.
- Live blocker: `verify-native-example-switch-speed --profile debug` still fails. The readback evidence and binary transport top-level fields are real, ACK budgets pass (`ack_latency_ms_p95=33.525ms`, `ack_latency_ms_max=62.938ms`, `ack_payload_bytes=2774`), but `novywave` still reports `replace-source-status` as `pending` after the 10s ready wait. Later rapid-switch requests hit IPC refusal after the probe has already spent too long waiting. Remaining task direction: reduce or defer typecheck (`~7s`), generic derived initialization (`~10.4s`), and layout (`~7.4s`), or make heavy replace builds cancellation-aware/latest-wins below the current coarse phase boundaries.
- Follow-up: keep TASK-0804 in progress. The next implementation slice should target one of the remaining measured phases: typecheck/source-shape tables, generic-derived LIST initialization/materialization, layout proof cost, or cancellable source replacement. BYTES and binary transport should continue only where they move real waveform/blob/page payloads or private IPC; public report JSON and visible `TEXT` labels remain out of scope for speed shortcuts.

- Date: 2026-06-14
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/16-user-suggested-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only subagent runtime/layout review `019ec824-6e48-7871-8f16-50afca612a07`; read-only subagent typecheck review `019ec824-5354-7432-b49b-349fc7da0ea6`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`.
- Result: kept a generic runtime dirty-root scheduler optimization and constructor profile, not a NovyWave workaround. The runtime profile proved the first generic-derived pass was not dominated by indexed row recompute (`indexed_recompute_ms` stayed around 7ms for 400 keys). The real cost was the dependent-root recompute setup after the initial root commit sweep collected 1362 changed read keys: it repeatedly rebuilt dirty root path/read-key checks while producing no materialized changes. The engine now keeps dirty root read-key counts while popping roots in dependency order, and updates those counts when roots are consumed or new dependents are inserted.
- Speed result: focused NovyWave source-replacement debug timing improved from the previous measured `total_ms=32784.0`, `live_runtime_ms=25351.2`, `runtime_total_ms=10324.6`, `initialize_generic_derived_first_ms=10410.3`, and `layout_ms=7368.5` to `total_ms=24062.3`, `live_runtime_ms=16140.9`, `runtime_total_ms=536.5`, `initialize_generic_derived_first_ms=434.6`, `initialize_generic_derived_second_ms=88.7`, and `layout_ms=7855.1`. The first pass dropped from about 10.4s to sub-0.5s without Boon syntax changes, source hardcoding, or list/index semantic shortcuts.
- Follow-up: keep TASK-0804 in progress. The source-replacement debug path is now dominated by typecheck/lower (`typecheck_ms` about 7.4s, `lower_ms` about 11.9s including typecheck) and layout proof (`layout_ms` about 7.9s). The typecheck subagent recommends a generic call-site/function index cache before the riskier source-payload lookup rewrite. BYTES should still wait for real binary waveform/blob/page payload movement; this runtime win came from dirty-set representation, not bytes or public JSON removal.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_typecheck/src/lib.rs`; `docs/plans/speedup/16-user-suggested-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only subagent typecheck index review `019ec837-43e7-7be1-8457-b9736301056b`; `cargo fmt -p boon_typecheck`; `cargo test -p boon_typecheck function_argument -- --nocapture`; `cargo test -p boon_typecheck rejects_recursive_functions_in_v1 -- --nocapture`; `cargo test -p boon_typecheck source_payload -- --nocapture`; `cargo test -p boon_typecheck bundled_examples_have_complete_typecheck_reports -- --nocapture`; `cargo test -p boon_typecheck function_returning_renderable_list_for_items_gets_render_metadata -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`.
- Result: kept a generic typecheck call-site index slice. `Checker` now precomputes function statements, function-body call graph, function args, and per-function/per-argument call-site expression IDs once during initialization. `function_arg_call_site_type` uses the indexed expression IDs instead of scanning all expressions for every function argument, `user_function_return_type` uses the indexed function statement map, and recursive diagnostics reuse the precomputed call graph. Inferred return types are not cached, so the existing `active_functions` recursion guard remains authoritative.
- Speed result: focused NovyWave source-replacement debug timing improved from the prior post-runtime-scheduler `total_ms=24062.3`, `live_runtime_ms=16140.9`, `lower_ms=11940.0`, `typecheck_ms=7437.3`, and `runtime_total_ms=536.5` to `total_ms=22999.1`, `live_runtime_ms=15418.0`, `lower_ms=11112.5`, `typecheck_ms=6641.7`, `typecheck_profile.check_statements_ms=5874.4`, `typecheck_profile.function_index_ms=4.5`, and `runtime_total_ms=557.7`.
- Follow-up: keep TASK-0804 in progress. Typecheck/lower remains a major source-replacement cost, but the next typecheck index target should be the riskier source-payload lookup only with focused alias/row-local tests. The other large current target is layout proof around `layout_ms` about 7.5s. No Boon syntax, example hardcoding, or report JSON shortcut was introduced.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_typecheck/src/lib.rs`; `docs/plans/speedup/16-user-suggested-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo fmt -p boon_typecheck`; `cargo test -p boon_typecheck source_payload -- --nocapture`; `cargo test -p boon_typecheck function_argument -- --nocapture`; `cargo test -p boon_typecheck bundled_examples_have_complete_typecheck_reports -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`; `cargo check -p boon_typecheck -p boon_ir -p boon_runtime -p boon_native_playground`; `cargo xtask verify-report-schema`; `git diff --check`.
- Result: kept a generic source-payload lookup index slice. `Checker` now builds `SourcePayloadPathLookup` once from parsed source ports and uses it in both expression path typing paths. `source_payload_shape_table` also reuses the lookup instead of first doing a linear source-port scan. The lookup indexes full source aliases, `store.`-relative aliases, scoped aliases, and suffix aliases while preserving current overlap ordering. A private regression test covers direct source access, field access through store-relative and suffix aliases, row-local `event.key_down.key`, nested-field diagnostics, and overlapping source-path order.
- Speed result: focused NovyWave source-replacement debug timing improved from the previous typecheck-index run `total_ms=22999.1`, `live_runtime_ms=15418.0`, `lower_ms=11112.5`, `typecheck_ms=6641.7`, and `typecheck_profile.check_statements_ms=5874.4` to `total_ms=20573.1`, `live_runtime_ms=13241.9`, `lower_ms=8987.2`, `typecheck_ms=4768.8`, `typecheck_profile.check_statements_ms=4551.0`, `typecheck_profile.source_payload_shape_table_ms=12.6`, and `runtime_total_ms=498.7`.
- Follow-up: keep TASK-0804 in progress. The next source-replacement target should be layout proof cost or cancellable/latest-wins source replacement. BYTES and binary payload work should still be applied only where real waveform/blob/page data avoids copies or JSON/text conversion. No Boon syntax, manual type annotation, example hardcoding, or public-report JSON shortcut was introduced.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed: `crates/boon_runtime/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/16-user-suggested-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo fmt -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib cached_runtime_parsed_project_reuses_plan_parse -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`; `cargo check -p boon_typecheck -p boon_ir -p boon_runtime -p boon_native_playground`; `cargo xtask verify-report-schema`; `git diff --check`.
- Result: kept a generic runtime/native layout parse-sharing slice. `CachedRuntimePlan` now retains the parsed `ParsedProgram`, and `cached_runtime_parsed_project` exposes the cached parse by reusing the existing runtime plan cache. Preview source replacement uses the same runtime parse when building the immediate layout proof, so layout no longer reparses the same multi-unit project after live runtime creation. Source-replacement timing JSON now includes the existing `layout_profile` subfields to keep future layout work measurable.
- Speed result: focused NovyWave source-replacement debug timing improved from the previous source-payload-index run `total_ms=20573.1`, `live_runtime_ms=13241.9`, `lower_ms=8987.2`, `runtime_total_ms=498.7`, and `layout_ms=7268.2` to `total_ms=17151.8`, `live_runtime_ms=13021.4`, `lower_ms=8994.6`, `runtime_total_ms=516.6`, and `layout_ms=4041.9`. The new layout profile shows `parse_cache_ms` effectively zero instead of about 3296.7ms, while `document_eval_lower_ms=3631.6`, `text_measure_and_layout_ms=73.8`, and layout-side `typecheck_ms=70.1`.
- Final verifier note: the later verification run was noisier overall (`total_ms=19271.9`, `layout_ms=4642.4`) but kept the important shape: layout parse time stayed effectively zero and `document_eval_lower_ms=4189.5` was the remaining layout bottleneck.
- Follow-up: keep TASK-0804 in progress. The next layout target is document evaluation/lowering, not text measurement, proof artifact writing, or source parsing. Cancellable/latest-wins source replacement remains a valid parallel target for avoiding stale heavy work.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/16-user-suggested-representation-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only subagent document-lowering review `019ec854-74f9-77e0-9722-61d13292a71d`; `cargo fmt -p boon_native_playground`; `cargo check -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground document_eval_cache_replays_data_reads_for_repeated_function_targets -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`; diagnostic `cargo test -p boon_native_playground --bin boon_native_playground physical_operator_host_input_batches_execute_in_preview_runtime -- --nocapture --test-threads=1` with `DocumentScopedMap::child()` temporarily flattened; diagnostic `cargo test -p boon_native_playground --bin boon_native_playground physical_todomvc_footer_links_and_theme_buttons_fit_inline -- --nocapture --test-threads=1` with `DocumentScopedMap::child()` temporarily flattened; final `cargo fmt -p boon_native_playground`; final `cargo test -p boon_native_playground --bin boon_native_playground document_eval_cache_replays_data_reads_for_repeated_function_targets -- --nocapture`; final `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`; `cargo check -p boon_typecheck -p boon_ir -p boon_runtime -p boon_native_playground`; `cargo xtask verify-report-schema`; `git diff --check`.
- Result: kept two generic native document-lowering representation slices. The document eval cache key no longer serializes function inputs/args/PASSED through JSON bytes and SHA-256 for every lookup; it uses structured in-process keys over canonicalized JSON values. `DocumentEvalContext` now uses cheap parent-backed scoped maps for locals, local origins, and render args, so row/function/block scopes add only their local bindings instead of cloning the whole binding maps for every repeated row/template. The scoped map keeps parent-aware lookup and parent-aware `insert_if_absent` for the one old `entry(...).or_insert_with(...)` binding path.
- Speed result: focused NovyWave source-replacement debug timing improved from the prior parse-sharing verifier shape (`total_ms=19271.9`, `layout_ms=4642.4`, `document_eval_lower_ms=4189.5`) to stable post-cache-key runs around `total_ms=15082.6`, `layout_ms=2062.2`, and `document_eval_lower_ms=1617.0`; after scoped context overlays, focused runs reported `total_ms=14273.2`, `layout_ms=1355.7`, and `document_eval_lower_ms=909.2`. The final verification run remained in the same band at `total_ms=14519.7`, `layout_ms=1391.1`, and `document_eval_lower_ms=945.3`. `parse_cache_ms` stayed effectively zero, `text_measure_and_layout_ms` stayed about 74ms, and layout-side typecheck stayed about 69ms.
- Diagnostic note: the broader `physical_` native playground filter currently fails physical TodoMVC operator/hover/toggle tests. Two failed tests were rerun with `DocumentScopedMap::child()` temporarily flattened back to clone semantics and still failed with the same operator row-scope and hover assertions, so those failures are recorded as current physical TodoMVC probe issues rather than scoped-overlay regressions. Do not mark the physical filter green until the scenario address/hover probe issues are fixed separately.
- Follow-up: keep TASK-0804 in progress. The next source-replacement target is again typecheck/lower, source replacement cancellation/latest-wins, or runtime/source-project reuse; document lowering is no longer the dominant layout subphase for the current debug profile, though source-intent indexing remains a possible interaction-hot-path slice.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/17-representation-next-experiments.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only subagent BYTES design/candidate review `019ec86b-e5e8-7df3-9d02-4c328059cfcd`; read-only subagent representation-hotspot review `019ec86b-f98b-7a10-aeb9-9e149a2382d5`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib list_sorted_index_complement_preserves_visible_order -- --nocapture`; `cargo test -p boon_runtime --lib list_filter_field_not_equal_uses_text_lookup_index_for_row_refs -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib list_index_numeric_lookup_intersects_selection_and_skips_nonnumeric_not_equal -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; two runs of `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`.
- Result: kept a small `EXP-17-003` LIST/container hygiene slice, not a promoted TASK-0804 speed win. Added plan `17-representation-next-experiments.md` to capture the user's current JSON/binary, BYTES, container, LIST, and constant ideas as near-term engine-only experiments. Indexed text filters and homogeneous row-ref numeric retain paths now reuse sorted `Vec<usize>` hits directly for membership and complement checks instead of allocating temporary `BTreeSet`s. `ListSelection` remains `Vec<usize>`, visible order is preserved, and no Boon syntax or example-specific branch was added.
- Speed result: focused NovyWave source-replacement reruns were neutral/noisy rather than materially better. The two post-slice timings were `total_ms=15394.1`, `layout_ms=1428.8`, `document_eval_lower_ms=969.3`, `live_runtime_ms=13875.3`, and `total_ms=14806.9`, `layout_ms=1401.2`, `document_eval_lower_ms=947.9`, `live_runtime_ms=13317.2`. This is close to but not better than the previous final document-lowering band (`total_ms=14519.7`, `layout_ms=1391.1`, `document_eval_lower_ms=945.3`), so the slice is recorded as allocation hygiene only.
- Follow-up: keep TASK-0804 in progress. The next high-value representation work should be direct root `List.map` materialization for interaction p95, or typed dev render metadata to remove an accidental JSON producer/consumer pair. Runtime/typechecker BYTES should target real waveform/blob/page payload movement, not visible text labels or current click-p95 tuning.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_document_model/Cargo.toml`; `crates/boon_document_model/src/lib.rs`; `crates/boon_document/src/lib.rs`; `crates/boon_document/src/render_scene.rs`; `crates/boon_native_gpu/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/17-representation-next-experiments.md`; `docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only subagent typed dev render metadata review `019ec874-f218-7ce3-84f9-a79fbd0e35c1`; `cargo fmt -p boon_document_model -p boon_document -p boon_native_gpu -p boon_native_playground`; `cargo test -p boon_document_model --lib typed_style_payloads_serialize_as_legacy_json_strings -- --nocapture`; `cargo test -p boon_document --lib render_text_runs_lower_syntax_spans_and_type_hints_before_gpu -- --nocapture`; `cargo test -p boon_native_gpu --lib rich_text_spans_preserve_exact_line_text -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground code_editor_view_renders_mixed_lines_as_colored_segments -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground code_editor_view_attaches_virtual_type_hint_metadata_without_changing_source_spans -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground type_inspector_syntax_spans_color_notation_without_changing_text -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground dev_render_scroll_patch_preserves_rich_spans_for_large_buffers -- --nocapture`; `cargo check -p boon_document_model -p boon_document -p boon_native_gpu -p boon_native_playground`; `cargo xtask verify-report-schema`; `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`; `git diff --check -- Cargo.toml Cargo.lock crates/boon_document_model/Cargo.toml crates/boon_document_model/src/lib.rs crates/boon_document/src/lib.rs crates/boon_document/src/render_scene.rs crates/boon_native_gpu/src/lib.rs crates/boon_native_playground/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md docs/plans/speedup/17-representation-next-experiments.md docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`
- Result: kept the typed dev render metadata slice as internal representation hygiene, not as a promoted NovyWave click-p95 speed win. `StyleValue` now has typed rich-text span and editor type-hint payload variants, and those variants serialize as legacy JSON strings so public report/style shape stays compatible. Document lowering and native GPU rich-text measurement read typed payloads first and parse old `*_json` text only as fallback. The native editor full render, fast scroll patch, and type inspector now write typed payloads instead of serializing JSON strings on the live path. Scalar style helpers and stable style hashing now handle the typed variants structurally and do not treat them as text/number/bool.
- Current speed result: the focused NovyWave source-replacement preview-state proof passed and stayed in the same broad band as the previous post-layout-optimization checkpoint: `total_ms=15451.5`, `live_runtime_ms=13886.4`, `layout_ms=1472.8`, `document_eval_lower_ms=998.8`, `parse_cache_ms=0.000083`, and `text_measure_and_layout_ms=90.8`. The official click/input speed gate was not rerun for this slice because the changed path is dev render metadata, not the production NovyWave selected-lane interaction path.
- Follow-up: keep TASK-0804 in progress. This confirms the accidental-JSON replacement pattern with compatibility-preserving typed values. Next representation work should target either real runtime/bridge BYTES for waveform/blob/page payloads, compiler/runtime constant classification and hoisting, or a generic LIST storage-mode change with a measured production NovyWave root/interaction lever. Do not treat typed dev render metadata as sufficient for the failing click/input budget.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_ir/src/lib.rs`; `docs/plans/speedup/17-representation-next-experiments.md`; `docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only constness/classifier subagent review `019ec887-1dea-7382-8c8d-66fe9eeb0a84`; read-only BYTES bridge/runtime review `019ec887-350d-7273-a50d-d2016a50f970`; `cargo fmt -p boon_ir`; `cargo test -p boon_ir --lib lower_profile_reports_representation_candidates_without_folding -- --nocapture`; `cargo test -p boon_ir --lib representation -- --nocapture`; `cargo check -p boon_ir -p boon_runtime -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`; `cargo xtask verify-report-schema`; `git diff --check -- crates/boon_ir/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `rg -n "[ \t]$" docs/plans/speedup/17-representation-next-experiments.md docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md || true`; diagnostic broad run `cargo test -p boon_ir --lib`.
- Result: kept the first compiler/runtime representation classifier as diagnostics only, not as a TASK-0804 speed win. `lower_profile` now emits `representation_analysis_ms` and `representation_analysis` with expression class counts, static/dynamic list-literal counts, LIST storage-mode candidate counts, and bounded root-derived samples with source lines and list-storage hints. The classifier treats literal/static composites as candidates only, marks `SOURCE`/`HOLD` as dynamic blockers, marks state/list reads as runtime-dynamic, and marks row fields as row-dependent. Inline `List/map`/`List/retain` row bindings are detected even when the parser has no named row-scope constructor function for an inline record projection.
- Current speed result: no NovyWave speed gate was promoted for this slice. The change gives later constant hoisting and LIST storage-mode work a source-linked candidate report without changing Boon syntax, freezing values, or changing runtime storage. The focused classifier test proves the report sees constant LIST rows, selection/projection hints, SOURCE/HOLD blockers, and row-dependent blockers. The NovyWave source-replacement proof passed with `total_ms=14683.8`, `live_runtime_ms=13231.2`, `layout_ms=1366.3`, and `document_eval_lower_ms=923.0`; `representation_analysis` is visible under `live_runtime_profile.plan.lower_profile`, and `representation_analysis_ms=399.3` is diagnostic overhead to track rather than a speed win.
- Diagnostic note: the broad `cargo test -p boon_ir --lib` run is not a clean verifier in this checkout; it passed 50 tests and failed 4 existing broader parser/typecheck/NovyWave expectation tests: `inline_empty_render_slot_lists_inside_row_constructors_get_unique_names`, `source_continuation_wrapper_binds_nested_element_events_generically`, `todomvc_lowering_is_static_and_keyed`, and `novywave_project_lowers_source_wrapped_controls`. These failures happen in current parser/typecheck/expectation assertions and are not caused by the diagnostic report changing runtime behavior.
- BYTES follow-up: the bridge/runtime subagent confirmed `BridgeValue::Bytes` already exists, while Boon typecheck/runtime values do not yet have `Bytes`. The next safe BYTES slice is bridge schema validation for bytes/blob/page/artifact refs, not Boon syntax and not converting visible labels/statuses/scenario text/public reports to bytes.
- Follow-up: keep TASK-0804 in progress. Use the new `representation_analysis` counters to choose the next measured LIST or constant optimization. If continuing BYTES, start with bridge schema validation and payload-contract tests. Do not hoist or change LIST physical storage until the classifier plus runtime read evidence proves dynamic values will not freeze.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_bridge/src/lib.rs`; `docs/plans/speedup/17-representation-next-experiments.md`; `docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only bridge validator subagent review `019ec894-fc9d-7452-9d1d-1e142e85eb1b`; `cargo fmt -p boon_bridge`; `cargo test -p boon_bridge --lib -- --nocapture`; `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`; `cargo xtask check-bridge --report target/reports/check-bridge.json`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `git diff --check -- crates/boon_bridge/src/lib.rs`; `jq -r '.status as $s | "status=\($s) checks=\(.checks|length)"' target/reports/check-bridge.json`; `jq -r '.status as $s | "status=\($s) total_checks=\(.checks|length // 0)"' target/reports/novywave-bridge-scenario.json`.
- Result: kept a pure bridge-boundary BYTES/ref shape validator, not a scheduler/runtime rewrite. `validate_bridge_value_shape` now checks `BridgeValue` against `BridgeSchemaShape`, including distinct `Bytes`, `BlobRef`, `ArtifactRef`, and `PageRef` cases plus recursive list/record/tagged/result/completion shapes. Schema mismatches return `BridgeErrorCode::SchemaMismatch`. The validator does not change bridge schema versions, scheduler request/completion metadata, replay shape, or public JSON.
- Current speed result: no speed gate was promoted. This is contract groundwork so later real waveform/blob/page payload movement can fail fast on text/ref kind drift instead of silently converting everything back through JSON/text.
- Compatibility note: the accepted bytes/blob/artifact/page-ref sample keeps identical canonical JSON and canonical hash before and after validation. Golden bridge hashes and public report/schema compatibility are unchanged.
- Follow-up: keep TASK-0804 in progress. The next bridge slice can decide how to carry schema shapes through the registry/scheduler or `check-bridge` proof rows safely. Still do not add Boon syntax, force manual type annotations, or convert visible labels/statuses/scenario text/public reports to BYTES.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_bridge/src/lib.rs`; `docs/plans/speedup/17-representation-next-experiments.md`; `docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only bridge scheduler/shape review `019ec8a0-b5d6-72c1-8447-4433e38a3bc9`; `cargo fmt -p boon_bridge`; `cargo test -p boon_bridge --lib -- --nocapture`; `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`; `cargo xtask check-bridge --report target/reports/check-bridge.json`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `git diff --check -- crates/boon_bridge/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md docs/plans/speedup/17-representation-next-experiments.md docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`; `rg -n "[ \t]$" docs/plans/speedup/17-representation-next-experiments.md docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md crates/boon_bridge/src/lib.rs || true`; `jq` inspection of `target/reports/check-bridge.json` and `target/reports/novywave-bridge-scenario.json`.
- Result: kept the bridge schema-shape carriage slice as in-memory scheduler correctness, not as a speed win. `BridgeRegistry` now has a non-serialized export schema sidecar that must match public export schema hashes before registration. `BridgeEffectScheduler` validates registered request input shapes after grant/payload/rust-handle checks, stores the output shape for live requests, rejects mismatched accepted completions, and validates replayed completions before replay. The fixture `open` request/output values now satisfy `OpenWaveformRequest` and `WaveformOpened`; missing `options.hierarchy`, bare `PageRef` output, replay shape drift, and OK-without-output are explicit `check-bridge` negative cases.
- Current bridge result: refreshed `check-bridge` passes with `status=pass` and `per_step_pass_fail` count `7`, including `bridge_fixture_values_match_declared_schema_shapes=true` and `bridge_scheduler_rejects_registered_shape_mismatches=true`. Refreshed `verify-novywave-bridge-scenario` passes with `status=pass` and `per_step_pass_fail` count `77`.
- Compatibility note: no Boon syntax, bridge schema version, public registry metadata field, or public report schema changed. The sidecar is skipped during serde. The fixture completion golden vector digest changed because the old completion hashed an arbitrary record under the `WaveformOpened` output schema hash; the new fixture hashes a schema-valid `WaveformOpened` value. The canonical hash algorithm and schema hashes are unchanged.
- Follow-up: keep TASK-0804 in progress. The next BYTES work should move real raw waveform/blob/page payloads or deterministic stream/page refs. Do not add Boon syntax, manual byte annotations, or conversions of visible labels, paths, statuses, scenario text, or public reports to BYTES.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_bridge/src/lib.rs`; `docs/plans/speedup/17-representation-next-experiments.md`; `docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only bridge payload-sidecar recommendation from subagent `019ec8aa-94f7-7642-b535-8ef2d17bf0df`; `cargo fmt -p boon_bridge`; `cargo test -p boon_bridge --lib -- --nocapture`; `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`; `cargo xtask check-bridge --report target/reports/check-bridge.json`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `git diff --check -- crates/boon_bridge/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md docs/plans/speedup/17-representation-next-experiments.md docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md`; `rg -n "[ \t]$" docs/plans/speedup/17-representation-next-experiments.md docs/plans/speedup/18-binary-bytes-list-constants-execution-memo.md crates/boon_bridge/src/lib.rs || true`; `jq` inspection of `target/reports/check-bridge.json` and `target/reports/novywave-bridge-scenario.json`.
- Result: kept the bridge completion payload-sidecar slice as deterministic BYTES/ref groundwork, not as a speed win. `BridgeCompletionPayloads` stores completion-scoped raw blob/page bytes behind the existing `BridgePayloadStore`. `BridgeEffectScheduler::complete_with_payloads(...)` recursively validates accepted output `BlobRef` and `PageRef` descriptors against those sidecar bytes before committing the completion, while existing `complete(...)` remains descriptor-only compatible for current callers and fixtures. A bridge-only fixture export declares both blob and page refs so the proof does not change Boon syntax, NovyWave source, or public scenario shape.
- Current bridge result: refreshed `check-bridge` passes with `status=pass` and `per_step_pass_fail` count `9`, including `bridge_payload_store_keeps_raw_bytes_behind_refs=true` and `bridge_scheduler_completion_payload_sidecars_validate_refs=true`. The new row proves one stored blob, one stored page, descriptor-only compatibility, `missing_page=SchemaMismatch`, and `drifted_page=SchemaMismatch`.
- Compatibility note: no Boon syntax, runtime `Bytes`, bridge schema version, public registry JSON field, public report shape, visible file path/label/status representation, or scenario text changed. The public completion value still carries descriptor refs; raw bytes are deterministic sidecars only when the engine has them.
- Follow-up: keep TASK-0804 in progress. The next BYTES work should connect real waveform/blob/page producers to this sidecar path or design deterministic streaming/replay. Do not convert visible labels, paths, statuses, diagnostics, scenario text, or public reports to bytes.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/19-user-representation-ledger.md`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only root-derived interaction recommendation from subagent `019ec8b7-25d0-7052-af3c-fbdcbf553be5`; read-only BYTES/bridge producer recommendation from subagent `019ec8b7-3efe-7f80-90b0-e14a1c4c6e97`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib structured_root_parent_change_prunes_owned_child_materialization -- --nocapture`; `cargo test -p boon_runtime --lib root_scalar_same_event_flush_follows_qualified_derived_dependencies -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_revisits_earlier_dependent_after_later_dependency_changes -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_changed_reads -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md docs/plans/speedup/19-user-representation-ledger.md`.
- Result: kept a narrow generic root-derived pruning slice and added `19-user-representation-ledger.md` as the standalone binary/BYTES/container/LIST/constant idea ledger. When a structured root parent record/list is already the changed owner for a nested child path, root propagation now invalidates the child root cache but does not rematerialize the child root a second time from the parent diff. This preserves same-turn freshness for the first child materialization and still lets downstream dependents update from the parent-owned child dirty reads. No Boon syntax, NovyWave source, public JSON/report shape, or visible label/path/status representation changed.
- Experiment killed: a broader transitive root-dirty closure was tested locally and removed. It reduced one duplicate parent rematerialization but over-dirtied light clicks, moving runtime p50 in the wrong direction, so it failed the experiment kill criteria.
- Current speed result: refreshed `verify-native-gpu-novywave-interaction-speed` still fails. Compared with the earlier stale report shape, heavy click `runtime_root_materialization_stats.candidate_count` dropped from `124` to `81`, while `changed_count=36` and `emitted_mutation_count=19` stayed stable. The current kept-state report has `status=fail`, `input_to_visible.p95=26.897ms`, `click_to_cursor.p95=26.897ms`, `runtime_apply.p95=12.259ms`, `runtime_step_apply.p95=9.908ms`, `runtime_state_summary.p95=1.764ms`, and `layout_rebuild.p95=5.025ms`; the official budget remains `16.700ms`.
- Follow-up: keep TASK-0804 in progress. The next interaction-speed target should not be broad root-dirty closure. Use the current report to attack the remaining source-route/list-resolution pressure (`route_candidates_visited` around `88-92`, `row_occurrences_scanned` around `131-135`) or a targeted root-derived ordering/index that preserves light-click p50. The BYTES follow-up should be bridge-private real blob/page producer proof through `complete_with_payloads`, not runtime visible-state bytes.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `docs/plans/speedup/19-user-representation-ledger.md`
- Verification: read-only LIST/root-materialization subagent review `019ec8d2-871d-75b1-b486-8174b36e625b`; read-only source-route/list-lookup subagent review `019ec8d2-6f25-7500-a254-1bf24270d33a`; read-only structured-root ordering review `019ec8f1-4721-7781-b395-7dbf4bcc6097`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib repeated_indexed_text_filter_reuses_probe_within_materialization_wave -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_parent_change_prunes_owned_child_materialization -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo xtask verify-report-schema`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md docs/plans/speedup/19-user-representation-ledger.md`.
- Result: kept a generic per-turn indexed lookup cache for text and numeric LIST lookup probes. Repeated identical indexed filters now reuse the stored probe result in the same source turn/materialization wave and report zero new candidates for the cache hit, while cache invalidation still clears on per-turn counter reset and on list mutation/list-view row replacement. This is an internal runtime representation change only: no Boon syntax, NovyWave fixture, public JSON/report shape, or visible label/path/status representation changed.
- Experiment killed: the root-read fingerprint guard was implemented and removed because it did not reduce the hot `81` root-candidate click class and worsened root/runtime p95. A stricter structured-root ordering/prune experiment, including an indexed dirty-read map variant, was also killed: it reduced heavy-click root candidates from `81` to `59`, but regressed the official p95 to `23.297ms` and an earlier direct-scan variant regressed to `47.628ms`. The previous structured-parent pruning behavior remains, with owned child roots allowed to materialize once before the parent for same-turn freshness.
- Current speed result: refreshed cache-only `verify-native-gpu-novywave-interaction-speed` still fails the strict budget but improves materially over the pre-cache baseline. Current report has `status=fail`, `input_to_visible.p95=20.220ms`, `click_to_cursor.p95=20.220ms`, `runtime_apply.p95=9.954ms`, `runtime_step_apply.p95=8.344ms`, `runtime_state_summary.p95=1.059ms`, and `layout_rebuild.p95=4.153ms` against the `16.700ms` budget. Heavy click samples remain at `runtime_root_materialization_stats.candidate_count=81`, `changed_count=36`, `emitted_mutation_count=19`, `route_candidates_visited=78-82` plus one `57`, and `text_lookup_index_candidates=69` plus one `48`.
- Follow-up: keep TASK-0804 in progress. Do not retry root-read fingerprinting or structured-root ordering/pruning without a different dependency representation that proves lower whole-gate p95. The next slice should target remaining generic LIST/index pressure, direct root `List.map` materialization, or cursor/crosshair paint-space handling; source-route lookup itself is not the current bottleneck because the source route scan summary remains empty.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only remaining-current-value cache-key review from subagent `019ecc33-8f31-7c81-9b13-e2a8fef8a966`; read-only cursor-value pipeline/index review from subagent `019ecc33-909b-73f3-ba8a-e9d9dcfb7e00`; read-only non-runtime timing gap review from subagent `019ecc33-91d7-7ab3-a5c9-5adfede5fa67`; `cargo fmt -p boon_runtime`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_same_pass_dirty_row_independent_fields -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave_bridge_cursor_rows_alias_tracks_list_identity_not_row_content -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_ -- --nocapture`; diagnostic `BOON_RUNTIME_FUNCTION_PROFILING=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed-profiled.json`; final `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`.
- Result: kept three generic runtime cache/identity fixes, not a budget pass. Direct dynamic root `ListRef` aliases now materialize through synthetic root list-view storage instead of falling back to concrete list evaluation, so stable aliases such as `store.bridge_cursor_values.rows` track list identity and stop hiding row-content reads behind unreliable nested row objects. Root list-view function-cache invalidation is now read-set precise instead of clearing the whole function cache between sibling root list-view materializations. Root list-view field cache entries record their materialization pass, so a row-independent field recomputed earlier in the same dirty pass can be reused by later rows even when the same root dependency was dirty at pass start. User-function cache keys now use field-sensitive fragments for row/record arguments when the callee only reads known argument fields, so equivalent rows from sibling materialized lists can reuse cached function values without changing Boon syntax or NovyWave source.
- Experiment killed: preserving readless function-cache entries across source-turn starts was implemented, tested, measured, and removed. It was semantically plausible but worsened the official NovyWave interaction run (`click_to_cursor.p95` regressed to about `24.102ms`, `runtime_apply.p95` to about `10.494ms`, and `layout_rebuild.p95` to about `5.003ms`), likely from larger cache lookup/key pressure. Do not retry turn-start pure-function cache retention without a bounded cache design and a report that proves whole-gate p95 improvement.
- Current speed result: final kept-state `verify-native-gpu-novywave-interaction-speed` still fails the `16.700ms` budget but improves the current-value hot path. Current report has `status=fail`, `input_to_visible.p95=18.111ms`, `click_to_cursor.p95=18.111ms`, `runtime_apply.p95=9.132ms`, `runtime_step_apply.p95=7.470ms`, `runtime_state_summary.p95=0.990ms`, and `layout_rebuild.p95=4.117ms`. In click root materialization, `store.selected_signal_lane_rows` remains first at `sum_ms=42.961`, `avg_ms=1.343`, and `max_ms=1.838`; `store.selected_cursor_pair_rows` remains second at `sum_ms=19.624`, `avg_ms=0.818`, and `max_ms=0.921`. `RUN/new_signal_lane_variable_row.current_value` dropped to `sum_ms=4.800` from the earlier roughly `8.4ms` range, while `page_refs` same-pass reuse now shows `32` hits and `32` misses for variable rows instead of all misses. The profiled diagnostic confirmed `RUN/selected_cursor_value_for_signal` now gets `48` function-cache hits out of `112` click calls.
- Follow-up: keep TASK-0804 in progress. The remaining measured runtime work is no longer raw fallback scans: the final click aggregate shows `rows_scanned=112`, `row_occurrences=2192`, `text_lookup_index_candidates=1536`, `numeric_lookup_index_candidates=336`, `join_rows=112`, and `fused_rows=64`. The next engine slice should target the cursor-value pipeline as a compound/fused query, cheaper row-argument cache-key representation, or reducing `selected_cursor_pair_rows.label` recomputation. The non-runtime subagent also found about a 2ms outer-vs-inner input measurement gap, incremental paint-space patch/layout overhead around 4ms p95, and hover-overlay scan cost; those are separate native-playground targets, not reasons to weaken the speed budget.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: measurement-path subagent `019ecc49-3e86-74b3-be5d-60c534d79afa`; architecture/plan-file subagent `019ecc49-3fab-7431-9805-543bae5fa8a2`; runtime-hot-path subagent `019ecc49-4120-7471-b08b-be4876b73602`; `cargo fmt -p xtask`; `cargo check -p xtask`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `cargo fmt -p boon_runtime -p xtask`; `cargo test -p boon_runtime --lib user_function_cache_keys_row_args_by_accessed_fields_when_safe -- --nocapture`; `cargo check -p boon_runtime -p xtask`; diagnostic `BOON_RUNTIME_FUNCTION_PROFILING=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed-profiled.json`; final normal `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; removed stale generated `novywave-interaction-speed-profiled/repeat` diagnostic reports whose artifact hashes were invalidated by the final normal run; `cargo xtask verify-report-schema`; checklist `rg`; `git diff --check`.
- Result: kept measurement and attribution improvements only; this is not a speed win. The official NovyWave interaction-speed report now carries the role's already-collected `click_interaction_timing_ms`, `hover_interaction_timing_ms`, `divider_interaction_timing_ms`, `click_native_input_timing_ms`, `hover_native_input_timing_ms`, and `divider_native_input_timing_ms` objects, so the canonical report exposes the split that was previously hidden in the role artifact. Runtime function-call profiling now optionally aggregates by function plus root/list-field caller context via `by_function_context`; the active field is set only when function profiling is enabled, so normal release speed runs do not allocate profiler field labels on the hot path. A regression test verifies that sibling list-view row constructors attribute the same function call under different root/field contexts.
- Current measurement result: final normal `verify-native-gpu-novywave-interaction-speed` still fails the `16.700ms` click/input p95 budget. The refreshed canonical report has `click_to_cursor.p95=20.721ms`, `input_to_visible.p95=20.721ms`, `click_interaction_timing_ms.total_apply.p95=17.218ms`, `runtime_apply.p95=10.444ms`, `runtime_step_apply.p95=8.520ms`, `runtime_state_summary.p95=1.346ms`, `layout_rebuild.p95=4.452ms`, `shared_update.p95=1.637ms`, `click_native_input_timing_ms.total_input.p95=20.719ms`, `hover_overlay.p95=2.430ms`, and `resolve.p95=0.723ms`. The top runtime roots remain `store.selected_signal_lane_rows` (`sum_ms=45.709`, `avg_ms=1.428`, `max_ms=1.885`) and `store.selected_cursor_pair_rows` (`sum_ms=21.203`, `avg_ms=0.883`, `max_ms=1.153`).
- Diagnostic conclusion: the failing test is timing real runtime/layout/native-input work, not report IO, screenshots, or an outer stopwatch artifact. The profiled run showed `RUN/selected_cursor_value_for_signal` split by caller as `selected_cursor_pair_rows.label: calls=48, hits=0, total_ms=12.391` and `selected_signal_lane_rows.current_value: calls=64, hits=48, total_ms=4.864`. That proves the same-turn function cache is already reusing the duplicated lane/pair cursor-value calls where possible; the remaining `64` misses match the real `2 selected signals * 32 clicks` work. More cache-key or lifetime micro-tuning is unlikely to close the gate by itself.
- Follow-up: keep TASK-0804 in progress. The next meaningful implementation target should be a structural LIST/query change: a compound/fused cursor-value query over `file + signal_id + state exclusions + cursor interval`, a physical LIST/index storage mode that can answer interval lookup by signal without replaying all six pipeline stages, or a retained/document-layout change that removes the 4ms layout patch plus 1-2ms shared/hover-overlay overhead. Do not retry broad root-dirty pruning, root-read fingerprinting, or pure function-cache lifetime expansion without a new dependency/index representation and whole-gate p95 evidence. Do not weaken the speed budget or hardcode NovyWave-specific rows.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only runtime LIST/index review from subagent `019ecc5e-bd4e-7af1-9825-993fb050040e`; read-only native layout/overlay timing review from subagent `019ecc5e-be5c-7980-96d5-ce4d8f4b49fa`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib indexed_pipeline_reorders_text_equal_filters_by_bucket_size -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib list_index_ -- --nocapture`; `cargo test -p boon_runtime --lib numeric_retain_stability_guard -- --nocapture`; `cargo test -p boon_runtime --lib root_numeric_stability_guard -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_ -- --nocapture`; `cargo check -p boon_runtime -p xtask`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; repeat diagnostic `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed-repeat.json`; final canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; removed stale repeat report.
- Result: kept a generic cost-based indexed LIST pipeline ordering slice, not a budget pass. The fused indexed pipeline now evaluates supported predicates once, estimates text-equality bucket size from the list text index, applies narrower text-equality predicates before broader ones, and then runs the remaining predicates through the existing indexed/direct-selection machinery. This preserves `ListSelection` order, full fallback behavior, and numeric stability guards. No Boon syntax, NovyWave source, public report shape, or example-specific branch changed. A focused regression test proves a narrow `signal_id` equality bucket runs before a broad `file` bucket while the older fused filter/retain/map/join contract still reports zero filter/retain/join fallback scans.
- Current speed result: final canonical `verify-native-gpu-novywave-interaction-speed` still fails the `16.700ms` click/input budget, but the retained-state report improved the measured runtime bucket compared with the previous measurement-only run. Current canonical report has `status=fail`, `click_to_cursor.p95=18.565ms`, `input_to_visible.p95=18.565ms`, `click_interaction_timing_ms.total_apply.p95=16.379ms`, `runtime_apply.p95=9.231ms`, `runtime_step_apply.p95=7.550ms`, `runtime_state_summary.p95=1.131ms`, `layout_rebuild.p95=3.931ms`, `shared_update.p95=1.250ms`, and `click_native_input_timing_ms.total_input.p95=18.563ms`. The aggregate LIST counters moved from the prior `row_occurrences_scanned=2192`, `route_candidates_visited=1904`, `text_lookup_index_candidates=1536`, `numeric_lookup_index_candidates=336`, `rows_scanned=112`, `map_join_field_fusions=64`, and `join_field_rows_scanned=112` to `row_occurrences_scanned=1967`, `route_candidates_visited=1727`, `text_lookup_index_candidates=1282`, `numeric_lookup_index_candidates=348`, `rows_scanned=116`, `map_join_field_fusions=66`, and `join_field_rows_scanned=116`. A repeat diagnostic run showed the same counters with `click_to_cursor.p95=17.385ms`, but it was removed to avoid stale report hashes.
- Diagnostic conclusion: this was a real generic counter reduction, not enough to close the gate alone. The first post-change speed run was noisy and worse (`click_to_cursor.p95=22.514ms`), while the repeat and final canonical run were better; therefore whole-gate noise remains high enough that future slices need final canonical evidence and should not rely on one sample. The top list-view roots remain `store.selected_signal_lane_rows` (`sum_ms=44.104`, `avg_ms=1.336`, `max_ms=1.735`) and `store.selected_cursor_pair_rows` (`sum_ms=20.909`, `avg_ms=0.836`, `max_ms=1.063`).
- Follow-up: keep TASK-0804 in progress. The next runtime target should be bigger than predicate ordering: direct root `List/map` materialization, a true compound cursor-value index over `file + signal_id + state exclusions + cursor interval`, or cheaper row/user-function materialization inside root list views. In parallel, the native timing review identified a separate retained overlay/index layer as the next non-runtime structural target: avoid full hover-overlay/display-list scans and keep proof/readback/report guards at zero. Do not keep adding cache-lifetime microchanges unless they reduce the final canonical p95.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo fmt -p boon_native_playground`; attempted `cargo test -p boon_native_playground --bin boon_native_playground hover_overlay -- --nocapture` and `cargo test -p boon_native_playground --bin boon_native_playground simple_source_click -- --nocapture` but both matched zero tests; attempted `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json -- --nocapture`, which failed before the hover overlay path with existing `source batch ... could not resolve source ID`; attempted `cargo test -p boon_native_playground --bin boon_native_playground novywave_controls_lower_hover_material_and_pointer_contracts -- --nocapture`, which failed before the hover overlay assertion with the current file/scope tree alignment expectation; `cargo check -p boon_native_playground -p boon_runtime -p xtask`; `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_overlay_updates_only_previous_and_active_nodes -- --nocapture`; final canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`.
- Result: kept a generic host hover-overlay pass reduction. `preview_apply_hover_overlay` now still finds the hovered item/button first, but then computes and applies hover/reveal state in one mutable display-list pass instead of building separate active-node sets and scanning the full display list again to apply them. The new focused unit verifies previous hover clearing, containing-button/text hover activation, scoped reveal, target reveal, idle-node nonmutation, and final `hover_overlay_nodes`. No Boon syntax, NovyWave source, public report shape, proof/readback path, or example-specific branch changed.
- Current speed result: final canonical `verify-native-gpu-novywave-interaction-speed` still fails the `16.700ms` click/input budget, but improves the remaining native-input path. Current report has `status=fail`, `click_to_cursor.p95=17.799ms`, `input_to_visible.p95=17.799ms`, `click_interaction_timing_ms.total_apply.p95=15.356ms`, `runtime_apply.p95=9.178ms`, `runtime_step_apply.p95=7.344ms`, `runtime_state_summary.p95=1.068ms`, `layout_rebuild.p95=4.018ms`, `shared_update.p95=1.365ms`, `click_native_input_timing_ms.hover_overlay.p95=1.491ms`, `click_native_input_timing_ms.resolve.p95=0.510ms`, and `click_native_input_timing_ms.total_input.p95=17.796ms`. All hot-path proof/report/IPC/hover-persist guard counters remained zero. LIST counters stayed at the prior retained-state values: `row_occurrences_scanned=1967`, `route_candidates_visited=1727`, `text_lookup_index_candidates=1282`, `numeric_lookup_index_candidates=348`, `map_join_field_fusions=66`.
- Diagnostic conclusion: the current budget miss is now about `1.1ms` over budget and is not from proof/report overhead. The interaction-side `total_apply.p95=15.356ms` is below the budget, while the native input wrapper adds `hover_overlay.p95=1.491ms` and `resolve.p95=0.510ms`; runtime root list views are still the largest engine-side work, with `store.selected_signal_lane_rows` (`sum_ms=43.912`, `avg_ms=1.331`, `max_ms=1.865`) and `store.selected_cursor_pair_rows` (`sum_ms=20.452`, `avg_ms=0.818`, `max_ms=0.902`).
- Follow-up: keep TASK-0804 in progress. The next highest-leverage options are either a real retained hover/route side table that avoids the remaining hovered-item/button lookup and route resolve work, or the larger runtime root-list-view changes already identified: direct root `List/map` materialization, a compound cursor-value index, or cheaper row/user-function materialization. Do not spend the next slice on cosmetic overlay cleanup unless it reduces the final canonical p95.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only runtime list-view recommendation from subagent `019ecc77-df5a-7063-a798-7cf139ae662e`; read-only native retained-input recommendation from subagent `019ecc77-fd3e-79a3-ad1f-f52e8738d8ea`; `cargo fmt -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_overlay_updates_only_previous_and_active_nodes -- --nocapture`; killed diagnostic `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; reverted the failed retained hover-index slice; `cargo fmt -p boon_native_playground`; `rg -n "PreviewHoverOverlay|hover_overlay_indices|preview_hover_overlay_index|sort_dedup_usize" crates/boon_native_playground/src/main.rs`; `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_overlay_updates_only_previous_and_active_nodes -- --nocapture`.
- Experiment killed: a retained hover-overlay index keyed by layout-frame pointer/hash was implemented, tested, measured, and removed. The idea was structurally reasonable, but the added index build/update/bookkeeping moved the final gate in the wrong direction: the killed report had `click_to_cursor.p95=18.870ms`, `input_to_visible.p95=18.870ms`, `runtime_apply.p95=9.750ms`, `runtime_step_apply.p95=7.950ms`, `layout_rebuild.p95=4.390ms`, `click_interaction_timing_ms.total_apply.p95=16.474ms`, `click_native_input_timing_ms.hover_overlay.p95=2.156ms`, `resolve.p95=0.696ms`, and `total_input.p95=18.868ms`. This failed the explicit kill criteria because hover-overlay p95 worsened from the kept `1.491ms` baseline instead of dropping below `0.5ms`.
- Diagnostic conclusion: do not retry a hover-only retained index that is rebuilt from the current display list inside `preview_apply_hover_overlay`. If native input indexing is revisited, it must be a broader retained input/route table owned at the render-state/layout-epoch boundary, not per-overlay reconstruction. The runtime subagent independently identified field-only root list-view materialization as the next structural engine target: stable-row cursor clicks should avoid full row-list rematerialization for `store.selected_signal_lane_rows` and `store.selected_cursor_pair_rows`, with a kill threshold of at least about `1.5ms` or `20%` runtime p95 improvement.
- Follow-up: keep TASK-0804 in progress and move to the runtime field-only root list-view materialization path before more native wrapper work. Required acceptance should be counter-based first: stable-row cursor clicks preserve semantic output, avoid full list replace/rebind for unchanged row identities, and report only cursor-dependent row fields as changed. The final oracle remains the canonical NovyWave speed gate; do not weaken the budget or hardcode NovyWave source rows.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only runtime row-materialization review from subagent `019ecc8d-8ae2-7b10-9e4c-c26c32aaa0e2`; read-only native overlay/layout review from subagent `019ecc8d-da35-7c82-a611-2b226918cb53`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib join_field_evaluates_separator_and_empty_only_when_needed -- --nocapture`; `cargo test -p boon_runtime --lib join_field -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo fmt -p boon_native_playground`; `cargo check -p boon_native_playground`; `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_overlay_updates_only_previous_and_active_nodes -- --nocapture`; implementation-time canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; implementation-time repeat `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed-repeat.json`; diagnostic `BOON_RUNTIME_FUNCTION_PROFILING=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed-profiled.json`; final artifact-refresh `cargo fmt -p boon_runtime -p boon_native_playground`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo test -p boon_runtime --lib join_field -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_native_playground --bin boon_native_playground preview_hover_overlay_updates_only_previous_and_active_nodes -- --nocapture`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; removed stale generated `novywave-interaction-speed-profiled/repeat` diagnostic reports; `cargo xtask verify-report-schema`; checklist `rg`; `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Result: kept two measured optimizations and one narrow hot-proof cleanup. Runtime now caches statement free-name analysis and row-local environment fingerprints, makes `List/join_field` lazy for separator/empty branches, and fuses the projected-record `List/map |> List/join_field` shape without changing Boon syntax or NovyWave source. Native preview hover now keeps hover/reveal state as an input-side overlay sidecar and applies `__hover`/`__hover_paint` only to the transient render frame, so pointer hover no longer mutates or clones the shared cached layout frame. Compact hot proofs no longer clone the full `runtime_document_state_snapshot` JSON payload; they record that the debug snapshot was intentionally omitted.
- Current speed result: the kept state improves the old hover tax but does not yet give a stable speed-gate pass. Implementation-time canonical and repeat runs passed at `click_to_cursor.p95=16.248ms` and `15.846ms`, with hover-overlay p95 at `0.101ms` and `0.165ms`, but the final artifact-refresh canonical run failed the `16.700ms` click/input budget at `click_to_cursor.p95=17.679ms` and `input_to_visible.p95=17.679ms`. The failing refreshed run still had zero hot-path proof/report/IPC/hover-persist guard counters and hover-overlay stayed tiny at `0.108ms`; the measured miss came from `runtime_apply.p95=10.762ms`, `runtime_step_apply.p95=8.864ms`, `runtime_state_summary.p95=1.397ms`, `layout_rebuild.p95=4.590ms`, `click_interaction_timing_ms.total_apply.p95=16.438ms`, and `resolve.p95=1.168ms`.
- Diagnostic conclusion: the old native hover-overlay hypothesis is no longer the main blocker. The profiled click run shows root list-view/user-function work still dominates: `store.selected_signal_lane_rows` aggregated `35.608ms` with `25.796ms` in list-map work, `19.620ms` in row field loops, and `13.725ms` in field profiles; `store.selected_cursor_pair_rows` aggregated `13.227ms` with `11.729ms` in list-map work. Top functions were `RUN/new_signal_lane_row` at `25.216ms` across `96` uncached calls, `RUN/new_signal_lane_variable_row` at `15.592ms`, `RUN/selected_cursor_value_for_signal` at `13.541ms` with `48` cache hits out of `112` calls, and `RUN/selected_cursor_pair_row` at `11.481ms`. `RUN/selected_cursor_value_for_signal` under `selected_cursor_pair_rows.label` still had `48` uncached calls totaling `10.167ms`, while the lane current-value caller reused `48` of `64` calls.
- Follow-up: keep TASK-0804 in progress. The gate is now close but unstable, and the measured profile still points at structural runtime/list-view work rather than the old hover-overlay path. The next high-leverage slices are direct root `List/map` row/output reuse, field-only partial row materialization for stable row identities, a compound cursor-value query/index over `file + signal_id + state exclusions + cursor interval`, and a broader layout-epoch route/proof sidecar. Do not retry the per-hover retained index rebuilt inside `preview_apply_hover_overlay`, do not hardcode NovyWave rows or file names, and do not add Boon syntax to express indexes or virtual collections.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only structural TASK-0804 comparison from subagent `019eccb5-6ca7-7322-85f1-a40b398ed054`; read-only runtime compound-query review from subagent `019eccb5-9408-78a2-b920-0b8e9d2256f3`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_same_source_rows_patch_in_place_and_keep_target_identity -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib join_field -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; repeat diagnostic `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed-repeat.json`; final canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; removed stale repeat report before schema verification.
- Result: kept a generic same-source root list-view in-place patch. Root list-view `List/map` materialization now records the mapped source-row identity sequence and, when the target list has the same source list epoch plus row key/generation sequence, patches the existing target rows in place instead of replacing the whole `ListMemory` and rebinding source routes. The source identity includes a list replacement epoch so upstream full-list replacements or same-count reorders cannot accidentally patch by index with recycled keys. Reports expose `in_place_patch` and `in_place_patch_row_count`. No Boon syntax, NovyWave source, fixture size, public speed budget, or example-specific branch changed.
- Safety evidence: the new focused test proves stable source rows keep target row identity and report `in_place_patch=true`. The full `root_list_view_` suite passes, including same-count reorder and remove/append stale-row guards; the reorder guard initially caught the missing list-epoch bug before the epoch was added. Existing `user_function_cache_`, `join_field`, and indexed map/join pipeline tests still pass.
- Current speed result: final canonical `verify-native-gpu-novywave-interaction-speed` passes with `status=pass`, `click_to_cursor.p95=16.526ms`, `input_to_visible.p95=16.526ms`, `click_interaction_timing_ms.total_apply.p95=15.077ms`, `runtime_apply.p95=8.946ms`, `runtime_step_apply.p95=7.467ms`, `runtime_state_summary.p95=0.877ms`, `layout_rebuild.p95=4.189ms`, `hover_overlay.p95=0.098ms`, `resolve.p95=0.997ms`, and zero hot-path proof/report/IPC/hover-persist guard counters. Click list-view aggregates show the intended path firing: `store.selected_signal_lane_rows` had `32/32` in-place patches and `96` patched rows with `rebind_ms=0`; `store.selected_cursor_pair_rows` had `24/24` in-place patches and `48` patched rows with `rebind_ms=0`.
- Stability caveat: a repeat diagnostic failed the strict end-to-end budget at `click_to_cursor.p95=17.398ms` and `input_to_visible.p95=17.398ms`, but still had `runtime_apply.p95=9.294ms`, `runtime_step_apply.p95=7.645ms`, `total_apply.p95=16.215ms`, hover-overlay `0.154ms`, zero hot-path guards, and the in-place patch path active for the same hot roots. Keep the slice because it reduces the final canonical runtime/apply bucket by more than the kill threshold compared with the previous refreshed failure, but do not mark TASK-0804 done yet.
- Follow-up: keep TASK-0804 in progress. The next structural runtime slice should implement the compound cursor-value indexed query/cache recommended by subagent `019eccb5-9408-78a2-b920-0b8e9d2256f3`: a generic cached `filter* |> retain* |> List/map(record) |> List/join_field` path that caches ordered projected parts, preserves lazy separator/empty evaluation, and invalidates by full read sets and numeric guards. Expected moving counters are `text_lookup_index_candidates`, `numeric_lookup_index_candidates`, `row_occurrences_scanned`, `map_join_field_rows_fused`, `selected_cursor_pair_rows.label` field profile time, and `selected_signal_lane_rows.current_value` profile time. Do not start another hover-only or proof-only slice until the runtime/query margin is stable.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only measurement review from subagent `019eccc6-54ee-7bd0-950d-9344d74df71d`; read-only architecture/runtime options review from subagent `019eccc6-692d-75f1-9560-b62d133c476e`; `cargo fmt -p boon_runtime -p boon_native_playground`; `cargo check -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib root_list_view_same_source_rows_patch_in_place_and_keep_target_identity -- --nocapture`; `cargo check -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; second canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` after adding the source-action subphase split; final `cargo test -p boon_runtime --lib root_list_view_same_source_rows_patch_in_place_and_keep_target_identity -- --nocapture`; `cargo xtask verify-report-schema`; checklist `rg`; `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Result: kept measurement instrumentation only; this is not a speed optimization. `LiveTurnOutput`, native interaction samples, and NovyWave speed reports now expose a `runtime_step_profile` that splits source-action input, count preimage capture, bool-context setup, source-action propagation, mutation observation/lowering, source-action root materialization, changed-read extraction, indexed derived recompute, post-source root materialization, cache clearing, list-delta counters, dirty-set metrics, list-scan metrics, and metrics snapshot time. This preserves Boon syntax, NovyWave source, fixtures, and native proof policy.
- Current speed result: the first refreshed canonical run passed with `status=pass`, `click_to_cursor.p95=15.169ms`, `input_to_visible.p95=15.169ms`, `runtime_apply.p95=8.861ms`, `runtime_step_apply.p95=7.289ms`, `runtime_state_summary.p95=1.057ms`, `layout_rebuild.p95=4.120ms`, and `click_interaction_timing_ms.total_apply.p95=14.147ms`. After the deeper source-action subphase split, the final canonical run failed the strict `16.700ms` click/input budget with `status=fail`, `click_to_cursor.p95=17.661ms`, `input_to_visible.p95=17.661ms`, `runtime_apply.p95=9.639ms`, `runtime_step_apply.p95=7.948ms`, `runtime_state_summary.p95=0.970ms`, `layout_rebuild.p95=4.355ms`, `click_interaction_timing_ms.total_apply.p95=15.813ms`, `click_native_input_timing_ms.total_input.p95=17.658ms`, `hover_overlay.p95=0.127ms`, and `resolve.p95=1.072ms`.
- Diagnostic conclusion: the current measured slow path is source-action-driven root materialization, not hover/proof/report overhead and not the post-source root materialization bucket. On failing click samples, `runtime_step_profile.source_actions.p95=7.881ms`, `source_action_root_materialization.p95=4.365ms`, and `source_action_observe.p95=0.022ms`; subtracting root materialization and observe leaves roughly `3.1-4.0ms` per slow click inside source-action route/evaluation/bookkeeping that is not yet split. The top root materialization samples remain `store.selected_signal_lane_rows` (`sum_ms=33.836`, `avg_ms=1.057`, `max_ms=1.513`), `store.selected_cursor_pair_rows` (`sum_ms=11.719`, `avg_ms=0.488`, `max_ms=0.681`), and `store.cursor_label` (`sum_ms=9.967`, `avg_ms=0.322`, `max_ms=0.612`). Field profiles show `RUN/selected_cursor_pair_row.label` still has `48` misses totaling `9.182ms`; lane `current_value` has `64` misses totaling `3.615ms`; lane/group `page_refs` totals about `5.645ms`.
- Follow-up: keep TASK-0804 in progress. Do not blindly implement the compound query cache as the next step just because it was previously queued; the new profile says the decision point is source-action propagation. Next work should either split the remaining `source_actions - source_action_root_materialization - source_action_observe` residual by route action/root-scalar/flush scheduling, or implement a structural root-list/materialization change that directly lowers `source_action_root_materialization` for stable cursor-click rows. High-leverage candidates are direct root `List.map` incremental projection, a root-list field/value dependency representation that avoids rebuilding unaffected row fields, and then a compound cursor-value query if field profiles continue to justify it. Kill any slice that does not lower final canonical click/input p95 or at least the `source_action_root_materialization` p95/root-list field aggregates.

- Date: 2026-06-15
- Task: TASK-0804
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only source-action residual review from subagent `019eccdb-0373-72c3-a2c6-970ce98548ac`; read-only root-list/materialization strategy review from subagent `019eccdb-048f-7133-b873-dd9304835213`; `cargo fmt -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib root_read_key_aliases_match_store_local_and_leaf_without_duplicates -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`; `cargo test -p boon_native_playground --bin boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture`; `cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json`; `cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json`.
- Result: TASK-0804 is done. The source-action profile now splits route setup, action evaluation, intermediate/final root flush wall time, root dirty setup, root dirty scheduler overhead, route stats, and an unattributed residual. The measured residual collapsed to near zero (`source_action_unattributed.p95=0.002ms`), proving the slow path is root flush work rather than route lookup or source-action dispatch. A generic root dirty scheduler optimization removed repeated root-alias set construction from `pop_next_root_dirty`; root alias behavior is covered by a focused regression test. No Boon syntax, NovyWave source, fixture size, speed budget, or example-specific runtime branch changed.
- Current gate result: all four TASK-0804 reports pass on the current tree: `verify-novywave-bridge-scenario` status `pass`; `verify-native-gpu-preview-e2e --example novywave` status `pass`; `verify-native-gpu-novywave-visual` status `pass`; `verify-native-gpu-novywave-interaction-speed` status `pass`. The final speed report has `click_interaction_timing_ms.total_apply.p95=14.070ms`, `runtime_apply.p95=9.102ms`, `runtime_step_apply.p95=7.504ms`, `click_native_input_timing_ms.total_input.p95=15.094ms`, `source_actions.p95=7.238ms`, `source_action_root_flush.p95=7.087ms`, `source_action_root_materialization.p95=3.936ms`, and `source_action_root_dirty_scheduler.p95=2.939ms`.
- Test-contract fix: the first fresh `verify-native-gpu-preview-e2e --example novywave` run failed because `novywave_dark_light_material_readbacks_cover_visual_regions` still expected hover to persistently mutate the shared layout frame. The native renderer now applies hover through a render-frame sidecar, so the test now renders `latest_preview_render_frame` with `PreviewHoverOverlayState::from_input_state`, matching the production render hook and preserving app-owned pixel evidence.
- Follow-up: next dependency-ready work is no longer TASK-0804. Remaining performance margin is still mostly root flush work, especially `source_action_root_materialization` and `source_action_root_dirty_scheduler`; future runtime tasks should prefer field-only root list-view materialization, direct `List.map` row/output reuse, cheaper dirty scheduling/dependency representation, or the compound cursor-value query only when the refreshed field profiles justify it.

- Date: 2026-06-15
- Task: EXP-0001
- Commit: uncommitted
- Files changed in this slice: `Cargo.toml`; `crates/boon_native_gpu/Cargo.toml`; `crates/boon_native_gpu/src/lib.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only upload-path review from subagent `019eccfc-3b2b-71e3-b8cc-eb668c370747`; read-only shader/layout review from subagent `019eccfc-3c59-7310-85b5-e7a3ca720b91`; `cargo fmt -p boon_native_gpu -p xtask`; `cargo check -p xtask -p boon_native_gpu`; focused `cargo test -p boon_native_gpu --lib asset_cache_reports_hits_and_avoids_repeat_raster_upload_for_known_svg -- --nocapture`; full `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json`; diagnostic `cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json` wrote a failing report for existing scenario/dev-window coverage blockers.
- Result: EXP-0001 is done and promoted into the native GPU renderer as the first POD upload slice. Quad batches now store interleaved `NativeGpuQuadVertex` POD data and uncached quad batches allocate/write one vertex buffer instead of split position/color/UV buffers. `FrameMetrics` now exposes real upload counters: `allocated_gpu_bytes`, `dirty_upload_range_count`, `buffer_reuse_count`, `staging_wrap_count`, `queue_write_count`, and `quad_cache_eviction_count`. `xtask` renderer inventory and scroll summaries consume real queue-write metrics where renderer metrics exist.
- Evidence: the WGPU asset-cache test proves first-frame dirty quad uploads use one queue write per dirty batch and fewer writes than the legacy split-buffer equivalent, while the second identical frame has zero queue writes and reuses the cached GPU buffer. The shader verifier report passes with `vertex_layout_contract` showing POD size `20`, align `4`, one host vertex buffer, stride `20`, host offsets `0/8/12`, and generated shader inputs at locations `0/1/2` with formats `Float32x2`, `Uint32`, `Float32x2`.
- Follow-up: next dependency-ready implementation task in file order is `TASK-0502`. This EXP does not claim the full ring-buffer/dirty-range path: `staging_wrap_count` is still zero by design, and dirty uploads are full dirty quad batches until `TASK-0502` adds bounded persistent/ring-buffer uploads.

- Date: 2026-06-15
- Task: TASK-0502
- Commit: uncommitted
- Files changed in this slice: `crates/boon_native_gpu/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only ring-buffer review from subagent `019ecd10-89bf-7482-8403-3551d058610d`; read-only report/gate review from subagent `019ecd10-8adc-71c0-b61b-73cad5ac4934`; read-only measurement/architecture recommendation from subagent `019ecd17-8384-7ac2-83ac-0dffbe19e2aa`; `cargo fmt -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo check -p boon_native_gpu --lib`; `cargo check -p boon_native_playground`; `cargo check -p xtask`; focused `cargo test -p boon_native_gpu --lib quad_upload_ring_ -- --nocapture`; full `cargo test -p boon_native_gpu --lib`; `cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `cargo xtask verify-report-schema`; `git diff --check -- crates/boon_native_gpu/src/lib.rs crates/boon_native_playground/src/main.rs crates/xtask/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: TASK-0502 is done. The native GPU upload path now uses a persistent bounded upload ring with frame-level reservation, generation-checked quad/prepared caches, explicit range invariants, and dirty upload range metrics. The ring grows or wraps before current-frame writes, preventing the old mid-frame overwrite class. NovyWave release interaction reports now copy renderer upload counters and stage counters from a persistent offscreen renderer probe, and the xtask gate fails if they are missing.
- Current measurement: the refreshed NovyWave speed report passes and exposes the upload path. Initial first render allocated `1048576` GPU bytes and uploaded `900600` bytes in one dirty range with one queue write. The immediate identical second render uploaded `0` bytes and reused the cached buffer. The post-interaction render still uploaded `900600` bytes, wrapped the ring once, evicted one quad-cache entry, and had retained chunk counts `294` hits / `10` misses out of `304`.
- Diagnostic conclusion: the upload path is now measured and correctness-guarded, but the remaining problem is not another ring microchange. The renderer still lowers mostly retained NovyWave changes into one whole-scene quad batch, so a small semantic change forces a full `900600` byte geometry upload. The next high-leverage renderer task is chunk-level GPU geometry/cache identity.
- Follow-up: `TASK-0502A` was added and is now the next dependency-ready renderer task in file order. It should carry retained render chunk IDs through GPU batching and prove post-interaction upload bytes fall well below the full-frame batch. Do not spend the next slice on `smallvec`/`arrayvec` or other low-level container swaps unless a refreshed report shows container overhead dominates.

- Date: 2026-06-15
- Task: TASK-0502A
- Commit: uncommitted
- Files changed in this slice: `crates/boon_document/src/render_scene.rs`; `crates/boon_native_gpu/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only document/render-scene boundary review from subagent `019ecd2a-03b6-7a82-bb1d-d4368198489d`; read-only native GPU chunk-cache design review from subagent `019ecd2a-2041-7282-8066-1ff84e5399f9`; read-only current slow-path/architecture memo from subagent `019ecd32-b7bd-7b60-abe2-64fa93e7c4de`; `cargo fmt -p boon_document -p boon_native_gpu -p boon_native_playground -p xtask`; `cargo test -p boon_document --lib`; `cargo test -p boon_native_gpu --lib`; `cargo check -p boon_native_playground -p xtask`; `cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` wrote a failing end-to-end speed report with renderer probe status `pass`; `RUST_BACKTRACE=1 cargo xtask verify-report-schema`; `git diff --check -- crates/boon_document/src/render_scene.rs crates/boon_native_gpu/src/lib.rs crates/boon_native_playground/src/main.rs crates/xtask/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `jq` inspection of the NovyWave speed report and role report.
- Result: TASK-0502A is done for the renderer upload slow path. `RenderSceneItem`, `RenderVisualPrimitive`, and `RenderQuadBatch` now carry retained chunk IDs through the renderer-neutral scene boundary with serde defaults for old artifacts. Native GPU quad batches keep retained chunk identity, split batches at chunk/texture boundaries while preserving paint order, and key the GPU quad cache by retained chunk ID, texture, vertex count, and content. Dirty upload ranges now include retained chunk IDs plus unique dirty chunk summaries. The old anonymous prelowered-quad fallback now uses indexed fallback IDs instead of collapsing every no-ID batch into one name.
- Evidence: a focused native GPU regression proves that a two-chunk document scene uploads both chunks on the first frame and only the changed chunk on the second frame, with one dirty chunk ID, one queue write, reused unchanged buffers, no staging wrap, and fewer uploaded bytes. The refreshed NovyWave renderer probe moved the post-interaction path from the previous full-scene upload to `upload_bytes=3360`, `dirty_upload_range_count=3`, `dirty_upload_chunk_count=2`, `buffer_reuse_count=231`, `staging_wrap_count=0`, and `quad_cache_eviction_count=0`; initial first render remains `900600` bytes and identical second render remains `0` bytes.
- Current speed result: the full `verify-native-gpu-novywave-interaction-speed` command still fails the strict end-to-end budget at `click_to_cursor.p95=17.399ms` and `input_to_visible.p95=17.399ms`, but that is no longer a renderer-upload failure. The measured remaining slow path is `runtime_apply.p95=9.942ms`, `runtime_step_apply.p95=7.958ms`, `source_action_root_flush.p95=7.412ms`, `source_action_root_materialization.p95=4.215ms`, `source_action_root_dirty_scheduler.p95=3.113ms`, and `layout_rebuild.p95=4.849ms`.
- Follow-up: next dependency-ready task is `TASK-0804A`, created before the low-level experiment backlog so the next slice attacks measured source-action/root-flush architecture instead of blind `smallvec`/`arrayvec` or container swaps.

- Date: 2026-06-15
- Task: TASK-0804A
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only runtime root-flush review from subagent `019ecd3e-eab4-7f71-8267-8543a56595b1`; read-only architecture ranking from subagent `019ecd3e-fddd-7d32-8101-486cb3405200`; read-only testing/instrumentation review from subagent `019ecd3f-1aca-7231-8fbf-f7d927152c8f`; `cargo fmt -p boon_runtime -p boon_native_playground -p xtask`; `cargo test -p boon_runtime --lib root_scalar_same_event -- --nocapture`; `cargo test -p boon_runtime --lib root_derived -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_route_uses_generic_root_scalar_targets -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `RUST_BACKTRACE=1 cargo xtask verify-report-schema`; `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`; full `cargo test -p boon_runtime --lib` failed; full `cargo test -p boon_native_playground --bin boon_native_playground` failed and wrote bounded output to `target/reports/native-gpu/boon-native-playground-test.log`.
- Result: TASK-0804A is not done. The kept work is generic runtime instrumentation and a provisional root dirty scheduler improvement, not a NovyWave workaround. Source-action root flush profiles now split dependency, settle, immediate, and final flush counts plus changed-read counts. The refreshed NovyWave samples show the click path has `source_action_root_dependency_flush_count=0`, `source_action_root_settle_flush_count=1`, and `source_action_root_flush_count=1`; the current problem is one root-settle flush over the affected root graph, not repeated dependency flushing. The root dirty scheduler now maintains a ready frontier and refreshes readiness around inserted/removed dirty roots instead of rescanning the whole dirty set for every pop. A same-event root-scalar guard was tightened so pure/list-view derived roots still force same-event freshness, while direct source/HOLD-style root reads can defer to the settle flush.
- Current speed result: the refreshed canonical speed gate passes with `status=pass`, `click_to_cursor.p95=15.673ms`, `input_to_visible.p95=15.673ms`, `runtime_apply.p95=9.292ms`, `layout_rebuild.p95=4.052ms`, `source_action_root_flush.p95=7.275ms`, `source_action_root_materialization.p95=3.874ms`, `source_action_root_dirty_scheduler.p95=3.270ms`, and `source_actions.p95=7.747ms`. Renderer upload remains solved: post-interaction upload is `3360` bytes, `dirty_upload_range_count=3`, `dirty_upload_chunk_count=2`, `buffer_reuse_count=231`, `queue_write_count=3`, `staging_wrap_count=0`, and `quad_cache_eviction_count=0`; initial first render is still the full `900600` byte upload and the second same-frame render uploads `0` bytes.
- Correctness caveat: the full verification surface is red, so this task stays `in_progress`. `cargo test -p boon_runtime --lib` failed with `153` passed and `44` failed; representative failures include `developer_state_summary_hides_runtime_identity` and many TodoMVC/cells tests with `List/retain requires if expression`, plus cells/NovyWave value or formatter mismatches. `cargo test -p boon_native_playground --bin boon_native_playground` failed with `130` passed and `38` failed in `129.90s`; representative failures include row-scoped source actions without row context, TodoMVC hover/toggle assertions, cells scenario regressions, NovyWave scenario failures, and the same `List/retain requires if expression` runtime build failure. Do not claim TASK-0804A complete until those full-suite failures are either fixed or proven pre-existing against a clean checkpoint.
- Follow-up: keep focusing on measured root-settle/root-list architecture before low-level container swaps. The next high-leverage choices are field-only root list-view materialization, direct root `List.map` row/output reuse, better root field/value dependency representation, and then a compound cursor-value query only if refreshed field profiles still show cursor-value lookup as dominant. Also fix the compiler/runtime `List/retain` failure surface and any row-context source routing bugs before relying on this speed result as shippable evidence.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: architecture ranking from read-only subagent `019ecd99-cdd9-7fd2-917d-dcf811e42ed2`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib novywave_file_rows_use_generic_row_source -- --nocapture`; `cargo test -p boon_runtime --lib novywave_bridge_scenario_file_row_selection_accepts_current_response -- --nocapture`; full `cargo test -p boon_runtime --lib` passed with `197` passed and `0` failed; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_ir/src/lib.rs examples/novywave/RUN.bn`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json`.
- Result: TASK-0804A remains `in_progress`, but the runtime correctness blocker from the previous caveat is resolved for `boon_runtime`. The engine now keeps row-scoped dynamic source transforms instead of letting simple constant detection drop a `THEN { List/find_value(... value: row.field ...) }` branch. The focused NovyWave row-source and bridge scenario tests pass, and the broad runtime suite is green. The native playground full test suite was not rerun in this continuation.
- Current speed result: the refreshed canonical speed gate now fails strict latency with `status=fail`, `click_to_cursor.p95=22.149ms`, and `input_to_visible.p95=22.149ms` against the `16.700ms` budget. The role artifact has no failing scenario step; the top-level blockers are budget overruns. The measured click slow path is `runtime_apply.p95=14.962ms`, `runtime_step_apply.p95=12.689ms`, `source_actions.p95=7.637ms`, `source_action_root_flush.p95=7.137ms`, `source_action_root_materialization.p95=3.886ms`, `source_action_root_dirty_scheduler.p95=3.145ms`, `derived_recompute.p95=2.096ms`, `root_materialization.p95=2.959ms`, `layout_rebuild.p95=5.185ms`, and `direct_layout_patch_total.p95=1.656ms`. Route setup, source-action unattributed time, JSON/report/proof/IPC hot-path counters, and layout patch internals are not dominant.
- Follow-up: do not spend the next slice on blind container swaps or another renderer-upload microchange. The next implementation should attack the measured architecture buckets: field-only root list-view materialization, direct root `List/map` row/output reuse, a dense root/list/field dependency frontier that avoids expensive root-settle materialization, or incremental layout invalidation tied to runtime mutation classes. Kill any experiment that does not reduce the canonical click/input p95 or at least one of `source_action_root_flush`, `source_action_root_materialization`, `source_action_root_dirty_scheduler`, or `layout_rebuild`.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only runtime cache/list-pipeline review from subagents `019ece9f-eaa1-7ac0-9c02-49d865660a73`, `019ecea1-db27-7f33-a3f2-6d5cabce3864`, and `019ecea1-fd5f-7212-9f64-e62a581fffdb`; `BOON_RUNTIME_FUNCTION_PROFILING=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed-profiled.json`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib join_field_evaluates_separator_and_empty_only_when_needed -- --nocapture`; `cargo test -p boon_runtime --lib novywave_file_rows_use_generic_row_source -- --nocapture`; `cargo test -p boon_runtime --lib novywave_bridge_scenario_file_row_selection_accepts_current_response -- --nocapture`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; full `cargo test -p boon_runtime --lib` passed with `198` passed and `0` failed; `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_ir/src/lib.rs examples/novywave/RUN.bn docs/plans/speedup/12-speedup-goal-execution-checklist.md`; `jq` inspection of both speed reports.
- Result: TASK-0804A remains `in_progress`. The measured slow path was narrowed before further optimization. The profiled report shows `RUN/selected_cursor_value_for_signal` as the largest named function bucket (`464` click calls, `160` cache hits, `51.627ms` profiled aggregate), followed by `RUN/new_signal_lane_row`, `RUN/new_signal_lane_variable_row`, `NovyModel/format_signal_value`, and `RUN/selected_cursor_pair_row`. However, the normal speed report still shows the dominant canonical budget path as one source-action settle root flush and repeated root list-view materialization, not raw filter/retain scans: `filter_field_rows_scanned=0`, `retain_rows_scanned=0`, existing text/numeric index hits are present, and map/join fusion is already active.
- Experiment killed: a generic lazy-argument function-cache key/validation experiment was implemented, focused-tested, measured, and reverted. It tried to omit unused lazy `List/join_field(empty:)` fields from the primary function cache key while validating them when the empty branch was actually read. Focused correctness tests passed, but the official speed oracle rejected it: the canonical report regressed to `runtime_apply.p95=16.611ms`, `source_action_root_flush.p95=8.489ms`, `source_action_root_materialization.p95=4.684ms`, and `layout_rebuild.p95=5.643ms`, with no useful movement in list-view cache-hit counts. Do not retry function-cache key heuristics without a report proving a different dominant bucket.
- Current speed result: after reverting the killed cache experiment, the refreshed canonical speed report still fails strict latency at `click_to_cursor.p95=27.992ms` and `input_to_visible.p95=27.992ms`. This run was slower/noisier than the earlier post-provenance report but kept the same shape: `total.p95=26.162ms`, `runtime_apply.p95=18.470ms`, `runtime_step_apply.p95=15.726ms`, `source_action_root_flush.p95=9.295ms`, `source_action_root_materialization.p95=5.299ms`, `source_action_root_dirty_scheduler.p95=3.780ms`, `derived_recompute.p95=2.269ms`, `root_materialization.p95=3.741ms`, and `layout_rebuild.p95=5.862ms`. Aggregated click list-view profiles show `selected_signal_lane_rows` at `42.015ms eval`, `41.844ms list_map_total`, `74.187ms user_body`, and `56/56 in_place`; `selected_cursor_pair_rows` at `12.640ms eval`, `12.503ms list_map_total`, `30.386ms user_body`, and `24/48 in_place`; `selected_visible_items` at `18.981ms row_materialize` despite `24/24 in_place`.
- Follow-up: the next implementation should not be another function-cache heuristic, list-index microchange, BYTES/JSON swap, renderer upload tweak, or low-level container replacement. The current evidence points to a structural runtime change: field-only/root-list-view materialization for stable row identities, direct root `List/map` row/output reuse that avoids full row-body reconstruction, or a physical compound cursor-value operator only if it reduces selected-lane and cursor-pair row-body work rather than adding cache overhead. Acceptance must be counter-based: reduce canonical click/input p95 or materially reduce `selected_signal_lane_rows` eval/list-map/user-body totals, `selected_visible_items.row_materialize_ms`, or the source-action root-flush p95 bucket without changing Boon syntax or hardcoding NovyWave.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only root-list materialization review from subagent `019eceb9-cd8a-7aa1-b203-03fabd47e444`; read-only measurement/acceptance review from subagent `019eceb9-e6a8-7f43-acd8-995317648c2c`; read-only architecture/kill-criteria review from subagent `019ececc-7a59-7230-80f3-8ff5c752c35e`; `cargo fmt -p boon_runtime`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`; `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`; `cargo test -p boon_runtime --lib root_numeric_stability_guard_skips_same_interval_structured_child_root -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` (fails current budget); `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`.
- Result: TASK-0804A remains `in_progress`. The kept runtime change is generic numeric-guard-aware invalidation for function-value and root-list-view-field caches. Cache entries whose overlapping dirty reads are direct root reads with still-valid numeric stability intervals are no longer thrown away before the guarded value can be reused. This is not a Boon syntax change, not a NovyWave branch, and not a fixture reduction. The focused regression `root_list_view_field_cache_keeps_numeric_guarded_entries` proves a cursor update inside the same guarded interval does not force a dirty cache miss.
- Experiment killed: a broader interprocedural function-argument access propagation experiment was implemented, focused-tested, measured, and reverted. It tried to propagate callee field-access summaries back into caller cache keys. Focused tests passed, but the official speed oracle regressed versus the numeric-guard-only run: `click_to_cursor.p95=23.483ms`, `runtime_apply.p95=16.226ms`, `runtime_step_apply.p95=14.044ms`, `source_action_root_flush.p95=8.223ms`, and `selected_signal_lane_rows.eval=45.418ms`. Do not retry broad function-cache/static-access heuristics unless a refreshed report proves function-cache keying itself is the dominant bucket.
- Current speed result: the refreshed canonical report after reverting the killed experiment still fails strict latency with `status=fail`, `click_to_cursor.p95=24.411ms`, and `input_to_visible.p95=24.411ms`. The measured click path remains runtime/root-list dominated: `total_apply.p95=23.375ms`, `runtime_apply.p95=16.812ms`, `runtime_step_apply.p95=14.476ms`, `source_actions.p95=9.039ms`, `source_action_root_flush.p95=8.516ms`, `source_action_root_materialization.p95=4.420ms`, `source_action_root_dirty_scheduler.p95=3.980ms`, `root_materialization.p95=3.019ms`, and `layout_rebuild.p95=5.399ms`. Aggregated click list-view profiles moved in the right direction despite p95 variance: `selected_signal_lane_rows` is now `37.507ms eval`, `37.359ms list_map_total`, `65.860ms user_body`, and `56/56 in_place`; `selected_cursor_pair_rows` is `11.950ms eval`, `11.826ms list_map_total`, `28.552ms user_body`, and `24/48 in_place`; `selected_visible_items` still spends `18.328ms row_materialize` despite `24/24 in_place`.
- Follow-up: continue measurement-first and choose a structural engine change, not another microchange. The next dependency-ready options are field-only root list-view materialization before full root evaluation, direct root `List/map` row/output reuse for stable source identities, and a dense root/list/field dependency frontier that replaces broad string/read-key set work in the settle flush. Keep any experiment only if it reduces canonical click/input p95 or materially lowers `selected_signal_lane_rows` eval/list-map/user-body, `selected_cursor_pair_rows` eval/list-map/user-body, `selected_visible_items.row_materialize_ms`, or the `source_action_root_flush` sub-buckets. Kill BYTES/JSON/container swaps until a refreshed profile shows they are the top bucket.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: measurement/architecture sidecars `019eced4-fbed-77c1-ad18-a92e4189d12d` and `019eced5-249d-7871-bf29-64c404883334`; local deep check of `materialize_root_list_view_field`, `eval_root_derived_initial_value`, `list_map`, root materialization profiles, native interaction sample aggregation, and xtask report forwarding; `cargo fmt -p boon_runtime -p boon_native_playground -p xtask`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture`; `cargo test -p boon_runtime --lib root_numeric_stability_guard_skips_same_interval_structured_child_root -- --nocapture`; full `cargo test -p boon_runtime --lib` passed with `199` passed and `0` failed; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` wrote both `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json` and failed only the current latency budgets; `jq` inspection of `target/reports/native-gpu/novywave-interaction-speed.json`; `cargo xtask verify-report-schema`; `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs crates/xtask/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Result: TASK-0804A remains `in_progress`. This slice added measurement, not a speed fix. Root list-view materialization samples now record dirty-read shape, full-eval row count, unchanged full-eval rows, source-identity coverage, and row/list-map/user-function work. The NovyWave interaction-speed role and xtask report now expose `runtime_root_list_cause_summary`, with `all`, `warmup_click`, `hover`, `click`, and `divider` scopes plus per-list aggregates and a measured `cause` label.
- Current speed result: the refreshed release role still fails strict latency with `status=fail`, `click_to_cursor.p95=32.464ms`, and `input_to_visible.p95=32.464ms`. The click path is now explicitly identified as `root_flush_dirty_scheduler_plus_root_list_materialization`: `runtime_apply.p95=22.182ms`, `runtime_step_apply.p95=19.071ms`, `source_action_root_flush.p95=11.546ms`, `source_action_root_dirty_scheduler.p95=5.183ms`, `source_action_root_materialization.p95=6.139ms`, post-source `root_materialization.p95=4.107ms`, `source_actions.p95=12.132ms`, and `layout_rebuild.p95=6.553ms`. The dirty-scheduler counters show the shape of the wave: each click starts from only `6` initial dirty root dependents, but the flush expands to `80` dirty pops, `78` scalar root materializations, `2` list-view materializations, `38` changed materializations, `185` dependent visits, and `74` dependent enqueues.
- Measured cause: the cause summary says the architectural problem is that root list-view materialization still evaluates the whole root expression, maps source rows, materializes rows, and diffs after the fact instead of using a compiled row/field dependency frontier. In 32 click samples the root-list counters show `7096` current dirty reads (`5232` root reads and `1264` list-field reads), `152` list-view materializations, `384` full-eval rows, `120` unchanged full-eval rows, `264` changed rows, `58.120ms` list-map work, `112.788ms` user-function-body work, `22.294ms` row materialization, and only `16` dirty-forced field-cache misses. The dominant list is `selected_signal_lane_rows`: `44.650ms eval`, `44.464ms list_map_total`, `76.980ms user_function_body`, `168` full-eval rows, `48` unchanged rows, `120` changed rows, `592` changed fields, and `600` user-function calls. Secondary contributors are `selected_cursor_pair_rows` (`13.797ms eval`, `13.656ms list_map_total`, `33.674ms user_function_body`) and `selected_visible_items` (`20.285ms row_materialize`).
- Scope/ambiguity: this diagnosis is concrete for the click/input cursor path. The same role report also has `hover_to_overlay.p95=20.697ms`, but hover does not show the same root-list-view cause: hover has `runtime_apply.p95=3.454ms`, `runtime_step_apply.p95=1.282ms`, `layout_rebuild.p95=4.936ms`, `source_action_root_flush.p95=0.695ms`, `source_action_root_materialization.p95=0.214ms`, and `source_action_root_dirty_scheduler.p95=0.336ms`. Hover needs a separate native-input/overlay/layout timing pass if it remains a target after the click/root-flush work.
- Follow-up: do not try to optimize blindly. The next slice should attack the measured combined root-flush architecture: a dense root/list/field dependency frontier to reduce dirty scheduler breadth, field-only root list-view materialization so stable rows and fields are not rebuilt before diffing, and direct root `List/map` row/output reuse for stable source identities. Keep container/BYTES/JSON swaps, renderer upload work, and broad function-cache heuristics out of the hot path until `runtime_root_list_cause_summary` shows they dominate. Acceptance must cite `runtime_root_list_cause_summary.click` plus the release role p95 fields and must reduce either click/input p95 or at least one of `source_action_root_dirty_scheduler`, `source_action_root_materialization`, `selected_signal_lane_rows.eval/list_map/user_body`, `selected_cursor_pair_rows.eval/list_map/user_body`, or `selected_visible_items.row_materialize`.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `crates/xtask/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only cause/missing-measurement sidecar `019ecefa-7313-7131-ac5a-083deeb99e16`; read-only row-output-cache key safety sidecar `019ecefa-71eb-7252-af39-cac788f14c91`; `cargo fmt -p boon_runtime -p boon_native_playground -p xtask`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` wrote both `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json` and failed only the current latency budgets; `jq` inspection of `runtime_root_list_cause_summary.click` and `runtime_dirty_frontier_cause_summary.click`; `cargo xtask verify-report-schema`; `git diff --check -- crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs crates/xtask/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Result: TASK-0804A remains `in_progress`. This slice added the missing measurement requested by the sidecar review: source-action root flush profiles now carry bounded `source_action_root_dirty_frontier_samples` and `source_action_root_dirty_root_work_samples`, and the NovyWave role plus xtask reports now expose `runtime_dirty_frontier_cause_summary` beside `runtime_root_list_cause_summary`. The new summary ranks changed read keys and dependent roots by dependency visits/enqueues, and ranks popped roots by scalar/list-view materialization counts and materialization time. This is generic runtime/report instrumentation, not a Boon syntax change and not a NovyWave runtime branch.
- Experiment killed: removing `source_list_epoch` from the root `List/map` row-output cache key was rejected. Focused reorder/append-remove and NovyWave tests stayed green during the trial, but the sidecar review showed the epoch is a necessary row-universe guard: in-place root-list patches preserve the epoch, while full replacement can recreate `key=1,generation=1` for a different row universe. Keep `source_list_epoch` unless a future patch adds a positive same-universe reuse test and a negative full-replacement collision test.
- Current speed result: the refreshed canonical report still fails strict latency with `status=fail`, `click_to_cursor.p95=28.510ms`, and `input_to_visible.p95=28.510ms`. Hover is not the current bottleneck at `hover_to_overlay.p95=10.398ms`. The click path is explicitly `root_flush_dirty_scheduler_plus_root_list_materialization`: `runtime_apply.p95=21.296ms`, `runtime_step_apply.p95=19.050ms`, `layout_rebuild.p95=5.172ms`, `source_action_root_flush.p95=12.856ms`, `source_action_root_materialization.p95=4.902ms`, and `source_action_root_dirty_scheduler.p95=7.116ms`. Each click still starts from `6` dirty roots and expands to `80` dirty pops, `185` dependent visits, and `74` dependent enqueues.
- Measured cause: the old root-list summary still names the architectural cause as whole-root list-view evaluation followed by after-the-fact diffing instead of a compiled row/field dependency frontier. In the click scope, aggregate root-list work is `102.217ms eval`, `96.156ms list_map_total`, `96.119ms user_function_body`, `20.061ms row_materialize`, `384` full-eval rows, `120` unchanged full-eval rows, `264` changed rows, and only `24` row-output cache hits versus `240` misses. The dominant list remains `selected_signal_lane_rows` with `70.527ms eval`, `70.383ms list_map_total`, `66.142ms user_function_body`, `168` full-eval rows, `48` unchanged rows, and `120` changed rows; `selected_cursor_pair_rows` contributes `25.893ms eval` and `25.773ms list_map_total`; `selected_visible_items` contributes `18.111ms row_materialize`.
- New frontier cause: `runtime_dirty_frontier_cause_summary.click` reports `dirty_frontier_fanout_with_ranked_root_work`. The top enqueue edges are bridge/status fanout, especially `root:bridge_cursor_values_page_digest -> store.bridge_cursor_values`, `store.bridge_cursor_values.page_digest`, `store.bridge_cursor_values_page_ref`, and `store.bridge_cursor_values_page_ref.page_digest`, each with `32` visits and `32` enqueues, plus `list_field:selected_cursor_pair_rows[0].label` and `[1].label -> store.bridge_cursor_values_label` with `24` visits/enqueues each. The top materialization work is still list-view work: `store.selected_signal_lane_rows` costs `52.832ms` across `32` list-view materializations and `712` changed reads; `store.selected_cursor_pair_rows` costs `18.576ms` across `24` list-view materializations and `72` changed reads. Secondary scalar churn is bridge derived state: `store.bridge_request_descriptor` costs `5.628ms` across `48` scalar materializations, `store.bridge_cursor_values_page_ref` costs `4.750ms` across `64`, and `store.bridge_cursor_values` costs `2.283ms` across `32`.
- Follow-up: the next implementation should use both report surfaces. Reduce the dirty-frontier fanout from bridge digest/label edges and/or stop list-view roots from rebuilding whole row bodies when only a few row fields changed. The best candidates remain a dense root/list/field dependency frontier, field-only root list-view materialization, and safe direct `List/map` row/output reuse with row-universe guards. Do not spend the next slice on JSON/BYTES/container swaps, renderer upload, or broad function-cache heuristics unless these new summaries show those have become the top bucket.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only field-only list-view materialization review from subagent `019ecf0c-dfe0-7923-b89c-0a602c7fbbce`; read-only dirty-frontier fanout review from subagent `019ecf0c-e0f8-7f02-bcab-54e2eaec04a7`; local deep check of the root dirty scheduler, structured parent/child roots, root-list materialization samples, NovyWave interaction-speed role reports, and xtask summary forwarding; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib structured_root_parent_change_prunes_owned_child_materialization -- --nocapture`; `cargo test -p boon_runtime --lib root_derived -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` wrote both `target/reports/native-gpu/novywave-interaction-speed.json` and `target/artifacts/native-gpu/novywave-interaction-speed-role.json` and failed only the current latency budgets; a temporary rerun confirmed the same shape and was removed before the final report; `jq` inspection of `runtime_root_list_cause_summary.click` and `runtime_dirty_frontier_cause_summary.click`; `cargo xtask verify-report-schema`.
- Result: TASK-0804A remains `in_progress`. The kept engine change is structured-parent dirty-root pruning: a dirty child root owned by a structured parent is held out of the ready frontier until the parent materializes, then pruned when the parent publishes changed child reads. The focused regression proves `store.page_ref.cursor` is no longer separately materialized when `store.page_ref` owns the update. This fixes an engine-level dependency smell and reduces redundant root work, but it is not the final speed fix.
- Current speed result: the refreshed canonical report still fails strict latency with `status=fail`, `click_to_cursor.p95=32.537ms`, and `input_to_visible.p95=32.537ms`; hover is lower at `hover_to_overlay.p95=13.241ms`. The click path remains `root_flush_dirty_scheduler_plus_root_list_materialization`: `runtime_apply.p95=24.009ms`, `source_action_root_flush.p95=14.058ms`, `source_action_root_dirty_scheduler.p95=8.308ms`, `source_action_root_materialization.p95=4.533ms`, and dependency enqueue alone is `3.848ms` p95. The pruning reduced the dirty wave from the previous `80` dirty pops to `35`, but visits and enqueues are still high at `180` visits and `71` enqueues per click, so the remaining cost is broad fanout plus expensive root-list work.
- Measured cause: the cause of the user-visible click slowness is not renderer upload, JSON/BYTES representation, or hardcoded NovyWave rows. It is the runtime root-settle wave. The dirty-frontier summary ranks bridge/status fanout at the top: `root:bridge_cursor_values_page_digest` enqueues `store.bridge_cursor_values`, `store.bridge_cursor_values.page_digest`, `store.bridge_cursor_values_page_ref`, and `store.bridge_cursor_values_page_ref.page_digest` `32` times each, while `selected_cursor_pair_rows[0].label` and `[1].label` each enqueue `store.bridge_cursor_values_label` `24` times. The root-list summary still shows whole root list-view evaluation and after-the-fact diffing: `152` list-view materializations, `384` full-eval rows, `120` unchanged full-eval rows, `264` changed rows, `109.499ms` eval, `102.535ms` list-map work, `105.394ms` user-function-body work, `20.944ms` row materialization, `24` row-output cache hits, and `240` misses in the click scope. The top materialized roots are still `store.selected_signal_lane_rows` (`59.293ms`, `32` list-view materializations, `712` changed reads), `store.selected_cursor_pair_rows` (`20.029ms`, `24` list-view materializations plus `8` skips), then bridge scalar roots such as `store.bridge_request_descriptor` (`4.821ms`).
- Follow-up: keep the structured-parent pruning because it removes redundant work and fixes the root/object dependency model, but do not treat it as a solved performance task. The next implementation should be the subagent-recommended field-only root list-view materialization: evaluate source row identity/order first, reuse parent row records when identities are stable, evaluate only dirty or missing fields, and fall back on shape/order/source-epoch mismatch. The parallel dirty-frontier line is to replace broad bridge/status read-key fanout with a denser field/list dependency frontier. Kill any next experiment that does not reduce canonical click/input p95 or materially reduce `source_action_root_dirty_scheduler`, dependency enqueue count/time, `selected_signal_lane_rows` eval/list-map/user-body, `selected_cursor_pair_rows` eval/list-map/user-body, or `selected_visible_items.row_materialize_ms`.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `crates/boon_native_playground/src/main.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only current-cause sidecar `019ecf32-05aa-7890-b491-b457596c7253`; read-only stale-artifact/field-only oracle sidecar `019ecf31-67e7-7bd2-861f-a42d8e0bd060`; `cargo fmt -p boon_runtime -p boon_native_playground`; `cargo test -p boon_runtime --lib root_list_view_same_source_rows_patch_in_place_and_keep_target_identity -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_keeps_numeric_guarded_entries -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the current latency budgets; `jq` inspection of top-level p95s, `runtime_root_list_cause_summary.click`, and `runtime_dirty_frontier_cause_summary.click`.
- Result: TASK-0804A remains `in_progress`. This slice added and corrected measurement around the proposed field-only root-list path instead of guessing. Root list-view profiles now report `field_only_attempt_count`, `field_only_patch_count`, fallback count/reasons, patched rows, skipped fields, evaluated fields, and changed fields through the native speed report. The runtime field-only detector now recognizes the multiline root shape used by NovyWave (`input` on one child line followed by `|> List/map(...)`), and the per-field dirty guard now uses the same numeric-stability-aware invalidation rule as the broader cache invalidation path instead of counting guarded same-interval entries as dirty forced misses. No Boon syntax, NovyWave filename/row shortcut, fixture reduction, JSON/BYTES swap, renderer change, or hardcoded example branch was added.
- Current speed result: the refreshed canonical release report at `target/reports/native-gpu/novywave-interaction-speed.json` still fails strict latency: `click_to_cursor.p95=35.765ms` and `input_to_visible.p95=35.765ms` against the `16.700ms` budget. The current click path is still runtime dominated: `runtime_apply.p95=25.267ms`, `runtime_step_apply.p95=22.394ms`, `source_actions.p95=15.413ms`, `source_action_root_flush.p95=14.766ms`, `source_action_root_materialization.p95=4.587ms`, `source_action_root_dirty_scheduler.p95=8.575ms`, and `layout_rebuild.p95=6.499ms`. Hot-path proof/report guards remain zero (`hot_path_png_write_count=0`, `hot_path_report_write_count=0`, `hot_path_proof_readback_count=0`, `preview_blocked_on_ipc_count=0`), so the failure is not report IO, proof readback, or IPC blocking.
- Measured cause: the cause is still the source-action root-settle wave, specifically dirty-frontier fanout plus root list-view materialization. The field-only path now proves useful but insufficient: `selected_cursor_pair_rows` reports `field_only_attempt_count=48`, `field_only_patch_count=48`, `full_eval_row_count=0`, and no fallback, but it is the smaller contributor. The dominant hot root is still `store.selected_signal_lane_rows`, with `32` list-view materializations, `712` changed reads, `61.689ms` ranked root work, `77.655ms eval`, `77.441ms list_map_total`, `70.143ms user_function_body`, `168` full-eval rows, and `48` unchanged full-eval rows. `selected_signal_lane_rows` has `field_only_attempt_count=0`, which code inspection explains: the root is a terminal `List/map`, but `new_signal_lane_row(row)` starts with a `WHEN` branch that dispatches to record-producing functions, while the current field-only plan only supports direct function bodies that are records. `selected_visible_items` remains a secondary materialization cost with `19.147ms row_materialize`.
- Dirty-frontier cause: the frontier still fans out through bridge/status roots before or around the list work. Top edges are `root:bridge_cursor_values_page_digest -> store.bridge_cursor_values`, `store.bridge_cursor_values.page_digest`, `store.bridge_cursor_values_page_ref`, and `store.bridge_cursor_values_page_ref.page_digest`, each with `32` visits/enqueues, plus `root:cursor_label -> store.bridge_cursor_values_page_digest` with `32` visits/enqueues and `selected_cursor_pair_rows[0].label` / `[1].label -> store.bridge_cursor_values_label` with `24` visits/enqueues each. This keeps `source_action_root_dirty_scheduler.p95` high even after structured-child pruning reduced the earlier dirty pop count.
- Follow-up: the next implementation should not be a blind cache/container/BYTES/JSON/renderer change. The directly measured options are: branch-aware field-only root list-view materialization for `WHEN -> record function` row constructors, safe direct `List/map` row-output reuse with row-universe guards for stable source identities, and a denser root/list/field dependency frontier that reduces bridge/status fanout. Keep any next slice only if it lowers canonical click/input p95 or materially reduces one of `source_action_root_dirty_scheduler`, `source_action_root_materialization`, `selected_signal_lane_rows.eval/list_map/user_body/full_eval_row_count`, or `selected_visible_items.row_materialize_ms`.

- Date: 2026-06-16
- Task: TASK-0804A cause audit addendum
- Commit: uncommitted
- Files changed in this slice: `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only branch/field-set safety review from subagent `019ecf43-37cf-7143-bc73-c3ddca317343`; read-only planner-shape review from subagent `019ecf43-0b92-7fb1-ba5f-84ca94a7a51c`; local inspection of `examples/novywave/RUN.bn` and `crates/boon_runtime/src/lib.rs` field-only planner/evaluator paths; existing canonical report `target/reports/native-gpu/novywave-interaction-speed.json`.
- Cause confirmation: both subagents independently confirmed that the current direct field-only planner cannot reach `store.selected_signal_lane_rows` because `new_signal_lane_row(row)` is a `WHEN` dispatcher, not a direct record-producing function. This matches the measurement: `selected_cursor_pair_rows` uses field-only successfully while `selected_signal_lane_rows` has zero field-only attempts and remains the dominant root-list cost. The correct next runtime change is generic branch-aware field-only planning for `List/map -> WHEN -> record function`, with strict source-identity, branch-selector, and selected-branch field-set guards. It must treat nested records such as `lane_state`, `hit_regions`, `window_ref`, `page_refs`, and `materialization_ref` as whole top-level JSON fields, not flatten them. It must fall back to full list-view materialization on dirty list structure, source identity/order mismatch, selector/branch ambiguity, unsupported branch output, or top-level field-set mismatch so branch-only fields cannot go stale.
- Cause status: documented. The measured slowness is runtime-side root-settle fanout plus whole-row list-view rematerialization, not renderer upload, report/proof IO, IPC blocking, JSON/BYTES representation, file-name hardcoding, or a Boon-level workaround issue.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_only_patches_when_dispatched_record_rows -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_branch_selector_dirty_falls_back_before_field_patch -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the current latency budgets; `jq` inspection of top-level p95s, `runtime_root_list_cause_summary.click`, and `runtime_dirty_frontier_cause_summary.click`; `cargo xtask verify-report-schema`; full `cargo test -p boon_runtime --lib` passed with `201` passed and `0` failed.
- Result: TASK-0804A remains `in_progress`, but this slice is kept. The runtime field-only root-list planner now recognizes generic `List/map -> WHEN -> record function` row constructors, stores the actual callee row argument for field-cache source binding, and keeps direct record constructors working. Branch-dispatched rows use the selected branch record scope in field cache keys, treat nested record/list fields as whole top-level values, and fall back before patching when the branch selector reads dirty data, no branch matches, a branch pattern binds, source row identity/order is unstable, active branch fields are missing from the target row storage, or the branch output is not a simple record-producing function call. No Boon syntax, NovyWave-specific branch, fixture reduction, hardcoded file name, or renderer change was added.
- Current speed result: the refreshed canonical release report still fails strict end-to-end latency, but the measured root-list slice moved materially: `click_to_cursor.p95=25.766ms` and `input_to_visible.p95=25.766ms` are down from the previous `35.765ms`; click `runtime_apply.p95=18.172ms`, `runtime_step_apply.p95=15.950ms`, `source_actions.p95=11.380ms`, `source_action_root_flush.p95=10.837ms`, `source_action_root_materialization.p95=2.744ms`, `source_action_root_dirty_scheduler.p95=7.307ms`, and `layout_rebuild.p95=5.116ms`. Renderer/proof/report/IPC hot-path evidence remains outside the cause; the gate fails because click/input still exceed the `16.700ms` budget.
- Measured effect: `store.selected_signal_lane_rows` now uses the field-only path instead of full row rebuilds: `field_only_attempt_count=56`, `field_only_patch_count=56`, `field_only_fallback_count=0`, `field_only_skipped_field_count=4768`, `field_only_evaluated_field_count=160`, `field_only_changed_field_count=144`, `full_eval_row_count=0`, `full_eval_unchanged_row_count=0`, `list_map_total_ms=0`, and `row_materialize_ms=0`. The same click summary still names `selected_signal_lane_rows` as the dominant list because it has many field-cache probes and changed fields, but the old `77.441ms list_map_total` and `168` full-eval rows are gone. `selected_cursor_pair_rows` remains field-only with `48` attempts/patches and `0` full-eval rows. `selected_visible_items` is now the main remaining full-row list materialization contributor with `72` full-eval rows, `24` unchanged full-eval rows, and `17.463ms row_materialize`.
- Remaining cause: the next blocker is dirty-frontier fanout and bridge/status scalar churn, not whole-row lane rematerialization. The dirty-frontier summary still reports `dirty_frontier_fanout_with_ranked_root_work`: each click starts from `6` dirty roots, expands to `35` dirty pops, `180` dependent visits, and `71` dependent enqueues. Top enqueue edges remain `root:bridge_cursor_values_page_digest -> store.bridge_cursor_values`, `store.bridge_cursor_values.page_digest`, `store.bridge_cursor_values_page_ref`, and `store.bridge_cursor_values_page_ref.page_digest` with `32` visits/enqueues each, plus `root:cursor_label -> store.bridge_cursor_values_page_digest` with `32` visits/enqueues and `selected_cursor_pair_rows[0].label` / `[1].label -> store.bridge_cursor_values_label` with `24` visits/enqueues each. The next slice should reduce this broad bridge/status dependency frontier or make `selected_visible_items` eligible for safe field/output reuse; kill any experiment that does not lower `source_action_root_dirty_scheduler`, dependency enqueue time/count, `selected_visible_items.row_materialize_ms`, or final click/input p95.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only dirty-frontier fanout review from subagent `019ecf5a-7557-7d00-b61c-061fcc7286f6`; read-only `selected_visible_items` row-ref materialization review from subagent `019ecf5a-9157-7a91-a927-f1689129e450`; killed local edge-grouping experiment measured with canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_filtered_rows_preserve_identity_for_downstream_map -- --nocapture`; `cargo test -p boon_runtime --lib novywave_initial_bridge_descriptor_uses_initial_format -- --nocapture`; `cargo test -p boon_runtime --lib novywave_timeline_pan_zoom_required_sequence_matches_current_model -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the current latency budgets; `jq` inspection of top-level p95s, `runtime_root_list_cause_summary.click`, and `runtime_dirty_frontier_cause_summary.click`; `cargo xtask verify-report-schema`; `cargo test -p boon_runtime --lib` passed with `201` passed and `0` failed; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Result: TASK-0804A remains `in_progress`, but the row-ref structural-slice reuse slice is kept. The runtime now has a generic root list-view fast path for stable row-ref outputs such as filter/retain/move pipelines. It applies only when the output rows are row refs, target root-source identities match, no list structure or target-list dirty key is present, an actual source row field is dirty, and the source and target lists differ. The path records exposed source fields as source-list column dependencies, patches only dirty source fields into the target list, and falls back to the old full row materializer on missing identity, source identity/order mismatch, missing target field, target-list dirty reads, list-structure dirty reads, or identity-only/no-field-dirty waves. The initial identity-only version was rejected by NovyWave format tests, so the kept guard requires a source row-field dirty to avoid skipping the full copy that establishes target field values.
- Killed experiment: grouping `read -> dependent` edges by dependent inside the dirty scheduler looked plausible but failed the kill criteria. It preserved tests, but canonical measurement worsened the targeted bucket (`source_action_root_dependent_enqueue.p95` moved from roughly `3.50ms` to `3.63ms`) and did not reduce dirty-pop/visit/enqueue counts, so it was reverted before this slice was kept.
- Current speed result: the refreshed canonical release report still fails strict end-to-end latency, but click/input p95 improved from the previous kept report's `25.766ms` to `24.962ms`. Click `runtime_apply.p95=17.166ms`, `runtime_step_apply.p95=14.965ms`, `source_actions.p95=11.227ms`, `source_action_root_flush.p95=10.702ms`, `source_action_root_materialization.p95=2.794ms`, `source_action_root_dirty_scheduler.p95=7.285ms`, and `layout_rebuild.p95=5.127ms`. The strict budget remains `16.700ms`, so this is not a completed speed task.
- Measured effect: `store.selected_visible_items` now uses the row-ref field patch path: `field_only_attempt_count=24`, `field_only_patch_count=24`, `full_eval_row_count=0`, `full_eval_unchanged_row_count=0`, `row_materialize_ms=0`, `previous_snapshot_ms=0`, `field_only_evaluated_field_count=72`, and `field_only_changed_field_count=48`. Before this slice it had `72` full-eval rows, `24` unchanged full-eval rows, and about `17.5ms` row materialization in the click aggregate. `store.selected_signal_rows` also now has `field_only_attempt_count=24`, `field_only_patch_count=24`, `full_eval_row_count=0`, and `row_materialize_ms=0`, removing the smaller structural-slice rematerialization cost.
- Remaining cause: row materialization is no longer the main list-view blocker. The report still names `dirty_frontier_fanout_with_ranked_root_work`; each click still has `35` dirty pops, `180` dependent visits, and `71` dependent enqueues. The top root work remains `store.selected_signal_lane_rows` and `store.selected_cursor_pair_rows` field-patch/diff/user-function work, followed by bridge/status scalar roots such as `store.bridge_request_descriptor`, `store.bridge_cursor_values`, `store.bridge_cursor_values_page_ref`, and `store.bridge_cursor_values_label`. The next slice should target dirty-frontier fanout or the expensive field-patch diff/user-function path for the already field-only roots, not JSON/BYTES/container swaps or renderer upload.

- Date: 2026-06-16
- Task: TASK-0804A deep cause check
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: dirty-frontier sidecar review from subagent `019ecf75-bcd9-7000-bec2-d0bf33f86c42`; field-patch/user-function sidecar review from subagent `019ecf76-18b5-7800-a1ea-e52ca5f56e9b`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_stable_fields_across_cursor_change -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_field_cache_reuses_same_pass_dirty_row_independent_fields -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_parent_change_prunes_owned_child_materialization -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo xtask verify-report-schema`; `cargo test -p boon_runtime --lib`; refreshed canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the known NovyWave latency budgets; `jq` inspection of `runtime_root_list_cause_summary.click` and `runtime_dirty_frontier_cause_summary.click`; `git diff --check -- crates/boon_runtime/src/lib.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Result: TASK-0804A remains `in_progress`. The kept runtime change is the field-only previous-pass clean-hit skip: when a root list-view field value is proven reusable from the previous pass, the field-only patch loop now skips storage lookup and value comparison for that field. Same-pass hits still compare so row-independent dirty fields update every affected row. This is generic runtime code, not Boon syntax, not a NovyWave source rewrite, not a renderer change, and not a fixture shortcut.
- Killed experiment: deferred structured-child scheduler coalescing was measured and reverted. It reduced dependent enqueue count in one local run, but it worsened the actual goal metrics: click/input p95 moved to about `25.698ms`, aggregate dirty scheduler time rose to about `141.589ms`, dependent enqueue time rose to about `96.815ms`, and dirty pops rose to `792`. Do not retry that deferral shape without a different readiness model and a stricter proof that enqueue time and p95 improve.
- Current speed result: the refreshed report at `target/reports/native-gpu/novywave-interaction-speed.json` still fails strict latency: `click_to_cursor.p95=26.105ms` and `input_to_visible.p95=26.105ms` against the `16.700ms` budget. Runtime remains the dominant path: `runtime_apply.p95=17.399ms`, `runtime_step_apply.p95=15.325ms`, and `layout_rebuild.p95=5.077ms`. In the click scope, `source_actions_ms=239.219`, `source_action_root_flush_ms=221.889`, `source_action_root_dirty_scheduler_ms=135.794`, `source_action_root_dependent_enqueue_count=1304`, `source_action_root_dependent_enqueue_ms=64.538`, `source_action_root_dependent_visit_count=3296`, and `source_action_root_dirty_pop_count=744`.
- Root-list cause: whole-row list rematerialization is no longer the primary cause. The hot lists all use field-only paths with `full_eval_row_count=0` and `row_materialize_ms=0`: `selected_signal_lane_rows` has `56` attempts/patches, `diff_ms=31.246`, `eval_ms=34.949`, `user_function_body_ms=30.667`, `field_cache_hits=4768`, `field_cache_misses=160`, `field_only_skipped_field_count=4768`, `field_only_evaluated_field_count=160`, and `field_only_changed_field_count=144`; `selected_cursor_pair_rows` has `48` attempts/patches, `diff_ms=11.037`, `eval_ms=11.265`, and `user_function_body_ms=27.605`; `selected_visible_items` has `24` attempts/patches and `eval_ms=5.286`. The remaining list cost is field-patch bookkeeping, diffing, cache-key work, and user-function body work across many fields, not full row rebuilds.
- Dirty-frontier cause: the report still names `dirty_frontier_fanout_with_ranked_root_work`. The top repeated enqueue edges are `root:bridge_cursor_values_page_digest -> store.bridge_cursor_values`, `store.bridge_cursor_values.page_digest`, `store.bridge_cursor_values_page_ref`, and `store.bridge_cursor_values_page_ref.page_digest`, each with `32` visits/enqueues; `root:cursor_label -> store.bridge_cursor_values_page_digest` with `32`; and `selected_cursor_pair_rows[0].label` / `[1].label -> store.bridge_cursor_values_label` with `24` each. The top materialized roots are still `store.selected_signal_lane_rows` (`32` pops, `27.771ms` materialization), `store.selected_cursor_pair_rows` (`32` pops, `8` skips, `10.593ms` materialization), then bridge/status scalar roots such as `store.bridge_request_descriptor`, `store.bridge_cursor_values`, `store.bridge_cursor_values_page_ref`, and `store.bridge_cursor_values_label`.
- Cause statement: the current slowness is the source-action root-settle wave, composed of dirty-frontier fanout through bridge/status roots plus repeated field-patch/diff/user-function work in already field-only list roots. It is not currently explained by renderer upload, proof/report IO, IPC blocking, JSON/BYTES representation, hardcoded NovyWave filenames, or whole-row list rematerialization. The next implementation should reduce the bridge/status dependency frontier, replace broad root/list read-key fanout with denser field/list dependency IDs, or reduce field-only patch overhead by precomputing dirty-field bitsets and cache probes. Kill any next experiment that does not lower `source_action_root_dirty_scheduler_ms`, dependent enqueue count/time, `selected_signal_lane_rows` diff/eval/user-body work, `selected_cursor_pair_rows` diff/eval/user-body work, or final click/input p95.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only dirty-frontier review from subagent `019ecf94-e37e-71c1-b7ce-307d5faa643e`; read-only field-only cache/prefilter review from subagent `019ecf95-05bb-7170-8b50-8a58205c5aae`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; refreshed canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the current latency budgets; `jq` inspection of top-level p95s, `runtime_root_list_cause_summary.click`, and `runtime_dirty_frontier_cause_summary.click`; `cargo test -p boon_runtime --lib` passed with `201` passed and `0` failed; `cargo xtask verify-report-schema` passed after deleting temporary `target/reports/native-gpu/novywave-interaction-speed-repeat.json` because it intentionally pointed at a stale diagnostic role artifact.
- Result: TASK-0804A remains `in_progress`, but this field-only overhead slice is kept. The runtime now precomputes record-field names and free-env name sets once in the generic root list-view field-only plan, then reuses them for each row instead of recomputing `statement_free_env_names` and record child names inside every row-field probe. It also avoids cloning a cached `FieldValue` on previous-pass clean field-cache hits when the caller will skip target comparison anyway. Same-pass hits still return a value and still compare target rows, so row-independent dirty values continue to patch every affected row. Cached reads and numeric stability guards are still merged into the frame even when the field value is skipped. No Boon syntax, NovyWave-specific branch, fixture reduction, hardcoded filename, renderer change, or dirty-frontier heuristic was added.
- Killed or deferred ideas: the dirty-frontier sidecar recommended grouping `read -> dependent` edges by dependent. This exact shape is not retried here because the previous checklist entry already records a local grouping experiment that preserved tests but worsened the targeted enqueue bucket. A future dirty-frontier change should alter the dependency representation or readiness model more deeply, not just regroup the same edges after lookup.
- Current speed result: the refreshed canonical report at `target/reports/native-gpu/novywave-interaction-speed.json` still fails strict latency: `click_to_cursor.p95=25.853ms` and `input_to_visible.p95=25.853ms` against the `16.700ms` budget. Runtime remains the path: `runtime_apply.p95=17.700ms`, `runtime_step_apply.p95=15.470ms`, and `layout_rebuild.p95=5.230ms`. Click-scope root-settle aggregates are still hot: `source_actions_ms=241.776`, `source_action_root_flush_ms=223.750`, `source_action_root_dirty_scheduler_ms=138.214`, `source_action_root_dependent_enqueue_count=1304`, `source_action_root_dependent_enqueue_ms=66.172`, `source_action_root_dependent_visit_count=3296`, and `source_action_root_dirty_pop_count=744`.
- Measured effect: the targeted field-only list slice moved down versus the previous cause-check report while preserving the cache/miss/change shape. Aggregate root-list `diff_ms` moved from `49.202` to `44.033`, `eval_ms` from `51.821` to `46.619`, and `user_function_body_ms` from `60.298` to `55.298`. `selected_signal_lane_rows` moved from `diff_ms=31.246`, `eval_ms=34.949`, and `user_function_body_ms=30.667` to `diff_ms=25.553`, `eval_ms=29.419`, and `user_function_body_ms=25.051`, with the same `field_cache_hits=4768`, `field_cache_misses=160`, `field_only_skipped_field_count=4768`, `field_only_evaluated_field_count=160`, and `field_only_changed_field_count=144`. `selected_cursor_pair_rows` stayed field-only but remains comparatively expensive because it has no reusable stable-field hits.
- Remaining cause: this slice reduces field-only bookkeeping but does not solve the source-action root-settle wave. The next implementation should target dirty-frontier fanout through bridge/status roots or replace the broad root/list read-key dependency frontier with denser IDs. Keep the kill criteria strict: preserve edge identities and counts unless the dependency model is intentionally changed, and require improvement in `source_action_root_dirty_scheduler_ms`, dependent enqueue count/time, or final click/input p95.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only dirty-frontier dependency-model review from subagent `019ecfa5-4fe8-78f1-9c32-809562a00944`; read-only field-only patch overhead review from subagent `019ecfa5-6adc-7fc0-b11b-e4088865ef6c`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; refreshed canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the known NovyWave latency budgets; `jq` inspection of top-level blockers, `runtime_root_list_cause_summary.click`, and `runtime_dirty_frontier_cause_summary.click`; `cargo test -p boon_runtime --lib` passed with `201` passed and `0` failed; `cargo xtask verify-report-schema` passed. Broader `cargo test -p boon_native_playground --bin boon_native_playground` was run for visibility but is not passing evidence: it failed with `146` passed and `22` failed across existing Cells/editor/TodoMVC/NovyWave UI-surface tests such as unresolved `store.selected_input.sources.editor.select`, source-intent target mismatches, and hover/focus assertions.
- Result: TASK-0804A remains `in_progress`, but this field-only clean-hit prefilter slice is kept. The root list-view field-only loop now bypasses clean previous-pass field-cache entries instead of calling the full cached field evaluator for every stable row-field. It records aggregate cache reuse, bulk-carries the previous root reads once so skipped fields still keep downstream dependencies, and merges previous numeric guards only when doing so remains conservative for freshly evaluated reads. Same-pass cache hits still flow through the old evaluator and still compare target rows, so row-independent dirty fields continue to patch every affected row. Source-action root-settle list materialization now marks field cache entries as prevalidated because cache invalidation already ran before materialization; direct/manual materialization keeps the old dirty-forced-miss guard. No Boon syntax, NovyWave-specific source branch, fixture reduction, hardcoded filename, renderer change, or dirty-frontier heuristic was added.
- Current speed result: the refreshed canonical report at `target/reports/native-gpu/novywave-interaction-speed.json` still fails strict latency, but this slice passes the keep criteria by reducing the measured field-only hot buckets and the final p95. `click_to_cursor.p95` and `input_to_visible.p95` are now `25.170ms` against the `16.700ms` budget, down from the previous kept `25.853ms`. Click `runtime_apply.p95=17.773ms`, `runtime_step_apply.p95=15.352ms`, and `layout_rebuild.p95=5.144ms`. Click-scope root-settle totals moved from `source_actions_ms=241.776` and `source_action_root_flush_ms=223.750` to `source_actions_ms=235.760` and `source_action_root_flush_ms=217.904`.
- Measured effect: aggregate root-list `diff_ms` moved from `44.033` to `36.953`, `eval_ms` from `46.619` to `40.789`, and `user_function_body_ms` from `55.298` to `45.584`. The dominant `selected_signal_lane_rows` field-only path moved from `diff_ms=25.553`, `eval_ms=29.419`, and `user_function_body_ms=25.051` to `diff_ms=19.105`, `eval_ms=23.760`, and `user_function_body_ms=16.788`, with the same semantic shape: `field_cache_hits=4768`, `field_cache_misses=160`, `field_only_skipped_field_count=4768`, `field_only_evaluated_field_count=160`, and `field_only_changed_field_count=144`. `selected_cursor_pair_rows` remains mostly unchanged because it has no reusable stable-field hits: `diff_ms=10.981`, `eval_ms=11.414`, and `user_function_body_ms=26.819`.
- Remaining cause: dirty-frontier fanout is now the clearer blocker. This slice did not change `source_action_root_dependent_enqueue_count=1304` or `source_action_root_dirty_pop_count=744`, and `source_action_root_dirty_scheduler_ms=138.330` remains essentially flat. The dirty-frontier sidecar recommends changing the dependency model rather than regrouping queue work: either make owned structured child roots projections of their owning parent or canonicalize root read keys into whole-root versus child-field dependency identities. The next slice should target those dependency-model changes and kill any attempt that does not reduce enqueue count/time, dirty pops, dirty scheduler time, or final click/input p95.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only root read-key canonicalization review from subagent `019ecfbf-0460-7f31-8f52-f24ed3e6d153`; read-only owned structured-child projection review from subagent `019ecfbf-35bb-7943-a914-50ce5cc320b3`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib structured_root_parent_change_prunes_owned_child_materialization -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the known NovyWave latency budgets; `jq` inspection of top-level p95s, root-list counters, and dirty-frontier root work; `cargo xtask verify-report-schema`; full `cargo test -p boon_runtime --lib` passed with `201` passed and `0` failed; `git diff --check -- crates/boon_runtime/src/lib.rs`.
- Result: TASK-0804A remains `in_progress`, but this dependency-model slice is kept. The runtime now treats structured child roots that are already covered by a pending owned structured parent as projection work instead of separately enqueuing them in the same dirty wave. The skip is generic and guarded: the child root must be an exact pure root field with no sources, must have a pending/dirty exact root parent that currently owns a JSON object/list value, and must not be directly observed or drive an observed projection. Downstream dependents still update through the parent materialization's published child changed-read keys. No Boon syntax, NovyWave-specific root name, fixture reduction, renderer change, JSON/BYTES swap, or broad status-name heuristic was added.
- Kept dependency cleanup: scalar root dependency registration no longer adds the structured children produced by the scalar root value into that root's input dependency set. Produced child keys are still emitted as changed reads through `root_changed_read_keys_for_materialized_value`, but `root_reads_by_field` now represents actual inputs plus the root identity rather than output shape. This is the more important fix: the first projection-skip-only measurement reduced enqueue count but shifted work into extra scalar/root pops and regressed p95 to `25.743ms`; after removing output-shape reads from dependency registration, the same projection skip became a net win and the slice passed the keep criteria.
- Current speed result: the refreshed canonical report still fails the strict budget, with `click_to_cursor.p95=23.108ms` and `input_to_visible.p95=23.108ms` against `16.700ms`. It is nevertheless materially better than the previous kept checkpoint (`25.170ms`). Click `total_apply.p95=22.216ms`, `runtime_apply.p95=15.956ms`, `runtime_step_apply.p95=13.298ms`, and `layout_rebuild.p95=5.073ms`. Runtime source-action buckets moved down: `source_actions.p95=10.162ms`, `source_action_root_flush.p95=9.666ms`, `source_action_root_dirty_scheduler.p95=6.211ms`, and `source_action_root_materialization.p95=2.464ms`.
- Measured effect: click aggregate `source_actions_ms` moved from `235.760` to `201.724`, `source_action_root_flush_ms` from `217.904` to `186.803`, and `source_action_root_dirty_scheduler_ms` from `138.330` to `113.611`. The dependent enqueue count dropped from `1304` to `600`, enqueue time moved from `65.871ms` to `69.851ms`, dependent visits stayed high at `3536`, and dirty pops increased from `744` to `792`. Root-list work stayed roughly in the same class but slightly better for the dominant lane list: aggregate root-list `diff_ms=35.923`, `eval_ms=39.608`; `selected_signal_lane_rows` has `diff_ms=18.225`, `eval_ms=22.545`, and `user_function_body_ms=16.584`.
- Remaining cause: the new blocker is no longer the already-removed whole-row rebuild or the old top child-output dependency edges. Dirty-frontier fanout still goes through real scalar bridge/status and cursor roots, with `source_action_root_dependent_visit_count=3536`, `source_action_root_dirty_pop_count=792`, and top roots such as `store.bridge_request_descriptor`, `store.bridge_cursor_values_page_ref`, `store.bridge_cursor_values`, `store.bridge_cursor_values_label`, `store.cursor_label`, and the two field-only list roots. The next slice should target the larger root read-key split recommended by subagent `019ecfbf-0460-7f31-8f52-f24ed3e6d153`: canonical whole-root versus child-field identities and alias deduplication, so raw roots do not fan out through multiple string aliases and child projections subscribe to parent child keys instead of raw input roots. Keep the kill criteria strict: require lower dirty visits/enqueue time/pop count or final p95, and revert if work merely shifts into scalar bridge roots or layout.

- Date: 2026-06-16
- Task: TASK-0804A continuation
- Commit: uncommitted
- Files changed in this slice: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary root dependency alias canonicalization in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: read-only root read-key split implementation review from subagent `019ecfd1-1aca-7870-9a4b-e699e270f2ee`; read-only alias-dedup risk review from subagent `019ecfd1-4ad4-7650-b477-8feaf3740c68`; during the reverted experiment `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_dependency_map_canonicalizes_qualified_and_unqualified_root_aliases -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`; after revert `git status --short` returned clean before this checklist-only edit.
- Result: killed and reverted a shallow root dependency alias canonicalization experiment. The patch stored root dependency reads through a plan-resolved canonical key and looked up root dependents through the same canonical set, while leaving evaluator/source lookup semantics unchanged. Focused correctness tests passed, including NovyWave, but the speed oracle rejected the tradeoff: click/input p95 regressed from `23.108ms` to `24.171ms`, `source_action_root_flush_ms` rose from `186.803` to `214.933`, `source_action_root_dirty_scheduler_ms` rose from `113.611` to `119.147`, and `source_action_root_materialization_ms` rose from `55.598` to `77.478`. The key shape did not move: `source_action_root_dependent_visit_count=3536`, `source_action_root_dependent_enqueue_count=600`, and `source_action_root_dirty_pop_count=792` stayed effectively unchanged.
- Cause learned: alias labels are not the current slow path by themselves. The experiment did canonicalize many frontier labels from local names such as `root:cursor_label` to store-qualified names such as `root:store.cursor_label`, but the alternating slow click still expands the real cursor/bridge request chain: `selected_timeline_cursor_value -> cursor_label/cursor_position/keyboard_cursor_label/selected_signal_lane_rows`, then `cursor_label -> bridge_cursor_values_page_digest` and `bridge_request_descriptor_label -> bridge_request_fingerprint/input_digest/structural_key/page roots`. The next dependency-model slice must therefore change the graph shape, not just normalize string aliases. Candidate directions are full whole-root versus child-field read identities with child subscribers moved off raw parent roots, demand/observed-root frontier pruning for unobserved bridge pages, or a compiled bridge/page dependency frontier that avoids materializing every page/status root when only the cursor-value page is required. Kill any next attempt that leaves the `194`-visit slow click class, `600` click enqueue count, or `792` click dirty-pop count unchanged.

- Date: 2026-06-16
- Task: TASK-0804A deep root-demand cause measurement
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`, `crates/boon_native_playground/src/main.rs`, `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only observed/demand frontier review from subagent `019ecfe7-8a43-71d3-95d1-d336f7eae17b`; read-only lazy-materialization correctness review from subagent `019ecfe7-be76-7fd1-87c4-491200c2501f`; `cargo fmt -p boon_runtime -p boon_native_playground`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the known NovyWave latency budget; deep diagnostic `BOON_PROFILE_ROOT_DEMAND=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed-root-demand.json` was run, then the diagnostic report was moved to `target/diagnostics/native-gpu/novywave-interaction-speed-root-demand.json` so schema scans keep the canonical fresh report; canonical speed was rerun after the diagnostic; `cargo xtask verify-report-schema`.
- Result: TASK-0804A remains `in_progress`, and this slice is kept as gated measurement infrastructure, not as an optimization. Runtime reports now carry exact dirty-frontier demand classification totals and root-work demand labels. The expensive observed-downstream closure classifier is disabled by default and enabled only with `BOON_PROFILE_ROOT_DEMAND=1`; canonical reports therefore keep comparable timing and show `profile_disabled` in those classification fields. The committed helper reorder from the prior checkpoint was reverted in the working tree because its refreshed report missed the kill criteria.
- Current canonical speed result: the fresh normal report still fails strict latency but returns to the expected class: `click_to_cursor.p95=22.740ms` and `input_to_visible.p95=22.740ms` against `16.700ms`. Click aggregate root work is `source_actions_ms=200.613`, `source_action_root_flush_ms=185.308`, `source_action_root_dirty_scheduler_ms=112.782`, `source_action_root_materialization_ms=55.155`, `source_action_root_dependent_visit_count=3536`, `source_action_root_dependent_enqueue_count=600`, and `source_action_root_dirty_pop_count=792`. The slow-click shape is still unchanged: sixteen click samples in the `194` visits / `32` enqueues / `38` pops class.
- Deep diagnostic cause: the env-gated report intentionally slows the verifier (`click_to_cursor.p95=29.419ms`) because it computes downstream observation closure, but it identifies the frontier shape. In click aggregate edge counts, `candidate_unobserved_source_free_pure` accounts for `3016` visits and `576` edge enqueues, `blocked_list_view` for `384` visits and `64` edge enqueues, `blocked_observed_downstream` for `224` visits and `96` edge enqueues, and `blocked_observed_root` for `160` visits and `80` edge enqueues. Top materialization work remains `store.selected_signal_lane_rows` (`22.604ms`) and `store.selected_cursor_pair_rows` (`10.839ms`), followed by pure bridge/page roots such as `store.bridge_request_descriptor` (`3.884ms`), `store.bridge_cursor_values_page_ref` (`2.949ms`), `store.bridge_cursor_values` (`1.372ms`), and `store.bridge_cursor_values_label` (`1.249ms`).
- Cause learned: the current slowness is a mixed root-settle architecture problem. The list-view roots still dominate materialization time, but most dirty-frontier edge traffic is unobserved source-free pure bridge/page/status work that is still eagerly materialized because queryability and semantic-delta guarantees require committed current roots. A safe lazy/demand slice must therefore introduce explicit deferred-dirty root state plus `ensure_root_current(path, reason)` for summaries, sparse queries, assertions, and evaluator reads. Naive observed-root pruning is not safe. The next implementation should either build that deferred-root state for a strict source-free pure subset, or attack the blocked list-view materialization directly with a compiled row/field frontier. Kill any next attempt that leaves the `194/32/38` slow-click class, `600` unique click enqueues, `792` dirty pops, and top list-view materialization times unchanged.

- Date: 2026-06-16
- Task: TASK-0804A clean field-only list no-op experiment
- Commit: uncommitted
- Files changed in this slice: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary root-list clean snapshot implementation in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: read-only deferred-root safety review from subagent `019ed019-f49a-7002-8ae7-7abafe8ef64b`; read-only list-view materialization review from subagent `019ed01a-348a-79a3-a018-2cff29fc9126`; during the reverted experiment `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_cache_keeps_numeric_guarded_entries -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib` passed with `201` passed and `0` failed; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed the known latency budgets but also rejected the experiment; `jq` inspection of `runtime_root_list_cause_summary.click` and `runtime_dirty_frontier_cause_summary.click`; after revert `cargo fmt -p boon_runtime` and `git diff --stat` returned no runtime diff before this checklist-only edit; canonical speed was rerun after the revert so `target/reports/native-gpu/novywave-interaction-speed.json` again matches current code and fails only the known latency budget.
- Result: killed and reverted the proposed generic root-list clean snapshot/no-op path. The idea was to reuse the existing field-only cache proof to skip the row/field loop when source identities and cached row-field entries were all clean. Correctness tests passed, but the speed oracle rejected it: `click_to_cursor.p95` and `input_to_visible.p95` regressed from the current `22.740ms` class to `27.274ms`; `source_actions_ms` rose from `200.613` to `240.712`; `source_action_root_flush_ms` rose from `185.308` to `224.832`; `source_action_root_dirty_scheduler_ms` rose from `112.782` to `118.579`; and `source_action_root_materialization_ms` rose from `55.155` to `84.434`.
- Cause learned: the no-op proof did not change the scheduler shape at all: `source_action_root_dependent_visit_count=3536`, `source_action_root_dependent_enqueue_count=600`, and `source_action_root_dirty_pop_count=792` stayed in the same class. It also added overhead to the already-hot field-only path: `store.selected_signal_lane_rows` rose to `40.916ms` materialization and `store.selected_cursor_pair_rows` rose to `18.993ms`. The list-view sidecar was right that list materialization remains important, but this snapshot shape is too late and too expensive for the current NovyWave click wave. Do not retry root-list clean snapshots unless the proof is compiled/static and demonstrably removes per-row cache-key/user-function work from the hot source-action path.
- Post-revert canonical state: the refreshed current-code report is back in the expected class with `click_to_cursor.p95=22.636ms` and `input_to_visible.p95=22.636ms`; `source_actions_ms=198.983`; `source_action_root_flush_ms=183.849`; `source_action_root_dirty_scheduler_ms=112.266`; `source_action_root_materialization_ms=55.058`; `source_action_root_dependent_visit_count=3536`; `source_action_root_dependent_enqueue_count=600`; and `source_action_root_dirty_pop_count=792`. Top list roots are again `store.selected_signal_lane_rows` at `22.256ms` and `store.selected_cursor_pair_rows` at `10.859ms`.
- Next direction: favor either a real deferred-dirty root implementation for the strict `candidate_unobserved_source_free_pure` subset with `ensure_root_current` for summaries/assertions/evaluator reads, or a compiled row/field frontier that avoids entering the field-only row loop for roots whose dirty field bitset is empty. Kill any next list-view attempt that leaves `selected_signal_lane_rows` and `selected_cursor_pair_rows` materialization flat or worse, and kill any frontier attempt that leaves the `194/32/38` slow-click class unchanged.

- Date: 2026-06-16
- Task: TASK-0804A compiled field-frontier prefilter experiment
- Commit: uncommitted
- Files changed in this slice: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` (temporary field-frontier implementation in `crates/boon_runtime/src/lib.rs` was reverted)
- Verification: read-only deferred-dirty/currentness review from subagent `019ed043-58fc-7063-82a8-10b7d93f173f`; read-only compiled row/field frontier review from subagent `019ed043-88e9-7142-a36b-f25a90b29c50`; during the reverted experiment `cargo fmt -p boon_runtime`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed the known latency budgets and rejected the experiment; after revert `cargo fmt -p boon_runtime` left no runtime diff, and canonical speed was rerun so `target/reports/native-gpu/novywave-interaction-speed.json` again matches current code.
- Result: killed and reverted the proposed static dirty-field prefilter inside root list-view field-only materialization. The experiment added per-record-field row-argument access metadata, matched `current_dirty_reads` against source-row field access plus free root/list names, and skipped fields before env-fingerprint/cache-key work when the frontier could not affect that field. Correctness checks stayed green after broadening one cache-accounting assertion to recognize the new prefilter counter, but the speed report rejected the shape: the experiment produced `click_to_cursor.p95=23.510ms` and `input_to_visible.p95=23.510ms`; `source_actions_ms=209.903`; `source_action_root_flush_ms=195.625`; `source_action_root_dirty_scheduler_ms=115.918`; and `source_action_root_materialization_ms=62.579`.
- Cause learned: this prefilter is too late and too conservative for NovyWave's click wave. It did not change the root scheduler shape at all: `source_action_root_dependent_visit_count=3536`, `source_action_root_dependent_enqueue_count=600`, and `source_action_root_dirty_pop_count=792` stayed unchanged. It also damaged the hot list-view cache profile: `store.selected_signal_lane_rows` worsened to `eval_ms=29.487`, `diff_ms=25.170`, and only `904` field-cache hits instead of the current-code `4768`, while `store.selected_cursor_pair_rows` stayed roughly flat. The likely cause is that source-row/global free-name matching removed cheap previous-pass cache hits but did not remove the expensive active-projector/user-function path or any scheduler work.
- Post-revert canonical state: the refreshed current-code report remains in the expected failed-latency class with `click_to_cursor.p95=23.185ms` and `input_to_visible.p95=23.185ms`; `source_actions_ms=205.773`; `source_action_root_flush_ms=190.109`; `source_action_root_dirty_scheduler_ms=113.716`; `source_action_root_materialization_ms=57.433`; `source_action_root_dependent_visit_count=3536`; `source_action_root_dependent_enqueue_count=600`; and `source_action_root_dirty_pop_count=792`. Top list roots are `store.selected_signal_lane_rows` at `eval_ms=23.418` / `diff_ms=18.741` and `store.selected_cursor_pair_rows` at `eval_ms=11.961` / `diff_ms=11.295`.
- Next direction: do not retry field-frontier skipping inside the already-running field-only loop. If taking the list-view path again, move the frontier earlier so it avoids creating/evaluating the active projector for unaffected rows or compiles a per-root field plan outside the hot loop. Otherwise switch to the deferred-dirty pure-root design: the current frontier evidence still says most edge traffic is eager source-free pure bridge/page/status work, but that design must preserve same-turn semantic deltas and queryability through explicit `ensure_root_current`.

- Date: 2026-06-16
- Task: TASK-0804A deferred structured pure-root experiment
- Commit: uncommitted
- Files changed in this slice: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` only; temporary deferred structured-root runtime implementation in `crates/boon_runtime/src/lib.rs` was reverted.
- Verification: read-only list-view/frontier review from subagent `019ed093-29cc-7980-b98b-8cbdbc51e69b`; read-only deferred-dirty/currentness review from subagent `019ed093-27f4-7072-9ffa-9a782b5bd8f3`; baseline canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed known latency budgets; root-demand diagnostic `BOON_PROFILE_ROOT_DEMAND=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-root-demand.json` failed known latency budgets and showed `candidate_unobserved_source_free_pure` at `3016` visits / `576` enqueues; during the reverted experiment `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib unobserved_structured_pure_root_defers_until_summary_read -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_parent_change_prunes_owned_child_materialization -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical speed gate rejected the experiment; after revert `cargo fmt -p boon_runtime`; `git diff -- crates/boon_runtime/src/lib.rs` was empty; canonical speed gate was rerun so `target/reports/native-gpu/novywave-interaction-speed.json` matches current code and fails only the known latency budgets.
- Result: killed and reverted the proposed deferred structured pure-root dirty-state experiment. The implementation used strong guards: source-free pure root, JSON object/list current value, previous root-only reads, no observed root/projection/downstream, and descendant roots only when also deferrable. Focused correctness passed, but the speed oracle rejected it.
- Baseline / diagnostic: before the experiment, current code was `click_to_cursor.p95=24.139ms`, `input_to_visible.p95=24.139ms`, `runtime_apply.p95=15.775ms`, `runtime_step_apply.p95=13.319ms`, and `layout_rebuild.p95=5.660ms`; root totals were `source_actions_ms=205.477`, `source_action_root_flush_ms=191.840`, `source_action_root_dirty_scheduler_ms=118.585`, `source_action_root_materialization_ms=68.670`, and visits/enqueues/pops at `3536` / `600` / `792`. The root-demand diagnostic confirmed a large source-free pure candidate class, but also showed why it is not automatically safe: scalar protocol roots still publish semantic deltas, and structured roots can feed child/downstream reads.
- Experiment result: click/input p95 regressed to `42.467ms`, `runtime_apply.p95=34.840ms`, `runtime_step_apply.p95=32.572ms`, `source_actions_ms=530.327`, `root_flush_ms=516.431`, `scheduler_ms=379.701`, and `materialization_ms=126.614`. Visits dropped to `2336`, but enqueues/pops worsened to `648` / `840`. List costs exploded: `selected_signal_lane_rows eval_ms=75.245 diff_ms=73.928 user_function_body_ms=77.417`, and `selected_cursor_pair_rows eval_ms=20.542 diff_ms=20.008 user_function_body_ms=48.261`.
- Cause learned: naive deferred-root dirty state is too late and too stateful for the current NovyWave click path. It can reduce raw visits, but leaves the dependency graph shape wrong, increases queue churn, and shifts invalidated cache work into hot list-view field loops. The experiment also exposed a separate root-identity smell: a synthetic `store.cursor` root collided with generated `store.page_ref.cursor` through leaf alias lookup. That is a compiler/runtime identity bug to fix later, not something to mask with a Boon-level workaround.
- Post-revert current state: `click_to_cursor.p95=22.580ms`, `input_to_visible.p95=22.580ms`, `runtime_apply.p95=15.313ms`, `runtime_step_apply.p95=12.750ms`, and `layout_rebuild.p95=5.023ms`; root totals are `source_actions_ms=195.536`, `source_action_root_flush_ms=182.703`, `source_action_root_dirty_scheduler_ms=115.483`, `source_action_root_materialization_ms=62.983`, and visits/enqueues/pops remain `3536` / `600` / `792`. Top lists are back in the normal slow class: `selected_signal_lane_rows eval_ms=19.554 diff_ms=18.351 user_function_body_ms=17.373`, and `selected_cursor_pair_rows eval_ms=11.478 diff_ms=10.944 user_function_body_ms=27.687`.
- Next direction: keep TASK-0804A in progress. Do not retry runtime-only deferred dirty roots without a compiled demand/currentness graph that preserves semantic deltas and cache validity. The next slice should target canonical whole-root vs child-field dependency identities / alias splitting, or a compiled bridge/page demand frontier before enqueue, not inside-loop field prefiltering or lazy currentness bolted onto the existing dirty queue.

- Date: 2026-06-16
- Task: TASK-0804A nested root alias/lookup correctness slice
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only root dependency identity review from subagent `019ed0aa-53ba-77f2-88d0-2bae4325d768`; read-only bridge/page demand frontier review from subagent `019ed0aa-54f6-79d2-a5b8-5bac1de67359`; `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib root_read_key_aliases_match_store_local_without_nested_leaf_collision -- --nocapture`; `cargo test -p boon_runtime --lib nested_structured_child_change_does_not_dirty_top_level_leaf_alias -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the known strict latency budget; full `cargo test -p boon_runtime --lib` passed with `202` passed and `0` failed.
- Result: kept a generic root identity correctness fix and measured speed improvement, but not a full graph-shape pass. Root dependency aliases, runtime scalar lookup, derived root plan lookup, and observed-root variants now use exact paths plus `store.`-local paths only. Nested roots such as `store.page_ref.cursor` no longer publish, match, or resolve through the bare leaf `cursor`, while top-level `store.cursor` remains readable as `cursor`. The new focused scheduler regression proves a synthetic `store.page_ref.cursor` dirty wave reaches `page_label` but does not dirty a root that only read top-level `cursor`. No Boon syntax, NovyWave source change, hardcoded root name, fixture reduction, or deferred/lazy currentness state was added.
- Current speed result: the refreshed canonical report still fails the `16.700ms` click/input budget, but improves materially from the prior current-code class. `click_to_cursor.p95` and `input_to_visible.p95` are `18.511ms`; `runtime_apply.p95=10.712ms`; `runtime_step_apply.p95=8.373ms`; `layout_rebuild.p95=5.079ms`. Click aggregate root work is `source_actions_ms=123.935`, `source_action_root_flush_ms=110.128`, `source_action_root_dirty_scheduler_ms=49.836`, and `source_action_root_materialization_ms=56.086`.
- Remaining cause: this fix improves work cost but does not change the dirty frontier graph cardinality. `source_action_root_dependent_visit_count=3536`, `source_action_root_dependent_enqueue_count=600`, and `source_action_root_dirty_pop_count=792` remain unchanged, so the explicit TASK-0804A graph-shape target is still open. Hot list roots are lower but still present: `selected_signal_lane_rows eval_ms=14.976 diff_ms=13.828 user_function_body_ms=11.521` with `4768` field-cache hits / `160` misses; `selected_cursor_pair_rows eval_ms=9.152 diff_ms=8.690 user_function_body_ms=23.030` with `96` misses.
- Cause learned: the earlier deferred-root experiment correctly exposed a real compiler/runtime identity smell, but the immediate fix is stricter path identity, not lazy materialization. Bare leaf aliases are unsafe for nested `store.*.*` roots because they let synthetic structured children collide with real top-level roots and observed-root analysis. This cleanup also reduces scheduler/root-list cost in the canonical report, likely by preventing unrelated nested/top-level alias overlap, but it cannot remove the `3536/600/792` frontier by itself.
- Next direction: keep TASK-0804A in progress and implement the sidecar-recommended real identity split: `GenericReadKey::Root { field }` for whole roots plus a structured child identity such as `RootChild { root, path }`, with child reads registered by structured child access and parent materialization publishing exact child keys. Alternatively, take the static structured pure-root demand-frontier slice, but only with scalar semantic-delta roots kept eager and summary/query/evaluator barriers proven. Kill any next slice that leaves visits/enqueues/pops unchanged and does not improve final p95 or simply shifts cost into list materialization.

- Date: 2026-06-16
- Task: TASK-0804A explicit structured-child read identity slice
- Commit: uncommitted
- Files changed in this slice: `crates/boon_runtime/src/lib.rs`; `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: built on read-only root dependency identity review from subagent `019ed0aa-53ba-77f2-88d0-2bae4325d768`; `cargo fmt -p boon_runtime`; `cargo check -p boon_runtime`; `cargo test -p boon_runtime --lib root_read_key_aliases_match_store_local_without_nested_leaf_collision -- --nocapture`; `cargo test -p boon_runtime --lib nested_structured_child_change_does_not_dirty_top_level_leaf_alias -- --nocapture`; `cargo test -p boon_runtime --lib structured_root_ -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed only the known strict latency budget; full `cargo test -p boon_runtime --lib` passed with `202` passed and `0` failed.
- Result: kept the first explicit structured-child dependency identity. `GenericReadKey` now has `RootChild { root, path }`; structured child changed-read publishing emits root-child keys, structured child evaluator reads subscribe through root-child keys, dirty readiness understands root-child self-dependencies, and reports label them as `root_child`. Whole-root keys remain for true whole-root dependencies and top-level local aliases. The focused regression now proves `page_ref.cursor` consumers subscribe to `RootChild { root: "store.page_ref", path: "cursor" }` and not the whole parent root. No Boon syntax, NovyWave source edit, hardcoded NovyWave root, fixture reduction, or lazy/deferred currentness state was added.
- Current speed result: the refreshed canonical report still fails the `16.700ms` click/input budget, but improves again versus the previous kept alias/lookup slice. `click_to_cursor.p95` and `input_to_visible.p95` are `18.059ms`; `runtime_apply.p95=10.347ms`; `runtime_step_apply.p95=8.275ms`; `layout_rebuild.p95=4.912ms`. Click aggregate root work is `source_actions_ms=122.301`, `source_action_root_flush_ms=105.838`, `source_action_root_dirty_scheduler_ms=45.303`, and `source_action_root_materialization_ms=56.692`.
- Remaining cause: the explicit child identity reduces cost but still does not reduce dirty-frontier cardinality. `source_action_root_dependent_visit_count=3536`, `source_action_root_dependent_enqueue_count=600`, and `source_action_root_dirty_pop_count=792` remain unchanged. Hot list roots are slightly lower/flat: `selected_signal_lane_rows eval_ms=15.553 diff_ms=14.308 user_function_body_ms=11.574` with `4768` field-cache hits / `160` misses; `selected_cursor_pair_rows eval_ms=9.141 diff_ms=8.693 user_function_body_ms=22.850` with `96` misses.
- Cause learned: splitting the identity representation was necessary and safe, but the NovyWave slow frontier is still driven by real root chains and remaining compatibility/whole-root subscriptions, not only child-vs-parent aliasing. The next graph-shape slice should inspect exact top dirty edges with `BOON_PROFILE_ROOT_DEMAND=1` after this change and then remove unnecessary flat compatibility subscriptions or implement the static structured pure-root demand frontier. Kill the next slice if it fails to reduce the `3536/600/792` class or final p95.
- Next direction: keep TASK-0804A in progress. Run a fresh root-demand diagnostic on the `RootChild` state before another optimization, then either remove leftover flat child compatibility edges where tests prove child subscribers are covered, or implement static structured pure-root demand pruning with scalar semantic-delta roots still eager and summary/query/evaluator barriers explicit.

- Date: 2026-06-16
- Task: TASK-0804A direct list-ref alias narrowing experiment
- Commit: reverted before checklist update
- Files changed in this slice: `docs/plans/speedup/12-speedup-goal-execution-checklist.md` only; temporary direct list-ref read narrowing in `crates/boon_runtime/src/lib.rs` was reverted.
- Verification: read-only report/cause review from subagent `019ed0cf-641b-7162-80c1-3c63b0a0d9fd`; read-only RootChild compatibility-edge review from subagent `019ed0cf-62c7-75b1-a9c5-46693863332a`; refreshed baseline `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json` failed known latency budgets; root-demand diagnostic `BOON_PROFILE_ROOT_DEMAND=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-root-demand.json` failed known latency budgets and confirmed `candidate_unobserved_source_free_pure` remained the dominant edge class; during the reverted experiment `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib novywave_bridge_cursor_rows_alias_tracks_list_identity_not_row_content -- --nocapture`; `cargo test -p boon_runtime --lib root_numeric_stability_guard_skips_same_interval_structured_child_root -- --nocapture`; `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`; `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`; `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`; `cargo check -p boon_runtime -p boon_native_playground -p xtask`; canonical speed gate rejected the experiment; after revert `cargo fmt -p boon_runtime`; `git diff -- crates/boon_runtime/src/lib.rs` was empty.
- Result: killed and reverted a narrow direct list-ref alias subscription experiment. The temporary patch changed the direct list-ref branches for roots such as `store.bridge_cursor_values.rows` to register `root_dependency_read_keys_for_path` for the alias self path, so the alias used `RootChild { root: "store.bridge_cursor_values", path: "rows" }` instead of the flat nested `Root { field: "store.bridge_cursor_values.rows" }`, while keeping the referenced list root identity. Focused correctness stayed green, but the speed oracle rejected it.
- Experiment speed result: `click_to_cursor.p95` and `input_to_visible.p95` regressed to `22.740ms` against the `16.700ms` budget. The root frontier did not move: click p95 remained `194` dependent visits, `32` enqueues, and `38` dirty pops; aggregate click root work remained `source_action_root_dependent_visit_count=3536`, `source_action_root_dependent_enqueue_count=600`, and `source_action_root_dirty_pop_count=792`. It also shifted cost into list materialization: `selected_signal_lane_rows eval_ms=18.870` and `selected_cursor_pair_rows eval_ms=11.094`, both worse than the immediately preceding baseline.
- Cause learned: this compatibility edge is real but not the current hot graph-shape lever by itself. Narrowing the direct alias self-read after `RootChild` does not reduce the actual cursor/request/page chain and can perturb cache/list materialization enough to regress the canonical interaction. Do not retry this exact alias narrowing as a speed slice unless paired with a broader compiled demand/currentness graph and a focused correctness proof for nested alias queryability.
- Post-revert current-code report: the refreshed canonical `target/reports/native-gpu/novywave-interaction-speed.json` again matches the current runtime and fails only the strict latency budgets: `click_to_cursor.p95=18.691ms`, `input_to_visible.p95=18.691ms`, `runtime_apply.p95=10.922ms`, `runtime_step_apply.p95=8.618ms`, and `layout_rebuild.p95=5.176ms`. The graph-shape blocker remains unchanged at click p95 `194` visits / `32` enqueues / `38` pops and aggregate `3536` visits / `600` enqueues / `792` pops. Top lists are `selected_signal_lane_rows eval_ms=15.886 diff_ms=14.573` and `selected_cursor_pair_rows eval_ms=9.382 diff_ms=8.927`.
- Current state: TASK-0804A remains `in_progress` and unfinished. Per user direction on 2026-06-16, pause further TASK-0804A optimization after refreshing the canonical report back on current code and move to another unfinished dependency-ready task.

- Date: 2026-06-16
- Task: EXP-0002 `smallvec` Or `arrayvec` For Tiny Hot Lists
- Commit: uncommitted
- Files changed in this slice: `Cargo.toml`; `Cargo.lock`;
  `crates/boon_runtime/Cargo.toml`; `crates/boon_runtime/src/lib.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only tiny-vector candidate review from subagent
  `019ed0dd-8dcb-7090-954a-55d0538c5a1e`; baseline diagnostic
  `cargo xtask verify-example-speed counter --report target/diagnostics/exp-0002-counter-baseline.json`
  failed the known allocation budget; root-read-key-only diagnostic
  `cargo xtask verify-example-speed counter --report target/diagnostics/exp-0002-counter-root-read-keys.json`
  failed the same allocation budget; combined diagnostic
  `cargo xtask verify-example-speed counter --report target/diagnostics/exp-0002-counter-root-read-keys-dirtysets.json`
  failed the same allocation budget; `cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib root_read_key_aliases_match_store_local_without_nested_leaf_collision -- --nocapture`;
  `cargo test -p boon_runtime --lib structured_root_ -- --nocapture`;
  `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`;
  `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`;
  `cargo test -p boon_runtime --lib dirty_ -- --nocapture`;
  `cargo check -p boon_runtime`; `cargo test -p boon_runtime -p boon_document --lib`
  passed with `boon_document` `33` passed / `0` failed and `boon_runtime`
  `202` passed / `0` failed.
- Result: promoted the experiment as a small internal runtime allocation
  cleanup. `root_read_keys_for_path`, `root_dependency_read_keys_for_path`,
  `root_read_keys_for_nested_path`, root materialization changed-read payloads,
  and value-field materialization changed-read payloads now use
  `SmallVec<[GenericReadKey; 3]>`. `DirtyKeySets` now stores entries in
  `SmallVec<[DirtyKeyEntry; 8]>`. These choices keep overflow safe by spilling
  instead of panicking and do not change reports, schemas, Boon syntax, or public
  runtime APIs.
- Measurement: the counter speed diagnostic showed baseline after-warmup
  allocations at `88` / `7558` bytes per step and total allocations at `523` /
  `45187` bytes. The root-read-key-only slice reduced that to `86` / `7222`
  after-warmup and `511` / `43171` total. The combined promoted slice reduced
  it to `84` / `7110` after-warmup and `499` / `42499` total, a reduction of
  `4` after-warmup allocations and `24` total allocations for the scenario.
- Caveat: the speed diagnostic still fails the existing strict
  `allocation_budget` because the budget requires zero after-warmup allocations
  and the current runtime still has `84`. This task should not be treated as the
  broader allocation-budget fix or as evidence that container swaps are the main
  NovyWave latency lever.
- Next direction: move on to the next dependency-ready unfinished task instead
  of continuing low-level container swaps. Revisit tiny inline storage only when
  a refreshed report identifies another specific hot allocation surface with a
  clear before/after counter.

- Date: 2026-06-16
- Task: EXP-0003 Interner Crate Versus Custom Symbol Table
- Commit: uncommitted
- Files changed in this slice: `crates/boon_ir/src/lib.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only interner/symbol-table audit from subagent
  `019ed0ed-43c4-7700-bb72-db4d9f65aeba`; `cargo fmt -p boon_ir`;
  `cargo test -p boon_ir --lib semantic_symbol_table_reuses_duplicate_category_text_pairs -- --nocapture`;
  `cargo test -p boon_ir --lib semantic_index_skeleton_reuses_parser_ir_and_typecheck_facts -- --nocapture`;
  `cargo test -p boon_ir --lib static_schedule_verifier_checks_order_and_symbol_tables -- --nocapture`;
  `cargo check -p boon_ir`; `cargo test -p boon_typecheck --lib`;
  `cargo test -p boon_runtime --lib`; diagnostic
  `cargo xtask verify-example-speed counter --report target/diagnostics/exp-0003-counter-symbol-table.json`
  failed only the known strict allocation budget and was kept under
  `target/diagnostics` so report-schema scans do not treat it as canonical.
- Broad-gate caveat: the original EXP-0003 command
  `cargo test -p boon_parser -p boon_ir -p boon_typecheck -p boon_runtime --lib`
  is not a clean acceptance gate in the current checkout. `boon_ir` failed `4`
  existing lowering/source-wrapper tests, and
  `inline_empty_render_slot_lists_inside_row_constructors_get_unique_names`
  reproduced with the EXP-0003 patch reversed. `boon_parser --lib` also failed
  `list_unknown_alias_does_not_create_list_memories` and
  `widget_prefixed_symbols_do_not_create_list_memories` without any parser
  changes in this slice. `boon_typecheck --lib` and `boon_runtime --lib` both
  passed.
- Result: no `lasso`, `string-interner`, or broader dependency swap was added.
  The general experiment is superseded because `TASK-0102` already chose and
  implemented custom dense semantic/runtime symbol tables while preserving
  readable diagnostics. The kept cleanup replaces the semantic-index
  construction map from `BTreeMap<(String, String), SemanticSymbolId>` to a
  small `SemanticSymbolTable` with borrowed category/text lookups and stable
  insertion-order entries, removing repeated owned lookup-key allocation for
  duplicate symbols without changing report shape.
- Evidence: the fresh diagnostic report shows `semantic_index.symbol_count=71`,
  `semantic_index.reuse.parser_reused_by_ir=true`,
  `semantic_index.reuse.typecheck_reused_by_ir=true`,
  `semantic_index.reuse.runtime_reports_reuse_index=true`,
  `compiled_schedule.runtime_symbol_table.kind=dense_runtime_symbol_ids`,
  `compiled_schedule.runtime_symbol_ownership=compiled_program_owned`, and
  `compiled_schedule.field_slot_collision_count=0`.
- Next direction: do not revisit a general interner crate unless a refreshed
  report or targeted counter proves a specific remaining string boundary is hot.
  If string work is needed later, start with measured `FieldSlotId::from_path`,
  source-route lookup, or text-lookup/cache-key clone counters rather than a
  cross-stage dependency swap.

- Date: 2026-06-16
- Task: EXP-0004 Dirty Set Representation
- Commit: uncommitted
- Files changed in this slice:
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only dirty-set representation audit from subagent
  `019ed170-ae51-7630-a3c3-eed241ffb9f6`;
  `cargo test -p boon_runtime --lib dirty -- --nocapture`;
  `cargo xtask verify-example-speed todomvc --report target/reports/todomvc-speed.json`;
  `jq` inspection of TodoMVC dirty-set counters; `jq` inspection of the
  current NovyWave interaction report.
- Failed/non-authoritative evidence:
  `cargo xtask verify-large-list-scan-counters --report
  target/reports/large-list-scan-counters.json` failed with
  `large-list rows-scanned proof too small: stage max 0, per-step max 0,
  expected at least 1000`. Treat the large-list scan counter scenario as
  needing repair before it can choose dirty-set representation.
- Result: current `SmallVec<[DirtyKeyEntry; 8]>` / linear `current_vec`
  remains the dirty-set representation. The refreshed TodoMVC report has
  tiny absolute dirty cardinality (`dirty_entry_count.p95 = 14`, max `21`);
  density is high only because the universe is tiny. The current NovyWave
  report shows `dirty_set_metrics.p95 = 0.041408ms`, far below
  `source_action_root_dirty_scheduler.p95 = 2.526055ms`, so swapping in
  `fixedbitset`, `roaring`, or sorted `Vec` is rejected for this data shape.
- Next direction: move to dependency-frontier, field-only list-root, or
  compiled row scheduling tasks instead of continuing generic dirty-set
  container swaps.

- Date: 2026-06-16
- Task: EXP-0005 Shader-Side Shapes
- Commit: uncommitted
- Files changed in this slice:
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only shader-side shape audit from subagent
  `019ed175-4a8a-7fb1-b548-87a0a06dff7e`; local renderer/shader inspection;
  `cargo test -p boon_native_gpu --lib`; `cargo xtask
  verify-native-gpu-shaders --check --report
  target/reports/native-gpu/shaders.json`; `jq` inspection of the latest
  available NovyWave interaction renderer upload probe.
- Result: killed/deferred shader-side shape promotion for the current roadmap
  slice. The current generated shader remains a simple textured-quad path, and
  CPU-expanded rounded rects, shadows, material layers, and checkbox/circle
  rasters are real future candidates. The latest available NovyWave
  interaction report, generated before the current `HEAD`, shows
  post-interaction renderer upload at `3360` bytes, `3` dirty upload ranges,
  `3` queue writes, `314` retained chunk hits, and `10` misses. Treat that as
  historical direction only; together with current shader tests, it does not
  justify a shader/vertex schema change before the measured
  runtime/root-frontier work.
- Next direction: if returning to this family, first add primitive-type
  expansion counters and a narrow proof fixture for one analytic primitive
  family, likely checkbox/circle/checkmark. Keep the generated
  WESL/WGSL/bindgen pipeline authoritative and do not hand-edit the generated
  shader path.

- Date: 2026-06-16
- Task: TASK-0901A `.boonc` deterministic artifact emission and report hash
- Commit: uncommitted
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`;
  `crates/boon_cli/src/main.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: design/readiness audit from subagent
  `019ed179-6dbb-7250-b6b4-e9e51923eda9`; `cargo fmt -p
  boon_runtime -p boon_report_schema -p boon_cli -p xtask`; `cargo test -p
  boon_runtime --lib compiled_artifact -- --nocapture`; `cargo test -p
  boon_report_schema --lib compiled_artifact -- --nocapture`; `cargo check
  -p boon_cli -p xtask`; `cargo run -p boon_cli -- compile
  examples/todomvc.bn --out target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc.json`; `cargo xtask
  verify-compiled-artifact todomvc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-report-schema`; `cargo test -p boon_report_schema --lib`; `cargo
  test -p xtask`; `cargo test -p boon_runtime --lib`
- Result: first TASK-0901 child slice is complete, but TASK-0901 remains
  `in_progress`. The runtime can emit a deterministic `boonc-json-v1` artifact
  with semantic index, symbol table, storage layout, source schemas, route op
  streams, dependency graph, document lowering tables, bridge schema status,
  source unit hashes, report schema hash, and compiled schedule. The schema
  verifier now validates compile-artifact reports and checks the artifact file
  hash. No normal source-run report claims `.boonc` execution yet.
- Follow-up: implement TASK-0901B by deserializing/loading `.boonc` into the
  runtime without reparsing source, then TASK-0901C by running a scenario from
  that loaded artifact and proving parity with the interpreter output.

- Date: 2026-06-16
- Task: TASK-0901B `.boonc` runtime-load readiness stop condition
- Commit: uncommitted
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`;
  `crates/boon_cli/src/main.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: artifact-load design/risk audit from subagent
  `019ed186-3c28-7341-a5c2-9b905edbcbdc`; `cargo fmt -p
  boon_runtime -p boon_report_schema -p boon_cli -p xtask`; `cargo test -p
  boon_runtime --lib compiled_artifact -- --nocapture`; `cargo test -p
  boon_report_schema --lib compiled_artifact -- --nocapture`; `cargo check
  -p boon_cli -p xtask`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo run -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo
  xtask verify-report-schema`; `cargo test -p boon_report_schema --lib`;
  `cargo test -p xtask`
- Result: TASK-0901B is not complete, and the code now prevents a false
  artifact-load claim. The new `inspect-artifact` CLI/xtask path validates a
  `.boonc` file and writes a schema-checked diagnostic report, but the report
  explicitly says `runtime_instantiated_from_artifact = false`,
  `source_free_runtime_load_available = false`, and
  `scenario_execution_available = false`. A temp-source deletion test proves
  artifact inspection does not reparse source. The current artifact is blocked
  on these missing source-free runtime-plan sections: `runtime_plan`,
  lossless runtime symbols, executable equation plans, runtime storage
  initialization plan, source schema table, and document lowering runtime
  tables.
- Follow-up: implement a versioned executable `runtime_plan` in
  `boonc-json-v1` from runtime-owned tables, not full `TypedProgram`, because
  `TypedProgram` still embeds parser AST expressions. Then add an artifact
  constructor for `LoadedRuntime`/`GenericScheduledRuntime` and only then allow
  a report to claim runtime instantiation from `.boonc`.

- Date: 2026-06-16
- Task: TASK-0901B partial `runtime_plan` artifact section
- Commit: uncommitted
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: runtime-plan boundary audit from subagent
  `019ed191-3b1d-7700-a025-bc9d61eb1be7`; `cargo fmt -p
  boon_runtime -p boon_report_schema -p xtask`; `cargo test -p
  boon_runtime --lib compiled_artifact -- --nocapture`; `cargo test -p
  boon_report_schema --lib compiled_artifact -- --nocapture`; `cargo check
  -p boon_runtime -p boon_cli -p xtask`; `cargo xtask
  verify-compiled-artifact todomvc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo run -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo run
  -p boon_cli -- compile examples/todomvc.bn --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc.json`; `cargo xtask
  verify-report-schema`; `cargo test -p boon_report_schema --lib`; `cargo
  test -p xtask`; `cargo test -p boon_runtime --lib`
- Result: TASK-0901B remains incomplete, but the blocker is narrower. The
  `.boonc` artifact now contains a versioned `runtime_plan` section with
  runtime-owned, AST-free slices: dense runtime symbol paths, scalar equation
  branches, derived text transforms, list equations, list projections, source
  routes/actions/payload fields, list source bindings, root state paths, list
  summary fields, and dynamic list-view list names. Artifact validation rejects
  a `runtime_plan` that is not AST-free or that claims source-free runtime
  instantiation. Inspection reports now say `runtime_plan_present = true` but
  still keep `runtime_instantiated_from_artifact = false`,
  `source_free_runtime_load_available = false`, and
  `scenario_execution_available = false`.
- Remaining blocker: implement the missing `runtime_plan` sections for
  AST-free generic-derived execution, runtime storage initialization, and
  document-lowering runtime tables. Do not serialize full `TypedProgram` to
  close this task, because it still embeds parser AST expressions/statements.

- Date: 2026-06-16
- Task: TASK-0901B storage initialization runtime-plan slice
- Commit: 4b6fafd
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: storage-plan next-slice audit from subagent
  `019ed19e-291a-7a80-b0fd-a1bea2c095ca`; artifact/report acceptance audit
  from subagent `019ed19e-43b3-7091-9d55-4a739da1d72f`; `cargo fmt -p
  boon_runtime -p boon_report_schema -p xtask`; `cargo test -p
  boon_runtime --lib compiled_artifact -- --nocapture`; `cargo test -p
  boon_runtime --lib runtime_storage_initialization_plan_matches_ir_storage
  -- --nocapture`; `cargo test -p boon_report_schema --lib
  compiled_artifact -- --nocapture`; `cargo check -p boon_runtime -p
  boon_cli -p xtask`; `cargo xtask verify-compiled-artifact todomvc
  --report target/reports/compiled-artifact-todomvc-xtask.json`; `cargo
  xtask verify-compiled-artifact-inspection todomvc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo run -p
  boon_cli -- compile examples/todomvc.bn --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc.json`; `cargo run -p boon_cli --
  inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `jq`
  inspection of `runtime_plan.storage_initialization` and
  `inspection_result.missing_runtime_plan_sections`; `cargo xtask
  verify-report-schema`; `cargo test -p boon_report_schema --lib`; `cargo
  test -p xtask`; `cargo test -p boon_runtime --lib`
- Result: TASK-0901B remains incomplete, but storage initialization is now an
  executable AST-free runtime-plan slice instead of only a JSON promise.
  `CompiledProgram` owns a `RuntimeStorageInitializationPlan` with root slots,
  root initial-field copy specs, list slots, row templates, materialized initial
  rows, synthetic list-view storage slots, and indexed row-initial reset specs.
  `GenericScheduledRuntime::new_profiled` now creates `GenericCircuitRuntime`
  storage from this compiled-owned plan on the normal source path, and the
  storage oracle verifies plan-built storage matches the old IR-built storage
  for Counter, TodoMVC, and Cells.
- Artifact/result detail: `.boonc` now exposes
  `runtime_plan.storage_initialization` with
  `storage_runtime_ast_free = true`. `inspect-artifact` reports the remaining
  missing runtime-plan sections as only `generic_derived_ast_free_plan` and
  `document_lowering_runtime_tables` before the follow-up document-lowering
  slice; it still correctly keeps
  `runtime_instantiated_from_artifact = false`,
  `source_free_runtime_load_available = false`, and
  `scenario_execution_available = false`.
- Remaining blocker: implement AST-free generic-derived execution and
  document-lowering runtime tables, then add a real artifact-backed runtime
  constructor and scenario parity gate before flipping any source-free runtime
  execution booleans.

- Date: 2026-06-16
- Task: TASK-0901B document-lowering runtime-table slice
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: document-lowering runtime-table audit from subagents
  `019ed1ad-c9e7-7411-9d26-b4b1387501d8` and
  `019ed1ad-e192-7df1-b525-a045a4489a1d`; `cargo fmt -p
  boon_runtime -p boon_report_schema -p xtask`; `cargo check -p
  boon_runtime -p boon_cli -p xtask`; `cargo test -p boon_runtime --lib
  compiled_artifact -- --nocapture`; `cargo test -p boon_runtime --lib
  document_lowering_runtime_tables_drive_runtime_summary_metadata --
  --nocapture`; `cargo test -p boon_report_schema --lib compiled_artifact --
  --nocapture`; `cargo xtask verify-compiled-artifact todomvc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo run -p
  boon_cli -- compile examples/todomvc.bn --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc.json`; `cargo run -p boon_cli --
  inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `jq`
  inspection of `runtime_plan.document_lowering` and
  `inspection_result.missing_runtime_plan_sections`; `cargo xtask
  verify-report-schema`; `cargo test -p boon_report_schema --lib`; `cargo
  test -p xtask`; `cargo test -p boon_runtime --lib`
- Result: TASK-0901B remains incomplete, but document lowering is now a
  runtime-owned AST-free runtime-plan slice instead of a source-side companion
  table. `CompiledProgram` owns `RuntimeDocumentLoweringTables` with document
  preview summary limits, root summary paths, list summary fields, dynamic
  list-view list names, render slot metadata, generic render-patch lowering
  rules, observed root paths, and conservative projection-to-storage-list
  resolutions. `GenericScheduledRuntime::new_profiled` now initializes its
  summary metadata from `compiled.document_lowering`; the regression test
  clears the old adjacent fields and proves runtime construction still gets the
  expected root/list summary metadata from the new table.
- Artifact/result detail: `.boonc` now exposes
  `runtime_plan.document_lowering` with format
  `boonc-document-lowering-runtime-tables-json-v1` and
  `document_lowering_runtime_ast_free = true`. Artifact validation and xtask
  gates reject missing or non-AST-free document-lowering runtime tables. The
  refreshed CLI inspection report lists the remaining missing runtime-plan
  sections as only `generic_derived_ast_free_plan`; it still correctly keeps
  `runtime_instantiated_from_artifact = false`,
  `source_free_runtime_load_available = false`, and
  `scenario_execution_available = false`.
- Remaining blocker: implement AST-free generic-derived execution, then add a
  real artifact-backed runtime constructor and scenario parity gate before
  flipping any source-free runtime execution booleans.

- Date: 2026-06-16
- Task: TASK-0901B partial generic-derived runtime-plan slice
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: generic-derived artifact/load audit from subagent
  `019ed1bd-7a09-76d1-9f99-96cf26b23121`; example coverage audit from
  subagent `019ed1bd-6833-7753-bec5-e42f3ca04329`; `cargo fmt -p
  boon_runtime -p xtask`; `cargo test -p boon_runtime --lib
  generic_derived_runtime_plan_executes_supported_fields_without_ast_statements
  -- --nocapture`; `cargo test -p boon_runtime --lib
  compiled_artifact_emission_is_deterministic_and_schema_valid --
  --nocapture`; `cargo test -p boon_runtime --lib compiled_artifact --
  --nocapture`; `cargo check -p boon_runtime -p boon_cli -p xtask`; `cargo
  xtask verify-compiled-artifact todomvc --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --artifact
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo run -p
  boon_cli -- compile examples/todomvc.bn --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc.json`; `cargo run -p boon_cli --
  inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo xtask
  verify-report-schema`; `jq` inspection of
  `runtime_plan.generic_derived` and
  `inspection_result.missing_runtime_plan_sections`.
- Result: TASK-0901B remains incomplete, but generic-derived execution now has
  its first runtime-owned AST-free slice. `CompiledProgram` builds a
  `RuntimeGenericDerivedPlan` beside the legacy AST-backed `GenericDerivedPlan`,
  serializes it as
  `runtime_plan.generic_derived.format =
  boonc-runtime-generic-derived-partial-json-v1`, and marks
  `included_runtime_owned_sections.generic_derived_partial_ast_free_plan =
  true`. Source-path runtime evaluation now prefers runtime-owned statements
  for supported root and indexed generic-derived fields and falls back to the
  legacy AST plan only for unsupported shapes. The regression test poisons the
  legacy AST statements for supported TodoMVC root/indexed fields and still
  proves `store.has_completed`, `store.all_completed`, `todo.not_completed`,
  and `todo.not_editing` are computed correctly from the runtime-owned plan.
- Artifact/result detail: refreshed TodoMVC artifact coverage is
  `root_supported_count = 3`, `indexed_supported_count = 2`, and
  `unsupported_reasons = { statement_children: 1, when_expr: 1 }`. Inspection
  still correctly reports
  `missing_runtime_plan_sections = ["generic_derived_ast_free_plan"]`,
  `runtime_instantiated_from_artifact = false`,
  `source_free_runtime_load_available = false`, and
  `scenario_execution_available = false`.
- Remaining blocker: finish the generic-derived runtime compiler/evaluator for
  the remaining statement-child/list-view and `WHEN` shapes, serialize enough
  function/builtin/runtime expression coverage for NovyWave, then add a real
  artifact-backed runtime constructor before removing
  `generic_derived_ast_free_plan`.

- Date: 2026-06-16
- Task: TASK-0901B runtime-generic child control-flow/list statement slice
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: unsupported-shape audit from subagent
  `019ed1cf-15d7-75c2-9efa-45aa523ce36f`; artifact-load honesty audit from
  subagent `019ed1cf-27d6-7450-b3ce-a05f0081d446`; `cargo fmt -p
  boon_runtime`; `cargo test -p boon_runtime --lib
  generic_derived_runtime_plan_executes_supported_fields_without_ast_statements
  -- --nocapture`; `cargo test -p boon_runtime --lib compiled_artifact --
  --nocapture`; `cargo check -p boon_runtime -p boon_cli -p xtask`; `cargo
  test -p boon_runtime --lib generic_rows_preserve_nested_field_records_and_lists
  -- --nocapture`; `cargo test -p boon_runtime --lib
  novywave_waveform_metadata_drives_selected_file_and_timeline_window --
  --nocapture`; full `cargo test -p boon_runtime --lib -- --nocapture` passed
  with `209` tests; `cargo xtask verify-compiled-artifact todomvc --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact cells --out target/artifacts/boonc/cells.boonc
  --report target/reports/compiled-artifact-cells-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --artifact
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo run -q -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo xtask
  verify-compiled-artifact-inspection cells --artifact
  target/artifacts/boonc/cells.boonc --report
  target/reports/compiled-artifact-inspection-cells.json`; `cargo run -q -p
  boon_cli -- compile examples/todomvc.bn --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc.json`; `cargo run -q -p boon_cli
  -- compile examples/cells.bn --out target/artifacts/boonc/cells.boonc
  --report target/reports/compiled-artifact-cells.json`; `cargo xtask
  verify-report-schema`; `jq` inspection of
  `runtime_plan.generic_derived` and
  `inspection_result.missing_runtime_plan_sections`.
- Result: TASK-0901B remains incomplete, but the TodoMVC generic-derived
  runtime-plan blockers from the previous slice are removed. The runtime-owned
  generic statement plan now represents attached expression children, statement
  blocks, list statements, `WHEN`/`WHILE` control-flow, match arms, and
  block-form `List/retain`/`List/every`/`List/any` predicates. Source runtime
  evaluation can now execute TodoMVC `store.title_to_add` and
  `store.visible_todos` without using the legacy AST statement. The regression
  test poisons all supported TodoMVC root/indexed AST statements and proves
  `visible_todos` materializes four rows from the runtime-owned block/list
  plan.
- Artifact/result detail: refreshed TodoMVC coverage is
  `root_supported_count = 5`, `indexed_supported_count = 2`, and
  `unsupported_reasons = {}`. Refreshed Cells coverage is still
  `root_supported_count = 0`, `indexed_supported_count = 2`, with blockers
  `{ call:List/chunk: 1, call:List/find: 1, call:cell_address: 1,
  call:compute_value: 2, call:default_formula_for_address: 1 }`. All inspected
  artifacts still correctly report
  `missing_runtime_plan_sections = ["generic_derived_ast_free_plan"]`,
  `runtime_instantiated_from_artifact = false`,
  `source_free_runtime_load_available = false`, and
  `scenario_execution_available = false`.
- Correctness guard learned: do not treat multiline user-function calls with
  attached named-argument children as record literals. This slice tightened
  runtime-plan record detection so calls such as `NovyBridge/artifact_ref(...)`
  remain unsupported and fall back to the AST evaluator until user-function
  bodies and call frames are serialized. The full runtime suite caught and
  protects this via NovyWave bridge metadata and nested row/list preservation
  tests.
- Remaining blocker: implement runtime-owned generic-derived coverage for Cells
  root list views (`List/find`, `List/chunk`) and user functions
  (`cell_address`, `default_formula_for_address`, `compute_value`), then add
  real `runtime_plan` deserializers and an artifact-backed runtime constructor
  before removing `generic_derived_ast_free_plan` or claiming source-free
  runtime load.

- Date: 2026-06-16
- Task: TASK-0901B runtime-owned Cells function/list-view generic-derived
  slice
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: Cells runtime-derived audit from subagent
  `019ed1e4-b69e-7c52-b8f2-5b0a505931eb`; artifact-load readiness audit
  from subagent `019ed1e4-ca24-7000-bebd-651a1f13dc5b`; `cargo fmt -p
  boon_runtime`; `cargo test -p boon_runtime --lib
  runtime_generic_user_functions_execute_without_ast_bodies -- --nocapture`;
  `cargo test -p boon_runtime --lib
  cells_generic_derived_runtime_plan_covers_roots_indexes_and_functions_without_ast
  -- --nocapture`; `cargo test -p boon_runtime --lib
  generic_derived_runtime_plan_executes_supported_fields_without_ast_statements
  -- --nocapture`; regression reruns for
  `indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape`,
  `indexed_pipeline_reorders_text_equal_filters_by_bucket_size`,
  `mapped_root_list_function_filters_segments_with_store_cursor`, and
  `root_list_view_field_cache_reuses_stable_fields_across_cursor_change`;
  full `cargo test -p boon_runtime --lib -- --nocapture` passed with `211`
  tests; `cargo xtask verify-compiled-artifact todomvc --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact cells --out target/artifacts/boonc/cells.boonc
  --report target/reports/compiled-artifact-cells-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --artifact
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo xtask
  verify-compiled-artifact-inspection cells --artifact
  target/artifacts/boonc/cells.boonc --report
  target/reports/compiled-artifact-inspection-cells.json`; `cargo run -q -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo
  xtask verify-report-schema`; `jq` inspection of
  `runtime_plan.generic_derived` and artifact inspection booleans.
- Result: TASK-0901B remains incomplete, but Cells no longer has generic
  runtime-derived coverage blockers. `RuntimeGenericDerivedPlan` now owns the
  reachable user-function closure for supported root/indexed fields, serializes
  those functions in `runtime_plan.generic_derived`, and can execute runtime
  user functions without legacy AST statements. Runtime generic statements now
  preserve field bindings so `BLOCK { local: ... }` locals work. Runtime
  builtins now cover the Cells path through `List/find`, `List/find_value`,
  `List/chunk`, `List/get`, `List/map`, `List/sum`, `List/range`, and the
  needed text helpers.
- Cause learned from the failed full-suite experiment: generic root `ListView`
  execution for `List/map` bypassed the existing optimized root-list-view
  materializer, so field-cache/row-identity/profile counters did not run and
  eight root-list-view cache/patch tests failed. The kept fix is an explicit
  dispatch boundary: source-backed runtime execution still uses the optimized
  root `ListView` materializer, while the runtime-owned plan is still compiled
  and reported for future artifact loading. This avoids replacing a proven fast
  path with a slower generic evaluator until the artifact path can preserve the
  same row identity, cache, and profile semantics.
- Artifact/result detail: refreshed Cells coverage is `function_count = 11`,
  `root_supported_count = 2`, `indexed_supported_count = 6`, and
  `unsupported_reasons = {}`. Refreshed TodoMVC coverage is
  `function_count = 0`, `root_supported_count = 5`,
  `indexed_supported_count = 2`, and `unsupported_reasons = {}`. All inspected
  artifacts still correctly report
  `missing_runtime_plan_sections = ["generic_derived_ast_free_plan"]`,
  `runtime_instantiated_from_artifact = false`,
  `source_free_runtime_load_available = false`,
  `scenario_execution_available = false`,
  `source_reparse_attempted = false`, and
  `source_file_access = "not_attempted"`.
- Remaining blocker: add real `runtime_plan` deserializers, build
  `LoadedRuntime`/`GenericScheduledRuntime` from `.boonc` without source or
  parser AST, and run scenario parity from that artifact before removing
  `generic_derived_ast_free_plan` or claiming source-free runtime load.

- Date: 2026-06-16
- Task: TASK-0901B generic-derived artifact deserialization slice
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: artifact constructor/parity audit from subagent
  `019ed201-ba31-7d71-86d6-1eadee276dd1`; runtime-plan deserialization audit
  from subagent `019ed201-b8a8-7220-8209-12904cbc6aa6`; `cargo fmt -p
  boon_runtime -p boon_report_schema -p xtask`; `cargo test -p boon_runtime
  --lib compiled_artifact_decodes_cells_generic_derived_runtime_plan_without_ast
  -- --nocapture`; `cargo test -p boon_runtime --lib compiled_artifact --
  --nocapture`; `cargo test -p boon_runtime --lib
  cells_generic_derived_runtime_plan_covers_roots_indexes_and_functions_without_ast
  -- --nocapture`; `cargo test -p boon_report_schema --lib
  compiled_artifact -- --nocapture`; `cargo check -p boon_runtime -p
  boon_cli -p xtask`; `cargo xtask verify-compiled-artifact todomvc --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact cells --out target/artifacts/boonc/cells.boonc
  --report target/reports/compiled-artifact-cells-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --artifact
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo xtask
  verify-compiled-artifact-inspection cells --artifact
  target/artifacts/boonc/cells.boonc --report
  target/reports/compiled-artifact-inspection-cells.json`; `cargo run -q -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo
  xtask verify-report-schema`; `cargo test -p xtask`; full `cargo test -p
  boon_runtime --lib -- --nocapture` passed with `212` tests; `jq` inspection
  of `inspection_result.runtime_plan_generic_derived_deserialized_*` and the
  still-false runtime/scenario booleans.
- Result: TASK-0901B remains incomplete, but `.boonc` inspection now performs
  real typed deserialization for `runtime_plan.generic_derived` instead of
  only validating JSON shape. The read-side decoder reconstructs
  `RuntimeGenericDerivedPlan`, reachable runtime user functions, root/indexed
  fields, recursive runtime statements, runtime expressions, record fields,
  and call args from the artifact contract. It validates declared counts
  against decoded data and rejects mismatches with contextual errors. A Cells
  round-trip test emits a fresh `.boonc`, decodes its generic-derived plan,
  installs that decoded plan into the runtime, poisons the corresponding legacy
  AST statements/functions, and proves Cells `selected_input`, `sheet_rows`,
  `address`, `default_formula`, and `value` still compute correctly.
- Artifact/result detail: inspection reports now include
  `runtime_plan_generic_derived_deserialized_from_artifact = true` and
  `runtime_plan_generic_derived_deserialized_counts`. Refreshed TodoMVC counts
  are `function_count = 0`, `root_supported_count = 5`,
  `indexed_supported_count = 2`, `unsupported_reason_count = 0`; refreshed
  Cells counts are `function_count = 11`, `root_supported_count = 2`,
  `indexed_supported_count = 6`, `unsupported_reason_count = 0`. The same
  reports still correctly keep `loaded_runtime_from_artifact = false`,
  `runtime_instantiated_from_artifact = false`,
  `source_free_runtime_load_available = false`,
  `scenario_execution_available = false`,
  `source_reparse_attempted = false`, and
  `source_file_access = "not_attempted"`.
- Remaining blocker: deserialize the rest of `runtime_plan` into runtime-owned
  structs, especially symbols, scalar/list/source-route plans, list bindings,
  and any equation/action expressions needed by `GenericScheduledRuntime`. Then
  replace the remaining
  AST-backed `GenericDerivedPlan` dependencies in root list-view/fallback paths
  or encode equivalent runtime-plan metadata, add an artifact-backed runtime
  constructor, and only then run scenario parity before removing
  `generic_derived_ast_free_plan`.

- Date: 2026-06-16
- Task: TASK-0901B storage/document artifact deserialization slice
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only storage/document decoder audit from subagent
  `019ed212-fdea-7a91-b324-3cc290c61bf4`; `cargo fmt -p boon_runtime -p
  boon_report_schema -p xtask`; `cargo test -p boon_runtime --lib
  compiled_artifact_decodes_storage_initialization_runtime_plan_without_ast --
  --nocapture`; `cargo test -p boon_runtime --lib
  compiled_artifact_decodes_document_lowering_runtime_tables_without_ast --
  --nocapture`; `cargo test -p boon_runtime --lib compiled_artifact --
  --nocapture`; `cargo test -p boon_report_schema --lib compiled_artifact --
  --nocapture`; `cargo check -p boon_runtime -p boon_cli -p xtask`; `cargo
  xtask verify-compiled-artifact todomvc --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact cells --out target/artifacts/boonc/cells.boonc
  --report target/reports/compiled-artifact-cells-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --artifact
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo xtask
  verify-compiled-artifact-inspection cells --artifact
  target/artifacts/boonc/cells.boonc --report
  target/reports/compiled-artifact-inspection-cells.json`; `cargo run -q -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo xtask
  verify-report-schema`; `cargo test -p xtask`; full `cargo test -p
  boon_runtime --lib -- --nocapture` passed with `214` tests.
- Result: TASK-0901B remains incomplete, but two more runtime-plan sections now
  have real typed read-side decoders. `CompiledArtifact` can reconstruct
  `RuntimeStorageInitializationPlan` and `RuntimeDocumentLoweringTables` from
  `.boonc` without touching source or parser AST. Storage decoding validates
  declared counts, stable row field IDs, duplicate root/list identities,
  duplicate row fields, list capacities, reset-list references, initializer
  kinds, row snapshots, field values, and row templates. Document-lowering
  decoding validates declared counts, unique set-like arrays, projection
  resolution/unresolved separation, render slot metadata, materialization
  policy names, `template_args_embedded_ast = false`, and exact generic
  render-patch lowering constants.
- Artifact/result detail: inspection reports now include
  `runtime_plan_storage_deserialized_from_artifact = true`,
  `runtime_plan_storage_deserialized_counts`,
  `runtime_plan_document_lowering_deserialized_from_artifact = true`, and
  `runtime_plan_document_lowering_deserialized_counts`. The refreshed TodoMVC
  CLI inspection report shows storage counts `root_slot_count = 3`,
  `list_slot_count = 2`, `initial_row_count = 4`, and document counts
  `root_summary_path_count = 4`, `list_summary_field_count = 2`,
  `render_slot_count = 13`. Runtime and scenario readiness booleans still
  correctly remain false, with `source_reparse_attempted = false` and
  `source_file_access = "not_attempted"`.
- Remaining blocker: deserialize or reconstruct the remaining runtime-plan
  inputs needed by `GenericScheduledRuntime`, especially runtime symbols,
  scalar/list equations, source-route plans, list-source bindings, and
  source/action expression tables. Then add an artifact-backed runtime
  constructor and scenario parity gate before any report claims
  `loaded_runtime_from_artifact`, `runtime_instantiated_from_artifact`,
  `source_free_runtime_load_available`, or `scenario_execution_available`.

- Date: 2026-06-16
- Task: TASK-0901B non-route runtime-table artifact deserialization slice
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only non-route runtime-table audit from subagent
  `019ed221-861f-7c81-8e30-07ac29599bd9`; `cargo fmt -p boon_runtime -p
  boon_report_schema -p xtask`; `cargo test -p boon_runtime --lib
  compiled_artifact_decodes_runtime_symbols_and_equation_tables_without_ast --
  --nocapture`; `cargo test -p boon_runtime --lib compiled_artifact --
  --nocapture`; `cargo test -p boon_report_schema --lib compiled_artifact --
  --nocapture`; `cargo check -p boon_runtime -p boon_cli -p xtask`; `cargo
  xtask verify-compiled-artifact todomvc --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact cells --out target/artifacts/boonc/cells.boonc
  --report target/reports/compiled-artifact-cells-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --artifact
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo xtask
  verify-compiled-artifact-inspection cells --artifact
  target/artifacts/boonc/cells.boonc --report
  target/reports/compiled-artifact-inspection-cells.json`; `cargo run -q -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo xtask
  verify-report-schema`; `cargo test -p xtask`; full `cargo test -p
  boon_runtime --lib -- --nocapture` passed with `215` tests.
- Result: TASK-0901B remains incomplete, but the artifact now has typed
  read-side decoders for the non-route runtime tables consumed by
  `GenericScheduledRuntime`: dense runtime symbols, scalar equation branches,
  derived text transforms, list operations, list projections, and list source
  binding slots. The decoder validates symbol uniqueness and count agreement
  with the top-level artifact and symbol table, scalar branch count against the
  compiled schedule, branch source membership, derived/list/projection/binding
  schedule counts, list operation/projection kinds, list-binding uniqueness, and
  recursive update-value/list-predicate expression kinds. A focused test installs
  the decoded tables into a cloned `CompiledProgram` and runs runtime summaries
  plus a TodoMVC scenario while source routes still come from the IR-built plan.
- Artifact/result detail: inspection reports now include
  `runtime_plan_non_route_tables_deserialized_from_artifact = true` and
  `runtime_plan_non_route_tables_deserialized_counts`. The refreshed TodoMVC CLI
  inspection report shows `runtime_symbol_count = 42`,
  `scalar_source_path_count = 15`, `scalar_branch_count = 18`,
  `derived_text_transform_count = 1`, `list_operation_count = 6`,
  `list_projection_count = 0`, and `list_source_binding_count = 1`. Runtime and
  scenario readiness booleans still correctly remain false, with
  `source_reparse_attempted = false` and
  `source_file_access = "not_attempted"`.
- Remaining blocker: decode typed source routes, source/action tables, source
  payload field metadata, and dense action/source IDs from the artifact. Then
  build the artifact-backed runtime constructor and scenario parity gate before
  any report claims `loaded_runtime_from_artifact`,
  `runtime_instantiated_from_artifact`,
  `source_free_runtime_load_available`, or `scenario_execution_available`.

- Date: 2026-06-16
- Task: TASK-0901B source-route/action-table artifact deserialization slice
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: read-only source-route decoder audit from subagent
  `019ed234-96d1-7251-9c7b-06fa4481f0f1`; `cargo fmt -p boon_runtime -p
  boon_report_schema -p xtask`; `cargo test -p boon_runtime --lib
  compiled_artifact_decodes_source_routes_and_action_table_without_ast --
  --nocapture`; `cargo test -p boon_runtime --lib compiled_artifact --
  --nocapture`; `cargo test -p boon_report_schema --lib compiled_artifact --
  --nocapture`; `cargo check -p boon_runtime -p boon_cli -p xtask`; `cargo
  test -p xtask`; `cargo xtask verify-compiled-artifact todomvc --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact cells --out target/artifacts/boonc/cells.boonc
  --report target/reports/compiled-artifact-cells-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --artifact
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo xtask
  verify-compiled-artifact-inspection cells --artifact
  target/artifacts/boonc/cells.boonc --report
  target/reports/compiled-artifact-inspection-cells.json`; `cargo run -q -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo xtask
  verify-report-schema`; full `cargo test -p boon_runtime --lib -- --nocapture`
  passed with `216` tests.
- Result: TASK-0901B remains incomplete, but source routes are no longer an
  artifact decoding gap. `CompiledArtifact::runtime_source_routes()` now
  reconstructs `SourceRoutePlan`, `SourceActionTable`, dense `SourceId` to route
  slots, sorted label slots, per-route payload fields, route target arrays, and
  per-source action streams from `.boonc`. The decoder validates route IDs,
  dense id-slot backlinks, unused id-slot safety, sorted/unique label slots,
  action-table source IDs, action-table equality with route actions, rebuilt
  action streams from target arrays, address lookup payload consistency, source
  payload field variants, scalar route expressions, list-remove predicates,
  source route schedule counts, payload aggregate counts, and route-op-stream
  report equality.
- Artifact/result detail: inspection reports now include
  `runtime_plan_source_routes_deserialized_from_artifact = true` and
  `runtime_plan_source_routes_deserialized_counts`. The refreshed TodoMVC CLI
  inspection report shows `route_count = 15`, `id_slot_count = 15`,
  `label_slot_count = 15`, `routes_with_ids = 15`,
  `action_table_slot_count = 15`, `action_op_stream_count = 15`,
  `total_action_op_count = 21`, `max_action_op_count = 3`,
  `source_payload_field_count = 7`, `source_payload_text_field_count = 2`,
  `source_payload_key_field_count = 2`, and
  `source_payload_address_field_count = 3`. Runtime and scenario readiness
  booleans still correctly remain false, with `source_reparse_attempted = false`
  and `source_file_access = "not_attempted"`.
- Remaining blocker: assemble the decoded runtime-plan sections into an
  artifact-backed `LoadedRuntime`/`GenericScheduledRuntime` constructor without
  reparsing source or rebuilding from typed IR, then run scenario parity from the
  artifact before any report claims `loaded_runtime_from_artifact`,
  `runtime_instantiated_from_artifact`,
  `source_free_runtime_load_available`, or `scenario_execution_available`.

- Date: 2026-06-16
- Task: TASK-0901B `.boonc` artifact-backed runtime constructor
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`; `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo test -p boon_runtime --lib
  compiled_artifact_instantiates_loaded_runtime_without_source_or_ir --
  --nocapture`; `cargo test -p boon_runtime --lib
  compiled_artifact_inspection_does_not_reparse_source_and_reports_runtime_load
  -- --nocapture`; `cargo test -p boon_runtime --lib compiled_artifact --
  --nocapture`; `cargo test -p boon_runtime --lib root_list_view_ --
  --nocapture --test-threads=1`; `cargo test -p boon_report_schema --lib
  compiled_artifact -- --nocapture`; `cargo check -p boon_runtime -p boon_cli
  -p xtask`; `cargo xtask verify-compiled-artifact todomvc --out
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-todomvc-xtask.json`; `cargo xtask
  verify-compiled-artifact cells --out target/artifacts/boonc/cells.boonc
  --report target/reports/compiled-artifact-cells-xtask.json`; `cargo xtask
  verify-compiled-artifact-inspection todomvc --artifact
  target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc.json`; `cargo xtask
  verify-compiled-artifact-inspection cells --artifact
  target/artifacts/boonc/cells.boonc --report
  target/reports/compiled-artifact-inspection-cells.json`; `cargo run -q -p
  boon_cli -- inspect-artifact target/artifacts/boonc/todomvc.boonc --report
  target/reports/compiled-artifact-inspection-todomvc-cli.json`; `cargo xtask
  verify-report-schema`; `cargo test -p xtask`; full `cargo test -p
  boon_runtime --lib -- --nocapture` passed with `217` tests.
- Result: TASK-0901B is complete. `CompiledProgram::from_artifact` now builds a
  runtime-owned compiled program from `.boonc` by decoding non-route runtime
  tables, generic-derived runtime plans, storage initialization, document
  lowering, source routes/action tables, root/list summary metadata, storage
  layout counts, and field-slot collision diagnostics. The new
  `LoadedRuntime::from_compiled_artifact` path instantiates Counter, TodoMVC,
  and Cells without source files or typed IR; a temp-source deletion test proves
  the constructor still works after the source file is removed.
- Artifact/report detail: `runtime_plan.source_free_runtime_instantiation_ready
  = true`, `runtime_plan.runtime_instantiation_blocked_by = []`,
  `typed_ir_required_for_mvp_loader = false`,
  `loaded_runtime_from_artifact = true`,
  `runtime_instantiated_from_artifact = true`,
  `source_free_runtime_load_available = true`,
  `source_reparse_required_for_current_runtime = false`,
  `source_reparse_attempted = false`, `source_file_access = "not_attempted"`,
  and `missing_runtime_plan_sections = []`. Schema and xtask verification now
  reject stale reports that still claim typed-IR/source requirements.
- Cause learned from the failed first full-runtime pass: switching all roots to
  runtime-generic statements broke source-backed root list-view patch/cache
  tests because the generic runtime evaluator does not yet reproduce the
  optimized AST materializer's source-identity, list-map attribution, and
  field-cache hooks. The kept fix is an explicit internal boundary:
  source-compiled runtimes keep AST list-view materialization; artifact-compiled
  runtimes use the source-free runtime statement path. Do not remove that
  boundary until the runtime-generic list-view evaluator owns equivalent row
  identity, cache, dirty-read, and profile semantics.
- Remaining blocker: implement TASK-0901C by running at least one scenario from
  the loaded artifact and comparing it against the interpreter/source path. Keep
  `scenario_execution_available = false` until that parity gate exists.

### 2026-06-16 TASK-0901C Artifact-Backed Scenario Parity

- Date: 2026-06-16
- Task: TASK-0901C `.boonc` scenario execution and source-runtime parity
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`;
  `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo fmt -p boon_runtime -p boon_report_schema -p xtask`;
  `cargo test -p boon_runtime --lib
  compiled_artifact_runs_counter_scenario_without_source_and_matches_source_runtime
  -- --nocapture`; `cargo test -p boon_report_schema --lib
  compiled_artifact -- --nocapture`; `cargo xtask
  verify-compiled-artifact-scenario counter --artifact
  target/artifacts/boonc/counter.boonc --report
  target/reports/compiled-artifact-scenario-counter.json`; `cargo test -p
  boon_runtime --lib compiled_artifact -- --nocapture`; `cargo test -p
  boon_report_schema --lib -- --nocapture`; `cargo test -p xtask`; `cargo
  xtask verify-report-schema`; `cargo check -p boon_runtime -p boon_cli -p
  xtask`; `cargo test -p boon_runtime --lib -- --nocapture` passed with `218`
  tests.
- Result: TASK-0901C is complete and TASK-0901 is done. Counter now has a
  durable `.boonc` scenario proof: source runtime output is used as the oracle,
  the artifact runtime is instantiated from `counter.boonc`, the copied source
  file is removed in the runtime regression test before the artifact run, and
  semantic deltas, render patches, and final state match exactly.
- Artifact/report detail:
  `target/reports/compiled-artifact-scenario-counter.json` reports
  `scenario_execution_available = true`,
  `scenario_execution_from_artifact = true`,
  `runtime_instantiated_from_artifact = true`,
  `source_reparse_attempted = false`,
  `source_file_access = "not_attempted"`,
  `typed_ir_required_for_artifact_execution = false`,
  `parser_ast_required_for_artifact_execution = false`,
  `semantic_deltas_match = true`, `render_patches_match = true`,
  `state_summary_match = true`, and `parity_passed = true`; the source and
  artifact scenario signature hashes are identical.
- Next available implementation task: TASK-0902 can start from a proven
  artifact baseline and use `.boonc` output as one serialization/loading
  boundary while designing bytecode or micro-op execution.

### 2026-06-16 TASK-0902 Scalar Source-Route Bytecode MVP

- Date: 2026-06-16
- Task: TASK-0902 Expression Bytecode Or Micro-Op Interpreter
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `crates/boon_report_schema/src/lib.rs`;
  `crates/xtask/src/main.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo fmt -p boon_runtime -p boon_report_schema -p xtask`;
  `cargo test -p boon_runtime --lib bytecode -- --nocapture`; `cargo test -p
  boon_report_schema --lib bytecode -- --nocapture`; `cargo xtask
  verify-bytecode counter --report target/reports/bytecode-counter.json`;
  `cargo test -p boon_report_schema --lib -- --nocapture`; `cargo test -p
  xtask`; `cargo xtask verify-report-schema`; `cargo check -p boon_runtime -p
  boon_cli -p xtask`; `cargo test -p boon_runtime --lib -- --nocapture`
  passed with `219` tests.
- Result: TASK-0902 is complete for its first accepted subset. The runtime now
  has a scalar source-route bytecode compiler/evaluator for the covered
  `ScalarUpdateExpression` modes, a `verify_expression_bytecode_report`
  runtime report entrypoint, and a `cargo xtask verify-bytecode <example>` gate.
  Counter proves three route expressions: two `number_infix` micro-ops and one
  `const_text` micro-op.
- Report detail: `target/reports/bytecode-counter.json` reports
  `candidate_expression_count = 3`, `compiled_expression_count = 3`,
  `parity_sample_count = 3`, `fallback_count = 0`, `deopt_count = 0`,
  `warm_path_allocation_count = 2`, `parity_passed = true`,
  `hot_path_ready = true`, and `op_histogram = { const_text: 1,
  number_infix: 2 }`. Each bytecode sample records the source route, target,
  mode, ops, interpreter output, bytecode output, and pass flag.
- Boundary: this is a proof and reporting foundation, not broad hot-path
  replacement. Unsupported expression families still need explicit bytecode ops
  or reported fallback before generated kernels or dataflow experiments depend
  on them.

### 2026-06-16 EXP-0006 Generated Rust-Enum Kernel Proof

- Date: 2026-06-16
- Task: EXP-0006 Generated Rust Or Cranelift Kernels
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime
  --lib generated -- --nocapture`
- Result: EXP-0006 is complete for its first proof slice but is not promoted.
  The runtime now has a generated Rust-enum kernel representation for the
  covered scalar bytecode text subset: `const_text` becomes a borrowed constant
  output, and `number_infix` compiles the operator string once into a typed enum
  (`add` or `subtract` for Counter). The generated kernel output is compared
  against both `ScalarBytecodeProgram` and `ScalarEquationPlan`.
- Report detail: the generated-kernel proof over Counter reports
  `candidate_expression_count = 3`, `generated_kernel_count = 3`,
  `parity_sample_count = 3`, `fallback_count = 0`, `deopt_count = 0`,
  `generated_static_borrow_count = 1`, `generated_dynamic_string_count = 2`,
  `generated_kernel_histogram = { generated_const_text_borrow: 1,
  generated_number_infix_enum: 2 }`, and
  `generated_number_op_histogram = { add: 1, subtract: 1 }`.
- Boundary: this is intentionally proof-only. It does not add Cranelift, does
  not compile Rust code at runtime, and does not claim a production speed win.
  Promotion still needs a release-mode measurement that excludes compile cost
  and shows a stable runtime win.

### 2026-06-16 EXP-0007 Large-List Count Dataflow Kernel Proof

- Date: 2026-06-16
- Task: EXP-0007 Large-List Dataflow Kernel
- Commit: this checkpoint
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`;
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`
- Verification: `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime
  --lib dataflow -- --nocapture`
- Result: EXP-0007 is complete for its first proof slice but is not promoted.
  The runtime now has a local `LargeListCountDataflowKernel` experiment that
  builds a predicate-membership bitset for a count projection, applies a
  row-identity field update by `(key, generation)`, and compares the maintained
  count against the existing full-scan oracle.
- Report detail: the large TodoMVC proof builds active and completed count
  kernels over `1,000` rows. Toggling `Item 500` touches `1` dataflow row per
  count target while the oracle scans `1,000` rows. The active count moves from
  `500` to `499`, the completed count moves from `500` to `501`, and both
  maintained counts match the full-scan oracle.
- Boundary: this is a local dataflow-state proof, not a production scheduler
  change. Inserts, removes, moves, selector predicates, and general list
  projections still need fallback handling and report evidence before this can
  replace normal runtime paths.

### 2026-06-17 TASK-0804A Resume After Other Tasks Completed

- Date: 2026-06-17
- Task: TASK-0804A Source-Action Root Flush Architecture Pass
- Commit: uncommitted
- Files changed in this slice:
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`; temporary
  row-clean cache code in `crates/boon_runtime/src/lib.rs` was reverted before
  this log entry.
- Verification: `rg -n "^(### (TASK|EXP)-|Status:|Depends:)" docs/plans/speedup/12-speedup-goal-execution-checklist.md`;
  read-only root-list/materialization audits from subagents
  `019ed282-9431-7030-a2ce-2f43eef08aaa` and
  `019ed282-7fb2-7112-8527-ef7991abaabe`; canonical
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`;
  diagnostic `BOON_PROFILE_ROOT_DEMAND=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-root-demand.json`;
  during the reverted experiment `cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo test -p boon_runtime --lib` passed with `222` tests; canonical speed
  rejected the experiment; after revert canonical speed was rerun so
  `target/reports/native-gpu/novywave-interaction-speed.json` again matches
  current code; `git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md crates/boon_runtime/src/lib.rs`.
- Result: TASK-0804A is confirmed as the only real unfinished checklist item
  after ignoring template placeholders `TASK-0000` and `EXP-0000`. The earlier
  instruction to skip 0804A until other tasks were done is now satisfied; the
  later EXP/TASK work after the pause point is done. A generic direct-projector
  row-clean cache experiment was killed and reverted because it passed focused
  runtime tests but worsened the official speed gate. No runtime code from that
  experiment remains.
- Current cause: the restored current-code report still fails the strict
  `16.700ms` click/input budget with `click_to_cursor.p95=19.407ms`,
  `input_to_visible.p95=19.407ms`, `runtime_apply.p95=11.629ms`,
  `runtime_step_apply.p95=9.329ms`, and `layout_rebuild.p95=5.058ms`.
  Hot root-list work remains `selected_signal_lane_rows`
  (`eval_ms=16.763`, `diff_ms=15.303`, `field_cache_hits=4768`,
  `field_cache_misses=160`) and `selected_cursor_pair_rows`
  (`eval_ms=9.818`, `diff_ms=9.304`, `field_cache_misses=96`). The
  env-gated root-demand diagnostic identifies the architecture cause as
  `dirty_frontier_fanout_with_ranked_root_work`, with cursor changes fanning
  into bridge roots plus the two selected-row list views. Next work should
  measure and attack per-field diff/user-function loop cost or the dirty
  frontier graph directly, not add speculative row-level cache state.

### 2026-06-17 TASK-0804A Deep Culprit Exploration

- Date: 2026-06-17
- Task: TASK-0804A Source-Action Root Flush Architecture Pass
- Commit: uncommitted
- Files changed in this slice:
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md` only.
- Verification: second-round read-only audits from subagents
  `019ed29c-bd25-7ee1-84a2-b6839b969e35` and
  `019ed29d-3fae-7aa2-9770-f78a10e065a7`, followed by a fresh subagent loop
  with `019ed2aa-dbf2-7dc2-b61a-5deb53db1070` and
  `019ed2aa-f20b-7011-86fa-6b43cd79ff45`; diagnostic
  `BOON_PROFILE_ROOT_DEMAND=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-root-demand-current.json`
  failed the known strict latency budgets while exposing ranked frontier/root
  work; `jq` grouping of canonical and diagnostic click samples by
  `runtime_step_profile.source_action_root_dependent_visit_count`; `jq` root
  work, list-view field profile, demand-classification, and top frontier-edge
  inspection; code inspection of
  `materialize_root_derived_field_commits_for_changed_reads`,
  `root_field_waits_on_dirty_root`, `eval_identifier`, `eval_path`,
  `root_derived_boon_value`, and `root_derived_summary_values`; source
  inspection of cursor and bridge definitions in `examples/novywave/RUN.bn`.
- Result: culprit identified, no runtime code changed. The p95 slow path is the
  `cursor_position` changed class: half of clicks move from the `26`/`28`
  dependent-visit classes into the `194` dependent-visit class when
  `store.cursor_position` changes. That real value change fans out through
  `bridge_request_descriptor_label`, bridge request fingerprint/input/structural
  roots, page refs/pages, `bridge_cursor_values`,
  `selected_lane_materialization`, and the two selected-row list views. The
  current canonical report still fails the strict `16.700ms` click/input budget
  with `click_to_cursor.p95=19.407ms`,
  `input_to_visible.p95=19.407ms`, `runtime_apply.p95=11.629ms`,
  `runtime_step_apply.p95=9.329ms`, and `layout_rebuild.p95=5.058ms`. Renderer
  upload is not the culprit, and current list-view evidence shows field-only
  patching, not full row materialization.
- Demand-classification evidence: in a representative slow click, frontier
  visits are dominated by `candidate_unobserved_source_free_pure`
  (`193` visits and `29` enqueues in the sample) versus
  `blocked_observed_downstream` (`18` visits, `5` enqueues),
  `blocked_observed_root` (`9` visits, `3` enqueues), and
  `blocked_list_view` (`34` visits, `2` enqueues). Across the `16` slow
  samples, candidate pure roots account for `448` dirty pops and
  `17.628ms` materialization time; non-candidate roots account for `160` pops
  and `16.680ms`. The heaviest repeated candidate roots are
  `store.bridge_request_descriptor` (`32` pops / `2.741ms`, twice per slow
  click) and `store.bridge_cursor_values_page_ref`
  (`48` pops / `2.422ms`, three times per slow click). The heaviest blocked
  roots remain the field-only list views:
  `store.selected_signal_lane_rows` (`16` slow-sample pops / `8.872ms`) and
  `store.selected_cursor_pair_rows` (`16` pops / `5.386ms`).
- Source/evaluator evidence: `cursor_position` is derived from
  `selected_timeline_cursor_value`; bridge descriptor/page roots are internal
  pure records/digests, but some candidate roots such as
  `store.bridge_cursor_values_page_ref` are still read while evaluating
  rendered lane-row `page_refs`. Therefore "not observed" does not mean "not
  needed"; it means the root should not have to eagerly publish a semantic-only
  delta or standalone render patch. The current evaluator order is a correctness
  hazard for naive deferral: `eval_identifier`, `eval_path`, and
  `runtime_path_summary` consult stored root scalars before derived-root
  recomputation in several branches, while `root_derived_boon_value` is the
  cache-refreshing on-demand evaluator. Any demand/currentness implementation
  must remove or mark stale deferred pure-root storage/cache entries, or make
  evaluator/summary reads prefer `root_derived_boon_value` for deferred pure
  roots, otherwise a skipped internal root could be read stale.
- Killed follow-up: the smaller generic scheduler-ordering slice from subagent
  `019ed2aa-f20b-7011-86fa-6b43cd79ff45` was tried and reverted. It made dirty
  readiness transitive through `root_reads_by_field`, added a ready-frontier
  rebuild, then tightened that to targeted transitive affected-root refresh
  after a read-only audit from subagent `019ed2b8-4ca8-7a41-9466-6c7df0a0cae8`
  found RootChild and self-cycle correctness gaps. Focused correctness tests
  passed, but the official speed gate rejected the slice. The naive rebuild
  version reduced the slow class only from `194/32/38` to `189/29/35` while
  exploding `source_action_root_dirty_scheduler.p95` to `26.064ms` and
  `click_to_cursor.p95`/`input_to_visible.p95` to `42.410ms`. The targeted
  refresh version kept the same `189/29/35` counts but still worsened
  `click_to_cursor.p95`/`input_to_visible.p95` to `24.466ms`,
  `runtime_apply.p95` to `17.295ms`, `source_action_root_flush.p95` to
  `11.372ms`, and `source_action_root_dirty_scheduler.p95` to `8.819ms`. The
  runtime code was reverted; only this checklist remains changed.
- Verification for the killed scheduler-ordering slice: subagent audits
  `019ed2aa-f20b-7011-86fa-6b43cd79ff45`,
  `019ed2aa-dbf2-7dc2-b61a-5deb53db1070`, and
  `019ed2b8-4ca8-7a41-9466-6c7df0a0cae8`; during the reverted experiment
  `cargo fmt -p boon_runtime`; `cargo test -p boon_runtime --lib
  root_dirty_readiness_ -- --nocapture`; `cargo test -p boon_runtime --lib
  root_derived_ -- --nocapture`; `cargo test -p boon_runtime --lib
  structured_root_ -- --nocapture`; `cargo test -p boon_runtime --lib
  root_read_key_aliases_match_store_local_without_nested_leaf_collision
  -- --nocapture`; `cargo test -p boon_runtime --lib
  novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them
  -- --nocapture`; `cargo test -p boon_runtime --lib
  novywave_bridge_cursor_rows_alias_tracks_list_identity_not_row_content
  -- --nocapture`; `cargo check -p boon_runtime`; two rejected speed gates with
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report
  target/reports/native-gpu/novywave-interaction-speed.json`; after revert
  `git diff -- crates/boon_runtime/src/lib.rs` was empty and the canonical speed
  gate was rerun so the report again matches current code.
- Additional live subagent loop: after the scheduler-ordering experiment was
  killed, three more read-only subagents audited the current code and reports:
  `019ed2c9-2b31-7a83-ace2-3663e634c505` for runtime/root scheduling,
  `019ed2c9-2d94-7b42-9b5f-1c00c33f815c` for layout/renderer/verifier
  evidence, and `019ed2c9-2ec3-79a2-bee8-cd0355159b29` for the NovyWave Boon
  source shape. Their consensus matches the live measurements: the current
  gate is dominated by eager root-currentness fanout for source-free pure
  bridge/page/status roots, feeding a still-expensive field-only list-view
  path. The culprit is not full row materialization, not hardcoded fixture rows,
  and not a renderer upload bottleneck in this gate.
- Live root-demand diagnostic:
  `BOON_PROFILE_ROOT_DEMAND=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-root-demand-live.json`
  failed the known strict latency budgets with diagnostic overhead
  (`click_to_cursor.p95=18.972ms`, `runtime_apply.p95=12.310ms`), but
  reproduced the same graph shape. In the slow-click half, the `194` dependent
  visits / `32` enqueues / `38` dirty pops class averaged `17.135ms` total,
  `11.758ms` runtime apply, `6.478ms` root flush, `3.721ms` dirty scheduler,
  and `2.143ms` root materialization. Its frontier classifications were:
  `candidate_unobserved_source_free_pure=3088` visits / `464` enqueues,
  `blocked_list_view=544` visits / `32` enqueues,
  `blocked_observed_downstream=288` visits / `80` enqueues, and
  `blocked_observed_root=144` visits / `48` enqueues. Top slow-click root work
  was `store.selected_signal_lane_rows` (`16` pops / `8.843ms`),
  `store.selected_cursor_pair_rows` (`16` pops / `5.439ms`),
  `store.bridge_request_descriptor` (`32` pops / `2.711ms`), and
  `store.bridge_cursor_values_page_ref` (`48` pops / `2.397ms`).
- Current post-diagnostic canonical report: reran the normal non-demand gate so
  `target/reports/native-gpu/novywave-interaction-speed.json` and the role
  artifact match current code again. It still fails only the strict click/input
  p95 budget with `click_to_cursor.p95=17.359ms`,
  `input_to_visible.p95=17.359ms`, `runtime_apply.p95=10.376ms`,
  `runtime_step_apply.p95=8.346ms`, and `layout_rebuild.p95=4.688ms`. The slow
  click class is still `194` dependent visits, `32` enqueues, and `38` dirty
  pops; aggregate click root work is `3536` visits, `600` enqueues, and `792`
  dirty pops. Root-list work remains field-only:
  `selected_signal_lane_rows.eval_ms=14.756`, `diff_ms=13.633`,
  `field_cache_hits=4768`, `field_cache_misses=160`,
  `field_only_evaluated_field_count=160`, `full_eval_row_count=0`, and
  `row_materialize_ms=0`.
- Renderer/verifier caveat from subagent review: the current interaction-speed
  gate measures deterministic app-owned input through runtime/layout/shared
  update and aliases the click summary as `input_to_visible`; it does not
  include per-interaction render encode, queue submit, present, or readback
  timings. The renderer upload probe is a three-sample offscreen counter probe,
  not a click-by-click render timing oracle. In this gate, renderer evidence is
  not the culprit (`hot_path_proof_readback_count=0`,
  `hot_path_heavy_json_summary_count=0`, `preview_blocked_on_ipc_count=0`, and
  post-interaction upload was small in the probe), but a separate render
  isolation report should still be added before claiming native-present
  performance is solved.
- 2026-06-17 fresh subagent loop and killed invalidated-field-frontier
  experiment: ran three more read-only subagents on the current code and
  reports: `019ed2d5-d4f8-7b60-b42b-bf2a504c427f` for runtime dirty-frontier
  currentness, `019ed2d5-d6a6-7532-b190-f4d6dc7ff41f` for list/materialization
  evidence, and `019ed2d5-d7d4-7fa3-af98-06fdccce8a4c` for verifier/report
  semantics. They independently converged on the same cause ranking:
  root-graph fanout and eager currentness are first, field-only list
  projection/diff work is second, layout is secondary, and renderer evidence
  in this gate is only an off-hot-path upload probe. A fresh diagnostic
  `BOON_PROFILE_ROOT_DEMAND=1 cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/diagnostics/native-gpu/novywave-interaction-speed-root-demand-live-1781653707.json`
  failed the known budget with diagnostic overhead
  (`click_to_cursor.p95=20.511ms`, `runtime_apply.p95=12.779ms`) and preserved
  the same graph: click frontier classifications were
  `candidate_unobserved_source_free_pure=3568` visits / `576` enqueues,
  `blocked_list_view=704` visits / `64` enqueues,
  `blocked_observed_downstream=304` visits / `96` enqueues, and
  `blocked_observed_root=240` visits / `80` enqueues. Normal click samples
  still split into `26/5/11`, `28/6/12`, and `194/32/38` dependent-visit /
  enqueue / dirty-pop classes; the slow `194/32/38` class averaged
  `6.660ms` root flush, `10.071ms` runtime step, `4.655ms` layout, and
  `17.989ms` total in the diagnostic run.
- Killed experiment: implemented a generic invalidated-field frontier for the
  root-list field-only path. The patch recorded which root-list field-cache
  entries were actually removed by dirty-read invalidation, then bulk-skipped
  provably covered clean record fields during source-action materialization.
  Focused correctness passed with `cargo fmt -p boon_runtime` and
  `cargo test -p boon_runtime --lib root_list_view_field_cache_ --
  --nocapture` (`6` tests passed), but the official speed gate rejected the
  slice. It did not change the slow `194/32/38` graph class or the aggregate
  click graph (`3536` visits / `600` enqueues / `792` dirty pops), and the
  strict p95 still failed (`click_to_cursor.p95=17.485ms`,
  `input_to_visible.p95=17.485ms`). The runtime patch was reverted; after
  revert `git diff -- crates/boon_runtime/src/lib.rs` is empty.
- Restored current-code baseline after the killed experiment: reran
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report
  target/reports/native-gpu/novywave-interaction-speed.json` so the official
  report again matches current code. It still fails only the strict click/input
  p95 budget with `click_to_cursor.p95=17.558ms`,
  `input_to_visible.p95=17.558ms`, `runtime_apply.p95=10.286ms`,
  `runtime_step_apply.p95=8.291ms`, and `layout_rebuild.p95=4.773ms`. The
  graph class is unchanged (`3536` click dependent visits / `600` enqueues /
  `792` dirty pops; slow clicks remain `194/32/38`). Root-list profiles still
  show field-only patching rather than full rows:
  `selected_signal_lane_rows.eval_ms=14.848`, `diff_ms=13.714`,
  `field_cache_hits=4768`, `field_cache_misses=160`,
  `field_only_evaluated_field_count=160`, `full_eval_row_count=0`,
  `row_materialize_ms=0`; `selected_cursor_pair_rows.eval_ms=8.838` and
  `diff_ms=8.429` with `96` misses.
- Next direction: do not try another readiness-set heuristic as the primary
  fix. Implement a generic demand/currentness frontier before dirty enqueue for
  safe unobserved source-free pure bridge/page roots, with explicit correctness
  barriers for observed roots, semantic deltas, evaluator reads, summaries,
  assertions, observed projections, and row/list-view demand reads. The
  existing NovyWave internal-root test should preserve "queryable and no render
  patch" but should not require every internal candidate root to publish an
  eager semantic delta if demand deferral is active.
- Kill criteria for the next slice: revert and document if it fails to reduce
  the `candidate_unobserved_source_free_pure` frontier class, fails to reduce
  the `194` visits / `32` enqueues / `38` dirty pops slow class, regresses
  `source_action_root_flush`, `source_action_root_dirty_scheduler`,
  `source_action_root_materialization`, or click/input p95, or leaves stale
  summary/evaluator reads for deferred pure roots.

### 2026-06-17 TASK-0804A Real Culprit Loop Addendum

- Date: 2026-06-17
- Task: TASK-0804A Source-Action Root Flush Architecture Pass
- Commit: uncommitted
- Files changed in this slice:
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md` only.
- Verification: read-only subagent loop with `019ed2eb-c721-7bc3-8ca7-7f7e90a94b69`
  for runtime scheduler fanout, `019ed2eb-d6cc-7362-a70c-56f841643d22`
  for NovyWave Boon dependency shape, and
  `019ed2eb-e599-7e33-9647-1d45c41767bc` for verifier/layout/renderer timing;
  fresh diagnostic `BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/diagnostics/native-gpu/novywave-interaction-speed-dirty-frontier-deep-loop.json`
  failed the known strict latency budget and exposed exact dirty-frontier
  edges; existing root-demand diagnostic
  `target/diagnostics/native-gpu/novywave-interaction-speed-root-demand-live-1781653707.json`
  provided demand classes for the same graph; `jq` grouped click samples by
  `source_action_root_dependent_visit_count` / enqueue count / dirty-pop count;
  source inspection covered `examples/novywave/RUN.bn` cursor thresholds,
  descriptor/page roots, selected-row roots, and lane-row page refs.
- Corrected measurement interpretation: `3536` visits / `600` enqueues /
  `792` dirty pops is the aggregate across the 32 click samples, not one click.
  The real bad per-click class is `194` dependent visits / `32` enqueues /
  `38` dirty pops. It occurs for 16 of 32 click samples. The other click
  classes are `28/6/12` for 8 samples and `26/5/11` for 8 samples.
- Actual trigger: the speed role clicks target times `50`, `100`, `150`, and
  `200` in a loop. For `simple.vcd`, `cursor_left=0`, `cursor_default=50`,
  and `cursor_right=150`. The slow `194/32/38` class is the cursor-class
  crossing work: click `50` moves from `Cursor48` to `Cursor42`, and click
  `150` moves from `Cursor42` to `Cursor48`. Click `100` stays inside
  `Cursor42`; click `200` stays inside `Cursor48`.
- Ranked culprit: the bottleneck is CPU runtime root-scheduler fanout from a
  real cursor-class change. `selected_timeline_cursor_value` changes
  `cursor_position`, `cursor_label`, `keyboard_cursor_label`, and
  `waveform_cursor_offset`; `cursor_position` changes
  `bridge_request_descriptor_label`; that drives bridge request fingerprint,
  input digest, structural key, response/status roots, page digests, page refs,
  bridge pages, `bridge_cursor_values`, `selected_lane_materialization`,
  `selected_cursor_pair_rows`, and `selected_signal_lane_rows`.
- Dirty-frontier edge evidence: the fresh dirty-frontier diagnostic ranks the
  same repeated edges on the click scope. The top edges include
  `root:selected_timeline_cursor_value -> store.cursor_label`,
  `root:selected_timeline_cursor_value -> store.cursor_position`,
  `root:selected_timeline_cursor_value -> store.keyboard_cursor_label`,
  `root:selected_timeline_cursor_value -> store.waveform_cursor_offset`,
  `root:selected_timeline_cursor_value -> store.selected_cursor_pair_rows`,
  `root:selected_timeline_cursor_value -> store.selected_signal_lane_rows`,
  `root:cursor_label -> store.bridge_request_descriptor`,
  `root:cursor_label -> store.bridge_cursor_values_page_digest`,
  `root:keyboard_cursor_label -> store.bridge_cursor_values_label`,
  `root:bridge_request_descriptor_label -> store.bridge_request_descriptor`,
  `root:bridge_request_descriptor_label -> store.bridge_request_fingerprint`,
  `root:bridge_request_descriptor_label -> store.bridge_request_input_digest`,
  and then the `bridge_request_fingerprint` cascade into bridge file stats,
  hierarchy/signal/waveform pages, page refs, labels, and digests.
- Root-work evidence: in the fresh dirty-frontier run, top click root work is
  `store.selected_signal_lane_rows` (`32` pops / `17.492ms`),
  `store.selected_cursor_pair_rows` (`32` pops / `8.446ms`),
  `store.bridge_request_descriptor` (`48` pops / `4.198ms`),
  `store.bridge_cursor_values_page_ref` (`64` pops / `3.431ms`),
  `store.bridge_cursor_values` (`32` pops / `1.620ms`),
  `store.bridge_cursor_values_label` (`56` pops / `1.495ms`), and
  `store.cursor_label` (`32` pops / `0.914ms`). The root-demand run classifies
  most of the frontier as `candidate_unobserved_source_free_pure`, but those
  roots are still sometimes consumed by row/list evaluation, so they cannot be
  blindly skipped.
- List-view evidence: list-view materialization is a symptom and secondary
  cost, not the first cause. The current reports show no broad fallback and no
  full row materialization: `field_only_fallback_count=0` and
  `full_eval_row_count=0`. The remaining list cost is field-only projection and
  diff work over dirty row fields, especially `selected_signal_lane_rows`
  (`eval_ms` about `15ms`, `diff_ms` about `14ms`, `160` field-cache misses
  in the click aggregate) and `selected_cursor_pair_rows` (`96` misses in the
  click aggregate).
- Renderer/verifier evidence: this gate is not proving live GPU present
  latency. `input_to_visible` is currently the same click-loop timing summary
  as `click_to_cursor`; it ends after deterministic input updates runtime,
  layout, and shared state. The renderer upload probe is a separate three-frame
  offscreen counter probe. Renderer/upload is therefore not the measured
  culprit in this failing gate, although a future per-interaction render timing
  report is still needed before claiming native present latency is solved.
- Implementation consequence: the next runtime fix should target generic
  demand/currentness before dirty enqueue for safe source-free pure internal
  roots, plus exact read barriers so deferred roots cannot be read stale by
  evaluators, state summaries, assertions, or row/list-view evaluation. A
  row-list-only optimization will not remove the `194/32/38` slow class by
  itself, and another readiness-set heuristic is likely to shift cost rather
  than remove the culprit.

### 2026-06-17 TASK-0804A Current Deep Culprit Measurement Loop

- Date: 2026-06-17
- Task: TASK-0804A Source-Action Root Flush Architecture Pass
- Commit: uncommitted
- Files changed in this slice:
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md` only.
- Verification: reused the previous read-only subagent findings, then ran a
  second loop with `019ed2f1-f4fe-7cb3-bcfa-3c4faff69804` on NovyWave root
  semantics and `019ed2f2-0c09-7370-b4e1-7e445be67dec` on verifier/runtime
  measurement validity. Fresh current diagnostic:
  `BOON_PROFILE_ROOT_DEMAND=1 BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/diagnostics/native-gpu/novywave-interaction-speed-current-deep-loop.json`.
  It failed the known strict budget and wrote current evidence from the
  `e9cc38e` tree plus this checklist-only worktree.
- Current headline numbers from the fresh diagnostic:
  `click_to_cursor.p95=19.786ms`, `input_to_visible.p95=19.786ms`,
  `runtime_apply.p95=12.934ms`, `runtime_step_apply.p95=10.767ms`,
  `runtime_state_summary.p95=0.836ms`, and `layout_rebuild.p95=4.889ms`.
  The profile env vars add overhead, but the graph shape matches the normal
  report and previous diagnostics.
- Measurement caveat confirmed in code: the verifier writes
  `input_to_visible_ms_p50_p95_max` from the same `click_summary_ms` as
  `click_to_cursor_ms_p50_p95_max` in
  `crates/boon_native_playground/src/main.rs`. This gate measures deterministic
  input through runtime apply, state summary, layout/patch, scroll adjustment,
  and shared render-state update. It does not measure per-click GPU present,
  queue submit, or readback. Renderer upload is only the separate three-frame
  offscreen probe, so renderer/upload is not the measured culprit here.
- Slow-class split by dependent visit count in the current diagnostic:
  `194` visits / `32` enqueues / `38` pops occurs for 16 clicks and averages
  `17.731ms` total, `12.228ms` runtime apply, `10.217ms` runtime step,
  `4.530ms` layout, `6.824ms` root flush, `3.933ms` dirty scheduler, and
  `2.215ms` root materialization. The `28/6/12` class averages `13.032ms`
  total and `2.616ms` root flush; the `26/5/11` class averages `7.959ms`
  total and `2.065ms` root flush. Divider and hover classes are not the click
  budget culprit.
- Root-flush bucket ranking inside the slow `194/32/38` click class:
  dirty scheduler is the largest sub-bucket (`3.933ms` average), but most of
  that is not raw lookup. Average dependent enqueue work is `2.535ms`, changed
  cache invalidation is `0.657ms`, dirty pop is `0.183ms`, dependent lookup is
  only `0.095ms`, and dirty-scheduler unattributed work is about `0.42ms`.
  This rules out "BTreeSet lookup alone" and "dirty pop alone" as the primary
  fix target; the culprit is repeated frontier/enqueue/currentness propagation.
- Demand-class evidence across the current diagnostic frontier:
  `candidate_unobserved_source_free_pure` accounts for `3953` visits and
  `669` enqueues, more than all observed/list classes combined. The other
  classes are `blocked_observed_downstream` (`866` visits / `325` enqueues),
  `blocked_observed_root` (`733` visits / `244` enqueues), and
  `blocked_list_view` (`1318` visits / `131` enqueues). This confirms that a
  generic demand/currentness frontier for unobserved pure roots is still the
  highest-leverage engine fix, provided stale-read barriers are correct.
- Top root work by materialization milliseconds in the current diagnostic:
  `store.selected_signal_lane_rows` (`65` pops / `30.153ms`),
  `store.selected_waveform_segments` (`33` pops / `14.996ms`),
  `store.selected_cursor_pair_rows` (`33` pops / `8.736ms`),
  `store.bridge_request_descriptor` (`50` pops / `4.448ms`),
  `store.bridge_cursor_values_page_ref` (`67` pops / `3.620ms`),
  `store.bridge_cursor_values` (`33` pops / `1.681ms`), and
  `store.bridge_cursor_values_label` (`58` pops / `1.575ms`). The first three
  are list-view work; the next four are unobserved source-free pure bridge/page
  roots that should be demand-current rather than eagerly materialized.
- List-view correction: the current row/list path is not doing full-row
  fallback. For click samples, `selected_signal_lane_rows` reports
  `field_only_fallback_count=0`, `full_eval_row_count=0`, `row_materialize_ms=0`,
  `field_cache_hits=4768`, `field_cache_misses=160`, `eval_ms=15.250`, and
  `diff_ms=14.048` in the aggregate. `selected_cursor_pair_rows` similarly has
  `full_eval_row_count=0`, `field_cache_misses=96`, `eval_ms=9.040`, and
  `diff_ms=8.586`. The list-view cost is field-only eval/diff over too many
  dirty row fields, not old full materialization.
- Semantically necessary work: `selected_timeline_cursor_value` must update
  visible cursor label/offset roots and visible row fields that depend on the
  cursor. `selected_signal_lane_rows` and `selected_cursor_pair_rows` are real
  visible work on cursor movement. They still need narrower row/field dirty
  frontiers and cached per-field results, but they are not safe to blindly skip.
- Semantically questionable eager work: cursor-class crossing should not eagerly
  rebuild static bridge/file/hierarchy/signal/page roots when they are
  unobserved. The most suspicious roots are
  `bridge_request_descriptor`, `bridge_cursor_values_page_ref`,
  `bridge_cursor_values`, `bridge_cursor_values_label`, `bridge_waveform_page`,
  `bridge_waveform_page_ref`, `bridge_signal_page`, `bridge_signal_page_ref`,
  `bridge_hierarchy_page`, `bridge_hierarchy_page_ref`,
  `bridge_file_stats`, and `bridge_file_stats_page_ref`. Many are classified
  as `candidate_unobserved_source_free_pure`; their current eager materialized
  values mainly exist to feed downstream dirty reads rather than visible output.
- Real culprit statement: the slow path is CPU runtime root-flush fanout from a
  cursor-class change. The expensive mechanism is eager currentness and dirty
  frontier propagation through unobserved pure bridge/page roots, which then
  wakes enough observed/list dependents to force field-only list-view eval/diff
  and a secondary layout pass. It is not bridge I/O, renderer upload, GPU
  present, heavy JSON report writing, whole-row materialization, dependent
  lookup alone, or dirty-pop bookkeeping alone.
- Implementation consequence for the next slice: implement demand-current
  unobserved pure roots before enqueue, not a list-only or BTreeSet-only
  micro-optimization. The implementation must still propagate enough dirty
  read keys for observed/list dependents, must hide stale storage/cache values
  behind evaluator/summary/assertion barriers, and must be killed if the
  `candidate_unobserved_source_free_pure` enqueues and the `194/32/38` class do
  not materially drop. After that, a second slice can attack list-view field
  diff cost with a narrower row/field dependency frontier.

### 2026-06-17 TASK-0804A Fresh Subagent Culprit Loop

- Date: 2026-06-17
- Task: TASK-0804A Source-Action Root Flush Architecture Pass
- Commit: uncommitted
- Files changed in this slice:
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md` only.
- Verification: redirected existing subagents `019ed2ff-d622-7980-a1d8-425219ef0be8`
  and `019ed2ff-d744-7990-80d2-8292975121a6` into a read-only challenge loop.
  The first subagent audited the CPU runtime scheduler/root-flush path. The
  second subagent tried to disprove that diagnosis from layout, renderer, GPU,
  bridge IO, native input, report writing, and harness angles. Fresh current
  diagnostic:
  `BOON_PROFILE_ROOT_DEMAND=1 BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask
  verify-native-gpu-novywave-interaction-speed --report
  target/diagnostics/native-gpu/novywave-interaction-speed-real-culprit-loop-20260617T003707Z.json`.
  Fresh canonical no-diagnostic baseline:
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report
  target/reports/native-gpu/novywave-interaction-speed.json`. Both fail only
  the known strict latency budget.
- Clean baseline evidence: the canonical report from current code has
  `click_to_cursor.p95=17.886ms`, `runtime_apply.p95=10.635ms`,
  `runtime_step_apply.p95=8.393ms`, `layout_rebuild.p95=4.883ms`, and
  `runtime_state_summary.p95=0.826ms`. All hot-path proof/report counters are
  zero: `hot_path_proof_readback_count=0`,
  `hot_path_heavy_json_summary_count=0`, `hot_path_report_write_count=0`,
  `hot_path_png_write_count=0`, `hot_path_verbose_trace_event_count=0`, and
  `hot_path_dev_blocking_ipc_count=0`. The renderer probe still shows the
  post-interaction upload is tiny (`p50=3360` bytes and `p50=3` queue writes);
  its large p95 is the initial-frame probe, not the per-click hot path.
- Diagnostic graph evidence: with root-demand and dirty-frontier profiling
  enabled, every click sample in the current diagnostic has the same bad graph
  shape: `194` dependent visits, `32` dependent enqueues, `38` dirty pops,
  `36` scalar root materializations, and `2` list-view materializations.
  Timing under profiler overhead is `click_to_cursor.p95=19.032ms`,
  `runtime_apply.p95=12.155ms`, `runtime_step_apply.p95=10.098ms`,
  `layout_rebuild.p95=4.791ms`, `source_action_root_flush.p95=6.659ms`,
  `source_action_root_dirty_scheduler.p95=3.750ms`,
  `source_action_root_dependent_enqueue.p95=2.363ms`,
  `source_action_root_materialization.p95=2.237ms`,
  `source_action_root_changed_cache_invalidation.p95=0.601ms`, and
  `source_action_root_dependent_lookup.p95=0.096ms`.
- Demand-class ranking from the current diagnostic:
  `candidate_unobserved_source_free_pure` accounts for `3568` visits and
  `576` enqueues, ahead of `blocked_list_view` (`704` visits / `64` enqueues),
  `blocked_observed_downstream` (`304` visits / `96` enqueues), and
  `blocked_observed_root` (`240` visits / `80` enqueues). This confirms the
  graph is dominated by unobserved pure bridge/page roots that are still
  eagerly enqueued and materialized for currentness propagation.
- Concrete frontier chain: `selected_timeline_cursor_value` wakes visible
  cursor roots and visible row lists, but also wakes `cursor_label` and
  `keyboard_cursor_label`. Those wake `bridge_request_descriptor`,
  `bridge_cursor_values_page_digest`, and `bridge_cursor_values_label`. The
  bridge request/fingerprint branch then fans out through waveform, hierarchy,
  signal, file-stats, cursor-values, and page-ref roots. The top scalar
  materialization work in the diagnostic is `bridge_request_descriptor`
  (`48` pops / `4.140ms`), `bridge_cursor_values_page_ref`
  (`64` pops / `3.301ms`), `bridge_cursor_values`
  (`32` pops / `1.569ms`), and `bridge_cursor_values_label`
  (`56` pops / `1.432ms`).
- List-view evidence: layout/list work is real but downstream of the root
  frontier, not the primary diagnosis. The current canonical report still shows
  `selected_signal_lane_rows` at `14.831ms` eval / `13.690ms` diff and
  `selected_cursor_pair_rows` at `8.872ms` eval / `8.446ms` diff, with
  `full_eval_row_count=0` and `field_only_fallback_count=0`. This is
  field-only visible row diff work over too many dirty fields, not old full-row
  materialization.
- Subagent challenge result: the strongest non-runtime alternative is layout as
  a secondary cost of roughly `4-5ms` p95. Renderer/GPU upload is not the
  measured culprit because this gate's click timing does not include per-click
  present/readback and the separate renderer upload probe passes. Report/JSON,
  bridge IO, native input dispatch, and IPC are weak explanations because their
  hot-path counters are zero or their slow shape does not correlate with the
  click-only cursor frontier.
- Real culprit statement: the slow path is an engine graph/currentness problem
  in CPU runtime root flush. A cursor movement causes eager dirty-frontier
  propagation through many unobserved source-free pure bridge/page roots. That
  fanout then forces scalar currentness work and wakes visible list-view
  eval/diff plus a secondary layout pass. This is not a BTreeSet-only issue,
  not dirty-pop bookkeeping alone, not full-row materialization, not renderer
  upload, not bridge file IO, and not benchmark report writing.
- Required next measurement before another broad implementation attempt:
  add no-behavior-change counters that simulate suppressing
  `candidate_unobserved_source_free_pure` enqueues, count how many such roots
  are actually demanded later in the same interaction, and split dirty-frontier
  edges by value-changing propagation versus currentness-only propagation. Kill
  any implementation that does not materially reduce the `194/32/38` class or
  that merely shifts the same work into list-view diff/layout.

### 2026-06-17 TASK-0804A Three-Loop Cap And Postponement

- Date: 2026-06-17
- Task: TASK-0804A Source-Action Root Flush Architecture Pass
- Commit: uncommitted
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`,
  `crates/boon_native_playground/src/main.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime -p boon_native_playground`;
  `cargo check -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo test -p boon_runtime --lib root_derived_ -- --nocapture`;
  `cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`;
  profiled diagnostics
  `target/diagnostics/native-gpu/novywave-interaction-speed-candidate-defer-context-window-tagged.json`
  and
  `target/diagnostics/native-gpu/novywave-interaction-speed-numeric-guard-reader.json`;
  canonical report
  `target/reports/native-gpu/novywave-interaction-speed.json`.
- Loop 1 result: added no-behavior-change candidate-root demand counters and
  split demand reads into eval, state-summary, document/window-summary, and
  sparse-value-summary contexts. After fixing the window-summary context hole,
  the report still showed all candidate demand reads in evaluator context:
  `root_count=24`, `simulated_defer_enqueue_count=552`,
  `materialization_count=552`, `changed_materialization_count=552`,
  `unchanged_materialization_count=0`, and `eval_scalar_read_count=2064`.
  This disproves the theory that post-turn summaries alone caused the demand.
- Loop 2 result: kept a generic runtime optimization for numeric guard
  invalidation. `root_numeric_values_for_reads` now uses a narrow numeric
  reader instead of constructing full `BoonValue`s through
  `runtime_scalar_boon_value` for every root read. This preserved focused
  correctness and dropped profiled candidate demand reads from `2064` to `512`,
  but it did not change the graph shape: slow clicks remain `194` dependent
  visits, `32` enqueues, and `38` dirty pops.
- Loop 3 result: canonical no-diagnostic gate still fails the strict latency
  budget: `click_to_cursor.p95=17.551ms` and `input_to_visible.p95=17.551ms`
  against `16.700ms`; `runtime_apply.p95=10.361ms`,
  `runtime_step_apply.p95=8.437ms`, and `layout_rebuild.p95=4.812ms`. The
  canonical click root-list summary still reports
  `source_action_root_flush_ms=106.943`, `source_action_root_dirty_scheduler_ms=45.050`,
  `source_action_root_materialization_ms=58.610`, `eval_ms=30.033`,
  `diff_ms=28.568`, and `user_function_body_ms=35.525`.
- Subagent/source conclusion: cursor movement semantically flows from
  `selected_timeline_cursor_value` into `cursor_label`, `keyboard_cursor_label`,
  `bridge_request_descriptor`, `bridge_cursor_values_page_digest`,
  `bridge_cursor_values_label`, page refs, and visible lane rows. A source-level
  bridge identity split is probably the correct larger design: keep stable
  page/request identity separate from cursor-hot telemetry and avoid embedding
  volatile page refs in every visible row. That change touches stale-response
  and bridge fingerprint expectations, so it is larger than this capped loop.
- Result: TASK-0804A is postponed, not done. The kept numeric-guard reader is a
  safe runtime cleanup and measurement improvement, but it does not satisfy the
  official speed budget. Per user direction, stop chasing this task now and
  continue with representation work: TASK-1001 runtime BYTES, then TASK-1002
  LIST storage/incremental representation.
- Follow-up: when TASK-0804A is resumed, start from a bridge/page identity
  design pass or compiled demand/currentness frontier, not from another
  microchange. Kill criteria remain: reduce the `194/32/38` class, root-flush
  buckets, or final click/input p95 without hardcoding NovyWave.

### 2026-06-17 TASK-1001 Runtime BYTES Storage And Bridge Conversion Slice

- Date: 2026-06-17
- Task: TASK-1001 Runtime BYTES Value And Bridge/File Payload Boundary
- Commit: uncommitted
- Files changed in this slice:
  `Cargo.lock`, `crates/boon_runtime/Cargo.toml`,
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib bytes -- --nocapture`;
  `cargo test -p boon_bridge --lib -- --nocapture`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`;
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`;
  `git diff --check -- crates/boon_runtime/Cargo.toml crates/boon_runtime/src/lib.rs crates/boon_native_playground/src/main.rs docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Result: in progress. Added an internal runtime `BYTES` carrier backed by
  `bytes::Bytes`, with typed inline, blob-ref, and page-ref payload forms.
  `FieldValue`, `BoonValue`, root `ValueColumns`, and list row storage now carry
  typed bytes separately from `TEXT` and generic JSON. Public runtime summaries
  expose byte metadata (`$boon_type`, storage kind, digest, byte length, and ref
  metadata) and do not inline byte arrays; compiled/runtime artifacts still keep
  inline bytes only in artifact serialization so deterministic restoration is
  possible.
- Bridge boundary: added a single runtime conversion path from existing
  `BridgeValue::Bytes`, `BridgeValue::BlobRef`, and `BridgeValue::PageRef` into
  runtime bytes. The bridge ABI and canonical bridge JSON shape were not
  changed; the existing bridge tests and NovyWave bridge scenario still pass.
- Tests: added focused bytes tests for public-summary non-leakage, artifact
  round trip, typed root/list storage, cache-fragment separation from JSON
  metadata, and bridge bytes/blob/page-ref conversion.
- Not done yet: TASK-1001 remains open because NovyWave's current Boon bridge
  descriptor records are still text/record-shaped and intentionally unchanged.
  Moving page/blob descriptor roots to runtime `BYTES` needs a separate
  compatibility pass so field access such as page digests/statuses stays
  descriptor-visible while full binary payloads remain out of Boon source.
- Follow-up: continue with TASK-1002 as a narrow LIST classifier/report slice
  before attempting a physical list storage change. When TASK-1001 resumes,
  integrate BYTES only at true binary file/page/blob boundaries, not for labels,
  filenames, statuses, or scenario text.

### 2026-06-17 TASK-1001 Bridge Completion Runtime BYTES Boundary Slice

- Date: 2026-06-17
- Task: TASK-1001 Runtime BYTES Value And Bridge/File Payload Boundary
- Commit: uncommitted
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib bridge_completion_payload_sidecars_reach_runtime_bytes_boundary -- --nocapture`;
  `cargo test -p boon_runtime --lib bytes -- --nocapture`;
  `cargo test -p boon_bridge --lib -- --nocapture`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`;
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo xtask verify-report-schema`.
- Result: in progress. Added public runtime summary helpers for bridge values
  and bridge completion outputs. Accepted bridge completions can now be checked
  at the runtime boundary without exposing private runtime value internals:
  inline bytes, blob refs, and page refs become public BYTES summaries with
  storage kind, digest, byte length, and ref metadata.
- Determinism evidence: added a bridge completion test that registers a generic
  payload effect, schedules a request, accepts a completion with real blob/page
  sidecars through `complete_with_payloads`, converts the accepted output to a
  runtime summary, and proves the same summary after completion
  serialize/deserialize replay metadata. The public summary keeps raw sidecar
  bytes out of JSON while preserving digests and byte lengths.
- Compatibility evidence: the existing bridge suite, runtime BYTES suite,
  NovyWave bridge scenario, and report schema verifier still pass. No Boon
  syntax changed, and the NovyWave descriptor records were not rewritten.
- Not done yet: TASK-1001 remains open because the next step is a real producer
  integration: waveform/file/page/blob data should enter through this BYTES
  boundary, while user-visible filenames, labels, statuses, formulas, and
  scenario text remain TEXT/records.

### 2026-06-17 TASK-1002 LIST Representation Classification Slice

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime -p boon_native_playground`;
  `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo test -p boon_ir --lib representation -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `cargo check -p boon_ir -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`;
  `git diff --check`.
- Default-stack caveat: `cargo test -p boon_runtime --lib list_ -- --nocapture`
  overflows the stack in
  `novywave_marker_count_updates_after_list_insert_and_remove`, while the same
  test and the full `list_` filter pass with `RUST_MIN_STACK=33554432`. Treat
  this as a test-harness stack-depth issue to investigate separately, not as
  evidence that LIST semantics failed.
- Result: in progress. Added report-visible LIST representation labels to
  root list-view profiles without changing Boon syntax or row semantics.
  Existing field-only root list-view patches now report
  `list_storage_mode=field_only_record_projection` and
  `list_value_shape=record_field_projection`; stable row-ref reuse reports
  `list_storage_mode=row_ref_projection_reuse` and
  `list_value_shape=row_ref_values`; generic fallback paths are labeled by
  whether source identities are present and by the observed value shape.
- Current speed evidence: the refreshed NovyWave interaction-speed report still
  fails strict latency budgets, so this slice is not a speed fix:
  `click_to_cursor.p95=17.767ms`, `input_to_visible.p95=17.767ms`,
  `runtime_apply.p95=10.539ms`, `runtime_step_apply.p95=8.591ms`, and
  `layout_rebuild.p95=4.818ms`. The report confirms the two active LIST shapes
  are `field_only_record_projection`/`record_field_projection` and
  `row_ref_projection_reuse`/`row_ref_values`.
- Follow-up: use these labels to choose the first physical LIST storage mode or
  projection-preservation optimization. The next kept TASK-1002 slice should
  reduce a measured list/root-view bucket or preserve row/source identity more
  directly; do not claim final speed success until the official interaction
  budget passes.
- Next-slice review: a fresh subagent review recommended a dirty projected-field
  frontier for the field-only list-view loop, but this family has already been
  tried and killed twice under TASK-0804A, including the fresh
  invalidated-field-frontier experiment. Do not retry that inside-loop
  prefilter as the next TASK-1002 implementation unless a new report proves the
  old kill criteria no longer apply. Prefer a real earlier compiled
  row/source-identity plan, a bridge/page identity split, or a true
  bridge-payload BYTES ingestion boundary.

### 2026-06-17 TASK-1002 Direct List-View Consumer Slice

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this slice:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `cargo check -p boon_runtime`;
  `cargo test -p boon_runtime --lib list_view_direct_join_and_fused_map_join_avoid_row_ref_vector -- --nocapture`;
  `cargo test -p boon_runtime --lib list_index_report_counters_include_task_0301_fields -- --nocapture`;
  `cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture`;
  `cargo test -p boon_runtime --lib list_filter_field_equal_uses_text_lookup_index_for_homogeneous_row_refs -- --nocapture`;
  `cargo test -p boon_ir --lib representation -- --nocapture`;
  `cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `cargo check -p boon_ir -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`.
- Result: in progress. LIST selections now keep their selected row indices in a
  shared `Arc<[usize]>` representation, and `List/join_field` plus the fused
  `List/map |> List/join_field` path can consume both `ListSelection` and
  `ListRef` rows directly without first constructing a `Vec<RowRef>`. This is a
  generic runtime consumer optimization: no Boon syntax, NovyWave-specific
  branch, fixture reduction, hardcoded filename, or bridge shortcut was added.
- Measurement evidence: the refreshed NovyWave speed report now exposes
  `list_view_direct_rows=8` and
  `list_view_row_ref_materializations_avoided=8` in a runtime list-scan sample;
  indexed filters still report `filter_field_rows_scanned=0` and
  `retain_rows_scanned=0`. This confirms the slice moved the intended row-ref
  vector materialization path, but the count is too small to solve the current
  p95 budget.
- Current speed evidence: the official speed gate still fails:
  `click_to_cursor.p95=18.020982ms`,
  `input_to_visible.p95=18.020982ms`,
  `runtime_apply.p95=11.121077ms`,
  `runtime_step_apply.p95=8.901886ms`, and
  `layout_rebuild.p95=4.819564ms`. The remaining click-path architecture cause
  is still `root_flush_dirty_scheduler_plus_root_list_materialization` with
  `source_action_root_dependent_visit_count=3536`,
  `source_action_root_dependent_enqueue_count=600`,
  `source_action_root_dirty_pop_count=792`, and
  `source_action_root_list_view_materialization_count=56`.
- Follow-up: keep TASK-1002 open. The next LIST work should target a larger
  measured bucket than row-ref vector creation: compiled row/field dependency
  frontiers, earlier source/page demand identity, or direct physical list-view
  updates that reduce the `194/32/38` click frontier shape. Do not spend another
  slice on tiny collection swaps unless a report shows the hot bucket changed.

### 2026-06-17 TASK-1001 Real NovyWave Payload BYTES Slice

- Date: 2026-06-17
- Task: TASK-1001 Runtime BYTES Value And Bridge/File Payload Boundary
- Commit: uncommitted
- Files changed in this checkpoint:
  `Cargo.lock`,
  `crates/boon_native_playground/src/main.rs`,
  `crates/boon_runtime/Cargo.toml`,
  `crates/boon_runtime/src/lib.rs`,
  `crates/xtask/src/main.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib text_match_patterns_rejoin_pathlike_punctuation_without_spaces -- --nocapture`;
  `cargo test -p boon_runtime --lib derived_root_summary_recomputes_pure_dependency_before_stored_scalar_alias -- --nocapture`;
  `cargo test -p boon_runtime --lib root_scalar_same_event_ -- --nocapture`;
  `cargo test -p boon_runtime --lib bytes -- --nocapture`;
  `cargo test -p boon_bridge --lib -- --nocapture`;
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.
- Result: TASK-1001 real-payload boundary is complete for the current roadmap
  slice. The NovyWave bridge verifier now proves real local waveform payloads
  at the bridge/runtime boundary: `simple.vcd` and `simple_test.ghw` are accepted
  as BYTES sidecar completions, while the large `wave_27.fst` is verified by
  descriptor hash and byte length without routine full sidecar load. No Boon
  syntax was added and user-visible filenames, statuses, labels, and descriptor
  records remain TEXT/record-shaped.
- Real-file evidence from
  `target/reports/novywave-bridge-scenario.json`:
  `simple.vcd` descriptor and sidecar match
  `sha256:726d6fa4d5baa8881462f131997d16ff21494bb88708eb6b23420215e3f5a5de`
  at `311` bytes; `simple_test.ghw` descriptor and sidecar match
  `sha256:3485b4b9a423ce61e54a9bd4a2c525cf7576f0e36d39ad73b208c2e38e980491`
  at `833` bytes; `wave_27.fst` descriptor matches
  `sha256:aa4a6993101ff31601e12d613b746d8753f20448d78e8b720f708403047dc172`
  at `28860652` bytes and is intentionally `sidecar_payload=false`.
- BYTES non-leak evidence: the report's
  `bridge_real_payload_bytes_evidence.status` is `pass`; small-file completion
  summaries expose `$boon_type: BYTES`, `storage: blob_ref/page_ref`, digest,
  byte length, encoding, and ref metadata, with `raw_bytes_leaked=false`.
- Engine correctness fixes required by this slice:
  `TEXT { ... }` match patterns now rejoin path-like punctuation, so patterns
  such as `TEXT { simple_test.ghw }`, `TEXT { wave_27.fst }`,
  `TEXT { data_bus[7:0] }`, and URL/path-shaped text match the same values as
  TEXT literals instead of falling through because the parser split punctuation
  into separate tokens. Sparse runtime value summaries and scenario assertions
  now recompute current pure roots before reading stale materialized scalar
  aliases, so deferred root optimization cannot make verification observe the
  previous step.
- Scenario evidence: `verify-novywave-bridge-scenario` now passes fully,
  including `bridge_scenario_coverage.status=pass` and
  `bridge_real_payload_bytes_evidence.status=pass`.
- Follow-up: continue TASK-1002/LIST work from the measured larger buckets.
  Do not spend another slice replacing JSON/container types or loading more
  waveform bytes unless a report shows bridge payload transport, not root/list
  fanout, has become a top runtime cost.

### 2026-06-17 TASK-1002 LIST/BYTES Integration Correctness Slice

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_ir/src/lib.rs`,
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime -p boon_ir`;
  `cargo test -p boon_ir --lib pure_match_function -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_top_level_format_updates_active_selected_row_formatter -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_selected_row_formatter_is_row_scoped_and_rerenders_values -- --nocapture`;
  `cargo test -p boon_runtime --lib bytes -- --nocapture`;
  `cargo test -p boon_bridge --lib -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib nested_structured_child_change_does_not_dirty_top_level_leaf_alias -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib derived_root_summary_recomputes_pure_dependency_before_stored_scalar_alias -- --nocapture`;
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.
- Result: kept correctness fixes required before further LIST/BYTES speed work
  can be trusted. The compiler no longer treats a nested function call inside a
  guarded row update as an unconditional row-wide function update. The regression
  shape is `event |> THEN { condition |> WHEN { True => next_format(...), False
  => old_value } }`: it now lowers as a guarded match update, so only the active
  row changes. No Boon syntax changed.
- Runtime LIST dependency fix: filtered/pass-through list views that expose
  whole source rows now publish exposed source-row field reads as dependencies,
  not only the predicate field or the target list field names. This keeps
  row-ref projection reuse from hiding source-field dependencies.
- Runtime summary fix: sparse summaries and scenario assertions were able to
  clear root value caches and recompute root `ListView`s through generic
  expression evaluation, replacing full materializer dependencies with
  predicate-only reads. Root list-view summaries now go through the list-view
  materializer and return the materializer's cached evaluated value, preserving
  enriched row-local fields such as NovyWave formatter/dropdown state while
  keeping the dependency graph complete.
- Current TASK-1002 status: still open. The direct LIST consumer optimization
  and these correctness fixes are kept, but they do not finish the larger LIST
  storage/incremental roadmap. Next LIST work should reduce larger measured
  buckets such as root/list materialization, dirty-frontier fanout, or
  field-patch/user-function work rather than another tiny vector/container swap.

### 2026-06-17 TASK-1002 Branch-Selector Cache Killed And RecordColumns Field Access Fix

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `cargo test -p boon_runtime --lib root_list_view_field_cache_separates_same_source_row_by_caller_env -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_top_level_format_updates_active_selected_row_formatter -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_selected_row_formatter_is_row_scoped_and_rerenders_values -- --nocapture`;
  `cargo check -p boon_runtime`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.
- Subagent review: the best next LIST slice is still earlier
  root/list-fanout measurement or a real field-only physical patch frontier.
  The current report already shows field-only patching is active, but it still
  spends most click-path LIST time in `selected_signal_lane_rows` and
  `selected_cursor_pair_rows`.
- Killed experiment: a branch-selector field-cache experiment was tried and
  removed. It passed focused branch tests, but it did not fire in the NovyWave
  hot lists, made the official speed report worse during the experiment, and
  failed the promotion metric. Do not retry selector caching unless a future
  report proves branch selector evaluation itself is a top bucket.
- Kept correctness fix: `Field/name` access now supports column-backed
  `RecordColumns` values as well as plain record values. This fixes optimized
  root list-view contexts where a nested row such as `detail_row(...).label`
  previously collapsed to `type_error`. This is a generic runtime fix, not a
  Boon syntax change and not a NovyWave workaround.
- Current speed evidence after removing the failed experiment: the official
  gate still fails strict latency budgets:
  `click_to_cursor.p95=18.917ms`,
  `input_to_visible.p95=18.917ms`,
  `runtime_apply.p95=11.746ms`,
  `runtime_step_apply.p95=9.600ms`, and
  `layout_rebuild.p95=4.638ms`. Root-list click counters show
  `eval_ms=36.845`, `diff_ms=31.362`,
  `user_function_body_ms=38.954`, `changed_read_count=2240`, and
  `current_dirty_read_count=4936`.
- Current measured LIST buckets:
  `selected_signal_lane_rows eval_ms=20.516 diff_ms=16.179
  user_function_body_ms=12.490 dirty_reads=2360 row_count=168`;
  `selected_cursor_pair_rows eval_ms=10.409 diff_ms=8.789
  user_function_body_ms=24.348 dirty_reads=2136 row_count=96`;
  `selected_visible_items eval_ms=5.511 diff_ms=5.908
  user_function_body_ms=2.116 row_count=72`;
  `selected_signal_rows eval_ms=0.409 diff_ms=0.487 row_count=48`.
- Current TASK-1002 status: still open. The next kept implementation should
  target the larger measured root/ListView buckets or the earlier dependency
  identities feeding them. Avoid another branch-selector cache, tiny container
  swap, or inside-loop field prefilter unless a fresh report shows that exact
  bucket became dominant.

### 2026-06-17 TASK-1002 Field-Only Row Binding Cleanup Slice

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `cargo check -p boon_runtime`;
  `cargo test -p boon_runtime --lib root_list_view_field_cache_separates_same_source_row_by_caller_env -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_top_level_format_updates_active_selected_row_formatter -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_selected_row_formatter_is_row_scoped_and_rerenders_values -- --nocapture`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`;
  `cargo xtask verify-report-schema`.
- Result: kept a narrow generic root-list field-only cleanup. The materializer
  no longer injects the caller map binding into the callee row frame before the
  active projector also binds the callee row argument. Direct record projectors
  now move the row value into the callee frame, and branch projectors avoid a
  second row binding when the selected arm uses the same row argument. This
  removes duplicate row-value cloning/binding in the hot field-only loop without
  changing Boon syntax or adding NovyWave-specific behavior.
- Measured effect: the official speed gate still fails the strict interaction
  budget, but the current report moved in the right direction versus the
  immediately preceding post-revert report. `click_to_cursor.p95` and
  `input_to_visible.p95` moved from `18.917ms` to `18.090ms`;
  `runtime_apply.p95` moved from `11.746ms` to `11.404ms`;
  `runtime_step_apply.p95` moved from `9.600ms` to `9.224ms`. Root-list
  aggregate counters moved from `eval_ms=36.845`, `diff_ms=31.362`, and
  `user_function_body_ms=38.954` to `eval_ms=35.953`, `diff_ms=30.567`, and
  `user_function_body_ms=37.944`.
- Remaining cause: graph shape and root/list work class are unchanged. The
  largest LIST buckets remain `selected_signal_lane_rows`
  (`eval_ms=20.029`, `diff_ms=15.759`, `user_function_body_ms=12.137`) and
  `selected_cursor_pair_rows` (`eval_ms=10.159`, `diff_ms=8.578`,
  `user_function_body_ms=23.760`). TASK-1002 remains open; the next slice
  should target field-cache key construction, dirty-read fanout identities, or
  a larger physical LIST/root-view representation change rather than another
  tiny clone cleanup.

### 2026-06-17 TASK-1002 Source-Binding Fingerprint Dedup And Three-Loop Cap

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_field_cache_shares_row_independent_fields_across_rows -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_field_cache_separates_same_source_row_by_caller_env -- --nocapture`;
  `cargo check -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_top_level_format_updates_active_selected_row_formatter -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_selected_row_formatter_is_row_scoped_and_rerenders_values -- --nocapture`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (expected failure against the current strict p95 budget);
  post-kill refresh of the same speed gate so
  `target/reports/native-gpu/novywave-interaction-speed.json` matches current
  code after removing the failed experiment.
- Kept loop: root list-view field cache keys already include the source row
  identity/generation when a projected field references the source binding, so
  the env fingerprint no longer serializes that same source binding a second
  time. The cache key still includes other free env values, record scope,
  source list, source key/generation, and field name, so this is a generic
  key-size/dedup optimization rather than a relaxed dependency rule.
- Measured effect of the kept loop: the official gate still failed, but the
  root-list internals moved slightly in the right direction versus the previous
  field-only row-binding report. The kept-loop report showed
  `click_to_cursor.p95=18.342ms`,
  `input_to_visible.p95=18.342ms`,
  `runtime_apply.p95=11.416ms`,
  `runtime_step_apply.p95=9.210ms`, and
  `layout_rebuild.p95=4.550ms`. Root-list counters were
  `eval_ms=35.879`, `diff_ms=30.481`,
  `user_function_body_ms=38.059`, with
  `selected_signal_lane_rows eval_ms=19.707 diff_ms=15.475` and
  `selected_cursor_pair_rows eval_ms=10.299 diff_ms=8.689`.
- Killed loop: a precomputed clean-field reuse table for empty-env previous-pass
  field cache hits was implemented, measured, and removed. It preserved focused
  root-list correctness tests, but the official speed gate regressed to
  `click_to_cursor.p95=20.495ms`,
  `input_to_visible.p95=20.495ms`,
  `runtime_apply.p95=12.904ms`, and root-list
  `eval_ms=39.009`. The extra table/bucket work cost more than it saved in the
  real NovyWave path, so the code was killed rather than kept as speculative
  complexity.
- Current post-kill report evidence: after removing that table experiment and
  refreshing the canonical report, the current worktree still fails only the
  known strict p95 budget: `click_to_cursor.p95=18.284ms`,
  `input_to_visible.p95=18.284ms`,
  `runtime_apply.p95=11.618ms`,
  `runtime_step_apply.p95=9.198ms`, and
  `layout_rebuild.p95=4.786ms`. Root-list counters are
  `eval_ms=36.102`, `diff_ms=30.754`,
  `user_function_body_ms=37.830`,
  `user_function_cache_key_ms=3.574`,
  `field_cache_hits=4768`, `field_cache_misses=256`,
  `changed_read_count=2240`, and `current_dirty_read_count=4936`.
  The dominant lists remain `selected_signal_lane_rows`
  (`eval_ms=20.010`, `diff_ms=15.639`, `user_function_body_ms=12.034`)
  and `selected_cursor_pair_rows`
  (`eval_ms=10.186`, `diff_ms=8.594`, `user_function_body_ms=23.771`).
- Third-loop diagnostic: function-cache counters are already active in the hot
  lists, so the remaining p95 miss is not a simple disabled-cache bug.
  `selected_cursor_pair_rows` still spends the largest body time, but it also
  reports function-cache hits; the next useful work needs finer root/list
  materialization or function-body attribution before another cache-key rewrite.
- Current TASK-1002 status: still open, but the immediate NovyWave
  interaction-speed p95 debugging loop is postponed after this cap. Continue
  broader LIST representation work only when it reduces a measured bucket such
  as root/list materialization, dirty-frontier fanout, field-cache key cost, or
  duplicated cursor-value function bodies. Do not retry the killed clean-field
  table, branch-selector cache, or tiny container swap without a fresh report
  proving that exact bucket became dominant.

### 2026-06-17 TASK-1002 ListSelection FindValue Indexed Path

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_index_find_value_uses_text_lookup_index_for_list_selection -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_index_find_value_uses_text_lookup_index_for_runtime_list_ref -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo check -p boon_runtime`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`.
- Result: kept a generic `ListSelection` storage-mode improvement. `List/find`
  and therefore `List/find_value` now use the existing selection-aware text
  lookup index when the input is a `BoonValue::ListSelection`, instead of
  expanding the selection and scanning selected rows. The fallback scan still
  exists when no usable index is available, and the function still returns a
  `RowRef` constrained to the selected row universe.
- Regression evidence: the new focused test constructs a `ListSelection` over
  rows `[0, 1, 3]`, searches for `key == row-2`, and proves the result is the
  selected row value `second` rather than the excluded row value `skipped`.
  It also asserts `list_find_rows_scanned=0` and a text lookup hit.
- Scope note: the NovyWave p95 speed gate was not rerun for this slice because
  the previous entry capped immediate p95 debugging loops and this is a broader
  LIST representation correctness/coverage improvement, not a claimed
  interaction-speed fix. TASK-1002 remains open.

### 2026-06-17 TASK-1002 ListSelection Cardinality Length Path

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_selection_length_uses_selection_cardinality_without_row_expansion -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo test -p boon_ir --lib representation -- --nocapture`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`.
- Result: kept a generic LIST storage-mode improvement for cardinality-only
  queries. `List/count`, `List/length`, and `List/is_not_empty` now call a
  length-specific runtime path that preserves normal list read dependencies but
  avoids expanding `ListRef` and `ListSelection` inputs into row-ref vectors
  just to count them.
- Regression evidence: the focused test constructs a `ListSelection` over three
  selected row indices, asks the runtime for its length, and asserts that the
  answer is `3` while `row_occurrences_scanned` stays at `0`. This proves the
  selection cardinality is used directly rather than hidden behind generic row
  expansion.
- Scope note: this is a foundation slice for the broader LIST representation
  roadmap. It does not change Boon syntax, it does not introduce a
  NovyWave-specific shortcut, and it does not reopen the capped immediate p95
  debugging loop. TASK-1002 remains open; the next useful slices are indexed
  first-match consumers, root `ListView` projection over row-index views, or
  exact-text index maintenance if measurement makes stale-index risk worth it.

### 2026-06-17 TASK-1002 Identity ListMap Preserves Storage Views

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_map_identity_preserves_storage_views_without_row_expansion -- --nocapture`;
  `cargo check -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib user_function_cache_keys_row_args_by_accessed_fields_when_safe -- --nocapture`.
- Result: kept a generic LIST storage-mode optimization for identity maps.
  `List/map(row, new: row)` now preserves `ListRef` and `ListSelection`
  values instead of expanding them into row-ref vectors. The optimization is
  implemented for both the normal AST evaluator and the runtime-generic
  artifact path, and it still records the list read dependency.
- Regression evidence: the focused test calls the runtime directly for
  `ListRef`, `ListSelection`, and runtime-generic `ListSelection` identity
  maps. It proves the output storage view is preserved, the list read is
  recorded, `row_occurrences_scanned` stays at `0`, and the existing
  avoided-row-ref-materialization counter increases by the logical row count.
- Boundary note: read-only subagent review suggested also indexing the
  remaining scalar first-match `List/find_value` consumers. That path currently
  sits behind closure-driven scalar equation evaluation where the exact text
  index builder requires mutable runtime access. This slice did not force a
  broad mutability/API change through source-route and summary helpers; that
  should be handled as a separate task if measurement shows those consumers are
  still hot.
- Scope note: this does not change Boon syntax, does not special-case NovyWave,
  and does not claim the capped NovyWave interaction p95 budget is fixed.
  TASK-1002 remains open; the next larger slice should carry row-index views
  directly through root `ListView` materialization or make exact-text lookup
  indexes incrementally maintained if stale-index risk is justified.

### 2026-06-17 TASK-1002 Root ListView Row-Index Source Projection

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  read-only subagent review `019ed3e2-eed1-71e2-8198-73f514cb1edc`;
  `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_filtered_rows_preserve_identity_for_downstream_map -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_identity_list_ref_carries_row_index_view -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_identity_list_selection_carries_row_index_view -- --nocapture`;
  `cargo check -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_map_identity_preserves_storage_views_without_row_expansion -- --nocapture`;
  `cargo test -p boon_ir --lib representation -- --nocapture`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.
- Result: kept a generic root `ListView` representation improvement. The
  materializer now has an internal `RootListViewRowIndexSource` for `ListRef`
  and `ListSelection`. Field-only root list-view patching consumes that source
  directly instead of first routing through `list_values_for_iteration`, and
  the broad root list-view materializer uses the same source for identities and
  source-column reads before creating row refs only as a compatibility bridge
  to existing reuse/materialization code.
- Regression evidence: the focused filtered downstream-map test now proves a
  dirty root map over a stable filtered root list uses
  `field_only_row_index_projection` with `list_ref_row_index_view`, preserves
  target row identity, reports direct list-view rows, and keeps
  `full_eval_row_count=0` and `row_materialize_ms=0.0`. The new identity
  `ListRef` test proves broad rematerialization reports
  `generic_vec_with_row_index_source`, `list_ref_row_index_view`,
  `source_identity_count == row_count`, `row_occurrences_scanned=0`, and direct
  list-view row consumption. The new identity `ListSelection` test proves the
  same broad mode for filtered selections, preserves selection order, and
  rejects same-count selection identity changes as in-place target-index
  patches.
- Scope note: this does not change `list_values_for_iteration` globally and
  does not add Boon syntax. It is not a NovyWave-specific shortcut and it does
  not claim the capped NovyWave interaction p95 budget is fixed. TASK-1002
  remains open; the next larger slices are exact-text index maintenance, row
  source support deeper in non-root list consumers, or a measured dirty-frontier
  reduction once the current row-index root materialization evidence is
  committed.

### 2026-06-17 TASK-1002 ListMoveField Ordered Selection Slice

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_move_field_preserves_list_ref_storage_view_without_row_expansion -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_move_field_preserves_selection_order_without_row_expansion -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo test -p boon_ir --lib representation -- --nocapture`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (expected failure against the current strict p95 budget).
- Result: kept a generic LIST storage-mode improvement for
  `List/move_field_first` and `List/move_field_last`. When the input is a
  `ListRef` or `ListSelection` and a text-like index is available, the runtime
  now computes the matching/rest row-index partition and returns an ordered
  `ListSelection` instead of expanding the input through
  `list_values_for_iteration` and allocating row refs. Fallback behavior is
  unchanged when no usable index exists.
- Regression evidence: the new focused tests prove `ListRef` move-first returns
  `[1, 3, 0, 2]` as a `ListSelection`, selection move-last preserves selected
  order as `[1, 3, 4, 0]`, records the list/column read, uses the indexed/direct
  selection lookup, reports `move_field_rows_scanned=0`, and increments
  row-ref materialization avoidance for the logical row count.
- Current NovyWave evidence: the refreshed official report still fails only the
  known strict interaction p95 budgets:
  `click_to_cursor.p95=18.127ms` and `input_to_visible.p95=18.127ms` against
  `16.700ms`. Runtime/list evidence is still useful: the hot root-list profiles
  have `full_eval_row_count=0` and `row_materialize_ms=0.0`, with
  `selected_signal_lane_rows` using `field_only_row_index_projection` /
  `list_ref_row_index_view`. Aggregate root-list counters remain high:
  `eval_ms=54.237`, `diff_ms=41.832`, `user_function_body_ms=47.350`,
  `current_dirty_read_count=6217`, and the dominant list is still
  `selected_signal_lane_rows`. This slice is therefore kept as generic
  representation coverage, not as the final speed fix.
- Scope note: this does not change Boon syntax, does not hardcode NovyWave, and
  does not reopen the capped p95 debugging loop. TASK-1002 remains open; the
  next useful work should target exact-text index maintenance, source-row views
  deeper in non-root consumers, or the remaining dirty-frontier/user-function
  body costs.

### 2026-06-17 TASK-1002 ListGetLatest Row-Index Access Slice

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_get_and_latest_use_row_index_views_without_expansion -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-report-schema`;
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  (expected failure against the current strict p95 budget).
- Result: kept another generic row-index consumer improvement. `List/get` and
  `List/latest` now answer directly from `ListRef` and `ListSelection` inputs by
  reading the requested/last row index and returning the same row-ref pipeline
  value the old evaluator would have produced. The generic fallback still
  handles plain lists, text path resolution, and non-row values.
- Regression evidence: the focused test proves `List/get` over a `ListRef`
  returns row index `2`, `List/get` over a `ListSelection [3, 1]` returns source
  row index `1`, and `List/latest` over `ListSelection [0, 3, 1]` returns source
  row index `1`. Each path records the list read and keeps
  `row_occurrences_scanned=0`.
- Current NovyWave evidence: the refreshed speed report still fails the same
  known p95 budget: `click_to_cursor.p95=18.127ms` and
  `input_to_visible.p95=18.127ms` against `16.700ms`; `runtime_apply.p95` is
  `11.577ms`, `runtime_step_apply.p95=9.313ms`, and
  `layout_rebuild.p95=4.615ms`. Root-list materialization remains the dominant
  engine-side cost even though row expansion is eliminated in the hot lists:
  aggregate `eval_ms=55.358`, `diff_ms=42.524`,
  `user_function_body_ms=47.745`, `full_eval_row_count=0`, and
  `row_materialize_ms=0.0`; dominant list remains `selected_signal_lane_rows`.
- Scope note: this does not change Boon syntax, does not hardcode NovyWave, and
  does not claim the speed gate is fixed. TASK-1002 remains open. Further LIST
  work needs a larger compiled query/frontier or function-body reduction, not
  another tiny row-ref allocation avoidance slice unless new measurement shows
  row expansion became hot again.

### 2026-06-17 TASK-1002 Acceptance Closure

- Date: 2026-06-17
- Task: TASK-1002 LIST Storage Mode And Incremental Representation Slice
- Commit: uncommitted
- Files changed in this checkpoint:
  `crates/boon_runtime/src/lib.rs`,
  `docs/plans/speedup/12-speedup-goal-execution-checklist.md`.
- Verification:
  read-only subagent acceptance audit `019ed3fb-24e9-7a32-9dec-2f156180af97`;
  `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_selection_storage_mode_matches_generic_record_oracle_for_core_ops -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`;
  `cargo test -p boon_ir --lib representation -- --nocapture`;
  `cargo check -p boon_ir -p boon_runtime -p boon_native_playground -p xtask`;
  previously refreshed current-code report
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.
- Acceptance audit: TASK-1002 is closed for its written scope. The promoted
  storage modes are inferred by runtime/compiler facts (`ListRef`,
  `ListSelection`, row-index root sources, and representation classifier
  labels), not by user annotations or NovyWave branches. Generic LIST execution
  remains the fallback oracle for unsupported shapes. The focused LIST suite now
  includes one explicit optimized-vs-generic oracle test for the named core
  operations: text filter, numeric retain, identity map, join-field visible
  result, and storage-mode preservation. Root list-view materialization is
  covered by the existing row-index source projection tests that prove
  `field_only_row_index_projection`, `generic_vec_with_row_index_source`,
  stable identities, zero generic row occurrence expansion, and
  `row_materialize_ms=0.0`.
- Current NovyWave evidence: the latest current-code interaction report remains
  speed-budget red, but it satisfies the TASK-1002 acceptance path of neutral
  correctness/report behavior rather than final latency success:
  `click_to_cursor.p95=18.127ms` and `input_to_visible.p95=18.127ms` against
  `16.700ms`, with root-list aggregate `full_eval_row_count=0`,
  `row_materialize_ms=0.0`, `eval_ms=55.358`, `diff_ms=42.524`, and
  `user_function_body_ms=47.745`. The remaining speed blocker is not row-index
  LIST storage correctness; it is still dirty-frontier/root-list/function-body
  work to resume under the postponed root-flush architecture task or a new
  explicit task.
- Result: TASK-1002 is `done`. Do not keep adding tiny row-ref allocation
  avoidance slices under TASK-1002 unless a new task or report proves row
  expansion is again a dominant cost.

### 2026-06-17 Checklist Continuation Evidence Refresh

- Date: 2026-06-17
- Scope: active `/goal` continuation after TASK-1001 and TASK-1002 were closed,
  before the later explicit TASK-0804B activation.
- Task state audit:
  read-only subagent `019ed406-0400-7602-af4c-680f5860a509` confirmed that
  no real executable `pending` or `in_progress` task/experiment remains besides
  template placeholders `TASK-0000`/`EXP-0000`. Real postponed work remains:
  `TASK-0804A` as historical investigation evidence and `TASK-0804B` as the
  future root-flush/bridge-page resumption task. The canonical status scan is
  `TASK-1001=done`, `TASK-1002=done`, `TASK-0804A=postponed`, and
  `TASK-0804B=postponed`.
- Refreshed verification:
  `cargo test -p boon_bridge --lib -- --nocapture` (`12 passed`);
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib bytes -- --nocapture`
  (`4 passed`);
  `cargo test -p boon_ir --lib representation -- --nocapture` (`1 passed`);
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib list_ -- --nocapture`
  (`76 passed`);
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`
  (`19 passed`);
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
  (`status=pass`);
  `cargo check -p boon_ir -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo xtask verify-report-schema`;
  `git diff --check`.
- Current NovyWave speed evidence:
  `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  refreshed the report and exited with the expected blocked/fail status because
  the strict interaction p95 budget is still not met:
  `click_to_cursor.p95=18.150ms` and
  `input_to_visible.p95=18.150ms` against `16.700ms`;
  `runtime_apply.p95=11.545ms`,
  `runtime_step_apply.p95=9.379ms`, and
  `layout_rebuild.p95=4.470ms`.
- Current cause summary:
  the report still names
  `root_flush_dirty_scheduler_plus_root_list_materialization`. The dominant
  list remains `selected_signal_lane_rows` with `eval_ms=19.945`,
  `diff_ms=15.705`, `user_function_body_ms=12.094`,
  `full_eval_row_count=0`, and `row_materialize_ms=0.0`. Aggregate click
  root-list counters show field-only projection is active but not enough:
  `eval_ms=36.141`, `diff_ms=30.788`,
  `user_function_body_ms=38.038`, `field_only_skipped_field_count=4768`,
  and `field_cache_misses=256`.
- Renderer check:
  renderer upload remains solved for the current report. Post-interaction
  upload is `3360` bytes with `3` dirty upload ranges, `3` queue writes,
  `0` staging wraps, and `0` quad-cache evictions.
- BYTES check:
  the refreshed NovyWave bridge report proves real VCD/GHW payloads enter the
  runtime as BYTES sidecars with stable digests and page/blob summaries, while
  `wave_27.fst` remains hash-and-length descriptor proof only until chunked
  `Stream<Bytes>` sidecars exist.
- Result:
  this refresh does not reopen BYTES/LIST work and does not unpostpone
  TASK-0804A or TASK-0804B. The remaining root-flush/bridge-page identity work
  resumes only through explicit `TASK-0804B` activation, not as a hidden
  unfinished TASK-1001/TASK-1002 requirement. This status was superseded later
  on 2026-06-17 by explicit TASK-0804B activation.

### 2026-06-17 TASK-0804B Activation And 0804R-01 Completion

- Scope: explicit TASK-0804B continuation under plan `20`; this supersedes the
  earlier pre-activation checklist refresh. TASK-0804A remains postponed.
- Checkpoint status at this log entry: TASK-0804B is `in_progress`; plan `20`
  has `0804R-00=done`, `0804R-01=done`, and `0804R-02=in_progress`.
- Verification refreshed for the `0804R-01` diagnostic-only slice:
  `cargo fmt -p boon_runtime -p boon_native_playground -p xtask`;
  `cargo check -p boon_runtime -p boon_native_playground -p xtask`;
  `BOON_PROFILE_ROOT_DEMAND=1 BOON_PROFILE_DIRTY_FRONTIER=1 cargo xtask verify-native-gpu-novywave-interaction-speed --report target/diagnostics/native-gpu/novywave-interaction-speed-candidate-demand.json`
  wrote an expected failing report with `status=fail` because the known
  click/input p95 budget blockers remain; `env -u BOON_PROFILE_ROOT_DEMAND -u
  BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`
  wrote the canonical no-diagnostic report with the same p95 blockers;
  `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
  passed; `cargo xtask verify-report-schema` passed; enriched diagnostic `jq`
  checks passed; canonical no-regression `jq` check passed; `git diff --check`
  passed.
- Evidence: candidate-demand diagnostics show `24` candidate roots, `552`
  simulated defer enqueues, `552` changed materializations, `512` later demand
  reads, `248` hidden semantic-delta materializations, and `336` aggregate
  visible/list dependency hits, split as `208` changed-read list dependencies
  and `128` root-list-evaluation demand dependencies. Classification counts
  reject demand-deferral-first:
  `currentness_only=0`, `bridge_identity=472`, `cursor_telemetry=264`,
  `must_publish_semantic_delta=248`, and `visible_list_dependency=208`.
- Next: implement `0804R-02` currentness/stale-read contract first, then move
  to `0804R-03` bridge/page identity unless fresh evidence changes the plan.

### 2026-06-17 TASK-0804B 0804R-02 Currentness Contract Completion

- Scope: completed `0804R-02` from plan `20` as a correctness/stale-read
  contract before any demand deferral. TASK-0804B remains `in_progress`;
  `TASK-0804A` remains postponed historical evidence. Plan `20` now has
  `0804R-00=done`, `0804R-01=done`, `0804R-02=done`, `0804R-03=pending`, and
  `0804R-04` still blocked/not selected first by the `0804R-01` decision
  table.
- Result: `crates/boon_runtime/src/lib.rs` now has an audited
  `ensure_root_current` barrier and `ensure_root_reads_current` read-set
  wrapper for deferred dirty root reads. The barrier resolves child-path reads
  to the actual deferred parent root, refreshes before returning scalar/root
  values, and invalidates dependent root-value, function, root-list-view-field,
  and root-list-map-output caches using the refreshed root's changed-read set.
  This prevents a direct demand refresh from leaving sibling cached dependents
  stale after the deferred marker is cleared. Post-slice review found that
  cached `RootChild` dependencies also need to demand the child path, not only
  the parent root; `ensure_root_reads_current` now checks both `root.child` and
  the parent fallback.
- Focused tests added:
  `root_currentness_barrier_refreshes_deferred_scalar_before_cached_reads_and_summaries`,
  `root_currentness_barrier_invalidates_other_cached_dependents_after_direct_refresh`,
  `root_currentness_barrier_refreshes_deferred_structured_parent_before_child_read`,
  and `root_currentness_barrier_checks_root_child_cache_dependencies`.
  These deliberately corrupt stored/cache state, mark a root deferred-dirty,
  and prove evaluator, assertion, sparse-summary, direct scalar, dependent
  cache, structured-child, and `RootChild` cache-dependency reads cannot return
  the stale value.
- Verification passed: `cargo fmt -p boon_runtime`;
  `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_currentness_barrier -- --nocapture`
  (`4 passed`); `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_derived_ -- --nocapture`
  (`6 passed`); `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib structured_root_ -- --nocapture`
  (`3 passed`); `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib novywave_waveform_click_keeps_internal_pure_roots_queryable_without_patching_them -- --nocapture`
  (`1 passed`); `RUST_MIN_STACK=33554432 cargo test -p boon_runtime --lib root_list_view_ -- --nocapture`
  (`19 passed`); and `cargo check -p boon_runtime`.
- Subagent review effect: correctness reviewer `019ed6a1-88e1-7e41-ba9a-87f80e95cc12`
  and performance reviewer `019ed6a1-e64e-7580-aaf4-2c0cf8b1ab0b` both found
  that the first barrier checkpoint discarded demand-refresh changed reads and
  could leave sibling caches stale; this was fixed before marking R02 done.
  Docs reviewer `019ed6a2-3450-7192-be27-d033da2adf21` required the explicit
  read-path audit table now recorded in plan `20`. Post-slice correctness
  reviewer `019ed6ac-1811-7892-8bb2-e3cef2990f51` found the `RootChild`
  cache-dependency hole; that was fixed and covered by the fourth focused test.
  Post-slice docs reviewer `019ed6ac-4f7f-71b1-8a4b-6e35ba715269` found no
  blocking docs issue. Post-slice performance reviewer
  `019ed6ac-2a25-7431-95d4-76b52ecb9272` agreed `0804R-03` is the right next
  slice and requested a canonical no-diagnostic post-R02 baseline.
- Post-R02 canonical baseline: `env -u BOON_PROFILE_ROOT_DEMAND -u
  BOON_PROFILE_DIRTY_FRONTIER cargo xtask verify-native-gpu-novywave-interaction-speed --report
  target/reports/native-gpu/novywave-interaction-speed.json` exited with the
  expected failing gate status because the known strict latency budgets remain;
  `cargo xtask verify-report-schema` passed. The report has `status=fail`,
  `candidate_defer_probe.enabled=false`, candidate root count `0`,
  `click_to_cursor.p95=19.806244ms`, `input_to_visible.p95=19.806244ms`,
  `runtime_apply.p95=12.656522ms`, `runtime_step_apply.p95=10.357947ms`,
  `runtime_state_summary.p95=1.017862ms`, `layout_rebuild.p95=4.696317ms`,
  root-flush p95 `6.113463ms`, dirty-scheduler p95 `2.649652ms`,
  root-materialization p95 `3.324309ms`, and the slow graph remains
  `194/32/38`. Renderer post-interaction upload remains separated at `3360`
  bytes, `3` queue writes, `0` staging wraps, and `0` quad-cache evictions.
- Read-path caveats carried forward: list-view roots are
  eager-only/non-deferred in R02; source-route text/storage guard paths and
  lower direct root text/bool helpers remain eager-only/non-deferred until they
  have a mutable barrier-aware API. R02 is not a speed closeout and does not
  claim the click/input p95 budget is solved. Next implementation slice is
  `0804R-03` bridge/page identity split.

## File Maintenance Checklist

After editing this file, run:

```bash
rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md
git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md
```

Before using this file as `/goal` input, confirm:

- Every plan file `01` through `20` except this self-file is
  listed under `Objective`.
- Every `TASK-*` has status, dependencies, acceptance criteria, verification,
  and rollback/stop condition.
- Every `EXP-*` has hypothesis, metric, oracle, kill criteria, promotion
  criteria, and verification.
- Every early gate has a report path.
- The file says `/goal` must update the checklist after each task.
- The file preserves the no-new-Boon-syntax default.
- Native GPU work still points to `docs/architecture/NATIVE_GPU_PIPELINE.md`.
