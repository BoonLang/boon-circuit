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
/goal Follow docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md until the entire plan is implemented and honestly verified. Performance is the main goal. Start from the current 2026-07-02 checkpoints, not old reports: product Cells clicks are measured from the accepted-input product commit with typed product patch/result evidence; required post-present proof subscribers are enqueued before pending-input yield; Readback proof mode can produce a WGPU visible-surface readback artifact for the exact product `FrameEvidenceKey`; the verifier prefers exact product-frame artifacts over later recent-frame proof; and the product-verifier adapter policy is now explicit. Default Cells visible-click runs require `require_hardware_product` and fail fast when isolated Weston selects software Vulkan llvmpipe, with full adapter/request/environment evidence and no fake `inf` interaction blockers. `--allow-software-adapter-diagnostic` still runs the llvmpipe diagnostic lane: the latest A2 sample selected A2, showed formula text `15`, and proof-only passed, but product acceptance still fails because the adapter is software. A direct NVIDIA ICD/Optimus environment probe did not produce a hardware product surface; it failed before first frame. Continue by solving the hardware-backed verifier surface honestly and generically, not by weakening budgets: add adapter inventory/candidate evidence, find or implement a hardware-compatible native verifier path that still uses app-owned host events and WGPU proof, and keep software-surface runs diagnostic-only. In parallel or immediately after, keep cutting the product architecture instead of looping on micro-optimizations: make `PreviewHotLoop` / `NativeFrameClock` / `ActivePreviewScene` the generic product-frame owner, sample input at the start of an already scheduled demand-driven burst frame, patch retained selection/focus/formula-bar state directly, submit the product frame quickly, and keep proof/readback/reporting/dev IPC/accessibility/HUD work behind bounded post-present services keyed by exact `FrameEvidenceKey`. Product UX may use only exact product commits for the measured product-present frame; later-frame proof is proof lag only, never product UX. Do not add Cells/example-specific hacks anywhere in compiler, runtime, document, renderer, app-window, playground, or verifier code. Use subagents for independent architecture/runtime/WGPU/testing/external-library review whenever useful, and prefer larger architecture cuts over another loop of tiny tactical patches. Do not claim completion until fresh hardware-backed multi-sample native UX, exact proof identity, perf-HUD, generic runtime, no-hacks, stale-proof negative, and schema gates pass for the current worktree and binary; if blocked, leave the repo coherent and report the exact blocker, evidence, and next implementation step.
```
