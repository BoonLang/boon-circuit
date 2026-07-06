# `/goal` Prompt

Use this prompt for the next unattended unified implementation pass. The
long-form prompt and native performance evidence are embedded in
`docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md` and
`docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md`.

Primary source of truth:

- `docs/architecture/NATIVE_GPU_PIPELINE.md` remains the native GPU contract.
- `docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md` is the active unified
  implementation prompt.
- `docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md` is the active
  native performance and render-graph implementation plan.
- AGENTS.md instructions remain binding.

Current checkpoint:

- Latest runtime cleanup cut removed the test-only `LiveRuntimeEngine::LoadedRuntime`
  branch, the `LoadedRuntimeHarness` wrapper, the `LoadedRuntime` shell, and the
  private `apply_checked_step*` helpers that duplicated product event execution.
  `crates/boon_runtime/src/lib.rs` now has no `LoadedRuntime`,
  `LoadedRuntimeHarness`, or `GenericScheduledRuntime` code references. Do not
  recreate a `LoadedRuntime` wrapper, `GenericScheduledRuntime` wrapper, or
  quarantine bucket to keep old tests alive. Several
  `LoadedRuntimeHarness::from_source` tests were deleted rather than migrated
  because direct PlanExecutor runs exposed unsupported old-fixture semantics
  rather than current product evidence: unqualified root initial copies, row
  latest/source-leaf summaries, arbitrary derived chunk summaries, old render
  projection fixtures, broad BYTES builtin summary shape, and Cells recompute
  sample accounting. Replace those only with PlanExecutor/product tests after
  the missing executor semantics are deliberately implemented.
- Latest report/control-plane cleanup removed stale
  `loaded_runtime_from_artifact` / `legacy_loaded_runtime_from_artifact`
  inspection requirements and renamed generic runtime slice labels away from
  `generic_loaded_runtime*`. The compiler-boundary audit now checks that the
  loaded-runtime engine branch, shell, harness, and legacy fallback helpers are
  removed, while keeping the remaining `GenericScheduledRuntime` test island
  explicit. Fresh focused evidence for this slice: `cargo check -q -p xtask -p
  boon_runtime -p boon_report_schema`, `cargo test -q -p boon_report_schema
  --lib`, `cargo test -q -p boon_runtime --lib --no-run`, focused runtime
  artifact/slice tests, and `cargo run -q -p xtask -- verify-report-schema
  target/reports/compiler-boundaries.json` pass. Fresh
  `verify-compiler-boundaries` is schema-valid but still reports `status=fail`
  because two broader PlanExecutor migration blockers remain:
  PlanExecutor core does not yet own indexed list row expression refresh
  iteration, and `boon_runtime` live runtime constructors still require a
  `TypedProgram` instead of the compiled runtime program.
- Latest PlanExecutor boundary cut moved the remaining root-context startup
  list row-expression refresh wrapper into `boon_plan_executor`. Runtime now
  calls an executor-owned root-aware startup refresh boundary instead of
  importing the generic `_with` refresh iterator and supplying runtime-owned
  row-expression iteration. Fresh `verify-compiler-boundaries` now reports
  `planexecutor-list-row-expression-refresh-extracted=true`; the only remaining
  blocker is `runtime-live-constructors-use-compiled-program=false`.
- Latest compiler-boundary control-plane cleanup updated the live-constructor
  audit to match the post-LoadedRuntime architecture: `CachedRuntimePlan` keeps
  `CompiledProgram`, live PlanExecutor sessions consume compiled runtime data,
  and the old typed-IR/LoadedRuntime constructor shapes remain rejected. Fresh
  `cargo run -q -p xtask -- verify-compiler-boundaries --report
  target/reports/compiler-boundaries.json` now passes, and
  `cargo run -q -p xtask -- verify-report-schema
  target/reports/compiler-boundaries.json` passes.
- Latest runtime deletion cut removed the remaining test-only
  `GenericScheduledRuntime` wrapper and its old scheduled-runtime method body
  from `crates/boon_runtime/src/lib.rs`, while keeping shared value conversion
  helpers as normal free functions. The compiler-boundary gate now checks
  `generic-scheduled-runtime-removed=true` instead of accepting a quarantined
  island. Fresh evidence: `cargo check -q -p boon_runtime -p xtask`,
  `cargo test -q -p boon_runtime --lib --no-run`, focused
  `generic_runtime_owns_todo_list_structural_checks`, fresh
  `verify-compiler-boundaries`, and schema validation of
  `target/reports/compiler-boundaries.json` pass.
- Latest architecture cleanup checkpoint moved root branch state and startup
  row-expression refresh behind the PlanExecutor boundary. Runtime no longer
  owns `RootBytesRuntimeEnvironment`, no longer passes loose root JSON/private
  BYTES/fixed-bank maps into `execute_root_scalar_update_branch(...)`, and no
  longer owns the startup row-expression evaluator. This is not complete
  readiness: explicit diagnostic legacy comparison report surfaces, native
  legacy negative counters, stale native Cells reports, and native/product
  performance work still need deletion, replacement, or fresh verification.
- Scenario-events product reports are now PlanExecutor-only. The
  `ScenarioEventsCommandOutputInput` / `ScenarioEventsCommandReportInput`
  structs no longer accept `legacy_comparison`, `legacy_comparison_acceptance`,
  or `compare_legacy`; `run_plan_scenario_events(...)` no longer constructs
  disabled legacy placeholders; the report schema rejects `legacy_comparison*`
  fields on `run-plan-scenario-events`; and `verify-compiler-boundaries` has a
  dedicated check that this product path cannot regain legacy comparison
  plumbing. Do not reintroduce disabled legacy fields to make old reports pass.
  The unused enabled root-scenario legacy comparison assembly has also been
  deleted, `verify-compiler-boundaries` now checks that it stays removed, and
  BYTES storage profile reports no longer emit `legacy_comparison_enabled=false`
  sentinels. Product source-route/root-scenario reports now also omit disabled
  `legacy_comparison*` objects, legacy parity rows, and legacy status fields;
  BYTES file read/write wrapper reports no longer copy legacy comparison from
  their underlying root-scenario reports. The product CLI no longer exposes a
  `run --engine ...` switch, no longer recognizes retired legacy/compare aliases,
  and native/source replay refresh commands now exercise the default
  PlanExecutor-backed `boon_cli run` path without `--engine plan`. In normal
  builds `LiveRuntimeEngine` can no longer hold `LoadedRuntime`; the legacy
  variant, legacy constructors, and `apply_checked_step*` legacy fallback helpers
  are `#[cfg(test)]` quarantined while old runtime coverage is migrated or
  deleted. Default `hello_3d` world-output construction now proves PlanExecutor
  provenance rather than expecting a hidden fallback rejection. `LoadedRuntime`
  and `GenericScheduledRuntime` themselves are now `#[cfg(test)]`, with the
  remaining production utility (`List` count predicate row-field extraction)
  pulled into a free helper. Old-runtime tests now construct the legacy engine
  through `LoadedRuntimeHarness`; the `LiveRuntime::*_legacy` constructor
  methods have been deleted, and `verify-compiler-boundaries` rejects
  reintroducing those constructor methods or direct `LiveRuntime::*_legacy`
  calls. The raw `run_loaded_runtime_scenario*` helpers and the old
  `run_loaded_runtime_source_initial_state` diagnostic helper have now been
  deleted from `boon_runtime`, along with the generic `ScenarioExecutor` trait,
  the old `run_generic_scenario` report loop, `base_example_report`,
  `enrich_report`, retired runtime change-batch protocol helpers, and the
  obsolete `RuntimeProfile` report enum. Do not recreate them to make stale
  report tests pass. The old product-style `LoadedRuntimeHarness` live-source
  cluster for TodoMVC/Counter/NovyWave has also been cut, with PlanExecutor
  replacement tests for source-batch sequence/event-id rejection and duplicate
  TodoMVC occurrence routing; the last direct `LoadedRuntimeHarness::new` test
  was deleted as an obsolete row-local TodoMVC edit-mode expectation. The
  four physical TodoMVC product-behavior tests that still used
  `LoadedRuntimeHarness::from_project` have also been deleted in favor of the
  PlanExecutor scenario-events/source-replay path. A fresh
  `todo_mvc_physical-scenario-events-full.json` report passes with
  `status=pass`, `plan_status=pass`, and 22
  selected source-event steps, and its schema check passes; the current native
  physical preview report is still not readiness evidence because its visual
  content status is fail. The first generic `LoadedRuntimeHarness::from_source`
  behavior/internal-test slice has also been cut instead of preserved through hidden
  old-runtime execution: source text payload summary, source payload concat
  update, root-derived dependency materialization, root-derived revisit, and
  root-list-view identity-only behavior tests were removed, along with
  old-GenericRuntime-only currentness cache-corruption tests, exact lookup
  invalidation helper tests, and lazy chunk summary scan-counter tests. One
  windowed materialization summary test now runs through the normal
  `LiveRuntime::from_source` PlanExecutor-backed constructor. Direct
  PlanExecutor migration
  attempts exposed real unsupported product gaps (incomplete arbitrary-fixture
  document summary/state surfaces, unresolved root initial field copies, and
  missing typed field ids for a row structured-parent fixture). A later
  checkpoint implemented typed MachinePlan operands plus root/indexed
  PlanExecutor execution for `PrefixPayloadConcat` and `PrefixRootConcat`, with
  focused product-path coverage in
  `root_scalar_plan_executor_replays_prefix_concat_update_branches`, so do not
  keep treating prefix concat as a legacy-runtime blocker.
  Do not reintroduce the deleted tests through
  `LoadedRuntimeHarness`; replace them only with PlanExecutor/product tests
  after the missing executor semantics/diagnostics are implemented. This cut
  reduced direct `LoadedRuntimeHarness::from_source` calls in
  `crates/boon_runtime/src/lib.rs` from 119 to 106. The remaining legacy
  cuts are deleting or migrating
  the test-only `LoadedRuntime` / `GenericScheduledRuntime` coverage island, now
  mostly lower-level runtime/list/currentness diagnostics plus explicit
  `LoadedRuntimeHarness::from_*` diagnostics, and any native legacy negative
  counters that are no longer useful as removal guards.
- Default-engine readiness is fresh and passing with `default_engine=plan`,
  `default_switch_allowed=true`, TodoMVC compare, Cells compare, explicit
  legacy smoke, and default PlanExecutor execution all schema-valid.
- BYTES/MachinePlan aggregate is not fresh in the current worktree. Its new
  failure taxonomy shows `0` structural schema failures, `0` status failures,
  `0` fallback failures, `62` refresh-debt children, and `0` true blocker
  children across `66` required reports. The aggregate now emits
  machine-readable `refresh_commands[].argv` and `true_blocker_children`; use
  `xtask run-report-refresh-queue ... --rerun-aggregate --label ... --limit ...`
  and compact `jq` summaries instead of inspecting large child reports
  manually. Dry-runs are schema-valid and report that the aggregate rerun was
  intentionally skipped. Cells refreshes must stay PlanExecutor/product-only;
  public CLI legacy-compare refreshes are retired.
- Native handoff no longer consumes native-preview source replay side reports.
  Preview E2E must prove native behavior from app-owned host input, runtime
  outputs, retained ProductFrameGraph evidence, and WGPU/readback artifacts.
  PlanExecutor source replay remains BYTES/MachinePlan semantic evidence only
  and must not be reintroduced as a native `upstream_dependency`.
- Native GPU handoff aggregate is not fresh in the current worktree. Its new
  failure taxonomy separates freshness-only schema/contract failures from
  product-contract failures. The handoff manifest now requires
  release/hardware preview E2E reports and a focus-safe hardware
  product-surface present-floor report. The public
  `verify-native-gpu-present-floor --report ...` refresh path uses
  `cosmic-background-launch`; raw `--inner-app-window` reports are diagnostic
  children and no longer satisfy the handoff contract. After the scoped
  fingerprint, manifest dependency, and native aggregate schema WIP, the fresh
  aggregate report is schema-valid but still `status=fail`: after the
  native source-replay cut it reports `16` refresh-debt children and `0` true
  blocker children. The only native `report_dependency_graph` edge is
  `todomvc-physical-reference-parity -> preview-e2e-todo_mvc_physical` with
  kind `consumes-native-report`. This is refresh/control-plane debt, not fresh
  Cells product evidence. The aggregate now emits manifest-canonical
  `refresh_argv` and diagnostic `observed_argv`; run queue-filtered refreshes
  before broad handoff debugging.
- The native aggregate now fast-paths stale child identity. If a child report has
  stale git, worktree, or binary identity, `verify-native-gpu-all` records one
  `identity-freshness-fast-path` refresh-debt item and skips schema, semantic,
  and artifact validation until the report is regenerated. This avoids treating
  stale megabyte reports as fresh product failures.
- The next implementation work should refresh/fix readiness blockers with the
  aggregate refresh plan first, then promote the renderer-owned retained
  `ProductFrameGraph`; do not restart Cells micro-optimization unless fresh
  product-latency evidence regresses.
- Native handoff no longer has BYTES-owned native-preview source replay labels
  in its dependency graph. Native handoff still has remaining refresh debt for
  heavier product/window labels.
- The renderer graph has advanced from the old
  `executor_wrapped_product_passes` contract to a typed, renderer-owned linear
  `ProductFrameGraph` that reports
  `renderer_render_graph_execution_kind=retained_product_frame_graph_linear_v1`.
  It now owns retained resource state in `VisibleLayoutRenderer`, reports
  retained resource epoch hashes plus dirty/reused resource counts, and keeps
  topology/lifetime hashes separate from mutable workload/resource state. This
  is still not the final dirty-resource scheduler.
- Source-free compiled artifacts now embed a canonical verified `MachinePlan`.
  Artifact inspection instantiates `PlanExecutorLiveSession` from the embedded
  plan with `runtime_engine=plan_executor`, `source_reparse_attempted=false`,
  and `generic_fallback_enabled=false`; representative artifact scenario parity
  now compares source PlanExecutor output to artifact-loaded PlanExecutor output
  for TodoMVC, Cells, and an indexed BYTES source-payload fixture. A separate
  source-deleted TodoMVC test keeps source-free execution explicit. Remaining
  artifact work is eventual deletion/quarantine of the explicit legacy artifact
  diagnostic path.
- PlanExecutor live/session/batch parity now has representative coverage across
  TodoMVC, Cells, root BYTES source payload, and indexed BYTES source payload.
  The helper compares direct PlanExecutor step replay, live source events,
  `PlanExecutorLiveSession`, and `LiveRuntime::apply_source_batch_turn` against
  the same selected source-event scenario steps.
- PlanExecutor command reports now use top-level `status` for product
  acceptance and `plan_executor_status` for the executor lane; they no longer
  carry the obsolete `comparison_status` or `accepted_for_product_status`
  compatibility fields. Native source replay can use product acceptance without
  confusing legacy comparison state with product execution failure. Native
  replay consumers reject missing executor status, so stale pre-split reports
  are refresh debt rather than usable product evidence.
- Native reports now carry scoped worktree fingerprints. The native aggregate
  may use the `native-gpu-handoff` scoped fingerprint for child freshness while
  still requiring current git and binary identity. The scope includes product
  and verifier inputs and excludes progress/goal prose, so docs-only plan churn
  stops invalidating otherwise current native product reports.
- Native handoff report dependencies now come from
  `docs/architecture/native_gpu_handoff_manifest.json`. Native dependencies
  are native-report edges only; PlanExecutor source replay is not native proof.
  Aggregate output includes native edges in `report_dependency_graph` and
  `required_reports[].upstream_dependencies`.
- `verify-report-schema` now has command-specific validation for
  `verify-native-gpu-all`: native handoff aggregate reports must match the
  canonical manifest, expose manifest-owned dependency edges, keep refresh
  commands bounded and replayable, and keep refresh/product/dependency taxonomy
  counts consistent. A failing native aggregate can still be schema-valid when
  it honestly reports refresh debt or fresh blockers.
- `run-report-refresh-queue` is now dependency-aware and schema-locked. Native
  refresh dry-runs expand only manifest-owned native-report dependencies, such
  as `todomvc-physical-reference-parity -> preview-e2e-todo_mvc_physical`.
  Queue reports
  include `selection_mode`, dependency expansion/deferred counts,
  `refresh_phase_summaries`, ordered `refresh_execution_plan`,
  `selected_by_label_filter`, `boon_cli_prebuild`, and owner-aggregate rerun
  intent. Non-dry queues build `boon_cli` once before executing selected replay
  refreshes, while dry-runs report that preflight as skipped. This removes one
  major source of stale-report churn: run the queue before interpreting native
  preview failures as product bugs.
- The dependency-aware queue was run non-dry for the upstream native-preview
  replay prerequisites and for the first eight native static/contract labels.
  Those selected refreshes passed and temporarily reduced native refresh debt to
  the remaining heavy/window labels. After adding native aggregate
  `true_blocker_child_count` / `true_blocker_children` and rebuilding `xtask`,
  the regenerated native aggregate is schema-valid and reports
  `true_blocker_child_count=0`, but all native children are refresh debt again
  because the verifier binary hash changed. Treat this as control-plane identity
  churn, not fresh Cells product evidence.
- Native `xtask` reports now carry scoped `verifier_identity` evidence. For
  native/verifier reports, schema and the native aggregate treat a matching
  verifier identity as freshness authority over the legacy `binary_hash`, and
  treat a mismatched scoped identity as refresh debt even if the binary hash
  happens to match. This cuts one major source of native control-plane churn.
  A fresh `verify-platform-contract` run proves the new report shape, and the
  current native aggregate classifies that child as
  `binary_freshness_basis=scoped-verifier-identity`; the remaining native
  children are still legacy stale-binary reports until refreshed once.
- BYTES-owned `boon_cli` source replay reports now carry
  `worktree_fingerprint_scope=plan-executor-source-replay`,
  `worktree_scoped_fingerprint`, scoped `worktree_fingerprints`, and
  `source_replay_identity`. Those reports are validated by BYTES/MachinePlan
  gates only; native aggregate freshness no longer checks
  `source_replay_identity`.
- Scoped worktree fingerprints now hash the scoped committed `HEAD` tree plus
  scoped dirty status/diff in both `boon_runtime` source-replay reports and
  `xtask` aggregate verification. The BYTES/MachinePlan aggregate now uses
  `plan-executor-source-replay` scoped freshness plus fresh
  `source_replay_identity` for `run-plan-scenario-events` children instead of
  treating full-worktree/git mismatch as a product blocker. After this scheme
  change, `verify-bytes-machine-plan-all --check-existing` is schema-valid and
  reports `refresh_debt_child_count=65`, `true_blocker_child_count=0`; the
  native-preview source-replay children have fresh source replay identity but
  need one refresh for the new scoped fingerprint.
- `boon_cli run --engine plan --report ...` is quiet by default now. Public CLI
  legacy runtime and legacy-comparison replay commands are retired; normal
  refreshes should rely on report files and compact `jq` summaries, with
  `--print-report` only for intentional stdout JSON inspection.
- `run_plan_scenario_events(...)` is product-only now. It no longer accepts a
  `compare_legacy` switch or calls the runtime bridge that replayed selected
  steps through `LoadedRuntime`; scenario-event reports may still carry
  schema-compatible `legacy_comparison.enabled=false` fields until the report
  schema migration removes them.
- `verify-compiler-boundaries` is down to five known blockers after removing
  the direct compile facade false positive and moving startup list-row
  expression refresh iteration into PlanExecutor. The remaining cluster is
  list row initial-state refresh/mirror ownership plus root-state/update
  ownership in the runtime scenario fallback path.
- The native aggregate now treats a fresh child report whose own `blockers[]`
  are freshness-only as refresh debt, not a true product blocker. This fixed the
  physical TodoMVC reference parity false blocker: after refreshing only
  `todomvc-physical-reference-parity`, the current aggregate is schema-valid and
  reports `refresh_debt_child_count=18`,
  `identity_fast_refresh_child_count=17`,
  `refresh_first_product_contract_child_count=1`,
  `true_blocker_child_count=0`, and refresh reasons
  `child-reported-freshness=1` plus `identity-freshness-fast-path=17`.
- `run-report-refresh-queue` reports now sidecar bulky controller arrays
  (`results`, `closed_loop_cycles`, `owner_aggregate_reruns`, and post-aggregate
  remaining-command lists) behind SHA-256/byte-length checked JSON sidecar refs.
  A compact dry-run smoke report validated with `verify-report-schema` and was
  about 7 KB inline with three small sidecars, so queue reports no longer need
  to dump large child-result payloads into the conversation.
- The full native refresh queue was executed once with
  `--until-clean --max-runs 2`. It executed 22 refresh commands, passed 17, and
  exposed 5 fresh failing labels before the next schema-code change invalidated
  current native report identity again: `cells-visible-click-e2e-release`,
  `preview-e2e-cells`, `preview-e2e-todo_mvc_physical`,
  `preview-e2e-todomvc`, and `todomvc-physical-reference-parity`. The queue
  report is now schema-valid after fixing nested sidecar validation for
  `post_refresh_aggregate.remaining_selected_refresh_commands`.
- Fresh Cells click evidence from that queue says the accepted product lane is
  within budget (`input_to_present_ms.p95=11.443ms`, `max=13.111ms`) and proof
  is separated, but the report still fails because retained updates publish via
  `deferred_visible_sync` / `post_turn_full_document` with missing render-scene
  patch evidence, and the runtime-work contract still flags one recomputed field
  per selection click despite zero list scans, zero row scans, and zero root
  materialization candidates. The next Cells cut is retained
  selection/formula-bar patch publication plus a runtime-work contract audit,
  not another broad renderer/proof micro-optimization loop.
- Fresh preview E2E failures are harness/coverage blockers: Cells needs headed
  visual cursor/readback evidence, TodoMVC needs manifest-required real-window
  coverage instead of only `boon-driver` evidence, and physical TodoMVC needs
  live-state plus app-window input provenance proof. Fix those harness gaps
  before using preview E2E failures as product renderer evidence.
- Native handoff report dependencies now support native reports only. The
  manifest models `todomvc-physical-reference-parity ->
  preview-e2e-todo_mvc_physical`, `verify-native-gpu-all` emits that edge with
  owner `verify-native-gpu-all`, and `run-report-refresh-queue` expands
  label-filtered refreshes through the graph while deduplicating duplicate
  stale labels.

Short slash command:

```text
/goal Complete the next unified architecture slice for /home/martinkavik/repos/boon-circuit without getting stuck in Cells micro-optimizations.

Start by inspecting current HEAD, git status, AGENTS.md, docs/architecture/NATIVE_GPU_PIPELINE.md, docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md, docs/plans/GOAL_PROMPT.md, and current target reports. Treat stale reports as non-proof.

Use subagents before implementation:
- one for native GPU/report freshness and verifier debt,
- one for PlanExecutor/LiveRuntime unification,
- one for retained ProductRenderGraph/resource graph architecture,
- optionally one for negative/legacy-path audit.
Fold their results into one implementation plan before editing.

Implementation priorities:
1. Use the current persistent PlanExecutor live-session checkpoint instead of restarting from whole-scenario command helpers. `PlanExecutorRuntimeState`, `PlanExecutorLiveSession`, normal `LiveRuntime::{from_source,from_project,from_project_profiled}` construction, PlanExecutor-backed batch/single-event/scenario-helper live turns, PlanExecutor source inventory, PlanExecutor targeted value/query summaries, and PlanExecutor document/window summaries exist; extend that mode rather than creating another parallel runtime surface.
2. Keep normal runtime execution PlanExecutor-backed. `boon_cli run`, default document `LiveRuntime::{from_source,from_project,from_project_profiled}`, native document preview paths, and verifier helper construction must report the same PlanExecutor engine provenance. Scenario-shaped `LiveRuntime::new/new_from_project` and explicit `from_*_plan_executor` migration aliases have been deleted; parse scenarios explicitly where a verifier needs scenario validation, then use the normal runtime constructors. Test-only legacy inspections and old legacy-runtime unit coverage must go through explicit legacy accessors/constructors if they still exist, native document-summary/layout-proof helpers use profiled PlanExecutor project construction, native preview now selects PlanExecutor for document-only projects while keeping `world:` / `manufacturing:` output roots on explicit LoadedRuntime diagnostics until PlanExecutor supports those outputs, and operator-host ACK/native E2E schema now require document input provenance `engine=plan_executor` with `generic_fallback_enabled=false`. Preserve focused runtime checks while refreshing default-engine/readiness reports and removing any remaining hidden normal-path legacy fallback. Keep legacy execution only behind explicit diagnostic/compare/output-root gates, and do not silently fall back from PlanExecutor mode to `LoadedRuntime` / `GenericScheduledRuntime`.
3. Add equivalence coverage proving `run_plan_scenario_events`, `PlanExecutorLiveSession`, `LiveRuntime::apply_source_batch_turn`, and artifact-loaded PlanExecutor match. TodoMVC explicit PlanExecutor batch equivalence, representative artifact-loaded PlanExecutor parity for TodoMVC/Cells/indexed BYTES, representative live/session/batch parity for TodoMVC/Cells/root BYTES/indexed BYTES, a source-deleted TodoMVC artifact scenario, and focused TodoMVC/Cells document-summary coverage exist; runtime dependency semantics are now sharper for exact `List/find`, list-structure reads, stable source-identity patching, projected source bindings, and thread-local BYTES counters. Continue with dedicated demand-current/list-find edge cases, explicit legacy diagnostic quarantine/deletion, and render-patch ownership by PlanExecutor/render graph where the surface is executable.
4. Refresh current evidence after code cuts. Start from aggregate-provided structured `refresh_commands[].argv` and the `run-report-refresh-queue --until-clean --max-runs N` runner (`--closed-loop` is an alias; `--rerun-aggregate` is one cycle only): focus-safe public present-floor, release/hardware preview E2E TodoMVC, release/hardware preview E2E Cells, release/hardware preview E2E physical TodoMVC, then `verify-native-gpu-all --check-existing`. Rerun BYTES/MachinePlan/default-engine/readiness reports from their aggregate refresh queue. If reports fail only because they are stale or schema-sidecar issues, fix the verifier/report contract instead of changing product code.
5. Treat the verifier/control plane as architecture, not bookkeeping. Use `refresh_commands[].argv`, `refresh_debt_child_count`, `identity_fast_refresh_child_count`, `true_blocker_children`, `product_contract_children`, `refresh_first_product_contract_children`, `report_dependency_graph`, `required_reports[].upstream_dependencies`, and scoped freshness fields from aggregate reports to decide the next command; do not dump megabyte child reports into the conversation and do not spend time proving stale reports are stale. If an aggregate cannot tell refresh debt from true blockers, cannot replay child commands exactly, or has a verifier-consumed side report hidden outside a manifest/aggregate dependency graph, fix the aggregate/report contract before changing product code.
6. Cut harness/control-plane debt before more product tuning when it blocks reliable progress: every verifier-consumed side report must be aggregate-owned and appear in a parent manifest-backed `report_dependency_graph`; native-preview source replay reports must stay PlanExecutor-only unless a legacy parity gate explicitly asks for compare; native report dependencies such as physical parity consuming preview E2E must be manifest-owned `consumes-native-report` edges; queue reports must stay compact, schema-valid, dependency-ordered, and deduplicated by report label; PlanExecutor product reports must use top-level `status` for product acceptance and `plan_executor_status` for executor provenance while omitting obsolete legacy comparison/product-acceptance side fields; native consumers must reject missing executor status; the refresh queue has `--until-clean --max-runs N` / `--closed-loop` mode that reruns the owner aggregate after each cycle, reports final stop reason plus selected-label burndown, orders upstream dependency refreshes before native consumers, records ordered execution-plan metadata, and prebuilds `boon_cli` before non-dry replay refresh execution; native `xtask` reports now use scoped `verifier_identity` so matching verifier contracts can supersede legacy binary-hash churn; and broad aggregates should move toward scoped input fingerprints plus summary/content-addressed sidecars instead of repeatedly parsing huge child JSON just to classify stale evidence.
   The PlanExecutor source replay scoped freshness/identity cut is now implemented for BYTES-owned `boon_cli` replay reports, and public CLI legacy replay/compare commands are retired rather than exposed through `run --engine compare`, product `--compare-legacy`, or a diagnostic compare command. The `run --engine` switch itself has now been removed from `boon_cli`; default `boon_cli run` is the product PlanExecutor path, and aggregate replay argv must not reintroduce `--engine plan`. The native aggregate also classifies child-reported stale consumed evidence as refresh debt rather than a true product blocker. The next control-plane cut is to execute the dependency-aware queue on the remaining native handoff refresh debt and only debug native/runtime/product code from fresh true blockers. If the aggregate remains too expensive after stale children are refreshed once, move toward compact summary/content-addressed sidecars instead of repeatedly parsing huge child JSON just to classify stale evidence.
7. Continue ProductFrameGraph from the current typed linear renderer-owned slice with retained resource state into actual dirty-resource scheduling with proof/readback as post-present subscribers keyed by frame evidence. Preserve `retained_product_frame_graph_linear_v1` evidence and retained epoch/dirty/reuse counters while deepening it; do not fall back to stringly playground-side topology.
8. Remove old paths only when replacements are proven: normal `LoadedRuntime` / `GenericScheduledRuntime` use, AST-derived runtime planning, string-prefix output detection, `legacy_render_frame_metrics` product fallback, old Ply vocabulary in readiness, and verifier sidecars that make huge proof payloads the canonical signal.
9. Do not introduce Cells/example-specific compiler, runtime, renderer, or verifier hacks. If Cells regresses, classify the blocker first as frame scheduling, retained scene/render graph, proof subscriber, IPC, or runtime currentness/list dependency.

Clear end condition:
- current HEAD has fresh schema-valid native GPU manifest reports and `verify-native-gpu-all --check-existing` passes, or every remaining native failure is a fresh true blocker with code-level root cause and an implemented fix attempt in the working tree unless the user explicitly asked for a commit;
- BYTES/MachinePlan/default-engine/readiness reports are fresh and no longer contradict current CLI/runtime behavior;
- normal runtime clients use PlanExecutor provenance, with no hidden legacy fallback in default/native paths;
- `cargo test -p boon_runtime --lib --quiet` remains green, and any new default-engine blockers are fixed or explicitly classified with code-level root causes and non-optional follow-up blockers;
- focused equivalence and source-free artifact tests pass;
- ProductRenderGraph/ProductFrameGraph is either a real retained renderer-owned graph kept with fresh no-regression evidence, or any failed attempt is quarantined with measured evidence and a non-optional follow-up blocker recorded; the current typed linear graph is progress but not final completion;
- docs/plans/GOAL_PROMPT.md and progress ledgers are updated to match fresh evidence;
- no stale reports, human observation, desktop screenshots, Ply/COSMIC scraping, example-name shortcuts, or proof/readback coupling are used to claim success.

Before marking the goal complete, use subagents for an honest verification review against the stop condition. Do not commit or push unless explicitly asked.
```
