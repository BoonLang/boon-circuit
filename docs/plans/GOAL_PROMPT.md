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
/goal Follow docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md until the entire plan is implemented and honestly verified. Performance is the main goal. Start from the current 2026-07-02 checkpoints, not old reports: the visible-click verifier now ties product latency to the accepted-input product commit instead of the later release/follow-up frame; the latest one-click release diagnostic on llvmpipe had exact product commit matching, typed product patch/result evidence, zero product fallbacks, zero missed frames, and about 10-11 ms accepted-input-to-formula visibility, but it still failed because llvmpipe is software-only and the proof lane still proved a later frame/input event with about 3 frames of proof lag. Cut the architecture instead of looping on micro-optimizations: make `PreviewHotLoop` / `NativeFrameClock` / `ActivePreviewScene` the generic product-frame owner, sample input at the start of an already scheduled demand-driven burst frame, patch retained selection/focus/formula-bar state directly, submit the product frame quickly, and move proof/readback/reporting/dev IPC/accessibility/HUD work behind bounded post-present `FrameEvidenceKey`-keyed services. The next proof cut must preserve exact identity: accepted input -> product commit -> requested proof key -> completed app-owned WGPU proof, with no latest-report, proof-frame, or nearby-input fallback counted as product UX. Do not add Cells/example-specific hacks anywhere in compiler, runtime, document, renderer, app-window, playground, or verifier code. Use subagents for independent architecture/runtime/WGPU/testing/external-library review whenever useful, and prefer larger architecture cuts over another loop of tiny tactical patches. If flaky tooling or verifier design is slowing progress, fix it too. Do not claim completion until fresh hardware-backed multi-sample native UX, exact proof identity, perf-HUD, generic runtime, no-hacks, stale-proof negative, and schema gates pass for the current worktree and binary; if blocked, leave the repo coherent and report the exact blocker, evidence, and next implementation step.
```
