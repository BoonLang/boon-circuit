# TodoMVC Reference Parity Plan

## Goal

Make the native Boon Circuit TodoMVC preview visually and behaviorally
indistinguishable from the original JavaScript TodoMVC browser reference while
keeping the renderer generic.

The user should not be able to tell whether the visible app is the original
browser TodoMVC or Boon code rendered through the generic Ply backend.

## Hard Constraint

No TodoMVC-specific rendering shortcuts are allowed in Rust.

Rust may implement generic render primitives, layout attributes, style
attributes, asset loading, hit testing, and verification tooling. Rust must not
contain branches such as `if todomvc`, `TodoMvcView`, `todo_row_title`, or
hardcoded visual behavior that only exists to bend the renderer for TodoMVC.

TodoMVC-specific structure, text, bindings, dimensions, colors, and behavior
must live in Boon source or generic test fixtures.

## References

- Original Boon Actors/Zoon-style source:
  `/home/martinkavik/repos/boon/playground/frontend/src/examples/todo_mvc/todo_mvc.bn`
- Browser reference screenshot:
  `/home/martinkavik/repos/raybox/assets/todomvc/reference_screenshot.png`
- Current Boon Circuit example:
  `examples/todomvc.bn`
- Current generic native renderer:
  `crates/boon_ply_playground/src/lib.rs`

## Definition Of Done

1. TodoMVC is rendered from `examples/todomvc.bn` `VIEW { ... }`.
2. The semantic circuit still lowers with `VIEW` stripped from state logic.
3. The Ply preview walker is fully example-agnostic.
4. The visual output matches the browser reference within deterministic
   tolerances.
5. TodoMVC interactions work through visible native controls:
   add, reject empty add, toggle all, toggle row, filter active/completed/all,
   edit/commit/cancel/blur, clear completed, delete.
6. Cells still works through the same generic renderer path.
7. Manual launch uses Wayland and a readable, stable scale.
8. Verification catches visual, behavioral, performance, and renderer-genericity
   regressions.

## Deterministic Visual Algorithm

Implement a repo-local visual comparator instead of relying only on subjective
manual screenshots.

1. Load the browser reference image from
   `/home/martinkavik/repos/raybox/assets/todomvc/reference_screenshot.png`.
2. Run the native TodoMVC smoke capture into
   `target/reports/todomvc-reference-parity.png`.
3. Crop both images to the TodoMVC app region:
   title, main panel, rows, footer, and instructional footer text.
4. Normalize scale by fitting the Boon crop to the reference crop's app-panel
   width, not to full monitor size.
5. Produce a perceptual diff using deterministic metrics:
   mean absolute RGBA error, p95 error, max connected mismatch region, and
   structural bounding boxes for title, input, rows, footer, and footer text.
6. Fail if any of these exceed tolerances:
   - title bounding box center differs by more than 8 px
   - main panel width differs by more than 8 px after normalization
   - row height differs by more than 6 px
   - input height differs by more than 6 px
   - footer height differs by more than 6 px
   - mean pixel error over crop exceeds 8/255
   - p95 pixel error over crop exceeds 32/255
   - any connected mismatch region larger than 2 percent of crop area
7. Write artifacts:
   - normalized reference crop
   - normalized Boon crop
   - heatmap diff
   - JSON report with metrics and pass/fail thresholds

The comparator may be implemented in Rust, Zig, or a small repo-local xtask,
but it must run from a checked-in command and must not need network access.

## Required Generic Render Features

Add only generic primitives and attributes:

- `Text`: font family, size, color, alignment, italic, underline,
  strikethrough, width, height, padding.
- `Column` and `Row`: width, height, min/max width, background, border sides,
  shadows, gap, padding, alignment.
- `Input`: placeholder style, focus style, cursor/selection colors,
  submit/change/blur/focus/cancel source bindings.
- `Button`: text style, hover style, selected style, border radius, optional
  underline.
- `Checkbox`: generic icon/text/asset style for checked and unchecked states.
- `ForEach`: stable list-key-backed native element identity hidden from Boon
  data comparisons.
- `When`/visibility: generic conditional display from Boon state values.
- `Image` or `Icon`: generic asset support for the original checkbox SVGs if
  text glyphs cannot match closely enough.

These features must be usable by Cells or future examples. A regression should
fail if a new renderer feature is named after TodoMVC.

## TodoMVC VIEW Rewrite

Rewrite `examples/todomvc.bn` `VIEW` to mirror the original Boon source:

- Main title: `todos`, reference color, exact top margin and size.
- Main panel: centered, fixed reference width, white background, subtle border
  and stacked shadows.
- Input row: toggle-all affordance and italic placeholder matching the browser
  reference.
- Rows: 40 px style at normalized scale, circular checkbox, large left-aligned
  title, completed row strikethrough and dimmed text.
- Delete button: `×`, reference color, hover-visible when generic hover is
  available; until then visible but flagged in report.
- Footer: item count, filters centered, clear completed aligned right and hidden
  when no completed rows exist.
- Instructional footer: three centered lines matching the original.

Keep data identity hidden. The view may bind to hidden renderer keys internally,
but Boon comparison and visible state must compare only data.

## Behavioral Verification

Add or update headed native tests so they drive real visible controls:

- New todo input: type text, Enter adds row, input clears.
- Empty input: Enter does not add a row.
- Toggle row: visible checkbox changes state and active count.
- Toggle all: all rows complete, then all rows active again.
- Filters: All, Active, Completed show the correct row subset without full list
  diffing.
- Edit title: open edit, change, Enter commits.
- Escape cancel: edit draft is discarded.
- Blur commit: losing focus commits trimmed text.
- Clear completed removes completed rows.
- Delete removes the hovered/visible row.

Each test must assert semantic deltas, render patches, visible screenshot, and
final state summary.

## Genericity Verification

Add a static check that fails if playground preview code contains:

- `todomvc` or `cells` in render dispatch except example selection and test
  names.
- `TodoMvc`/`Cells` renderer structs.
- Hardcoded TodoMVC visual strings in Rust such as `todos`,
  `What needs to be done?`, `Clear completed`, `todo_row`, or
  `selected_filter`.

Allowed locations for example-specific strings:

- `examples/*.bn`
- scenario files
- report/test names
- docs

## Speed And Resource Gates

Reference parity must not hide slowness:

- Example switch p95 under 50 ms in dev build and under 16 ms in release build
  after warmup.
- TodoMVC single interaction p95 under 4 ms in release build.
- Cells edit/commit p95 under 4 ms in release build for the 7GUIs scenario.
- 10,000-row TodoMVC stress must update proportional deltas only; no full list
  copy between runtime and renderer.
- No unbounded allocations during steady-state interaction after warmup.

Reports must include RSS, frame time, interaction latency, render patch count,
semantic delta count, and screenshot path.

## Manual Launch Contract

Use this exact launch path for human testing:

```sh
cargo build -p boon_ply_playground
cosmic-background-launch --workspace boon-circuit -- \
  ./target/debug/boon_ply_playground --example todomvc --mode app
```

The app must run through Wayland:

- `XDG_SESSION_TYPE=wayland`
- `WAYLAND_DISPLAY` set
- native report includes `native_display_contract.status = "pass"`
- no stale old playground process remains

## Required Commands

Run before claiming done:

```sh
cargo fmt --check
cargo test -p boon_parser -p boon_runtime -p boon_ply_playground
xvfb-run -a -s "-screen 0 1600x1000x24" cargo run --release -p boon_ply_playground -- --smoke-launch --example todomvc --frames 4 --report target/reports/todomvc-reference-parity-smoke.json
xvfb-run -a -s "-screen 0 1600x1000x24" cargo run --release -p boon_ply_playground -- --smoke-launch --example cells --frames 4 --report target/reports/cells-reference-parity-smoke.json
xvfb-run -a -s "-screen 0 1600x1000x24" cargo run --release -p boon_ply_playground -- --smoke-launch --example todomvc --frames 4 --report target/reports/todomvc-release-switch-smoke.json
cargo xtask verify-todomvc-reference-parity --report target/reports/todomvc-reference-parity.json
cargo xtask verify-playground-genericity --report target/reports/playground-genericity.json
cargo xtask verify-todomvc-headed-focusless --report target/reports/todomvc-headed-focusless.json
cargo xtask verify-cells-headed-focusless --report target/reports/cells-headed-focusless.json
```

If an xtask does not exist yet, implementing it is part of the goal.

## `/goal` Implementation Prompt

```text
Implement docs/plans/TODOMVC_REFERENCE_PARITY_PLAN.md in /home/martinkavik/repos/boon-circuit.

Do not stop at a plan. Make TodoMVC visually and behaviorally match the original JavaScript TodoMVC browser reference while keeping the native Ply renderer generic. Use /home/martinkavik/repos/boon/playground/frontend/src/examples/todo_mvc/todo_mvc.bn and /home/martinkavik/repos/raybox/assets/todomvc/reference_screenshot.png as the visual/structural references.

Hard constraints:
- No TodoMVC-specific Rust renderer, no Rust branches that bend rendering for TodoMVC, and no example-specific preview widgets.
- TodoMVC-specific layout, text, style, and SOURCE bindings must live in examples/todomvc.bn VIEW.
- Rust may only add generic render primitives, generic style attributes, generic parser/render IR support, generic verification, and generic Ply backend behavior.
- Preserve Cells and future examples through the same generic renderer.
- Launch visible native tests through Wayland and cosmic-background-launch --workspace boon-circuit.

Implement the deterministic visual comparator, headed behavioral checks, genericity check, and speed/resource gates described in the plan. Run all required commands from the plan and leave reports/screenshots under target/reports. At the end, launch the rebuilt app in the boon-circuit workspace for manual testing and report exact process id, commands run, artifacts, and any remaining blockers.

After this implementation prompt finishes, run the checker prompt below as a separate /goal pass before considering the result ready for final manual testing.
```

## `/goal` Checker Prompt

```text
Check the implementation of docs/plans/TODOMVC_REFERENCE_PARITY_PLAN.md in /home/martinkavik/repos/boon-circuit.

Do not implement new features unless needed to fix a failed gate. Verify that TodoMVC is rendered from Boon VIEW data through a generic Ply renderer and that no TodoMVC-specific Rust rendering shortcut remains. Compare the native screenshot against /home/martinkavik/repos/raybox/assets/todomvc/reference_screenshot.png using the repo-local deterministic comparator. Run the required tests, smoke launches, genericity check, speed/resource gates, and Wayland launch check.

Report only confirmed evidence: commands, pass/fail status, artifact paths, process ids, and concrete blockers. If the TodoMVC window would still be visibly distinguishable from the original browser reference, mark the goal incomplete and explain exactly which visual or behavioral metrics failed.
```
