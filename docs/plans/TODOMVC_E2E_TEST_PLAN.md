# TodoMVC E2E Test Plan

This plan is derived from the existing TodoMVC test material in
`~/repos/boon`, but it tightens the contract for Boon Circuit.

This is a TodoMVC-specific specialization of
[EXAMPLE_VERIFICATION_PLAN.md](EXAMPLE_VERIFICATION_PLAN.md). The shared plan is
also the contract for Cells and future examples.

The old repo has useful coverage in:

- `playground/frontend/src/examples/todo_mvc/todo_mvc.expected`
- `crates/boon-cli/tests/todo_mvc_logic.bn.test`
- `crates/boon-cli/tests/todo_mvc_complete.bn.test`
- `crates/boon-engine-actors-lite/src/todo_acceptance.rs`
- `tools/scripts/test_todo_mvc_wasm.sh`

The new repo should not copy the old tests mechanically. The old browser tests
contain sleeps, commented-out bug tests, engine-specific skips, and DOM text
checks that can hide semantic failures. Earlier experiments also showed that
headless WebGPU/browser paths can fail differently from real windows, and that
Ply can have real display bugs such as bad Wayland scaling or a blurred first
frame. Boon Circuit therefore treats a headed native Ply window as the primary
e2e acceptance surface. Semantic and headless tests are still required, but they
are supporting evidence and debug tools, not substitutes for seeing and driving
the real UI.

## What Counts As Honest

A TodoMVC e2e pass requires all of these:

1. The tested program is `examples/todomvc.bn`, not Rust hardcoded TodoMVC.
2. The source has no app-visible row `id`, no `next_todo_id`, and no identity or
   reference comparison.
3. One scenario file drives all test surfaces.
4. Semantic state is asserted after every action.
5. Semantic deltas and render deltas are asserted for key actions.
6. A headed Ply window is opened, inspected, and driven through real OS input.
7. The renderer is checked without diffing the whole UI tree.
8. Manual testing uses the same scenario labels and records evidence.
9. No skipped/commented bug scenario is counted as covered.
10. No fixed sleeps are used as correctness. Tests wait for deterministic idle or
   fail after a bounded tick/frame limit.
11. Release-mode interaction latency is within the example budget.
12. RAM and VRAM deltas are within the example budget.
13. Reports include source hash, scenario hash, runtime stats, display/backend
    metadata, screenshots, and failure artifacts.

## Test Layers

### Layer 1: Headed Ply Replay

This is the primary e2e gate.

Command shape:

```bash
cargo xtask verify-todomvc-headed-ply \
  examples/todomvc.bn \
  examples/todomvc.scn \
  --report target/reports/todomvc-headed-ply.json
```

This opens the same native Ply playground a user will run, keeps the window
visible, injects input through the OS event path, and records semantic/render
evidence while the UI is on screen. The test should be automated enough to be
repeatable, but it must remain observable by a human tester.

The replay must drive real interactions:

- pointer move, hover, click, and double-click.
- keyboard typing, Enter, Escape, Tab when relevant.
- focus and blur through the real window.
- checkbox hit targets.
- delete button hover and click.
- edits while the input is visible.

Visual checkpoints are mandatory:

```text
initial window is sharp, correctly scaled, and not blurred
main input focus is visible
checkboxes and delete hit targets are visible
text is not clipped or overlapped
footer/filter layout is stable
edit input appears in the correct row
large-list smoke keeps interaction responsive
```

The headed replay report must include:

```text
program_hash
scenario_hash
os
display_server
window_backend
display_scale
display_socket_or_compositor_connection
window_size
framebuffer_size
window_pid
window_title
input_backend
capture_backend
focused_window_proof
graph_node_count
semantic_trace_hash
render_patch_trace_hash
per-step pass/fail
checkpoint_screenshot_or_video_paths
checkpoint_artifact_sha256s
nonblank_screenshot_hashes
input_focus_evidence
per_step_pointer_keyboard_route
timing_frame_stats
manual_observer_notes_if_present
```

Run at least one headed pass on the normal desktop stack. When possible, run
both Wayland and X11/XWayland, and include common display scale factors such as
100 percent and the user's configured scaled mode. If scaling is blurred on
startup, that is a failed e2e run even if semantic state is correct.

### Layer 2: Manual Human Pass

Manual testing is a first-class acceptance gate, not a courtesy pass. A real
tester should interact with the live playground using the same scenario labels
as the replay, while watching both the UI and the state/delta inspector.

Template and preparation shape:

```bash
cargo xtask verify-todomvc-human --write-template --report target/reports/manual-templates/todomvc-human.json
cargo xtask prepare-todomvc-human-report \
  --observer <real-name> \
  --started <unix-start> \
  --finished <unix-finish> \
  --window-pid <visible-playground-pid> \
  --focused-window-proof <how-focus-was-confirmed> \
  --notes <visual-notes> \
  --capture-method <tool-used> \
  --artifact <manual-png-or-video> \
  --pass-label <each-todomvc-scenario-label> \
  --report target/reports/todomvc-human.json
```

The playground must expose:

```text
scenario checklist
current semantic state
last source event
last semantic deltas
last render patches
selected row data
runtime graph/node count
dirty key count
display scale/window backend
```

Manual pass evidence is written to:

```text
target/reports/todomvc-human.json
target/reports/manual-artifacts/todomvc-human-checkpoint-*.png
```

The manual report records each checked scenario label, display/backend details,
the final state hash, screenshots, notes about visual quality, and whether the
tester deviated from the scripted scenario. It must include `manual_observer`,
`generated_at_utc`, source/scenario hash matches, screenshot or video artifact
hashes, the screenshot/video capture method, and per-label pass/fail. Manual
checkpoint files must be created during the recorded session, not copied from an
old run. The prepared report must also record the visible manual playground
process id through `--window-pid` and a concrete `--focused-window-proof`;
reusing the headed verifier process id is not valid manual evidence.

Checker form:

```bash
cargo xtask verify-todomvc-human --check --max-age 24h --report target/reports/todomvc-human.json
```

### Layer 3: Semantic Trace Runner

This is the deterministic semantics gate and the main debugging tool when the
headed UI fails. It is required before release, but it is not sufficient e2e
acceptance by itself.

Command shape:

```bash
cargo xtask verify-todomvc-semantic \
  examples/todomvc.bn \
  examples/todomvc.scn \
  --report target/reports/todomvc-semantic.json
```

It runs the real parser, typed IR, static graph runtime, source event injection,
`HOLD` commits, `LIST` memories, and semantic delta emission.

It does not start a renderer, so it cannot prove focus, scaling, blur,
hit-testing, or frame presentation.

Assertions:

```text
store.new_todo_text
store.selected_filter
store.todos visible data
store.todos completed flags
active_count
completed_count
all_completed
focused source/field when modeled semantically
semantic deltas emitted in the tick
runtime stats
```

Runtime stats must include:

```text
program_hash
scenario_hash
graph_node_count
list_slots_allocated
dirty_node_count per step
dirty_key_count per step
semantic_delta_count per step
allocation counters if available
```

### Layer 4: Headless Ply Render Smoke

This verifies lowering from semantic deltas to deterministic render patches.
It is a fast CI smoke and regression aid only. It is not an acceptance gate for
user-visible behavior because headless paths can miss windowing, scaling,
presentation, WebGPU, and input-routing failures.

Command shape:

```bash
cargo xtask verify-todomvc-ply-headless \
  examples/todomvc.bn \
  examples/todomvc.scn \
  --report target/reports/todomvc-ply-headless.json
```

Assertions:

```text
render insert/remove/move patches
text patches
checkbox checked patches
input value patches
focus patches
hover/delete-button visibility
no whole-tree diff required
```

This layer may inspect the render tree, but it should prove the patches that led
to the tree. A final text snapshot alone is not enough.

### Layer 5: TodoMVC Speed And Resource Gate

This specializes the shared speed/resource gate from
[EXAMPLE_VERIFICATION_PLAN.md](EXAMPLE_VERIFICATION_PLAN.md).

Command shape:

```bash
cargo xtask verify-todomvc-speed \
  examples/todomvc.bn \
  examples/todomvc.scn \
  --budget examples/todomvc.budget.toml \
  --report target/reports/todomvc-speed.json
cargo bench -p boon_runtime --bench todomvc
```

The `cargo bench` path writes `target/reports/todomvc-bench.json` and a linked
`target/reports/todomvc-bench-speed.json`. Both must be schema-valid; the
readiness audit treats missing benchmark evidence as a blocker.

Default TodoMVC budgets:

```toml
[latency_ms]
single_row_toggle_input_to_idle_p95 = 3.0
add_todo_input_to_idle_p95 = 3.0
edit_commit_input_to_idle_p95 = 3.0
filter_change_input_to_idle_p95 = 3.0
clear_completed_normal_input_to_idle_p95 = 4.0
max_single_step = 8.0

[memory]
normal_steady_rss_delta_mib = 64
normal_peak_rss_delta_mib = 96
large_list_steady_rss_delta_mib = 128
large_list_peak_rss_delta_mib = 192
steady_vram_delta_mib = 64
peak_vram_delta_mib = 96

[allocations]
bounded_profile_allocs_after_warmup = 0
graph_rebuilds_per_interaction = 0
```

The stress profile should include 1,000 and 10,000 todos. Single-row actions
must stay proportional to the changed row plus declared aggregates/views. Bulk
actions such as `ClearCompleted` must either finish within the normal budget for
normal list sizes or be modeled as explicit bounded multi-tick work for stress
profiles.

## Scenario File Shape

Use a single TOML scenario file:

```toml
name = "todomvc"
source = "examples/todomvc.bn"

[[step]]
id = "initial"
assert.state.active_count = 2
assert.visible_text = ["Buy groceries", "Clean room", "2 items left"]
assert.no_data_field = "id"

[[step]]
id = "add-test-todo"
user_action = { kind = "type_text", target = "new todo input", text = "Test todo" }
expected_source_event = { source = "store.sources.new_todo_input.change", payload = { text = "Test todo" } }

[[step]]
id = "submit-test-todo"
user_action = { kind = "key_down", target = "new todo input", key = "Enter" }
expected_source_event = { source = "store.sources.new_todo_input.key_down", payload = { key = "Enter", text = "Test todo" } }
assert.state.active_count = 3
assert.rows = [
  { title = "Buy groceries", completed = false },
  { title = "Clean room", completed = false },
  { title = "Test todo", completed = false },
]
assert.semantic_delta_contains = ["ListInsert", "FieldSet:title"]
assert.render_delta_contains = ["InsertElement", "BindSource"]
```

The exact field names can evolve with the harness, but the first implementation
should use one TOML format for all examples. Headed/manual layers execute
`user_action` through the visible OS window and then assert that the expected
source event was produced. Semantic-only layers may inject
`expected_source_event` directly. Scenario actions never target hidden runtime
ids.

Renderer-only actions such as hover can be included, but they must be marked as
user actions that do not necessarily produce a source event:

```toml
[[step]]
id = "hover-delete"
user_action = { kind = "pointer_hover", target_text = "Buy milk EDITED" }
assert.render_text_contains = ["x"]
```

## Required Scenarios

### Initial State

Required assertions:

- header/footer render.
- main input is focused and typeable.
- all filter is selected.
- two default todos are visible.
- two active items are counted.
- no todo data contains an `id` field.

### Add Todo

Required actions:

- type `Test todo`.
- press Enter.
- assert the row appears.
- assert active count increments.
- assert main input clears.
- assert adding empty or whitespace-only text does not append.

Required delta checks:

- append emits one list insert.
- row-local source bindings are created.
- no whole list replacement is emitted.

### Filtering

Required actions:

- mark `Buy groceries` completed.
- switch to Active.
- switch to Completed.
- switch back to All.

Required assertions:

- active view hides completed rows.
- completed view hides active rows.
- counts do not change just because a filter changes.
- filter changes emit view/render membership deltas, not semantic list rewrites.

### Per-Row Toggle Isolation

Required actions:

- add at least one dynamic todo.
- toggle only that dynamic todo.
- toggle it back.

Required assertions:

- only that row's `completed` data changes.
- active count updates.
- unrelated row titles/completed flags are unchanged.
- semantic delta is one field change plus derived aggregate/render deltas.

### Toggle All

Required actions:

- toggle all active rows completed.
- toggle all back to active.
- toggle all with a partial completed set.
- toggle all while a filter hides some rows.

Required assertions:

- hidden rows are still affected by toggle all.
- new rows added after double toggle start unchecked.
- old skipped/commented toggle bug scenarios are active here, not skipped.

### Clear Completed

Required actions:

- clear when some rows are completed.
- clear when all rows are completed.
- clear when no rows are completed.
- clear all, then add a new row and toggle it.

Required assertions:

- only completed rows are removed.
- active rows remain.
- removed row sources are unbound.
- stale events targeting removed rows are ignored.
- newly added rows get fresh hidden runtime slots/generations but no Boon-visible
  identity.

### Edit Lifecycle

Required actions:

- double-click a row.
- assert edit input appears, stays visible, and is typeable.
- type text.
- Escape cancels and preserves original title.
- double-click again.
- type text and Enter saves.
- blur behavior follows the source semantics.

Required assertions:

- edit mode does not flash.
- edit draft is row-local.
- Enter uses the latest text payload.
- a stale input/change event after Enter cannot revert the saved title.

### Delete Button

Required actions:

- hover a row.
- assert delete button appears.
- move pointer from text to delete button without losing the button.
- click delete.

Required assertions:

- only that row is removed.
- active count updates.
- source bindings are removed.
- no hidden runtime key appears in Boon state.

### Empty State And Footer

Required assertions:

- empty list keeps header and main input.
- footer/count/filter area hides or shows according to TodoMVC rules.
- footer text renders as text, never as `[Element]`.

### Restart/Persistence

This is split into two checks:

1. Runtime snapshot/restart:
   - serialize runtime state after scenario setup.
   - create a fresh runtime.
   - restore snapshot.
   - assert semantic state and render deltas.

2. Playground persistence:
   - only after the playground implements persistence.
   - full process/page restart, not just rerun in memory.
   - assert restored state from persisted data.

Do not count persistence as passed before the storage layer exists.

### Large List And Performance

Required deterministic stress cases:

- seed 1,000 todos.
- seed target-profile maximum, such as 10,000 todos.
- toggle one row.
- edit one row.
- filter active/completed.
- clear completed with mixed data.

Required assertions:

- graph node count is unchanged by row count.
- toggling one row dirties one row key plus declared aggregates/views.
- render deltas are proportional to changed rows.
- no full list snapshot is emitted for single-row changes.
- allocation counters match the chosen profile.

### Hidden Identity And Data Equality

Required assertions:

- Boon source cannot read runtime keys, slots, generations, or source ids.
- equality compares data only.
- two equal todo records compare equal even if backed by different hidden slots.
- deleting and re-adding an equal record does not expose identity.
- stale source events are rejected below the language boundary.

## Report Contract

Every automated report should include:

```text
command
report_version
generated_at_utc
command_argv
exit_status
git_commit
binary_hash
source_path
source_hash
scenario_path
scenario_hash
budget_hash
runtime_profile
renderer
window_mode
window_backend
display_server
display_scale
window_size
framebuffer_size
program_hash
total_ticks
total_source_events
total_semantic_deltas
total_render_deltas
graph_node_count
max_dirty_nodes
max_dirty_keys
allocations
latency_ms_p50_p95_p99_max
rss_delta_mib_steady_peak
vram_delta_mib_steady_peak_or_unavailable_reason
artifact_sha256s
per-step pass/fail
failure artifacts
```

Headed and manual reports must additionally include:

```text
input_injection_method
window_pid
window_title
display_socket_or_compositor_connection
input_backend
capture_backend
focused_window_proof
focus_trace
checkpoint_screenshot_or_video_paths
visual_checkpoint_pass_fail
manual_observer
manual_notes
```

Failure artifacts:

```text
semantic state before/after failing step
source event payload
semantic deltas from failing tick
render deltas from failing tick
render tree snapshot when renderer is involved
screenshot when UI is involved
window/display metadata when UI is involved
```

## Commands To Exist

The final repo should expose:

```bash
cargo xtask verify-todomvc-headed-ply
cargo xtask verify-todomvc-human
cargo xtask verify-todomvc-semantic
cargo xtask verify-todomvc-ply-headless
cargo xtask verify-todomvc-speed
cargo xtask verify-todomvc-negative
cargo xtask verify-todomvc-all
cargo xtask bench-todomvc
cargo xtask explain-todomvc-hardware
```

`verify-todomvc-all` should fail if `verify-todomvc-headed-ply`,
`verify-todomvc-human`, `verify-todomvc-semantic`,
`verify-todomvc-ply-headless`, `verify-todomvc-speed`, or
`verify-todomvc-negative` is missing or failing. It regenerates required reports
by default and may check existing reports only with an explicit
`--check-existing` mode. Missing implementation is a blocker, not a skipped
pass.

Negative verification must prove the harness fails on bad source hashes, bad
scenario hashes, stale manual reports, missing screenshots/video, direct source
event injection in headed replay, hidden runtime id exposure, and app-visible
identity-based row routing.
It must also keep fixtures for fake full-OS-input reports, headed-only or
invalid manual artifacts, broken headed/manual report bindings, future manual
timestamps, failed or debug speed reports, missing speed stress/resource fields,
hand-written human reports without helper provenance, and manual report helpers
that omit or invent scenario pass labels. Readiness auditing treats those
fixture ids as required contract evidence.

## Manual Checklist

Manual test runs should follow these labels:

1. initial render and focus.
2. add valid todo.
3. reject empty todo.
4. filter active/completed/all.
5. toggle static row.
6. toggle dynamic row.
7. toggle all twice.
8. toggle all under filtered view.
9. clear some completed rows.
10. clear all rows.
11. add after clear.
12. edit cancel.
13. edit save.
14. hover delete.
15. empty state.
16. restart/persistence when implemented.
17. large-list quick smoke when implemented.

For every manual run, the tester should keep the delta/state inspector open and
verify at least one representative source event, semantic delta, and render
patch after add, toggle, clear, edit, and delete.

The first manual checkpoint is visual, before any scripted action:

- window starts at the expected size and scale.
- rendered text is sharp.
- no first-frame blur remains after the app becomes idle.
- no labels, buttons, checkboxes, inputs, or footer text overlap.
- the focused input has a visible caret/focus state.
- pointer hover and click targets line up with visible pixels.

## What Not To Bring From The Old Tests

Do not bring these forward as-is:

- fixed `sleep`/`wait` timing as correctness.
- commented-out bug tests.
- expected-fail milestones that still exit successfully.
- `preview text contains` as the only assertion.
- headless renderer/browser results as the only UI proof.
- screenshot-only approval without semantic and render-delta evidence.
- app-specific Rust preview logic as the implementation under test.
- identity-like todo ids in Boon source.

Useful pieces to preserve:

- the broad scenario coverage from `todo_mvc.expected`.
- edit-save acceptance trace.
- focus/typeability checks.
- dynamic checkbox isolation.
- clear completed after clear/re-add/re-check.
- full-refresh persistence idea, once persistence exists.
- visual/manual inspection as a required acceptance layer.
