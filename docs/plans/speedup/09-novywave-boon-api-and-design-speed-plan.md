# NovyWave Boon API And Design Speed Plan

Date: 2026-06-12

## Purpose

This plan turns the earlier speedup inventories into an implementation sequence
for the Boon source, Boon API, and eventual Boon design changes needed to make
NovyWave correct, reliable, and fast. The same work must also make smaller
examples better proof cases, because a NovyWave-only fix would be too easy to
hide as an example-specific shortcut.

This is not a performance measurement report and it does not claim that the
current implementation is fast. It is a step-by-step plan for changing the
shape of the code so that later Rust engine, runtime, layout, and renderer
optimizations have a clean target.

Hard boundaries:

- Do not add Boon-level workarounds for real compiler, typechecker, runtime,
  driver, bridge, or renderer bugs as the final implementation.
- Do not weaken native GPU contracts, reports, budgets, or negative checks to
  make NovyWave look ready.
- Do not make NovyWave special inside the runtime, BoonDriver, native preview,
  document model, layout, or renderer.
- Do not claim live Wellen parsing, real file-open behavior, real-window
  evidence, or human observation until those paths exist and are proven by the
  right reports.
- Keep waveform data out of broad Boon app state. Boon should hold typed
  descriptors, selected IDs, view state, and bounded pages, not full waveform
  payloads.

## Target Shape

The target path is:

```text
operator/user intent
-> typed source payload
-> app transition over normalized state
-> inferred/dependency-aware selectors and indexes
-> typed bridge/effect request, when external data is needed
-> bounded PageRef/BlobRef/ArtifactRef completions
-> virtual viewport rows and pages
-> document/layout materialization
-> renderer primitives and bounded GPU uploads
-> BoonDriver/scenario evidence
```

The source should read like the app model, not like renderer glue. The engine
should understand the app's identities and invalidation points, not rediscover
them through text labels, source path heuristics, or list scans.

## Current Anchors

Use these documents as the working context before changing code:

- `docs/plans/NOVYWAVE_BOON_REWRITE_PLAN.md`
- `docs/plans/NOVYWAVE_INTERACTION_SPEED_INVESTIGATION_PLAN.md`
- `docs/plans/speedup/01-inspiration.md`
- `docs/plans/speedup/05-rust-wgpu-performance-measurement.md`
- `docs/plans/speedup/06-human-like-scenario-testing.md`
- `docs/plans/speedup/07-novywave-boon-code-smells.md`
- `docs/plans/speedup/08-repo-code-smell-risk-inventory.md`
- `docs/architecture/BOON_RUST_BRIDGE.md`
- `docs/architecture/BOON_DRIVER.md`
- `docs/architecture/NATIVE_GPU_PIPELINE.md`

Important current source pressure points:

- `examples/novywave/View/NovyView.bn` is the large view hotspot and currently
  mixes rows, labels, layout intent, event capture, hover state, and wave
  drawing logic.
- `examples/novywave/RUN.bn` still carries too much model, bridge descriptor,
  request identity, debug, and scenario-shaped state in one place.
- `examples/novywave/RUN.bn` has structural request keys assembled with
  `Text/concat`, compact page/request labels, descriptor strings parsed back
  into fields, and per-signal branches that cannot scale to arbitrary files.
- `examples/novywave/View/NovyView.bn` filters waveform segments inside row
  view code, which is the wrong shape for a virtualized waveform renderer.
- `examples/novywave/Bridge/NovyBridge.bn` is descriptive bridge source, not a
  live Wellen host bridge.
- `examples/cells` is the best small proof case for typed source payloads,
  visible-range materialization, selectors, and scroll speed.
- `examples/cells/store.bn` and `examples/cells/view.bn` currently make Cells a
  useful proof case for address routing and full-grid materialization risks.
- `examples/todomvc.bn` is the clean proof case for list row identity, edit
  state, filters, and stale row events.
- `examples/todo_mvc_physical` is the proof case for route values, style
  tokens, generated assets, and a larger app shell.
- `examples/counter.bn` should remain the tiny smoke test for
  `SOURCE`/`HOLD`/`LATEST`/`THEN` regressions.

## Prerequisite Phase: Guard Engine Correctness And Evidence Integrity

Goal: fix or fence the current engine and proof shortcuts that would make later
source/API work impossible to trust.

This phase does not need to make NovyWave fast. It must make failures visible
before scenario reports become acceptance evidence.

Steps:

1. Add failing checks or targeted fixes for row-source unbind integrity. A
   removed or remapped row must not leave an old source binding able to mutate
   a new row.
2. Add negative tests for TodoMVC-shaped runtime behavior. The runtime must
   route by generic source identity, payload schema, row binding, and generation
   rather than recognizing TodoMVC names or paths.
3. Make document patch failures observable. Missing patch targets, ignored
   updates, and stale patch application must report structured failure instead
   of silently producing a plausible frame.
4. Inventory source-routing recognizers and dynamic fallback paths. Each
   readiness path should either remove the recognizer, put it behind a legacy
   gate, or count it in the report as a non-ready fallback.
5. Add metrics for linear row/list scans that remain in hot paths:
   rows scanned, rows touched, selector recomputes, route candidates visited,
   and visible rows materialized.
6. Fix scenario integrity before using scenario evidence. Current known issues
   include duplicate NovyWave `select-primary-file` entries in scenario/manifest
   data, Cells manifest scroll entries that need executable `.scn` steps or
   explicit generated-probe provenance, and TodoMVC manifest
   `reject-empty-todo` entries that must be reconciled with the split
   `reject-empty-todo-type` / `reject-empty-todo-submit` scenario steps.
7. Keep manifest evidence tiers honest. NovyWave currently has
   `required_evidence_tier = "host-synthetic"` while human testing is marked as
   needed; automated reports must not upgrade that to `real-window` or `human`.
8. Implement `verify-scenario-manifest-integrity` before treating scenario
   reports as acceptance evidence. It is expected to fail at first until the
   known inventory issues above are fixed or explicitly classified as generated
   or phased probes.
9. Make document patch failures observable before document materialization work.
   The target shape is `DocumentState::apply_patch ->
   Result<PatchApplyReport, PatchApplyError>` or an equivalent structured
   result, not silent `()` success when patch targets are missing.

Acceptance:

- Existing reports fail when source/scenario/fixture/budget hashes are stale or
  contradictory.
- Runtime and document failures cannot be hidden by a successful-looking visual
  frame.
- Each known app-specific runtime shortcut has a negative test, a report field,
  or an explicit legacy-only classification.
- Later source cleanup phases can rely on scenario output without confusing
  model-only proof for user-visible proof.

## Phase 0: Lock The Baseline And Vocabulary

Goal: make sure future work changes the right system, with names that can be
shared across NovyWave, Cells, TodoMVC, BoonDriver, and the engine.

Steps:

1. Record the current source inventory for examples that will be touched:
   NovyWave, Cells, TodoMVC, TodoMVC Physical, and Counter.
2. Record the current scenario inventory and fail on obvious integrity bugs
   before using scenarios as evidence.
3. Decide the canonical names for domain IDs and view state:
   `ArtifactRef`, `WaveformRef`, `ScopeId`, `SignalId`, `PageRef`, `BlobRef`,
   `WaveRequestKey`, `Viewport`, `TimeWindow`, `Cursor`, `Marker`,
   `ValueFormat`, and `VisibleRange`.
4. Mark each name as one of:
   source-only shim, existing engine support, engine feature needed, or bridge
   feature needed.
5. Add a short design note near the source or in a follow-up plan explaining
   which names are ordinary Boon structural values/tags today and which
   inferred schemas the compiler/runtime must understand later. This does not
   require new record or variant declaration syntax.

Acceptance:

- Each domain fact has one owner in the app model.
- Labels, compact debug strings, and display text are projections only.
- No request identity, page identity, or row identity depends on
  `Text/concat`.
- Scenario reports do not claim stronger evidence tiers than the manifest
  requires.

## Phase 1: Normalize NovyWave State Without New Language Features

Goal: reshape NovyWave source so the current engine can still run it, while the
source exposes the model that a faster engine should eventually optimize.

Steps:

1. Split `examples/novywave/RUN.bn` conceptually, and then physically where the
   current module system allows it, into:
   app state, fixture data, bridge request/response shims, selectors,
   transition helpers, view model helpers, and debug/report projections.
2. Create one canonical `active_file` record. It should own the selected
   artifact, active waveform reference, parser/status summary, and selected
   page generation.
3. Create one canonical `active_selection` record. It should own selected
   scope, selected signal IDs, cursor, markers, current format, and focused row
   or lane.
4. Create one canonical `viewport` record. It should own horizontal time
   window, vertical visible rows, lane height, scroll offset, zoom level, and
   overscan policy.
5. Create one canonical `bridge_request` record. It should own request kind,
   artifact reference, schema version, page/window parameters, generation, and
   deterministic input digest.
6. Replace duplicate strings and compact labels with projection helpers such as
   `active_file_label`, `bridge_request_label`, `visible_window_label`, and
   `debug_status_label`.
7. Wrap current scans behind selector-shaped helpers even if those helpers
   still scan internally at first:
   `metadata_by_file_id`, `signal_by_id`, `scope_by_id`,
   `selected_row_ids`, `segments_by_signal_window`,
   `cursor_value_by_signal`, and `visible_rows_for_viewport`.
8. Move fixture waveform data behind page-shaped records. The initial data may
   still be hardcoded, but the app should consume it as bounded pages with page
   IDs, schema, digest, row count, transition count, and payload byte count.
9. Introduce payload-shaped transition helpers for signal actions:
   `SelectSignal(SignalId)`, `RemoveSignal(SignalId)`,
   `SetSignalFormat(SignalId, ValueFormat)`, `FocusSignal(SignalId)`, and
   `MoveSignal(SignalId, position)`.
10. Where the current source/event system cannot express those commands yet,
    keep the existing per-signal compatibility branches behind one clearly
    named adapter and mark them as typed source payload and row identity
    blockers.
11. Keep debug and scenario proof summaries out of hot model state. They should
    be generated from canonical records only when the dev window, scenario
    report, or diagnostics panel asks for them.

Acceptance:

- Early Phase 1 does not claim arbitrary dynamic-signal support. That acceptance
  depends on typed source payloads and row identity from later engine/API
  phases.
- The only remaining per-signal source branches are isolated in one adapter,
  listed as blockers, and covered by negative tests or report fields so they
  cannot be mistaken for the final design.
- Changing a display label does not change selection, page identity, request
  identity, stale-response detection, or row identity.
- The hot path for cursor movement, hover, pan, zoom, selection, and row focus
  is expressed in terms of IDs, pages, and selectors, not text descriptors.
- Any remaining linear scan has a named selector wrapper and a future metric
  target, so the later engine can replace it with an index without changing
  app semantics.

## Phase 2: Make Cells The First Shared Proof Case

Goal: prove the shared source/API direction on a smaller model before forcing
all of it through NovyWave.

Why Cells first:

- It has a logical 26 x 100 grid.
- It already pressures address-based selection and formula evaluation.
- It can expose whether the renderer is drawing a real large model or a
  shrunken fake grid.
- It is small enough to refactor while still being a serious virtualized
  collection test.

Steps:

1. Introduce typed-ish source payload records for cell address, edit text, key
   input, focus, blur, and selection.
2. Make cell identity independent of visible text and row/column labels.
3. Move formula parsing and evaluation into helper modules with clear AST-like
   records instead of view-shaped string handling.
4. Add selector-shaped helpers for cell by address, visible rows, visible
   columns, selected cell, edited cell, and formula dependencies.
5. Change the view to consume a visible range, not the full logical grid.
6. Keep the scenario/report proof honest: reports must show the logical grid
   size and the materialized visible range separately.

Acceptance:

- Scenarios prove edits, formula fanout, selection movement, and scroll over
  the logical 2600-cell grid.
- Materialized cell count is bounded by visible rows/columns plus overscan.
- A renamed row/column label or source reformat does not break selection.
- Cells uses the same future API vocabulary as NovyWave: typed source payloads,
  selectors, visible ranges, and virtual materialization.

## Phase 3: Clean TodoMVC And TodoMVC Physical As API Probes

Goal: remove example-specific assumptions from the runtime by making the small
apps demand generic row routing, state transitions, route values, style tokens,
and asset descriptors.

Flat TodoMVC steps:

1. Name typed actions for add, edit start, edit change, edit commit, edit
   cancel, toggle, remove, clear completed, and filter selection.
2. Make row identity explicit and stable across duplicate titles, edits,
   filters, and removals.
3. Express edit state as a small state machine: idle, editing draft, committing,
   canceling, and blurred.
4. Make visible todos a selector over canonical todos and the active filter.
5. Keep row-local sources tied to row binding identity and generation, not UI
   text.

Flat TodoMVC acceptance:

- Duplicate todo titles work.
- Stale row events after remove/filter do not mutate a new row.
- Edit commit, blur, cancel, clear, and re-add are covered by scenarios.
- Runtime code does not contain TodoMVC-shaped special cases.

TodoMVC Physical steps:

1. Replace route strings with typed route values or source-level route records.
2. Reuse the shared Todo row/edit model instead of carrying a different app
   state shape for the physical shell.
3. Define style token records for colors, spacing, typography, border, shadow,
   motion, and state variants.
4. Make generated assets referenced through typed asset descriptors, not
   untracked SVG/data-url strings.
5. Treat theme switching as a style-token update and renderer invalidation
   case, not a full app rebuild case.

TodoMVC Physical acceptance:

- Route changes are typed state changes.
- Style changes produce bounded document/render invalidation.
- Generated assets have digest identities and are actually referenced through
  the app model.
- The app remains a proof case for larger UI shell behavior, not a separate
  runtime path.

Counter steps:

1. Keep Counter minimal.
2. Canonicalize event/source spelling to the same terminology used elsewhere.
3. Use Counter only to catch regressions in previous-state updates and simple
   multi-step scenarios.

Counter acceptance:

- Increment, decrement, reset, and repeated previous-state updates remain
  obvious and fast.
- No Counter-specific runtime path exists.

## Phase 4: Add The Boon API Surface Needed By The Source

Goal: define the language/API shape that makes the source cleanup real instead
of cosmetic.

Do not start by inventing a high-level reducer framework or new surface syntax.
Current Boon already has useful `SOURCE`/`HOLD`/`LATEST`/`WHEN` patterns,
structural records, tags, tagged objects, functions, and document elements. The
immediate need is to make identities, payloads, records, selectors,
materialization, and external effects explicit enough in the compiler/runtime
that the engine can reason about them.

Syntax stance:

- Do not add required `record` or `variant` declaration syntax. Existing Boon
  structural records, tags, and tagged objects are the user-facing data model.
- Do not require `SOURCE<T>` syntax in normal code. Source payload types should
  be inferred from usage, host control contracts, and bridge schemas.
- Do not add `INDEX` or `SELECTOR` syntax. Incrementality should be inferred
  from normal functions, list operations, field access, stable IDs, and source
  dependencies.
- Do not add syntax for virtual collections. Use generic components/elements
  expressed with existing function/object syntax.
- Treat bridge imports as the one planned source-facing import surface in this
  plan. Current multi-file examples are loaded through manifest/project
  `source_files` and slash-qualified functions; `IMPORT wellen.v1 AS Wave` is
  the already-designed external bridge surface and must be implemented
  generically rather than as a NovyWave shortcut.

Needed API/design work:

1. Structural tagged records and variants.
   Use existing record, tag, and tagged-object values such as
   `SignalId[id: ...]` and `SelectSignal[signal: ...]`. The runtime/compiler
   work is schema inference, schema export, schema hashes, canonical encoding,
   field compatibility checks, and exhaustive matching/branch diagnostics, not
   a new declaration syntax.
2. Inferred source payload types.
   Infer source payload shape from source usage, host element/control
   contracts, and bridge completion schemas so routes carry address, key, text,
   pointer, focus, row binding, and app command data without event-name
   heuristics. If the compiler cannot infer a single correct payload shape, it
   should produce a compiler error with concrete restructuring advice; normal
   users should not be forced to add manual type annotations to resolve
   ambiguity.
3. Source batches and host source intents.
   Define a public `SourceIntent` path from host input through BoonDriver and
   native windows into runtime dispatch. Generate source batches from typed IR,
   not source text path normalization.
4. Stable row and scope identities.
   Define row binding identity, occurrence ID, generation, row key, and stale
   event rejection as generic runtime concepts.
5. Selectors and indexes.
   Infer incremental selectors and indexes from ordinary Boon functions, list
   operations, field access, stable IDs, and source dependencies. The compiler
   should build or request the needed runtime indexes where possible. When it
   cannot prove a safe incremental plan, or finds ambiguous identity/dependency
   meaning, it should produce a compiler error with specific tips such as
   adding a stable ID field, avoiding label identity, or splitting a derived
   query into a named helper. Do not add user-facing `INDEX`/`SELECTOR` syntax.
6. Virtual lists, virtual grids, and page windows.
   Provide generic virtual collection components/elements through existing
   function/object syntax. Define the runtime/document protocol for logical
   collections, visible ranges, overscan, materialization requests, passive
   scroll, and page refs. The component must be reusable for NovyWave, Cells,
   TodoMVC-like lists, file browsers, log viewers, tables, and future examples;
   it must not encode example-specific behavior.
7. Typed style and material tokens.
   Promote performance-relevant style data out of arbitrary string maps:
   color, typography, border, shadow, spacing, layout hints, pseudo-state, and
   renderer primitive hints.
8. Typed assets and blobs.
   Define `AssetRef`, `BlobRef`, `ArtifactRef`, descriptors, digests, byte
   lengths, decode status, upload status, cache policy, and diagnostics.
9. Bridge/effect imports.
   Support static imports like `IMPORT wellen.v1 AS Wave`, bridge schemas,
   capabilities, pure/effect separation, request metadata, completion payloads,
   cancellation, deduplication, replay, and stale completion rejection.
10. Project/build bridge metadata.
    Define the practical contract that makes a bridge import executable:
    `Boon.toml`, generated runner metadata, bridge registry entries,
    SDK/version compatibility checks, Cargo lock or equivalent dependency
    fingerprints, capability grants, and diagnostic output when any part of the
    bridge build contract is missing or stale.

Source spelling examples:

```boon
selected_signal_action:
    SelectSignal[signal: SignalId[id: signal.id]]

FUNCTION segments_for_signal_window(signal_id, window) {
    waveform_segment_records
        |> List/filter_field_equal(field: TEXT { signal_id }, value: signal_id)
        |> List/filter_overlap(field: TEXT { time_window }, window: window)
}

waveform_rows:
    Virtual/list(
        items: selected_signal_rows
        key: FUNCTION(row) { row.signal_id }
        viewport: waveform_viewport
        overscan: [before: 4, after: 8]
        row: FUNCTION(row) {
            signal_lane_row(row: row)
        }
    )

logo_asset:
    AssetRef[
        digest: TEXT { sha256:... }
        media_type: TEXT { image/svg+xml }
        inline_text: NoInlineText
        blob: BlobRef[digest: TEXT { sha256:... }, byte_length: 1234]
    ]
```

These examples are intended to use normal Boon values, functions, records, tags,
and tagged objects. The exact library names may change, but the implementation
must preserve the no-new-syntax stance.

Minimum indexable selector contract:

- The helper is pure from the compiler/runtime point of view: no host effects,
  time, random data, hidden mutable globals, or report-only state.
- The input collection has a discoverable stable key, such as a row key,
  address, signal ID, artifact/page ID, or generated hidden row identity.
- Dependencies are visible through function arguments, source fields, field
  access, and list operations.
- The output identity is stable enough for dirty-set comparison and renderer
  reuse.
- If the compiler cannot prove those facts, it must produce a compiler error
  with a concrete data-shape fix instead of asking users to write indexing
  syntax.

Current missing implementation dependencies:

- no complete `boon_bridge` crate or equivalent bridge SDK surface;
- no `Boon.toml` project loader for bridge-enabled apps;
- no `check-bridge` command, bridge executor, or completion-as-source path;
- no canonical schema hash/golden-vector layer for bridge and page refs;
- only partial `SourcePayloadSchema`, route table, and type fallback substrate;
- no readiness-proof replacement yet for `classify_source_event`-style runtime
  recognizers.

Acceptance:

- The source can express NovyWave without string identity or request labels.
- The runtime can route a source event without knowing the app is TodoMVC,
  Cells, or NovyWave.
- The document/layout layer can ask for visible materialization without causing
  a full app graph rebuild.
- The renderer can cache and update by typed primitives, style tokens, and page
  identities.
- Bridge completions are pure data and can be replayed as source inputs.

## Phase 5: Implement Parser, IR, Typecheck, Runtime, And Document Dependencies

Goal: make the API surface executable in the engine in the right dependency
order.

Current partial substrate:

- IR already has source payload schema concepts and runtime has route tables,
  but readiness depends on removing or fencing event-name/path recognizers from
  readiness paths.
- Typecheck has useful shape/fallback reporting, but open-object fallback must
  not hide bridge, source route, selector, or renderer contract ambiguity.
- Document model names for materialized ranges and layout demands exist, but
  patch application must report missing/stale targets before virtualization can
  be trusted.

Implementation order:

1. Stabilize parser, AST, and semantic index.
   Move policy checks out of syntax parsing where possible. Preserve source
   spans and semantic nodes needed for inferred source payloads, structural
   schemas, bridge schemas, and diagnostics.
2. Add schema-aware IR and typecheck.
   Represent structural records, tags, tagged-object variants, inferred payload
   schemas, selector keys, bridge schemas, and canonical type display in IR.
   Reject unknown/open-object shapes in readiness modes where they would hide
   routing or bridge bugs. Do this without requiring nominal record/variant
   declarations from users.
3. Replace source-routing heuristics with typed route plans.
   Runtime dispatch should be generated from typed IR using source ID, inferred
   payload schema, row binding identity, and discovered row scope. The host
   should send a source intent, not private app-specific commands.
4. Add source batch dispatch.
   Support ordered batches for events that produce multiple source updates,
   such as pointer focus plus click, edit commit plus blur, bridge completion
   plus status update, or scenario step plus expected source intent.
5. Add indexed list and selector infrastructure.
   Build typed indexes for row identity, row occurrence, text/value lookup,
   selected IDs, visible rows, and selector outputs from ordinary Boon
   functions and list operations. Expose metrics for rows scanned, rows
   touched, selector recomputes, cache hits, and dirty outputs. If an expression
   cannot be indexed safely, or has ambiguous identity/dependency meaning,
   report a compiler error that explains why and points to the required stable
   data shape instead of asking users to write indexing syntax.
6. Add document patch result reporting.
   Make document patch application return structured success/failure reports so
   missing targets, stale patch application, and ignored updates cannot produce
   plausible frames.
7. Add document materialization.
   Let layout demand visible ranges and let runtime return keyed materialized
   rows/pages. Passive scroll must not require runtime graph rebuilds when only
   layout offsets change.
8. Add typed style/document contracts.
   Replace performance-sensitive string-keyed style maps with typed/interned
   tokens and renderer primitive data where the values affect invalidation or
   batching.
9. Add asset pipeline support.
   Use digest identities, async decode/raster/upload, cache limits, diagnostics,
   and render-ready refs instead of synchronous generated-data payloads on hot
   paths.
10. Implement bridge/effects after schema, typed source, and selector work.
   Bridge requests and completions depend on canonical schemas, typed source
   completions, stable page identity, runtime replay, and app/project metadata.

Acceptance:

- The dependency chain is explicit:
  `parser/AST + semantic index -> type/schema model -> typed source routes ->
  indexed runtime/list storage -> document materialization -> renderer/style/
  assets -> bridge/effects`.
- Bridge work does not start from NovyWave-specific compact labels.
- Each engine feature has negative tests proving generic behavior and rejecting
  app-specific shortcuts.

Minimum MVP slice before Phase 6:

- inferred source payload schemas;
- row identity and generation;
- public source batch API;
- row lookup indexes and scan metrics;
- document patch failure reporting;
- readiness fencing for event-name/path recognizers.

## Phase 6: Migrate NovyWave To Page-Based Bridge Semantics

Goal: make NovyWave consume the same bridge shape that a real `wellen.v1`
module will eventually provide, while preserving deterministic fixtures until
the host bridge exists.

Steps:

1. Create a fixture adapter with the same public shape as the planned bridge:
   open result, hierarchy page, signal page, waveform page, cursor values,
   file stats, diagnostics, and status.
2. Make every fixture page carry schema version, request fingerprint, response
   fingerprint, input digest, page digest, generation, row/sample/transition
   counts, byte length, and status.
3. Model bridge states explicitly:
   idle, opening, loading page, ready, stale response, canceled, unsupported
   format, payload too large, permission/grant missing, parse failed, and
   schema mismatch.
4. Keep Rust/Wellen handles out of Boon-visible data. The Boon side may see
   `ArtifactRef`, `WaveformRef`, `PageRef`, `BlobRef`, and pure descriptors
   only.
5. Add stale-response rejection as app logic over typed generations and
   request fingerprints, then later move the common mechanics into the bridge
   runtime.
6. Add payload caps for inline data. Larger waveform samples and assets must
   use `PageRef`/`BlobRef`, not broad app graph values.
7. Add diagnostics that are generated from request/page records and never used
   as semantic identity.
8. Add project/build checks for the eventual real bridge path:
   `Boon.toml` bridge imports, registry metadata, capability grants, generated
   runner fingerprints, SDK version compatibility, dependency lock metadata,
   and clear errors for stale or missing bridge components.

Acceptance:

- Empty state, fixture open, deterministic VCD, planned GHW/FST-like page
  states, stale page rejection, payload caps, missing grants, and parse errors
  are all represented as typed statuses.
- The Boon app never receives full waveform payloads or Rust handles.
- A real `wellen.v1` implementation can replace the fixture adapter without
  changing NovyWave app transitions or view selectors.

## Phase 7: Rebuild NovyWave View Around Rows, Pages, And Virtualization

Goal: remove split-column and per-row duplicate work before relying on renderer
optimizations.

Steps:

1. Introduce one keyed `SignalLaneRow` view model for a selected signal. It
   owns row identity, signal label, current value, format, lane state, focus
   state, hit regions, and the wave page/window to draw.
2. Feed `SignalLaneRow` records into a generic virtual-list element/component
   expressed with existing Boon function/object syntax. Do not virtualize the
   current split name/value/wave maps separately.
3. Preselect wave segments by signal ID and time window before render. The
   rendered row should not filter all waveform segments per row.
4. Make cursor, hover, marker, and selection overlays consume typed row/window
   records and page refs.
5. Keep labels and current values cached as projections of typed values.
6. Keep dark/light mode and theme state as style tokens so theme changes can be
   invalidated separately from waveform data.
7. Require row alignment evidence for labels, values, lanes, cursor, and
   markers in app-owned readback crops.

Acceptance:

- Materialized rows equal visible rows plus overscan.
- Wave segment data is already scoped to the row/window/page before rendering.
- Row label, value, lane, cursor, marker, and hover overlay stay aligned during
  scroll, pan, zoom, resize, and theme changes.
- Renderer optimizations can reduce draw/upload work without depending on
  NovyWave-specific geometry.
- The virtual collection component is reusable by any large logical list or
  grid. It does not mention NovyWave signals, Cells addresses, Todo rows, or
  any other example-specific concept in its public contract.

## Phase 8: Add Scenario, Driver, And Report Gates

Goal: prove the new design through human-like automated paths without letting
AI, scripts, or app-specific shortcuts fake success.

Immediate integrity fixes to plan:

1. Fail duplicate `.scn` step IDs.
2. Fail duplicate manifest scenario references unless they are explicitly
   phased or generated probes.
3. Fail manifest labels that are not present in scenario/probe inventory.
4. Fail authored raw-coordinate selectors.
5. Fail target-text-only selectors that do not disambiguate role/control.
6. Fail input steps that do not expect a public source intent.

Current expected integrity failures:

- NovyWave duplicates `select-primary-file` in the scenario and manifest.
- Cells manifest scroll/focus labels are not executable `.scn` steps yet.
- TodoMVC manifest references `reject-empty-todo`, while `examples/todomvc.scn`
  splits that story into `reject-empty-todo-type` and
  `reject-empty-todo-submit`.

Required gates to add or extend:

The NovyWave-specific native GPU gates below are future readiness gates for
this plan. They do not, by themselves, change the current native GPU handoff
contract. Until `docs/architecture/NATIVE_GPU_PIPELINE.md` is intentionally
updated, its existing source/project payload, host-input, and app-owned
readback requirements remain the authority.

Gate status:

| Gate | Status | Aggregate status | Planned report |
| --- | --- | --- | --- |
| `verify-scenario-manifest-integrity` | new | prerequisite, expected to fail initially | `target/reports/scenario-manifest-integrity.json` |
| `verify-boon-driver-e2e --example novywave` | command exists, NovyWave coverage future | not current native aggregate | `target/reports/boon-driver-e2e-novywave.json` |
| `verify-novywave-bridge-scenario` | new | future NovyWave readiness | `target/reports/novywave-bridge-scenario.json` |
| `verify-metamorphic-hidden-fixtures` | new | future anti-shortcut aggregate input | `target/reports/metamorphic-hidden-fixtures.json` |
| `verify-native-gpu-preview-e2e --example novywave` | existing style gate to harden/add coverage | not current native aggregate until contract update | `target/reports/native-gpu/preview-e2e-novywave.json` |
| `verify-native-gpu-novywave-visual` | exists, needs hardening | future NovyWave readiness | `target/reports/native-gpu/novywave-visual.json` |
| `verify-native-gpu-novywave-interaction-speed` | exists, needs hardening | future NovyWave readiness | `target/reports/native-gpu/novywave-interaction-speed.json` |
| `verify-native-gpu-scroll-speed --example novywave` | existing generic command to extend | future NovyWave readiness | `target/reports/native-gpu/scroll-speed-novywave.json` |
| `verify-native-gpu-negative` | exists, needs NovyWave fabrications | native negative aggregate after extension | `target/reports/native-gpu/negative.json` |

New gates to add:

1. `verify-scenario-manifest-integrity`
   Check scenario IDs, manifest references, selector quality, source-intent
   expectations, evidence tier declarations, and generated probe provenance.
2. `verify-boon-driver-e2e --example novywave`
   Prove operator host input, hit/focus/scroll routing, source intent, public
   runtime dispatch, document/render patch, and app-owned WGPU readback for
   important steps.
3. `verify-novywave-bridge-scenario`
   Cover empty state, load dialog, deterministic VCD, planned GHW/FST page
   behavior, scope selection, signal search, selected-row reorder/grouping,
   format cycling, cursor/pan/zoom, stale page rejection, marker operations,
   dark/light mode, payload caps, and missing grants.
4. `verify-metamorphic-hidden-fixtures`
   Rerun core stories after legal source reformat, source path move, fixture
   ID/path changes, label and symbol renames, declaration order changes where
   legal, viewport changes, and theme changes.

Existing gates to harden:

1. `verify-native-gpu-preview-e2e --example novywave`
   Prove the preview receives source/project payload, not example names or
   scenario data. Required false fields include:
   `preview_receives_example_name`, `preview_received_scenario_data`,
   `preview_bound_scenario_data`, and `private_runtime_dispatch_used`.
2. `verify-native-gpu-novywave-visual`
   Use app-owned readbacks only. Require nonblank/non-single-color frames,
   waveform row/label alignment, visible cursor/marker, readable dark/light
   text, no large blank overlay, crop hashes, backend, adapter, surface format,
   and scale metadata. Scaffold `CopyToPresent` proof, missing acquired surface
   texture, or unbounded readback/map waits cannot satisfy this gate.
3. `verify-native-gpu-novywave-interaction-speed`
   Run release-only, warmed, with proof-mode overhead excluded. Require sample
   count, p50/p95/p99/max timings, missed frames, input-to-visible,
   hover-to-overlay, click-to-cursor, divider-drag-to-layout,
   resize-to-present, stage timings, and longest stall owner. Hot-path
   PNG/report/persist writes must be zero.
   It must also prove measurement intent: warmup policy, phase isolation, real
   divider drag route, real resize-to-present route, sample counts, and evidence
   that each metric measured the intended path rather than a replay, model-only
   update, or report-writing path.
4. `verify-native-gpu-scroll-speed --example novywave`
   Prove vertical and horizontal scroll/pan/zoom through operator host input or
   stronger. Require full source/fixture size, no timeline replay pretending to
   be native frame timing, `runtime_dispatch_count_for_passive_scroll=0`,
   `graph_rebuild_count=0`, and `preview_blocked_on_ipc_count=0`.
5. `verify-native-gpu-negative`
   Reject mutated source/scenario/fixture/budget hashes, future timestamps,
   missing artifacts, copied pixel hashes, fake real OS input, fake human
   observation, private runtime dispatch, source-event-only IPC, preview
   scenario-data leakage, full waveform payload entering Boon, model-only
   timing, debug-build speed reports, and reduced fixture sizes.
   Honesty booleans such as `preview_received_scenario_data`,
   `private_runtime_dispatch_used`, and `full_waveform_payload_entered_boon`
   must be verified by independent negative/fabricated-report cases, not
   trusted merely because the producer report says `false`.

Metamorphic fixture contract:

- Persist generator inputs and seeds in the report.
- Record source, scenario, fixture, viewport, and theme mutations separately.
- Define expected semantic invariants before running the mutated case.
- Define visual equivalence by stable app-owned crops and semantic labels, not
  by whole-frame pixel equality.
- Allowlist documentation-only strings and report-only paths that are expected
  to change.
- Fail if success depends on example names, fixture paths, label text, source
  formatting, declaration order, or scenario data entering the preview.

Required top-level report fields:

```text
status
command_argv
exit_status
generated_at_utc
git_commit
worktree_fingerprint
binary_hash
playground_binary_hash
build_profile
measurement_mode
source_path
source_files
source_hash
scenario_path
scenario_hash
scenario_labels
budget_path
budget_hash
fixture_path
fixture_hash
fixture_seed
artifact_sha256s
```

Required evidence fields:

```text
required_evidence_tier
observed_evidence_tier
operator_host_input
real_os_input
human_observation
input_injection_method
visual_capture_method
private_runtime_dispatch_used
source_event_only_ipc_shortcut
preview_receives_example_name
preview_received_scenario_data
```

Required NovyWave-specific report fields:

```text
bridge_request_id
bridge_request_fingerprint
bridge_response_fingerprint
accepted_page_id
rejected_stale_page_ids
payload_bytes
transition_count
visible_rows
page_window
rust_handle_exposed=false
full_waveform_payload_entered_boon=false
```

Required performance fields:

```text
sample_count
warmup_policy
duration_ms
frame_time_ms_p50_p95_p99_max
input_to_visible_ms_p50_p95_p99_max
missed_frame_count
stage_timing_ms
longest_stall_stage
draw_calls_p50_p95_max
queue_write_count_p50_p95_max
upload_bytes_p50_p95_max
pipeline_switch_count_p95
text_shape_cache_hits_misses_evictions
glyph_atlas_upload_bytes
gpu_backend
adapter
present_mode
display_refresh_hz
```

Acceptance:

- Runtime-only evidence is allowed as semantic support, not UI proof.
- Automated reports never upgrade themselves to human observation.
- Performance reports cannot be produced from debug builds or from reduced
  fixtures unless the report says so and fails the release acceptance budget.
- Hidden/metamorphic fixture changes catch example-name branches, hardcoded
  labels, and source path assumptions.

## Phase 9: Remove Shortcut Surfaces As Features Land

Goal: make every new source/API/design feature retire at least one existing
shortcut or ambiguity.

Retirement checklist:

1. When typed source payloads land, remove event-name normalization,
   `key_down` to submit mapping, app-specific source path parsing, and
   private runtime dispatch routes from readiness paths.
2. When row identity lands, remove label-based row selection and text-only
   target matching from critical scenarios.
3. When selectors/indexes land, remove hot path full-list scans for selected
   rows, segments, cursor values, visible rows, and filtered todos.
4. When virtual materialization lands, remove full logical-grid rendering from
   Cells and full waveform-row materialization from NovyWave.
5. When style tokens land, remove performance-sensitive string-keyed style maps
   and theme-driven full rebuilds from readiness paths.
6. When asset refs land, remove generated large data-url strings from hot paths
   and add digest/cache diagnostics.
7. When bridge/effects land, remove descriptive NovyWave bridge shims as proof
   and require real schema/capability/request/completion reports.

Acceptance:

- Each retired shortcut has a negative test or report field that would catch it
  coming back.
- Legacy compatibility may remain behind explicit test names, but readiness
  gates use the generic API path.

## Phase 10: Final Readiness Definition

NovyWave should be considered correct/reliable/fast-ready only when all of the
following are true:

- The source model has canonical typed identities for files, waveforms, scopes,
  signals, pages, cursor, markers, viewport, and format.
- App transitions are command/payload based, not one source/branch per known
  signal.
- Selectors and indexes define the hot data access pattern even if some early
  implementations still scan under a measured wrapper.
- Waveform samples and large artifacts enter Boon only as bounded pages or
  blob/artifact refs.
- The view consumes virtual rows/pages and typed style tokens.
- Records, variants, source payloads, selectors, indexes, and virtual
  collections use existing Boon value/function/object syntax. Bridge imports
  remain the only intended new user-facing import surface in this plan.
- Driver and scenario evidence goes through public host input and public source
  dispatch.
- Native preview receives source/project payload, not example names or scenario
  data.
- Reports include source, scenario, fixture, budget, binary, and worktree
  fingerprints.
- Negative and metamorphic tests reject hardcoded labels, source paths, fixture
  IDs, example names, fake evidence, and reduced data.
- Release-mode interaction reports show real input-to-visible and frame timing
  budgets without hot-path report writes.

The intended end state is not just a faster NovyWave. It is a Boon design where
large interactive apps naturally express stable identity, bounded data,
materialized views, generic input routing, and provable rendering behavior.
