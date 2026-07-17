# NovyWave Boon Rewrite Plan

Status: implementation in progress

## Implementation Checkpoint (2026-07-17)

Implemented generically in the engine and native host:

- bounded `File/read_stream()` with invocation ownership, terminal cleanup,
  cancellation, retained `ContentRef` output, and bounded host polling;
- a thin official `wellen` adapter for VCD, FST, and GHW;
- typed `Wellen/open()`, `Wellen/hierarchy_page()`,
  `Wellen/signal_page()`, and `Wellen/cursor_values()` calls;
- committed real VCD, FST, and GHW package assets;
- a native integration path that reads the packaged VCD, opens it through
  `wellen`, requests bounded hierarchy/signal/cursor pages, and exposes the
  resulting format and status through retained NovyWave UI fields;
- generic parser, typechecker, plan, and executor support for multiline calls
  in `WHEN` arms, state-triggered effect chains, and tagged structural values.

Still required before NovyWave is complete:

- app-owned native visual scenarios that prove real VCD, FST, and GHW data in
  the rendered waveform surface, not only the inspector/status fields;
- replacement/cancellation and bounded-backpressure scenarios through the
  complete native application path;
- removal of the remaining bootstrap waveform-page data after the real page
  results drive the same signal-row and trace model.

## Goal

Rewrite NovyWave as a Boon-first application, starting as a new multi-file
playground example in this repository:

```text
examples/novywave/
```

The first version is not a replacement for the existing NovyWave product. It is
a Boon playground example that proves the language, runtime, visual tooling, and
physical material system can express a serious waveform viewer.

The target experience is NovyWave shaped by the physical/PBR-style scene system
used by `todo_mvc_physical`: dense technical UI, high-contrast waveform data,
dark and light modes, and physical materials for panels, traces, cursor,
markers, controls, and workspace surfaces.

## Hard Boundaries

This is a clean Boon rewrite, not a Rust or Tauri port.

- Do not port custom Rust code from `/home/martinkavik/repos/NovyWave`.
- Do not copy or translate NovyWave frontend, backend, Tauri, Actor+Relay,
  Fast2D, plugin-host, config, file-management, or timeline implementation code.
- Do not port Tauri-native commands, updater logic, windowing code, packaging
  code, or desktop shell integration.
- Do not make Boon values depend on Rust object identity, native handles,
  resource-table indexes, file descriptors, parser objects, cache keys, Tauri
  commands, or process-local sessions.
- Do not fork external Rust crates unless a later explicit plan approves the
  fork and explains why an official upstream dependency cannot work.

The existing NovyWave repository may be used as reference material only:

- product behavior and workflows;
- screenshots, mockups, and docs;
- test waveform files;
- public file-format expectations;
- user-facing keyboard shortcuts and terminology.

The Boon implementation must be new Boon code. The Rust side must be new, thin
bridge code around official external libraries and host capabilities.

## Ownership Split

Boon should own as much logic as possible. This project exists partly to exercise
Boon expressivity, runtime speed, visual inspection, and physical UI tooling.

Boon owns:

- application state and state transitions;
- selected files, selected signals, groups, markers, row sizes, formats, and
  workspace-shaped data;
- scope tree filtering, signal search, sorting, selection, grouping, and
  disambiguation;
- timeline viewport math, cursor movement, pan/zoom behavior, request planning,
  stale-response rejection, and request fingerprints;
- signal value formatting choices and UI-facing formatted values when raw data
  is sufficient;
- render intent for digital traces, analog traces, gaps, unknown values, cursor,
  markers, grid lines, labels, and hover states;
- all layout, theme, material, interaction, empty-state, loading-state, and
  error-state decisions;
- scenario behavior and deterministic example fixtures.

Rust owns only the parts that cannot reasonably be Boon-owned in the first
playground versions:

- calling official external libraries such as `wellen`;
- filesystem and host capability access;
- file watching when that phase is reached;
- parsing waveform files and extracting bounded data pages;
- optional background threading needed by parser libraries;
- schema validation at the Boon/Rust boundary;
- diagnostics for bridge/library failures.

Rust must not decide application layout, styling, user workflow, selected signal
state, request policy, cache policy visible to Boon, or product behavior beyond
returning canonical data requested by Boon.

## External Rust Libraries

The first bridge target is `wellen` for VCD, FST, and GHW waveform parsing.

Dependency policy:

- Prefer official crates.io releases.
- If crates.io cannot supply the needed capability, use the official upstream
  repository directly and pin a tag or revision.
- Do not vendor or fork external libraries as a shortcut.
- Keep all bridge APIs stable, typed, and versioned so the Boon example does not
  depend on crate internals.

The bridge module name should be a Boon-facing contract such as `wellen.v1`. It
is not a permission for Boon code to hold a `wellen::Hierarchy`,
`wellen::SignalSource`, or any other Rust object.

## Playground Shape

The example should follow the multi-file pattern already used by Cells and
`todo_mvc_physical`.

Planned source layout:

```text
examples/novywave/
  BUILD.bn
  RUN.bn
  Bridge/
    Wellen.bn
    Files.bn
  Generated/
    FixtureWaveforms.bn
    Assets.bn
  Model/
    File.bn
    Scope.bn
    Signal.bn
    Selection.bn
    Timeline.bn
    Marker.bn
    Workspace.bn
  Theme/
    Material.bn
    Dark.bn
    Light.bn
  View/
    App.bn
    Toolbar.bn
    FilePanel.bn
    ScopeTree.bn
    SignalTable.bn
    Timeline.bn
    WaveformRows.bn
    Inspector.bn
    EmptyState.bn
  assets/
    ...
```

The manifest entry should be planned with:

- `id = "novywave"`;
- `label = "NovyWave"`;
- `source = "examples/novywave/RUN.bn"`;
- explicit `source_files`, `build_files`, and `asset_files`;
- a scenario file such as `examples/novywave.scn`;
- a budget file such as `examples/novywave.budget.toml`;
- visual artifacts for preview framebuffer, dev framebuffer, material crops,
  waveform crops, fixture comparison crops, and dark/light mode crops.

Generated bounded fixtures are allowed only as bootstrap data for deterministic
model and renderer development. Fixture-backed waveform data is not completion
evidence for NovyWave.

NovyWave is complete only when committed, small, real VCD, FST, and GHW test
files are selected and read through the generic host file/stream contract, are
parsed by the official `wellen` library, and drive the same Boon-owned model and
view used by the playground. Functional scenarios and app-owned visual proof
must exercise those real files. Filename branches, example-specific host
metadata, predecoded fixture substitutions, and fixture-only acceptance are
forbidden.

The Boon source must use ordinary expressions, `SOURCE`, `HOLD`, `THEN`,
`WHEN`, and `WHILE` with registered typed host calls. It must not introduce a
top-level effect declaration block, manually route result variants to synthetic
sources, or use any other syntax that is not part of the language. If the
generic streaming contract is not implemented yet, implement that engine
contract first rather than inventing NovyWave syntax.

Every active file stream is owned by the dataflow invocation that created it.
EOF, success, failure, timeout, and cancellation close the host resource
automatically. Replacing or removing the producing expression, including a
`WHILE` branch transition, cancels and drops the superseded stream. The host
must bound outstanding chunks, drain only within an explicit bound, and use
RAII cleanup so abandoned readers cannot retain file descriptors, buffers, or
workers.

## Boon Data Model

The plan should define Boon-owned values for the app surface. Names may change
during implementation, but the ownership should not.

Core records:

- `WaveformFile`: stable structural file identity descriptor, display name,
  format, time bounds, loading state, diagnostics.
- `ScopeNode`: id, name, full path, child scopes, visible state, signal summary.
- `Signal`: id, scope path, name, type, width, analog metadata.
- `SelectedSignal`: signal reference, display format, row height, group id,
  analog limits, visibility.
- `SignalGroup`: id, name, collapsed state, ordered selected signal ids.
- `TimelineViewport`: visible time range, cursor time, zoom level, pan origin,
  grid density, selection window.
- `SignalPageRequest`: canonical request generated by Boon from viewport and
  selected signals.
- `SignalPage`: bounded transitions and value ranges returned by a fixture or
  bridge.
- `CursorSnapshot`: cursor-time values for visible selected signals.
- `Marker`: id, time, label, color/material role.
- `WorkspaceState`: selected files, panel sizes, theme mode, recent paths,
  persisted user-facing preferences.
- `BridgeDiagnostic`: library error, file error, unsupported format, timeout,
  stale response, schema mismatch, or permission failure.

Data rules:

- All Boon-visible values are serializable and structurally comparable.
- Large waveform data is represented as bounded pages, summaries, and
  descriptors, not as unbounded full-file data.
- Stable ids are derived from canonical data such as file digest, path snapshot,
  scope path, signal name, format, and request window. They are not Rust handles.
- Boon request fingerprints are structural values so stale bridge results can be
  rejected by normal Boon logic.

## Bridge Contract

The Wellen bridge exposes a small effect surface:

```text
module: wellen.v1

open(OpenWaveformRequest) -> WaveformOpened
hierarchy_page(HierarchyPageRequest) -> HierarchyPage
signal_page(SignalPageRequest) -> SignalPage
cursor_values(CursorValuesRequest) -> CursorSnapshot
```

`WaveformOpened` carries bounded file statistics such as byte length, time
bounds, scope count, signal count, timescale, and provider. There is no separate
`file_stats` operation or fixture-only statistics path.

Allowed bridge responsibilities:

- open a waveform source;
- parse or index with `wellen`;
- keep Rust-side parser/cache state invisible to Boon;
- answer bounded page requests;
- downsample only when explicitly requested by Boon request data;
- return raw transitions, ranges, statistics, and diagnostics.

Forbidden bridge responsibilities:

- selecting signals;
- choosing viewport windows;
- deciding row heights;
- planning cache policy exposed as app behavior;
- formatting UI labels beyond raw library facts;
- styling waveform rows or controls;
- handling keyboard shortcuts;
- implementing NovyWave workflow logic copied from Rust.

If host filesystem or watcher support is added later, it should be a separate
thin bridge contract that returns pure path descriptors, directory pages, watcher
events, and diagnostics. It must not recreate Tauri command surfaces.

## Physical Visual Direction

The UI should be recognizably NovyWave, but rendered through Boon physical
materials rather than the current Rust/Web UI stack.

The material contract should be domain-specific:

- `AppBackground`;
- `PanelSurface`;
- `PanelInset`;
- `TimelineGlass`;
- `WaveformGrid`;
- `DigitalTraceHigh`;
- `DigitalTraceLow`;
- `DigitalTraceUnknown`;
- `AnalogTrace`;
- `CursorGlow`;
- `MarkerChip`;
- `SelectedRow`;
- `HoverControl`;
- `PressedControl`;
- `Warning`;
- `Error`.

The visual system should exercise:

- `scene: Scene/new(...)`;
- depth, relief, gloss, metal, glow, material tint, borders, rounded corners,
  movement, and scene lights;
- dense but readable technical layout;
- dark and light modes;
- glassy timeline overlays where useful, especially cursor and marker layers;
- physical separation between panels without wasting space;
- readable text and clear waveform contrast at 1440x1024 and 1920x1080 style
  targets.

The plan should use NovyWave screenshots and design mockups as reference
fixtures, while allowing the Boon version to improve the visual language through
physical materials.

## Implementation Phases

### Phase 1: Plan And Fixture Inventory

- Create this plan as the durable source of truth.
- Inventory NovyWave reference screenshots, design mockups, and small test
  waveform files.
- Choose the first static fixture set from small VCD/FST/GHW files.
- Define generated fixture page shapes that exercise digital traces, analog
  traces, hierarchy, search, groups, markers, empty state, and error state.

### Phase 2: Static Multi-File Playground Example

- Add `examples/novywave/` with `RUN.bn`, `BUILD.bn`, generated fixtures, model
  modules, view modules, and theme modules.
- Add the manifest entry, scenario, budget, and visual artifact definitions.
- Render the application shell entirely from generated fixture data.
- No live Wellen bridge is required in this phase.

### Phase 3: Boon-Owned Model And Interaction

- Implement Boon state transitions for file selection, scope expansion, signal
  selection, groups, markers, row sizes, formats, cursor, pan, zoom, and search.
- Generate structural request fingerprints in Boon.
- Reject stale fixture responses through Boon logic.
- Keep all behavior deterministic for scenario playback.

### Phase 4: Physical/PBR Rendering

- Build the NovyWave material module on top of the physical scene API used by
  `todo_mvc_physical`.
- Render panels, timeline, waveform rows, grid, traces, cursor, markers, toolbar,
  tree, selected-signal table, hover states, focus states, and empty/error states.
- Verify dark and light modes visually through app-owned readback artifacts.

### Phase 5: Thin Wellen Bridge

- Add a new bridge adapter around official `wellen`.
- Expose only the small `wellen.v1` effect contract.
- Return bounded hierarchy pages, signal pages, cursor values, statistics, and
  diagnostics.
- Keep request planning, stale response handling, visible range selection,
  formatting policy, and UI behavior in Boon.

### Phase 6: Larger Product Behaviors

- Add multi-file comparison.
- Add analog trace rendering and auto-limit behavior in Boon where raw pages make
  it practical.
- Add persistence-shaped Boon data for workspace state without depending on
  NovyWave Rust config code.
- Add file watcher and plugin event models as pure data contracts, not Tauri or
  NovyWave backend ports.

### Phase 7: App Extraction Plan

- After the playground reaches useful parity, write a separate plan for turning
  the example into a standalone Boon app outside the playground.
- That later plan should cover packaging, desktop integration, updater strategy,
  bridge distribution, and migration from the current NovyWave product.

## Verification Plan

Static checks:

- Boon multi-file loading finds every listed source file.
- `BUILD.bn` generated fixture assets are deterministic.
- Manifest metadata is complete and ordered.
- Bridge schemas are versioned and hashable.
- No NovyWave custom Rust or Tauri source files are copied into the Boon example
  or bridge.

Scenario coverage:

- initial empty state;
- load generated fixture;
- expand and collapse scopes;
- search signals;
- select and remove signals;
- reorder selected rows;
- cycle formats: ASCII, binary, grouped binary, hex, octal, signed, unsigned;
- zoom and pan with keyboard and pointer routes;
- move cursor and jump to transitions;
- add, rename, and remove markers;
- create, collapse, and expand groups;
- resize signal rows;
- switch dark and light modes;
- compare two files;
- display analog traces;
- show corrupted/unsupported file diagnostics once bridge support exists.

Visual coverage:

- app-owned framebuffer before/after interaction;
- dev-window framebuffer with all example files visible;
- dark and light mode crops;
- material crops for panel depth, timeline glass, trace glow, cursor glow, marker
  chips, and pressed/hover controls;
- reference crops compared against NovyWave screenshots and design mockups;
- readability assertions for text, tree labels, signal values, and timeline
  labels;
- contrast assertions for digital high/low/unknown and analog traces.

Performance coverage:

- initial render budget;
- example switch budget;
- file tab switch budget;
- timeline pan and zoom frame budgets;
- selected-signal scroll budget;
- large hierarchy paging budget;
- bridge page latency budget;
- bounded materialization counts for visible rows and waveform segments.

Bridge coverage:

- successful VCD, FST, and GHW open through official `wellen`;
- committed real VCD, FST, and GHW files, not generated decoded waveform
  fixtures, drive functional and visual scenarios;
- corrupted and unsupported files return diagnostics;
- huge files return bounded pages without copying whole data into Boon;
- stale responses are structurally rejected by Boon;
- bridge results do not expose Rust handles or process-local ids;
- repeated equal requests produce comparable equal Boon-visible results when
  source data is unchanged.
- stream completion closes the reader, replacement cancels the previous reader,
  a `WHILE` branch transition drops the inactive reader, and cancellation under
  backpressure leaves no live host resource.

## Acceptance Criteria For The Plan Itself

This plan is complete when it makes the following constraints explicit:

- NovyWave is rewritten as Boon-owned app logic, not ported Rust logic.
- No custom NovyWave Rust code or Tauri-native code is copied or translated.
- Official external Rust libraries are allowed only behind thin bridge
  contracts.
- The first deliverable is a playground multi-file example.
- Physical/PBR styling is based on the material API exercised by
  `todo_mvc_physical`.
- Verification includes visual, scenario, performance, bridge, and clean-room
  checks.
- Real-file VCD, FST, and GHW scenarios pass without invented declaration
  syntax or example-specific host paths.
