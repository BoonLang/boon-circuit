# NovyWave Rust Reference Parity Goal

This document is a durable handoff for implementing NovyWave visual and
behavioral parity in the Boon native playground.

## Original Plan

### Summary

Compare the existing NovyWave Rust frontend against the Boon NovyWave example as
a clean-room reference pass, then fix the Boon version by behavior and layout
contract, not by porting Rust code.

Main findings to plan around:

- Rust selected variables are a coordinated three-column surface: name, value,
  and one continuous waveform canvas with shared row metrics. Boon currently
  renders self-contained waveform rows plus dense toolbar bands.
- Rust separates top-level layout into dock modes, resizable panels, scrollable
  content, and keyboard footer bands. Boon uses many fixed heights and width
  presets, so interaction/resize can compress controls.
- Rust has dedicated UI flows for file/workspace dialogs, marker manager,
  analog limits, variable search, grouping, row resize, and format dropdowns.
  Boon has partial deterministic versions, but several are flattened into
  buttons or static labels.
- Native Boon relayout must be fixed first so comparison results are not
  distorted by default viewport fallback.

### Key Changes

- Fix native viewport correctness in `crates/boon_native_playground/src/main.rs`:
  - Preserve root `Fill`/`minimum: Screen` instead of forcing root-child height
    to `720`.
  - Carry the current surface viewport through resize, click, keyboard, and
    scenario relayout paths.
  - Add a regression check proving NovyWave does not fall back to `920x720`
    after interaction.

- Rework Boon NovyWave view structure in `examples/novywave/View/NovyView.bn`:
  - Replace the current `timeline_panel` with a selected-variables panel
    matching the Rust contract: header, name column, value column, wave column,
    and footer bands.
  - Move zoom/pan/cursor controls into footer/status bands instead of stacking
    them above the waveform.
  - Make the waveform a single continuous region with row metrics, not
    independent per-row wave areas.
  - Keep top browser panels as files/scopes plus variables/search, with
    fill/scroll behavior instead of hard-coded `346`, `650`, and `124` heights.

- Expand Boon model state in `examples/novywave/RUN.bn` only where needed for
  clean parity:
  - Add explicit row metric/list values for group headers, variable rows,
    dividers, and footer height.
  - Add state for marker manager, analog limits dialog, format menu state, dock
    mode, panel sizes, and selected-variable grouping.
  - Keep all state serializable and structurally comparable; no Rust handles,
    resource ids, or copied Tauri command surfaces.

- Split Boon view modules after the parity pass:
  - Keep `RUN.bn` as orchestration/model wiring.
  - Move view code toward the existing plan shape: app shell, toolbar/header,
    file panel, variable panel, selected-variable panel, waveform rows, dialogs,
    and theme/materials.
  - Keep behavior equivalent before splitting; do not combine refactor and
    behavior changes without verifier coverage.

- Tune theme/materials in `examples/novywave/Theme/NovyTheme.bn`:
  - Reduce overpowering cursor/marker/trace glow and clip it to intended
    waveform regions.
  - Match Rust/design contrast roles: panels quiet, waveform high contrast,
    selected rows clear, controls readable.
  - Preserve Boon physical material identity rather than copying CSS styling.

### Comparison Ledger

Create a repo-local parity ledger, then use it as the implementation checklist:

- Shell and dock layout: Rust `main_layout`, `panel_layout`, `dragging` versus
  Boon `loaded_app_*`, `panel_layout`, divider sources.
- Files/scopes panel: Rust `file_management`, tree view, empty/loading/error
  states versus Boon `left_panel`, `files_list`, scope rows.
- Variables panel: Rust `variable_selection_ui`, virtual list, search states
  versus Boon `variables_panel`, `search_box`, `variable_rows`.
- Selected variables: Rust `selected_variables_panel` and
  `selected_variables_layout` versus Boon `timeline_panel`, selected rows,
  groups, row resize.
- Waveform rendering: Rust `waveform_canvas`/`rendering` continuous canvas
  versus Boon document primitives and native GPU rows.
- Controls and dialogs: Rust action buttons, file picker, workspace picker,
  marker manager, analog limits, format dropdown versus Boon dialog/control
  functions.
- Verification/test API: Rust test API and screenshots versus Boon app-owned
  readbacks, layout artifacts, scenarios, and native GPU reports.

### Test Plan

- Add or update structural layout tests:
  - no toolbar/text overlap;
  - no clipped required labels;
  - selected-variable row spans align across name, value, and wave columns;
  - dock/stacked layouts preserve minimum usable sizes;
  - event relayout uses the real current viewport.

- Add native visual reports:
  - `cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json`
  - include the report from
    `cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json`.

- Cover scenarios:
  - empty, loading, loaded;
  - file/scope selection;
  - search and no-match state;
  - select/remove/restore/reorder variables;
  - group create/rename/collapse/delete;
  - format changes;
  - marker add/rename/jump/delete;
  - analog auto/manual/invalid limits;
  - dock/stack resize;
  - dark/light mode.

- Final gates:
  - targeted NovyWave visual/layout tests;
  - `cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json`;
  - `cargo xtask verify-native-gpu-scroll-speed --example novywave --report target/reports/native-gpu/scroll-speed-novywave.json`;
  - `cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json`.

### Assumptions

- `/home/martinkavik/repos/NovyWave` is reference material only.
- Do not copy or translate custom Rust/Tauri/Fast2D code into Boon.
- Boon owns layout, UI workflow, state transitions, request policy, formatting
  policy, and visual intent.
- Rust bridge work remains thin, pure-data, and outside this view-parity fix
  unless a missing fixture/data contract blocks the UI.
- Do not commit or push unless explicitly requested.

## Detailed Goal Prompt

```text
Implement the NovyWave Rust-reference parity plan in /home/martinkavik/repos/boon-circuit.

Goal: make the Boon NovyWave example visually and behaviorally match the
original Rust NovyWave reference as closely as the Boon/native document API
allows, especially the selected-variables/waveform area. Continue in a
compare-fix-verify loop until the Boon app has no obvious visual/layout parity
defects against the Rust reference and the native GPU gates pass.

Hard constraints:
- Follow AGENTS.md. Do not commit or push.
- Treat docs/architecture/NATIVE_GPU_PIPELINE.md as the active native GPU
  contract.
- Do not copy or translate Rust/Tauri/Fast2D implementation code from
  /home/martinkavik/repos/NovyWave. Use it only as behavior, layout, and visual
  reference material.
- Boon owns view logic, state transitions, layout, styling, formatting policy,
  request policy, and workflow behavior.
- Rust changes in boon-circuit must stay generic/native infrastructure or
  verifier work, not NovyWave-specific shortcuts except thin test/proof plumbing
  where already established.
- Native proof must use app-owned WGPU readbacks, layout artifacts, reports,
  host events, and native GPU verifiers. Do not use compositor screenshots,
  browser windows, xdotool/ydotool, or human observation as pass evidence.

Start by implementing the planned foundations:
1. Fix native viewport correctness in crates/boon_native_playground/src/main.rs:
   - preserve root Fill/minimum Screen behavior;
   - stop forcing root-child Fill height to 720;
   - carry the current surface viewport through resize, click, keyboard, and
     scenario relayout paths;
   - add regression coverage proving NovyWave does not fall back to 920x720
     after interactions.

2. Rework examples/novywave/View/NovyView.bn toward the Rust reference
   structure:
   - app shell with stacked/docked layouts;
   - top files/scopes and variables/search panels;
   - bottom selected-variables panel with header, name column, value column,
     continuous wave column, and footer/status bands;
   - remove the overloaded toolbar pile above the waveform;
   - keep controls readable, separated, and non-overlapping;
   - make waveform rows share row metrics across name/value/wave columns.

3. Expand examples/novywave/RUN.bn only as needed for clean Boon-owned parity:
   - row metrics and divider/column sizing;
   - dock/stack mode and panel sizes;
   - grouping, marker manager, analog limits, format menu/dropdown, row resize,
     search states;
   - keep everything serializable and structurally comparable.

4. Tune examples/novywave/Theme/NovyTheme.bn:
   - reduce overpowering glow;
   - improve button/control contrast;
   - keep waveform data high contrast;
   - preserve Boon physical material identity rather than copying CSS.

Then enter the loop:
- Compare Rust reference modules under /home/martinkavik/repos/NovyWave/frontend/src
  against Boon NovyWave files.
- Maintain a parity ledger covering shell/dock layout, files/scopes, variables
  panel, selected variables, waveform rendering, controls/dialogs,
  keyboard/footer controls, visual theme, and verification.
- For each mismatch, decide whether it is:
  - must-match behavior;
  - visual reference to approximate in Boon;
  - intentionally different because Boon uses a different API;
  - out of scope because it belongs to Rust bridge/product integration.
- Fix must-match and visual defects in Boon/native code.
- Regenerate app-owned artifacts and inspect them.
- Repeat until the Rust reference screenshots and Boon native readbacks are
  visually aligned enough that no major layout, overlap, readability,
  missing-control, or workflow mismatch remains.

Reference sources to inspect repeatedly:
- /home/martinkavik/repos/NovyWave/frontend/src/main.rs
- /home/martinkavik/repos/NovyWave/frontend/src/app.rs
- /home/martinkavik/repos/NovyWave/frontend/src/file_management.rs
- /home/martinkavik/repos/NovyWave/frontend/src/variable_selection_ui.rs
- /home/martinkavik/repos/NovyWave/frontend/src/selected_variables_panel.rs
- /home/martinkavik/repos/NovyWave/frontend/src/selected_variables_layout.rs
- /home/martinkavik/repos/NovyWave/frontend/src/visualizer/canvas/waveform_canvas.rs
- /home/martinkavik/repos/NovyWave/frontend/src/visualizer/canvas/rendering.rs
- /home/martinkavik/repos/NovyWave/docs/screenshots/
- /home/martinkavik/repos/NovyWave/design/figma/
- examples/novywave/RUN.bn
- examples/novywave/View/NovyView.bn
- examples/novywave/Model/NovyModel.bn
- examples/novywave/Theme/NovyTheme.bn

Add/update verification:
- Add/promote cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json.
- Include novywave_visual_spatial_evidence in preview-e2e-novywave.json.
- Assert no text/control overlap, no clipped required labels, no
  outside-viewport required items, row alignment across selected-variable
  columns, separate ruler/footer lanes, and clipped glow.
- Add negative cases for stale visual metrics, overlap-only broad-luma pass,
  clipped labels, ruler collisions, and viewport fallback.

Run targeted verification as you go:
- cargo test -p boon_native_playground novywave_dark_light_material_readbacks_cover_visual_regions -- --nocapture
- cargo xtask verify-native-gpu-novywave-visual --report target/reports/native-gpu/novywave-visual.json
- cargo xtask verify-native-gpu-preview-e2e --example novywave --report target/reports/native-gpu/preview-e2e-novywave.json
- cargo xtask verify-native-gpu-scroll-speed --example novywave --report target/reports/native-gpu/scroll-speed-novywave.json
- cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json
- cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json

After native gates pass, restart the latest release playground for manual
follow-up per AGENTS.md, killing only the matching old NovyWave release
playground process tree first.

Stop only when:
- the parity ledger has no open must-fix visual/behavioral items;
- the latest app-owned Boon readbacks visually match the Rust reference
  structure;
- all targeted NovyWave reports pass and are fresh;
- verify-native-gpu-all --check-existing passes.
```

## Short /goal Prompt

```text
Read and execute docs/plans/NOVYWAVE_RUST_REFERENCE_PARITY_GOAL_PROMPT.md.

Implement the plan end to end. Follow AGENTS.md and docs/architecture/NATIVE_GPU_PIPELINE.md. Do not commit or push.

Use /home/martinkavik/repos/NovyWave only as clean-room visual/behavior reference. Fix native viewport/layout correctness first, then keep comparing original Rust NovyWave view/elements with the Boon NovyWave implementation and fixing mismatches until they visually and behaviorally match as closely as Boon/native document APIs allow.

Do not stop until the parity ledger has no must-fix visual/behavior items, app-owned readbacks look correct, targeted NovyWave native reports pass, and verify-native-gpu-all --check-existing passes.
```
