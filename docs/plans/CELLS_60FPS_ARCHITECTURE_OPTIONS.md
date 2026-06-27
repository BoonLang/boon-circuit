# Cells 60 FPS Architecture Options

Status: working architecture note for TASK-0804A follow-up work. This is not a
replacement for `docs/architecture/NATIVE_GPU_PIPELINE.md`; the native GPU
contract remains authoritative.

## Current Diagnosis

The old spreadsheet-scale runtime failure is no longer the primary visible-click
blocker:

- `List/find(cells, field: address, value: target)` is on an indexed path for
  the current Cells formula/selection reads.
- `cells.value` and `cells.error` are demand-current rather than startup-eager.
- Selection clicks can update the retained visible state with no list scans, no
  root materialization, and no recomputed cell fields.

The remaining speed risk is mostly in the native input-to-present tail and in
architecture debt that can reappear on larger sheets or richer formulas:

- late app-window input can still cross an extra loop/present boundary;
- selection/formula-bar updates still rely on an input-overlay render-scene patch
  rather than a first-class retained render-scene mutation;
- `List/chunk` still has full-list materialization debt below the windowed
  summary layer;
- full summary APIs can still pull more currentness work than a hot interaction
  should need.

## External Patterns To Copy Carefully

- AG Grid and Handsontable virtualize rows and columns and render only viewport
  plus buffer. Boon should keep logical cells separate from materialized visible
  rows/cells.
- Glide Data Grid keeps cell data external, fetches visible cells lazily, and
  treats editing as a bounded callback/update path. Boon should separate edit
  session state from committed runtime graph updates.
- HyperFormula separates parsing, dependency graph maintenance, and evaluation,
  and optimizes range dependencies instead of expanding every range into
  quadratic cell edges.
- LibreOffice Calc stores cells in typed columnar blocks rather than allocating a
  heavyweight object for every coordinate.
- Rust GUI/rendering stacks split differently: `egui` is immediate-mode and can
  be useful for tools, while Xilem/Masonry and Vello are closer to retained UI
  and retained vector-rendering goals.

References:

- https://www.ag-grid.com/javascript-data-grid/dom-virtualisation/
- https://handsontable.com/docs/angular-data-grid/row-virtualization/
- https://github.com/glideapps/glide-data-grid
- https://hyperformula.handsontable.com/docs/guide/dependency-graph.html
- https://kohei.us/2019/12/12/benchmark-results-on-mdds-multi_type_vector/
- https://docs.rs/egui/latest/egui/
- https://github.com/linebender/xilem
- https://docs.rs/vello

## Option A: Direct Input Scheduling

Goal: remove remaining input wake / poll / present boundary waste.

Implementation shape:

- resample immediately when app-window input arrives after the first loop sample;
- avoid dropping a nearly-ready frame into a full extra loop unless the stale
  frame would actually be presented;
- make real app-window input wake the required hot path and keep passive polling
  as idle safety only;
- for source-input updates that already produced a visible dirty state, defer
  nonessential cursor/accessibility/caret telemetry to a later timer frame.

Best use: near-term p95/max latency stability.

Risk: duplicate/lost edge handling if input cursor acceptance is wrong.

## Option B: First-Class Retained Render Scene

Goal: make selection/formula-bar changes mutate retained render state directly.

Implementation shape:

- store the current `RenderScene` plus revision/hash in preview shared state;
- apply `RenderScenePatch` at source-event time;
- have the render hook encode the retained patched scene instead of lowering
  layout/frame state into a scene again;
- report dirty node ids, operation kinds, upload bytes, and retained chunk hits.

Best use: reducing render-hook work and making retained-render proof semantic,
not only metadata/probe based.

Risk: stale scene/base hash handling and text cache invalidation.

## Option C: Sparse List/Chunk Runtime

Goal: make `List/chunk(cells, size: 26, ...)` demand-windowed below summaries.

Implementation shape:

- keep logical row count and row identity without materializing every chunk row;
- derive visible chunks from layout demand ranges;
- materialize selected/dependent rows plus visible overscan only;
- keep formula dependency graph independent of render materialization.

Best use: larger-than-2600 grids and richer formulas.

Risk: stale hidden row fields if currentness barriers miss a demand read.

## Option D: Edit Session Overlay

Goal: keep active selection, formula-bar text, caret, and in-cell editing out of
the committed runtime graph until a source binding explicitly commits it.

Implementation shape:

- host/document edit session owns selected address, editing text, caret, range
  selection, hover, focus ring, and drag handles;
- formula bar and in-cell editor share that session;
- typing updates overlay state and bounded parse diagnostics;
- commit/cancel emits a normal source event batch.

Best use: spreadsheet-like feel and lower runtime pressure.

Risk: must preserve Boon semantics for apps that intentionally bind selection or
editing state as source data.

## Implemented Slice: Post-Turn Summary Reuse

The simple source-click path now reuses the state summary already produced by
`preview_apply_live_events_internal` to refresh the selection proxy focused
text. This removes the duplicate selected-input runtime read after live-event
apply while preserving retained-state and runtime fallbacks.

The visible-click verifier now also reports and gates a runtime-work contract:
selection/formula-bar clicks must not perform list scans, root materialization,
or recomputed cell fields.
