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
  intentionally skipped. The Cells
  compare refresh queue preserves the real `boon_cli run examples/cells.bn
  --scenario examples/cells.scn --engine compare` shape.
- Hidden native-preview source replay side reports have been pulled into the
  BYTES aggregate. TodoMVC, Cells, and physical TodoMVC native-preview replay
  children refresh with canonical `boon_cli run ... --engine plan` commands and
  are exposed through native `report_dependency_graph` edges when preview E2E
  consumes them. After local code changes, the native aggregate can emit
  upstream refresh commands for these reports directly instead of hiding the
  dependency inside preview E2E. The handoff manifest now owns those
  `upstream_dependencies`, so `verify-native-gpu-all` no longer relies on a
  separate hardcoded native-preview replay dependency table.
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
  PlanExecutor source replay identity cut it reports `18` refresh-debt
  children, `18` identity-fast refresh children, `0` fresh product-contract
  children, `0` refresh-first product-contract children, `0` upstream
  dependency refresh-debt reports, and `0` upstream true blockers.
  This is refresh/control-plane debt, not fresh Cells product evidence. The
  aggregate now emits
  manifest-canonical `refresh_argv`, diagnostic `observed_argv`, and
  `report_dependency_graph` upstream replay edges, so run queue-filtered
  refreshes before broad handoff debugging.
- The native aggregate now fast-paths stale child identity. If a child report has
  stale git, worktree, or binary identity, `verify-native-gpu-all` records one
  `identity-freshness-fast-path` refresh-debt item and skips schema, semantic,
  and artifact validation until the report is regenerated. This avoids treating
  stale megabyte reports as fresh product failures.
- The next implementation work should refresh/fix readiness blockers with the
  aggregate refresh plan first, then promote the renderer-owned retained
  `ProductFrameGraph`; do not restart Cells micro-optimization unless fresh
  product-latency evidence regresses.
- Closed-loop refresh has now burned down the three BYTES-owned native-preview
  source replay labels and the first eight native contract labels. Native
  handoff still has remaining refresh debt for heavier product/window labels.
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
- PlanExecutor command reports now split `plan_executor_status`,
  `comparison_status`, and `accepted_for_product_status` from the compatibility
  top-level `status`. Native source replay can use product acceptance without
  confusing intentionally disabled or currentness-accepted legacy comparison
  with product execution failure. Native replay consumers now reject missing
  split fields, so stale pre-split reports are refresh debt rather than usable
  product evidence.
- Native reports now carry scoped worktree fingerprints. The native aggregate
  may use the `native-gpu-handoff` scoped fingerprint for child freshness while
  still requiring current git and binary identity. The scope includes product
  and verifier inputs and excludes progress/goal prose, so docs-only plan churn
  stops invalidating otherwise current native product reports.
- Native handoff report dependencies now come from
  `docs/architecture/native_gpu_handoff_manifest.json`, including the three
  BYTES-owned native-preview source replay reports consumed by preview E2E.
  Aggregate output includes those edges in `report_dependency_graph` and
  `required_reports[].upstream_dependencies`.
- `verify-report-schema` now has command-specific validation for
  `verify-native-gpu-all`: native handoff aggregate reports must match the
  canonical manifest, expose manifest-owned dependency edges, keep refresh
  commands bounded and replayable, and keep refresh/product/dependency taxonomy
  counts consistent. A failing native aggregate can still be schema-valid when
  it honestly reports refresh debt or fresh blockers.
- `run-report-refresh-queue` is now dependency-aware and schema-locked. Full
  native refresh dry-runs select the three upstream BYTES source replay reports
  first; a label-filtered `preview-e2e-cells` dry-run expands to
  `cells-native-preview-source-replay` before `preview-e2e-cells`. Queue reports
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
  `source_replay_identity`. The three native preview source replay reports for
  TodoMVC, Cells, and physical TodoMVC have been regenerated and schema-checked.
  The latest native aggregate reports all three upstream dependencies as
  `schema_valid=true`, `worktree_fresh=true`,
  `worktree_fingerprint_basis=scoped`,
  `source_replay_identity_present=true`,
  `source_replay_identity_fresh=true`, `freshness_debt=false`, and
  `true_blocker=false`.
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
- `boon_cli run --engine plan|compare --report ...` is quiet by default now.
  Use `--print-report` only for intentional stdout JSON inspection; normal
  refreshes should rely on report files and compact `jq` summaries.
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
- Native handoff report dependencies now support both
  `consumes-source-replay-report` and `consumes-native-report`. The manifest
  models `todomvc-physical-reference-parity -> preview-e2e-todo_mvc_physical`,
  `verify-native-gpu-all` emits that edge with owner `verify-native-gpu-all`,
  and `run-report-refresh-queue` expands label-filtered refreshes through the
  graph while deduplicating duplicate stale labels. The current parity dry-run
  is schema-valid and selects exactly three reports in dependency order:
  `todo-mvc-physical-native-preview-source-replay`,
  `preview-e2e-todo_mvc_physical`, then
  `todomvc-physical-reference-parity`.

Short slash command:

```text
/goal Complete the next unified architecture slice for /home/martinkavik/repos/boon-circuit without getting stuck in Cells micro-optimizations.

Start by inspecting current HEAD, git status, AGENTS.md, docs/architecture/NATIVE_GPU_PIPELINE.md, docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md, docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md, docs/plans/GOAL_PROMPT.md, and current target reports. Treat stale reports as non-proof.

Use subagents before implementation:
- one for native GPU/report freshness and verifier debt,
- one for PlanExecutor/LiveRuntime unification,
- one for retained ProductRenderGraph/resource graph architecture,
- optionally one for negative/legacy-path audit.
Fold their results into one implementation plan before editing.

Implementation priorities:
1. Use the current persistent PlanExecutor live-session checkpoint instead of restarting from whole-scenario command helpers. `PlanExecutorRuntimeState`, `PlanExecutorLiveSession`, explicit `LiveRuntime::{from_source_plan_executor,from_project_plan_executor}` mode, PlanExecutor-backed batch/single-event/scenario-helper live turns, PlanExecutor source inventory, PlanExecutor targeted value/query summaries, and PlanExecutor document/window summaries exist; extend that mode rather than creating another parallel runtime surface.
2. Make normal runtime execution PlanExecutor-backed. `boon_cli run`, default document `LiveRuntime::{from_source,from_project,from_project_profiled,new,new_from_project}`, explicit PlanExecutor `LiveRuntime`, and native document preview paths must report the same PlanExecutor engine provenance. The explicit `LiveRuntimeEngine::{Legacy, PlanExecutor}` boundary exists, test-only legacy inspections and old legacy-runtime unit coverage must go through explicit legacy accessors/constructors, native document-summary/layout-proof helpers use profiled PlanExecutor project construction, native preview now selects PlanExecutor for document-only projects while keeping `world:` / `manufacturing:` output roots on explicit legacy diagnostics until PlanExecutor supports those outputs, and operator-host ACK/native E2E schema now require document input provenance `engine=plan_executor` with `generic_fallback_enabled=false`. The current runtime slice has `cargo test -p boon_runtime --lib --quiet` passing (`358` tests, `0` failed); preserve that while refreshing default-engine/readiness reports and removing any remaining hidden normal-path legacy fallback. Keep legacy execution only behind explicit diagnostic/compare/output-root gates, and do not silently fall back from PlanExecutor mode to `LoadedRuntime` / `GenericScheduledRuntime`.
3. Add equivalence coverage proving `run_plan_scenario_events`, `PlanExecutorLiveSession`, `LiveRuntime::apply_source_batch_turn`, and artifact-loaded PlanExecutor match. TodoMVC explicit PlanExecutor batch equivalence, representative artifact-loaded PlanExecutor parity for TodoMVC/Cells/indexed BYTES, representative live/session/batch parity for TodoMVC/Cells/root BYTES/indexed BYTES, a source-deleted TodoMVC artifact scenario, and focused TodoMVC/Cells document-summary coverage exist; runtime dependency semantics are now sharper for exact `List/find`, list-structure reads, stable source-identity patching, projected source bindings, and thread-local BYTES counters. Continue with dedicated demand-current/list-find edge cases, explicit legacy diagnostic quarantine/deletion, and render-patch ownership by PlanExecutor/render graph where the surface is executable.
4. Refresh current evidence after code cuts. Start from aggregate-provided structured `refresh_commands[].argv` and the `run-report-refresh-queue --until-clean --max-runs N` runner (`--closed-loop` is an alias; `--rerun-aggregate` is one cycle only): focus-safe public present-floor, release/hardware preview E2E TodoMVC, release/hardware preview E2E Cells, release/hardware preview E2E physical TodoMVC, then `verify-native-gpu-all --check-existing`. Rerun BYTES/MachinePlan/default-engine/readiness reports from their aggregate refresh queue. If reports fail only because they are stale or schema-sidecar issues, fix the verifier/report contract instead of changing product code.
5. Treat the verifier/control plane as architecture, not bookkeeping. Use `refresh_commands[].argv`, `refresh_debt_child_count`, `identity_fast_refresh_child_count`, `true_blocker_children`, `product_contract_children`, `refresh_first_product_contract_children`, `report_dependency_graph`, `required_reports[].upstream_dependencies`, and scoped freshness fields from aggregate reports to decide the next command; do not dump megabyte child reports into the conversation and do not spend time proving stale reports are stale. If an aggregate cannot tell refresh debt from true blockers, cannot replay child commands exactly, or has a verifier-consumed side report hidden outside a manifest/aggregate dependency graph, fix the aggregate/report contract before changing product code.
6. Cut harness/control-plane debt before more product tuning when it blocks reliable progress: every verifier-consumed side report must be aggregate-owned and appear in a parent manifest-backed `report_dependency_graph`; native-preview source replay reports must stay PlanExecutor-only unless a legacy parity gate explicitly asks for compare; native report dependencies such as physical parity consuming preview E2E must be manifest-owned `consumes-native-report` edges; queue reports must stay compact, schema-valid, dependency-ordered, and deduplicated by report label; PlanExecutor reports must keep `plan_executor_status`, `comparison_status`, and `accepted_for_product_status` distinct and native consumers must reject missing split fields; the refresh queue has `--until-clean --max-runs N` / `--closed-loop` mode that reruns the owner aggregate after each cycle, reports final stop reason plus selected-label burndown, orders upstream dependency refreshes before native consumers, records ordered execution-plan metadata, and prebuilds `boon_cli` before non-dry replay refresh execution; native `xtask` reports now use scoped `verifier_identity` so matching verifier contracts can supersede legacy binary-hash churn; and broad aggregates should move toward scoped input fingerprints plus summary/content-addressed sidecars instead of repeatedly parsing huge child JSON just to classify stale evidence.
   The PlanExecutor source replay scoped freshness/identity cut is now implemented for BYTES-owned `boon_cli` replay reports, and explicit `boon_cli run --engine plan|compare --report ...` is quiet unless `--print-report` is requested. The native aggregate also classifies child-reported stale consumed evidence as refresh debt rather than a true product blocker. The next control-plane cut is to execute the dependency-aware queue on the remaining native handoff refresh debt and only debug native/runtime/product code from fresh true blockers. If the aggregate remains too expensive after stale children are refreshed once, move toward compact summary/content-addressed sidecars instead of repeatedly parsing huge child JSON just to classify stale evidence.
7. Continue ProductFrameGraph from the current typed linear renderer-owned slice with retained resource state into actual dirty-resource scheduling with proof/readback as post-present subscribers keyed by frame evidence. Preserve `retained_product_frame_graph_linear_v1` evidence and retained epoch/dirty/reuse counters while deepening it; do not fall back to stringly playground-side topology.
8. Remove or quarantine old paths only when replacements are proven: normal `LoadedRuntime` / `GenericScheduledRuntime` use, AST-derived runtime planning, legacy_scene/string-prefix output detection, `legacy_render_frame_metrics` product fallback, old Ply vocabulary in readiness, and verifier sidecars that make huge proof payloads the canonical signal.
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
