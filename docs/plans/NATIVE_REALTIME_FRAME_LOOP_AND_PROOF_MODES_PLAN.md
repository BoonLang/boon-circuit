# Native Realtime Frame Loop, Proof Modes, And Performance HUD Plan

Status: planned

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

- Keep frame policy small and explicit:
  - `DemandDriven`: idle-efficient mode for idle-wake proof.
  - `RequestedAnimation`: vsync-paced bursts while input, scroll, caret,
    animation, or replay is active, then settle back to demand-driven idle.
  - `ContinuousProbe`: diagnostics/verifier mode only.
- Keep proof as instrumentation, not a frame policy:
  - `Off`, `Counters`, `Trace`, `Proof`.
  - Normal UX budgets exclude WGPU readback completion, JSON report writes,
    verbose trace emission, and proof serialization.
  - Reports split `ux_latency`, `proof_latency`, and
    `instrumentation_overhead`.
- Define frame phases as a scheduler contract:
  - drain host input;
  - coalesce source, selection, scroll, and viewport intents;
  - apply bounded runtime/layout/render-scene work;
  - extract narrow render input;
  - prepare/upload/queue/encode/present;
  - run proof/readback only when instrumentation asks for it.
- Use active/pending frames:
  - the active frame keeps scrolling and selection overlays responsive;
  - pending runtime/layout/render snapshots swap only when complete and current;
  - stale pending snapshots are dropped by epoch and frame sequence.
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
Preview perf  fps 59.8, render 1.3ms, latency 8.4ms, proof off, age 0.9s
```

Add a tiny `PreviewPerfStats` snapshot with fixed scalar fields:

- frame sequence;
- sample time;
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

## Runtime And Compiler Guardrails

Performance fixes must stay generic. Cells is a large fixture and useful stress
case, not a compiler/runtime special case.

- No production branches in `crates/boon_ir` or `crates/boon_runtime` on:
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

## Implementation Slices

1. Documentation and options:
   - add this plan;
   - document `DemandDriven`, `RequestedAnimation`, and `ContinuousProbe`;
   - document instrumentation modes separately from frame policy.
2. Telemetry model:
   - add `PreviewPerfStats`;
   - record fixed-bucket or rolling-window timings without allocation-heavy hot
     path work;
   - add `preview-perf-snapshot` IPC;
   - report telemetry overhead and dropped updates.
3. Dev footer row:
   - render exactly one compact preview performance row from cached state;
   - prove it wraps within the existing footer limits;
   - prove it does not query runtime or block on IPC in render/hot paths.
4. Scheduler:
   - keep `DemandDriven` idle behavior;
   - add requested-animation bursts for active input/scroll/caret/replay;
   - sample host input at frame start;
   - keep proof/readback outside normal UX latency.
5. Active/pending frame state:
   - keep active retained render state available for cheap scroll/selection
     updates;
   - apply pending snapshots only when complete and current;
   - reject stale pending snapshots by epoch/frame sequence.
6. Retained extraction and upload:
   - narrow the extraction from layout/document state to render input;
   - update only dirty render chunks, text runs, transforms, clips, and buffers;
   - report upload bytes, draw calls, cache hits, and materialized visible
     ranges.
7. Generic runtime/compiler audit:
   - add non-Cells fixtures for sparse startup, indexed lookup, exact
     invalidation, currentness barriers, move/remove/reinsert safety, and
     bounded fanout;
   - add an audit test rejecting production compiler/runtime example-specific
     branches.

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

Performance is the main goal. Implement the native preview architecture so normal visible interaction uses fast scheduled realtime bursts, retained/hot renderer state, sparse runtime/layout work, and proof/debug modes that are toggleable and separately measured. Do not treat idle-wake as the main UX benchmark; keep it as a demand-driven smoke gate only.

Use subagents whenever useful for independent architecture, runtime/compiler, WGPU/rendering, testing, or external-library research. If the path starts becoming too complex, too hacky, or circular, stop micro-tuning and choose a simpler generic architecture that matches the source-of-truth docs.

Do not add Cells/example-specific compiler or runtime hacks. No production branches on example names, source paths, cells/address/value/error/A0, or fixture-specific strings. Fix engine/runtime/compiler architecture instead.

Implement the dev-window preview performance row and bounded preview-perf snapshot. Prove visually and functionally with deterministic native tests, app-owned WGPU readbacks, release-mode latency reports, runtime counters, and schema-valid reports. Fix broken/flaky verification infrastructure too when it blocks reliable progress.

Do not claim completion until all required native UX, proof, perf-HUD, generic runtime, and audit gates pass on fresh reports for the current worktree/binary. If blocked, leave the repo coherent and report the exact blocker, evidence, and next implementation step.
```
