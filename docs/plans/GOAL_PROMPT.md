# `/goal` Prompt

Use this prompt for the next unattended native preview performance pass. The
long-form prompt and current evidence are embedded in
`docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md`.

Primary source of truth:

- `docs/architecture/NATIVE_GPU_PIPELINE.md` remains the native GPU contract.
- `docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md` is the active
  performance implementation plan.
- AGENTS.md instructions remain binding.

Short slash command:

```text
/goal Follow docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md until the entire plan is implemented and honestly verified. Performance is the main goal. Start from the current 2026-07-02 checkpoints, not old reports: hardware adapter evidence is now required, `NativeFrameClockPolicy` exists but must become the real product-frame owner, deferred product proof requests are limited to retained visual proof after present, and the latest one-click release smoke passed the exact product/proof sample on llvmpipe but still failed because software adapters are diagnostic only and aggregate preview-loop p95 was still over budget. Cut the architecture instead of looping on micro-optimizations: build the generic `PreviewHotLoop` / `NativeFrameClock` / `ActivePreviewScene` path, sample input at the start of an already scheduled demand-driven burst frame, patch retained selection/focus/formula-bar state directly, submit the product frame quickly, and move proof/readback/reporting/dev IPC/accessibility/HUD work behind post-present `FrameEvidenceKey`-keyed services. Do not add Cells/example-specific hacks anywhere in compiler, runtime, document, renderer, app-window, playground, or verifier code. Use subagents for independent architecture/runtime/WGPU/testing/external-library review whenever useful. If flaky tooling or verifier design is slowing progress, fix it too. Do not claim completion until fresh hardware-backed native UX, proof identity, perf-HUD, generic runtime, no-hacks, stale-proof negative, and schema gates pass for the current worktree and binary; if blocked, leave the repo coherent and report the exact blocker, evidence, and next implementation step.
```
