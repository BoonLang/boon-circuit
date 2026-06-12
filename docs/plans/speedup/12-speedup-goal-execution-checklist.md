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

## Task Schema

Every implementation task must use this exact shape:

```md
### TASK-0000 Short Title
Status: pending
Type: implementation | gate | refactor | measurement | cleanup
Priority: P0 | P1 | P2 | P3
Depends on: none | TASK-0000
Source plans: 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11
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
Status: pending
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

### TASK-0002 Classify And Fix Known Scenario Drift
Status: pending
Type: cleanup
Priority: P0
Depends on: TASK-0001
Source plans: 06, 09, 10
Likely areas: `examples/manifest.toml`, `examples/novywave.scn`, `examples/cells.scn`, `examples/todomvc.scn`, `examples/todo_mvc_physical.scn`
Goal:
Make the existing bundled scenarios pass the integrity rules or explicitly classify generated/probe cases.
Acceptance:
- NovyWave duplicate `select-primary-file` is removed, renamed, or classified without duplicate identity.
- Cells scroll/focus labels are executable `.scn` steps or explicitly generated probes with provenance.
- TodoMVC `reject-empty-todo` manifest reference is reconciled with `reject-empty-todo-type` and `reject-empty-todo-submit`.
- TodoMVC Physical action steps without assertions are given assertions or documented exemptions.
- The integrity report is passing before any scenario output is used as acceptance evidence.
Verification:
- `cargo xtask verify-scenario-manifest-integrity --report target/reports/scenario-manifest-integrity.json`
- `cargo test -p boon_runtime --lib`
Rollback / stop condition:
- Stop if fixing drift would weaken a scenario assertion. Add a follow-up task to improve scenario expressiveness instead.
Notes:
- Do not delete scenario coverage just to make the gate green.

### TASK-0003 Interaction, Proof, And Diagnostic Report Modes
Status: pending
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

### TASK-0004 Flow IDs And Release-Mode Stage Counters
Status: pending
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

### TASK-0005 SourceStore Row Bind And Unbind Correctness
Status: pending
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

### TASK-0006 Document Patch Result And Invariants
Status: pending
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

### TASK-0007 Scaffold Proof Demotion And Readback Deadlines
Status: pending
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

## Phase 1: Parser, IR, Typecheck, And Semantic Index

### TASK-0101 Semantic Index Skeleton
Status: pending
Type: implementation
Priority: P1
Depends on: TASK-0004
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

### TASK-0102 Cross-Stage Symbol Interning And Collision Diagnostics
Status: pending
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

### TASK-0103 Typechecker Readiness Fallback Gates
Status: pending
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

## Phase 2: Runtime Correctness And Source Routing

### TASK-0201 Typed Source Route Op Streams
Status: pending
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

### TASK-0202 Public Source Batch Runtime Boundary
Status: pending
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

### TASK-0203 Row Identity, Generation, And Stale Event Rejection
Status: pending
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

## Phase 3: Storage, Dirty Sets, List Indexes, And Deltas

### TASK-0301 List Scan Counters And First Inferred Indexes
Status: pending
Type: measurement
Priority: P1
Depends on: TASK-0004, TASK-0203
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

### TASK-0302 Derived List Delta Operators
Status: pending
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

### TASK-0303 Dirty Set Redesign Gate
Status: pending
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

## Phase 4: Document, Layout, Materialization, And Passive Scroll

### TASK-0401 Generic Virtual Materialization Protocol
Status: pending
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

### TASK-0402 Passive Scroll Property-Tree Path
Status: pending
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

### TASK-0403 Computed Style IDs And Invalidation Classes
Status: pending
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

## Phase 5: Renderer, Text, Assets, And GPU Uploads

### TASK-0501 Retained Render Chunk IDs
Status: pending
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

### TASK-0502 POD And Ring-Buffer GPU Upload Path
Status: pending
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
- This task should only happen after `EXP-0001` justifies the dependency/pattern.

### TASK-0503 RenderScene Boundary And Renderer Semantics Cleanup
Status: pending
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

### TASK-0504 Shared Text Service And Bounded Shaped-Run Cache
Status: pending
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

### TASK-0505 AssetRef And Async Asset Pipeline
Status: pending
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

## Phase 6: Host Event Loop, IPC, And Dev/Preview Separation

### TASK-0601 Live IPC And Latest-Wins Worker Counters
Status: pending
Type: measurement
Priority: P1
Depends on: TASK-0003, TASK-0004
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

### TASK-0602 Event-Driven Loop And Fixed Sleep Audit
Status: pending
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

### TASK-0603 Typed Hit Side Table
Status: pending
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

## Phase 7: Bridge, Effects, And NovyWave Page Refs

### TASK-0701 Bridge Schema And Effect Kernel Skeleton
Status: pending
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

### TASK-0702 NovyWave PageRef, ArtifactRef, And BlobRef Fixture Path
Status: pending
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

### TASK-0703 NovyWave View Over Rows, Pages, And Virtualization
Status: pending
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

## Phase 8: BoonDriver, Reports, Anti-Cheating, And Scenarios

### TASK-0801 BoonDriver Scenario Engine Path
Status: pending
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
Status: pending
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

### TASK-0803 Metamorphic Hidden Fixture Gate
Status: pending
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
- This gate can start with one example and expand.

### TASK-0804 NovyWave Scenario And Speed Gates
Status: pending
Type: gate
Priority: P2
Depends on: TASK-0703, TASK-0801, TASK-0802
Source plans: 05, 06, 07, 09, 10, 11
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

## Phase 9: Low-Level Rust Experiments

### EXP-0001 `bytemuck` POD GPU Uploads
Status: pending
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

### EXP-0002 `smallvec` Or `arrayvec` For Tiny Hot Lists
Status: pending
Type: experiment
Depends on: TASK-0004
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

### EXP-0003 Interner Crate Versus Custom Symbol Table
Status: pending
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

### EXP-0004 Dirty Set Representation
Status: pending
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

### EXP-0005 Shader-Side Shapes
Status: pending
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

### EXP-0006 Generated Rust Or Cranelift Kernels
Status: pending
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

### EXP-0007 Large-List Dataflow Kernel
Status: pending
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

## Phase 10: Compiled Artifact, Bytecode, And Future Kernel Work

### TASK-0901 `.boonc` Compiled Artifact MVP
Status: pending
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
- `cargo xtask verify-report-schema`
Rollback / stop condition:
- Stop if artifact scope is too broad. Split into serialization, runtime load, scenario run, and report hash child tasks.
Notes:
- This task should not remove the current interpreter path.

### TASK-0902 Expression Bytecode Or Micro-Op Interpreter
Status: pending
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
- A scenario report compares interpreter and bytecode outputs for at least one example.
Rollback / stop condition:
- Stop if expression semantics are not sufficiently typed. Return to typechecker readiness tasks.
Notes:
- This task unlocks later generated kernel experiments.

## Progress Log

Append entries here as `/goal` executes tasks. Do not delete older entries.

```md
- Date:
- Task:
- Commit:
- Files changed:
- Verification:
- Result:
- Follow-up:
```

## File Maintenance Checklist

After editing this file, run:

```bash
rg -n "Status: pending|Depends on:|Acceptance:|Verification:|Kill criteria|Progress Log" docs/plans/speedup/12-speedup-goal-execution-checklist.md
git diff --check -- docs/plans/speedup/12-speedup-goal-execution-checklist.md
```

Before using this file as `/goal` input, confirm:

- Every plan file `01` through `11` is listed under `Objective`.
- Every `TASK-*` has status, dependencies, acceptance criteria, verification,
  and rollback/stop condition.
- Every `EXP-*` has hypothesis, metric, oracle, kill criteria, promotion
  criteria, and verification.
- Every early gate has a report path.
- The file says `/goal` must update the checklist after each task.
- The file preserves the no-new-Boon-syntax default.
- Native GPU work still points to `docs/architecture/NATIVE_GPU_PIPELINE.md`.
