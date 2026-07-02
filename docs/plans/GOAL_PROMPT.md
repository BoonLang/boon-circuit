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
/goal Follow docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md until the entire plan is implemented and honestly verified. Performance is the main goal. Start from the current 2026-07-02 checkpoints, not old reports: product Cells clicks are now measured from the accepted-input product commit with typed product patch/result evidence, and the app-window WIP now enqueues required post-present proof subscribers before pending-input yield and can produce a WGPU visible-surface readback artifact for the exact product `FrameEvidenceKey` in Readback proof mode. The latest one-click llvmpipe diagnostic still failed because the verifier/proof lane selected a later frame/input proof even though exact product-frame artifacts existed, and because llvmpipe is software-only. First fix the generic proof identity join: accepted input -> product commit -> requested proof key -> completed app-owned WGPU proof must join by exact `FrameEvidenceKey`, with no latest-report, recent-frame, proof-frame, nearby-input, or proof-lag fallback counted as product UX. Then cut the architecture instead of looping on micro-optimizations: make `PreviewHotLoop` / `NativeFrameClock` / `ActivePreviewScene` the generic product-frame owner, sample input at the start of an already scheduled demand-driven burst frame, patch retained selection/focus/formula-bar state directly, submit the product frame quickly, and keep proof/readback/reporting/dev IPC/accessibility/HUD work behind bounded post-present services. Do not add Cells/example-specific hacks anywhere in compiler, runtime, document, renderer, app-window, playground, or verifier code. Use subagents for independent architecture/runtime/WGPU/testing/external-library review whenever useful, and prefer larger architecture cuts over another loop of tiny tactical patches. If flaky tooling or verifier design is slowing progress, fix it too. Do not claim completion until fresh hardware-backed multi-sample native UX, exact proof identity, perf-HUD, generic runtime, no-hacks, stale-proof negative, and schema gates pass for the current worktree and binary; if blocked, leave the repo coherent and report the exact blocker, evidence, and next implementation step.
```
