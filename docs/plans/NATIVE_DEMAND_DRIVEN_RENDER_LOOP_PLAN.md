# Native Demand-Driven Render Loop Plan

Status: planned

Created: 2026-05-27

Review loop: revised after subagent review of `boon_native_app_window`,
`boon_native_playground`, `xtask`/report schemas, and the active native GPU
architecture contract.

## Summary

The native playground currently keeps both visible child windows active with an
unconditional interactive render loop. After first-frame proof, the shared
`boon_native_app_window` loop repeatedly samples input, acquires a WGPU surface
texture, calls the role render hook, presents the frame, and sleeps for 16 ms.
An idle preview and an idle dev window therefore still do GPU work at roughly
60 FPS. The dev window is especially expensive because its render hook can
refresh diagnostics, lay out the editor/debug shell, shape text, and encode the
full visible UI every frame.

The fix must be generic. It must not special-case Counter, TodoMVC, Cells, any
source path, any visible label, or any scenario name. The preview must wake for
source/runtime/document changes and then return to idle. The dev window must
wake for editor, catalog, transport, diagnostics, telemetry, caret, or footer
changes and then return to idle. Future custom examples must use the same wake
paths as bundled examples.

## Current Problem

The current shared window loop is in
`crates/boon_native_app_window/src/lib.rs` after the initial visible surface
proof. The loop always:

- checks size/scale;
- acquires a surface texture;
- samples coalesced mouse/keyboard input;
- calls the role render hook;
- submits and presents;
- sleeps for 16 ms.

For a manual launch, `--hold-ms 0` keeps this loop alive forever. The preview
and dev role reports say `interactive_frame_loop: true`, but they do not yet
distinguish continuous frames from demand-driven frames, skipped idle polls, or
the dirty reason for each rendered frame.

This violates the architecture goal that the dev UI must not slow down the
production preview, and it creates a bad foundation for multiple custom
examples because every open window would burn CPU even when unchanged.

## Goals

- Idle native preview and dev windows must not render continuously.
- Input polling and IPC handling must continue while rendering is idle.
- Accepted source replacement, runtime events, document/layout changes, error
  overlay changes, resize/scale changes, scroll/focus changes, caret blink, and
  verifier samples must wake the correct surface.
- Wake logic must be source-driven, role-driven, and revision-driven, not
  example-driven.
- The preview role must accept Boon source/project payloads without knowing
  whether they came from a built-in example, custom example, or edited buffer.
- The dev role must not make the preview repaint merely because debug telemetry
  was polled.
- Reports must prove idle frames were skipped and post-idle input/source changes
  update app-owned visible pixels.
- Code editor scrolling must stay fast in the same debug/manual build the user
  normally runs, not only in release probe reports.
- Example switching must not block the dev window while preview reparses,
  rebuilds runtime state, lays out the new document, or computes debug
  summaries.

## Non-Goals

- Do not rewrite the renderer.
- Do not replace `app_window`.
- Do not add example-specific scheduler branches.
- Do not depend on whole-desktop screenshots, COSMIC toplevel scraping, Xvfb,
  Ply, `xdotool`, or `ydotool`.
- Do not weaken existing native GPU report schemas, budgets, or negative checks.
- Do not treat visible manual launch as verifier evidence.

## Design

### Shared Scheduler Boundary

Add a small scheduler to `boon_native_app_window`. It should own host/window
timing and frame decisions, while role-specific state stays in
`boon_native_playground`. The shared crate must not learn runtime, document,
source, example, telemetry, editor, or catalog concepts.

Suggested shared types:

```rust
pub enum NativeRenderLoopMode {
    ContinuousProbe,
    DemandDriven,
}

pub enum NativeSchedulerReason {
    FirstFrame,
    SurfaceChanged,
    HostInput,
    Timer,
    ExternalWake,
    VerifierFrame,
    RequestedAnimation,
}

pub enum NativeRoleDirtyReason {
    SourcePayloadAccepted,
    WorkspaceSelectionChanged,
    RuntimeTurnApplied,
    DocumentPatchApplied,
    LayoutChanged,
    ScrollChanged,
    FocusChanged,
    TelemetrySummaryChanged,
    CaretBlink,
    VerifierFrame,
    ErrorOverlayChanged,
}

pub struct NativeRenderLoopState {
    pub mode: NativeRenderLoopMode,
    pub dirty_revision: u64,
    pub presented_revision: u64,
    pub rendered_frame_count: u64,
    pub skipped_idle_poll_count: u64,
    pub input_poll_count: u64,
    pub forced_frame_count: u64,
    pub scheduled_wake_count: u64,
    pub last_scheduler_reason: Option<NativeSchedulerReason>,
    pub last_role_dirty_reason: Option<NativeRoleDirtyReason>,
    pub next_wake_at: Option<Instant>,
}
```

`ContinuousProbe` preserves current proof/sample behavior where a verifier asks
for measured frames. Manual desktop launches should use `DemandDriven`.

Role dirty reasons belong to `boon_native_playground` reports only, and they
must use a closed generic enum rather than arbitrary strings. Schema and
negative checks must reject role reasons containing `custom:`, bundled example
IDs, visible labels, file paths, `://`, scenario names, or hardcoded UI text.
The shared scheduler may record a role reason for diagnostics, but it must never
branch on that reason.

The demand-driven loop should:

- poll input cheaply even when no frame is needed;
- avoid acquiring a WGPU surface texture unless the scheduler decides to render;
- render on the first frame, then only when dirty or when a scheduled wake is
  due;
- use a short poll interval while focused or immediately after input;
- use a longer idle backoff when no input, IPC, wake handle, or timer is
  pending;
- never block IPC processing behind frame presentation;
- keep explicit verifier readback/sample requests able to force a frame.

### Pre-Render Role Update Hook

Dirty detection cannot stay inside render hooks. Today preview and dev mutate
state from the render hook, but the render hook is called only after
`get_current_texture()` and WGPU encoder/view creation. A demand-driven loop
needs a non-WGPU hook that runs before deciding to render:

```rust
pub struct NativeWindowHooks {
    pub poll: NativeRolePollHook,
    pub render: NativeRenderHook,
}

pub struct NativePollContext<'a> {
    pub window_id: &'a boon_host::WindowId,
    pub surface_id: &'a boon_host::SurfaceId,
    pub surface_epoch: u64,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub input_delta: NativeInputDelta,
    pub now: Instant,
    pub forced_frame: bool,
}

pub struct NativePollResult {
    pub dirty: bool,
    pub role_revision: u64,
    pub role_dirty_reason: Option<NativeRoleDirtyReason>,
    pub next_wake_at: Option<Instant>,
    pub wants_animation_frame: bool,
}
```

The poll hook owns role mutation:

- preview input routing, source events, scroll/focus/edit state, error changes,
  and runtime/document revision updates;
- dev editor input, key repeat, caret blink, footer scroll, tab/workspace
  commands, preview transport commands, telemetry refresh, and diagnostics;
- timer checks that would otherwise be skipped when no frame is rendered.

The render hook should be WGPU-only: encode an already materialized snapshot,
return render proof data, and avoid dispatching source events, IPC commands, or
telemetry requests.

### Wake Handle

Long idle backoff must not delay source replacement or timer wakeups. Add a
generic wake mechanism owned by `boon_native_app_window` and clonable by role
code:

```rust
pub struct NativeWakeHandle { /* channel, condvar, or atomic event */ }
```

The wake handle should wake the demand loop without acquiring a WGPU surface.
Preview IPC must trigger it after accepted source/project payloads, runtime
events, error updates, or explicit verifier requests. Dev timers and transport
callbacks must trigger it after due telemetry refreshes, command
acknowledgements, delayed caret wakes, or key-repeat wakes. The loop still polls
at a bounded interval as a safety net, but correctness and latency must not
depend on long polling.

### Render Hook Result

Replace the current hook result of `serde_json::Value` with a small structured
result that still carries the proof JSON:

```rust
pub struct NativeRenderHookResult {
    pub proof: serde_json::Value,
    pub content_revision: u64,
    pub rendered: bool,
    pub content_changed: bool,
    pub role_dirty_reason: Option<NativeRoleDirtyReason>,
}
```

If the exact type migration is too large for one step, introduce an adapter:
existing hooks can return `NativeRenderHookResult::rendered_with_proof(proof)`
until preview/dev are migrated. The final state must not require rendering just
to learn that nothing changed.

Revisions are two-phase. The role poll hook reports the candidate
`role_revision`. The app-window loop may mark that revision as presented only
after command submission, `frame.present()`, and any required readback succeed
for the current surface epoch. A render hook must not claim a revision is
presented before the surface present has happened.

### Input Deltas

`sample_input_adapter` currently consumes some state, especially scroll deltas.
Demand-driven polling needs a non-destructive input cursor so an event cannot be
observed during an idle poll and then lost before the role update uses it.

Suggested types:

```rust
pub struct NativeInputCursor {
    pub last_mouse_button_sequence: u64,
    pub last_keyboard_sequence: u64,
    pub accumulated_scroll_x: f64,
    pub accumulated_scroll_y: f64,
}

pub struct NativeInputDelta {
    pub mouse_button_events: Vec<...>,
    pub keyboard_events: Vec<...>,
    pub scroll_delta_x: f64,
    pub scroll_delta_y: f64,
    pub mouse_window_pos: Option<...>,
    pub pressed_keys: Vec<String>,
    pub mouse_buttons_down: Vec<String>,
}
```

The cursor should advance only after the role poll hook has accepted the delta.
If the poll hook marks the role dirty, the accepted delta must remain visible to
the role state that the next render encodes.

The scheduler should mark `HostInput` dirty only when the role says the input
can change visible state. Raw mouse movement over an idle surface should not
force repaint unless hover/focus visuals are active.

### WGPU Surface Lifecycle

The current loop treats most non-success surface acquisition results as fatal
and keeps `surface_epoch` fixed. The demand-driven loop must make surface state
observable:

- increment surface epoch on reconfigure, lost/outdated recovery, or format
  change;
- invalidate renderer caches and readback proof caches when epoch, size, scale,
  or format changes;
- handle `Suboptimal` as renderable but mark surface changed;
- handle lost/outdated by reconfiguring and scheduling a frame;
- handle timeout by skipping the frame without marking the role revision
  presented;
- handle close/minimize without spinning or crashing;
- report lifecycle transitions in the role summary.

### Preview Invalidation

Extend `PreviewSharedRenderState` with explicit revisions:

- `dirty_revision`;
- `presented_revision`;
- `source_revision`;
- `runtime_revision`;
- `layout_revision`;
- `error_revision`;
- `scroll_revision`;
- `focus_revision`;
- `last_dirty_reason`;
- `last_presented_hash`.

Replace direct field mutation with methods:

- `mark_dirty(reason)`;
- `replace_source(project_or_source_payload)`;
- `note_error(error)`;
- `clear_error()`;
- `apply_runtime_event(source_batch)`;
- `apply_scroll(delta)`;
- `apply_focus_or_edit_overlay(change)`;
- `request_verifier_frame()`.

Increment `dirty_revision` for generic events only:

- initial source load;
- source/project payload accepted by IPC;
- source validation error changed;
- `LiveRuntime::apply_source_event` accepted a source event;
- runtime state or document output changed;
- layout frame changed;
- scroll/focus/edit state changed;
- error overlay changed or cleared;
- verifier readback requested.

Do not use example ID, example label, source file path, visible text, or
scenario name as a dirty reason.

The preview render hook should encode when the poll hook reports
`dirty_revision != presented_revision`, when size/scale/epoch changed, or when
an explicit verifier frame/readback is requested. After successful present, the
app-window loop commits `presented_revision = rendered_revision` and stores
frame hash/proof metadata.

Invalid or deferred source replacement must not poison the visible preview. Keep
the last good renderable layout frame and show a generic pending/error overlay
when a new payload is invalid, too large, or still deferred. The window should
stay alive, wake for the overlay update, and expose the error in telemetry.

App-owned readback proof caching must be keyed by content revision, surface
epoch, size, format, and readback request. Reusing the first proof after a
source/runtime/layout/error/scroll/focus/caret change must be a verifier
failure.

### Dev Window Invalidation

Extend `DevWindowShell` with explicit revisions:

- `dirty_revision`;
- `presented_revision`;
- `editor_revision`;
- `selection_revision`;
- `scroll_revision`;
- `workspace_revision`;
- `transport_revision`;
- `telemetry_revision`;
- `diagnostic_revision`;
- `footer_revision`;
- `caret_revision`;
- `last_dirty_reason`.

Increment these through existing generic model boundaries:

- `ExampleWorkspace` changes selected buffer, dirty flag, custom name, custom
  entry, tab selection, add/remove/rename state;
- `CodeEditorModel` changes text, caret, selection, undo/redo, scroll;
- `PreviewTransport` changes command status, ack, error, or connection status;
- telemetry summary polling returns a different content hash;
- diagnostics or formatting result changes;
- footer scroll changes.

Dev telemetry polling should run on a timer, for example once per second while
visible, and should only dirty the dev window when the summary hash changes. It
must not dirty the preview. Editor caret blinking should schedule wakes only
while the editor or dev text input is focused.

Post-idle clicks must still route without first rendering a new frame. Maintain
a non-WGPU hit-test/layout snapshot for dev input dispatch. Rendering consumes
that snapshot; input dispatch updates it when model state changes.

Preview transport commands need monotonic command IDs and source revisions.
Preview ACKs with stale command IDs must update diagnostics if useful, but must
not overwrite selected source status or clear a newer dirty revision.

### Code Editor Scroll Path

The existing release scroll report proves a 10,000-line dev editor corpus can
scroll in about one frame, but the manual debug path is not protected yet. The
plan must make dev editor scrolling an explicit first-class path:

- wheel events are accepted by the pre-render poll hook without first acquiring
  a WGPU surface;
- scroll deltas update only editor scroll state and editor/layout revisions;
- visible line materialization stays bounded to visible lines plus overscan;
- syntax tokens are reused from the buffer model and are not reparsed on scroll;
- debug footer telemetry polling does not run in the scroll hot path;
- the hit-test snapshot updates when scroll changes, but source replacement and
  preview runtime summaries do not run for passive editor scrolling;
- horizontal scroll and vertical scroll are measured separately;
- horizontal wheel/trackpad deltas update `CodeEditorModel.scroll_column`;
- horizontal-only scroll changes visible columns without changing
  `scroll_line`;
- hit testing and caret x mapping account for `scroll_column`.

Add debug and release gates for the same user-visible surface:

```bash
cargo xtask verify-native-dev-editor-scroll-speed \
  --profile debug \
  --report target/reports/native-gpu/dev-editor-scroll-speed-debug.json

cargo xtask verify-native-dev-editor-scroll-speed \
  --profile release \
  --report target/reports/native-gpu/dev-editor-scroll-speed-release.json
```

This must be a real new xtask command with `--profile debug|release`, default
report path `target/reports/native-gpu/dev-editor-scroll-speed-{profile}.json`,
`build_profile`, `tested_binary`, and profile-specific budgets. Existing
`verify-native-gpu-scroll-speed --surface dev-code-editor` must either become a
compatibility alias to the release gate or be removed from the final gate after
`NATIVE_GPU_PIPELINE.md` is updated. The implementation must avoid three
overlapping dev-editor scroll reports.

The verifier must use a passive scroll-only probe mode. It must not reuse the
current command probe path that also runs tab switches, dev commands, editor
input, and custom example actions. The gate fails if `command_probe`,
`replace_selected_preview`, `PreviewTransport::replace_code`, or
`PreviewTransport::runtime_summary` runs during the scroll sample.

Required scroll report fields:

- `profile`;
- `build_profile`;
- `tested_binary`;
- `surface_under_test`;
- `line_count`;
- `longest_line_bytes`;
- `scroll_line_before_after`;
- `scroll_column_before_after`;
- `visible_line_range_before_after`;
- `visible_column_range_before_after`;
- `materialized_line_count_max`;
- `syntax_token_count`;
- `parser_diagnostic_delta`;
- `footer_telemetry_poll_delta`;
- `preview_runtime_summary_query_delta`;
- `source_replace_count_for_passive_scroll`;
- `replace_code_count_during_scroll`;
- `runtime_dispatch_count_for_passive_scroll`;
- `graph_rebuild_count`;
- `telemetry_poll_count_in_scroll_hot_path`;
- `dev_editor_frame_ms_p50_p95_p99_max`;
- `wheel_to_visible_ms_p95_per_axis`;
- `missed_frame_count`;
- `dropped_frame_count`;
- `frames_over_16_7_ms`;
- `text_runs_shaped_p95`;
- `text_cache_hit_rate`;
- `glyph_atlas_evictions`;
- `upload_bytes_p50_p95_max`;
- `preview_blocked_on_ipc_count`;
- app-owned readback artifacts;
- operator/real wheel input evidence.

The scroll gate must include a selected custom-example buffer in addition to the
large generated corpus.

### Example Switching Path

Current tab switching is generic but can be slow because `SelectTab` calls
`replace_selected_preview`, which sends synchronous `ReplaceCode`. The preview
then may parse, lower, build runtime summary, run document layout, and serialize
large debug data before the dev command returns. Existing evidence showed
TodoMVC switching around tens of milliseconds, but switching back to Cells took
close to one second because Cells has much larger source/runtime/debug payloads.

Demand-driven rendering helps the preview wake after switching, but it does not
by itself make `ReplaceCode` fast. Add an explicit asynchronous source-replace
contract:

- selecting a tab first runs `select_source_ui`, which updates selected
  tab/catalog/editor state and dirties the dev shell before source loading,
  parsing, layout, runtime summary work, or preview IPC can block;
- source loading is preloaded/cached or async with a pending buffer placeholder;
- dev then runs `enqueue_preview_replace` with a generic `SourceProjectPayload`;
- preview returns a small immediate `replace-source-queued` ACK after validating
  hashes and enqueueing the replace job;
- parse/lower/runtime/layout/debug-summary work runs in the preview role without
  blocking the dev UI;
- parse/lower/runtime/layout/debug-summary work never runs on the render loop or
  IPC accept path;
- preview keeps the last good frame while the replacement is pending;
- dev shows bounded pending/error/ready status from command IDs;
- once the preview has a new renderable frame, it increments preview dirty
  revision and wakes rendering through `NativeWakeHandle`;
- large runtime summaries are never serialized into the synchronous ACK;
- stale ACKs/results are ignored by command ID and source revision;
- built-in examples and custom examples use the same source/project payload
  route.

Concrete source/project payload schema:

```rust
pub struct SourceProjectPayload {
    pub command_id: u64,
    pub source_revision: u64,
    pub source_identity: String,
    pub project_hash: String,
    pub entrypoint_unit: String,
    pub units: Vec<SourceProjectUnit>,
    pub scenario_payload: Option<ScenarioPayload>,
}

pub struct SourceProjectUnit {
    pub virtual_uri: String,
    pub text: String,
    pub sha256: String,
}
```

`source_identity` and `virtual_uri` are opaque data keys. Filesystem paths,
display labels, and scenario file paths may appear in dev/verifier diagnostics,
but preview must not read sibling `.scn` files or branch by path, label, bundled
example ID, or custom/example origin during switching.

Protocol messages:

- `replace-source`: request carrying `SourceProjectPayload`;
- `replace-source-queued`: immediate bounded ACK with only `command_id`,
  `source_revision`, `project_hash`, queue status, byte counts, and timing;
- `replace-source-status`: bounded pending/ready/error status query or event;
- `replace-source-result`: async result with parse/lower/runtime/layout status,
  frame revision, and bounded debug-summary metadata.

The synchronous ACK must not contain full source, `document_layout_proof`,
`preview_runtime_summary`, parse/lower output, layout frames, runtime state, or
debug summaries. The switch-speed gate must fail if dev tab visual update waits
for the preview ACK.

Add explicit dev state:

- `next_command_id`;
- selected `source_identity`;
- selected `source_revision`;
- `pending_replace`;
- `latest_ready_replace`;
- stale ACK/result diagnostics.

Add explicit preview state:

- committed source/runtime/layout revision;
- pending replace job;
- latest accepted command/source revision;
- last good frame/layout;
- pending/error overlay state;
- replace result status cache.

Add a bounded latest-wins source-replace worker:

- IPC thread validates hashes, enqueues, ACKs, and returns;
- worker parses/lowers/builds runtime/layout outside state locks;
- stale queued work is coalesced or cancelled;
- committed preview state swaps atomically only after a renderable layout
  succeeds;
- invalid/deferred/error results keep the last good committed frame and update
  only pending/error overlay plus dirty revision;
- result status wakes dev/preview through `NativeWakeHandle`.

Required worker report fields:

- `replace_job_queue_depth`;
- `replace_job_dropped_stale`;
- `render_thread_blocked_on_replace_count`;
- `preview_frame_ms_p95_during_replace`;
- `preview_blocked_on_ipc_count`.

Pending/ready status must use the replace-status/result path. It must not depend
on the existing one-second debug-summary polling timer.

Add an explicit speed gate:

```bash
cargo xtask verify-native-example-switch-speed \
  --profile debug \
  --report target/reports/native-gpu/example-switch-speed-debug.json

cargo xtask verify-native-example-switch-speed \
  --profile release \
  --report target/reports/native-gpu/example-switch-speed-release.json
```

The gate must cover Counter, TodoMVC, Cells, and at least two custom examples.
It must measure:

- click-to-dev-tab-visual-update;
- click-to-preview-pending-status;
- click-to-preview-new-frame-presented;
- synchronous ACK payload bytes and latency;
- async parse/lower/runtime/layout latency;
- debug-summary payload bytes and latency;
- stale command/result rejection.

The gate must cover rapid A-B-A switching, a large custom source above the old
64 KiB synchronous layout threshold, invalid custom source preserving the last
good frame, duplicate/renamed labels, and changed logical paths.

The gate must fail if scheduler, preview, renderer, dirty/wake reasons, or
source-replace execution branches depend on bundled example IDs, visible
labels, file paths, scenarios, or custom-example special cases. Dev/catalog use
of opaque stable source IDs as data keys is allowed.

### Caret And Animation Policy

Default policy is no continuous animation.

Caret blink should be modeled as a scheduled dirty reason:

- editor focused: schedule next wake at the configured blink interval;
- preview text input focused: schedule next wake at the configured blink
  interval;
- no focused text input: do not schedule caret wakes;
- on caret movement or text edit: make caret visible immediately and restart the
  blink timer.

If a future example needs animation, it must request animation through a generic
document/runtime capability such as `RequestedAnimation`, not by example name.

### Multiple Custom Examples

The plan must support many custom examples without frozen previews:

- each editor buffer/project has a source identity independent of display name;
- selecting or editing any buffer sends a generic source/project payload;
- the preview accepts the payload, updates runtime/layout/error state, increments
  preview dirty revision, and wakes one render;
- removing or renaming a custom example changes only dev workspace/catalog state
  unless the selected source changes;
- stale IPC acknowledgements are ignored by monotonically increasing command IDs
  or source revisions.

No scheduler code may branch on official examples, custom examples, source file
paths, or tab labels.

The verifier must use at least two custom entries, not only a single
`custom-counter.bn` fixture:

- stable custom IDs independent of display names;
- duplicate or changed display labels;
- two single-file custom examples;
- one multi-file custom project;
- rename and remove operations;
- switch between custom entries;
- edit/run selected custom source;
- proof that preview receives only source/project payload, source revision, and
  content hash, never an example name as a render shortcut.

## Observability

The existing `AppWindowSurfaceProof` is emitted before the hold/interactive
loop, so it cannot currently contain idle-loop counters. Add either a final role
loop summary written after the idle observation window, or a periodically
updated role report that the verifier can query while the process stays alive.

Add report fields to role proofs, final role summaries, or idle/wake reports:

- `render_loop_mode`;
- `idle_observation_ms`;
- `preview_child_pid`;
- `dev_child_pid`;
- `cpu_measurement_source`;
- `idle_cpu_percent_preview_p95`;
- `idle_cpu_percent_dev_p95`;
- `combined_idle_cpu_percent_p95`;
- `dirty_revision`;
- `presented_revision`;
- `rendered_frame_count`;
- `preview_idle_rendered_frame_delta`;
- `dev_idle_rendered_frame_delta`;
- `skipped_idle_poll_count`;
- `input_poll_count`;
- `forced_frame_count`;
- `scheduled_wake_count`;
- `last_scheduler_reason`;
- `last_role_dirty_reason`;
- `last_rendered_at_ms`;
- `last_input_poll_at_ms`;
- `post_idle_input_to_present_ms`;
- `post_idle_source_replace_to_present_ms`;
- `post_idle_frame_hash_before`;
- `post_idle_frame_hash_after`;
- `post_idle_frame_hash_changed`;
- `post_idle_source_replace_hash_changed`;
- `readback_artifact_before`;
- `readback_artifact_after`;
- `visual_capture_method`;
- `operator_host_input`;
- `real_os_input`;
- `private_runtime_dispatch_used`;
- `preview_receives_example_name`.

These fields must be generic and must be emitted for bundled and custom
examples.

CPU evidence should come from child-process-owned or procfs evidence: child PID,
command line, `/proc/<pid>/stat` tick deltas, wall-time sample interval, and raw
sample values. Do not use compositor state, whole-desktop screenshots, or human
observation as idle proof.

## Implementation Steps

1. Add scheduler data types and unit tests in `boon_native_app_window`.
2. Add `NativeRenderLoopMode`, `NativeWindowHooks`, `NativePollResult`,
   `NativeWakeHandle`, `NativeInputCursor`, and `NativeInputDelta`.
3. Add fake-clock scheduler tests and input-cursor tests before changing the
   live loop.
4. Keep the initial proof/sample frames unchanged for existing verifier paths.
5. Add a compatibility adapter for existing render hooks.
6. Move preview input, scroll/focus/edit overlays, source-event dispatch,
   runtime updates, and error overlay changes from render hooks into poll/update
   paths.
7. Move dev input, key repeat, caret blink, footer scroll, tab/workspace
   commands, transport commands, telemetry refresh, and diagnostics from render
   hooks into poll/update paths.
8. Wire preview IPC, dev transport callbacks, timers, and verifier requests to
   `NativeWakeHandle`.
9. Replace the infinite 16 ms loop with a demand-driven loop for manual
   `--hold-ms 0` desktop/preview/dev roles.
10. Make render hooks structured and WGPU-only.
11. Add two-phase presented revision commits after successful submit/present.
12. Add surface lifecycle handling, epoch increments, and cache invalidation.
13. Add explicit dev-editor scroll revisions, horizontal `scroll_column`
    handling, profile-specific scroll reports, and debug/release scroll budgets.
14. Convert preview source replacement to the async command/revision contract so
    example switching does not synchronously block on source loading,
    parse/lower/runtime/layout work, or runtime summaries.
15. Add role report fields for idle render behavior, post-idle wake behavior,
    dev-editor scroll behavior, and example-switch behavior.
16. Register the idle/wake, dev-editor-scroll, and example-switch gates in
    `xtask`, report schemas, label-contract audits, negative fixtures, budgets,
    and the native GPU aggregate.
17. Update `docs/architecture/NATIVE_GPU_PIPELINE.md` so the active native GPU
    contract names these new gates, their reports, and their budgets.
18. Keep `verify-native-gpu-scroll-speed --surface dev-code-editor` only as a
    compatibility alias to the new release dev-editor-scroll gate, or remove it
    from the active contract when `NATIVE_GPU_PIPELINE.md` is updated. Do not
    leave three overlapping editor-scroll reports.
19. Run the full active native GPU gates plus the new gates. Visible launch is a
    separate manual follow-up after reports pass.

## Tests

### Unit Tests

Add tests for the scheduler:

- idle demand-driven loop renders first frame once and then skips render work;
- resize marks dirty and renders one frame;
- scale change marks dirty and renders one frame;
- input delta can wake without requiring continuous frames;
- no input delta keeps the surface idle;
- focused caret schedules a wake at the blink interval;
- unfocused caret schedules no wake;
- explicit verifier readback forces a frame;
- requested animation can request continuous frames through a generic flag;
- dirty revision/presented revision cannot go backwards;
- two-phase presented revision commits only after successful present;
- surface lost/outdated/timeout handling does not mark unpresented revisions as
  presented;
- surface epoch increments invalidate renderer/readback caches;
- wake handle interrupts idle backoff;
- input cursor does not consume scroll/key/button events before role update
  accepts them.

Add preview tests:

- source replacement increments preview dirty revision;
- runtime source event increments preview dirty revision only on accepted change;
- render error changes dirty revision and keeps last valid frame;
- clearing render error changes dirty revision;
- scroll/focus changes dirty revision without example branches;
- invalid/deferred source payload keeps last good layout and shows an overlay;
- app-owned proof cache invalidates on content revision, surface epoch, size,
  format, and readback request changes;
- stale source-revision ACKs cannot overwrite newer preview state.

Add dev tests:

- editor text edit increments editor and shell dirty revisions;
- caret move increments selection revision and schedules blink;
- telemetry refresh with identical hash does not dirty the window;
- telemetry refresh with changed hash dirties the window;
- custom example rename/remove dirties dev shell only;
- selected source switch dirties dev shell and sends generic preview replace;
- duplicate custom labels still route by stable ID;
- stale preview ACKs are ignored by selected buffer status;
- non-WGPU hit-test snapshot routes post-idle clicks before a new render;
- passive editor scroll does not call `replace_selected_preview`;
- passive editor scroll does not query preview runtime summaries;
- passive editor scroll rebuilds only bounded visible/overscan editor layout;
- horizontal editor scroll updates `scroll_column` without changing
  `scroll_line`;
- hit testing and caret x mapping include `scroll_column`;
- tab selection updates dev state before preview replace completion;
- `select_source_ui` dirties selected tab/catalog/editor state before source
  loading, preview IPC, or preview ACK completion;
- async source loading shows a bounded pending placeholder instead of blocking
  tab selection;
- async source replace keeps last good preview frame while pending;
- large runtime summary is not included in synchronous replace ACK;
- synchronous replace ACK does not contain full source, layout proof, preview
  runtime summary, parse/lower output, runtime state, or debug summary;
- source replacement result for an old command ID cannot overwrite a newer tab;
- latest-wins source replacement coalesces/cancels stale queued work;
- rapid A-B-A switching presents only the latest accepted revision;
- invalid custom source keeps the previous good preview frame and reports a
  pending/error overlay.

### Native Verifier Tests

Add a native GPU idle/wake gate, for example:

```bash
cargo xtask verify-native-gpu-idle-wake \
  --example cells \
  --idle-ms 5000 \
  --report target/reports/native-gpu/idle-wake-cells.json
```

The gate must prove:

- preview and dev child processes stay alive;
- idle render counts remain under budget;
- idle CPU remains under budget;
- CPU is measured from child PID procfs tick deltas with raw samples and elapsed
  wall time in the report;
- input polling continues during idle;
- a post-idle click/type/scroll wakes the correct role;
- a post-idle source replacement wakes preview and changes the frame hash;
- no example ID or visible-label branch was used to wake rendering;
- app-owned readback proves the frame changed;
- frame hashes change only after deterministic visible changes, not because a
  stale first-frame proof was copied;
- stale windows or missing role reports fail loudly.

Add table-driven custom project coverage:

```bash
cargo xtask verify-native-gpu-idle-wake \
  --custom-project-fixture target/fixtures/native-gpu/custom-projects.json \
  --idle-ms 5000 \
  --report target/reports/native-gpu/idle-wake-custom-projects.json
```

The custom fixture must contain at least two source entries with stable IDs,
renames, removals, duplicate/changing labels, switching, editing, and run
operations. This prevents the implementation from only working for manifest
examples.

Register all new gates in every enforcement surface:

- `crates/xtask/src/main.rs` `XTASK_COMMANDS`;
- `crates/xtask/src/main.rs` dispatch arms;
- `crates/xtask` verifier functions;
- `native_default_report_path()`;
- `native_gpu_required_reports()` and `verify-native-gpu-all`;
- `native_gpu_label_contract_blockers()`;
- blocker-audit allowlists in `crates/boon_report_schema`;
- blocker-audit allowlists in `crates/xtask`;
- report schema required-field validation;
- `verify-report-schema` report discovery/scan compatibility;
- `budgets/native-gpu.toml`;
- `docs/architecture/NATIVE_GPU_PIPELINE.md`.

`verify-native-gpu-all` must require the debug and release versions of
dev-editor-scroll and example-switch speed, because the manually launched
playground normally uses debug unless explicitly launched from `target/release`.

The dev-editor-scroll report must include at least:

- `profile`;
- `build_profile`;
- `tested_binary`;
- `surface_under_test`;
- `line_count`;
- `longest_line_bytes`;
- `scroll_line_before_after`;
- `scroll_column_before_after`;
- `visible_line_range_before_after`;
- `visible_column_range_before_after`;
- `dev_editor_frame_ms_p50_p95_p99_max`;
- `wheel_to_visible_ms_p95_per_axis`;
- `missed_frame_count`;
- `dropped_frame_count`;
- `frames_over_16_7_ms`;
- `runtime_dispatch_count_for_passive_scroll`;
- `graph_rebuild_count`;
- `source_replace_count_for_passive_scroll`;
- `replace_code_count_during_scroll`;
- `preview_runtime_summary_query_count_for_passive_scroll`;
- `preview_runtime_summary_query_delta`;
- `telemetry_poll_count_in_scroll_hot_path`;
- `footer_telemetry_poll_delta`;
- `visible_line_count`;
- `materialized_line_count_max`;
- `text_runs_shaped_p95`;
- `text_cache_hit_rate`;
- `glyph_atlas_evictions`;
- `upload_bytes_p50_p95_max`;
- `preview_blocked_on_ipc_count`;
- app-owned readback artifacts;
- operator/real wheel input evidence.

The example-switch-speed report must include at least:

- `profile`;
- `build_profile`;
- `switch_sequence`;
- `custom_fixture_hash`;
- per-switch `command_id`;
- per-switch `source_revision`;
- per-switch `source_hash`;
- per-switch `payload_kind`;
- per-switch `ack_latency_ms`;
- per-switch `ack_payload_bytes`;
- per-switch `click_to_dev_tab_visual_update_ms`;
- per-switch `click_to_preview_pending_status_ms`;
- per-switch `click_to_preview_new_frame_presented_ms`;
- per-switch parse/lower/runtime/layout timings;
- debug-summary bytes and latency;
- `stale_ack_rejected`;
- `stale_result_rejected`;
- `preview_receives_example_name: false`;
- `sync_ack_contains_runtime_summary: false`;
- `sync_ack_contains_layout_proof: false`;
- `last_good_frame_kept_while_pending: true`;
- before/after app-owned readback hashes.

Add negative fixtures that fail loudly for:

- copied first-frame hashes;
- stale or missing PIDs;
- fake CPU samples;
- COSMIC, Ply, desktop screenshot, or human-observation evidence;
- missing custom project identity;
- example-name wake branches;
- release report reused for a debug gate;
- passive editor scroll calling source replacement;
- passive editor scroll querying preview runtime summaries;
- full-file editor widget-tree materialization;
- full-file reshaping on scroll;
- missing horizontal scroll-axis evidence;
- switch ACK containing runtime summary;
- switch ACK containing layout proof;
- switch command without command ID;
- switch command without source revision;
- stale switch result applied to the current source;
- preview receiving an example name as render input;
- source replacement branching by visible label or filesystem path;
- dirty reason containing `custom:`, bundled IDs, labels, paths, `://`,
  scenario names, or hardcoded UI text.

## Targets

`budgets/native-gpu.toml` must have profile-specific sections:

- `[idle_wake.debug]`;
- `[idle_wake.release]`;
- `[dev_editor_scroll.debug]`;
- `[dev_editor_scroll.release]`;
- `[example_switch.debug]`;
- `[example_switch.release]`.

Debug mode:

- idle preview CPU p95 <= 3%;
- idle dev CPU p95 <= 5%;
- combined idle playground CPU p95 <= 8%;
- idle preview rendered frames after first stable frame <= 2 per 5 seconds
  unless a caret or animation is focused;
- idle dev rendered frames after first stable frame <= 10 per 5 seconds when
  editor caret is focused, <= 2 per 5 seconds when unfocused;
- post-idle input-to-present p95 <= 120 ms;
- post-idle source-replace-to-present p95 <= 250 ms;
- dev editor wheel-to-visible p95 <= 50 ms, max <= 120 ms;
- dev editor scroll must not perform preview IPC or source replacement;
- example switch click-to-dev-tab-visual-update p95 <= 50 ms;
- example switch synchronous ACK p95 <= 50 ms and max <= 120 ms;
- example switch click-to-preview-new-frame-presented p95 <= 350 ms for bundled
  examples and <= 500 ms for large custom examples;
- synchronous replace ACK payload <= 16 KiB.

Release mode:

- idle preview CPU p95 <= 1%;
- idle dev CPU p95 <= 2%;
- combined idle playground CPU p95 <= 3%;
- idle preview rendered frames after first stable frame <= 1 per 5 seconds
  unless a caret or animation is focused;
- idle dev rendered frames after first stable frame <= 6 per 5 seconds when
  editor caret is focused, <= 1 per 5 seconds when unfocused;
- post-idle input-to-present p95 <= 33 ms;
- post-idle source-replace-to-present p95 <= 75 ms;
- dev editor wheel-to-visible p95 <= 16.7 ms, max <= 33.4 ms;
- example switch click-to-dev-tab-visual-update p95 <= 16.7 ms;
- example switch synchronous ACK p95 <= 16.7 ms and max <= 50 ms;
- example switch click-to-preview-new-frame-presented p95 <= 120 ms for bundled
  examples and <= 200 ms for large custom examples;
- synchronous replace ACK payload <= 16 KiB.

The CPU budgets are intentionally stricter than the immediate observed bug. If
they are too strict on the current machine, the implementation should report the
measured baseline and adjust only with evidence in this file.

## Final Gate

After implementation, run:

```bash
cargo test -p boon_native_app_window --lib
cargo test -p boon_native_playground --bin boon_native_playground
cargo test -p boon_runtime --lib
cargo xtask verify-platform-contract --report target/reports/native-gpu/platform-contract.json
cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json
cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json
cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json
cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json
cargo xtask verify-native-gpu-multiwindow --report target/reports/native-gpu/multiwindow.json
cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json
cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json
cargo xtask verify-native-gpu-idle-wake --example counter --report target/reports/native-gpu/idle-wake-counter.json
cargo xtask verify-native-gpu-idle-wake --example todomvc --report target/reports/native-gpu/idle-wake-todomvc.json
cargo xtask verify-native-gpu-idle-wake --example cells --report target/reports/native-gpu/idle-wake-cells.json
cargo xtask verify-native-gpu-idle-wake --custom-project-fixture target/fixtures/native-gpu/custom-projects.json --report target/reports/native-gpu/idle-wake-custom-projects.json
cargo xtask verify-native-dev-editor-scroll-speed --profile debug --report target/reports/native-gpu/dev-editor-scroll-speed-debug.json
cargo xtask verify-native-dev-editor-scroll-speed --profile release --report target/reports/native-gpu/dev-editor-scroll-speed-release.json
cargo xtask verify-native-example-switch-speed --profile debug --report target/reports/native-gpu/example-switch-speed-debug.json
cargo xtask verify-native-example-switch-speed --profile release --report target/reports/native-gpu/example-switch-speed-release.json
cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json
cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json
cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json
cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
```

`verify-native-gpu-scroll-speed --surface dev-code-editor` may remain in the
final gate only as a compatibility alias whose report is hash-linked to
`dev-editor-scroll-speed-release.json`, or until `NATIVE_GPU_PIPELINE.md` is
updated to remove the older surface-specific command.

## Manual Follow-Up

Visible launch is not native proof. After the native reports pass, build and
launch only for human testing:

```bash
cargo build -p boon_native_playground
cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_native_playground --role desktop --example cells
pgrep -af boon_native_playground
```

## Acceptance Criteria

- Idle CPU no longer stays high when no user input, IPC update, caret blink, or
  animation is active.
- Preview updates after being idle without needing a restart.
- Dev updates after being idle without needing a restart.
- Switching, adding, renaming, or removing custom examples cannot freeze the
  preview because all wake paths are revision-based.
- No new branches are added for Counter, TodoMVC, Cells, source paths, visible
  labels, or scenario labels.
- Existing interaction-speed and scroll-speed reports still pass.
- Idle/wake evidence is enforced by xtask/schema/budgets/negative checks and by
  `verify-native-gpu-all`.
- Native reports include enough evidence to diagnose any future idle CPU or
  frozen-preview regression.
