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
/goal Follow docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md until the entire plan is implemented and honestly verified. Performance is the main goal. Start from the current 2026-07-02 checkpoints, not old reports: product Cells clicks are measured from the accepted-input product commit with typed product patch/result evidence; required post-present proof subscribers are enqueued before pending-input yield; Readback proof mode can produce a WGPU visible-surface readback artifact for the exact product `FrameEvidenceKey`; and the verifier now prefers exact product-frame artifacts over later recent-frame proof. The latest one-click llvmpipe diagnostic has exact product/proof identity and proof-only pass, with about 12.2 ms accepted-input-to-formula visibility, but the top-level gate still fails because the isolated Weston product surface selected software Vulkan llvmpipe instead of hardware-backed evidence. A subagent confirmed the app-window path is not forcing software: it requests `HighPerformance`, `force_fallback_adapter: false`, and `compatible_surface: Some(surface)`; likely WGPU only sees llvmpipe as compatible with the headless Weston surface. First make hardware adapter policy honest and generic: add a product-verifier hardware-required adapter policy/fail-fast path with full adapter/request/environment evidence, keep software runs diagnostic-only, and do not let llvmpipe timing satisfy product UX gates. Then continue by cutting the actual product architecture instead of looping on micro-optimizations: make `PreviewHotLoop` / `NativeFrameClock` / `ActivePreviewScene` the generic product-frame owner, sample input at the start of an already scheduled demand-driven burst frame, patch retained selection/focus/formula-bar state directly, submit the product frame quickly, and keep proof/readback/reporting/dev IPC/accessibility/HUD work behind bounded post-present services keyed by exact `FrameEvidenceKey`. Product UX may use only exact product commits for the measured product-present frame; later-frame proof is proof lag only, never product UX. Do not add Cells/example-specific hacks anywhere in compiler, runtime, document, renderer, app-window, playground, or verifier code. Use subagents for independent architecture/runtime/WGPU/testing/external-library review whenever useful, and prefer larger architecture cuts over another loop of tiny tactical patches. If flaky tooling, hardware-adapter selection, or verifier design is slowing progress, fix it too. Do not claim completion until fresh hardware-backed multi-sample native UX, exact proof identity, perf-HUD, generic runtime, no-hacks, stale-proof negative, and schema gates pass for the current worktree and binary; if blocked, leave the repo coherent and report the exact blocker, evidence, and next implementation step.
```
