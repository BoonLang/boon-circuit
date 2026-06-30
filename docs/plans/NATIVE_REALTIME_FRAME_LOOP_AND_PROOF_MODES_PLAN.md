# Native Realtime Frame Loop, Proof Modes, And Performance HUD Plan

Status: in progress

Created: 2026-06-30

Source of truth: this plan is a focused delta to
`docs/plans/NATIVE_DEMAND_DRIVEN_RENDER_LOOP_PLAN.md` and the active native GPU
contract in `docs/architecture/NATIVE_GPU_PIPELINE.md`. It is not a replacement
for either document.

## Summary

Native preview performance must be treated as a product requirement. Normal
visible interaction should feel like a 60 FPS native app, while proof,
debugging, tracing, and readback remain honest but opt-in or separately
accounted for.

`idle-wake` means the app can sit idle without continuous rendering, then wake
after an input or source event and present correct pixels. It is a CPU/battery
and event-loop correctness smoke gate. It is not the main UX mode and must not
force every click, scroll, or text edit through a cold sleep-to-proof path.

The active-interaction path should use short scheduled realtime bursts, hot
renderer resources, sparse runtime/layout work, and retained render state. Proof
readback remains mandatory for verifier evidence, but proof latency is measured
separately from UX latency.

## Architecture Direction

- Keep long-lived render-loop mode small and explicit:
  - `DemandDriven`: product mode and idle-wake proof mode.
  - `ContinuousProbe`: diagnostics/verifier mode only.
- `RequestedAnimation` is not a long-lived process mode unless
  `docs/plans/NATIVE_DEMAND_DRIVEN_RENDER_LOOP_PLAN.md` and
  `docs/architecture/NATIVE_GPU_PIPELINE.md` are updated at the same time.
  Preferred implementation:

```rust
pub enum NativeRenderLoopMode {
    DemandDriven,
    ContinuousProbe,
}

pub enum NativeFramePacing {
    Idle,
    RequestedAnimationBurst {
        reason: RequestedAnimationReason,
        started_at: MonotonicNanos,
        quiet_after: MonotonicNanos,
        hard_stop_after: MonotonicNanos,
    },
}
```

- Keep proof as instrumentation, not frame scheduling:
  - hot-path product counters are always on and included in UX latency;
  - `Counters` means no optional instrumentation beyond product counters;
  - `Trace` and `Proof` are optional and must report their own overhead;
  - normal UX budgets exclude WGPU readback completion, JSON report writes,
    verbose trace emission, and proof serialization.
  - reports split `ux_latency`, `proof_latency`, and
    `instrumentation_overhead`.
- Define frame phases as a scheduler contract:
  - drain host input;
  - coalesce source, selection, scroll, and viewport intents;
  - apply bounded runtime/layout/render-scene work;
  - extract narrow render input;
  - prepare/upload/queue/encode/present;
  - run proof/readback only when instrumentation asks for it.
- Use the burst transition table below:

| Current state | Event | Next state |
| --- | --- | --- |
| `Idle` | visible-changing input, scroll, caret, replay, animation request | `RequestedAnimationBurst` |
| `Idle` | source/runtime/layout wake | render one frame, then `Idle` unless more work remains |
| `RequestedAnimationBurst` | continued input/animation | extend `quiet_after`, bounded by `hard_stop_after` |
| `RequestedAnimationBurst` | no dirty work and no animation request | `Idle` after quiet interval or quiet frames |
| any | verifier forced sample | render/proof sample without changing product mode |
| any | surface lost, resize, or scale change | mark `SurfaceChanged`, invalidate epoch caches, render when valid |
| any | `ContinuousProbe` CLI/verifier | `ContinuousProbe` until verifier stops it |

- Use these initial pacing defaults unless fresh measurements justify changing
  them:
  - `requested_animation_burst_min_frames = 2`;
  - `requested_animation_quiet_ms = 100`;
  - `requested_animation_hard_cap_ms = 1000`;
  - `requested_animation_max_pending_snapshots = 1`.
- Use active/pending frames:
  - the active frame keeps scrolling and selection overlays responsive;
  - pending runtime/layout/render snapshots swap only when complete and current;
  - stale pending snapshots are dropped by epoch and frame sequence.
- Apply active/pending backpressure:
  - at most one pending runtime/layout/render snapshot per role;
  - newer source/layout/surface epochs supersede older pending snapshots;
  - stale pending work is cancelled or dropped before expensive build work when
    possible;
  - a pending snapshot may commit only if source revision, layout revision,
    render scene revision, surface epoch, and frame sequence are still current;
  - while pending work is incomplete, scroll and selection may update the active
    retained frame through transform, clip, and overlay state only;
  - hit testing uses the active layout snapshot until the pending snapshot
    commits.
- Keep the visual pipeline staged:
  - source/runtime state;
  - document model;
  - layout fragments;
  - display-list primitives;
  - WGPU renderer.
- The renderer must consume stable identities from the document/layout/render
  pipeline. It must not rediscover identity from geometry, labels, strings, or
  example-specific knowledge.

## External Architecture Lessons

These are implementation inspirations, not dependency decisions.

- GPUI/Zed: use a hybrid immediate/retained model. Rebuild lightweight view
  values when useful, but retain layout/text/render caches keyed by stable
  document and render identities.
- GPUI/Zed: keep explicit layout, prepaint, and paint phases. In Boon terms,
  layout computes bounds, prepaint resolves hit/scroll regions, and paint
  lowers to render primitives.
- GPUI/Zed: optimize a small primitive vocabulary: quads, rounded boxes,
  shadows, glyph runs, images/surfaces, simple paths, layers, clips, and
  instance batches.
- GPUI/Zed: text shaping is first-class cached work. Key shaped runs by content
  hash, font, size, scale, style, wrap width, and writing mode inputs.
- Bevy: keep change detection cheap and explicit. Dirty/current state should be
  first-class on document, layout, text cache, render scene, and GPU buffers.
- Bevy: extraction into render-owned data should be narrow and measured:
  visible ranges, clipped display items, text/texture deltas, and telemetry IDs
  only.
- Bevy: use ordered render phases instead of ad hoc render branches: extract,
  prepare, queue, encode, present, proof.
- Servo and browser engines: keep script/runtime, layout, display list, and
  compositor/rendering separate. Scroll and selection should be compositor-like
  fast paths where possible.
- Browser engines: separate property trees such as transform, clip, scroll, and
  effects from visual items. Scrolling should update retained transform/clip
  state and small uniforms rather than rebuild display lists.
- WebRender/Chrome: render high-level display items into batches with culling,
  spatial/clip trees, and explicit upload/draw counts.
- Ply history: keep the idea of a real native playground with an editor, but do
  not copy legacy Ply proof, macroquad/miniquad coupling, Xvfb, desktop
  screenshots, global desktop input tools, or monolithic example-specific
  playground shortcuts.

## Dev Window Preview Performance Row

The first UI implementation is one compact dev-footer row, not a new panel:

```text
Preview perf  mode idle, last 8.4ms, render 1.3ms, age 0.9s, proof off, drops 0
Preview perf  mode burst, fps 59.8, p95 14.2ms, proof counters, drops 0
Preview perf  mode probe, fps 60.0, proof readback, proof p95 6.1ms, drops 0
```

In `DemandDriven` idle, FPS may legitimately be zero. The HUD must show mode,
last frame latency, sample age, proof mode, and drops so a healthy idle preview
does not look like a failed 0 FPS renderer.

Add a tiny `PreviewPerfStats` snapshot with fixed scalar fields:

- frame sequence;
- sample time;
- pacing state: idle, burst, or probe;
- FPS or renders per second;
- render-hook milliseconds;
- present-call milliseconds;
- input-to-present milliseconds;
- missed-frame count;
- proof/readback mode;
- telemetry drop count;
- last missed-frame cause.

Expose stats through a cheap `preview-perf-snapshot` IPC endpoint:

- copy fixed scalar fields only;
- never lock runtime state;
- never compute a full runtime summary;
- never parse `preview_loop_report` JSON for UI display;
- never increase full `runtime-summary` cadence just to animate FPS;
- throttle dev display refresh to roughly 4-10 Hz.

Reports and HUD should expose the same key terms:

- `input_to_present_ms_p50_p95_p99_max`;
- `render_hook_ms_p50_p95_p99_max`;
- `layout_ms_p50_p95_p99_max`;
- `present_call_ms_p50_p95_p99_max`;
- `upload_bytes_p50_p95_max`;
- `draw_call_count_p50_p95_max`;
- `glyph_cache_hit_rate`;
- `materialized_cell_count`;
- `telemetry_drop_count`;
- `proof_overhead_ms_p50_p95_max`;
- `last_missed_frame_cause`.

The dev window must consume cached shell state while rendering. It must not
perform a transport call from `footer_lines()` or any render hook.

## Timing Definitions

Latency reports must define start/end timestamps, minimum sample count, warmup
policy, adapter identity, present mode, outlier policy, and whether each metric
is CPU-submit, compositor-present, GPU-completion, or proof-completion time.

- `input_to_present_ms`:
  - starts when the role poll hook accepts a host input delta that can affect
    visible state;
  - ends when the frame containing that accepted delta is submitted and
    `present()` returns;
  - excludes async proof/readback completion.
- `render_hook_ms`:
  - CPU wall time inside the role render hook only;
  - excludes scheduler polling, IPC, report serialization, and proof
    serialization;
  - layout work is included only when the current implementation still performs
    layout inside the render hook, and must then be reported separately too.
- `layout_ms`:
  - CPU wall time spent producing or updating `LayoutFrame`, layout fragments,
    or retained layout state.
- `present_call_ms`:
  - CPU wall time for the present path through surface acquisition, command
    submission, and `frame.present()`;
  - does not claim GPU completion.
- `proof_overhead_ms`:
  - CPU/GPU/readback/reporting cost attributable to proof mode;
  - reported separately from `input_to_present_ms`.
- `instrumentation_overhead`:
  - measured delta or fixed-accounted cost of counters, trace, proof, and report
    emission.

Preview-local UX latencies must use the preview process monotonic clock. Do not
compare serialized `Instant` values across preview and dev processes. Cross-
process command latency must be reported as explicitly owned send/ack timings.

## Genericity And No-Hacks Guardrails

Performance fixes must stay generic. Cells is a large fixture and useful stress
case, not a compiler/runtime/document/renderer/verifier special case.

- No production branches in these crates on example-specific names, source
  paths, labels, or fixture strings:
  - `crates/boon_ir`;
  - `crates/boon_runtime`;
  - `crates/boon_document`;
  - `crates/boon_native_gpu`;
  - `crates/boon_native_playground`;
  - `crates/boon_native_app_window`;
  - `crates/xtask` verifier shortcuts.
- The banned strings include:
  - example name;
  - source path;
  - `cells`;
  - `address`;
  - `value`;
  - `error`;
  - `A0`;
  - other fixture-specific strings.
- Startup sparsity must be semantic. It must come from derived-value policy and
  currentness semantics, not from field names like `cells.value`.
- `List/find` and `List/find_value` must use generic list indexes for arbitrary
  list and field names, including duplicate values and selection semantics.
- Exact lookup invalidation must track old and new lookup values. Do not
  invalidate all lookups just to stay correct, and do not invalidate only the
  new value.
- `address` remains app data. Runtime identity remains hidden key plus
  generation.
- Formula/list fanout must be generic read-index/currentness behavior.
- Batch reset and startup fast paths must be generic pattern recognizers with
  fallback equivalence tests.

## Current Implementation Progress

2026-06-30 first implementation slice:

- Added generic `NativeFramePacing`, `NativePreviewPerfStats`, and
  `FrameEvidenceKey` data models in `boon_native_app_window`.
- Render-loop reports now include `frame_pacing`, `preview_perf_stats`, and
  `frame_evidence_key`, with proof overhead kept separate from
  `input_to_present_ms`.
- Visible-surface readback artifacts can carry the same `FrameEvidenceKey` as
  the presented frame they prove.
- Preview app-window hooks publish bounded perf stats into preview IPC state
  without reading runtime summaries.
- Added a cheap `preview-perf-snapshot` IPC endpoint that copies fixed scalar
  fields only and avoids `LiveRuntime::state_summary()`.
- The dev window footer now includes one cached `Preview perf` row and refreshes
  the snapshot outside input hot paths at a 250 ms cadence. If only the footer
  row changes and a retained footer layout exists, the dev window patches footer
  text instead of forcing a full layout refresh.
- Implemented `RequestedAnimation` as a bounded pacing substate inside
  `DemandDriven`, not as a third render-loop mode. Host input and explicit
  animation requests now schedule short 60 FPS follow-up bursts using the plan's
  initial min-frame, quiet, and hard-cap defaults.
- Focused verification passed:
  - `cargo check -q -p boon_native_app_window`
  - `cargo check -q -p boon_native_playground`
  - `cargo test -q -p boon_native_app_window frame_evidence_key`
  - `cargo test -q -p boon_native_app_window preview_perf_stats`
  - `cargo test -q -p boon_native_app_window requested_animation_burst`
  - `cargo test -q -p boon_native_playground preview_perf`

This slice does not complete the plan. Deeper frame-pacing verification,
active/pending snapshots, retained extraction/upload, full proof identity gates,
native UX latency gates, and generic runtime/list/currentness work remain.

2026-06-30 proof-contract slice:

- Render-loop reports now include `proof_lag_frames` when a readback artifact
  proves an earlier presented frame than the current report frame.
- `verify_report_schema` now applies a native GPU contract check to passing
  native reports:
  - native UX reports are rejected when they claim
    `render_loop_mode=continuous_probe`;
  - `frame_pacing` and `preview_perf_stats` shapes are validated when present;
  - every `wgpu-visible-surface-copy-src-readback` artifact must carry a
    structured `frame_evidence_key`;
  - readback artifact `content_revision`, `rendered_frame_count`, and
    `surface_epoch` are checked against the embedded frame evidence key when
    those artifact fields are present;
  - `preview_perf_stats.frame_evidence_key` must match the top-level
    `frame_evidence_key`;
  - `proof_lag_frames` must match the top-level frame and
    `last_interactive_readback_artifact.frame_evidence_key`.
- `verify-native-gpu-negative` now includes adversarial cases for:
  - hash-only visible readback proof;
  - mismatched readback frame evidence;
  - preview perf frame-evidence mismatch;
  - missing proof-lag reporting.
- Focused verification passed:
  - `cargo test -q -p boon_report_schema native_gpu_schema`
  - `cargo check -q -p boon_native_app_window`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`
  - `cargo xtask verify-report-schema target/reports/native-gpu/negative.json`
  - `git diff --check`
  - production diff no-hacks scan for `cells`, `address`, `A0`, source path,
    fixture-specific, and example-specific strings

2026-06-30 retained scroll slice:

- Preview runtime-backed scroll now has a generic same-materialized-window fast
  path:
  - reads `document_scroll_window` from the current layout proof;
  - when the new scroll offset stays in the same row/column materialization
    window, reuses the retained base layout frame and applies only the residual
    transform;
  - avoids the live-runtime `document_state_summary_for_window` relower path for
    same-window wheel movement;
  - stores `scroll_materialization_mode` and
    `retained_same_materialized_window_scroll` in `layout_profile` for verifier
    evidence.
- Scroll transform metadata now carries `base_layout_frame_hash`, so repeated
  retained scroll updates reapply the total residual to the base frame instead
  of compounding transforms.
- Focused verification passed:
  - `cargo check -q -p boon_native_playground`
  - `cargo test -q -p boon_native_playground cells_preview_same_window_scroll_uses_retained_materialized_frame`
  - `cargo test -q -p boon_native_playground cells_preview_scroll_input_moves_window_forward_and_back`
  - `cargo test -q -p boon_native_playground cells_shift_wheel_scrolls_horizontally`
  - `cargo test -q -p boon_native_playground cells_horizontal_scroll_keeps_row_gutter_fixed_and_headers_synced`

2026-06-30 native UX product-path gate slice:

- `verify_report_schema` and `verify-native-gpu-negative` now reject passing
  native UX reports that cheat the product path:
  - `render_loop_mode=continuous_probe`;
  - proof required to make visible updates happen;
  - input injected below `HostEvent` / `HostInputEvent`;
  - preview IPC blocking or dev perf-row hot-path IPC/runtime queries;
  - passive scroll runtime dispatch, source replacement, summary queries, or
    graph rebuild;
  - browser/headless/Xvfb/COSMIC/human/desktop screenshot proof substituted for
    app-owned native WGPU proof.
- UX classification is command-pattern based, not example-name based. The
  verifier does not branch on Cells, address fields, formula fields, source
  paths, or fixture strings for this gate.
- Added adversarial negative fixtures for continuous-probe UX, proof-gated
  visible updates, below-host input, preview IPC blocking, passive-scroll
  runtime dispatch and graph rebuild, dev perf-row hot-path queries, nested
  private runtime dispatch, desktop screenshot proof, browser proof
  substitution, and COSMIC toplevel proof.
- Native negative report schema now requires the product-path adversarial case
  subset to remain listed in `required_negative_cases` and rejects duplicate or
  under-counted native negative manifests. The subset is intentionally generic
  and does not duplicate older example-named negative fixture IDs into the
  schema crate.
- Focused verification passed:
  - `cargo test -q -p boon_report_schema native_gpu`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`
  - `cargo xtask verify-report-schema target/reports/native-gpu/negative.json`
  - `git diff --check`

2026-06-30 active route snapshot slice:

- Hit routing now has a generic active route snapshot cache:
  - caches the fully typed `PreviewHitRouteTable`, not only the static hit
    side table;
  - keys reuse by active layout hash, static route fingerprint,
    runtime-state snapshot identity, shared-state update count, and scroll
    offsets;
  - keeps the existing static route table cache for reusable layout/source
    identity while preventing stale state-summary reuse.
- Static route fingerprints now include route-affecting text-input style
  inputs used by the hit route path: `input_live_change`, `size`, and
  `text_inset`.
- Static snapshot route keys are reused only when the active layout frame is
  the exact cached snapshot frame. Retained layout overrides recompute the
  static key from the active frame instead of trusting a precomputed snapshot
  key.
- Native input timing samples already expose `route_table_key_source`; focused
  tests now require the click path to report `active_route_snapshot` after a
  warmed active-frame route table.
- This is a bounded active-frame cache step, not the full active/pending
  runtime-layout-render snapshot architecture. Pending snapshot ownership,
  stale-worker cancellation, and render-scene upload deltas remain.
- Focused verification passed:
  - `cargo check -q -p boon_native_playground`
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses_active_snapshot_and_rejects_stale_state`
  - `cargo test -q -p boon_native_playground preview_route_cache_key_recomputes_for_retained_layout_override`
  - `cargo test -q -p boon_native_playground preview_route_cache_key_uses_snapshot_key_with_embedded_source_intents`
  - `cargo test -q -p boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json`
  - `cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path`
  - `cargo test -q -p boon_native_playground cells_focus_only_route_syncs_formula_bar_text`
  - `cargo test -q -p boon_native_playground host_route_does_not_use_direct_node_source_for_stale_row_identity`
  - `cargo test -q -p boon_native_playground cells_preview_same_window_scroll_uses_retained_materialized_frame`

2026-06-30 replace-source pending lifecycle slice:

- The source replacement worker now reports a real single-slot lifecycle:
  - queued pending snapshot count;
  - in-flight build count and identity;
  - superseded in-flight build count;
  - stale in-flight snapshot observation;
  - started, completed, coalesced, dropped, and stale-rejected counters.
- `active_pending_snapshot_backpressure` now distinguishes current
  commit-eligible pending snapshots from superseded in-flight background work.
  A queued newer replacement can coexist with a stale in-flight older build, but
  only the queued replacement counts as the current pending snapshot.
- `xtask` now requires these lifecycle fields in active/pending backpressure
  proof and rejects reports that claim both a queued and current in-flight
  pending snapshot at the same time.
- Added a stale commit negative test proving an older built source result cannot
  mutate active preview source/runtime/layout/shared render state after a newer
  source revision has been accepted.
- This still does not satisfy the full plan requirement for pending snapshots to
  validate surface epoch, frame sequence, layout revision, and render-scene
  revision before commit. That evidence must be threaded through future
  runtime-layout-render snapshot objects.
- Focused verification passed:
  - `cargo check -q -p xtask`
  - `cargo test -q -p boon_native_playground preview_replace_worker_queue_reports_live_latest_wins_metrics`
  - `cargo test -q -p boon_native_playground preview_replace_worker_backpressure_separates_current_pending_from_stale_in_flight`
  - `cargo test -q -p boon_native_playground stale_replace_source_commit_does_not_mutate_active_preview_state`
  - `cargo test -q -p boon_native_playground replace_source_ack_is_small_and_worker_commits_latest_revision`

## Implementation Slices

1. Terminology and schema:
   - define `NativeRenderLoopMode`, `NativeFramePacing`, and instrumentation
     profile;
   - keep `RequestedAnimation` as a bounded burst pacing substate unless the
     native GPU architecture docs are updated too;
   - document product counters separately from optional trace/proof work.
2. Low-cost stats backbone:
   - add `PreviewPerfStats`;
   - record fixed-bucket or rolling-window timings without allocation-heavy hot
     path work;
   - add `preview-perf-snapshot` IPC;
   - report telemetry overhead and dropped updates.
3. Proof identity:
   - add `FrameEvidenceKey`;
   - thread evidence keys through presented frames and proof artifacts;
   - add stale-proof and mismatched-epoch negative tests.
4. Scheduler:
   - keep `DemandDriven` idle behavior;
   - implement requested-animation burst pacing and exit rules;
   - sample host input at frame start;
   - keep proof/readback outside normal UX latency.
5. Dev footer row:
   - render exactly one compact preview performance row from cached state;
   - prove it wraps within the existing footer limits;
   - prove it does not query runtime or block on IPC in render/hot paths.
6. Active/pending frame state:
   - keep active retained render state available for cheap scroll/selection
     updates;
   - apply pending snapshots only when complete and current;
   - reject stale pending snapshots by epoch/frame sequence.
7. Retained extraction and upload:
   - narrow the extraction from layout/document state to render input;
   - update only dirty render chunks, text runs, transforms, clips, and buffers;
   - report upload bytes, draw calls, cache hits, and materialized visible
     ranges.
8. Generic runtime/compiler/document/renderer/verifier audit:
   - add non-Cells fixtures for sparse startup, indexed lookup, exact
     invalidation, currentness barriers, move/remove/reinsert safety, and
     bounded fanout;
   - add audit tests rejecting production example-specific branches across
     runtime, compiler, document, renderer, playground, app-window, and verifier
     code.

## Verification

Native UX gates:

- Cells click/formula-bar visible update in release:
  - p95 `<= 16.7ms`;
  - app-owned native events;
  - app-owned WGPU/readback proof for selected samples;
  - proof latency reported separately.
- Cells vertical and horizontal scroll:
  - p95 `<= 16.7ms`;
  - max `<= 33.4ms` except explicitly bounded reported outliers.
- Dev code-editor wheel scroll:
  - no crash;
  - no preview IPC blocking;
  - release p95 within configured budget.
- Idle-wake:
  - passes in `DemandDriven`;
  - reported only as idle correctness and wake smoke proof, not as the primary
    UX benchmark.

Proof gates:

- app-owned WGPU readback remains the native visual proof path;
- proof latency and proof serialization are reported separately;
- no desktop screenshots, Xvfb, legacy Ply, browser screenshots, COSMIC
  scraping, or human observation as native GPU proof.
- every proof artifact must carry a structured frame evidence key:

```rust
pub struct FrameEvidenceKey {
    pub frame_seq: u64,
    pub content_revision: u64,
    pub layout_revision: u64,
    pub render_scene_revision: u64,
    pub surface_id: SurfaceId,
    pub surface_epoch: u64,
    pub input_event_seq: Option<u64>,
    pub present_id: u64,
    pub proof_request_id: Option<u64>,
}
```

- proof `frame_seq` must equal or explicitly reference the measured presented
  frame;
- proof `surface_epoch` must match the presented surface epoch;
- proof `content_revision`, `layout_revision`, and `render_scene_revision` must
  match the rendered content revisions;
- `proof_lag_frames` must be reported when proof completes after presentation;
- stale first-frame proof reuse fails;
- proof cache hit without matching `FrameEvidenceKey` fails;
- hash-only proof without structured artifact metadata fails.

Native UX gates fail if:

- `render_loop_mode == ContinuousProbe`;
- proof mode is required to make the visible update happen;
- input is injected below `HostEvent` / `HostInputEvent`;
- `preview_blocked_on_ipc_count > 0`;
- passive scroll causes runtime dispatch or graph rebuild;
- the dev perf row performs IPC or runtime queries from render hooks;
- visual proof comes from desktop screenshot, human observation, browser
  screenshot, Xvfb, legacy Ply, or COSMIC scraping.

Performance HUD gates:

- stats row is visible in the dev window;
- `preview_perf_hot_path_query_count = 0`;
- `preview_blocked_on_ipc_count = 0`;
- `preview_perf_payload_bytes` is bounded;
- stats-visible preview frame p95 remains within budget;
- stats-visible dev frame p95 remains within budget.

Generic runtime gates:

- non-Cells fixtures prove startup sparsity, indexed lookup, exact invalidation,
  currentness barriers, move/remove/reinsert safety, and bounded dependent
  recompute;
- audit test rejects production runtime/compiler example-specific branches;
- Cells remains at least the existing 2600 logical cells and reports logical,
  materialized, rendered, and formula-evaluated counts separately.

## Embedded `/goal` Prompt

Use this later as:

```text
/goal Follow docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md until the entire plan is implemented and honestly verified.

Performance is the main goal. Implement the native preview architecture so normal visible interaction uses bounded requested-animation bursts inside DemandDriven mode, retained/hot renderer state, sparse runtime/layout work, and proof/debug modes that are toggleable and separately measured. Do not treat idle-wake as the main UX benchmark; keep it as a demand-driven smoke gate only. Do not turn RequestedAnimation into a third long-lived product mode unless the native GPU architecture docs are updated too.

Use subagents whenever useful for independent architecture, runtime/compiler, WGPU/rendering, testing, or external-library research. If the path starts becoming too complex, too hacky, or circular, stop micro-tuning and choose a simpler generic architecture that matches the source-of-truth docs.

Do not add Cells/example-specific hacks in compiler, runtime, document, renderer, app-window, playground, or verifier code. No production branches on example names, source paths, cells/address/value/error/A0, or fixture-specific strings. Fix engine/runtime/document/rendering architecture instead.

Implement the dev-window preview performance row and bounded preview-perf snapshot. Add exact timing definitions, product counters, burst exit criteria, active/pending snapshot backpressure, and FrameEvidenceKey proof identity. Prove visually and functionally with deterministic native tests, app-owned WGPU readbacks tied to the measured presented frame, release-mode latency reports, runtime counters, and schema-valid reports. Fix broken/flaky verification infrastructure too when it blocks reliable progress.

Do not claim completion until all required native UX, proof identity, perf-HUD, generic runtime, no-hacks audit, and stale-proof negative gates pass on fresh reports for the current worktree/binary. If blocked, leave the repo coherent and report the exact blocker, evidence, and next implementation step.
```
