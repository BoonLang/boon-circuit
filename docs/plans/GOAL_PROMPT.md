# `/goal` Prompt

Use this prompt for the next unattended unified implementation pass. It is a
compact entrypoint; use the architecture docs and manifest-owned reports as the
source of truth.

## Current Contracts

- Native GPU contract: `docs/architecture/NATIVE_GPU_PIPELINE.md`
- Unified architecture summary:
  `docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md`
- Native performance/render-graph plan:
  `docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md`
- Native handoff manifest:
  `docs/architecture/native_gpu_handoff_manifest.json`

## Current Checkpoint

- Normal runtime/product execution is PlanExecutor-backed.
- Current source inspection shows no pre-PlanExecutor runtime implementation
  references in `crates/boon_runtime`.
  diagnostic compare aliases.
- Native preview E2E and scroll evidence no longer accept the top-level
  `preview_native_gpu_render_proof` alias. Surface-scoped proof under
  `preview_surface_proof` plus app-owned `native_gpu_render_proof` is the active
  proof shape.
- Manifest scenario handling separates semantic `input_scenarios` from native
  preview acceptance. Full semantic input coverage may be delegated; native
  preview status is driven by native preview, initial visible, and scroll-focus
  evidence.
- `ProductFrameGraph` exists as a typed linear retained graph with retained
  resource-state evidence. It is progress, not the final dirty-resource
  scheduler.

## Prompt

```text
/goal Continue the Boon Circuit architecture cleanup and performance goal from the current HEAD.

First inspect:
- git status and recent commits;
- AGENTS.md;
- docs/architecture/NATIVE_GPU_PIPELINE.md;
- docs/plans/GOAL_PROMPT.md;
- current native/BYTES aggregate reports, using compact summaries only.

Treat stale reports as non-proof. Do not use human observation, desktop
screenshots, browser/Ply/Xvfb/COSMIC scraping, example-name shortcuts, or
proof/readback latency as product UX proof.

Use subagents before deep cuts:
- one for native GPU/report freshness and verifier debt;
- one for PlanExecutor/LiveRuntime/default runtime authority;
- one for ProductFrameGraph/retained renderer architecture;

Implementation priorities:

1. Keep normal execution PlanExecutor-backed. Do not recreate pre-PlanExecutor
   or hidden fallback paths. If old tests need them, delete or replace the tests
   with PlanExecutor/product coverage.

2. Cut verifier/control-plane ambiguity before product tuning when it blocks
   reliable evidence. Every verifier-consumed side report must be manifest-owned
   or aggregate-owned, appear in a report dependency graph, and have bounded
   refresh commands. Aggregate output must distinguish refresh debt from fresh
   product blockers without dumping giant child reports.

3. Continue native proof cleanup. Product UX latency, proof/readback latency,
   report generation, and dev-window telemetry are separate lanes linked by
   frame identity. Surface-scoped proof is canonical; do not reintroduce
   top-level proof aliases as acceptance candidates.

4. Move ProductFrameGraph from the current linear retained graph toward a real
   renderer-owned dirty-resource scheduler. Keep retained GPU resources hot,
   keep proof/readback as post-present subscribers, and report dirty/reused
   resources, upload bytes, draw calls, and proof lag separately.

5. Keep Cells generic. Fix runtime/list/currentness/render architecture, not
   Cells-specific compiler/runtime/renderer branches. If Cells regresses,
   classify the blocker first as input scheduling, retained state publication,
   ProductFrameGraph/resource scheduling, proof-lane backpressure, IPC, or
   runtime currentness/list dependency.

6. Refresh evidence only after coherent code cuts. Use manifest-provided
   refresh argv and `run-report-refresh-queue --until-clean --max-runs N` where
   useful. Use compact `jq` summaries, not whole JSON dumps.

Clear stop condition before marking complete:
- worktree clean after commits;
- `cargo fmt --check` and `git diff --check` pass;
- relevant crate checks/tests pass for touched areas;
- regenerated native GPU report schemas pass;
- manifest-backed native handoff aggregate passes, or every remaining failure
  is a fresh true blocker with code-level root cause and no hidden fallback;
- Cells product interaction is measured with fresh native evidence and product
  UX latency is separate from proof/readback latency;
- docs/plans/GOAL_PROMPT.md and current architecture docs match current evidence;
- subagents have reviewed the completion claim before marking the goal done.
```

## Next Suggested Cut

Run the current aggregate summaries and decide from fresh taxonomy:

```bash
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
cargo xtask verify-report-schema target/reports/native-gpu-all.json
```

If the aggregate reports only refresh debt, use its refresh queue instead of
editing product code. If it reports fresh true blockers, fix the blocker class
directly and avoid local micro-optimization loops.
