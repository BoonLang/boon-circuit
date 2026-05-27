# Pure Boon Cells Formula Plan

## Goal

Cells formulas must be ordinary Boon source, not a Rust-backed spreadsheet
primitive. The native runtime should execute the generic Boon constructs needed
by that source:

- `BLOCK`
- `WHILE`
- arbitrary user-defined pure functions
- nested/helper calls such as `compute_value -> cell_formula -> compute_value`
- generic derived dependency recompute

Rust may implement generic Boon execution machinery and generic stdlib
operators. Rust must not contain Cells/Formula business logic.

## Baseline

Current committed baseline: `6fb3db5 Refactor cells toward Boon source model`.

The current source still contains `Formula/parse`, `Formula/dependencies`,
`Formula/eval`, and `Formula/error` in `examples/cells/cell.bn`. The runtime
still contains formula-specific parser/evaluator/dependency code. That is the
main remaining architectural violation.

## Contract

Accepted after this plan:

- `rg -n "Formula/" crates examples -S` has no production hits.
- `rg -n "FormulaAst|FormulaTerm|FormulaOperatorPlan|AddressedFormulaRuntime|parse_formula_ast|formula_ast_dependencies|cell_index\\(" crates -S` has no production hits.
- Cells formula behavior lives in Boon files, preferably
  `examples/cells/formula.bn`.
- The preview receives combined Boon source only, never an example-specific
  formula/runtime shortcut.
- The Cells scenario still proves literals, add/sum, infix formulas,
  dependency replacement, cycle errors, cancel/commit/blur flow, and fanout
  recompute.

Allowed Rust:

- generic AST/function execution
- generic `Text/*`, `Number/*`, `Bool/*`, and `List/*` operators
- generic table/list construction
- generic dirty propagation and read-tracked derived recompute
- generic document/source routing

Forbidden Rust:

- formula ASTs or formula parsers
- `Formula/*` operator lowering
- spreadsheet-specific dependency extraction
- example-id/path/name branches for Cells
- hardcoded `A0`, `B0`, `formula_text`, `value`, or `error` behavior outside
  generic source/IR tests

## Source Shape

Port the useful shape from the original playground into the native Cells source,
but keep `SOURCE` routing instead of `LINK`:

```boon
FUNCTION cell_formula(target_address) {
    List/find(cells, field: address, value: target_address).formula_text
}

FUNCTION compute_cell(address, formula_text) {
    BLOCK {
        value: compute_value(formula_text: formula_text)
        [value: value, error: Text/empty()]
    }
}

FUNCTION compute_value(formula_text) {
    formula_text |> Text/starts_with(prefix: TEXT { = }) |> WHILE {
        False => formula_text |> Text/to_number() |> WHILE {
            NaN => 0
            number => number
        }
        __ => BLOCK {
            formula_length: formula_text |> Text/length()
            expression: formula_text |> Text/substring(start: 1, length: formula_length - 1)
            expression_value(text: expression)
        }
    }
}
```

The actual implementation must support the current scenario syntax:

- literal numbers: `41`
- direct references: `A0`
- binary infix: `A0+1`, `A0+2`, `8/2`
- functions: `add(A0,A1)`, `sum(A0:A2)`
- errors: `parse_error`, `cycle_error`, `missing_ref`, `type_error`,
  `div_by_zero`

## Implementation Milestones

### 1. Guardrails First

Add failing tests before deleting runtime Formula code:

- Cells source must not contain `Formula/`.
- Parser/IR must accept formula helper functions written in Boon.
- Runtime must fail a fixture that tries to rely on `Formula/*`.
- Genericity audit must reject production `FormulaAst`, `FormulaTerm`,
  `FormulaOperatorPlan`, and `AddressedFormulaRuntime*`.

Keep the existing Cells scenario unchanged until the pure Boon path passes it.

### 2. Generic Boon Value Evaluator

Add a runtime evaluator over parsed AST expressions. This is not a Formula
engine; it is a Boon expression engine.

Minimum value model:

- text
- number
- bool
- record
- list
- empty/skip
- `NaN`
- structured error text

Minimum expression support:

- identifiers and paths
- string/text/number/bool literals
- records and list literals
- infix `+`, `-`, `*`, `/`, `==`, `>=`, `<=`, `>`, `<`
- calls with named and positional arguments
- pipes
- `WHEN`
- `WHILE` as bounded branch selection, not an unbounded loop
- `BLOCK` locals with last expression as result

Function support:

- collect `FUNCTION name(args...)` bodies from the parsed project
- bind named arguments into a lexical environment
- allow nested helper calls
- use an evaluation stack and call budget to catch accidental infinite
  recursion
- memoize pure calls where all arguments are stable scalar values

### 3. Generic Stdlib Operators

Implement only generic operators needed by Cells source:

- `Text/empty`
- `Text/trim`
- `Text/starts_with`
- `Text/substring`
- `Text/length`
- `Text/find`
- `Text/to_number`
- `Text/is_empty`
- `Bool/not`
- `Bool/and`
- `List/range`
- `List/map`
- `List/retain`
- `List/count`
- `List/get`
- `List/find`
- `List/chunk`
- `List/sum`

These operators must work for any source, not for Cells by name.

### 4. Generic Derived Recompute

Replace Formula-specific recompute with generic derived field recompute.

For each derived field expression:

- evaluate it through the generic evaluator
- record every root/list field read during evaluation
- store a reverse dependency index from read field keys to derived field keys
- when source events mutate state, mark directly changed fields dirty
- recompute only dirty derived fields and reverse dependents
- emit generic `FieldSet` semantic deltas and document invalidation patches

Cycle handling:

- keep an evaluation stack of derived field keys
- if evaluation re-enters an active key, return a structured `cycle_error`
- keep the runtime alive and emit normal generic deltas

Cells can then compute:

```boon
computed:
    compute_cell(address: address, formula_text: formula_text)

value:
    computed.value

error:
    computed.error
```

No Rust code should know that those fields are spreadsheet fields.

### 5. Rewrite Cells Source

Rewrite `examples/cells/formula.bn` and `examples/cells/cell.bn`:

- delete `sheet_reader`
- delete all `Formula/*` calls
- add Boon helper functions for formula text parsing/eval
- keep source fields and HOLD semantics unchanged
- keep display fields named by Boon source, not Rust

Use the existing scenario vocabulary (`A0`, `B0`, `C0`) unless intentionally
changing the scenario and all native reports together.

### 6. Remove Formula Rust

Delete the formula-specific path after the Boon path passes focused tests:

- `FormulaOperation`
- `FormulaReader`
- `FormulaOperatorPlan`
- `RuntimeFormulaOperation*`
- `FormulaAst`
- `FormulaTerm`
- `FormulaOp`
- `AddressedFormulaRuntime*`
- Formula-specific scenario preparation/assertion helpers
- formula parser/evaluator/dependency functions
- parser reserved-operator entries for `Formula/*`
- IR formula lowering and static verification

Replace tests with generic tests that prove the same behavior from Boon helper
source.

### 7. Verification

Run in this order:

```bash
cargo fmt
cargo test -p boon_parser -p boon_ir -p boon_runtime --lib
cargo test -p boon_native_playground --bin boon_native_playground
cargo run -q -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/debug/cells-cli-run.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json
cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json
```

Then run audits:

```bash
rg -n "Formula/" crates examples -S
rg -n "FormulaAst|FormulaTerm|FormulaOperatorPlan|AddressedFormulaRuntime|parse_formula_ast|formula_ast_dependencies" crates -S
rg -n "Grid/cells|SetCellText|SetCellEditor|default_grid_formula|example.*cells|cells.*shortcut" crates examples -S
```

Before native handoff readiness, run the native GPU gates required by
`docs/architecture/NATIVE_GPU_PIPELINE.md`.

## Reporting Requirements

Final implementation report must include:

- exact commit baseline used
- files changed
- what generic Boon execution support was added
- proof that `Formula/*` is gone from production code/source
- exact CLI/native report summaries
- whether scroll speed still has `wall_clock_frame_budget_pass: false`
- whether manual Cells testing is ready as a separate follow-up
