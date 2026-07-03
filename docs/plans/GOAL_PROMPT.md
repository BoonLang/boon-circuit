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

Short slash command:

```text
/goal follow docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md from the current HEAD. First inspect current git status, recent commits, readiness reports, and native GPU reports. Treat commit 8117094 and target/reports/native-gpu/cells-visible-click-e2e-release.json as latest known evidence that headed Cells visible-click passed on hardware, but verify freshness before relying on it.

Continue remaining BYTES/MachinePlan/default-engine readiness work, but do not leave the native WGPU render graph as optional backlog. Implement and measure a generic ProductRenderGraph / PresentPlan slice for native product frames: ActivePreviewScene + ProductPatch must compile into explicit product passes, while proof/readback/reporting stay post-present subscribers keyed by FrameEvidenceKey.

Measure before and after on Cells and TodoMVC. Keep the render graph if it improves performance or removes/quarantines legacy product hot-path coupling without budget regressions. If it worsens performance and does not simplify code, revert or quarantine it and record an evidence-based ADR/progress entry. Do not add Cells/example-specific hacks anywhere in compiler, runtime, document, renderer, app-window, playground, or verifier code.

Mark the goal achieved only when one of these is true: (1) ProductRenderGraph / PresentPlan is implemented and kept, with fresh schema-valid before/after Cells and TodoMVC reports proving required budgets still pass, proof/readback/reporting stay post-present, no product-frame proof/readback coupling exists, and the render graph either improves performance or removes/quarantines legacy hot-path coupling without regression; or (2) ProductRenderGraph / PresentPlan was implemented, measured, found worse or not simplifying, then reverted or quarantined with an ADR/progress entry and fresh schema-valid reports proving the old path still passes. Do not mark the goal achieved for an honest blocker handoff; a blocker handoff may end the current work turn, but the goal remains active unless the strict blocked-audit rule is satisfied. Do not commit or push unless explicitly asked.
```
