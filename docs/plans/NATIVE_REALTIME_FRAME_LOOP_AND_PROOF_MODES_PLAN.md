# Native Realtime Frame Loop, Proof Modes, And Performance HUD Plan

Status: in progress

Created: 2026-06-30

Compacted: 2026-07-06

This is a focused delta to
`docs/plans/NATIVE_DEMAND_DRIVEN_RENDER_LOOP_PLAN.md` and the native GPU
contract in `docs/architecture/NATIVE_GPU_PIPELINE.md`. The native GPU contract
is authoritative when this file and the architecture contract disagree.

Historical run logs were removed from this file on 2026-07-06. Use git history
for old evidence. Keep this file short and implementation-facing.

## Product Requirement

Native preview interaction is product UX, not just verifier throughput.
Selection, editing, scrolling, and formula-bar updates should run like a hot
native frame loop:

- sample input at the start of an already scheduled frame;
- patch retained runtime/document/layout/render state directly;
- submit product work quickly;
- run proof/readback/reporting on a bounded lane after product presentation;
- link proof artifacts to product frames with frame identity, but do not charge
  proof completion to normal UX latency.

`idle-wake` is only a CPU/battery and event-loop smoke gate. It proves the app
can sleep and wake correctly. It is not the 60 FPS acceptance test.

## Runtime Modes

Use two long-lived modes:

- `DemandDriven`: product default; idle when no visible work exists.
- `ContinuousProbe`: verifier/diagnostic only; not valid for normal UX latency
  gates.

Requested animation is a bounded pacing substate inside `DemandDriven`, not a
third product mode:

- enter after visible-changing input, scroll, caret, replay, or animation
  request;
- extend while visible work continues;
- exit after a quiet interval or a hard cap;
- never become unbounded continuous rendering.

## Instrumentation Modes

- Always-on counters: cheap atomics/snapshots for frame sequence, last
  input-to-present latency, dropped frames, queue depth, and proof lag.
- Trace: opt-in detailed spans and reports.
- Readback proof: opt-in or verifier-owned WGPU readback artifacts.

The dev-window HUD may read only cached scalar snapshots. It must not query the
runtime, parse JSON reports, block on IPC, or call proof/readback paths from a
render hook.

## Proof Contract

Product UX latency and proof latency are separate metrics. Proof is still
mandatory for native verifier evidence.

Every proof artifact that claims a visible product frame must carry enough
identity to link it to the presented frame:

- `frame_seq`
- `content_revision`
- `layout_revision`
- `render_scene_revision`
- `surface_id`
- `surface_epoch`
- `input_event_seq` when applicable
- `present_id`
- `proof_request_id`
- capture method and proof completion timestamp

Reports must state proof lag in frames. Stale first-frame proof reuse,
mismatched surface epoch, mismatched content revision, hash-only proof without
structured metadata, browser/Ply/Xvfb evidence, desktop screenshots, and COSMIC
scraping fail native UX gates.

## Cells Evidence Boundary

The headed Cells visible-click release gate previously passed on commit
`8117094 Pass headed Cells visible click gate` with hardware WGPU evidence in
`target/reports/native-gpu/cells-visible-click-e2e-release.json`. Treat that as
historical evidence only; verify freshness before relying on it.

The focused Cells visible-click slice is not the whole unified goal. Remaining
work includes:

- fresh native handoff reports after current cleanup;
- passive scroll and broader Cells semantic-delta parity;
- retained WGPU dirty-resource scheduling;
- PlanExecutor default-path cleanup;
- removal of stale verifier/report acceptance paths.

## Architecture Direction

Current target architecture:

- `PlanExecutor` is the product runtime authority.
- Runtime/list/currentness paths are generic; no Cells-specific shortcuts.
- `ProductFrameGraph` / `PresentPlan` owns native product-frame scheduling.
- Surface proofs are scoped under the relevant surface report.
- Top-level render-proof aliases are diagnostic only and must not satisfy gates
  that require surface/frame identity.
- Product path must not block on proof subscribers, JSON serialization, report
  refresh, readback queues, or dev-window IPC.

## Performance Acceptance

For visible interactions on release/hardware builds:

- product `input_to_present_ms` p95 <= 16.7ms;
- bounded max <= 33.4ms unless explicitly reported as a bounded external
  compositor/driver outlier;
- zero normal product missed frames for the accepted sample window;
- no full-grid recompute, relower, render-scene rebuild, or summary rebuild for
  normal select/edit/scroll;
- proof/readback p95 reported separately with frame identity and proof lag.

Slower COSMIC compositor or Linux driver behavior is allowed only as an
explicitly measured external outlier. It must not hide product-path stalls,
proof-lane backpressure, or verifier measurement bugs.

## Current Next Tasks

1. Delete duplicate native verifier proof/report acceptance paths, especially
   stale top-level preview proof dependencies.
2. Split semantic scenario coverage from native input proof coverage so native
   gates do not fail on unrelated full-manifest expectations.
3. Remove remaining legacy runtime fallback routes where `PlanExecutor` is the
   intended product authority.
4. Turn `ProductFrameGraph` from a linear/report projection into renderer-owned
   dirty-resource scheduling with retained GPU resources.
5. Refresh focused native reports only after the harness is lean enough that
   report failures classify fresh product blockers instead of stale proof debt.

## Verification Commands

Use the manifest-owned native handoff list from
`docs/architecture/native_gpu_handoff_manifest.json`. Do not maintain a second
handoff list here.

Core checks for this plan:

```bash
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
cargo xtask verify-report-schema target/reports/native-gpu-all.json
```

For focused Cells work, use the release/hardware visible-click verifier and keep
product latency separate from proof/readback latency. Do not accept human
observation or desktop screenshots as proof.

## Stop Conditions

Stop and reassess architecture instead of continuing micro-optimizations when:

- the same fresh blocker class repeats across two focused attempts;
- verifier proof latency dominates while product latency is within budget;
- stale report identity causes product-looking failures;
- a code path exists only to preserve old report shape or legacy runtime
  comparison behavior.

When stopping, document the blocker in the compact unified status file and cut
the obsolete path in the next slice.
