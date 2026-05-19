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
CellTable indexed by hidden runtime key:
  address
  exists
  formula_text
  editing_text
  parsed_formula
  value
  error
  editing
```

Dependency relation:

```text
CellDependency:
  from_address
  to_address
```

`from_address -> to_address` means `to_address` reads `from_address`.

`address` is ordinary spreadsheet data such as `A1`, not hidden runtime
identity. It can be displayed and compared as data. Runtime keys, slots, and
source generations remain below the Boon language boundary.

## Boon Shape

Illustrative target:

```boon
cells:
    Grid/cells(columns: 26, rows: 100)
    |> List/map(seed, new: new_cell(seed: seed))

FUNCTION new_cell(seed) {
    sources: [
        editor: [
            change: SOURCE
            commit: SOURCE
            cancel: SOURCE
        ]
    ]

    [
        address: seed.address

        editing_text:
            TEXT {} |> HOLD draft {
                LATEST {
                    sources.editor.change.text
                    sources.editor.cancel |> THEN { formula_text }
                    sources.editor.commit.text
                }
            }

        formula_text:
            TEXT {} |> HOLD formula_text {
                LATEST {
                    sources.editor.commit.text
                }
            }

        editing:
            False |> HOLD editing {
                LATEST {
                    sources.editor.change |> THEN { True }
                    sources.editor.commit |> THEN { False }
                    sources.editor.cancel |> THEN { False }
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

The first implementation should keep this syntax close to the original Boon
style. The important boundary is that parsing/evaluation are generic primitives,
while edit state, commit/cancel behavior, and cell display state are expressed in
Boon.

Runtime validation derives cell source ports from this Boon row template. The
checked example uses `change`, `commit`, and `cancel`, but hidden source routing
should follow parsed `SOURCE` ports rather than a separate Rust list of editor
event names.

`Grid/cells` produces domain coordinates and display addresses, for example:

```text
[row: 1, column: 1, address: TEXT { A1 }]
```

Those coordinates are ordinary data. They are not the hidden list key.

## Formula Primitive Contract

`Formula/parse`, `Formula/dependencies`, and `Formula/eval` are generic
spreadsheet primitives, not a hardcoded Cells app.

Minimum supported formula contract:

```text
literals: numbers and text
references: A1 style cell addresses
operators: + - * / for numeric values
functions: SUM(range) can be added after the base proof
errors: parse_error, cycle_error, missing_ref, type_error, div_by_zero
```

Current implementation note: numeric literals, A1 references, single binary
`+`, `-`, `*`, and `/` expressions, dependency-edge replacement, `parse_error`,
`cycle_error`, and `div_by_zero` are covered by runtime tests. Range functions
such as `SUM` remain future work.

`Formula/parse(text)` returns a small formula AST or a deterministic error.
`Formula/dependencies(ast)` returns a set of domain cell addresses. `Formula/eval`
reads values by domain address through the runtime's dependency-aware reader and
records which addresses were read. The reader must not expose hidden runtime
keys.

## Propagation

When `formula_text[A1]` changes:

1. recompute `parsed_formula[A1]`.
2. recompute dependency edges for `A1`.
3. mark reverse dependents dirty.
4. topologically recompute affected `value[*]`.
5. emit `FieldSet` deltas for changed displayed values/errors.

The runtime uses an adjacency index:

```text
dependents[from_address] -> Set<CellAddress>
dependencies[to_address] -> Set<CellAddress>
```

This is not the same as Differential Dataflow. It is a purpose-built dependency
index owned by the static runtime.
Verification reports include `dependency_edge_walk_count` for Cells steps, so an
edit with small fanout can prove it walks reverse dependency edges instead of
scanning the whole grid.

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
cargo xtask verify-cells-negative
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
