# Cells Playground Reality Parity Plan

## Goal

Make the visible native Cells playground match the documented 7GUIs Cells
example instead of only proving a four-cell scenario subset.

The preview boundary is strict: the visible Cells surface must be generated from
`examples/cells.bn` state, initial data, and `VIEW` declarations. The native
playground may provide generic UI components and generic interpretation of
component attributes, but it must not add a Cells-specific widget, renderer
branch, hardcoded formula bar, or hardcoded spreadsheet preview.

The current source declares `Grid/cells(columns: 26, rows: 100)` and a generic
`ForEach cells as cell` view, but the visible playground renders only four long
rows because `cells_summary()` exposes only `A0`, `B0`, `C0`, and `D0` to the
renderer. This plan fixes that product/reality gap and adds verification that
prevents future examples from passing semantic/operator gates while their
visible app is much narrower than the markdown requirements.

## Current Mismatch

- `examples/cells.bn` declares a 26x100 grid and row-template `VIEW`.
- `crates/boon_runtime/src/lib.rs` currently summarizes only
  `["A0", "B0", "C0", "D0"]`.
- `crates/boon_ply_playground/src/lib.rs` renders `ForEach cells` from
  `output.state_summary`, so the UI can only render those four rows.
- Existing `cells.scn` and headed/operator reports exercise A0-D0 and therefore
  do not prove that the visible playground is spreadsheet-shaped.
- Existing speed stress reports prove an internal 26x100 runtime path, not that
  the visible app exposes a 26x100 grid/viewport.

This is not a parser/IR/runtime-storage issue. It is a visible playground state
projection and verification-coverage issue.

## Key Changes

- Replace the four-cell `cells_summary()` projection with a visible grid
  viewport projection derived from the real `Grid/cells` dimensions and generic
  keyed storage.
  - The summary must expose enough structured data for the renderer to draw a
    spreadsheet viewport, not four hand-picked scenario cells.
  - The default visible viewport must include at least columns A-Z and enough
    rows to prove grid shape on a normal 1500x1000 headed capture.
  - A0-D0 may remain highlighted or inspected, but they must not be the only
    renderable cells.

- Render Cells as a spreadsheet viewport through generic renderer primitives.
  - Add generic `Grid`/viewport support only if existing `Row`/`Column`/`ForEach`
    primitives cannot produce a stable spreadsheet layout.
  - The renderer must show column headers, row addresses, and cell editors/values
    in a compact grid, not full-width row cards.
  - Focused-cell styling, display-value versus edit-formula behavior, and
    vertical/horizontal wheel scrolling must be expressed as generic component
    attributes consumed by the generic renderer.
  - Editing, commit, cancel, formula display, error display, and selection/focus
    must still route through the existing Boon `SOURCE` bindings.

- Initialize the Cells example with spreadsheet demo data through `Grid/cells` initial
  fields, not through a renderer special case.
  - A0, A1, and A2 should start with literal demo numbers.
  - B0 should demonstrate a function formula such as `=add(A0,A1)`.
  - C0 should demonstrate a range formula such as `=sum(A0:A2)`.
  - The initially visible unfocused cells should show evaluated values, and the
    focused editor should show the formula text.

- Extend Cells scenarios and headed checks so visible behavior matches the docs.
  - Keep the existing A0-D0 scenario steps.
  - Add at least one visible non-A-D interaction, for example `Z0` or `A99`, to
    prove the renderer is not hardcoded to the first four cells.
  - Add an off-scenario visible-shape assertion that counts rendered cell
    editors/tiles and fails if fewer than the configured viewport cells are
    present.
  - If scrolling is implemented, add a scroll/focus step that reaches a cell not
    initially visible and verifies source routing still works.

- Add a Cells visible-reality report.
  - New or extended xtask command should write
    `target/reports/cells-visible-reality.json`.
  - The report must include source grid dimensions, viewport dimensions, rendered
    cell count, visible address sample, screenshot path/hash, nonblank image
    stats, and pass/fail checks.
  - It must fail when `examples/cells.bn` declares 26x100 but the visible app
    exposes only A0-D0.

- Wire reality checks into readiness.
  - `verify-cells-all`, `verify-examples-all`, `audit-manual-readiness`, and
    `audit-goal-readiness` must require the Cells visible-reality report.
  - `verify-report-schema` must validate the report shape and artifact hashes.
  - `verify-playground-genericity` must scan the new renderer/report path and
    reject Cells-only hardcoded render branches outside examples, scenarios,
    report labels, or explicitly named test fixtures.

- Update markdown so requirements, implementation, and verification use the same
  terms.
  - Keep `docs/examples/CELLS_CIRCUIT_STYLE.md` as the product/spec source.
  - Explicitly say whether Cells is a full spreadsheet viewport, a bounded
    viewport over a 26x100 model, or a four-cell demo. The intended fix is a
    bounded spreadsheet viewport over the 26x100 model, not a four-cell demo.
  - Document that semantic/runtime stress evidence is insufficient without
    visible-reality evidence.

## Future Mismatch Prevention

- Every example markdown spec must identify its visible app shape in a
  machine-checkable way: declared source dimensions, minimum rendered element
  count, key visible labels/addresses, and required interaction surfaces.
- Every visible app requirement must identify whether it is sourced from Boon
  source text, generic renderer behavior, runtime state summary projection, or
  report verification. Example-specific UI behavior belongs in Boon source and
  scenario data, not in playground renderer branches.
- Every example must have a visible-reality report or equivalent headed report
  section that binds those visible requirements to screenshot/canvas/UI evidence.
- Readiness must reject broad claims when only a scenario subset was exercised.
  Scenario evidence can prove behavior for named labels; it cannot prove the
  full visible surface unless it also counts and samples the surface.
- Any report field named `complete`, `final`, `all`, `generic`, or `ready` must
  be derived from both semantic/runtime evidence and visible-reality evidence
  when the example has a UI.
- Future examples must add their visible-reality gate before they are allowed in
  `verify-examples-all`.

## Test Plan

Run the normal implementation gates:

```bash
cargo fmt --check
cargo test -p boon_parser -p boon_ir -p boon_runtime -p boon_ply_playground
cargo test -p boon_runtime -p xtask
```

Run Cells-specific checks:

```bash
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/cells-cli-run.json
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply
cargo xtask verify-cells-visible-reality --report target/reports/cells-visible-reality.json
cargo xtask verify-cells-operator-e2e --report target/reports/cells-operator-e2e.json
cargo xtask verify-cells-all --check-existing --report target/reports/cells-all.json
```

Run cross-example/readiness checks:

```bash
cargo xtask verify-playground-genericity --report target/reports/playground-genericity.json
cargo xtask verify-runtime-finality --report target/reports/runtime-finality.json
cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json
cargo xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json
cargo xtask audit-manual-readiness --report target/reports/debug/manual-readiness.json
cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
cargo xtask verify-report-schema
```

Manual sanity check:

```bash
cargo build -p boon_ply_playground
cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_ply_playground --example cells --mode app
```

The visible Cells app must show a spreadsheet-like viewport with many compact
cells, not four long row editors. Editing visible cells must still update values
and formulas through Boon source events.

## Acceptance Criteria

- `target/reports/cells-visible-reality.json` passes and proves the visible
  viewport is derived from the 26x100 `Grid/cells` model.
- `target/reports/cells-headed-ply.json` still passes with full OS pointer and
  keyboard input and no missing scenario steps.
- `target/reports/cells-all.json`, `target/reports/examples-all.json`, and
  `target/reports/goal-readiness.json` pass only after the visible-reality gate
  passes.
- A regression that reintroduces `["A0", "B0", "C0", "D0"]` as the complete
  visible Cells surface fails an automated gate.
- Documentation no longer lets semantic/stress reports be mistaken for proof of
  the visible Cells product.

## Copy-Paste `/goal` Prompt

```text
/goal In /home/martinkavik/repos/boon-circuit, implement docs/plans/CELLS_PLAYGROUND_REALITY_PARITY_PLAN.md. Fix the Cells playground reality gap: examples/cells.bn declares Grid/cells(columns: 26, rows: 100), but the visible playground currently renders only four long cells because cells_summary exposes only A0-D0. Do not weaken the docs to match the four-cell UI. Make the visible native Cells playground render a spreadsheet-like viewport derived from the real 26x100 model, with compact cells, visible addresses/headers, and existing edit/commit/cancel/formula behavior still routed through Boon SOURCE bindings.

Add automated verification so this mismatch cannot recur. Create or extend an xtask report at target/reports/cells-visible-reality.json that proves source grid dimensions, viewport dimensions, rendered cell count, visible address samples beyond A0-D0, screenshot/hash/nonblank evidence, and pass/fail checks. Wire that report into verify-cells-all, verify-examples-all, audit-manual-readiness, audit-goal-readiness, verify-report-schema, and genericity/finality scans as appropriate. Extend Cells scenarios/headed checks with at least one non-A-D visible interaction or scroll/focus proof so the headed/operator gates cannot pass by exercising only A0-D0.

Preserve the existing parser/IR/runtime honesty work. Avoid Cells-specific renderer branches except in examples, scenarios, report labels, or explicitly named test fixtures. Update docs/examples/CELLS_CIRCUIT_STYLE.md and the shared verification/readiness docs so semantic/runtime stress evidence is not presented as proof of visible app parity.

Do not stop until these pass:
cargo fmt --check
cargo test -p boon_parser -p boon_ir -p boon_runtime -p boon_ply_playground
cargo test -p boon_runtime -p xtask
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/cells-cli-run.json
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply
cargo xtask verify-cells-visible-reality --report target/reports/cells-visible-reality.json
cargo xtask verify-cells-operator-e2e --report target/reports/cells-operator-e2e.json
cargo xtask verify-cells-all --check-existing --report target/reports/cells-all.json
cargo xtask verify-playground-genericity --report target/reports/playground-genericity.json
cargo xtask verify-runtime-finality --report target/reports/runtime-finality.json
cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json
cargo xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json
cargo xtask audit-manual-readiness --report target/reports/debug/manual-readiness.json
cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
cargo xtask verify-report-schema

Then launch the visible app:
cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_ply_playground --example cells --mode app

At the end, report changed files, report paths, launch PID, verification results, and any remaining mismatch between markdown requirements and the visible Cells UI.
```
