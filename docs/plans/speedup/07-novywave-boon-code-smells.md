# NovyWave Boon Code Smells

Date: 2026-06-12

## Purpose

This file records smells in the current NovyWave Boon source that can keep the
example slow even if the Boon parser, runtime, event path, layout, and renderer
become much faster.

The goal is not to shame the example. NovyWave is doing useful work as a
stress case. The point is to separate engine bottlenecks from app-shape
bottlenecks:

- a fast engine can skip unchanged work, but it cannot make a giant app graph
  cheap if every interaction changes broad, stringly, duplicated state;
- a fast renderer can retain pixels, but it cannot help if the view keeps
  materializing too many rows, text labels, and waveform segments;
- a fast bridge can stream pages, but it cannot help if Boon app state insists
  on owning full waveform data and parsing page identity out of labels.

The desired shape is:

```text
events
-> typed commands
-> reducer
-> normalized state
-> selectors and declared indexes
-> viewport/page cache
-> bounded renderer input
```

Boon should own interaction state, selected rows, viewport state, display
format, and declarative requests. Rust/Wellen should own waveform parsing,
indexes, caches, page/tile data, cursor extraction, and payload limits.

## Current Source Baseline

The current NovyWave example is already large enough to expose app-level
performance risks:

- `examples/novywave/View/NovyView.bn` is about 5,729 lines.
- `examples/novywave/RUN.bn` is about 3,097 lines.
- The NovyWave Boon files total about 9,982 lines.
- `RUN.bn` contains more than one hundred `SOURCE` occurrences.
- `RUN.bn`, `NovyView.bn`, `NovyModel.bn`, and `NovyTheme.bn` contain dense
  use of `HOLD`, `LATEST`, `WHEN`, `List/filter`, `List/map`, `List/retain`,
  and text construction.

Important local anchors:

- `examples/novywave/RUN.bn:1` defines a large top-level store with many event
  sources.
- `examples/novywave/RUN.bn:629` hardcodes waveform file metadata.
- `examples/novywave/RUN.bn:1052` hardcodes signal catalog data.
- `examples/novywave/RUN.bn:1126` hardcodes waveform segment records.
- `examples/novywave/RUN.bn:1527` derives selected waveform segments by list
  filtering.
- `examples/novywave/RUN.bn:2506` builds structural request keys from text.
- `examples/novywave/RUN.bn:2560` builds descriptor strings.
- `examples/novywave/RUN.bn:2672` builds compact page labels.
- `examples/novywave/RUN.bn:2790` parses descriptor strings back into fields.
- `examples/novywave/RUN.bn:3009` computes cursor values by scanning waveform
  records per selected signal.
- `examples/novywave/View/NovyView.bn:1` starts a giant view module covering
  shell, dialogs, browser panes, signal rows, waveform lanes, inspector, and
  controls.
- `examples/novywave/View/NovyView.bn:4035` filters already-derived waveform
  segment lists again by row.
- `examples/novywave/Model/NovyModel.bn:130` formats waveform values through
  repeated text trim/parse/radix logic.
- `examples/novywave/Bridge/NovyBridge.bn:3` returns textual bridge summaries
  rather than a real typed bridge protocol.
- `examples/novywave.scn:367` exercises stale-page behavior through synthetic
  probe state rather than real out-of-order bridge completions.

## Severity Model

Use this severity model when turning the notes into work:

- **Critical:** this will remain slow or fragile even with a faster engine.
- **High:** this widens invalidation, allocation, or state inconsistency risk.
- **Medium:** this makes optimization hard to reason about or easy to regress.
- **Low:** this is acceptable demo debt unless it spreads into core patterns.

Do not rewrite everything at once. The right sequence is to measure first, then
change the source shape or language/API surface where the measurement points.
The big changes below are candidates for design work, not approval to bypass
the current engine contracts.

## Critical Smells

### Waveform Data Lives In The Boon App Graph

NovyWave currently models file metadata, signal catalog rows, selected defaults,
hierarchy rows, and waveform transition records directly in Boon. This is fine
for a small scenario fixture, but it is the wrong hot-path shape for a real
waveform viewer.

Risk:

- large VCD/FST/GHW files cannot fit naturally into Boon semantic state;
- every file change risks invalidating a huge graph;
- runtime/list costs appear before rendering even starts;
- the source encourages tests to assert fixture labels instead of bridge
  behavior and payload limits.

Better shape:

- Rust/Wellen owns waveform data, indexes, and caches.
- Boon owns a small `WaveformRef`, selected row IDs, viewport, cursor, format,
  and request descriptors.
- Waveform rows and lanes consume bounded `HierarchyPageRef`, `SignalPageRef`,
  `WavePageRef`, `CursorValuePageRef`, or `BlobRef` values.
- The renderer consumes retained waveform page/tile descriptors or packed
  columnar data, not a freshly materialized Boon list of all transitions.

API direction:

```text
Wave/open(path_grant) -> ArtifactRef | OpenError
Wave/hierarchy_page(artifact, scope, offset, limit) -> HierarchyPageRef
Wave/signal_page(artifact, visible_signal_ids, t0, t1, pixel_width, format)
  -> WavePageRef
Wave/cursor_values(artifact, signal_ids, cursor_time, format)
  -> CursorValuePageRef
```

### Full-List Scans In Interactive Paths

The current code repeatedly filters and maps broad lists to answer narrow
questions:

- active file metadata is derived through many independent list lookups;
- selected waveform segments scan records by file and viewport;
- cursor value lookup scans waveform records once per selected signal;
- waveform lane rendering filters the selected segment list again per row;
- selected row visibility asks the signal catalog for membership repeatedly.

Risk:

- cursor move, pan, zoom, selection, and scroll can become
  `rows * segments` work;
- a faster renderer only exposes the runtime/list bottleneck more clearly;
- profiler output will look noisy because each visible row recreates similar
  derived data.

Better shape:

- compute `active_file_metadata` once and derive fields from that record;
- store metadata by file ID, not as repeated list scans;
- maintain or declare indexes such as:

```text
INDEX waveform_file_metadata BY file_id
INDEX signal_catalog BY signal_id
INDEX signal_catalog BY (scope_id, variable_id)
INDEX waveform_segments BY (artifact_ref, signal_id, time_page)
INDEX cursor_values BY (artifact_ref, cursor_time, format)
```

- add selector memoization so unchanged inputs preserve output identity.

### String Descriptors Are Doing Model Work

NovyWave currently uses text as IDs, request keys, bridge descriptors, compact
page labels, display labels, and parser inputs. Examples include descriptor
strings such as `SIMPLE_TB_S/A_SIGNAL/Fit/Center/Cursor42/Hexadecimal`, compact
labels such as `A_FIT`, and structural keys assembled with `Text/concat`.

Risk:

- equality becomes string equality over display-shaped values;
- labels and identity can drift independently;
- every request/change allocates and compares more text;
- parsing strings back into fields hides missing cases from the compiler;
- bridge responses cannot carry schema hashes, epochs, generation IDs, byte
  counts, or payload diagnostics cleanly.

Better shape:

- labels are final view projections only;
- identities are typed records, enums, or interned atoms;
- request keys are canonical structural values with generation/epoch fields;
- bridge pages carry `schema_hash`, `input_digest`, `page_digest`,
  `byte_len`, `sample_count`, and `response_generation`.

Candidate types:

```text
RECORD WaveRequest {
  artifact: ArtifactRef
  signals: List<SignalId>
  window: TimeWindow
  pixel_width: Px
  format: ValueFormat
  max_inline_bytes: Bytes
}

RECORD WavePage {
  request: WaveRequestKey
  page: PageRef
  encoding: WavePageEncoding
  byte_len: Bytes
  sample_count: Count
  dropped_or_compressed_segments: Count
}
```

### The Bridge Is A Pure-Data Mock

`NovyBridge.bn` returns text summaries, while `RUN.bn` builds request and
response descriptors locally. That is useful for bootstrapping the scenario,
but it does not exercise the hard parts of a real bridge:

- scheduling;
- dedupe;
- cancellation;
- stale completion rejection;
- schema drift;
- filesystem capability grants;
- payload caps;
- `BlobRef` fallback;
- out-of-order completion;
- timeout and diagnostic reporting.

Better shape:

- import a real bridge module, for example `IMPORT wellen.v1 AS Wave`;
- bridge effects return typed completions;
- every completion includes the request key, input digest, schema hash, epoch,
  status, and diagnostics;
- stale pages are rejected by the generic bridge/runtime path, not by string
  comparison in app code.

### Page Labels Are Not Durable Page References

Compact labels such as `A_FIT`, `CMP_UART`, and `DATA_RIGHT` are UI/debug
labels. They are not enough to represent a page of waveform data.

A real page reference needs at least:

- artifact identity;
- request identity;
- schema version/hash;
- time range;
- visible signal set;
- pixel resolution or bucket strategy;
- byte length;
- payload encoding;
- page digest;
- generation/epoch;
- dropped, compressed, or capped segment diagnostics.

The document should treat compact labels as smoke-test affordances only. They
should not become the app model.

## High Smells

### Duplicated Canonical State

The source keeps overlapping versions of the same idea:

- active signal value;
- active signal label;
- active signal key;
- bridge response signal;
- compact request label;
- response descriptor;
- parsed descriptor fields.

Risk:

- source events update several cells with related meaning;
- stale combinations become possible;
- invalidation broadens because many fields must be kept in sync;
- the code must re-derive the same identity in many forms.

Better shape:

- one canonical `active_selection` record;
- one canonical `viewport` record;
- one canonical `bridge_request` record;
- selector functions derive labels, compact diagnostics, and debug strings;
- response state is keyed by request, not copied into parallel app fields.

### Source-Per-Known-Signal Event Routing

The current source has global and row-local event paths for known signals such
as `clk`, `reset_n`, `tx_data`, and `ghw_state`. Real waveform files need
dynamic row identities.

Risk:

- adding signals adds code and branches;
- row removal/selection cannot scale with arbitrary signal IDs;
- source binding metadata becomes a large global bag;
- app code compensates for missing event payloads.

Better shape:

```text
SOURCE remove_signal: SignalId
SOURCE select_signal: SignalId
SOURCE set_signal_format: { signal: SignalId, format: ValueFormat }
SOURCE move_signal: { signal: SignalId, before: SignalId | none }
```

Language/runtime direction:

- typed action payloads;
- parameterized sources;
- row-scoped source selectors;
- stable item keys for lists and components.

### Virtualization Is Expressed As Ordinary Lists

Several NovyWave regions have virtual-scroll language in the model, but the
view still maps broad lists into document nodes. Selected signal rows are also
split into name/value/wave columns, which risks duplicate row work and
alignment bugs.

Risk:

- scroll work materializes too many rows;
- row identity by index breaks retained state;
- column-separated row maps duplicate dependencies;
- hover/focus/selection invalidation can affect whole list projections.

Better shape:

```text
VirtualList {
  key: lane_id
  total_count: lanes.len
  row_height: 24
  overscan: 4
  viewport: selected_rows_viewport
  render_row: SignalLaneRow
}
```

The virtual primitive should own visible-window slicing, stable row identity,
overscan, hit regions, focus route, and bounded materialization. The source
should not have to hand-code this with ordinary `List/map` and scroll flags.

### Formatting And Parsing Live In Hot Paths

Signal values are repeatedly trimmed, parsed, radix-inferred, width-inferred,
and formatted. That work is visible in `NovyModel.bn` and then reappears in row
and segment rendering.

Risk:

- cursor movement does text work per selected row;
- format toggles allocate many labels;
- string parsing hides signal width/type information;
- renderer text layout becomes the first visible bottleneck after runtime work.

Better shape:

- store waveform values as typed raw values: bit vector, integer, real, enum,
  string, high impedance, unknown;
- cache display values by `(value_id, bit_width, format, translator_id)`;
- make value translation a pure cacheable function:

```text
DisplayValue = translate(type_id, raw_bits, format)
```

Surfer is useful prior art here because it treats semantic value translation as
a viewer feature, not as arbitrary UI construction.

### Theme And Style Logic Is Repeated

The view and theme modules repeatedly branch on mode and pass style-like values
through app functions. Hover/focus styling uses ad hoc fields such as
`__hover_background`.

Risk:

- dark/light mode changes invalidate broad view expressions;
- repeated style records increase allocation and document diff size;
- pseudo-state is encoded inconsistently;
- renderer cannot easily intern computed style.

Better shape:

- design tokens for semantic colors, spacing, text styles, waveform lanes, and
  interactive states;
- style classes/variants instead of repeated full records;
- pseudo-state selectors for hover, active, selected, focused, disabled;
- mode-aware token resolution in the style engine.

Example:

```text
class signal-row {
  bg: token.surface.row
  fg: token.text.primary
}

class signal-row:selected {
  bg: token.surface.row_selected
}
```

## Medium Smells

### Giant Modules Hide Invalidation Domains

`NovyView.bn` mixes shell, dialogs, file tree, toolbar, variable browser,
selected rows, waveform lanes, inspector, icons, controls, and style helpers.
`RUN.bn` mixes app state, fixture data, bridge model, keyboard handling,
viewport math, signal selection, and scenario/debug labels.

Risk:

- dependencies widen because everything can see the whole store;
- small changes are difficult to audit;
- optimization cannot easily identify shell-only, row-only, waveform-only, or
  dialog-only updates.

Better split:

- `NovyWaveCore`: normalized app state, reducer, selectors;
- `WaveformBridge`: typed effects, page refs, diagnostics;
- `WorkspaceTree`: file/tree state and rendering;
- `SignalBrowser`: scopes, variables, search, filters;
- `SelectedSignals`: row order, selection, per-row state;
- `WaveformViewport`: time window, zoom, pan, cursor, pages;
- `NovyTheme`: tokens, classes, variants;
- `DebugReference`: scenario/reference/debug labels outside hot UI state.

### Static Fixture Paths Leak Into App Semantics

The current source contains local paths and fixture-specific metadata for the
demo files. Fixtures are good, but they should not become architecture.

Risk:

- scenario success can mean "the hardcoded fixture branch matched";
- external files re-enter the fixture-shaped state machine;
- real file mutation, weak/strong fingerprints, path grants, and parser errors
  are hidden.

Better shape:

- keep deterministic fixtures in a fixture manifest;
- route all file opens through the bridge effect path;
- attach content identity and diagnostics to bridge completions;
- let scenario code assert fixture-specific facts through public page
  descriptors, not through app hardcoding.

### Keyboard Routing Is Scattered

Keyboard state affects cursor movement, zoom, pan, bridge compact labels, and
reset logic in several places.

Risk:

- the same key can accidentally affect several domains;
- focus scopes are unclear;
- keyboard changes broaden invalidation;
- tests cannot tell which route handled the key.

Better shape:

```text
KEYMAP waveform_viewport {
  Left -> MoveCursor(-1 step)
  Right -> MoveCursor(+1 step)
  Ctrl+Plus -> ZoomIn
  Ctrl+Minus -> ZoomOut
  Home -> PanToStart
}
```

Keymaps should be declarative, focus-scoped, and produce typed commands.

### Generated Assets Are Inline Text

The current generated assets are small enough that the pattern is tolerable,
but inline SVG text is the wrong model for larger generated resources or
reference images.

Better shape:

- `asset` declarations or generated asset manifests;
- digest and media type;
- renderer-owned loading;
- `BlobRef` for large payloads.

## API And Language Improvements

### First-Class Records And Enums

NovyWave should not need to encode formats, zoom levels, files, scopes,
signals, diagnostics, pages, and bridge requests as bare atoms or strings.

Needed features:

- `enum` declarations with exhaustive `WHEN`;
- `record` declarations with typed fields and defaults;
- schema hashes for bridge-facing records;
- typed field access;
- interned atom identity for cheap equality.

Candidate domain types:

```text
enum ValueFormat { Binary, Hexadecimal, Decimal, ASCII, Signed, Analog }
enum ZoomMode { Fit, Center, In, Out, Manual }
enum CursorMode { Hidden, At(Time), FollowPointer }

record SignalId { artifact: ArtifactRef, scope: ScopeId, local: Atom }
record TimeWindow { start: Time, end: Time, px_per_time: Ratio }
record Viewport { first_row: Index, visible_rows: Count, window: TimeWindow }
```

### Explicit Modules And Component Contracts

The current file-stem namespace convention is not enough for large examples.
NovyWave would benefit from explicit source-level contracts:

```text
MODULE NovyWave.SignalBrowser

IMPORT wellen.v1 AS Wave
EXPORT COMPONENT SignalTree
EXPORT SELECTOR visible_signal_ids
```

Component declarations should support typed props, event outputs, children or
slots, stable identity, and style variants.

### Reducers And State Machines

Clusters of `HOLD` and `LATEST` cells currently encode state transitions
implicitly. Dialogs, panel layout, markers, cursor, zoom, pan, and bridge
requests would be easier to optimize if their event tables were explicit.

Candidate direction:

```text
STATE_MACHINE load_dialog {
  Closed
  Open { selected_path: PathText }
  Loading { request: OpenRequestKey }
  Failed { message: Text }

  on OpenDialog -> Open
  on ConfirmPath(path) when path.valid -> Loading
  on OpenCompleted(ok) -> Closed
  on OpenCompleted(error) -> Failed
}
```

This lets the compiler/runtime know which fields can change for a given event.

### Declared Indexes And Selectors

NovyWave currently compensates for missing indexes by filtering lists. The
engine should eventually understand indexes/selectors as first-class
incremental dataflow nodes.

Candidate direction:

```text
INDEX signals_by_id ON signals BY signal.id
INDEX signals_by_scope ON signals BY signal.scope
INDEX pages_by_request ON wave_pages BY page.request_key

SELECTOR visible_lane_ids(viewport, lane_order) {
  lane_order |> List/window(viewport.first_row, viewport.visible_rows)
}
```

Selectors should:

- declare dependencies;
- cache outputs;
- preserve output identity when values are equal;
- report recompute counts and dirty causes;
- fail if they allocate too much in hot interaction paths.

### Virtual Viewport Primitives

Large app authors should not hand-roll viewport math with ordinary lists. Boon
needs primitives for:

- vertical row virtualization;
- horizontal time-window paging;
- two-axis grids;
- overscan;
- sticky headers/columns;
- stable hit regions;
- focus retention;
- scroll-to-item;
- page/tile prefetch.

Waveform viewers especially need two axes:

```text
page_key = {
  artifact,
  signal_ids,
  time_start,
  time_end,
  pixel_width,
  row_height,
  value_format,
  zoom_bucket,
}
```

### Effectful Bridge Modules

Bridge work should be declared as effects, not simulated as text records inside
the app.

Minimum effect contract:

- canonical request key;
- schema hash;
- request epoch;
- cancellation epoch;
- input digest;
- capability grants;
- byte limits;
- timeout;
- success/error/stale/canceled status;
- diagnostics;
- optional `BlobRef` payload.

The scenario suite should include:

- duplicate request dedupe;
- old completion after new request;
- canceled request later completing;
- schema drift rejection;
- payload cap overflow;
- missing filesystem grant;
- large-file page with `byte_len`, `sample_count`, and
  `dropped_or_compressed_segments`.

### Debug Data Outside The Hot App State

Reference labels, structural debug strings, scenario fingerprints, compact
labels, and provenance text are valuable. They should not be the primary live
model.

Better shape:

- app state contains typed records and page refs;
- debug projections are computed on demand or only in proof/report mode;
- scenario reports record compact labels and provenance after the interaction;
- hot interaction paths avoid rebuilding report-only strings.

## Prior Art To Steal From

### GTKWave And FST

GTKWave's format documentation is blunt: VCD is slow and memory-heavy for the
viewer, while FST is designed for fast sequential and random access. That is the
right lesson for NovyWave: do not make the UI own a VCD-shaped list of changes.
The viewer should query an indexed trace store.

Source: <https://gtkwave.github.io/gtkwave/intro/formats.html>

### Wellen

`wellen` is a Rust waveform library optimized for waveform viewers that only
need to access a subset of signals. NovyWave should lean into that: a Boon UI
request should name signals and a time window, then receive bounded pages or
refs.

Source: <https://github.com/ekiwi/wellen>

### Surfer

Surfer points toward semantic value translation as a viewer feature. NovyWave
should treat raw waveform values and domain-specific translations as pure,
cacheable data transforms, not as view construction side effects.

Source: <https://blog.yosyshq.com/p/community-spotlight-surfer/>

### Virtualized Lists

Web list virtualization and egui's `ScrollArea::show_rows` both point to the
same rule: large lists should materialize only visible rows plus overscan.
NovyWave should not build all signal rows, all variable rows, or all segments
as ordinary document nodes.

Sources:

- <https://web.dev/articles/virtualize-long-lists-react-window>
- <https://docs.rs/egui/latest/egui/containers/scroll_area/struct.ScrollArea.html>

### Normalized State And Selectors

Redux's normalized state guidance, Redux/Reselect selectors, and React's state
structure guidance all argue against duplicated derived state. NovyWave should
store IDs and canonical records, then use memoized selectors for derived labels,
visible rows, and page requests.

Sources:

- <https://redux.js.org/usage/structuring-reducers/normalizing-state-shape>
- <https://redux.js.org/usage/deriving-data-selectors>
- <https://github.com/reduxjs/reselect>
- <https://react.dev/learn/choosing-the-state-structure>

### Fine-Grained Derived State

Svelte `$derived`, Solid fine-grained reactivity, and MobX computed values all
converge on the same rule: derived values should be pure, dependency-tracked,
cached, and recomputed only when inputs change.

Sources:

- <https://svelte.dev/docs/svelte/%24derived>
- <https://docs.solidjs.com/advanced-concepts/fine-grained-reactivity>
- <https://mobx.js.org/computeds.html>

### Declarative Effects

Elm's command/subscription split is a useful model for Boon event handlers:
handlers should produce commands and state changes, while the runtime owns the
effect scheduling.

Source: <https://guide.elm-lang.org/effects/>

### Browser Layout Guardrails

Browser performance guidance warns against large layouts, layout thrashing, and
read-after-write geometry patterns. Boon should keep measurement, layout, and
render writes in explicit phases and should not expose arbitrary geometry reads
inside render expressions.

Sources:

- <https://web.dev/articles/avoid-large-complex-layouts-and-layout-thrashing>
- <https://developer.chrome.com/docs/performance/insights/forced-reflow>
- <https://developer.mozilla.org/en-US/docs/Web/CSS/Guides/Containment/Using>

## Migration Sketch

### Step 1: Measure The Current Smells

Add profiling/report fields before rewriting:

- list scan counts by function;
- rows materialized by region;
- segment records scanned per cursor move/pan/zoom;
- string allocations for request/response labels;
- text formatting calls by value/format;
- selector-equivalent recompute counts;
- payload bytes crossing bridge or renderer boundaries;
- document nodes created per interaction.

### Step 2: Extract Canonical Records In Boon

Without changing language syntax yet:

- create a canonical `active_selection` record;
- create a canonical `viewport` record;
- compute `active_file_metadata` once;
- split display labels from identity fields;
- move compact labels into debug-only derived values.

### Step 3: Add Typed Bridge Page Concepts

Before real Rust bridge effects are complete, make the Boon model use the
target terms:

- `ArtifactRef`;
- `WaveformRef`;
- `HierarchyPageRef`;
- `SignalPageRef`;
- `WavePageRef`;
- `CursorValuePageRef`;
- `BlobRef`;
- request epoch and page generation.

This makes the example ready for the real bridge without teaching the source
that labels are page identities.

### Step 4: Replace Full Scans With Indexes Or Selector Shims

Introduce helper functions that behave like future indexes:

- `metadata_by_file_id`;
- `signal_by_id`;
- `signals_by_scope`;
- `segments_by_signal_and_window`;
- `cursor_value_by_signal`.

Even if the first implementation is still a list scan, the source shape should
make the future engine/API improvement obvious.

### Step 5: Introduce Real Virtual Regions

Make selected rows, variable rows, and waveform lanes use one row component
with stable identity. The row should own name, value, and lane cells together.
Then replace app-authored visible-window calculations with a real virtual list
primitive when the engine supports it.

### Step 6: Move Formatting To Typed Value Translation

Keep raw values and bit widths as domain values. Cache display labels by
value/format. Treat ASCII, binary, hex, signed, decimal, enum, and analog views
as translators.

### Step 7: Split Debug/Scenario Labels Out Of Runtime Hot Paths

Keep scenario observability, but generate reference labels in reports or debug
selectors. Do not use report strings as state keys, page identity, or ordinary
render input.

## Acceptance Criteria For Future Cleanup

A cleaned-up NovyWave source should satisfy these source-level checks before
engine speed claims are trusted:

- opening a file routes through a typed bridge effect or a bridge-shaped test
  double, not a hardcoded descriptor branch;
- waveform samples do not live as broad Boon app state;
- page identity is typed and includes generation/epoch;
- selected row actions carry `SignalId` payloads;
- cursor value lookup does not scan all records per selected row;
- visible rows and visible waveform segments are selected by viewport/page
  primitives;
- debug labels are derived projections, not model identity;
- scenario assertions can still prove the same user-visible behavior through
  public state and app-owned render evidence.

