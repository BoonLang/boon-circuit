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

- BYTES/MachinePlan aggregate evidence is fresh and passing at `63/63`
  required reports after the report-schema source-derived replay fix.
- Default-engine readiness is fresh and passing with `default_engine=plan` and
  `default_switch_allowed=true`.
- Recursive `verify-report-schema` is fresh and passing after quarantining two
  stale ad hoc target reports outside `target/reports`.
- `audit-machine-readiness` and `audit-goal-readiness` still fail honestly on
  stale runtime production/finality, stale Cells release benchmark, and
  native/TodoMVC handoff blockers.
- The next implementation work should refresh/fix those readiness blockers and
  then promote the renderer-owned retained `ProductFrameGraph`; do not restart
  BYTES schema replay or Cells micro-optimization unless fresh evidence
  contradicts this checkpoint.

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
3. Add equivalence coverage proving `run_plan_scenario_events`, `PlanExecutorLiveSession`, and `LiveRuntime::apply_source_batch_turn` match. TodoMVC explicit PlanExecutor batch equivalence and focused TodoMVC/Cells document-summary coverage exist; runtime dependency semantics are now sharper for exact `List/find`, list-structure reads, stable source-identity patching, projected source bindings, and thread-local BYTES counters. Continue with Cells source-event equivalence, BYTES, indexed source payloads, demand-current/list-find cases, and source-free artifact loading.
4. Refresh current evidence after code cuts. The current native handoff blocker is TodoMVC preview E2E/report debt and stale manifest children, not another Cells micro-optimization: resolve the evidence-tier mismatch against `NATIVE_GPU_PIPELINE.md`, add/repair TodoMVC runtime assertions, complete scenario/visible-reality coverage, and rerun the focused TodoMVC report before the manifest aggregate. Rerun BYTES/MachinePlan/default-engine/readiness reports, then the native GPU manifest gates and aggregate on the current commit. If reports fail only because they are stale or schema-sidecar issues, fix the verifier/report contract instead of changing product code.
5. Promote ProductRenderGraph from a reporting wrapper into a renderer-owned retained ProductFrameGraph with typed resources/passes, stable plan identity separate from execution metrics, and proof/readback as post-present subscribers keyed by frame evidence.
6. Remove or quarantine old paths only when replacements are proven: normal `LoadedRuntime` / `GenericScheduledRuntime` use, AST-derived runtime planning, legacy_scene/string-prefix output detection, `legacy_render_frame_metrics` product fallback, old Ply vocabulary in readiness, and verifier sidecars that make huge proof payloads the canonical signal.
7. Do not introduce Cells/example-specific compiler, runtime, renderer, or verifier hacks. If Cells regresses, classify the blocker first as frame scheduling, retained scene/render graph, proof subscriber, IPC, or runtime currentness/list dependency.

Clear end condition:
- current HEAD has fresh schema-valid native GPU manifest reports and `verify-native-gpu-all --check-existing` passes, or every remaining native failure is a fresh true blocker with code-level root cause and an implemented fix attempt in the working tree unless the user explicitly asked for a commit;
- BYTES/MachinePlan/default-engine/readiness reports are fresh and no longer contradict current CLI/runtime behavior;
- normal runtime clients use PlanExecutor provenance, with no hidden legacy fallback in default/native paths;
- `cargo test -p boon_runtime --lib --quiet` remains green, and any new default-engine blockers are fixed or explicitly classified with code-level root causes and non-optional follow-up blockers;
- focused equivalence and source-free artifact tests pass;
- ProductRenderGraph is either a real retained renderer-owned graph kept with fresh no-regression evidence, or any failed attempt is quarantined with measured evidence and a non-optional follow-up blocker recorded;
- docs/plans/GOAL_PROMPT.md and progress ledgers are updated to match fresh evidence;
- no stale reports, human observation, desktop screenshots, Ply/COSMIC scraping, example-name shortcuts, or proof/readback coupling are used to claim success.

Before marking the goal complete, use subagents for an honest verification review against the stop condition. Do not commit or push unless explicitly asked.
```
