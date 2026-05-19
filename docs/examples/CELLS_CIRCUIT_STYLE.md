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
