# Native GPU Scroll And Example Switching Regression Recovery

Date: 2026-05-28

Status: recovery plan and regression diagnosis. This file is not a native GPU
handoff readiness claim. Native handoff readiness is still governed by
`docs/architecture/NATIVE_GPU_PIPELINE.md` and the `AGENTS.md` gate list.

## Purpose

This document explains why the recent native dev-window performance work
regressed code-editor scrolling and example switching, then gives a concrete
repair plan. It is intentionally narrower than
`docs/plans/NATIVE_DEV_WINDOW_EDITOR_AND_EXAMPLE_SWITCHING_PLAN.md`, which mixed
editor UX, example catalog, preview IPC, demand-driven rendering, verifier
changes, and speed budgets into one broad effort.

The immediate objective is to recover correctness and honest measurement before
attempting another speed pass.

## Evidence Snapshot

Current checkout state observed on 2026-05-28:

- The native GPU worktree is dirty in `budgets/native-gpu.toml`,
  `crates/boon_native_app_window/src/lib.rs`,
  `crates/boon_native_playground/src/main.rs`,
  `crates/xtask/src/main.rs`, and
  `docs/architecture/NATIVE_GPU_PIPELINE.md`.
- The relevant commit stack after `origin/main` has about 12,728 inserted lines
  and 2,852 deleted lines in the native GPU budget/app/playground/xtask/docs
  surface.
- The currently dirty native GPU files add another roughly 1,623 inserted lines
  and 301 deleted lines.
- `cargo check -p boon_native_playground -p xtask` passed during the
  investigation pass.
- `target/reports/native-gpu-all.json` is failing/stale and must not be used as
  handoff evidence.

Focused current-checkout verifier observations:

- `verify-native-dev-editor-scroll-speed --profile debug` passed, but it used
  one post-input measured frame per axis. The vertical post-input frame was
  about 26.35 ms, horizontal about 11.13 ms, with a 10,000 line editor fixture
  and only 48 materialized lines.
- Current canonical reports show release example switching can be acceptable,
  but debug is loose. The focused debug report passed while Cells source
  replacement reported about 1,912 ms worker time: about 1,336 ms
  `LiveRuntime::from_source`, about 292 ms runtime summary, and about 282 ms
  layout. The `custom:b` Cells-derived switch was similar.
- The same example-switch report measured a first visible readback before final
  replace-source readiness; `pending_overlay_presented_before_result` was true.
  That can make a pending overlay look like preview success.
- An existing preview role report under `target/reports/native-gpu/roles/`
  recorded `status: "pass"` while also recording
  `loop_error: "render hook result content_revision 2 is older than dirty_revision 3"`.

## Why Code Editor Scrolling Regressed

### 1. The plan scope became too broad

`docs/plans/NATIVE_DEV_WINDOW_EDITOR_AND_EXAMPLE_SWITCHING_PLAN.md` requires
browser-like tabs, full editor controls, formatting, source/project payloads,
example catalog behavior, performance gates, and preview isolation in one plan.
That plan is valuable as historical context, but it was too broad to use as a
single implementation contract for a scroll-speed fix.

The result was code touching the dev shell, preview IPC, render loop scheduler,
native verifiers, budgets, and the active architecture document together. A
scroll UX problem should not have needed changes across all of those surfaces.

### 2. Wheel input was intentionally made less responsive

Commit `545c8fb` changed editor wheel scaling in
`crates/boon_native_playground/src/main.rs` from:

```rust
scaled_scroll_steps(vertical_delta, 8.0, 3)
scaled_scroll_steps(horizontal_delta, 8.0, 3)
```

to:

```rust
scaled_scroll_steps(vertical_delta, 24.0, 1)
scaled_scroll_steps(horizontal_delta, 24.0, 1)
```

The current footer path still uses `8.0, 3`, while the editor path uses
`24.0, 1`. That makes a typical wheel tick move fewer editor lines/columns and
is a direct explanation for the user-visible feeling that scrolling got slower.

### 3. The hot path still does too much work before deciding it is a scroll

In the current dev poll path, `dev_input_may_change` can enter the hot path, but
the code still calls `shell.document_for_viewport(...)` before applying input.
That means wheel input can pay document/model construction cost before the code
knows whether it is just a clamped scroll offset update.

The scroll handler also calls `max_editor_scroll_column` on wheel input. That
function scans the selected buffer lines to find the widest line. On large
editor buffers this is an avoidable per-wheel cost.

### 4. The fast scroll patch is frame surgery, not a complete architecture

`patch_dev_render_editor_scroll` mutates existing display-list items after
layout. It rewrites text nodes and gutter nodes, recomputes syntax span JSON,
selection/caret style, and bracket-column styles for visible lines. That avoids
a full layout in some cases, but it does not prove that every frame-owned
metadata structure, hit region, scroll region, materialized range, text cache
identity, and renderer cache key is updated consistently.

This explains the "broken scrolling" symptom: the code is trying to keep an old
layout frame alive while replacing pieces of it after the fact. The idea of a
scroll-only fast path is still correct, but it must be an official complete
incremental update, not partial display-list surgery.

### 5. Some input can be consumed without a useful render

The editor scroll path depends on `mouse_window_pos` being present and inside
the current editor bounds. If the position is missing, stale, or transformed
late, the wheel event can be ignored. The native app loop accepts the input
cursor after polling, so verifier or compositor timing can consume input without
proving a coherent editor scroll.

### 6. The verifier can pass while the UX is still bad

There are two dev editor scroll verifier shapes:

- `verify-native-gpu-scroll-speed --surface dev-code-editor`, which is weaker
  and compatibility-shaped.
- `verify-native-dev-editor-scroll-speed`, which is stronger but still samples
  only a very small number of frames and contains several hard-coded or derived
  counters in `xtask`.

The stronger debug report passing with one 26 ms post-input vertical frame is
not enough to prove smooth sustained scrolling. It also does not prove real
renderer cache hit/miss behavior for glyph shaping, text run reuse, or GPU
buffer updates.

## Why Example Switching Got Slower And Preview Can Fail

### 1. The preview switch ACK and final preview frame were split, then measured together

`preview_enqueue_source_project` now returns a small
`replace-source-queued` ACK quickly and sets a pending status overlay. The real
work happens later in `preview_build_source_project` and is committed by
`preview_commit_source_project_result`.

That shape is good directionally, but the verifier currently treats the first
visible readback after the ACK as the preview frame timing. Because the pending
overlay itself changes pixels, the report can pass on overlay rendering before
the selected source is actually parsed, lowered, laid out, and presented.

### 2. Source replacement still does expensive full work per accepted payload

For each accepted payload, `preview_build_source_project` currently performs:

- `LiveRuntime::from_source`;
- runtime state summary and document state summary;
- `native_document_layout_proof_with_state`;
- final shared render state commit.

The debug report showed Cells and a Cells-derived custom switch each taking
about 1.9 seconds in this worker path. That is why example switching remains
slow even though the synchronous ACK is fast.

### 3. Latest-wins is only latest-pending, not cancellation of running work

`PreviewReplaceWorkerQueue` stores only one `pending` payload and drops stale
pending items. However, once a large job is running, it is not cancelled. Rapid
switches can still wait behind an expensive currently-running parse/runtime/
layout job.

### 4. Revision bookkeeping can mark preview reports as pass with a real loop error

The app-window render loop validates `content_revision` against
`dirty_revision`. Current preview loop evidence recorded a failure:

```text
render hook result content_revision 2 is older than dirty_revision 3
```

The same report had `status: "pass"`. That means the report writer and the
aggregate verifiers are not treating `loop_error` as fatal. This is a preview
failure masking bug, not only a performance issue.

The likely trigger is that the pending overlay increments shared render state
and dirty revision, then the final source-result render/present bookkeeping can
lag or disagree with the dirty revision expected by the app-window scheduler.

### 5. Multi-file payload proof is overstated

The payload hash can cover multiple units, but the current preview build path
uses the entrypoint unit text with `LiveRuntime::from_source`. Extra units are
hash-carried, but not project-parsed as executable dependencies. The reports
must not claim true multi-unit execution until the runtime path actually
supports that.

### 6. The debug budget was relaxed instead of fixing the path

`budgets/native-gpu.toml` currently allows
`click_to_preview_new_frame_presented_ms_p95_large_custom = 2200.0` in debug.
That can normalize slow large-custom behavior and makes the debug gate much
less useful than release. The recovery must restore strict budgets after the
verifier measures the final source frame instead of the pending overlay.

## Contract Drift

The active `AGENTS.md` handoff gate list contains the native GPU gates ending in
`verify-native-gpu-all --check-existing`. It does not promote the newer
`verify-native-gpu-idle-wake`, `verify-native-dev-editor-scroll-speed`, or
`verify-native-example-switch-speed` commands as required handoff gates.

`docs/architecture/NATIVE_GPU_PIPELINE.md` and `crates/xtask/src/main.rs` have
drifted toward including the product/editor/example-switch gates in the native
GPU aggregate. In the current checkout, `verify-native-gpu-all` may reject the
tree for those additional reports even though `AGENTS.md` lists a shorter
handoff gate set. That makes it harder to distinguish:

- handoff readiness for the two-window native GPU pipeline;
- product regression coverage for dev editor and example switching;
- experimental demand-driven render-loop validation.

Recovery rule: do not weaken native GPU schemas, reports, budgets, freshness
checks, or negative checks to make the broader aggregate pass. Resolve the
contract mismatch deliberately: either split product regression gates from the
handoff aggregate, or update `AGENTS.md` and the architecture together so the
same gate list is authoritative everywhere.

## Recovery Strategy

Use a surgical recovery first. Do not wholesale revert the whole branch unless
the app must be unblocked immediately and the surgical path cannot restore
scrolling and preview correctness quickly.

The valuable pieces to preserve are:

- source-only preview replacement direction;
- no example-name rendering shortcut in preview;
- bounded ACK payload direction;
- app-owned reports and WGPU readback proof direction.

The pieces to revert or repair first are:

- editor wheel scaling;
- incomplete display-list-only scroll patching;
- pending-overlay timing being counted as final preview success;
- loop reports passing with `loop_error`;
- debug budget relaxation for large custom switch timing;
- aggregate gate drift.

## Phase 0: Freeze And Baseline

1. Preserve the current dirty worktree before touching native code:

```bash
git status --short
git diff --stat
git diff -- budgets/native-gpu.toml crates/boon_native_app_window/src/lib.rs crates/boon_native_playground/src/main.rs crates/xtask/src/main.rs docs/architecture/NATIVE_GPU_PIPELINE.md
```

2. Do not adjust budgets upward to pass speed gates.

3. Record fresh failing or weak evidence under investigation-specific report
   names, not the canonical handoff report names:

```bash
cargo xtask verify-native-dev-editor-scroll-speed --profile debug --report target/reports/native-gpu/investigation-dev-editor-scroll-speed-debug.json
cargo xtask verify-native-example-switch-speed --profile debug --report target/reports/native-gpu/investigation-example-switch-debug.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/investigation-preview-e2e-cells.json
```

4. Treat any role report with non-null `loop_error` as a failure even if its
   top-level `status` says `pass`.

## Phase 1: Restore Correct Scrolling Before Optimizing

Implement this as the smallest native playground patch set:

1. Restore editor wheel constants to the previously responsive behavior, or
   introduce named per-surface constants that keep the editor at the old
   effective speed:

```rust
const DEV_EDITOR_WHEEL_UNIT: f64 = 8.0;
const DEV_EDITOR_WHEEL_MIN_STEPS: isize = 3;
```

2. Make `dev_apply_real_window_input` report `changed = true` only when
   `scroll_line`, `scroll_column`, selection, caret, focus, or buffer content
   actually changes after clamping.

3. Route pure wheel input through cached layout bounds before constructing a
   new `DocumentFrame`. For scroll-only input, the hot path should need:

- current editor/footer bounds from the cached layout frame;
- current scroll offsets;
- cached maximum scroll column;
- wheel delta and pressed Shift state.

4. Cache editor width metrics on `CodeEditorModel` or adjacent dev-shell state:

- longest rendered line column;
- per-line approximate column width when needed;
- invalidation on buffer edit, tab switch, format, or source replacement.

5. Preserve `patch_dev_render_editor_scroll` only if the repair proves it is a
   complete scroll-only update. It must update all layout-frame metadata,
   hit/scroll regions, materialized ranges, renderer cache identities, and
   report counters consistently. If that cannot be proven quickly, disable it
   for the first correctness patch and accept a full layout refresh after
   scroll.

6. If a fast path is still needed, implement it one layer lower as a real
   virtualized text/list scroll primitive:

- stable visible line window;
- scroll uniform or y-offset update in renderer state;
- shaped-run/glyph cache reuse measured by renderer-owned counters;
- explicit new-line materialization for only newly exposed rows.

7. Add a regression assertion that wheel input over the editor changes offsets
   by at least the old effective minimum step unless clamped at the buffer
   boundary.

## Phase 2: Fix Preview Switching Correctness

1. Keep ACK, pending overlay, final source result, and final presented frame as
   separate states:

- ACK: validates payload shape and queues work only.
- Pending overlay: optional UI state; never counted as source switch success.
- Ready result: parse/runtime/layout result for the selected source revision.
- Presented frame: WGPU readback bound to the ready result frame revision and
  source/project hash.

2. Fix revision bookkeeping so a pending overlay cannot satisfy or invalidate
   the final source-result revision. `content_revision`, shared render
   `update_count`, dirty revision, and reported `frame_revision` must have one
   documented meaning.

3. Make `write_render_loop_state_report` write `status: "fail"` when
   `loop_error` is non-null. Update aggregate verifiers to reject non-null
   `loop_error` in every role report they consume.

4. Update `run_native_example_switch_live_probe` so it waits for
   `replace-source-result` first, then waits for a readback whose report is
   explicitly tied to that result frame revision and source/project hash.

5. Add generation checks around expensive source replacement work:

- before parse/runtime construction;
- after parse/runtime construction;
- before runtime summary;
- before layout;
- before commit.

If the payload is stale at any point, stop work and report stale cancellation.

6. Replace full runtime summary in the switch hot path with a bounded startup
   summary or lazy summary unless the verifier explicitly requests full debug
   data outside the user-visible switch path.

7. Either implement true project parsing/execution for `SourceProjectPayload`
   or change reports to say extra units are hash-carried only.

8. Restore strict debug/release budgets after the verifier measures the final
   source frame. Do not keep the 2.2 second debug budget as a success threshold.

## Phase 3: Repair Verifiers

### Dev Editor Scroll Gate

The scroll gate must fail unless it proves sustained, coherent scrolling:

- launch the native desktop/dev path through app-owned isolated input;
- use a generated 10,000+ line source with a long line;
- run sustained vertical and horizontal wheel sequences, not one frame;
- record before/after scroll offsets for every step;
- record WGPU readback hashes and frame revisions after every visible update;
- record renderer-owned text/glyph/buffer cache counters, not hard-coded xtask
  values;
- fail if no-op/clamped wheel events are counted as scroll success;
- fail if `loop_error` is present in preview or dev role reports;
- fail if p95 is computed from fewer than a meaningful sample count.

Keep `verify-native-gpu-scroll-speed --surface dev-code-editor` only as the
handoff compatibility gate required by `AGENTS.md`. Use
`verify-native-dev-editor-scroll-speed` as a stricter regression gate, not as a
replacement for honest user-facing evidence.

### Example Switch Gate

The example switch gate must fail unless it proves final source presentation:

- ACK p95 is measured separately from final preview p95;
- pending overlay readback is recorded separately and cannot satisfy final
  switch success;
- final readback must be after `replace-source-result` and bound to the result
  frame revision plus source/project hash;
- preview child remains alive after every switch;
- every consumed preview loop report has `loop_error == null`;
- Cells/custom timings expose parse/runtime/layout components and fail against
  real budgets;
- rapid switching proves cancellation of running stale work, not just dropping
  stale pending work.

### Aggregate Gate

The aggregate gate must have one explicit contract. Current mismatch resolution
is part of the recovery:

- for handoff readiness, it should match the `AGENTS.md` native GPU gate list;
- for product regression readiness, it may also require dev-editor scroll
  speed, example switching, and idle-wake/demand-loop probes;
- it must not silently mix those scopes without labeling why each report is
  required.

Dev-editor scroll speed, example switching, and idle-wake/demand-loop probes
should either:

- run in a separate regression aggregate; or
- be clearly marked optional/product-regression gates in the report.

## Phase 4: Architecture Update After Recovery

After correctness is restored and verifiers are honest, implement the durable
architecture:

1. Dev editor rendering becomes a virtualized text surface with renderer-level
   scroll and cache metrics. Do not mutate arbitrary display-list nodes after
   layout unless the layout frame has an official incremental-update API.

2. Source replacement becomes a generation-based job pipeline:

- immutable payload generation;
- cancellable compile/runtime/layout jobs;
- final commit only if generation is still current;
- separate status overlay generation;
- reportable cancellation and queue metrics.

3. Source/project payloads compile through the same project abstraction that
   bundled examples use. Hash validation is not a substitute for executable
   multi-file semantics.

4. Runtime summary and debugger telemetry become lazy, bounded, and separate
   from the user-visible preview switch path.

## Revert Options

### Option A: Emergency rollback

Use only if the native playground must be unblocked before surgical repair can
be completed. In a throwaway worktree first, revert the demand-driven/editor
switching stack from `d5472fe` through `HEAD`, then reapply only the source-only
preview contract fixes that are known to be correct. Validate with the
`AGENTS.md` native GPU gates.

This is the fastest path to known earlier behavior, but it risks losing useful
source-only IPC and verifier improvements.

### Option B: Surgical repair

Preferred path.

1. Restore editor wheel responsiveness.
2. Remove or disable incomplete display-list scroll patching.
3. Fail role reports on non-null `loop_error`.
4. Fix example-switch measurement to final source frame.
5. Add cancellation/generation checks to the worker.
6. Restore strict budgets.
7. Re-run regression gates and the `AGENTS.md` handoff gates.

This keeps useful work while directly removing the regressions.

### Option C: Full architecture pass

Use after Option B is green. Replace the patched hot paths with the durable
virtualized editor and cancellable source-project pipeline described above.

## Acceptance Commands

First compile:

```bash
cargo check -p boon_native_playground -p xtask
```

Then run focused regression gates:

```bash
cargo xtask verify-native-dev-editor-scroll-speed --profile debug --report target/reports/native-gpu/dev-editor-scroll-speed-debug.json
cargo xtask verify-native-dev-editor-scroll-speed --profile release --report target/reports/native-gpu/dev-editor-scroll-speed-release.json
cargo xtask verify-native-example-switch-speed --profile debug --report target/reports/native-gpu/example-switch-speed-debug.json
cargo xtask verify-native-example-switch-speed --profile release --report target/reports/native-gpu/example-switch-speed-release.json
```

Before claiming native GPU handoff readiness, resolve the aggregate scope
mismatch. In the current checkout, `verify-native-gpu-all --check-existing`
requires more reports than the shorter `AGENTS.md` command list, including
idle-wake, visible-launch, native-examples, dev-window-editor, example-tabs,
editor-format, dev-editor-scroll-speed, example-switch-speed, and older speed
reports. Therefore one of these must happen first:

- update `verify-native-gpu-all` so handoff readiness aggregates only the
  `AGENTS.md` native GPU gate list; or
- update `AGENTS.md`, `docs/architecture/NATIVE_GPU_PIPELINE.md`, and this
  recovery plan together so the broader aggregate list is explicitly the
  authoritative handoff contract; or
- keep two aggregates: one `AGENTS.md` handoff aggregate and one product
  regression aggregate.

After the scope mismatch is resolved, run the authoritative handoff gate list.
The current `AGENTS.md` list is:

```bash
cargo xtask verify-platform-contract --report target/reports/native-gpu/platform-contract.json
cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json
cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json
cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json
cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json
cargo xtask verify-native-gpu-multiwindow --report target/reports/native-gpu/multiwindow.json
cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json
cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json
cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json
cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json
cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json
cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
```

If the broader current `verify-native-gpu-all` remains authoritative, these
commands are not sufficient by themselves; every additional report required by
that aggregate must also be generated fresh and must be documented as part of
the handoff contract.

## Subagent Verification

Initial investigation was split across independent tracks:

- recent diff/regression mapping;
- dev editor scroll hot path;
- preview/example-switch failure path;
- plan/verifier contract drift.

Those tracks agreed on the main findings:

- editor scroll responsiveness was reduced by the wheel scaling change;
- the current scroll fast path is an incomplete display-list mutation strategy;
- example switch ACK/readback timing can count pending overlay pixels instead
  of the final source result;
- preview role reports can say pass while containing a real loop error;
- the broader editor/example-switch plan drifted beyond the active native GPU
  handoff contract.

Current subagent review also checked this recovery artifact for contradictions.
Those review notes must be kept with the implementation summary, not used as a
substitute for native GPU verifier evidence.

## Definition Of Done

The regression is not resolved when a report says `pass`. It is resolved only
when all of the following are true:

- editor wheel input is visibly and measurably responsive again under sustained
  vertical and horizontal scroll probes;
- no scroll input is counted as success without actual offset and readback
  changes unless the buffer is clamped;
- preview source switching measures final source presentation, not pending
  overlay presentation;
- no consumed role report has non-null `loop_error`;
- Cells/custom source replacement no longer takes about 1.9 seconds on the
  measured path, or the remaining cost is isolated from user-visible switching;
- strict budgets are restored;
- `AGENTS.md` handoff gates and the focused regression gates pass from fresh
  reports bound to the current binary, worktree, budgets, PIDs, host events, and
  WGPU readbacks.
