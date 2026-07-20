# Boon Example Portfolio Plan

Date: 2026-07-20

Status: proposed authoritative portfolio and implementation plan. No example,
runtime optimization, browser backend, GPU backend, audio path, or FPGA path is
claimed to be implemented by this document.

This plan defines the next public Boon example portfolio and the internal
performance laboratory that grows with it. It covers six performance/product
examples and seven clean, canonical 7GUIs examples. Existing examples remain
valid unless this plan explicitly changes only their catalog presentation.

## Executive Decision

Add these thirteen new manifest examples:

| Group | Manifest id | Dev-catalog label | Purpose |
|---|---|---|---|
| 7GUIs | `seven_guis_counter` | `7GUIs · Counter` | Minimal state and event scaffolding |
| 7GUIs | `seven_guis_temperature_converter` | `7GUIs · Temperature Converter` | Bidirectional editing and invalid drafts |
| 7GUIs | `seven_guis_flight_booker` | `7GUIs · Flight Booker` | Constraints, dates, choices, and disabled controls |
| 7GUIs | `seven_guis_timer` | `7GUIs · Timer` | Time, slider interaction, and competing events |
| 7GUIs | `seven_guis_crud` | `7GUIs · CRUD` | Keyed list mutation, filtered views, and selection |
| 7GUIs | `seven_guis_circle_drawer` | `7GUIs · Circle Drawer` | Custom drawing, dialog context, and undo/redo |
| 7GUIs | `seven_guis_cells` | `7GUIs · Cells` | Minimal spreadsheet dependency propagation |
| Performance | `boon_search` | `BoonSearch` | Pure-Boon compact full-text search |
| Performance | `boon_tune` | `BoonTune` | FFT/YINFFT tuner, live audio, and later FPGA |
| Performance | `mandelbrot` | `Mandelbrot Explorer` | Numeric kernels, progressive tiles, CPU/GPU scheduling |
| Performance | `path_tracer` | `Path Tracer` | Realistic parallel rendering and spatial data |
| Performance | `boon_fingerprint` | `Boon Fingerprint` | Exact BLAKE3 byte/tree kernel and full-mode compatibility target |
| Performance | `runtime_lab` | `Boon Runtime Lab` | Unified Boon-only benchmark and regression application |

The portfolio is deliberately consolidated:

- FFT, spectrum, spectrogram, and pitch-estimation work belongs in BoonTune;
  there is no separate FFT example.
- The canonical DAG/spreadsheet example is 7GUIs Cells. The existing advanced
  Cells remains the large spreadsheet/runtime stress application; there is no
  additional generic DAG example.
- AWFY, 1BRC profiles, NPB classes, LZ4 kernels, and extracted microbenchmarks
  are suites inside Boon Runtime Lab, not separate public example tabs.
- There is no Wellen rewrite. NovyWave can adopt reusable Boon codecs or DSP
  libraries later without creating a competing example now.
- No Rust or JavaScript implementation, comparison port, benchmark harness,
  vendored alternative, or source copy is added anywhere in this repository
  merely to compare languages. Runtime Lab executes Boon implementations only.
  Cross-language workload ports and comparison harnesses are outside this
  repository. This plan creates no hook, fixture lane, vendored source, or
  runner for them.

## Existing Examples Are Preserved

The existing `counter` example remains unchanged: same manifest id, source,
behavior, scenarios, budget, label, and `basic` grouping. The canonical 7GUIs
Counter is a new independent example and does not import, replace, simplify,
or migrate the existing Counter.

The existing `cells` remains a separate advanced application with its stable
id, behavior, scenarios, budgets, performance instrumentation, and proof
contract preserved. A later behavior-preserving refactor may move reusable
pure-Boon formula/dependency modules into a generic package, but the canonical
application may not import its advanced application state or scenarios. To
prevent it from being mistaken for the new canonical task, its catalog label
becomes `Cells Advanced`. Its non-`7gui` category is selected from the generic
catalog taxonomy during implementation. Before changing presentation metadata,
check all manifest consumers; no consumer may branch on a label or category.
The canonical `seven_guis_cells` remains independent application source.

Documentation must distinguish the two:

- `docs/examples/CELLS_CIRCUIT_STYLE.md` continues to describe the advanced
  proof target and should be titled/referenced as Cells Advanced.
- The canonical task gets a concise `docs/examples/SEVEN_GUIS_CELLS.md`
  contract focused on official 7GUIs behavior and readable source.
- README proof-target text must say which Cells variant is the performance
  proof and which is the canonical notation example.

Active documentation uses this migration rule:

| Existing term/identifier | Meaning after this plan | Migration rule |
|---|---|---|
| manifest id `cells` | Cells Advanced | Stable machine id; do not rename |
| `verify-cells`, its report and budgets | Cells Advanced proof | Keep bound to id `cells` |
| prose label `Cells` when referring to the advanced app | Cells Advanced | Clarify prose in the active native pipeline, handoff/runbook, goal, and verification docs |
| `seven_guis_cells` | Canonical 7GUIs Cells | New independent scenarios, budgets, and reports; never inherit `verify-cells` accidentally |

## Non-Negotiable Architecture Rules

1. Every application behavior is expressed in Boon source. Native, browser,
   compiler, runtime, document, renderer, scheduler, verifier, and codegen
   paths may not branch on an example id, label, path, or category.
2. The desktop process may use example identity for catalog/editor UX. The
   production-style preview receives Boon source units plus only the generic
   source-replacement/launch identity required by the host protocol. It never
   receives manifest id, example name/label/category, or an example-specific
   native rendering shortcut.
3. A real language, compiler, typechecker, runtime, document, renderer,
   scheduling, or host limitation is fixed generically. No example-level
   workaround is accepted as the final implementation.
4. The CPU interpreter is the first semantic reference on native and browser.
   Worker, WGPU/WebGPU, future Rust/Zig codegen, and FPGA results must validate
   against the same source semantics and fixtures.
5. A GPU implementation is generated from an eligible pure Boon kernel and the
   authoritative typed IR. Do not hand-write a second WGSL algorithm that only
   resembles the Boon program.
   Browser CPU/Wasm preserves Boon's finite binary64 `Number` semantics. A
   portable WebGPU floating kernel uses a separately named reduced-precision
   profile such as `ApproxF32`, with its own profile id, oracle, tolerances, and
   eligibility limits; it is never described as interpreter-equivalent. Exact
   32-bit word kernels may lower proven lanes to WGSL `u32` without exposing a
   second Boon scalar type.
6. FPGA DSP is lowered from bounded Boon kernels and explicit hardware profiles.
   Board I/O shells may be target adapters, but no second handwritten DSP
   implementation may become the evidence path.
7. Performance claims are accepted only after result correctness. A faster
   wrong digest, wrong image, wrong note, wrong search result, or failed
   numerical tolerance is a failed run.
8. Do not add Rust or JavaScript versions for comparison anywhere in the repo.
   Rust remains the implementation language of the Boon engine and platform
   adapters; that does not authorize duplicate workload implementations.
9. Do not use `List/query` or `List/query_prefix` in BoonSearch. The active
   `TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md` removes those calls. Search
   must be ordinary Boon code over typed collections and compact numeric/byte
   buffers; compiler optimizations must remain generic.
10. Human observation is a separate follow-up after automated native evidence.
    It never substitutes for scenarios, exact outputs, app-owned timing, WGPU
    readback, or machine-readable FPGA results.

## Source And Catalog Layout

The intended application layout is:

```text
examples/
  7guis/
    counter/
    temperature_converter/
    flight_booker/
    timer/
    crud/
    circle_drawer/
    cells/
  boon_search/
  boon_tune/
  mandelbrot/
  path_tracer/
  boon_fingerprint/
  runtime_lab/
```

Each example owns, as applicable:

```text
RUN.bn                  executable entrypoint
Model/*.bn              domain/state logic
View/*.bn               document/scene logic
Algorithm/*.bn          pure computation kernels
Fixtures/*.bn           small deterministic embedded fixtures
<example>.scn           semantic/product scenarios
<example>.budget.toml   latency, memory, and work budgets
```

Exact capitalization follows the repository's canonical module rules. Do not
create empty layers merely to match this sketch. A small 7GUIs task should stay
small and may need only `RUN.bn` plus its scenario and budget.

All seven canonical tasks use manifest category `7gui`. The dev catalog renders
that category as one `7GUIs` group in official task order. Grouping is generic
manifest/catalog behavior, not seven hardcoded example ids.

`boon_example_manifest` owns validation of category, category order, and item
order. Desktop catalog loading owns normalized ordering. The desktop-to-dev
catalog protocol carries generic category, category order, item order, and
visibility fields, and the dev window renders groups without id branches.
Unknown or custom categories use a documented deterministic fallback. This
metadata is dev/catalog-only: source replacement into the production preview
does not send manifest id, label, category, order, or group metadata.

Every manifest entry defines its source units, deterministic scenario, budget,
evidence tier, initial visible assertions, interaction scenarios, visual
artifacts, and performance thresholds. New examples must enter through the
same catalog/source-replacement path as existing examples.

Reusable algorithms should become ordinary Boon modules/packages when more
than one application consumes them. A canonical example must still expose all
relevant source units in the dev window; reuse may not hide its application
logic behind a native intrinsic.

## Delivery Order For Every Example

Every example is delivered vertically, native first and browser second:

1. **Semantic core**: typecheck, compile, deterministic fixtures, result oracle,
   and CLI scenario through the ordinary interpreter.
2. **Native single-CPU interpreter**: production preview plus dev/debug window,
   ordinary source replacement, retained rendering, product interactions, and
   app-owned evidence.
3. **Native automatic CPU execution**: only for workloads with enough parallel
   work; include scheduler choice, task count, crossover, cancellation, and
   deterministic reduction evidence.
4. **Native WGPU compute**: only for an eligible pure kernel and only after the
   interpreter result is authoritative.
5. **Browser Wasm interpreter**: same source, same fixtures, same digest or
   numerical tolerance, one compute worker where needed so UI stays responsive.
6. **Browser worker execution**: independent workers or shared-memory workers
   according to the workload and security headers. Record the actual mode.
7. **Browser WebGPU**: only for eligible generated kernels. Report transfer and
   dispatch overhead separately from kernel time.
8. **Additional target**: BoonTune alone adds the planned FPGA path in this
   portfolio. Other hardware targets require their own later decision.

"Works in every host" does not mean "force every computation onto every
backend." Unsupported or counterproductive modes report `NotEligible` with a
reason. They do not contain artificial GPU rewrites.

## Backend Eligibility

| Example | CPU interpreter | CPU workers | WGPU/WebGPU compute | FPGA |
|---|---|---|---|---|
| Seven canonical 7GUIs | Required | Not required; Cells may later use wide dependency frontiers | Normal GPU rendering only, not a compute claim | No |
| BoonSearch | Required | Index building, batch queries, and sufficiently large shards | Not planned; irregular text/postings work is a poor first GPU target | No |
| BoonTune | Required | One persistent live-analysis worker; pools for offline corpus/batches | Batched FFT, spectrogram, and corpus analysis; live mono may remain CPU | Required later |
| Mandelbrot | Required | Required for tiles | Required | No |
| Path Tracer | Required | Required for tiles/samples | Required through portable compute | No |
| Boon Fingerprint | Required | Required for large tree leaves | Optional research only; not an acceptance requirement | No |
| Runtime Lab | Required | Suite-specific | Only generated variants explicitly separated from canonical CPU cases | No |

## Canonical 7GUIs Suite

The official task descriptions at <https://eugenkiss.github.io/7guis/tasks/>
are the behavioral source. These examples prioritize readable Boon notation and
generic toolkit behavior, not extra product features or headline throughput.

Shared acceptance rules:

- Use clean independent application state. Do not import existing Counter or
  Cells application source.
- Match the official interaction contract before adding visual polish.
- Keep domain logic visibly separated from rendering where the task calls for
  it, especially CRUD and Cells.
- Provide keyboard navigation, focus indication, disabled semantics, accessible
  names/roles/states, deterministic TEST scenarios, and native visual evidence.
- Do not add specialized native widgets merely to make a task pass. Generic
  select/listbox, slider, gauge, dialog, pointer, and shape capabilities are
  valid engine work and must serve arbitrary Boon applications.
- All user-visible state changes originate from Boon source-owned state and
  source bindings.

### 7GUIs Counter

The preview contains one read-only value initially showing `0` and one `Count`
button. Every activation increments the value by one.

The source should be the smallest honest executable Boon UI in the suite. It
must not gain decrement, reset, migration, themes, persistence controls, or
benchmark instrumentation. Those features remain in other existing examples.

Required scenarios:

- initial value is `0`;
- pointer activation increments once;
- keyboard activation increments once;
- repeated activations preserve exact integer state;
- one pointer, keyboard, or accessibility activation produces one source
  activation; duplicate/replayed host envelopes are rejected by generic input
  routing tests rather than by Counter-specific state.

### 7GUIs Temperature Converter

The preview contains Celsius and Fahrenheit text inputs, initially empty. A
valid numeric edit in either field updates the converted value in the other.
An invalid draft remains visible in the edited field and does not update the
other field.

The model owns:

- each visible text draft;
- the last valid canonical temperature and edit direction;
- deterministic formatting that does not create a reactive ping-pong loop;
- validity state separate from parsing fallback.

Formulas are `F = C * 9 / 5 + 32` and `C = (F - 32) * 5 / 9`. Fixtures cover
empty input, sign, decimals, whitespace policy, zero, freezing/boiling points,
invalid text, recovery after invalid input, and alternating edits between
fields. Locale-dependent parsing is outside the canonical task unless Boon
later defines it explicitly.

The canonical contract freezes a finite decimal grammar, including accepted
sign, decimal point, exponent, and surrounding-whitespace rules. Non-finite
spellings and overflow are invalid. It also freezes output rounding,
significant digits, trailing-zero policy, and negative-zero display through
examples. A programmatic peer update is a value projection, not a second edit:
it emits no edit event, does not steal focus, does not reset the active caret,
and does not destroy an in-progress IME composition. An invalid draft leaves
the peer field byte-for-byte unchanged.

### 7GUIs Flight Booker

The preview contains a choice between one-way and return flights, start and
return date inputs, and a booking button.

Contract:

- one-way is the initial mode;
- both date fields initially contain the same deterministic valid date;
- the return field is disabled in one-way mode;
- enabled malformed dates are styled invalid and disable booking;
- return booking is disabled when the return date is strictly before the start;
- equal start and return dates are valid;
- a valid activation shows a source-owned confirmation containing the selected
  mode and applicable date or dates;
- disabled controls do not dispatch events through pointer, keyboard,
  accessibility, scenario, or synthetic-host routes.

Use one reusable pure-Boon calendar module with an explicit canonical grammar,
initially `DD.MM.YYYY`. It validates month lengths and leap years and compares a
derived ordinal value. Do not add a Flight-Booker-specific Rust date parser.
The visual choice control may be a generic select/combobox; it must implement
keyboard and accessibility semantics, not merely look like one.

The booking confirmation is a generic modal overlay within the production
preview. It does not create an example-specific third native process/window.
Scenarios cover leap day, invalid text in the disabled return field not
blocking one-way booking, preservation of the return draft while modes switch,
every modal dismissal route, and focus restoration to the booking control.

### 7GUIs Timer

The preview contains an elapsed-time gauge, numerical elapsed label, duration
slider, and Reset button.

Contract:

- elapsed time is represented as accumulated running time plus the monotonic
  start of the current running epoch, and advances until `elapsed >= duration`;
- moving the duration slider updates the gauge while the pointer is moving;
- if a completed timer gets a duration greater than elapsed, it resumes;
- Reset makes elapsed zero without corrupting the selected duration;
- missed/coalesced host ticks do not make elapsed time run slow;
- timer and user input in the same turn have deterministic ordering.

This example requires every time-sensitive scheduled, pointer, keyboard,
accessibility, and scenario source envelope to expose enough monotonic
observation and source-sequence information for correct elapsed time.
`received_tick_count * interval` is not accepted. The slider requires generic
pointer capture,
continuous change payloads, keyboard adjustment, min/max/step semantics, and an
accessible numeric value. The gauge is a generic progress semantic with a
retained visual implementation.

Every event first settles elapsed time through that event's timestamp under
the prior running state, then applies the event in source-sequence order. Reset
starts a new running epoch at that timestamp. Raising duration after completion
resumes from the duration-change timestamp, not from a later tick. The contract
freezes initial duration, slider minimum/maximum/step, elapsed-label precision,
gauge clamping, and zero-duration behavior. Deterministic tests use an injected
virtual monotonic clock and include same-timestamp tick/reset/slider collisions,
coalesced ticks, suspension, and resume.

Clock advancement that first crosses the active duration freezes displayed and
stored elapsed exactly at that duration. Lowering duration below an already
stored elapsed value does not rewrite elapsed; raising duration resumes only
when the new duration is greater than stored elapsed. No interval before a
Reset timestamp may be re-added afterward.

### 7GUIs CRUD

The preview contains a surname-prefix filter, given-name and family-name
inputs, a one-selection listbox, and Create, Update, and Delete buttons.

Contract:

- filtering is immediate and case policy is explicit;
- filtering changes a derived view, not the authoritative person list;
- the listbox occupies the remaining available vertical space;
- filtering preserves authoritative database order;
- selection uses hidden stable list identity rather than display text or
  ordinal position;
- Create appends one person from the drafts;
- Update replaces the selected person's names without changing row identity;
- Delete removes the selected person;
- Update/Delete are enabled only with a live selection;
- deleting or filtering away a selection follows one documented deterministic
  clearing policy;
- duplicate visible names remain distinct rows.

The listbox is a generic selectable-list component with keyboard navigation,
selected semantics, scroll behavior, and row-local sources. Do not represent it
as an unlabelled pile of example-specific buttons. The model/view separation
should be obvious in the source.

The canonical contract freezes visible-name formatting and prefix
normalization. It does not invent a non-empty-name validation rule absent from
the canonical task. Update preserves identity and database position; duplicate
names and an empty prefix remain valid test cases.

### 7GUIs Circle Drawer

The preview contains Undo and Redo controls above a drawing surface.

Contract:

- primary click in empty space creates a fixed-diameter unfilled circle;
- pointer hover continuously selects and gray-fills the nearest circle whose
  center is within its radius, and leaving all circles clears selection;
- primary click inside an existing circle creates no circle;
- ties resolve deterministically by distance and then stable creation identity;
- secondary click on the selected circle opens a generic context menu;
- `Adjust diameter…` opens a modal slider whose changes apply live;
- every modal close path commits the final diameter exactly once as one
  significant undo action, while an unchanged diameter commits no history;
- Undo/Redo handles creation and diameter changes;
- Undo and Redo controls expose correct disabled state;
- a new significant action after Undo truncates the redo branch;
- pointer movement, capture, modal focus, Escape/close, and resize remain
  deterministic.

The drawing surface uses generic retained scene/layer primitives and pointer
coordinates. If the current scene cannot express arbitrary positioned circles
cleanly, add a generic vector/shape primitive rather than a Circle-Drawer
widget. Secondary buttons, pointer down/up/move, pointer capture, local
coordinates, and context-menu intent are generic host/document capabilities.

Use a source-owned event log plus history cursor, or two explicit source-owned
history stacks, whichever produces clearer typed Boon source. The live diameter
draft is separate from the committed history entry.

The official separate-dialog interaction is implemented as a generic
in-preview modal dialog; it does not create an example-specific third native
process/window. Undo/redo of creation preserves stable circle identity, and
hover selection is re-derived from the current pointer position after history
changes. Context-menu keyboard access and modal focus trapping/restoration use
generic accessibility behavior. If precise drawing-surface keyboard operation
is not included in the first canonical profile, that limitation is stated
honestly rather than claiming complete keyboard equivalence.

### 7GUIs Cells

The canonical preview is a clean, scrollable 26-column by 100-row spreadsheet.
Columns are `A` through `Z`; rows are `0` through `99`. Double-clicking a cell
edits its formula, committing parses/evaluates it, and only affected dependents
recompute until stable.

This example is smaller and more readable than Cells Advanced, but it is not a
fake four-cell demo. It must include:

- all 2,600 logical cells and a bounded/virtualized visible viewport;
- literals, basic arithmetic, and cell references;
- deterministic parse, reference, type, division, and cycle errors;
- explicit forward and reverse dependency edges;
- formula replacement that removes stale edges;
- affected-only topological propagation with unchanged-value cutoff;
- reusable spreadsheet-grid/component boundaries;
- deterministic cell addressing independent from hidden row identity.

Before implementation, `seven_guis_cells_semantics_v1` freezes the normative
cell language and editing behavior:

- blank and literal values, whether formulas require a leading `=`, finite
  numeric grammar, whitespace, parentheses, unary operators, precedence, and
  supported binary operators;
- the `A0` through `Z99` reference grammar, out-of-range policy, value/result
  formatting, and exact parse/reference/type/division/cycle error values;
- Enter and blur commit, Escape cancellation, double-click-to-edit, focus
  movement, and whether a parse-error draft remains editable while the last
  committed graph stays authoritative;
- deterministic propagation of typed errors to dependents, identification of
  every member of a cyclic strongly connected component, and recovery after a
  cycle or error is removed.

The application source is independent from `examples/cells`. It may consume
reusable pure-Boon formula, dependency, list, and UI modules, but it may not
import Cells Advanced application state or scenarios. The engine may optimize
only generic typed collection/currentness operations usable by arbitrary Boon
programs; it may not own a spreadsheet parser, evaluator, dependency graph, or
propagation intrinsic. Shared formula/dependency code is pure Boon rather than
two copied applications or a native implementation.

Required stress scenarios include a chain, fanout, layered diamonds, cycle
creation/removal, formula edge replacement, and one unrelated edit proving no
whole-grid recomputation. The canonical UI remains minimal; Cells Advanced owns
large performance dashboards and future spreadsheet product features.
Virtualization may not change formula state, dependency ownership, current
selection, edit focus, or error propagation. The generic pointer activation
contract must distinguish single and double activation deterministically.

### Shared 7GUIs Interaction And Accessibility Contract

The suite's accessibility promise is behavioral, not decorative. Generic
components define and test listbox Arrow/Home/End navigation, combobox
selection, slider Arrow/Page/Home/End adjustment, modal focus trap and focus
restoration, keyboard context-menu invocation, disabled-event suppression, and
Cells grid navigation/edit-mode transitions. Accessible activation generates
the same source event as pointer/keyboard activation, once. Any task whose
custom surface lacks an equivalent interaction in an initial profile records
that fact as an explicit acceptance limitation rather than silently passing.

## BoonSearch

BoonSearch is a complete Boon-owned client-side full-text search library and
interactive preview. It is inspired by the LocalSearch product shape described
at <https://kavik.cz/work/> and <https://github.com/Stremio/local-search>, but
it does not embed, call, vendor, or ship the Rust implementation or a JavaScript
alternative.

### Product Contract

The preview loads or generates a catalog, builds/loads its index, and provides
search-as-you-type with:

- Unicode-aware deterministic normalization and tokenization;
- exact complete-token matching;
- prefix matching for the last query token;
- typo-tolerant matching with a length-dependent edit-distance limit;
- field weights and optional document boosts;
- stable bounded top-k ranking;
- snippets/highlighting derived from stored offsets where enabled;
- deterministic ties by stable document identity;
- index build/load/update/compact controls and visible memory/index size;
- small bundled fixtures plus separately acquired larger corpora.

The initial relevance policy is explicit and versioned. Prefer rare exact
terms, intersect selective postings first, relax to OR only under a documented
fallback, and perform fuzzy expansion only when exact/prefix results are
insufficient. Scoring may use a compact BM25F-style formula or a deliberately
simpler specialized formula, but the formula and constants are fixtures, not
silent tuning.

`boon_search_semantics_v1` freezes the Unicode data version, normalization
form, full/simple case-fold and locale policy, segmentation/token rules,
punctuation and diacritic policy, and the edit-distance variant. Stored match
offsets declare their unit and include a deterministic normalized-to-original
span map suitable for both native text and browser DOM highlighting. Ranking
freezes score arithmetic and term accumulation order, or uses explicitly
quantized scores, so near ties cannot reorder across workers or hosts. Each
query binds one immutable index-generation snapshot; base, delta, tombstone,
and compaction generations cannot mix within a result.

### Reusable Library Contract

BoonSearch is a reusable Boon library as well as a preview. It exposes typed,
versioned Boon APIs for:

- building an index from document/field records;
- loading and validating a serialized index;
- serializing a compatible index;
- executing a versioned query and returning stable typed results/spans;
- adding, replacing, and removing documents in a delta generation; and
- compacting base, delta, and tombstones into a new immutable generation.

Configuration, document, query, result, error, statistics, and format-header
types are versioned. The library owns the compact-format compatibility policy
and validation limits. The preview and Runtime Lab import the same Boon
library; neither copies the kernels. Choose its exact package path only after
confirming the repository's package/module convention.

### Pure-Boon Index Architecture

Do not port the entire Rust `fst` API. Build only the reusable structures the
product needs:

1. dense document storage and compact sorted postings;
2. a packed radix trie as the first term dictionary;
3. an immutable minimal DAFSA/FSA when footprint measurements justify it;
4. dense term ids, document ids, score buffers, and generation/touched stamps;
5. precomputed document norms and term statistics;
6. bounded heap/selection for top-k instead of full result sorting;
7. banded Levenshtein or optimal-string-alignment traversal for fuzzy search;
8. an immutable base index plus mutable delta, tombstones, and deterministic
   compaction for incremental updates;
9. a versioned compact serialized index independent from the runtime Wasm.

Unicode scalar/symbol transitions are preferred over copying a byte-level
`utf8-ranges` dependency unless profiling and footprint prove that a byte FSA
is materially better. Do not port `num-traits`. Do not add a special hash
function until measurements prove a trusted-corpus map needs one; deterministic
sorted chunk builds and dense buffers should eliminate most hot hash maps.

This implementation uses ordinary typed list pipelines and numeric/byte
buffers. Compiler-selected indexes for generic typed operations are allowed,
but a handwritten Rust search intrinsic, `List/query`, or query-specific
runtime engine is not evidence for pure interpreted BoonSearch.

### Parallelism

Index construction is the primary automatic-parallel story:

```text
document chunks
  -> local normalized term/posting runs
  -> deterministic sorted merge
  -> dictionary + compact postings
```

Small interactive queries normally remain sequential. Parallel query work is
eligible only after a cost model sees enough shards, matching terms, or posting
work. Batch-query throughput is a separate natural parallel scenario. Every
parallel reduction has a stable merge order and identical top-k output.

### Browser Footprint

The browser package separates:

- the generic Boon Wasm interpreter;
- the BoonSearch source and verified `MachinePlan` artifact;
- an optional separately compiled index-builder `MachinePlan` only after the
  generic lazy executable contract below exists;
- a separately cached index blob;
- minimal platform boot, DOM, audio-independent worker, and storage glue.

Large datasets and indexes are lazy, checksum-verified, and not committed
without an explicit license review. The repository ships a small synthetic or
permissively licensed fixture sufficient for correctness. External corpus
downloads are untimed setup and their provenance/digest is recorded.

Browser footprint is an acceptance budget, not a dashboard curiosity. Report
and budget compressed and uncompressed bytes separately for the generic
runtime Wasm, BoonSearch source/`MachinePlan`, boot/worker/storage glue, initial
fixture, optional builder artifact, and separately cached index. Also budget
cold download/compile/instantiate/plan/load time plus startup and steady peak
memory. Freeze numeric thresholds from the first honest baseline before the
browser implementation is accepted; large corpora/indexes and unrelated Lab
modules cannot be hidden inside the initial payload.

### Evidence

Measure index-build bytes, documents, tokens, unique terms, and postings per
second; serialized size; peak memory; allocations; load/first-query time; warm
p50/p95/p99; batch throughput; 1/2/4/8-worker scaling; scheduler crossover;
and core time separately from UI rendering. Correctness includes canonical
ids/scores, Unicode fixtures, edit operations, normalized/original span maps,
snapshot isolation, stable ties, and relevance metrics where judged data is
available.

## BoonTune

BoonTune is the portfolio's useful FFT application, real-time deadline test,
browser audio proof, and first FPGA product endpoint.

### User Experience

The preview shows:

- detected note and octave or selected guitar string;
- a large signed cents needle and flat/in-tune/sharp state;
- measured frequency, input level, confidence, and calibration A4;
- standard, drop, and chromatic tuning modes;
- waveform and optional FFT spectrum/harmonic markers;
- selected execution backend and why it was selected;
- frame deadline, dropped/stale frame, and capture-overrun counters;
- deterministic fixture input and live microphone input.

### Detector

Do not choose the largest FFT bin as the final pitch. The production path is:

```text
canonical mono PCM
  -> DC blocker/noise gate/declared filtering and decimation
  -> overlapping window with fixed coefficients
  -> FFT -> power spectrum -> inverse FFT/autocorrelation
  -> squared-difference function
  -> cumulative-mean normalized difference
  -> threshold/lag candidate -> interpolation
  -> note/cents mapping
  -> confidence gate and temporal smoothing
```

YINFFT keeps FFT central while avoiding many harmonic/octave failures of a raw
spectral peak. The visible forward-FFT spectrum and harmonic product spectrum
are diagnostics or coarse candidates, not the pitch oracle. Initial analysis
profiles use power-of-two windows and a fixed radix-2 FFT; a general
RustFFT-sized planner is not required.

Before optimizing, freeze `tuner_semantics_v1`: canonical input and analysis
sample rates; frame and hop sizes; exact window coefficients; channel/DC/filter
state and reset rules; FFT sign and normalization; autocorrelation and
difference equations; cumulative-mean normalization; lag bounds;
threshold/fallback choice; interpolation; confidence and level definitions;
gate; smoothing/hysteresis; equal-temperament note rounding including ties;
and the sample position assigned to a result. It also freezes string numbering,
standard E2-A2-D3-G3-B3-E4 targets, every named drop-tuning target set,
selected-string versus automatic-string behavior, nearest-target/tie rules,
accepted frequency range, cents reference, note spelling/accidental policy,
and out-of-range result/display behavior. Chromatic mode maps to the nearest
equal-tempered note; guitar modes map only under their declared string policy.
Fixtures retain selected intermediate traces for FFT/power,
autocorrelation/difference, threshold selection, and interpolation so a
matching final note cannot hide an incorrect pipeline.

Pitch mapping uses configurable A4, initially 440 Hz, and the normal equal-
temperament relationship. Cross-backend results use a bounded structural
record such as validity, note id, signed cents in fixed units, confidence,
level, and input sample index. Floating and fixed-point profiles declare their
tolerances.

### Native And Browser Audio

The audio callback/worklet only captures, downmixes, and writes preallocated
buffers. It performs no allocation, logging, blocking, interpreter execution,
FFT, or UI work.

- Native maps a generic Boon audio-input source to the platform audio host.
- Browser maps it to permission-controlled `getUserMedia` and AudioWorklet.
- A persistent compute worker consumes the newest complete frames.
- UI consumes bounded pitch results at approximately 20-30 updates/second.
- Backpressure drops superseded analysis frames rather than accumulating stale
  latency; every drop is counted.
- A browser without shared memory uses transferable/chunk messages. A secure
  cross-origin-isolated deployment may use shared Wasm memory and atomics.

Native and browser capture request the least-processed available stream.
Browser requests echo cancellation, noise suppression, and automatic gain
control off; native records any device/OS processing it can query. Both record
requested and effective device, processing, rate, channels, channel
selection/downmix, resampler/filter version and group delay, clipping, and
canonical-PCM settings/digest. Unknown or uncontrollable processing is an
evidence flag. Live microphone results are usability evidence; cents-accuracy
claims on recorded guitar require independently established pitch truth.
Acquisition and stable-state boundaries are defined in frames/sample positions,
not inferred from UI arrival time.

Live single-channel tuning is expected to remain CPU work because worker-pool
and GPU dispatch overhead may exceed the kernel. Offline corpus analysis,
multiple channels, spectrograms, and large batches are the worker/GPU proof.

### Deterministic Audio Fixtures

Fixtures include exact tones, cents offsets, amplitude ramps, missing
fundamentals, strong harmonics, attack transients, silence, noise, string
transitions, low/drop tunings, and short licensed/recorded guitar samples.
Record expected note, cents interval, confidence policy, acquisition bound, and
allowed settling window. Do not put an expected note label in the PCM transport
sent to a backend.

Initial product targets are goals to validate, not claims:

- synthetic median error at most 0.2 cent and p95 at most 1 cent;
- independently referenced recorded/live-guitar corpus median absolute error
  at most 1 cent and p95 at most 3 cents;
- zero accepted high-confidence octave errors in the canonical corpus;
- clean E2-E4 acquisition near 100 ms, with a 150 ms allowance for difficult
  low/drop/microphone cases;
- stable display within 150-250 ms;
- detector p99 below the configured hop deadline and zero capture overruns.

Every generated fixture has a versioned generator specification: phase,
amplitude, quantization, clipping, duration, channel policy, and seed where
applicable. It is materialized before any timed run. The golden identity is the
digest of canonical integer PCM after normalization. Interpreter, browser, and
FPGA receive the exact same stored/captured bytes; the FPGA transport stream is
capturable and replayable. Fixture acceptance distinguishes `acquiring`,
`stable`, `no_signal`, `unstable`, clipping, and detector-error states.

### PC-Mediated USB-To-FPGA Architecture

The USB microphone connects to the PC, not directly to the FPGA. The PC is the
USB Audio host and provides two interchangeable sources:

```text
standard USB microphone --+
                          +-> canonical PCM framer -> board transport -> FPGA
known WAV/tone generator --+
```

The PC may perform declared platform normalization such as channel selection
and sample-rate conversion, but it does not detect pitch. The exact canonical
PCM bytes sent to the board can also be sent to the interpreter oracle.

The complete path distinguishes three clocks: USB-microphone capture clock, PC
transport clock, and FPGA DSP/display clocks. Audio time is always derived from
absolute sample index and declared sample rate, never packet arrival time.
Supported run modes are:

- `live_realtime`: nonblocking latest-useful audio. Credit exhaustion creates
  an indexed discontinuity, drops superseded samples, and resets overlap,
  filter, and smoothing state before reacquisition;
- `fixture_realtime`: lossless real-time pacing for deterministic latency and
  deadline tests; insufficient sustained transport goodput is a failure;
- `fixture_batch`: lossless credit-paced correctness/resource testing, never
  presented as live-latency evidence.

Each transport profile proves sustained payload goodput with margin above its
PCM rate before live tuning is eligible.

Real-time evidence separates acquisition in input samples, detector compute
cycles, FIFO/queue wait, PC-to-board transfer, result return, and
host-callback-to-atomically-latched-display wall time. Host logs callback,
send, and receive timestamps. FPGA results include DSP-start, result-ready, and
display-latch cycle counters plus clock frequency, correlated by
run/config/sample/display ids. Unsynchronized host and board clocks are never
subtracted directly.

Define two versioned input profiles:

- `pcm12k_mono_s16`: simple first hardware profile; PC performs declared
  downmix/resampling outside the FPGA kernel timing boundary;
- `pcm48k_mono_s16`: later full-band profile in which FPGA-side filtering and
  decimation are part of the measured pipeline.

The board link is transport-neutral. Depending on the selected board it may be
USB bulk, USB CDC/high-speed bridge, a sufficiently fast UART bridge, or
Ethernet. Board selection and electrical transport are deployment profiles,
not changes to the Boon tuner algorithm.

The bounded, versioned protocol is a resynchronizable state machine, not just a
packet struct. Every envelope contains magic and kind, bounded header/payload
lengths, protocol/semantic/config versions, run and configuration epochs,
sequence, absolute sample range, and a specified CRC coverage. Start/config
negotiation returns an acknowledgement and effective first sample. Message
kinds cover audio, credit, result, error, reset, flush/end, acknowledgement,
and orderly teardown. Oversize input is rejected before allocation/FIFO use.

Versioned audio frames contain at least:

```text
protocol version
run id
sequence
sample rate
channel count
sample format/endian
absolute first-sample index
sample count
PCM payload
integrity check
```

Control messages configure reset, calibration A4, tuning mode, analysis
profile, and bounded FIFO credit/backpressure. Gaps, duplicates, invalid
frames, FIFO overrun, and underflow are visible errors.

Each transport profile freezes the credit unit, maximum outstanding credit,
grant sequence, and acknowledgement/retry rules. Grants and consumption are
idempotent and cannot wrap into extra capacity; a lost or duplicated credit or
acknowledgement cannot deadlock a fixture stream or permit FIFO overrun.

Identical duplicates are idempotent; conflicting duplicates are errors; gaps
reset stream-dependent DSP state; stale run/config epochs are rejected. The
protocol specifies sequence/counter wrap, timeout, resynchronization, abort,
disconnect, and teardown. Negative tests fuzz truncation, corrupted lengths or
CRC, reordering, duplication, wrap, stale epochs, invalid formats, and all
bounds. Live credit exhaustion follows the declared discontinuity policy;
fixture modes never silently drop.

The FPGA returns a machine-readable result associated with the input sample
range:

```text
run/config/semantic epochs and result sequence
analysis-window and consumed sample ranges, including result sample position
valid/no-signal/unstable/error status
note/string and octave
signed cents in fixed units
confidence and level
logical display characters
latched logical display frame id, per-digit segment masks, and indicator state
deadline/overflow counters
protocol/DSP error flags
```

The board drives the physical display from the same atomically latched logical
display frame reported to the PC. Automated evidence compares pitch, logical
glyphs, segment masks, and simulated/probed multiplex waveforms with host-side
expectations. Frame changes swap only at a scan boundary, with no torn digit.
Manual viewing remains a separate physical-wiring confirmation; a camera is not
the correctness oracle. A watchdog/disconnect state clears a stale musical note
to a defined no-signal/error frame.

Display profiles:

- one digit plus LEDs: string number `1..6`, with flat/in-tune/sharp LEDs;
- four digits: compact note/octave/signed-cents presentation;
- six or more digits: explicit note, accidental, octave, and signed cents.

The multiplex scanner refreshes independently from DSP result cadence. A
versioned glyph table covers notes, accidentals, sign, blank, no-signal,
unstable, error, clipping, and out-of-range cases. Digit/segment order,
decimal-point/indicator mappings, common-anode/common-cathode polarity,
blanking, refresh rate, duty cycle, and pin assignments live in the board
profile. Simulation verifies all masks, polarity, scan timing, blanking, duty,
atomic updates, and absence of ghost/torn frames; later probe evidence verifies
the physical waveform separately.

### FPGA Lowering

The bounded hardware pipeline owns FIFOs, fixed-size windows, fixed-point
window coefficients, FFT/YINFFT stages, peak interpolation, confidence, note
lookup, smoothing, and display registers. Transport, DSP, and display clock
domains use explicit asynchronous FIFOs or proven synchronizers/handshakes and
synchronized reset release. Clock-domain crossing, reset, FIFO full/empty, and
disconnect behavior are simulation properties, not board-only assumptions.

A bit-accurate model is derived from the same Boon tuner semantics and hardware
profile. The profile freezes coefficient quantization, every bit width and
binary point, rounding, saturation/wrap behavior, stage scaling, and overflow
policy. Fixture stage traces compare model and generated RTL; unexpected
overflow is a failure rather than a hidden quality reduction. Fixed point is a
generated backend representation, not a second normal Boon source-number type.
Resource reports include LUTs, DSPs, BRAM, maximum clock, frame latency,
initiation interval, scaling/overflow events, CDC results, and numerical error
against the CPU interpreter.

Bring-up order is synthetic PCM simulation, transport loopback, known-tone
hardware input, machine-readable pitch output, display-register verification,
atomic multiplex-waveform verification, then live USB microphone through the
PC.

FPGA remains a backend of the same `boon_tune` application and fixture ids; it
does not add a board-specific preview example. Every result records source,
semantic/config, fixture, bridge, board profile, protocol, generated RTL,
bitstream, and board identity digests/versions. Simulation is mandatory even
when hardware is absent. An unavailable board reports `HardwareUnavailable`,
distinct from failed correctness. Browser-to-board/WebUSB is outside this
portfolio phase; the initial hardware bridge is desktop/PC mediated.

## Mandelbrot Explorer

Mandelbrot is the first compact numeric/scheduler/GPU proof. Each output pixel
maps independently to a complex coordinate and iterates `z = z*z + c` until it
escapes or reaches the iteration limit.

The preview provides pan, zoom, iteration limit, palette, reset, backend mode,
and progressive quality. Rendering is tiled. A viewport change increments a
generation, cancels or discards stale tiles, prioritizes visible coarse tiles,
and progressively refines without blocking input.

Named profiles freeze width/height, viewport bounds, coordinate transform and
pixel-center rule, escape radius, maximum iterations, and numeric precision.
The raw iteration-count buffer is the primary oracle; the palette image is a
secondary presentation artifact. CPU/native and CPU/browser use canonical
binary64 `Number`. `ApproxF32` GPU profiles use a precision-matched reference
and declared raw-buffer/image mismatch metrics. Each backend publishes a safe
zoom limit or reports `NotEligible`; a collapsed deep zoom is not accepted as a
faster image.

Evidence includes:

- canonical viewport/coordinate mapping and exact iteration buffers for the
  CPU reference profile;
- declared tolerance/image comparison for lower-precision GPU profiles;
- pixels/second and actual iteration count/second;
- first useful frame and time to final quality;
- 1/2/4/8-worker speedup and efficiency;
- task count, steals, cancellation latency, stale work, and tile granularity;
- CPU-to-GPU transfer, dispatch, kernel, readback, and presentation stages;
- identical output under different worker completion orders.

The interpreter implementation must pressure dense numeric buffers and loops,
not allocate one object per pixel or complex number. GPU WGSL is generated from
the pure pixel/tile kernel. The host owns scheduling, buffers, and presentation,
not a duplicate fractal implementation.

## Path Tracer

Path Tracer is the advanced compute example after Mandelbrot proves the basic
kernel/backend path. It demonstrates work used in film, architectural/product
rendering, game-light baking, synthetic sensor data, and physically based
graphics.

Initial scene scope:

- spheres and a floor;
- diffuse, metal, emissive, and dielectric materials;
- movable camera and material/light controls;
- progressive accumulation and visible sample count;
- bounded bounce depth and Russian roulette only after the simple reference is
  correct.

Every random sample comes from an exact integer counter-based generator keyed
by `(scene_version, pixel, logical_sample, bounce, dimension)`. Logical sample
ids and retry keys are idempotent. Per-pixel samples accumulate in a fixed
logical order or specified fixed reduction tree; schedule-independent random
numbers alone are insufficient because floating addition order also matters.
Worker order, retries, and tile assignment therefore cannot change the CPU
reference image. Camera/scene changes create a new generation and
cancel/discard stale accumulation.

Later scope adds triangles, meshes, and a Boon-owned BVH. Build and traversal
are separate benchmark kernels. Portable WGPU/WebGPU uses explicit compute
intersections; hardware ray-tracing extensions are not a portable acceptance
dependency.

Evidence includes rays/second, samples/second, first usable image, time to fixed
samples-per-pixel, worker/GPU scaling, cancellation, memory, BVH build/traversal,
and image comparison against a deterministic CPU reference. It records complete
scene/camera/material/profile digests, geometric intersection and material
unit oracles, finite-value/NaN checks, energy bounds, and a defined image-error
metric. Generated GPU output is evaluated under its named precision tolerance,
not claimed bit-identical to binary64 CPU accumulation.

## Boon Fingerprint

Boon Fingerprint is a pure-Boon file/byte-stream application and exact-output
BLAKE3 optimization kernel. Reference algorithm, specification, and test
vectors come from <https://github.com/BLAKE3-team/BLAKE3>; no Rust
implementation is added to or invoked by this repository for comparison. An
early implementation is labelled `BLAKE3 hash-mode subset`, not generally
`BLAKE3 compatible`, until it also implements XOF/seek, keyed hashing, and
derive-key mode and passes their official vectors.

The preview supports generated data and user-selected files, streaming
progress, digest output, chunk/tree visualization, cancellation, and
single/automatic CPU modes.

The implementation begins with the readable scalar algorithm and official
vectors:

- 1 KiB chunks and block compression;
- exact modulo-32-bit word addition, XOR, logical shifts, and rotations over
  whole `Number` values whose domain is `0..2^32-1`;
- explicit little-endian fixed-size byte/word conversion;
- chaining values and deterministic binary tree reduction;
- streaming updates without retaining the entire input;
- 64-bit chunk/output counters represented as two exact 32-bit Number words or
  `BYTES[8]`, with explicit carry, low/high compression lanes, and stream-length
  overflow detection; never as one inexact Number beyond `2^53`;
- XOF output/seek, keyed, and key-derivation modes after base hash mode is
  correct, before the full compatibility label is used.

Large contiguous correctly aligned subtree ranges are natural worker tasks;
dispatching each 1 KiB chunk separately would make overhead dominate. The
runtime chooses a sequential path below a measured threshold and a deterministic
tree schedule above it, preserving absolute chunk counters and canonical
parent-stack merges. GPU hashing is optional research and not a portfolio
acceptance requirement.

Evidence includes official vectors for every implemented mode and output/seek
shape, incremental-vs-one-shot equality, chunk-boundary/adversarial sizes,
digest stability across workers, bytes/second, allocations, memory, task
granularity, and tree-reduction overhead. Full portfolio completion requires
all official modes/vectors. Until an independent security review, label the
code as an educational compatibility implementation rather than a
security-audited cryptographic library.

## Boon Runtime Lab

Boon Runtime Lab is one public application with a common runner, result
protocol, suite manifest, and five suite cards. It is not one giant source file
and it contains no Rust or JavaScript workload implementation.

### Suites

1. **Language Core — AWFY**: faithful Boon ports of selected
   <https://github.com/smarr/are-we-fast-yet> workloads. Start with Mandelbrot,
   NBody, Richards, DeltaBlue, JSON, and Storage, then expand. Canonical cases
   remain sequential; separately named throughput/parallel experiments cannot
   replace canonical results. Each case pins upstream revision, harness and
   inner iterations, algorithm, data structures/control and operation counts,
   input, and result oracle. A Boon representation is canonical only when it
   preserves that declared work; a material immutable/dataflow adaptation that
   changes object/allocation/control work is labelled `AWFY-derived`, not
   silently presented as the canonical AWFY workload.
2. **Streaming Data — 1BRC task**: parse `station;temperature`, aggregate
   min/mean/max, and emit canonical sorted output. Scaled profiles are named
   `1BRC task · 1M`, `10M`, or `100M`; only the exact billion-row contract may
   be labelled `1BRC · 1B`. The semantic profile freezes UTF-8/name rules,
   integer-tenths parsing, accumulator range/overflow, exact mean rounding, and
   output ordering and grammar.
3. **Parallel Science — NPB**: begin with exact NASA EP and IS class S/W
   parameters and verification, then CG. Official class names are used only for
   exact published sizes and algorithms. Every case pins NPB specification,
   release, language-independent flavor, class parameters, and verification
   values. Modified kernels are `NPB-derived`.
4. **Compression — LZ4**: pure-Boon block decompression first, then compression
   and frame parsing. Validate against official format vectors and independently
   encoded fixtures. A raw-block profile provides and limits the expected output
   size because the block is not self-describing. Tests cover zero/invalid
   offsets, overlapping match copies, truncation, malformed lengths, and every
   input/output bound failure. Round-trip through the same implementation is
   insufficient.
5. **Runtime Probes**: microbenchmarks extracted from the public examples and
   generic runtime operations.

Runtime Probes eventually include:

- BoonSearch normalization, trie/FSA traversal, posting intersection, scoring,
  fuzzy row, and top-k;
- BoonTune radix-2 FFT, YINFFT window, note mapping, and ring-buffer movement;
- Mandelbrot pixel/tile kernel;
- Path Tracer RNG, intersection, material scatter, and BVH traversal;
- BLAKE3 compression and parent reduction;
- canonical/advanced Cells dirty propagation, indexed lookup, chain, fanout,
  and unchanged cutoff;
- parser/typechecker/plan startup, value dispatch, numeric arrays, bytes,
  allocation, calls, lists/maps/sets, and scheduler spawn/reduction thresholds.

Microprobes are diagnostic and never the sole optimization acceptance. They use
runtime-opaque deterministic inputs and consumed checksums to prevent constant
folding/dead-work removal. Results attribute timings and typed counters to
stable source/typed-IR spans where possible: IR operations, dispatches,
bounds checks, allocations, specialization decisions, task transfer, and
reduction work. Every optimization must still pass semantic/differential tests
and improve or preserve the end-to-end product workload; no probe-specific
special case is accepted.

Each suite/case records algorithm/spec provenance: upstream URL and exact
revision/version, files or specification text used to derive the independent
Boon implementation, SPDX/license/notice, modification notes, fixture/asset
license, and generator version/seed. Rust or JavaScript workload source is not
vendored as a comparison or implementation aid. Ordinary browser bootstrap is
platform glue, not workload code.

### Preview UX

Header controls:

- engine: `Interpreter`, later generated backends when real;
- target and artifact revision;
- profile: `Preview`, `Measure`, `Extended`;
- CPU policy: `1`, `Auto`, or an explicit sweep;
- estimated duration, memory, and download;
- `Run All`, `Run Selected`, `Stop`, `Rerun failed`, and raw-result export.

Each suite card shows selection, workload/profile, supported worker choices,
provenance, progress, correctness, cold stages, median, supported percentiles,
variability, throughput, workers used, speedup versus the same-run one-worker
case, and raw samples. Unsupported p95/p99 renders as
`Unavailable(insufficient_samples)`, never as an invented percentile.

`Run All` executes every case in the active profile, including cases currently
filtered from view. `Run Selected` executes explicit user selections.
Unsupported cases remain in the result as `NotEligible`; neither action
silently omits them. Cases execute sequentially so suites do not contend, and
the runner confirms worker/GPU/resource quiescence between cases, records case
order, completes downloads before timing, and throttles UI updates outside
timed samples. Where feasible, one-worker and Auto treatments are interleaved
or counterbalanced to reduce warm-cache and thermal-order bias. A workload may
use internal workers. Browser compute is off the UI thread; `1 worker` means
one compute worker plus the UI coordinator.

Do not show a global score or geometric mean across unlike workloads. Preview
results are visibly labelled `demonstration, not benchmark-grade`.

### Profiles

| Profile | Browser | Desktop |
|---|---|---|
| Preview | Representative AWFY subset, 1BRC-task 1M, NPB S subset, small LZ4 fixtures, probe smoke set | Same portable set for same-machine host comparison |
| Measure | Full supported AWFY, 1BRC-task 10M if preflight passes, NPB S/W, 32-64 MiB codec corpus, 1 and Auto workers | Full AWFY, 1BRC-task 100M, NPB W/A where supported, larger codec corpus, 1 and Auto |
| Extended | Explicit opt-in browser-safe large variants | 1B rows, larger official NPB classes, full corpora, explicit worker sweep |

Sizes are starting targets and may be lowered by measured interpreter/browser
limits, but labels must remain honest. Dataset generation/download is outside
kernel timing unless a case explicitly measures end-to-end I/O.

### Runner Lifecycle

Every case implements:

```text
load assets
-> prepare deterministic input
-> untimed correctness probe
-> calibrate fixed inner batch
-> warm up
-> collect fixed raw samples
-> verify output
-> release resources
```

The runner records asset fetch, runtime load, parse/typecheck/plan or generated
compile, input preparation, worker startup, kernel execution, verification, and
release separately. Cold and steady views are distinct.

Cold scopes are named rather than collapsed: download and HTTP-cache state,
Wasm download/compile/instantiate, source parse/typecheck/plan, worker startup,
GPU adapter/device/pipeline and first dispatch, data/index load, and OS/page
cache where observable. An already running Lab cannot measure its own true
process/page cold start; that requires repeated trials driven by a generic
host-owned launcher. Unknown or uncontrolled cache state is flagged and never
called cold. A statistic such as p95 is suppressed when the raw sample count is
insufficient to support it.

Measure uses a monotonic clock, a batch long enough to exceed timer resolution,
a declared warmup, fixed sample count, median, eligible p95/p99, MAD/IQR,
min/max, and raw values. It never reports only the fastest sample.
Hidden/background browser
pages, coarse timers, worker restarts, resource-preflight failures, thermal or
power-mode changes where detectable, and correctness failures create visible
validity flags.

### Correctness

- AWFY retains upstream problem sizes and magic/verification results.
- 1BRC uses an independently generated canonical sorted digest and fixtures for
  UTF-8, line splits, negatives, ties, rounding, final lines, and many stations.
- NPB records official class verification values/norms and tolerances.
- LZ4 records input/output hashes, the declared output bound, and decodes
  independently encoded valid and malformed fixtures.
- Microbenchmarks return a deterministic consumed result/checksum and operation
  count so future codegen cannot eliminate the work.

Performance is summarized only after verification. Failed, skipped,
unsupported, and cancelled cases remain in the audit result.

### Cancellation

Every run has a generation-scoped cancellation token. Stop prevents new work,
checks between bounded chunks/iterations, acknowledges worker release, and
terminates an unresponsive isolated worker after a grace period. Late results
from an old generation cannot alter a new run. Cancelled samples are retained
as partial audit records and excluded from timing summaries. Cancellation
latency is itself a scheduler metric.

### Result Protocol

The versioned result records at least:

```text
schema and run id/status
suite/workload/semantic version/profile/timing scope
source, plan, engine, and artifact digests
target/runtime/OS/architecture/environment metadata available to that host
input id, size, seed, and digest
workers requested/effective and scheduler policy
warmups, batch size, samples, timer resolution, and timing-source metadata
correctness oracle, expected/actual digest or tolerance
cold/setup/transfer/dispatch/presentation/readback stages and raw steady samples
work units, throughput, memory, allocations, tasks, speedup, efficiency
validity flags and cancellation/failure diagnostics
```

Metrics unavailable to a host, such as browser allocation/peak-memory detail or
GPU device timestamps, serialize as `Unavailable` with a reason, never numeric
zero.

Comparison uses two explicit field sets:

- `equivalence_key`: workload semantic version, complete parameters, input
  digest, work/precision definition, timing scope, physical machine/hardware
  identity, and all other conditions fixed by that comparison;
- `treatment_dimensions`: the explicitly selected variables, such as
  source/artifact revision, engine/backend, native-versus-browser host
  target/runtime, scheduler/worker policy, or another implementation choice.

Only records with matching equivalence fields may be compared. Treatment
dimensions selected by the comparison are expected to differ; every unselected
dimension must still match. Thus a native-versus-browser trial may vary the
host target/runtime on the same physical machine, while an ordinary regression
keeps host target/runtime fixed. Merely sharing a workload name never makes two
records comparable.

### Bundle Strategy

The initial browser bundle contains the shell, suite manifest, result schema,
small correctness fixtures, and smallest probes. Suite executables and large
assets are versioned, lazy-loaded where the generic artifact contract permits,
hash-verified, and cached. `Run All` completes all required downloads before
kernel timing. Cache size and clearing are visible.

1BRC small data may be deterministically generated outside timing. NPB creates
its defined arrays from seeds. Larger LZ4 corpora are opt-in and license-
reviewed. No corpus is committed merely because another benchmark repository
uses it.

### Lazy Executable Artifact Contract

`MachinePlan` remains the only dynamically loadable Boon application/module
artifact for the interpreter, and `MachineInstance` the only mutable
interpreter execution owner. Generated WGSL, future native code, or RTL are
derived, digest-bound backend artifacts, never alternate authored source
modules. Every lazy interpreter executable is compiled from visible Boon source
and records source/artifact digest, typed import ABI, required capabilities,
semantic/profile id, and compiler version. Loading verifies these before
instance creation. Each instance owns isolated state, cancellation, resource
bounds, and failure reporting; a failed suite cannot corrupt the shell or
another suite.

Generic dynamic `MachinePlan` loading/linking must be designed and verified
before Runtime Lab or BoonSearch relies on it. Until then, executable code stays
inside the main verified `MachinePlan` and only fixtures, corpora, indexes, or
other data load lazily. The portfolio must not invent an unverified second
executable `module` format.

## Shared Engine Work Driven By The Portfolio

The examples are optimization customers, not reasons for native shortcuts.
Profile first, extract a reproducible probe, repair the generic owner, validate
all affected examples, and then update Runtime Lab.

### Document And Input Semantics

The canonical 7GUIs require generic support for:

- select/combobox and one-selection listbox semantics;
- slider/range input with continuous pointer and keyboard changes;
- progress/gauge semantics;
- modal dialog and context-menu overlay/focus behavior;
- disabled event suppression across every input route;
- secondary pointer buttons, down/up/move, pointer capture, and local geometry;
- generic positioned vector/circle drawing if existing scene primitives are
  insufficient;
- deterministic single/double activation recognition and replay rejection;
- source-versus-programmatic text-update origin, caret preservation, and IME
  composition safety;
- an injectable virtual monotonic clock plus time-sensitive scheduled, pointer,
  keyboard, accessibility, and scenario source envelopes carrying observation
  timestamps from one runtime-local monotonic epoch and a deterministic source
  sequence. Timer settles through each event timestamp before applying it, and
  TEST replaces this authority with the virtual clock;
- accessible roles, labels, values, selected/invalid/disabled states, focus,
  and keyboard operation.

Visual components can be written in Boon. Platform event delivery, semantic
roles, focus/capture, disabled enforcement, and accessibility are engine/host
responsibilities.

### Numeric And Memory Model

Boon source keeps one visible finite IEEE-754 binary64 `Number` type and
immutable `BYTES`; this portfolio does not introduce a public integer/`u32` or
standalone byte scalar, nor a casually mutable collection. Performance work
must preserve those observable semantics while enabling:

- dense lowered numeric and byte storage, efficient read-only slices/views,
  fixed-size arrays, complex-number layouts, and compact immutable serialized
  structures;
- compiler-proven nonescaping/single-owner scratch or builder regions for
  bounded in-place kernels, frozen when published through pure source
  semantics;
- exact width-constrained word operations over whole Number values in
  `0..2^32-1`: modulo addition, XOR, logical shifts, rotations, range errors,
  and explicit little-endian conversion to/from immutable BYTES;
- an internal typed-IR `Word32` lane/array refinement only when range/width is
  proven, with checked boundaries and no observable second scalar type;
- `sqrt`, `log2`, trigonometry, interpolation, finite-number diagnostics, and
  complex layouts without per-value heap objects;
- allocation-free repeated execution, reusable plans/twiddle tables,
  predictable bounds checks with safe proof-based elimination/hoisting, and
  lazy immutable data loading where the host supports it;
- generated fixed-point hardware profiles with explicit widths, scaling,
  rounding, and overflow, without changing normal source Number semantics.

Before the first compute lowering in Phase 3, an architecture checkpoint
updates the authoritative language/IR contracts for pure kernel boundaries,
Number-preserving dense storage, compiler-proven scratch/builders, and named
reduced-precision backend profiles. Before BLAKE3 in Phase 5, a second
language/BYTES/IR checkpoint freezes exact word operations, Word32 proof lanes,
counter pairs, and endian boundaries. Do not smuggle bit operations through
approximate floating arithmetic. Differential tests compare boxed-Number
reference behavior with every unboxed/Word32/scratch optimization, including
errors and publication boundaries.

These are generic language/IR/runtime capabilities. Do not add `BLAKE3/round`,
`Search/score`, `Tuner/pitch`, or `Mandelbrot/pixel` Rust intrinsics.

### Automatic Task Runtime

The task runtime needs:

- pure-kernel eligibility derived from typed IR;
- estimated work and target-specific spawn/transfer thresholds;
- deterministic fixed reduction trees and stable top-k/merge order;
- work stealing for sufficiently large independent tasks;
- long-lived workers for streaming/real-time pipelines;
- latest-wins generations, bounded cooperative CPU cancellation points, and
  stale-result rejection;
- bounded queues, backpressure, and UI-core reservation/cooperative yielding;
- task, steal, work, span, frontier width, utilization, wait, transfer, and
  cancellation metrics;
- an explanation of `Auto` choices, including why sequential CPU was selected.

Small search queries, one live tuner channel, and tiny GUI interactions should
remain sequential. Chains in Cells cannot be parallelized; wide ready
frontiers may be. Parallelism is a runtime choice only after its overhead is
covered.

Application authors express pure chunkable kernels, typed pipelines, and stable
reductions, not threads or a hardcoded worker count. The compiler/runtime
derives eligibility and partitions work from typed IR, profile size, target
costs, and current load. An algorithm may expose natural shard, tile, or
subtree boundaries, but `Auto` owns whether and how many workers execute them
and records the decision. Explicit worker counts exist for measurement and
diagnosis, not as a different application implementation.

Generated GPU dispatch cannot promise in-kernel preemption. GPU cancellation
uses bounded dispatch sizes, stops future dispatch, discards stale-generation
results, and records already-submitted/wasted GPU work and time.

### Browser Runtime

Browser work requires:

- compact Wasm interpreter and verified `MachinePlan` artifact loading under
  the lazy executable contract;
- independent workers without cross-origin isolation, each with a separate
  interpreter instance, plan, and Wasm linear memory. Nonshared transport moves
  JS-owned transferable `ArrayBuffer`s and measures copy into/out of Wasm plus
  duplicated runtime/plan memory; it does not claim zero-copy ordinary Wasm
  memory;
- optional shared Wasm memory/atomics under secure cross-origin-isolated
  deployment as a distinct measured mode;
- deterministic worker lifecycle, cancellation, and actual worker-count
  reporting;
- Web Audio permission/capture adapters for BoonTune;
- WebGPU lowering for eligible pure kernels;
- storage/cache adapters for search indexes, lab assets, and result exports;
- visibility/timer-resolution/resource validity flags.

CPU browser execution remains interpreted. WebGPU is generated code from the
same typed IR and is labelled accordingly.

WebGPU evidence records adapter/device/features and the timing source.
`timestamp-query` is optional; without it, queue-completion host elapsed is
reported and never labelled kernel-only. Measurements separate adapter/device
setup, shader/pipeline compilation, first dispatch, upload, steady dispatch,
presentation, and validation readback. Readback is required for evidence but is
not forced into normal product-frame timing. Portable floating compute uses
named `ApproxF32`; portable WGSL `f64` is not assumed. Exact proven word lanes
may lower to WGSL `u32`.

### Code Generation

Future Rust and Zig backends consume the same authoritative typed IR, dense
slots, currentness, source routes, and kernel boundaries. They are additional
Runtime Lab engine modes only when they execute the same fixtures and emit the
same result protocol. Generated Rust/Zig artifacts are compiler output, not
handwritten source alternatives or cross-language comparison workloads. The
examples must not be rewritten per backend.

### Capability Ownership Matrix

This portfolio coordinates existing architectural owners; it does not replace
their contracts:

| Capability | Primary generic owner | Required contract update |
|---|---|---|
| syntax, types, one-Number/word semantics | parser, typechecker, typed IR | language, BYTES, and IR architecture docs |
| executable/lazy artifacts | compiler plan builder and plan loader | `MachinePlan`/ABI/loading contract |
| evaluation, currentness, dense slots/scratch | plan executor and runtime | executor/runtime architecture and differential tests |
| automatic CPU work and streaming workers | task runtime/scheduler | task eligibility, cost, reduction, cancellation contract |
| widgets, events, focus, accessibility | document model and host bridge | document/input semantics contract |
| native drawing and generated WGPU | native renderer/compute lowering | active native GPU pipeline plus compute contract |
| browser interpreter/workers/storage/WebGPU | browser host | browser host, isolation, artifact, and evidence contract |
| audio capture and real-time framing | native/browser audio adapters | audio source and callback/worklet contract |
| scenarios, measurements, reports | verifier/xtask and result-schema owner | versioned portfolio evidence schemas |
| FPGA lowering and board link/display | hardware compiler plus board adapter | RTL profile, protocol, CDC, and board-evidence contract |

Each capability change updates its authoritative architecture document before
or with implementation. This portfolio plan remains scope, dependency, and
acceptance guidance; it must not silently become a second incompatible engine
architecture.

## Benchmark And Performance Contract

The repository contains no cross-language workload ports or comparison
harnesses under this plan and makes no Rust-vs-Boon or JavaScript-vs-Boon
claim. Evidence answers:

- Is the Boon result correct?
- Is the interpreted product responsive enough?
- Which generic runtime cost dominates?
- Did a runtime/compiler change improve the same workload and machine?
- Does automatic parallelism beat the same engine's one-worker result?
- Does generated CPU/GPU/FPGA output satisfy its declared exact-or-tolerance
  oracle derived from the interpreter reference?

For every published measurement record:

- source and artifact revisions;
- target, engine, browser/runtime version, OS/architecture, available hardware
  facts, power/thermal mode where known;
- complete workload profile and input digest;
- cold stages separate from steady kernel work;
- warmup, sample count, raw samples, median, supported percentiles or an
  explicit `Unavailable(insufficient_samples)`, variability, and timing scope;
- correctness oracle and actual result;
- memory, allocations, transfers, task metrics, and cancellations appropriate
  to the workload;
- requested and effective workers/backends;
- validity flags.

Report two different scaling questions rather than conflating them:

```text
fixed_graph_speedup = fixed_graph_1_worker_time / fixed_graph_N_worker_time
fixed_graph_efficiency = fixed_graph_speedup / effective_workers

auto_end_to_end_speedup = policy_1_end_to_end_time / Auto_end_to_end_time
```

Fixed-graph scaling holds source, input, semantics, logical task graph/chunk
boundaries, engine, timing scope, and host constant to isolate execution
scaling. End-to-end Auto scaling holds semantic work, input, correctness,
engine, timing scope, and host constant but permits the runtime to choose a
different partition; it includes planning/scheduler overhead and records the
chosen graph digest, chunk sizes, task count, and effective workers for both
treatments. Never force an unnatural N-worker graph through the one-worker path
and call it Auto performance.

Do not change algorithms silently to create a favorable graph. Legacy,
improved, scalar, parallel, generated, reduced-precision, and GPU variants have
different semantic/profile ids where their work differs.

## Verification Contract

Every implementation slice adds evidence proportional to its risk:

1. parser/typechecker/IR/plan tests for new generic language capabilities;
2. executor/runtime tests for exact algorithms, currentness, cancellation,
   source routing, worker determinism, and negative cases;
3. example CLI scenarios for semantic behavior;
4. manifest validation and source-unit loading;
5. generic native dev+preview portfolio evidence consistent with the active
   `NATIVE_GPU_PIPELINE.md` source-driven two-window contract;
6. app-owned render/readback artifacts and exact frame/input identities;
7. browser scenarios using the same fixture ids and result oracle;
8. generated CPU/GPU exact equivalence for identical profiles, or the declared
   interpreter-derived tolerance oracle for approximate profiles;
9. BoonTune bit-accurate fixed-point equivalence, CDC/reset/FIFO simulation,
   bounded protocol evidence, machine-readable pitch, atomically latched
   display masks, simulated/probed multiplex waveform, resource/timing closure,
   and later separate physical-wiring observation;
10. Runtime Lab regression cases for every optimized kernel.

The verifier/xtask evidence owner defines a versioned, manifest-driven
per-example portfolio report schema covering semantic, native, and browser
stages. It consumes generic source/manifest/scenario/budget metadata and may not
branch on the thirteen ids. These reports prove portfolio examples; they are
not automatically native handoff gates.

The existing `verify-cells` command/report remains bound to manifest id `cells`
and therefore means Cells Advanced. `seven_guis_cells` receives a distinct
scenario and portfolio report. A new example enters
`docs/architecture/native_gpu_handoff_manifest.json` only through an explicit
schema, command, argument, and byte-budget decision. Until then, the
manifest-backed aggregate proves only the targets actually listed in that
manifest.

No native example is accepted because a screenshot merely looks plausible.
No FPGA tuner is accepted because a human recognized a displayed letter. Human
testing follows automated evidence and confirms usability/physical wiring.

When implementation stabilizes, run only the native handoff commands named by
`docs/architecture/native_gpu_handoff_manifest.json`, followed by its
manifest-backed aggregate. Do not expand that manifest for every benchmark
microkernel; add a new product gate only when the example is a declared native
handoff target and the manifest/schema budgets are designed accordingly.

## Implementation Phases

Phases are dependency ordered. Each example still completes its native-first,
browser-second vertical slice before being called complete.

### Phase 0 — Plan And Baselines

- Accept this plan and record unresolved hardware/dataset choices below.
- Preserve fresh baseline scenarios and reports for existing Counter, Cells
  Advanced, TodoMVC, NovyWave, and the generic native pipeline.
- Record interpreter startup, numeric loop, byte loop, list, allocation, and
  task-spawn baselines before portfolio-driven optimization.
- Design and accept the first architecture gate for pure kernel eligibility,
  Number-preserving dense/scratch lowering, and named approximate backend
  profiles before Phase 3 compute work.

### Phase 1 — Canonical UI Foundations And 7GUIs

- Add generic manifest category grouping without example-id branches.
- Implement/fix disabled behavior, select/listbox, slider/gauge, modal/context
  overlay, secondary pointer/capture, monotonic timer payload, and accessible
  semantics as required by the tasks.
- Add the seven canonical examples in increasing capability order:
  Counter, Temperature Converter, CRUD, Flight Booker, Timer, Circle Drawer,
  Cells.
- Preserve existing Counter exactly and preserve Cells Advanced behavior,
  scenarios, budgets, and proof contract; any shared-module refactor is
  separately behavior-preserving.
- Apply the Cells terminology migration across active prose while preserving
  all stable advanced machine identifiers.
- Add scenarios, budgets, native evidence, and then browser scenarios for each.

### Phase 2 — Runtime Lab Shell And Baseline Probes

- Implement the versioned suite manifest, generic runner, cancellation,
  correctness gates, raw result schema, Preview/Measure profiles, export, and
  lazy data loading. Add lazy `MachinePlan` loading only after its generic ABI,
  ownership, validation, cancellation, and isolation contract is implemented.
- Add generic interpreter/startup/numeric/bytes/list/allocation/scheduler probes.
- Do not wait for every suite or flagship kernel before shipping the shell.

### Phase 3 — Mandelbrot And Parallel/GPU Kernel Path

- Confirm the first architecture gate and its differential tests pass before
  adding any WGPU/WebGPU compute lowering.
- Implement scalar interpreter reference and progressive native UI.
- Add automatic CPU tiles with deterministic output/cancellation.
- Add generated WGPU and browser interpreter/worker/WebGPU `ApproxF32` modes
  with separately declared precision and timing sources.
- Extract pixel/tile/scheduler probes into Runtime Lab.

### Phase 4 — BoonSearch And Compact Collections

- Freeze query/relevance fixtures and implement a simple correct inverted index.
- Optimize dense documents/postings/score buffers/top-k.
- Add radix trie, serialization, delta/tombstone/compaction, then minimal DAFSA
  only when measurements justify it.
- Add deterministic parallel build and batch/shard modes.
- Complete native and browser product/footprint evidence and Runtime Lab probes.

### Phase 5 — Boon Fingerprint And Exact Integer/Byte Kernels

- Complete the mandatory one-Number/BYTES/typed-IR architecture checkpoint and
  add exact width-constrained word operations plus proof-specialized internal
  lanes and scratch ownership generically.
- Implement base hash-mode vectors first, then XOF/seek, keyed, and derive-key;
  add streaming, canonical subtree parallelism, native, and browser modes.
- Add exact Runtime Lab probes and keep GPU optional.

### Phase 6 — BoonTune Software Pipeline

- Freeze `tuner_semantics_v1` and implement deterministic offline FFT/YINFFT
  intermediate and final fixtures first.
- Add native capture/ring/deadline path and live preview.
- Add persistent worker, browser AudioWorklet/worker, and batch
  worker/WebGPU/spectrogram modes.
- Add accuracy/deadline/capture probes to Runtime Lab.

### Phase 7 — Path Tracer

- Reuse the proven numeric buffer, task, cancellation, RNG, and GPU paths.
- Implement spheres/materials/progressive reference, then workers and GPU.
- Add triangles/BVH as a separately accepted later slice.
- Add kernel probes and image evidence.

### Phase 8 — Runtime Lab Suite Expansion

- Add the selected AWFY workloads, 1BRC profiles, NPB EP/IS then CG, and LZ4
  decompression then compression/frame variants.
- Maintain source provenance and exact semantic/profile identifiers.
- Run all suites as Boon-only implementations.

### Phase 9 — BoonTune FPGA

- Select board, transport, input/display profiles, clocks, and fixed-point
  budgets from actual hardware.
- Implement the bounded transport-neutral protocol state machine and host
  fixture/live modes with explicit capture, transport, DSP, and display clocks.
- Prove bit-accurate/CDC/display simulation, board transport, pitch result
  readback, atomic display masks/multiplex waveform, resource/timing closure,
  then live microphone.
- Keep PC USB audio and board transport as platform adapters; DSP remains Boon.

### Phase 10 — Future Generated CPU Backends

- Add Rust/Zig generated execution only after an authoritative typed-IR backend
  and equivalence contract exist.
- Register them as Runtime Lab engine modes without changing example source.
- This phase does not authorize handwritten Rust/Zig workload comparisons.

## Risks And Mitigations

### Portfolio Scope

Thirteen examples can turn into parallel unfinished experiments. Complete
vertical slices and add their Runtime Lab probes before starting another large
kernel. 7GUIs tasks may share generic capabilities but remain independently
usable.

### Example-Driven Native Shortcuts

Pressure to make a demo pass can leak example names or algorithms into Rust.
Require a generic owner, negative test with another application shape, and no
example identity below catalog/editor boundaries.

### Interpreter Is Initially Too Slow

Do not disguise the result with a native intrinsic. Reduce temporary
allocations, specialize generic typed IR, improve data layout/calls/loops, and
publish honest absolute latency. Keep reduced demo profiles clearly labelled.

### Scheduler Overhead

Tiny work can become slower in parallel. Require one-worker baselines,
crossover measurements, deterministic merges, and `Auto` explanations.

### Browser Bundle And Memory

Thirteen examples and lab suites can bloat Wasm/startup. Keep one generic
runtime, lazy-load data, load executable `MachinePlan`s only through the
verified generic artifact contract, preflight memory, measure duplicated worker
instances/copies, and expose cache lifecycle.

### GPU Semantic Drift

Different precision/order can alter images and DSP. Keep one CPU reference,
declare precision profiles/tolerances, use portable `ApproxF32` rather than
assuming WGSL `f64`, generate kernels from typed IR, and record timing source,
setup, transfer, dispatch, presentation, and validation overhead separately.

### Audio Permissions And Real-Time Deadlines

Browser/native devices vary and callbacks cannot block. Always provide
deterministic fixtures, isolate capture, bound rings, drop stale analysis,
report requested/granted processing and actual sample/channel/resampling
settings, and measure acquisition/deadline failures from sample positions.

### USB/Board Transport

Boards commonly expose different USB roles and bridge speeds. Keep the framing
protocol transport-neutral, use credit/backpressure and sequence checks, and
select the actual transport only after board inspection.

### Fixed-Point FPGA Accuracy

FFT/YIN scaling can overflow or lose cents accuracy. Build a bit-accurate
software model, record every scale point, sweep fixtures before synthesis, and
report fixed-vs-reference error with hardware resources.

### Dataset Licensing And Benchmark Provenance

Interesting corpora may prohibit redistribution or lack stable provenance.
Commit only small synthetic/permissive fixtures after review; download larger
assets by explicit opt-in with URL, license, version, and digest.

Algorithm ports can also lose attribution or accidentally copy another
language implementation. For AWFY, NPB, LZ4, BLAKE3, YIN/YINFFT, FFT, search,
and path-tracing assets, record upstream specification/revision, exact material
consulted, SPDX/license/notice, modifications, fixture/vector origin, and
generator version/seed. Keep the implementation independently Boon-owned and
do not create uncommitted Rust/JavaScript comparison ports as a staging path.

### Misleading Benchmark Claims

Preview runs, mixed semantics, fastest-only samples, global scores, or failed
oracles can mislead. Enforce the Runtime Lab result schema, validity flags,
raw samples, exact comparison keys, and Boon-only claim scope.

### Existing Concurrent Work

The repository may evolve while this plan is implemented. Re-read active
architecture plans and the current handoff manifest at each phase boundary.
Resolve overlapping ownership deliberately; do not overwrite unrelated user or
agent changes.

## Deferred Non-Blocking Decisions

No decision here blocks writing or accepting this plan. Resolve each from live
evidence before the relevant implementation slice:

- exact FPGA board, PC-to-board transport, display digit count/polarity, and
  pin/clock constraints;
- exact browser cross-origin-isolation deployment used for shared-memory tests;
- initial large BoonSearch corpus and redistribution/download policy;
- initial LZ4 corpus beyond committed compatibility fixtures;
- exact Runtime Lab Measure/Extended durations after interpreter baselines;
- exact `ApproxF32` tolerance/eligibility limits per WebGPU workload; any later
  nonportable higher-precision backend is a separately named capability, not a
  portable-WebGPU assumption;
- whether Cells Advanced catalog group is named `showcase`, `performance`, or
  another generic group already supported when implementation begins.

These choices may tune profiles and adapters. They may not change application
semantics, add comparison implementations, or introduce example-specific
engine behavior.

## Completion Criteria

This portfolio is complete only when:

- all thirteen new manifest examples exist and load through ordinary source
  replacement;
- existing Counter is unchanged and existing Cells Advanced behavior,
  scenarios, budgets, and proof contract are preserved and clearly
  distinguished;
- all seven canonical 7GUIs tasks meet official behavior with scenarios,
  native evidence, and browser execution;
- all six performance examples pass correctness before timing and meet their
  declared native/browser target stages;
- BoonSearch is algorithmically pure Boon and does not depend on removed query
  APIs or a native search implementation, and passes its declared browser
  footprint budget while using the Wasm interpreter;
- BoonTune supports deterministic and live software input, and its FPGA stage
  supports PC-mediated USB-microphone/test audio, bit-accurate fixed-point and
  CDC/reset/FIFO simulation, bounded protocol evidence, machine-readable pitch,
  atomically latched display masks, multiplex-waveform proof, resource/timing
  closure, and later separate physical-wiring observation;
- Mandelbrot and Path Tracer execute the same source-derived kernel across CPU
  and generated GPU profiles;
- Boon Fingerprint implements hash, XOF/seek, keyed, and derive-key modes and
  passes their official vectors across streaming/worker modes;
- Runtime Lab contains the five suite groups, validates every result, exports
  raw versioned evidence, and contains no Rust/JavaScript comparison workload;
- runtime/compiler optimizations are generic and covered by extracted probes;
- active native manifest gates and aggregate pass when required, followed by
  separate human testing;
- documentation accurately distinguishes implemented, planned, generated,
  interpreted, worker, GPU, and FPGA modes.

## Primary External References

- 7GUIs tasks: <https://eugenkiss.github.io/7guis/tasks/>
- LocalSearch reference/product history: <https://github.com/Stremio/local-search>
- BLAKE3 algorithm and vectors: <https://github.com/BLAKE3-team/BLAKE3>
- RustFFT algorithm/planning reference only: <https://github.com/ejmahler/RustFFT>
- YIN pitch estimator: <https://pubmed.ncbi.nlm.nih.gov/12002874/>
- Aubio YINFFT description: <https://aubio.org/doc/0.4.0/pitch_8h.html>
- Web Audio: <https://www.w3.org/TR/webaudio-1.1/>
- Media Capture and Streams: <https://www.w3.org/TR/mediacapture-streams/>
- WebGPU/WGSL: <https://gpuweb.github.io/gpuweb/>
- WebAssembly threads: <https://github.com/WebAssembly/threads/blob/main/proposals/threads/Overview.md>
- Are We Fast Yet?: <https://github.com/smarr/are-we-fast-yet>
- One Billion Row Challenge: <https://github.com/gunnarmorling/1brc>
- NAS Parallel Benchmarks: <https://www.nas.nasa.gov/software/npb.html>
- LZ4 block format: <https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md>
- AMD/Xilinx FFT hardware reference: <https://docs.amd.com/r/en-US/pg109-xfft>
