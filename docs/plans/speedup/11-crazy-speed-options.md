# Crazy Speed Options For Boon And NovyWave

## Purpose

This file records aggressive speed, performance, and simplification ideas found
from a read-only review of the existing speedup plans and current repo code.

It is intentionally broader than an implementation plan. Some ideas are small
fixes that should happen soon. Some are architectural bets. Some are deliberately
wild, but still grounded in the current Boon compiler, runtime, document,
renderer, native window, scenario, and NovyWave source shape.

The goal is to keep future planning from collapsing into narrow local fixes. The
engine should become easier to reason about and easier to make fast, not only
faster by accident in one example.

## Boundaries

- Do not add Boon syntax just to make the engine faster.
- Prefer inference, compiled metadata, route tables, indexes, and diagnostics.
- Keep NovyWave-specific acceleration out of generic engine layers.
- Keep proof/readback/report work out of interaction latency measurements.
- Keep full recompute and semantic reports as verifier oracles, not frame-time
  behavior.
- Treat low-level Rust dependencies as experiments after counters show the
  relevant cost.

## Main Thesis

NovyWave does not feel slow because of one missed micro-optimization. It feels
slow because too much of the stack still behaves like a stringly, whole-program,
whole-list, whole-frame interpreter.

The main performance target is:

1. Compile source into stable typed identities and dense op streams.
2. Apply user input as a small typed route, not as heuristic source recovery.
3. Update only affected runtime slots, rows, document patches, layout fragments,
   render chunks, and GPU buffers.
4. Separate interaction work from proof, diagnostics, reports, and readbacks.
5. Use full recompute as an honesty oracle until incremental paths are proven.

## Evidence From Current Code

- `crates/boon_runtime/src/lib.rs` is large and still mixes generic runtime
  execution, source routing, scenario reporting, summaries, and specialized
  compatibility paths.
- `SourceStore::unbind_row` currently takes a row slot before validating the
  row identity, making stale unbinds dangerous for future cache correctness.
- `GenericCircuitRuntime::classify_source_event` still classifies events with
  ordered route-kind heuristics.
- `apply_source_actions` clones the action vector for every source application.
- `FieldSlotId::from_path` still uses hash-derived field IDs with a small masked
  ID space, which is a long-term collision and diagnostics risk.
- `DocumentState::apply_patch` silently ignores missing patch targets and
  returns no invalidation report.
- `boon_document` and `boon_document_model` use string document IDs and
  `BTreeMap<String, StyleValue>` style maps in hot-looking paths.
- `boon_native_gpu` still prepares quads by building byte vectors, hashing
  geometry, creating GPU buffers, and issuing multiple writes.
- Native GPU proof/readback paths can perform synchronous polling, hashing, PNG
  writes, and report writes that must never be mixed into interaction budgets.
- `examples/novywave/RUN.bn` keeps waveform-like metadata, selected rows,
  segment fixtures, bridge labels, and many source ports as ordinary Boon data.

## Highest-Leverage Bets

### 1. Single Semantic Index

Build one semantic index after parsing and before IR/typecheck/runtime lowering.
It should own:

- stable symbol IDs;
- source IDs;
- list IDs;
- row scope IDs;
- function IDs;
- view binding IDs;
- field IDs;
- module/source-unit identity;
- source payload schemas;
- dependency edges;
- candidate list indexes;
- diagnostics spans.

Current parser, IR, typecheck, and runtime passes rediscover related facts in
different shapes. A single index would simplify code and give every later layer
the same stable identities.

First proof:

- lower/typecheck profile shows fewer whole-program passes;
- stable ID table appears in reports;
- hot routes can be resolved without string path recovery;
- field ID collisions are impossible or reported explicitly.

### 2. Typed Route Op Streams

Compile `SOURCE` usage into typed source payload schemas and dense action plans.
The runtime should not ask "what kind of source event might this be?" on every
event. It should run a small route program that was generated from typechecked
source.

The route stream can contain operations like:

- decode payload;
- require row key/generation;
- reject stale source epoch;
- resolve row slot;
- update root field;
- update indexed row field;
- append row;
- remove row;
- update source bindings;
- mark dirty dependency IDs;
- emit document/runtime delta.

First proof:

- `route_actions_cloned = 0`;
- source route heuristic hit count is zero on readiness paths;
- p95 `apply_source_event` improves or at least allocates less;
- ambiguous source routing becomes a compiler/typechecker diagnostic.

### 3. Real Row And List Identity

Stop recovering row/list identity from display labels, source path suffixes, or
list names at event time. Runtime source events should carry stable identity:

- source ID;
- source epoch;
- list ID;
- row key;
- row generation;
- optional occurrence/index for repeated widgets;
- typed payload fields.

First proof:

- duplicate labels route correctly;
- plural/singular source path variants are no longer needed in hot paths;
- stale row events are rejected before mutating runtime state;
- row unbind/bind reports expose key/generation/epoch mismatches.

### 4. Derived List Delta Operators

Compile common list operations into derivative operators instead of recomputing
entire projections.

Examples:

- `filter` keeps a membership bitset or row-key set;
- `map` updates only the mapped output row for a changed input row;
- `count` adjusts by delta;
- `find_value` uses inferred indexes;
- sorted/order views track move deltas;
- grouped views update only affected groups.

First proof:

- large synthetic list has rows-scanned counters near changed rows, not total
  rows;
- single-row edit matches full recompute oracle;
- small TodoMVC and Cells do not regress.

### 5. Virtual Materialization As An Engine Protocol

Virtual and infinite collections should be generic, not NovyWave-specific.

The engine should understand:

- logical row count;
- stable row key range;
- viewport range;
- overscan;
- row height policy;
- column window;
- materialized document nodes;
- source binding lifecycle for materialized rows;
- stale-event rejection for unmaterialized or recycled rows.

First proof:

- scrolling changes materialized ranges without runtime semantic dispatch;
- bounded row/window counters stay under budget;
- source bindings are correctly rebound across recycled rows;
- Cells, TodoMVC, and NovyWave can all use the same protocol.

### 6. Retained Render Chunks

Replace whole-frame render caching with retained render chunks.

Possible chunk IDs:

- static chrome;
- scroll content;
- text run group;
- caret/selection overlay;
- hover/focus overlay;
- waveform row page;
- timeline grid;
- dialog layer;
- proof overlay;
- dev editor visible line range.

Each chunk should have:

- stable chunk ID;
- layout bounds;
- clip;
- transform;
- material/style identity;
- dependency set;
- GPU buffer range;
- text run IDs;
- texture/asset refs;
- generation.

First proof:

- passive scroll updates transforms/materialized chunks without rebuilding the
  whole layout frame;
- caret blink or hover does not re-upload static chrome;
- `upload_bytes`, draw calls, text-shaped runs, and cache misses are stage
  counters in release mode.

### 7. Interaction Mode Versus Proof Mode

Split reports and runtime paths into at least three modes:

- `interaction`: latency budget, no proof readbacks, no PNG writes, no heavy
  JSON summaries, no blocking diagnostic IPC.
- `proof`: WGPU readbacks, hashes, artifacts, schema checks, negative checks.
- `diagnostic`: rich debug tables, summaries, tracing, expensive provenance.

First proof:

- interaction reports fail if proof/readback/report serialization happens on
  the hot path;
- proof reports include source/scenario/budget/artifact hashes;
- stale reports cannot be accepted as readiness evidence.

### 8. NovyWave Bridge Page Refs

Move waveform samples behind generic page and artifact references. Boon should
own the application intent, not the waveform payload.

Boon state should keep:

- active file/page descriptor;
- viewport time window;
- selected rows;
- cursor/marker state;
- format intent;
- page request descriptor;
- accepted response descriptor;
- compact UI labels derived from typed values.

The bridge/runtime should keep:

- file descriptor;
- schema hash;
- page key;
- waveform ref;
- bounded transition pages;
- hierarchy pages;
- cached page payloads;
- stale response policy;
- cancellation policy.

First proof:

- NovyWave scenarios prove bounded page requests;
- full waveform data is not materialized into Boon app lists;
- fake bridge golden vectors run before real Wellen integration;
- debug labels are projections, not canonical state.

### 9. Shape IDs And Hidden Classes

Use shape IDs for objects, tagged objects, style records, render items, and
bridge payloads.

Shape IDs should replace repeated `BTreeMap<String, Type>` and
`BTreeMap<String, StyleValue>` interpretation in hot paths. Strings remain for
diagnostics and serialization, but execution should use dense offsets.

First proof:

- shape cache hit rate is reported;
- style lookup counters drop;
- render/style-heavy examples allocate less;
- field lookup by dense offset replaces repeated string lookup in hot paths.

### 10. Runtime Artifact Without AST

Emit a `.boonc`-style compiled artifact that the runtime can execute without
loading parser AST structures.

The artifact can contain:

- semantic index;
- symbol table;
- storage layout;
- source schemas;
- route op streams;
- expression bytecode;
- dependency graph;
- document lowering tables;
- bridge schemas;
- report schema hash;
- source unit hashes.

First proof:

- a scenario runs from the compiled artifact;
- artifact output equals current interpreter output;
- parser/AST memory is not present in the runtime path;
- artifact hash appears in reports.

## Compiler And Parser Options

### Semantic Parser Output

Keep syntax parsing syntax-only, but make the parser produce spans and token
identity in a way that can feed the semantic index without cloning strings
everywhere.

Ideas:

- intern lexemes during parsing;
- keep source ranges as primary identity for diagnostics;
- separate source syntax validation from example/readiness policy;
- stop mixing example policy like forbidden `EXAMPLE`, `LINK`, or shorthand
  style names into the language parser;
- preserve module/source-unit IDs from manifest/project loading.

### Incremental Project Index

For dev loops, compile each source unit into an indexed module summary:

- exported functions;
- source ports;
- holds/lists/state cells;
- type facts;
- dependencies;
- diagnostics;
- parse spans.

Then source edits only invalidate dependent units.

### Bytecode Or Micro-Ops

Before JIT, compile expressions into compact bytecode or micro-ops. The current
interpreter-shaped runtime can stay as oracle while the bytecode path proves
itself.

Useful op families:

- read root slot;
- read row slot;
- read payload field;
- const value;
- branch on tag/enum/bool;
- text helper;
- numeric helper;
- list index lookup;
- call pure function;
- construct object by shape ID;
- construct tagged object by variant ID.

First proof:

- op histogram;
- warm event allocation count near zero;
- bytecode output equals interpreter output;
- fallback to interpreter is reported and fails readiness for hot paths.

### Typechecker Readiness Gates

The typechecker can remain flexible during development, but runtime-critical
paths should fail readiness when they depend on:

- dynamic fallback;
- open object fallback;
- unknown type coverage;
- ambiguous source payload;
- ambiguous list row context;
- untyped bridge payload;
- unresolved function import.

The user should not need to add manual types to paper over ambiguity. Ambiguity
should be a compiler error with concrete source spans and suggested structural
fixes.

## Runtime Options

### Dense Slots Everywhere

Move hot runtime storage toward dense IDs:

- `FieldId`;
- `SourceId`;
- `ListId`;
- `RowKey`;
- `Generation`;
- `ShapeId`;
- `FunctionId`;
- `DirtyKeyId`;
- `DocumentNodeId`;
- `RenderChunkId`.

Keep strings for:

- diagnostics;
- reports;
- source maps;
- human-readable debug views;
- serialized proof artifacts.

### Dirty Set Redesign

Replace string-heavy dirty vectors with measured options:

- sorted small vectors for tiny sparse dirty sets;
- fixed bitsets for dense static graphs;
- roaring bitmaps for sparse large graphs;
- per-list dirty row sets;
- per-field dirty row sets;
- dependency generation counters.

Do not pick the final structure before counters show density and cardinality.

### SourceStore Hardening

Before relying on source-binding performance caches:

- validate row identity before taking slots;
- return structured bind/unbind reports;
- expose active/stale binding counters;
- reject stale source epochs;
- test duplicate labels, recycled rows, and deleted rows;
- avoid capacity panics or silent drops.

### Hot Runtime Versus Summary Runtime

Separate:

- hot event turn;
- state summary generation;
- semantic delta rendering;
- debug table generation;
- JSON report materialization;
- scenario assertion projection.

The hot event turn should emit small typed deltas. Summaries should be explicitly
requested by test/proof/dev tools.

### Full Recompute Oracle

Keep the current broad recompute behavior as an oracle:

- run incremental path;
- run full recompute in verifier mode;
- compare outputs, dirty sets, and document patches;
- fail on mismatch with source spans and route IDs.

This allows aggressive incremental work without lying to ourselves.

## Document And Layout Options

### PatchApplyReport

Change document patch application to return a report:

- applied;
- missing target;
- stale generation;
- inserted node;
- removed node;
- changed text;
- changed style;
- changed binding;
- changed scroll;
- changed materialization;
- invalidation class;
- affected subtree;
- affected scroll root;
- affected render chunks.

This is a prerequisite for retained layout and render chunks.

### Computed Style IDs

Move from raw `BTreeMap<String, StyleValue>` in hot layout/render paths to:

- parsed style records;
- inherited style IDs;
- material IDs;
- font IDs;
- explicit clip/transform/opacity fields;
- side tables for debug style maps.

### Property Tree

Introduce a property tree separate from layout geometry:

- scroll transform;
- opacity;
- clip;
- z/depth;
- focus/hover;
- caret blink;
- selection range.

Passive scroll should mostly update property transforms and materialized ranges,
not runtime graph state.

### Layout Fragments

Instead of one full `LayoutFrame`, produce fragments:

- node fragment;
- text fragment;
- scroll fragment;
- hit fragment;
- materialization demand;
- overlay fragment.

The renderer can then update only dirty fragments.

## Renderer And GPU Options

### RenderScene Boundary

Lower app/editor semantics before the GPU crate. The renderer should consume a
primitive `RenderScene`, not inspect high-level document widget meaning.

Potential shape:

- bins;
- instances;
- text runs;
- textures;
- clips;
- transforms;
- materials;
- hit side table reference;
- proof markers.

### POD Uploads

Replace split position/color/UV vectors and byte conversions with one or two
`#[repr(C)]` structs:

- `RectInstance`;
- `TextRunInstance`;
- `ImageInstance`;
- `LineInstance`;
- `WaveformSegmentInstance`.

Use `bytemuck` or equivalent after a small focused experiment.

### Ring Or Staging Buffer

Avoid creating new GPU buffers for unchanged geometry. Keep persistent buffers
and write dirty ranges.

Track:

- allocated GPU bytes;
- uploaded bytes;
- dirty ranges;
- buffer reuse count;
- staging wrap count;
- cache evictions.

### Shader-Side Shapes

Consider shader primitives for:

- rounded rectangles;
- borders;
- checkmarks;
- underlines;
- strikethroughs;
- shadows;
- waveform digital segments;
- timeline grid lines;
- cursor/marker lines.

This can reduce CPU-expanded geometry, but should be measured before replacing
all current rect generation.

### Text Service

Unify text measurement, shaping, editor metrics, and glyph atlas stats.

The service should expose:

- shaped run ID;
- text metrics;
- column edge map;
- glyph atlas hit/miss;
- font ID;
- rich span ID;
- cache generation;
- memory budget.

### AssetRef And BlobRef

Use digest-based asset/blob references rather than large data URL text in hot
source/render paths.

Good candidates:

- SVG icons;
- generated assets;
- waveform page payloads;
- screenshots/proof artifacts;
- bridge blobs.

## Native Window And IPC Options

### Latest-Wins Workers

The preview replace worker queue pattern should become the standard pattern for
expensive preview/dev work:

- coalesce stale updates;
- keep one persistent worker;
- report dropped/coalesced counts;
- never block preview rendering on dev/debug transport.

### Event-Driven Loop

Demand-driven rendering should not be followed by fixed sleeps in interaction
mode. Use:

- event wake;
- scheduled animation wake;
- present deadline;
- idle wait;
- explicit proof hold timers.

### Typed Hit Side Table

Keep JSON proof reports, but use typed hit-test data for interaction:

- node ID;
- source binding ID;
- bounds;
- z/depth;
- scroll root;
- row key/generation;
- coarse y-bucket or spatial bins.

First proof:

- click/hover path does not scan JSON display proof;
- route report still serializes the same proof data afterward.

### IPC Budgets

IPC reports should include:

- queue depth max;
- coalesced debug updates;
- dropped debug updates;
- preview blocked count;
- blocked duration;
- heartbeat gap;
- preview RSS;
- bytes p50/p95/max for debug query/subscription/update.

## NovyWave-Specific Engine Pressure

NovyWave currently stresses every weak boundary:

- many named sources;
- waveform metadata as ordinary lists;
- repeated `List/find_value`;
- selected row projections;
- segment filtering and mapping;
- string-concat bridge request labels;
- value formatting in Boon helper functions;
- theme/material branching;
- giant view source files;
- mock bridge data as app state.

The generic engine response should be:

- typed source payloads;
- row-scoped routing;
- inferred indexes;
- selector memoization;
- bridge page refs;
- bounded materialization;
- retained waveform/timeline chunks;
- typed value formatting cache;
- style/material IDs.

No syntax change is needed for these. The source can keep using records,
tagged objects, functions, lists, and source ports while the compiler/runtime
becomes more clever.

## Verification And Anti-Cheating Options

### Scenario Manifest Integrity

Add a gate that rejects:

- duplicate scenario IDs;
- manifest refs missing from `.scn`;
- duplicate manifest refs unless explicitly allowed;
- action steps with no assertion;
- stale source/scenario/budget hashes;
- missing required evidence tier;
- visual artifacts without linked report hash;
- private dispatch evidence where a public route is required.

### Flow IDs

Every interaction should have a flow ID spanning:

- host input;
- hit/focus/scroll routing;
- source intent;
- runtime route;
- runtime patch;
- document patch;
- layout/materialization;
- render chunk update;
- GPU upload;
- submit/present;
- optional readback.

Reports should show stage p50/p95/p99/max and counters for that flow.

### Metamorphic Scenarios

To prevent hardcoded shortcuts:

- rename labels;
- reorder legal declarations;
- vary viewport;
- vary scale;
- vary theme;
- vary row order;
- vary fixture IDs;
- use hidden equivalent files;
- compare outputs against semantic invariants, not only exact labels.

### Negative Fixtures

Negative checks should mutate:

- source hash;
- scenario hash;
- budget hash;
- artifact hash;
- pixel hash;
- source event field;
- route ID;
- real OS input claim;
- private dispatch flag;
- stale source generation;
- stale row generation;
- duplicate scenario ID.

Each mutation must fail a named check.

## Low-Level Rust And Dependency Experiments

Use dependencies only where they remove measured cost.

Likely early candidates:

- `bytemuck` for GPU POD uploads;
- `smallvec` or `arrayvec` for tiny hot vectors;
- an interner or custom symbol table for cross-stage symbols;
- `fixedbitset` for dense dirty sets;
- `roaring` for sparse large dirty sets;
- bounded LRU or clock cache where clear-all caches show up in traces.

Measure-first candidates:

- global allocator swap;
- Rayon parallelism;
- Salsa-like query system;
- Tree-sitter parser;
- Vello renderer;
- mmap waveform pages;
- SIMD for pixel scans or hit testing;
- JIT via Cranelift.

Do not SIMD or JIT around bad data movement. Remove bad data movement first.

## Radical Architecture Options

### Browser-Style Pipeline

Runtime emits typed document patches. Document emits layout fragments. Layout
emits retained render chunks. Windows consume frame deltas.

This is a large change, but it maps cleanly to browser engines and avoids
whole-frame invalidation.

### Layered Compositor

Split frame rendering into layers:

- background/static chrome;
- scroll content;
- waveform chunks;
- text;
- caret/selection;
- hover/focus;
- modal/dialog;
- dev overlay;
- proof/readback overlay.

Each layer has independent invalidation and upload.

### Data-Oriented Engine Core

Replace map/string-heavy hot paths with SoA storage:

- root fields;
- list columns;
- row metadata;
- dirty bitsets;
- dependency edges;
- document node columns;
- layout columns;
- render chunk columns.

Diagnostics project this back into rich named objects.

### Compiled Boon Kernel

After bytecode is stable, generate native kernels for pure derived fields or
list projections. Keep the interpreter and full recompute as oracle.

### Waveform/Series Primitive

Add generic interval/series render primitives, not a NovyWave shortcut:

- time range;
- row ID;
- value segment range;
- style/material;
- cursor markers;
- selection range;
- decimation/binning policy.

This can serve waveform viewers, charts, logs, timelines, profilers, and traces.

## Preferred Implementation Order

1. Add scenario manifest integrity and freshness gates.
2. Split `interaction`, `proof`, and `diagnostic` reporting modes.
3. Add release-mode flow telemetry and counters.
4. Fix `SourceStore` row unbind/bind correctness.
5. Make document patch application fail closed with `PatchApplyReport`.
6. Add semantic index skeleton with stable IDs and source maps.
7. Intern cross-stage symbols and add field collision diagnostics.
8. Replace source-event classification with typed route op streams.
9. Add row identity/generation to route inputs and stale-event rejection.
10. Add list scan counters and inferred indexes.
11. Add derivative list delta operators with full recompute oracle.
12. Add virtual materialization protocol.
13. Promote passive scroll to a generic property-tree path.
14. Add retained render chunk IDs.
15. Replace GPU quad uploads with POD/ring-buffer uploads.
16. Move app/editor semantics out of `boon_native_gpu`.
17. Add shared text service and bounded shaped-run cache.
18. Add bridge page/artifact/blob refs for NovyWave.
19. Emit `.boonc` compiled artifacts.
20. Experiment with bytecode, generated kernels, and large-list dataflow core.

## Acceptance Criteria

This file is useful only if future work can turn ideas into measurable gates.
Each adopted idea should define:

- target examples;
- before/after counters;
- correctness oracle;
- interaction budget;
- proof/report budget;
- negative checks;
- rollback criteria;
- whether it changes public Boon source behavior.

The default answer for user-facing Boon changes should remain "no" unless an
engine-only solution cannot be made correct or diagnosable.
