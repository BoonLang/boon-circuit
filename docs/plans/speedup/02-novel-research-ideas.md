# Novel Research Ideas For Boon And NovyWave Speed

Date: 2026-06-12

## Purpose

This file captures speed ideas from papers, whitepapers, technical blogs,
forums, and engine documentation that may transfer to Boon and the NovyWave
example. It complements `01-inspiration.md`: the first file names broad systems
to study, while this file ranks deeper ideas by how directly they could improve
Boon's parser, typed IR, runtime, document/layout pipeline, renderer, event
loop, and waveform data path.

This is not an implementation plan. Ideas here still need local profiling,
small prototypes, and verifier-backed acceptance before they become engine
architecture.

Local constraints:

- `docs/architecture/NATIVE_GPU_PIPELINE.md` remains the active native GPU
  contract.
- Engine limitations should be fixed in the engine, not hidden with Boon-level
  workarounds.
- No speed path may branch on example names, source file names, scenario step
  ids, or hardcoded NovyWave values.
- Human observation is useful follow-up, but native GPU speed proof must come
  from app-owned reports, host events, timing counters, process evidence, and
  WGPU readback when pixel proof is required.

## Reading Map

| Idea | Source type | Boon layer | NovyWave transfer | Risk |
| --- | --- | --- | --- | --- |
| Incremental source trees | Parser docs and compiler blogs | Parser/IR | Edit source without full preview replacement | Medium: needs source-version contract |
| Query compiler | Compiler architecture | Parser/typecheck/IR | Reuse unchanged routes and view dependencies | Medium: cache invalidation complexity |
| Self-adjusting computation | PL papers | Runtime/dev queries | Demand bounded summaries and visible pages | High: can overcomplicate core runtime |
| Differential operators | DB/dataflow papers | LIST/root projections | Exact list-view patches from deltas | Medium: must prove against full recompute |
| Datalog invalidation facts | Static analysis papers | IR/document boundary | Compile field-to-view invalidation | Medium: fact model can drift from runtime |
| Quickened action plans | VM papers and blogs | Runtime execution | Dense source-event micro-ops | Low: good after profiling identifies hot ops |
| Shape IDs and sparse sets | VM/ECS docs | Runtime/document storage | Dense list rows, style records, node fields | Low: fits existing hidden-key direction |
| Retained display chunks | Browser engine docs | Layout/render | Patch hover/cursor chunks only | Medium: invalidation cost must be measured |
| Tile-aware picture caches | Browser/rendering blogs | Renderer/GPU | Waveform tiles by row/time/scale/theme | Medium: memory pressure and eviction |
| Present-deadline pacing | Browser/game docs | Host/render loop | Sample late, cap queued frames | Medium: proof mode must stay deterministic |
| Backpressure lanes | Runtime/concurrency docs | IPC/observability | Lossy telemetry, reliable commands | Low: matches preview/dev separation |
| LOD waveform pages | Visualization papers | Bridge/data model | Pixel-scale pages and min/max pyramids | Low: direct fit for waveform viewer |
| Columnar page storage | Data format docs | Bridge/renderer | GPU-ready typed page columns | Medium: API shape needs care |

## Highest-Transfer Ideas

1. **Build a query-shaped compiler path.** Use Tree-sitter-style edit ranges,
   Salsa/rustc red-green reuse, and explicit source hashes so parse, namespace,
   typecheck, IR lowering, source routes, and view dependency indexes can reuse
   unchanged work. This should first target editor/source changes, not ordinary
   runtime ticks.

2. **Compile list operations into delta operators.** `List/map`,
   `List/filter`, `List/retain`, joins, aggregates, and root projections should
   consume change values and emit exact `ListViewPatch` or root patches. Full
   recompute remains the oracle for tests and fallback.

3. **Generate invalidation facts from typed IR.** The compiler should derive
   facts such as `runtime_field -> view_node`, `field -> invalidation_class`,
   `source -> action_plan`, and `list_view -> ForEach origin`. This is the
   engine-grade alternative to benchmark allowlists.

4. **Quicken runtime action plans.** Keep a compact op stream for source events
   and specialize hot generic ops into dense typed forms with cheap guards and
   deopt counters. The first target is eliminating string and map lookup from
   hot runtime turns, not adding a JIT.

5. **Use shape IDs for records, styles, and document nodes.** Hidden-class-like
   descriptors can turn field/style lookup into `shape_id + offset`, and style
   transitions can share structure instead of cloning large maps.

6. **Make LIST storage physically dense.** Store live keys, sparse key-to-row
   indexes, generations, field columns, valid bits, source bindings, and view
   memberships separately. Keep debug/report structs at the boundary.

7. **Retain display chunks with bailout counters.** Cache display chunks by
   stable identity, merge changed chunks, and record why a full rebuild was
   chosen. Dirty regions are an optimization only when their proof and merge
   cost is lower than rebuilding.

8. **Split scroll, transform, clip, and effect state.** Passive scroll and pan
   should update property state and newly visible ranges without runtime
   dispatch or full layout when no semantic source binding is hit.

9. **Use tile-aware waveform caches.** Cache waveform regions by file, signal,
   time range, pixel scale, theme, and row mode. Cursor movement should dirty a
   narrow overlay, not the entire waveform surface.

10. **Create pixel-scale waveform pages.** Replace fixed `max_transitions`
    thinking with Boon-owned render budgets based on visible pixels, device
    scale, signal type, and zoom. Rust bridge pages return bounded typed data.

11. **Split ordered events from latest-wins signals.** Preserve order for
    text/key/click semantics, but coalesce hover, wheel, pointer motion, drag,
    resize, cursor preview, and debug hover where semantics allow it.

12. **Trace input-to-present flows.** Add one flow id from source event or host
    input through runtime turn, document patch, layout patch, display retain,
    GPU upload, and present. Measure latency stages, not only FPS.

## Layer Notes

### Compiler, Runtime, And Incrementality

Tree-sitter, Salsa, rustc incremental compilation, Adapton, DBSP,
Differential Dataflow, Noria, and IncA all point at the same design pressure:
make change explicit. Boon already has static typed structure, so the transfer
should be conservative:

- source edits produce versioned parse/lower artifacts and changed ranges;
- compiler passes become keyed queries where practical;
- unchanged query outputs preserve downstream route and view dependency ids;
- list and projection operators get derivative implementations;
- runtime turns report changed keys, derived operators touched, fallback
  reasons, and full-recompute comparison results in tests.

The dangerous version is a wholesale replacement of Boon's runtime with a
general dataflow database. The useful version is smaller: borrow arrangements,
change values, and proof techniques while keeping the static circuit runtime.

### VM And Data-Structure Specialization

Python's PEP 659, QuickJS atoms, V8 hidden classes, inline-cache/quickening
papers, sparse-set ECS storage, generational arenas, and data-oriented design
are relevant because Boon hot paths should be mostly known after parse/lower.

Transfers:

- intern source labels, field names, style names, tags, module names, and
  document attribute names before runtime hot paths;
- use compact action plans and quickened field/list ops with generation guards;
- add counters for quickening hits, misses, deopts, top op kinds, and top dirty
  fanout causes;
- use generational handles for rows, document nodes, source bindings, and
  retained display chunks;
- use SoA columns for runtime, layout, and render hot paths, with AoS structs
  only for serde, reports, and debug views;
- allocate phase-local scratch in resettable arenas while keeping semantic
  state in long-lived generational arenas.

The likely first wins are replacing hot `BTreeMap<String, ...>` style or node
lookups with interned IDs plus shape descriptors, then measuring before
changing dispatch mechanics.

### Document, Layout, And Rendering

Firefox retained display lists, Blink Slimming Paint, RenderingNG, WebRender
picture caching, Vello, Qt scene graph notes, and GPU instancing docs all
suggest retaining smaller artifacts than full frames.

Transfers:

- use display chunk identity such as
  `node_id + fragment_kind + style_id + property_state + content_hash`;
- keep scroll, transform, clip, and effect state separate from display items;
- add dirty regions with cost and bailout counters;
- introduce a `RenderUploadPlan` per frame with instance bytes, atlas bytes,
  waveform tile bytes, uniform bytes, draw counts, and cache stats;
- batch display items by primitive/material class: quads, borders, glyphs,
  icons, waveform segments, clips, overlays, and debug artifacts;
- split text caches into measurement, shaping, layout, and glyph-atlas layers.

Caution from browser and forum discussions: invalidation can cost more than it
saves. Boon reports should show both sparse wins and full-rebuild bailouts.

### Event Loop, Scheduling, And Observability

Browser event loops, Android Swappy, DXGI waitable swapchains, NVIDIA Reflex,
Raph Levien's swapchain notes, Pointer Events, Linux ring buffers, Reactive
Streams, Tokio channels, and the LMAX Disruptor all support the same shape:
different lanes need different ordering and backpressure policies.

Transfers:

- pace from present/display deadlines rather than free-running timers;
- sample latency-sensitive input late when deterministic proof mode does not
  require exact intermediate states;
- split ordered input events from latest-wins frame signals;
- make command, telemetry, debug summary, and current-state IPC lanes explicit;
- expose queue depth, overwritten samples, blocked sends, coalesced events, and
  dropped events;
- keep proof mode deterministic and labeled separately from interaction mode;
- emit Perfetto or Chrome JSON traces with flow ids across the whole pipeline.

This is especially important for dev-window observability: trace aggregation,
syntax highlighting, debug graph views, and waveform preprocessing should yield
or run in background lanes, never block preview interaction.

### Waveform Pages And Time-Series Rendering

M4, OM3, MinMaxCache, ForeCache, Datashader, Arrow, wellen, Surfer, FST/GTKWave,
Tracy, Perfetto, audiowaveform, and uPlot are the most directly NovyWave-shaped
sources. The strongest theme is to render from page summaries chosen by pixel
scale, not from unbounded semantic transitions.

Transfers:

- analog dense rows use M4-style first, last, min, and max buckets;
- digital dense rows use edge envelopes: first value, last value, first
  transition, last transition, transition count, mixed flags, and unknown/high-Z
  flags;
- build LOD pages keyed by file id, signal id, lod, time start/end, pixel scale,
  format, and theme;
- keep selected-signal sparse extraction as a hard invariant;
- add predictive prefetch based on pan direction, zoom center, and visible row
  range;
- use columnar bridge pages with arrays such as `time_delta[]`, `value_code[]`,
  `flags[]`, `min[]`, `max[]`, `first[]`, and `last[]`;
- let Boon own viewport semantics, page choice, cache policy, and correctness
  rules while Rust owns parser access, sparse extraction, mmap/Arc page storage,
  and GPU-ready buffers.

The near-term test oracle should compare page rendering against exact
transitions for bounded fixtures and zoom levels before trusting visual
summaries.

## Cautions

- Do not adopt a research system wholesale. Pull out contracts, metrics, and
  data shapes that fit Boon's static runtime.
- Do not turn quickening or PGO into a semantic fork. Generated or specialized
  paths need equality checks against the generic interpreter.
- Do not let dirty-region machinery hide stale hit regions, stale source
  bindings, or stale accessibility/control metadata.
- Do not use larger overscan/cache buffers to hide slow delegates. The
  underlying row, layout, and display chunks still need to be cheap.
- Do not expose Rust parser/cache handles to Boon. Bridge pages must stay typed,
  bounded, serializable or handle-like by contract, and fingerprinted.
- Do not benchmark with proof-mode readback, report writes, or dev IPC in the
  live interaction path unless that is the specific thing being measured.

## Source Appendix

Compiler, incremental computation, and dataflow:

- Tree-sitter: <https://tree-sitter.github.io/tree-sitter/>
- Tree-sitter advanced parsing: <https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html>
- Salsa overview: <https://salsa-rs.github.io/salsa/overview.html>
- Salsa red-green algorithm: <https://salsa-rs.github.io/salsa/reference/algorithm.html>
- rustc incremental compilation: <https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html>
- Build Systems a la Carte: <https://www.microsoft.com/en-us/research/uploads/prod/2018/03/build-systems.pdf>
- Adapton PLDI 2014: <https://matthewhammer.org/adapton/adapton-pldi2014.pdf>
- Self-adjusting computation: <https://www.umut-acar.org/self-adjusting-computation>
- Incremental Lambda Calculus: <https://inc-lc.github.io/>
- DBSP: <https://www.vldb.org/pvldb/vol16/p1601-budiu.pdf>
- Differential Dataflow: <https://www.cidrdb.org/cidr2013/Papers/CIDR13_Paper111.pdf>
- Timely Dataflow: <https://cacm.acm.org/research/incremental-iterative-data-processing-with-timely-dataflow/>
- Noria: <https://www.usenix.org/conference/osdi18/presentation/gjengset>
- IncA: <https://github.com/szabta89/IncA>

Runtime, VM, and data structures:

- PEP 659: <https://peps.python.org/pep-0659/>
- Inline Caching Meets Quickening: <https://bernsteinbear.com/assets/img/ic-meets-quickening.pdf>
- QuickJS internals: <https://bellard.org/quickjs/quickjs.html>
- Wasm3 interpreter notes: <https://github.com/wasm3/wasm3/blob/main/docs/Interpreter.md>
- V8 hidden classes: <https://v8.dev/docs/hidden-classes>
- V8 fast properties: <https://v8.dev/blog/fast-properties>
- JavaScript shapes and inline caches: <https://mathiasbynens.be/notes/shapes-ics>
- EnTT sparse sets: <https://skypjack.github.io/2020-08-02-ecs-baf-part-9/>
- Bevy component storage tradeoffs: <https://bevy-cheatbook.github.io/patterns/component-storage.html>
- slotmap: <https://docs.rs/slotmap/>
- generational-arena: <https://docs.rs/generational-arena/>
- Data Locality: <https://gameprogrammingpatterns.com/data-locality.html>
- Rust Performance Book: <https://nnethercote.github.io/perf-book/heap-allocations.html>
- Rust BTreeMap case study: <https://faultlore.com/blah/rust-btree-case/>

Rendering, layout, and GPU:

- Firefox retained display lists: <https://hacks.mozilla.org/2018/06/retained-display-lists/>
- Blink Slimming Paint: <https://www.chromium.org/blink/slimming-paint/>
- Blink paint README: <https://chromium.googlesource.com/chromium/src/+/HEAD/third_party/blink/renderer/core/paint/README.md>
- RenderingNG architecture: <https://developer.chrome.com/docs/chromium/renderingng-architecture>
- RenderingNG data structures: <https://developer.chrome.com/docs/chromium/renderingng-data-structures>
- Chromium compositor thread architecture: <https://www.chromium.org/developers/design-documents/compositor-thread-architecture/>
- WebRender picture caching: <https://mozillagfx.wordpress.com/2018/11/02/webrender-picture-caching/>
- WebRender batching: <https://hacks.mozilla.org/2017/10/the-whole-web-at-maximum-fps-how-webrender-gets-rid-of-jank/>
- Vello vision: <https://github.com/linebender/vello/blob/main/doc/vision.md>
- Vello conference slides: <https://www.datocms-assets.com/98516/1707130683-levien_2023.pdf>
- Qt Quick scene graph: <https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph.html>
- wgpu StagingBelt: <https://docs.rs/wgpu/latest/wgpu/util/struct.StagingBelt.html>
- wgpu Queue: <https://docs.rs/wgpu/latest/wgpu/struct.Queue.html>
- glyphon: <https://docs.rs/glyphon/latest/glyphon/>

Scheduling, input, and tracing:

- HTML event loop: <https://html.spec.whatwg.org/multipage/webappapis.html#event-loops>
- Android Frame Pacing: <https://developer.android.com/games/sdk/frame-pacing>
- DXGI latency: <https://learn.microsoft.com/en-us/windows/uwp/gaming/reduce-latency-with-dxgi-1-3-swap-chains>
- Swapchain frame pacing: <https://raphlinus.github.io/ui/graphics/gpu/2021/10/22/swapchain-frame-pacing.html>
- NVIDIA Reflex: <https://developer.nvidia.com/performance-rendering-tools/reflex>
- Chrome input/compositor pipeline: <https://developer.chrome.com/blog/inside-browser-part4>
- Pointer Events: <https://www.w3.org/TR/pointerevents/>
- High-performance input handling: <https://nolanlawson.com/2019/08/11/high-performance-input-handling-on-the-web/>
- Linux lockless ring buffer: <https://docs.kernel.org/trace/ring-buffer-design.html>
- Reactive Streams: <https://github.com/reactive-streams/reactive-streams-jvm>
- Tokio mpsc: <https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html>
- Tokio watch: <https://docs.rs/tokio/latest/tokio/sync/watch/index.html>
- LMAX Disruptor: <https://lmax-exchange.github.io/disruptor/files/Disruptor-1.0.pdf>
- Perfetto track events: <https://perfetto.dev/docs/instrumentation/track-events>
- Chrome trace format in Perfetto: <https://perfetto.dev/docs/getting-started/other-formats>
- wgpu timestamp queries: <https://docs.rs/wgpu/latest/wgpu/enum.QueryType.html>

Waveforms and time-series visualization:

- M4 paper: <https://www.vldb.org/pvldb/vol7/p797-jugel.pdf?ref=ssp.sh>
- AG Grid M4 notes: <https://blog.ag-grid.com/optimizing-large-data-set-visualisations-with-the-m4-algorithm/>
- tsdownsample: <https://github.com/predict-idlab/tsdownsample>
- OM3: <https://www.yunhaiwang.net/sigmod2023/om3/paper.pdf>
- ForeCache: <https://www.cs.tufts.edu/~remco/publications/2016/SIGMOD2016-ForeCache.pdf>
- MinMaxCache: <https://www.vldb.org/pvldb/vol17/p2091-maroulis.pdf>
- Datashader time series: <https://datashader.org/user_guide/Timeseries.html>
- Arrow columnar format: <https://arrow.apache.org/docs/format/Columnar.html>
- wellen: <https://github.com/ekiwi/wellen>
- Surfer paper: <https://link.springer.com/chapter/10.1007/978-3-031-98685-7_19>
- FST format notes: <https://blog.timhutt.co.uk/fst_spec/>
- GTKWave formats: <https://gtkwave.github.io/gtkwave/intro/formats.html>
- Perfetto large traces: <https://perfetto.dev/docs/visualization/large-traces>
- Tracy speed post: <https://wolf.nereid.pl/posts/how-tracy-faster/>
- audiowaveform: <https://github.com/bbc/audiowaveform>
- webgpu-instanced-lines: <https://github.com/rreusser/webgpu-instanced-lines>
