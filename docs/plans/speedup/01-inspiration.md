# Boon And NovyWave Speedup Inspiration

Date: 2026-06-12

## Purpose

This file collects implementation inspiration for making Boon and the NovyWave
example faster without shortcuts. The goal is not to copy another UI toolkit,
browser engine, or VM. The goal is to identify durable engine patterns that fit
Boon's current architecture:

- fix compiler, runtime, document, host, layout, and renderer limitations in the
  engine rather than hiding them with Boon-level workarounds;
- keep NovyWave a stress workload for generic Boon behavior, not a source of
  example-name render shortcuts;
- keep `docs/architecture/NATIVE_GPU_PIPELINE.md` as the active native GPU
  contract;
- prove speed with app-owned reports, host events, timing counters, process
  evidence, and WGPU readback where proof mode needs pixels.

## Local Baseline

The existing NovyWave speed plan already moved the investigation away from
"debug build vs release build" and toward sparse engine work. Earlier reports
showed runtime application and full document/layout rebuilds dominating
interaction latency. Later work made steady hover fast and improved release-mode
outer interaction p95s, but the current report shape has an important blind
spot: `target/reports/native-gpu/novywave-interaction-speed.json` can pass its
outer interaction budgets while internal runtime/layout timing summaries are
empty or null because hot-path profile recording is suppressed for the budget
run.

That means the docs series should not treat the existing NovyWave speed gate as
the whole truth. The next speed work needs profiling coverage that is strong
enough to explain manual slowness without reintroducing hot-path report writes,
PNG writes, readbacks, full summaries, or dev-window IPC waits.

Relevant local contracts and plans:

- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/plans/NOVYWAVE_INTERACTION_SPEED_INVESTIGATION_PLAN.md`
- `docs/plans/NOVYWAVE_BOON_REWRITE_PLAN.md`
- `examples/manifest.toml`
- `examples/novywave.budget.toml`

## Target Fast Path

The desired steady-state interaction path should look like this:

```text
source event
-> keyed runtime dirty set
-> runtime/document patches
-> layout/display invalidation
-> retained fragments/display chunks
-> scroll/property-tree or dirty-region update
-> bounded GPU uploads
-> present
```

Full parse, full IR lowering, full runtime summary, full document lowering,
full layout, full display-list rebuild, full renderer cache rebuild, full
readback, and full report serialization should be bailout or verifier behavior,
not the ordinary frame path for hover, cursor movement, drag, scroll, pan,
zoom, or small text edits.

## Inspiration By Layer

### Parser And Source Editing

- Tree-sitter: parse incrementally and edit syntax trees by byte/range
  changes. Boon can use the same shape for editor source changes: parse and
  lower off the hot preview path, attach source version/hash metadata, then
  commit only if the edited source version still matches.
- rustc and Salsa: use query-style dependency tracking, stable fingerprints,
  and red/green reuse so unchanged source fragments do not force downstream
  parse/lower/typecheck work.
- Servo/html5ever: split tokenization/tree-operation production from tree
  construction. For Boon, this suggests a future `AstPatch` or `IrPatch`
  boundary rather than a single all-or-nothing source replacement path.

Immediate transfer:

- keep parser diagnostics span-rich, but intern names, paths, operators, tags,
  module names, and source labels into dense IDs before runtime hot paths;
- make source editing produce versioned parse/lower artifacts and explicit
  bailout reasons when a full replacement is required.

### IR, Runtime, And Event Application

- Wasm3: decode once into compact executable operations instead of interpreting
  source-shaped structures repeatedly. Boon's `SourceRoutePlan` direction maps
  well to explicit micro-ops such as `ResolveSourceId`, `ResolveBoundRow`,
  `SetTextSlot`, `SetBoolSlot`, `AppendRow`, `RemoveRow`, `MarkReadDirty`, and
  `EmitPatch`.
- QuickJS and inline-cache systems: intern property names into atoms and quicken
  generic field access after the first successful lookup. Boon can quicken
  `GetField(field_atom)` into dense column/list access with generation checks.
- Wasmtime/Wasmer: use execution tiers, but keep Tier 0 strong. Boon should
  first build a compact, measurable action-plan interpreter before considering
  Cranelift or another native-code backend for pure derived kernels.
- Salsa/Adapton: recompute demand-aware outputs and stop propagation when an
  intermediate value is unchanged. Hover/cursor inputs should be low-durability
  volatile values; parsed file descriptors and static source facts should be
  higher-durability values.
- HyperFormula: represent range dependencies compositionally. NovyWave waveform
  pages, selected rows, cursor values, and viewport ranges should depend on
  reusable time-window/page nodes, not every visible sample.

Immediate transfer:

- compile Boon event handling into dense, generation-stamped action plans;
- canonicalize dirty keys internally so aliases do not widen fanout;
- add first-class list-view patch semantics for stable keyed inserts, removes,
  moves, row-field updates, and root projections;
- profile like a VM: per-op counts, quickening misses, column-cache misses,
  allocation after warmup, dirty fanout histograms, top recompute causes, and
  turn budget exhaustion.

### Document, Style, And Layout

- Xilem: diff lightweight view values into retained widget state. Boon should
  lower runtime `FieldSet` and list-view changes into exact `DocumentPatch` and
  `LayoutPatch` updates where dependency metadata allows it.
- Blink style invalidation and RenderingNG: treat invalidation sets as compiler
  products. Boon can compile indexes from `(source_id, list_id, field_id,
  style/class flag)` to affected document, layout, hit-region, and paint scopes.
- Stylo: cache computed style structurally. Many NovyWave rows, controls,
  labels, panels, and waveform items share the same style inputs. Store compact
  style IDs rather than cloning full style records into every node.
- Servo/LayoutNG: separate logical document nodes from flat layout fragments.
  A node can produce multiple fragments with stable IDs, subtree ranges, and
  property-tree state. NovyWave waveform rows and Cells grid cells should be
  fragments over materialized ranges, not special-case widgets.
- Qt Quick scene graph: synchronize only dirty nodes into retained render state.
  Boon should make dirty scopes explicit, for example `hover_overlay_only`,
  `scroll_offset_only`, `text_edit_range`, `resize_layout`, and `source_replace`.

Immediate transfer:

- make view-origin metadata mandatory for `ForEach` and list-derived document
  nodes;
- split invalidation classes into paint-only, layout-only, hit-region,
  source-binding, list-structure, conditional-structure, and full-document;
- keep a report field for every full-lowering fallback reason;
- make scroll and pan update spatial/scroll state without runtime dispatch when
  no semantic source binding is hit.

### Host Input, Event Loop, And Frame Pacing

- SDL3: wake immediately on host input, drain pending events at frame start,
  and coalesce high-frequency wheel, pointer motion, and drag streams into one
  semantic delta per frame.
- Chrome input pipeline: route input through hit testing and compositor-scroll
  paths where possible; send only semantic events into runtime.
- SDL3 present modes and native render loops: keep a separate one-frame latency
  interaction profile from verifier/proof mode. Proof readback must not define
  ordinary manual interaction pacing.
- Dear ImGui: visible-range clipping and simple draw-list contracts are useful
  for keeping input-heavy UI predictable, even when Boon itself is retained.

Immediate transfer:

- record `poll_wait_ms`, input queue depth, coalesced event counts, dropped
  event counts, input-to-runtime latency, runtime-to-layout latency, and
  layout-to-present latency;
- make latest-wins coalescing explicit for hover, wheel, and drag where it is
  semantically legal;
- never block preview interaction on dev IPC, debug summaries, report writes,
  PNG writes, or verifier-only readbacks.

### Display Lists, Rendering, Text, And GPU Uploads

- GPUI: lower UI into primitive classes such as rectangles, shadows, glyphs,
  icons, images, lines, borders, carets, and selections; batch each class with
  dedicated shaders or instance buffers.
- Vello: retain scene fragments. Static panels, grid chrome, waveform traces,
  labels, and unchanged text runs should be cached and stitched with small
  dynamic overlays.
- WebRender and Firefox retained display lists: intern scene pieces and retain
  display chunks. A useful identity shape is
  `node_id + fragment_kind + style_id + property_tree_state + content_hash`.
- Chromium property trees: split scroll, transform, clip, and effect state from
  display items so scroll/pan updates can avoid layout and paint rebuilds.
- glyphon/cosmic-text: keep renderer-neutral text measurement separate from
  shaped-run caches and glyph-atlas caches.
- wgpu staging utilities and sokol-style resource reuse: coalesce many small
  writes into staging/ring uploads and report upload bytes by kind.

Immediate transfer:

- introduce dirty regions with surface, scroll root, layer, rects, reason, and
  revision;
- keep stable render bins for quads, borders, text, waveform segments, clips,
  overlays, and debug artifacts;
- tile NovyWave waveform regions by row and time range so cursor movement,
  selection, and scroll invalidate narrow slices;
- report draw calls, pipeline switches, bind group/cache misses, instance upload
  bytes, atlas upload bytes, glyph cache hits/misses, waveform tile hits/misses,
  dirty rect area, and reused display chunk counts.

### Dev Window, IPC, And Observability

- Browser engines and Flutter separate tracing/profiling from normal rendering.
  Boon's dev window should subscribe to bounded summaries, counters, and paged
  queries only. It must not mirror the runtime heap or raw preview render state.
- Perfetto and Chromium trace events: use flow IDs to connect source event seq,
  runtime turn, document patch, layout frame hash, render frame seq, GPU upload,
  and present.
- Qt scene graph logging: expose batch/cache diagnostics in structured form, not
  only logs.

Immediate transfer:

- add trace tracks for `host_input`, `runtime_tick`, `document_patch_apply`,
  `layout`, `display_list_retain`, `scene_build`, `gpu_upload`, `present`, and
  `dev_ipc`;
- collect counters for dirty keys, dirty nodes, reused display chunks, uploaded
  bytes, draw calls, tile invalidations, lock waits, and stale/discarded frame
  revisions;
- keep human observation as a follow-up after native GPU gates and reports, not
  as performance proof.

### NovyWave Waveform Bridge And Data Paging

- Keep the bridge pure-data at the Boon boundary. Rust may hold parser/cache
  state for `wellen` or other official libraries, but Boon should see stable
  descriptors, page IDs, bounded ranges, stats, diagnostics, and response
  fingerprints.
- Let Boon own selected files, selected signals, row order, row height,
  grouping, cursor, viewport, format, stale-response rejection, and request
  planning.
- Treat waveform data as pages, summaries, and range nodes. Do not materialize
  unbounded file data into Boon values or renderer display lists.

Immediate transfer:

- make visible rows, selected signals, cursor values, and requested time windows
  the demand roots for recomputation;
- cache and diff waveform pages by file identity, signal id, time range,
  resolution, format, and request fingerprint;
- report stale bridge responses, page cache hits/misses, decoded sample counts,
  and visible waveform tile counts.

## Source Links

Rust UI, renderer, and GPU:

- GPUI/Zed: <https://zed.dev/blog/videogame>
- Xilem: <https://docs.rs/xilem/latest/xilem/>
- Xilem architecture notes: <https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html>
- Vello vision: <https://github.com/linebender/vello/blob/main/doc/vision.md>
- egui: <https://docs.rs/egui/latest/egui/>
- epaint: <https://docs.rs/epaint/latest/epaint/>
- iced runtime: <https://docs.iced.rs/iced_runtime/>
- iced wgpu: <https://docs.iced.rs/iced_wgpu/>
- glyphon: <https://github.com/grovesNL/glyphon>
- cosmic-text: <https://docs.rs/cosmic-text/latest/cosmic_text/>
- wgpu `Queue`: <https://docs.rs/wgpu/latest/wgpu/struct.Queue.html>
- wgpu `StagingBelt`: <https://docs.rs/wgpu/latest/wgpu/util/struct.StagingBelt.html>

Native and non-Rust UI/rendering:

- SDL3 GPU: <https://wiki.libsdl.org/SDL3/CategoryGPU>
- SDL3 events: <https://wiki.libsdl.org/SDL3/CategoryEvents>
- SDL3 present mode: <https://wiki.libsdl.org/SDL3/SDL_GPUPresentMode>
- sokol: <https://github.com/floooh/sokol>
- Dear ImGui backends: <https://github.com/ocornut/imgui/blob/master/docs/BACKENDS.md>
- Qt Quick scene graph: <https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph.html>
- Qt scene graph renderer: <https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html>
- Flutter architecture: <https://docs.flutter.dev/resources/architectural-overview>
- Flutter performance: <https://docs.flutter.dev/perf/best-practices>
- Skia canvas/GPU docs: <https://skia.org/docs/user/api/skcanvas_creation/>

Browser engines and tracing:

- Servo architecture: <https://book.servo.org/design-documentation/architecture.html>
- Servo layout: <https://book.servo.org/design-documentation/layout.html>
- Servo off-main-thread parsing: <https://servo.org/blog/2017/08/23/gsoc-parsing/>
- html5ever: <https://github.com/servo/html5ever>
- Blink style invalidation: <https://chromium.googlesource.com/chromium/src/+/master/third_party/blink/renderer/core/css/style-invalidation.md>
- Blink paint README: <https://chromium.googlesource.com/chromium/src/+/HEAD/third_party/blink/renderer/core/paint/README.md>
- RenderingNG architecture: <https://developer.chrome.com/docs/chromium/renderingng-architecture>
- RenderingNG data structures: <https://developer.chrome.com/docs/chromium/renderingng-data-structures>
- Chrome input/rendering pipeline: <https://developer.chrome.com/blog/inside-browser-part4>
- Firefox rendering overview: <https://firefox-source-docs.mozilla.org/gfx/RenderingOverview.html>
- Retained display lists: <https://hacks.mozilla.org/2018/06/retained-display-lists/>
- Stylo/Quantum CSS: <https://hacks.mozilla.org/2017/08/inside-a-super-fast-css-engine-quantum-css-aka-stylo/>
- Perfetto track events: <https://perfetto.dev/docs/instrumentation/track-events>
- Chromium trace events: <https://chromium.googlesource.com/chromium/src.git/+/HEAD/docs/trace_events.md>

Parser, incremental computation, and runtimes:

- Tree-sitter: <https://tree-sitter.github.io/tree-sitter/>
- Salsa: <https://salsa-rs.github.io/salsa/>
- Rust incremental compilation overview: <https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html>
- Wasm3: <https://github.com/wasm3/wasm3>
- Wasmtime: <https://docs.wasmtime.dev/>
- Wasmtime profiling: <https://docs.wasmtime.dev/examples-profiling.html>
- Wasmer compilers: <https://docs.wasmer.io/runtime/compilers>
- QuickJS: <https://bellard.org/quickjs/>
- HyperFormula dependency graph: <https://hyperformula.handsontable.com/guide/dependency-graph.html>

## Follow-Up File Ideas

- `02-current-bottlenecks.md`: refresh NovyWave evidence and separate manual
  slowness from verifier-pass artifacts.
- `03-runtime-action-plan.md`: dense action plans, quickening, list-view
  patches, demand-aware recompute, and VM-grade counters.
- `04-document-layout-patches.md`: view origins, invalidation classes, layout
  fragments, property trees, and full-lowering bailout rules.
- `05-renderer-gpu-patches.md`: retained display chunks, waveform tiles, text
  caches, upload batching, and render metrics.
- `06-measurement-contract.md`: trace schema, phase-isolated samples, warmup
  policy, proof-mode separation, and acceptance gates.
