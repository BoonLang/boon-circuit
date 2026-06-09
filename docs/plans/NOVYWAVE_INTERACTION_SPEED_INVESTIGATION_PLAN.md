# NovyWave 60fps Interaction Speed Plan

Date: 2026-06-09

## Summary

Make release-mode NovyWave interaction run at 60 fps or better. Hover, click,
divider drag, and resize must prove p95 <= 16.7 ms, and ordinary interaction
frames must not block on debug summaries, IPC, readback PNGs, report writes, or
synchronous persistence.

The current review ranks the likely bottlenecks as input wake delay,
verifier-grade readback in live preview, full runtime summary plus full layout
rebuild after small events, repeated NovyWave list projections, clone-heavy
render state, dev inspector contention, and string-heavy runtime identity.

## Key Changes

- Add `cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json`.
- Register the command in xtask help/dispatch, report-schema allowlist, default
  report path selection, native report validation, and
  `verify-native-gpu-regression-all` after schema/negative coverage exists.
  Do not add it to `verify-native-gpu-all` unless `NATIVE_GPU_PIPELINE.md` and
  `AGENTS.md` are updated together.
- Add `[novywave_interaction.release]` budgets in `budgets/native-gpu.toml`:
  `input_to_visible_ms_p95 <= 16.7`, `input_to_visible_ms_max <= 33.4`,
  `hover_to_overlay_ms_p95 <= 16.7`, `click_to_cursor_ms_p95 <= 16.7`,
  `divider_drag_to_layout_ms_p95 <= 16.7`,
  `resize_to_present_ms_p95 <= 33.4`, `preview_blocked_on_ipc_count == 0`,
  `hot_path_png_write_count == 0`, `hot_path_report_write_count == 0`, and
  `hover_persist_write_count == 0`.
- Add a bounded in-memory interaction sample buffer flushed after each verifier
  run. Samples must include input, runtime, layout, render, readback,
  persistence, IPC, and lock wait spans; no pretty JSON, PNG writes, or report
  writes inside measured hot frames.
- Add engine-facing APIs as needed: a `RuntimeTurn`-style output with changed
  root/list ids, changed dependency keys, and bounded document/render patches;
  typed pointer/drag payloads; dense ids/tags for hot runtime identity;
  maintained list indexes/projections. Do not write Boon workarounds for engine
  limitations.

## Implemented Baseline On 2026-06-09

- Added the release-mode `verify-native-gpu-novywave-interaction-speed` xtask
  gate, report-schema allowlist entry, default report path, regression
  aggregate entry, budget keys, and child `boon_native_playground --role
  interaction-speed --example novywave` role.
- Added release/probe readback separation: probe launches can pass
  `--frame-readback`; ordinary manual preview/dev windows no longer do
  per-frame readback just because role reports exist.
- Fixed passive input motion accounting by tracking
  `NativeInputCursor.last_mouse_motion_event_count`, so motion-only hover input
  is treated as host input.
- Prevented NovyWave hover-only waveform events from synchronously persisting UI
  state.
- Reused the `apply_source_event_for_document_window` windowed state summary in
  preview turns instead of discarding it and calling full
  `document_state_summary()` again.

Latest baseline command:

```bash
cargo xtask verify-native-gpu-novywave-interaction-speed
```

Latest baseline result: failing as intended against 60fps budgets. The semantic
checks pass: click moves the cursor, hover keeps the click cursor and moves only
the zoom center, selected rows remain `A[3:0], B[3:0]`, and no hot-path
PNG/report/hover-persist writes are reported. The remaining blocker is full
document relayout per interaction.

Measured p95s after the windowed-summary fix:

- `click_to_cursor_ms_p50_p95_max.p95`: 670.660 ms
- `hover_to_overlay_ms_p50_p95_max.p95`: 688.037 ms
- `divider_drag_to_layout_ms_p50_p95_max.p95`: 564.143 ms
- `resize_to_present_ms_p50_p95_max.p95`: 576.150 ms

The previous baseline before reusing the windowed state summary was roughly
990.518 ms click p95, 902.269 ms hover p95, 813.480 ms divider p95, and
614.487 ms resize p95. The fix helps, but the next optimization must eliminate
the full `native_document_layout_proof_with_project_state_embedded_for_viewport`
path for small cursor/hover/divider updates by introducing bounded render
patches or compact layout snapshots.

## Current Investigation State On 2026-06-09

Additional engine/runtime work has reduced part of the hot path, but the latest
release verifier still fails the 60fps target. The important split from
`target/reports/native-gpu/novywave-interaction-speed.json` is:

- `hover_to_overlay_ms_p50_p95_max.p95`: 462.148 ms
- `click_to_cursor_ms_p50_p95_max.p95`: 528.296 ms
- `divider_drag_to_layout_ms_p50_p95_max.p95`: 415.958 ms
- `resize_to_present_ms_p50_p95_max.p95`: 586.385 ms
- `runtime_apply_ms_p50_p95_max.p95`: 99.544 ms
- `runtime_step_apply_ms_p50_p95_max.p95`: 64.653 ms
- `runtime_state_summary_ms_p50_p95_max.p95`: 34.234 ms, with p50
  0.438 ms after cached/patched summaries
- `layout_rebuild_ms_p50_p95_max.p95`: 417.177 ms
- `document_eval_lower_ms_p50_p95_max.p95`: 387.476 ms

The next implementation priority is therefore a generic dirty-layout/render
patch route. Small root `FieldSet` turns must map changed runtime fields to
concrete document nodes and attributes, then update only the affected text/style
nodes or compact layout snapshot. Full document lowering, full layout proof JSON,
and full state summaries must remain verifier/debug/report behavior, not normal
preview interaction behavior.

## Follow-Up Review Findings On 2026-06-09

The next review narrowed the remaining work into three generic problem areas.

### Measurement Contract Problems

- The speed harness includes the first measured hover and click samples. It does
  not define a warmup/drop policy, so one first full-layout interaction can
  dominate p95 even when later steady hover samples are already near budget.
- Interaction profiles are phase-mixed: hover, click, divider, and resize
  profiles are drained together, so aggregate runtime/layout p95s are not
  attributable to one interaction class.
- Divider samples currently send synthetic `Grow`/`Shrink` live events, not the
  actual press/move/release divider drag route.
- Resize samples measure layout proof rebuild time, not input-to-present or
  app-owned readback evidence. Rename or fix the metric before accepting it as
  `resize_to_present`.
- Xtask budget checks consume summary p95s but do not enforce sample counts,
  phase isolation, or proof that each metric measured the intended route.

### Runtime/List-View Problems

- NovyWave click starts as a root source event, but derived cursor state fans out
  into keyed `waveform_segment_records.width` list field commits. The renderer
  then sees list deltas and falls back to full document rebuild.
- The runtime has row-level derived list field commits but no first-class
  derived root `ListView` materialization boundary for paths such as
  `store.selected_waveform_segments`, `store.selected_cursor_pair_rows`, and
  `store.selected_visible_items`.
- Add generic list-view patch semantics: diff root `ListView` outputs by stable
  list keys and emit list-view patches, not only underlying list row field
  deltas.
- Canonicalize root dirty keys internally so aliases such as `store.foo` and
  `foo` do not widen fanout.
- Compile `List/map`, `List/filter`, `List/retain`, and `List/join_field` into
  derived operators with explicit read sets and patch semantics.

### Document Binding/Layout Patch Problems

- The cached-frame patch path is the right direction, but it must prove concrete
  patched node/attr samples in the app-owned report.
- Style records must register concrete target attrs like `width`, `height`,
  `border`, and `border_radius`; a generic synthetic `style` target is too
  coarse for sparse patches.
- `ForEach`/list item lowering needs origin metadata so row-derived view nodes
  can be patched from keyed list deltas.
- Root-only patches should stay conservative, but list-row and list-view patches
  are required before cursor clicks and first hover can avoid full relayout.

## Implementation Phases

### Phase 0: Baseline Verifier And Instrumentation

- Implement the new NovyWave interaction-speed verifier in release mode.
- Drive app-owned host events through the public host/document/source-intent/
  runtime/layout/render route, not private runtime mutation.
- Run deterministic scenarios: 3s idle baseline, initial loaded waveform,
  20-position waveform hover sweep, 10 deterministic waveform clicks,
  pan/zoom/cursor keyboard sequence from `examples/novywave.scn`,
  Files/Selected Variables divider drag, selected-variable column drag, resize
  sequence, dev idle, dev busy with summary/value queries, and proof-off/proof-on
  A/B.
- Required metrics: `input_to_visible_ms`, `hover_to_overlay_ms`,
  `click_to_cursor_ms`, `divider_drag_to_layout_ms`, `resize_to_present_ms`,
  `poll_wait_ms`, `hit_test_ms`, `runtime_apply_ms`, `derived_recompute_ms`,
  `document_summary_ms`, `layout_lower_ms`, `render_encode_ms`, `readback_ms`,
  `persist_ui_state_ms`, `ipc_handle_ms`, `dev_runtime_lock_wait_ms`,
  `events_coalesced`, and `events_dropped`.

### Phase 1: Remove Measurement Pollution And Obvious Hot-Path Stalls

- Split verifier/proof mode from normal manual/live interaction mode so
  app-owned readback remains available for gates but normal interaction frames do
  not synchronously map WGPU buffers, write PNGs, hash proofs, or write reports.
- Fix input wake latency so pointer/drag/resize input wakes the demand-driven
  loop immediately instead of waiting up to the passive poll interval.
- Prevent hover-only state from persisting; debounce committed UI persistence
  with last-write-wins background behavior.

### Phase 2: Runtime And Boon Engine Hot Path

- Remove the double-summary shape: `apply_source_event_for_document_window`
  should not force a window summary if preview immediately needs a patch/turn
  result, and preview must not build full `document_state_summary()` for every
  small changed event.
- Use changed ids and sparse patches for hover/click/drag. Full debug summaries
  must be async/coalesced and served from snapshots.
- Add indexed list lookup/projection support and fused/memoized list pipelines
  so NovyWave does not scan/filter/map whole waveform lists per hover or per row.
- Replace hot internal string comparisons with tags/dense ids where state is
  domain identity, while keeping user labels, paths, digests, waveform refs, and
  visible text as text.

### Phase 3: NovyWave Boon Source Cleanup Backed By Engine Support

- Make waveform hover numeric and overlay-only: pointer x/width projects to
  zoom-center time/offset/label and dirties only zoom-center overlay nodes.
- Keep click broader but bounded: cursor value, cursor labels, cursor overlay,
  and affected selected-row value fields only.
- Derive one active metadata record and one selected timeline metadata record
  instead of repeated `List/find_value`.
- Derive selected rows once, cursor values by signal once per cursor change, and
  visible waveform segments by signal once per viewport/canvas state.
- Coalesce divider drag into one numeric delta per frame; avoid repeated
  `Grow`/`Shrink` text commands as hot-path state.

### Phase 4: Layout, Render, IPC Architecture

- Store render snapshots as immutable `Arc`/revision data instead of cloning
  large `serde_json::Value` layout proofs and full `LayoutFrame`s every render.
- Generate large JSON layout proofs only for reports/verifiers; render should
  consume compact layout snapshots.
- Improve renderer metrics so `text_runs_shaped` means actual shaping misses,
  not visible text count; add text cache hit/miss and glyphon prepare/render
  timings.
- Serve dev runtime summary/value queries from bounded immutable snapshots or
  stale-ok try-lock responses. Real IPC metrics must measure actual
  accept/read/handle/write, queue depth, dropped/coalesced telemetry, and
  preview heartbeat; synthetic `preview_blocked_on_ipc_count: 0` is not
  sufficient.

### Phase 5: Final Deterministic Verification And Manual Handoff

- Freshly run `verify-native-gpu-novywave-interaction-speed`,
  `verify-native-gpu-novywave-visual`, `verify-native-gpu-preview-e2e --example
  novywave`, `verify-native-gpu-scroll-speed --example novywave`,
  `verify-native-gpu-negative`, and `verify-native-gpu-regression-all
  --check-existing`.
- Reports must be fresh for commit, worktree fingerprint, binary hash, source
  hash, scenario hash, and budget hash.
- After reports pass, stop only matching old NovyWave release playground
  processes, build `boon_native_playground` in release mode, launch the current
  NovyWave example with workspace-qualified `cosmic-background-launch`, and
  prove it with `pgrep`. Human testing remains follow-up evidence, not verifier
  evidence.

## Test Plan

- Unit/runtime tests: hover dirties only zoom overlay state, click dirties
  cursor/value state only, divider drag coalesces to one update per frame,
  indexed list lookup matches previous list semantics, and tags/ids do not
  collide with user fields.
- Native playground tests: manual mode has zero hot-path readback/report/PNG
  writes, proof mode still produces app-owned WGPU artifacts, and dev summary
  queries cannot block preview input frames.
- Report-schema tests: missing metrics, stale hashes, false `real_os_input`,
  nonzero `preview_blocked_on_ipc_count`, hot-path PNG writes, and private
  runtime dispatch are rejected.
- Acceptance commands: run the fresh NovyWave interaction-speed gate plus
  existing NovyWave visual/E2E/scroll gates and
  `verify-native-gpu-regression-all --check-existing`.

## Assumptions

- Keep two child processes and app-owned WGPU evidence per the active native GPU
  contract.
- Shared memory/direct memory is only allowed for bounded read-only telemetry
  snapshots unless the architecture contract is explicitly changed.
- Do not use screenshots, browser/Ply/Xvfb evidence, or compositor scraping as
  native GPU proof.
