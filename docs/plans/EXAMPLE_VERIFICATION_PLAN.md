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

Use one TOML-compatible scenario format for all examples in the first
implementation. The scenario file describes source events, human-visible target
labels, semantic assertions, render-delta assertions, visual checkpoints, and
speed/resource workloads.

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
cargo xtask verify-example-negative <name>
cargo xtask verify-example-all <name>
cargo xtask verify-examples-all
cargo xtask bench-example <name>
cargo xtask verify-report-schema
cargo xtask audit-goal-readiness
```

Example-specific aliases such as `verify-todomvc-headed-ply` or
`verify-cells-speed` may exist, but they should call the generic harness.
`verify-example-all <name>` regenerates all required reports by default. It may
check existing reports only in an explicit `--check-existing` mode. It fails if
any required layer is missing:

```text
headed Ply replay
manual human report/check
semantic trace
headless renderer smoke
speed/resource gate
negative harness verification
report schema validation
```

`audit-goal-readiness` is stricter than schema validation. It scans the current
reports and fails if the repo is not actually ready for final handoff. A report
that is shape-valid but still says the runtime is adapter-backed, a headed replay
that still carries `os_input_limitation`, or a missing fresh human report remains
a blocker.

## Human-Like Input

The headed replay and manual pass must use real interaction shapes:

- pointer movement, hover, click, and double-click.
- keyboard typing, Enter, Escape, Tab, and blur/focus transitions.
- real row/cell hit targets, not hidden runtime ids.
- visible window state, not only internal render-tree assertions.
- visual checkpoints before, during, and after interactions.

Scenario files must distinguish:

```text
user_action              what the headed/manual harness does through OS input
expected_source_event    what the runtime should receive after hit testing
semantic_assertion       expected Boon state/delta after the tick
render_assertion         expected render patch/pixel consequence
```

Current scenario files use explicit `expect_semantic_delta_contains` and
`expect_render_delta_contains` fields for key interactions. The runner fails if
an expected semantic delta or render patch kind is absent from that step's trace,
so state-only success cannot hide a missing keyed delta, source binding, or
renderer patch.

Headed replay fails if it injects `expected_source_event` directly instead of
creating it through the visible window and input backend.

Headed replay must also fail cleanly. The xtask wrapper owns a single headed
Ply lock, removes stale locks whose recorded process id is no longer alive, and
times out the native verifier child instead of leaving a spinning window or
blocked aggregate run. A timed-out headed verifier is a blocker, not a partial
pass; it may leave a debug-only failure report under `target/reports/debug/`,
but it must not create a passing top-level headed report. Stale headed JSON
reports should be removed or regenerated rather than kept as schema-valid
evidence.

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
The xtask speed aliases must therefore re-exec into a release xtask binary and
reject speed reports whose `build_profile` is not `release`.

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
per_operation_outliers
```

Measure speed in two layers:

- runtime microbench: parser already warmed, no window, semantic tick and render
  lowering only.
- headed presentation: real window, real input route, real Ply patch application,
  frame presentation, and visual artifacts.

The couple-of-milliseconds budget applies to normal runtime interaction work.
Headed reports must still be fast and must not miss frames, but they include OS,
window manager, and presentation metadata so regressions are interpreted against
the measured machine/profile.

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
apply_heap_alloc_count
apply_heap_alloc_bytes
expectation_heap_alloc_count
expectation_heap_alloc_bytes
graph_node_count
graph_rebuild_count
list_slot_count
dirty_node_count_p50_p95_p99_max
dirty_key_count_p50_p95_p99_max
render_patch_count_p50_p95_p99_max
```

Every report, including manual reports, must include:

```text
report_version
generated_at_utc
command_argv
exit_status
git_commit
binary_hash
source_hash
scenario_hash
budget_hash
artifact_sha256s
```

Executable example reports for semantic, headless Ply, headed Ply, and speed
layers must also include `runtime_execution` metadata:

```text
implementation
source_loaded_from_boon
typed_ir_loaded
static_schedule_verified
generic_interpreter_complete
example_behavior_adapter
```

This is an honesty field, not decorative metadata. During the prototype it may
say that the run is still adapter-backed, but that must remain visible in the
report until the generic static-graph interpreter executes the source equations
directly.

If VRAM cannot be read on a platform, the report must say so explicitly and the
headed/manual visual pass still remains required.

Manual reports must additionally include `manual_observer`, display/backend
metadata, checklist pass/fail per label, screenshot/video artifact hashes, and a
freshness limit. The checker form should reject stale reports:
The listed checkpoint paths must be screenshot/video artifacts and must also be
present in `artifact_sha256s`; hashing an unrelated file is not accepted as a
manual visual pass.

```bash
cargo xtask verify-example-human <name> --check --max-age 24h
```

Headed replay reports must include `window_pid`, `window_title`, display socket
or compositor connection, input backend, capture backend, focused-window proof,
nonblank screenshot hashes, and per-step pointer/keyboard routes.
For a report to claim `input_injection_method:
os_pointer_keyboard_to_visible_window`, it must also include `os_input_steps`
covering every scenario label in order. Each step must record a visible target
element id, nonzero bounds from the Ply layout, the OS action route, the
observed source event for source-producing actions, and a screenshot/video
checkpoint path that is present in `artifact_sha256s`.
If a headed verifier proves only part of the OS-input path, it must include an
`os_input_limitation` field and still fail negative fixtures that omit that
limitation. A hybrid report is useful evidence, but it is not the final
per-step OS hit-testing contract.

Headed checks must run serially on a developer desktop. They open real windows
and send real OS input to the focused Ply surface, so parallel headed runs can
steal focus from each other and must be treated as invalid, not flaky success.

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
9. negative verification fixtures for bad hashes, stale reports, missing visual
   artifacts, direct source injection, and hidden identity exposure.
10. report artifacts checked by `verify-example-all <name>`.

The harness should make the easiest path the honest path: run the real source,
show the real UI, measure real latency, and store enough evidence to reproduce a
failure.
