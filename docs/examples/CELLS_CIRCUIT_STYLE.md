# Cells Circuit-Style Target

The 7GUIs Cells example is the second proof target after TodoMVC. It stresses
dependency tracking, nested updates, and invalidation without needing a runtime
actor per cell.

## Goals

- Cell expressions live in Boon source, not hardcoded Rust.
- The engine handles arbitrary supported cell expressions from the editor.
- Dependencies between cells are explicit runtime data.
- Only affected cells recompute after an edit.
- Cycles are detected and reported deterministically.
- The visible preview is generated from the Boon example's `document`, state,
  and generic state summary data. The native playground may add generic UI
  primitives such as `Row`, `Column`, `Text`, `Input`, focus styling, and
  scrolling, but it must not contain a dedicated Cells widget or hardcoded Cells
  preview.

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

`address` is ordinary spreadsheet data such as `A0`, not hidden runtime
identity. It can be displayed and compared as data. Runtime keys, slots, and
source generations remain below the Boon language boundary.

## Boon Shape

Illustrative target:

```boon
cells:
    List/range(from: 0, to: 2599)
    |> List/map(cell, new: new_cell(cell: cell))

FUNCTION new_cell(cell) {
    sources: [
        editor: [
            change: SOURCE
            commit: SOURCE
            cancel: SOURCE
        ]
    ]

    [
        address:
            cell_address(index: cell.index)

        default_formula:
            default_formula_for_address(address: address)

        editing_text:
            default_formula |> HOLD draft {
                LATEST {
                    sources.editor.change.text
                    sources.editor.cancel |> THEN { formula_text }
                    sources.editor.commit.text
                }
            }

        formula_text:
            default_formula |> HOLD formula_text {
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

        value:
            compute_value(address: address, formula_text: formula_text)

        error:
            Error/text(compute_value(address: address, formula_text: formula_text))
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

`List/range` produces ordinary indexed rows. The row template derives visible
addresses with `cell_address(index: cell.index)`, and seeded demo cell
expressions come from generic lookup helpers such as
`default_formula_for_address(address: address)`, for example:

```text
[row: 0, column: A, address: TEXT { A0 }, default_formula: TEXT { 5 }]
[row: 0, column: B, address: TEXT { B0 }, default_formula: TEXT { =add(A0,A1) }]
[row: 0, column: C, address: TEXT { C0 }, default_formula: TEXT { =sum(A0:A2) }]
```

Those coordinates are ordinary data. They are not the hidden list key.

## Visible Playground Shape

The native playground must present Cells as a spreadsheet-like bounded viewport
over the declared 26x100 model, not as a four-cell demo. The current visible
contract is:

```text
source model: 26 columns x 100 rows
visible projection: 26 columns x 100 rows
visible selectable cells: 2600
visible text editors: formula bar plus any explicit active-cell editor
required visible samples beyond A0-D0: Z0, A99, and Z99
```

The runtime state summary may expose bounded source-declared projections such as
`sheet_columns`, `store.sheet_rows`, and visible `cells` to feed the generic
`document` renderer. Those projections are UI data derived from the
authoritative list storage. They are not allowed to replace or shrink the
underlying 26x100 runtime model.

The Cells source owns the spreadsheet layout declaration. Header rows, row
labels, selectable display cells, displayed values, edit-mode cell expressions,
focused cell styling, and the scrollable body must be declared in the manifest-backed
`examples/cells/*.bn` source files with generic `document` elements. The root
`examples/cells.bn` is only the executable document entrypoint. The playground
renderer only interprets those generic component attributes.

## Cell Expression Helper Contract

`CellExpression/parse`, `CellExpression/dependencies`, and `CellExpression/eval` are generic
spreadsheet primitives, not a hardcoded Cells app.

Minimum supported formula contract:

```text
literals: numbers and text
references: A0 style cell addresses with 7GUIs rows 0 through 99
operators: + - * / for numeric values
functions: add(left,right), sum(vertical_range)
errors: parse_error, cycle_error, missing_ref, type_error, div_by_zero
```

Current implementation note: numeric literals, A0 references, single binary
`+`, `-`, `*`, and `/` expressions, `add(left,right)`, vertical `sum(A0:A2)`,
dependency-edge replacement, `parse_error`, `cycle_error`, and `div_by_zero` are
covered by runtime tests.

The user-facing formula state remains `TEXT`: `formula_text`, edit drafts,
semantic deltas, and visible editor values must not become `BYTES`. The current
source helper converts only the trimmed formula expression through
`Text/to_bytes(encoding: Ascii)` before grammar scanning. Operator, comma,
colon, and function-prefix searches use `Bytes/find` / `Bytes/starts_with`, and
the resulting byte offsets are reused for `Text/substring` only because the
strict ASCII boundary rejects non-ASCII formula syntax first. Ordinary literal
cell text still stays TEXT and must not be encoded merely to exercise BYTES.

`CellExpression/parse(text)` returns a small formula AST or a deterministic error.
`CellExpression/dependencies(ast)` returns a set of domain cell addresses. `CellExpression/eval`
reads values by domain address through the runtime's dependency-aware reader and
records which addresses were read. The reader must not expose hidden runtime
keys.

## Propagation

When `formula_text[A0]` changes:

1. recompute `parsed_formula[A0]`.
2. recompute dependency edges for `A0`.
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
A0 -> B0 -> C0 -> A0
```

the engine should:

- mark every cell in the cycle with a deterministic cycle error.
- avoid infinite reevaluation.
- keep previous committed values available where needed for debugging.

## Renderer Delta Example

```text
FieldSet(/cells:A0, formula_text, "=B0+1")
FieldSet(/cells:A0, value, 42)
FieldSet(/cells:C4, value, 100)
FieldSet(/cells:C4, error, null)
```

The grid renderer updates only affected cells. It does not diff the whole grid.

## Verification Contract

Cells must follow the native GPU contract in
[../architecture/NATIVE_GPU_PIPELINE.md](../architecture/NATIVE_GPU_PIPELINE.md).
It is not accepted by semantic tests alone.

Required commands:

```bash
cargo test -p boon_parser -p boon_ir -p boon_runtime --lib
cargo test -p boon_native_playground --bin boon_native_playground
cargo run -q -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/debug/cells-cli-run.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json
cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json
```

Native GPU and follow-up human passes must test real spreadsheet interaction:

- click a cell.
- type a literal.
- type a formula.
- commit with Enter.
- cancel with Escape.
- move focus with pointer and keyboard.
- scroll the grid if the visible viewport is smaller than the declared grid.
- inspect that only affected visible cells update.

Semantic, speed, and stress reports prove the runtime/dependency behavior. They
do not by themselves prove the visible playground shape. The visible shape is
accepted only when `target/reports/cells-visible-reality.json` proves the source
grid dimensions, viewport dimensions, rendered addressed editor count, required
address samples, and nonblank screenshot evidence.

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
