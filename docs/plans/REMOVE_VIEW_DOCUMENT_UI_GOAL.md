# Remove VIEW and Render From `document`

## Objective

Remove the special `VIEW { ... }` surface from Boon Circuit examples and replace
it with regular Boon code that builds a top-level `document` value. The preview
must be generated from parsed/lowered Boon data flowing through `document`, using
generic UI elements, style records, children, list mapping, data bindings, and
source bindings.

Do not preserve a hidden `VIEW` shortcut, stringly line extraction, or
Rust-side hardcoded UI construction.

## Required Context

Before editing, read:

- `AGENTS.md`
- `docs/plans/IMPLEMENTATION_PLAN.md`
- `docs/plans/EXAMPLE_VERIFICATION_PLAN.md`
- `docs/plans/MANUAL_TESTING_RUNBOOK.md`
- `examples/todomvc.bn`
- `examples/cells.bn`
- `crates/boon_parser/src/lib.rs`
- `crates/boon_ir/src/lib.rs`
- `crates/boon_ply_playground/src/lib.rs`
- `~/repos/boon/docs/language/BOON_SYNTAX.md`
- Small syntax fixtures under `~/repos/boon/crates/boon-cli/tests/*.bn`

Compare the intended syntax with both the sibling `~/repos/boon` examples/docs
and the actual `boon-circuit` parser. Do not assume they match.

## Syntax Requirements

- Reconcile comment syntax deliberately. Current `boon-circuit` examples use
  `#`, while `~/repos/boon/docs/language/BOON_SYNTAX.md` shows `--`.
- Verify what `crates/boon_parser/src/lib.rs` actually accepts today.
- Add parser tests for the intended comment syntax.
- Update examples and docs consistently.
- Keep identifiers and function names valid for the parser actually in this
  repo.
- Ensure every edited `.bn` file parses through the real parser and lowers
  through the real IR. Do not make syntax only the playground understands.

## Implementation Requirements

- Replace `VIEW { ... }` in TodoMVC and Cells with regular declarations centered
  on a top-level `document` variable.
- Express UI as normal Boon data:
  - generic element records or calls
  - style records
  - attrs/properties
  - child lists
  - `ForEach`/list mapping or the closest parser-supported equivalent
  - data bindings
  - source bindings
- Derive renderer bindings from structured AST/IR data reachable from
  `document`, not from whitespace splitting or raw view lines.
- Remove or retire `AstStatementKind::View`, `parse_view_lines`,
  `parsed_view_lines`, `view_lines_from_ast`, and any other `VIEW` extraction
  path unless a legacy-only compatibility path is explicitly tested and not used
  by current examples.
- Update IR view binding discovery to use typed/structured `document` data.
- Update the playground renderer to render from the generic `document`
  structure.
- Keep the renderer generic. Do not introduce TodoMVC-specific or
  Cells-specific Rust shortcuts.

## Behavior To Preserve

- TodoMVC behavior and visual parity.
- Cells behavior and visual parity.
- Cells official grid size: columns `A..Z`, rows `0..99`, total `2600` cells.
- Cells formula bar editing.
- Focused-cell styling.
- Blank display for empty cells.
- Cell expression result display, including real `0` when the formula result is zero.
- Header horizontal scroll sync.
- Vertical wheel scrolling and Shift+wheel horizontal scrolling.
- Code/source editor scrolling.
- Scenario expectations, source event routing, and report schemas.

## Performance Requirements

Make Cells interaction faster without shrinking the official `26x100` grid and
without hiding cells just to pass tests.

Prefer real performance fixes:

- avoid full-grid text resets on every keystroke when only one input changed;
- avoid repeated full parse/lower/render work where dirty scoped work is
  possible;
- avoid avoidable cloning of large render trees or state summaries;
- avoid repeated string parsing/splitting on hot input paths;
- avoid O(2600) text-control updates per keypress unless there is evidence it is
  necessary;
- preserve correctness of dependencies, cell expressions, focus, and source events.

Also make the source/code editor scroll fast. Use the generic scroll/input path
where possible instead of only tuning the Cells grid.

## Forbidden Shortcuts

Do not:

- hardcode TodoMVC or Cells UI in Rust;
- hardcode `document` strings in Rust;
- keep `VIEW` alive as the real implementation path;
- shrink the Cells grid below `26x100`;
- virtualize by lying to verification about rendered or reachable cells;
- fake source events;
- fabricate human reports;
- weaken report schemas or readiness checks to pass;
- bypass parser/IR lowering for examples;
- mark headed evidence as human evidence.

## Verification Requirements

Use headed and OS-event verification. The user will not be present during
implementation, so use isolated Xvfb/headed OS events by default and do not wait
for human testing.

Run and fix until these pass:

```bash
cargo fmt --check
cargo test -p boon_parser -p boon_ir -p boon_runtime -p boon_ply_playground --lib
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn --report target/reports/todomvc-cli-run.json
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/cells-cli-run.json
cargo xtask verify-todomvc-visible-reality --report target/reports/todomvc-visible-reality.json
cargo xtask verify-cells-visible-reality --report target/reports/cells-visible-reality.json
cargo xtask verify-todomvc-headed-ply --report target/reports/todomvc-headed-ply.json
cargo xtask verify-cells-headed-ply --report target/reports/cells-headed-ply.json
cargo xtask verify-report-schema
```

Add or update verifier checks so regressions cannot pass if:

- current examples still contain `VIEW`;
- rendering still comes from line-split `VIEW` text;
- `document` is missing;
- comment syntax is accepted inconsistently;
- Cells is smaller than `26x100`;
- Cells interaction is slow after the full grid renders;
- source/code editor scrolling remains slow;
- headed checks only prove a tiny scroll movement.

Capture and report concrete performance evidence before and after for:

- Cells focused-cell editing latency;
- Cells formula bar editing latency;
- Cells wheel scrolling;
- source/code editor wheel scrolling.

Include report paths and key numbers in the final response.

## Completion Rules

- Do not commit or push unless the user explicitly asks after reviewing the
  result.
- If a requirement is impossible without changing the parser syntax contract,
  document the exact blocker and keep the verification honest.
- Final response must list changed files, verification commands, report paths,
  and any remaining human-testing follow-up.
