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

2026-06-30 pending frame-evidence commit guard slice:

- Source replacement now captures the current preview `FrameEvidenceKey` when a
  pending source/runtime/layout/render snapshot is accepted.
- Final source replacement commit validates that the accepted frame evidence and
  current preview frame evidence still share the same surface id and surface
  epoch, and that frame sequence, content revision, layout revision,
  render-scene revision, and present id have not regressed. A later slice
  tightens this from no-regression to exact frame-evidence matching.
- A stale frame-evidence result is rejected before mutating active preview
  source, runtime units, runtime summary, live runtime, world scene/session, or
  retained shared render state.
- Replace-source status and `active_pending_snapshot_backpressure` now expose:
  - accepted pending frame evidence;
  - current frame evidence at commit/report time;
  - pending frame-evidence status;
  - stale rejection reason;
  - currentness policy.
- `xtask` now requires active/pending backpressure proof to carry the
  frame-evidence/currentness fields and validates their allowed status and
  policy values.
- This is still a conservative commit-currentness guard. The app-window
  `layout_revision` and `render_scene_revision` values are currently aliases of
  the content revision; a future renderer/layout identity slice must make those
  independent identities before the full active/pending snapshot contract can be
  called complete.
- Focused verification passed:
  - `cargo test -q -p boon_native_playground replace_source_commit_rejects_stale_surface_epoch_before_state_mutation`
  - `cargo test -q -p boon_native_playground stale_replace_source_commit_does_not_mutate_active_preview_state`
  - `cargo test -q -p boon_native_playground preview_replace_worker_backpressure_separates_current_pending_from_stale_in_flight`
  - `cargo test -q -p boon_native_playground replace_source_ack_is_small_and_worker_commits_latest_revision`
  - `cargo check -q -p boon_native_playground`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`
  - `cargo xtask verify-report-schema target/reports/native-gpu/negative.json`

2026-06-30 native UX proof-currentness gate slice:

- Native UX report validation now rejects visible proof that is stale for the
  measured interaction:
  - `proof_lag_frames > 0`;
  - stale-for-latest-input flags;
  - pending interactive readback flags;
  - visible-surface readback artifacts that did not complete before deadline;
  - `last_interactive_readback_artifact.frame_evidence_key` differing from the
    top-level `frame_evidence_key`.
- The gate is command-classified and generic. It does not branch on examples,
  source paths, Cells fields, formula fields, or fixture labels.
- `verify-native-gpu-negative` now includes required adversarial cases for
  lagged UX proof, pending UX readback, and same-frame proof identity mismatch.
- Focused verification passed:
  - `cargo test -q -p boon_report_schema native_gpu_schema_rejects_lagged_ux_proof`
  - `cargo test -q -p boon_report_schema native_gpu_schema_rejects_pending_ux_readback`
  - `cargo test -q -p boon_report_schema native_gpu_schema_rejects_same_frame_proof_identity_mismatch`
  - `cargo test -q -p boon_report_schema native_gpu_negative_schema_requires_product_path_cases`
  - `cargo check -q -p boon_report_schema`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`
  - `cargo xtask verify-report-schema target/reports/native-gpu/negative.json`

2026-06-30 independent frame-evidence layer revision slice:

- `NativeRenderLoopState` now tracks last presented content, layout, and
  render-scene revisions independently instead of deriving all three
  `FrameEvidenceKey` fields from content revision.
- `NativeRenderHookResult` can carry optional layout and render-scene revisions.
  Hooks that do not provide them retain the older fallback behavior; hooks that
  do provide them let reports distinguish document content, layout identity, and
  render-scene identity.
- Preview and dev render hooks now derive monotonic layer revisions from generic
  proof identities:
  - layout identity comes from `layout_frame_hash`;
  - render-scene identity comes from `render_scene_identity` or
    `render_scene_hash`;
  - no production path branches on example names, source paths, Cells fields, or
    fixture labels.
- Dev-window proof JSON now emits `layout_frame_hash`, `render_scene_hash`, and
  `render_scene_identity`, matching the preview proof identity shape.
- App-owned prelowered render-scene readback artifacts now carry
  `render_scene_identity_hash` in addition to the older compatibility
  `layout_frame_hash` field.
- Report schema and xtask native frame-evidence linkage now validate
  `last_render_layout_revision == frame_evidence_key.layout_revision` and
  `last_render_scene_revision == frame_evidence_key.render_scene_revision` when
  those top-level fields are present.
- This slice resolves the aliasing caveat recorded in the pending
  frame-evidence commit guard slice. It still does not complete the whole plan:
  retained extraction/upload deltas, full active/pending runtime-layout-render
  snapshot ownership, release-mode UX latency gates, and generic runtime/list
  currentness work remain.
- Focused verification passed:
  - `cargo test -q -p boon_native_app_window frame_evidence`
  - `cargo test -q -p boon_native_app_window render_hook_result_can_carry_independent_layer_revisions`
  - `cargo test -q -p boon_native_app_window presented_state_records_render_layer_revisions`
  - `cargo test -q -p boon_report_schema native_gpu_schema_accepts_structured_frame_evidence`
  - `cargo test -q -p boon_report_schema native_gpu_schema_rejects_mismatched_top_level_layer_revisions`
  - `cargo test -q -p boon_native_gpu app_owned_scene_readback_uses_prelowered_render_scene_identity`
  - `cargo check -q -p boon_native_app_window`
  - `cargo check -q -p boon_native_gpu`
  - `cargo check -q -p boon_native_playground`
  - `cargo check -q -p boon_report_schema`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json`
  - `cargo xtask verify-report-schema target/reports/native-gpu/negative.json`
  - `git diff --check`
  - production diff no-hacks scan for `cells`, `address`, `A0`, source path,
    fixture-specific, example-specific, and hardcode strings

2026-06-30 scroll real-window evidence currentness slice:

- Scroll-speed report assembly now treats isolated Weston measured-loop input as
  usable scroll evidence only when it is packaged with same-frame native proof:
  - app-window input adapter shows delivered real OS wheel events;
  - measured loop report status is pass;
  - `last_external_render_proof` used the visible-surface render path and
    skipped render-hook offscreen app-owned proof;
  - `last_interactive_readback_artifact` is an app-owned WGPU visible-surface
    readback completed before deadline;
  - readback `FrameEvidenceKey` matches the measured frame evidence key.
- The scroll model now separates observed real-window wheel input from proven
  real-window speed. Real-window speed proof additionally requires a real
  post-input timing window (`post-real-window-input` or
  `axis-specific-post-real-window-input`) with measured post-input frames.
- This prevents the previous bad shape where an isolated input adapter could
  turn an operator-host plan into apparent real-window speed proof without
  current timing/proof linkage.
- A fresh Cells scroll-speed verifier attempt was stopped after it entered the
  long axis-retry path. The stale report still failed on missing real-window
  wheel proof; the interrupted fresh run confirmed the broad isolated measured
  loop had real app-window wheel events and same-frame WGPU proof, but no
  complete axis-specific post-input timing report yet.
- Focused verification passed:
  - `cargo test -q -p xtask isolated_scroll`
  - `cargo test -q -p xtask axis_specific_real_window_input_overrides_planned_operator_wheel_input`
  - `cargo test -q -p xtask scroll_hot_path_rejects_render_hook_offscreen_proof`
  - `cargo check -q -p xtask`
  - `git diff --check`

2026-06-30 axis scroll fallback and proof identity slice:

- Axis-specific real-window scroll observation now resolves app-owned evidence
  generically from the strongest current report source:
  - loop report `observed_input_adapter` for delivered wheel input;
  - loop report `last_interactive_readback_artifact` and
    `last_external_render_proof` for same-frame proof currentness;
  - supervisor/role surface proof for post-input timing windows.
- Deterministic role/loop fallback is guarded by role status, isolated Weston
  display connection, matching `surface_id`, matching `surface_epoch`,
  same-frame WGPU readback, and non-empty post-input timing. It does not branch
  on Cells or fixture labels.
- `boon_native_app_window` now recursively attaches the presented
  `FrameEvidenceKey` to nested visible-surface WGPU readback proof objects, so
  proof identity is emitted at the source instead of patched only by verifiers.
- Fresh verification passed:
  - `target/debug/xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`
  - `target/debug/xtask verify-report-schema target/reports/native-gpu/scroll-speed-cells.json`
- Fresh Cells scroll-speed report highlights:
  - `status=pass`
  - `required_real_window_speed_proven=true`
  - `speed_timing_window=axis-specific-post-real-window-input`
  - `preview_frame_ms_p95=11.789346`
  - `post_input_frame_timing.measured_frame_count=118`
  - `post_input_frame_timing.presented_frame_ms_over_16_7_count=0`

2026-06-30 Cells visible-click structured proof slice:

- `boon_native_app_window` recognizes structured nested
  `wgpu-visible-surface-copy-src-readback` proof as replacing the duplicate
  interaction readback only when the nested proof status is pass.
- `verify-native-cells-visible-click-e2e` now consumes that structured
  same-frame proof instead of waiting for an obsolete duplicate
  `last_interactive_readback_artifact`:
  - the proof `FrameEvidenceKey` must match the measured presented frame;
  - the visible-surface render path must be direct and app-owned;
  - `preview_blocked_on_ipc_count` must remain zero;
  - retained bound text updates must prove the formula text and selected address;
  - focused-node render-state evidence must prove the clicked cell is selected
    and focused.
- Metadata-only retained text sync still fails, and a mismatched
  `FrameEvidenceKey` structured proof fails.
- Fresh verification passed:
  - `cargo test -q -p boon_native_app_window external_visible_readback_proof`
  - `cargo test -q -p xtask cells_visual_formula_probe_requires_expected_formula_bar_text_value`
  - `cargo check -q -p xtask`
  - `timeout 900s cargo run -q -p xtask -- verify-native-cells-visible-click-e2e --profile release --repeat-count 2 --report target/reports/native-gpu/cells-visible-click-e2e-release-current.json`
  - `cargo run -q -p xtask -- verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release-current.json`
- Fresh Cells visible-click report highlights:
  - `status=pass`
  - `target_count=8`
  - `input_wake_to_formula_visible_ms_p95=15.929690`
  - `click_to_formula_visible_ms_p95=27.529414`
  - `present_call_ms.p95=8.888936`
  - `render_started_to_render_hook_completed_ms.p95=3.067628`
  - `retained_update_contract.total=8`, `retained=8`, `full_lower=0`
  - `runtime_work_contract.total=8`, `zero_scans=8`,
    `zero_root_materialization=8`, `zero_recompute=8`
- This does not complete the full plan. Remaining work still includes full
  aggregate native GPU gates, dev-code-editor crash/scroll coverage,
  broader no-hacks audit, and generic runtime/list/currentness verification.

2026-07-01 dev-editor scroll retry evidence slice:

- `verify-native-dev-editor-scroll-speed` now uses the shared axis-specific
  native scroll retry helper instead of running vertical and horizontal probes
  once. The helper is profile-aware, so the standalone dev-editor gate can run
  debug or release while the existing release-only scroll-speed gate keeps its
  behavior.
- When all retry attempts fail, the helper now returns the strongest failed
  observation instead of blindly returning the last attempt. The selection score
  prefers real wheel delivery, nonzero axis delta, post-input timing, role/loop
  report agreement, same-frame proof, and lower p95 frame timing. Reports now
  include `axis_retry_selected_attempt` and `axis_retry_selection` so verifier
  evidence explains which attempt was used.
- Focused verification passed:
  - `cargo check -q -p xtask`
  - `cargo test -q -p xtask native_scroll_axis`
  - `git diff --check`
- Fresh release verifier attempt:
  - `timeout 900s cargo xtask verify-native-dev-editor-scroll-speed --profile
    release --report target/reports/native-gpu/dev-editor-scroll-speed-release.json`
  - status remains `fail`;
  - both axes now show real wheel movement:
    `scroll_line_delta=15`, `scroll_column_delta=12`;
  - horizontal timing is within budget:
    `horizontal.presented_frame_ms_p95=11.248325`;
  - remaining blockers are:
    - vertical p95/max still above release budget:
      `wheel_to_visible_p95=25.122470`, max `29.140229`;
    - the report still counts cumulative full layout refreshes during the
      passive-scroll run:
      `full_layout_refresh_count_for_passive_scroll=27`,
      `fast_frame_patch_count_for_passive_scroll=4`;
    - same-frame dev-surface readback proof is not yet produced for the
      measured frame. The measured loop frame evidence was
      `frame_seq=162`, `content_revision=31`, while the available WGPU readback
      artifact was from an older frame (`frame_seq=126`, `content_revision=13`).
- This slice improves verifier determinism and diagnostic honesty. It does not
  complete dev-editor scroll performance. The next architecture work is to make
  dev-surface interaction frames keep retained render-scene patch/cache state
  hot through the measured frame window, avoid cumulative startup/layout counts
  in passive-scroll hot-path checks, and schedule same-frame visible-surface
  WGPU proof for the measured dev frame without putting readback on the UX hot
  path.

2026-07-01 dev-editor proof/counter completion slice:

- Dev-window interactive visible-surface readback is enabled for verifier proof
  mode, with final report drain and proof-mode backpressure so timer/requested
  animation frames do not advance past an in-flight measured proof frame.
- Dev visible-surface proof now carries the same render-target metadata shape
  used by preview proof, including the explicit app-owned offscreen-readback
  skip marker when the dev hook already rendered a direct visible surface.
- Dev render reports now distinguish lifetime render/layout counters from
  passive-scroll hot-path counters:
  - `full_layout_refresh_count` / `fast_frame_patch_count` remain lifetime
    diagnostics;
  - `passive_scroll_full_layout_refresh_count` /
    `passive_scroll_fast_frame_patch_count` are the gate inputs;
  - missing passive-scroll fields fail closed instead of falling back to
    lifetime counters.
- Focused verification passed:
  - `cargo fmt --check`
  - `cargo check -q -p boon_native_playground`
  - `cargo check -q -p xtask`
  - `cargo test -q -p xtask native_scroll_axis`
  - `git diff --check`
  - `timeout 900s cargo xtask verify-native-dev-editor-scroll-speed --profile
    release --report target/reports/native-gpu/dev-editor-scroll-speed-release.json`
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/dev-editor-scroll-speed-release.json`
- Fresh release report highlights:
  - `status=pass`
  - `dev_editor_frame_ms_p50_p95_p99_max.p95=12.144384`
  - `dev_editor_frame_ms_p50_p95_p99_max.max=19.247104`
  - `full_layout_refresh_count_for_passive_scroll=0`
  - `fast_frame_patch_count_for_passive_scroll=1`
  - lifetime diagnostics remain visible:
    `full_layout_refresh_count_lifetime=21`,
    `fast_frame_patch_count_lifetime=4`
  - both axes report measured-loop same-frame app-owned readback proof.
- This does not complete the full realtime/proof plan. Remaining work still
  includes full aggregate native GPU gates, direct retained render-scene patch
  hit-rate improvement, broader no-hacks audits, and generic runtime/list
  currentness verification.

2026-07-01 preview E2E proof-path refresh slice:

- `verify-native-gpu-preview-e2e` now refreshes stale or missing headed visual
  evidence through the existing `verify-native-gpu-headed-scenario` route before
  judging the preview E2E result. This keeps headed proof tied to the current
  source hash instead of failing on stale report artifacts.
- Preview E2E now keeps render-hook offscreen readback out of the UX proof path
  by launching with `--skip-render-hook-app-owned-proof` and promoting the
  measured visible-surface WGPU readback as `native_gpu_render_proof`.
- The promoted proof records `kind=app_owned_pixels`,
  `capture_method=wgpu-visible-surface-copy-src-readback`, and carries the
  visible-surface `frame_evidence_key` from the measured frame. Replacement
  proofs that do not carry a real artifact are no longer promoted as native
  render proof.
- Compact preview frame metrics now include bounded retained chunk and asset
  observability samples (`retained_chunks`, `asset_refs`, and
  `asset_failure_diagnostics`) instead of omitting the fields that the visible
  reality harness requires.
- Focused verification passed:
  - `cargo fmt --check`
  - `cargo check -q -p xtask`
  - `cargo check -q -p boon_native_playground`
  - `cargo test -q -p xtask native_preview_promotes_measured_loop_real_window_evidence`
  - `cargo test -q -p xtask isolated_preview_e2e_keeps_dev_app_owned_input_probe`
  - `cargo test -q -p xtask isolated_weston_desktop_preview_e2e_uses_demand_driven_product_mode`
  - `cargo test -q -p xtask headed_visual_refresh_args_target_current_example_report`
  - `timeout 1200s cargo xtask verify-native-gpu-preview-e2e --example
    todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json`
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/preview-e2e-todomvc.json`
- Fresh Todomvc preview E2E report highlights:
  - `status=pass`
  - `visible_reality_harness.status=pass`
  - `scenario_evidence.status=pass`
  - `preview_surface_proof.external_render_proof.render_backend_trait=boon_native_gpu::encode_render_scene_to_surface`
  - `preview_surface_proof.external_render_proof.offscreen_app_owned_scene_readback_skipped=true`
  - `native_gpu_render_proof.artifact.kind=app_owned_pixels`
  - `native_gpu_render_proof.artifact.capture_method=wgpu-visible-surface-copy-src-readback`
- This slice improves proof identity and verifier freshness for generic native
  preview E2E. It does not complete the full plan; Cells speed/click coverage,
  aggregate native GPU gates, perf HUD, pending snapshot currentness, and the
  no-hacks audit remain open.

2026-07-01 preview perf rolling-stats and guard slice:

- `boon_native_app_window` now keeps a bounded rolling preview-performance
  accumulator in the native preview process instead of publishing only the last
  sample:
  - `render_hook_ms_p50_p95_p99_max`;
  - `present_call_ms_p50_p95_p99_max`;
  - `input_to_present_ms_p50_p95_p99_max`;
  - `proof_overhead_ms_p50_p95_max`.
- Render-loop reports now snapshot the same accumulated preview-local timing
  window before async report serialization. Proof overhead remains separate and
  is added at report construction rather than folded into UX latency.
- The dev footer still uses one compact `Preview perf` row from cached shell
  state, but now shows rolling p95 during burst/probe modes when available.
- `preview-perf-snapshot` no longer appends replace-worker latest-wins state to
  the perf endpoint. It reports an explicit payload cap and whether the fixed
  payload stayed within that cap.
- Dev render proof now emits explicit zero guard fields for preview-perf
  hot-path IPC/runtime origins:
  - footer-lines IPC;
  - render-hook IPC;
  - runtime-summary queries;
  - input-hot-path perf queries.
- Native report schema now rejects preview perf stats that omit the rolling
  summary objects and rejects incomplete dev preview-perf hot-path guard blocks.
- Independent no-hacks audit still found production blockers that remain open:
  - IR/runtime `address`, `value`, and `error` string-coupled behavior;
  - native playground interaction-speed dispatch by example id;
  - a Cells-shaped retained focus/selection fast path using app field names.
- Focused verification passed:
  - `cargo fmt --check`
  - `cargo test -q -p boon_native_app_window preview_perf`
  - `cargo test -q -p boon_native_playground preview_perf`
  - `cargo test -q -p boon_report_schema native_gpu_schema`
  - `cargo check -q -p boon_native_playground`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-negative --report
    target/reports/native-gpu/negative.json`
  - `cargo xtask verify-report-schema target/reports/native-gpu/negative.json`
  - `git diff --check`
- This does not complete the full plan. Remaining work still includes aggregate
  native GPU gates, full HUD presentation for renderer
  upload/layout/materialization stats, full active/pending
  runtime-layout-render snapshot ownership, the no-hacks cleanup above, and
  generic runtime/list/currentness verification.

2026-07-01 typed render-metrics preview stats slice:

- Added a generic `NativeRenderFrameMetrics` payload to the native app-window
  render-hook result. The payload carries renderer/layout facts without knowing
  which Boon example is running:
  - `layout_ms`;
  - `upload_bytes`;
  - `draw_call_count`;
  - `glyph_cache_hit_rate`;
  - `materialized_item_count`;
  - `visible_display_item_count`;
  - `queue_write_count`;
  - `preview_blocked_on_ipc_count`.
- The bounded preview-performance accumulator now records rolling summaries for
  layout time, upload bytes, draw calls, glyph-cache hit rate, and materialized
  item count alongside the existing render/present/input/proof timings.
- The native playground render hooks publish those typed metrics from
  app-owned WGPU frame metrics and layout materialization reports. This keeps
  the preview perf snapshot and native reports on structured data instead of
  parsing proof JSON or runtime summaries.
- The native report schema now requires these render-resource summary objects
  in `preview_perf_stats`, and the playground preview-perf fixtures cover the
  expanded contract.
- Focused verification passed:
  - `cargo fmt`
  - `cargo test -q -p boon_native_app_window preview_perf`
  - `cargo test -q -p boon_native_playground preview_perf`
  - `cargo test -q -p boon_report_schema native_gpu_schema`
- This does not complete the full realtime plan. Remaining work still includes
  showing the extra renderer-resource metrics in a polished dev-window HUD row
  or panel, full native GPU aggregate gates, active/pending frame ownership,
  retained selection/focus genericization, and generic runtime/list/currentness
  work.

2026-07-01 generic root `List/find` cache/currentness slice:

- The root list-view projection path for generic `List/find(list, field, value)`
  now uses the shared profiled exact text lookup cache instead of bypassing it
  with a direct storage lookup. Direct-find profiles now report cache lookup
  time, storage lookup time, cache insert time, cache-hit status, and cache
  entry count from the same generic cache used by ordinary `List/find`.
- Root list-view row diffs now emit exact old and new text lookup read keys for
  changed textlike fields. A row changing from one key value to another
  invalidates projections/caches for those two lookup values, while unrelated
  exact lookup values can remain current.
- Added non-Cells runtime fixtures with arbitrary record/key names proving:
  - root `List/find` projection uses the exact text lookup cache and reports a
    cache hit when seeded;
  - root `List/find` projection keeps zero row scans on indexed exact lookup;
  - root list-view changed reads include old/new lookup values without
    invalidating unrelated exact lookups.
- Focused verification passed:
  - `cargo test -q -p boon_runtime root_list_find_projection_uses_cached_exact_text_lookup_for_generic_records`
  - `cargo test -q -p boon_runtime root_list_view_changed_reads_emit_exact_lookup_values_for_generic_text_fields`
  - `cargo test -q -p boon_runtime list_index_find`
  - `cargo test -q -p boon_runtime exact_list_lookup_invalidation_tracks_old_and_new_text_values`
  - `cargo test -q -p boon_runtime indexed_lookup_cache_invalidates_only_changed_list_field`
  - `cargo test -q -p boon_runtime root_currentness_barrier`
  - `cargo test -q -p boon_runtime root_list_view_prevalidated_clean_hit_still_refreshes_deferred_dirty_root`
  - `cargo test -q -p boon_ir indexed_derived_startup_recompute_is_ir_semantic_not_path_heuristic`
  - `cargo check -q -p boon_runtime`
  - `cargo fmt --check`
- This does not complete the full runtime plan. Remaining generic runtime work
  still includes formula dependency tracking/fanout, range invalidation, startup
  batch initialization, demand-current audit for all selected/render reads, and
  broad cache invalidation cleanup outside this root list-view path.

2026-07-01 retained overlay genericity review:

- A read-only subagent review confirmed that the retained selection/focus patch
  path is still address-shaped in native playground production code. The next
  visual/click slice should replace address-string overlay state with generic
  selected node sets derived from data-binding snapshots and source-intent
  dependencies.
- Proposed next implementation order:
  - add generic non-address overlay tests first;
  - add a data-binding-based retained selection patch collector;
  - replace the simple-click retained patch block with generic selected-node
    patch state;
  - replace retained frame/render-scene address helpers with node-set helpers;
  - move formula-bar proxy text refresh through bound text-input/source-intent
    sync paths;
  - update the focus-only host route last.
- This is recorded as the next production no-hacks blocker. It is not resolved
  by the root `List/find` runtime slice above.

2026-07-01 generic retained selection pre-pass slice:

- Added a generic `PreviewRetainedSelectionPatch` collector for retained
  selection/focus work. It compares changed static-equality bindings in the
  current post-turn state summary against the render snapshot's previous
  runtime document values, then returns:
  - previous selected/static-equality nodes;
  - current selected/static-equality nodes;
  - retained bound-sync nodes needed to patch style, text, and source-intent
    bindings.
- The collector is data-binding driven and has a non-address fixture using
  `store.active_item` with arbitrary item nodes. It does not branch on Cells,
  source paths, address fields, or fixture values.
- The simple source-click path now asks this collector for changed
  static-equality nodes after applying live events. Those nodes are fed into
  the existing retained bound-sync path before the older address-shaped fallback
  patcher runs. If the generic bound-sync updates the selected style nodes, the
  old fallback is skipped by the existing retained-sync guard.
- A narrower non-static dependency extender was added for this path so generic
  sync can include bound text/source-intent controls without pulling every
  static-equality alternative for the same data path into the hot patch set.
- Focused verification passed:
  - `cargo test -q -p boon_native_playground retained_selection_patch_uses_generic_static_equality_bindings`
  - `cargo test -q -p boon_native_playground static_equality_patch_lookup_uses_old_and_new_value_index`
  - `cargo test -q -p boon_native_playground targeted_bound_sync_expands_to_selection_dependent_formula_bar`
  - `cargo test -q -p boon_native_playground cells_click_selection_updates_formula_bar_and_selected_style`
  - `cargo test -q -p boon_native_playground cells_focus_only_route_syncs_formula_bar_text`
  - `cargo check -q -p boon_native_playground`
- This is a transition slice, not the full overlay cleanup. Remaining work
  still includes replacing `PreviewFocusOverlayState` address fields with
  selected node sets, replacing retained frame/render-scene address helpers,
  removing `store.selected_address` text fallback patching from production, and
  updating the focus-only host route to avoid injecting selection identity
  through address-specific code.

2026-07-01 generic selected-node overlay sidecar slice:

- `PreviewFocusOverlayState` now carries previous/current selected node sets
  alongside the older address fallback fields. The simple source-click path
  seeds those node sets from the generic retained selection patch collector,
  so downstream focus/selection overlay work can remain document-node based.
- Retained focus/selection item patching now applies selected state from
  generic node membership first and falls back to address lookup only when no
  selected node set is available. The old item-level address-only patch helper
  was replaced with a generic selected-overlay helper.
- Input-overlay render-scene patching now includes previous/current selected
  node sets when computing touched nodes. The sidecar path can therefore patch
  stale and newly selected primitives without rediscovering identity from app
  field names when generic binding evidence exists.
- Render proof diagnostics now report selected-node counts and bounded samples
  in `input_overlay_focus_state`, making it visible when the generic path is
  active.
- Added a non-Cells render-scene sidecar fixture using arbitrary selected
  `choice-*` nodes and no address/source-intent metadata, proving the sidecar
  can match full overlay lowering from node sets alone.
- Focused verification passed:
  - `cargo test -q -p boon_native_playground input_overlay_render_scene_patch_uses_generic_selected_node_sets`
  - `cargo test -q -p boon_native_playground input_overlay_render_scene_patch_matches_full_overlay_lowering`
  - `cargo test -q -p boon_native_playground cells_input_overlay_render_scene_patch_updates_stale_selected_cell_primitives`
  - `cargo test -q -p boon_native_playground cells_click_selection_updates_formula_bar_and_selected_style`
  - `cargo test -q -p boon_native_playground retained_selection_patch_uses_generic_static_equality_bindings`
  - `cargo test -q -p boon_native_playground cells_focus_only_route_syncs_formula_bar_text`
  - `cargo test -q -p boon_native_playground targeted_bound_sync_expands_to_selection_dependent_formula_bar`
  - `cargo check -q -p boon_native_playground`
- This is still not the full no-hacks cleanup. Remaining work includes
  replacing focus-only route selection identity with a data-binding/source-
  intent generic path, removing `store.selected_address` text fallback patching
  from production, and auditing/reporting the remaining address-shaped fallback
  helpers as diagnostics rather than product hot-path requirements.

2026-07-01 generic focus-only selection patch slice:

- The focus-only retained route now derives selected nodes from the generic
  retained selection patch collector and the current state summary. It no
  longer fabricates `store.selected_address` or `store.selected_input.address`
  into a synthetic state summary before patching retained state.
- Focus-only selection repaint now uses generic selected node membership when
  available, with the older event payload fallback used only if no generic
  static-equality binding evidence exists.
- The retained selected overlay fallback no longer scans top-left text by
  coordinates and previous address text. Text updates are left to the generic
  bound-text/source-intent sync path, which already has binding targets and
  state-summary currentness.
- Added a non-Cells focus-only fixture using arbitrary `store.active_item`
  static equality bindings and no address payload. It proves focus-only
  retained selection and bound text updates work from data bindings alone.
- Focused verification passed:
  - `cargo test -q -p boon_native_playground focus_only_route_uses_generic_selection_binding_without_address_payload`
  - `cargo test -q -p boon_native_playground cells_focus_only_route_syncs_formula_bar_text`
  - `cargo test -q -p boon_native_playground cells_click_selection_updates_formula_bar_and_selected_style`
  - `cargo test -q -p boon_native_playground targeted_bound_sync_expands_to_selection_dependent_formula_bar`
  - `cargo test -q -p boon_native_playground input_overlay_render_scene_patch_uses_generic_selected_node_sets`
  - `cargo check -q -p boon_native_playground`
- Remaining no-hacks work: address-shaped fallback helpers still exist for
  older source-intent payloads and must be audited or moved behind explicit
  diagnostic/fallback reporting. Broader runtime/compiler string-coupling and
  aggregate native GPU release gates also remain open.

2026-07-01 generic simple-click selection patch slice:

- The simple source-click hot path now treats generic retained selection patch
  evidence as authoritative. When previous/current selected node sets are
  available from static-equality data bindings, it skips the older
  source-intent address node-discovery pass for selected style sync.
- Added a generic retained selected-node overlay patch helper. It patches
  selected styles from explicit previous/current node sets and records retained
  sync stats without relying on app field names, address source-intent values,
  or text coordinate fallbacks.
- The existing address-shaped retained selected overlay patch remains as a
  compatibility fallback only when generic selected-node evidence is absent.
- Focus overlay state is now refreshed from generic retained selection patches
  even when no selected address payload is available.
- Added direct non-Cells coverage for the retained selected-node patch helper
  with arbitrary `choice-*` nodes.
- Focused verification passed:
  - `cargo test -q -p boon_native_playground retained_selected_node_overlay_patches_generic_node_sets`
  - `cargo test -q -p boon_native_playground cells_click_selection_updates_formula_bar_and_selected_style`
  - `cargo test -q -p boon_native_playground input_overlay_render_scene_patch_uses_generic_selected_node_sets`
  - `cargo test -q -p boon_native_playground focus_only_route_uses_generic_selection_binding_without_address_payload`
- Remaining no-hacks work: the address fallback path still exists and should
  be explicitly reported when used or removed after the verifier no longer
  needs legacy address-payload compatibility. Broader runtime/compiler
  hardcoded field-name audits and aggregate release performance gates remain
  open.

2026-07-01 legacy selection fallback reporting and aggregate gate slice:

- Retained selection overlay sync stats now report
  `selection_overlay_source`, `legacy_selection_fallback_count`, and
  `legacy_selection_fallback_reason`.
- The generic selected-node overlay helper records
  `selection_overlay_source=generic-selected-node-set` and zero legacy fallback
  count. The older address/source-intent compatibility helper records
  `selection_overlay_source=legacy-address-source-intent` with a nonzero
  fallback count, making the fallback visible instead of silently blending into
  the product hot path.
- `verify-native-cells-visible-click-e2e` retained-update contract now fails
  measured samples that use the legacy selection fallback. The fallback remains
  diagnostic compatibility code in the playground, but it is no longer accepted
  for the release visible-click UX proof path.
- `verify-native-gpu-all` now requires the release Cells visible-click E2E
  report at `target/reports/native-gpu/cells-visible-click-e2e-release.json`,
  and the architecture handoff command list includes the same gate.
- Added focused unit coverage for:
  - generic retained selected-node overlay reporting zero legacy fallback;
  - legacy address overlay reporting fallback use;
  - visible-click retained-update contract rejection for legacy fallback;
  - native handoff aggregate requiring the visible-click release report;
  - native label-contract rejection of legacy visible-click selection fallback.
- This still does not complete the full plan. Remaining work includes removing
  or further genericizing the address-shaped compatibility paths, strengthening
  perf-HUD positive gates, broadening active/pending snapshot currentness proof,
  and finishing the generic runtime/list/formula currentness work.

2026-07-01 generic row-lookup payload runtime slice:

- Row-scoped source resolution in `boon_runtime` can now use the typed row
  lookup field from named source payloads before falling back to the legacy
  address payload. This applies to both plan-executor row resolution and the
  generic scheduled runtime path.
- Source-event classification now treats a typed row-lookup payload as row
  context, so a row source with `payload["file"]` can resolve its target row
  without `event.address`.
- Row resolution reports now expose `row_lookup_field`,
  `row_lookup_payload`, and `method=row_lookup_payload` when the typed payload
  path is used.
- Artifact validation no longer requires every `address_lookup_field` route to
  declare an `Address` payload. This keeps row identity metadata separate from
  source payload expressions while preserving the legacy compiler metadata for
  compatibility.
- Added non-Cells runtime coverage with arbitrary `row.file` identity proving a
  source event carrying only `payload["file"]` updates the selected row.
- Focused verification passed:
  - `cargo test -q -p boon_runtime row_scoped_source_resolves_named_lookup_payload_without_address`
  - `cargo check -q -p boon_ir -p boon_runtime`
- This still does not complete the source-identity architecture. The compiler
  still names this metadata `address_lookup_field` and still auto-declares a
  legacy `Address` payload for compatibility. The next slice should rename or
  split that metadata into generic row lookup identity and then remove the
  remaining address-shaped native/playground fallbacks once release verifiers no
  longer rely on them.

2026-07-01 row-lookup metadata alias slice:

- `SourcePayloadSchema`, compiler source-route metadata, runtime source-route
  artifacts, and static program analysis now carry `row_lookup_field` as the
  generic source identity name. `address_lookup_field` remains serialized and
  accepted as a compatibility alias during the transition.
- Runtime route construction prefers `row_lookup_field` and falls back to
  `address_lookup_field`; legacy consumers still see the old field populated so
  existing native/playground paths do not break mid-migration.
- Plan-executor test fixtures were updated with explicit `row_lookup_field:
  None` defaults so schema changes compile in test builds, not just normal
  library builds.
- Focused verification passed:
  - `cargo test -q -p boon_ir scoped_source_lookup_prefers_source_intent_identity_field`
  - `cargo test -q -p boon_runtime row_scoped_source_resolves_named_lookup_payload_without_address`
  - `cargo test -q -p boon_plan_executor --no-run`
  - `cargo check -q -p boon_ir -p boon_plan -p boon_compiler -p boon_runtime -p boon_native_playground`
- This still does not complete the architecture. The old compatibility field
  and address-shaped helper names remain in a few call sites, and the larger
  native UX/frame-loop/runtime-currentness plan still needs implementation and
  release native GPU verification.

2026-07-01 frame-scoped input latency measurement slice:

- `NativeRenderLoopState` now accounts accepted host-input latency once per
  presented input generation. `preview_perf_stats.input_to_present_ms` is fed
  only from the frame that actually presented the accepted input, so later
  timer/proof/burst frames cannot keep inflating an old click's latency.
- Render-loop reports now expose `frame_input_to_present_ms` and
  `input_to_present_accounted_event_wake_count` separately from older
  wake/accept debug timings. Report `preview_perf_stats.input_to_present_ms`
  uses the frame-scoped value instead of recomputing from stale state.
- The Cells visible-click verifier now prefers `frame_input_to_present_ms` for
  `render_loop_input_accept_to_present_ms`, while still recording the legacy
  accept-to-present value for transition diagnostics.
- Added focused coverage for:
  - accepted input latency starts at the role poll hook accept point;
  - accepted input latency is single-use for the presented input frame;
  - serialized render-loop reports carry the frame-scoped value into
    `preview_perf_stats`.
- Focused verification passed:
  - `cargo fmt --check`
  - `cargo test -q -p boon_native_app_window`
  - `cargo check -q -p boon_native_app_window -p xtask`
  - `cargo check -q -p boon_native_playground`
- This improves measurement honesty but does not complete performance. The next
  slices still need retained hot-path work, no-address native input cleanup,
  active/pending snapshot currentness, and fresh release native GPU reports.

2026-07-01 exact pending frame-evidence currentness slice:

- Pending source/runtime/layout/render snapshot commits now require the current
  preview `FrameEvidenceKey` to exactly match the accepted pending snapshot
  frame evidence for surface id, surface epoch, frame sequence, content
  revision, layout revision, render-scene revision, and present id.
- Same-surface newer frames are now rejected with `frame_seq_changed` instead
  of accepted under the old no-regression rule. This makes the commit rule match
  the active/pending contract: a pending snapshot may commit only while it is
  still the current frame snapshot.
- The reported `pending_snapshot_commit_currentness_policy` is now
  `source-revision-plus-exact-frame-evidence`, and `xtask` validates that
  policy string in active/pending backpressure proof.
- Added focused coverage for exact frame-evidence currentness on an advanced
  same-surface frame. Existing stale surface-epoch coverage still proves stale
  frame evidence is rejected before mutating active preview state.
- Focused verification passed:
  - `cargo test -q -p boon_native_playground pending_frame_evidence_commit_requires_exact_current_frame`
  - `cargo test -q -p boon_native_playground replace_source_commit_rejects_stale_surface_epoch_before_state_mutation`
 - `cargo check -q -p boon_native_playground -p xtask`
- This improves active/pending currentness, but layout and render-scene
  revisions are still sourced from the current frame evidence model. The full
  plan still needs independent retained layout/render identities, no-address
  native input cleanup, release native GPU reports, and the full aggregate
  gates.

2026-07-01 generic node/binding selection-proxy refresh slice:

- Selection-proxy focused text refresh now tries stable document node identity
  and cached document text-binding targets before falling back to the legacy
  selected-address path. This keeps formula-bar/text-input refresh generic:
  the native input path does not need to know that a selected item is a
  spreadsheet cell or that the state shape has `/store/selected_address`.
- Retained text refresh also uses selected/focused overlay node identity first,
  so an already-present layout frame can update the focused native text payload
  without rediscovering the target from address-shaped metadata.
- Added focused coverage for state-summary text binding refresh and retained
  text-input refresh with no focused address present.
- Focused verification passed:
  - `cargo test -q -p boon_native_playground selection_proxy_state_summary_refresh_uses_node_text_binding_without_address`
 - `cargo test -q -p boon_native_playground selection_proxy_retained_refresh_uses_selected_node_without_address`
- This still does not complete Cells click latency. The remaining work is to
  remove the leftover selected-address compatibility dependency from the hot
  path, prove click-to-formula-bar timing through native host events and WGPU
  evidence, and finish the retained frame-loop/runtime-currentness slices.

2026-07-01 selection-proxy selected-address fallback narrowing:

- `preview_refresh_selection_proxy_from_state_summary` now applies selected or
  focused document node text bindings before consulting the legacy
  `/store/selected_address` compatibility field. A normal node/binding refresh
  no longer mutates `focused_address` or depends on a spreadsheet-shaped
  selected-address state path.
- Added a regression where the state summary contains a conflicting
  `store.selected_address`, while the selected text-input node has a valid
  generic text binding. The selection-proxy focused text must use the node
  binding and leave `focused_address` unset.
- A read-only subagent audit agreed that selection-proxy focused text is the
  safest next address-shaped hot path to retire, and identified deeper
  follow-ups in source-event routing and retained selected-address style
  fallback.
- Focused verification passed:
  - `cargo fmt --check`
  - `cargo test -q -p boon_native_playground selection_proxy_`
  - `cargo check -q -p boon_native_playground`
- This is still a compatibility-stage cleanup. Address-shaped fallbacks remain
  for older layouts and the larger event dispatch spine still carries address
  payloads; those need a row/node/binding identity migration before the
  no-hacks audit can pass.

2026-07-01 retained bound-text sync no-address fast path:

- Retained bound text-input sync now keeps the address-shaped
  focused-editing-text fallback lazy. If a generic text binding already
  supplies the next retained text value, the sync path no longer resolves
  `address` source-intent metadata just to discard it.
- The retained sync collector now deduplicates text updates by display item, so
  a text input with both state-binding and text-binding index entries is patched
  once instead of queuing duplicate retained text work.
- Added a generic no-address regression where a retained text input updates
  from a cached document text binding with no address metadata present. The
  proof asserts the retained frame text, the single text update, and zero legacy
  selection fallback count.
- Focused verification passed:
  - `cargo fmt --check`
  - `cargo test -q -p boon_native_playground retained_bound_text_sync_uses_text_binding_without_address_metadata`
  - `cargo test -q -p boon_native_playground selection_proxy_`
  - `cargo check -q -p boon_native_playground`
- This removes another avoidable address lookup from retained text syncing, but
  the larger source-event route still carries `address` payload compatibility
  and selected-address overlay fallback remains for older evidence.

2026-07-01 source-event row identity with legacy address payload slice:

- The generic layout-proof and retained hit-route event enrichers no longer
  treat a non-empty `address` payload as a reason to skip stable row identity.
  Legacy address data can still ride along for compatibility, but source-intent
  `list_id`, `target_key`, `target_generation`, `source_id`, and `bind_epoch`
  now remain the primary dispatch identity when the layout/source route exposes
  them.
- Added regressions for both event-enrichment paths using a generic row source
  and a legacy address payload. The assertions prove the row/source identity is
  attached without mutating or discarding the compatibility address.
- Focused verification passed:
  - `cargo fmt --check`
  - `cargo test -q -p boon_native_playground row_identity`
  - `cargo test -q -p boon_native_playground selection_proxy_`
  - `cargo test -q -p boon_native_playground retained_bound_text_sync_uses_text_binding_without_address_metadata`
  - `cargo test -q -p boon_native_playground focus_only_route_uses_generic_selection_binding_without_address_payload`
  - `cargo test -q -p boon_native_playground cells_focus_only_route_syncs_formula_bar_text`
  - `cargo test -q -p boon_native_playground cells_press_only_input_defers_until_release_batch`
  - `cargo check -q -p boon_native_playground`
- This is a routing correctness and hot-path cleanup step, not a completion
  claim. The remaining work is still to prove release-mode native UX latency
  with app-owned host events/WGPU evidence and to finish the retained
  frame-loop, proof identity, runtime currentness, and aggregate gate slices.

2026-07-01 manifest scroll coverage and honest scroll-speed blocker slice:

- `verify-native-gpu-preview-e2e --example cells` now counts current
  real-window/app-owned scroll evidence when checking manifest scenario
  coverage. The collector no longer requires old `operator_*` scroll booleans
  when the scroll-speed report proves stronger `app_owned_window_*`,
  `real_*`, or `real_window_*` wheel input fields.
- This does not weaken the scroll-speed gate. An over-budget scroll-speed
  report still fails; the verifier now reports that as an over-target
  real-window frame-budget problem instead of incorrectly saying the evidence
  is only lower-tier or missing.
- Fresh verification:
  - `cargo fmt --check`
  - `cargo test -q -p xtask manifest_scroll_coverage`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json`
    passed with `scenario_evidence.status=pass`, app-owned WGPU readback,
    real-window input, and `preview_blocked_on_ipc_count=0`.
  - `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`
    still failed honestly: p95 `20.986136999999996ms`, max `22.901203ms`,
    `required_real_window_speed_proven=false`, `runtime_dispatch_count_for_passive_scroll=0`,
    `graph_rebuild_count=0`, `materialized_cell_count_max=336`, and
    `logical_cell_count=2600`.
- The next performance slice should target renderer/present pacing for scroll:
  the fresh report shows no passive runtime dispatch, no graph rebuild, no
  preview IPC blocking, and bounded materialization, so the remaining scroll
  miss is in the frame/present/render budget rather than app-level recompute.

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
