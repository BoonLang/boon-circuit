# Cells Circuit-Style Target

The 7GUIs Cells example is the second proof target after TodoMVC. It stresses
dependency tracking, nested updates, and invalidation without needing a runtime
actor per cell.

## Goals

- Formulas live in Boon source, not hardcoded Rust.
- The engine handles arbitrary supported formulas from the editor.
- Dependencies between cells are explicit runtime data.
- Only affected cells recompute after an edit.
- Cycles are detected and reported deterministically.

## Table Shape

```text
CellTable keyed by CellId:
  exists
  formula_text
  parsed_formula
  value
  error
  editing
```

Dependency relation:

```text
CellDependency:
  from_cell
  to_cell
```

`from_cell -> to_cell` means `to_cell` reads `from_cell`.

## Boon Shape

Illustrative target:

```boon
cells:
    Grid/cells(columns: 26, rows: 100)
    |> List/map(new_cell)

FUNCTION new_cell(seed) {
    sources: [
        editor: [
            change: SOURCE
            commit: SOURCE
            cancel: SOURCE
        ]
    ]

    [
        id: seed.id

        formula_text:
            "" |> HOLD text {
                LATEST {
                    sources.editor.commit.text |> THEN {
                        text
                    }
                }
            }

        parsed_formula:
            Formula/parse(formula_text)

        dependencies:
            Formula/dependencies(parsed_formula)

        value:
            Formula/eval(
                formula: parsed_formula,
                read: cell_value_reader
            )

        error:
            Formula/error(parsed_formula, value)
    ]
}
```

The real syntax may differ. The important boundary is that parsing/evaluation
are generic primitives, while cell behavior is expressed in Boon.

## Propagation

When `formula_text[A1]` changes:

1. recompute `parsed_formula[A1]`.
2. recompute dependency edges for `A1`.
3. mark reverse dependents dirty.
4. topologically recompute affected `value[*]`.
5. emit `FieldSet` deltas for changed displayed values/errors.

The runtime may use an adjacency index:

```text
dependents[from_cell] -> Set<CellId>
dependencies[to_cell] -> Set<CellId>
```

This is not the same as Differential Dataflow. It is a purpose-built dependency
index owned by the static runtime.

## Cycle Handling

If a dependency cycle appears:

```text
A1 -> B1 -> C1 -> A1
```

the engine should:

- mark every cell in the cycle with a deterministic cycle error.
- avoid infinite reevaluation.
- keep previous committed values available where needed for debugging.

## Renderer Delta Example

```text
FieldSet(/cells:A1, formula_text, "=B1+1")
FieldSet(/cells:A1, value, 42)
FieldSet(/cells:C4, value, 100)
FieldSet(/cells:C4, error, null)
```

The grid renderer updates only affected cells. It does not diff the whole grid.

## Verification Contract

Cells must follow the shared contract in
[../plans/EXAMPLE_VERIFICATION_PLAN.md](../plans/EXAMPLE_VERIFICATION_PLAN.md).
It is not accepted by semantic tests alone.

Required commands:

```bash
cargo xtask verify-cells-headed-ply
cargo xtask verify-cells-human
cargo xtask verify-cells-semantic
cargo xtask verify-cells-ply-headless
cargo xtask verify-cells-speed
cargo xtask verify-cells-all
```

The headed Ply and manual passes must test real spreadsheet interaction:

- click a cell.
- type a literal.
- type a formula.
- commit with Enter.
- cancel with Escape.
- move focus with pointer and keyboard.
- scroll the grid if the visible viewport is smaller than the declared grid.
- inspect that only affected visible cells update.

Required semantic scenarios:

- literal edit.
- formula edit.
- formula references another cell.
- dependency chain recomputes in deterministic topological order.
- fanout recomputes all dependents and no unrelated cells.
- cycle reports deterministic errors and does not loop forever.
- deleting or replacing a formula removes stale dependency edges.
- editing one unrelated cell does not recompute or redraw the whole grid.

## Cells Speed And Resource Gate

Cells is only a useful proof if it feels immediate while dependency tracking is
real. Normal interactions should complete in a couple of milliseconds in release
mode.

Default Cells budgets:

```toml
[latency_ms]
literal_edit_input_to_idle_p95 = 3.0
formula_edit_input_to_idle_p95 = 3.0
single_dependency_update_p95 = 3.0
fanout_100_update_p95 = 4.0
cycle_detection_p95 = 4.0
max_single_step = 8.0

[memory]
grid_26x100_steady_rss_delta_mib = 64
grid_26x100_peak_rss_delta_mib = 96
grid_stress_steady_rss_delta_mib = 128
grid_stress_peak_rss_delta_mib = 192
steady_vram_delta_mib = 64
peak_vram_delta_mib = 96

[allocations]
bounded_profile_allocs_after_warmup = 0
graph_rebuilds_per_interaction = 0
```

The speed report must include:

```text
edited_cell
formula_text_length
dependency_edge_count
dirty_cell_count
recomputed_cell_count
visible_cell_patch_count
semantic_tick_ms_p50_p95_p99_max
render_lowering_ms_p50_p95_p99_max
ply_patch_apply_ms_p50_p95_p99_max
input_to_idle_ms_p50_p95_p99_max
rss_delta_mib_steady_peak
vram_delta_mib_steady_peak_or_unavailable_reason
heap_alloc_count_per_step
graph_rebuild_count
```

Failure rules:

- if a single unrelated edit recomputes the whole grid, the test fails.
- if a hidden dependency edge survives formula replacement, the test fails.
- if a headed cell edit is visually blurred, clipped, unfocused, or delayed, the
  test fails even if semantic values are correct.
- if a large fanout cannot finish within the budget, it must be explicit
  multi-tick work with bounded per-tick latency and visible progress.
