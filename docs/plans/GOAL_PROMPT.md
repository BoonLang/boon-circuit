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
/goal Execute the Cells-first native performance slice from docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md. Use the long backlog as steering, not mandatory scope. Start from the 2026-07-03 post-restart checkpoint: COSMIC background-launch works and the headed A2 Cells smoke proves hardware, app-owned host input, WGPU readback, and formula-bar functionality, but it is not 60 FPS acceptance.

First make `verify-native-cells-visible-click-e2e --profile release --headed-host-input` the canonical full Cells gate: remove any smoke-only headed shortcut, keep strict hardware adapter policy, use app-owned host events and WGPU readback, preserve layout-derived target selection, require exact product commits, typed active-scene product patch/result evidence, exact or explicitly lagged `FrameEvidenceKey` proof, multi-sample release coverage, p95 <= 16.7ms, bounded max <= 33.4ms, zero missed product frames, and schema-valid reports. Do not accept A2 one-click debug smoke, isolated Weston llvmpipe, human observation, COSMIC scraping, desktop screenshots, xdotool/ydotool, browser/Ply/Xvfb paths, direct SourceBatch injection, or software-adapter product evidence.

If the full headed gate fails, fix only the failing lane. Product pass plus proof fail means work the proof registry/subscriber/verifier join, not product timing. Hardware fail means fix launcher/adapter evidence, not Cells. Queue/present dominance means add same-surface hardware present-floor evidence before touching runtime/layout. Clean runtime/list/formula counters mean no formula micro-tuning.

Hard loop stop: after two fresh reports from the same gate show the same dominant blocker class, stop local optimizations. Record report paths, blocker class, rejected tactic, selected architecture boundary, old path to delete/quarantine, and the next gate that will prove it.

If product latency remains the blocker, implement the generic hot-loop architecture cut: make `PreviewHotLoop` / `NativeFrameClock` / `ActivePreviewScene` the product-frame owner; sample input at the start of an already scheduled demand-driven burst frame; patch retained selection/focus/formula-bar state directly; submit quickly; move proof/readback/reporting/accessibility/HUD/dev IPC behind bounded post-present services keyed by exact `FrameEvidenceKey`.

Do not add Cells/example-specific hacks anywhere in compiler, runtime, document, renderer, app-window, playground, or verifier code. Use subagents only for independent reads that validate the selected architecture cut or blocker classification. End the goal when the Cells-first gates pass on fresh hardware-backed reports for the current worktree/binary, or when a fresh schema-valid report identifies the next blocker and the repo is coherent.
```
