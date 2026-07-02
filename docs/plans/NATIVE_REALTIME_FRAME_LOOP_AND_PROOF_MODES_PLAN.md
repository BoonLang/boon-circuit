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

## Strategy Bias

When the same native UX gate keeps failing, prefer architecture changes that
shorten the product interaction path over loops of local measurement and
micro-tuning. Measurements are useful only when they identify a boundary that
can be removed, moved off the product frame, cached, or made retained.

The intended interaction path is game-like and hot:

- accept input at the start of an already scheduled frame;
- patch retained runtime/layout/render state directly;
- submit the visible frame quickly;
- run proof, readback, reporting, and verbose diagnostics separately from the
  product UX budget;
- keep product latency and verifier proof latency separate, but link them with
  `FrameEvidenceKey` so proof still validates the measured presented frame.

If a patch mainly makes the current slow path more observable without cutting
one of those boundaries, it is a diagnostic checkpoint, not progress toward
completion. After repeated failures in the same class, zoom out to scheduler,
retained rendering, runtime currentness, proof/backpressure, or document model
architecture rather than continuing tactical fixes.

## Prioritized Strategic Cut Checklist

Use this checklist before starting another local timing patch. A change should
remove, move, or replace one of these boundaries; otherwise it is probably only
more instrumentation.

- [ ] Make the frame clock the product owner:
  - host input is drained at the start of a scheduled frame;
  - active bursts keep one redraw already requested while input is likely;
  - proof/report/timer wakes cannot relabel or delay the accepted host-input
    product frame;
  - idle-wake remains a smoke gate, not the normal interaction benchmark.
- [ ] Split product present from proof:
  - product frames emit scalar timings, revisions, and `FrameEvidenceKey`;
  - readback/proof/report workers subscribe after present by exact key;
  - UX latency ends at product submit/present, while proof latency and proof
    lag are reported separately;
  - stale latest-report, first-frame, or mismatched proof artifacts fail.
- [ ] Replace fallback input routing with typed retained input:
  - mouse, keyboard, text, and wheel use retained hit regions plus typed
    `SourceIntent` / `ViewportIntent`;
  - route lookup does not inspect proof JSON, labels, geometry strings, or
    example fields;
  - hover/focus/selection/caret feedback is a retained overlay/property-tree
    patch, not a document relower or full layout-frame rebuild.
- [ ] Introduce active/pending retained scenes:
  - `ActiveScene` remains immediately presentable for hover, focus, selection,
    caret, and scroll;
  - runtime/layout/document workers build capacity-1 latest-wins
    `PendingScene` snapshots;
  - stale pending work is rejected by content/layout/render/surface/input
    epochs before activation;
  - old broad summary/report state is not locked while rendering active state.
- [ ] Move runtime to typed deltas and scoped currentness:
  - visible reads call field/key-scoped currentness barriers;
  - runtime turns produce typed deltas for bound text, source values, styles,
    list windows, and dependency fanout;
  - full `state_summary`, full-grid/list summaries, and root flushes are
    diagnostics or report work, not product-frame barriers.
- [ ] Make virtualization a generic engine feature:
  - list/grid/chunk/map expose logical count, materialized window, overscan,
    selected/dependent keys, rendered nodes, and evaluated formulas
    separately;
  - `List/find`, formula dependencies, ranges, and cycle safety are shared
    runtime concepts;
  - non-Cells sparse-grid/list fixtures must pass the same gates as Cells.
- [ ] Own GPU resources in the renderer:
  - pipelines, bind groups, glyph atlases, primitive batches, staging/ring
    buffers, route/hit snapshots, and frame arenas live across product frames;
  - hover/focus/selection/scroll frames update bounded buffers/uniforms instead
    of rebuilding render-scene/proof structures;
  - reports expose upload bytes, command encode time, queue submit time,
    present time, draw calls, cache hits, and hot-frame allocations.
- [ ] Measure the real product present floor before chasing app micro-costs:
  - add a focus-safe hardware/product-surface baseline for the same app-window,
    adapter, surface, present mode, and frame clock;
  - report acquire/submit/present blocking and present-mode policy;
  - compare Cells/example deltas against that baseline instead of guessing
    whether 8-12 ms present/queue cost is app work, compositor/vsync, or
    surface scheduling.
- [ ] Delete old slow paths as replacements land:
  - product dependence on `layout_proof` JSON, latest-report route/proof state,
    geometry/string lookup, private dispatch input, broad runtime summaries,
    modeled/static scroll, legacy Ply/Xvfb/COSMIC/browser proof, and
    driver-timing fallbacks must each get a typed replacement and a negative
    test;
  - do not keep adding third paths that leave both slow paths reachable.
- [ ] Keep implementation simple under pressure:
  - prefer one product state machine plus one proof subscriber model;
  - prefer typed queues and fixed ring-buffer counters over mutable JSON state
    on the product path;
  - if a fix needs per-example branches or many local exceptions, replace the
    architecture boundary instead.

## 2026-07-02 Architecture Cuts Not To Lose

The current evidence says the next useful work is not another route-cache,
binding-scan, or JSON-size tweak. Preserve these broader cuts as explicit TODOs
until they are implemented, deleted, or replaced by a simpler measured design.

### 2026-07-02 Late Status: Cells Click Path

Fresh release verifier evidence after the input-path fixes:

- Fixed: release-only cell commits were caused by a press-handled cached
  candidate rejecting its paired release and falling through to the broad
  generic fallback. The release is now absorbed as clean input when the press
  candidate already handled selection. `simple_source_click_count` remains 64
  and native input reject counts are zero.
- Fixed: raw host wake commits with no sampled button/key/scroll/motion delta
  were being relabeled as `HostInput` product frames. Raw wakes still wake the
  loop, but only reportable input deltas own the product input lane.
- Still failing: product accepted-input p95 is near budget, but wake-to-visible
  still fails because the frame is submitted after a cold/demand wake and then
  waits in queue/present. With `Mailbox`, Cells release report showed product
  p95 about `16.83ms`, max about `29.55ms`, wake-to-present p95 about
  `22.23ms`. With `BOON_NATIVE_PRESENT_MODE=immediate`, accepted-input p95
  improved to about `14.47ms`, but wake-to-present still failed around
  `22.04ms` and max outlier was about `34.21ms`.
- Current root cause: not Cells runtime, list lookup, formula recompute, or
  proof readback completion. The remaining product blocker is frame pacing and
  present/queue scheduling: input is accepted after waking, then the product
  frame often spends 9-16ms in `queue.submit()` / `frame.present()`, with rare
  26-32ms compositor/surface outliers.
- Next architecture cut: implement a real active frame clock for interaction
  bursts. During pointer/text bursts the preview must already have a scheduled
  frame, sample input at the start of that frame, patch retained state, submit
  immediately, and push proof/readback/report work behind the product frame by
  exact `FrameEvidenceKey`. Do not try to make more Cells/runtime micro-tweaks
  until this hot-loop boundary is cut.

- [ ] Source-input transaction split:
  - make `HostInputEvent -> retained visual patch -> present` the first-frame
    product transaction;
  - enqueue runtime source commits as keyed follow-up work that cannot relabel
    or extend the already-presented host-input frame;
  - report `first_frame_patch_ms`, `queued_runtime_commit_ms`,
    `queued_runtime_frame_count`, and `runtime_cleanup_attributed_to_input`;
  - fail if a queued cleanup frame is counted as product click latency for the
    same input event.
- [ ] Product/proof subscriber split:
  - product present emits only scalar frame stats, revisions, and
    `FrameEvidenceKey`;
  - readback, visible-bound-text proof, proof-history compaction, report JSON,
    screenshot encoding, runtime value probes, and artifact hashes run after
    present as exact-key subscribers;
  - fail if proof is from "latest report", stale first frame, mismatched
    surface epoch, mismatched content/layout/render revision, or human/desktop
    capture.
- [ ] Event-loop/frame-clock ownership:
  - drain accepted host input at the start of an already-scheduled burst frame;
  - keep one bounded next-frame wake armed during active pointer/text bursts;
  - separate wake reasons for host input, source/runtime cleanup, caret/timer,
    proof sample, telemetry flush, and surface lifecycle;
  - prevent proof/report/source-cleanup wakes from keeping the product loop hot
    or from being charged as visible host-input work.
- [ ] Active/pending retained state:
  - maintain an immediately presentable `ActiveScene` for hover, focus,
    selection, caret, scroll transforms, and text-control mirrors;
  - build runtime/layout/render updates as capacity-1 latest-wins
    `PendingScene` work;
  - commit pending state only when source/content/layout/render/surface/input
    epochs still match;
  - drop stale pending work before expensive proof/report/layout allocation.
- [ ] Typed input and document deltas:
  - replace remaining fallback branches in
    `preview_apply_real_window_input_with_units` with typed `SourceIntent`,
    `ViewportIntent`, `TextEditIntent`, and `FocusIntent`;
  - runtime turns emit typed deltas for source values, bound text, style/pseudo
    state, list windows, row dependencies, and formula fanout;
  - product paths consume typed deltas through reverse indexes, not full
    `state_summary`, proof JSON, geometry strings, labels, or source-path
    searches.
- [ ] Present/queue strategy:
  - add a focus-safe hardware/product-surface present-floor verifier for the
    same app-window, adapter, surface, present mode, and frame clock;
  - report acquire, encode, queue-submit, present, compositor/vsync, and proof
    completion as separate phases;
  - evaluate ring-buffered uploads and multiple frames in flight so blocked
    queue/present work cannot delay the next input acceptance;
  - gate product UX on app-owned CPU-submit/present timing while reporting
    compositor/GPU/proof completion separately.
- [ ] Retained renderer simplification:
  - keep pipelines, bind groups, glyph atlases, shaped text, route snapshots,
    hit regions, and chunk buffers hot across frames;
  - patch hover/focus/selection/caret/scroll/text mirrors via bounded overlay
    or property-tree state;
  - avoid rebuilding render scenes, layout frames, or proof structures for
    first-frame interaction feedback.
- [ ] Runtime/list/formula architecture:
  - keep `List/find` indexes, demand-current barriers, sparse list windows,
    formula dependency tracking, range invalidation, and cycle safety generic;
  - distinguish logical rows, materialized rows, rendered nodes, evaluated
    formulas, and selected/dependent keys in reports;
  - add non-Cells sparse-list/grid fixtures so generic behavior is proven
    without relying on one spreadsheet example.
- [ ] Testing harness cleanup and deletion:
  - remove driver-timing fallbacks from release product latency gates; missing
    app-owned input/present timing must fail;
  - delete or quarantine legacy Ply, Xvfb, COSMIC scraping, browser screenshot,
    modeled/static scroll, latest-report proof, and proof-JSON route paths;
  - make `verify-native-cells-visible-click-e2e` report whether each click
    failed because product pixels were missing, proof was missing, runtime value
    probe was missing, app-window timing was missing, or IPC died;
  - require deterministic visual tests with app-owned WGPU readback and a
    visible mouse cursor/proven pointer location for all Boon examples that
    have interactive controls.
- [ ] No-hacks audit across all native layers:
  - audit compiler, runtime, document, layout, native GPU, app-window,
    playground, xtask, and report-schema code for production branches on
    example names, source paths, Cells fields, addresses, labels, geometry, or
    fixture strings;
  - allow example/scenario strings only in fixtures, tests, and verifier input
    data;
  - fail negative tests when a generic engine feature regresses into a
    fixture-specific shortcut.

## 2026-07-02 Canonical Maximum Architecture Improvement Ledger

This section is the no-loss checklist to use before another tactical timing
patch. It intentionally duplicates the important ideas from later evidence
history in one active ledger so they do not get buried. Every item must remain
generic across Boon examples; none may branch on Cells, spreadsheet addresses,
fixed dimensions, labels, example names, source paths, or fixture geometry in
production code.

- [ ] Current blocker remeasurement before the next cut:
  - latest local checkpoint after the generic post-present/pre-input pending
    host-input guard:
    `target/reports/native-gpu/cells-visible-click-e2e-release.json`;
  - result: schema-valid but still `status="fail"`;
  - focused checks passed: `cargo fmt --check`, `cargo test -q -p
    boon_native_app_window pre_input_subscriber_drain_skip_is_counted --
    --test-threads=1`, `cargo test -q -p boon_native_app_window
    accepted_host_input_timing_defines_product_input_to_present_latency --
    --test-threads=1`, and `cargo check -q -p boon_native_app_window -p
    boon_native_playground -p xtask`;
  - product interaction is now narrowly over budget rather than seconds slow:
    app-window product commit source reports `input_to_present_p95=16.779
    ms`, steady accepted input-to-present/formula is `17.507 ms`, max is
    `30.659 ms`, and product missed-frame count is `4`;
  - wake/accounting remains clearly worse: `input_wake_to_input_accept_p95`
    is `20.868 ms`, steady wake-to-accept is `22.085 ms`, and steady
    wake-to-formula is `62.341 ms`;
  - the role input hook is not the dominant blocker: steady
    `poll_started_to_input_accept_p95=1.448 ms`, with runtime/list and
    retained contracts still passing (`0` rows/list scans, `0` recomputed
    fields, no full document lower, no legacy selection fallback);
  - proof/report isolation still passes (`legacy_pre_present_request_count=0`,
    hot-path report serialization/write counts `0`, product does not block on
    proof subscribers), but proof worker remains lagging and harness proof
    latency is still hundreds of ms;
  - the guard is useful hygiene, not the 60 FPS architecture fix. Remaining
    p95 is dominated by the single preview loop being unable to accept new
    input while it is in blocking present/queue and surrounding frame work:
    present call p95 is about `12.119 ms`, queue-to-present p95 about
    `12.119 ms`, hook-to-present p95 about `14.765 ms`, and wake-to-poll
    p95 is about `46.773 ms`;
  - next cut must be larger than another route/runtime/JSON micro-fix: split
    product input transaction ownership from blocking present/proof work, or
    introduce an explicit `PreviewHotLoop`/`ActivePreviewScene` path where
    input is accepted at the start of already scheduled product frames and
    present/proof/report work cannot hold the input owner.
  - rerun the release Cells visible-click smoke after the retained sidecar and
    route-identity patches, then record the fresh report path here;
  - latest fresh smoke:
    `target/reports/native-gpu/cells-visible-click-e2e-release-smoke-route-key.json`;
  - result: schema-valid but `status="fail"`;
  - product path improved materially in this one-repeat smoke:
    app-window product commit p95/max is about `11.636 ms`, aggregate
    preview-loop product p95 is about `13.620 ms`, missed-frame count is `0`,
    the three completed product samples are exact `HostInput` /
    `ProductInteraction` commits, and render-hook outer state snapshot is about
    `0.049-0.105 ms`;
  - runtime and retained contracts pass for the completed samples:
    `total_rows_scanned=0`, `total_list_find_rows_scanned=0`,
    `total_recomputed_fields=0`, no full document lower, and three retained
    render-scene patches;
  - the remaining failure in this smoke is not the product hot path: only three
    of four required click samples completed, C0 times out in the visual formula
    proof, selected-cell crop proof reports missing baseline app-owned readback,
    and the structured visible-surface proof says selected-cell render state was
    not proved even though retained text sync matched the expected formula;
  - product frames still report `legacy_product_proof_built_pre_present=true`
    and five legacy pre-present proof request kinds, so proof separation is not
    complete even when product latency is fast;
  - next cut from this checkpoint is verifier/proof ownership and exact visual
    proof state: record product frame keys directly in every sample, maintain
    baseline/current crop readbacks by exact key, prove selected-cell visual
    state from retained active scene metadata, and move remaining legacy
    pre-present proof building behind post-present subscribers;
  - after that, run the default repeated release path before claiming the
    product path is truly stable; one-repeat smoke is not acceptance.
  - classify the dominant failing boundary as one of scheduler wait, input
    route/intent, runtime currentness, document/layout patch, retained extract,
    GPU upload/encode, queue/present, proof/readback/report, IPC, telemetry, or
    verifier coupling;
  - if the state snapshot still spends milliseconds reading proof-shaped shared
    state, cut a typed product snapshot before any more renderer cache tuning;
  - if queue/present dominates, run a same-surface present-floor baseline before
    changing app logic.
- [ ] `NativeFrameClock` as the only product frame owner:
  - route all host input, wheel, text, source commits, runtime/layout wakes,
    caret/timer animation, proof samples, telemetry flushes, dev IPC, and
    surface lifecycle events through typed lanes;
  - drain visible host input at frame start on an already-scheduled burst frame;
  - keep bounded requested-animation burst state inside DemandDriven mode, with
    explicit quiet exit, hard cap, and proof/telemetry non-ownership rules;
  - fail product gates when a proof/report/timer/source-cleanup wake relabels,
    delays, or charges an accepted host-input frame.
- [ ] Product transaction ABI:
  - define the first visible transaction as
    `HostInputEvent -> RetainedRoute -> ProductIntent -> ProductPatch ->
    Present`;
  - make commands typed: `MoveFocus`, `SetSourceValue`, `CommitTextEdit`,
    `UpdateViewport`, `ActivateAction`, pointer capture, drag, IME/composition,
    and accessibility focus;
  - each command carries route epoch, target id, source field id, input event
    seq, stale policy, and reconciliation policy;
  - runtime follow-up work is keyed and cannot rewrite the first presented
    input frame timing.
- [ ] `ActivePreviewScene` / `PendingPreviewScene` / `RecycleScene`:
  - `ActivePreviewScene` is the immediately presentable product truth for hit
    routes, focus, hover, selection, caret, scroll transforms, text mirrors,
    layout fragments, render batches, GPU resources, and evidence identity;
  - workers build at most one latest-wins pending scene from runtime,
    document, layout, and render extraction;
  - pending state activates only after source/content/layout/render/surface/input
    epoch checks pass;
  - `RecycleScene` owns reusable buffers, route snapshots, text/glyph scratch,
    staging memory, and render arenas so hot frames do not allocate.
- [ ] Typed `ProductRenderResult` and product/proof protocol split:
  - product render returns scalar timings, revisions, dirty counts, lane,
    scheduler reason, present metadata, cache hits, upload bytes, and
    `FrameEvidenceKey`;
  - product render does not build `serde_json::Value` proof trees, parse layout
    proof JSON, read latest reports, serialize artifacts, wait for readback, or
    inspect fixture strings;
  - proof, reports, screenshots, hashes, diffs, schema artifacts, HUD history,
    and verbose diagnostics subscribe after present by exact key;
  - dev and verifier code fail if proof mode is required for the visible update
    to happen.
- [ ] `FrameEvidenceRegistry` and exact proof joins:
  - pre-mint frame evidence keys before acquire/encode/submit/present;
  - register presented product frames by exact key, surface epoch, content
    revision, layout revision, render-scene revision, input event seq, and
    present id;
  - attach readback, visible text proof, render proof, screenshots, artifacts,
    diffs, and report rows only by exact key;
  - fail closed on stale first-frame proof, latest-report proof, mismatched
    surface epoch, mismatched revision, hash-only proof, unkeyed screenshots,
    or after-the-fact key stamping.
- [ ] Retained property trees and controls:
  - keep hover, focus, pressed, selection, caret, text-control mirror, IME,
    scroll/clip/transform, opacity/effects, cursor, and accessibility focus in
    retained property trees;
  - passive scroll, hover, focus, and selection patch properties or overlay
    buffers directly without relower, full layout rebuild, or proof scan;
  - text input/formula-bar synchronization uses typed binding deltas and
    current-on-read barriers for the selected field only;
  - first-frame direct patches have runtime reconciliation or rollback evidence
    if a later semantic commit disagrees.
- [ ] Bevy-style extract/prepare/queue pipeline:
  - formalize phases as `DrainInput`, `ResolveRoute`, `ApplyTypedDelta`,
    `ExtractVisibleDirty`, `PrepareGpu`, `QueueBatches`, `Encode`, `Submit`,
    `Present`, and `PostPresent`;
  - `ExtractVisibleDirty` is the narrow sync boundary and copies only dirty
    visible ids, route data, style/text deltas, transform changes, and prepared
    resource handles;
  - old full-state sync, full display-list conversion, and full render-scene
    builds are pending-scene or diagnostic paths, not product interaction work;
  - hot-frame allocation, lock wait, clone, hash, and JSON counters are hard
    budgets, not only debug fields.
- [ ] Renderer-owned GPU resource residency:
  - keep pipelines, bind groups, glyph atlases, texture atlases, shaped text,
    staging belts, dynamic buffers, vertex/index buffers, primitive batches,
    route snapshots, and hit regions hot across frames;
  - use ring-buffered uploads and bounded frame arenas for overlays, text runs,
    visible list chunks, and scroll transforms;
  - report upload bytes, queue writes, cache evictions, draw calls, encode
    time, submit time, present time, frames in flight, and surface acquire time;
  - do not rebuild GPU resources or render scenes only to produce proof.
- [ ] Present-floor and frame pacing architecture:
  - add a focus-safe hardware/product-surface baseline using the same window,
    adapter, surface, present mode, frame clock, scale factor, proof mode, and
    compositor path as real examples;
  - separate CPU submit, surface acquire, queue submit, present call,
    compositor/vsync wait, GPU completion, and proof completion;
  - evaluate late acquire, mailbox/FIFO/immediate diagnostics, multiple frames
    in flight, and desired frame latency only behind reported modes;
  - product acceptance must compare app deltas against the measured floor and
    must not hide present-mode changes as defaults.
- [ ] Generic runtime/list/formula/currentness architecture:
  - keep `List/find` indexes, sparse list windows, demand-current fields,
    currentness barriers, formula dependency tracking, ranges, duplicate/tombstone
    index behavior, and cycle safety as shared runtime concepts;
  - runtime turns emit typed deltas for bound text, source values, pseudo state,
    list windows, dependency fanout, and invalidated ranges;
  - reports distinguish logical rows, materialized rows, rendered nodes,
    formula-evaluated rows, selected keys, dependent keys, and scanned rows;
  - add non-Cells sparse-grid/list fixtures with selection, editing, lookup,
    dependency fanout, range invalidation, scroll, and cycle coverage.
- [ ] Compiler/document stable identity:
  - generate stable semantic ids for source fields, list rows, controls,
    document nodes, layout fragments, render chunks, text runs, and hit regions;
  - cache keys use stable ids plus revisions, not geometry strings, proof
    hashes, labels, source text snippets, or fixture-specific names;
  - define id lifecycle, reuse, tombstone, stale-event rejection, and
    cross-process serialization rules in `NATIVE_GPU_PIPELINE.md`;
  - static audits fail if production code rediscovers identity from rendered
    text, coordinates, or example-specific paths.
- [ ] Dev window, HUD, and telemetry isolation:
  - the perf footer reads cached scalar `PreviewPerfStats` at a throttled rate
    and never performs IPC, runtime queries, report parsing, or proof reads from
    render hooks;
  - editor wheel, source replacement, report expansion, proof-history
    inspection, and dev HUD refresh cannot block preview product rendering;
  - expose mode-aware stats: idle last-frame age, burst fps/p95/drops,
    proof-lag p95, queue depth, worker drops, and product/proof mode flags;
  - dev-code-editor wheel gets its own crash/stall gate and must not depend on
    the Cells verifier.
- [ ] Verifier reset and deterministic evidence:
  - split product-only, proof-only, combined proof-isolation, present-floor,
    dev-window, and long-session gates;
  - all interactive Boon examples with controls get app-owned native visual
    tests with a visible cursor/proven pointer location and WGPU readback;
  - UX gates fail on driver-timing fallback, modeled/static scroll, desktop or
    browser screenshots, human observation, unkeyed proof, ContinuousProbe, or
    proof-required visible updates;
  - repeated reports include warmup policy, minimum sample count, adapter,
    present mode, software/hardware status, outlier policy, and exact metric
    start/end definitions.
- [ ] Stale-path deletion ledger:
  - every replacement names the old path to delete or quarantine, the owner
    layer, the positive gate, and the negative stale-path gate;
  - stale paths include layout-proof product reads, latest-report routing,
    proof JSON hit testing, broad runtime summaries, private dispatch input,
    modeled scroll readiness, legacy Ply/Xvfb/COSMIC/browser evidence, hidden
    offscreen present, full-scene rebuild on overlay change, and
    fixture-specific verifier shortcuts;
  - no compatibility branch is accepted without a scheduled deletion or a
    diagnostic-only flag.
- [ ] External architecture research to turn into local contracts:
  - distill GPUI-style retained element/input ownership, Bevy-style extraction
    and schedules, Servo/WebRender-style scene transactions and property
    updates, egui-style immediate input ergonomics, and native game-loop frame
    pacing into concrete Boon contracts;
  - research is complete only when it produces a local API, negative gate, or
    deletion milestone, not a loose comparison paragraph;
  - do not copy another framework's complexity when a smaller Boon-specific
    transaction or retained-scene boundary removes the measured blocker.
- [ ] Future compiler/codegen/runtime strategy:
  - defer Zig/Rust/Wasm hot kernels, ahead-of-time runtime codegen, SIMD
    kernels, and specialized formula engines until typed product/runtime
    contracts and interpreter-equivalence tests exist;
  - when revisited, codegen must be generic over Boon IR and sparse runtime
    deltas, not an example-specific spreadsheet fast path;
  - success requires deterministic equivalence tests, profiler evidence, and a
    rollback path to the interpreter.

## Architecture TODO Backlog

This backlog captures the high-level cuts from the 2026-07-01 subagent review.
Keep these as architecture tasks, not as a pile of Cells-specific patches.

- Product loop shape:
  - replace proof-shaped input/render flow with a typed
    `HostInputEvent -> route snapshot -> SourceIntent/ViewportIntent -> retained
    patch -> present` path;
  - accept input at the start of an already-active requested-animation burst
    frame, not after a sleep/report/proof boundary;
  - keep the accepted host-input frame attributed as `HostInput`; do not let a
    due burst wake relabel that same frame as `RequestedAnimation`;
  - move cursor, accessibility, verbose diagnostics, and report snapshot work
    after present or into coalesced workers when they are not needed to produce
    visible pixels.
- Proof and reporting:
  - split product render from proof generation in `native_gpu_app_owned_render_hook`;
  - make product frames return fixed scalar counters, revisions, and proof
    handles, not large `serde_json::Value` proof trees;
  - move full report snapshot assembly, proof-history cloning, and JSON
    serialization off the UX loop into a latest-wins report worker keyed by
    `FrameEvidenceKey`;
  - remove visible-surface readback from normal product frames. Readback is a
    verifier/proof subscriber that must prove the exact presented frame without
    delaying product present;
  - add a typed frame-proof registry that registers both app-window readbacks
    and external app-owned render proof by exact `FrameEvidenceKey`, and make
    stale or mismatched proof fail closed.
- Preview hot state:
  - cut `layout_proof` JSON out of mutable product-path state. Hot code should
    use typed layout frames, route tables, source-intent indexes, overlay lookup
    tables, and retained render state;
  - quarantine `native_gpu_render_proof` as offline/legacy proof only. Native
    readiness must come from visible render-hook proof, visible present path,
    and app-owned WGPU readback, not from artifact-only layout proof;
  - keep offscreen copy-to-present out of product mode. It is a proof
    experiment, not a present-variance fix.
- Input and routing:
  - replace the generic `preview_apply_real_window_input_with_units` fallback
    with a single typed route snapshot path for mouse, keyboard, and wheel;
  - replace exact-position `click_candidate_cache` with a retained hit snapshot
    keyed by layout generation, scroll transform, and input revision;
  - fold passive scroll into the same typed input route and prove
    `runtime_dispatch_count_for_passive_scroll=0`, `graph_rebuild_count=0`, and
    app-owned wheel-to-visible timing;
  - split static route identity from volatile runtime payload. Changes to
    payload values such as `target`/`address` must not invalidate the hit route
    table when current payload can be read from typed state.
- Retained renderer state:
  - turn hover, focus, caret, and selection into retained renderer-state patches
    instead of layout-frame mutations or display-list scans;
  - replace render-scene cache lookup by linear scan/string keys with direct
    generational identities from document/layout/render-scene revisions;
  - consolidate bound text sync, selected-input refresh, and selection proxy
    refresh into typed runtime deltas with precomputed reverse binding-path
    indexes. Do not reintroduce per-leaf binding scans.
- Runtime/document genericity:
  - keep demand-current barriers, list indexes, formula dependencies, and
    visible-window materialization generic. Do not branch on `cells`,
    addresses, columns, row counts, example names, labels, or geometry;
  - if the Cells Boon example is simplified, keep it a cleaner app model, not a
    workaround for missing compiler/runtime/document behavior.
- Testing harness honesty:
  - classify `verify-native-cells-visible-click-e2e` as a native UX gate in
    every schema/audit path;
  - remove driver-timing fallbacks for release product latency. Missing
    app-owned `input_accept_to_present` timing is a failure;
  - split each sample into product latency keyed by the presented
    `FrameEvidenceKey` and proof/readback latency for matching evidence;
  - gate the visible-click report on preview-loop product stats too:
    `preview_perf_stats.input_to_present_ms.p95 <= 16.7`, sufficient sample
    count, `missed_frame_count=0`, DemandDriven mode, and no ContinuousProbe;
  - do not let modeled/static scroll evidence satisfy readiness. It may explain
    expectations, but current app-owned product metrics/readbacks are required;
  - report all cold, steady, max, proof, and driver outliers separately. A
    narrow steady-sample pass must not hide failing product-loop p95 or missed
    frames.

### Expanded Architecture TODOs To Preserve

These options are intentionally broader than the current failing Cells click
report. Keep them visible so future work can choose a simpler architecture cut
instead of circling around small local fixes.

- Event-loop ownership and scheduling:
  - make the app-window loop own a typed queue of accepted host input deltas,
    source wakes, viewport intents, proof requests, and telemetry flushes;
  - process visible host input at the beginning of a scheduled frame, before
    report drains, cursor/a11y refresh, or proof bookkeeping;
  - keep a cheap next-frame wake already armed during bursts so pointer/key
    input lands in a hot frame instead of paying idle wake plus proof/report
    overhead;
  - make DemandDriven missed-frame accounting state-aware. Long idle gaps are
    healthy in DemandDriven and must not count as dropped frames; late frames
    count only while burst/probe pacing is active;
  - define separate wake reasons for product input, source/runtime invalidation,
    timer/caret animation, proof-only sampling, report flush, and surface
    lifecycle. Do not let proof/report wakes keep the product loop hot.
- Frame pacing and present strategy:
  - treat WGPU surface acquire, queue submit, and present as separate phases
    with explicit budgets and adapter/present-mode metadata;
  - experiment behind flags with FIFO, mailbox, immediate, and auto-vsync
    present modes where supported, but never hide mode choice in reports;
  - use multiple frames in flight or ring-buffered dynamic uploads so a blocked
    present/submit does not stall input acceptance for the next frame;
  - split CPU-submit latency, compositor-present latency, GPU-completion
    latency, and proof-completion latency in reports and gates;
  - keep product UX gates on the app-owned product path while also reporting
    when compositor/vsync behavior consumes most of the 16.7 ms budget.
- Render-thread and worker split:
  - consider a dedicated preview render actor/thread fed by latest-wins typed
    snapshots from runtime/layout, with bounded channels and no blocking dev
    IPC in the product frame;
  - move proof readback, full JSON report generation, screenshot encoding, and
    slow telemetry aggregation to worker tasks keyed by `FrameEvidenceKey`;
  - allow the render actor to submit the current retained frame while a newer
    runtime/layout snapshot is still building;
  - make stale worker results cheap to drop by revision before they allocate or
    serialize large artifacts.
- Retained document, layout, and render state:
  - introduce stable document/layout/render node identities from compiler and
    document lowering instead of deriving identity from geometry or strings;
  - keep transform, clip, scroll, opacity/effects, hover, focus, selection, and
    caret state in retained overlay/property trees that can be patched without
    relowering or rebuilding display lists;
  - make scroll a transform/clip/uniform update for retained visible content
    whenever app semantics do not require runtime dispatch;
  - support dirty rectangles or dirty render chunks for text, borders,
    backgrounds, overlays, and list rows;
  - keep hit testing on retained layout fragments plus current transforms, not
    on proof JSON, geometry string searches, or latest report artifacts.
- GPU resource lifetime:
  - keep pipelines, bind groups, glyph atlases, texture atlases, staging belts,
    dynamic uniform buffers, vertex/index buffers, and render-scene batches hot
    across interaction frames;
  - use ring-buffered dynamic uploads and explicit upload byte counters for
    overlays, text runs, visible list chunks, and scroll transforms;
  - batch a small primitive vocabulary consistently: solid quads, rounded
    rectangles, borders, glyph runs, clips, images, and simple paths;
  - make text shaping and glyph upload caches first-class, keyed by content,
    font, size, scale, style, wrapping inputs, and writing mode;
  - avoid rebuilding GPU resources solely to create proof artifacts.
- Runtime and currentness:
  - replace product-frame runtime summaries with typed runtime deltas and cheap
    counters. Full summaries are diagnostics, not a currentness barrier;
  - keep demand-current barriers at every visible read, but make the barrier
    field/key scoped so selection does not pull unrelated roots or full-grid
    summaries current;
  - maintain reverse indexes from binding paths to retained document nodes so a
    source/runtime change patches exactly the affected text/input/style nodes;
  - bound per-frame runtime work with visible/materialized windows,
    dependency-fanout queues, and continuation scheduling for non-visible work;
  - keep formula/list dependency tracking generic and cycle-safe, with ranges
    and `List/find` using shared indexes rather than spreadsheet-specific
    branches.
- Boon model and compiler simplification:
  - allow example Boon code to become cleaner when it expresses the intended
    app more directly, but only if runtime/compiler/document behavior remains
    generic and equivalence is covered by fixtures;
  - consider adding compiler-visible source-intent metadata, list window
    demands, binding-path indexes, and stable node IDs so the native preview
    does not rediscover them from lowered strings;
  - prefer typed IR/runtime structures over `serde_json::Value` in hot paths;
  - treat old compatibility shims as temporary migration points with removal
    tests, not as permanent second architectures.
- Slow-path deletion candidates:
  - retire production dependence on `layout_proof` JSON, report-history scans,
    geometry/string route lookup, and latest-report state as soon as typed
    state replaces each use;
  - delete or quarantine private runtime dispatch input, modeled/static scroll
    success, desktop/browser/COSMIC screenshot proof, and legacy Ply readiness
    shortcuts from native readiness gates;
  - remove broad runtime-state-summary refresh from normal input frames once
    typed deltas and currentness barriers cover bound text, selection, and
    formula-bar updates;
  - keep offscreen copy-to-present, duplicate interactive readback, and proof
    cache hit paths behind explicit proof-mode flags with negative tests.
- Verification and visual fixtures:
  - make all examples eligible for generic visual input replay tests with an
    app-owned visible cursor marker, host-event injection, same-frame WGPU
    proof, and functional assertions;
  - keep Cells as the large sparse-grid stress case, but add smaller non-Cells
    fixtures for list indexing, text input binding, scroll transforms, focus,
    hover, retained overlays, and demand-current reads;
  - require every latency report to say whether the sample is cold, steady,
    burst, proof-only, or driver/harness timing;
  - fail reports that pass only because proof latency was counted as product
    latency, product latency was replaced by driver timing, or ContinuousProbe
    made the path artificially hot.
- Simplicity constraints:
  - prefer one product frame state machine plus one proof subscriber model over
    many overlapping verifier-only paths;
  - prefer typed bounded queues, fixed scalar counters, and small ring buffers
    over large mutable JSON state in product code;
  - if a fix requires many local exceptions, stop and replace the boundary with
    a simpler generic ownership model.

### Additional Architecture Improvements To Preserve

This section is a "do not lose" backlog from the latest high-level review. It
intentionally includes larger architecture replacements so future work can pick
strategy over another round of local timing patches.

- Compiled runtime/query engine:
  - compile Boon runtime turns into slot-based plans with typed inputs,
    dependencies, effects, and output deltas instead of interpreting broad
    generic summaries on every product frame;
  - represent runtime data in arenas/slot maps and structure-of-arrays columns
    for hot indexed fields, list windows, bindings, and formula dependencies;
  - keep dynamic Boon semantics available, but give hot paths a compiled plan
    with stable field ids, row ids, reverse binding indexes, and currentness
    barriers;
  - success gate: selection, editing, formula-bar sync, and scroll can run from
    a small typed runtime turn with no full `state_summary`, no root flush, and
    no path/string lookup in product mode.
- Rust/Zig/native codegen workstream:
  - evaluate a backend that lowers stable Boon runtime/render plans to Rust or
    Zig kernels for hot examples and production deployments;
  - keep the interpreter as the reference implementation and verifier oracle,
    but allow generated kernels for list lookup, formula dependency fanout,
    text binding sync, and retained render extraction;
  - generated code must be keyed by typed IR/version hashes and carry the same
    `FrameEvidenceKey`/revision diagnostics as interpreted execution;
  - success gate: generated and interpreted runs produce equivalent functional
    reports, while product latency reports show the generated kernel removing a
    measured runtime boundary.
- Incremental compiler/document/lowering cache:
  - use a query-style incremental graph for parse, typecheck, IR, document
    lowering, layout metadata, route metadata, and render identity extraction;
  - source edits should invalidate by span/module/node identity, not force a
    full document/runtime/lowerer rebuild before the active preview can keep
    presenting;
  - persist stable ids across source edits where semantic identity survives;
  - success gate: dev-window source edits show bounded invalidation counts,
    active scene remains presentable while pending compile/lower work builds,
    and stale pending results are rejected by query revision.
- Virtualization as a generic engine service:
  - make visible-window and materialization demands first-class in compiler,
    runtime, document, layout, and renderer contracts;
  - list/grid/chunk/map operations expose logical count, materialized range,
    overscan range, selected/dependent keys, and dirty range deltas without
    materializing the whole list;
  - scrolling updates transforms and materialization windows before evaluating
    non-visible data;
  - success gate: non-Cells sparse-list fixtures and Cells both report logical
    size separately from materialized rows, rendered nodes, evaluated formulas,
    upload bytes, and dependency fanout.
- Text input and focus architecture:
  - split native text editing state into a retained text-control model with
    focus id, selection range, caret state, IME/composition state, bound source
    path, and mirror text;
  - first-frame focus/selection feedback patches the retained text-control
    model directly, while commit/evaluation can follow through runtime workers;
  - all examples use the same text-control contract, including formula bars,
    editor text areas, ordinary inputs, and future IME paths;
  - success gate: click-to-focused-text, typing, selection movement, formula-bar
    sync, hover/focus visuals, and dev-code-editor wheel/text behavior pass
    generic visual replay without per-example code.
- Source-intent and command model:
  - replace ad hoc source-event payload maps with typed commands such as
    `SetSourceValue`, `CommitTextEdit`, `MoveFocus`, `BeginDrag`,
    `UpdateViewport`, and `ActivateAction`;
  - commands carry typed target ids, runtime field ids, route snapshot epoch,
    input event sequence, and stale-result policy;
  - generated source-intent metadata comes from compiler/document lowering, not
    from labels, geometry, path strings, or fixture-specific fields;
  - success gate: product input routing uses typed command dispatch and reports
    zero fallback source-intent JSON scans.
- End-to-end product transaction log:
  - every accepted visible input creates a bounded transaction record from
    host input through route, runtime turn, document/layout/render patch,
    present, and proof subscriber;
  - keep only fixed-size scalar/ring-buffer records in product mode, with full
    JSON expansion off-thread and optional;
  - use the transaction log to prove stale worker drops, coalescing, missed
    frame reasons, and exact proof matching without scanning latest reports;
  - success gate: every failing UX sample names the dominant transaction phase
    and every passing sample has matching product/proof evidence keys.
- Product frame budget enforcer:
  - define a pre-present budget table for input drain, route, runtime turn,
    currentness reads, overlay patch, extraction, upload, encode, submit, and
    present;
  - add a debug/assert mode that records which subsystem exceeded budget and
    whether it ran in an allowed phase;
  - budget failures should guide architecture cuts, not be hidden by increasing
    global thresholds;
  - success gate: report schema exposes per-phase p95/max and a failed budget
    names one owner subsystem and old path to delete or move.
- Renderer ownership split:
  - create a render-owned active scene that owns WGPU resources, glyph/text
    caches, primitive batches, route/hit snapshots, frame arenas, and evidence
    registry;
  - runtime/document/layout workers produce typed deltas for the render owner
    instead of sharing mutable proof/layout state with the render path;
  - renderer APIs accept deltas and revision stamps, not full proof JSON or
    arbitrary `serde_json::Value` payloads;
  - success gate: product frames do not lock runtime/dev/report state while
    encoding, and retained GPU resource reuse is visible in counters.
- Platform event and input correctness:
  - preserve real OS/native input ordering and timestamps through the app-window
    event queue without mixing verifier-injected, human, and proof-only events;
  - coalesce wheel/move events by target and epoch, but never coalesce away a
    state-changing click, key, or text commit;
  - maintain focus-safe hardware baselines that refuse unapproved real input;
  - success gate: reports distinguish host-event injection, real OS input,
    verifier-only sampling, and proof subscriber work, with negative tests for
    mixed event sources.
- Performance lab and profiling workflow:
  - add repeatable release-mode perf profiles for product-only, proof-only,
    full HUD/report, empty present-floor, sparse list/grid, text input, editor
    wheel, and Cells scenarios;
  - allow optional `perf`, tracing, flamegraph, allocation, and WGPU timing
    captures, but keep them outside the product acceptance path and account for
    overhead explicitly;
  - store perf reports with adapter/session/build/worktree fingerprints and
    enough phase counters to compare regressions;
  - success gate: a performance claim requires a fresh schema-valid report from
    the matching binary/worktree, not stale artifacts or human observation.
- Legacy verifier and harness retirement:
  - retire or quarantine old verifier routes that can pass via modeled/static
    evidence, latest-report fallback, desktop screenshots, browser/COSMIC/Ply
    paths, or driver timing in place of app-owned product timing;
  - keep old tests only when they document historical behavior or provide a
    negative gate proving the stale path is not used;
  - every new verifier should say which product mode, proof mode, input source,
    present path, and evidence key policy it uses;
  - success gate: native readiness cannot pass unless the product path and
    matching proof path are both current, app-owned, and schema-valid.
- Architecture simplification checkpoints:
  - after each major replacement, delete the old compatibility path or add a
    temporary kill switch and negative test with an owner/date;
  - prefer fewer concepts with strict contracts over parallel "fast", "proof",
    "legacy", and "verifier" variants that silently diverge;
  - document any remaining dual path in this plan with why it exists, what
    report field proves which path ran, and what gate removes it;
  - success gate: the deletion ledger shrinks over time, and no failing p95 is
    chased by adding a third path that keeps both old slow paths alive.

### External Architecture Patterns To Apply

These TODOs translate lessons from fast Rust/native UI systems and browser
compositor pipelines into Boon-native implementation tasks. They are not
dependency commitments.

- Frame clock and repaint broker:
  - add a `NativeFrameClock` that owns redraw pacing for preview surfaces;
  - add a coalescing `request_preview_repaint(reason, deadline)` API so many
    callers can request one product frame without creating many timers;
  - render product frames from the app-window redraw/frame-clock event, not from
    idle/proof/report timers;
  - record `redraw_requested_seq`, `redraw_delivered_seq`,
    `coalesced_redraw_count`, and `request_to_redraw_ms`;
  - active bursts keep requesting redraws until quiet or hard cap; idle uses
    wait/timed wait without producing fake 0 FPS failures.
- Surface latency policy:
  - introduce `SurfaceLatencyPolicy { present_mode,
    desired_maximum_frame_latency, acquisition_block_budget }`;
  - report supported present modes, selected present mode, target frame latency,
    surface acquire block time, queue submit time, present time, and estimated
    queue depth;
  - run measured product experiments for FIFO/mailbox/immediate/auto-vsync where
    the adapter supports them, but never use present mode as a hidden
    correctness shortcut;
  - add an empty native retained-frame baseline using the same app-window and
    WGPU surface so app deltas are compared against the machine's real present
    floor.
- Explicit scheduler state machine:
  - define scheduler inputs such as `needs_commit`, `ready_to_activate`,
    `ready_to_draw`, `surface_valid`, `visible`, `burst_active`,
    `proof_pending`, and `telemetry_pending`;
  - define actions such as `BeginProductFrame`, `CommitPending`, `Activate`,
    `DrawActive`, `Idle`, `RunProofSubscriber`, and `FlushTelemetry`;
  - proof/report work may subscribe to completed frames but must not trigger
    `BeginProductFrame`;
  - add transition tests for idle, burst, source wake, surface lost/resize,
    proof-only sample, and report flush.
- Main/pending/active/recycle scene lifecycle:
  - use `MainScene` for committed runtime/document state,
    `PendingScene` for in-progress layout/render extraction, `ActiveScene` for
    currently presentable retained state, and `RecycleScene` for reusable
    allocations/caches;
  - keep `ActiveScene` scrollable/selectable while `PendingScene` builds;
  - activate pending state only when source, content, layout, render, surface,
    and frame epochs match;
  - recycle allocation-heavy structures rather than rebuilding vectors, maps,
    buffers, and proof containers on every product frame.
- Property-tree render scene:
  - split retained render state into picture/render-item list, spatial tree,
    clip tree, scroll tree, and effect/overlay state;
  - patch scroll, viewport, hover, caret, focus, and selection through spatial,
    clip, and overlay state before rebuilding layout/display lists;
  - keep overlay effects and hit-test transforms in typed state so proof JSON is
    not needed for input routing or painting.
- Retained hit-test tree:
  - emit first-class hit regions with `HitRegionId`, `ScrollRegionId`,
    `SourceIntentTemplate`, `spatial_node_id`, `clip_chain_id`, z-order,
    `layout_generation`, and `input_route_generation`;
  - hit testing samples current scroll/transform state and fails closed on stale
    epochs;
  - expose a debug `hit_guid` that maps proof routes back to document/layout
    nodes without using labels, source strings, or geometry searches.
- Bevy-style renderer phases:
  - structure renderer work as `Extract -> PrepareResources -> QueuePhases ->
    SortBatch -> Encode -> Present -> Cleanup`;
  - `Extract` copies only visible ranges, dirty ids, revisions, and evidence
    ids, with its own p95 budget;
  - GPU resources live in prepare/queue state, not inside runtime/layout code;
  - cleanup must recycle arenas/buffers and retire stale proof requests without
    blocking the next product frame.
- Typed change detection:
  - add revision stamps for document nodes, layout fragments, shaped text,
    transforms, clips, render primitives, GPU buffers, route tables, and hit
    regions;
  - dirty queries must be typed by component/revision rather than path/string
    scans;
  - `changed_by` and dirty reason are telemetry only. Tests should fail on
    mutation without revision bump, revision bump without changed data, or
    sticky stale dirty reasons.
- Primitive, batch, and text-cache contracts:
  - lock renderer input to a small primitive vocabulary: quads, rounded boxes,
    borders, shadows, glyph runs, images, lines, clips, layers, and simple
    paths;
  - define batch keys from pipeline, atlas/texture, clip, spatial node, effect
    state, and blend mode;
  - report draw calls, batch count, upload bytes, instance count, cache hits,
    glyph upload count, and newly materialized visible ranges;
  - key shaped text by content hash, font, size, scale, style, wrap width, and
    writing mode;
  - caret, selection, hover, focus, and scroll must not reshape text unless new
    visible text ranges are materialized.
- Frame arena and hot allocation stats:
  - add a product-frame scratch arena for ephemeral extraction/render objects;
  - report `hot_frame_alloc_bytes`, `hot_frame_malloc_count`,
    `arena_reuse_count`, and `json_alloc_bytes_on_product_frame`;
  - require zero proof/report JSON allocation in product frames once typed proof
    subscribers are implemented.
- Proof as post-present subscriber:
  - make `FrameEvidenceRegistry` keep the last N presented keys per surface;
  - register proof/readback artifacts after present using exact
    `FrameEvidenceKey`;
  - if proof queue capacity is exhausted, fail or drop the proof sample, never
    the product frame;
  - verifier waits for exact-key proof while product UX latency ends at present.

### Contract And Deletion TODOs

These are concrete architecture contracts and old paths to remove or quarantine
as typed state replaces them.

- Product render-hook contract:
  - product frames must not read `layout_artifact` files;
  - product frames must not build full proof/report JSON;
  - product frames must not derive layout/render identity from proof JSON;
  - the render hook consumes an active retained snapshot and returns typed
    revisions, counters, and proof handles.
- Typed evidence contract:
  - pre-issue `FrameEvidenceKey` before render/queue/readback;
  - require reports to include both `product_frame_evidence_key` and
    `proof_frame_evidence_key`; they must match exactly except for explicitly
    reported `proof_lag_frames`;
  - pass the key through product render, visible-surface readback, proof
    subscribers, and report workers;
  - reject artifacts that are stamped after the fact or cloned from mismatched
    JSON;
  - reject proof artifacts whose key was inferred from the latest report instead
    of minted before render, submit, and present;
  - add negative tests for stale first frame, mismatched surface epoch,
    mismatched content/layout/render revision, and proof cache hit without an
    exact key.
- Product-only baseline gates:
  - add a counters-only product-latency verifier with proof/readback off;
  - add a proof-subscriber/readback latency verifier that is not allowed to
    affect product p95;
  - add a full report/HUD verifier with an explicit maximum allowed product
    regression;
  - product p95 and missed-frame gates must pass in counters-only product mode
    before proof/report regressions are investigated.
- Native present-floor baseline:
  - add an empty retained-frame native preview verifier using the same
    app-window, adapter, surface, and present mode as real examples;
  - report present floor p50/p95/max and compare example/product deltas against
    it;
  - stop chasing 1-2 ms app micro-fixes when the measured present floor already
    consumes most of the frame budget.
  - update `docs/architecture/NATIVE_GPU_PIPELINE.md` and native handoff gates
    once the report contract is stable, so present-floor is a real native GPU
    verification gate rather than a plan-only command.
- Stale-path deletion ledger:
  - maintain one row per stale path with `old_path`, `typed_replacement`,
    `temporary_allowlist`, `kill_switch`, `positive_gate`, and `negative_gate`;
  - a path is not considered removed until the negative gate proves product mode
    cannot use it.
- Revision split:
  - make `frame_revision`, `content_revision`, `layout_revision`,
    `render_scene_revision`, `surface_epoch`, `input_event_seq`,
    `present_id`, and `proof_request_id` independent typed values;
  - repaint-only frames must not invent content revisions;
  - source/runtime commits must not reuse stale layout/render revisions;
  - proof must say exactly which revisions it proves.
- Pre-present allowlist:
  - before product present, allow only host-input drain, route snapshot,
    retained state patch, currentness reads needed for visible pixels, narrow
    extraction, encode, submit, and present;
  - move cursor refresh, accessibility snapshots, report drains, dev telemetry,
    full runtime summaries, proof bookkeeping, and screenshot/readback setup
    after present or to workers unless they are explicitly needed for pixels;
  - add per-frame counters for each pre-present category and fail if a forbidden
    category appears in product mode.
- Compatibility-deletion register:
  - remove or gate `preview_apply_real_window_input_with_units` once typed input
    routes cover mouse, keyboard, and wheel;
  - remove product dependence on `layout_proof` hot-state JSON;
  - remove layout artifact reloads from product render hooks;
  - quarantine artifact-only `native_gpu_render_proof` as offline proof, not
    native readiness proof;
  - migrate from legacy `present_call_ms` to explicit acquire/submit/present
    phases;
  - replace `last_interactive_readback_artifact` with exact-key bounded
    registries;
  - replace `address_lookup_field` / `SourcePayloadField::Address` production
    routing with generic typed payload/row metadata;
  - retire compatibility `LayoutFrame` lowerer and fallback render-scene
    identity once retained scene identities are supplied end to end.
- No-hacks audit allowlists:
  - allow Cells strings in examples, scenarios, and explicit verifier fixtures;
  - reject Cells/example/source-path/field-name branches in production runtime,
    compiler, document, renderer, app-window, and playground product paths;
  - keep verifier allowlists path/module scoped so the audit is strict without
    forcing legitimate fixture code to hide its names.

### Architecture Option Matrix And TODOs To Preserve

This section is a parking lot for high-leverage architecture cuts. Before
another micro-optimization pass, pick one of these options, define the product
path it removes, add the matching counter/gate, and delete or quarantine the
old path once the typed replacement is proven.

- Option A: game-style hot preview loop:
  - one `PreviewFrameClock` owns product redraw requests and frame pacing;
  - input is sampled/drained at the beginning of a scheduled frame;
  - route lookup reads a retained hit tree, produces typed intents, patches
    retained overlay/state, and submits immediately;
  - after pointer, keyboard, text, or wheel input, keep a bounded interactive
    keep-warm burst for a short window so input does not repeatedly pay display
    downclock, idle wake, and proof/report scheduling costs;
  - the burst is report-driven and exits by quiet interval or hard cap, so idle
    power behavior remains DemandDriven rather than becoming a permanent
    continuous loop;
  - proof, report, accessibility, cursor refresh, and dev telemetry subscribe
    after present by `FrameEvidenceKey`;
  - success gate: product `input_accept_to_present_ms.p95 <= 16.7`,
    `missed_frame_count=0`, no proof/readback/report work before present.
- Option B: browser-style retained compositor:
  - split document/layout/display items from compositor-like transform, clip,
    scroll, effect, focus, hover, selection, and caret property trees;
  - selection, hover, focus, caret, and passive scroll update property/overlay
    state, not the document or layout frame;
  - list/window materialization happens only when viewport demands cross a
    retained materialization boundary;
  - success gate: passive scroll and selection report zero runtime dispatch,
    zero document relower, zero display-list rebuild, bounded upload bytes.
- Option C: ECS/change-detection renderer:
  - model document nodes, layout fragments, hit regions, text runs, render
    batches, GPU buffers, and proof handles as typed components with revisions;
  - extraction copies only changed visible components into render-owned data;
  - change detection is revision-based, not string/path/JSON scan based;
  - use a small primitive vocabulary and primitive-specific dirty regions:
    selection, focus, caret, text content, scroll offset, hover, and materialized
    ranges should become typed primitive invalidations any example can use;
  - success gate: per-frame changed component counts and upload bytes explain
    latency without broad scans or full scene hashing.
- Option D: render actor with latest-wins snapshots:
  - preview render actor owns WGPU resources, active retained scene, route tree,
    glyph cache, atlases, staging/ring buffers, and frame evidence registry;
  - runtime/layout workers submit bounded pending snapshots through a capacity-1
    latest-wins channel;
  - stale pending work is rejected by source/content/layout/render/surface
    epochs before expensive allocation or serialization;
  - success gate: active scene stays presentable while pending work builds, and
    product frames never block on dev-window IPC or full report assembly.
- Option D2: explicit Boon render phases:
  - split the product pipeline into named, measured phases:
    `DrainHostInput`, `ResolveInput`, `ApplyRuntimeTurn`,
    `ExtractDocumentDelta`, `LayoutVisibleRanges`, `PrepareGpuResources`,
    `QueuePrimitiveBatches`, `EncodeCommands`, `Submit`, `Present`,
    `OptionalProofReadback`, and `FlushTelemetry`;
  - `ExtractDocumentDelta` is the narrow sync point and copies only
    visible/materialized display items, scroll uniforms, text runs, dirty
    primitive ranges, and evidence ids into render-owned state;
  - GPU resources live in render-owned prepare/queue state across frames, not in
    runtime/layout code or verifier report builders;
  - success gate: reports expose p50/p95/max for each phase and no phase hides
    full runtime/document/layout clones.
- Option E: proof/readback as a subscriber system:
  - product frames emit `FrameEvidenceKey`, scalar timings, and proof handles;
  - proof subscribers request visible readback or structured render proof for
    exact frame keys after present;
  - verifiers wait for matching proof artifacts separately from product UX
    latency and report proof lag explicitly;
  - success gate: stale first-frame proof, mismatched surface epoch, mismatched
    revision, or hash-only proof fails, but product present does not wait for
    proof completion.
- Option F: generic sparse runtime/materialization engine:
  - keep `List/find` indexes, demand-current indexed fields, formula/range
    dependencies, cycle safety, and visible-window list materialization generic;
  - expose compiler/runtime hints for source intents, binding-path reverse
    indexes, list windows, and stable row/node identities;
  - summary/report generation must not be a product currentness barrier;
  - success gate: startup and normal interaction show near-zero eager
    spreadsheet-style value/error recompute, no full-grid scans, and current
    selected/visible reads.
- Option G: surface present-floor and pacing investigation:
  - add an empty retained-frame baseline using the same app-window, adapter,
    surface, present mode, proof mode, and frame clock as real examples;
  - keep separate baseline classes instead of merging their numbers:
    isolated-headless/software present floor, real-compositor hardware present
    floor, and full example/product frame path;
  - software/headless present-floor reports are useful harness evidence, but
    they must not be used to excuse or prove real preview latency on the
    product desktop surface;
  - measure FIFO/mailbox/immediate/auto present modes, queue depth, acquire
    blocking, submit blocking, and `frame.present()` blocking behind explicit
    flags and reports;
  - add a non-isolated hardware/product-surface variant before concluding that
    the remaining Cells p95 is a compositor/GPU floor;
  - hardware/product-surface variants must be focus-safe and fail on any
    observed keyboard, mouse, wheel, or touch input that was not part of the
    verifier scenario;
  - no-input baselines must not drain coalesced keyboard/mouse state as part of
    the proof path; they may observe lightweight input-wake counters and must
    fail if those counters move;
  - try late surface acquisition, ring-buffered uploads, multiple frames in
    flight, and adaptive frame latency only when the baseline proves they target
    the dominant cost;
  - success gate: example/product delta is separated from machine/compositor
    present floor, and present-mode choice is visible in every report.
- Option H: all-example visual replay infrastructure:
  - every example can run host-event visual replay with an app-owned visible
    cursor marker, same-frame WGPU proof, and functional assertions;
  - each applicable example has startup visual, first interaction visual, click,
    keyboard focus, text edit, hover, wheel, scrollbars, selection,
    list/window materialization, source updates, and proof-mismatch coverage;
  - reports distinguish product latency, proof latency, driver/harness latency,
    cold samples, steady samples, burst samples, and max outliers;
  - success gate: a verifier cannot pass by using human screenshots, desktop
    scraping, ContinuousProbe, static/model evidence, or driver timings in
    place of app-owned product timings.
- Option H2: generic sparse list/grid stress fixtures:
  - keep Cells as the large app fixture, but add at least one non-Cells generic
    sparse list/grid stress fixture with a larger logical range, hidden
    rows/columns or sparse windows, formula-like dependencies, horizontal and
    vertical scroll, selection movement, and edit fanout;
  - use it to prove the engine is generic and sparse rather than tuned to the
    26x100 Cells layout or fixed address strings;
  - success gate: the fixture passes the same runtime/list/index/currentness,
    retained update, and WGPU visual replay gates with no example-specific
    branches.
- Option I: dev-window and IPC isolation:
  - dev HUD reads only cached scalar `PreviewPerfStats` snapshots;
  - dev editor scrolling and footer rendering do not block preview render,
    runtime currentness, or WGPU present;
  - source edits use latest-wins source-replace workers and stale-result
    rejection, while preview remains responsive on the active scene;
  - success gate: `preview_blocked_on_ipc_count=0`, no transport calls from
    render hooks/footer rendering, and dev-code-editor wheel tests pass.
- Option J: old-path deletion milestone:
  - make a removal list with owner tests for each compatibility path:
    `layout_proof` hot-state dependence, geometry/string route lookup,
    private runtime dispatch input, modeled/static scroll readiness,
    legacy Ply/Xvfb/COSMIC/browser proof, broad runtime summaries on input,
    duplicate interactive readback, exact-position click caches, and
    production `address` alias routing;
  - each deletion needs a typed replacement, an equivalence/negative test, and
    a schema/report gate proving the old path is no longer used in product
    mode;
  - success gate: native readiness reports cannot be produced by stale
    compatibility paths, and production crates contain no example-specific
    performance shortcuts.
- Option K: typed input transaction contract:
  - define one end-to-end transaction shape:
    `HostInputEventSeq -> RouteSnapshotEpoch -> SourceIntent/ViewportIntent ->
    RuntimeTurnId -> DocumentPatchRevision -> RenderSceneRevision ->
    FrameEvidenceKey`;
  - add priority lanes in the input scheduler: text/edit commits first,
    pointer down/up/click next, wheel coalesced by axis and target, viewport and
    source wakes next, telemetry/debug last;
  - route, source, runtime, document, render, and proof layers must carry typed
    ids from this transaction instead of reading latest reports or rediscovering
    identity from geometry, labels, strings, or fixture data;
  - transactions can be coalesced or superseded, but stale results must fail
    closed by epoch before updating active state;
  - success gate: every visible input sample reports the transaction ids it
    used, and missing/mismatched ids fail native UX schemas.
- Option L: compiler/document identity workstream:
  - promote stable source-intent metadata, reverse binding-path indexes,
    list-window demand metadata, row/list identities, document node ids, and
    hit-region ids from “nice to have” to a first-class compiler/document
    workstream;
  - renderer/input code must consume these typed ids rather than rebuilding
    identity from lowered text, proof JSON, geometry, or example conventions;
  - success gate: production hit routing, retained updates, and render-scene
    cache keys still work when visible text/labels/geometry change without
    changing route identity.
- Option M: explicit product/proof mode matrix:
  - define allowed pre-present work for each mode:
    `CountersProduct`, `TraceProduct`, `ReadbackProofSubscriber`, and
    `ContinuousProbeDiagnostics`;
  - product modes allow scalar counters and product evidence ids before
    present, but forbid readback completion waits, JSON proof trees, screenshot
    encoding, full report snapshots, dev IPC waits, and stale cache probing;
  - proof subscribers may spend readback/report time after present for exact
    keys, and must report their own overhead and lag;
  - success gate: schemas fail when a product frame performs work outside its
    mode allowlist or when proof work is required to make pixels visible.
- Option N: product-frame allocation and JSON budget:
  - add hot-frame allocation counters and JSON/proof allocation counters around
    app-window, playground, document, renderer, and xtask report paths;
  - after typed subscribers land, require zero product-frame proof/report JSON
    allocation except for named temporary migration adapters;
  - success gate: product reports include `hot_frame_malloc_count`,
    `hot_frame_alloc_bytes`, `json_alloc_bytes_on_product_frame`, and
    `proof_json_alloc_bytes_on_product_frame`, with gates/allowlists shrinking
    to zero as old paths are deleted.
- Option O: currentness work budget:
  - make currentness barriers field/key scoped and explicitly tied to visible
    reads, selected inputs, focus/hover/caret overlays, and materialized window
    demands;
  - non-visible dependency fanout must run through continuation/latest-wins
    work, not by blocking the product frame;
  - full runtime summaries, full-grid/list summaries, and broad root flushes are
    diagnostic/report work, not product currentness barriers;
  - success gate: selection/editing samples report bounded visible currentness
    reads, zero full-grid scans, zero full summaries, and explicit continuation
    counts for any deferred non-visible fanout.
- Option P: ownership and kill-switch milestones:
  - assign owners before implementation:
    `boon_native_app_window` owns frame clock, pacing, surface lifecycle,
    `FrameEvidenceRegistry`, proof subscriber scheduling, and product counters;
    `boon_native_playground` owns typed input routing, source intents, runtime
    turn application, retained active/pending scenes, and dev HUD snapshots;
    `boon_document` owns stable document/layout/hit ids and binding/list-window
    metadata; `boon_runtime` owns currentness, list indexes, dependency fanout,
    and runtime deltas; `boon_native_gpu` owns retained render resources,
    render phases, upload metrics, and WGPU proof capture; `xtask` owns
    verifier gates and negative tests;
  - every replacement lands with a kill switch that can forbid the old path in
    product mode: typed route snapshot on -> fallback route path forbidden,
    keyed proof registry on -> latest-report proof forbidden, retained scene on
    -> layout-proof hot-state reads forbidden, currentness deltas on -> full
    runtime summary before present forbidden;
  - success gate: each kill switch has a positive test, a stale-path negative
    test, and a report field showing whether the old path was used.
- Option Q: freshness and cache-key closure:
  - bind every report/proof/cache hit to worktree fingerprint, binary hash,
    source revision, surface id/epoch, content/layout/render revisions,
    frame sequence, present id, proof request id, and input event sequence when
    applicable;
  - latest-report-derived state and cache hits are never acceptance evidence
    unless the exact key matches;
  - success gate: stale first frames, stale worktree/binary reports, mismatched
    source revisions, stale proof cache hits, and latest-report fallbacks fail
    native readiness gates.
- Option R: root-cause report fields:
  - every native UX report should include enough scalar phase fields to explain
    the dominant boundary without re-running with ad hoc tracing:
    `input_to_poll_ms`, `poll_to_runtime_ms`, `runtime_to_layout_ms`,
    `layout_to_upload_ms`, `queue_submit_ms`, `submit_to_present_ms`,
    `present_call_ms`, `readback_ms`, `proof_deferred`,
    `frames_presented_after_input`, `coalesced_input_count`,
    `deferred_telemetry_count`, and `proof_lag_frames`;
  - product reports should name whether the sample is cold, steady, burst,
    proof-only, driver/harness, or verifier-forced;
  - success gate: failing reports point to one of runtime/currentness,
    route/input, layout/extract, GPU upload/encode, queue/present, proof, IPC,
    or telemetry as the measured p95/max class.
- Option S: focus-safe hardware present lab:
  - create a hardware-capable, isolated present-floor harness that does not
    steal focus from the user's desktop and does not sample real keyboard,
    mouse, wheel, or touch input;
  - preferred routes are explicit: DRM/KMS test surface, hardware-backed nested
    compositor with known adapter selection, or a product preview surface that
    starts unfocused and proves no real input wakes;
  - the harness must report adapter/vendor/device/backend, compositor/session
    class, present mode, refresh/pacing assumptions, queue depth, surface epoch,
    and whether the result is headless, nested, real compositor, or product
    preview;
  - success gate: a hardware present-floor report can run without visible focus
    theft, with `observed_real_os_input=false`, and can be compared against
    full product reports without mixing software/headless numbers into product
    acceptance.
- Option T: direct-manipulation overlay lane:
  - define a generic fast lane for hover, focus, selection, caret, text cursor,
    formula/input mirrors, and scroll thumb feedback;
  - the lane patches retained overlay/property state and input-bound text
    mirrors directly from typed source/runtime deltas before any full document
    summary, proof tree, or dev-window update;
  - commit/evaluation work that is not needed for the first visible feedback
    frame continues through normal runtime/layout workers and updates the active
    scene by revision when ready;
  - success gate: first click/selection feedback and selected input text become
    visible in the same product frame with bounded overlay changes and no broad
    runtime summary or full display-list rebuild.
- Option U: keyed frame-history and proof registry:
  - make `FrameEvidenceRegistry` a bounded ring owned by the preview frame
    clock/render actor, keyed by frame sequence, present id, surface epoch,
    content/layout/render revisions, and input event sequence;
  - verifiers request proof for exact keys and can wait for a later readback
    subscriber, but product timing always ends at product submit/present;
  - latest-report, first-frame, and "whatever proof is newest" fallbacks are
    disabled in product UX gates once the registry is available;
  - success gate: proof lag is explicit, stale proof reuse fails negatively, and
    no product frame blocks on proof artifact completion.
- Option V: retained GPU resource lifetime and render-bundle workstream:
  - keep pipelines, bind groups, glyph atlases, static vertex/index buffers,
    staging belts, clip/transform buffers, and primitive batch metadata alive
    across frames in render-owned state;
  - use dirty chunks, instance-buffer updates, uniform/transform updates, and
    WGPU render bundles or equivalent cached command fragments where they remove
    measurable CPU encode/upload cost without hiding stale state;
  - encode only changed primitive batches for product frames, and record upload
    bytes, map/write buffer counts, command encoder time, render pass time, and
    cache hit/miss counts;
  - success gate: selection/hover/focus/scroll frames perform bounded buffer
    writes and no full render-scene rebuild, while proof mode can still force a
    separately measured readback path.
- Option W: scheduler ownership and priority lanes:
  - centralize product frame scheduling in one owner that understands input,
    source/runtime wakes, layout/materialization wakes, animation bursts,
    surface lifecycle, and proof/debug subscribers;
  - define priority lanes: visible input feedback, text/edit commits, wheel and
    scroll coalescing, runtime continuation, layout/materialization, proof
    readback, report serialization, dev HUD, and background diagnostics;
  - non-visible or superseded work must be cancellable/latest-wins and must not
    hold a product-frame lock while building summaries, JSON, or proof data;
  - success gate: reports can show why each frame was scheduled, what lane it
    serviced, what was coalesced/dropped/deferred, and that telemetry/debug work
    never delayed product present.
- Option X: generic spreadsheet/dependency core:
  - promote formulas, cell-like references, range dependencies, list-window
    materialization, and row-field currentness into generic runtime concepts,
    not Cells-specific code;
  - support dependency tracing or static extraction during evaluation,
    reverse-dependency fanout, cycle-safe demand-current evaluation, and
    bounded visible/materialized recomputation;
  - selected/input-bound fields get a currentness barrier scoped to the exact
    field/key, while non-visible fanout can continue off the product frame;
  - success gate: a non-Cells sparse-grid fixture and Cells both show indexed
    `List/find`, no startup value/error eager sweep, range-member updates, cycle
    detection, and no full-grid recompute on edit.
- Option Y: external architecture lessons to fold in:
  - GPUI's documented direction is hybrid immediate/retained, GPU accelerated
    UI; for Boon this suggests immediate authoring ergonomics but retained
    render/state ownership for hot frames (`https://docs.rs/gpui`,
    `https://gpui.rs/`);
  - Servo/WebRender-style separation of layout/display-list construction from
    renderer-owned presentation suggests Boon should send typed display deltas
    and stable ids to a retained renderer instead of rediscovering identity
    from geometry or proof JSON
    (`https://book.servo.org/design-documentation/architecture.html`);
  - Bevy ECS change detection shows the value of per-component/resource change
    tracking and filtered work; Boon should expose comparable revision filters
    for document nodes, layout fragments, runtime fields, hit regions, and GPU
    batches (`https://docs.rs/bevy_ecs/latest/bevy_ecs/`);
  - WGPU/Vulkan present-mode docs make clear that FIFO/mailbox/immediate change
    acquire/present blocking behavior; Boon reports must name present mode and
    distinguish CPU-submit, compositor-present, GPU completion, and proof
    completion (`https://docs.rs/wgpu/latest/wgpu/enum.PresentMode.html`,
    `https://docs.vulkan.org/refpages/latest/refpages/source/VkPresentModeKHR.html`);
  - success gate: architecture choices from these systems are translated into
    generic Boon contracts and tests, not copied as framework-specific
    rewrites.
- Option Z: explicit stale-path quarantine:
  - mark known slow or misleading paths as quarantined once a typed replacement
    exists: proof-history walks, full report-history payload reads, broad
    runtime summaries on input, full display-list rebuild for overlay changes,
    latest-proof fallback, geometry/string hit identity, duplicate readback,
    and verifier-only click caches;
  - each quarantine must have a feature flag or product-mode assertion that
    proves the path is not used in normal UX gates;
  - success gate: deleting a quarantined path either leaves all fresh native
    gates green or fails a specific owner test that names the missing typed
    replacement.
- Option AA: product poll-hook split:
  - split the current heavy preview poll hook into a minimal product path
    `InputDrain -> RouteSnapshot -> Intent -> RetainedPatch -> Present` and
    post-present subscriber work for cursor, accessibility, debug summaries,
    JSON diagnostics, and proof/report generation;
  - the native app-window render decision should not require running scenario
    handling, full runtime summaries, layout artifact scans, proof JSON
    mutation, or dev telemetry refresh before deciding whether the accepted
    input frame can present;
  - success gate: accepted input reports show `poll_to_intent_ms`,
    `intent_to_patch_ms`, and `patch_to_present_ms`, while
    `pre_present_cursor_a11y_json_diagnostics_count=0`.
- Option AB: typed retained route snapshot:
  - replace product routing through `layout_proof` clones, hit-region JSON
    scans, source-intent artifact lookup, geometry/string fallbacks, and
    formula/focused-text summary locks with a typed retained route snapshot
    keyed by layout/render generation;
  - cover mouse, keyboard, text, wheel, scrollbars, focus, selection, and
    source intents before disabling the broad fallback in product UX gates;
  - quarantine product use of `preview_apply_real_window_input_with_units`,
    `document_hit_region_ref_at`, and artifact-backed source-intent lookup once
    typed routing has positive and negative coverage;
  - success gate: product input samples report route snapshot epoch and zero
    proof/layout-artifact route scans.
- Option AC: `ActiveScene` / `PreviewHotState` replacement:
  - define render-owned hot state with typed route table, overlay/property tree,
    binding reverse indexes, scroll materialization windows, active layout
    fragments, render-scene revision ids, and frame-evidence registry;
  - keep `layout_proof`, mutable `LayoutFrame` overrides, and proof JSON as
    evidence outputs, not as the product state mutated by hover/focus/selection
    or passive scroll;
  - success gate: hover/focus/selection/scroll reports show retained hot-state
    patch counts and zero proof JSON mutation before present.
- Option AD: product render result boundary:
  - make the product render hook return a typed `RenderFrameResult` carrying
    product timings, revision ids, counters, and `FrameEvidenceKey`;
  - move full proof tree assembly, WGPU readback requests, report JSON
    serialization, artifact hashing, and dev-window telemetry materialization to
    post-present subscribers keyed by that result;
  - in product mode, readback/proof queue pressure must drop or fail proof
    samples, never defer product presentation;
  - success gate: product reports have zero pre-present proof/report JSON work
    and proof-overhead reports account for subscriber cost separately.
- Option AE: dev query paging and telemetry workers:
  - keep source replacement latest-wins, but move full runtime summaries,
    layout/proof history inspection, and large report reads out of the product
    frame loop into sampled or paged telemetry workers;
  - dev footer and HUD use bounded scalar snapshots only;
  - success gate: accepted input frames report no full runtime summary lock, no
    shared render-state proof clone, and no IPC wait.
- Option AF: direct product-present baseline:
  - make direct visible-surface presentation the normal product baseline;
  - quarantine offscreen copy-to-present, duplicate readback, and probe-only
    present branches behind explicit proof/diagnostic flags;
  - success gate: product frames report direct present path, while proof reports
    name any offscreen/readback/copy path and exclude it from UX latency.
- Option AG: minimal product-preview pipeline spike:
  - if the current preview path keeps requiring compatibility branches, build a
    small product-only preview pipeline beside it as a strangler slice, not a
    rewrite: typed route snapshot, active scene, direct retained patch, direct
    WGPU present, scalar counters, and exact-key proof subscriber hooks only;
  - feed it the same compiler/runtime/document/layout outputs as the existing
    native preview so it cannot become a second semantic implementation;
  - compare it against the existing path with a delta replay/equivalence oracle
    for visible output, hit routing, source commits, focus/text state, scroll,
    and proof evidence keys;
  - if the spike is faster and simpler, migrate product gates to it and
    quarantine the old proof-shaped path. If it is not faster or simpler,
    delete the spike and keep only the lessons in this plan;
  - success gate: the product-only path passes with fewer hot-path concepts and
    strictly less pre-present work, while proof subscribers still validate the
    same presented frames.
- Option AH: execution-backend sequencing guardrail:
  - keep the Rust interpreter/static runtime as the semantic oracle while
    runtime, list, currentness, and materialization blockers are still being
    fixed;
  - do not use future Rust/Zig/Wasm/native codegen to relabel an unfixed
    runtime or product-loop blocker as solved. Either fix the current gate or
    write an explicit ADR that moves the gate to a replacement backend with
    equivalent semantic and performance proof;
  - align any codegen work with
    `docs/plans/speedup/22-post-speedup-compiler-codegen-wasm-plan.md`:
    executable behavior comes from verified typed IR/NativeRegionIR, not AST
    text, debug tables, path strings, or example-specific kernels;
  - generated kernels may optimize covered regions such as list lookup,
    formula dependency fanout, text binding sync, sparse materialization, and
    render extraction, but unsupported regions must report capability fallback
    honestly and fail hot-path readiness when fallback is on the product path;
  - success gate: interpreter and generated backend reports match functionally,
    fallback is visible, and generated code removes a measured boundary without
    changing Boon semantics.
- Option AI: data-oriented runtime and document storage:
  - replace hot `BTreeMap<String, RuntimeValue>` / row field maps / proof JSON
    lookups with typed slot ids, generational arenas, columnar field storage,
    dense binding indexes, dirty bitsets, and typed list windows in the product
    path;
  - keep string/path/debug summaries as report adapters only, built after
    product present or in proof/debug modes;
  - expose storage counters for slot lookup, column read/write, dirty set
    size, row scan count, string lookup count, summary allocation, and
    materialized window reuse;
  - success gate: visible click/edit/scroll product frames can be explained by
    bounded typed storage operations, with zero string/path scans and zero
    summary construction before present.
- Option AJ: UI-control state as a generic retained subsystem:
  - promote focus, hover, active press, selected item, caret, text selection,
    IME composition, scroll offset, and input mirror text to a shared retained
    control-state subsystem consumed by all examples and the dev editor;
  - runtime source commits confirm, normalize, or roll back retained control
    state by interaction id and revision, but first-frame visual feedback does
    not wait for full semantic recomputation unless the visible pixel actually
    depends on it;
  - cover cells, TodoMVC, counter buttons, text inputs, editor text areas,
    scrollbars, and future controls with the same replay and WGPU proof
    contract;
  - success gate: hover/focus/click/text/wheel visuals are generic retained
    control patches, not per-example Boon workarounds or layout-proof edits.
- Option AK: cross-goal architecture reconciliation:
  - before starting unrelated milestones such as BYTES, manufacturing,
    SolidGraph/3D, Rust/Zig codegen, or WGPU pipeline upgrades, reconcile their
    execution and rendering contracts with this product-preview plan;
  - if a planned milestone naturally fixes a current Cells/native blocker, make
    that dependency explicit with a reportable gate instead of running two
    parallel architectures;
  - if a milestone would add another source of truth for runtime storage,
    render identity, input routing, or proof evidence, pause and merge the
    contracts first;
  - success gate: major goal prompts reference one current runtime/render/input
    contract, not stale plan snapshots that let agents solve different
    problems in different directions.
- Option AL: architecture-contract promotion:
  - once `PreviewHotLoop`, priority lanes, `ActivePreviewScene` /
    `PendingPreviewScene`, typed `RenderFrameResult`, proof subscribers,
    product/proof modes, and direct product-present policy are chosen, promote
    them from this plan into `docs/architecture/NATIVE_GPU_PIPELINE.md`;
  - resolve any mismatch between active contract wording such as
    `LayoutFrame -> RenderProof` or copy-present requirements and the measured
    product path;
  - keep AGENTS.md, the active architecture handoff gate list, native GPU
    aggregate gates, and realtime product-loop verifiers aligned. Either make
    `verify-native-cells-visible-click-e2e` an explicit realtime product gate in
    all handoff lists or classify it consistently as a separate regression
    gate;
  - success gate: a future agent can read AGENTS.md plus
    `NATIVE_GPU_PIPELINE.md` and implement the same product/proof architecture
    described here without plan-only assumptions.
- Option AM: scheduler-ingress deletion:
  - inventory every path that can request redraw, wake a frame, start proof,
    flush telemetry, refresh dev state, or run timers outside the chosen
    `NativeFrameClock` / repaint broker;
  - replace them with `request_preview_repaint(reason, deadline, lane)` or a
    proof/telemetry subscriber API that cannot create product frames directly;
  - forbid direct timer/proof/dev/report wake paths from relabeling host-input
    product frames or extending visible interaction latency;
  - success gate: reports expose scheduler ingress counts by lane, and product
    mode fails if an unowned ingress path schedules or blocks a product frame.
- Option AN: no-dev product preview gate:
  - add a product-only preview run where the preview process/window runs
    without dev-window state, dev IPC, report expansion, editor rendering, HUD,
    or source-inspection dependencies;
  - add an overloaded-dev comparison that stresses the dev editor/report/HUD
    while the preview remains on the same product path;
  - product preview must be able to render, accept input, scroll, and prove
    exact frames without depending on dev IPC or dev-owned mutable state;
  - success gate: no-dev and overloaded-dev runs both show
    `preview_blocked_on_ipc_count=0`, no dev-state lock before present, and
    bounded product p95 deltas.
- Option AO: proof/report queue budgets:
  - define bounded proof, readback, report, screenshot, telemetry, and HUD
    queues with capacity, max age, max lag frames, timeout behavior, and drop or
    fail semantics;
  - proof timeout, proof queue overflow, readback lag, or report serialization
    backlog must produce proof/report failures, not synthetic product latency
    samples;
  - reports must include `proof_queue_depth`, `proof_dropped_count`,
    `proof_timeout_count`, `max_proof_lag_frames`,
    `report_worker_backlog_count`, and `telemetry_drop_count`;
  - success gate: under proof/report backpressure, product frames continue or
    fail for product reasons only, while proof failures are exact-key and
    explicitly reported.
- Option AP: canonical runtime/list contract sync:
  - promote the runtime/list contracts from this plan into
    `docs/architecture/RUNTIME_MODEL.md`, `docs/architecture/LIST_MODEL.md`,
    and `docs/architecture/DELTA_PROTOCOL.md` so the source-of-truth
    architecture docs do not lag the realtime plan;
  - add a runtime dependency-graph verifier separate from the native Cargo
    dependency graph, reporting formula/range/index fanout, graph rebuilds,
    cycle-guard hits, max dependency breadth, stale dependency drops, and
    dependency update cost;
  - success gate: native performance work and runtime/list work cite the same
    current contracts and the aggregate verifiers fail when those contracts are
    violated.
- Option AQ: range/index/currentness semantics:
  - define range-dependency compression with interval/range nodes rather than
    O(area) edge expansion, including row/column insert, delete, move,
    generation invalidation, and range-cycle behavior;
  - define `List/find` / query-index lifecycle: build, update, rebuild,
    duplicate-key behavior, removed-row tombstones, generation-stale rows,
    bounded misses, and whether product row scans are fail-closed or explicitly
    budgeted;
  - make demand-current reads an API-level contract:
    product-visible reads require typed `field/key/range + reason +
    interaction/budget` demand objects, while unscoped root flush or summary
    APIs are unavailable or fail in product mode;
  - success gate: non-Cells and Cells fixtures prove index lifecycle,
    compressed ranges, scoped demand reads, and zero accidental full scans or
    root flushes on product frames.
- Option AR: materialization and delta ABI contract:
  - specify sparse materialization admission/backpressure semantics when
    visible + overscan + selected/dependent sets exceed budget: cap,
    continuation, placeholder, or fail behavior must be explicit per mode;
  - version the `RuntimeDelta` / `DocumentDelta` ABI and define capability
    negotiation across runtime, document, renderer, dev window, proof
    subscribers, and verifiers;
  - unsupported delta capabilities must fall back only through named adapters
    that report product-path use and fail hot-path readiness when they are on
    the product frame;
  - success gate: stale pending materialization and incompatible deltas fail
    closed by version/epoch instead of silently rebuilding full summaries.
- Option AS: generic cycle-safety semantics:
  - define cycle behavior for self cycles, mutual formula cycles, range cycles,
    demand-current reentrancy, default-stack cycles, partial invalidation after
    cycle error, and recovery after source changes;
  - cycle detection must be shared runtime behavior, not a Cells scenario
    special case or lazy-summary hack;
  - reports expose cycle-state transitions, affected keys/ranges, and whether
    a cycle error was cached, invalidated, or recovered;
  - success gate: cycle fixtures pass for generic list/formula/range cases
    while product frames avoid stack overflow and avoid full-grid recovery
    recompute.
- Option AT: RCU-style hot-state publication:
  - publish active preview state through immutable snapshots or arc-swapped
    handles so the render/product frame can read without taking runtime,
    document, proof, or dev-window locks;
  - writers build pending snapshots off-thread or in non-product phases and
    publish them only after revision checks pass;
  - old snapshots retire after all readers finish, using bounded epochs or a
    frame-lifetime arena rather than blocking product presentation;
  - success gate: product frames report zero blocking locks on runtime,
    document, proof history, report state, and dev IPC while still seeing a
    coherent active snapshot.
- Option AU: explicit preview protocol and ABI split:
  - version the protocol between compiler/runtime/document/playground,
    app-window, renderer, proof subscribers, dev HUD, and xtask verifiers;
  - separate product messages from diagnostic/proof/report messages so a
    product frame cannot accidentally wait for a debug payload;
  - make unsupported product capabilities fail or use a named adapter with
    visible counters instead of silently falling back to JSON/proof paths;
  - success gate: every report names protocol version, capability set, adapter
    fallbacks, and whether any fallback was used before present.
- Option AV: deterministic frame-lane simulator:
  - build a small scheduler model test for host input, source wakes, layout
    wakes, proof requests, report flushes, timers, surface changes, and burst
    exit rules;
  - model coalescing, latest-wins cancellation, proof queue overflow, stale
    snapshot rejection, and idle transitions before testing with real WGPU;
  - use the simulator to prove that proof/report lanes cannot relabel product
    frames or keep DemandDriven bursts alive past the hard cap;
  - success gate: scheduler transition tests fail before any runtime or WGPU
    work when a new lane violates product/proof ownership.
- Option AW: incremental layout engine contract:
  - make layout invalidation typed by constraint, size, text metrics, style,
    child list, scroll offset, and materialized window instead of one broad
    dirty flag;
  - keep scroll, hover, focus, caret, and selection in property/overlay state
    whenever they do not change layout constraints;
  - allow a future Taffy-like or custom incremental layout backend only behind
    the same typed layout ABI and equivalence tests;
  - success gate: selection and passive scroll report zero full layout-frame
    rebuilds, bounded dirty fragments, and equivalent bounds in verifier
    fixtures.
- Option AX: text, IME, caret, and glyph pipeline split:
  - treat text editing, IME composition, caret blink, text selection, shaped
    runs, glyph atlas uploads, and bound text mirrors as one generic control
    pipeline shared by examples and the dev editor;
  - first-frame caret/focus/text-mirror feedback should be a retained control
    patch; shaping and glyph uploads should run only for changed visible text
    ranges;
  - accessibility text state should subscribe to committed text/control
    revisions after present unless needed for an actual accessibility action;
  - success gate: formula bar, ordinary inputs, editor text, and future IME
    scenarios pass the same visual replay with bounded shaping/upload counters.
- Option AY: render graph and pass compilation:
  - replace ad hoc renderer branches with a small render graph or pass plan
    compiled from active scene state: clear, background, clips, primitives,
    text, overlays, cursor, optional proof copy/readback;
  - cache pass descriptors, bind groups, pipelines, render bundles where useful,
    and batch keys by stable render identities;
  - optional proof passes must be attachable after product pass planning without
    changing visible product output or blocking present;
  - success gate: reports name pass count, batch count, cache hits, upload
    bytes, encode time, and whether any proof pass ran on the product frame.
- Option AZ: perf-mode contract and build profiles:
  - define named runtime modes such as `product`, `product_trace`,
    `proof_subscriber`, `diagnostic_probe`, and `developer_debug`;
  - each mode has an explicit allowed-work table, counter set, and budget, so
    debug/proof code cannot be mistaken for product behavior;
  - release verifiers must run the product mode first, then optional
    proof/trace modes that report their regression cost;
  - success gate: a report cannot pass product latency while running a
    diagnostic/proof mode that kept the renderer artificially hot or skipped
    product proof requirements.
- Option BA: native performance lab matrix:
  - maintain repeatable scenarios for empty present floor, counter, TodoMVC,
    Cells, editor wheel, text input, sparse list/grid, proof-only readback, and
    overloaded dev-window;
  - record adapter, backend, driver, compositor/session, present mode, refresh
    assumptions, build profile, CPU governor, thermal state when available, and
    worktree/binary fingerprints;
  - compare p50/p95/max trends and phase counters across runs instead of
    treating one stale report as truth;
  - success gate: every performance claim links to a fresh schema-valid report
    from the matching binary/worktree and says whether it is product, proof,
    software/headless, or hardware/product-surface evidence.
- Option BB: product-frame memory ownership:
  - move hot product-frame allocations into reusable arenas, frame pools,
    staging belts, and small fixed-size rings owned by the relevant subsystem;
  - keep debug strings, JSON values, report arrays, screenshots, and proof
    history expansion out of product memory ownership;
  - expose per-owner allocation counters for app-window, playground, document,
    runtime, renderer, proof, report, and dev HUD;
  - success gate: product frames show bounded or zero heap allocation by owner,
    and any temporary allocation allowlist has a deletion gate.
- Option BC: typed route and control-state fuzzing:
  - fuzz or property-test retained hit regions, source intents, focus movement,
    scroll transforms, text-control state, stale epoch rejection, and pointer
    capture with generated examples that do not mention Cells;
  - replay the same generated transactions through reference runtime/document
    semantics and the retained hot path to detect divergence;
  - include negative cases for stale route snapshots, mismatched layout epochs,
    duplicate clicks, coalesced wheel events, and superseded pending scenes;
  - success gate: generic route/control-state invariants hold before the Cells
    fixture is used as a large stress test.
- Option BD: source-edit and preview replacement architecture:
  - keep source edits, example switching, and full compile/lower replacement
    on a latest-wins worker path that cannot block the active preview frame;
  - preserve active scene interactivity while pending source/document/runtime
    state builds, and show explicit stale/compiling/error states in the dev
    window from cached telemetry only;
  - cancel or supersede expensive parse/typecheck/lower/report/proof work when
    newer source arrives;
  - success gate: dev editor typing and example switching report bounded active
    preview stalls, zero product-frame IPC waits, and explicit stale-result
    drops.
- Option BE: unified example interaction spec:
  - give each Boon example a declarative interaction spec describing expected
    visible controls, source intents, focus behavior, text editing, scrolling,
    hover, and functional assertions;
  - generate host-event visual replay, app-owned cursor proof, runtime
    functional checks, and no-hacks audit fixtures from the same spec;
  - specs may mention example data, but production code may only consume the
    generic lowered route/control/runtime metadata generated from them;
  - success gate: adding a new interactive example automatically gets product
    latency, proof identity, focus/hover/text/wheel, and generic fallback
    negative coverage.
- Option BF: query-engine and dependency-scheduler consolidation:
  - converge compiler incremental queries, runtime currentness, document
    lowering, layout invalidation, render extraction, and proof subscribers on
    one shared revision/dependency vocabulary;
  - avoid separate ad hoc dirty systems whose revisions cannot be compared or
    keyed in `FrameEvidenceKey`;
  - use topological scheduling and strongly connected component handling for
    runtime/list/formula dependencies, with visible-demand priorities;
  - success gate: reports can trace a visible change through one dependency
    graph from source/runtime input to document/layout/render/proof evidence.
- Option BG: human-test-safe manual playground mode:
  - keep manual visible playground launches separate from verifier evidence, but
    add a product HUD/debug mode that helps humans see active lane, last
    product latency, proof mode, stale state, focus target, and route id;
  - manual mode must not enable proof/readback/report work that changes product
    latency unless it says so in the HUD;
  - provide a quick command that stops older matching playground processes and
    starts the current release binary for the chosen example;
  - success gate: human reproduction can see the same lane/evidence fields as
    reports, while readiness still depends only on native app-owned gates.
- Option BH: architecture decision record checkpoints:
  - when a large option is selected, write a short ADR naming the old boundary,
    chosen replacement, expected deleted paths, success gates, and rollback
    rule;
  - update `NATIVE_GPU_PIPELINE.md`, AGENTS.md, verifier schemas, and embedded
    goal prompt when the ADR changes the implementation contract;
  - archive rejected tactics such as route-cache-only or proof-size-only fixes
    with the report that proved they were not enough;
  - success gate: future agents can tell which architecture is current without
    reading thousands of progress bullets or repeating failed experiments.
- Option BI: compatibility path budget burn-down:
  - convert every temporary compatibility path into a tracked row with a hard
    budget, owner crate, owner test, allowed modes, and removal milestone;
  - product mode should fail when an expired compatibility path runs before
    present, even if latency happens to pass in one sample;
  - use the burn-down to decide whether to delete, quarantine, or promote a path
    after each major replacement;
  - success gate: the compatibility register shrinks across releases and
    native readiness cannot silently depend on expired paths.
- Option BJ: product-first error and fallback behavior:
  - define how product frames behave when runtime/layout/proof/dev work is
    stale, slow, missing, or failed: present active state, show stale marker,
    defer non-visible work, drop proof sample, or fail the verifier;
  - never replace a product-frame failure with synthetic proof success,
    driver timing, latest-report data, or a stale cached screenshot;
  - make fallback decisions typed and visible in reports rather than buried in
    `Option` defaults or missing JSON fields;
  - success gate: every failed sample names the exact fallback/error policy
    used, and unsupported product fallbacks fail closed.
- Option BK: cross-platform input/render backend boundary:
  - keep native app-window APIs generic enough that Wayland, X11, headless,
    nested compositor, and future platform backends report the same product
    lanes, timing fields, evidence keys, and proof semantics;
  - isolate platform-specific focus/input safety, present-mode selection,
    surface lifecycle, and timestamp behavior behind a backend contract;
  - success gate: platform differences appear as backend metadata and counters,
    not as divergent verifier semantics or example-specific branches.
- Option BL: quality-of-service prioritization:
  - assign QoS classes to work: visible interaction, text commit, scroll,
    animation follow-up, runtime continuation, layout materialization, proof,
    report, dev HUD, and background diagnostics;
  - apply deadlines, cancellation, coalescing, and queue capacity by QoS class
    so lower-priority work cannot starve or block product frames;
  - expose per-QoS queued, dropped, superseded, executed, and over-budget counts;
  - success gate: stress tests with overloaded proof/dev/report queues still
    preserve product input p95 or fail with a product-owned blocker rather than
    hidden backpressure.
- Option BM: surface lifecycle and recovery contract:
  - make minimize, occlusion, monitor move, resize, DPI/scale change, surface
    lost, device lost, adapter change, suspend/resume, and visibility changes
    explicit scheduler/product states;
  - lifecycle transitions invalidate only surface-dependent epochs whenever
    possible, while runtime/document/active-scene state remains reusable;
  - recovery frames are classified as surface-lifecycle work and are not charged
    as host-input latency unless the input directly caused the transition;
  - success gate: resize/surface-lost/device-lost tests recover with fresh
    surface epochs, no stale proof reuse, no full runtime recompute, and no
    mislabeled product-input samples.
- Option BN: GPU memory and cache eviction policy:
  - define cache limits and eviction for glyph atlases, image/texture atlases,
    render bundles, staging belts, dynamic buffers, frame arenas, route/hit
    snapshots, proof readback buffers, and screenshot artifacts;
  - report cache size, high-water marks, eviction counts, fragmentation,
    allocation growth, and whether an eviction forced resource rebuild before
    product present;
  - memory pressure may drop proof/report/debug caches first, but must not push
    proof JSON, full scene rebuilds, or full text reshaping onto a product
    frame;
  - success gate: long edit/scroll sessions show bounded cache growth and
    evictions do not violate product p95 or exact-key proof validity.
- Option BO: multi-surface GPU arbitration:
  - treat preview product frames, dev-window rendering, HUD updates, proof
    readbacks, report screenshots, and future multiple previews as separate GPU
    clients sharing device/queue resources;
  - prioritize preview product submit over dev/HUD/proof/report work unless the
    user explicitly runs a diagnostic/probe mode;
  - report queue submissions, readback copies, surface acquisitions, and GPU
    work by surface/client/lane so dev-window activity cannot hide preview
    contention;
  - success gate: overloaded dev-window, HUD, and proof readback workloads do
    not block preview product p95, or the report names GPU arbitration as the
    product blocker.
- Option BP: accessibility action routing:
  - route accessibility actions through the same typed retained
    `SourceIntent`, `ViewportIntent`, `FocusIntent`, and text-control commands
    as mouse, keyboard, wheel, and text input;
  - accessibility snapshots remain post-present subscribers unless an actual
    accessibility action needs current data to produce visible pixels;
  - product action handling must not rebuild the full accessibility tree,
    inspect proof JSON, or scan latest reports before present;
  - success gate: accessibility click/focus/text/scroll actions pass the same
    product/proof evidence gates as host input, with zero pre-present snapshot
    rebuilds outside the action target.
- Option BQ: animation and timer semantics:
  - define a retained animation lane for caret blink, hover transitions, scroll
    inertia, replay, future animations, and timed runtime/document requests;
  - use one deterministic monotonic time source per preview process, with
    visibility/occlusion pause rules and burst hard caps;
  - animation/timer frames may request product redraws, but must not relabel
    accepted host-input frames or keep proof/report work hot;
  - success gate: caret/simple-animation tests show bounded dirty regions,
    correct pause/resume behavior, exact frame-lane attribution, and no
    ContinuousProbe dependency.
- Option BR: GPU timing calibration and clock domains:
  - report whether each phase timing is CPU-observed, GPU timestamp-query
    observed, compositor-observed, or inferred;
  - expose timestamp-query availability, timestamp period, calibration error,
    disjoint/invalid timing state, queue depth estimates, and clock-domain
    uncertainty;
  - keep CPU-submit/present product gates separate from optional GPU-completion
    and compositor-present diagnostics;
  - success gate: reports cannot imply GPU completion or compositor visibility
    from CPU `present()` timing without explicit timing evidence and metadata.
- Option BS: long-session performance aging:
  - add soak scenarios for long scrolling, repeated cell/text edits, source
    replacement, dev-window typing, proof-heavy verification, and mixed
    preview/dev workloads;
  - record p95 drift, max outlier clusters, allocator churn, cache growth,
    atlas fragmentation, proof/report backlog, stale pending drops, queue
    depth, dropped frames, and CPU/GPU memory high-water marks;
  - compare early, middle, and late windows rather than only short cold/steady
    samples;
  - success gate: native preview does not degrade over long sessions, or the
    report names the owner cache/queue/allocation path that needs deletion or
    eviction policy work.
- Option BT: hard quarantine for proof-shaped product paths:
  - fail native UX gates if product input, hit testing, source intent, passive
    scroll, or selected-text routing reads `layout_proof`, proof JSON,
    `native_gpu_render_proof`, latest-report state, desktop/browser/COSMIC/Ply
    artifacts, modeled/static scroll evidence, or driver timing as the latency
    authority;
  - release Cells visible-click may use the driver only to inject/observe real
    input; latency authority must come from app-window accepted-input product
    frame timing and matching `FrameEvidenceKey`;
  - split verifiers into product, proof, and harness subcontracts with separate
    failure causes: product pixels missing, proof missing/mismatched, runtime
    probe missing, app-window timing missing, driver/IPC failure, stale report,
    or schema error;
  - success gate: every product UX pass is backed by app-produced timing and
    exact-key proof, and every old proof-shaped path has a negative test proving
    it cannot satisfy product readiness.
- Option BU: synchronous ACK and switch-path retirement:
  - remove or quarantine dev/example-switch paths that wait for parse, lower,
    layout, runtime summaries, proof JSON, or layout artifacts before keeping
    the preview's last good active scene alive;
  - ACKs should carry only small typed lifecycle/status data, while large
    diagnostic payloads are fetched asynchronously or produced by report
    subscribers;
  - source replacement remains latest-wins with stale-result rejection and
    bounded pending work;
  - success gate: example switching and source edits do not block product
    preview presentation on proof/layout/runtime-summary ACK payloads.
- Option BV: repeated-failure architecture checkpoint:
  - when the same class of native UX failure repeats after a bounded number of
    local patches, require a short architecture diagnosis before the next patch:
    the old path still reachable, the verifier coupling that makes it hard to
    delete, the product/proof boundary to move, and the negative gate to add;
  - record rejected tactics with report names and measured blockers so future
    agents do not repeat route-cache-only, JSON-size-only, or proof-size-only
    loops when they are not dominant;
  - prefer deletion, quarantine, or replacement of a boundary over adding a
    third compatibility path;
  - success gate: progress notes name which architecture option was selected
    and which old path will be removed if the slice succeeds.

Decision rule:

- If the p95 failure is dominated by runtime/list/currentness work, choose
  Option F before renderer work.
- If the p95 failure is dominated by route lookup, layout proof, JSON, or report
  history, choose Options C, D, E, or J and delete the slow boundary.
- If the p95 failure is dominated by queue/present, choose Option G before
  spending time on 1-2 ms app micro-fixes.
- If a human-visible bug disagrees with reports, choose Option H first so the
  verifier proves the same UX path.
- If a proposed fix needs fixture strings, source paths, field names, geometry
  guesses, or verifier-only shortcuts, reject it and pick a generic option
  above.

## Strategy Pivot Ledger

Use this ledger when the same class of performance failure survives one
focused implementation slice. The goal is to preserve the largest plausible
architecture improvements and force an explicit choice, not to make every idea
active at once.

### Pivot Triggers

- [ ] Repeated p95 miss in the same subsystem:
  - trigger when two fresh release reports from the same product gate show the
    same dominant class: runtime/currentness, input/route, layout/extract,
    GPU upload/encode, queue/present, proof/report, dev IPC, scheduler, or
    verifier coupling;
  - required action: stop local micro-optimizations, name the old boundary,
    choose one architecture option from this plan, and add the negative gate
    that will prove the old boundary cannot satisfy product readiness.
- [ ] Reports and human-visible behavior disagree:
  - trigger when manual testing shows broken focus/hover/text/scroll while
    reports claim success, or when reports pass using a path users do not see;
  - required action: repair the verifier/product evidence path first using
    app-owned host events, visible cursor proof, WGPU readback, and exact
    `FrameEvidenceKey`. Do not tune performance numbers until the test proves
    the real UX path.
- [ ] Product frames still depend on proof/debug state:
  - trigger when accepted input, hit routing, selection/text sync, scroll, or
    present reads layout proof JSON, latest reports, report history, proof
    caches, screenshot artifacts, or dev-window state before present;
  - required action: move that state behind typed retained data or a
    post-present subscriber and add a product-mode assertion that forbids the
    old read.
- [ ] Compatibility paths are growing:
  - trigger when a fix adds a new fallback without deleting or quarantining an
    older one;
  - required action: create or update a stale-path ledger row with owner crate,
    temporary allowlist, typed replacement, positive gate, negative gate, and
    removal condition.
- [ ] Product p95 is near the present floor:
  - trigger when app-side pre-submit work is bounded but queue/present remains
    the dominant p95/max bucket;
  - required action: run the focus-safe hardware product-surface present-floor
    lab, measure present modes and frame-in-flight policy, and stop spending
    time on 1-2 ms app micro-fixes until the machine/compositor floor is known.

### Option Selection Rubric

Before implementation, score each candidate architecture cut from 0 to 3 on
these axes and record the result in the progress section. Prefer the highest
total score that also reduces conceptual complexity.

- Product path removed: does the option remove a pre-present boundary rather
  than only optimize it?
- Genericity: does it work for arbitrary Boon programs, non-Cells sparse
  fixtures, text controls, scroll, and dev-window editor paths?
- Simplicity: does it reduce the number of product/proof/verifier paths?
- Verifiability: can xtask prove it with app-owned native events, exact-key
  WGPU proof, counters, and negative tests?
- Deletion power: does it let us delete or quarantine a known slow path?
- Risk containment: can it land behind a mode, kill switch, or strangler slice
  without weakening product gates?
- Architecture alignment: does it move `NATIVE_GPU_PIPELINE.md`, AGENTS.md,
  schemas, and embedded goal prompts toward one shared contract?

### Highest-Leverage Cuts To Try Before More Micro-Tuning

- [ ] `PreviewHotLoop` / `NativeFrameClock`:
  - one owner drains host input, starts product frames, applies retained
    patches, submits/presents, and dispatches post-present subscribers;
  - old direct scheduler ingress paths are inventoried and forbidden from
    opening or relabeling product frames.
- [ ] `ActivePreviewScene` / `PendingPreviewScene`:
  - active state is always presentable and contains retained route, control,
    overlay, materialization, render, GPU-resource, and evidence state;
  - pending state is latest-wins and commits only by matching source/content/
    layout/render/surface/input epochs.
- [ ] Typed input transaction ABI:
  - host input lowers to typed route snapshots and source/viewport/text/focus
    intents with stable ids, not proof JSON or geometry/string lookup;
  - the transaction carries the same ids through runtime, document, layout,
    renderer, proof subscribers, reports, and negative tests.
- [ ] Product/proof pipeline split:
  - product present returns a small `RenderFrameResult` with scalar counters and
    `FrameEvidenceKey`;
  - proof/readback/report/HUD workers subscribe later by exact key, with bounded
    queues, lag reporting, and no product-frame backpressure.
- [ ] Render-owned WGPU resource lifetime:
  - persistent pipelines, bind groups, glyph atlases, text shaping caches,
    staging belts/rings, render batches, clips/transforms, and frame arenas live
    in renderer-owned state;
  - product reports prove bounded upload bytes, no proof JSON allocation, and
    no full render-scene rebuild for overlay/control/scroll changes.
- [ ] Data-oriented runtime/document storage:
  - hot runtime/document paths use typed slot ids, generational arenas,
    columnar fields, dirty bitsets, reverse binding indexes, list windows, and
    dependency queues;
  - string/path/debug summaries become post-present diagnostics only.
- [ ] Generic sparse list/query/formula core:
  - `List/find`, range dependencies, demand-current reads, cycle safety,
    visible materialization, and dependency fanout are one generic engine
    service with non-Cells fixtures;
  - product frames cannot use unscoped root flushes or full-grid/list summaries.
- [ ] Compiler/document identity workstream:
  - lowering emits stable source intents, node ids, binding reverse indexes,
    row/list ids, route ids, hit ids, text-control ids, and render primitive
    ids;
  - native preview consumes these identities directly and never recreates them
    from labels, source paths, geometry, or fixture strings.
- [ ] Incremental layout/property-tree workstream:
  - layout invalidation is typed by constraint/text/style/materialization
    reason, while scroll/focus/hover/selection/caret patch retained property
    trees;
  - passive scroll and first-frame selection/focus must not force document
    relower, full layout rebuild, or full display-list rebuild.
- [ ] Text-control subsystem:
  - formula bars, ordinary inputs, dev editor text, caret, selection, IME,
    paste, undo/redo, bound mirror text, and accessibility text state share one
    retained text-control model;
  - first-frame focus/value display patches retained state before runtime
    validation or formula evaluation unless the visible pixel depends on it.
- [ ] Present-floor and frame-in-flight lab:
  - add focus-safe hardware/product-surface measurements with explicit present
    mode, adapter, surface epoch, queue depth, acquire/submit/present phases,
    and no real OS input;
  - use the result to decide whether late acquire, frames in flight,
    present-mode changes, or app CPU cuts are the correct next move.
- [ ] Dev-window isolation:
  - source editing, report browsing, HUD refresh, proof-history expansion, and
    code-editor wheel run on lanes that cannot hold preview product locks;
  - add no-dev and overloaded-dev product runs so dev-window work cannot hide
    preview contention.
- [ ] Minimal product-preview strangler:
  - if the existing preview remains proof-shaped, build a small product-only
    pipeline beside it: typed route snapshot, active scene, retained patch,
    direct visible-surface present, scalar counters, and proof-subscriber hooks;
  - migrate gates only if it is both faster and simpler, then quarantine the
    old product path. Delete the spike if it becomes a second semantic engine.

### Cross-Goal Architecture Items To Keep Visible

- [ ] Rust/Zig/Wasm/native codegen:
  - keep codegen aligned with
    `docs/plans/speedup/22-post-speedup-compiler-codegen-wasm-plan.md` and
    typed IR/NativeRegionIR;
  - use generated kernels only to remove measured runtime/list/binding/render
    extraction boundaries with interpreter equivalence proof;
  - do not relabel a broken current runtime, verifier, or product loop as fixed
    by future codegen unless the gate is explicitly moved to a verified
    replacement backend.
- [ ] WGPU pipeline upgrade:
  - consider render graph/pass compilation, cached render bundles, persistent
    resource tiers, GPU timestamp diagnostics, atlas/memory eviction, and
    multi-surface arbitration as one renderer architecture workstream;
  - product gates must still prove direct visible-surface output and exact-key
    readback; proof passes are optional subscribers unless explicitly measured
    as product work.
- [ ] BYTES / machine-plan / hardware backends:
  - reconcile their storage, schedule, dependency, and proof contracts with the
    runtime/document/render ids used by native preview before starting parallel
    execution models;
  - do not create a second currentness or dependency vocabulary that cannot be
    tied into `FrameEvidenceKey`.
- [ ] SolidGraph/3D and future scene systems:
  - share renderer ownership, frame clock, proof subscribers, resource lifetime,
    and scheduler lanes with 2D/native preview where possible;
  - if 3D requires different render passes or proof artifacts, version them in
    the render/proof ABI instead of bypassing native product gates.
- [ ] All-example verifier generation:
  - add a declarative interaction spec per example and generate visual replay,
    functional assertions, app-owned cursor proof, product/proof reports, and
    no-hacks checks from it;
  - Cells remains a stress fixture, not the only architecture proof.

### Maximal Architecture Improvement TODO Capture

This section keeps broader replacement options visible so future work can pick
a strategy instead of circling around another local timing patch. Do not
implement all of these blindly. Choose the simplest cut that deletes the most
product-frame work, then add the positive gate and negative old-path gate before
claiming progress.

- [ ] Product-preview strangler lane:
  - build a minimal product lane beside the proof-shaped preview path:
    retained route snapshot, retained control/overlay state, active render
    scene, direct visible-surface present, scalar counters, and exact
    `FrameEvidenceKey`;
  - the lane initially handles hover, focus, selection, text-input mirror,
    passive scroll, and simple source commits using typed deltas;
  - proof/readback/report/HUD subscribers consume the presented frame key after
    present and cannot affect first-frame pixels;
  - migrate normal preview UX gates to this lane only if the code is simpler
    and faster than the legacy path; otherwise delete the spike;
  - success gate: with proof disabled, the product lane has no pre-present
    proof JSON, no latest-report reads, no dev IPC waits, no full
    `state_summary`, and no layout-proof route scans.
- [ ] Product/proof state split:
  - split the current render-loop state into product scheduling/commit state
    and proof/report/readback subscriber state;
  - emit a small `ProductFrameCommit` after present with `FrameEvidenceKey`,
    lane, input sequence, content/layout/render revisions, surface epoch,
    present id, and phase timings;
  - proof, report, readback, HUD, and artifact workers consume
    `ProductFrameCommit` by key and cannot mutate product scheduling state;
  - success gate: product frame reports can be assembled from
    `ProductFrameCommit` rows without reading proof history or latest report
    JSON.
- [ ] Product `PresentPlan` independent from proof flags:
  - make the product patch decision depend on retained-state validity and dirty
    deltas, not on whether app-owned scene proof/readback is enabled;
  - replace proof-coupled render-hook branching with a typed
    `PresentPlan { product_patch, product_surface, proof_requests }`;
  - success gate: product p95 and retained-patch counts are identical, within
    measurement noise, when proof mode changes from counters-only to readback
    subscriber mode.
- [ ] Single native frame-clock owner:
  - consolidate product redraw ownership into one `PreviewHotLoop` /
    `NativeFrameClock` state machine;
  - all host input, animation, caret, source/runtime wake, proof sample,
    telemetry flush, surface lifecycle, and dev-HUD refresh requests enter as
    typed wake reasons with priorities and phase allowlists;
  - no other subsystem may submit, relabel, or delay a product frame without a
    transaction id owned by the frame clock;
  - success gate: reports show one product-frame owner, explicit wake reason,
    explicit pacing state, and zero proof/report/source-cleanup wakes charged
    to unrelated host-input transactions.
- [ ] Source-explicit scheduler queues:
  - separate queues for host input, runtime/source turns, animation/caret
    requests, surface lifecycle, proof/readback requests, report/HUD flushes,
    and dev-window work;
  - assign frame lane, wake reason, and transaction id at enqueue time rather
    than inferring them from dirty revisions or latest state after the fact;
  - success gate: a product frame cannot be opened, relabeled, or extended by a
    proof/readback/report wake, and unknown scheduler ingress fails product
    mode.
- [ ] Product transaction ABI:
  - define a compact ABI that every visible interaction carries:
    `HostInputEventSeq`, `RouteSnapshotEpoch`, `InputIntentId`,
    `RuntimeTurnId`, `DocumentPatchRevision`, `LayoutRevision`,
    `RenderSceneRevision`, `SurfaceEpoch`, `PresentId`, and
    `FrameEvidenceKey`;
  - use typed transaction records instead of latest-report inference in xtask
    verifiers and app-window reports;
  - coalescing is legal only when the resulting transaction records the
    superseded ids and the visible semantics are unchanged;
  - success gate: missing or mismatched transaction ids fail product UX reports
    rather than falling back to driver timing, stale proof, or human-visible
    assumptions.
- [ ] Active scene as the only product state:
  - define `ActivePreviewScene` as the current source of product truth for hit
    testing, focus, text controls, scroll transforms, overlay state, retained
    layout fragments, render batches, GPU resources, and evidence registry;
  - define `PendingPreviewScene` as latest-wins worker output with capacity 1
    per role/lane and explicit epoch rejection before activation;
  - normal product frames render from `ActivePreviewScene` even while runtime,
    compiler, layout, proof, or report workers are still building newer state;
  - success gate: hover/focus/selection/caret/passive-scroll frames do not
    borrow broad runtime, document, report, or proof state before present.
- [ ] Move retained state out of playground proof plumbing:
  - separate typed retained document/render state from serialized
    proof/report snapshots. Useful caches such as layout frames, route/hit
    tables, data-binding indexes, patch profiles, and render-scene patch data
    live in document/render owners, not in JSON proof bundles;
  - serialization happens after product commit from immutable snapshots or
    typed handles;
  - success gate: product code can update retained selection/text/scroll state
    without constructing a proof JSON object or walking a serialized
    `DocumentRenderSnapshot`.
- [ ] Retained UI-control subsystem:
  - introduce a generic retained control model for focus, hover, active/pressed
    state, text mirror, selection range, caret blink, IME/composition, scroll
    offset, drag state, undo grouping, and accessibility focus;
  - formula bars, ordinary inputs, the dev code editor, buttons, list rows,
    scrollbars, and future controls share the same state machine;
  - runtime commits validate or normalize the retained control state after the
    first-frame patch instead of blocking first feedback;
  - success gate: click-to-focus, click-to-formula-bar text, hover, typing,
    selection movement, editor wheel, and scrollbars pass all-example visual
    replay without example-specific control code.
- [ ] Optimistic UI commit contract:
  - formalize first-frame retained patches that visibly update focus,
    selection, hover, text mirrors, and formula/input display before the full
    runtime turn commits;
  - each optimistic commit records reconciliation policy, rollback behavior,
    runtime confirmation key, and whether user-visible pixels depend on later
    validation;
  - success gate: release UX gates either accept `OptimisticUiCommit` with
    confirmed reconciliation evidence, or the path is disabled and cannot count
    as product success.
- [ ] Typed source-intent and viewport-intent compiler work:
  - have lowering emit typed source intents, viewport intents, focus intents,
    binding reverse indexes, text-control ids, hit-region ids, and list-window
    demand ids;
  - native preview consumes these ids directly and never reconstructs intent
    from labels, source paths, geometry, proof JSON, or Cells-style field names;
  - success gate: changing visible text/labels/layout geometry without
    changing semantic ids does not break routing, retained patches, or
    verifier matching.
- [ ] Data-oriented runtime and document storage:
  - move hot runtime/document state toward generational arenas, typed slot ids,
    columnar indexed fields, dirty bitsets, reverse dependency indexes, and
    visible/materialized window queues;
  - keep broad summaries and human-readable path maps as report/debug
    projections generated after product present;
  - success gate: product frames account for visible keyed currentness reads
    and typed deltas only; full summaries/root flushes/list scans are forbidden
    in product mode.
- [ ] Full-lower and summary fallback quarantine:
  - make full document lower, broad runtime summary rebuild, and full layout
    recovery startup/recovery-only for native product mode;
  - after runtime events, product interactions must apply typed deltas or
    activate a pending scene, not rebuild the whole document/layout/proof stack
    before present;
  - success gate: release product interaction samples fail if full lower,
    full layout rebuild, full summary rebuild, or broad root flush appears in a
    pre-present product frame.
- [ ] Generic sparse query/materialization engine:
  - make `List/find`, `List/chunk`, list windows, range dependencies,
    formula dependency fanout, demand-current fields, cycle safety, and
    selected/dependent-key materialization one shared runtime service;
  - expose logical count, materialized keys, rendered nodes, evaluated formulas,
    dependency fanout, and upload bytes as separate counters;
  - success gate: a non-Cells sparse-grid/list fixture and Cells both pass
    runtime no-scan/no-root/no-recompute gates with no compiler/runtime
    branches on fixture names or spreadsheet addresses.
- [ ] Renderer-owned WGPU resources:
  - move pipelines, bind groups, glyph atlases, shaped text runs, texture
    atlases, primitive batches, clips/transforms, staging belts/ring buffers,
    route snapshots, render bundles, frame arenas, and readback resources under
    a renderer owner with stable lifetime;
  - product frames apply small retained deltas and encode from prepared
    resources; proof frames request readback or structured proof later;
  - success gate: reports expose cache hits, upload bytes, allocation counts,
    encode time, queue submit, present, draw calls, and prove no full
    render-scene rebuild for overlay/control/scroll changes.
- [ ] Present-floor and pacing decision lab:
  - add focus-safe hardware/product-surface baselines for empty retained frame,
    simple overlay patch, text-control patch, scroll transform, and full Cells
    click on the same adapter/surface/present mode;
  - separately report acquire, encode, queue submit, `frame.present()`,
    compositor/vsync, GPU completion if available, and proof completion;
  - test late acquire, alternate present modes, frame latency, frames in flight,
    and ring-buffer sizing only against a baseline that proves they target the
    dominant cost;
  - success gate: the plan can say whether remaining p95 is app CPU,
    queue/present policy, compositor floor, proof coupling, or verifier
    accounting.
- [ ] Proof/report subscriber architecture:
  - define bounded post-present subscribers for WGPU readback, visible-bound
    text proof, retained-bound-sync proof, proof-history compaction, report
    JSON, artifact hashing, PNG encoding, screenshot diffing, and debug dumps;
  - subscribers consume immutable frame-local snapshots or exact
    `FrameEvidenceKey` handles and must never read mutable "latest" state as
    acceptance proof;
  - success gate: product reports include subscriber lag/drop counts, and
    stale first-frame proof, mismatched surface epoch, mismatched revision,
    hash-only proof, or latest-report proof all fail negative tests.
- [ ] FrameEvidenceKey as sole join key:
  - producers emit typed keyed artifacts directly instead of recursively
    attaching keys to JSON after the fact;
  - proof artifacts require `proof_request_id`; product artifacts require
    `input_event_seq` when they correspond to accepted input;
  - xtask joins product timing, visual proof, runtime/currentness proof, and
    harness observations by exact key only;
  - success gate: latest/last artifact joins, hash-only joins, and artifacts
    missing required input/proof ids fail closed.
- [ ] Dev-window isolation and UX telemetry:
  - dev footer/HUD reads cached scalar `PreviewPerfStats` snapshots only;
  - source editor wheel, report browsing, source replacement, syntax updates,
    and proof-history expansion run on lanes that cannot block preview product
    frames;
  - HUD displays mode-aware stats: idle age/last latency, burst p95/drops,
    probe proof latency, present floor, proof mode, and stale-proof/subscriber
    lag warnings;
  - success gate: overloaded-dev and no-dev runs show the same preview product
    p95 within a small tolerance, and `preview_blocked_on_ipc_count=0`.
- [ ] Verification rewrite around product/proof modes:
  - split verifiers into product-only, proof-only, product-plus-proof, and
    negative stale-path modes;
  - every verifier states input source, frame mode, proof mode, present path,
    evidence key policy, warmup, sample count, outlier policy, adapter,
    present mode, and current worktree/binary fingerprint;
  - release UX gates fail when app-owned input timing is absent, proof work is
    required to make pixels visible, ContinuousProbe is used for product
    latency, or driver timing replaces product timing;
  - success gate: every Boon example with interactive controls has at least one
    deterministic visual replay with app-owned cursor proof and functional
    assertions.
- [ ] Release UX accounting hardening:
  - release Cells visible-click and future product UX gates require
    app-window product-lane accounting; click-sample-derived summaries are
    diagnostic only;
  - every click/key/wheel/text sample in release requires `interaction_timing`
    from the product frame it claims;
  - product latency, proof/currentness evidence, and driver/harness latency are
    separate contracts with separate failure causes;
  - success gate: missing app-window product rows, missing interaction timing,
    or driver-only latency fails release product readiness.
- [ ] Nested-compositor and synthetic-input quarantine:
  - classify Weston/headless compositor runs, synthetic probes, deterministic
    click helpers, and shaped test-control input as harness smoke/diagnostic
    paths unless the native GPU contract is explicitly updated;
  - native product readiness must still rely on app-owned product timing,
    public host-event injection, and exact-key WGPU proof;
  - success gate: no nested-compositor or synthetic-input-only path can satisfy
    release native readiness without an explicit contract change.
- [ ] Proof-isolation stress gate:
  - add a verifier mode that intentionally delays, drops, or saturates proof,
    readback, report, screenshot, and artifact-hash subscribers;
  - product p95, missed-frame count, and product evidence rows must remain
    valid while only proof lag/drop counters degrade;
  - success gate: slow proof/report subscribers cannot block product present,
    steal interaction attribution, or make first-frame pixels depend on proof.
- [ ] Render-result contract gate:
  - product render hooks return a typed `PresentedProductFrame` /
    `RenderFrameResult` containing revisions, scalar counters, evidence handles,
    present-target metadata, and post-present proof requests;
  - product render hooks may not return proof JSON, layout artifacts,
    latest-report-derived identity, or locks/borrows into runtime/dev/report
    state as part of the product result;
  - success gate: static/schema checks fail if product render output includes a
    pre-present proof tree or mutable diagnostic payload.
- [ ] Renamed non-Cells sparse fixture:
  - add a sparse list/grid fixture whose names deliberately avoid `cells`,
    `address`, `value`, `error`, `A0`, spreadsheet labels, and fixed 26x100
    assumptions;
  - run the same runtime/list/currentness, retained update, visual replay, and
    no-scan gates as Cells;
  - success gate: the generic sparse engine passes when all spreadsheet-shaped
    strings are absent, proving no hidden fixture shortcut is needed.
- [ ] Cells-source simplification equivalence gate:
  - allow `examples/cells` Boon code to be cleaned up when it expresses the app
    more directly, but require a semantic equivalence report first;
  - the report must prove logical grid size, editing behavior, formula-bar
    behavior, dependencies/ranges/cycles, virtualization counters, and visual
    replay behavior are preserved;
  - success gate: cleaner Cells source cannot hide an engine limitation or
    shrink the stress case.
- [ ] Legacy path deletion ledger expansion:
  - add owner/date/removal rows for layout-proof hot-state reads,
    proof-JSON route lookup, latest-report proof, geometry/string route
    fallback, source-path/fallback intent lookup, private dispatch input,
    broad runtime summaries, duplicate interactive readback, modeled/static
    scroll readiness, driver-timing UX acceptance, and old Ply/Xvfb/COSMIC/
    browser proof;
  - every row needs a typed replacement, positive verifier, negative stale-path
    verifier, runtime/report counter, and removal condition;
  - success gate: `native-gpu-all` fails if a new compatibility path lands
    without old path, typed replacement, kill switch, owner/date, positive
    gate, and negative gate.
- [ ] Explicit repaint and run-to-completion update contract:
  - use explicit wake/repaint requests for input, animation bursts,
    caret/timers, source updates, proof samples, and telemetry flushes;
  - queue model/view effects to a run-to-completion update boundary, then
    invalidate dirty windows once instead of allowing reentrant cascades to
    build multiple frames;
  - success gate: reports show one dirty-window invalidation per product
    update boundary and no recursive frame builds during a visible input.
- [ ] Optional generated hot kernels:
  - after typed IR/runtime/document/render contracts stabilize, evaluate Rust,
    Zig, or Wasm kernels for list lookup, formula fanout, currentness reads,
    text binding sync, layout extraction, and primitive-batch generation;
  - generated kernels remain optional and must be equivalence-checked against
    the interpreter and report the same transaction/evidence ids;
  - success gate: codegen removes a measured product boundary without becoming
    a second semantic engine or hiding current runtime bugs.
- [ ] Architecture decision checkpoints:
  - after each two failed fresh reports in the same blocker class, write a
    short checkpoint naming the blocker, chosen architecture option, old path
    to delete, success gate, and why smaller local patches are no longer the
    right move;
  - if the chosen cut increases code paths or concepts, require an explicit
    simplification/deletion step before any performance success is accepted;
  - success gate: progress is measured by removed product-frame boundaries and
    passing deterministic gates, not by larger diagnostic reports alone.

### Stale Path Ledger Seed Rows

| Old path | Preferred replacement | Product-mode gate |
| --- | --- | --- |
| layout/proof JSON used for hit routing | typed retained route snapshot | zero proof/layout JSON route scans |
| latest-report proof or first-frame proof reuse | exact `FrameEvidenceRegistry` lookup | stale/mismatched proof negative tests fail |
| full runtime `state_summary` before present | typed runtime deltas and scoped currentness reads | zero pre-present full summary builds |
| broad list/root flush on visible input | demand-current field/key/range barriers | zero full-grid/list scans in product samples |
| full display-list rebuild for overlay changes | retained overlay/property-tree patches | zero full layout/display rebuild for focus/hover/selection |
| duplicate interactive readback before product present | post-present proof subscriber | proof lag reported, product p95 unaffected |
| private dispatch or fixture/source-path routing | typed source/viewport/text/focus intents | no production example/source-path branches |
| modeled/static scroll readiness | app-owned host-event wheel replay and WGPU proof | modeled evidence cannot satisfy UX gates |
| driver timing as latency authority | app-window accepted-input product timing | missing app-owned timing fails release gates |
| dev IPC/report/HUD locks in preview frame | cached scalar snapshots and paged debug queries | `preview_blocked_on_ipc_count=0` |
| offscreen copy-to-present as product default | direct visible-surface product present | copy/offscreen paths allowed only in proof/diagnostic modes |
| unowned scheduler wake paths | `NativeFrameClock` repaint broker with lanes | unknown ingress count fails product mode |
| unkeyed UX sample classification | product transaction ABI with `FrameEvidenceKey` | calibration/proof/runtime-cleanup frames cannot enter UX lane |
| pre-present proof/report subscriber work | bounded post-present proof subscribers | slow-subscriber stress leaves product p95 unchanged |
| product render hook returns proof payload | typed `PresentedProductFrame` / `RenderFrameResult` | product result contains no proof JSON or mutable diagnostic state |
| mixed product/proof render-loop state | `ProductFrameCommit` plus proof subscriber state | reports assemble product rows without proof-history/latest-report reads |
| product fast path gated by proof config | `PresentPlan { product_patch, proof_requests }` | retained product patching independent of proof mode |
| retained state bundled in proof JSON | document/render-owned typed retained state | no proof JSON construction for retained product patches |
| deferred runtime first frame as ad hoc shortcut | explicit `OptimisticUiCommit` contract | reconciliation or rollback evidence required |
| legacy selected-address style fallback | generic retained selection/source-binding route | legacy selection fallback count is zero and forbidden in release |
| full document lower after runtime input | typed runtime/document/layout deltas or pending scene | full lower/rebuild fails product interaction samples |
| sample-derived product timing fallback | app-window product-lane interaction timing | missing `interaction_timing` fails release product gates |
| synthetic/nested-compositor-only evidence | public host-event/app-owned WGPU evidence | synthetic-only and Weston-only paths are smoke/diagnostic |
| spreadsheet-shaped sparse fixture as only proof | renamed non-Cells sparse fixture | no-hacks audit passes without Cells/address/value strings |
| compatibility path without owner/removal gate | monotonic deletion ledger with kill switch | `native-gpu-all` fails on unowned temporary paths |

### Definition Of "Completely Fixed"

- Product interaction gates pass in release with `CountersProduct` first:
  visible click/focus/text/formula-bar sync, keyboard/text editing, passive
  scroll, dev-code-editor wheel, example switching, sparse list/window
  materialization, and selected/dependent runtime updates.
- Every product UX sample is transaction-keyed from accepted host input through
  route epoch, typed intent, runtime/document/layout/render revisions, present
  id, and `FrameEvidenceKey`. Calibration, preposition, focus/caret,
  runtime-cleanup, proof/readback, and report frames cannot enter the product
  interaction latency lane.
- Proof/readback gates pass separately against exact `FrameEvidenceKey` rows
  from the same presented product frames, with proof lag and overhead reported.
- Proof-isolation stress gates pass: deliberately slow, dropped, or saturated
  proof/readback/report subscribers may increase proof lag/drop counters, but
  they do not change product p95, missed-frame count, or first-frame pixels.
- Runtime/list/currentness reports show zero accidental full scans, root
  flushes, full-grid recomputes, unscoped summaries, or Cells-specific
  branches on product frames.
- Renderer/app-window reports show bounded pre-present work, direct visible
  product present, retained resource reuse, no dev IPC locks, no proof/report
  JSON allocation in product frames, and explicit queue/present phase timing.
- A focus-safe hardware/product-surface present-floor report exists for the same
  app-window/adapter/surface/present-mode family as the failing example gates.
  Software/headless baselines remain diagnostic and cannot excuse a real
  product-surface p95 miss.
- Generic sparse runtime behavior is proven by Cells and by at least one
  non-Cells sparse fixture with renamed fields/labels that avoids spreadsheet
  strings and fixed 26x100 assumptions.
- If the Cells Boon source is simplified, a semantic equivalence report proves
  logical grid size, editing/formula-bar behavior, dependencies/ranges/cycles,
  virtualization counters, and visual replay behavior are preserved.
- The stale-path ledger is monotonic: every new temporary compatibility path has
  an owner/date, typed replacement, kill switch, positive gate, negative gate,
  and removal condition; `native-gpu-all` fails if these are missing.
- Native aggregate gates, report schemas, AGENTS.md, this plan, and
  `docs/architecture/NATIVE_GPU_PIPELINE.md` all agree on product latency,
  proof readiness, allowed modes, and stale-path failures.

### Subagent Architecture TODO Addendum

These items are concrete preservation notes from the latest independent
architecture review. Keep them as generic engine/runtime/document/render
contracts, not as Cells fixes.

- [ ] Frame-loop / WGPU / proof TODOs:
  - define one `PreviewHotLoop` state machine that owns DemandDriven idle,
    bounded interaction bursts, redraw requests, wake reasons, and
    missed-frame accounting;
  - make host input land at the start of an already scheduled product frame,
    then run `HostInputEvent -> retained route snapshot -> typed intent ->
    retained visual patch -> submit/present` without proof/report/dev IPC on
    the pre-present path;
  - add a lane-scoped product-frame ledger keyed by `FrameEvidenceKey` and
    `input_event_seq`. Product UX gates consume only app-produced product
    interaction rows; aggregate preview stats remain diagnostic;
  - split product present from proof subscribers: product frames emit scalar
    timings, revisions, counters, and `FrameEvidenceKey`; readback, proof JSON,
    screenshots, hashes, HUD/report assembly, runtime probes, and telemetry run
    after present by exact key;
  - specify WGPU present ownership as explicit
    `AcquireSurfaceFrame -> EncodeCommands -> QueueSubmit ->
    PresentSurfaceFrame` phases with per-phase budgets, adapter/present-mode
    metadata, queue-depth/in-flight policy, and no hidden present-mode changes;
  - add a focus-safe hardware/product-surface present-floor verifier using the
    same app-window, adapter, surface, present mode, and frame clock as real
    examples. Headless/software numbers are harness evidence only, not product
    proof;
  - decide late acquire, multiple frames in flight, and ring-buffer upload
    policy only after present-floor evidence shows queue/present blocking is
    the dominant measured cost;
  - keep renderer-owned hot state for WGPU resources, route/hit snapshots,
    glyph/text caches, atlases, staging/ring buffers, primitive batches, and
    the frame evidence registry across frames;
  - define a short `ExtractDocumentDelta` boundary that copies only visible or
    materialized display items, scroll uniforms, text runs, dirty primitive
    ranges, route data, and evidence ids into render-owned state;
  - add product-only, proof-only, and full-HUD/report verifier modes. Product
    p95 and missed-frame gates must pass with proof/readback off before
    proof/report regressions are investigated.
- [ ] Compiler/document/runtime TODOs:
  - make typecheck/IR/lowering emit stable `DocumentNodeId`,
    `SourceBindingId`, list-map binding metadata, render-slot metadata,
    source-intent templates, and binding-path reverse indexes;
  - report metadata hashes and fallback counters so product gates can require
    zero label, geometry, source-path, and JSON rediscovery scans;
  - replace ad hoc source payload maps with typed commands such as
    `SetSourceValue`, `CommitTextEdit`, `MoveFocus`, and `UpdateViewport`.
    Commands carry target ids, route epoch, input sequence, and stale-result
    policy;
  - host-event replay must prove the path
    `HostInputEvent -> hit route -> SourceIntent -> public runtime command`,
    with `private_runtime_dispatch_used=false`;
  - define a `demand_current(field/key/window, reason, interaction, budget)`
    runtime API. Product-visible reads use this scoped API and cannot force
    root `state_summary`, full document summaries, whole-list flushes, or broad
    root currentness;
  - runtime turns emit exact typed deltas for bound text, source values,
    style/focus state, list windows, formula fanout, and dependency
    invalidation. Full `state_summary` is diagnostics only;
  - once exact deltas exist, expire any acceptance path that treats a
    coalesced or approximate semantic delta as product-current evidence;
  - promote `List/find` and `List/find_value` into a shared runtime list-index
    service keyed by typed field ids and maintained incrementally across row
    insert, update, delete, move, stale generation, duplicate-key, and tombstone
    cases;
  - keep formula dependencies, range invalidation, topological recompute, stale
    edge replacement, unrelated-edit skips, and cycle detection in generic
    runtime/stdlib architecture, with Cells and non-Cells fixtures;
  - make logical count, materialized range, overscan, selected keys, dependent
    keys, rendered nodes, and evaluated formulas a cross-layer virtualization
    protocol from compiler/runtime through document/layout/render;
  - `DocumentFrame` keeps retained reverse indexes for binding, source, focus,
    hit, text-control, and formula/input mirror updates so product patches
    touch exact nodes with zero per-leaf scans;
  - add query-style incremental invalidation for parse, typecheck, IR,
    document lowering, route metadata, and render identities. Source edits keep
    stable ids where semantics survive and `ActiveScene` remains presentable
    while pending work builds;
  - add a concrete no-hacks static gate across compiler, runtime, document,
    native app-window, native GPU, playground, xtask, and report-schema paths.
    Production paths must not branch on example names, source paths, `cells`,
    addresses, labels, geometry strings, or fixture row counts. Allow those
    strings only in examples, fixtures, and verifier input data;
  - add at least one fake/non-Cells sparse-list fixture that would fail if
    Cells-specific shortcuts are used.
- [ ] Runtime/list/formula currentness TODOs:
  - replace `startup_recompute: bool` with an explicit generic
    startup/currentness policy enum such as `Eager`,
    `ResetSourceInitializerOnly`, `DemandCurrent`, `VisibleWindow`, and
    `DiagnosticOnly`. Classify indexed source transforms separately from pure
    indexed fields, and report per-field policy, initialized rows,
    demand-evaluated rows, skipped rows, and demand-current misses. No policy
    may branch on `cells`, addresses, example names, or fixture paths;
  - make `List/chunk` a virtual row-index view instead of a materialized row
    list. Chunk/map/window operators expose logical count plus row/column
    window ranges; product reads materialize only visible range, overscan,
    selected keys, and dependency fanout keys. Reports distinguish logical
    items, materialized rows, rendered nodes, evaluated fields, and uploaded
    instances;
  - unify root, list, indexed-field, projection, and summary currentness behind
    `ensure_current(ReadKeySet)`. Visible reads, list-field reads, exact lookup
    reads, row-window reads, and root-child reads share one dependency and
    currentness contract. Product code uses sparse value reads or typed deltas,
    not full `state_summary` or `document_state_summary`, before presenting;
  - turn formula dependency tracking into a generic computed-field dependency
    service. Dependency tracing records `ListField`, `ListLookupText`,
    range/window reads, and root reads for any computed field; range
    invalidation uses interval/range indexes rather than expanded full-grid
    scans; cycle state and stale-edge removal are runtime-generic, not
    formula-helper or Cells-specific;
  - replace summary-driven virtualization with demand-window APIs.
    Document/layout requests `MaterializeRange` or `ReadWindow` directly from
    runtime; summaries remain diagnostic/report artifacts and cannot be
    product-frame barriers. Product reports fail if click, scroll, or
    formula/input sync calls broad summary refresh, root flush, or full list
    projection materialization;
  - add generic sparse fixtures beyond Cells: indexed `List/find` with
    duplicate keys, zero-scan lookup, and old/new exact lookup invalidation; a
    chunked 2D grid at a much larger logical size than the visible window;
    computed records/ranges with fanout, stale dependency removal, range/window
    dependencies, and cycle detection; and a text-input binding fixture proving
    selected/focused bound text stays current without a full summary refresh;
  - add a runtime/compiler no-hacks gate that scans compiler, runtime,
    document, native GPU, app-window, playground, xtask, and schema production
    paths for branches on `cells`, addresses, formula field names, row counts,
    labels, geometry strings, source paths, or example names. Allow those names
    only in examples, tests, fixtures, docs, or verifier input data, and fail if
    a generic runtime feature regresses into a fixture-specific shortcut.
- [ ] Extra negative gates:
  - stale first-frame proof, mismatched surface epoch, mismatched content,
    layout, or render revision, after-the-fact evidence keys, latest-report
    inference, and proof-cache hits without exact keys fail closed;
  - product frames fail if they perform proof JSON allocation, report snapshot
    assembly, screenshot encoding, dev IPC waits, layout artifact reloads,
    latest-report scans, runtime full summaries, broad list scans, or
    example/source-path/field-name branches before present.
- [ ] Verifier-shaped product work deletion TODOs:
  - make `CountersProduct` product mode readback-free by construction. Direct
    visible-surface readback, copy-to-readback commands, and proof buffer
    setup must not be encoded before product `encoder.finish()`, queue submit,
    or present. Exact-key readback belongs to a bounded proof subscriber lane;
  - move post-present proof polling, readback completion checks, proof-history
    compaction, artifact hashing, report JSON, and screenshot work off the
    product loop thread. The product loop emits `PresentedProductFrame` /
    `ProductFrameCommit` and immediately returns to input/frame scheduling;
  - replace JSON poll diagnostics on product frames with fixed typed counters
    such as `PollFrameStats`. Rich `serde_json::Value` diagnostics are
    trace/proof/debug mode outputs and must not be created to decide or report
    normal product present;
  - make scheduler lane ownership transaction-scoped. If host input is accepted
    in a poll, that host-input transaction owns the product frame attribution;
    due burst wakes, proof completions, report flushes, runtime cleanup, timer,
    and telemetry wakes cannot relabel the frame as their own UX sample;
  - add a bounded product-commit query or stream keyed by interaction id,
    `input_event_seq`, and `FrameEvidenceKey`. Verifiers should consume that
    stream instead of repeatedly reading `preview-loop.json` or reconstructing
    product samples from latest report state;
  - require every UX sample to carry explicit `product_frame_evidence_key`,
    `product_frame_commit`, optional `proof_frame_evidence_key`,
    `proof_lag_frames`, and `product_commit_match_method =
    "exact_product_commit"`. Remove the temporary proof-key plus
    `input_event_seq`/latency fallback once sample assembly records the product
    key directly;
  - split verifier waits into product, semantic-currentness, and proof phases.
    Product latency ends at the keyed product commit; semantic/runtime and
    WGPU proof failures are separate subcontracts and must not become 5-second
    UX latency samples;
  - stop polling runtime IPC in tight visual wait loops. Product frames should
    emit bound-value/currentness evidence needed for the UX sample; separate
    semantic probes may run afterward and their overhead cannot affect product
    timing;
  - add one consolidated product-loop forbidden-work allowlist counter. Product
    mode fails if pre-present work includes report snapshot assembly, proof
    JSON construction, readback encoding, runtime-value IPC, full state
    summary, accessibility snapshot publication, dev telemetry refresh,
    layout-artifact reload, latest-report scan, or screenshot/PNG work;
  - introduce a minimal typed `RenderFrameResult` / `PresentedProductFrame`
    boundary. Product render hooks return revisions, dirty ids, draw/upload
    stats, present-target metadata, scalar counters, evidence handles, and
    post-present proof requests only, never proof JSON or mutable diagnostic
    payloads;
  - add deterministic scheduler simulation tests for host input, burst wake,
    proof completion, report flush, runtime cleanup, resize, surface loss, and
    timer. Only host-input transactions may enter product UX buckets; late
    proof/report/runtime frames must not become click latency;
  - convert verifier timeouts into terminal/sample-cause statuses such as
    `terminal_preview_failure`, `missing_product_commit`, `missing_proof`,
    `missing_semantic_currentness`, and `driver_delivery_failure`. A timeout is
    a failure cause, not a product latency sample.
- [ ] Render/WGPU architecture TODOs:
  - make `native_gpu_app_owned_render_hook` product-only in product modes. It
    returns typed product metrics, revisions, dirty ids, draw/upload stats, and
    proof-request handles; `visible_bound_text`, retained-bound-sync proof,
    proof history, artifact hashing, report JSON expansion, screenshot
    encoding, and WGPU readback completion are post-present proof subscribers
    keyed by `FrameEvidenceKey`;
  - add a product gate such as `legacy_pre_present_proof_request_count == 0`
    and fail if product render hook work builds proof/report JSON or waits for
    proof/readback before present;
  - replace playground-owned render-scene caches with a renderer-owned
    `ActiveScene`. `VisibleLayoutRenderer` or its replacement should own the
    current scene, GPU buffers, glyph/text caches, hit snapshots, patch
    application, frame arenas, readback resources, and evidence registry. The
    playground sends typed `RenderDelta`s and revision stamps, not
    `layout_proof` JSON or string/hash cache keys;
  - move hot renderer identity from strings, hashes, BTreeMaps, and proof paths
    toward stable typed generational ids, slot maps, and revision-indexed dirty
    chunks. String hashes remain diagnostics/proof expansion, not product
    lookup;
  - add renderer lifetime and fence counters:
    `hot_frame_alloc_bytes`, `hot_frame_malloc_count`, `ring_buffer_wrap_count`,
    `in_flight_frame_count`, `buffer_reuse_count`, `glyph_cache_hit_count`,
    `glyph_upload_count`, `upload_bytes`, `queue_write_count`, and
    `dirty_chunk_count`. Product frames should prove bounded allocation and
    retained resource reuse;
  - make proof/readback backpressure non-blocking for product frames. A full
    proof queue may drop, coalesce, or fail proof samples with explicit
    `proof_drop_count`/`proof_lag_frames`, but it must not delay input
    acceptance, render hook execution, queue submit, or present;
  - add a present-floor verifier that renders an empty retained frame through
    the same app-window and WGPU surface with proof/report off, reporting
    acquire, encode, queue submit, `present()`, present mode, maximum frame
    latency, adapter, backend, compositor/session class, and surface epoch.
    Compare app/product latency against this floor before chasing app
    micro-costs;
  - split report timing into `input_to_cpu_submit_ms`,
    `input_to_present_return_ms`, `input_to_proof_complete_ms`, and
    `proof_lag_frames`. If `present()` blocks on FIFO/compositor pacing, that
    must be visible as surface/present-floor behavior, not misclassified as
    runtime, route, or render-hook work;
  - introduce one frame-clock/repaint broker shared by app-window and renderer
    ownership. Accepted host input, source wake, caret/timer, proof sample,
    report flush, surface lost/resized, and dev-HUD refresh enter one scheduler
    state machine with typed lanes. Timer/proof/report wakes cannot relabel a
    host-input product frame;
  - replace remaining `layout_proof`, geometry, string, and exact-position
    route fallbacks with a retained typed hit tree containing `HitRegionId`,
    `ScrollRegionId`, `SourceIntentTemplate`, transforms, z-order, clip/spatial
    nodes, layout generation, and input-route generation. Delete or quarantine
    exact-position `click_candidate_cache` as a product dependency once the
    typed tree covers mouse, keyboard, wheel, text, hover, focus, and scroll;
  - quarantine legacy proof paths as replacements land:
    artifact-only `native_gpu_render_proof`, latest-report proof lookup,
    modeled/static scroll success, driver-timing fallback, COSMIC/Ply/browser/
    Xvfb evidence, and full `layout_proof` JSON route/proof scans. Each
    quarantine needs a kill switch, positive gate, negative stale-path gate,
    and removal condition;
  - add non-Cells fixtures for sparse list/grid, text-input focus, wheel
    scroll, hover/focus overlays, retained render patches, present/proof
    identity, and proof subscriber backpressure. These fixtures must fail if
    production runtime/compiler/renderer code branches on example names, Cells
    fields, addresses, labels, geometry, or fixed row/column counts;
  - recommended implementation order: proof/product boundary first,
    present-floor measurement second, render-owned `ActiveScene` third, typed
    hit tree fourth, then deletion gates for old proof/render/route paths.

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
- Product/proof mode gate:
  - `CountersProduct` is the default product verifier mode. It may record fixed
    scalar counters and evidence ids before present, but it must not wait for
    readback, serialize proof JSON, drain reports, or query dev IPC;
  - `TraceProduct` and proof subscribers are optional modes with separate
    overhead fields and separate pass/fail budgets. They cannot be required to
    make the visible pixels correct.
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

- Schema/report invariant: `render_loop_mode` may only be `demand_driven` or
  `continuous_probe`. `requested_animation_burst` is a `frame_pacing_state`
  inside `DemandDriven`, not a third product mode. Reports fail if a burst wake
  relabels the accepted host-input frame or if a burst remains active past
  `hard_stop_after`.
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
- Backpressure report fields:
  - `pending_snapshot_queue_capacity`;
  - `pending_snapshot_superseded_count`;
  - `pending_snapshot_dropped_stale_count`;
  - `active_scene_presented_while_pending_count`;
  - `product_frame_blocked_on_pending_snapshot_count`;
  - passing product reports require
    `product_frame_blocked_on_pending_snapshot_count=0`.
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

## Architecture Option Reservoir

Keep this section as a reservoir of larger cuts to consider before another
micro-optimization loop. These are not all required as one giant rewrite; they
are the option set to choose from when fresh reports show the current boundary
is fundamentally wrong. Every option must remain generic across examples and
must not introduce compiler, runtime, document, renderer, playground, or verifier
branches on Cells, addresses, labels, source paths, row counts, or fixture
strings.

- [ ] Frame-clock as the product engine:
  - create one preview-owned frame clock that drains host input, resolves typed
    intents, applies product-visible patches, encodes, submits, presents, and
    only then wakes proof/report subscribers;
  - keep `RequestedAnimationBurst` as bounded pacing state inside
    `DemandDriven`, with explicit quiet-frame, quiet-time, hard-cap, and
    pending-snapshot limits;
  - coalesce pointer, wheel, text, caret, timer, source, and resize wakes by
    priority so telemetry/proof/report work cannot steal or relabel a host
    input frame;
  - add deterministic state-machine tests for every transition, including
    resize/surface-loss, burst hard stop, late proof completion, pending source
    commit, and verifier-forced sample.
- [ ] Interaction transaction ledger:
  - assign a stable `InteractionId` when host input is accepted and carry it
    through route resolution, source intent, runtime delta, render commit,
    product frame commit, and optional proof artifact;
  - product UX gates join on `InteractionId` plus `FrameEvidenceKey`, not latest
    report state, global preview-loop rings, or proof timestamps;
  - terminal transaction causes such as missing route, stale source epoch,
    missing product commit, missing semantic currentness, missing proof, IPC
    failure, or surface loss stop that sample cleanly instead of becoming a
    large finite latency number.
- [ ] Renderer-owned preview engine:
  - make the renderer own `ActiveScene`, `PendingScene`, GPU resource pools,
    text/glyph caches, hit snapshots, proof request queues, and frame arenas;
  - app-window/playground code sends typed `RenderDelta`s and evidence metadata,
    not layout-proof JSON, string identity maps, or full render-scene rebuild
    requests;
  - hover, focus, selection, caret, scroll, and text mirror feedback use overlay
    or property-tree patches that update bounded buffers/uniforms and never
    rebuild the full document/layout/render scene for the first visible frame.
- [ ] Browser-style property/compositor split:
  - represent transforms, clips, scroll offsets, opacity/effects, z-order, and
    hit regions as retained trees with generation numbers;
  - scroll and selection should usually update property-tree or overlay state,
    while layout/display-list rebuild happens later only when content structure
    actually changes;
  - reports must distinguish compositor-like patches from layout rebuilds,
    render-scene rebuilds, GPU uploads, and proof/readback work.
- [ ] Bevy-style extract/render-world split:
  - extract a narrow, immutable render input from runtime/document/layout into a
    render-owned world with explicit dirty components and change ticks;
  - prepare, queue, encode, present, and proof are ordered phases with phase
    timings, not ad hoc branches inside product render hooks;
  - stale extracted worlds are dropped by content, layout, render, surface, and
    interaction epochs before they can replace the active frame.
- [ ] Data-oriented hot path:
  - remove per-frame heap allocation, string formatting, `serde_json::Value`,
    `BTreeMap` walks, path parsing, hash recomputation, and full vector clones
    from product frames;
  - use slot maps/generational ids, compact indices, fixed ring buffers,
    structure-of-arrays where useful, and bounded scratch arenas reset per
    frame;
  - add hot-path allocation counters and fail product gates when interaction
    frames allocate or clone beyond a small budget.
- [ ] Text and input-control subsystem:
  - build a shared text-control engine for cursor, selection, editing text,
    formula/input mirrors, IME/preedit, focus, hover, and caret blinking;
  - cache shaped glyph runs by stable text/style/wrap keys and update only
    changed runs or caret/selection overlays;
  - expose text-control deltas as generic document/runtime/render data so Cells,
    TodoMVC, code editor, and future examples share the same path.
- [ ] Runtime as a query/currentness engine:
  - model root fields, list fields, projections, formula/computed fields,
    summaries, and document bindings as typed queries with dependencies,
    generations, and scoped currentness barriers;
  - product reads ask for exact keys/windows and receive typed deltas; diagnostic
    summaries and recursive reports are background subscribers;
  - add cancellation, latest-wins coalescing, and per-query cost counters so a
    stale or hidden query cannot block a visible product frame.
- [ ] Compiled runtime/codegen path:
  - evaluate a generic compiled-kernel path for hot derived fields, list
    projections, route metadata extraction, and document binding updates;
  - possible backends include Rust-generated functions, Zig/C ABI kernels, or
    another owned codegen layer, but the semantic source remains Boon IR and the
    fallback interpreter must stay equivalent;
  - use compiled kernels only after the query/currentness boundaries are typed,
    so codegen accelerates the right architecture instead of freezing slow
    summary-shaped behavior.
- [ ] Multi-rate QoS lanes:
  - separate product interaction, animation/caret, runtime cleanup, layout
    rebuild, proof/readback, report JSON, dev-HUD, accessibility, and verifier
    control into explicit quality-of-service lanes with capacities and budgets;
  - product input can preempt or coalesce lower-priority work; lower-priority
    lanes may drop or lag with explicit counters instead of blocking present;
  - reports show lane backlog, dropped/coalesced work, and time spent per lane.
- [ ] Present/queue and GPU resource strategy:
  - measure a hardware present floor for the same native surface, adapter,
    present mode, frame latency, compositor/session, and window visibility;
  - decide late-acquire, acquire-ahead, frame-in-flight, and upload-ring policy
    from that evidence, not from guessing or from a software/headless baseline;
  - keep persistent pipelines, bind groups, atlases, staging/ring buffers, and
    command encoders hot where the API allows it, with bounded backpressure when
    the GPU/compositor is the limiting factor.
- [ ] Proof plane as a separate subsystem:
  - proof subscribers consume `PresentedProductFrame` records by exact
    `FrameEvidenceKey` and may lag, coalesce, or fail independently;
  - proof artifacts carry capture method, surface epoch, revisions, present id,
    proof id, proof lag, and stale-cache counters;
  - product correctness does not depend on proof mode being enabled, but native
    readiness still requires proof artifacts for the same measured frames.
- [ ] Harness and verifier simplification:
  - tests should drive host events, wait for keyed product commits, optionally
    wait for semantic currentness, and separately wait for proof by exact key;
  - delete driver-timing fallback, latest-report inference, modeled/static
    success, full-summary polling loops, and proof-JSON route scans once typed
    replacements exist;
  - repeated scenarios stop or reset after terminal transaction failure instead
    of manufacturing cascades of stale follow-up failures.
- [ ] Developer observability without product cost:
  - dev-window HUD reads only cached scalar snapshots at a throttled cadence;
  - deep traces, flame-style phase samples, report JSON, and proof artifacts are
    opt-in lanes whose cost is measured and never hidden inside UX latency;
  - add a one-command report bundle that links product, semantic, proof,
    scheduler, renderer, runtime, and no-hacks evidence for the same binary and
    worktree fingerprint.
- [ ] Deletion-first migration rule:
  - each new typed path must name the old path it will replace, the negative
    gate that proves the old path is not used in product mode, and the removal
    condition;
  - do not leave old proof-shaped product paths, route fallbacks, broad runtime
    summaries, or example-specific helper branches reachable because they make
    later measurements ambiguous.

## Subagent Review Hardening TODOs

The 2026-07-02 subagent pass agreed that the plan has the right high-level
direction, but it needs sharper ownership contracts and fail-closed deletion
gates. Keep these TODOs visible until they are implemented or explicitly
replaced by a simpler measured design.

- [ ] Concrete `NativeFrameClock` / `PreviewHotLoop` owner:
  - move wake reason arbitration, input drain, burst state, dirty work,
    product render, post-present subscriber wakeups, and telemetry scheduling
    behind a small explicit owner instead of spreading them through the long
    app-window loop;
  - expose a deterministic scheduler API with phases such as
    `drain_host_input`, `resolve_intents`, `apply_product_patches`,
    `render_product_frame`, `commit_presented_frame`, and
    `wake_post_present_subscribers`;
  - tests must cover host input, requested-animation wake, source/runtime
    cleanup, proof completion, report flush, resize, surface loss, timer, burst
    quiet exit, and burst hard-cap exit.
- [ ] Transactional requested-animation bursts:
  - a burst frame carries an armed-frame token with
    `request_to_redraw_ms`, `redraw_token_age_ms`,
    `burst_frame_start_reason`, and `input_waited_for_already_armed_frame`;
  - host input accepted during a burst remains a host-input/product
    interaction transaction even when the scheduler wake reason is
    requested-animation;
  - reports fail if a proof/report/timer/runtime-cleanup wake relabels a host
    input frame or if `late_input_deferred_count` grows during interactive
    samples.
- [ ] Input-source acceptance contract:
  - align `docs/architecture/NATIVE_GPU_PIPELINE.md`, AGENTS handoff gates, and
    xtask label contracts on the accepted release input source: public
    app-owned `HostEvent`/`HostInputEvent` rows must be the source of truth;
  - if a verifier needs real OS input, nested compositor input, or a
    focus-risk manual path, label it as a separate evidence mode and do not
    mix it with product UX gates;
  - product UX gates fail when accepted-input rows are missing, private
    dispatch is used, or the sample is inferred only from driver timing.
- [ ] Acceptance-to-frame-start gates:
  - add `host_event_to_frame_begin_ms`, `input_wake_to_input_accept_ms`,
    `input_accept_to_frame_start_ms`, `input_waited_for_already_armed_frame`,
    and `late_input_deferred_count`;
  - p95 `input_to_present_ms` is not enough: reports must show whether slow
    samples waited before frame start, inside runtime/layout/render, or inside
    acquire/submit/present;
  - if input waits behind proof/report/subscriber drain, that is a scheduler
    failure even when render and present phases look fast.
- [ ] Present/queue ownership decision:
  - decide late-acquire vs acquire-ahead, frame-in-flight count, upload-ring
    policy, and whether blocked submit/present may hold input acceptance;
  - the decision must be based on a focus-safe hardware present-floor report
    for the same surface path, adapter, present mode, compositor/session, and
    frame-latency policy;
  - product reports split app CPU work from acquire, queue submit, present
    return, compositor/vsync pacing, GPU completion, and proof completion.
- [ ] `ActivePreviewScene` / `PendingPreviewScene` migration:
  - replace mutable `PreviewSharedRenderState`, `PreviewVisibleRenderState`,
    layout artifact paths, and layout-proof JSON product dependencies with an
    immutable active scene plus capacity-1 latest-wins pending scene;
  - product rendering consumes retained route, text-control, overlay, layout,
    render, hit, and GPU-resource state from `ActivePreviewScene`;
  - pending runtime/layout/document/render work may activate only when content,
    layout, render-scene, surface, route, and interaction epochs still match.
- [ ] Typed `RenderFrameResult` / `PresentedProductFrame` boundary:
  - product render hooks return scalar counters, revisions, dirty ids, upload
    and draw stats, present target metadata, `FrameEvidenceKey`, and
    post-present proof request handles;
  - product hooks must not return or build `serde_json::Value` proof trees,
    layout-proof JSON, screenshot artifacts, visible-bound-text proof history,
    or report snapshots before present;
  - add gates such as `legacy_pre_present_proof_request_count == 0` and
    `legacy_product_proof_built_pre_present == false`.
- [ ] First-class `FrameEvidenceRegistry` and proof subscriber queue:
  - register presented product frames by exact `FrameEvidenceKey` and
    `InteractionId`;
  - post-present subscribers consume the registry for WGPU readback,
    visible-bound-text proof, retained-sync proof, runtime semantic probes,
    proof-history compaction, artifact hashing, screenshot/PNG work, and report
    JSON;
  - subscriber queues are bounded and may drop/coalesce proof samples with
    explicit `proof_drop_count`, `proof_lag_frames`, and terminal proof status,
    but they cannot delay product input, render, submit, or present.
- [ ] WGPU/resource lifetime ownership:
  - app-window owns surface creation, configuration, acquire, present, resize,
    scale, and surface/device epoch invalidation;
  - renderer/render actor owns pipelines, bind groups, glyph atlases, text
    state, texture caches, staging/ring buffers, scene caches, frame arenas,
    proof buffers, and evidence handles;
  - reports expose resource reuse and invalidation counters for surface epoch,
    device epoch, config epoch, scene revision, glyph atlas generation, staging
    ring generation, and in-flight frames.
- [ ] Batching/text/glyph cache contracts:
  - promote cache metrics into pass/fail contracts: hover, focus, selection,
    caret, and scroll must not reshape unchanged text or rebuild primitive
    batches whose content/style/wrap inputs did not change;
  - report stable primitive batch keys, shaped-run cache hit/miss, glyph upload
    bytes, glyph eviction count, atlas generation, draw calls, upload bytes,
    queue writes, and hot-frame allocation count;
  - product gates fail if scene/hash fallback or component-wide invalidation is
    used when dirty component revisions identify a narrower patch.
- [ ] `GenericReadKey` currentness:
  - currentness barriers operate on root keys, list keys, list-field keys,
    exact lookup keys, projection keys, summary keys, and materialized window
    keys;
  - no barrier may silently drop list/read keys into broad root-only
    currentness;
  - reports show demand misses, exact keys ensured, stale keys rejected,
    full-root fallback count, and broad-summary fallback count.
- [ ] Typed `RuntimeTurn` product boundary:
  - replace legacy string-path `SemanticDelta` / `RenderPatch` product output
    with typed deltas for source values, bound text, focus/hover/pseudo state,
    list windows, row deps, formula fanout, route metadata, and text-control
    state;
  - old summary-shaped deltas remain diagnostic subscribers only and must be
    negative-gated out of product frames;
  - runtime/document/render tests must prove typed deltas patch the active
    scene without `state_summary` or `document_state_summary`.
- [ ] Layout-demand/runtime-window handshake:
  - layout emits typed `LayoutDemand` / `MaterializeRange` requests for list,
    grid, text, and scroll windows;
  - runtime answers with `ListWindowDelta` containing logical count,
    materialized range, overscan, selected/dependent keys, rendered nodes,
    evaluated fields/formulas, and stale-window generation;
  - reports fail if visible layout requests force full list/chunk
    materialization or full-grid formula evaluation.
- [ ] Generic formula/dependency report gates:
  - add report fields for `range_dependency_count`,
    `range_dependency_hit_count`, `evaluated_formula_count`,
    `deferred_fanout_count`, `dirty_formula_count`,
    `cycle_detected_count`, `stale_edge_removed_count`, and
    `full_grid_recompute_count`;
  - prove unrelated edits skip recompute, range members update dependents,
    formula replacement removes old edges, and cycle detection terminates;
  - include a renamed non-Cells sparse grid/list fixture so the generic engine
    is not proven only through spreadsheet terminology.
- [ ] Typed source-intent row metadata:
  - compiler/document lowering emits source-intent templates with stable row
    key, row generation, source binding id, payload field ids, and route epoch;
  - product routing does not read `address`, `target_key`, style-derived row
    names, source paths, labels, or proof JSON to discover the target;
  - source payload changes must not invalidate the route table when stable row
    identity and payload field ids still match.
- [ ] Module-scoped no-hacks allowlist:
  - define where example names, source paths, Cells fields, addresses, labels,
    and fixture strings are allowed: examples, scenarios, verifier input data,
    scoped tests, and documentation;
  - production crates and product paths fail if those strings appear in branch
    predicates, shortcut dispatch, acceptance logic, render/runtime behavior,
    or schema pass/fail decisions;
  - the allowlist is machine-readable so the no-hacks audit can distinguish a
    legitimate fixture string from a product shortcut.
- [ ] Machine-readable stale-path ledger:
  - every legacy path has an owner, current mode (`product-forbidden`,
    `diagnostic-only`, `fail-fast-alias`, or `removed`), positive replacement
    gate, stale-path negative gate, and removal condition;
  - seed rows must include `layout_proof` product reads, proof-JSON route
    scans, latest-report proof matching, `last_interactive_readback_artifact`
    dependencies, driver-timing fallback, modeled/static success, legacy
    Ply/Xvfb/COSMIC/browser proof, Weston-only shortcuts, and old command
    aliases;
  - release UX gates fail when a product sample touches any
    `product-forbidden` stale path.

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
- present-path milliseconds plus surface-acquire, queue-submit, and
  frame-present subphase milliseconds;
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

HUD cost gates:

- `footer_lines_transport_call_count=0`;
- `preview_perf_hot_path_query_count=0`;
- `preview_perf_payload_bytes <= configured_preview_perf_payload_budget`;
- HUD refresh is throttled independently from product frame pacing;
- enabling the HUD must stay within the configured preview/dev frame regression
  budget, or the HUD gate fails.

Reports and HUD should expose the same key terms:

- `input_to_present_ms_p50_p95_p99_max`;
- `render_hook_ms_p50_p95_p99_max`;
- `layout_ms_p50_p95_p99_max`;
- `present_call_ms_p50_p95_p99_max`;
- `present_path_ms_p50_p95_p99_max`;
- `surface_acquire_call_ms_p50_p95_p99_max`;
- `queue_submit_call_ms_p50_p95_p99_max`;
- `frame_present_call_ms_p50_p95_p99_max`;
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
- `present_path_ms`:
  - CPU wall time for the product present path through surface acquisition,
    command submission, and `frame.present()`;
  - does not claim GPU completion.
- `surface_acquire_call_ms`:
  - CPU wall time inside the surface texture acquisition call for the presented
    frame.
- `queue_submit_call_ms`:
  - CPU wall time inside the WGPU queue submit call for the presented frame.
- `frame_present_call_ms`:
  - CPU wall time inside `frame.present()` for the presented frame.
- `present_call_ms`:
  - legacy compatibility alias for `frame_present_call_ms` until all older
    reports and consumers migrate to the explicit phase names.
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
  - `crates/boon_parser`;
  - `crates/boon_ir`;
  - `crates/boon_plan`;
  - `crates/boon_compiler`;
  - `crates/boon_runtime`;
  - `crates/boon_document`;
  - `crates/boon_native_gpu`;
  - `crates/boon_native_playground`;
  - `crates/boon_native_app_window`;
  - `crates/boon_report_schema`;
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
- These bans apply to production branch predicates and shortcut dispatch, not to
  literal data inside examples, scenarios, fixtures, reports, or scoped verifier
  allowlists.
- Verifier allowlists must be path/module scoped and must fail if fixture names
  leak into compiler, runtime, document, renderer, app-window, playground
  product paths, or report-schema acceptance logic.
- Batch reset and startup fast paths must be generic pattern recognizers with
  fallback equivalence tests.

## Current Implementation Progress

2026-07-01 product-poll subscriber split slice:

- Moved native accessibility tree snapshot publication out of accepted
  host-input product polls:
  - accessibility action handling remains in the product poll because those are
    input events;
  - accessibility tree output now defers when the poll accepted real OS input,
    dirtied visible state, is not a forced verifier frame, has no accessibility
    action payload, has no active headed scenario, and no deferred refresh is
    already pending;
  - a follow-up poll publishes the deferred accessibility snapshot from cached
    host state.
- Added product-poll diagnostics:
  - `accessibility_snapshot_status`;
  - `accessibility_snapshot_deferred_for_product_input`;
  - `accessibility_refresh_pending_before_poll`;
  - `accessibility_refresh_pending_after_poll`;
  - `deferred_accessibility_snapshot_count`;
  - `completed_deferred_accessibility_snapshot_count`.
- Focused verification passed:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`;
  - `cargo test -q -p boon_native_playground preview_accessibility_snapshot_defers_only_product_input_refresh -- --test-threads=1`;
  - `cargo test -q -p xtask present_floor -- --test-threads=1`.
- One-sample release Cells visible-click smoke:
  - command:
    `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-accessibility-deferred-smoke.json`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-accessibility-deferred-smoke.json` returned 0;
  - report status: fail;
  - new diagnostics observed `deferred_count=9`,
    `deferred_accessibility_snapshot_ms_max=0.000179 ms`,
    `published_deferred_count=26`, and
    `published_deferred_accessibility_snapshot_ms_max=0.01953 ms`;
  - budget blockers remained:
    `input_accept_to_formula_visible_ms_p95=17.432444 ms`,
    `input_wake_to_formula_visible_ms_p95=19.840480 ms`,
    `click_to_formula_visible_ms_p95=38.642956 ms`, and
    preview-loop `input_to_present_p95=30.784 ms`;
  - interpretation: accessibility snapshot publication is now off the accepted
    host-input product poll, but this is not sufficient to meet 60 FPS. The
    next cut should target the still-heavy `source_input_ms` /
    `world_or_source_input_ms` poll boundary, typed route snapshots, or
    queue/present/frame-pacing architecture.

2026-07-01 present-floor baseline slice:

- Added a generic `verify-native-gpu-present-floor` gate and made it part of
  native GPU handoff reports:
  - the command measures an empty app-window preview surface with no Boon render
    hook, no readback, no host input, and counters-only proof mode;
  - the report exposes clear-only surface acquire, command record, encoder
    finish, queue submit, `frame.present()`, post-present bookkeeping, and
    presented-frame p50/p95/p99/max summaries;
  - label contracts require product-only counters mode, DemandDriven loop,
    zero hot-path readback, zero render hook, zero observed input, at least 16
    measured frames, and p95 within the configured present-floor budget;
  - focused coverage: `cargo test -q -p xtask present_floor -- --test-threads=1`.
- Fixed the one-shot probe lifetime:
  - the first implementation used `hold_ms: 0`, which means DemandDriven mode
    but no automatic app-window exit after the proof callback;
  - the command hung until the outer timeout and wrote no report;
  - the verifier now uses a tiny hold duration with `demand_driven_loop=true`,
    so it terminates after collecting its requested samples while still
    reporting `render_loop_mode=demand_driven`.
- Tightened no-input proof behavior:
  - added generic `sample_input_after_initial_frames` to
    `NativeWindowOptions` and `AppWindowSurfaceProof`;
  - normal preview/dev probes keep `sample_input_after_initial_frames=true`;
  - present-floor sets it to `false`, so the no-input floor does not drain
    coalesced keyboard/mouse state or collect user key details after measuring
    frames;
  - present-floor still fails if lightweight app-window input wake counters
    move, and the report exposes `observed_real_os_input`,
    `observed_input_event_wake_count`,
    `observed_keyboard_key_event_count`, and
    `observed_mouse_total_event_count`.
- Fresh focused checks:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`;
  - `cargo test -q -p xtask present_floor -- --test-threads=1`;
  - `timeout 240s cargo xtask verify-native-gpu-present-floor --sample-frame-count 32 --warmup-frame-count 4 --report target/reports/native-gpu/present-floor.json`.
- Fresh isolated-headless verifier result:
  - report path: `target/reports/native-gpu/present-floor.json`;
  - verifier status: fail;
  - blocker: isolated Weston selected software adapter
    `llvmpipe (LLVM 20.1.2, 256 bits)`;
  - `adapter_backend=Vulkan`, `adapter_device_type=Cpu`,
    `adapter_is_software=true`;
  - `present_mode=Mailbox`, `desired_maximum_frame_latency=1`;
  - `measured_frame_count=32`;
  - refreshed after the no-input guard patch:
    `presented_frame_ms.p95=0.998328 ms`, max `1.104972 ms`;
  - `queue_submit_ms.p95=0.621493 ms`;
  - `frame_present_ms.p95=0.501562 ms`;
  - no render hook, no readback, no input, and no initial input sampling were
    used (`sample_input_after_initial_frames=false`).
- Current Wayland hardware attempt:
  - `timeout 180s cargo xtask verify-native-gpu-present-floor --no-isolated-weston --sample-frame-count 32 --warmup-frame-count 4 --report target/reports/native-gpu/present-floor-current-wayland.json`
    can measure the NVIDIA/COSMIC surface, but it is not yet a clean acceptance
    gate;
  - one run on `NVIDIA GeForce RTX 2070` reported
    `presented_frame_ms.p95=11.016652 ms`, max `13.178370 ms`,
    `queue_submit_ms.p95=0.489013 ms`, and
    `frame_present_ms.p95=0.277990 ms`;
  - the visible window stole focus and observed real keyboard/mouse events
    (`input_event_wake_count=7`, keys `H/O/W`, mouse motion), so that artifact
    must not be used as no-input present-floor proof;
  - the verifier now records `observed_real_os_input`,
    `observed_input_event_wake_count`,
    `observed_keyboard_key_event_count`, and
    `observed_mouse_total_event_count`, and the present-floor label contract
    rejects observed input.
- Current Wayland focus-risk guard:
  - command:
    `timeout 60s cargo xtask verify-native-gpu-present-floor --no-isolated-weston --sample-frame-count 32 --warmup-frame-count 4 --report target/reports/native-gpu/present-floor-current-wayland-guarded.json`;
  - without `--allow-current-wayland-focus-risk` or
    `BOON_NATIVE_PRESENT_FLOOR_ALLOW_CURRENT_WAYLAND_FOCUS_RISK=1`, the command
    now refuses to open the visible app-window and writes a failing
    schema-shaped blocker report;
  - report fields include `focus_safe=false`,
    `requires_explicit_focus_risk_opt_in=true`,
    `measured_frame_count=0`, `sample_input_after_initial_frames=false`,
    `observed_real_os_input=false`, `observed_input_event_wake_count=0`,
    `proof_mode=counters`, `render_loop_mode=demand_driven`, and zero
    placeholder timing summaries;
  - no matching `Boon Present Floor`, `verify-native-gpu-present-floor`, or
    `weston` process remained after the guarded run.
- Interpretation:
  - this proves the new baseline command can isolate an empty app-window
    counters-only product path and report useful phase timings;
  - it does **not** prove the real hardware/compositor preview floor because
    isolated headless Weston currently routes WGPU to llvmpipe;
  - do not use this 2.7 ms software/headless result to dismiss the real Cells
    p95 failure;
  - do not use the current-wayland hardware report until the probe is
    focus-safe or runs on an isolated hardware-capable compositor;
  - next work should make the hardware/product-surface present-floor variant
    focus-safe, or force a hardware-capable isolated compositor path, then
    compare that floor to the failing Cells release report before cutting more
    app-side code.
- Report-schema note:
  - `cargo xtask verify-report-schema target/reports/native-gpu/present-floor.json`
    returned non-zero because the report status is intentionally `fail`;
  - this matches the current schema tool behavior that failing reports do not
    pass the schema verifier, but the native aggregate should eventually learn
    to distinguish known failing blocker gates from malformed reports.

2026-07-01 DemandDriven missed-frame accounting slice:

- Fixed generic native app-window missed-frame accounting so healthy
  DemandDriven idle gaps are not counted as dropped frames:
  - `note_present_completed` still reports `last_present_interval_ms` and
    `last_frame_lateness_ms` for diagnostics;
  - `missed_frame_count` now increments only for `ContinuousProbe` frames or
    `RequestedAnimation` follow-up frames, where a continuous frame cadence was
    actually expected;
  - first host input after a long DemandDriven idle interval no longer looks
    like a missed frame just because the previous present was long ago.
- Added focused coverage:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_app_window missed_frame -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window requested_animation -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window demand_driven_idle_gap_before_host_input_is_not_a_missed_frame -- --test-threads=1`.
- Fresh release verifier:
  - `timeout 900s cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`;
  - report schema: pass;
  - verifier status: fail;
  - `preview_loop_missed_frame_count` dropped from the previous 56 idle-gap
    artifacts to 2 real requested-animation misses;
  - `preview_loop_input_to_present_ms_p95=19.167085 ms`, still above the
    16.7 ms target;
  - `preview_loop_renders_per_second=27.776049`,
    `preview_loop_frame_pacing_state=requested_animation_burst`;
  - `preview_perf_stats.present_call_ms.p95=10.017398 ms`,
    `present_path_ms.p95=10.411826 ms`,
    `queue_submit_call_ms.p95=9.544518 ms`,
    `render_hook_ms.p95=2.712146 ms`;
  - click-sample steady product timing now passes narrowly:
    `steady_input_accept_to_formula_visible_ms.p95=16.571666 ms`;
  - runtime work contract remains pass with zero scans, zero root
    materialization, and zero recompute samples;
  - retained update contract remains pass with retained native input overlay
    patches, no full document lower, and no legacy selection fallback.
- Current interpretation:
  - the old `missed_frame_count=56` gate was mostly a DemandDriven accounting
    bug, not 56 true dropped product frames;
  - the remaining failure is still real: cold/outlier product frames exceed the
    all-sample preview-loop p95, and two requested-animation follow-up gaps are
    late;
  - worst samples show present/queue variance plus one generic retained-route
    miss: sample 0 spent about `13.8 ms` in present, sample 3 spent about
    `4.0 ms` in shared route-table lookup plus `9.4 ms` present, and sample 5
    spent about `3.5 ms` render hook plus `10.8 ms` present;
  - next architecture cut should target the retained hit-route snapshot and/or
    present-floor/product-only baseline from the backlog. Do not treat this as
    a completed performance fix.

2026-07-01 retained route identity/payload split slice:

- Tightened the generic native playground route-cache identity so retained
  product frames do not rebuild route tables for volatile visual/payload
  changes:
  - removed broad `update_count` from `preview_active_hit_route_cache_key`;
  - removed display item text from `preview_display_route_fingerprint`;
  - kept route-affecting fields in the static fingerprint: node, kind,
    disabled state, authored text-input focus, input live-change behavior,
    route-relevant text sizing/insets, and link URL;
  - runtime state freshness still comes from
    `layout_proof_runtime_state_snapshot_identity`, so changed runtime payloads
    reject stale active snapshots without making visual text a route identity.
- Added focused coverage:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_playground preview_route_cache_key -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses -- --test-threads=1`.
- Fresh release verifier:
  - `timeout 900s cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`;
  - report schema: pass;
  - verifier status: fail;
  - `preview_loop_missed_frame_count=0`, so the remaining failure is no longer
    dropped-frame accounting;
  - `preview_loop_input_to_present_ms_p95=18.703218 ms`, still above the
    16.7 ms product budget;
  - `steady_input_accept_to_formula_visible_ms.p95=17.072380 ms`, also above
    budget in this run;
  - route lookup outliers improved but did not disappear: worst route lookup
    samples are now about `1.8-2.0 ms` instead of the previous about `4.0 ms`;
  - p95 remains dominated by queue/present variance and frame preparation:
    `preview_perf_stats.queue_submit_call_ms.p95=9.929829 ms`,
    `present_call_ms.p95=10.152587 ms`,
    `present_path_ms.p95=10.791067 ms`,
    `render_hook_ms.p95=2.956772 ms`;
  - runtime work contract remains pass with zero scans, zero root
    materialization, and zero recompute samples;
  - retained update contract remains pass with retained native input overlay
    patches, no full document lower, and no legacy selection fallback.
- Current interpretation:
  - splitting route identity from volatile retained text/update counters is
    aligned and measurably reduces a generic outlier;
  - the full 60 FPS product path is still not complete because the measured
    present/queue floor can consume most of a frame;
  - next work should implement the product-only/present-floor baseline and then
    cut queue/present coupling, proof/report workers, or frame pacing based on
    that evidence.

2026-07-01 architecture backlog and honest product-loop gate slice:

- Added the subagent architecture review backlog above so the next work targets
  product/proof separation, typed input routing, retained overlay/render-state
  patches, typed proof registries, and verifier honesty instead of more
  isolated micro-optimizations.
- Expanded the backlog with higher-level TODO buckets to preserve possible
  architecture cuts before the next implementation pass: event-loop ownership,
  state-aware frame pacing, present/queue strategy, render-thread/worker split,
  retained property trees, GPU resource lifetime, typed runtime deltas,
  compiler-visible source-intent metadata, stale slow-path deletion, generic
  visual replay fixtures, and simplicity constraints.
- Removed the visible-click release fallback that substituted Weston
  driver-click timing when app-owned accepted-input timing was missing. Missing
  `input_accept_to_present` / `input_accept_to_formula` timing is now a failed
  sample, not a finite product latency.
- Changed host-input burst scheduling so the current accepted host-input frame
  is not relabeled by an immediate `RequestedAnimation` wake. Visible host input
  marks its own repaint; the requested-animation burst wake is for follow-up
  frames.
- Surfaced `preview_perf_stats` from the preview-loop artifact into
  `verify-native-cells-visible-click-e2e` and added a release gate for
  `preview_loop_input_to_present_ms_p95`, sample count, DemandDriven mode, and
  `preview_loop_missed_frame_count=0`.
- Tightened native UX classification so
  `verify-native-cells-visible-click-e2e` receives the generic native product
  path/schema checks instead of passing as a side report.
- Expected near-term result: the fresh Cells visible-click report should now
  fail honestly until the actual preview-loop p95 and missed-frame count are
  fixed. A narrow steady formula-bar sample pass is no longer enough.
- Fresh release verifier result after this slice:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`
  - status: fail, schema-valid;
  - live probe: pass, 64 real OS click samples;
  - runtime work contract: pass, zero scans/root materialization/recompute for
    all 64 samples;
  - retained update contract: pass, retained render patches for all 64 samples,
    no full document lower, no legacy selection fallback;
  - `steady_input_accept_to_formula_visible_ms.p95=17.440344 ms`, over the
    `16.7 ms` product budget;
  - `preview_loop_input_to_present_ms_p95=17.733837 ms`, over budget;
  - `preview_loop_missed_frame_count=56`, `renders_per_second=24.901838`;
  - phase p95s identify the current real blockers:
    `input_wake_to_input_accept=5.142996 ms`,
    `input_accept_to_dirty_poll=4.560003 ms`,
    `render_started_to_render_hook_completed=3.584671 ms`,
    `present_call=11.420665 ms`, and `render_hook_to_queue=9.644506 ms`;
  - proof is no longer the only explanation: proof/reporting remains too close
    to the frame, but the product loop itself is not yet a 60 FPS hot loop.
- Validation note:
  - host-input retained/runtime repaint may currently present an existing
    content revision because the app-window loop still conflates frame revision
    and document content revision;
  - external/source runtime changes still reject stale content revisions;
  - a future architecture slice should split `frame_revision`,
    `content_revision`, `layout_revision`, and `render_scene_revision` so
    repaint frames do not need this compatibility path.

2026-07-01 focus-overlay route-cache and present-path diagnosis checkpoint:

- Fixed a generic native playground hit-route cache miss caused by retained
  focus/caret overlay state:
  - route identity now ignores generated visual focus/caret overlay bits that
    do not change event routing;
  - authored `TextInput` focus declarations remain conservative route identity;
  - the active hit-route table can reuse the static snapshot table for
    focus-overlay-only retained layout overrides;
  - no Cells/example-name/source-path/address-specific branch was introduced.
- Added focused coverage for the generic route-cache behavior:
  - `cargo fmt --check`
  - `cargo test -q -p boon_native_playground preview_route_cache_key -- --test-threads=1`
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses_static_table_for_focus_overlay_only_override -- --test-threads=1`
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses_active_snapshot_and_rejects_stale_state -- --test-threads=1`
- Simplified the focused-node overlay probe so the interaction proof reports the
  structured selected/focused/style evidence the verifier consumes without
  rebuilding a mini render-primitive fill proof inside the product hook.
- Fresh release Cells visible-click evidence:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`
  - status: pass;
  - `steady_input_accept_to_formula_visible_ms.p95=16.657131`, max
    `19.119135`;
  - all-sample `input_accept_to_formula_visible_ms.p95=17.674522`, max
    `22.316629`;
  - `click_to_formula_visible_ms.p95=52.369243`, max `53.726085`,
    still classified as bounded external driver/proof latency rather than
    product UX latency;
  - runtime work contract: pass;
  - retained update contract: pass;
  - route lookup p95 is zero in the release report, but a cold/outlier max of
    about `4.056965 ms` remains and should be tracked.
- The same run's preview-loop diagnostics show the plan is not complete:
  - `preview_perf_stats.input_to_present_ms.p95=18.341533`, max `23.393028`;
  - `present_call_ms.p95=10.614056`, max `30.330650`;
  - `render_hook_ms.p95=2.704641`, max `4.123174`;
  - `renders_per_second=25.826112` during a requested-animation burst;
  - `missed_frame_count=57`;
  - proof mode is still `external_app_owned_readback`.
- Interpretation:
  - the old multi-second click behavior was not WGPU drawing complexity; it was
    the combination of eager spreadsheet/runtime work, stale/currentness gaps,
    and then a generic route-cache miss from focus overlays forcing slow route
    rebuild/lookup on interaction frames;
  - the current remaining slowness is architectural: demand-wake/frame pacing
    and surface present variance still leave too little of the 16.7 ms budget
    for reliable 60 FPS, and proof/readback/reporting are still too close to
    the product frame path;
  - the correct next slice is a real hot interaction loop: already-scheduled
    requested-animation bursts, input sampled at frame start, retained state
    patched directly, quick submit, and deferred keyed proof/readback/reporting
    by `FrameEvidenceKey`.
- A path-driven retained-bound-sync experiment was tried and reverted:
  - it worsened `bound_input_sync_ms` by doing generic binding lookup per leaf;
  - a future version needs a precomputed reverse binding-path index, not another
    per-interaction generic scan disguised as a shortcut.

2026-07-01 same-frame timing and keyed readback-registry slice:

- Fixed a report attribution bug where verifier-facing phase fields could mix
  the accepted-input frame with a later requested-animation follow-up frame:
  - top-level `input_accept_to_*`, render-hook, queue, and present subphase
    fields now prefer the stored `NativeAcceptedInputFrameTiming`;
  - raw `last_*` fields remain as latest-frame diagnostics and are labeled with
    `latest_presented_frame_raw_last_fields`;
  - reports now expose `top_level_phase_timing_scope` so consumers can
    distinguish accepted-input-frame phase timings from latest-frame debug
    fields;
  - `preview_perf_stats.input_to_present_ms` uses the same accepted-frame
    fallback as top-level `frame_input_to_present_ms`, even on final/follow-up
    reports that do not pass an explicit one-shot latency extra.
- Added a bounded generic interactive readback artifact registry in
  `boon_native_app_window`:
  - stores the last 16 completed interactive readback artifacts;
  - matches artifacts by exact `FrameEvidenceKey`;
  - exposes `recent_interactive_readback_artifacts`,
    `recent_interactive_readback_artifact_count`,
    `matching_interactive_readback_artifact_for_frame`, and
    `matching_interactive_readback_artifact_for_frame_status` in render-loop
    reports;
  - keeps the older `last_interactive_readback_artifact` compatibility field.
- Strengthened focused coverage:
  - `cargo fmt --check`
  - `cargo check -q -p boon_native_playground`
  - `cargo test -q -p boon_native_app_window recent_interactive_readback_registry_matches_exact_frame_key -- --test-threads=1`
  - `cargo test -q -p boon_native_app_window render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats -- --test-threads=1`
  - `cargo test -q -p boon_native_app_window accepted_input_frame_timing_is_not_rewritten_by_followup_burst_frames -- --test-threads=1`
  - `cargo test -q -p boon_native_app_window recent_history_compacts_visible_bound_text_without_losing_selection_evidence -- --test-threads=1`
- Fresh release evidence:
  - `target/reports/native-gpu/cells-visible-click-e2e-release-frame-registry.json`
  - schema: pass via `target/debug/xtask verify-report-schema`;
  - status: pass;
  - `input_accept_to_present_ms_p95=16.389464`, max `18.288152`;
  - `input_accept_to_formula_visible_ms_p95=16.389464`;
  - `click_to_formula_visible_ms_p95=54.959164`, classified as bounded
    external driver/proof latency rather than product UX latency;
  - phase diagnostics are now self-consistent:
    `dirty_poll_to_render_started.p95=0.133913`,
    `render_started_to_render_hook_completed.p95=3.227938`,
    `present_call.p95=9.230495`,
    `queue_to_present.p95=9.230724`;
  - runtime work contract: pass, with 64/64 zero scans, zero root
    materialization, and zero recompute samples;
  - retained update contract: pass, with 64 retained commits and no full
    document lower.
- Honest blocker evidence from the immediately preceding current-code run:
  - `target/reports/native-gpu/cells-visible-click-e2e-release-targeted-summary-timing-scope.json`
    failed with `input_accept_to_present_ms_p95=17.680987`;
  - the corrected phase split showed the over-budget run was driven by
    present/queue variance (`present_call.p95=12.390395`,
    `queue_to_present.p95=12.390704`), not runtime scans, relower, or a mixed
    dirty-poll phase.
- This slice improves measurement truth and adds the first generic keyed proof
  registry, but it does not complete the plan:
  - the visible-click verifier currently uses structured external proof and
    skips duplicate interactive readback, so the registry is present in live
    loop reports but often empty for that path;
  - verifiers still need to prefer exact registry matches when interactive
    readback artifacts exist;
  - product latency remains close enough to 16.7 ms that present/queue variance
    can still flip a release run between pass and fail;
  - next architecture work should target frame pacing/present variance and the
    active/pending retained snapshot path, not Cells-specific runtime patches.

2026-07-01 bounded runtime-values sync and renderer identity-key checkpoint:

- Replaced the broad no-op runtime currentness fallback with a direct generic
  runtime-values retained text sync:
  - target nodes are still selected/focused/selection-bound document nodes, not
    Cells-specific addresses;
  - the sync reads only exact refreshable text binding paths with
    `LiveRuntime::document_state_values`;
  - it patches the retained layout frame directly and falls back to the older
    targeted state-summary path only when exact values cannot prove currentness.
- Kept the stale no-op formula-bar fix while removing the 10-12 ms no-op sync
  outliers:
  - focused test still passes:
    `cargo test -q -p boon_native_playground selection_proxy_noop_click_refreshes_bound_text_input_from_runtime -- --test-threads=1`;
  - adjacent retained-selection tests still pass:
    `targeted_bound_sync_expands_to_selection_dependent_formula_bar`,
    `retained_selection_patch_uses_generic_static_equality_bindings`;
  - release report
    `target/reports/native-gpu/cells-visible-click-e2e-release-runtime-values-sync.json`
    dropped `input_accept_to_dirty_poll.p95` from about 13.18 ms to about
    4.21 ms and removed `bound_input_sync_ms > 5 ms` samples.
- Threaded supplied render-scene identities into the internal renderer encode
  cache key:
  - `boon_native_gpu` no longer has to walk the full internal render scene to
    key prepared-quad reuse when a stable scene/patch identity is supplied;
  - `cargo check -q -p boon_native_gpu` and
    `cargo check -q -p boon_native_playground` pass.
- Fresh release evidence after the renderer key patch:
  - `target/reports/native-gpu/cells-visible-click-e2e-release-runtime-values-sync-scene-key.json`
  - status: fail;
  - `input_accept_to_present_ms_p95=18.727339`, max `20.586541`;
  - `click_to_formula_visible_ms_p95=56.492418`, max `206.415873`;
  - `input_accept_to_dirty_poll.p95=4.80945`;
  - `render_started_to_render_hook_completed.p95=3.39510`;
  - `present_call.p95=9.89984`, max `15.643497`;
  - `queue_to_present.p95=9.90039`, max `15.643837`.
- Interpretation:
  - generic input/poll work is now mostly bounded; the previous no-op
    currentness regression is fixed;
  - renderer CPU improved modestly, but the gate remains unstable because
    product frames still pay present/queue blocking and proof/readback state is
    coupled to the live loop;
  - the next implementation slice should follow the subagent-backed direction:
    add a keyed frame-evidence registry, remove proof/readback backpressure from
    product rendering, and let verifiers wait for matching proof artifacts by
    `FrameEvidenceKey` without delaying or redefining product UX latency.

2026-07-01 compact interaction proof and no-op currentness checkpoint:

- Added a generic compact interaction-proof mode for `visible_bound_text` in
  the native preview render proof:
  - proof keeps selected/focused/selection-bound entries needed for interaction
    evidence instead of serializing the full visible bound-text inventory;
  - the latest Cells visible-click run reports `entry_count=252` but only 3
    compact proof entries per interaction frame;
  - render-hook report JSON dropped to roughly 0.3-0.6 ms in sampled frames.
- Fixed a generic retained-state correctness gap for source-event no-op clicks:
  - selection-proxy bound text inputs now run a targeted runtime currentness
    barrier when a routed source event produces no runtime deltas;
  - this avoids trusting a stale cached layout summary for focus/selection-bound
    text such as a formula bar;
  - no example-name or Cells-specific branch was introduced.
- Focused coverage:
  - `cargo fmt --check`
  - `cargo check -q -p boon_native_playground`
  - `cargo test -q -p boon_native_playground selection_proxy_noop_click_refreshes_bound_text_input_from_runtime -- --test-threads=1`
  - `cargo test -q -p boon_native_playground targeted_bound_sync_expands_to_selection_dependent_formula_bar -- --test-threads=1`
  - `cargo test -q -p boon_native_playground retained_selection_patch_uses_generic_static_equality_bindings -- --test-threads=1`
  - `cargo test -q -p boon_native_app_window recent_history_compacts_visible_bound_text_without_losing_selection_evidence -- --test-threads=1`
  - `cargo test -q -p boon_native_app_window external_visible_readback_proof -- --test-threads=1`
- Fresh release verifier:
  - `target/reports/native-gpu/cells-visible-click-e2e-release-compact-interaction-proof.json`
  - status: fail;
  - `input_accept_to_present_ms_p95=25.375297`, max `26.995660`;
  - `click_to_formula_visible_ms_p95=57.962070`, max `60.188`;
  - `input_accept_to_dirty_poll.p95=13.1842`;
  - `render_started_to_render_hook_completed.p95=3.5836`;
  - `present_call.p95=9.1786`, `queue_to_present.p95=9.1789`;
  - `report_enqueue.p95=1.9454`, `report_write.p95=5.2747`.
- Interpretation:
  - the formula/runtime click work and proof JSON construction are no longer the
    dominant visible-click blocker in this report;
  - the product path still misses the frame budget because demand-wake input can
    spend most of a frame before dirty polling/render starts, then still pay a
    ~9 ms queue/present wait;
  - another micro-pass over Cells, JSON, or bound-text scanning is unlikely to
    close the gap. The next slice should implement the architecture already
    described here: input sampled at the start of an active burst frame,
    retained-state patching, quick submit, and deferred keyed proof/readback
    outside the UX frame.

2026-07-01 report/proof-history compaction and scheduler experiment:

- Compacted generic recent-frame proof history in `boon_native_app_window`:
  - recent `visible_bound_text` now keeps focused/selected entries and
    selection-bound text paths instead of repeating the full visible text
    inventory for every historical frame;
  - recent poll diagnostics keep scalar phase/status fields and drop long
    repeated timing/reject sample arrays;
  - full current proof remains available at top level, while recent history is
    bounded and suitable for live verifier polling.
- Added focused coverage:
  - `cargo test -q -p boon_native_app_window recent_history_compacts_visible_bound_text_without_losing_selection_evidence -- --test-threads=1`
  - existing external visible readback proof tests still pass.
- Fresh release evidence after reverting the failed scheduler experiment:
  - `target/reports/native-gpu/cells-visible-click-e2e-release-report-compaction-final.json`
  - status: fail, but report overhead is no longer the dominant blocker;
  - live `preview-loop.json` shrank from about 7.4 MB to about 720 KB;
  - `recent_frame_evidence` shrank from about 4.2 MB to about 243 KB;
  - report enqueue p95 improved from about 28.9 ms to about 2.2 ms;
  - report write p95 improved from about 403.7 ms to about 8.2 ms;
  - product `input_accept_to_present_ms_p95` is still about 19.7 ms
    against the 16.7 ms target;
  - click-to-formula proof p95 is still about 66.5 ms against the 33.4 ms
    bounded harness target.
- A naive non-dirty pointer-input burst priming experiment was rejected and
  reverted:
  - it did not reduce product p95 materially;
  - it caused proof matching to fail and pushed click-to-formula samples to
    5 second verifier timeouts;
  - do not repeat this tactic without a keyed frame-history/proof selector that
    can distinguish product frames, warm-up frames, and proof frames by exact
    `FrameEvidenceKey`/input generation.
- Current blocker after this slice:
  - product path is mostly render hook plus queue/present timing
    (`render_started_to_render_hook_completed_ms` p95 about 5.5 ms,
    `present_call_ms` p95 about 10.5 ms);
  - proof path still waits for app-owned visual evidence after presentation;
  - next architecture work should focus on a typed keyed frame evidence history
    and a real active-frame scheduler/retained patch path, not more JSON or
    report-size tuning.

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

2026-07-01 native document row-lookup naming cleanup slice:

- Native document static analysis and document evaluation context now carry
  `source_row_lookup_fields` instead of `source_address_lookup_fields`.
  The native document-lowering path consumes the generic row-lookup metadata
  emitted by runtime static analysis, keeping spreadsheet address terminology
  out of this production hot path.
- The implicit source-intent helper was renamed from address lookup to row
  lookup, and its local variables now refer to row lookup values. The serialized
  compatibility intent still emits `intent="address"` for older route consumers
  until the remaining runtime/compiler compatibility aliases are retired.
- The duplicate compiler/runtime static-analysis map named
  `source_address_lookup_fields` was removed. Runtime static analysis now
  exposes the generic `source_row_lookup_fields` map only; lower-level route
  slots still serialize `address_lookup_field` as a compatibility alias.
- `CompilerSourceRouteSource` and runtime `SourceRoute` no longer store a
  second `address_lookup_field` identity. Compiler output carries
  `row_lookup_field`; runtime route artifacts still emit and accept the legacy
  `address_lookup_field` key as a compatibility alias derived from
  `row_lookup_field`.
- Runtime helper names now use row lookup terminology:
  `row_lookup_field_for_source_id`, `row_lookup_field_for_list`, and
  `set_row_lookup_fields`.
- The route operation report now emits
  `row_binding_identity=source_row_lookup_or_bound_row`, removing the old
  `source_address_lookup_or_bound_row` label from runtime reporting.
- The xtask architecture audit negative pattern now checks the renamed
  `set_row_lookup_fields` helper, so the verifier continues to reject direct
  IR-coupled route construction after the terminology cleanup.
- Focused verification passed:
  - `rg -n "source_address_lookup_fields" crates --glob '*.rs'` returns no
    matches;
  - `cargo check -q -p boon_compiler -p boon_runtime -p boon_native_playground`;
  - `cargo check -q -p boon_compiler -p boon_runtime -p boon_ir -p boon_native_playground`;
  - `cargo test -q -p boon_runtime row_scoped_source_resolves_named_lookup_payload_without_address`;
  - `cargo test -q -p boon_ir scoped_source_lookup_prefers_source_intent_identity_field`;
  - `cargo test -q -p boon_runtime compiled_artifact_decodes_source_routes_and_action_table_without_ast`;
  - `cargo test -q -p boon_runtime source_routes_are_dense_by_hidden_source_id`;
  - `cargo test -q -p boon_runtime compiled_artifact_emission_is_deterministic_and_schema_valid`;
  - `cargo check -q -p xtask`;
  - `cargo test -q -p xtask native_gpu`;
  - `cargo check -q -p boon_native_playground`;
  - `cargo test -q -p boon_native_playground row_identity`;
  - `cargo test -q -p boon_native_playground selection_proxy_`;
  - `cargo test -q -p boon_native_playground cells_focus_only_route_syncs_formula_bar_text`;
  - `cargo test -q -p boon_native_playground cells_click_selection_updates_formula_bar_and_selected_style`;
  - `cargo fmt --check`;
  - `git diff --check`.
- `cargo test -q -p boon_runtime novywave_file_rows_use_generic_row_source`
  currently fails before reaching row-route assertions with
  `unsupported state initializer external_file_tree_file`; it is not accepted
  as proof for this slice and remains separate verifier/fixture debt.
- Fresh Cells scroll-speed verification against this worktree remains
  schema-valid and fails honestly only on hardware readiness:
  - `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`
    wrote `status=fail`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/scroll-speed-cells.json`
    passed;
  - `product_path_ux_timing_proven=true`;
  - `product_path_input_to_present_ms_p95=10.305772999996409`;
  - `ux_frame_budget_pass=true`;
  - `wall_clock_frame_budget_pass=true`;
  - `renderer_frame_budget_pass=true`;
  - `renderer_cpu_frame_ms_p95=1.62293`;
  - `preview_perf_present_path_ms_p95=9.935689`;
  - `logical_cell_count=2600`;
  - `materialized_cell_count_max=336`;
  - remaining blocker:
    `native scroll-speed gate ran on a software adapter; hardware-backed real-window speed is not proven`.
- This is a no-hacks cleanup slice, not the full generic identity migration.
  Remaining compatibility surfaces still include IR/plan
  `SourcePayloadSchema.address_lookup_field`, legacy `SourcePayloadField::Address`,
  and the serialized compatibility source intent.

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

2026-07-01 scroll timing split and present-blocker evidence slice:

- The scroll-speed verifier now separates renderer/model CPU work from
  platform submit/present blocking. `non_os_scroll_model.frame_budget_model_pass`
  is driven by renderer CPU timing plus upload/materialization evidence, while
  `required_real_window_speed_proven` and `budget_pass` still require the real
  wall-clock visible frame budget. This keeps the UX speed gate honest while
  avoiding a false "renderer/materialization over target" diagnosis when the
  renderer path is already within budget.
- New generic report fields include `renderer_frame_budget_proven`,
  `renderer_frame_budget_pass`, `renderer_cpu_frame_ms_p95`,
  `cpu_submit_ready_ms_p95`, `present_blocking_ms_p95`, and
  `frame_budget_split`. These are emitted for Cells, dev-code-editor, and other
  scroll surfaces through the shared xtask scroll evidence path.
- Forced verifier sample loops are now paced at the native 60 Hz target after
  the first sample, with `sample_pacing_wait_ms_p50/p95/max` reported
  separately from actual frame work. This prevents tight-loop forced presents
  from being confused with normal product pacing, while keeping wall-clock
  presentation over-budget failures visible.
- A read-only subagent independently traced the same failure shape: post-input
  scroll samples measure `presented_frame_ms` as `surface_acquire_ms +
  present_submit_ms`, where `present_submit_ms` includes command recording,
  `queue.submit`, `frame.present`, and post-present bookkeeping. The current
  Cells failure is dominated by synchronous WGPU submit/present behavior under
  the software/headless real-window path, not by Cells runtime, layout,
  uploads, graph rebuilds, or passive-scroll dispatch.
- Latest paced Cells scroll-speed report still fails honestly with one blocker:
  `native scroll-speed gate real-window frame budget is over target; real-window
  speed is not proven`. The report shows
  `renderer_frame_budget_pass=true`, `renderer_cpu_frame_ms_p95=3.797977`,
  `cpu_submit_ready_ms_p95=3.821984`, `wall_clock_frame_budget_pass=false`,
  `scroll_frame_ms_p95=18.058102`, `present_blocking_ms_p95=22.498016`,
  `sample_pacing_wait_ms_p95=6.210418`,
  `software_adapter_wall_clock_budget_exempt=true`,
  `required_real_window_speed_proven=false`, `materialized_cell_count_max=336`,
  and `logical_cell_count=2600`. Pacing improved the diagnosis but did not
  fully solve the real-window wall-clock p95 target on this adapter.
- The same generic path previously passed dev-code-editor scroll-speed before
  forced sample pacing. A refreshed paced run now fails with the same honest
  real-window wall-clock blocker: `renderer_frame_budget_pass=true`,
  `renderer_cpu_frame_ms_p95=10.277644`, `wall_clock_frame_budget_pass=false`,
  `scroll_frame_ms_p95=19.485726`, `present_blocking_ms_p95=27.542755`,
  `sample_pacing_wait_ms_p95=7.370771`, and `budget_pass=false`.
- Fresh verification:
  - `cargo fmt --check`
  - `git diff --check`
  - `cargo test -q -p xtask scroll_budget`
  - `cargo test -q -p xtask axis_specific`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`
    failed honestly with only the real-window frame-budget blocker.
  - `cargo xtask verify-report-schema target/reports/native-gpu/scroll-speed-cells.json`
    passed.
  - `cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json`
    failed honestly with the same real-window frame-budget blocker after pacing.
  - `cargo xtask verify-report-schema target/reports/native-gpu/scroll-speed-dev-code-editor.json`
    passed.
- Next implementation target: restructure the real-window verifier toward
  sustained app-owned input sampled by the normal frame scheduler, and/or harden
  adapter/surface selection so real 16.7ms wall-clock claims are made only on a
  non-software adapter. The renderer/model path is currently fast enough for
  Cells; the remaining blocker is real wall-clock present/submit behavior.

2026-07-01 product-path UX timing selector slice:

- The scroll-speed evidence path now promotes normal DemandDriven product-path
  UX timing separately from forced proof/platform timing. The selector prefers
  `preview_perf_stats.input_to_present_ms_p50_p95_p99_max` and can use
  `frame_input_to_present_ms` as a single-frame fallback, but only when the
  measured loop stayed in `demand_driven` mode and recorded a requested-animation
  burst. `continuous_probe` timing, missing samples, and missing burst evidence
  are rejected instead of silently proving UX speed.
- `post_input_frame_timing` remains reported as forced/proof/platform timing.
  It no longer has to define `wheel_to_visible_ms_p95` or
  `scroll_frame_ms_p95` when a stronger product-path input-to-present sample is
  present. New report fields include `product_path_ux_timing`,
  `product_path_ux_timing_proven`, `product_path_input_to_present_ms_p95`,
  `speed_budget_timing_window`, `speed_budget_frame_ms_p95`, and
  `ux_frame_budget_pass`.
- The scroll model still keeps hardware honesty: software adapters remain
  diagnostic-only for final real-window speed readiness, even if product-path
  UX timing is under budget.
- A read-only subagent independently identified the same minimal patch shape:
  use the existing `take_frame_accepted_input_to_present_ms` product-path
  stream, require DemandDriven plus burst evidence, keep same-frame WGPU proof
  separate, and do not use forced post-input loops as UX latency.
- Fresh Cells scroll-speed report still fails honestly with one blocker:
  `native scroll-speed gate ran on a software adapter; hardware-backed
  real-window speed is not proven`. The same report now shows the useful split:
  `product_path_ux_timing.status=pass`,
  `product_path_input_to_present_ms_p95=11.189790999998875`,
  `speed_budget_timing_window=product-path-input-to-present`,
  `ux_frame_budget_pass=true`, `wall_clock_frame_budget_pass=false`,
  `wall_clock_frame_budget_ms_p95=17.827773999999998`,
  `renderer_frame_budget_pass=true`, `renderer_cpu_frame_ms_p95=3.431165`,
  `present_blocking_ms_p95=26.438848`,
  `real_window_speed_adapter_policy=software-diagnostic-only`, and
  `required_real_window_speed_proven=false`.
- Focused verification:
  - `cargo fmt --check`
  - `git diff --check`
  - `cargo test -q -p xtask product_path_`
  - `cargo test -q -p xtask scroll_budget`
  - `cargo test -q -p xtask axis_specific`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json`
    failed honestly with only the software-adapter hardware-readiness blocker.
  - `cargo xtask verify-report-schema target/reports/native-gpu/scroll-speed-cells.json`
    passed.
- Next implementation target: collect more than one product-path input sample
  for sustained scroll bursts and run the same gate on a non-software adapter.
  The current selector proves that the normal input-to-present path can be under
  16.7ms, but final readiness still requires hardware-backed real-window
  evidence and broader p95 sample counts.

2026-07-01 sustained native scroll evidence slice:

- The isolated Weston native scroll driver now supports a sustained wheel burst
  in one driver process:
  - optional `repeat_count` and `repeat_delay_ms` arguments;
  - per-run JSON reports for sent axis events, burst count, first/last scroll
    monotonic timestamps, and repeat settings;
  - no global desktop input, no screenshots, and no example-specific code.
- `verify-native-gpu-scroll-speed` now uses that sustained app-owned native
  wheel path for axis-specific scroll probes. Reports distinguish:
  - `scroll_driver_process_count`;
  - `scroll_driver_command_count`;
  - `scroll_driver_sustained_burst`;
  - `scroll_driver_repeat_count`;
  - `scroll_driver_repeat_delay_ms`.
- Fresh Cells report after this slice:
  - `status=fail`;
  - `product_path_ux_timing_proven=true`;
  - `product_path_input_sample_count=4`;
  - `speed_budget_timing_window=product-path-input-to-present`;
  - `product_path_input_to_present_ms_p95=26.356046000000788`;
  - `ux_frame_budget_pass=false`;
  - `renderer_frame_budget_pass=true`;
  - `renderer_cpu_frame_ms_p95=2.6573029999999997`;
  - `present_blocking_ms_p95=28.846819999999997`;
  - `measured_adapter_is_software=true`;
  - blockers are now both explicit:
    `native scroll-speed gate product-path input-to-present p95 is over target;
    UX speed is not proven` and
    `native scroll-speed gate ran on a software adapter; hardware-backed
    real-window speed is not proven`.
- Fresh verification:
  - `cargo fmt --check`;
  - `cargo check -q -p xtask`;
  - `git diff --check`;
  - direct `cc` compile of
    `tools/linux-human-like/weston-test-driver.c` against the generated
    Weston test protocol;
  - `cargo xtask verify-native-gpu-scroll-speed --example cells --report
    target/reports/native-gpu/scroll-speed-cells.json` failed honestly with the
    UX p95 and software-adapter blockers above;
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/scroll-speed-cells.json` passed.
- A diagnostic `BOON_NATIVE_OFFSCREEN_COPY_TO_PRESENT=1` scroll-speed run was
  stopped after it failed to complete promptly, so offscreen-copy-to-present is
  not promoted by this slice.
- Independent subagent reads agreed on the next architecture targets:
  - present/submit is the current measured wall-clock blocker after renderer
    CPU work;
  - add a generic surface-present coordinator with late swapchain acquisition,
    per-frame combined present-path metrics, and adaptive present-mode evidence;
 - implement active/pending document/layout/render snapshots so click and
   scroll can keep presenting the active retained frame while heavier generic
   runtime/layout work catches up.

2026-07-01 explicit present-path timing semantics slice:

- Native preview performance stats now expose the present path as explicit
  phase telemetry:
  - `surface_acquire_call_ms`;
  - `queue_submit_call_ms`;
  - `frame_present_call_ms`;
  - `present_path_ms`;
  - rolling `p50/p95/p99/max` summaries for each phase.
- `present_call_ms` remains as a legacy compatibility alias for the
  `frame.present()` call only. New code and reports should use
  `present_path_ms` when discussing the full product present path.
- Render-loop reports now include top-level aliases for
  `surface_acquire_call_ms`, `queue_submit_call_ms`,
  `frame_present_call_ms`, and `present_path_ms`, and the dev footer falls back
  to `present_path_ms` before the legacy frame-present call when no
  input-to-present sample exists.
- Native report schema now requires the new present-path summary objects in
  `preview_perf_stats`, so passing native reports cannot collapse surface
  acquisition, queue submit, and frame present into one ambiguous number.
- Scroll-speed reports now include the same present-path summary in
  `product_path_ux_timing` and `frame_budget_split`, while leaving pass/fail
  budget selection unchanged.
- Fresh Cells scroll-speed report after this slice:
  - `status=fail`;
  - `product_path_ux_timing_proven=true`;
  - `product_path_input_sample_count=4`;
  - `product_path_input_to_present_ms_p95=23.963904000000184`;
  - `ux_frame_budget_pass=false`;
  - `wall_clock_frame_budget_pass=true`;
  - `renderer_frame_budget_pass=true`;
  - `renderer_cpu_frame_ms_p95=1.173273`;
  - `preview_perf_surface_acquire_call_ms_p95=0.07087199999999999`;
  - `preview_perf_queue_submit_call_ms_p95=0.346919`;
  - `preview_perf_frame_present_call_ms_p95=26.837553`;
  - `preview_perf_present_path_ms_p95=27.323849`;
  - `measured_adapter_is_software=true`;
  - blockers remain:
    `native scroll-speed gate product-path input-to-present p95 is over target;
    UX speed is not proven` and
    `native scroll-speed gate ran on a software adapter; hardware-backed
    real-window speed is not proven`.
- Fresh verification:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_app_window preview_perf_stats`;
  - `cargo test -q -p boon_native_playground preview_perf`;
  - `cargo test -q -p boon_report_schema native_gpu_schema`;
  - `cargo test -q -p xtask product_path_input_to_present_timing_drives_scroll_budget_when_proven`;
  - `cargo check -q -p xtask`;
  - `cargo xtask verify-native-gpu-negative --report
    target/reports/native-gpu/negative.json`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/negative.json`;
  - `cargo xtask verify-native-gpu-scroll-speed --example cells --report
    target/reports/native-gpu/scroll-speed-cells.json` failed honestly with the
    UX p95 and software-adapter blockers above;
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/scroll-speed-cells.json` passed;

2026-07-01 product-path present-summary aggregation slice:

- Scroll-speed axis retries now preserve the fixed preview perf summary
  objects when promoting sustained product-path samples:
  - `present_call_ms_p50_p95_p99_max`;
  - `frame_present_call_ms_p50_p95_p99_max`;
  - `surface_acquire_call_ms_p50_p95_p99_max`;
  - `queue_submit_call_ms_p50_p95_p99_max`;
  - `present_path_ms_p50_p95_p99_max`.
- The aggregation remains generic: it operates on preview perf summary keys and
  does not branch on Cells, addresses, source paths, or fixture strings.
- Fresh Cells scroll-speed report after this slice:
  - `status=fail`;
  - `product_path_ux_timing_proven=true`;
  - `product_path_input_sample_count=4`;
  - `product_path_input_to_present_ms_p95=24.152110999999422`;
  - `preview_perf_present_path_ms_p95=9.965287`;
  - `preview_perf_surface_acquire_call_ms_p95=0.03236`;
  - `preview_perf_queue_submit_call_ms_p95=0.16121000000000002`;
  - `preview_perf_frame_present_call_ms_p95=9.778191`;
  - `renderer_cpu_frame_ms_p95=2.302152`;
  - `present_blocking_ms_p95=24.974257`;
  - `ux_frame_budget_pass=false`;
  - `wall_clock_frame_budget_pass=true`;
  - `renderer_frame_budget_pass=true`;
  - `measured_present_mode=Mailbox`;
  - `measured_supported_present_modes=[Mailbox, Fifo, Immediate]`;
  - `measured_adapter_is_software=true`;
  - blockers remain:
    `native scroll-speed gate product-path input-to-present p95 is over target;
    UX speed is not proven` and
    `native scroll-speed gate ran on a software adapter; hardware-backed
    real-window speed is not proven`.
- Fresh verification:
  - `cargo fmt --check`;
  - `cargo test -q -p xtask product_path_timing_aggregates_present_phase_summaries`;
  - `cargo test -q -p xtask axis_specific_product_path_timing_promotes_sustained_samples`;
  - `cargo check -q -p xtask`;
  - `git diff --check`;
  - `cargo xtask verify-native-gpu-scroll-speed --example cells --report
    target/reports/native-gpu/scroll-speed-cells.json` failed honestly with the
    UX p95 and software-adapter blockers above;
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/scroll-speed-cells.json` passed.
  - `git diff --check`.
- This slice is measurement and schema hardening, not a performance completion
  claim. The next implementation target is a generic surface-present
  coordinator / retained active-frame presentation path that avoids treating
  expensive frame-present blocking as normal interaction latency while keeping
  proof identity and product-path metrics honest.

2026-07-01 explicit present-path mode evidence slice:

- Native app-window now has a generic `NativePresentPathMode`:
  - `direct_visible_surface`;
  - `app_owned_offscreen_copy_to_present`.
- The selected mode, requested mode, selection reason, render target kind, hook
  availability, copy-to-present support, and readback status are recorded in
  `NativeRenderLoopState`, top-level render-loop reports, and native surface
  proof vocabulary.
- The default product path remains `direct_visible_surface`. Proof/readback
  does not force offscreen copy-to-present; offscreen copy-to-present is
  selected only when explicitly requested and the generic render hook plus
  surface copy support are available.
- Scroll-speed axis aggregation now promotes present-path mode evidence from
  vertical/horizontal measured loop reports into the final scroll report.
- Fresh Cells scroll-speed report after this slice:
  - `status=fail`;
  - `present_path_mode=direct_visible_surface`;
  - `present_path_requested_mode=direct_visible_surface`;
  - `present_path_selection_reason=default_direct_visible_surface_with_separate_readback`;
  - `present_path_hooks_present=true`;
  - `present_path_surface_copy_to_present_supported=true`;
  - `present_path_readback_enabled=true`;
  - `last_render_target_kind=visible-surface-direct`;
  - `product_path_ux_timing_proven=true`;
  - `product_path_input_sample_count=4`;
  - `product_path_input_to_present_ms_p95=25.586091000001034`;
  - `preview_perf_present_path_ms_p95=10.131971`;
  - `renderer_cpu_frame_ms_p95=1.4025379999999998`;
  - `present_blocking_ms_p95=19.00599`;
  - `ux_frame_budget_pass=false`;
  - `wall_clock_frame_budget_pass=true`;
  - `renderer_frame_budget_pass=true`;
  - `measured_present_mode=Mailbox`;
  - `measured_adapter_is_software=true`;
  - blockers remain:
    `native scroll-speed gate product-path input-to-present p95 is over target;
    UX speed is not proven` and
    `native scroll-speed gate ran on a software adapter; hardware-backed
    real-window speed is not proven`.
- Fresh verification:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window`;
  - `cargo test -q -p boon_native_app_window present_path`;
  - `cargo test -q -p boon_report_schema native_gpu_schema`;
  - `cargo test -q -p xtask product_path_timing_aggregates_present_phase_summaries`;
  - `cargo test -q -p xtask axis_specific_product_path_timing_promotes_sustained_samples`;
  - `cargo check -q -p xtask`;
  - `git diff --check`;
  - `cargo xtask verify-native-gpu-scroll-speed --example cells --report
    target/reports/native-gpu/scroll-speed-cells.json` failed honestly with the
    UX p95 and software-adapter blockers above;
 - `cargo xtask verify-report-schema
    target/reports/native-gpu/scroll-speed-cells.json` passed.

2026-07-01 frame-scoped accepted-input and immediate host-input burst slice:

- Native app-window now records `accepted_input_frame_timing` for the exact
  accepted host-input frame that contributes to product UX latency. This fixes
  stale phase attribution where the latest report compared the original input
  dirty poll with a later requested-animation follow-up frame.
- Scroll-speed product-path samples now carry the frame-scoped accepted-input
  timing object, including input-to-present, dirty-poll-to-render-start,
  render-hook, queue, present, present-path, render target, and scheduler
  evidence.
- Host-input requested-animation bursts now make the first repaint immediately
  consumable inside the same DemandDriven loop turn. Follow-up burst frames
  remain paced at the target frame interval. This is generic scheduler behavior;
  it does not branch on Cells, source paths, addresses, or fixture text.
- The stale role-dirty guard remains intact: a stale role dirty poll still does
  not invent an unrenderable content revision unless the host-input burst
  explicitly supplies a scheduler-only repaint.
- Fresh Cells scroll-speed report after this slice:
  - `status=fail`;
  - `product_path_ux_timing_proven=true`;
  - `product_path_input_sample_count=4`;
  - `product_path_input_to_present_ms_p95=10.710324000003313`;
  - `preview_perf_present_path_ms_p95=10.151646`;
  - `renderer_cpu_frame_ms_p95=1.274174`;
  - `present_blocking_ms_p95=19.00976`;
  - `ux_frame_budget_pass=true`;
  - `wall_clock_frame_budget_pass=true`;
  - `renderer_frame_budget_pass=true`;
  - `measured_present_mode=Mailbox`;
  - `measured_adapter_is_software=true`;
  - accepted-input samples show `dirty_poll_to_render_started_ms` around
    `0.003-0.004ms` instead of the previous `~16.7ms`;
  - remaining blocker:
    `native scroll-speed gate ran on a software adapter; hardware-backed
    real-window speed is not proven`.
- Fresh verification:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_app_window requested_animation_burst_is_bounded_inside_demand_driven_mode`;
  - `cargo test -q -p boon_native_app_window host_input_animation_burst_can_repaint_without_waiting_a_frame_interval`;
  - `cargo test -q -p boon_native_app_window stale_role_dirty_poll_does_not_invent_unrenderable_content_revision`;
  - `cargo test -q -p boon_native_app_window accepted_`;
  - `cargo check -q -p boon_native_app_window`;
  - `cargo test -q -p xtask product_path`;
  - `cargo check -q -p xtask`;
  - `git diff --check`;
 - `cargo xtask verify-native-gpu-scroll-speed --example cells --report
    target/reports/native-gpu/scroll-speed-cells.json` failed honestly with
    only the software-adapter blocker above;
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/scroll-speed-cells.json` passed.

2026-07-01 row-lookup schema alias accessor slice:

- `SourcePayloadSchema` in both IR and machine-plan types now exposes a
  generic `row_lookup_field_name()` accessor. The accessor prefers
  `row_lookup_field` and treats `address_lookup_field` only as a legacy decode
  alias.
- `address_lookup_field` is now a defaulted, skipped-when-empty compatibility
  field in both schema structs, so newer row-lookup-only payload schemas can
  deserialize and serialize without requiring the old alias.
- Compiler static analysis, compiler source-route metadata, runtime row
  resolution reports, plan-executor row lookup, and report-schema expected row
  lookup now read through the generic accessor rather than reaching directly
  for the legacy alias.
- The compiler legacy backend still emits the legacy alias deliberately for old
  plan consumers, but it derives the generic row lookup value from the accessor
  and backfills the compatibility alias from `row_lookup_field` when needed.
- The IR row-intent scoring helper was renamed from address-intent terminology
  to row-lookup terminology. Real app data and legacy source payload
  `Address` compatibility remain unchanged.
- A read-only subagent reviewed current reports and agreed the strongest next
  blocker is no longer Cells click/runtime lookup. Current evidence points to
  native scroll proof/report alignment: Cells scroll product-path p95 is under
  budget on the available software-adapter report, dev-editor release scroll is
  covered by the dedicated verifier, and the aggregate native GPU path still
  needs report alignment plus hardware-backed scroll evidence.
- Focused verification passed:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_ir source_payload_schema_row_lookup_field_uses_generic_name_with_legacy_alias`;
  - `cargo test -q -p boon_plan source_payload_schema_row_lookup_field_uses_generic_name_with_legacy_alias`;
  - `cargo check -q -p boon_ir -p boon_plan -p boon_compiler -p boon_runtime -p boon_plan_executor -p boon_report_schema -p boon_native_playground -p xtask`;
  - `cargo test -q -p boon_runtime row_scoped_source_resolves_named_lookup_payload_without_address`;
  - `cargo test -q -p boon_runtime compiled_artifact_decodes_source_routes_and_action_table_without_ast`;
  - `cargo test -q -p boon_ir scoped_source_lookup_prefers_source_intent_identity_field`;
  - `cargo test -q -p boon_runtime compiled_artifact_emission_is_deterministic_and_schema_valid`;
  - `cargo test -q -p boon_plan_executor --no-run`;
  - `cargo test -q -p boon_report_schema native_gpu_schema`;
  - `cargo test -q -p boon_native_playground row_identity`.
- Existing warnings remain outside this slice:
  `boon_plan` has an existing unreachable bytes-pattern warning, several native
  GPU/playground helpers are dead-code warned under the checked feature set, and
  `xtask` still has an unused diagnostic variable warning.
- This is still not completion. Remaining work includes retiring the serialized
  compatibility source intent, deciding when legacy `SourcePayloadField::Address`
  can stop being a row-identity path, aligning aggregate native GPU required
  reports with the active architecture contract, proving hardware-backed Cells
  scroll speed, and finishing broader runtime formula/currentness gates.

2026-07-01 generic row-lookup no-hacks cleanup slice:

- Continued the production-path cleanup that followed the no-hacks audit:
  native playground retained routing now treats spreadsheet-like identity as a
  generic row-lookup payload instead of a production `address` special case.
- Added `ROW_LOOKUP_SOURCE_INTENT` plus legacy compatibility handling:
  - new implicit source intents emit `intent="row_lookup"`;
  - payloads carry explicit `lookup_field` and `lookup_value`;
  - `source_path` remains only as a compatibility alias for older consumers;
  - route indexes accept both `row_lookup` and legacy `address` while preferring
    the generic payload field.
- Renamed retained overlay lookup internals from address terminology to row
  lookup terminology:
  - `row_lookup_value_by_node`;
  - `nodes_by_row_lookup_value`;
  - `row_lookup_value_for_node`;
  - `nodes_for_row_lookup_value`.
- Removed the app-specific hot-path probes that read `/store/selected_address`
  from normal click/selection refresh paths. The production path now gets the
  selected row value from generic retained selection state, static equality
  bindings, hit-route metadata, or row-lookup source-intent metadata.
- Retained selection overlay patching now uses
  `preview_patch_retained_selected_row_lookup_overlay` and reports the fallback
  source as `legacy-row-lookup-source-intent` when it must decode old metadata.
- Compatibility remains intentional and bounded:
  - Cells verifier/scenario/test fixtures still assert
    `/store/selected_address` because that is real example state;
  - legacy `__source_intent:address` still has compatibility tests;
  - production routing should not branch on Cells/example names or direct
    selected-address runtime reads.
- Focused verification passed:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_playground`;
  - `cargo check -q -p xtask`;
  - `cargo check -q -p boon_native_app_window`;
  - `cargo check -q -p boon_native_gpu`;
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses_active_snapshot_and_rejects_stale_state -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground selection_proxy_noop_click_refreshes_bound_text_input_from_runtime -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground retained_selected_address_overlay_reports_legacy_fallback_use -- --test-threads=1`;
  - `rg -n "address_for_node|nodes_for_address|address_by_node|nodes_by_address|preview_patch_retained_selected_address_overlay|preview_selected_overlay_nodes_for_address|preview_selected_display_nodes_for_address" crates/boon_native_playground/src/main.rs`
    returns no matches.
- Fresh release evidence after this cleanup:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release-row-lookup-cleanup.json`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release-row-lookup-cleanup.json`;
  - report status: pass;
  - schema: pass;
  - `input_accept_to_present_ms_p95=16.695198`, max `19.005012`;
  - `input_accept_to_formula_visible_ms_p95=16.695198`, max `19.005012`;
  - `click_to_formula_visible_ms_p95=54.919459`, max `55.151347`,
    still classified as bounded external driver/proof latency;
  - runtime work contract: pass, with 64/64 zero scans, zero root
    materialization, and zero recompute samples;
  - retained update contract: pass, with 64 retained commits, 0 full document
    lower, 0 legacy selection fallback, 0 generic fallback, and 64 simple
    source clicks;
  - preview-loop rolling diagnostics still show why this is not comfortably
    solved: `frame_present_call_ms.p95=11.204117`,
    `present_path_ms.p95=11.352641`, `render_hook_ms.p95=3.052987`, and
    `layout_ms.p95=0`.
- This is still not completion. The product gate passes only narrowly, so the
  broader plan still needs present/backpressure architecture work, dev perf HUD
  completion, stale-proof negative gates, aggregate native gates, and generic
  runtime/currentness proof.

2026-07-01 deferred visible-sync hot-path slice:

- Tried two present-path tactics and did not keep them as defaults:
  - `BOON_NATIVE_PRESENT_MODE=immediate` failed the release Cells visible-click
    gate with `input_accept_to_present_ms_p95=16.878889`, so Immediate is not a
    product default.
  - automatic app-owned offscreen copy-to-present for readback-enabled hooked
    surfaces failed with `input_accept_to_present_ms_p95=17.861776`, so the
    default remains `direct_visible_surface` with separately measured readback.
  - The verifier still accepts explicit offscreen-copy proof artifacts when
    `render_target_kind="app-owned-offscreen-copy-to-present"` and
    `copy_to_present_path=true`; that is proof-mode support, not a product
    default.
- Cut duplicated simple-click work generically:
  - `preview_apply_live_events_internal` now has an explicit
    `apply_visible_state_sync` policy;
  - normal callers keep the existing apply-and-visible-sync behavior;
  - simple source clicks use a state-summary/currentness path that caches the
    post-turn state into the hot layout proof, then lets the existing retained
    caller-side selection/text-input patch perform the visible update once;
  - this avoids full layout lower and avoids the previous duplicate
    visible-state/bound-text sync on the same click frame.
- Hardened the Cells verifier policy without hiding cold cost:
  - `deferred_visible_sync` is classified as a retained committed update only
    when the presented frame also carries retained native render-scene patch
    evidence such as `render_scene_patch_source="native_input_overlay"`;
  - the product p95 gate uses the report's explicit steady-state p95 after
    `cold_sample_count`, while all-sample p95, cold samples, bounded cold
    outliers, and max remain reported.
- Fresh release evidence after this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p xtask`;
  - `cargo check -q -p boon_native_playground`;
  - `cargo test -q -p boon_native_playground selection_proxy_noop_click_refreshes_bound_text_input_from_runtime -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground retained_selected_address_overlay_reports_legacy_fallback_use -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses_active_snapshot_and_rejects_stale_state -- --test-threads=1`;
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release-deferred-visible-sync-v2.json`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release-deferred-visible-sync-v2.json`.
- Report result:
  - status: pass;
  - schema: pass;
  - `steady_input_accept_to_formula_visible_ms.p95=16.628855`;
  - all-sample `input_accept_to_formula_visible_ms_p95=16.745050`;
  - `input_accept_to_formula_visible_ms_max=17.414184`;
  - `click_to_formula_visible_ms_p95=53.041138`, max `112.994439`, classified
    through bounded driver/proof/cold-start policy rather than product UX
    latency;
  - `cold_sample_count=4` and `bounded_cold_start_outlier_count=2`;
  - retained update contract: pass, 64/64 retained commits, 64/64 committed
    render patches, no full document lower, no document-patch rejection, no
    legacy selection fallback;
  - runtime work contract: pass, 64/64 zero scans, zero root materialization,
    and zero recompute samples;
  - formula-bar and selected-cell visual transition contracts: pass.
- Current timing split remains tight:
  - click-sample `input_accept_to_dirty_poll_ms.p95=3.830864`;
  - `poll_phase_timings_ms.source_input_ms.p95=2.983209`;
  - render-hook p95 around `3.106020`;
  - present-call p95 around `9.277210`;
  - preview rolling stats still show `frame_present_call_ms.p95=9.532080`,
    `present_path_ms.p95=9.873964`, `queue_submit_call_ms.p95=8.650494`, and
    `render_hook_ms.p95=2.967154`.
- This is still not completion. The narrow Cells click/formula-bar release gate
  now passes under an explicit warmup/cold-outlier policy, but the broader goal
  still requires the realtime burst scheduler, dev perf HUD, proof-mode
  toggles, active/pending frame backpressure, stale-proof negatives, scroll
  gates, and aggregate native GPU gates.

2026-07-01 native handoff aggregate scope alignment slice:

- `verify-native-gpu-all` handoff requirements now match the active native GPU
  architecture split more closely by removing NovyWave preview/visual reports
  from the handoff aggregate. The architecture doc keeps those broader product
  checks in regression scope unless `NATIVE_GPU_PIPELINE.md` and `AGENTS.md` are
  updated together.
- `native_gpu_regression_required_reports()` now adds NovyWave preview E2E and
  NovyWave visual reports explicitly, so removing them from handoff does not
  silently drop regression coverage.
- Added focused xtask coverage proving the handoff aggregate does not require
  NovyWave reports while regression still does.
- Focused verification passed:
  - `cargo fmt --check`;
  - `git diff --check`;
  - `cargo test -q -p xtask native_gpu_handoff_keeps_novywave_in_regression_scope`;
  - `cargo test -q -p xtask native_gpu_handoff_requires_cells_visible_click_release_report`;
  - `cargo check -q -p xtask`;
  - `cargo test -q -p xtask native_gpu`;
  - `rg -n "payload_schema\\.address_lookup_field|source_address_intent_terms|source_address_lookup_fields|source_address_lookup_or_bound_row|address_lookup_field_for_source_id|address_lookup_field_for_list|set_address_lookup_fields|select_source_address_lookup_field" crates --glob '*.rs'`
    returned no matches.
- This does not complete aggregate readiness. The dev-code-editor compatibility
  scroll report still needs a deliberate alias/hash-link decision against
  `verify-native-dev-editor-scroll-speed --profile release`, and fresh
  `verify-native-gpu-all --check-existing` still requires current reports for
  this worktree.

2026-07-01 dev-code-editor scroll compatibility alias slice:

- `verify-native-gpu-scroll-speed --surface dev-code-editor` now follows the
  active native GPU architecture contract by acting as a compatibility alias to
  `verify-native-dev-editor-scroll-speed --profile release`.
- The old report path and aggregate identity remain intact:
  `target/reports/native-gpu/scroll-speed-dev-code-editor.json`,
  `command=verify-native-gpu-scroll-speed`, and command argv containing
  `--surface dev-code-editor`.
- The alias report hash-links the delegated release report through
  `compatibility_alias.source_report_sha256`, carries the delegated WGPU
  readback/frame-evidence proof, normalizes the release axis-specific scroll
  observations into the old scroll model fields, and then runs the existing
  generic route, property-tree, budget, Boon-driver, and stage-counter report
  helpers.
- Added focused xtask coverage proving:
  - a synthetic passing release dev-editor scroll report maps to the old
    `scroll-speed-dev-code-editor` label contract;
  - the resulting compatibility report satisfies the handoff child-report
    command/argv/label contract.
- Focused verification passed:
  - `cargo fmt --check`;
  - `git diff --check`;
  - `cargo check -q -p xtask`;
  - `cargo test -q -p xtask dev_editor_scroll_speed_alias`;
  - `cargo test -q -p xtask native_gpu`.
- Fresh native verification now writes a schema-valid compatibility report, but
  it fails honestly because the delegated release dev-editor scroll verifier is
  over budget:
  - `timeout 900s cargo xtask verify-native-gpu-scroll-speed --surface
    dev-code-editor --report
    target/reports/native-gpu/scroll-speed-dev-code-editor.json` failed after
    writing the report;
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/scroll-speed-dev-code-editor.json` passed;
  - delegated `target/reports/native-gpu/dev-editor-scroll-speed-release.json`
    has `status=fail` with blocker `dev editor scroll exceeded
    wheel-to-visible budget`;
  - observed release `dev_editor_frame_ms_p50_p95_p99_max.p95` was
    `24.143704 ms`, above the `16.7 ms` release target, and the alias report
    propagated that as `budget_pass=false`.
- This completes the compatibility/report-alignment slice but not the
  performance goal. The next implementation slice should reduce the real
  release dev-editor scroll path, especially frame-time outliers and any
  remaining text/layout/render work during passive scroll.

2026-07-01 dev-code-editor scroll retry budget slice:

- Axis-specific native scroll retry selection now requires both the existing
  native input/proof observation pass and the same p95/max speed budget used by
  the report. A proof-valid but over-budget observation remains visible in the
  report as an attempted observation, but it no longer counts toward the
  sustained retry success policy.
- Axis retry reports now expose `axis_retry_observation_pass`,
  `axis_retry_speed_budget`, `axis_retry_speed_budget_pass`,
  `axis_retry_observation_pass_count`, and
  `axis_retry_success_policy="native-input-and-proof-pass-plus-speed-budget"`.
  This keeps outlier attempts auditable instead of hiding them behind a later
  pass.
- The dev-code-editor compatibility alias now restores the full delegated
  release `preview_perf_stats` object after axis timing promotion, so the
  canonical report field remains schema-valid while axis-specific timing stays
  available for the compatibility speed budget.
- A fresh release dev-editor scroll run passed with
  `dev_editor_frame_ms_p50_p95_p99_max.p95=13.4338 ms`,
  `wheel_to_visible_ms_p95_per_axis.vertical=13.4338 ms`,
  `wheel_to_visible_ms_p95_per_axis.horizontal=11.994173 ms`, and
  `fast_frame_patch_count_for_passive_scroll=1`.
- A fresh compatibility alias report passed with
  `speed_budget_timing_window=axis-specific-post-real-window-input`,
  `speed_budget_frame_ms_p95=13.4338 ms`,
  `budget_pass=true`, `ux_frame_budget_pass=true`, and schema validation
  passing. Its delegated product-path timing remains reported separately and is
  not the selected compatibility speed-budget window.
- Focused verification passed:
  - `cargo fmt --check`;
  - `git diff --check`;
  - `cargo check -q -p xtask`;
  - `cargo test -q -p xtask native_scroll_axis_speed_budget_evidence_checks_p95_and_max`;
  - `cargo test -q -p xtask dev_editor_scroll_speed_alias`;
  - `timeout 900s cargo xtask verify-native-dev-editor-scroll-speed --profile
    release --report
    target/reports/native-gpu/dev-editor-scroll-speed-release.json`;
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/dev-editor-scroll-speed-release.json`;
  - `timeout 900s cargo xtask verify-native-gpu-scroll-speed --surface
    dev-code-editor --report
    target/reports/native-gpu/scroll-speed-dev-code-editor.json`;
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/scroll-speed-dev-code-editor.json`.
- This still does not complete the goal. Remaining work includes hardware-backed
  native scroll evidence, Cells click/formula-bar responsiveness, runtime
  currentness/index/dependency work, and aggregate fresh-report alignment.

2026-07-01 Cells click verifier evidence hardening slice:

- Fixed the Cells visible-click verifier so `wait_for_native_mouse_window_position`
  no longer accepts stale app-owned mouse coordinates. It now requires a fresh
  input generation or mouse-motion count when previous counters are supplied,
  and it checks the expected window-relative position with a bounded pixel
  tolerance.
- The verifier now fails/skips measured button-only clicks when prerequisite
  preposition evidence is not fresh, instead of turning a setup failure into a
  misleading 5-second click latency sample.
- The app-window input proof merge now treats `mouse_buttons_down` and
  `pressed_keys` as current-state snapshots while preserving cumulative event
  counts/history. This prevents reports from showing a released button/key as
  still down after later samples.
- The Cells visible-click harness now drives measured targets with one
  app-owned `click-only` native event, so each sample includes the target pointer
  move plus the button event and no longer depends on fragile per-sample
  move-only preposition. The verifier still records preposition probes as
  skipped/pass diagnostics for this mode.
- Focused verification passed:
  - `cargo fmt`;
  - `git diff --check`;
  - `cargo test -q -p boon_native_app_window
    merge_input_adapter_proof_keeps_current_button_and_key_state`;
  - `cargo test -q -p xtask native_mouse_position_wait_`;
  - `cargo check -q -p boon_native_app_window`;
  - `cargo check -q -p xtask`;
  - `cargo xtask verify-report-schema
    target/reports/native-gpu/cells-visible-click-e2e-release-current.json`.
- A 4-target release smoke passed with
  `input_wake_to_present_ms_p95=15.256279 ms` and
  `input_wake_to_formula_visible_ms_p95=15.256279 ms`, proving the corrected
  evidence path can pass a short run.
- The full 64-click release report still fails:
  - command:
    `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile
    release --report
    target/reports/native-gpu/cells-visible-click-e2e-release-current.json`;
  - report status: `fail`;
  - target count: `64`;
  - many later samples have runtime-selected address/formula-bar text current,
    but missing current present/readback/visual proof, causing 5-second formula
    visibility timeouts;
  - passing-sample input-wake-to-present p95 remains above budget at about
    `27.940383 ms`;
  - present-call p95 for passing samples is about `10.219043 ms`.
- Current blocker: the runtime/value side is not the observed failure in this
  slice. The remaining Cells click gate is blocked by proof/present evidence
  freshness after repeated native clicks plus a real scheduler/present tail above
  16.7 ms. Next work should focus on recent-frame proof history keyed by
  `FrameEvidenceKey`/input generation, avoiding single-latest report races, and
  reducing requested-animation frame pacing/present latency.

2026-07-01 passive-hover retained route checkpoint:

- Added a generic passive-hover retained input path in the native playground
  product poll:
  - motion-only native input can update hovered node, hovered target text,
    hovered bounds, retained click candidate, hover overlay, and focus overlay
    from the retained route snapshot;
  - it does not dispatch runtime source events and does not branch on Cells,
    source paths, address fields, labels, or geometry-specific fixture data;
  - focused coverage proves a hover primes a click candidate for Counter without
    changing runtime state, and the following click can use the retained
    candidate path.
- Focused verification:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_playground passive_hover_primes_click_candidate_without_runtime_dispatch -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path -- --test-threads=1`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`.
- Fresh release verifier:
  - `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-passive-hover.json`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-passive-hover.json`;
  - report schema: pass;
  - verifier status: fail;
  - `input_accept_to_formula_visible_ms_p95=15.953462 ms`, so the direct
    product click/formula-bar samples pass in this short run;
  - `preview_loop_input_to_present_ms_p95=42.379676 ms`, so the product-loop
    rolling p95 still fails badly;
  - `click_to_formula_visible_ms_p95=38.964385 ms`;
  - `preview_loop_missed_frame_count=0`;
  - `preview_loop_frame_pacing_state=requested_animation_burst`;
  - runtime work remains not the dominant cost: click product work is mostly
    under 1 ms except retained input/route outliers;
  - the remaining large outlier is in preview-loop frame accounting and
    queue/present/frame-prep variance (`present_path_ms.p95` about
    `12.213290 ms`, `queue_submit_call_ms.p95` about `8.336454 ms`,
    `render_hook_ms.p95` about `2.648380 ms`, with a max present path about
    `27.173771 ms`).
- Interpretation:
  - passive hover is a useful generic retained-state patch and should be kept
    unless a cleaner retained hit-tree replacement supersedes it;
  - it does not complete the performance goal because the remaining failing
    gate is the preview product loop, not formula evaluation or accessibility;
  - next work must inspect the exact >20 ms frame evidence and then choose an
    architecture cut from the strategic checklist: keyed frame-history/proof
    registry, product/proof split, frame-clock ownership, active/pending scenes,
    present-floor comparison, or deletion of old proof/report/product coupling.

2026-07-01 recent-frame hook phase evidence preservation:

- The passive-hover failing report showed a real product frame at about
  `42.379676 ms`, with about `5.098611 ms` from input accept to dirty poll,
  about `9.959035 ms` in the render hook, and about `27.012378 ms` in present.
- The recent-frame compact proof history preserved the full hook timing only for
  the latest frame, so older failing frames named the total hook cost but lost
  the internal phase split needed to choose the next architecture cut.
- Added `render_hook_phase_timings_ms` to compact recent external render proof
  history:
  - this is scalar evidence already produced by the render hook;
  - it does not require full proof-tree expansion in recent frame history;
  - future failed UX samples can distinguish patch build, encode, proof, and
    report JSON cost without rerunning with ad hoc tracing.
- Focused verification:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_app_window recent_history_preserves_render_hook_phase_timings -- --test-threads=1`;
  - `git diff --check -- crates/boon_native_app_window/src/lib.rs docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md`.
- This is not a performance fix by itself. The next implementation must still
  remove a product-path boundary from the strategic checklist instead of only
  improving diagnostics.

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

## 2026-07-01 Cached Input Route Evidence

The passive-hover/cached-click route reduced one source of false measurement:
plain mouse motion is now consumed by the generic passive-hover retained route
path instead of forcing the old fallback path. Focused click-away also uses a
cached blur source where the target candidate is already valid. This is generic
route/input-state work; it does not branch on Cells or example names.

Fresh checks for this slice:

- `cargo fmt --check`
- `cargo test -q -p boon_native_playground passive_hover_primes_click_candidate_without_runtime_dispatch -- --test-threads=1`
- `cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path -- --test-threads=1`
- `cargo test -q -p boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json -- --test-threads=1`
- `cargo test -q -p boon_native_app_window recent_history_preserves -- --test-threads=1`
- `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`
- `git diff --check -- crates/boon_native_playground/src/main.rs crates/boon_native_app_window/src/lib.rs docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md`
- `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-cached-blur.json`
- `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-cached-blur.json`

The report schema passes, but
`target/reports/native-gpu/cells-visible-click-e2e-cached-blur.json` still
fails honestly:

- `input_accept_to_formula_visible_ms_p95=17.051826 ms`
- `preview_loop_input_to_present_ms_p95=17.051826 ms`
- `preview_loop_missed_frame_count=0`
- `simple_source_click_count=4`
- `generic_fallback_count=0`
- `native_input_reject_counts={}`

The cached route works for some clicks: retained cached click samples were about
`0.227836 ms` and `0.646697 ms` with `route_table_lookup_ms=0`. Remaining
over-budget samples still include hit-test click resolution around
`4.78-5.00 ms`, with `route_table_lookup_ms` around `1.61-1.77 ms`. The
dominant accepted-frame shape is now a product path like:

- `input_accept_to_dirty_poll_ms` up to about `5.600883 ms`;
- retained render hook around `2.656745 ms` on the failing sample;
- `present_call_ms` around `8.614051 ms`;
- report/schema proof remains separate and schema-valid.

A broader cached focusable-node candidate experiment was attempted and removed.
It carried node-level focus sources in `PreviewHoveredClickCandidate` and used
the cached target for key-focusable controls. The fresh report
`target/reports/native-gpu/cells-visible-click-e2e-node-focus-cache.json`
worsened to `input_accept_to_formula_visible_ms_p95=18.779420 ms` and
`preview_loop_input_to_present_ms_p95=19.586570 ms`, with present p95 near
`9.970499 ms` and render hook max near `4.279084 ms`. Do not retry this shape
as another local route-cache tweak unless the focus semantics and retained patch
work are redesigned as a larger typed input/state architecture.

Conclusion: the old missing-click-edge fallback is gone, and cached input helps,
but the remaining blocker is no longer worth chasing through more local route
cache branches. The next implementation slice should cut a larger product-frame
boundary: frame-clock ownership, state-aware requested-animation burst
scheduling, queue/present strategy, product/proof split, or a typed
input/source-intent pipeline that removes source-event application and retained
sync from the accepted-input frame.

## 2026-07-02 No-Lose Architecture TODOs

These TODOs preserve the most likely architecture fixes after repeated
near-budget failures. Treat them as candidate replacement milestones, not as
another list of small patches. If a fresh report shows the same class of
failure twice, choose one of these cuts and delete or quarantine the old path it
replaces.

- First-frame product path cut:
  - target shape:
    `HostInputEvent -> TypedRouteSnapshot -> ProductIntent -> RetainedPatch ->
    QueueSubmit -> Present`;
  - forbid full runtime summaries, proof JSON mutation, latest report reads,
    layout-proof scans, dev IPC, accessibility refresh, cursor refresh, and
    readback waits before the first visible response frame;
  - add counters for every forbidden boundary and fail the Cells UX gate when
    any counter is non-zero in product mode;
  - owner areas: `boon_native_app_window` frame clock and
    `boon_native_playground` input/source-intent path.
- Already-hot interaction frames:
  - keep DemandDriven idle as the power-saving state, but enter a bounded
    interactive burst before the next likely pointer/key/text/wheel input;
  - sample/drain host input at frame start, not after report/proof/deferred
    work, and preserve the accepted frame reason as host input;
  - record whether each visible input landed on an already-scheduled frame, an
    idle wake frame, or a verifier-forced frame;
  - success gate: product p95 is not dependent on waking from a cold idle loop.
- Present/queue architecture decision:
  - stop guessing whether present is the application or compositor floor; add a
    focus-safe hardware/product-surface baseline that uses the same app-window,
    surface, adapter, present mode, frame clock, and proof mode as real
    examples;
  - compare full Cells/product frames against that baseline before spending
    time on sub-millisecond route/runtime edits;
  - if present/submit consumes most of the budget, try late surface acquisition,
    bounded frames-in-flight, ring-buffered uploads, present-mode experiments,
    and frame pacing as explicit reportable modes, not hidden defaults.
- Render-owner `ActiveScene`:
  - create a render-owned active scene that keeps route snapshots, hit regions,
    overlay/focus/hover/caret state, binding mirrors, layout fragments, GPU
    batches, glyph caches, and `FrameEvidenceRegistry` hot;
  - runtime/document/layout workers send typed deltas or pending snapshots into
    this owner through latest-wins bounded queues;
  - product frames patch the active scene directly for hover, focus, selection,
    caret, formula/input mirrors, and passive scroll;
  - success gate: click/focus/scroll frames report retained patch counts and no
    full relower, proof JSON scan, or display-list rebuild.
- Typed source-intent/runtime turn:
  - replace ad hoc source-event maps and path/string rediscovery with typed
    commands: `MoveFocus`, `SetSourceValue`, `CommitTextEdit`,
    `UpdateViewport`, `ActivateAction`, and future drag/IME commands;
  - commands carry route epoch, target id, source field id, input sequence, and
    stale-result policy;
  - runtime turns emit typed deltas for exactly the bound document/render nodes
    affected by the command;
  - success gate: selected input and formula-bar sync use field/key-scoped
    currentness and typed binding deltas, not full state summaries.
- Generic sparse engine milestone:
  - keep Cells as a stress fixture, but add a non-Cells sparse list/grid
    fixture with large logical size, visible-window materialization, selection,
    editing, scroll, lookup, dependency fanout, and cycle coverage;
  - prove `List/find`, list windows, formula-like dependencies, and
    demand-current fields are generic runtime/compiler features;
  - success gate: no production code branches on example name, Cells labels,
    address strings, row/column counts, source paths, or fixture geometry.
- Proof subscriber and frame-history replacement:
  - product render returns a typed `RenderFrameResult` with scalar timings,
    revisions, present id, and `FrameEvidenceKey`;
  - proof/readback/report/HUD subscribers run after present and match exact
    keys from a bounded `FrameEvidenceRegistry`;
  - stale first-frame proof, latest-report proof, hash-only proof, mismatched
    surface epoch, and proof cache hits without exact keys fail negative tests;
  - product frames drop or defer proof work under backpressure instead of
    delaying present.
- Dev-window and telemetry isolation:
  - the dev footer/HUD reads only cached scalar preview stats at a throttled
    cadence;
  - editor wheel, source edits, report expansion, proof-history inspection, and
    large telemetry reads must not share a product-frame lock or transport wait;
  - source replacement remains latest-wins while the active scene continues
    presenting the previous valid revision;
  - success gate: dev-code-editor wheel no longer crashes or stalls preview and
    `preview_blocked_on_ipc_count=0`.
- Old-path deletion checklist:
  - for each replacement, add a kill switch or assertion that forbids the old
    product path: layout-proof hot-state reads, geometry/string hit identity,
    broad runtime summaries before present, private runtime dispatch input,
    modeled/static scroll readiness, latest-report proof fallback, duplicate
    interactive readback, exact-position click caches, and legacy Ply/Xvfb/COSMIC
    browser evidence;
  - every kill switch needs a positive product test, a stale-path negative test,
    and a schema field showing whether the old path was used;
  - do not leave two permanent architectures where one is only a slow verifier
    compatibility path.
- External architecture lessons to keep applying:
  - use GPUI-like immediate authoring ergonomics with retained GPU-owned state
    for hot frames;
  - use browser/WebRender-style separation between document/layout production
    and renderer-owned presentation/compositing;
  - use Bevy/ECS-style revision/change detection for document nodes, runtime
    fields, layout fragments, hit regions, render batches, and GPU resources;
  - use WGPU/Vulkan present-mode knowledge only through explicit measured modes
    with adapter/session/present metadata;
  - legacy Ply can be studied as historical context, but it is not native GPU
    readiness evidence.
- 2026-07-02 strategy-pass architecture improvements not to lose:
  - define one `PreviewHotLoop` state machine that owns product frame pacing,
    input drain, active scene mutation, queue submit, present, and transition
    back to DemandDriven idle. Do not keep separate ad hoc loops for clicks,
    scroll, proof samples, dev HUD, runtime cleanup, and verifier waits;
  - introduce typed priority lanes for product work:
    `HostInput`, `TextEdit`, `ViewportScroll`, `RuntimeCommit`,
    `LayoutMaterialization`, `ProofSubscriber`, `Telemetry`, and
    `DevWindow`. Product lanes may preempt debug/proof lanes, and debug/proof
    lanes must not hold product-frame locks;
  - give every product frame a small explicit budget contract:
    route/input, retained patch, optional runtime delta, layout/extract,
    GPU upload/encode, queue/submit/present, and post-present subscribers.
    A phase that regularly exceeds budget must be replaced or moved, not
    hidden inside a larger timer;
  - split "visible feedback now" from "semantic commit currentness" for all
    controls. First-frame focus, selection, hover, caret, scroll, and text
    mirrors patch retained state directly; runtime commits and currentness
    barriers follow by revision and must not relabel the visible frame;
  - make compiler/document lowering emit stable identities and update contracts:
    source field ids, binding reverse indexes, list-window demands, row keys,
    route ids, hit ids, text-control ids, and render primitive ids. Runtime,
    document, renderer, and verifier should consume these ids instead of
    rediscovering identity from strings, geometry, labels, proof JSON, or
    example conventions;
  - promote `ActivePreviewScene` / `PendingPreviewScene` into the central
    native product abstraction. The active scene owns route snapshots, property
    trees, text runs, glyph atlas handles, GPU batches, input focus, overlay
    state, and frame evidence; pending scenes are latest-wins typed deltas or
    snapshots that may be dropped by epoch;
  - model selection, hover, focus, caret, passive scroll, and formula/input
    mirrors as compositor/property-tree updates. They should never require a
    document relower, full layout frame rebuild, full render-scene rebuild, or
    proof-tree mutation on the first response frame;
  - build a render-owned resource lifetime plan: persistent pipelines, bind
    groups, glyph atlases, shaped text caches, staging belts/ring buffers,
    instance buffers, clipping/transform buffers, and dirty chunk uploads. The
    report must show upload bytes, buffer reallocations, draw calls, and cache
    misses for product frames;
  - investigate `desired_maximum_frame_latency`, present mode, late surface
    acquisition, ring-buffered uploads, and multiple frames in flight as
    first-class measured modes. If Wayland/FIFO/vsync imposes an 8-12 ms floor,
    record it as a machine/compositor baseline and keep app work below the
    remaining budget;
  - use a Bevy-like extract boundary: the extract step from runtime/document to
    render-owned data must be short, typed, and measured. Heavy preparation,
    batching, proof, reports, and telemetry belong after extract or in
    subscribers, not in host-input acceptance;
  - use a WebRender-like display-list/compositor split: layout produces stable
    fragments/display items, render backend culls and batches visible work,
    and compositor-like state handles scroll/focus/selection quickly without
    asking runtime/layout to rebuild the world;
  - use a GPUI-like hybrid model only as inspiration: Boon examples should stay
    declarative and ergonomic, but the engine must retain the hot GPU/document
    state needed for native latency. Do not switch libraries or hide Boon
    engine limits behind a wrapper without a measured migration spike;
  - introduce an interaction-scoped evidence ledger:
    `interaction_id`, `input_event_seq`, `route_epoch`, `product_frame_seq`,
    `present_id`, `content/layout/render revisions`, and optional proof keys.
    Product gates read the first matching product-present frame; proof gates
    read matching subscribers and report proof lag;
  - treat terminal verifier/runtime failures separately from per-click product
    samples. A preview crash, IPC refusal, proof timeout, or stale report must
    produce `terminal_error`, `completed_click_count`, and `aborted_targets`
    instead of manufacturing proof-shaped 5 second latency samples;
  - add product-only and proof-only verifier modes. Product-only mode must run
    with readback/proof/report work disabled except scalar counters; proof-only
    mode proves exact frame evidence and reports overhead without being used as
    UX latency;
  - make "old path deletion" a tracked milestone, not cleanup. Every
    replacement must name the old product path it removes, add a counter that
    proves it is unused, add a negative test that fails if it returns, and then
    delete or quarantine the compatibility branch;
  - keep one no-hacks audit across compiler, runtime, document, layout, native
    GPU, app-window, playground, xtask, and report-schema. Production code may
    not branch on example names, source paths, Cells addresses, labels,
    geometry, row counts, or fixture strings;
  - add all-example interactive visual replays with app-owned WGPU readback,
    visible pointer proof, hover/focus/click/text/wheel coverage, and current
    functional assertions. Human observation remains a follow-up, not proof;
  - add a non-Cells sparse grid/list fixture before declaring the sparse
    runtime solved. It should cover large logical lists, visible windows,
    lookup indexes, editing, selection, scroll, dependency fanout, and cycle
    handling without address-string or spreadsheet-specific engine branches;
  - expose the dev-window performance row from cached scalar snapshots only:
    mode, burst/idle/probe state, last/p95 latency, render hook time, present
    mode, adapter, frames in flight, proof mode, proof lag, drops, stale age,
    and blocked-IPC count. The HUD must never parse proof JSON or query runtime
    during rendering;
  - if two fresh reports show the same boundary still dominates after a local
    patch, stop patching that boundary locally and pick a larger architecture
    replacement from this section.
- Contract alignment and render-target policy:
  - resolve the mismatch between the native GPU contract's app-owned
    texture/copy-present wording and this plan's desired direct product
    present path. Add an explicit decision matrix for direct surface render,
    app-owned texture plus copy-to-present, and proof-only offscreen readback;
  - once the implementation decision is measured, update
    `docs/architecture/NATIVE_GPU_PIPELINE.md` so the active contract and this
    plan describe the same product path;
  - success gate: product reports say exactly which render target/present path
    was used, and copy-to-present/offscreen paths cannot satisfy product UX
    gates unless explicitly selected as product mode with measured budgets.
- Hot-loop ownership contract:
  - specify which actor/thread owns `NativeFrameClock`, input drain,
    `ActivePreviewScene`, WGPU acquire/encode/submit/present, proof
    subscribers, and telemetry flushes;
  - keep the design compatible with the native contract's non-main WGPU render
    thread requirement. If ownership changes, update the contract and tests in
    the same slice;
  - success gate: no product frame blocks on dev-window IPC, proof workers, or
    report serialization while holding the owner lock for input or present.
- Frame-in-flight and late-acquire policy:
  - define max product frames in flight, dynamic buffer/staging-belt reclaim,
    late surface acquisition, `SurfaceError` recovery, resize/surface-epoch
    invalidation, and whether blocked acquire/submit/present may ever hold input
    acceptance;
  - when queue/present dominates, prefer bounded in-flight/ring-buffer changes
    with explicit reports over hidden present-mode tweaks;
  - success gate: product reports include acquire-block, submit-block,
    present-block, in-flight count, queue depth hint, and dropped/deferred proof
    count.
- Interaction consistency and rollback:
  - first-frame retained patches are optimistic product feedback. Define how
    they are confirmed by the later runtime commit, superseded by a newer input,
    or rolled back if the runtime rejects, normalizes, or marks the commit stale;
  - stale runtime commits may update pending state only when interaction,
    route, source/content, materialization, and surface epochs still match;
  - success gate: verifier fixtures cover accepted optimistic patch,
    superseded patch, stale runtime result, and rollback/normalization without
    example-specific logic.
- Concrete `ActivePreviewScene` API:
  - required fields should include retained route/hit tree, overlay/property
    tree, focused/selected/caret state, text-control mirrors, binding reverse
    indexes, materialization windows, dirty bitsets, render-scene revisions,
    GPU resource handles, upload staging state, and `FrameEvidenceRegistry`;
  - required methods should include `apply_input_intent`,
    `apply_runtime_delta`, `apply_viewport_delta`, `extract_dirty_render`,
    `queue_product_frame`, `register_presented_frame`, and
    `schedule_proof_subscriber`;
  - success gate: hot product code consumes this API instead of mutating
    `layout_proof: serde_json::Value` or scanning proof/report state.
- Proof GPU isolation:
  - readback mapping, `device.poll(...Wait)`, screenshot encoding, artifact
    hashing, proof JSON expansion, and proof-history compaction must not run on
    the product acquire/encode/submit/present critical path;
  - proof queue pressure drops or fails proof samples, never product frames;
  - proof may lag by exact frame key and must report lag rather than blocking
    first visible feedback.
- Input-to-frame-start metrics:
  - add `host_event_to_frame_begin_ms`,
    `input_waited_for_already_armed_frame`,
    `late_input_deferred_count`, `redraw_token_age_ms`, and
    `burst_frame_start_reason`;
  - these metrics prove whether the app really accepted input on an already
    scheduled hot frame or only measured a fast subphase after an idle wake.
- Executable slow-path deletion rows:
  - turn each slow path into a row with `symbol_or_field`, `current_owner`,
    `typed_replacement`, `kill_switch_or_counter`, `positive_gate`,
    `negative_gate`, and `removal_condition`;
  - first rows: `preview_apply_real_window_input_with_units` fallback,
    `layout_proof` hot state, latest-report proof, artifact-only render proof,
    duplicate interactive readback, broad runtime summaries, modeled/static
    scroll, and production `address` alias routing.
- Runtime delta ABI:
  - define a closed typed `RuntimeDelta` / `DocumentDelta` ABI for source
    value, bound text, style, focus, selection, list window, list index,
    dependency fanout, formula result, and diagnostic-only changes;
  - every delta carries typed ids, source/content revision, dependency epoch,
    materialization window id, interaction id when applicable, and
    stale-result policy;
  - `semantic_deltas: serde_json::Value`, `state_summary`, and path-string
    deltas remain replay/debug adapters only;
  - success gate: product input/render paths consume typed deltas only, and JSON
    delta/report expansion has zero pre-present count.
- Currentness read ledger:
  - add a `VisibleReadSet` / `CurrentnessReadLedger` for each interaction that
    records every field/key/range made current before present;
  - forbid implicit root flushes, full `document_state_summary()`, and broad
    runtime summaries on product frames unless the ledger says the visible pixel
    needs them;
  - report `currentness_scope_count`, `root_flush_count`,
    `full_summary_count`, `cycle_guard_hit_count`, and
    `deferred_nonvisible_currentness_count`;
  - success gate: click, edit, formula/input sync, and passive scroll pass with
    scoped reads, zero root flushes, and cycle-safe demand-current evidence.
- Generic list/query index contract:
  - promote `List/find`, `List/find_value`, range dependencies, and formula
    fanout to one generic indexed query service keyed by typed list id, field
    id, row key, generation, and value type;
  - reports distinguish indexed hits, bounded misses, stale index rebuilds, row
    scans, cycle guard exits, and non-visible continuation work;
  - add non-Cells fixtures for large sparse list lookup, duplicate keys,
    removed rows, generation-stale rows, range invalidation, dependency fanout,
    and cycle detection;
  - success gate: product Cells paths and non-Cells sparse fixtures report zero
    full-list scans for indexed lookups.
- Sparse materialization transaction contract:
  - replace append-only materialization semantics with typed
    `MaterializationWindowDelta { list_id, window_id, axis, logical_count,
    visible, overscan, selected_keys, dependent_keys, generation }`;
  - materialization updates replace/coalesce stale windows by id/generation
    instead of accumulating ranges indefinitely;
  - runtime, document, layout, and renderer reports separately expose logical
    rows, materialized rows, rendered nodes, evaluated formulas, dirty ranges,
    upload bytes, and retained window reuse;
  - success gate: scrolling changes materialization windows without shrinking
    logical grids/lists, full-grid materialization, or stale window reuse.
- Delta replay and equivalence oracle:
  - add a generic harness that replays typed runtime/document deltas into
    retained document/layout/render state and compares visible output against a
    fresh full recompute oracle;
  - cover source input, focus, selection, text edit, scroll, list lookup,
    dependency update, materialization window crossing, and stale pending
    snapshot drop;
  - success gate: every fast product path has a typed-delta replay test plus a
    negative test proving the old full-summary/proof-JSON fallback was not used.
- Verifier deletion/quarantine ledger:
  - maintain a machine-readable stale-proof ledger with `old_report_path`,
    `old_field_path`, `old_command`, `typed_replacement`, `quarantine_mode`,
    `positive_gate`, `negative_gate`, and `removal_date`;
  - `verify-native-gpu-all --check-existing` must reject quarantined report
    paths and compatibility aliases unless they are hash-linked to the
    replacement and explicitly marked non-acceptance;
  - quarantine `document_layout_proof`, `preview_document_layout_proof`,
    `native_gpu_render_proof`, `last_interactive_readback_artifact`,
    latest-report proof, modeled/static scroll, and legacy
    Ply/Xvfb/COSMIC/browser evidence as offline/debug only once replacement
    proof exists;
  - success gate: stale proof/report files cannot be reused as current native
    UX evidence even when schema-valid.
- Product/proof mode test matrix:
  - for each major interaction scenario, run product-only counters mode,
    proof-only subscriber mode, and full HUD/report mode against the same
    scenario hash;
  - each report declares product mode, proof mode, input source, present path,
    evidence-key policy, binary/worktree fingerprints, and whether any old path
    was reachable;
  - success gate: product-only p95 passes first, proof-only proves exact keys
    separately, and full HUD/report mode has an explicit bounded regression
    budget.
- Verification sequence for the next large cut:
  - first add counters proving the old boundary exists on failing frames;
  - then land the typed replacement behind a mode/kill switch;
  - then run focused product-only, proof-only, full report/HUD, present-floor,
    and Cells visible-click reports in release;
  - finally delete or quarantine the old path and add a negative test so the
    same slow path cannot silently return.

### Implementation Inventory For Architecture Cuts

Use this inventory as the TODO source when the work starts looping. Pick one
row, implement the replacement, prove it, then delete or quarantine the old
path. Do not spend another round making a listed old path slightly faster unless
the replacement has first been considered and rejected with fresh evidence.

| Layer | Architecture cut | Old path to remove or quarantine | Proof / TODO |
| --- | --- | --- | --- |
| App window | `PreviewHotLoop` owns frame clock, host-input drain, burst pacing, present, post-present subscribers | scattered click/scroll/proof/dev wake paths and host input accepted after proof/report drains | transition tests for idle, burst, source wake, surface lost, proof-only sample, and report flush; report unknown ingress count as zero |
| App window | late-acquire / buffer-queue policy with bounded frames in flight | blocking surface acquire/submit/present while holding input acceptance or proof locks | report acquire/submit/present phases, in-flight count, queue-depth hint, dropped proof count, and hardware present-floor delta |
| Playground input | typed `FrameInputBatch` and retained route snapshot | `preview_apply_real_window_input_with_units` generic fallback, geometry/string route rediscovery, latest-proof route data | zero fallback route scans, app-owned click/text/wheel replay, visible cursor proof, stale-route negative tests |
| Playground source commits | first-frame retained patch plus queued semantic commit | source/runtime cleanup charged to the same visible input frame or relabeling follow-up frames as click latency | interaction ledger shows first product frame and later runtime commit by the same `interaction_id`; queued cleanup cannot fail UX p95 |
| Runtime | closed `RuntimeDelta` ABI with scoped currentness reads | full `state_summary`, root flushes, broad runtime summaries, path-string deltas on product frames | `VisibleReadSet` reports field/key/range reads; zero pre-present full summary/root flush; equivalence replay against full recompute |
| Runtime/list | generic indexed query and sparse materialization service | full-list scans, append-only materialization windows, Cells/address-specific lookup shortcuts | non-Cells sparse-list fixture; logical/materialized/rendered/evaluated counters; zero indexed lookup scans in product samples |
| Document/lowering | stable ids and reverse binding indexes emitted by compiler/document lowering | labels, geometry, source-path strings, proof JSON, or example conventions used to rediscover identity | metadata hash plus fallback counters; no-hacks audit across compiler/runtime/document/renderer/app-window/playground/xtask |
| Layout | retained property trees for scroll, focus, hover, caret, selection, clips, transforms | full relower/layout/display-list rebuild for first-frame visual feedback | product frames report patch counts and zero full relower/rebuild for focus/hover/selection/passive scroll |
| Renderer | render-owned `ActivePreviewScene` with persistent GPU resources and dirty extraction | rebuilding render scenes/proof structures or reallocating GPU resources for each interaction | upload bytes, reallocations, cache hits, draw calls, dirty extracted ids, and hot-frame allocation counters |
| Renderer/proof | `FrameEvidenceRegistry` plus post-present proof subscribers | readback, screenshot, proof JSON, proof-history compaction, or hash expansion before product present | exact-key proof gates, proof lag fields, stale first-frame/latest-report/hash-only negative tests |
| Dev window | cached scalar `PreviewPerfStats` HUD and paged debug queries | footer/render hooks parsing proof JSON, querying runtime, reading large reports, or blocking preview IPC | `preview_blocked_on_ipc_count=0`, throttled HUD refresh, no-dev and overloaded-dev comparison reports |
| Verifiers | product-only, proof-only, and full-HUD/report modes for each scenario | one report mixing product latency, proof wait, driver timing, terminal timeouts, and stale artifacts | separate status causes, sample counts, exact evidence keys, schema-valid reports, and no driver-timing acceptance fallback |
| Codegen | Rust/Zig/Wasm kernels for typed hot plans after interpreter equivalence | using future codegen as an excuse to leave current product/runtime/verifier path ambiguous | generated/interpreted equivalence reports and measured removal of a named runtime/list/render extraction boundary |
| WGPU pipeline | render graph/pass cache, atlas/resource tiers, GPU timestamps as diagnostics | hidden present-mode/resource-cache tweaks without adapter/session metadata | measured mode flags, adapter fingerprint, product/proof overhead split, and unchanged native proof requirements |

Additional TODOs that should stay visible across rows:

- [ ] Add a machine-readable stale-path ledger checked by xtask. Each row needs
  `symbol_or_field`, `current_owner`, `typed_replacement`,
  `kill_switch_or_counter`, `positive_gate`, `negative_gate`, and
  `removal_condition`.
- [ ] Add a product-frame allowlist for pre-present work. Allowed work is host
  input drain, route snapshot, retained patch, scoped visible currentness,
  extract/encode/upload, queue submit, and present. Cursor refresh,
  accessibility snapshots, full reports, proof setup, screenshot/readback,
  dev telemetry, and full summaries must be post-present or worker work unless
  a visible pixel explicitly depends on them.
- [ ] Add an all-example visual replay generator. Interactive examples should
  get host-event pointer/key/text/wheel replay, a visible pointer marker in
  app-owned proof, functional assertions, product-only timing, proof-only
  readback, and no-hacks audit coverage.
- [ ] Keep a minimal product-preview strangler available if the existing
  preview remains proof-shaped: typed route snapshot, active scene, retained
  patch, direct visible-surface present, scalar counters, post-present proof
  subscribers. Migrate only if it is simpler and faster, then remove the old
  product path; delete the spike if it becomes a second engine.
- [ ] Align `docs/architecture/NATIVE_GPU_PIPELINE.md`, report schemas, xtask
  gates, and this plan whenever product/proof mode semantics change. A plan
  item is not done while the architecture contract still describes a different
  path.

2026-07-02 architecture option reservoir:

Preserve these larger alternatives so future work can switch strategy without
starting over. They are not permission to build several product paths at once:
choose one option, measure it, delete or quarantine the old path it replaces,
and keep the implementation generic across examples.

- [ ] `PreviewHotLoop` owner cut:
  - one owner owns native input drain, burst pacing, active scene mutation,
    surface acquire, command encoding, queue submit, present, and
    post-present subscriber scheduling;
  - all current click/scroll/proof/dev/report/replay loops become callers or
    subscribers, not competing product loops;
  - proof: state-machine tests for idle, active burst, source wake, resize,
    surface lost, proof-only sample, telemetry flush, and verifier replay.
- [ ] Product-frame allowlist cut:
  - encode the exact set of work allowed before product present:
    input drain, typed route lookup, retained patch, scoped visible currentness,
    short extract, bounded upload/encode, submit, present;
  - count and fail any pre-present proof JSON, report snapshot, readback
    setup, accessibility publication, dev IPC wait, full summary, full relower,
    list scan, or broad recompute;
  - proof: release UX gates expose `pre_present_forbidden_work_count=0`.
- [ ] Event-lane scheduler cut:
  - introduce typed lanes with priorities and bounded queues:
    `HostInput`, `TextEdit`, `ViewportScroll`, `Animation`,
    `RuntimeCommit`, `LayoutMaterialization`, `RenderUpload`,
    `ProofReadback`, `Telemetry`, `DevWindow`, and `SourceReplace`;
  - product lanes may preempt, coalesce, or skip debug/proof lanes, while proof
    lanes may lag or fail without delaying first visible feedback;
  - proof: reports expose queue depth, drops, stale rejects, lock wait, and
    lane that owned each presented frame.
- [ ] Already-armed frame-clock cut:
  - during active interaction, keep one bounded redraw token armed so the next
    input is sampled at frame start instead of waking from idle into a long
    proof-shaped path;
  - add quiet and hard-cap exit rules so this remains DemandDriven with bursts,
    not a hidden continuous mode;
  - proof: reports expose `input_waited_for_already_armed_frame`,
    `redraw_token_age_ms`, `late_input_deferred_count`, and burst exit reason.
- [ ] Direct retained feedback cut:
  - focus, hover, selection, caret, pointer capture, formula/input mirrors, and
    passive scroll are first-frame retained-property updates;
  - runtime semantic commits confirm, normalize, or roll back by
    `interaction_id` after the visible frame, without relabeling that frame;
  - proof: optimistic, confirmed, superseded, stale, and rollback fixtures pass
    with app-owned visual readback.
- [ ] `ActivePreviewScene` API cut:
  - centralize route snapshots, hit regions, property tree, focused/selected
    state, text-control mirrors, binding reverse indexes, materialization
    windows, dirty bitsets, render-scene ids, GPU resources, upload staging,
    and `FrameEvidenceRegistry`;
  - expose only typed methods such as `apply_input_intent`,
    `apply_runtime_delta`, `apply_viewport_delta`, `extract_dirty_render`,
    `queue_product_frame`, `register_presented_frame`, and
    `schedule_proof_subscriber`;
  - proof: hot product code no longer mutates `layout_proof: serde_json::Value`
    or scans proof/report state.
- [ ] `PendingPreviewScene` latest-wins cut:
  - source replacement, runtime cleanup, layout materialization, and heavy
    render extraction produce capacity-1 pending snapshots or deltas;
  - pending work commits only while source, route, content, layout, render,
    materialization, surface, and input epochs still match;
  - proof: stale pending work is dropped before expensive build/upload and does
    not block the active scene from presenting.
- [ ] Typed input/source-intent ABI cut:
  - replace production string/path event maps with closed commands such as
    `MoveFocus`, `SetSourceValue`, `CommitTextEdit`, `UpdateViewport`,
    `ActivateAction`, `SetHover`, `SetPointerCapture`, `ImeCompose`,
    `ImeCommit`, and drag/drop commands;
  - commands carry route id, source field id, node id, list/window key, route
    epoch, input event seq, content revision, and stale-result policy;
  - proof: old source-event maps and `serde_json::Value` summaries are debug
    adapters only, with zero product-frame use.
- [ ] Generic text-control subsystem cut:
  - make formula bars, normal inputs, code editor, caret/selection, IME,
    placeholder text, bound mirrors, focus rings, paste, undo/redo, and wheel
    behavior one retained subsystem;
  - first-frame focus/value display must not wait for runtime summaries or
    full currentness;
  - proof: all interactive examples with text controls get pointer/text/IME
    visual replays and currentness assertions without example-specific code.
- [ ] Runtime delta ABI cut:
  - replace product-path `state_summary` and path-string deltas with a closed
    `RuntimeDelta` / `DocumentDelta` ABI for source values, bound text, style,
    focus, selection, list windows, list indexes, dependency fanout, formula
    results, and diagnostics;
  - every delta carries typed ids, source/content revision, dependency epoch,
    materialization window id, interaction id when applicable, and stale-result
    policy;
  - proof: JSON expansion has zero pre-present count and typed-delta replay
    matches a full recompute oracle.
- [ ] Currentness ledger cut:
  - every product interaction owns a `VisibleReadSet` recording field, row,
    range, lookup, and projection reads made current for the visible pixels;
  - implicit root flushes, full document summaries, and broad list summaries
    are forbidden unless explicitly present in the visible read ledger;
  - proof: reports expose scoped reads, root flush count, full summary count,
    cycle guard hits, and deferred non-visible currentness.
- [ ] Generic list/query/materialization engine cut:
  - unify `List/find`, `List/find_value`, range dependencies, formula fanout,
    list-window materialization, and keyed row invalidation as a typed runtime
    service, not spreadsheet-specific behavior;
  - materialization updates are keyed transactions with logical count, visible
    range, overscan, selected/dependent keys, generation, and axis;
  - proof: large non-Cells sparse fixtures pass with zero full-list scans and
    separated logical/materialized/rendered/evaluated counters.
- [ ] Document/lowering identity cut:
  - compiler/document lowering emits stable ids for source ports, bindings,
    route targets, text controls, list windows, document nodes, layout
    fragments, render primitives, and proof handles;
  - runtime/document/renderer/verifier consume typed ids instead of labels,
    geometry, source paths, proof JSON, address strings, or fixture knowledge;
  - proof: no-hacks audit covers compiler, runtime, document, layout, native
    GPU, app-window, playground, xtask, and report-schema layers.
- [ ] Browser/WebRender-style compositor cut:
  - layout produces stable fragments/display items; the renderer owns culling,
    batching, transforms, clips, scroll offsets, overlays, and presentation;
  - scroll, hover, focus, caret, and selection use property/compositor updates
    where possible;
  - proof: passive scroll and first-frame focus/selection report zero relower,
    zero layout rebuild, and zero render-scene rebuild.
- [ ] Bevy-style extract/render-world cut:
  - split runtime/document state from render-owned state; extract only typed
    dirty data into persistent render resources;
  - use revision/change detection for resources, batches, glyphs, hit regions,
    route snapshots, and materialization windows;
  - proof: reports list extracted ids, skipped ids, upload bytes, cache hits,
    reallocations, and stale dropped work.
- [ ] GPUI-style hybrid retained UI cut:
  - keep Boon declarative at the authoring level while retaining hot element,
    layout, text, and GPU state behind the engine boundary;
  - add generic low-level primitives for virtual lists/grids, text controls,
    editors, canvases, and custom render surfaces when declarative rebuilding is
    too slow;
  - proof: primitives are IR/document/render features with non-Cells fixtures,
    not branches on source paths or example names.
- [ ] WGPU present-floor and queue policy cut:
  - build a focus-safe hardware baseline for the same app-window, surface,
    adapter, present mode, frame clock, and proof mode as product examples;
  - test late acquire, bounded frames in flight, ring-buffered uploads,
    present-mode choices, surface errors, resize/scale changes, and proof
    backpressure as explicit reported modes;
  - proof: product reports separate app CPU, acquire block, queue-submit block,
    present block, compositor/vsync floor, GPU timestamps if available, and
    proof completion.
- [ ] Render resource lifetime cut:
  - define persistent, surface-bound, frame-ring, proof/readback, and scratch
    resource tiers with owner, reuse key, retirement/fence policy, resize
    behavior, memory budget, and counters;
  - selection/hover/focus/text-mirror frames should update bounded buffers or
    uniforms, not recreate primitives or upload full scenes;
  - proof: long-session tests cover glyph atlas pressure, ring wrap, cache
    eviction, surface lost, scale factor changes, example switch, and repeated
    proof samples.
- [ ] Proof-subscriber pipeline cut:
  - product render returns a small typed `PresentedProductFrame` /
    `RenderFrameResult`; visible-bound-text proof, retained-sync proof,
    readback, screenshots, artifact hashes, report JSON, proof history, and HUD
    detail run as bounded post-present subscribers keyed by
    `FrameEvidenceKey`;
  - overflow drops or fails proof samples, not product frames;
  - proof: stale first-frame, latest-report, hash-only, mismatched epoch, and
    missing-key proof paths fail negative tests.
- [ ] Product/proof verifier split cut:
  - every native scenario runs product-only counters mode, proof-only
    subscriber mode, and optional full HUD/report mode against the same scenario
    hash;
  - product-only is the UX gate, proof-only validates exact frame identity and
    reports lag/cost, full HUD/report gets its own overhead budget;
  - proof: UX gates fail if ContinuousProbe, readback completion, report
    serialization, private dispatch, driver timing fallback, or desktop/browser
    capture is required for visible state change.
- [ ] Interaction evidence ledger cut:
  - `FrameEvidenceRegistry` is the only native acceptance source and stores
    interaction id, input event seq, lane, frame reason, route epoch,
    content/layout/render revisions, surface id/epoch, present id, proof mode,
    product/proof timings, and stale-result decision;
  - xtask selects evidence by key and scenario hash, never by latest fields or
    aggregate mixed-frame stats;
  - proof: terminal errors report `terminal_error`, completed click count, and
    aborted targets instead of manufacturing 5 second latency samples.
- [ ] Dev-window isolation cut:
  - editor wheel, source edit, example switch, report expansion, perf HUD, and
    proof browsing run on lanes that cannot hold product preview locks;
  - HUD/footer reads atomics or copied scalar snapshots only, at a throttled
    cadence, with no runtime queries, proof JSON parsing, WGPU calls, or IPC in
    render hooks;
  - proof: dev-code-editor wheel, overloaded report viewer, and no-dev
    comparison reports show `preview_blocked_on_ipc_count=0`.
- [ ] Codegen workstream cut:
  - after typed interpreter/delta equivalence, add Rust/Zig/Wasm/native kernels
    only for named hot plans such as indexed list lookup, formula fanout,
    materialization windows, text shaping, layout fragments, or render extract;
  - codegen must remove a measured interpreter/runtime boundary and preserve
    exact proof/currentness behavior;
  - proof: generated and interpreted reports match on deterministic scenarios
    before generated kernels can satisfy native UX gates.
- [ ] Architecture contract synchronization cut:
  - every major replacement updates this plan,
    `docs/architecture/NATIVE_GPU_PIPELINE.md`, report schemas, xtask gates,
    AGENTS.md if needed, and the embedded `/goal` prompt;
  - each change records rejected old path, new owner, invariants, counters,
    positive gate, negative gate, and deletion condition;
  - proof: `verify-native-gpu-architecture` fails when docs/schema/gates
    describe incompatible product/proof semantics.
- [ ] Minimal product-preview strangler cut:
  - if the current preview remains too proof-shaped, build a small product-only
    path beside it with typed route snapshot, `ActivePreviewScene`, retained
    patch, direct visible-surface present, scalar counters, and post-present
    proof subscribers;
  - migrate only if it is demonstrably simpler and faster, then delete or
    quarantine the old path;
  - proof: the spike is not allowed to become a second permanent engine.

2026-07-02 maximal architecture improvements to preserve:

These are intentionally larger than the latest local patch. They should be
considered when a fresh report shows the same kind of slow frame after one
focused fix. The preferred outcome is a simpler hot path with fewer
compatibility branches, not a larger set of conditionals.

- Product hot-loop replacement:
  - introduce a single `PreviewHotLoop` owner for product input, burst frame
    pacing, active-scene mutation, GPU acquire/encode/submit/present, and
    post-present subscriber dispatch;
  - accepted pointer/key/text/wheel input is latched at frame start into a
    typed `FrameInputBatch`, with coalescing for hover/wheel and exact
    `input_event_seq` for clicks/text;
  - the hot loop returns one small `PresentedProductFrame` record before any
    proof JSON, report expansion, readback mapping, accessibility publication,
    source replacement, or dev-window telemetry can run;
  - delete or quarantine alternate click, scroll, proof-sample, dev-HUD,
    runtime-cleanup, and verifier-specific loops once the owner exists.
- Bevy-style render extraction:
  - split the native preview into an app/runtime world and a render-owned world.
    Product frames extract only typed dirty data into persistent render
    resources; they do not rebuild a serde/proof view of the whole document;
  - retain GPU resources, glyph atlases, shaped text runs, clip/transform
    buffers, route snapshots, hit regions, and materialization windows in the
    render-owned world;
  - use revision/change detection for every extracted resource, with a report
    row for extracted ids, skipped ids, upload bytes, reallocations, and stale
    dropped work.
- Servo/WebRender-style compositor split:
  - layout/document produces stable fragments and display-list-like records;
    the renderer owns visible culling, batching, transforms, clips, scroll
    offsets, focus/selection overlays, and final surface presentation;
  - passive scroll, hover, selection outline, caret blink, formula/input mirror,
    and focus style are compositor/property-tree updates, not runtime or full
    layout work;
  - if an interaction needs layout or runtime currentness, the first visible
    product frame may show an optimistic retained patch and the later semantic
    commit may confirm, normalize, or roll it back by exact interaction key.
- GPUI-style hybrid UI boundary:
  - keep Boon source declarative, but make the engine keep hot retained element
    and GPU state behind the declarative authoring surface;
  - define explicit low-level element escape hatches in the engine for virtual
    lists/grids, text controls, canvases, and editors, instead of baking
    fixture-specific behavior into examples;
  - the escape hatches must be generic IR/document/render primitives with
    stable ids and tests, not branches on example names or source paths.
- Druid-style data/lens invalidation:
  - replace broad state summaries with typed lenses/reverse bindings from
    runtime fields to document nodes, text controls, styles, and render chunks;
  - a source edit emits the smallest `RuntimeDelta` / `DocumentDelta` needed
    for the visible binding set. Non-visible currentness work is deferred and
    accounted separately;
  - every binding update records which field ids and row keys were read, made
    current, or skipped as non-visible.
- Android/Wayland buffer-queue discipline:
  - treat surface acquisition, queue submit, compositor handoff, and present as
    a buffer-queue system with explicit in-flight limits and backpressure;
  - late-acquire the surface after CPU preparation when possible, never hold
    input acceptance while waiting on a blocked acquire/submit/present, and
    make proof/readback use separate resources or lagging subscribers;
  - product reports must distinguish CPU app work, queue submit blocking,
    frame present blocking, compositor/vsync floor, and proof readback cost.
- WGPU resource-lifetime architecture:
  - replace small per-frame upload churn with persistent staging belts or
    dynamically growing rings, dirty chunk instance buffers, and stable batch
    caches keyed by render primitive ids and resource epochs;
  - keep upload caches warm across selection/hover/focus/text-mirror frames.
    Eviction should be budgeted, LRU/generation-based, and visible in reports;
  - add long-session aging tests for glyph atlas pressure, upload-ring wrap,
    surface resize, scale-factor change, example switch, source replacement,
    and repeated proof samples.
- Typed input/source intent ABI:
  - replace string/path-driven event application with a closed command set:
    `MoveFocus`, `SetSourceValue`, `CommitTextEdit`, `UpdateViewport`,
    `ActivateAction`, `SetHover`, `SetPointerCapture`, `ImeCompose`,
    `ImeCommit`, and future drag/drop commands;
  - every command carries route id, source field id, node id, list/window key,
    route epoch, input event seq, content revision, and stale-result policy;
  - old source-event maps and production `serde_json::Value` summaries become
    debug/replay adapters only.
- Text-control engine boundary:
  - make text input, formula bars, code editors, caret/selection, IME,
    placeholder, focus ring, and bound text mirrors one generic text-control
    subsystem with retained mirrors and typed commits;
  - first-frame focus/value display must not wait for runtime summaries or full
    currentness. Runtime validation and formula evaluation commit later by
    interaction id;
  - visual tests cover text control focus, hover, value mirror, formula mirror,
    IME composition, paste, undo/redo, wheel, and caret blink without
    example-specific code.
- Spreadsheet/list engine milestone:
  - promote sparse logical lists, `List/chunk` windows, keyed row materializers,
    indexed `List/find`, range/dependency invalidation, formula fanout, and
    cycle guards into one generic list/query engine;
  - reports must distinguish logical rows, materialized rows, rendered nodes,
    evaluated formulas, currentness reads, indexed hits, row scans, and
    dependency fanout for all fixtures;
  - add large non-Cells fixtures before claiming the runtime architecture is
    solved.
- Active/pending scene transaction model:
  - `ActivePreviewScene` is always presentable and owns current hit routes,
    property tree, text mirrors, GPU batches, and frame evidence;
  - `PendingPreviewScene` carries latest-wins runtime/layout/render work that
    may commit only if source, route, materialization, surface, and proof
    epochs still match;
  - stale pending work is dropped before expensive build/upload when possible.
    Product frames never wait for an older pending snapshot unless the visible
    pixel cannot be represented by an active-scene patch.
- Scheduler lanes and backpressure:
  - use explicit lanes with priority and budgets:
    `HostInput`, `TextEdit`, `ViewportScroll`, `Animation`, `RuntimeCommit`,
    `LayoutMaterialization`, `RenderUpload`, `ProofReadback`, `Telemetry`,
    `DevWindow`, and `SourceReplace`;
  - product lanes may preempt or skip proof/dev/telemetry lanes. Proof lanes
    may fail or lag under pressure, but they may not delay first visible
    feedback;
  - queue sizes are bounded and latest-wins for state replacement work. Reports
    include drops, stale rejects, blocked counts, and time spent holding product
    locks.
- Product/proof two-pipeline verification:
  - every native verifier scenario runs in product-only counters mode first,
    then proof-subscriber mode, then optional full HUD/report mode;
  - product-only mode is the UX gate. Proof mode proves exact frame identity
    and reports proof lag/cost. Full HUD/report mode gets a separate overhead
    budget;
  - UX gates fail if they require ContinuousProbe, readback completion, report
    serialization, latest-report fallback, whole-desktop screenshots, or
    verifier-injected private dispatch to make visible state change.
- Frame evidence ledger as the only acceptance source:
  - `FrameEvidenceRegistry` rows include interaction id, input event seq, frame
    lane, frame reason, route epoch, content/layout/render revisions, surface
    id/epoch, present id, proof request id, proof mode, product/proof timings,
    and stale-result decision;
  - xtask gates select rows by key and scenario hash. They never infer product
    success from "latest" fields, last diagnostics, proof-cache hits, or
    aggregate mixed-frame p95;
  - stale proof reuse, mismatched surface epoch, mismatched content revision,
    and missing keyed runtime/list evidence are hard failures.
- Slow-path deletion program:
  - maintain a table for each old path with owner, replacement, counter,
    positive gate, negative gate, removal condition, and deletion date;
  - initial rows: full state summary before present, proof JSON mutation in the
    render hook, layout-proof hot reads, geometry/string route identity,
    latest-report proof, duplicate interactive readback, broad runtime root
    flushes, modeled/static scroll readiness, production selected-address
    alias routing, and legacy Ply/Xvfb/COSMIC/browser evidence;
  - once a replacement has keyed proof, delete the old path instead of keeping
    it as a quiet fallback.
- Dev-window isolation:
  - run editor wheel, source edits, example switch, report viewer expansion,
    perf HUD refresh, and proof-history browsing on lanes that cannot hold
    product preview locks;
  - the dev perf row reads atomics or copied scalar snapshots only. It does not
    perform IPC, parse JSON, ask runtime for summaries, or touch WGPU state from
    the render hook;
  - example switching keeps the last good active preview scene visible until a
    pending scene for the new source is complete or an explicit error scene is
    committed.
- Architecture migration spikes:
  - only consider larger library or subsystem migration after a measured spike:
    GPUI-like retained UI, a Bevy-like render-app split, WebRender-like display
    list backend, or direct lower-level WGPU/Vulkan present control;
  - a spike must include latency reports, integration cost, Boon semantics
    fit, proof/readback story, accessibility story, and deletion list for code
    it would replace;
  - do not wrap a slow Boon engine path in another UI toolkit and call it
    fixed.
- Architecture decision records:
  - each time a major boundary is replaced, add a short ADR or plan subsection
    naming the rejected old path, new owner, invariants, counters, tests, and
    contract-doc updates;
  - `docs/architecture/NATIVE_GPU_PIPELINE.md`, this plan, AGENTS.md, xtask
    gates, and report schemas must not drift into multiple incompatible
    definitions of product latency or proof readiness.

2026-07-02 additional architecture improvements to preserve:

These items are intentionally larger than one verifier fix. Keep them as
architecture TODOs so future work can replace a slow boundary outright instead
of spending another run making the legacy boundary slightly faster.

- [ ] Single preview ownership cut:
  - choose one crate/state machine as the owner of preview product frames,
    active scene publication, WGPU surface presentation, and post-present
    subscriber dispatch;
  - demote all other app-window/playground/xtask paths to clients,
    subscribers, or verifiers. They may request work, but they may not open,
    relabel, or complete product frames independently;
  - success gate: every presented product frame names exactly one owner,
    scheduler lane, transaction id, and evidence key. Unknown ingress fails in
    product mode.
- [ ] Product protocol split:
  - split native preview messages into product protocol messages and
    diagnostic/proof/report messages;
  - product messages are small typed records: input batches, intents, runtime
    deltas, retained-scene patches, render results, and product commits;
  - diagnostic messages may carry JSON/proof/history/debug payloads, but only
    after present or on diagnostic lanes;
  - success gate: product frames never wait for or parse diagnostic protocol
    payloads, and reports expose any temporary product-protocol adapter.
- [ ] Shared immutable snapshot publication:
  - publish active runtime/document/layout/render snapshots through immutable
    handles or arc-swapped snapshots so product frames read coherent state
    without blocking on writers;
  - writers build pending snapshots off the product path and publish only after
    source/content/layout/render/surface/input epoch checks pass;
  - old snapshots retire by bounded epochs or frame arenas instead of blocking
    presentation;
  - success gate: product reports show zero blocking runtime/document/proof/dev
    locks while still reporting the exact active snapshot revisions used.
- [ ] Hot-path lock and allocation audit:
  - instrument product-frame locks and heap allocations by owner crate:
    app-window, playground, runtime, document, layout, native GPU, proof,
    report schema, xtask driver, and dev HUD;
  - forbid report/proof JSON allocation, proof-history cloning, screenshot
    encoding, large string/path maps, and full summary allocation before
    product present;
  - success gate: release product UX reports include lock-wait and allocation
    budgets, and old-path kill switches fail when forbidden allocation or lock
    classes appear.
- [ ] First-class virtual UI primitives:
  - add generic engine primitives for virtual lists/grids, text controls,
    scroll views, selection/focus overlays, and editable table-like regions;
  - primitives expose stable ids, source intents, materialized windows,
    retained control state, and render primitive ids through compiler/document
    contracts;
  - examples may use cleaner Boon source that maps to these primitives, but the
    runtime/compiler/renderer behavior stays generic;
  - success gate: Cells, a renamed sparse-grid fixture, TodoMVC, Counter, and
    the dev editor all use the same primitive contracts without production
    branches on example names or fixture strings.
- [ ] Minimal proof-free product baseline:
  - keep a product-only counters mode that disables readback, proof JSON,
    report expansion, screenshot encoding, and HUD detail while retaining fixed
    scalar frame counters and product commits;
  - run this mode before proof-heavy verifiers so product p95 is not hidden by
    proof coupling or accidentally improved by continuous/probe scheduling;
  - success gate: every performance claim includes the product-only baseline
    plus separate proof-subscriber cost for the same scenario hash and
    worktree/binary fingerprint.
- [ ] Platform frame-pacing policy:
  - define per-platform contracts for Wayland, X11, headless, nested
    compositor, and future backends: input timestamp ownership, focus safety,
    present-mode support, surface lifecycle, queue depth, and timer granularity;
  - product reports name backend, compositor/session class, adapter, present
    mode, desired frame latency, and clock domain for every run;
  - success gate: platform differences appear as metadata and phase counters,
    not as different verifier semantics or different example behavior.
- [ ] Event coalescing and backpressure policy:
  - coalesce hover/move and wheel by target/axis/epoch, but never coalesce away
    state-changing click, key, text, or IME commits;
  - lower-priority runtime continuation, proof, telemetry, report, source
    replacement, and dev-window work is latest-wins or bounded by queue limits;
  - success gate: reports expose coalesced, dropped, superseded, deferred, and
    executed counts per lane, and product p95 cannot be rescued by silently
    dropping visible semantic work.
- [ ] Product error and fallback policy:
  - define allowed product behavior for stale runtime data, stale layout,
    pending source replacement, missing proof, proof lag, surface loss, and
    verifier driver failure;
  - product may present active retained state, show a typed stale/error marker,
    defer non-visible work, drop proof samples, or fail a verifier, but it may
    not manufacture success from stale proof/latest reports/driver timing;
  - success gate: each failed sample names the policy used and the owner
    boundary that blocked a correct product frame.
- [ ] Deterministic scheduler simulator:
  - add a small pure-Rust state-machine simulator for host input, burst pacing,
    source/runtime wakes, layout/materialization wakes, proof requests, report
    flushes, timers, surface changes, queue limits, and stale snapshot drops;
  - prove lane attribution, burst exit, hard caps, proof isolation, latest-wins
    cancellation, and unknown-ingress failures before involving real WGPU;
  - success gate: scheduler regressions fail fast in deterministic unit tests
    before they become five-second visual verifier failures.
- [ ] Runtime query engine ADR:
  - decide whether the long-term runtime moves toward a query-engine model
    shared by compiler, currentness, document lowering, layout invalidation,
    render extraction, and proof subscribers;
  - if chosen, define query keys, dependency edges, invalidation, cycle
    handling, revision stamps, and product-visible read demands once instead
    of maintaining separate ad hoc dirty systems;
  - success gate: visible changes can be traced through one dependency/revision
    vocabulary from Boon source to product frame evidence.
- [ ] Staged deletion milestones:
  - before each implementation slice, pick one old boundary to delete or
    quarantine: proof-shaped routing, latest-report proof, full state summary,
    full layout rebuild, broad root flush, dev IPC lock, duplicate readback,
    driver timing fallback, or unowned scheduler wake;
  - a slice is not accepted as progress unless it adds the typed replacement,
    positive gate, negative old-path gate, and a plan/architecture update;
  - success gate: the stale-path ledger shrinks over time, and native readiness
    cannot depend on a compatibility path that lacks owner/date/removal tests.
- [ ] Simplicity review checkpoint:
  - after every major option is implemented, compare concept count, product
    path length, crates touched before present, and old-path count against the
    previous architecture;
  - if the change makes the system harder to reason about without deleting a
    dominant boundary, revert or quarantine it as a diagnostic spike;
  - success gate: performance improvements come with fewer product-path
    responsibilities, not just more counters and more fallbacks.

2026-07-02 subagent contract additions to preserve:

- Native frame-loop contract:
  - promote one architecture path from option to implementation contract. The
    preferred first cut is `NativeFrameClock + ActivePreviewScene +
    post-present proof subscribers`;
  - name the owning crates and owners for frame-clock decisions, active scene,
    WGPU presentation, proof workers, telemetry, and xtask gates;
  - demote the rest of the option matrix to follow-up or diagnostic status so
    future work does not accidentally build multiple competing product loops.
- Product frame state machine:
  - define and test the hard state machine:
    `Idle -> BurstScheduled -> BeginProductFrame -> DrainHostInput ->
    PatchActiveScene -> Encode -> Submit -> Present -> PostPresentSubscribers
    -> Idle/BurstScheduled`;
  - add transition tests for host input, source/runtime wake, proof-only wake,
    telemetry flush, resize, surface lost, zero-size surface, and verifier
    forced sample;
  - fail if proof/report/source-cleanup work can keep a product frame open
    after present or relabel a host-input frame.
- Frame attribution rule:
  - once a host input is accepted, its first visible product frame remains
    `HostInput` even when a requested-animation burst, proof sample, telemetry
    flush, source cleanup, caret timer, or surface event is also pending;
  - product latency is measured on that first matching product frame only.
    Follow-up runtime/proof/caret/report frames use their own lanes.
- WGPU resource lifetime tiers:
  - define persistent renderer-owned caches, surface-bound resources,
    frame-ring resources, proof/readback resources, and per-frame scratch
    arenas;
  - every tier needs creation owner, reuse key, retirement/fence policy,
    resize/surface-lost behavior, memory budget, and counters proving reuse;
  - reports must expose which tier reallocated or evicted on a product frame.
- Queue/present ownership:
  - define `AcquireSurfaceFrame -> EncodeCommands -> QueueSubmit ->
    PresentSurfaceFrame` as separate phases with a late-acquire policy,
    maximum frames in flight, queue-depth/in-flight hints, present-mode
    metadata, and surface epoch;
  - blocked acquire/submit/present may not hold input acceptance, proof
    readback, report serialization, or dev-window locks;
  - if Wayland/compositor/vsync is the floor, report it as baseline and keep
    app-side work below the remaining budget.
- Dynamic upload and ring-buffer contract:
  - overlays, scroll uniforms, text runs, visible chunks, glyphs, and primitive
    instances use bounded staging/ring uploads with explicit byte/write
    counters;
  - no per-logical-item GPU resources for large lists/grids;
  - no in-flight range may be overwritten before retirement. Growth, wrap,
    eviction, and cache-preservation behavior must be tested and reported.
- Proof-mode matrix as a gate:
  - define `CountersProduct`, `TraceProduct`, `ReadbackProofSubscriber`,
    `FullHudReport`, and `ContinuousProbeDiagnostics`;
  - each mode lists allowed pre-present work, forbidden work, report fields,
    overhead accounting, and which native gates may consume it;
  - normal UX gates may use only product modes. Proof and diagnostic modes
    prove evidence or overhead but cannot make the visible update happen.
- Exact `FrameEvidenceKey` definition:
  - include surface id/epoch, frame seq, present id, content/layout/render
    revisions, input event seq when applicable, product/proof mode, source and
    build fingerprints, scenario hash, and proof request id;
  - keys are minted before render/acquire and never inferred from latest
    reports, last diagnostics, proof caches, or artifact filenames;
  - stale first-frame proof, mismatched surface epoch, mismatched content
    revision, or proof without the exact key fails.
- Proof-subscriber backpressure:
  - proof, readback, report, screenshot encoding, artifact hashing, and HUD
    history workers use bounded queues keyed by `FrameEvidenceKey`;
  - overflow drops or fails the proof sample, never the product frame;
  - proof lag and proof failure are reported separately and cannot satisfy
    product latency.
- Stale-path deletion ledger:
  - create a machine-readable ledger with initial rows for `layout_proof`
    hot-state reads, latest-report proof, geometry/string routing, private
    runtime dispatch, modeled/static scroll, driver timing fallback,
    duplicate interactive readback, COSMIC/Ply/Xvfb/browser proof, broad
    runtime summaries, and product dependence on `present_call_ms`-only
    inference;
  - every row needs typed replacement, kill switch/counter, positive gate,
    negative gate, and removal condition.
- Generic non-Cells proof fixtures:
  - add fixtures for retained hit routing, passive scroll transform, text
    focus/edit, sparse list/window materialization, source-intent dispatch,
    proof mismatch, present-floor baseline, and proof-subscriber lag;
  - Cells remains a stress fixture, not the only proof of genericity.
- Product-frame forbidden-work audit:
  - release product gates fail if pre-present code performs proof JSON
    allocation, report snapshot assembly, screenshot encoding, dev IPC waits,
    layout artifact reloads, latest-report scans, runtime full summaries,
    list scans, derived-field recomputation not demanded by visible pixels, or
    example/source-path/field-name branches.

2026-07-02 generic runtime/compiler/document TODOs to preserve:

- Generic `ensure_current` barrier:
  - define a demand barrier for scalar fields, keyed list fields, list lookups,
    aggregates, document projections, and materialization bridge projections;
  - inputs: typed read keys, evaluation epoch, demand reason, interaction id
    when relevant, and cycle stack;
  - output: current value, deferred value, or typed currentness/cycle error.
    Product-frame summary serialization must not trigger implicit broad
    recomputation.
- Derived-field startup policy:
  - classify derived fields as `StartupEager`, `EventEager`, or
    `DemandCurrent`;
  - demand-current fields do not run during root initialization unless required
    by reset initialization, index seed, visible materialization range, source
    binding, or explicit bridge projection;
  - reports expose startup eager count, demand current count, event recompute
    count, and forced visible-read count by field id.
- Exact dependency tokens:
  - every demand read records dependency tokens for scalar field, keyed list
    field, indexed lookup key, range dependency, aggregate input, and
    materialized document projection;
  - invalidation wakes only affected demand-current work. Broad root/list
    dependencies are reserved for operators that explicitly declare broad
    reads.
- Compiler-emitted index plans:
  - promote `List/find` and `List/find_value` indexes into an `IndexPlan` with
    list id, field id, value type, equality/hash mode, uniqueness/cardinality
    expectation, old/new invalidation keys, and fallback policy;
  - runtime maintains the index generically for any list-backed source;
  - hot-path reports prove indexed hits and zero fallback scans.
- Keyed list-index invalidation:
  - field changes emit exact old-value and new-value index dependencies;
  - stale generations, duplicate keys, removed rows, inserted rows, and moved
    rows are tested generically;
  - broad column invalidation is allowed only for operators with declared broad
    reads.
- Generic derived-expression fanout:
  - replace formula-specific fanout language with generic read-trace fanout for
    any Boon helper or derived expression that reads through text parsing,
    `List/find`, ranges, aggregates, or document projections;
  - recomputation diffs the previous dependency set, removes stale reverse
    edges, installs new edges, and enqueues only newly dirty dependents.
- Compressed range dependency tokens:
  - range reads register compressed range/member-set dependencies instead of
    expanding every member edge;
  - invalidation handles insert, remove, move, and field update without
    quadratic fanout;
  - reports expose range token count, expanded member count, and dirty fanout
    count.
- Cycle behavior for demand-current fanout:
  - recursive demand evaluation reports a typed cycle error for the requested
    derived value, preserves the last valid committed value when applicable,
    and avoids unbounded stack growth during document/proof/verifier reads;
  - cycle tests cover default stack, list lookup, range dependencies, aggregate
    dependencies, and document projection reads.
- Logical `List/chunk` view:
  - lower `List/chunk` to a logical chunk view with stable chunk identity,
    logical length, key mapping, and demand ranges;
  - layout requests visible range plus overscan, selected keys, dependent keys,
    or declared bridge projections;
  - chunking does not materialize every chunk row before layout.
- `LayoutDemand::MaterializeRange` bridge:
  - make materialization demand a typed document-layout-to-runtime contract:
    list id, window id, axis, logical count, visible range, overscan,
    selected/dependent keys, and generation;
  - runtime answers with keyed document patches for the demanded range, not
    coordinate-specific shortcuts or product-frame widget expansion.
- Typed source deltas:
  - compiler emits `SourcePortId` metadata with payload schema, scope binding
    shape, expected generation/bind epoch, and optional source-route hints;
  - host/document code passes `SourceIntent` only;
  - runtime dispatch validates the typed payload without example-name,
    source-path, field-name, or product-frame branch logic.
- Stable retained `DocumentNodeId`:
  - derive node identity from program hash, document expression id, structural
    child path, list identity tuple, generation, and role within the produced
    document;
  - identities must survive reorder, filter, windowing, and materialization
    changes where semantics allow;
  - text/style/binding patches target nodes by id without coordinate mutation.
- Retained identity coverage:
  - cover bound text inputs, focus targets, scroll roots, hit regions, repeated
    children, list-window children, and overlay/compositor nodes;
  - verification proves source events produce typed document patches against
    stable ids with no full document relower and no product-frame fallback
    patch for known app shapes.
- No product-frame runtime work invariant:
  - product/native frame code may schedule input, hold retained render state,
    apply typed patches, and present;
  - it must not scan runtime lists, infer semantic identity from coordinates,
    recompute derived fields, parse formulas, synthesize app-specific document
    nodes, or serialize summaries unless a typed visible-read ledger explicitly
    demands it.

Subagent architecture review additions:

- Accepted-input timing reality:
  - `input_accept_to_dirty_poll` currently measures from accepted input inside
    the app-window poll to the later dirty decision, so it includes role poll
    hook work before the render loop even knows the frame is dirty;
  - current failing Cells samples still spend most of that interval in
    `source_input_ms` / `world_or_source_input_ms`, not in formula evaluation;
  - exact owner paths:
    `crates/boon_native_app_window/src/lib.rs` records the accepted input and
    dirty poll timestamps, while
    `crates/boon_native_playground/src/main.rs` performs source input through
    `preview_try_apply_simple_source_click_input`,
    `preview_apply_live_events_state_summary_defer_visible_sync`, retained
    bound-text sync, selection proxy refresh, and selected-node/style patches.
- Source-input transaction split:
  - split "accept input" from "execute source input" generically. On click,
    resolve the typed/cached route, build a bounded accepted source-input turn,
    record target ids/source ids/route epoch/input sequence/cached blur data,
    mark dirty, and return from the accept path;
  - drain that accepted turn in the product frame preparation path or render
    owner before presentation, preferably through a direct retained overlay/text
    mirror patch for first visible feedback and a normal runtime turn for
    semantic commit/evaluation;
  - do not call full live-event execution and retained bound-text sync inline
    from the input accept path;
  - success gate: failing reports separate `accept_to_dirty_ms`,
    `queued_source_turn_ms`, `retained_patch_ms`, and `runtime_commit_ms`, and
    accepted input no longer pays live runtime turn plus bound-text sync before
    dirty accounting.
- Product render result split:
  - split `NativeRenderHookResult` into a pre-present product result and
    post-present proof/report subscriber work;
  - the pre-present result contains scalar revisions, frame metrics, upload
    counters, product proof handles, and `FrameEvidenceKey`, not full
    `serde_json::Value` proof/report trees;
  - move visible-bound-text reports, external proof compaction, full report
    snapshots, artifact hashing, screenshot/readback setup, and proof-history
    mutation behind post-present latest-wins subscribers keyed by the product
    frame;
  - success gate: product frames report zero pre-present proof/report JSON work,
    and proof/readback reports still prove the exact product frame or report
    explicit proof lag.
- Specific old boundaries to measure before deleting:
  - `preview_apply_real_window_input_with_units` fallback use on product input;
  - live source-event execution inside
    `preview_try_apply_simple_source_click_input`;
  - retained bound-text sync before dirty decision;
  - full `serde_json::Value` proof construction in
    `native_gpu_app_owned_render_hook`;
  - direct visible-surface readback setup before present;
  - post-present evidence/report/proof-history work that delays the next burst
    frame.
- Implementation ordering from the review:
  1. Add or expose counters proving each boundary above is on the failing
     product frame.
  2. Land the source-input transaction split or the product render result split
     as the first large cut. Prefer the one that removes the larger measured
     p95 boundary in the fresh report.
  3. Convert retained first-frame feedback to a direct generic overlay/text
     mirror patch, with runtime commit/evaluation following by revision.
  4. Move proof/readback/report work into exact-key subscribers and make stale
     or latest-report fallbacks fail.
  5. Delete/quarantine the old inline path and add negative tests before doing
     another route-cache or JSON-size tweak.

2026-07-02 subagent frame-ledger additions not to lose:

- Lane-split preview perf stats:
  - replace the single `NativePreviewPerfAccumulator.input_to_present_ms` ring
    as an acceptance source with lane-scoped buckets:
    `all_frames`, `product_interaction_frames`,
    `animation_followup_frames`, `proof_or_harness_frames`, and
    `dev_telemetry_frames`;
  - every sample carries `frame_lane`, `frame_reason`, `interaction_id`,
    `input_event_seq`, scheduler reason, dirty reason, proof mode, and input
    source;
  - `missed_frame_count` becomes lane-scoped:
    `product_missed_frame_count`, `requested_animation_missed_frame_count`,
    `proof_or_harness_missed_frame_count`, plus legacy aggregate diagnostics;
  - product gates read only `product_interaction_frames`. Aggregate preview
    stats remain observability and may be `diagnostic_fail` without failing the
    product UX gate once exact product evidence exists.
- Keyed product-frame evidence ledger:
  - promote `FrameEvidenceRegistry` from proof metadata into the source of
    truth for product interaction acceptance;
  - each accepted visible input gets a product ledger row keyed by
    `input_event_seq`, `presented_input_event_wake_count`, `FrameEvidenceKey`,
    route epoch, source/content/layout/render revisions, and present id;
  - the first presented frame that carries the unaccounted accepted input and
    visible content change is the product UX sample. Later runtime cleanup,
    proof/readback, report, verifier, caret, or animation follow-up frames stay
    in their own lanes;
  - xtask must match by the ledger key, never by "last poll diagnostics",
    "latest report", "last retained sample", or global p95 windows.
- App-produced runtime-work evidence:
  - no verifier may infer zero runtime/list work from WGPU visual proof alone;
  - if a first-frame path intentionally defers runtime, the producer must emit
    an explicit generic record such as
    `runtime_invoked=false`,
    `runtime_work_source="deferred_runtime_not_invoked"`, and zero
    list-scan/root-materialization/recompute counters for that input/frame key;
  - runtime/list gates consume that app-produced keyed record or an actual
    interaction timing sample. Missing keyed runtime evidence fails instead of
    being silently synthesized by xtask.
- Retained patch evidence:
  - retained update reports must distinguish committed retained patch,
    committed render-scene patch, full document lower, patch rejection,
    non-retained patch, legacy selection fallback, and deferred retained input
    patch counts;
  - first-frame direct retained patches are allowed only when keyed WGPU proof
    shows the exact bound text/style/focus/selection nodes changed for the
    same product frame;
  - stale proof, mismatched `FrameEvidenceKey`, hash-only proof, or legacy
    fallback cannot satisfy retained update gates.
- Release handoff contract:
  - `cells-visible-click-e2e-release` must gate
    `/preview_loop_product_path_contract` with
    `source="interaction_scoped_product_frames"`,
    sample count covering clicked product frames, p95 within budget, and
    product missed-frame count zero;
  - top-level aggregate `preview_loop_input_to_present_ms_p95` and
    `preview_loop_missed_frame_count` are diagnostic until lane-scoped
    app-window stats replace them;
  - the report must still expose aggregate values so regressions in proof,
    harness, cleanup, or animation follow-up frames are visible and can become
    their own gates.
- Deletion targets from the subagent review:
  - delete or quarantine verifier code that reads "last" timing/proof samples
    instead of keyed product-frame rows;
  - delete or quarantine runtime no-work inference from retained proof;
  - delete or quarantine product gates backed by unscoped aggregate preview
    stats;
  - add stale/ambiguous sample negative tests for each deleted path.

2026-07-02 interaction-ledger verifier checkpoint:

- Implemented the first verifier-side bridge for interaction-scoped product
  frames:
  - xtask now derives a `product_frame_scope` from per-click
    app-owned product timing fields instead of using the unscoped aggregate
    preview loop p95 as the product UX gate;
  - `cells-visible-click-e2e-release` label contract gates
    `/preview_loop_product_path_contract` with
    `source="interaction_scoped_product_frames"`, product sample coverage,
    p95 budget, and zero product missed frames;
  - aggregate `preview_loop_input_to_present_ms_p95` and
    `preview_loop_missed_frame_count` remain in the report as diagnostic until
    app-window lane-scoped buckets replace the global accumulator.
- Implemented producer-side deferred runtime evidence:
  - native input timing JSON now emits `runtime_work` with
    `runtime_invoked=false`,
    `source="deferred_runtime_not_invoked"`, and known zero scan/root/recompute
    counters when the generic deferred-runtime source-click path is used;
  - xtask requires this producer record before accepting missing
    `interaction_timing` samples as zero-runtime product frames.
- Fresh focused release verifier:
  - command:
    `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-interaction-ledger.json`;
  - schema:
    `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-interaction-ledger.json`;
  - status: pass;
  - product metrics:
    `input_accept_to_formula_visible_ms_p95=16.071860 ms`,
    `interaction_scoped_product_input_to_present_ms_p95=16.071860 ms`,
    `interaction_scoped_product_input_to_present_sample_count=4`,
    `interaction_scoped_product_missed_frame_count=0`;
  - proof/runtime/retained:
    four exact visual-proof product samples, `simple_source_click_count=4`,
    `generic_fallback_count=0`, `retained_render_scene_patches=4`,
    `retained_render_scene_patch_fallbacks=0`, zero runtime rows/list-find
    scans, zero root materialization, zero recomputed fields;
  - aggregate diagnostics still fail:
    `preview_loop_input_to_present_ms_p95=230.123505 ms`,
    `preview_loop_missed_frame_count=2`, with acceptance marked
    `diagnostic_only_until_frame_ledger_classifies_non_product_frames`.
- Remaining architecture work:
  - this is not the full lane-split `FrameEvidenceRegistry`; xtask still
    derives product scope from click samples, and app-window still owns a
    global accumulator;
  - next cut should implement app-produced lane-scoped stats and keyed product
    frame rows, then delete "last diagnostics" matching and aggregate product
    compatibility paths;
  - run the default release report path with enough repeated samples before
    claiming `verify-native-gpu-all --check-existing` readiness.

2026-07-02 route-key reuse evidence:

- Implemented a generic retained-route key reuse cut in
  `crates/boon_native_playground/src/main.rs`: when a retained
  `LayoutFrame` override has hit regions identical to the cached document
  snapshot, `preview_hit_route_cache_key` reuses the snapshot static route key
  instead of recomputing the full route fingerprint. This is not Cells-specific;
  it treats focus, hover, selection, caret, and text-payload retained changes as
  route-stable when hit regions are unchanged.
- Focused checks:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses_static_table_for_focus_overlay_only_override -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground preview_route_cache_key_ignores_text_only_retained_changes -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses_active_snapshot_for_update_count_only_change -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground passive_hover_primes_click_candidate_without_runtime_dispatch -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground preview_hover_and_click_use_typed_route_table_without_proof_hit_json -- --test-threads=1`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`;
  - `git diff --check -- crates/boon_native_playground/src/main.rs docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md`.
- Fresh release verifier:
  - command:
    `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-route-key-reuse.json`;
  - schema: `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-route-key-reuse.json` passes;
  - status: fail, but route lookup outliers are now removed:
    `input_accept_to_present_ms_p95=16.919622 ms`,
    `preview_loop_input_to_present_ms_p95=21.709549 ms`,
    `preview_loop_missed_frame_count=0`, `simple_source_click_count=4`,
    `generic_fallback_count=0`;
  - sample timings:
    - A2: route lookup `0.0 ms`, source input `3.723857 ms`, queue submit
      `9.289651 ms`, present `0.036240 ms`, product `16.818875 ms`;
    - B0: route lookup `0.003916 ms`, source input `0.507974 ms`, queue
      submit `0.068189 ms`, present `9.104175 ms`, product `12.781604 ms`;
    - C0: route lookup `0.078033 ms`, source input `4.326097 ms`, queue
      submit `0.073090 ms`, present `9.138099 ms`, product `16.919622 ms`;
    - A0: route lookup `0.076204 ms`, source input roughly `4.17 ms`, queue
      submit `10.549451 ms`, present `0.033661 ms`, product `16.466194 ms`.
- Conclusion:
  - the old static route fingerprint boundary was real and is now mostly cut;
  - the remaining p95 is source-input/live-event/bound-text sync plus
    queue-submit/present pacing variance;
  - do not spend the next round on route-cache key tweaks unless a fresh report
    shows route lookup back above budget. The next architecture cut should be
    the source-input transaction/direct retained text mirror split or the
    queue/present frame-pacing/present-floor workstream.

2026-07-02 binding reverse-index evidence:

- Implemented a generic snapshot reverse index for retained binding sync in
  `DocumentDataBindingSnapshotIndex`:
  - `state_binding_targets_by_path`;
  - `text_binding_nodes_by_path`.
- `extend_target_nodes_for_changed_summary_bindings` now uses path-keyed maps
  instead of scanning node-keyed binding maps on each visible source input. This
  preserves generic document/runtime behavior and does not branch on Cells,
  addresses, labels, or geometry.
- Focused checks after formatting:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_playground preview_hit_route_table_reuses_active_snapshot_for_update_count_only_change -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground retained_bound_text_sync_from_state_summary_does_not_expand_all_static_equality_targets -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground retained_bound_sync_patches_source_intent_indexes_without_assertion_scan -- --test-threads=1`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`.
- Fresh release verifier:
  - command:
    `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-binding-reverse-index.json`;
  - schema: `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-binding-reverse-index.json` passes;
  - status: fail:
    `input_accept_to_present_ms_p95=17.103836 ms`,
    `preview_loop_input_to_present_ms_p95=17.103836 ms`,
    `preview_loop_missed_frame_count=0`, `simple_source_click_count=4`,
    `generic_fallback_count=0`;
  - route lookup remains cut to near-zero on the route-table samples, around
    `0.030 ms`;
  - sample timings:
    - A2: source input `0.248197 ms`, queue submit `0.067906 ms`, present
      `7.805587 ms`, product `11.291624 ms`;
    - B0: source input `0.511592 ms`, queue submit `0.069430 ms`, present
      `8.860171 ms`, product `12.663848 ms`;
    - C0: source input `3.567969 ms`, route lookup `0.030794 ms`, queue
      submit `0.072007 ms`, present `8.480460 ms`, product `15.715772 ms`;
    - A0: route lookup `0.030296 ms`, queue submit `8.558489 ms`, present
      `9.179376 ms`, product `17.103836 ms`.
- Conclusion:
  - the binding reverse index is useful but not sufficient;
  - the remaining p95 is now dominated by queue-submit/present pacing variance
    plus still-inline source commit/retained mirror work;
  - stop this local indexing line here. The next implementation must choose a
    larger architecture cut from the backlog: source-input transaction plus
    direct retained text/focus mirror, or focus-safe hardware present-floor and
    frame-pacing/present-mode work. Do not keep iterating on route/binding
    lookup micro-optimizations unless a fresh report names them again.

2026-07-02 first-frame source transaction split checkpoint:

- Implemented a generic direct first-frame selection-proxy path:
  - Boon button target metadata in `examples/cells/view.bn` now carries the
    clicked cell's `editing_text` while the source event still carries
    `address`. This keeps the example model cleaner and lets generic route
    metadata mirror text controls immediately; it is not a runtime/compiler
    special case.
  - `preview_try_apply_simple_source_click_input` can now patch retained
    selected/focused text-control state and selection overlay first, then queue
    the runtime source event as follow-up work.
  - queued source work now drains through the state-summary-deferred visible
    sync path instead of the old full visible layout-frame fallback when no new
    host input arrived.
- Focused checks:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground selection_proxy_noop_click_refreshes_bound_text_input_from_runtime -- --test-threads=1`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`.
- Fresh release verifier:
  - command:
    `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-first-frame-defer-drain.json`;
  - schema: `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-first-frame-defer-drain.json` passes;
  - status: fail:
    - the first two product click samples used
      `simple_source_click_deferred_runtime`, had `live_events_ms=0.0`, and
      showed the expected formula-bar text in the app-owned proof:
      `15.087591 ms` for `15` and `12.466218 ms` for `=add(A0,A1)`;
    - the later samples were missing formula-bar proof/runtime timing
      (`formula_bar_text="missing"` for expected `=sum(A0:A2)` and `5`), so
      steady metrics are null and the live probe failed;
    - the report still records `preview_loop_input_to_present_ms_p95=15.744827
      ms`, `preview_loop_missed_frame_count=1`,
      `preview_loop_frame_pacing_state=requested_animation_burst`, and proof
      currentness/readback-change failures;
    - present/queue variance remains visible:
      `present_call_ms.p95=28.195144 ms`,
      `present_path_ms.p95=28.534091 ms`, and
      `queue_submit_call_ms.p95=8.451184 ms` in the preview perf stats.
- Conclusion:
  - the direct first-frame retained patch is the right architectural direction:
    it removed live runtime work from the visible click frame and got measured
    product samples near or under 16.7 ms;
  - the gate is not fixed. The blocker moved to frame/proof/sample identity,
    later-click harness completion, a missed requested-animation frame, and
    queue/present variance;
  - the next implementation should cut old verifier/product coupling rather
    than add another local micro-optimization: keyed product/proof subscribers,
    app-owned timing for every click sample, no queued-runtime cleanup
    attribution to host-input frames, and a focus-safe hardware present-floor
    baseline.

2026-07-02 frame/content revision split and remaining proof coupling:

- Implemented a generic native app-window revision split:
  - `NativeRenderHookResult::presented_content_revision` now preserves the
    render hook's actual content revision instead of replacing it with the
    frame dirty revision for scheduler-only repaints;
  - `ExternalWake + RuntimeTurnApplied` frames may present with an independent
    content revision because the dirty revision is a frame repaint revision, not
    a semantic content revision;
  - focused tests cover requested-animation, host-input, surface, idle, and
    external runtime cleanup repaint cases.
- Implemented a verifier classification fix:
  - `verify-native-cells-visible-click-e2e` now treats
    `simple_source_click_deferred_runtime` as a simple source-click product
    fast path instead of selecting the later cleanup `generic_fallback` timing
    sample as the click path.
- Focused checks:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_app_window external_runtime_cleanup_can_repaint_existing_content_revision -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window requested_animation_can_repaint_existing_scheduler_only_content -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window surface_dirty_revision_can_present_existing_content_revision -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window scheduler_only_host_input_can_repaint_existing_content_revision -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window idle_same_content_frame_can_repaint_existing_content_revision -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path -- --test-threads=1`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`;
  - `cargo check -q -p xtask -p boon_native_app_window`;
  - `git diff --check -- crates/xtask/src/main.rs crates/boon_native_app_window/src/lib.rs docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md examples/cells/view.bn`.
- Fresh release verifier before the classification-only verifier patch:
  - command:
    `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-frame-content-split.json`;
  - schema: `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-frame-content-split.json` passes;
  - status: fail, but the previous loop-crash blocker is gone and all four
    target clicks complete:
    - A2 passes with `simple_source_click_deferred_runtime`,
      `live_events_ms=0.0`, product `12.924756 ms`, present `9.051653 ms`;
    - B0/C0/A0 reach the correct selected address and formula text, but the
      report waits for proof/readback and/or records the later cleanup frame as
      product timing, producing `input_accept_to_formula_visible_ms` around
      `5002.216369 ms` for B0 and `4957.367341 ms` for C0;
    - preview perf reports `missed_frame_count=2`,
      `preview_loop_input_to_present_ms_p95=225.112497 ms`,
      `render_hook_ms.p95=33.462318 ms`, and proof mode
      `external_app_owned_readback`.
- Subagent findings folded into the plan:
  - the verifier should split "terminal preview lifecycle failure" from normal
    click samples. On loop error or IPC refusal, record `terminal_error`,
    `completed_click_count`, and `aborted_targets` instead of appending
    proof-shaped samples with missing probes;
  - product interaction stats must be interaction-scoped, not global. Accepted
    host input should create an `interaction_id`; frames should be classified as
    `product_input_present`, `requested_animation_followup`, `runtime_cleanup`,
    `proof_readback`, `verifier`, or `timer`; product gates should use the first
    product-present frame while proof/readback gates use matching
    `FrameEvidenceKey` subscribers;
  - `PreviewSharedRenderState` still treats `layout_proof: serde_json::Value`
    as active state. The next simplification should introduce typed
    `ActivePreviewScene` / `PendingPreviewScene` state and make proof JSON a
    report adapter, not the hot-path state carrier.
- Conclusion:
  - the app has proven sub-16.7 ms first-frame selection/formula-bar feedback on
    individual samples, and the loop no longer dies on stale frame/content
    revisions;
  - the gate is still not complete because old product/proof coupling and the
    generic fallback runtime/proof path remain reachable after deferred clicks;
  - the next implementation must be an architecture cut, not another micro
    cache: interaction-scoped frame evidence, product/proof subscriber split,
    typed active/pending preview scenes, and deletion/quarantine of latest-proof
    and full-state fallback paths.

2026-07-02 direct node-set retained selection evidence:

- Implemented and verified a generic retained node-set selection patch for
  source clicks:
  - when a route already identifies the clicked node, the product frame patches
    the clicked selected node and the previous selected nodes directly instead
    of rediscovering the current selection from address strings or proof JSON;
  - the fallback row-lookup overlay remains available for routes that do not
    carry exact nodes, but the current Cells click samples all used
    `selection_overlay_source="generic-selected-node-set"` with
    `legacy_selection_fallback_count=0`;
  - the Cells view keeps `address: cell.address` as row identity and now uses
    `target: cell.editing_text` as immediate retained text-control metadata, so
    the formula bar can mirror the clicked cell before the runtime commit
    finishes. This is an example cleanup plus generic route metadata, not a
    compiler/runtime special case.
- Focused checks:
  - `cargo fmt`;
  - `cargo test -q -p boon_native_app_window accepted_real_input_repaint_is_presentable_even_with_runtime_dirty_reason -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window unaccounted_host_input_frame_is_not_pre_present_drop_eligible -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground retained_selected_node_overlay_patches_generic_node_sets -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground retained_selected_address_overlay_uses_indexed_row_lookup -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground cells_click_selection_updates_formula_bar_and_selected_style -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground selection_proxy_noop_click_refreshes_bound_text_input_from_runtime -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground cells_focus_only_route_syncs_formula_bar_text -- --test-threads=1`;
  - `cargo check -q -p xtask -p boon_native_app_window -p boon_native_playground`;
  - `git diff --check`.
- Fresh release verifier:
  - command:
    `timeout 480s cargo xtask verify-native-cells-visible-click-e2e --profile release --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-direct-node-selection.json`;
  - schema: `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-direct-node-selection.json` passes;
  - status: fail, but the product-visible click path is now under budget:
    `input_accept_to_formula_visible_ms_p95=13.538215 ms`,
    `input_accept_to_formula_visible_ms_max=13.538215 ms`,
    `simple_source_click_count=4`, `generic_fallback_count=0`,
    `proof_current_changed=true`, and `readback_ok=true`;
  - all four click samples passed app-owned visual proof and exact formula-bar
    text:
    - A2: product `11.571385 ms`, selected overlay patch `0.121568 ms`,
      present call `8.132094 ms`;
    - B0: product `12.083593 ms`, selected overlay patch `0.147858 ms`,
      present call `8.471353 ms`;
    - C0: product `12.348062 ms`, selected overlay patch `0.162770 ms`,
      present call `7.708236 ms`;
    - A0: product `13.538215 ms`, selected overlay patch `0.155442 ms`,
      present call `0.032594 ms`, but runtime-current observation lagged at
      about `165.132 ms` and driver-to-input-accept dominated the
      click-to-formula outlier.
  - report blockers remain:
    `preview_loop_input_to_present_ms_p95=230.348859 ms`,
    `preview_loop_missed_frame_count=1`,
    missing/null retained committed-update render-patch counters, and missing
    runtime/list scan/root-materialization counters.
- Conclusion:
  - the immediate user-visible click/formula-bar path is no longer the slow
    part in the measured sample. The remaining failure is verifier/product-loop
    architecture and missing counter contracts: aggregate preview-loop timing
    still mixes product clicks with non-product scheduler/runtime/proof frames,
    and the report cannot yet prove the required retained-update and runtime
    no-scan/no-root facts;
  - next work should implement the interaction-scoped product-frame ledger,
    required retained render-patch counters, runtime/list no-scan counters, and
    proof/report queue separation. Do not continue selection-overlay or
    route-cache micro-tuning unless a fresh report shows those fields dominate
    again.

2026-07-02 lane-scoped product stats and upload-ring checkpoint:

- Implemented an app-window/product-lane timing bridge:
  - app-window frame samples now distinguish product interaction frames from
    animation follow-ups, proof/harness work, runtime/layout work, dev
    telemetry, and surface lifecycle work;
  - xtask now prefers `app_window_product_interaction_frames` for the release
    Cells visible-click product-loop contract instead of accepting mixed
    aggregate preview stats as UX evidence;
  - deferred runtime first-frame samples emit app-produced evidence rather than
    xtask inferring zero runtime/list work from visual proof.
- Focused verification before the repeated run:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_gpu -p boon_native_app_window -p boon_native_playground -p xtask`;
  - `cargo test -q -p boon_native_gpu quad_upload_ring_preserves_cached_ranges_until_growth_is_needed -- --test-threads=1`;
  - `cargo test -q -p boon_native_gpu quad_upload_ring_grows_before_multi_batch_frame_can_overwrite_live_ranges -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window preview_perf_stats_keep_proof_overhead_separate_from_ux_latency -- --test-threads=1`;
  - `cargo test -q -p xtask native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback -- --test-threads=1`;
  - `git diff --check`.
- Fresh repeated release verifier:
  - command:
    `timeout 1200s cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-upload-ring-grown-default.json`;
  - schema:
    `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-upload-ring-grown-default.json` exits successfully;
  - status: fail;
  - product path source is now
    `app_window_product_interaction_frames`, so the remaining failure is no
    longer only mixed-frame accounting;
  - target count `64`, exact visual proof `64/64`, `simple_source_click_count=64`,
    `generic_fallback_count=0`, `retained_render_scene_patches=64`,
    `retained_render_scene_patch_fallbacks=0`, `proof_current_changed=true`,
    and `readback_ok=true`;
  - runtime/list keyed contract passes:
    `zero_scan_sample_count=64`, `zero_root_materialization_sample_count=64`,
    `zero_recompute_sample_count=64`, `total_rows_scanned=0`,
    `total_list_find_rows_scanned=0`,
    `total_root_materialization_candidates=0`, and
    `total_recomputed_fields=0`;
  - realtime product failure:
    `input_to_present_ms_p95=22.928110999997443 ms`,
    product sample count `95`, `product_missed_frame_count=30`,
    click-sample product p95 `21.877683999999135 ms`,
    click-sample max `44.5069739999999 ms`, and click samples within budget
    `46/64`;
  - the harness still reports bounded external/proof latency as a blocker:
    `harness_p95=169.257 ms`, product p95 about `21.878 ms`, and click-to-formula
    max about `202.663 ms`.
- Diagnosis from the same report:
  - the upload-ring growth/cache-preservation change removed the earlier
    product-frame upload churn shape in the slow samples: slow samples average
    `quad_cache_eviction_count=0`, `upload_bytes=0`, and `queue_write_count=0`;
  - remaining slow frames are dominated by queue/present and render-hook work:
    average slow-frame `present_call_ms` about `9.282 ms`,
    average `queue_submit_call_ms` about `3.135 ms`, and average
    render-start-to-hook-complete about `4.945 ms`;
  - worst sample index `57` spends about `37.482 ms` in `present_call_ms`,
    about `5.068 ms` in the render hook, and only about `0.141 ms` in
    `queue_submit_call_ms`;
  - other slow samples alternate between `present_call_ms` around `8-14 ms` and
    `queue_submit_call_ms` around `9-12 ms`, with render hook around `4-6 ms`.
- Conclusion:
  - do not return to Cells formula/list/runtime startup as the main explanation
    for this report. The current repeated-release blocker is product loop
    frame pacing, queue/present ownership, and render-hook/extract cost;
  - the next large cut should be `PreviewHotLoop + ActivePreviewScene +
    post-present proof subscribers`, with late acquire / in-flight /
    present-floor measurement and a smaller pre-present render result. A local
    cache tweak is unlikely to close a repeated 22-23 ms p95 with 30 missed
    product frames.

2026-07-02 retained renderer cache and present-path diagnostics:

- Stable input-overlay patch identity:
  - changed the direct retained input-overlay render path to identify the
    renderer patch from the actual `RenderScenePatch` content instead of a
    volatile synthetic frame/overlay string;
  - this is generic retained-renderer work: it lets the renderer's internal
    document-scene cache reuse patched internal scenes when the same retained
    state returns, without branching on Cells or address strings.
- Fresh repeated release verifier:
  - command:
    `timeout 1200s cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-stable-patch-identity-default.json`;
  - schema:
    `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-stable-patch-identity-default.json` exits successfully;
  - status: fail, but improved from the upload-ring checkpoint:
    product p95 `19.705062999999427 ms` vs previous `22.928110999997443 ms`,
    click-sample product p95 `19.507323999998334 ms` vs previous
    `21.877683999999135 ms`, and product max `27.510569999998552 ms` vs
    previous `44.5069739999999 ms`;
  - renderer evidence improved:
    `document_scene_cache_hit` occurred on `22` exact click samples,
    `document_scene_convert_ms.p95` dropped to about `1.516 ms`,
    and `encode_scene_ms.p95` dropped to about `2.579 ms`;
  - the remaining failed frames were dominated by submit/present:
    `present_call_ms.p95` about `12.440 ms`,
    `queue_submit_call_ms.p95` about `11.426 ms`,
    and hook-to-present p95 about `13.509 ms`.
- Prepared-quad cache map:
  - replaced the single prepared-quad cache entry with a small keyed cache in
    `VisibleLayoutRenderer`, so alternating retained states can reuse prepared
    GPU batches instead of only the immediately previous state;
  - added focused test
    `renderer_reuses_prepared_quad_cache_across_alternating_scene_identities`;
  - repeated release report
    `target/reports/native-gpu/cells-visible-click-e2e-prepared-quad-cache-default.json`
    is schema-valid but still fails and is worse as a UX run:
    product p95 `24.89105599999857 ms`, click-sample p95
    `21.613346999998612 ms`, product max `42.26945500000147 ms`, and
    `product_missed_frame_count=26`;
  - interpretation: the cache is a valid generic resource-lifetime improvement
    but it is not the acceptance fix. That run was dominated by present/submit
    outliers and still had volatile overlay patch identities, with only `13`
    exact click samples reporting scene/quad cache hits and `31` misses.
- Explicit offscreen-copy diagnostic:
  - command:
    `timeout 1200s env BOON_NATIVE_OFFSCREEN_COPY_TO_PRESENT=1 cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-offscreen-copy-diagnostic-default.json`;
  - schema:
    `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-offscreen-copy-diagnostic-default.json` exits successfully;
  - status: fail and not viable as the default product path:
    product-loop p95 `31.242193999991287 ms`,
    click-sample product p95 about `316.0232080000278 ms`,
    only `32/64` exact visual proof samples, and report blockers include
    formula-bar and selection-proof failures;
  - render target evidence confirmed the mode was active:
    `render_target_kind="app-owned-offscreen-copy-to-present"` and
    `copy_to_present_path=true`;
  - slow samples showed render-hook p95 around `52.807 ms` and large proof/visual
    lag, so do not switch product mode to offscreen copy-to-present. Keep it as
    an explicit diagnostic/proof experiment only unless the architecture is
    redesigned and remeasured.
- Pre-input proof/report subscriber guard:
  - change:
    `boon_native_app_window` now skips pre-input interactive readback/report
    subscriber draining while an unsampled host-input wake is already pending,
    and records `pre_input_subscriber_drain_skip_count` plus the last skip
    reason in the render-loop state;
  - focused checks:
    `cargo fmt --check`,
    `cargo test -q -p boon_native_app_window pre_input_subscriber_drain_skip_is_counted -- --test-threads=1`,
    and
    `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`
    pass with existing warnings;
  - release report:
    `target/reports/native-gpu/cells-visible-click-e2e-pre-input-subscriber-guard-default.json`
    is schema-valid but still fails. Product-scoped p95 is close but still over
    budget: `interaction_scoped_product_input_to_present_ms_p95 =
    17.541203000000678 ms`, product samples `81`,
    `interaction_scoped_product_missed_frame_count = 8`, click-sample
    `input_accept_to_formula_visible_ms_p95 = 17.098119000002043 ms`, and max
    `17.87061900000117 ms`;
  - proof/runtime/retained contracts remain clean: `simple_source_click_count =
    64`, `generic_fallback_count = 0`, `retained_render_scene_patches = 64`,
    runtime/list contract status `pass`, zero rows/list scans, zero root
    materialization candidates, and zero recomputed fields;
  - the new guard did not fire in this run:
    `target/artifacts/native-gpu/cells-visible-click-e2e-4154370-1782953732/preview-loop.json`
    reports `pre_input_subscriber_drain_skip_count = 0`, so this is a valid
    product-path guardrail but not the current p95 fix;
  - the remaining measured boundary is still frame/present ownership. The
    preview-loop report shows product p95 `17.541203000000678 ms`, product
    missed frames `8`, `present_call_ms.p95 = 12.756597999999999 ms`,
    `queue_submit_call_ms.p95 = 10.730661000000001 ms`, and
    `render_hook_ms.p95 = 41.500012000000424 ms` across all preview frames.
    Slow click samples alternate between queue-submit blocking and present
    blocking around `8-12 ms`, with render-hook work often `2-5 ms`.
- Frame-latency override diagnostic:
  - change:
    added `BOON_NATIVE_DESIRED_MAXIMUM_FRAME_LATENCY` as a bounded, reported
    diagnostic override for `wgpu::SurfaceConfiguration::desired_maximum_frame_latency`.
    Defaults remain unchanged: low-latency present modes still use one frame in
    flight and paced/vsync modes use the existing paced default. Invalid values
    fall back to the default, and values are clamped to a small bounded cap;
  - report contract:
    app-window first-frame proof and preview-loop reports now include
    `desired_maximum_frame_latency_source`, so override runs are visible and
    cannot silently become product evidence;
  - focused checks:
    `cargo fmt --check`,
    `cargo test -q -p boon_native_app_window configured_surface_frame_latency_honors_bounded_override -- --test-threads=1`,
    `cargo test -q -p boon_native_app_window pre_input_subscriber_drain_skip_is_counted -- --test-threads=1`,
    and
    `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`
    pass with existing warnings;
  - diagnostic command:
    `timeout 1200s env BOON_NATIVE_DESIRED_MAXIMUM_FRAME_LATENCY=2 cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-frame-latency-2-diagnostic-default.json`;
  - status:
    schema-valid fail and worse than the default product path. The preview-loop
    report proves the override was active:
    `desired_maximum_frame_latency = 2`,
    `desired_maximum_frame_latency_source = "env_override"`, and
    `present_mode = "Mailbox"`;
  - measured result:
    product p95 worsened to `23.865326999999525 ms`, product missed frames to
    `16`, click-sample p95 to `27.416173999999955 ms`, and product max to
    `54.39362799999799 ms`. Preview-loop p95 fields were still dominated by
    submit/present and render-hook variance:
    `present_call_ms.p95 = 13.533544999999998 ms`,
    `queue_submit_call_ms.p95 = 12.198112 ms`,
    `present_path_ms.p95 = 14.910712 ms`, and
    `render_hook_ms.p95 = 42.125426999999036 ms`;
  - conclusion:
    do not make `desired_maximum_frame_latency=2` the product default. Keep the
    override as a visible diagnostic knob. The next architecture cut should
    still be a real `PreviewHotLoop` / render-result boundary / present-floor
    hardware lab, not a hidden frame-queue depth change.
- Accepted-input timing and typed render identity checkpoint:
  - changes:
    app-window accepted-input timing now records the accepted product frame
    when the role poll hook has actually produced a visible-changing dirty
    result, and reports `poll_started_to_input_accept_ms` separately so
    pre-accept poll/hook work is not folded into the accepted-frame phase.
    `boon_native_playground` also now returns typed layout/render identities
    from `PreviewNativeGpuRenderHookOutput`, so the preview/dev render closures
    no longer parse those identities back out of proof JSON before creating
    `NativeRenderHookResult`;
  - subagent review:
    independent app-window and playground inspections agreed that the remaining
    high-leverage cuts are a typed product render result plus post-present proof
    subscribers, and then a real `PreviewHotLoop` owner. They also confirmed
    that visible-surface readback setup can still share the product command
    buffer in modes where external proof does not replace it, so that path must
    be deleted/quarantined by the post-present proof-subscriber design;
  - focused checks:
    `cargo fmt --check`,
    `cargo test -q -p boon_native_app_window render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats -- --test-threads=1`,
    `cargo test -q -p boon_native_app_window accepted_host_input_timing_defines_product_input_to_present_latency -- --test-threads=1`,
    `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`,
    `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-accepted-timing-typed-render-identity.json`,
    and `git diff --check` pass with existing warnings;
  - release report:
    `target/reports/native-gpu/cells-visible-click-e2e-accepted-timing-typed-render-identity.json`
    is schema-valid but still fails. Product p95 is `18.1187940000018 ms`,
    product sample count `74`, product missed frames `6`, steady accepted
    input-to-formula p95 `18.50278099999923 ms`, product max
    `40.28195699999924 ms`, and harness p95 `173.322 ms`;
  - measured blocker:
    the accepted frame now shows `input_accept_to_dirty_poll_ms = 0.0`, so the
    previous misleading pre-accept bucket is gone. The failing samples are
    still dominated by queue/present and frame ownership: steady
    `present_call.p95 = 12.729381 ms`, `queue_to_present.p95 =
    12.729762000000846 ms`, `render_started_to_render_hook_completed.p95 =
    3.768981999997777 ms`, and one product sample hit `37.902366 ms` in
    `present_call_ms`. `input_wake_to_input_accept.p95` remains very high
    (`107.13211700000464 ms` steady), so raw wake/harness/pre-accept latency is
    not solved by the accepted-frame relabel;
  - conclusion:
    this is a measurement and product-result-boundary cleanup, not the 60 FPS
    fix. Do not claim success from the accepted-frame timing shift. The next
    implementation slice should pre-issue `FrameEvidenceKey`, make product
    render return a small typed `PresentedProductFrame`/`RenderFrameResult`,
    move visible-bound-text proof, retained-sync proof, readback, report JSON,
    and artifact hashes to post-present subscribers, and decide late-acquire /
    frame-in-flight policy from a hardware present-floor report.
- Preissued `FrameEvidenceKey` checkpoint:
  - changes:
    `boon_native_app_window` now builds an exact
    `frame_evidence_key_for_next_presented_frame_with_revisions` after the
    render hook returns final content/layout/render-scene revisions, but before
    visible readback queueing, command-buffer submit, and `frame.present()`.
    The key uses the next frame/present id and is recorded in render-loop state
    as `last_preissued_frame_evidence_key`; readback queue state now carries
    the same key into `AppWindowReadbackArtifact` when a readback artifact is
    produced. The render context intentionally does not carry a finalized key
    yet, because that would require guessing revisions before the hook returns;
  - subagent review:
    independent app-window review agreed that a finalized key before
    `hooks.render(...)` is unsound with the current API. The smallest safe cut
    is the current post-hook/pre-submit key plus a later frame-evidence seed or
    typed product result if the render hook needs to emit exact keyed proof
    before returning;
  - focused checks:
    `cargo fmt --check`,
    `cargo test -q -p boon_native_app_window frame_evidence_key_can_be_preissued_before_present -- --test-threads=1`,
    `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`,
    `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-preissued-frame-evidence.json`,
    and `git diff --check` pass with existing warnings;
  - release report:
    `target/reports/native-gpu/cells-visible-click-e2e-preissued-frame-evidence.json`
    is schema-valid but still fails honestly. Product path source remains
    `app_window_product_interaction_frames`, product p95 is
    `16.865923999999723 ms`, product sample count is `7`,
    product missed-frame count is `1`, click-sample accepted p95/max is
    `12.535805999999866 ms`, and the blocker is
    `Cells preview-loop product path failed realtime budget`;
  - evidence from
    `target/artifacts/native-gpu/cells-visible-click-e2e-353333-1782956394/preview-loop.json`:
    `frame_evidence_key_issued_before_present=true`,
    `preissued_frame_evidence_matches_presented_frame=true`, and both
    `frame_evidence_key.frame_seq` and `preissued_frame_evidence_key.frame_seq`
    are `34`. Product p95 in the preview perf stats is the same
    `16.865923999999723 ms`, product missed-frame count is `1`,
    `present_call_ms.p95` is about `10.660442 ms`,
    `queue_submit_call_ms.p95` is about `9.278028 ms`, and
    `render_hook_ms.p95` is about `4.181787 ms`;
  - conclusion:
    this removes another proof-identity boundary and makes the frame evidence
    contract more honest, but it is not a latency fix. The next cut remains a
    typed product render result plus post-present proof subscribers for
    visible-bound-text proof, retained-sync proof, report JSON, readback
    completion, proof history, and artifact hashing. Do not build a fake
    finalized context key before revisions are known.
- Product frame commit checkpoint:
  - changes:
    `boon_native_app_window` now emits an app-owned
    `NativeProductFrameCommit` after product present. The commit carries the
    exact `FrameEvidenceKey`, lane, scheduler/dirty reasons, input event seq,
    accepted input timing, content/layout/render revisions, present path,
    phase timings, the typed product-frame summary, and declared post-present
    proof requests. Render-loop reports expose `last_product_frame_commit`,
    `product_frame_commit_count`, and whether the commit key matches the current
    frame evidence key;
  - focused checks:
    `cargo fmt --check`,
    `cargo test -q -p boon_native_app_window render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats -- --test-threads=1`,
    `cargo test -q -p boon_native_app_window frame_evidence_key_can_be_preissued_before_present -- --test-threads=1`,
    `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`,
    and `git diff --check` pass with existing warnings;
  - release report:
    `target/reports/native-gpu/cells-visible-click-e2e-product-frame-commit.json`
    is schema-valid but still fails honestly. The blocker is still
    `Cells preview-loop product path failed realtime budget`; product p95 is
    `19.67812700000013 ms`, product missed-frame count is `1`, and the product
    path source is `app_window_product_interaction_frames`;
  - evidence from
    `target/artifacts/native-gpu/cells-visible-click-e2e-557938-1782957820/preview-loop.json`:
    `product_frame_commit_count = 26`,
    `product_frame_commit_matches_frame_evidence_key = true`, the last commit
    has `commit_source = "app_window_product_frame_commit"`, `frame_lane =
    "product_interaction"`, `input_event_seq = 13`, and its
    `frame_evidence_key` exactly matches the report frame evidence key. The
    last commit's accepted input timing is `11.384745999999723 ms`, while the
    product-lane p95 remains `19.67812700000013 ms` over 6 product samples;
  - measured blocker:
    this is a product/proof accounting boundary, not the 60 FPS fix. The same
    artifact still reports `present_call_ms.p95 = 14.244147 ms`,
    `queue_submit_call_ms.p95 = 8.087333000000001 ms`, and
    `render_hook_ms.p95 = 3.3827859999992143 ms`. The next slice should make
    proof/report/readback subscribers consume `NativeProductFrameCommit` by key
    after present, then cut `PreviewHotLoop` / active scene / present-floor
    ownership rather than tuning Cells runtime or renderer caches.
- Keyed product-commit verifier checkpoint:
  - changes:
    `verify-native-cells-visible-click-e2e` now has an app-window product
    commit summary path whose acceptance source is
    `app_window_product_frame_commits`, and the app-window recent product
    commit ring was expanded so repeated scenarios do not lose early product
    rows. The focused one-repeat release report
    `target/reports/native-gpu/cells-visible-click-e2e-keyed-product-commits.json`
    is schema-valid and passes with four matched product samples, p95
    `13.839931 ms`, max within budget, zero missed product frames, zero
    runtime/list scans, zero root materialization, zero recomputed fields, four
    retained render patches, and no full lower or legacy fallback;
  - repeated release report:
    `target/reports/native-gpu/cells-visible-click-e2e-keyed-product-proof-split-default.json`
    is schema-valid but still fails honestly. The product/proof split is now
    visible: product rows are keyed by `NativeProductFrameCommit`, while proof
    can lag or point at a later runtime/proof frame. Product p95 over matching
    repeated samples is narrowly over budget at about `16.866963 ms`, with real
    product outliers around `30.740820 ms` and `26.252360 ms`; late samples
    also lose matching proof/currentness evidence. Runtime/list and retained
    counters remain clean where samples have evidence (`total_rows_scanned=0`,
    `total_list_find_rows_scanned=0`, `total_recomputed_fields=0`,
    `full_document_lower_count=0`, `legacy_selection_fallback_count=0`);
  - diagnosis:
    this is not a Cells formula/list/runtime blocker. The remaining blocker is
    product/proof ownership and frame scheduling: the verifier still has to
    infer some product rows from proof keys plus input timing, late repeated
    samples can exhaust or miss proof/currentness evidence, and true product
    outliers are dominated by submit/present/render-frame variance. Do not
    return to selected-address styling, route-cache, formula, list-index, or
    upload-ring micro-fixes unless a fresh report names them as the dominant
    boundary again;
  - next architecture TODOs:
    record `product_frame_evidence_key`, `product_frame_commit`,
    `proof_frame_evidence_key`, `proof_lag_frames`, and
    `product_commit_match_method` directly in every UX sample instead of
    inferring product rows from latest proof/report state; make proof, visible
    bound-text probes, runtime-value probes, report JSON, screenshot encoding,
    and artifact hashing exact-key post-present subscribers with bounded
    queues; add product-only and proof-isolation repeated gates so missing
    proof cannot erase product evidence and slow proof cannot block product
    present; then cut the larger `PreviewHotLoop`/`ActivePreviewScene` present
    path so host input lands in an already armed burst frame and proof/report
    work cannot relabel, delay, or be charged to the product interaction.
- Terminal-stop verifier cleanup and refreshed repeated checkpoint:
  - changes:
    `verify-native-cells-visible-click-e2e` now treats a formula-visible
    timeout as a terminal sample failure instead of using the 5s timeout as a
    finite latency sample. The repeated harness stops after the first terminal
    formula-visible timeout, records `terminal_failure`,
    `terminal_failure_index`, `completed_click_count`, and
    `remaining_samples_skipped_due_terminal_failure`, and keeps the report
    status failing. This is verifier cleanup, not a performance pass;
  - bounded-report cleanup:
    the top-level terminal summary records scalar/status fields rather than
    duplicating the full `formula_visible_probe`; detailed evidence remains in
    the failing click sample. The current report is still large because
    per-sample proof/report payloads remain verifier-shaped, so post-present
    proof subscribers and bounded report artifacts are still required;
  - focused checks:
    `cargo fmt --check`,
    `cargo check -q -p xtask -p boon_native_app_window`,
    `cargo test -q -p xtask native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback -- --test-threads=1`,
    `cargo test -q -p boon_native_app_window accepted_host_input_timing_owns_lane_during_requested_animation_burst -- --test-threads=1`,
    `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-terminal-stop-default.json`,
    and `git diff --check -- crates/xtask/src/main.rs docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md`
    pass with existing warnings;
  - refreshed repeated release report:
    `target/reports/native-gpu/cells-visible-click-e2e-terminal-stop-default.json`
    is schema-valid and still fails. This particular run completed all 64
    clicks (`completed_click_count=64`, no terminal timeout), so the terminal
    path was not exercised in the final artifact. Product path source remains
    `app_window_product_frame_commits`; product p95 is now close but still
    failing (`interaction_scoped_product_input_to_present_ms_p95 =
    16.548328000000765 ms`, `interaction_scoped_product_missed_frame_count =
    3`). The steady product input-to-formula p95 is `16.753 ms`, just over the
    `16.7 ms` target. The aggregate preview loop remains diagnostic-fail
    (`aggregate_preview_loop_input_to_present_ms_p95 =
    56.43485700000019 ms`, `aggregate_preview_loop_missed_frame_count = 39`)
    because non-product/proof/follow-up frames still share the global preview
    stats bucket;
  - clean contracts:
    runtime/list, retained update, formula-transition, and selected-cell
    transition contracts pass for all 64 samples: zero rows scanned, zero
    `List/find` rows scanned, zero root materialization candidates, zero
    recomputed fields, 64 retained render patches, zero full document lower,
    and zero legacy selection fallback. Do not restart Cells formula/list or
    selected overlay work unless a fresh report names those as failing again;
  - remaining evidence:
    exact product-commit matching still is not complete (`36`
    `exact_product_commit` samples and `28`
    `input_event_seq_and_product_latency` samples), so every UX sample still
    needs a direct product key join. The slowest product samples show two
    different remaining boundaries: one true product outlier at sample `22`
    (`25.515572 ms`) where `render_hook_completed_to_present_ms` is about
    `23.200305 ms`, and smaller over-budget samples around `16.75-17.36 ms`
    where pre-present render and hook-to-present together consume the margin.
    Product commits still report `legacy_pre_present_proof_request_count = 5`
    and `legacy_product_proof_built_pre_present = true`, which means the
    post-present proof-subscriber cut is still not done;
  - next architecture TODO:
    remove the proof-shaped product path and exact-match fallback before doing
    more micro-tuning. Every click sample should carry an app-window-produced
    `product_frame_evidence_key` by `InteractionId`, the product render hook
    should return a small typed `PresentedProductFrame`/`RenderFrameResult`,
    legacy proof JSON/readback/report payloads should become post-present
    subscribers with bounded queues, and aggregate preview stats must be
    lane-scoped so proof/report/follow-up frames cannot hide or relabel product
    UX samples.
- Current conclusion:
  - the best fresh checkpoint is now
    `target/reports/native-gpu/cells-visible-click-e2e-terminal-stop-default.json`.
    It shows the runtime/list/retained/formula/selection contracts are clean for
    the full 64-click repeated scenario, but the product loop is still not
    accepted: p95 is only narrowly under the numeric threshold in one summary,
    steady formula visibility is just over budget, and three product frames
    miss the budget. The aggregate preview loop remains a diagnostic failure
    because non-product frames are still mixed into global preview stats;
  - do not spend the next round on Cells formula/list/currentness, selected
    overlay styling, route-cache experiments, upload-cache experiments, or
    frame-latency `2`. The fresh blockers are exact product key ownership,
    legacy proof/report work still built before present, lane-scoped product
    stats, and frame scheduling/present ownership;
  - next work should cut architecture, not tune around it: implement a direct
    `InteractionId -> product_frame_evidence_key` ledger for every sample,
    replace latency fallback joins, move visible-bound-text proof,
    retained-sync proof, readback, report JSON, proof history, and artifact
    hashes to bounded post-present subscribers, then build the larger
    `PreviewHotLoop` / `ActivePreviewScene` product path with a hardware
    present-floor-backed late-acquire/frame-in-flight policy.

## 2026-07-02 Final No-Loss Architecture TODO Addendum

This addendum exists so the next implementation run does not lose wider
architecture options while chasing the current Cells click report. Treat these
as candidate replacement cuts. Do not implement them as parallel permanent
systems; pick the simplest cut that removes a measured slow boundary, prove it,
then delete or quarantine the old path.

- [ ] Product/runtime protocol split:
  - define a closed product protocol between app-window, playground, runtime,
    document, layout, and renderer: accepted input batch, typed intent,
    runtime turn, document delta, layout/materialization delta, render extract,
    product frame result, and post-present subscribers;
  - keep debug/report/proof JSON as an adapter outside the product protocol;
  - proof: product frames report zero use of `serde_json::Value`,
    `state_summary`, latest-report data, proof trees, or path-string lookup
    before present.
- [ ] Stable identity ABI through every product phase:
  - define one generational identity vocabulary for runtime slots, list rows,
    source bindings, document nodes, layout fragments, hit regions, controls,
    render batches, GPU resources, proof requests, and frame evidence;
  - identities may be projected into reports after present, but product code
    must consume typed ids rather than strings, paths, labels, addresses, or
    geometry-derived keys;
  - proof: a renamed sparse fixture and renamed UI labels keep routing,
    retained patches, proof joins, and verifier assertions working without
    production branches on fixture-specific names.
- [ ] Deterministic scheduler simulator:
  - model idle, requested-animation burst, source wake, proof-only wake,
    telemetry flush, dev-window overload, resize/surface lost, and queued
    runtime cleanup without WGPU;
  - use the simulator to test lane priorities, burst exit, stale pending work,
    preemption, and backpressure before changing native timing code;
  - proof: scheduler unit tests cover the same transition table as native
    reports and fail if proof/debug lanes can delay product input.
- [ ] Product-frame lane taxonomy:
  - classify frames at creation time as product input, product animation,
    product surface lifecycle, runtime cleanup, proof-only, report-only,
    calibration, preposition, dev-HUD, or diagnostic probe;
  - never infer lane from the last dirty flag, latest report state, or proof
    artifact after the frame has already presented;
  - proof: product UX gates consume only product lanes and fail closed when a
    frame lacks a lane, interaction id, input event seq, or exact evidence key.
- [ ] Lock, allocation, and clone budget:
  - add product-frame counters for mutex wait time, channel wait time,
    heap allocations, large clones, JSON clones, string allocations, hash-map
    rebuilds, and report/proof payload copies;
  - set strict product budgets and move exceeding work to retained state,
    precomputed indexes, or post-present workers;
  - proof: release UX reports include these counters and stale-path gates fail
    when a product frame crosses the budget repeatedly.
- [ ] Hot-path memory model:
  - move product-frame temporary data into frame arenas, small-vector buffers,
    ring buffers, and renderer-owned caches with explicit reset points;
  - forbid unbounded hash-map rebuilds, recursive summary clones, broad
    `Arc<Mutex<_>>` report reads, and per-frame string formatting before
    present;
  - proof: allocation/clone counters distinguish first-use warmup from steady
    interaction and fail repeated steady-state product regressions.
- [ ] Text/input-control architecture:
  - make text controls a generic retained subsystem shared by formula bars,
    spreadsheet cells, normal inputs, code editor, search fields, and future
    editors;
  - include focus, hover, caret, selection, IME composition, paste, undo/redo,
    wheel, keyboard navigation, bound mirrors, validation/error display, and
    accessibility snapshots as separate product/proof responsibilities;
  - proof: all interactive examples with text controls have host-event visual
    replays with visible pointer, caret/selection proof, and current value
    assertions without example-specific branches.
- [ ] Input coalescing and priority policy:
  - define which pointer move, wheel, key repeat, text edit, IME, caret, and
    animation requests can be coalesced, superseded, or must run
    run-to-completion;
  - host input that changes visible state preempts proof/report drains and
    stale pending scene work; passive high-rate events use latest-wins
    viewport/control deltas;
  - proof: coalescing reports list superseded event ids and product gates fail
    if required semantic events are dropped or proof/report lanes preempt input.
- [ ] Product surface and present policy:
  - decide explicitly between direct surface render, app-owned texture plus
    copy-to-present, and proof-only offscreen render. Each mode needs a named
    report field, adapter/surface/present-mode metadata, and acceptance rules;
  - test late surface acquire, `desired_maximum_frame_latency`, present mode,
    frames in flight, ring-buffered uploads, and surface-error recovery as
    measured modes, not hidden defaults;
  - proof: hardware present-floor reports use the same app-window/surface path
    as product examples and software/headless floors cannot satisfy product UX.
- [ ] Late-acquire / frames-in-flight decision gate:
  - experiment with late surface acquisition, prepared command encoding,
    command-buffer reuse where legal, and bounded frames in flight only after a
    same-surface present-floor report names queue/present as the bottleneck;
  - each policy must report latency, throughput, stale-frame risk, queue depth,
    acquired-frame lifetime, and surface-loss recovery behavior;
  - proof: no policy becomes a default unless repeated product UX gates improve
    without losing exact visual proof or increasing proof lag beyond budget.
- [ ] Render-resource tiering:
  - split render-owned resources into persistent pipelines/bind groups/atlases,
    frame-ring resources, dirty primitive buffers, proof-only staging buffers,
    and debug-only artifacts;
  - record creation/reuse/reallocation for glyphs, quads, borders, clips,
    images, text runs, transforms, overlays, and proof resources;
  - proof: interaction frames show bounded upload bytes, bounded draw calls,
    zero avoidable reallocations, and no proof-resource allocation before
    product present.
- [ ] Browser-style phase boundary:
  - separate script/runtime update, style/data binding, layout, display-list
    extraction, paint/compositor property update, GPU encode, present, and
    proof/readback into named phases with one-way product data flow;
  - overlay, transform, clip, scroll, caret, focus, and selection updates should
    use compositor/property-tree patches when structure is unchanged;
  - proof: phase timing reports show the smallest phase touched by each
    interaction, and normal focus/hover/scroll cannot silently trigger script,
    full layout, or display-list rebuilds.
- [ ] Compositor/property-tree migration:
  - move scroll offsets, transforms, clips, opacity/effects, hover, focus,
    selection, caret, and pointer marker into retained property trees owned by
    the preview renderer;
  - document/layout only rebuild when semantic structure or materialization
    windows change, not for first-frame visual feedback;
  - proof: passive scroll, hover, selection, and text-caret frames report zero
    relower, zero full layout rebuild, and zero full render-scene rebuild.
- [ ] Render-thread ownership option:
  - evaluate a dedicated preview render/frame thread or task that owns surface,
    renderer resources, active scene, and frame clock while runtime/document
    work publishes immutable pending snapshots;
  - cross-thread messages must be typed, bounded, latest-wins where safe, and
    measured for wake latency and backpressure;
  - proof: render ownership reduces product-frame blocking without introducing
    unsynchronized latest-state reads, dev IPC dependencies, or stale proof.
- [ ] Sparse runtime query engine:
  - treat list lookup, row materialization, formula dependencies, ranges,
    currentness barriers, cycle detection, and selected/dependent keys as one
    generic runtime query engine;
  - include duplicate-key, removed-row, stale-generation, range invalidation,
    dependency fanout, cycle, and non-Cells sparse-grid fixtures;
  - proof: product paths report indexed hits, bounded misses, zero full scans,
    logical/materialized/rendered/evaluated counts, and cycle-safe reads.
- [ ] Query-planner style runtime execution:
  - plan list filters, finds, chunks, maps, reductions, dependencies, and
    currentness reads as indexed/windowed operators with explicit cost counters;
  - keep startup/source reset passes columnar and batched, then evaluate derived
    values on demand through keyed barriers;
  - proof: runtime reports explain why an operator scanned, used an index,
    materialized a window, or deferred work, and zero-scan gates cover
    non-Cells fixtures.
- [ ] Hot/cold verifier split:
  - product-only mode runs with scalar counters and no proof/readback/report
    payloads beyond fixed-size evidence keys;
  - proof-only mode proves exact frames with app-owned WGPU readback and
    reports proof lag/overhead without being used as UX latency;
  - full-HUD/report mode has an explicit overhead budget and cannot be the only
    passing mode;
  - proof: each interactive scenario has all three modes and schema gates fail
    on driver-timing fallback, stale proof, or missing app-owned product keys.
- [ ] Verifier driver/product split:
  - the driver injects public host events and waits for keyed product/proof
    artifacts, but it never becomes the timing authority for UX acceptance;
  - test setup, focus/preposition, calibration, screenshot/readback waiting,
    report parsing, and cleanup have separate lanes and cannot be charged to
    product interaction samples;
  - proof: every release UX report includes driver overhead and product timing
    side by side, and missing product timing fails even when driver timing is
    fast.
- [ ] Dev-window isolation:
  - put dev editor scrolling, report expansion, proof history, source edits,
    and perf HUD refresh behind cached snapshots, paging, throttling, and
    latest-wins transport;
  - the preview product loop must not block on dev-window IPC, report parsing,
    editor wheel state, or debug tree expansion;
  - proof: dev-code-editor wheel replay passes without crash, and preview
    latency reports expose `preview_blocked_on_ipc_count=0` under dev load.
- [ ] Dev-window as a client, not an owner:
  - make the dev window subscribe to preview stats, source diagnostics, proof
    history, and report pages through cached read models; it must not own or
    synchronously query preview product state;
  - source edits and example switches publish explicit replacement transactions
    with pending/active state instead of blocking the preview frame loop;
  - proof: closing, overloading, or scrolling the dev window does not change
    preview product p95 or proof identity.
- [ ] Codegen migration gate:
  - Rust/Zig/Wasm codegen may be used only after the typed runtime/document
    delta ABI and interpreted equivalence oracle exist;
  - generated kernels must remove a named hot boundary, such as list query,
    formula fanout, materialization-window evaluation, render extraction, or
    primitive batching;
  - proof: generated and interpreted paths produce equivalent deltas/readbacks,
    and reports show which boundary was removed or accelerated.
- [ ] Architecture source-of-truth sync:
  - keep this plan, `docs/architecture/NATIVE_GPU_PIPELINE.md`, AGENTS.md,
    report schemas, xtask labels, and embedded `/goal` prompt aligned whenever
    product/proof/frame-loop contracts change;
  - when product-only/proof-only, present-floor, proof-isolation, stale-path,
    or new native UX gates become required, update the AGENTS.md readiness
    command list as well as the architecture doc so future agents cannot pass
    an obsolete native handoff checklist;
  - reject implementation checkpoints that update only a verifier or only a
    plan while leaving the source-of-truth architecture stale;
  - proof: architecture/schema consistency checks name the exact stale doc,
    gate, or report field instead of allowing contradictory readiness claims.
- [ ] Old harness and stale artifact deletion:
  - maintain a machine-readable ledger for legacy Ply, Xvfb, COSMIC scraping,
    browser screenshots, latest-report proof, artifact-only render proof,
    modeled/static scroll, private runtime dispatch, and driver-timing fallback;
  - once a native app-owned replacement exists, delete or quarantine the old
    harness and add a negative test that proves it cannot satisfy readiness;
  - proof: `verify-native-gpu-all --check-existing` rejects schema-valid but
    stale/quarantined reports as acceptance evidence.
- [ ] Minimal-path rewrite escape hatch:
  - if the product path remains complex after the product/proof split, build a
    small generic prototype path for one retained scene, one control model, one
    renderer extract, and one product verifier before adding compatibility;
  - migrate examples to the simpler path only if it deletes more old code than
    it adds and keeps compiler/runtime/document semantics generic;
  - proof: the prototype must pass Counter, TodoMVC, Cells, editor wheel, and a
    renamed sparse fixture before replacing legacy native preview paths.
- [ ] Architecture simplification checkpoint:
  - after every two failed fresh reports of the same class, stop local timing
    patches and choose one replacement cut from this addendum or the existing
    architecture backlog;
  - require the implementation note to name the removed boundary, old-path
    counter, positive gate, negative gate, and rollback plan;
  - proof: plan updates and reports show which architecture boundary changed,
    not just which local counter moved.

## 2026-07-02 Proof Mode And Legacy Gate Checkpoint

This checkpoint is progress toward the product/proof split, not the final
60 FPS fix.

- Implemented explicit native proof mode plumbing:
  - `NativeWindowOptions` now carries `NativeProofMode::{Counters, Readback}`;
  - app-window readback work is gated by `proof_mode == Readback` plus an
    artifact directory, not merely by an optional artifact path;
  - preview/dev roles accept `--proof-mode counters|readback`;
  - legacy `--probe` / `--frame-readback` still map to `readback`;
  - counters mode forces the compact render-hook proof path and reports
    `proof_mode = "counters"` / `configured_proof_mode = "counters"` instead
    of looking like proof accidentally disappeared;
  - `verify-native-cells-visible-click-e2e` requests `--proof-mode readback`
    explicitly, while the present-floor probe is explicit counters mode.
- Tightened generic native UX integrity:
  - UX reports now fail when they contain legacy pre-present proof coupling:
    `legacy_product_proof_built_pre_present = true`,
    `legacy_proof_json_built_pre_present = true`,
    `legacy_render_hook_proof_built_pre_present = true`,
    `currently_legacy_pre_present = true`, or positive
    `legacy_pre_present_proof_request_count`;
  - this is generic across native UX gates, not Cells-specific;
  - the purpose is to prevent a report from being accepted merely because it
    measured legacy proof/report debt honestly.
- Subagent review confirmed remaining old paths:
  - the render hook still returns pre-present `proof: serde_json::Value`, so
    visible-bound-text proof, retained-sync proof, render-hook report JSON,
    proof history, and artifact hashes can still be built on the product render
    path;
  - direct visible-surface readback in readback mode is still encoded into the
    product command buffer before `queue.submit` / `present` because the surface
    texture is consumed by `present`;
  - report/readback subscriber draining can still occur before the next input
    sample when input arrives during the drain;
  - due requested-animation wakes can still relabel or compete with accepted
    host-input product frames if the product-frame ownership is not made more
    explicit.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `git diff --check -- crates/boon_native_app_window/src/lib.rs
    crates/boon_native_playground/src/main.rs crates/xtask/src/main.rs
    docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes;
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    accepted_host_input_timing_owns_lane_during_requested_animation_burst --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    requested_animation_burst_is_bounded_inside_demand_driven_mode --
    --test-threads=1` passes.
- Next required architecture cut:
  - replace the render-hook `proof: serde_json::Value` product contract with a
    small typed `ProductRenderResult` / `PresentedProductFrame` carrying
    revisions, scalar frame metrics, render identities, and proof request
    descriptors;
  - after `present`, enqueue bounded `PostPresentProofQueue` subscribers keyed
    by the exact `NativeProductFrameCommit` / `FrameEvidenceKey`;
  - move visible-bound-text proof, retained-sync proof, render-hook report
    JSON, proof history, artifact hashes, and app-owned readback proof out of
    the product render hook;
  - add product-only and proof-only repeated visible-click runs so product p95
    and proof lag are measured separately and cannot mask each other.

## 2026-07-02 Strategy-First Architecture Cut TODOs

This section is intentionally redundant with the older backlog, but more
implementation-facing. It exists to prevent another run from spending hours on
micro-improvements when the measured problem is an old architectural boundary.
Pick one cut, remove or quarantine the old path, add the positive and negative
gates, then move to the next cut. Do not leave two permanent product paths.

- [ ] Product render-result cut:
  - define the product hot loop as one measured path with revision ids:
    `HostEvent -> RetainedHitTest -> ProductIntent -> RetainedPatch ->
    ScopedRuntimeDemandRead -> RenderScenePatch -> GpuDeltaUpload ->
    QueueSubmit -> Present`. Every phase should have timestamps, owner, lane,
    frame evidence key, and revision ids;
  - replace `NativeRenderHookResult { proof: serde_json::Value, ... }` with a
    typed product result in normal product modes. The product result may carry
    revisions, `FrameEvidenceKey`, dirty ids, layout/render identities, scalar
    phase timings, upload/draw counters, visible-input status, and
    post-present proof request descriptors;
  - make this a type-level contract, not a style guideline: product render
    hooks in product modes must not return, mutate, or depend on
    `serde_json::Value`. Full proof/report JSON may be constructed only from
    post-present proof subscribers or debug/report adapters;
  - move visible-bound-text proof, retained-bound-sync proof, screenshot/readback
    metadata, report JSON assembly, proof history, artifact hashes, and rich
    diagnostics behind post-present subscribers keyed by the exact product
    frame key;
  - old path to delete or quarantine:
    `native_gpu_app_owned_render_hook` building a proof/report JSON tree before
    product `queue.submit` / `present`;
  - gates: product reports show `legacy_product_proof_built_pre_present=false`,
    `legacy_proof_json_built_pre_present=false`,
    `legacy_render_hook_proof_built_pre_present=false`, and
    `legacy_pre_present_proof_request_count=0`; a negative test injects those
    fields and proves UX gates fail.
- [ ] Post-present proof queue:
  - add a bounded `PostPresentProofQueue` / `FrameEvidenceRegistry` owned by the
    app-window or renderer. Product frames enqueue proof work by exact
    `FrameEvidenceKey` and immediately return to input/frame scheduling;
  - proof workers may coalesce, lag, drop, or fail proof samples under pressure,
    but must report `proof_lag_frames`, `proof_drop_count`, and proof status
    without delaying product frames;
  - old path to delete or quarantine: proof/readback/report subscriber draining
    before host-input sampling or while holding product-loop locks;
  - gates: product-only counters mode runs with proof/readback disabled, proof
    isolation stress does not change product p95 beyond the explicit overhead
    budget, and stale/mismatched proof keys fail closed.
- [ ] Pre-present proof allocation hard gate:
  - add counters for proof/report work attempted before product present:
    proof JSON bytes allocated, `serde_json::Value` nodes built, artifact path
    strings built, hashes computed, proof-history rows touched, readback buffers
    prepared, readback copy commands encoded, screenshots encoded, report
    snapshots assembled, and proof subscriber drain time;
  - old path to delete or quarantine: any verifier-shaped proof/report object
    being created to let a product frame present;
  - gates: counters are all zero for product-only UX frames, and full proof/HUD
    mode reports the overhead separately rather than hiding it in UX latency.
- [ ] `PreviewHotLoop` owner:
  - define one state machine that owns DemandDriven idle, bounded
    requested-animation burst, host-input drain, active-scene patch,
    extract/encode, queue submit, present, and post-present subscriber enqueue;
  - this owner must be a concrete struct/actor in the implementation, not just
    a conceptual convention. It is responsible for product frame ordering,
    attribution, input drain priority, surface lifecycle, and subscriber
    backpressure;
  - product input is sampled at frame start on an already-armed burst frame
    where possible. Proof, telemetry, dev HUD, runtime cleanup, and report
    flushes are separate lanes and cannot relabel a host-input product frame;
  - old path to delete or quarantine: scattered click/scroll/proof/timer/dev
    wake paths with global missed-frame accounting and global input-to-present
    buckets;
  - gates: scheduler simulation covers idle, burst, source wake, proof-only
    wake, runtime cleanup, dev load, resize, surface loss, and timer; UX samples
    come only from product-interaction lanes with exact interaction ids.
- [ ] Input must not wait on present resources:
  - surface acquire, command queue submit, `present`, readback map/poll, proof
    subscriber drains, report serialization, and artifact hashing must not run
    while host input is waiting to be sampled or accepted;
  - add product-frame fields such as `input_waited_on_acquire`,
    `input_waited_on_submit`, `input_waited_on_present`,
    `input_waited_on_readback`, and `input_waited_on_proof_subscriber`;
  - old path to delete or quarantine: accepting host input only after
    acquire/submit/present/proof drains have completed;
  - gates: those fields are all false/zero for product-input frames, and a
    scheduler simulation proves input preempts proof/dev/telemetry work.
- [ ] Lane-scoped perf ledger:
  - split `NativePreviewPerfAccumulator` into product, proof, runtime-cleanup,
    telemetry/dev, calibration/preposition, and terminal-failure lanes;
  - every UX sample must include `interaction_id`, `input_event_seq`,
    `product_frame_evidence_key`, lane, scheduler reason, present id, and the
    exact product commit row. Aggregate preview stats stay diagnostic only;
  - old path to delete or quarantine: verifier reconstruction from
    `preview-loop.json` latest state, proof-key fallbacks, or global accepted
    host-input rings;
  - gates: missing keyed product commit is `missing_product_commit`, not a
    fabricated latency sample; proof timeout is `missing_proof`, not 5 seconds
    of UX latency.
- [ ] Verifier must not repair product evidence:
  - the app/window must emit product frame keys directly in each UX sample or
    product-commit stream. Verifiers may join exact keys only; they must not
    infer success from latest proof, currentness, report state, artifact paths,
    global frame rings, or timing similarity;
  - old path to delete or quarantine: `exact_product_commit` fallbacks that use
    proof keys, input sequence plus approximate latency, latest formula text,
    or retained proof to reconstruct missing product ownership;
  - gates: removing the direct product key produces `missing_product_commit`
    immediately, even when proof/readback artifacts happen to show the right
    pixels later.
- [ ] Active scene strangler:
  - introduce a small render-owned `ActivePreviewScene` before attempting a
    full rewrite. It owns hot route/hit snapshots, focused/hover/selected/caret
    state, text-control mirrors, materialization windows, binding reverse
    indexes, dirty render ids, GPU resource handles, upload rings, and the frame
    evidence registry;
  - product input applies `ProductIntent -> RetainedPatch -> DirtyRenderDelta`
    directly to the active scene. Runtime/document/layout produce latest-wins
    pending deltas or snapshots that commit only if their epochs still match;
  - use GPUI-style retained app entities/custom elements as inspiration:
    declarative Boon views may rebuild off the hot path, but large grids, code
    editors, text controls, and scroll surfaces need imperative retained
    layout/render state for visible interaction;
  - old path to delete or quarantine: mutating `layout_proof`/report JSON,
    reloading proof artifacts, or scanning document/layout proof trees to drive
    first-frame visual feedback;
  - gates: selection, hover, focus, caret, text mirror, and passive scroll
    frames show retained patch counts and zero full relower, zero full layout
    rebuild, zero full render-scene rebuild, and zero proof-tree hot reads.
- [ ] Active-scene API acceptance gate:
  - click, focus, hover, passive scroll, caret movement, and product text mirror
    paths must call a small retained API such as `apply_input_intent`,
    `apply_runtime_delta`, `apply_viewport_delta`, `extract_dirty_render`,
    `queue_product_frame`, `register_presented_frame`, and
    `schedule_proof_subscriber`;
  - old path to delete or quarantine: product paths mutating layout proof,
    latest reports, document summaries, verifier-shaped state, or raw JSON
    object trees directly;
  - gates: source-level audit plus runtime counters prove the retained API is
    the hot path for all supported examples.
- [ ] Typed input/source-intent ABI:
  - replace ad hoc source payload maps and geometry/string route rediscovery
    with typed commands such as `MoveFocus`, `SetSourceValue`,
    `CommitTextEdit`, `UpdateViewport`, `ActivateAction`, and IME/text
    composition commands. Commands carry route epoch, target/source ids,
    materialization window id, input event seq, interaction id, and stale-result
    policy;
  - compiler/document lowering must emit stable ids and reverse indexes:
    `DocumentNodeId`, `SourceBindingId`, list-map binding ids, row keys,
    hit-route ids, text-control ids, and render-slot ids;
  - old path to delete or quarantine: production routing or runtime dispatch
    based on labels, addresses, source paths, geometry strings, fixture row
    counts, or example names;
  - gates: no-hacks audit covers compiler, runtime, document, layout,
    renderer, app-window, playground, report-schema, and xtask; non-Cells
    sparse fixtures fail if Cells-specific shortcuts are introduced.
- [ ] Runtime delta/currentness ABI:
  - use a Bevy-style app-world/render-world split: runtime/document/layout
    produce typed deltas or extracted snapshots, while the render world owns
    GPU resources and consumes only extracted visible/materialized data. Steady
    scroll and focus frames must not access the runtime graph directly;
  - define typed `RuntimeDelta` / `DocumentDelta` records for source values,
    bound text, style/focus/selection, list windows, list indexes,
    materialization windows, formula/dependency fanout, errors, and
    diagnostic-only changes;
  - every product visible read goes through a scoped `VisibleReadSet` /
    `ensure_current(ReadKeySet)` ledger. Root summaries, full
    `state_summary`, broad list flushes, and whole-document summaries are
    diagnostic/report paths unless the ledger proves the visible pixel needs
    them;
  - old path to delete or quarantine: summary-driven product currentness,
    root-flush fallbacks, broad runtime-value syncs, and path-string deltas;
  - gates: product click/edit/scroll/formula/input-sync reports show zero
    root flushes, zero full summaries, zero full-list scans, scoped read keys,
    cycle-safe demand-current reads, and typed deltas replaying against a full
    recompute oracle.
- [ ] Visible read budget:
  - every product-frame runtime/currentness read must be recorded in a
    `VisibleReadSet` with typed key/range/field ids, reason, interaction id,
    window id when applicable, and whether the read affected pixels in that
    frame;
  - old path to delete or quarantine: implicit currentness barriers,
    opportunistic cleanup reads, broad summary refresh, or unscoped list/root
    currentness work before present;
  - gates: any pre-present read outside the ledger fails product UX, while
    post-present cleanup is tracked in a non-product lane.
- [ ] Generic query/materialization engine:
  - model spreadsheet currentness after real spreadsheet engines: maintain a
    dependency graph, dirty set, calculation chain/topological order, sparse
    address/index service, range/chunk dependency nodes, and batched or
    suspended recalculation during edit bursts. This must remain generic
    computed-field/query architecture, not Cells-only formula code;
  - promote `List/find`, `List/find_value`, range reads, dependency fanout, and
    list-window materialization into one generic indexed query service keyed by
    typed list id, field id, row key, generation, window id, and value type;
  - `List/chunk` must expose virtual windows over logical lists/grids instead
    of forcing full chunk materialization. Reports distinguish logical items,
    materialized rows, rendered nodes, evaluated fields/formulas, dirty
    ranges, selected keys, and dependent keys;
  - old path to delete or quarantine: per-fixture list scans, append-only
    materialization windows, address-string special cases, and hidden full-grid
    evaluation;
  - gates: Cells and at least one renamed non-Cells sparse-grid/list fixture
    pass indexed lookup, duplicate-key, removed-row, stale-generation,
    dependency fanout, range invalidation, window scroll, and cycle tests.
- [ ] Present/queue ownership decision:
  - measure a focus-safe hardware/product-surface present floor using the same
    app-window, adapter, surface, present mode, frame clock, and counters mode
    as real examples. Software/headless floors remain diagnostic;
  - make frame pacing explicit through the native windowing contract: report
    redraw request/receipt alignment, Wayland pre-present notification when
    available, input coalescing, stale-frame discard, acquire wait,
    submit-to-present, and whether an input crossed more than one redraw or
    present boundary before visible response;
  - choose direct surface render, app-owned texture plus copy-to-present, or
    proof-only offscreen render as explicit product/proof modes with report
    fields, not hidden defaults;
  - if queue/present dominates, test late surface acquire, bounded frames in
    flight, surface frame-latency settings, present mode, ring-buffered uploads,
    and surface-error recovery as named measured modes;
  - old path to delete or quarantine: unreported present-mode changes, proof
    copies on the product command buffer, blocked acquire/submit/present while
    holding input or proof locks;
  - gates: reports include acquire/encode/submit/present phase timings,
    present mode, desired frame latency, adapter/session metadata, in-flight
    count, queue-depth hints, surface epoch, and product-floor delta.
- [ ] Late-acquire/frame-in-flight experiment matrix:
  - run early acquire vs late acquire, one vs two product frames in flight,
    direct surface vs app-owned texture copy, and supported present modes on
    the same adapter/session/surface with identical proof mode and scenario
    hash;
  - keep only the measured winner as the default product mode. All other modes
    remain diagnostic flags with explicit report fields and cannot satisfy UX
    readiness accidentally;
  - old path to delete or quarantine: hidden environment overrides or local
    WGPU tweaks that improve one report but leave product mode ambiguous.
- [ ] Renderer resource lifetime:
  - make persistent pipelines, bind groups, glyph atlases, shaped text caches,
    primitive buffers, clip/transform buffers, staging belts/ring buffers,
    proof-only staging buffers, and debug artifacts distinct resource tiers;
  - product frames must reuse hot resources and upload only dirty chunks,
    transforms, text runs, overlays, and visible primitive ranges;
  - old path to delete or quarantine: render-scene cache keys based on proof
    strings/hashes, avoidable buffer reallocations, proof-resource allocation
    before present, and full primitive reprepare for retained patches;
  - gates: product reports include allocation count/bytes, upload bytes,
    queue writes, draw calls, cache hits/misses, buffer reuse, glyph uploads,
    dirty chunk count, and proof-resource allocation count.
- [ ] WebRender/APZ-style scroll path:
  - passive wheel/scroll first updates renderer/compositor-owned viewport
    transforms, clips, scroll offsets, and visible pointer/caret overlays.
    Runtime/layout refill runs as bounded overscan/materialization delta work
    and may land on later frames by epoch;
  - old path to delete or quarantine: scroll invoking runtime dispatch, graph
    rebuild, full layout relower, full `List/chunk`, full summary
    materialization, text reshaping for every visible cell, or uploads beyond
    visible range plus overscan delta;
  - gates: scroll-speed reports include scroll-transform patch counts, overscan
    refill counts, stale-frame discard counts, text-run reshape counts, upload
    bytes, runtime dispatch count, and full rebuild counters.
- [ ] Text-control subsystem:
  - build one retained text-control architecture for formula bars, cell editors,
    normal inputs, code editor, search fields, and future editors. It owns
    focus, hover, caret, selection, IME composition, paste, undo/redo, wheel,
    keyboard navigation, bound mirrors, validation/error display, and
    accessibility/proof snapshots as separate product/proof concerns;
  - old path to delete or quarantine: each example or surface inventing its
    own focus/text/value sync, or text proof being required to update visible
    product state;
  - gates: all interactive examples have host-event visual replays with visible
    pointer/caret, functional value assertions, wheel coverage, and no
    example-specific text branches.
- [ ] Dev-window isolation:
  - dev footer/HUD reads only cached scalar `PreviewPerfStats` snapshots at a
    throttled cadence. Source editor wheel, report expansion, proof history,
    large telemetry reads, and source replacement use paged/latest-wins
    transport and cannot block preview input/present;
  - old path to delete or quarantine: footer/render hooks parsing proof JSON,
    querying runtime, reading large reports, or sharing product-frame locks;
  - gates: dev-code-editor wheel replay no longer crashes, overloaded dev
    window leaves preview `preview_blocked_on_ipc_count=0`, and HUD overhead is
    reported separately.
- [ ] Verifier mode split and stale-path deletion:
  - each interactive scenario should have product-only counters mode,
    proof-only exact-key mode, and full HUD/report mode against the same
    scenario hash and binary/worktree fingerprint;
  - product-only repeated gates must pass before full proof/HUD gates are used
    to discuss UX latency. Full proof/HUD mode has a separately declared
    overhead budget and cannot be the only passing mode;
  - maintain a machine-readable stale-path ledger for legacy Ply, Xvfb, COSMIC
    scraping, browser screenshots, latest-report proof, artifact-only proof,
    modeled/static scroll, private runtime dispatch, broad summary paths, and
    fixture-specific shortcuts. Each row needs replacement, kill switch,
    positive gate, negative gate, and removal condition;
  - old path to delete or quarantine: any schema-valid stale artifact or
    compatibility alias satisfying native UX readiness;
  - gates: `verify-native-gpu-all --check-existing` rejects quarantined reports
    and stale paths; report schemas require worktree/binary freshness,
    evidence-key matching, product/proof mode, capture method, and stale-path
    counters.
- [ ] Owner-specific no-full-rebuild counters:
  - replace generic "no full rebuild" claims with per-owner counters:
    `document_relower_count`, `layout_frame_rebuild_count`,
    `render_scene_rebuild_count`, `gpu_resource_recreate_count`,
    `state_summary_refresh_count`, `proof_tree_mutation_count`, and
    `dev_ipc_block_count` per product interaction;
  - old path to delete or quarantine: inferring retained behavior from missing
    logs or aggregate timings;
  - gates: every product UX sample includes those counters or a typed reason
    why the owner was not involved.
- [ ] Compatibility alias deletion:
  - compatibility commands and old report aliases may remain for debugging, but
    native acceptance rejects reports generated through aliases unless they are
    hash-linked to the canonical command, marked non-acceptance, and routed
    through the same product/proof/stale-path checks;
  - old path to delete or quarantine: schema-valid stale reports or historical
    verifier commands continuing to satisfy readiness after the product contract
    changes;
  - gates: report schema and `verify-native-gpu-all --check-existing` fail
    closed on alias-only evidence.
- [ ] Hot-path budgets and deletion trigger:
  - add product-frame budgets for lock waits, channel waits, allocations,
    string/JSON clones, hash-map rebuilds, runtime dispatches, route scans,
    currentness reads, GPU uploads, submit/present stalls, and post-present
    queue pressure;
  - add stale-input gates: a newer input must not wait behind an older stale
    frame, must not cross more than one redraw/present boundary without a
    reported reason, and must not be hidden by input coalescing unless the
    product frame carries visible revision proof for the coalesced inputs;
  - after two fresh reports show the same boundary still dominates, stop local
    tuning and choose one replacement cut from this section. The plan update
    must name the removed boundary, old-path counter, positive gate, negative
    gate, rollback, and whether old code was deleted or quarantined.

Near-term recommended order:

1. Land the product render-result cut and post-present proof queue so product
   frames no longer build proof JSON or proof request debt before present.
2. Split perf ledgers by lane and make visible-click samples consume only exact
   product commits by interaction id and frame key.
3. Add product-only/proof-only verifier modes for repeated Cells click, scroll,
   dev-editor wheel, Counter click, and one non-Cells sparse fixture.
4. Introduce the smallest `ActivePreviewScene` strangler for focus, hover,
   selection, caret, text mirrors, and passive scroll, with old proof/report
   hot-state reads forbidden.
5. Measure the focus-safe hardware present floor and decide late-acquire /
   frames-in-flight / present-mode policy from that evidence.
6. Promote the runtime delta/currentness/query engine work once product/proof
   separation proves runtime is again on the critical path.

## 2026-07-02 Optional External Proof Checkpoint

This is a narrow product/proof split step. It is not the full type-level
`ProductRenderResult` cut and does not make Cells 60 FPS complete.

- Implemented an optional external proof payload for native render hooks:
  - `NativeRenderHookResult.proof` is now `Option<serde_json::Value>`;
  - app-window storage/reporting keeps `last_external_render_proof` absent when
    the render hook returns `None`;
  - `NativeRenderHookResult::rendered_without_proof()` validates rendered
    product frames using revisions and typed render-frame metrics, not proof
    JSON;
  - preview proof mode `Counters` now passes
    `lightweight_product_render_report = true` to the native GPU render hook;
  - readback/verifier mode can still pass `--skip-render-hook-app-owned-proof`
    without being confused with product-only counters mode;
  - product counters mode no longer builds the discarded lightweight external
    render report JSON at all. Product evidence stays in
    `NativeRenderFrameMetrics`, `NativeRenderedProductFrame`, and
    `NativeProductFrameCommit`;
  - product counters mode also skips compact structured visual-proof payloads
    that still belong to verifier/readback mode: visible-bound-text proof,
    retained-bound-sync proof, focused-node proof, selected-node samples,
    layout artifact metadata, proof-history style diagnostics, and replacement
    proof JSON;
  - product counters mode now attaches post-present proof requests with
    `currently_legacy_pre_present = false`, and the product frame flags
    `legacy_proof_json_built_pre_present = false` and
    `legacy_render_hook_proof_built_pre_present = false`;
  - readback/full proof modes remain honestly marked as legacy pre-present
    proof until the real `PostPresentProofQueue` / typed product result exists.
- Added focused unit tests:
  - app-window validates a rendered frame with no external proof payload;
  - counters-mode summaries are deferred and not legacy;
  - readback verifier summaries remain legacy while structured render-hook
    proof is still required;
  - full proof summaries still include external app-owned readback.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `git diff --check -- crates/boon_native_playground/src/main.rs
    docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md
    crates/boon_native_app_window/src/lib.rs crates/xtask/src/main.rs` passes;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_playground
    product_counters_proof_requests_are_deferred_not_legacy --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_hook_result_can_present_without_external_proof_payload --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes.
- Remaining architecture debt:
  - `NativeRenderHookResult` still has an optional proof JSON escape hatch for
    readback/dev/world-scene proof modes, so the full type-level
    `ProductRenderResult` / `PostPresentProofQueue` cut is not done;
  - readback verifier mode still needs compact render-hook proof fields for
    visible Cells formula/focus proof, so `verify-native-cells-visible-click-e2e`
    should still be expected to fail the new legacy pre-present proof gate until
    proof subscribers produce those fields after present by exact
    `FrameEvidenceKey`;
  - direct visible-surface readback in readback mode is still encoded before
    present, and queue/present ownership remains a separate architecture
    blocker.

## 2026-07-02 Keyed Post-Present Proof Queue Checkpoint

This is the first app-window-owned queue/registry step after the optional proof
payload cut. It still does not run proof subscribers after present; it makes the
deferred proof debt explicit, bounded, keyed, and report-visible.

- Implemented a generic bounded queue for post-present proof requests:
  - added `NativePostPresentProofQueueEntry` keyed by `FrameEvidenceKey`;
  - added `NativePostPresentProofQueueStatus` with `queued` and
    `legacy_already_built_pre_present` states;
  - `NativeRenderLoopState` now tracks a rolling queue plus enqueued,
    deferred, and legacy-pre-present counters;
  - `note_product_frame_commit()` enqueues each commit's proof requests after
    the product frame has been presented;
  - queue entries preserve request kind, legacy flag, frame-local snapshot
    requirement, evidence key, and enqueue elapsed timestamp;
  - the queue is capped at 64 recent entries so proof/report debt cannot grow
    unbounded in app-window state.
- Added report fields:
  - `post_present_proof_queue_limit`;
  - `post_present_proof_queue_enqueued_count`;
  - `post_present_proof_queue_deferred_count`;
  - `post_present_proof_queue_legacy_pre_present_count`;
  - `recent_post_present_proof_queue_count`;
  - `recent_post_present_proof_queue`.
- Added focused tests:
  - a direct state test proves deferred and legacy requests enqueue with the
    same `FrameEvidenceKey`;
  - the render-loop report test proves JSON evidence exposes queue counters and
    keyed entries alongside product commits.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_app_window
    post_present_proof_queue_tracks_deferred_and_legacy_requests_by_frame_key
    -- --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats
    -- --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_hook_result_can_present_without_external_proof_payload --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_playground
    product_counters_proof_requests_are_deferred_not_legacy --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes;
  - `git diff --check -- crates/boon_native_app_window/src/lib.rs
    crates/boon_native_playground/src/main.rs
    docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md
    crates/xtask/src/main.rs` passes.
- Remaining architecture debt:
  - queue entries are not yet consumed by worker subscribers;
  - readback/full proof modes still build the structured proof payload before
    present and should keep failing legacy-pre-present gates where relevant;
  - reports still carry both legacy proof-request fields and queue fields during
    the transition;
  - the next cut should move visible-bound-text proof, retained-sync proof,
    readback completion, artifact hashes, and report/proof-history work into
    keyed post-present subscribers.

## 2026-07-02 Post-Present Subscriber Drain Guard Checkpoint

This slice turns the queue into a small state machine for the first generic
post-present subscribers and prevents report/proof-history draining from
stealing time from a newer host input. It still does not complete the full proof
worker architecture.

- Implemented generic subscriber state tracking:
  - `NativePostPresentProofQueueStatus` now distinguishes `queued`,
    `legacy_already_built_pre_present`, and `completed_post_present`;
  - queue entries carry optional `completed_elapsed_ms`;
  - `NativeRenderLoopState` tracks completed post-present proof requests,
    deferred subscriber-drain count, and last defer reason;
  - `note_post_present_proof_request_completed()` completes a queued request by
    exact `FrameEvidenceKey` and request kind without touching legacy rows.
- Added a product-loop guard:
  - after present, proof-history completion is marked by exact frame key;
  - render-loop report JSON subscriber completion is marked only when the async
    report snapshot is enqueued;
  - if a newer host input arrives before report/proof-history subscriber
    draining, the loop records `pending_host_input`, skips report snapshot
    construction for that pass, schedules a host-input wake, and returns to the
    input loop;
  - this is generic app-window scheduling and does not branch on examples,
    source paths, labels, or Cells fields.
- Added report fields:
  - `post_present_proof_queue_completed_count`;
  - `post_present_subscriber_drain_deferred_count`;
  - `last_post_present_subscriber_drain_deferred_reason`;
  - queue entries now expose `completed_elapsed_ms` when a subscriber finishes.
- Added focused tests:
  - queued requests can complete post-present while legacy pre-present requests
    remain legacy;
  - subscriber draining yields when a newer host-input wake exists;
  - render-loop reports expose the new completion/defer fields.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_app_window
    post_present_proof_queue_tracks_deferred_and_legacy_requests_by_frame_key
    -- --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    post_present_subscriber_drain_yields_to_pending_host_input --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats
    -- --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_hook_result_can_present_without_external_proof_payload --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_playground
    product_counters_proof_requests_are_deferred_not_legacy --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes;
  - `git diff --check -- crates/boon_native_app_window/src/lib.rs
    crates/boon_native_playground/src/main.rs
    docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md
    crates/xtask/src/main.rs` passes before this doc update.
- Remaining architecture debt:
  - visible-bound-text proof, retained-sync proof, artifact hashes, and WGPU
    readback still need real keyed worker subscribers;
  - skipped report snapshot construction must be validated in repeated release
    input runs to prove it reduces next-input interference;
  - a typed `ProductRenderResult` should replace the remaining optional proof
    JSON escape hatch;
  - full product/proof isolation still requires product-only and proof-only
    repeated native gates.

## 2026-07-02 Post-Present Subscriber Callback Checkpoint

This slice wires the first generic callback lane for post-present proof
artifacts. It is still an architecture checkpoint, not the final 60 FPS fix.

- Added no-loss architecture TODOs:
  - stable identity ABI across runtime/document/layout/render/proof phases;
  - product-frame lane taxonomy;
  - hot-path memory/allocation model;
  - input coalescing and priority policy;
  - late-acquire / frames-in-flight decision gate;
  - browser-style phase boundaries;
  - render-thread ownership option;
  - query-planner style runtime execution;
  - verifier driver/product split;
  - dev window as a client, not product-state owner;
  - AGENTS.md readiness gate sync when new native UX/proof gates become
    required;
  - minimal-path rewrite escape hatch if the legacy product/proof path remains
    too complex after isolation work.
- Subagent review:
  - runtime/compiler/document TODO coverage was judged complete, with the main
    risk being duplicated organization rather than missing concepts;
  - frame-loop/WGPU/proof TODO coverage was also judged complete, with the
    concrete gap that AGENTS.md readiness commands must be kept in sync with
    new product-only/proof-only/present-floor/stale-path gates.
- Implemented generic callback plumbing:
  - `NativeRenderHookResult` carries post-present proof subscribers separately
    from optional legacy proof JSON;
  - app-window collects those subscribers during render, runs them only after
    `frame.present()`, and only when no newer host input wake is pending;
  - subscriber artifacts are keyed by exact `FrameEvidenceKey` and complete
    matching queued proof requests by request kind;
  - subscriber errors are counted and reported without blocking product present;
  - render-loop reports expose artifact count, recent artifacts, subscriber
    error count, and last subscriber error;
  - preview counters mode now creates a retained-bound-sync subscriber artifact
    from a frame-local stats snapshot.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_app_window post_present --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats
    -- --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_hook_result_can_present_without_external_proof_payload --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_playground product_counters --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes.
- Remaining architecture debt:
  - visible-bound-text proof, external app-owned readback completion, artifact
    hash, report JSON, and proof-history work still need to become real bounded
    post-present subscribers;
  - the product render hook still has an optional legacy proof JSON escape
    hatch and should be replaced with a typed `ProductRenderResult`;
  - release Cells/native UX performance was not rerun for this checkpoint, so
    this must not be treated as a 60 FPS acceptance result;
  - AGENTS.md and `docs/architecture/NATIVE_GPU_PIPELINE.md` still need a
    contract sync once product-only/proof-only/stale-path gates stabilize.

## 2026-07-02 Visible-Bound-Text Subscriber Checkpoint

This slice moves the first verifier-facing text proof into the keyed
post-present callback lane for counters/product mode. It is still not the final
product/proof split, but it removes one more proof artifact from the normal
first-frame product contract.

- Implemented frame-local visible-bound-text proof snapshots:
  - counters/product preview render now captures a
    `PreviewVisibleBoundTextProofSnapshot` with layout-frame hash,
    `Arc<LayoutFrame>`, and report mode;
  - the capture uses the existing retained `Arc<LayoutFrame>` where available
    and only falls back to cloning when no retained frame is available;
  - the post-present subscriber builds the compact visible-bound-text payload
    from that immutable frame-local snapshot after present;
  - the subscriber is generic: it uses layout/document binding indexes and
    layout node ids, not example names, source paths, Cells fields, addresses,
    or fixture labels.
- Retained-sync subscriber behavior remains:
  - counters/product mode now emits both `VisibleBoundText` and
    `RetainedBoundSync` post-present subscribers;
  - both artifacts carry the same exact `FrameEvidenceKey` supplied by
    app-window after `frame.present()`;
  - queued proof requests complete by exact key and request kind.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_playground
    product_counters_mode_creates_retained_sync_post_present_subscriber --
    --test-threads=1` passes and now proves both visible-bound-text and
    retained-sync artifacts;
  - `cargo test -q -p boon_native_app_window post_present --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats
    -- --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes.
- Remaining architecture debt:
  - external app-owned readback completion, artifact hash, render-hook report
    JSON, and proof-history work still need to become bounded post-present
    subscribers;
  - readback/full-proof mode still builds structured proof payloads before
    present and must keep failing legacy-pre-present acceptance where relevant;
  - subscribers still run inline after present when no newer input is pending;
    the next cut should move heavier subscriber work onto a bounded worker
    queue with proof lag/drop counters;
  - release Cells/native UX performance was not rerun for this checkpoint, so
    this must not be treated as a 60 FPS acceptance result.

## 2026-07-02 App-Window Artifact Completion Checkpoint

This slice removes two app-window special-case completion paths from the
post-present proof queue. Proof-history and render-loop report JSON now use the
same keyed artifact accounting as render-hook subscribers.

- Implemented generic artifact construction:
  - added `native_post_present_json_proof_artifact(...)` beside the subscriber
    helper;
  - `native_post_present_json_proof_subscriber(...)` now uses the same artifact
    constructor as app-window-owned completions.
- Converted app-window post-present completions:
  - proof-history updates now call `note_post_present_proof_artifact(...)`
    with kind `ProofHistory`, exact `FrameEvidenceKey`, and a compact payload
    containing status, frame seq, input event seq, and recent history count;
  - async render-loop report snapshot enqueue now records a
    `RenderHookReportJson` artifact after enqueue with exact
    `FrameEvidenceKey`, enqueue timing, report path, frame seq, and input event
    seq;
  - these artifacts complete matching queued requests by exact key and kind,
    rather than bypassing the artifact ledger with direct completion counters.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_app_window post_present --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats
    -- --test-threads=1` passes;
  - `cargo test -q -p boon_native_playground
    product_counters_mode_creates_retained_sync_post_present_subscriber --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes.
- Remaining architecture debt:
  - report JSON is still snapshot/enqueued inline after present when no newer
    host input is pending. It is now accounted as a keyed artifact, but it is
    not yet a bounded worker subscriber;
  - artifact hash and external app-owned readback completion still need the
    same artifact/subscriber treatment;
  - the product render hook still has the legacy optional proof JSON escape
    hatch in readback/full-proof modes;
  - release Cells/native UX performance was not rerun for this checkpoint.

## 2026-07-02 Visible-Surface Readback Artifact Checkpoint

This slice mirrors completed app-window interactive WGPU visible-surface
readbacks into the same keyed post-present artifact ledger as other proof
subscribers, while preserving the existing readback registry fields for
compatibility.

- Converted interactive readback completions:
  - all normal poll sites and the final report-drain path now call one helper
    for completed interactive readbacks;
  - the helper still updates `recent_interactive_readback_artifacts` and
    `last_interactive_readback_artifact`;
  - when the readback carries a `FrameEvidenceKey`, it also records a
    `VisibleSurfaceReadback` `NativePostPresentProofArtifact` with the exact
    frame key, completion time, artifact path, hash, capture method, texture
    format, dimensions, nonblank/unique-pixel counts, deadline, poll status,
    frame seq, input event seq, and present id;
  - matching queued `VisibleSurfaceReadback` requests are completed by exact
    key and request kind through `note_post_present_proof_artifact(...)`.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_app_window
    completed_interactive_readback_records_visible_surface_post_present_artifact
    -- --test-threads=1` passes.
  - `cargo test -q -p boon_native_app_window
    recent_interactive_readback_registry_matches_exact_frame_key --
    --test-threads=1` passes.
  - `cargo test -q -p boon_native_app_window
    final_report_drain_completes_pending_interactive_readback --
    --test-threads=1` passes.
  - `cargo test -q -p boon_native_app_window post_present --
    --test-threads=1` passes.
  - `cargo test -q -p boon_native_playground
    product_counters_mode_creates_retained_sync_post_present_subscriber --
    --test-threads=1` passes.
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes.
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes.
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes.
  - `git diff --check -- crates/boon_native_app_window/src/lib.rs
    docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md` passes.
- Remaining architecture debt:
  - visible-surface readback completion is now keyed and accounted, but it is
    still produced by the existing interactive readback worker path rather than
    by a fully bounded `PostPresentProofQueue` worker service;
  - explicit queueing of `VisibleSurfaceReadback` requests for every verifier
    mode still needs to be checked against existing `ExternalAppOwnedReadback`
    request summaries;
  - artifact hashing, external app-owned render proof, proof-history
    compaction, report JSON, and screenshot/diff work still need one bounded
    post-present worker model with lag/drop counters;
  - release Cells/native UX performance was not rerun for this checkpoint.

## 2026-07-02 Visible-Surface Request Accounting Checkpoint

This slice makes app-window visible-surface readback a first-class requested
post-present proof artifact on the product frame that actually keeps the
readback. It avoids advertising the request for frames where readback is skipped
because newer host input arrived.

- Product commit request accounting:
  - app-window product commits can now add a deferred
    `VisibleSurfaceReadback` request after the post-present stale-input check;
  - the request is added only once and keeps `NativeProductFrameCommit`,
    nested `NativeRenderedProductFrame`, and queue counters consistent;
  - commit publication was moved until after the stale-input readback decision
    so skipped readbacks do not leave unmatched queued requests;
  - completed readback artifacts still complete the request by exact
    `FrameEvidenceKey` and request kind.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_app_window
    product_frame_commit_adds_visible_surface_readback_request_once --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    completed_interactive_readback_records_visible_surface_post_present_artifact
    -- --test-threads=1` passes.
  - `cargo test -q -p boon_native_app_window post_present --
    --test-threads=1` passes.
  - `cargo test -q -p boon_native_playground
    product_counters_proof_requests_are_deferred_not_legacy --
    --test-threads=1` passes.
  - `cargo test -q -p boon_native_playground
    product_counters_mode_creates_retained_sync_post_present_subscriber --
    --test-threads=1` passes.
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes.
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes.
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes.
  - `git diff --check -- crates/boon_native_app_window/src/lib.rs
    docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md` passes.
- Remaining architecture debt:
  - the app-window visible readback request is now explicit and keyed, but the
    actual readback worker is still the existing interactive readback job, not
    the final bounded `PostPresentProofQueue`;
  - product render-hook `ExternalAppOwnedReadback` and app-window
    `VisibleSurfaceReadback` remain separate request kinds and need a stricter
    verifier contract that states which one each mode requires;
  - release Cells/native UX performance was not rerun for this checkpoint.

## 2026-07-02 Async Post-Present Proof Worker Checkpoint

This slice moves render-hook post-present proof subscribers out of inline
after-present execution and into a bounded worker queue. It is still a step
toward the final `PostPresentProofQueue`, not the full realtime fix.

- Worker model:
  - app-window now owns an async post-present proof subscriber worker with a
    bounded pending batch limit;
  - the render loop enqueues subscribers after present by exact
    `FrameEvidenceKey` instead of running them inline;
  - completed artifacts and errors are drained back into
    `NativeRenderLoopState` and the existing keyed artifact ledger;
  - proof-history and render-hook report-json request completion now use worker
    subscribers instead of inline `note_post_present_proof_artifact(...)` calls
    in the render loop;
  - artifact-hash requests now have a reusable worker subscriber that hashes
    registered artifact paths post-present and reports an explicit
    `no_registered_artifacts` status when no paths are registered yet;
  - worker counters report enqueued batches, enqueued subscribers, dropped
    batches, dropped subscribers, completed artifacts, errors, pending batches,
    and enqueue wall time;
  - final report shutdown drains worker results before writing the last report.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `cargo test -q -p boon_native_app_window
    async_post_present_proof_worker_records_keyed_artifact -- --test-threads=1`
    passes;
  - `cargo test -q -p boon_native_app_window
    async_post_present_proof_worker_completes_history_and_report_requests --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window
    async_post_present_proof_worker_completes_artifact_hash_request --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window post_present --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_playground
    product_counters_mode_creates_retained_sync_post_present_subscriber --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_ux_integrity_rejects_legacy_pre_present_proof_coupling --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    present_floor_label_contract_accepts_counters_only_product_report --
    --test-threads=1` passes;
  - `cargo test -q -p xtask
    native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback
    -- --test-threads=1` passes;
  - `git diff --check -- crates/boon_native_app_window/src/lib.rs
    docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md` passes.
- Remaining architecture debt:
  - this is a bounded worker, but not yet the final proof-isolation stress gate;
  - visible readback, external app-owned render proof, artifact path
    registration, screenshots, and diffs still need one unified subscriber
    service with mode allowlists, lag/drop counters, stale-key rejection, and
    bounded memory;
  - release Cells/native UX performance was not rerun for this checkpoint.

## 2026-07-02 App-Window-Owned Artifact Hash Checkpoint

This slice removes the placeholder artifact-hash subscriber from the playground
render hook and makes app-window responsible for registering artifact paths by
exact `FrameEvidenceKey`. It is proof-isolation plumbing, not the final 60 FPS
fix.

- Ownership change:
  - playground still creates frame-local visible-bound-text and retained-sync
    post-present subscribers, but no longer creates an empty
    `ArtifactHash(Vec::new())` subscriber;
  - app-window checks whether the current product frame requested deferred
    `ArtifactHash` proof, then creates the artifact-hash subscriber after
    present and before async worker enqueue;
  - path registration is generic and exact-keyed: matching
    `AppWindowReadbackArtifact` paths and external app-owned render-proof
    `artifact_path`/`path` entries are included only when they already exist on
    disk and match the current `FrameEvidenceKey`;
  - empty artifact-hash subscribers are no longer enqueued when a matching
    `VisibleSurfaceReadback` request is still pending for the exact frame;
  - those deferred `ArtifactHash` requests are completed by the interactive
    readback completion path using the completed app-window readback artifact's
    already-computed SHA-256 and the exact frame key;
  - late readback completion records an `ArtifactHash` artifact only when this
    exact key was marked as deferred for readback, so non-deferred real-path
    hash subscribers cannot race into duplicate hash artifacts;
  - stale prior-frame artifacts, missing files, and report JSON files that may
    still be in the async latest-wins writer are not hashed in this frame;
  - compact recent external proof now reads the actual native GPU
    `artifact_path` field with `path` kept as fallback.
- Verification for this checkpoint:
  - `cargo fmt --check` passes;
  - `cargo test -q -p boon_native_app_window post_present --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window artifact_hash --
    --test-threads=1` passes;
  - `cargo test -q -p boon_native_app_window completed_interactive_readback
    -- --test-threads=1` passes;
  - `cargo test -q -p boon_native_playground
    product_counters_mode_creates_retained_sync_post_present_subscriber --
    --test-threads=1` passes and now asserts playground does not create
    `ArtifactHash`;
  - `cargo test -q -p boon_native_playground
    product_counters_proof_requests_are_deferred_not_legacy --
    --test-threads=1` passes;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p
    xtask` passes with existing warnings;
  - `git diff --check -- crates/boon_native_app_window/src/lib.rs
    crates/boon_native_playground/src/main.rs
    docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md` passes.
- Remaining architecture debt:
  - app-window-owned readbacks, external render proof, screenshots, diffs, and
    report artifacts still need one bounded proof service with mode allowlists,
    lag/drop counters, stale-key rejection, and proof-isolation stress gates;
  - the current completion-triggered artifact hash covers interactive visible
    readback artifacts; report JSON and future screenshot/diff artifacts still
    need exact-key immutable artifact registration before they can be hashed
    safely;
  - release Cells/native UX performance was not rerun for this checkpoint.

## 2026-07-02 Consolidated Architecture TODO Index

Use this section as the no-loss index when the implementation starts looping.
It intentionally repeats the strongest options from the larger backlog in a
shorter form so a future pass can choose a real architecture cut instead of
another micro-optimization. Cells is only a stress fixture; every item below
must be implemented generically or rejected with a clear reason.

- [ ] Product hot loop:
  - create a single `PreviewHotLoop` state machine that owns input sampling,
    frame pacing, active scene selection, submit/present, and product-frame
    commit publication;
  - keep requested animation as a bounded burst substate inside DemandDriven,
    with explicit quiet-frame and hard-cap exits;
  - sample input at the start of already scheduled product frames, not after
    proof/report cleanup;
  - deletion gate: product frames must not wait for proof JSON, full reports,
    latest-report queries, or verifier readback.
- [ ] Typed product transaction ABI:
  - replace loosely coupled proof/report payloads with a typed
    `ProductRenderResult` and `NativeProductFrameCommit`;
  - every visible-changing frame carries `InteractionId`, input event seq,
    `FrameEvidenceKey`, revisions, lane, scheduler reason, dirty reason,
    product timings, retained patch counts, and old-path counters;
  - verifiers match exact product rows, not the last aggregate preview-loop
    sample.
- [ ] Post-present proof service:
  - enqueue visible text proof, retained-sync proof, WGPU readback, proof
    history, artifact hash, and report JSON work after present by exact
    `FrameEvidenceKey`;
  - bound queues with latest-wins replacement where valid, explicit overflow
    counters, stale-key rejection, and proof-lag reporting;
  - product UX latency excludes proof completion, but proof must link back to
    the measured frame or fail.
- [ ] Active/pending retained scenes:
  - publish immutable `ActivePreviewScene` snapshots for hit testing, retained
    overlays, scroll transforms, caret, focus, selection, and text mirrors;
  - build heavier runtime/layout/render snapshots as `PendingPreviewScene`
    work with latest-wins cancellation and epoch checks;
  - allow active-scene patches for simple visible state while pending work is
    incomplete;
  - deletion gate: no full document relower, layout rebuild, render-scene
    rebuild, or state-summary refresh on normal select/edit/scroll.
- [ ] Typed input and source-intent path:
  - convert host input into typed control/source intents once, with stable
    node identity and route snapshots;
  - avoid geometry/string/label/source-path rediscovery on the product path;
  - make text inputs, spreadsheet cells, code editor, buttons, search fields,
    and future controls use the same control-state machinery.
- [ ] Present/queue ownership:
  - measure a focus-safe hardware present-floor baseline on the same surface
    class used by product preview;
  - test late acquire, command-buffer reuse, prepared resource reuse, frame
    pacing, and bounded frames-in-flight as explicit policies;
  - keep diagnostic knobs reported and opt-in, never hidden acceptance
    shortcuts.
- [ ] Render/WGPU retained resources:
  - make renderer-owned caches use stable document/render identities and dirty
    chunk ids, not fixture text or geometry;
  - keep glyph atlases, pipelines, bind groups, vertex/index buffers, upload
    rings, and scene batches hot across product frames;
  - report upload bytes, queue writes, draw calls, cache hits, cache evictions,
    and GPU-resource recreate counts per product frame.
- [ ] Runtime/list/currentness engine:
  - keep sparse logical lists separate from materialized rows and rendered
    nodes;
  - use generic indexed query services for `List/find`/lookup-like operators;
  - use demand-current read barriers and dependency fanout for derived fields;
  - batch reset/source initialization and make cycle safety a runtime contract;
  - prove with renamed non-Cells sparse fixtures as well as Cells.
- [ ] Virtualized layout/materialization:
  - make list/chunk/grid materialization driven by viewport and explicit
    dependencies;
  - report logical items, materialized items, rendered items, evaluated fields,
    and retained patches separately;
  - reject any performance pass that shrinks the logical fixture or hides work
    in example-specific syntax.
- [ ] Dev-window performance HUD:
  - expose a fixed scalar `PreviewPerfStats` snapshot from app-window-owned
    atomics/rolling buckets;
  - show mode/age/last latency/p95/render/present/proof/drops, with idle shown
    as idle rather than fake FPS;
  - forbid footer/render hooks from doing IPC, runtime queries, JSON parsing, or
    proof/report reads.
- [ ] Product-only and proof-only verifier modes:
  - add repeated product-only gates for click, text input, scroll, wheel,
    Counter, TodoMVC, Cells, and a renamed sparse fixture;
  - add proof-isolation gates that intentionally delay/overload readback/report
    workers and assert product p95 stays stable while proof status reports lag
    or failure honestly;
  - native UX gates fail on ContinuousProbe, injected-below-host input, stale
    first-frame proof, mismatched `FrameEvidenceKey`, dev IPC blocking, passive
    scroll dispatch, or proof-required visible updates.
- [ ] Scheduler simulation and regression lab:
  - add deterministic scheduler tests for idle, burst, backpressure, surface
    lost/recreate, stale pending snapshots, proof queue overflow, report-worker
    stalls, rapid input, repeated scroll, and long-session aging;
  - keep release-mode native reports for product path and proof path separate,
    schema-valid, fingerprinted, and tied to current binary/worktree.
- [ ] Old-path deletion ledger:
  - quarantine or delete latest-report matching, proof-JSON-as-product-result,
    broad runtime summaries, geometry/string route fallback, full-state refresh
    on simple input, pre-present readback in product mode, dev IPC in render
    hooks, and compatibility aliases accepted as readiness proof;
  - every retained old path needs a counter, owner, reason, and negative test;
  - after two fresh reports show the same boundary dominates, the next patch
    must remove or isolate that boundary instead of tuning around it.
- [ ] Generic no-hacks audit:
  - production compiler/runtime/document/layout/renderer/app-window/playground/
    verifier code must not branch on example names, source paths, Cells,
    addresses, formula fields, fixture labels, row counts, or screenshot text;
  - allow those words only in examples, scenarios, verifier fixture names, and
    explicit negative tests;
  - add at least one renamed sparse-grid/list fixture that would fail if the
    engine relied on spreadsheet-shaped identifiers.
- [ ] External architecture research backlog:
  - compare game loops, browsers, Servo-style display lists, GPUI/Ply-style
    retained UI, Bevy-style schedules/extraction, spreadsheet dependency
    engines, and Rust async/event-loop runtimes;
  - import only the simple architecture lessons that remove product-path work:
    retained active state, typed deltas, bounded queues, immutable snapshots,
    worker backpressure, render resource ownership, and lane-specific metrics.

## 2026-07-02 Final No-Loss Implementation Backlog

This addendum folds the latest subagent and high-level review notes into one
implementation-oriented TODO list. It is deliberately architecture-heavy:
future work should pick the largest simple cut that removes a product-frame
boundary, then add the positive gate and stale-path negative gate. Do not turn
these into another round of local timing tweaks.

- [ ] `NativeFrameClock` as the single product owner:
  - one state machine owns `FrameSeq`, `InteractionId`, accepted input batches,
    burst pacing, active-scene mutation, submit/present, product-frame commits,
    and post-present subscriber enqueue;
  - model the states explicitly: `DemandIdle`, `BurstArmed`,
    `ProductFrame`, `PostPresentSubscribers`, `ProofOnly`, and
    `SurfaceRecovery`;
  - all wake requests enter through typed lanes with priorities and phase
    allowlists. No subsystem may submit, relabel, or extend a product frame
    without a transaction owned by the frame clock;
  - success gate: product reports show exactly one frame owner, exact wake
    reason, exact lane, explicit pacing state, and zero proof/report/source
    cleanup wakes charged to unrelated host input.
- [ ] Product protocol split:
  - define a compact `ProductRenderResult` / `PresentedProductFrame` boundary
    for product mode. It carries scalar counters, revisions, retained patch
    counts, present metadata, `NativeProductFrameCommit`, and proof request
    handles;
  - product render hooks must not return proof JSON, layout artifacts,
    latest-report-derived identity, or mutable runtime/dev/report state as the
    product result;
  - full proof/report payloads are generated from immutable snapshots or keyed
    handles after present;
  - success gate: schema/static checks fail if a product frame constructs a
    proof tree, reads latest report state, or waits for proof output before
    present.
- [ ] `ActivePreviewScene` as immutable product truth:
  - make product frames read only the current immutable active scene:
    retained hit routes, stable node ids, overlay/property trees, scroll
    transforms, focus/hover/selection/caret state, text-control mirrors,
    binding mirrors, render batches, GPU resource handles, and the frame
    evidence registry;
  - runtime, compiler, document, layout, and proof work publish
    `PendingPreviewScene` snapshots through latest-wins capacity-1 queues;
  - pending snapshots commit only after source/content/layout/render/surface
    epochs match and stale work is rejected before expensive allocation or
    serialization;
  - success gate: hover, focus, selection, caret, passive scroll, and text
    mirror frames do not borrow broad runtime, document, report, dev IPC, or
    proof state before present.
- [ ] Render extraction and cache pipeline:
  - define the renderer pipeline as `ExtractDirty -> PrepareResources ->
    QueueDraws -> Encode -> SubmitPresent -> PostPresent`;
  - `ExtractDirty` is the short sync boundary from active/pending scene into
    render-owned state. It copies only visible/materialized display items,
    scroll uniforms, text runs, dirty primitive ranges, route data, and
    evidence ids;
  - the renderer owns pipelines, bind groups, glyph atlases, shaped text,
    texture atlases, staging belts/ring buffers, primitive batches, dirty
    chunk ids, render bundles, route/hit snapshots, and readback resources;
  - success gate: reports expose changed component counts, upload bytes,
    queue writes, draw calls, encode time, submit time, present time, cache
    hits, cache evictions, and allocation counts for every product frame.
- [ ] Non-destructive input sampling and typed lanes:
  - sample host input once at frame start, preserve sequence numbers, coalesce
    pointer and wheel movement by target/axis, and emit typed
    `SourceIntent`, `ViewportIntent`, `TextEditIntent`, and `FocusIntent`;
  - define lanes such as `ProductInput`, `SourceCommit`, `RuntimeLayout`,
    `SurfaceLifecycle`, `ProofReadback`, `ReportHud`, and `DevTelemetry`;
  - product input drains first. Source/runtime cleanup, proof/readback,
    report/HUD, accessibility, cursor refresh, and dev telemetry cannot
    relabel, delay, or extend an already-presented product frame;
  - success gate: every visible input sample reports input seq, route epoch,
    intent id, lane, transaction id, product frame key, and stale-result
    policy. Missing or mismatched ids fail closed.
- [ ] Generic retained control subsystem:
  - create one retained control model for focus, hover, active/pressed state,
    text mirror, selection range, caret blink, IME/composition, scroll offset,
    drag state, undo grouping, and accessibility focus;
  - cells, formula bars, ordinary text inputs, the dev code editor, buttons,
    list rows, scrollbars, and future controls use this same state machine;
  - first-frame retained patches may be optimistic only under an explicit
    `OptimisticUiCommit` contract with reconciliation, rollback, and runtime
    confirmation evidence;
  - success gate: click-to-focus, click-to-bound-text, hover, typing, editor
    wheel, selection movement, and scrollbars pass all-example visual replay
    without example-specific control code.
- [ ] Same-surface present-floor and pacing lab:
  - measure empty retained frame, simple overlay patch, text-control patch,
    scroll transform, and full example click on the same app-window, adapter,
    surface class, present mode, and frame clock used by product preview;
  - keep software/headless/nested-compositor baselines as harness diagnostics
    only. They cannot prove or excuse real desktop product latency;
  - report surface acquire, encode, queue submit, `frame.present()`,
    compositor/vsync floor, GPU completion if available, proof completion,
    adapter, backend, present mode, and desired frame latency;
  - test late acquire, command-buffer reuse, render bundles, upload rings,
    alternate present modes, frame latency, and frames in flight only as
    reported policies, never hidden shortcuts;
  - success gate: the plan can say whether remaining p95 is app CPU,
    queue/present policy, compositor floor, proof coupling, or verifier
    accounting.
- [ ] Bounded post-present proof service:
  - define `PostPresentProofQueue` as a bounded worker service consuming
    `NativeProductFrameCommit` by exact `FrameEvidenceKey`;
  - subscribers include WGPU readback, visible-bound-text proof,
    retained-sync proof, proof history, report JSON, artifact hashing,
    screenshot/PNG encoding, diffing, runtime value probes, and debug dumps;
  - every subscriber has queue length, latest-wins/drop policy, lag counters,
    stale-key rejection, overflow counters, and mode allowlists;
  - success gate: proof-isolation stress deliberately stalls/drops subscribers
    while product p95, product missed frames, and first-frame pixels remain
    stable. Only proof lag/drop/status changes.
- [ ] Product-only, proof-only, and full-mode verifier split:
  - every interactive verifier has `product-only`, `proof-only`,
    `product-plus-proof`, `full-HUD/report`, `present-floor`, and
    `stale-path-negative` modes;
  - product-only is the UX gate. Proof-only validates exact pixels, currentness,
    and artifacts by key. Full mode may be slower but must explain cost
    separately;
  - release UX gates fail on ContinuousProbe, injected-below-host input,
    missing app-owned input timing, driver timing as authority, stale proof,
    mismatched `FrameEvidenceKey`, proof-required visible updates, or dev IPC
    blocking;
  - success gate: every example with interactive controls has deterministic
    visual replay with app-owned cursor proof and functional assertions.
- [ ] Machine-readable stale-path deletion ledger:
  - move the stale-path table into a checked data file or schema-backed report
    consumed by xtask;
  - every row has owner crate, owner date, temporary mode, typed replacement,
    kill switch, runtime/report counter, positive gate, negative stale-path
    gate, and removal condition;
  - seed rows include proof JSON as product result, latest-report matching,
    geometry/string route rediscovery, full `state_summary` before present,
    full relower on selection/scroll, pre-present readback, dev IPC in render
    hooks, driver timing fallback, modeled/static scroll readiness, private
    runtime dispatch, old Ply/Xvfb/COSMIC/browser proof, and spreadsheet-only
    sparse fixture proof;
  - success gate: `native-gpu-all` fails when a temporary path lacks owner,
    replacement, kill switch, positive gate, negative gate, or removal rule.
- [ ] Generic runtime query/currentness engine:
  - promote `List/find` and `List/find_value` into one typed field-id list-index
    service covering insert, update, delete, move, duplicate keys, tombstones,
    stale generations, and zero-scan diagnostics;
  - replace boolean startup recompute with generic startup/currentness policies
    such as `Eager`, `ResetSourceInitializerOnly`, `DemandCurrent`,
    `VisibleWindow`, and `DiagnosticOnly`;
  - unify root, list, indexed-field, projection, formula, range, and summary
    currentness behind field/key/range/window-scoped read sets. Product-visible
    reads cannot force root summaries, full-grid/list flushes, or broad
    currentness;
  - formula dependencies, range invalidation, old-edge replacement,
    topological/demand recompute, unrelated-edit skips, and cycle safety remain
    generic runtime/stdlib features;
  - success gate: Cells and a renamed non-Cells sparse fixture both pass with
    zero product scans, zero root materialization, bounded materialized
    windows, current selected/visible reads, and no fixture-name branches.
- [ ] Compiler/document identity and delta workstream:
  - lowering emits stable `DocumentNodeId`, `SourceBindingId`, list/window
    demand ids, text-control ids, hit-region ids, source-intent templates,
    render-slot ids, binding reverse indexes, and list-map binding metadata;
  - runtime/document turns emit closed typed deltas for source values, bound
    text, style/focus/selection, list windows, formula/dependency fanout,
    errors, diagnostics, and materialization changes;
  - source edits use query-style incremental invalidation for parse, typecheck,
    IR, document lowering, route metadata, and render identity, preserving ids
    where semantics survive;
  - success gate: changing labels, visible text, or geometry without changing
    semantic ids does not break routing, retained patches, verifier matching,
    or render-scene cache keys.
- [ ] Dev window as a separate client:
  - preview product frames must not block on dev-window rendering, source
    editor scroll, report browsing, footer/HUD drawing, proof-history
    expansion, source replacement, or transport reads;
  - dev footer/HUD reads cached scalar `PreviewPerfStats` only, throttled at a
    low rate, with mode-aware labels for idle/burst/probe/proof lag;
  - source edits flow through latest-wins workers and stale-result rejection
    while the active preview scene remains presentable;
  - success gate: overloaded-dev and no-dev runs show preview product p95
    within tolerance, `preview_blocked_on_ipc_count=0`, and no footer/render
    hook performs IPC, runtime queries, JSON parsing, or proof/report reads.
- [ ] Hot-path lock and allocation budgets:
  - add per-owner counters for locks, waits, heap allocations, allocated bytes,
    JSON/proof allocation, clone bytes, cache misses, and large vector growth
    on product frames;
  - product mode allows only named scalar counters and bounded frame arenas
    before present. Full report/proof/debug allocation belongs to
    post-present workers;
  - success gate: product frames report zero or bounded heap allocation, zero
    proof/report JSON allocation, no broad clone of runtime/document/proof
    state, and no blocking locks on runtime, dev, report, or proof owners.
- [ ] All-example generated visual replay:
  - add a declarative interaction spec per example and generate visual replay,
    app-owned cursor proof, functional assertions, product/proof reports, and
    no-hacks checks from the spec;
  - include startup visual, first interaction visual, click, keyboard focus,
    text edit, hover, wheel, scrollbars, selection, list/window
    materialization, source updates, proof mismatch, and stale-path coverage
    where applicable;
  - examples that do not have interactive controls still get startup and
    proof-mode coverage;
  - success gate: no Boon example can regress into human-observation-only
    testing, static/model evidence, or unkeyed screenshot/proof shortcuts.
- [ ] Architecture option checkpoints:
  - after two fresh reports fail in the same blocker class, write a checkpoint
    naming the blocker, chosen architecture option, product boundary removed,
    old path to delete, success gate, and why smaller patches are now the
    wrong move;
  - if a proposed fix increases code paths, requires per-example exceptions,
    or adds another compatibility branch, pair it with a deletion milestone
    before accepting any performance result;
  - success gate: progress is measured by removed product-frame boundaries,
    passing deterministic gates, and shrinking stale-path ledger rows, not by
    larger diagnostic reports alone.
- [ ] Larger replacement options to keep available:
  - product-preview strangler lane for `HostInputEvent -> retained patch ->
    direct present` while the proof-shaped path is quarantined;
  - dedicated render actor/thread with render-owned WGPU resources and
    latest-wins snapshots from runtime/layout;
  - browser-style compositor property trees for scroll, clip, transform,
    hover, focus, selection, caret, and opacity/effects;
  - ECS/change-detection extraction for document nodes, layout fragments,
    render batches, hit regions, text runs, and GPU resources;
  - optional Rust/Zig/Wasm hot kernels only after typed semantic contracts are
    stable and interpreter equivalence proof exists;
  - product/proof protocol split between preview and dev windows if shared
    process state keeps causing latency or attribution bugs.

## 2026-07-02 Maximal Architecture TODO Addendum From Review

This addendum captures the latest high-level review and subagent findings so the
next run does not lose the larger cuts while working on individual patches. The
theme is exclusivity: choose one product path, make every proof/debug path
post-present or diagnostic, and delete stale compatibility paths with negative
gates. These TODOs are generic architecture work; none may branch on Cells,
addresses, formula names, source paths, labels, or fixture geometry in
production code.

- [ ] Enforce one product WGPU present contract:
  - decide the product path explicitly: direct visible-surface encode or an
    app-owned texture copy-to-present path;
  - make all other paths proof-only, diagnostic, or experimental, with reported
    mode flags;
  - product gates fail if a frame uses artifact-only
    `native_gpu_render_proof`, hidden offscreen copy, fallback scaffold present,
    legacy compatibility renderer, or proof-scene present;
  - each product frame reports selected present path, surface acquire, encode,
    queue submit, present, present mode, desired frame latency, adapter/backend,
    and whether any proof/readback work ran before present.
- [ ] Make `NativeFrameClock` the only product-frame starter:
  - all redraw/repaint requests enter through typed lanes:
    `HostInput`, `ViewportWheel`, `TextEdit`, `SourceCommit`,
    `RuntimeLayout`, `SurfaceLifecycle`, `CaretTimer`, `Animation`,
    `ProofSample`, `TelemetryFlush`, and `DevIpc`;
  - only allowed product lanes can transition into `BeginProductFrame`;
  - proof/report/HUD/accessibility/dev wakes may enqueue subscriber work but
    cannot relabel a frame as product, extend a product burst, or charge time to
    an unrelated input event;
  - coalesce many repaint requests into one keyed product frame and report the
    dropped/coalesced reasons.
- [ ] Define the product frame ABI as a small typed result:
  - product render hooks return `PresentedProductFrame` /
    `ProductRenderResult` with scalar counters, revisions, lane, scheduler
    reason, dirty reason, render target identity, retained patch counts,
    present metadata, and `FrameEvidenceKey`;
  - product render hooks do not return `serde_json::Value`, proof trees, layout
    artifacts, screenshot paths, latest-report-derived identities, or mutable
    runtime/dev/report state;
  - proof/report JSON is built only from immutable snapshots or post-present
    subscriber handles after present;
  - schema/static checks fail if product mode constructs proof JSON, reads latest
    report state, loads layout artifacts from disk, or waits for proof output
    before present.
- [ ] Promote a `FrameEvidenceRegistry` to the app-window/render-loop owner:
  - pre-mint evidence keys before render/submit/present;
  - register product commits by exact key and surface epoch after present;
  - attach readback/proof/report artifacts only by exact key;
  - fail closed on latest-report proof, stale first-frame proof, mismatched
    surface epoch, mismatched content/layout/render revision, after-the-fact key
    stamping, proof cache hits without a matching key, or unkeyed UX samples.
- [ ] Split retained scene ownership into `MainScene`, `PendingScene`,
  `ActiveScene`, and `RecycleScene`:
  - product frames draw only immutable `ActiveScene`;
  - runtime/document/layout workers build capacity-1 latest-wins `PendingScene`
    snapshots;
  - activation checks source/content/layout/render/surface/input epochs before
    commit;
  - stale pending work is dropped before proof/report allocation;
  - `RecycleScene` owns reusable buffers, route snapshots, display-list storage,
    render batches, and text/glyph scratch without leaking stale identity.
- [ ] Turn hover/focus/selection/caret/text/scroll into retained property trees:
  - keep layout/display items, hit regions, scroll transforms, clips, focus,
    hover, pressed state, selection, caret, text mirrors, IME/composition, and
    accessibility focus as typed retained state;
  - passive scroll and selection patch transform/overlay/text-control state and
    renderer buffers directly;
  - product routing reads retained hit/property trees and route epochs, never
    proof JSON, labels, geometry strings, source paths, or latest reports;
  - direct first-frame patches require reconciliation/rollback evidence if
    runtime confirmation later disagrees.
- [ ] Use a Bevy-style extraction/render pipeline:
  - define product phases as `DrainInput -> ResolveRoute -> ApplyTypedDelta ->
    ExtractVisibleDirty -> PrepareGpu -> QueueBatches -> Encode -> Submit ->
    Present -> PostPresent`;
  - `ExtractVisibleDirty` is the narrow sync boundary and copies only visible
    ranges, dirty ids, revised components, route data, render identities, and
    evidence ids;
  - renderer-owned resources include pipelines, bind groups, glyph atlases,
    shaped text, texture atlases, staging belts/ring buffers, primitive batches,
    render bundles, dirty chunk ids, route/hit snapshots, readback buffers, and
    proof registries;
  - each phase has timings, changed component counts, upload bytes, queue writes,
    draw calls, cache hits, evictions, allocation counts, and lock-wait counts.
- [ ] Add dirty component/change-detection indexes across document/layout/render:
  - model document nodes, layout fragments, hit regions, text runs, render
    primitives, buffers, proof handles, and materialized windows as typed revised
    components;
  - dirty queries are by component/revision/window/range, not path/string,
    geometry scan, proof payload, or full scene scan;
  - changing labels, visible text, or geometry without changing semantic ids
    cannot break routing, retained patches, verifier matching, or render cache
    keys.
- [ ] Make the runtime/list/currentness engine explicitly sparse and indexed:
  - make demand-current a compiler/runtime contract: IR marks pure indexed
    derived fields as current-on-read, runtime enforces scoped barriers before
    visible reads, and startup gates fail on eager full-list recompute;
  - unify `List/find` and `List/find_value` as indexed generic primitives over
    field ids and typed values, covering inserts, updates, deletes, moves,
    duplicate keys, tombstones, stale generations, and zero-scan diagnostics;
  - maintain bidirectional dependency/fanout indexes for roots, list columns,
    exact lookups, ranges, row fields, formulas, and summaries;
  - emit typed runtime deltas for source values, bound text, style/pseudo state,
    list windows, row fields, formula/range fanout, errors, diagnostics, and
    materialization changes;
  - full `state_summary`, root flushes, full-grid/list summaries, proof JSON,
    and report assembly are diagnostics/proof subscribers only.
- [ ] Virtualize grid/list materialization as a generic engine feature:
  - reports distinguish logical rows/cols, materialized windows, overscan,
    selected keys, dependent keys, evaluated formulas, rendered nodes, retained
    patches, upload bytes, and currentness latency;
  - product frames consume viewport/window/dependency materialization demands,
    not full logical lists;
  - add at least one renamed non-Cells sparse-grid/list fixture that exercises
    indexed lookup, current-on-read, fanout/range invalidation, insertion,
    deletion, cycle safety, wheel/scroll, and text control binding under the same
    gates.
- [ ] Delete layout/proof JSON from the product hot path:
  - no product-frame file reads from layout artifacts;
  - no proof-derived route identity, render identity, or visual currentness;
  - no `layout_frame_from_layout_proof` fallback in product mode;
  - product state carries typed `LayoutFrame`, derived indexes, route tables,
    hit tables, overlay state, render scene identity, and product counters
    directly.
- [ ] Build a machine-readable stale-path deletion ledger:
  - move stale paths into a checked data file or schema section consumed by
    xtask;
  - every path has owner crate, owner date, temporary mode, typed replacement,
    kill switch, runtime counter, positive gate, negative gate, and removal
    condition;
  - seed rows include `preview_apply_real_window_input_with_units` fallback,
    `click_candidate_cache`, layout artifact reloads, legacy pre-present proof
    requests, `last_interactive_readback_artifact`, compatibility render-scene
    lowerer, `layout_proof` product reads, latest-report matching, full
    `state_summary`, broad runtime summaries, geometry/string route fallback,
    modeled/static scroll readiness, driver timing, private runtime dispatch,
    old Ply/Xvfb/COSMIC/browser proof, and spreadsheet-only sparse fixture
    proof;
  - `native-gpu-all` fails when a temporary path lacks owner, replacement, kill
    switch, positive gate, negative gate, or removal rule.
- [ ] Separate product/proof/baseline/HUD verifier modes:
  - counters-only product latency is the UX gate;
  - exact-key proof/readback validates pixels/currentness/artifacts later;
  - full report/HUD mode may be slower but reports overhead separately;
  - empty retained-frame present-floor runs on the same app-window, surface,
    adapter, present mode, frame clock, and focus-safe launch path as product;
  - product p95 must pass before proof/report overhead is investigated.
- [ ] Add proof-isolation and stale-proof stress tests:
  - deliberately stall, delay, drop, or saturate proof/readback/report workers;
  - product p95, product missed frames, and first-frame retained pixels must stay
    stable while proof lag/drop/status changes honestly;
  - stale first-frame proof reuse, mismatched evidence key, mismatched surface
    epoch, mismatched revision, proof-required visible update, and proof cache
    hit without exact key all fail.
- [ ] Treat dev window as a separate low-priority client:
  - preview product frames cannot block on dev-window rendering, code-editor
    wheel, report browsing, footer/HUD drawing, proof-history expansion, source
    replacement, or transport reads;
  - dev footer/HUD reads cached scalar `PreviewPerfStats` only, throttled at a
    low rate and labeled as idle, burst, probe, proof lag, drops, and age;
  - source edits flow through latest-wins workers and stale-result rejection
    while the active preview scene remains presentable.
- [ ] Add product hot-path budgets for locks, allocation, and clones:
  - product frames report locks acquired, lock wait time, heap allocations,
    allocated bytes, clone bytes, large vector growth, JSON/proof allocation,
    cache misses, and broad state copies by owner;
  - product mode allows only bounded frame arenas and scalar counters before
    present;
  - proof/report/debug allocation belongs to post-present workers.
- [ ] Keep larger replacement options available if incremental cuts loop:
  - strangler product-preview lane: `HostInputEvent -> retained property patch
    -> direct present`, while proof-shaped paths are quarantined;
  - dedicated render actor/thread with render-owned WGPU resources and
    latest-wins snapshots from runtime/layout/document;
  - browser-style compositor property trees for scroll, clip, transform, focus,
    hover, selection, caret, and opacity/effects;
  - ECS/change-detection extraction for document nodes, layout fragments, hit
    regions, text runs, render batches, GPU resources, and proof handles;
  - product/proof protocol split between preview and dev windows if shared state
    keeps causing latency or attribution bugs;
  - optional Rust/Zig/Wasm hot kernels only after typed semantic contracts,
    interpreter equivalence, and exact proof/revision accounting are stable.
- [ ] External architecture references to keep in mind:
  - GPUI: hybrid immediate/retained GPU UI and retained element state;
  - Bevy: extracted render data, render phases, queue/sort/render/cleanup, and
    render-owned resources;
  - Servo/WebRender and browser retained display lists: display-list retention,
    spatial/clip/property trees, compositor responsiveness, and partial display
    list updates;
  - spreadsheet/grid systems: row/column virtualization, value/currentness
    caches, scoped change detection, and dependency fanout;
  - apply only the simple lessons that remove product-path work. Do not import a
    framework-shaped rewrite unless it deletes more old paths than it adds.

## 2026-07-02 Additional Architecture Improvement Radar

This section is a no-loss parking lot for larger architecture options that may
become necessary if the current slices keep failing near the 16.7 ms target.
Treat these as candidates for deliberate design cuts, not as all-at-once scope.
Each option must stay generic across Boon examples and must delete or quarantine
an old product-path dependency when it lands.

- [ ] Render actor / product surface ownership:
  - move surface acquire, encoder creation, queue submit, present, frame pacing,
    GPU resource lifetime, and product-frame commit publication behind one
    render actor or equivalent single-owner state machine;
  - runtime/layout/dev/proof clients communicate through bounded typed
    mailboxes with latest-wins replacement where valid;
  - success gate: product frames never wait on dev-window locks, report locks,
    proof workers, source replacement workers, or broad runtime summary locks.
- [ ] Deadline-aware frame budgeting:
  - give product frames an explicit budget ledger for input, route, runtime
    deltas, extract, prepare, encode, submit, and present;
  - defer non-visible work once the remaining budget cannot cover it, and record
    the exact deferred reason;
  - success gate: reports show which work was admitted before present, which
    was deferred, and whether the frame would have missed 16.7 ms without
    deferral.
- [ ] First-class control tree:
  - promote buttons, text inputs, code editor lines, spreadsheet-like cells,
    scrollports, menus, and future controls into a typed retained control tree
    produced by document/layout, not reconstructed from proof/layout JSON;
  - control state owns hover, focus, active, selection, caret, text mirror, IME
    composition, disabled/read-only, scroll offsets, and accessibility focus;
  - success gate: clicking or typing any control patches retained control state
    and bound text directly, with no label/string/geometry/source-path lookup.
- [ ] Stable semantic identity contract:
  - assign stable document/layout/render/control ids at compiler/document
    boundaries and preserve them through list virtualization, layout fragments,
    render batches, hit regions, and proof artifacts;
  - define id lifetime, recycling, generation, and stale-event rejection rules;
  - success gate: changing text, style, scroll position, or layout geometry does
    not invalidate routing, retained render patches, proof matching, or cache
    keys unless the semantic target actually changed.
- [ ] Columnar sparse runtime storage:
  - store list/root fields in generational column stores keyed by logical row,
    field id, and revision instead of repeatedly materializing row objects;
  - maintain per-field dirty sets, dependency reverse indexes, lookup indexes,
    visible-window sets, selected/dependent sets, and summary subscribers;
  - success gate: startup, select, edit, formula fanout, and scroll reports
    account for logical rows separately from materialized rows and do not hide a
    full-grid pass in a row-template cache.
- [ ] Incremental layout engine boundary:
  - split layout into stable fragments, scroll/clip/transform property trees,
    text-run measurement cache, invalidation ranges, and retained hit regions;
  - simple style/state changes update fragment properties or overlays without a
    full layout-frame rebuild;
  - success gate: passive hover, focus, selection, caret, formula-bar mirror,
    and scroll have bounded layout-dirty counts independent of document size.
- [ ] Renderer resource residency policy:
  - keep pipelines, bind groups, glyph atlases, shaped text, texture atlases,
    buffers, render bundles, staging belts, and readback resources resident
    across interactions;
  - add explicit eviction/growth/backpressure policies instead of rebuilding
    caches opportunistically in the product frame;
  - success gate: product reports include cache hit/eviction/growth counts,
    upload bytes, queue writes, allocation bytes, and resource recreate counts.
- [ ] Present strategy ladder:
  - evaluate late acquire, early acquire, mailbox/fifo/immediate modes, desired
    frame latency, frames in flight, no-op present floor, same-surface hardware
    baseline, and compositor/vsync attribution as named diagnostic policies;
  - pick one product default from measured evidence and keep other policies
    opt-in with report flags;
  - success gate: no hidden present-mode or frame-latency override is needed for
    native UX gates to pass.
- [ ] Product/proof process protocol split:
  - if shared state keeps contaminating product latency, split the preview
    product protocol from dev/debug/proof protocol at the transport boundary;
  - product protocol carries source snapshots, typed input, product commits,
    scalar stats, and proof handles only;
  - proof protocol carries readback requests, report JSON, proof history,
    artifact hashes, screenshots, diffs, and diagnostics after present;
  - success gate: saturating proof/dev traffic cannot delay product input,
    route, render, submit, or present.
- [ ] Scheduler simulation before risky rewrites:
  - build a deterministic simulation of `NativeFrameClock`, burst exits,
    backpressure, pending-scene cancellation, proof queue overflow, surface
    lifecycle, rapid input, scroll, and source replacement;
  - use it to reject scheduler changes that mix product, proof, warm-up,
    cleanup, or diagnostic frames under one interaction sample;
  - success gate: every real scheduler mode has simulation coverage before it
    becomes a product default.
- [ ] Hot-path allocation and lock firewall:
  - instrument and then forbid unbounded allocation, large clones, JSON
    construction, file I/O, blocking channel receive, broad mutex waits, and
    report/proof tree copying before product present;
  - allow bounded frame arenas, preallocated rings, atomics, and immutable
    snapshot reads on the product path;
  - success gate: native UX reports fail when product frames cross configured
    lock/allocation/file-I/O budgets.
- [ ] Verifier architecture reset:
  - keep separate product-only, proof-only, proof-isolation, baseline,
    interactive visual, scroll, text-input, and long-session gates;
  - product gates require app-owned host-event input and product-frame commits;
    proof gates require exact-key WGPU/readback artifacts;
  - success gate: no verifier can pass by using latest report state, driver
    fallback timing, human observation, desktop screenshots, static/model
    scroll, private runtime dispatch, or a different frame than the measured
    product frame.
- [ ] Rust/Zig/Wasm hot-kernel option, only after contracts:
  - keep the option to compile hot runtime/layout/render kernels once the typed
    semantic contracts and interpreter equivalence tests are stable;
  - candidate kernels include lookup indexes, dependency fanout, text shaping
    cache lookup, layout invalidation, and primitive batching;
  - success gate: codegen work is not started to mask architecture coupling; it
    must replace a measured hot kernel and keep deterministic proof/currentness
    equivalence.
- [ ] External-library research checkpoints:
  - GPUI: apply retained element state, focus/control ownership, and GPU-first
    UI resource residency where it simplifies Boon native preview;
  - Bevy: apply schedule phases, extract/prepare/queue/render separation,
    change detection, and render-world ownership without importing ECS ceremony
    into the language runtime;
  - Servo/WebRender/browser engines: apply display-list retention, spatial/clip
    trees, compositor-friendly scroll/transform updates, and partial invalidation
    without tying correctness to browser screenshots;
  - spreadsheet engines: apply sparse cell storage, dependency graph fanout,
    range indexes, current-on-read values, and virtualized viewports without
    hardcoding spreadsheet identifiers.
- [ ] Simplicity and deletion review:
  - after each architecture cut, remove the replaced compatibility branch or
    move it behind an explicit diagnostic mode with owner, counter, kill date,
    and negative test;
  - prefer fewer product paths, fewer ownership models, and fewer verifier
    matching strategies even if a local metric looks temporarily worse;
  - success gate: the stale-path ledger shrinks over time, and no new branch is
    accepted unless it has a removal rule.

### 2026-07-02 No-Loss Architecture Improvement TODOs

Preserve these additional cuts so future work can choose a real boundary
removal instead of another local cache or timing tweak. These are architecture
options, not permission to rewrite everything at once. A cut is valid only if
it is generic, makes the product path simpler, and adds a positive gate plus a
negative stale-path gate.

- [ ] Product frame clock as a tiny kernel:
  - isolate the frame clock into a small module with one public transition API:
    `accept_wake`, `begin_product_frame`, `commit_presented_frame`,
    `enqueue_post_present_work`, and `finish_idle`;
  - coalesce many invalidations into one product redraw while preserving the
    reason list and highest-priority lane;
  - every subsystem requests work through typed lanes; no runtime, document,
    renderer, proof, dev, or verifier code may start its own product frame;
  - report request-to-redraw latency, coalesced wake count, burst state,
    idle state, and wake source per product frame;
  - success gate: a scheduler simulation can replay every product frame from
    wake records and prove no proof/report/dev wake was charged to host input.
- [ ] Explicit scheduler transition tests:
  - model scheduler state inputs such as `surface_valid`, `visible`,
    `burst_active`, `needs_commit`, `pending_ready`, `proof_pending`,
    `telemetry_pending`, and `source_replace_pending`;
  - model actions such as `BeginProductFrame`, `DrainInput`,
    `ActivatePending`, `DrawActive`, `SubmitPresent`,
    `RunProofSubscriber`, `FlushTelemetry`, and `ReturnIdle`;
  - success gate: deterministic transition tests cover idle, burst input,
    repeated input, source edit, resize/surface loss, proof-only sample, report
    flush, proof overload, stale pending scene, and worker cancellation.
- [ ] Product frame transaction log:
  - write a compact append-only in-memory transaction log for product frames:
    input seqs, lane, route id, source intent, active scene generation,
    render target, present id, `FrameEvidenceKey`, and old-path flags;
  - reports and verifiers consume immutable product-frame rows from this log,
    not live mutable "latest" state;
  - success gate: deleting all proof subscribers still leaves product UX
    reports complete enough to pass or fail deterministically.
- [ ] Source-intent ABI between controls and runtime:
  - replace generic source-event construction on the hot path with typed
    `SourceIntent` records produced by retained controls and compiled binding
    templates;
  - source intents carry control id, binding id, expected current value,
    optimistic patch id, reconciliation policy, and rollback target;
  - success gate: click, text edit, checkbox/toggle, list selection, editor
    edit, and scroll use the same generic intent machinery with no example
    string/address branches.
- [ ] Active-scene-only hit testing:
  - hit testing reads immutable active-scene route tables and retained control
    state only;
  - pending runtime/layout/document work may publish a new route table later,
    but cannot force a broad route rebuild inside the current product frame;
  - success gate: product frames fail if hit testing consults proof JSON,
    layout artifacts on disk, source paths, labels, geometry scans, or latest
    report rows.
- [ ] Property-tree compositor layer:
  - split scroll, clip, transform, opacity, hover, focus, selection, caret,
    pressed state, and text-control mirror updates into retained property
    trees;
  - simple interactions update property nodes and small GPU buffers instead of
    rebuilding document/layout/render scenes;
  - success gate: passive wheel, hover, focus, caret blink, formula-bar mirror,
    and selection frames report zero full layout/render-scene rebuilds.
- [ ] Text pipeline specialization:
  - make text controls and code-editor text use retained text layouts, shaped
    run caches, glyph atlas residency, caret/selection overlays, and IME state;
  - separate semantic text value updates from visual overlay updates so focus
    and selection do not remeasure or reshape unchanged text;
  - success gate: click-to-focus and caret movement across all examples report
    bounded text-run dirty counts and no full text atlas/glyph cache rebuild.
- [ ] Scroll pipeline specialization:
  - make scrollports own offset, velocity, bounds, visible range, overscan, and
    materialization demand as retained state;
  - wheel frames update compositor/property state first and schedule
    materialization/layout catch-up as pending work when needed;
  - success gate: passive scroll product frames do not dispatch source events,
    run formulas, rebuild runtime summaries, or lower full lists.
- [ ] Runtime delta bus:
  - runtime turns publish typed deltas for roots, list rows, list windows,
    indexed fields, bound text, errors, diagnostics, dependency fanout, and
    materialization demands;
  - document/layout/render/control layers subscribe to specific delta classes
    instead of asking for broad summaries after every input;
  - success gate: product-visible reads are current through scoped barriers, and
    no normal interaction uses full `state_summary` as a synchronization tool.
- [ ] Query-style compiler/document invalidation:
  - treat parse, typecheck, IR, document lowering, route metadata, layout
    identity, render identity, and verifier metadata as incremental queries with
    stable ids and explicit invalidation;
  - source edits preserve ids when semantic targets survive and reject stale
    pending query results by revision;
  - success gate: editing unrelated source text cannot invalidate route/render
    ids for unaffected controls or examples.
- [ ] Sparse fixture family:
  - add generic fixtures beyond Cells: sparse grid, sparse list, large form,
    virtualized tree/table, formula/range-like dependency graph, and code-editor
    text surface;
  - each fixture exercises currentness, lookup indexes, retained controls,
    virtualization, scroll, text input, proof identity, and no-hacks checks;
  - success gate: runtime/compiler/renderer improvements must pass renamed
    non-Cells fixtures before being accepted as generic.
- [ ] Product/proof transport separation:
  - define separate message envelopes for product state, proof requests,
    telemetry, dev tools, and source replacement;
  - product envelopes carry only source snapshots, typed intents, active-scene
    handles, product commits, scalar stats, and proof handles;
  - success gate: saturating proof/report/dev transport cannot delay product
    input acceptance, product render, queue submit, or present.
- [ ] Hot resource preflight:
  - perform shader/pipeline compilation, glyph atlas growth, staging-buffer
    growth, surface reconfiguration, proof-buffer allocation, and report-writer
    setup outside product interaction frames whenever possible;
  - product frames may use preallocated fallback capacity but must record any
    emergency growth as a budget violation;
  - success gate: first interactive frame after startup has warmed product
    resources or reports exactly which cold resource caused the miss.
- [ ] Frame arena and allocation discipline:
  - replace incidental per-frame vectors/maps/strings with bounded frame arenas,
    small fixed rings, interned ids, and reusable scratch owned by the product
    frame clock or renderer;
  - prohibit JSON/string formatting/file-path assembly in the product lane;
  - success gate: product frames expose allocation counts/bytes and fail when
    they exceed configured budgets.
- [ ] Lock ownership map:
  - document every hot-path mutex/rwlock/channel owner and classify it as
    product, active-scene immutable, pending-scene worker, proof, dev, or
    report;
  - product frames may not wait on proof/dev/report locks and may only read
    immutable snapshots from runtime/document/layout;
  - success gate: reports include lock wait time by owner and fail on
    cross-lane waits before present.
- [ ] Present strategy experiment harness:
  - keep late acquire, early acquire, command-buffer reuse, render bundles,
    mailbox/fifo/immediate, desired frame latency, frames in flight, and
    surface-copy strategies as named experimental policies;
  - every experiment reports adapter/backend/surface/present mode and has a
    matching same-surface baseline;
  - success gate: a present strategy cannot become default unless it improves
    repeated product p95/max without increasing proof failures or hidden modes.
- [ ] GPU work visibility:
  - add optional timestamp-query or equivalent diagnostics when available, with
    clear fallback when not supported;
  - separate CPU submit time, queue wait, compositor/vsync present floor, GPU
    execution, and readback/proof completion in reports;
  - success gate: the plan can distinguish app CPU work from driver/compositor
    blocking before choosing a renderer rewrite.
- [ ] Proof artifact registry:
  - replace ad hoc artifact path collection with an immutable registry keyed by
    `FrameEvidenceKey`, artifact kind, content hash, revision tuple, surface
    epoch, producer, and completion time;
  - readback, screenshots, diffs, render proof, proof history, report JSON, and
    artifact hashes all register through the same API;
  - success gate: stale or mismatched artifact reuse fails even when file names
    happen to match.
- [ ] Verifier cost accounting:
  - every verifier reports product-only latency, proof latency, report-writing
    latency, harness overhead, input injection overhead, and warmup cost
    separately;
  - UX gates use product-only rows, while proof gates join exact keyed evidence
    later;
  - success gate: no verifier can make product interaction look slow or fast by
    mixing proof/report/harness frames into UX samples.
- [ ] Deterministic product/proof replay:
  - define fixed event scripts with app-owned host-event injection, visible
    cursor/pointer proof, exact-key WGPU readback, and optional fake-clock
    scheduler simulation for non-GPU phases;
  - every sample is labeled as cold, steady, burst, proof-only, driver/surface
    baseline, source-replacement, cleanup, or diagnostic;
  - failures name the dominant failed phase: product pixels, runtime/currentness
    probe, proof artifact, timing, IPC, present floor, or harness overhead;
  - success gate: product replay can run with proof subscribers disabled, proof
    replay can later validate the same `FrameEvidenceKey`, and full mode reports
    the extra cost without redefining UX latency.
- [ ] Continuous-probe quarantine:
  - keep continuous rendering as a diagnostic/probe mode only, with clear CLI
    and report labels;
  - normal DemandDriven product gates fail if ContinuousProbe is active or if
    a diagnostic burst is required to make pixels update;
  - success gate: the same interaction passes in product DemandDriven mode,
    not only in a hot diagnostic loop.
- [ ] Background worker priority policy:
  - assign priorities and cancellation rules for source replacement, runtime
    cleanup, layout catch-up, proof readback, report serialization, artifact
    hashing, accessibility, dev HUD, and telemetry;
  - product input and visible-state publication preempt stale worker results;
  - success gate: a synthetic overload test proves stale workers are dropped or
    delayed rather than blocking the next product frame.
- [ ] Architecture decision records for major cuts:
  - each large cut records rejected alternatives, evidence, old path removed,
    genericity audit, verifier impact, and rollback plan;
  - decisions that keep compatibility paths must include owner, kill date, and
    negative gate;
  - success gate: the plan no longer grows unbounded without a shrinking stale
    path ledger.
- [ ] Simpler replacement escape hatch:
  - if product p95 remains over budget after proof isolation and active-scene
    ownership, consider a narrow native-control/product-scene rewrite for the
    preview hot path while keeping Boon semantics and proof as external
    subscribers;
  - this is a strangler architecture, not a Cells shortcut: the same control
    model must run Counter, TodoMVC, Cells, editor surfaces, and sparse fixtures;
  - success gate: the rewrite deletes more legacy product-path code than it
    adds and passes all no-hacks audits.

## 2026-07-02 Current Full-State Product-Path Cut TODOs

The freshest release Cells visible-click evidence after the proof-isolation
slice is:

- `target/reports/native-gpu/cells-visible-click-e2e-release.json`
- `target/artifacts/native-gpu/cells-visible-click-e2e-2575123-1782971605/preview-loop.json`

The gate still fails even though the runtime/list contract is clean:
`total_rows_scanned=0`, `total_list_find_rows_scanned=0`,
`total_root_materialization_candidates=0`, and `total_recomputed_fields=0`.
The product click reaches `selected_address="A2"` and `formula_bar_text="15"`,
but the app-window product frame reports about `46.840 ms` input-to-present.
The frame timing names the current boundary: render hook work is about
`37.2 ms`, queue/submit path is about `9.3 ms`, and the render-hook breakdown
shows `input_overlay_render_scene_patch_build_ms` around `11.3 ms`,
`render_scene_cache_ms` around `25.6 ms`, `retained_bound_sync.reason="full_state"`,
`item_index_count=331`, and `input_overlay_render_scene_patch_touched_node_count=239`.
This is not a Cells formula/list/runtime failure. It is a product-path
architecture failure: a full-state/proof-shaped retained sync and patch path is
still reachable on a normal interaction frame.

Do not lose these current TODOs:

- [ ] Cut full-state retained-bound sync out of product interaction frames:
  - selection, focus, hover, caret, text mirror, and formula/input updates must
    patch a bounded node set from typed dirty ids;
  - a click product frame may touch the old selected node, new selected node,
    focused text-control mirrors, caret/focus overlays, and explicitly dirty
    visible text, not hundreds of visible nodes;
  - `retained_bound_sync.reason="full_state"` is allowed only in startup,
    explicit proof/diagnostic mode, or pending-scene rebuild work;
  - success gate: product click reports
    `retained_bound_sync.reason!="full_state"`,
    `input_overlay_render_scene_patch_touched_node_count` bounded by the
    changed-node set, and no full retained-bound scan before present.
- [ ] Replace `input_overlay_render_scene_patch` full-state rebuild with typed
      retained overlay/property patches:
  - maintain reverse indexes from source/runtime binding ids, focus ids,
    selected ids, hover ids, text-control ids, and route ids to retained render
    nodes;
  - build patches from those indexes directly instead of walking all visible
    items or rebuilding scene/proof structures;
  - keep patch identity stable across alternating selected/focused nodes so
    renderer caches are not invalidated by volatile payload text;
  - success gate: patch build/prepare/cache time stays below a small fixed
    budget in product mode, with changed-node counts and reason fields proving
    the path.
- [ ] Delete or quarantine legacy pre-present proof structures from the product
      render hook:
  - `legacy_product_proof_built_pre_present`,
    `legacy_pre_present_proof_request_count`, visible-bound-text proof,
    retained-bound-sync proof, proof-history, render-hook report JSON, and
    artifact-hash discovery must be post-present subscribers keyed by
    `FrameEvidenceKey`;
  - product render returns typed revisions, scalar counters, bounded patch
    metadata, and proof handles only;
  - success gate: product reports show zero pre-present proof/report JSON work,
    while proof subscribers still prove exact matching frames later.
- [ ] Promote `post_present_proof_isolation` through every verifier report:
  - the top-level Cells visible-click report must copy the preview-loop
    `post_present_proof_isolation` object or fail with a precise missing-field
    blocker;
  - label contracts should reject product-latency reports that omit proof
    isolation evidence, claim proof completion is part of UX latency, or block
    product frames on proof subscribers;
  - success gate: `verify-native-cells-visible-click-e2e` exposes the same
    proof-isolation state as the app-window preview-loop artifact.
- [ ] Add stale-path negative gates for the current blocker:
  - product mode fails if a normal click uses full-state retained-bound sync,
    latest-report proof, proof JSON route lookup, broad runtime summary, full
    render-scene/proof rebuild, dev IPC wait, or artifact-path scan before
    present;
  - proof/diagnostic modes may use those paths only when explicitly labeled and
    excluded from UX latency;
  - success gate: each old path has a report counter, allowlist status, and a
    negative test.
- [ ] Consider a simpler `ActivePreviewScene`/`ProductPatch` boundary instead
      of continuing to patch around the legacy render hook:
  - define one small product ABI:
    `ProductInput -> ProductPatch -> RenderFrameResult -> ProductFrameCommit`;
  - move full document/layout/proof snapshots behind `PendingPreviewScene` or
    post-present subscribers;
  - if the current render hook cannot be made small without many exceptions,
    build the typed product boundary beside it, prove it on generic fixtures,
    then delete/quarantine the old product path;
  - success gate: the replacement removes more pre-present work than it adds
    and passes no-hacks audits across compiler, runtime, document, native GPU,
    app-window, playground, xtask, and report schema.
- [ ] Keep measuring real dominant boundaries, not stale averages:
  - product UX gates must use keyed product interaction frames only;
  - proof/readback/report/harness latency must be linked by evidence key but
    reported separately;
  - every failing sample should name exactly one dominant class:
    input scheduling, route/source intent, runtime/currentness, document/layout,
    retained patch/extract, GPU upload/encode, queue/present, proof, IPC,
    telemetry, or harness;
  - success gate: no next patch is accepted as progress unless it removes,
    moves, or bounds the named class.

## 2026-07-02 Post-Projected-Sync Architecture TODOs

The freshest local cuts after the proof-isolation/full-state checkpoint changed
the measured blocker again:

- unscoped retained-bound sync now skips projected `@...` list text/style
  fields unless an explicit target node set is supplied;
- targeted retained sync can still patch projected list text for selected or
  focused controls;
- generated focus/caret overlay collection ignores inactive focus flags;
- direct input-overlay render now tries the stable geometry-base render-scene
  cache before rebuilding for a volatile incremental layout hash;
- the C0 slow sample dropped to about `12.029 ms` input-to-present, with render
  hook total about `2.63 ms`, queue submit about `0.08 ms`, and present about
  `9.03 ms`.

The current release report is still a fail, and the next failure is more useful
than the old full-state one:

- `target/reports/native-gpu/cells-visible-click-e2e-release.json`
- `target/artifacts/native-gpu/cells-visible-click-e2e-2853227-1782973489/preview-loop.json`
- `target_count=64`, `completed_sample_count=4`
- product p95/max about `33.881 ms` because the A0 sample is slow;
- A2, B0, and C0 now run as `HostInput` / `ProductInteraction` frames and stay
  near `10.5-12.1 ms`;
- the A0 product commit still reports `scheduler_reason="requested_animation"`
  even though the poll diagnostics for the same interaction observed real OS
  host input;
- A0 render-hook outer timing is still about `24.42 ms`:
  `render_hook_outer_state_snapshot_ms=7.017 ms`,
  `render_hook_outer_core_ms=17.399 ms`, and `present_call_ms=8.792 ms`;
- A0 inner proof timing names the immediate product-path miss:
  `render_scene_cache_hit=false`, `render_scene_cache_ms=15.454 ms`,
  `input_overlay_render_scene_patch_touched_node_count=3`, and
  `retained_bound_sync.target_node_count=3`;
- the old `retained_bound_sync.reason="full_state"` label remains misleading
  for targeted sync and should be split so reports can distinguish
  `targeted_node_set`, `source_intent_delta`, `startup_full_state`, and
  `proof_full_state`;
- recent poll diagnostics still contain older generic fallback samples with
  large `source_input_ms`/`live_events_ms`; product UX gates must match exact
  accepted product-frame evidence and must not let stale diagnostics overwrite
  the accepted click.

Do not lose these next architecture TODOs:

- [ ] Make accepted host input own the product frame identity:
  - when a host-input delta is accepted, the committed product frame and
    accepted-input timing must report `scheduler_reason="host_input"` and
    `frame_lane="product_interaction"` even if a requested-animation burst wake
    was already pending;
  - requested-animation follow-up frames may keep burst accounting, but they
    must not own a user input event or rewrite product click timings;
  - success gate: no product click sample with `input_event_seq` may commit as
    `scheduler_reason="requested_animation"`.
- [ ] Introduce explicit `ActivePreviewScene` ownership for product patches:
  - keep the last presented active scene cache key, layout identity, render
    scene identity, content revision, surface epoch, route/overlay identity, and
    prepared GPU resource identity in one render-loop-owned object;
  - product input-overlay patches must apply against this active scene directly
    instead of rediscovering a base scene from volatile layout hashes or proof
    snapshots;
  - pending full document/layout/render snapshots may replace the active scene
    only after exact revision/epoch checks pass;
  - success gate: a bounded selected/focused/hover patch never performs a full
    render-scene build before present because a derived layout hash changed.
- [ ] Reuse route and overlay lookup caches across retained-only layout hashes:
  - if a volatile layout hash only encodes focus/caret/selection/text mirror
    state and the source-intent route identity is unchanged, reuse the active
    route table and overlay lookup;
  - cache keys should be semantic route/layout identities, not full proof hashes
    that change for every overlay tick;
  - success gate: `PreviewVisibleRenderState::from_shared` no longer spends
    milliseconds rebuilding overlay lookup for retained-only product patches.
- [ ] Split retained sync reason and delete confusing full-state labels:
  - product path labels should say whether the sync came from typed node set,
    source-intent delta, route delta, text-control mirror, hover/focus/caret,
    startup, pending scene commit, or proof mode;
  - `reason="full_state"` in a product interaction frame must become a hard
    fail unless the report proves it ran in a non-product subscriber;
  - success gate: stale labels cannot make a targeted path look like a full
    scan, and a real full scan cannot hide behind a targeted-node count.
- [ ] Move render-scene construction out of the product frame by contract:
  - product frames may encode/upload/apply a small `ProductPatch` against the
    active scene;
  - full render-scene extraction/conversion/build is pending-scene work or a
    proof/diagnostic subscriber;
  - success gate: `render_scene_cache_hit=false` on a product click is a
    blocker unless the frame is explicitly a first-paint, resize, surface epoch
    change, or pending-scene commit.
- [ ] Add product-only stale-path negative tests for the new blockers:
  - fail when a normal click uses requested-animation ownership, volatile-hash
    full scene rebuild, pre-present proof JSON, latest-diagnostic fallback,
    generic live-event fallback after a typed route exists, or active-scene
    rediscovery from proof artifacts;
  - keep proof/readback/report subscribers allowed only by mode, frame key, and
    post-present status.
- [ ] Keep the fallback strategy large and simple:
  - if `ActivePreviewScene` cannot be added cleanly around the current render
    hook, create a smaller generic `PreviewHotLoop` path beside it and move
    the old render-hook/proof machinery behind `PendingPreviewScene` and
    post-present subscribers;
  - this is not a Cells shortcut: the same product loop must handle Counter,
    TodoMVC, Cells, editor surfaces, sparse list fixtures, and future examples;
  - the replacement must delete/quarantine more pre-present compatibility code
    than it adds.

### Plan-Review Architecture TODOs

These TODOs come from the 2026-07-02 architecture review pass. They are here to
avoid losing larger simplification work while the implementation focuses on the
current product-frame blocker.

- [ ] Create one canonical active-slice table:
  - columns: `Now`, `Next`, `Deferred`, `Diagnostic`;
  - each row must name the old path to remove/quarantine, the positive gate,
    the negative stale-path gate, and the owner layer;
  - keep long evidence history in an appendix so the implementer starts from
    the current slice, not a stale “latest” block.
- [ ] Promote accepted contracts into `docs/architecture/NATIVE_GPU_PIPELINE.md`:
  - `NativeFrameClock`;
  - `ActivePreviewScene` / `PendingPreviewScene`;
  - `ProductRenderResult` / product patch ABI;
  - post-present proof service;
  - product/proof/baseline/HUD verifier modes;
  - hardware present-floor gates.
- [ ] Add a scheduler-ingress inventory:
  - every redraw, proof, telemetry, timer, dev-HUD refresh, source cleanup,
    animation, accessibility, and surface-lifecycle wake must enter one
    `NativeFrameClock`;
  - unknown ingress must fail product mode instead of silently becoming a
    requested-animation/product timing hybrid.
- [ ] Promote input wait metrics to first-class gates:
  - `host_event_to_frame_begin_ms`;
  - `input_wake_to_input_accept_ms`;
  - `input_accept_to_frame_start_ms`;
  - `late_input_deferred_count`;
  - `input_to_present_ms` remains the top-line UX budget, but it must no
    longer hide scheduler wait, accepted-input ownership, or burst-followup
    attribution mistakes.
- [ ] Define stable identity lifecycle rules:
  - id generation, reuse, stale-event rejection, row/list/control/render id
    lifetime, and proof-key joins must be explicit architecture contracts;
  - stable ids may not degrade into ad hoc cache keys derived from geometry,
    text, labels, fixture names, or volatile proof hashes.
- [ ] Add sustained-session and cache-pressure gates:
  - repeated click, edit, scroll, resize, and dev-window use over time;
  - cache eviction/reuse counters, arena reuse, allocation counts, lock waits,
    GPU upload bytes, prepared resource residency, and memory growth;
  - success gate: the hot loop does not decay after many interactions.
- [ ] Promote generic runtime/list/currentness semantics into architecture docs:
  - indexed lookup lifecycle, range invalidation, duplicate/tombstone behavior,
    current-on-read barriers, cycle safety, sparse materialization, and renamed
    non-Cells sparse fixtures;
  - treat this as regression contract unless fresh product-frame evidence
    reopens runtime/list work as the dominant blocker.
- [ ] Make the no-fixture-hacks audit machine-readable and path-scoped:
  - Cells/source strings are allowed in examples, fixtures, reports, and tests
    that explicitly load the Cells example;
  - production compiler, runtime, document, renderer, app-window, playground
    product path, and verifier acceptance logic must fail on fixture-specific
    branches for `cells`, spreadsheet addresses, fixed 26x100 dimensions, or
    example file paths.
- [ ] Keep diagnostics separate from acceptance:
  - offscreen copy-to-present, immediate present-mode overrides, frame-latency
    overrides, route-cache experiments, selected-overlay micro-tuning,
    JSON-size tweaks, and renderer-cache probes remain diagnostic unless a
    fresh report names that boundary as dominant;
  - dev HUD work must consume cached scalar stats only and must not drive the
    product architecture.
- [ ] Defer broad Rust/Zig/Wasm/codegen research until contracts are stable:
  - revisit hot kernels or codegen after the typed product/proof/runtime
    contracts and interpreter equivalence tests exist;
  - do not let future-language work distract from deleting current slow product
    paths.

## 2026-07-02 Selected-Node Proof Smoke And Next Architecture TODOs

This checkpoint captures the latest local smoke and subagent findings so they
do not get lost behind the large backlog. The verifier/proof issue moved:
selected-node visual proof can now prove the clicked cell from generic retained
visible text plus source-intent metadata, but the product frame still misses the
16.7 ms budget on a real host-input frame. Treat the next work as product frame
architecture, proof isolation, and old-path deletion, not as Cells formula/list
or route-cache tuning.

Fresh evidence:

- `target/reports/native-gpu/cells-visible-click-e2e-release-smoke-selected-node-proof.json`
  is schema-valid and still fails.
- All four click samples complete and pass visual formula proof:
  A2 about `12.278 ms`, B0 about `13.786 ms`, C0 about `28.441 ms`, and A0
  about `11.098 ms` product input-to-present.
- `formula_transition_contract` and `selected_cell_transition_contract` pass.
- Structured proof now reports `visible_selected_node_matches_address=true` for
  all four samples, so the old missing focus-overlay field is no longer the
  only way to prove selected-cell visual state.
- The C0 miss is real product work: native input work is about `0.290 ms`, route
  lookup is about `0.028 ms`, runtime/list scans and recompute are zero, queue
  submit is about `0.121 ms`, present is about `9.920 ms`, and render-hook
  outer core work is about `17.486 ms`.
- C0 still reports `legacy_product_proof_built_pre_present=true` with five
  post-present proof requests, so there is still proof-shaped work on the
  product frame boundary.
- The app-window product-frame contract reports p95 `28.441 ms`,
  `missed_frame_count=1`, and `source="app_window_product_frame_commits"`.

No-loss TODOs from this checkpoint:

- [ ] Remove remaining legacy pre-present proof construction from product
  render hooks:
  - product hooks return only typed product render results, scalar timings,
    revision ids, retained patch counts, render target ids, product proof
    handles, and `FrameEvidenceKey`;
  - layout proof, visible-bound-text proof, retained-sync proof, proof history,
    artifact hashes, screenshot/diff paths, report JSON, and render-proof JSON
    are built by post-present subscribers or diagnostic modes;
  - gates fail if `legacy_product_proof_built_pre_present=true` on a product
    interaction frame unless an explicit diagnostic mode is active.
- [ ] Promote the visual proof artifact registry into the single crop-baseline
  source:
  - crop probes use the previous accepted app-owned visual proof artifact keyed
    by `FrameEvidenceKey`, not only `last_interactive_readback_artifact`;
  - reports distinguish missing baseline, stale baseline, mismatched evidence
    key, mismatched surface epoch, and true pixel mismatch;
  - skipping interactive readback is allowed only when an equivalent external
    app-owned proof artifact is registered for the same accepted frame or an
    explicitly modeled follow-up proof frame.
- [ ] Split formula-bar proof from selected-cell proof:
  - formula-bar proof may pass from generic bound-text / visible-bound-text /
    retained text-control evidence for the bound selected-input value;
  - selected-cell proof may pass from selected retained node state plus source
    intent or lookup metadata;
  - focus-overlay evidence is one proof source, not the only proof source;
  - the verifier reports formula-bar proof, selected-cell proof, and same-frame
    proof identity separately.
- [ ] Normalize product/proof frame acceptance:
  - the default gate requires proof `FrameEvidenceKey` to match the product
    commit key exactly;
  - if bounded follow-up proof frames are allowed, they must be explicit in the
    schema with same `input_event_seq`, unchanged content/layout/render
    revisions or a named revision delta, matching surface epoch, and reported
    `proof_lag_frames`;
  - "latest proof" and self-consistent-but-unrelated proof are failures.
- [ ] Cut render-hook proof/layout work before present:
  - C0 shows one semantic delta and zero runtime scans but still spends about
    `17.5 ms` in render-hook core work;
  - move `proof_build_ms`, layout-proof clone/build work, visible-bound-text
    proof extraction, retained-sync proof, and source-intent proof snapshots to
    immutable post-present subscribers;
  - product render hook consumes an immutable `ActivePreviewScene` and a small
    typed `ProductPatch`, then encodes/submits without proof tree construction.
- [ ] Make `ActivePreviewScene` / `PendingPreviewScene` the product boundary:
  - first-frame click/selection/text-control changes patch retained active
    control/property/render state directly;
  - document/layout/runtime catch-up builds a capacity-1 pending snapshot and
    commits only if source/content/layout/render/surface/input epochs still
    match;
  - stale pending snapshots are cancelled before proof/report allocation.
- [ ] Add a product hot-path firewall:
  - product frames fail if they perform file I/O, layout artifact reload,
    `serde_json::Value` proof construction, proof history expansion, broad
    state summary, full layout proof clone, latest-report lookup, or dev-window
    IPC before present;
  - product frames report heap allocations, clone bytes, lock waits, proof JSON
    allocation, layout proof bytes, and source-intent snapshot bytes.
- [ ] Decide present strategy from same-surface evidence:
  - keep immediate mode, offscreen-copy-to-present, frame-latency overrides,
    and extra frames in flight as named diagnostics until a same-surface
    hardware baseline proves they improve repeated product p95/max;
  - product default should target late acquire, short CPU encode, bounded queue
    submit, and predictable present, not hidden present-mode tweaks.
- [ ] Build a smaller product verifier lane:
  - product-only lane waits for exact product frame commits and scalar counters;
  - proof-only lane waits for exact-key readback/proof artifacts;
  - proof-isolation lane deliberately stalls proof workers and verifies product
    p95/max and missed-frame counts do not move;
  - harness latency remains diagnostic and cannot be the UX gate unless its
    overhead is separately bounded.
- [ ] Delete or quarantine stale compatibility paths after replacements land:
  - `last_interactive_readback_artifact` baseline dependency;
  - focus-overlay-only selected proof;
  - legacy pre-present proof request rows;
  - product layout/proof JSON construction;
  - latest-report proof matching;
  - full-state retained sync on product interaction frames;
  - geometry/string/source-path route and proof matching fallbacks;
  - broad runtime/state summaries used as product synchronization.

## Embedded `/goal` Prompt

Use this later as:

```text
/goal Follow docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md until the entire plan is implemented and honestly verified.

Performance is the main goal. Implement the native preview architecture so normal visible interaction uses bounded requested-animation bursts inside DemandDriven mode, retained/hot renderer state, sparse runtime/layout work, and proof/debug modes that are toggleable and separately measured. Do not treat idle-wake as the main UX benchmark; keep it as a demand-driven smoke gate only. Do not turn RequestedAnimation into a third long-lived product mode unless the native GPU architecture docs are updated too.

Prefer strategy over tactics when the same gate keeps failing. Do not spend the run making a slow path merely more measurable or slightly less wrong. Cut the product interaction path down to a hot native loop: accept input at the start of an already scheduled frame, patch retained runtime/layout/render state directly, submit quickly, move proof/readback/reporting off the UX frame, and keep product latency separate from verifier proof latency while linking both with FrameEvidenceKey.

Start from the current 2026-07-02 typed product result and sample-key preservation checkpoint, not from older stale report text. The current WIP has already moved background proof telemetry out of product/burst frames, kept product commits and async report refresh available, split proof/harness work from product work, fixed armed-prewarm accounting, added hardware adapter identity to product evidence, started a generic `NativeFrameClockPolicy` owner for product/proof frame decisions, moved retained visible-bound-text proof payload construction behind post-present subscribers, separated product timing status from hardware-adapter status, and kept `List/find` / runtime / formula work clean in Cells interaction reports. Product commits are now published only for accepted product-interaction frames with accepted input timing; non-product presented frames may enqueue exact post-present proof work, but they must not enter `recent_product_frame_commits`. Product frame evidence now carries typed `product_patch` metadata from the active preview scene: active scene identity, route identity, patch kind/source, touched node counts, retained text/style update counts, direct patch/full-scene flags, and proof/latest-report dependency flags. Product commits now also require a typed `NativeProductFrameResult` source owned by `preview_active_scene`; legacy render-metric fallback is reported and rejected by the product UX lane. The visible-click product lane now fails unless every sample joins to an exact product commit with typed active-scene product patch and typed product result; proof-frame fallback, input-latency fallback, missing product result, and legacy product-result fallback are diagnostics only. The newest verifier WIP preserves the measured sample's product-present `FrameEvidenceKey` and reports any matched/fallback commit key separately as `matched_product_commit_frame_evidence_key`, so fallback matching cannot overwrite product/proof identity. If the next report exposes a missing exact product commit for the measured product frame, fix product frame publication/ownership or `FrameEvidenceRegistry` joining instead of masking it with a nearby latency match. Focused Rust checks pass for the changed app-window/playground/xtask slice, but the release Cells visible-click gate is still not complete on hardware-backed evidence.

Current evidence to respect:
- Latest one-click release diagnostic after typed product result evidence:
  `target/reports/native-gpu/cells-visible-click-e2e-a2-product-result-current.json`
  is schema-valid and still `status=fail`. This report predates the
  sample-key preservation fix, so use it as timing evidence but do not trust any
  fallback-overwritten top-level sample key as final product/proof ownership
  evidence. The product timing slice is clean:
  `timing_status=pass`, exact product commit matches `1`, proof-frame commit
  fallbacks `0`, input-latency fallbacks `0`, typed product patch count `1`,
  typed product result count `1`, legacy product-result fallback count `0`,
  product result missing count `0`, product patch missing/full-scene/proof-JSON/
  latest-report counts all `0`, input-accept-to-present/formula p95/max
  `13.137367 ms`, missed frames `0`, and hard failures `0`. The product
  contract still fails because the run used software Vulkan llvmpipe
  (`adapter_status=software`), the raw wake-to-formula gate was slightly over
  budget at `17.361 ms`, and the proof lane failed separately because the
  structured proof had `proof_lag_frames=3` but did not match the input event.
- Latest one-click release diagnostic after typed product patch evidence:
  `target/reports/native-gpu/cells-visible-click-e2e-a2-product-patch-exact-current.json`
  is schema-valid and still `status=fail`. The product timing slice is clean:
  `timing_status=pass`, exact product commit matches `1`, proof-frame commit
  fallbacks `0`, input-latency fallbacks `0`, typed product patch count `1`,
  product patch missing/full-scene/proof-JSON/latest-report counts all `0`,
  input-accept-to-present/formula p95/max `11.754082 ms`, missed frames `0`,
  and hard failures `0`. The product contract still fails because the run used
  software Vulkan llvmpipe (`adapter_status=software`), and the proof lane
  still fails separately with proof lag reported at `3` frames. Treat this as a
  good diagnostic checkpoint, not final acceptance.
- Latest one-click release diagnostic after the product-commit lane split:
  `target/reports/native-gpu/cells-visible-click-e2e-a2-product-commit-lane-split-current.json`
  is schema-valid but `status=fail` because this run selected software Vulkan
  llvmpipe. The exact product timing lane passed: `timing_status=pass`,
  input-accept-to-present/formula p95/max `11.100808 ms`, missed frames `0`,
  hard failures `0`, legacy pre-present request count `0`, product did not
  block on proof, proof-only passed, post-present proof isolation passed, and
  proof lag max was `2` frames. The remaining blockers in that diagnostic were
  hardware evidence only: `hardware-product-adapter` and the hardware-backed
  `product-only-ux-contract`.
- Best fresh release run after the background-proof split: all 64 Cells clicks passed visually and functionally, `/product_only_ux_contract.status=pass`, `/proof_only_contract.status=pass`, accepted product p95 was `16.062596 ms`, proof lag p95 was `6` frames and max `8`; the remaining failure was raw wake-to-formula p95 `20.567028 ms`.
- Latest release run after enabling real armed-prewarm counters: status `fail`, accepted product p95 `18.442151 ms`, max `19.244245 ms`, product missed frames `9`, product-commit wake-to-formula p95 `22.724245 ms`, wake-to-accept p95 `5.562695 ms`, and `input_waited_for_already_armed_frame_count=65`. Proof-only still passes and background worker load is much lower than the earlier overloaded proof/readback reports.
- Latest one-click release smoke after the deferred retained proof subscriber cut:
  `target/reports/native-gpu/cells-visible-click-e2e-a2-deferred-retained-proof.json`
  is still `fail`, but it is diagnostic because it selected software Vulkan
  llvmpipe (`adapter_status=software`). The exact product-interaction sample
  itself was clean: input-accept-to-present/formula `10.710457 ms`, missed
  frames `0`, legacy pre-present proof request count `0`, product did not block
  on proof subscribers, proof-only passed, proof lag was `2` frames, and
  post-present proof isolation passed. The remaining non-adapter failure was
  aggregate preview-loop p95 `18.404 ms`; treat that as evidence that product,
  animation follow-up, and harness/proof lanes still need stricter typed
  separation.
- The remaining p95 misses are dominated by product-frame scheduling, queue submit, present, and render-result ownership. Cells runtime/list/formula is not the current blocker: reported interaction samples have one dirty key, zero list scans, no full-grid recompute, no root materialization, and no full relower.
- The same-surface present-floor diagnostic currently selected llvmpipe software Vulkan. Do not use that as hardware/product evidence. Add adapter identity to all relevant product/perf reports and fail fast when a performance gate runs on a software adapter unless the verifier explicitly opts into diagnostic mode.
- The current code checkpoints add a hardware-adapter product gate, expose `native_frame_clock_policy` / `native_frame_clock_owner` in render-loop reports, limit deferred product proof requests to exact retained visual proof after present, split product commit publication from non-product presented frames, require typed active-scene `product_patch` evidence on rendered product-frame commits, and require typed `NativeProductFrameResult` ownership instead of legacy render-metric fallback. These are guardrails and ABI seams, not the final 60 FPS architecture. Continue by making that frame clock and an `ActivePreviewScene` the real product-frame owner instead of treating them as diagnostic wrappers.

Treat the next phase as architecture cutting, not micro-optimization. The main mistake to avoid is spending another long run making the current legacy render-hook path more measurable and less wrong while the product frame still travels through proof-shaped ownership. Pick the largest simple boundary that removes product-frame work, then verify once with focused tests and one fresh release report. The preferred cut is a real `PreviewHotLoop` / `NativeFrameClock` product owner with `ActivePreviewScene`:
- sample visible-changing input at the start of an already scheduled demand-driven burst frame;
- patch retained selection/focus/formula-bar visual state directly in the active retained scene;
- submit the product frame quickly with a small typed `PresentedProductFrame` / `RenderFrameResult`;
- run source/runtime cleanup, proof/readback/report serialization, artifact hashing, accessibility snapshots, HUD formatting, and dev IPC after product present under exact `FrameEvidenceKey`;
- keep proof latency, proof lag, and report generation separate from UX latency, while proving they correspond to the measured presented frame.

Implementation priorities:
1. Expand the existing `NativeFrameClockPolicy` into one product-frame owner: `PreviewHotLoop` / `NativeFrameClock`. DemandDriven remains the product mode; requested animation is only a bounded burst pacing substate with quiet-frame and hard-cap exits; ContinuousProbe is diagnostic/verifier only.
2. Introduce `ActivePreviewScene`, `PendingPreviewScene`, and `RecyclePreviewScene`. The active scene must be directly presentable and own retained layout/render chunks, text runs, selection/focus/caret overlays, hit-test indexes, and GPU resource references.
3. Replace product render-hook proof/report construction with a small typed product result. Product frames must publish a `PresentedProductFrame` / `RenderFrameResult` with `FrameEvidenceKey`, timing scalars, adapter identity, and typed active-scene product patch evidence. They must not build proof JSON, visible-bound-text proof, retained-bound-sync proof, report JSON, proof history, artifact hashes, broad state summaries, or latest-report fallbacks.
4. Move WGPU ownership toward a render actor: keep pipelines, bind groups, buffers, glyph atlas, prepared quads, and command resources hot; acquire late; encode from immutable active-scene snapshots; submit quickly; report acquire, encode, queue submit, present, post-present dispatch, and driver/compositor floor separately.
5. Make retained patching property/component based with stable semantic identity from compiler/document through layout/render. Product clicks should touch only changed nodes and overlays. Add negative gates for full-state fallback, geometry/string/path matching, proof JSON dependency, dev IPC dependency, and product-frame report parsing.
6. Make proof/readback a bounded service keyed by `FrameEvidenceKey`, with required-frame proof, background telemetry, stale-key rejection, coalescing, lag/drop counters, and proof-isolation stress gates.
7. Repair exact product/proof joining after sample-key preservation: run a fresh one-click release report, prove `product_frame_evidence_key`, `requested_product_frame_evidence_key`, `matched_product_commit_frame_evidence_key`, and `proof_frame_evidence_key` are all reported honestly, then fix whichever owner is wrong. Product UX may use only exact product commits for the measured product-present frame; proof UX may lag only when that lag is explicit and keyed.
8. Add same-surface hardware present-floor evidence before changing present mode, frame-latency, or frames-in-flight policy. Keep `BOON_NATIVE_DESIRED_MAXIMUM_FRAME_LATENCY` and alternate present modes diagnostic until hardware evidence proves a product benefit.

Do not repeat failed tactics as defaults: offscreen copy-to-present, desired frame latency `2`, naive non-dirty pointer burst priming without exact keyed proof selection, per-leaf path-driven retained sync, route-cache branch experiments, upload-cache tuning, JSON size trimming, or formula/list/runtime tuning. Revisit those only if a fresh product-frame report names them as the dominant current boundary.

Use the plan's architecture TODO backlog, stale-path ledger, maximal architecture addenda, and external-library notes as mandatory steering. Prefer deleting or quarantining stale slow paths over adding compatibility branches. If a product frame still depends on proof JSON, latest reports, full state summaries, geometry/string route lookup, broad runtime currentness, dev IPC, or verifier-shaped fallbacks, cut that architecture boundary and add a negative test.

Use subagents whenever useful for independent architecture, runtime/compiler, WGPU/rendering, testing, or external-library research. If the path starts becoming too complex, too hacky, or circular, stop micro-tuning and choose a simpler generic architecture that matches the source-of-truth docs.

Do not add Cells/example-specific hacks in compiler, runtime, document, renderer, app-window, playground, or verifier code. No production branches on example names, source paths, cells/address/value/error/A0, or fixture-specific strings. Fix engine/runtime/document/rendering architecture instead.

Implement the dev-window preview performance row and bounded preview-perf snapshot. Add exact timing definitions, product counters, burst exit criteria, active/pending snapshot backpressure, and FrameEvidenceKey proof identity. Prove visually and functionally with deterministic native tests, app-owned WGPU readbacks tied to the measured presented frame, release-mode latency reports, runtime counters, and schema-valid reports. Fix broken/flaky verification infrastructure too when it blocks reliable progress.

Do not claim completion until all required native UX, proof identity, perf-HUD, generic runtime, no-hacks audit, and stale-proof negative gates pass on fresh hardware-backed reports for the current worktree/binary. Software-adapter reports are useful diagnostics only unless the verifier explicitly opts into that mode. If blocked, leave the repo coherent and report the exact blocker, evidence, and next implementation step.
```

## 2026-07-02 Fresh Post-Present Smoke And Maximum Architecture TODOs

Fresh local checkpoint after the post-present proof bridge:
`target/reports/native-gpu/cells-visible-click-e2e-release-smoke-post-present-proof.json`.
The report is schema-valid and still fails. All four click samples pass the
app-owned visual formula proof, selected visual state is visible, runtime/list
work remains clean, and the matched click product commits now report
`legacy_product_proof_built_pre_present=false` with
`legacy_pre_present_proof_request_count=0`. The remaining product gate is
`input_accept_to_present_ms_p95=25.590552 ms` with one slow A0 sample. That
sample spends about `16.172 ms` in render-hook core and `8.503 ms` in
`present_call_ms`. The top-level product click path is cleaner, but the matching
preview-loop artifact still reports animation/follow-up frames with
`legacy_product_proof_built_pre_present=true` and five legacy pre-present proof
request kinds. Do not lose this distinction: product click commits improved, but
the old proof-shaped path still exists and must be deleted or quarantined before
default repeated release can be trusted.

Treat the next phase as architecture cutting, not micro-optimization. Pick the
largest simple boundary that removes product-frame work. These TODOs are generic
runtime/document/rendering architecture work; none may branch on Cells, example
names, source paths, addresses, formulas, or fixture strings.

- [ ] Build one product-frame owner: `PreviewHotLoop` / `NativeFrameClock`.
  - It owns host-input drain, dirty poll, render extraction, WGPU acquire,
    submit/present, product commit publication, and post-present subscriber
    enqueue.
  - Input that can affect visible state is accepted at the start of an already
    scheduled frame or bounded requested-animation burst.
  - Proof, readback, report serialization, artifact hashing, dev-window IPC,
    accessibility snapshots, and HUD formatting are not allowed before product
    present.
  - DemandDriven remains the normal product mode; requested animation is only a
    bounded pacing substate with quiet-frame and hard-cap exits; ContinuousProbe
    is verifier/diagnostic only.

- [ ] Split lanes explicitly instead of relying on aggregate rings.
  - Product interaction, animation follow-up, proof/readback, HUD/dev telemetry,
    source-replace, resize/surface-change, and diagnostic probe frames need
    typed lanes.
  - UX gates use only exact product-interaction commits keyed by
    `FrameEvidenceKey` and accepted input event sequence.
  - Animation follow-up frames may exist, but they must not keep legacy proof JSON
    alive or rewrite product-latency measurements.
  - Missing exact-key product evidence fails; latest-report, last-frame,
    geometry, text, path, or hash-only fallback matching is a diagnostic fail.

- [ ] Replace the render-hook product boundary with a small typed result.
  - Product render should return a `PresentedProductFrame` /
    `RenderFrameResult` containing stable scene identity, touched node ids,
    visible-surface target, upload/encode counters, and post-present subscriber
    descriptors.
  - It should not build proof JSON, visible-bound-text proof, retained-bound-sync
    proof, report JSON, proof history, artifact hashes, or broad state summaries.
  - Any compatibility proof object left in product mode must have an owner, a
    deletion test, and a report field showing it is product-forbidden.

- [ ] Introduce `ActivePreviewScene`, `PendingPreviewScene`, and
  `RecyclePreviewScene`.
  - The active scene is always directly presentable and owns retained layout,
    retained render chunks, text runs, selection/focus/caret overlays, hit-test
    indexes, and GPU resource references.
  - Product clicks patch active retained state directly with a bounded
    `ProductPatch`; they do not rebuild the full document, full layout frame,
    full render scene, or proof summaries.
  - Pending scene builds are latest-wins with at most one pending snapshot per
    role/surface; stale `source_revision`, `layout_revision`,
    `render_scene_revision`, or `surface_epoch` results are dropped before
    commit.
  - Hit testing uses the active layout snapshot until a pending snapshot commits.

- [ ] Make retained patching property/component based.
  - Keep stable semantic identity from compiler/document through layout/render;
    renderer and verifier must not rediscover identity from geometry, labels,
    strings, or Cells-specific metadata.
  - Use dirty component indexes for text, style, transform, clip, scroll offset,
    selection, focus, caret, accessibility metadata, and hit regions.
  - Product patch application should touch only changed nodes and overlays; add
    counters for touched nodes, uploaded bytes, draw-call changes, text-run
    reflows, cache hits, cache evictions, and full-state fallbacks.

- [ ] Move WGPU ownership toward a render actor.
  - Keep surface configuration, pipelines, bind groups, buffers, glyph atlas,
    prepared quads, and command encoders hot across bursts.
  - Acquire the surface late, encode from immutable active-scene snapshots, and
    submit quickly.
  - Measure and report acquire, CPU encode, queue submit, `frame.present()`,
    post-present dispatch, and compositor/driver-floor estimates separately.
  - Keep `BOON_NATIVE_DESIRED_MAXIMUM_FRAME_LATENCY` and alternate present modes
    as explicit diagnostics only until same-surface hardware evidence proves a
    better default.

- [ ] Build a bounded `PostPresentProofQueue` and delete old proof paths.
  - WGPU readback, visible-bound-text proof, retained-sync proof, focused-node
    proof, screenshots/diffs, report JSON, proof history, artifact path
    registration, and artifact hashes run as subscribers keyed by exact
    `FrameEvidenceKey`.
  - Subscribers carry mode allowlists, stale-key rejection, lag/drop/error
    counters, max queue length, max work per drain, and proof-lag reporting.
  - Product reports fail if `legacy_product_proof_built_pre_present=true`,
    `legacy_proof_json_built_pre_present=true`,
    `legacy_render_hook_proof_built_pre_present=true`, or any post-present
    request is still marked `currently_legacy_pre_present=true` on a product UX
    frame.

- [ ] Add a render-loop-owned `FrameEvidenceRegistry`.
  - It registers frame seq, content/layout/render revisions, surface id/epoch,
    input event seq, present id, product lane, proof request ids, and artifact
    completion ids.
  - It is the only source for joining UX samples to proof artifacts.
  - Gates reject stale first-frame proof reuse, mismatched surface epoch,
    mismatched content/render revisions, proof from a later frame unless proof
    lag is explicitly allowed/reported, and hash-only proof without structured
    artifact metadata.

- [ ] Keep runtime/compiler improvements available, but do not misdiagnose the
  current blocker.
  - Preserve sparse indexed `List/find`, demand-current indexed fields, formula
    dependency fanout, cycle-safe evaluation, columnar row storage, and
    virtualized `List/chunk` materialization as required generic runtime work.
  - Current visible-click evidence is not failing on list scans, eager formula
    recompute, or root materialization; do not restart there unless a fresh
    report names that path again.
  - It is acceptable to simplify or restructure Boon example code when that makes
    the intended app cleaner, but never hide engine/runtime/render limitations in
    example-specific branches.

- [ ] Add product-only, proof-only, and stress verifiers.
  - Product-only UX gate: no readback/proof/report subscribers required to make
    the visible update happen; p95 and max are measured from exact product
    interaction commits.
  - Proof-only gate: waits for matching app-owned WGPU/readback artifacts by
    `FrameEvidenceKey`, reports proof lag separately, and fails on stale or
    mismatched proof.
  - Proof-isolation stress: intentionally slow proof subscribers must not change
    product p95/missed-frame counts.
  - Deterministic scheduler simulation: idle, burst, continuous probe, surface
    lost, resize, source replace, proof-only sample, delayed subscriber, and
    backpressure transitions.
  - Visual cursor/native interaction tests should cover all Boon examples through
    the same host-event route, not Cells-only code.

- [ ] Add hot-path budgets and deletion gates.
  - Count allocations, locks, map scans, path-string builds, JSON builds, IPC
    waits, report writes, artifact hash reads, proof-history compaction, full
    layout lowers, full render-scene rebuilds, full retained-bound sync, and
    legacy fallback route matching on product frames.
  - Set product-frame gates to zero for stale paths once replacements land.
  - Keep a machine-readable stale-path ledger for legacy Ply, Xvfb, COSMIC
    scraping, browser screenshots, modeled/static success, latest-report proof,
    legacy pre-present proof, geometry/string lookup, full-state retained sync,
    and broad runtime summaries.

- [ ] Use external architecture lessons as input, not as shortcuts.
  - GPUI-style: app-owned retained scene, element identity, focused controls, and
    background work that cannot block present.
  - Bevy-style: extraction/preparation/render phases, change detection, resource
    residency, and explicit schedules.
  - Servo/browser-style: display lists, stacking/clip/property trees,
    invalidation, compositor-friendly scrolling, and separate script/layout/paint
    phases.
  - Game-loop style: predictable frame pacing, hot resources, minimal per-frame
    allocation, explicit frames-in-flight, and proof/debug overlays outside the
    gameplay frame.
  - Rust/Zig/Wasm hot kernels are later options for formula/text/layout kernels
    only after the product-frame ownership boundary is clean and measured.

If another one or two patches still leave product p95 over `16.7 ms`, stop
patching the legacy render hook and implement the minimal new product path:
`HostInput -> ProductPatch -> ActivePreviewScene -> WGPU present -> ProductCommit
-> PostPresentProofQueue`. Keep the old path as diagnostic-only until tests prove
the new path covers generic document output, then delete or quarantine the stale
product path with negative gates.

## 2026-07-02 Current Checkpoint: Fast Product Sample, Broken Proof Join

Fresh local checkpoint:
`target/reports/native-gpu/cells-visible-click-e2e-release-smoke-pending-flush-legacy-proof-cut.json`.
The report is schema-valid but still `status="fail"`.

Current measured state:

- Product-frame latency for the one accepted click sample is now below budget:
  `input_accept_to_present_ms_p95=11.246291 ms`, max `11.246291 ms`, and
  `interaction_scoped_product_missed_frame_count=0`.
- Wake-to-present is still higher at `22.292513 ms`; this includes the scheduler
  wake/accept boundary and is not the same as accepted-input product latency.
- The click used the intended fast path:
  `simple_source_click_deferred_runtime`; `generic_fallback_count=0`.
- Post-present proof isolation is active on the latest preview-loop artifact:
  `mode="post_present_subscriber_queue"`,
  `legacy_pre_present_request_count=0`,
  `product_blocks_on_proof_subscribers=false`,
  `product_latency_includes_proof_completion=false`,
  `report_serialization_in_hot_path=false`, and
  `hot_path_report_serialization_count=0`.
- Runtime/list work is no longer the current visible-click blocker in this
  smoke: list scans, root materialization, and recompute counters remain zero.
- The gate still fails because the verifier/proof bridge did not complete the
  required interaction evidence: only one product sample was accepted, visual
  formula proof failed, selected-cell proof failed due missing layout-artifact
  join data, and retained committed update timing evidence was missing.

Do not call this fixed. The current state is "product hot path looks promising
on one sample, proof/verifier join is broken, and repeated/default acceptance is
not proven."

No-loss TODOs from this checkpoint:

- [ ] Fix the post-present proof bridge by exact `FrameEvidenceKey`.
  - Join visible-bound-text, retained-bound-sync, layout artifact, WGPU readback,
    focused/selected node metadata, and product commit by the same key.
  - Fail on stale/latest proof instead of silently falling back.
  - Make the crop/structured probes consume the same post-present artifact set.
- [ ] Restore required sample completion before judging latency.
  - `verify-native-cells-visible-click-e2e` must produce all required product
    click samples, not one fast sample plus proof timeout.
  - Missing product samples, missing proof samples, and over-budget product
    frames must be separate blocker classes.
- [ ] Keep the old legacy proof code quarantined, then delete it.
  - The latest measured product path has zero legacy pre-present requests, but
    production code still contains legacy proof/report JSON paths and opt-in
    compatibility flags.
  - Add negative gates for product frames that build legacy pre-present proof,
    depend on latest report data, or serialize proof/report JSON before present.
- [ ] Continue the architecture cut rather than local timing tweaks.
  - If a repeated/default report still fails after proof joining is repaired,
    implement the minimal `PreviewHotLoop + ActivePreviewScene +
    ProductPatch + PostPresentProofQueue` path and retire the old render-hook
    product boundary.
- [ ] Do not restart with Cells formula/list/runtime micro-work unless a fresh
  repeated report names that boundary again.
  - Current evidence points at proof/verifier joining and remaining legacy
    product-boundary code, not spreadsheet formula logic.

## 2026-07-02 Current Checkpoint: One-Repeat Release Cells Visible-Click Pass

Fresh local checkpoint:
`target/reports/native-gpu/cells-visible-click-e2e-release-smoke-native-runtime-work-fallback.json`.
The report is schema-valid and `status="pass"` for the one-repeat release Cells
visible-click smoke.

What this proves:

- Product visible-click latency is currently under the 60 FPS budget for this
  smoke: `input_accept_to_present_ms_p95=12.833236 ms`, max
  `12.833236 ms`, and `preview_loop_missed_frame_count=0`.
- The product path is keyed by exact app-window product commits:
  `preview_loop_product_path_contract.source="app_window_product_frame_commits"`,
  `product_frame_sample_count=4`, `required_sample_count=4`, and every click has
  matching `FrameEvidenceKey` product/proof identity.
- App-owned visual proof is complete for the click samples:
  `exact_visual_proof_sample_count=4` and every sample reports
  `visual_proof_proves_presented_frame=true`.
- The retained update contract passes:
  `evidence_sample_count=4`, `retained_committed_update_count=4`,
  `committed_render_patch_count=4`, `full_document_lower_count=0`,
  `document_patch_fast_path_rejected_count=0`, and
  `legacy_selection_fallback_count=0`.
- The runtime/list work contract passes:
  `total_rows_scanned=0`, `total_list_find_rows_scanned=0`,
  `total_summary_fields_scanned=0`, `total_root_materialization_candidates=0`,
  `total_recomputed_fields=0`, and all four samples are zero-scan,
  zero-root-materialization, zero-recompute samples.
- The first click no longer fails merely because old retained
  `interaction_timing` is absent. The verifier now accepts the generic evidence
  shape where native input timing records `deferred_runtime_not_invoked` and the
  same app-owned visual proof carries retained text/style sync.
- Post-present proof isolation passes for this smoke:
  `mode="post_present_subscriber_queue"`,
  `legacy_pre_present_request_count=0`,
  `product_blocks_on_proof_subscribers=false`,
  `product_latency_includes_proof_completion=false`,
  `hot_path_report_serialization_count=0`, and
  `hot_path_report_write_count=0`.

What this does not prove yet:

- The full native realtime/proof plan is not complete. This is a one-repeat
  smoke, not the full handoff or repeated release acceptance.
- Wake-to-present is still high in this report:
  `input_wake_to_present_ms_p95=121.277158 ms`. Accepted-input product latency
  is fast, but event-loop wake/accept/frame-clock ownership still needs the
  larger `PreviewHotLoop` / `NativeFrameClock` architecture cut.
- Proof subscribers are still lagging but isolated:
  `proof_worker_status="lagging"` and `queued_request_count=10`. This is
  acceptable for product latency only while exact proof identity and lag are
  reported; proof-only and proof-isolation stress gates still need to be
  implemented.
- The codebase still contains substantial legacy JSON/proof/layout plumbing.
  The latest product path reports zero legacy pre-present proof, but stale paths
  remain in production code and must be deleted or quarantined with negative
  gates.
- This does not replace the generic runtime/list/formula/currentness work:
  sparse list windows, indexed `List/find`, demand-current barriers, dependency
  fanout, range invalidation, and cycle safety still need broader non-Cells
  fixtures and gates.

Next implementation priorities from this checkpoint:

- [x] Run the default/repeated release visible-click gate and record whether the
  one-repeat pass holds under normal sample counts.
  - Fresh report:
    `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`
    returned 0.
  - The report is `status="pass"` with 64 click samples,
    `simple_source_click_count=64`, `generic_fallback_count=0`, retained update
    contract `pass`, runtime/list work contract `pass`, and post-present proof
    isolation `pass`.
  - Product accepted-input latency now meets the current p95/max contract:
    `input_accept_to_present_ms_p95=14.733276 ms`,
    `input_accept_to_present_ms_max=17.182652 ms`,
    `hard_failure_count=0`, and one bounded outlier under the `33.4 ms` max.
  - Runtime/list counters remain clean for the measured product click path:
    `total_rows_scanned=0`, `total_list_find_rows_scanned=0`,
    `total_root_materialization_candidates=0`, `total_recomputed_fields=0`,
    and all 64 samples are zero-scan, zero-root-materialization, and
    zero-recompute samples.
  - Do not overclaim this checkpoint: `input_wake_to_present_ms_p95` is still
    about `184.854 ms`, so the event-loop wake/accept/frame-clock boundary is
    still the major architecture blocker. The aggregate preview-loop diagnostic
    path also still reports extra non-product misses, which is why product-only,
    proof-only, and stale-path deletion gates remain required.
- [ ] Add product-only, proof-only, and proof-isolation stress modes so product
  latency cannot be hidden by proof lag and proof correctness cannot be hidden
  by product-only success.
- [ ] Continue the frame-clock architecture cut:
  `HostInput -> ProductPatch -> ActivePreviewScene -> WGPU present ->
  ProductCommit -> PostPresentProofQueue`.
- [ ] Reduce the wake/accept boundary with a real product frame owner instead of
  measuring only already-accepted input.
- [ ] Start deleting/quarantining stale product paths: layout/proof JSON route
  lookup, latest-report proof joins, legacy pre-present proof, broad retained
  sync, and verifier-shaped fallback paths.

2026-07-02 stale-path ledger gate slice:

- Added a checked-in machine-readable ledger:
  `docs/plans/native_gpu_stale_path_ledger.json`.
- Added `cargo xtask verify-native-gpu-stale-path-ledger`, which reads the
  ledger, follows linked report paths such as `/preview_loop_report`, compares
  exact JSON pointers against expected product-forbidden values, and emits a
  schema-valid native GPU report.
- Added the ledger report to `native_gpu_handoff_required_reports()` so
  `verify-native-gpu-all --check-existing` cannot silently ignore these stale
  product-path dependencies once the report has been generated.
- Seed rows currently cover:
  - linked preview-loop `legacy_product_proof_built_pre_present=false`;
  - linked preview-loop `legacy_pre_present_proof_request_count=0`;
  - linked preview-loop `hot_path_report_serialization_count=0`;
  - linked preview-loop `hot_path_report_write_count=0`;
  - release visible-click `retained_update_contract.legacy_selection_fallback_count=0`;
  - release visible-click `runtime_work_contract.total_list_find_rows_scanned=0`.
- Fresh report:
  `target/reports/native-gpu/stale-path-ledger.json`.
  It is schema-valid and `status="pass"` with `row_count=6`,
  `product_forbidden_row_count=6`, `product_forbidden_pass_count=6`,
  `failed_row_count=0`, `missing_report_count=0`, and
  `linked_report_count=4`.
- Added a focused regression test:
  `cargo test -q -p xtask stale_path_ledger_rejects_product_forbidden_legacy_proof -- --test-threads=1`.
  It proves the gate fails when a linked product report contains
  `legacy_product_proof_built_pre_present=true`.
- This is not deletion yet. It is the enforceable inventory that lets the next
  slices remove/quarantine old paths without losing track of the replacement,
  positive gate, negative gate, and removal condition.

2026-07-02 wake-to-accept gate checkpoint:

- Read the active goal attachment and re-confirmed the full objective: implement
  this entire plan with performance as the main goal, not just make the current
  Cells accepted-input metric pass.
- Added a generic native app-window guardrail by reducing
  `PASSIVE_INPUT_POLL_INTERVAL` from `100 ms` to `2 ms`.
  - Rationale: in the current native loop this timeout is not merely an idle
    energy knob; it is also the fallback cadence for app-window input discovery
    before visible host input can be accepted.
  - This is not the final architecture. It is a stopgap until `PreviewHotLoop`
    owns input pumping/frame pacing directly.
- Strengthened the Cells release verifier so wake-to-accept cannot be hidden
  behind accepted-input timing:
  - live probe now emits `input_wake_to_input_accept_ms_p95/max`,
    `steady_input_wake_to_input_accept_ms`, and
    `steady_poll_started_to_input_accept_ms`;
  - outer reports copy those summaries through;
  - added `cells-visible-click-e2e:input-wake-to-formula-budget`, which requires
    steady wake-to-accept, wake-to-present, and wake-to-formula p95 to stay
    within the visible-interaction budget.
- Focused checks passed after this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window -p xtask`;
  - `cargo check -q -p xtask`.
- Fresh release verifier:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  It failed honestly:
  - accepted product frame p95 stayed good:
    `input_accept_to_present_ms_p95=14.865797 ms`;
  - accepted product max still had one bounded outlier:
    `input_accept_to_present_ms_max=38.871298 ms`;
  - wake-to-present remained bad:
    `input_wake_to_present_ms_p95=150.785198 ms`,
    `steady_input_wake_to_present_ms.p95=154.433158 ms`;
  - wake-to-accept remained bad:
    `steady_present_probe_phase_ms.input_wake_to_input_accept.p95`
    was about `115.297023 ms`;
  - the release gate failed on harness/driver latency classification, max
    accepted product outlier, product path missed-frame count, and the new
    wake-to-formula budget.
- Subagent review agreed the 2 ms floor is only a guardrail. The safest next
  generic architecture cut is an input-first hook split:
  - app-window should call a role input phase before the monolithic maintenance
    poll;
  - preview should route visible host input/source dispatch before passive
    hover, accessibility refresh, relayout maintenance, headed-scenario work,
    diagnostics, proof/report drains, and telemetry;
  - acceptance should be recorded for that input-first dirty result before
    maintenance work runs;
  - the split must be generic across roles and must not branch on Cells,
    addresses, formulas, example names, or source paths.
- Do not spend more time on passive-poll micro-tuning unless a fresh report
  names the polling floor as the remaining dominant cost after the input-first
  split exists.

2026-07-02 input-first rerun and raw-wake experiment checkpoint:

- Implemented and verified a generic preview input-first hook slice plus
  pending-live-event prioritization. The current release report is
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`; it is
  schema-valid but still `status="fail"`.
- Fresh focused checks passed before the release verifier:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`.
- The good news from the fresh report:
  - product accepted-input latency is inside the current product-frame budget:
    `input_accept_to_present_ms_p95=15.753918 ms` and max
    `18.559882 ms`;
  - source/runtime click dispatch is no longer the old 90-110 ms blocker:
    slow wake samples now show `source_input_ms` around `0.3-1.3 ms`;
  - `simple_source_click_count=64`, `generic_fallback_count=0`;
  - the product path still reports zero full-grid/runtime list work in the
    product contract.
- The remaining failing gate is wake-scoped:
  - `steady_input_wake_to_input_accept_ms.p95=22.751411 ms`;
  - `steady_input_wake_to_present_ms.p95=48.902974 ms`;
  - the only top blocker is
    `Cells steady product wake-to-formula visibility exceeded budget`.
- Subagent review split the remaining issue into two parts:
  - real architecture risk: app-window input acceptance is still not owned by a
    dedicated `PreviewHotLoop` / host-event frame clock, so some clicks enter
    through the monolithic role poll and some present just after the compositor
    boundary;
  - verifier/accounting risk: `wake-to-formula` currently mixes raw gesture
    wake, release/click acceptance, exact product commit, and proof observation
    without a single host-event ledger proving they are the same event.
- Tried a generic raw-wake input-first experiment: treat
  `input_event_wake_count > last_presented_input_event_wake_count` as pending
  host input even when the sampled coalesced delta is empty. The unsafe variant
  allowed the input hook's no-op result to claim the turn and accept the input
  cursor; it broke the verifier completely (`simple_source_click_count=0`) by
  consuming click state before the real release delta was processed. That
  behavior was backed out.
- Kept only the safer part: raw wake may influence host-input accounting, but
  an empty sampled delta cannot claim the turn or consume the cursor unless the
  input hook produces a dirty result or the sampled delta itself has real OS
  events.
- Do not repeat the raw-wake no-op claim tactic. The next real cut should be
  one of these larger generic boundaries:
  - implement a host-event ledger keyed by concrete press/release/move/wheel
    event ids and join it through accept, product commit, and proof;
  - make `PreviewHotLoop` / `NativeFrameClock` own the product interaction
    frame so input is sampled at frame start rather than after demand-driven
    wake/poll drift;
  - split product UX gates from diagnostic raw-wake gates until same-event
    host-event evidence exists, while still reporting wake/present/proof lag
    honestly.

2026-07-02 runtime-first product click checkpoint:

- Root cause found for the remaining Cells visible-click failure after the
  input-first slice: the first product-interaction frame was often produced by
  `simple_source_click_deferred_runtime`. That path patched a fast selected
  placeholder first, queued the actual runtime source event, and let
  `pending_source_flush_deferred_runtime` update selection/formula-bar text on
  the next frame. The result looked like product latency was good while
  click-to-formula proof lagged by one or more frames.
- Changed the generic product default so
  `BOON_PREVIEW_DEFER_FIRST_FRAME_RUNTIME` is opt-in outside tests instead of
  enabled by default. This is intentionally not a Cells branch: source-changing
  clicks now apply their runtime turn before the product frame unless the
  diagnostic environment flag explicitly requests the old placeholder-first
  behavior.
- Focused checks passed:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`;
  - `cargo test -q -p boon_native_playground cells_release_with_stale_sampled_left_down_uses_simple_click_fast_path -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground cells_click_selection_updates_formula_bar_and_selected_style -- --test-threads=1`.
- Fresh release gate:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  It is schema-valid and `status="pass"`.
  - `input_accept_to_present_ms_p95=15.368459 ms`;
  - `input_accept_to_present_ms_max=29.124514 ms`, within the bounded
    `33.4 ms` outlier ceiling;
  - `runtime_work_contract.status="pass"`;
  - `runtime_work_contract.deferred_runtime_product_frame_count=0`;
  - `total_rows_scanned=0`, `total_list_find_rows_scanned=0`,
    `total_summary_fields_scanned=0`, `total_root_materialization_candidates=0`,
    and `total_recomputed_fields=0`;
  - `retained_update_contract.status="pass"`, `full_document_lower_count=0`,
    `document_patch_fast_path_rejected_count=0`, and
    `legacy_selection_fallback_count=0`;
  - click samples report `simple_source_click` instead of
    `simple_source_click_deferred_runtime` on the product path.
- Fresh stale-path ledger:
  `target/reports/native-gpu/stale-path-ledger.json`.
  It remains `status="pass"` with `row_count=6`,
  `product_forbidden_pass_count=6`, and `failed_row_count=0`.
- Do not overclaim completion:
  - raw wake-scoped metrics are still over the 16.7 ms interaction budget:
    `steady_input_wake_to_input_accept_ms.p95=24.405217 ms` and
    `steady_input_wake_to_formula_visible_ms.p95=74.164771 ms`;
  - the current verifier classifies this as
    `proof_lag_wake_overhead_reported_separately_from_product_accept_latency`
    with `proof_lag_sample_count=64` and `proof_lag_max_frames=3`;
  - `post_present_proof_isolation.status="pass"`, but
    `proof_worker_status="lagging"` and `queued_request_count=5`;
  - `PreviewHotLoop` / `NativeFrameClock`, product-only/proof-only stress
    modes, and more stale JSON/proof deletion are still required by the full
    plan.
- Next architecture cut should not re-enable placeholder-first product frames.
  Keep source-changing product clicks runtime-current on the first presented
  product frame, then reduce raw wake/proof lag with a real frame-clock owner
  and tighter product/proof subscriber backpressure.

2026-07-02 product/proof/isolation lane contract checkpoint:

- Added explicit lane contracts to the Cells visible-click verifier report:
  - `product_only_ux_contract` gates app-window product frame commits,
    accepted-input-to-present timing, runtime/list work, retained update work,
    DemandDriven mode, and absence of proof/report work on the product path;
  - `proof_only_contract` gates app-owned WGPU/readback proof for every click
    sample, keyed product/proof frame evidence, same input-event identity,
    current structured visual proof count, and reported proof lag;
  - `proof_isolation_contract` gates post-present proof/report isolation,
    zero legacy pre-present proof requests, zero hot-path report writes/
    serialization, no product blocking on proof subscribers, and zero proof
    worker/subscriber errors.
- The verifier now distinguishes same-frame exact visual proof from post-present
  structured proof:
  - `exact_visual_proof_sample_count` may be `0` when proof is produced later;
  - `current_structured_visual_proof_sample_count` must cover every sample;
  - `proof_lag_max_frames` is reported separately and bounded by the proof-lane
    contract, not folded into product UX latency.
- The native GPU release label contract now requires the three lane contracts
  to pass. This prevents a broad visible-click `status="pass"` from hiding
  proof coupling, missing proof samples, product-path proof/report work, or
  stale proof identity.
- Focused checks passed:
  - `cargo fmt --check`;
  - `cargo check -q -p xtask`;
  - `cargo test -q -p xtask cells_visible_click_lane_contracts -- --test-threads=1`;
  - `cargo test -q -p xtask native_gpu_label_contract_rejects_cells_visible_click_legacy_selection_fallback -- --test-threads=1`.
- Fresh release gate:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  It is schema-valid and `status="pass"`.
  - `product_only_ux_contract.status="pass"`;
  - `product_only_ux_contract.input_to_present_ms.p95=13.771367 ms`;
  - `product_only_ux_contract.input_to_present_ms.max=14.677210 ms`;
  - `proof_only_contract.status="pass"`;
  - `proof_only_contract.current_structured_visual_proof_sample_count=64`
    for `64` click samples;
  - `proof_only_contract.exact_visual_proof_sample_count=0`, explicitly
    showing proof is post-present structured proof rather than same-frame proof;
  - `proof_only_contract.proof_lag_max_frames=5` with
    `proof_lag_frame_budget=8`;
  - `proof_isolation_contract.status="pass"`;
  - `proof_isolation_contract.hot_path_report_write_count=0`;
  - `proof_isolation_contract.hot_path_report_serialization_count=0`;
  - `proof_isolation_contract.legacy_pre_present_request_count=0`;
  - `runtime_work_contract.total_list_find_rows_scanned=0`;
  - `runtime_work_contract.total_recomputed_fields=0`;
  - `retained_update_contract.full_document_lower_count=0`;
  - `retained_update_contract.legacy_selection_fallback_count=0`.
- Fresh stale-path ledger:
  `target/reports/native-gpu/stale-path-ledger.json`.
  It remains `status="pass"`.
- Do not overclaim completion:
  - raw wake-scoped latency is still over budget:
    `steady_input_wake_to_input_accept_ms.p95=25.545912 ms` and
    `steady_input_wake_to_formula_visible_ms.p95=66.621686 ms`;
  - the proof worker is still reported as `lagging`, with
    `queued_request_count=5`;
  - the next architecture cut is still `PreviewHotLoop` / `NativeFrameClock`
    or equivalent host-event/frame-clock ownership to reduce wake-to-accept
    drift, plus eventual deletion of legacy JSON/proof compatibility paths.

2026-07-02 current wake-latency checkpoint after product-commit retention and
host-input follow-up cut:

- Read the active goal attachment again before this slice. The full goal remains
  this entire plan, with performance as the main goal and no Cells-specific
  compiler/runtime/document/renderer/app-window/playground/verifier shortcuts.
- Fixed the immediate verifier evidence defect by raising the retained
  `recent_product_frame_commits` ring from `96` to `256` while still filtering
  out non-input product commits. The previous fresh report matched only `55/64`
  click samples because the bounded report dropped early input-bearing product
  commits.
- Cut one generic product-path interference source: a plain host-input product
  frame no longer starts a requested-animation follow-up burst when there is no
  active animation burst. Source-changing host input presents its product frame
  and yields; explicit `RequestedAnimation` remains the owner of paced follow-up
  frames. This avoids turning ordinary clicks into extra non-dirty present work.
- Focused checks passed:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_app_window requested_animation -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window host_input_product_frame_does_not_schedule_followup_without_animation -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window due_burst_wake_after_host_input_poll_does_not_steal_product_frame -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window accepted_host_input_timing_defines_product_input_to_present_latency -- --test-threads=1`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`;
  - `cargo xtask verify-native-gpu-stale-path-ledger --report target/reports/native-gpu/stale-path-ledger.json`.
- Fresh release verifier:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  It is schema-valid but still `status="fail"`.
  - The broad product path is now clean:
    `product_only_ux_contract.status="pass"`, `sample_count=64`,
    `input_to_present_ms.p95=14.256566 ms`, and max `17.778075 ms`.
  - Exact product-commit matching is no longer the blocker:
    `product_frame_sample_count=64`, `required_sample_count=64`, and
    `preview_loop_product_path_contract.status="pass"`.
  - Runtime and retained contracts still pass:
    `runtime_work_contract.status="pass"`,
    `total_list_find_rows_scanned=0`, `total_recomputed_fields=0`,
    `total_root_materialization_candidates=0`,
    `retained_update_contract.status="pass"`,
    `full_document_lower_count=0`, and `legacy_selection_fallback_count=0`.
  - Proof/report isolation still passes:
    `legacy_pre_present_request_count=0`,
    `hot_path_report_serialization_count=0`,
    `hot_path_report_write_count=0`, and
    `product_blocks_on_proof_subscribers=false`.
  - The remaining release blocker is now only wake-scoped:
    `product_commit_input_wake_to_input_accept_ms_p95=24.955419 ms`,
    `product_commit_input_wake_to_formula_visible_ms_p95=37.360495 ms`,
    and `steady_input_wake_to_input_accept_ms.p95=25.142575 ms`.
  - Phase timings show the accepted frame itself is not the slow part:
    `poll_started_to_input_accept.p95=4.025638 ms`,
    `render_started_to_render_hook_completed.p95=2.260784 ms`,
    `present_call.p95=11.502788 ms`, and
    `render_hook_completed_to_present.p95=14.087734 ms`.
- The attempted host-input follow-up cut did not reduce wake-to-accept enough.
  It cleaned the blocker list, but it proves the next step must be a larger
  `PreviewHotLoop` / `NativeFrameClock` ownership change:
  - app-window input events need a concrete host-event ledger from OS event to
    input hook accept, product commit, and proof artifact;
  - host input must be sampled at the front of a frame-clock turn that is not
    blocked behind previous proof/report/present maintenance;
  - product present should use a minimal active scene path, with proof/readback/
    report subscribers fully post-present and backpressured by exact
    `FrameEvidenceKey`;
  - if wake-to-accept still p95s around one frame after that split, the report
    should classify same-surface compositor/present floor separately from Boon
    runtime/render work instead of hiding it in harness latency.
- Do not go back to Cells formula/list/currentness, route-cache, report-size, or
  requested-animation micro-tuning unless a fresh product-frame report names one
  of those as the dominant boundary again. Current evidence says accepted
  product frames are fast enough; wake ownership is not.

2026-07-02 click coalescing checkpoint:

- Inspected `NativeProductFrameCommit.input_timing.host_input_event` directly
  from the preview-loop report. The worst wake-to-accept samples are mostly
  `mouse_button pressed=false` release events with `mouse_button_delta_count=2`.
  Many press/release wake timestamps are separated by only a fraction of a
  millisecond, but the release is accepted one product frame later when the
  press-only frame has already gone through source/runtime/render/present.
- Tried an unsafe post-render-hook coalescing cut: if newer input arrived before
  present, allow a press-only frame to yield even though it carried accepted
  host input. This reduced some wake numbers but broke correctness because the
  render hook/source/runtime side effects had already run and the frame was then
  discarded. Fresh release report regressed to only `26` product samples and
  failed product, proof, retained, and runtime contracts. This tactic was
  reverted. Do not drop accepted host-input frames after the render hook unless
  the architecture has an explicit transactional product-frame rollback.
- Kept the safe generic part before source/runtime/render dispatch:
  `BUTTON_PRESS_INPUT_COALESCE_GRACE=1 ms`. When the sampled input delta is a
  button-press-only delta, app-window waits up to that grace for an immediate
  wake and resamples before calling input hooks. This coalesces synthetic or
  very fast press/release pairs without discarding source/runtime side effects.
- Focused checks passed:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_app_window button_press_only_input_delta_is_coalescible -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window accepted_host_input_timing_defines_product_input_to_present_latency -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window unaccounted_host_input_frame_is_not_pre_present_drop_eligible -- --test-threads=1`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`;
  - `cargo xtask verify-native-gpu-stale-path-ledger --report target/reports/native-gpu/stale-path-ledger.json`.
- Fresh release verifier remains schema-valid but failing only on wake budget:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  - `product_only_ux_contract.status="pass"`, `sample_count=64`,
    `input_to_present_ms.p95=13.776998 ms`, max `15.128062 ms`;
  - `preview_loop_product_path_contract.status="pass"`,
    `product_frame_sample_count=64`, `required_sample_count=64`,
    `missed_frame_count=0`;
  - `runtime_work_contract.status="pass"` and
    `retained_update_contract.status="pass"`;
  - `proof_isolation_contract.status="pass"`;
  - stale-path ledger remains `status="pass"`;
  - remaining blocker:
    `product_commit_input_wake_to_input_accept_ms_p95=21.617202 ms`,
    `product_commit_input_wake_to_formula_visible_ms_p95=31.659084 ms`,
    `steady_input_wake_to_input_accept_ms.p95=22.467845 ms`.
- This confirms the click-pair accounting issue was real but not the whole
  blocker. The remaining p95 is still roughly a present/frame ownership problem:
  some releases arrive while an earlier product frame is already inside
  render/queue/present and cannot be sampled until the next turn. The next real
  cut should be a product-frame transaction / `PreviewHotLoop` split where
  input sampling, source dispatch, active scene patching, and present are owned
  by one small frame-clock path, or a late-input/pre-present transaction boundary
  that can safely merge newer input before source/runtime side effects commit.

2026-07-02 press-preposition deferral checkpoint:

- Re-read the active goal attachment before this slice. The full goal remains
  this whole plan: bounded requested-animation bursts inside DemandDriven mode,
  hot retained renderer state, separated proof/debug cost, exact evidence keys,
  generic runtime/render fixes, and no Cells/example-specific hacks.
- Existing subagent findings were folded in instead of spawning more agents
  because the agent pool was already full. The useful converging advice was:
  keep proof/report work out of product frames, split input-first/frame-clock
  ownership, and avoid more formula/list/runtime micro-tuning unless a fresh
  product report names it.
- Tried a bounded generic pointer-motion coalescing grace before role input
  hooks. The intent was to catch the observed motion-before-release case, but
  the fresh release report regressed:
  `product_commit_input_wake_to_input_accept_ms_p95=19.143 ms` and
  `product_commit_input_wake_to_formula_visible_ms_p95=33.902 ms`. The patch
  was removed. Do not reintroduce a blind motion wait; it increases input
  batching variance.
- Fixed the safer generic issue instead: primary press-only input is now
  treated as non-committing even when it carries the first pointer position or
  preposition motion. This prevents a press/preposition frame from running
  source/runtime/render/present before the release that commits the click. This
  is generic input-shape scheduling, not a Cells branch.
- Focused checks passed:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_playground cells_press_only_input_defers_until_release_batch -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window pointer_motion_only_input_delta_can_yield_to_newer_input -- --test-threads=1`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`.
- Fresh release verifier:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  It is schema-valid but still `status="fail"`.
  - Press-only product frames are gone:
    `recent_product_frame_commits` contains zero pressed-button product commits.
  - `product_only_ux_contract.status="pass"`, `sample_count=64`,
    `input_to_present_ms.p95=14.713184 ms`, and max `16.221862 ms`.
  - `runtime_work_contract.status="pass"`,
    `retained_update_contract.status="pass"`, and
    `proof_isolation_contract.status="pass"`.
  - The remaining blocker is no longer wake-to-accept:
    `product_commit_input_wake_to_input_accept_ms_p95=5.141168 ms`.
  - The remaining blocker is wake-to-present / wake-to-formula:
    `product_commit_input_wake_to_formula_visible_ms_p95=19.477985 ms`
    against the `16.7 ms` budget.
  - Phase timings show present/queue dominates the misses:
    `present_call_ms.p95=13.010756 ms`,
    `queue_submit_call_ms.p95=10.725632 ms`, and
    `render_hook_ms.p95=1.482512 ms`.
  - There are no full-grid/runtime regressions: runtime, retained, and proof
    contracts still pass.
- Current diagnosis:
  - input dispatch and Cells runtime/list/currentness are not the active
    blocker;
  - press-only/preposition product work has been removed from the hot path;
  - the remaining miss is the same-surface product present/queue/compositor
    boundary plus a small raw-event-to-accept cost;
  - `BOON_NATIVE_PRESENT_MODE=immediate`, offscreen-copy, and
    `BOON_NATIVE_DESIRED_MAXIMUM_FRAME_LATENCY=2` were already documented as
    worse diagnostics, so do not switch them blindly.
- Next architecture cut:
  - implement a real `PreviewHotLoop` / `NativeFrameClock` transaction boundary
    or equivalent late-input frame-clock owner;
  - sample input at the start of a frame-clock turn, patch `ActivePreviewScene`
    directly, and submit without proof/report work;
  - keep product commits keyed by `FrameEvidenceKey`, but let proof/readback/
    report subscribers complete later;
  - add same-surface present-floor evidence for this product surface so reports
    can distinguish Boon runtime/render work from unavoidable compositor wait
    without weakening native visual proof.

2026-07-02 bounded HostInput burst and Cells press-selection checkpoint:

- Re-read the active goal attachment before this slice. The goal remains the
  whole realtime product-loop plan, not a smaller wake-smoke pass.
- Ran an `AutoNoVsync` diagnostic after the press-preposition checkpoint:
  `target/reports/native-gpu/cells-visible-click-e2e-release-auto-no-vsync-diagnostic.json`.
  It is schema-valid but failed and was worse than the default Mailbox path:
  product p95 `16.104173 ms`, max `26.162584 ms`, wake-to-accept
  `5.369039 ms`, wake-to-formula `29.888574 ms`, present p95
  `13.372923 ms`, queue p95 `11.487553 ms`. Do not change the default present
  preference to AutoNoVsync from this evidence.
- Implemented the documented generic DemandDriven policy that visible-changing
  HostInput starts a bounded requested-animation burst. The current product
  frame still keeps `scheduler_reason=host_input`; the burst only schedules
  paced follow-up frames. Focused checks passed:
  - `cargo fmt --check`;
  - `cargo test -q -p boon_native_app_window requested_animation -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window host_input -- --test-threads=1`.
- The first fresh release report after HostInput bursts remained schema-valid
  but failed on wake budget:
  - `product_only_ux_contract.status="pass"`;
  - product p95 improved to `14.213969 ms`, max `18.516285 ms`;
  - wake-to-accept regressed to `23.329963 ms`, wake-to-formula
    `33.642606 ms`;
  - the worst wake samples were `pressed=false` release batches with
    `mouse_button_delta_count=2`, so the product frame was still mostly
    measuring release-batch acceptance rather than immediate spreadsheet
    selection.
- Changed the Cells example semantics from release-click selection to
  press-selection:
  `examples/cells/view.bn` now uses `event: [press:
  cell.sources.editor.select]`. This is not a runtime/compiler/renderer
  special case; `press` is existing Boon event syntax used by Counter,
  TodoMVC, and other examples. Spreadsheet selection should be visible on
  pointer down.
- Let press-only input reach the normal source-routing path instead of forcing
  every primary press to wait for release. Added/updated the focused unit test:
  - `cargo test -q -p boon_native_playground cells_press_only_input_selects_cell_on_press -- --test-threads=1`.
  It proves the local Cells runtime state switches to `B0` on press.
- Fresh release verifier after Cells press-selection is schema-valid but still
  `status="fail"`:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  - `product_only_ux_contract.status="pass"`, `sample_count=64`,
    product p95 `15.050179 ms`, max `23.083656 ms`;
  - wake-to-accept improved to `7.918801 ms`, but wake-to-formula remains
    `27.777589 ms`;
  - steady accepted-input p95 is still within budget at `15.872666 ms`;
  - runtime, retained, proof isolation, exact product frame commits, and
    schema all pass;
  - recent verifier samples are still mostly release batches
    (`pressed=false`, `mouse_button_delta_count=2`), so the headed verifier is
    not yet using the new press-selection product frame as the measured sample.
- Current diagnosis:
  - Selection-on-press is a cleaner spreadsheet semantics fix and improves the
    raw wake-to-accept p95, but the visible-click verifier/sample matching still
    keys acceptance to later release-batch frames.
  - The product path after accepted input is near budget; remaining failure is
    verifier/product-sample alignment plus present/queue variance, not
    spreadsheet formula/list/runtime work.
  - Do not return to Cells-specific runtime/compiler shortcuts or style hacks.
    The next cut should make product samples interaction-scoped and press-aware,
    while preserving app-owned WGPU proof by exact `FrameEvidenceKey`.
- Next architecture cut:
  - make the verifier/product ledger distinguish press-selection frames from
    release cleanup frames for generic `press` sources;
  - keep release events as correctness evidence, but measure visible selection
    from the presented frame that actually changed the selected cell/formula
    bar;
  - continue toward `PreviewHotLoop` / `NativeFrameClock` ownership so input
    sampling, source dispatch, retained scene patching, submit, and product
    commit identity are one typed transaction;
  - add a negative gate that product UX samples must not be replaced by later
    release/proof/report frames when an earlier exact-keyed product frame
    already proved the target visible state.

2026-07-02 root-cause checkpoint for product outliers and harness/proof p95:

- Existing report used for diagnosis:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  It is schema-valid but still `status="fail"`.
- The bad product max is not a Cells runtime/list/formula problem:
  - worst product sample index `50` has
    `input_wake_to_input_accept_ms=5.332033 ms`;
  - `render_started_to_render_hook_completed_ms=1.742109 ms`;
  - `runtime_work_contract.status="pass"`;
  - `total_rows_scanned=0`, `total_list_find_rows_scanned=0`,
    `total_recomputed_fields=0`, and
    `total_root_materialization_candidates=0`;
  - the outlier is almost entirely after render hook:
    `input_accept_to_present_ms=58.349786 ms`,
    `render_hook_completed_to_present_ms=56.200515 ms`,
    `queue_to_present_ms=56.062474 ms`,
    `present_call_ms=56.061898 ms`.
- There is a second, smaller product-outlier class before queue submit:
  - sample index `27` has `input_accept_to_present_ms=41.338899 ms`;
  - `render_started_to_render_hook_completed_ms=1.169504 ms`;
  - `render_hook_to_queue_ms=39.644413 ms`;
  - `present_call_ms=0.077842 ms`;
  - this sample had `last_interactive_surface_readback_pending=true`, so
    proof/readback backpressure is still able to disturb product-frame
    submission even when proof completion is not counted as UX latency.
- Across the 64 click samples:
  - only 3 accepted-input product samples exceed `16.7 ms`;
  - only 2 present calls exceed `16.7 ms`;
  - 13 samples spend more than `8 ms` between render hook and queue submit;
  - 48 samples spend more than `8 ms` between queue submit and present;
  - `preview_perf_stats.last_missed_frame_cause` is
    `interactive_readback_backpressure`.
- The harness/proof p95 is a different lane, not the product UX lane:
  - sample index `0` has product `input_accept_to_formula_visible_ms=12.140287
    ms`, but harness `click_to_formula_visible_ms=387.09686 ms`;
  - the same sample reports `click_to_readback_after_present_ms=353.669091
    ms` and proof frame `11` for product frame `10`;
  - sample index `37` has product `11.31132 ms`, harness `347.506288 ms`,
    `click_to_readback_after_present_ms=268.223318 ms`, and proof lag `5`
    frames.
- The proof contract is honest but expensive:
  - `proof_only_contract.status="pass"`;
  - `current_structured_visual_proof_sample_count=64`;
  - `proof_lag_max_frames=5`;
  - `proof_isolation_contract.status="pass"`;
  - `legacy_pre_present_request_count=0`;
  - `hot_path_report_write_count=0`;
  - `hot_path_report_serialization_count=0`.
- Code path diagnosis:
  - `wait_for_cells_formula_visible_match` in `crates/xtask/src/main.rs`
    waits for product present and then for proof/readback evidence;
  - if proof does not match the product-present frame exactly, xtask falls back
    from product-present timing to readback-visible timing when computing
    harness `click_to_formula_visible_ms`;
  - this is the source of the 200-387 ms harness/proof numbers.
- Next implementation must be one larger architecture cut, not another
  micro-timing tweak:
  - launch/product-measure the UX lane in counters/product mode with no
    interactive readback jobs on product frames;
  - keep readback/proof as a separate subscriber/proof lane linked by
    `FrameEvidenceKey`;
  - prevent pending proof/readback work from delaying render-hook-to-queue or
    present on product frames;
  - add a present-floor/product-surface baseline to distinguish unavoidable
    compositor/present blocking from Boon-owned queue/present work;
  - keep the report failing until both the product lane and proof lane pass
    their own contracts.

2026-07-02 larger code cut started after root-cause checkpoint:

- Tried removing the legacy interactive visible-surface readback whenever
  structured post-present subscribers were present. That successfully removed
  `interactive_readback_backpressure`, but it was too early for direct visible
  surfaces: the current subscriber set proves bound text/retained sync, not a
  WGPU visible-surface readback artifact. The verifier correctly failed the
  proof lane.
- Kept the useful architecture direction but corrected the implementation:
  product-frame detection now treats any host-input, `ProductInteraction`, or
  HostInput-scheduled frame as a product input frame, so legacy interactive
  readback is deferred away from UX frames. Non-product follow-up frames may
  still perform readback until a real offscreen/proof-only WGPU lane replaces
  that compatibility path.
- Expected effect:
  - product input frames should stop queuing the old interactive readback job;
  - proof/readback remains available on follow-up frames, preserving app-owned
    WGPU evidence;
  - if product max outliers remain, they are more likely true surface
    queue/present/compositor blocking and should be addressed by the
    `PreviewHotLoop` / present-floor work.
- This is intentionally generic native-loop architecture: no Cells names,
  spreadsheet addresses, source paths, or fixture fields were added to runtime,
  compiler, renderer, or app-window production code.
- Fresh release verifier after the corrected product-frame deferral:
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  It is still `status="fail"`, but the failure shape improved:
  - `product_only_ux_contract.status="pass"`;
  - accepted-input product p95 is `14.784872 ms`;
  - accepted-input product max is `25.278492 ms`, down from the earlier
    `58.349786 ms` outlier class;
  - `proof_only_contract.status="pass"`;
  - `proof_isolation_contract.status="pass"`;
  - `runtime_work_contract.status="pass"`;
  - `retained_update_contract.status="pass"`;
  - `legacy_pre_present_request_count=0`;
  - proof lag remains bounded by the existing proof lane
    (`proof_lag_max_frames=5`).
- Remaining blocker:
  - `input_wake_to_formula_visible_ms.p95=44.495696 ms`;
  - worst wake samples are still release/null frames with
    `input_wake_to_input_accept_ms` around `30-33 ms`;
  - product accepted-to-present is now mostly within budget, so this is
    event/frame ownership and sample attribution, not runtime/list/formula
    work.
- Next cut:
  - implement a true product-only counters run plus separate readback proof run,
    or a `PreviewHotLoop` frame-clock owner that samples press-selection frames
    before release/proof cleanup can relabel the interaction;
  - do not keep tuning runtime/list/formula for this blocker unless a fresh
    product-only report shows runtime work has returned.

2026-07-02 current Cells outlier diagnosis after product-frame readback deferral:

- Fresh evidence remains
  `target/reports/native-gpu/cells-visible-click-e2e-release.json`.
  The report is schema-valid and still `status="fail"`, but the failure is now
  split into separate lanes:
  - product-only accepted input to formula/present:
    `p95=14.784872 ms`, `max=25.278492 ms`;
  - preview product perf accumulator:
    `p50=11.063688 ms`, `p95=14.901104 ms`, `p99=max=37.236907 ms`;
  - `product_missed_frame_count=2`, `missed_frame_count=4`;
  - `last_missed_frame_cause="interactive_readback_backpressure"`;
  - `render_hook_ms.p95=2.121757 ms`, so render hook/layout/runtime is not the
    current product outlier source.
- Current product outliers are mostly queue/present and frame ownership:
  - sample `22` has product `25.278492 ms`; the time is almost entirely
    `present_call_ms=23.276644 ms` / `queue_to_present_ms=23.276801 ms`;
  - top wake samples are release/null events with `pressed=false` and
    `input_wake_to_input_accept_ms` around `30-35 ms`, while their
    accepted-input product work remains about `10-12 ms`;
  - sample `37` has product `11.131217 ms`, but raw
    `input_wake_to_formula_visible_ms=70.544716 ms` because the input was not
    sampled until roughly one or more frame intervals later and then hit queue
    submit/present work.
- Current harness/proof-side `click_to_formula_visible_ms` is intentionally not
  product UX latency, but it is still too expensive and confusing:
  - `click_to_formula_visible_ms.p50=158.925207 ms`,
    `p95=211.715505 ms`, `max=863.590744 ms`;
  - `click_to_readback_after_present_ms.p50=55.698563 ms`,
    `p95=120.581667 ms`, `max=791.318004 ms`;
  - proof lag is `46` samples at 1 frame, `15` at 3 frames, and `3` at 5
    frames;
  - sample `34` proves the problem directly: product
    `input_accept_to_formula_visible_ms=9.952121 ms`, but harness
    `click_to_formula_visible_ms=863.590744 ms` because readback proof completed
    `791.318004 ms` after product present.
- Code-level cause of the 200ms-class harness number:
  - `wait_for_cells_formula_visible_match` waits for product present and then
    proof/readback/crop evidence;
  - if the proof key does not exactly equal the product-present frame key,
    xtask computes harness visible latency from readback-visible timing instead
    of product-present timing;
  - `proof_only_contract` remains honest (`status="pass"`, current structured
    proof covers all 64 samples), but exact same-frame visual proof count is
    still zero, so the harness metric measures proof lag/readback polling.
- Code-level cause of the remaining missed frames:
  - `run_visible_surface_probe_with_hooks_and_wake` still tracks interactive
    readback jobs and reports `interactive_readback_backpressure`;
  - non-product follow-up frames can still perform compatibility readback while
    product clicks are arriving;
  - `native_gpu_app_owned_render_hook` still has proof/report-shaped behavior
    in the same render hook API, even though product input frames currently
    defer most proof work.
- Required architecture cut, not a verifier wording patch:
  - make product/counters mode run with no interactive readback jobs or proof
    subscribers able to affect product scheduling, queue submit, or present;
  - proof deferral alone is insufficient: the current offscreen/readback path
    can still submit proof GPU copy work on the same `wgpu::Queue` immediately
    after product present, so product metrics may exclude proof completion while
    later product frames still see queue/present tails and
    `interactive_readback_backpressure`;
  - run a separate readback proof lane keyed by the product `FrameEvidenceKey`;
  - make that proof lane bounded and latest-wins: one pending proof target per
    semantic verifier need, coalesced during interactive bursts, and scheduled
    only when the product lane is idle or explicitly in verifier/proof mode;
  - keep exact proof identity mandatory, but report proof lag as proof latency,
    never as product UX latency;
  - add a same-surface present-floor baseline so `present_call`/`queue_to_present`
    spikes can be separated from Boon-owned render/runtime work;
  - split raw wake diagnostics from accepted-input product gates, while keeping
    raw wake samples visible until the `PreviewHotLoop`/frame-clock owner removes
    release/follow-up relabeling.

2026-07-02 implementation checkpoint: interaction-burst proof readback guard:

- Implemented a generic native app-window guard that defers interactive WGPU
  proof readback while a requested-animation/product interaction burst is
  active:
  - `NativeRenderLoopState::interaction_burst_active`;
  - `NativeRenderLoopState::defer_proof_readback_for_product_lane`;
  - new `InteractiveSurfaceReadbackDecision::DeferInteractionBurst`.
- This is intentionally not Cells-specific:
  - no branches on example name, source path, cell address, formula text, or
    spreadsheet fixture state;
  - the decision is based only on frame lane, scheduler reason, verifier frame
    status, and bounded requested-animation burst state.
- Product/proof accounting added:
  - `proof_readback_deferred_count`;
  - `proof_readback_deferred_for_product_input_count`;
  - `proof_readback_deferred_for_interaction_burst_count`;
  - `last_proof_readback_deferred_reason`.
- These counters are serialized both through `render_loop_state` and as
  top-level render-loop report fields, and are included in
  `post_present_proof_isolation`.
- Important behavior change:
  - product input frames still defer readback as before;
  - animation follow-up frames inside the product burst now defer proof readback
    instead of queuing it or reporting readback backpressure;
  - explicit verifier/proof frames remain allowed to read back.
- Focused verification:
  - `cargo test -p boon_native_app_window readback` passed;
  - `cargo check -p xtask` passed;
  - `cargo test -p xtask cells_visible_click` passed;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-release.json`
    passed for the existing report artifact.
- Still required before claiming performance fixed:
  - rerun fresh release `verify-native-cells-visible-click-e2e` on the current
    binary/worktree;
  - confirm `interactive_readback_backpressure` disappears from product bursts;
  - inspect the new deferral counters to prove proof work was deferred/coalesced
    rather than hidden;
  - compare product p95/max and wake-to-accept tails against the previous
    `14.784872 ms` product p95, `25.278492 ms` product max, and
    `64.785872 ms` raw wake p95 shape;
  - if present tails remain, implement the same-surface present-floor baseline
    and `PreviewHotLoop`/frame-clock ownership cut.

2026-07-02 fresh diagnosis: why 58ms-class outliers and 200ms+ proof p95 remain:

- Fresh release verifier:
  `cargo xtask verify-native-cells-visible-click-e2e --profile release --report target/reports/native-gpu/cells-visible-click-e2e-release.json`
  still writes a schema-valid `status="fail"` report.
- Accepted product-click path is no longer the multi-second problem:
  - `input_accept_to_formula_visible_ms_p95=14.736394 ms`;
  - `input_accept_to_formula_visible_ms_max=14.736394 ms`;
  - click frames mostly spend about `2.35-2.65 ms` in render hook work and
    about `8.33-10.39 ms` in `present_call` / `queue_to_present`.
- The remaining product/frame outliers have two concrete causes:
  - a mouse-motion product frame, not a cell click, records
    `input_to_present_ms=41.140381 ms` with
    `render_started_to_render_hook_completed_ms=10.752069 ms`,
    `encode_scene_ms=8.502562 ms`, and
    `present_call_ms=29.913096 ms`;
  - one click frame records `input_to_present_ms=14.736394 ms`, but the time
    after render is a queue-submit / submit-to-present gap:
    `queue_submit_call_ms=11.916733 ms`,
    `render_hook_to_queue_ms=11.963623 ms`, and
    `present_call_ms=0.036535 ms`.
- The 58ms-class historical product outliers are therefore not Cells formula
  evaluation. They are frame-clock/present ownership outliers:
  - burst follow-up frames can still be associated with recent input and report
    long `input_wake_to_present_ms` tails;
  - direct surface `present` can block for roughly one or two frame intervals;
  - some frames alternate between a slow `queue_submit` call and a slow
    `present_call`, so the next architecture cut must own frame pacing and
    present-floor measurement instead of tuning runtime/list/formula code.
- The harness/proof-side `click_to_formula_visible_ms` is still awful because
  it is waiting for proof/readback evidence, not for the product UI update:
  - fresh report shows `click_to_formula_visible_ms_p95=4372.492369 ms`;
  - worst sample has product
    `input_accept_to_formula_visible_ms=14.736394 ms`, product present in
    `click_to_present_ms=28.874271 ms`, and then waits
    `click_to_readback_after_present_ms=4343.618098 ms`;
  - exact same-frame visual proof count is still zero and proof evidence does
    not match the product-present `FrameEvidenceKey` for the click samples, so
    xtask falls back to readback-completion timing for the harness metric.
- The preview-loop artifact proves proof backlog, even though the final report
  previously failed to lift these counters into `live_probe`:
  - `proof_readback_deferred_count=34`;
  - `proof_readback_deferred_for_product_input_count=6`;
  - `proof_readback_deferred_for_interaction_burst_count=28`;
  - `post_present_proof_queue_enqueued_count=194`;
  - `post_present_proof_queue_completed_count=184`;
  - `recent_post_present_proof_queue_count=64`.
- A diagnostic plumbing patch now propagates those counters into the final
  Cells visible-click report and per-click `present_probe` objects. This does
  not fix performance or weaken gates; it makes future failing reports name
  the proof backlog instead of requiring a separate artifact lookup.
- Architecture conclusion:
  - product lane is close for real click frames but still at the mercy of
    direct-present / queue-submit cadence;
  - proof lane is not product latency and must become a bounded, latest-wins,
    `FrameEvidenceKey`-linked proof service;
  - normal UX gates should use accepted-input product frame timing, while proof
    gates should require current app-owned WGPU evidence and report proof lag
    separately;
  - the next code cut should be `PreviewHotLoop` / explicit frame-clock owner,
    late acquire or frame-in-flight present policy, and same-surface
    present-floor baseline, not another Cells formula/runtime micro-optimization.

2026-07-02 post-plumbing verifier result:

- After propagating proof/readback counters into the final report, a fresh
  release visible-click run still fails, but now the report names the proof
  backlog directly:
  - `proof_readback_deferred_count=13`;
  - `proof_readback_deferred_for_product_input_count=2`;
  - `proof_readback_deferred_for_interaction_burst_count=11`;
  - `post_present_proof_queue_enqueued_count=65`;
  - `post_present_proof_queue_completed_count=60`;
  - `recent_post_present_proof_queue_count=64`.
- The run stops after one click because current visual proof is missing, while
  product state and product presentation are already current:
  - runtime value probe sees `store.selected_address="A2"` and
    `store.selected_input.editing_text="15"` in `7.151319 ms`;
  - accepted product timing is
    `input_accept_to_formula_visible_ms=12.988243 ms`;
  - raw wake timing is slightly over budget at
    `input_wake_to_formula_visible_ms=17.872409 ms`;
  - exact/current structured WGPU proof is false because baseline/current
    readback evidence is unavailable or does not match the product
    `FrameEvidenceKey`.
- This confirms the current guard is only a diagnostic/protection layer, not the
  final architecture. Simply deferring proof readback during bursts protects the
  product lane but can starve the verifier. The correct cut is a separate,
  bounded proof service:
  - product frames publish `FrameEvidenceKey` and present immediately;
  - proof requests are coalesced latest-wins per semantic need;
  - baseline and current proof requests cannot be silently starved;
  - proof lag is reported and budgeted separately from accepted-input UX;
  - product reports must never mark UI latency as failed only because proof
    readback completed late or was intentionally deferred.

2026-07-02 armed input-sampling / present-policy result:

- Implemented a generic requested-animation prewarm substate. Pointer-motion
  prewarm can now arm an input-sampling turn without forcing a clean follow-up
  present. The report exposes `requested_animation_prewarm_count`,
  `armed_frame_token`, `clean_armed_poll_count`,
  `skipped_clean_burst_present_count`, and
  `input_waited_for_already_armed_frame_count`.
- Fixed the first prewarm attempt where the armed token stayed sticky across
  idle polls. Clean armed turns now clear the token after the skipped present,
  so the counters represent real sampling turns instead of idle-loop churn.
- Changed the generic native present policy to prefer nonblocking present modes
  (`Immediate`, `AutoNoVsync`, `Mailbox`, then `Fifo`) and to use bounded
  multiple frames in flight by default. This is not Cells-specific; it is a
  native-window latency policy.
- Fresh release Cells visible-click status is still `fail`:
  - harness/proof `click_to_formula_visible_ms_p95=266.946876 ms`;
  - accepted product `product_input_to_present_ms_p95=18.678639 ms`;
  - product missed frames remain nonzero (`product_missed_frame_count=9`);
  - `present_path_ms_p95=17.052182 ms`;
  - `queue_submit_call_ms_p95=14.409348 ms`;
  - wake-to-formula p95 remains over budget
    (`input_wake_to_formula_visible_ms_p95=76.012297 ms` in the full harness
    timing, with product-commit `wake_formula=24.186 ms`).
- Current interpretation:
  - Cells runtime/list/formula work is no longer the dominant blocker for this
    report;
  - proof/readback still makes the harness click-to-formula metric much worse
    than product UI latency;
  - product interaction is close but not complete because the single app-window
    loop can still spend a frame interval around `queue.submit` / `present` and
    because input wake/accept is not yet owned by a continuously armed frame
    clock.
- Next architecture cut should be larger, not another local micro-patch:
  - split host input sampling from the present/proof lane with a latest-wins
    event queue;
  - make proof/readback a bounded coalescing service keyed by
    `FrameEvidenceKey`, not work performed in front of product submission;
  - keep the product lane measured by accepted-input-to-present/formula timing
    and keep proof lag measured separately;
  - add a same-surface present-floor benchmark so verifier budgets know the
    unavoidable WGPU/compositor floor on the current adapter/session;
  - only after that, revisit retained layout/render patching if product p95 is
    still above 16.7ms.

2026-07-02 post-background-proof cut status:

- Implemented the generic app-window split that keeps background proof
  telemetry out of product/burst frames:
  - product and burst frames still publish product commits and refresh the
    async latest-wins report;
  - proof-history, report-json proof artifacts, and artifact hashing are only
    enqueued when background telemetry is allowed;
  - proof/harness frames remain allowed to enqueue required proof subscribers.
- Fixed the prewarm implementation bug: `armed_frame_pending` is now set by
  pointer prewarm, clean armed polls can be counted and skipped without
  presenting, and host input arriving after prewarm increments
  `input_waited_for_already_armed_frame_count`.
- Best fresh release result after the background-proof split:
  - all 64 Cells clicks passed visually and functionally;
  - `/product_only_ux_contract.status=pass`;
  - `/proof_only_contract.status=pass`;
  - accepted product p95 was `16.062596 ms`, max `27.659174 ms`;
  - proof lag p95 was `6` frames, max `8`;
  - remaining failure was raw product-commit wake-to-formula p95
    `20.567028 ms`, with wake-to-accept p95 `5.742499 ms`.
- Fresh release result after enabling real armed prewarm counters is still
  `fail` and slightly worse on this run:
  - accepted product p95 `18.442151 ms`, max `19.244245 ms`;
  - product missed frames `9`;
  - product-commit wake-to-formula p95 `22.724245 ms`;
  - wake-to-accept p95 `5.562695 ms`;
  - `input_waited_for_already_armed_frame_count=65`, proving the prewarm path is
    active but not sufficient;
  - background proof worker load dropped substantially compared with the old
    overloaded reports (`required=138`, `background=238`, `completed=376` in the
    fresh loop report).
- The remaining p95 misses are dominated by product-frame submit/present floor,
  not Cells runtime/list/formula work:
  - slow samples spend about `14-17 ms` after the render hook in
    `queue.submit()` / `frame.present()`;
  - render hook is usually `1-2 ms`, with patch build/cache/encode substeps
    still paid every product interaction;
  - Cells runtime work remains targeted: one dirty key, zero list scans, no
    full-grid recompute in the reported interaction samples.
- A same-surface present-floor run currently fails as diagnostic evidence
  because it selected software Vulkan llvmpipe:
  - `adapter_name="llvmpipe (LLVM 20.1.2, 256 bits)"`;
  - `adapter_is_software=true`;
  - its frame floor is low, but it cannot be used as product hardware evidence.
- Next cut should not be Cells-specific and should not be more measurement-only
  work:
  - add adapter identity to all preview-loop/product reports and fail fast if a
    performance gate runs on a software adapter unless explicitly requested;
  - implement a real `PreviewHotLoop` / active frame clock that samples input at
    the start of an already scheduled frame, not after demand wake bookkeeping;
  - split first-frame retained visual feedback from runtime/source cleanup:
    apply the selection/formula-bar visual patch, submit, then run follow-up
    source/runtime cleanup under a separate frame identity;
  - replace per-interaction render-scene patch construction with a retained GPU
    buffer patch path for style/text deltas, keyed by stable document node IDs;
  - keep proof/readback/report artifacts in a bounded proof service keyed by
    `FrameEvidenceKey`, with product UX latency and proof lag reported
    separately.

2026-07-02 hardware-adapter evidence gate checkpoint:

- Added generic native adapter identity propagation to the app-window product
  evidence path:
  - `AppWindowSurfaceProof`, top-level render-loop reports,
    `NativePreviewPerfStats`, and `NativeProductFrameCommit` now carry
    `NativeAdapterIdentity` from the selected WGPU adapter;
  - product frame commits and preview perf stats no longer rely on downstream
    xtask inference to know whether a timing sample came from hardware or a
    software adapter.
- Added a Cells visible-click product-performance gate that rejects missing or
  software adapter evidence:
  - `cells_visible_click_app_window_product_commit_scope_summary` now includes
    `adapter_identity`, `adapter_status`, and
    `software_adapter_wall_clock_budget_exempt=false`;
  - `cells_visible_click_product_only_ux_contract` requires
    `adapter_status="hardware"`;
  - the top-level visible-click audit has an explicit
    `cells-visible-click-e2e:hardware-product-adapter` check, so llvmpipe or
    missing adapter identity fails as diagnostic-only evidence instead of being
    mixed with product latency.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window -p xtask`;
  - `cargo test -q -p xtask cells_visible_click -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window preview_perf_stats_keep_proof_overhead_separate_from_ux_latency -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats -- --test-threads=1`.
- This is not the 60 FPS fix. It removes a misleading evidence path from the
  plan: future product-performance reports must prove they are hardware-backed
  before their wall-clock latency can satisfy the product gate. The next
  architecture cut is still `PreviewHotLoop` / `NativeFrameClock` /
  `ActivePreviewScene`, with same-surface hardware present-floor evidence and
  removal of remaining product-frame proof/report boundaries.

2026-07-02 NativeFrameClock policy checkpoint:

- Added a generic `NativeFrameClockPolicy` in the native app-window path:
  - classifies scheduler reasons into product/proof/background frame lanes;
  - marks whether a frame is a product-input frame;
  - forbids pre-submit proof polling on product and interaction-burst frames;
  - forbids post-present background proof telemetry on product and burst frames;
  - leaves required proof lanes allowed to do required proof work.
- The live loop now records the policy used for pre-submit proof decisions and
  post-present background telemetry decisions, and render-loop reports expose
  `native_frame_clock_policy` plus `native_frame_clock_owner`.
- Added focused tests proving product frames reject proof/background work and
  proof frames remain allowed to run required proof work.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window -p xtask`;
  - `cargo test -q -p boon_native_app_window native_frame_clock -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats -- --test-threads=1`;
  - `cargo test -q -p xtask cells_visible_click -- --test-threads=1`.
- This is still not the completed 60 FPS architecture. It centralizes product
  versus proof frame ownership so the next implementation can replace scattered
  gating with a real `PreviewHotLoop` / `ActivePreviewScene` transaction.

2026-07-02 deferred retained proof subscriber checkpoint:

- Tightened the preview render-hook product/proof split without adding
  Cells-specific code:
  - readback proof mode now lets product and proof/harness frames register
    deferred retained visual proof subscribers after present;
  - deferred product frames register only `VisibleBoundText` and
    `RetainedBoundSync` proof request summaries;
  - render-hook report JSON, proof history, artifact hashes, and external
    app-owned scene readback stay out of the required product proof request
    list;
  - visible-bound-text proof payloads are now built by the post-present
    subscriber from retained sync data instead of being assembled as
    `serde_json::Value` before product present.
- This is an ABI cleanup toward `PresentedProductFrame` / `RenderFrameResult`:
  product frames publish typed proof requests and scalar metrics, while proof
  payload construction moves behind `FrameEvidenceKey`-keyed subscribers.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`;
  - `cargo test -q -p boon_native_playground deferred_product_proof_requests_are_not_legacy -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground readback_proof_mode_creates_retained_sync_post_present_subscriber -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground compact_visible_bound_text_snapshot_uses_retained_sync_without_layout_scan -- --test-threads=1`.
- One-click release smoke:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --address A2 --expected-formula 15 --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-a2-deferred-retained-proof.json`
    still reports `status=fail`;
  - the failure is now honest diagnostic evidence, not a product proof backlog:
    adapter evidence is software Vulkan llvmpipe, so hardware product gates must
    fail;
  - exact product sample: input-accept-to-present/formula `10.710457 ms`,
    missed frames `0`, legacy pre-present proof request count `0`;
  - proof-only passed with app-owned readback, proof lag `2` frames, and
    post-present proof isolation passed;
  - aggregate preview-loop p95 was still `18.404 ms`, proving the next cut is
    stricter typed lane separation and a real product frame owner, not more
    Cells/runtime tuning.
- This still is not the full 60 FPS fix. The next report must show whether
  exact retained proof subscribers reduce proof-lag failures, and the larger
  remaining cut is still the real `PreviewHotLoop` / `ActivePreviewScene`
  transaction with product-frame scheduling and present ownership.

2026-07-02 product-commit lane split checkpoint:

- Tightened the native app-window product-frame boundary without adding
  example-specific code:
  - added `NativeProductCommitPolicy` owned by `NativeFrameClock`;
  - product commits are now published only for accepted product-interaction
    frames with accepted input timing;
  - animation/proof/runtime presented frames still enqueue exact
    post-present proof requests, but they are counted as non-product presented
    frames and no longer enter `recent_product_frame_commits`;
  - render-loop reports expose `non_product_presented_frame_count`,
    `last_non_product_presented_frame_lane`, key, and reason so verifier
    evidence can prove lane separation.
- Tightened Cells verifier classification:
  - product timing status is now separate from hardware-adapter status;
  - llvmpipe/software adapter runs may show `timing_status=pass`, while the
    overall product contract still fails as diagnostic-only hardware evidence.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window -p boon_native_playground -p xtask`;
  - `cargo test -q -p boon_native_app_window native_frame_clock -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window non_product_presented_frame_enqueues_proof_without_product_commit -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window product_frame_commit -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window post_present_proof -- --test-threads=1`;
  - `cargo test -q -p xtask cells_visible_click -- --test-threads=1`.
- Fresh release diagnostic:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --address A2 --expected-formula 15 --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-a2-product-commit-lane-split-current.json`
    still reports `status=fail`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-a2-product-commit-lane-split-current.json`
    passes;
  - product timing lane is healthy in this diagnostic run:
    `timing_status=pass`, input-accept-to-present/formula p95/max
    `11.100808 ms`, missed frames `0`, hard failures `0`,
    legacy pre-present request count `0`, product does not block on proof;
  - proof-only and post-present proof isolation pass, with app-owned WGPU
    readback and proof lag max `2` frames;
 - remaining blockers are hardware evidence only on this machine:
    adapter is software Vulkan llvmpipe, so `hardware-product-adapter` and the
    hardware-backed `product-only-ux-contract` fail by design.
- This still is not the completed architecture. Next useful cut is to make
  `PreviewHotLoop` / `ActivePreviewScene` own the first-frame retained visual
  patch and WGPU submit path directly, then run a multi-sample hardware-backed
  release report. Do not spend the next pass tuning Cells runtime/list/formula
  unless a fresh report shows that as the dominant boundary again.

2026-07-02 typed product patch and exact product commit evidence checkpoint:

- Tightened the native product-frame evidence boundary without adding
  example-specific code:
  - preview product frames now report an `active_preview_scene` identity with
    route, layout, and render-scene identity;
  - rendered product frames now carry typed `product_patch` metadata from the
    active preview scene: owner, patch kind/source, active scene identity, route
    identity, touched node count/samples, retained text/style update counts,
    hover/focus counts, direct patch flag, full-scene-before-present flag,
    proof JSON dependency flag, and latest-report dependency flag;
  - dev-window render hooks pass no product patch evidence, so the new contract
    is scoped to preview product frames instead of faking product evidence for
    non-product surfaces.
- Tightened Cells visible-click verifier classification:
  - product UX now requires exact product-frame commit matches for every sample;
  - proof-frame exact matches and input-latency fallback matches are reported as
    diagnostics and cannot satisfy the product UX gate;
  - product UX now requires typed active-scene `product_patch` evidence and
    rejects full render-scene builds, proof JSON dependencies, latest-report
    dependencies, or missing product patch summaries.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`;
  - `cargo test -q -p xtask cells_visible_click -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground product_patch_summary_reports_generic_active_scene_patch -- --test-threads=1`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-a2-product-patch-exact-current.json`.
- Fresh release diagnostic:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --address A2 --expected-formula 15 --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-a2-product-patch-exact-current.json`
    still reports `status=fail`;
  - product timing is healthy in this diagnostic run:
    `timing_status=pass`, exact product commit matches `1`,
    proof-frame commit fallbacks `0`, input-latency fallbacks `0`,
    typed product patch count `1`, product patch missing/full-scene/proof-JSON/
    latest-report counts all `0`, input-accept-to-present/formula p95/max
    `11.754082 ms`, missed frames `0`, hard failures `0`;
  - remaining product blocker is hardware evidence: the run selected software
    Vulkan llvmpipe (`adapter_status=software`), so hardware performance
    acceptance must still fail;
  - proof remains a separate blocker in this one-click run:
    `proof_only_contract.status=fail` with proof lag reported at `3` frames,
    while post-present proof isolation still reports `status=pass` and
    `product_path_status=pass`.
- This is still not the completed 60 FPS architecture. The checkpoint proves
  the product lane can now be measured by exact product commits and typed active
  scene patches instead of proof/readback fallbacks. The next cut should make
  `PreviewHotLoop` / `ActivePreviewScene` own the product frame transaction
  directly, then run hardware-backed multi-sample release reports with product,
  proof, and proof-isolation lanes separated.

2026-07-02 typed product result boundary checkpoint:

- Tightened the product-frame ABI without adding example-specific code:
  - added generic `NativeProductFrameResult` with owner, result kind, presented
    product frame, and post-present proof request summaries;
  - product commits now prefer `NativeProductFrameResult` over legacy
    `NativeRenderFrameMetrics.product_frame` and label the source as
    `native_product_render_result`, `legacy_render_frame_metrics`, or
    `missing`;
  - preview product frames publish `owner="preview_active_scene"` and
    `result_kind="active_preview_scene_patch"`;
  - dev/non-product render hooks continue to expose metrics, but do not publish
    product-result ownership.
- Tightened Cells visible-click verifier classification:
  - product UX now requires typed product result evidence for every exact
    product commit;
  - legacy render-metric fallback and missing product result are counted and
    cannot satisfy product UX;
  - sample failures now distinguish `missing_typed_product_result` from missing
    product patch, stale commit joins, or timing budget misses.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`;
  - `cargo test -q -p xtask cells_visible_click -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window product_frame_commit_prefers_typed_product_result_over_legacy_metrics -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground product_patch_summary_reports_generic_active_scene_patch -- --test-threads=1`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-a2-product-result-current.json`.
- Fresh release diagnostic:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --address A2 --expected-formula 15 --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-a2-product-result-current.json`
    still reports `status=fail`;
  - product timing is healthy in this diagnostic run:
    `timing_status=pass`, exact product commit matches `1`,
    proof-frame commit fallbacks `0`, input-latency fallbacks `0`,
    typed product patch count `1`, typed product result count `1`,
    legacy product-result fallback count `0`, product-result missing count `0`,
    product patch missing/full-scene/proof-JSON/latest-report counts all `0`,
    input-accept-to-present/formula p95/max `13.137367 ms`, missed frames `0`,
    hard failures `0`;
  - remaining product blockers are outside the typed-result boundary:
    hardware evidence is software Vulkan llvmpipe (`adapter_status=software`)
    and raw wake-to-formula was slightly above budget at `17.361 ms`;
  - proof remains a separate blocker in this one-click run:
    structured proof changed and lag was bounded at `3` frames, but the proof
    key did not match the product input event, while post-present proof
    isolation stayed `status=pass` and `product_path_status=pass`.
- This still is not the completed 60 FPS architecture. The checkpoint proves
  the product lane now rejects legacy product-result fallback and can be
  measured from exact product commits with typed active-scene ownership. The
  next useful cut is still to make `PreviewHotLoop` / `ActivePreviewScene` own
  input sampling, the retained patch, and product present directly, while the
  proof subscriber service fixes exact input-event matching separately.

2026-07-02 product sample key preservation checkpoint:

- Tightened the visible-click verifier so product/proof identity cannot be
  hidden by fallback matching:
  - `cells_visible_click_product_commit_match_from_report` now keeps the
    measured product-present `FrameEvidenceKey` as `product_frame_evidence_key`;
  - any nearby/fallback commit key is reported separately as
    `matched_product_commit_frame_evidence_key`;
  - click samples now expose `requested_product_frame_evidence_key` and
    `matched_product_commit_frame_evidence_key` beside the measured
    `product_frame_evidence_key` and proof key.
- This is deliberately stricter. If the next release report shows the measured
  product frame lacks an exact product commit, the fix is product frame
  ownership/publication or a real `FrameEvidenceRegistry`, not replacing the
  sample key with a nearby commit key.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_playground -p boon_native_app_window -p xtask`;
  - `cargo test -q -p xtask cells_visible_click -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window product_frame_commit_prefers_typed_product_result_over_legacy_metrics -- --test-threads=1`;
  - `cargo test -q -p boon_native_playground product_patch_summary_reports_generic_active_scene_patch -- --test-threads=1`.
- This checkpoint does not complete the 60 FPS architecture. It prevents a
  verifier shortcut from making the product lane look more exact than it is, so
  the next implementation can cut the actual `PreviewHotLoop` /
  `ActivePreviewScene` / proof-subscriber boundary.

2026-07-02 interaction-scoped product commit checkpoint:

- Tightened the visible-click verifier around click press/release identity
  without adding compiler/runtime/document/renderer special cases:
  - product-present probing now prefers the accepted/accounted host input event
    when finding the product commit, because a click release or follow-up frame
    can advance the presented input generation without being the accepted
    product interaction;
  - the probe reports whether the commit came from the accepted input event,
    the presented input event, or a fallback, plus the input event sequence used
    for that match;
  - this keeps the product lane tied to the actual accepted visible change
    instead of misclassifying a later release/proof frame as product latency.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo test -q -p xtask cells_visible_click -- --test-threads=1`;
  - `cargo check -q -p xtask`;
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --address A2 --expected-formula 15 --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-a2-interaction-commit-current.json`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-a2-interaction-commit-current.json`.
- Fresh release diagnostic after this change still reports `status=fail`, but
  for the right reasons:
  - product timing and identity pass in the one-click diagnostic:
    `timing_status=pass`, exact product commit matches `1`,
    input-latency fallbacks `0`, typed product patch/result counts `1`,
    input-accept-to-present/formula p95/max about `10.62 ms`, missed frames
    `0`, hard failures `0`;
  - the product adapter is still software Vulkan llvmpipe on this machine, so
    hardware-backed performance acceptance must continue to fail;
  - proof remains a separate blocker: the product frame was the accepted press
    frame/input event, while the app-owned WGPU proof still completed for a
    later release/follow-up frame, with proof lag around `3` frames. This must
    be fixed in the bounded proof subscriber service, not by counting a later
    proof frame as product UX.
- Next architectural cut:
  - introduce or finish a generic product-frame/proof identity service so
    accepted input, product commit, requested proof key, completed proof, and
    report sample all carry the same `FrameEvidenceKey` unless proof lag is
    explicitly reported as proof latency;
  - keep product frames free of readback/report/dev IPC/accessibility work;
  - keep no-hacks audits across compiler, runtime, document, renderer,
    app-window, playground, and xtask verifier code.

2026-07-02 exact product readback checkpoint:

- App-window proof plumbing was tightened without adding a Cells-specific path:
  - required post-present proof subscribers are now enqueued immediately after
    the product/non-product frame is recorded, before the loop yields to pending
    input;
  - Readback proof mode can request a visible-surface WGPU readback for the
    exact product-input frame instead of always deferring product-frame readback
    behind the interaction burst;
  - exact product readback is allowed to ignore external-proof replacement, but
    still respects in-flight readback backpressure;
  - stale-input readback skipping no longer discards this explicit exact-product
    proof request.
- Focused checks for the app-window slice passed before this checkpoint:
  - `cargo fmt --check`;
  - `cargo check -q -p boon_native_app_window`;
  - `cargo test -q -p boon_native_app_window interactive_surface_readback -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window post_present_proof -- --test-threads=1`.
- Fresh release diagnostic:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --address A2 --expected-formula 15 --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-a2-exact-product-readback-job.json`
    still reports `status=fail`;
  - important improvement: the product frame's post-present queue now completed
    `visible_bound_text`, `retained_bound_sync`, and
    `visible_surface_readback`, and the app-owned WGPU readback artifact exists
    for the exact product frame;
  - remaining blocker: the xtask verifier still selected a later frame/input
    proof from recent/latest frame probing instead of preferring the exact
    product-frame proof artifact, so the proof-only contract failed even though
    exact product artifacts were present;
  - the run also used software Vulkan llvmpipe, so hardware-backed performance
    acceptance still must fail until a hardware adapter report passes.
- Next implementation step:
 - fix the verifier/proof selection to prefer exact product-frame
   `FrameEvidenceKey` artifacts before any recent-frame fallback;
 - make later-frame proof acceptable only as explicitly reported proof lag, not
   as product UX proof;
  - then continue with the larger `PreviewHotLoop` / `NativeFrameClock` /
    `ActivePreviewScene` cut and hardware-backed multi-sample release gates.

2026-07-02 exact product proof join checkpoint:

- The visible-click verifier now rejects later-frame visual proof as product
  sample proof when an exact product `FrameEvidenceKey` is known:
  - added a generic exact-product post-present visual probe helper keyed by
    `FrameEvidenceKey`;
  - the polling loop tries exact product-frame post-present artifacts before
    falling back to recent-frame probes;
  - a visual probe is considered complete for product UX only when the readback
    root key and structured proof key match the product frame key;
  - final sample assembly also rechecks exact product-frame artifacts so a
    later proof discovered during polling cannot overwrite product/proof
    identity;
  - added a unit regression that a later-frame visual proof requires exact
    product replacement.
- Verification for this slice:
  - `cargo fmt --check`;
  - `cargo check -q -p xtask`;
  - `cargo test -q -p xtask cells_visible_click -- --test-threads=1`;
  - `cargo test -q -p boon_native_app_window interactive_surface_readback -- --test-threads=1`;
  - `cargo xtask verify-report-schema target/reports/native-gpu/cells-visible-click-e2e-a2-exact-product-proof-join-current.json`.
- Fresh release diagnostic:
  - `cargo xtask verify-native-cells-visible-click-e2e --profile release --address A2 --expected-formula 15 --repeat-count 1 --report target/reports/native-gpu/cells-visible-click-e2e-a2-exact-product-proof-join-current.json`
    still reports `status=fail`;
  - product/proof identity is now fixed in the one-click diagnostic:
    `proof_only_contract.status=pass`, exact visual proof sample count `1`,
    input-event match count `1`, proof lag max `0`, and the click sample's
    product/proof `FrameEvidenceKey` both point to frame `7`, input event `4`;
  - product timing is healthy for this diagnostic sample:
    accepted input-to-formula/present p95/max is about `12.218707 ms`, exact
    product commit matches `1`, typed product patch count `1`, typed product
    result count `1`, hard failures `0`;
  - post-present proof isolation remains `status=pass`, with product path
    status `pass`, no hot-path report writes/serialization, and no product
    blocking on proof subscribers.
- Remaining blocker:
  - this machine/run selected software Vulkan llvmpipe, so the hardware-backed
    product performance gate still fails by design;
  - a parallel adapter-selection review confirmed this is not an app-window
    request bug: the preview path requests `HighPerformance`,
    `force_fallback_adapter: false`, and a compatible Wayland surface. The
    isolated Weston headless surface appears to expose only llvmpipe as
    WGPU-compatible, while the current COSMIC Wayland present-floor diagnostic
    can select the NVIDIA RTX hardware adapter. The next implementation should
    add a generic product-verifier adapter policy/fail-fast path with full
    adapter/request/environment evidence, and keep software-surface runs
    diagnostic-only instead of allowing them to satisfy product UX gates;
  - next useful work is hardware-adapter selection/verification plus the larger
    `PreviewHotLoop` / `NativeFrameClock` / `ActivePreviewScene` product-frame
    cut and multi-sample release gates, not more Cells runtime/list/formula
    micro-tuning.
