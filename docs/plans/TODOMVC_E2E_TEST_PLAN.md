# TodoMVC E2E Test Plan

This plan is derived from the existing TodoMVC test material in
`~/repos/boon`, but it tightens the contract for Boon Circuit.

The old repo has useful coverage in:

- `playground/frontend/src/examples/todo_mvc/todo_mvc.expected`
- `crates/boon-cli/tests/todo_mvc_logic.bn.test`
- `crates/boon-cli/tests/todo_mvc_complete.bn.test`
- `crates/boon-engine-actors-lite/src/todo_acceptance.rs`
- `tools/scripts/test_todo_mvc_wasm.sh`

The new repo should not copy the old tests mechanically. The old browser tests
contain sleeps, commented-out bug tests, engine-specific skips, and DOM text
checks that can hide semantic failures. Boon Circuit needs deterministic traces
first, then renderer/browser/manual proof as separate layers.

## What Counts As Honest

A TodoMVC e2e pass requires all of these:

1. The tested program is `examples/todomvc.bn`, not Rust hardcoded TodoMVC.
2. The source has no app-visible row `id`, no `next_todo_id`, and no identity or
   reference comparison.
3. One scenario file drives all test surfaces.
4. Semantic state is asserted after every action.
5. Semantic deltas and render deltas are asserted for key actions.
6. The renderer is checked without diffing the whole UI tree.
7. Manual testing uses the same scenario labels and records evidence.
8. No skipped/commented bug scenario is counted as covered.
9. No fixed sleeps are used as correctness. Tests wait for deterministic idle or
   fail after a bounded tick/frame limit.
10. Reports include source hash, scenario hash, runtime stats, and failure
    artifacts.

## Test Layers

### Layer 1: Semantic Trace Runner

This is the primary e2e gate.

Command shape:

```bash
cargo run -p boon_cli -- scenario \
  examples/todomvc.bn \
  examples/todomvc.scn \
  --report target/reports/todomvc-semantic.json
```

It runs the real parser, typed IR, static graph runtime, source event injection,
`HOLD` commits, `LIST` memories, and semantic delta emission.

It does not start a renderer.

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

### Layer 2: Headless Ply Render E2E

This verifies lowering from semantic deltas to deterministic render patches.

Command shape:

```bash
cargo run -p boon_cli -- scenario \
  examples/todomvc.bn \
  examples/todomvc.scn \
  --renderer ply-headless \
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

### Layer 3: Native Playground Replay

This verifies the actual native playground surface.

Command shape:

```bash
cargo run -p boon_ply_playground -- \
  --example examples/todomvc.bn \
  --replay examples/todomvc.scn \
  --report target/reports/todomvc-native-replay.json
```

This should open the same app that a user can interact with, replay the scenario,
and write evidence:

```text
semantic trace
render patch trace
final render tree
screenshots for selected checkpoints
input/focus evidence
timing/frame stats
```

### Layer 4: Manual Test Pass

Manual testing is not a substitute for Layer 1-3. It is a final human pass over
the live playground.

Command shape:

```bash
cargo run -p boon_ply_playground -- --example examples/todomvc.bn
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
```

Manual pass evidence is written to:

```text
target/reports/todomvc-manual-<timestamp>.json
target/reports/todomvc-manual-<timestamp>-final.png
```

The manual report records each checked scenario label, the final state hash, and
whether the user deviated from the scripted scenario.

## Scenario File Shape

Use a single scenario file, probably TOML or RON:

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
event = { source = "store.sources.new_todo_input.change", payload = { text = "Test todo" } }

[[step]]
id = "submit-test-todo"
event = { source = "store.sources.new_todo_input.key_down", payload = { key = "Enter", text = "Test todo" } }
assert.state.active_count = 3
assert.rows = [
  { title = "Buy groceries", completed = false },
  { title = "Clean room", completed = false },
  { title = "Test todo", completed = false },
]
assert.semantic_delta_contains = ["ListInsert", "FieldSet:title"]
assert.render_delta_contains = ["InsertElement", "BindSource"]
```

The exact syntax can change. The important rule is that scenario actions target
source paths and data payloads, not hidden runtime ids.

Renderer-only actions such as hover can be included, but they must be marked as
renderer actions:

```toml
[[step]]
id = "hover-delete"
renderer_event = { target_text = "Buy milk EDITED", event = "hover" }
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
git_commit
source_path
source_hash
scenario_path
scenario_hash
runtime_profile
renderer
program_hash
total_ticks
total_source_events
total_semantic_deltas
total_render_deltas
graph_node_count
max_dirty_nodes
max_dirty_keys
allocations
per-step pass/fail
failure artifacts
```

Failure artifacts:

```text
semantic state before/after failing step
source event payload
semantic deltas from failing tick
render deltas from failing tick
render tree snapshot when renderer is involved
screenshot when UI is involved
```

## Commands To Exist

The final repo should expose:

```bash
cargo xtask verify-todomvc-semantic
cargo xtask verify-todomvc-ply-headless
cargo xtask verify-todomvc-native-replay
cargo xtask verify-todomvc-manual-report
cargo xtask verify-todomvc-all
cargo xtask bench-todomvc
cargo xtask explain-todomvc-hardware
```

`verify-todomvc-all` should fail if any required layer is missing. Missing
implementation is a blocker, not a skipped pass.

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

## What Not To Bring From The Old Tests

Do not bring these forward as-is:

- fixed `sleep`/`wait` timing as correctness.
- commented-out bug tests.
- expected-fail milestones that still exit successfully.
- `preview text contains` as the only assertion.
- app-specific Rust preview logic as the implementation under test.
- identity-like todo ids in Boon source.

Useful pieces to preserve:

- the broad scenario coverage from `todo_mvc.expected`.
- edit-save acceptance trace.
- focus/typeability checks.
- dynamic checkbox isolation.
- clear completed after clear/re-add/re-check.
- full-refresh persistence idea, once persistence exists.
- visual/manual inspection as a separate evidence layer.
