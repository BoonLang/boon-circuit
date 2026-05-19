# Example Verification Plan

This is the shared acceptance contract for TodoMVC, Cells, and every future
Boon example. Example-specific plans may add scenarios, but they must not weaken
this contract.

## Required Files Per Example

Each accepted example should have:

```text
examples/<name>.bn
examples/<name>.scn
examples/<name>.budget.toml
target/reports/<name>-*.json
target/reports/<name>-checkpoint-*.png
```

The Boon source is the implementation under test. Rust code may provide generic
runtime primitives, renderer glue, and domain primitives such as formula parsing,
but it must not hardcode the example's app behavior.

## Acceptance Layers

Every interactive example must pass the same layers:

```text
headed Ply replay      primary e2e gate
manual human pass      real eyes/hands confirmation
semantic trace         deterministic runtime/debug gate
headless renderer      fast CI smoke only
speed/resource gate    release-mode latency and memory proof
```

Headless renderer and semantic tests are useful, but they are not enough to
accept an interactive example. A real headed native Ply window must be opened,
visible, sharp, correctly scaled, and driven through the same input route a user
uses.

## Generic Commands

The repo should expose generic commands:

```bash
cargo xtask verify-example-headed-ply <name>
cargo xtask verify-example-human <name>
cargo xtask verify-example-semantic <name>
cargo xtask verify-example-ply-headless <name>
cargo xtask verify-example-speed <name>
cargo xtask verify-example-all <name>
cargo xtask verify-examples-all
cargo xtask bench-example <name>
```

Example-specific aliases such as `verify-todomvc-headed-ply` or
`verify-cells-speed` may exist, but they should call the generic harness.

## Human-Like Input

The headed replay and manual pass must use real interaction shapes:

- pointer movement, hover, click, and double-click.
- keyboard typing, Enter, Escape, Tab, and blur/focus transitions.
- real row/cell hit targets, not hidden runtime ids.
- visible window state, not only internal render-tree assertions.
- visual checkpoints before, during, and after interactions.

The first checkpoint for every example is startup quality:

- correct window size.
- correct display scale.
- sharp text and graphics.
- no blurred first frame after idle.
- no clipped or overlapping controls.
- focused control is visibly focused.
- pointer hit targets match visible pixels.

## Speed Gate

Interactive latency is part of correctness. A slow example has failed even if
its final state is right.

Default release-mode budget:

```toml
[latency_ms]
semantic_tick_p95 = 1.0
render_lowering_p95 = 0.5
ply_patch_apply_p95 = 1.0
input_to_idle_p95 = 3.0
input_to_idle_p99 = 6.0
max_single_step = 8.0

[frame]
missed_frames_allowed = 0
presentation_budget_ms = 16.7
```

The important target is that normal interactions complete in a couple of
milliseconds. `input_to_idle_p95` is the main pass/fail value. Debug builds may
emit performance reports, but release builds are the authoritative speed gate.

Every speed report must include:

```text
build_profile
git_commit
source_hash
scenario_hash
cpu_model
gpu_model_if_available
os
display_server
window_backend
display_scale
semantic_tick_ms_p50_p95_p99_max
render_lowering_ms_p50_p95_p99_max
ply_patch_apply_ms_p50_p95_p99_max
input_to_idle_ms_p50_p95_p99_max
frame_time_ms_p50_p95_p99_max
missed_frame_count
operation_count
per-operation outliers
```

The harness should run:

1. compile/load warm-up.
2. renderer warm-up until the first stable idle frame.
3. fixed scenario replay.
4. repeated interaction loop, usually 100 to 1000 iterations.
5. stress scenario for the example's largest supported profile.

Fixed sleeps do not prove speed. The harness waits for deterministic idle, then
measures the real time from input injection to idle state.

## Resource Gate

The budget is measured as an example delta over an empty playground baseline.

Default release-mode budget:

```toml
[memory]
steady_rss_delta_mib = 64
peak_rss_delta_mib = 96
steady_vram_delta_mib = 64
peak_vram_delta_mib = 96

[allocations]
bounded_profile_allocs_after_warmup = 0
dynamic_profile_allocs_per_interaction = 4
graph_rebuilds_per_interaction = 0
```

Large examples may set stricter or larger explicit budgets in
`examples/<name>.budget.toml`, but they must justify the number. A missing budget
is a blocker.

Every resource report must include:

```text
baseline_rss_mib
steady_rss_mib
peak_rss_mib
baseline_vram_mib_if_available
steady_vram_mib_if_available
peak_vram_mib_if_available
heap_alloc_count_per_step
heap_alloc_bytes_per_step
graph_node_count
graph_rebuild_count
list_slot_count
dirty_node_count_p50_p95_p99_max
dirty_key_count_p50_p95_p99_max
render_patch_count_p50_p95_p99_max
```

If VRAM cannot be read on a platform, the report must say so explicitly and the
headed/manual visual pass still remains required.

## Bulk Operations

Bulk operations are allowed, but they need an explicit latency policy.

For normal example sizes, bulk operations should still satisfy the interactive
budget. For large stress profiles, either:

- the operation completes inside the same budget, or
- the Boon program models it as explicit multi-tick work with visible progress
  and bounded per-tick latency.

Silent long blocking work is not acceptable. A `ClearCompleted`, large formula
fanout, or mass update cannot freeze the playground and then report success.

## Future Example Checklist

Before a future example can be considered real, add:

1. source file and scenario file.
2. headed replay coverage for the primary workflow.
3. manual checklist using the same labels as the scenario.
4. semantic assertions after every input.
5. render delta assertions for visible changes.
6. speed budget file.
7. RAM/VRAM budget file.
8. stress scenario sized to the example's declared profile.
9. report artifacts checked by `verify-example-all <name>`.

The harness should make the easiest path the honest path: run the real source,
show the real UI, measure real latency, and store enough evidence to reproduce a
failure.
