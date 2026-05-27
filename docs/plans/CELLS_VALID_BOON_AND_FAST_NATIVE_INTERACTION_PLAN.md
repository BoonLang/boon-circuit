# Cells Valid Boon Source And Fast Native Interaction

## Summary

Cells has two milestones:

- The Cells example source must be valid generic Boon, with no spreadsheet-specific
  shortcuts in user source or native runtime behavior.
- Cells native preview interactions must feel immediate. The measured baseline
  before this plan was about 3.09 seconds for a debug Cells layout proof and
  about 0.60 seconds in release, while the semantic runtime select path was
  already about 0.03 ms.

`Element/repeat` stays. It is the generic document primitive for rendering one
child per item in a list. Cells needs it for column headers, sheet rows, and row
cells without hand-writing thousands of elements or adding a Cells-specific grid
widget.

## Key Changes

- Remove `List/table` from the Cells source. Generate the 26x100 sheet from
  generic `List/range` and `List/map`, then derive addresses and seeded formulas
  with generic Boon helpers.
- Keep formula parsing and evaluation in Boon helper functions. Rust may provide
  generic `Text/*`, `List/*`, `Bool/*`, `Error/*`, and `Element/*` operators,
  but not Cells formula business logic.
- Add source gates proving manifest-backed Cells source contains no `Formula`,
  `Grid`, `List/table`, `EXAMPLE`, or `#` comments.
- Virtualize native `Element/repeat` lowering for large list-backed UI so live
  focus/edit materializes visible rows and columns plus overscan, not all 2600
  cells.
- Keep full document-layout artifacts for verifier/report commands, but remove
  full parse + full state summary + pretty JSON artifact write/read from the
  live preview event path.

## Targets

Debug mode:

- Cell select/focus p95 <= 120 ms, p99 <= 180 ms, max <= 250 ms.
- Visible typed edit p95 <= 150 ms, max <= 300 ms.
- Commit/fanout p95 <= 200 ms, max <= 350 ms.
- Semantic runtime p95 remains <= 5 ms.

Release mode:

- Cell select/focus p95 <= 16.7 ms, p99 <= 25 ms, max <= 50 ms.
- Visible typed edit p95 <= 25 ms, max <= 50 ms.
- Commit/fanout p95 <= 33 ms, max <= 75 ms.
- Scroll p95 <= 16.7 ms, p99 <= 25 ms.

## Test Plan

- `cargo test -p boon_parser -p boon_ir -p boon_runtime -p boon_native_playground --lib`
- `cargo run -q -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/debug/cells-cli-run.json`
- `cargo xtask verify-native-cells-interaction-speed --profile debug`
- `cargo xtask verify-native-cells-interaction-speed --profile release`
- Native GPU gates from `AGENTS.md`.
- `cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json`

## Assumptions

- `Element/repeat` is generic UI list rendering, not Cells business logic.
- `List/range`, `List/map`, `List/get`, `List/find`, `List/find_value`,
  `List/chunk`, `List/sum`, and scalar text/bool/error operators are allowed
  generic stdlib behavior.
- `List/table` is removed from Cells because it is too spreadsheet-shaped for
  this milestone.
