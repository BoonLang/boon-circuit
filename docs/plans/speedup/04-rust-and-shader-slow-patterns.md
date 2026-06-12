# Rust And Shader Slow Patterns

Date: 2026-06-12

## Purpose

This file catalogs Rust, WGPU, WGSL, renderer, event-loop, and proof-path
patterns that can make Boon and the NovyWave example slow. It complements the
previous speedup notes by focusing on what not to do.

This is a review and profiling checklist, not a rewrite mandate. A pattern
should become implementation work only after local evidence shows that it is
hot, semantically relevant, and fixable without shortcuts.

Local constraints:

- `docs/architecture/NATIVE_GPU_PIPELINE.md` remains the active native GPU
  contract.
- Keep the generated `WESL -> WGSL -> wgsl_bindgen` shader path. Do not load
  generated WGSL manually or duplicate generated bind group, pipeline layout,
  entry point, or shader constant definitions.
- Keep the preview generic over Boon source. Do not introduce example-name,
  source-file-name, scenario-step, or hardcoded NovyWave render shortcuts.
- Readback, PNG writing, hashing, full layout artifacts, and heavy reports are
  proof/verifier behavior. They must not define ordinary interaction pacing.
- Fix engine limitations in the engine. A Boon-level workaround is acceptable
  only as a temporary diagnostic aid, not as the final speedup.
- Use app-owned counters, reports, host events, WGPU timestamps/readbacks, and
  CPU profiles as proof. Do not use compositor screenshots or human observation
  as native GPU acceptance evidence.

## How To Use This File

For each suspected bottleneck, record four things before changing code:

1. the bad pattern and the exact hot path where it appears;
2. why it should be slow on the current workload;
3. the better engine shape to test;
4. the proof required to accept or reject the change.

Prefer small A/B experiments with representative NovyWave, Cells, TodoMVC, and
generic Boon workloads. Avoid broad cleanup patches whose speed effect cannot
be isolated.

## Measurement Traps

| Bad pattern | Why slow or misleading | Better shape | Proof required |
| --- | --- | --- | --- |
| Profiling debug builds | Debug builds can be orders of magnitude slower and can move the bottleneck away from release behavior | Profile optimized builds with stable flags and line-table debug info where needed | Command line, binary hash, profile artifact, workload description |
| Optimized-away microbenchmarks | The compiler may remove artificial work and make the result meaningless | Use representative inputs and `black_box` for synthetic benchmarks | Criterion report plus a macro benchmark on a real scenario |
| No warmup or stale flamegraphs | Cold caches, first-time shader work, and stale binaries can dominate | Warm the path and tie reports to the binary/worktree/config | Fresh report metadata and repeated runs |
| Aggregate FPS only | FPS does not connect input, runtime, layout, render, submit, present, IPC, and readback for one frame | Use frame ids and flow ids across the whole pipeline | Trace showing per-frame causality and stage latencies |
| Heavy tracing as default observability | Logging and profiling can change the workload being measured | Keep counters/spans cheap by default; make deep traces opt-in | A/B overhead with tracing/logging on and off |
| PGO trained on toy or stale data | PGO optimizes for the training path, including bad branches and stale hot spots | Train on representative workloads and validate on separate workloads | Profile provenance plus validation numbers outside the training set |
| Cargo-cult allocator swaps | Allocators are workload-specific and can hide allocation design problems | Reduce allocations first; test allocator changes behind a feature | Same workload A/B with throughput, RSS, fragmentation, and tail latency |

## Rust CPU And Runtime Patterns

| Bad pattern | Why slow | Better shape | Proof required |
| --- | --- | --- | --- |
| Hot-loop allocation churn: new `Vec`, `String`, `HashMap`, `PathBuf`, or `format!` per item | Allocation/deallocation costs allocator work, may lock, and collection growth copies data | Reuse buffers with `clear`, `with_capacity`, `reserve`, `write!`, or phase scratch arenas | Allocation count/bytes/callsite and before/after timing |
| Blind `.clone()`, `.to_owned()`, or `.to_string()` | Heap-backed clones allocate and copy data even when a borrow would do | Borrow `&str`, `&Path`, `&[T]`; move ownership; use `Cow` or `clone_from` where appropriate | Hot clone callstacks and allocation delta, not grep alone |
| Converting paths through strings | `Path` may not be UTF-8; string conversion can validate, allocate, or lose OS semantics | Keep APIs on `&Path`, `PathBuf`, `OsStr`, or `AsRef<Path>`; format only at boundaries | Allocation profile plus non-UTF-8 path tests |
| Default `HashMap` for hot dense/internal keys | The default hasher prioritizes HashDoS resistance; dense IDs often want arrays or faster trusted hashers | Use dense `Vec`/slot storage for dense IDs; pre-size maps; use faster hashers only for internal trusted keys | A/B benchmark on real keys and recorded threat model |
| `contains_key` followed by `insert` or `get_mut` | The key is hashed/searched twice and may be allocated twice | Use the entry API, borrowed lookup, interning, or stable numeric IDs | Lookup count and key comparison/hash cost |
| Collecting iterators only to loop again | `collect::<Vec<_>>()` allocates and can obscure size information | Stream the iterator, use `extend`, expose `impl Iterator`, or add `size_hint` where useful | Allocation profile and representative benchmark |
| `Box<dyn Trait>` or `&dyn Trait` per hot element | Vtable calls block inlining and `Box` adds allocation/pointer chasing | Keep dynamic dispatch at plugin/boundary layers; use enums, generics, or batching in inner loops | Dyn-vs-enum/generic benchmark and cache-miss profile |
| Huge generic bodies used with many concrete types | Monomorphization can bloat machine code and hurt instruction cache | Use thin generic wrappers around monomorphic private cores; reserve `dyn` for cold paths | `cargo bloat`, `llvm-lines`, and runtime/cache measurements |
| Pointer-rich linear scans: `Vec<Box<T>>`, nested maps, linked structures | Pointer chasing and small allocations cause cache misses | Use dense `Vec` plus indices, hot/cold field splits, flattening, or struct-of-arrays storage | Cache-miss profile and macro benchmark |
| Arena allocation with broad lifetimes | Bump arenas cannot free individual objects and may skip per-object `Drop` behavior | Use arenas for phase/frame/request scratch only; reset promptly; keep semantic state in generational storage | Peak RSS, destructor/resource tests, and allocation reduction |
| `Arc<Mutex<T>>` as default architecture | Atomic refcounts, heap indirection, and lock contention serialize work | Prefer ownership/borrowing, `Rc` on single-threaded paths, lock sharding, channels, or atomics for simple counters | Lock contention profile and thread-scaling benchmark |
| JSON/report generation in hot paths | `serde_json::Value`, `json!`, pretty printing, and unbuffered writes allocate and perform syscalls | Use typed `Serialize` structs, `to_writer`, `BufWriter`, sampling, and cold pretty printing | Disable-report benchmark, allocation profile, and syscall count |

## Renderer And UI Frame Patterns

| Bad pattern | Why slow | Better shape | Proof required |
| --- | --- | --- | --- |
| One giant invalidation root | A small edit dirties full layout, batching, paint, and render analysis | Split static, dynamic, interactive, scroll, and overlay surfaces by update frequency | Dirty-region counts, cache-hit rates, and edit latency |
| Caching volatile widgets or tiles | Cache maintenance adds memory traffic and repaint work without reuse | Mark volatile content volatile; cache only stable groups or profitable tiles | Cache rebuild count, tile invalidation percentage, and upload bytes |
| Immediate mode means continuous redraw | Idle redraw burns CPU/GPU and battery | Use event/request-driven repaint with scheduled animation ticks only where needed | Zero-input frame count, idle CPU/GPU cost, and repaint reasons |
| Retained shell rebuilt every frame before immediate rendering | Pays retained synchronization and immediate traversal every frame | Keep app state authoritative, derive visible UI incrementally, and virtualize large lists | Model rebuild time and visible-item count versus total item count |
| Layout read/write thrash | Interleaved mutation and measurement forces synchronous layout work | Batch reads first, writes after; cache prior-frame measurements | Forced-layout count and duration per frame |
| Scroll rebuilds the document | Wheel or drag scroll is often offset plus newly visible ranges, not source/runtime work | Use a compositor-style scroll path: mutate offset, patch visible ranges, defer full relayout | Wheel-to-visible p95/max and relayout count during passive scroll |
| Glyph, font, or atlas churn | Text rasterization and texture upload become frame critical | Keep persistent font objects, shaped-run caches, and incremental atlas deltas | Glyph rasterizations, atlas reallocations, text cache hit/miss/eviction counts |
| Draw call and state churn | Driver validation, texture switches, bind changes, and pipeline switches dominate simple 2D work | Batch by primitive/material/texture, atlas compatible assets, and merge stable layers | Draw calls, pipeline binds, texture switches, encoder time, and overdraw |
| Bad frame pacing or queue stuffing | Input waits behind queued frames and short/long frame alternation creates stutter | Bound frames in flight, pace around present deadlines, and sample input close to render | Input-to-present latency and actual versus expected present timeline |
| Dev tools block preview | Debug windows, logs, inspectors, or editors can turn tool spikes into preview jank | Let preview own its frame loop; communicate by bounded snapshots and IPC lanes | Preview p95/max unchanged under dev tracing/editor stress |
| Readback in normal preview | Readback can wait for GPU completion and mapped buffers cannot be used by the GPU | Keep readback in proof mode with async staging and delayed consumption | No normal-mode readback calls; isolated proof-mode readback latency |

## WGPU And WebGPU Resource Patterns

| Bad pattern | Why slow | Better shape | Proof required |
| --- | --- | --- | --- |
| Creating shader modules or render/compute pipelines on the frame path | Pipeline creation is where shader compilation and driver work can stall | Prebuild/cache pipelines; use async creation where available; evaluate `wgpu::PipelineCache` where supported | Pipeline/module creations per frame and cold/warm timing |
| Relying on implicit/default pipeline layouts | Default layouts are shader-derived and can prevent bind group reuse across pipelines | Define explicit shared layouts by convention, such as frame/camera, material, and texture groups | Distinct layout signatures and redundant `set_bind_group` calls |
| Creating or updating bind groups every draw/frame | Descriptor allocation/update can dominate CPU time in draw-heavy scenes | Cache bind groups; recycle descriptor-equivalent state; use per-frame buffers with offsets | CPU profile around bind group creation and bind-group count per frame |
| One small buffer and one bind group per object | Many buffers/descriptors increase CPU overhead and reduce batching | Pack object data into ring/storage/uniform buffers; bind once and index or offset per draw | Buffer object count, bind group count, and CPU frame time |
| Broad buffer usage flags, especially mappable hot buffers | Usage flags affect memory placement; mappable primary buffers can be slow on native backends | Split GPU-local buffers from upload/download staging buffers and request only needed usages | Usage audit and backend/frame-time comparison |
| Many tiny `queue.write_buffer` calls | On native, `write_buffer` copies into staging memory; many small calls amplify temporary allocation/copy overhead | Batch writes; use `write_buffer_with` when constructing bytes directly; use `StagingBelt` for many small writes | Write-call count, bytes, allocation callsites, and upload CPU time |
| Expecting `write_buffer` or `write_texture` to execute immediately | Transfers start on the next queue submit, and misplaced submits can delay visibility or add work | Schedule uploads before encoding consumers and batch submits unless an empty submit is intentional | Upload/submit ordering logs and frame pacing comparison |
| Mapping or reading back GPU-used resources in the render loop | Mapping waits until the GPU is done and mapped buffers are unavailable to the GPU | Copy to `MAP_READ | COPY_DST` staging buffers and consume via async readback rings | `map_async` latency, queue idle time, and frame spikes |
| Full-frame texture readbacks or careless row padding | Buffer-texture copies need 256-byte row alignment, increasing bandwidth and copy work | Read minimal rectangles or GPU-reduced summaries; account for padded row bytes | Useful bytes versus padded bytes and timestamped copy/map stages |
| Timestamp queries with immediate CPU readback | Profiling can accidentally synchronize the CPU with GPU work | Resolve timestamps to buffers, copy into a readback ring, and consume later | Profiler overhead budget and timestamp/readback latency |
| Unnecessary copies/transfers between passes | Copies and conservative synchronization reduce overlap and can create bubbles | Keep data GPU-side, use direct sampling/storage, or add latency between producer and consumer | GPU timeline showing copy-heavy bubbles before/after |

## WGSL And Shader Code Patterns

| Bad pattern | Why slow or risky | Better shape | Proof required |
| --- | --- | --- | --- |
| Host/WGSL layout mismatch, especially `vec3`, arrays, and uniforms | Misalignment causes validation failures, wrong reads, or wasteful repacking | Use explicit padding, `@align`/`@size`, host-side offset assertions, and `vec4`/packed structs where practical | Compile-time size/offset tests and shader readback tests |
| Divergent dynamic branches or loops in hot shaders | Divergence hurts SIMD occupancy and can block hoisting of long-latency operations | Split passes/materials, specialize shaders, branch on uniform state, or precompute masks | GPU timestamps, instruction/register counts, and variant comparison |
| Texture sampling inside non-uniform fragment control flow with implicit derivatives | Derivative-based operations require uniformity guarantees and can produce artifacts or slow paths | Sample before divergent branches, use explicit LOD/grad where appropriate, or split the draw | Visual LOD tests, uniformity diagnostics, and GPU timing |
| Too many samples, large uncompressed textures, or heavy filtering | Texture fetches are long-latency and bandwidth-sensitive | Use mipmaps, compression, smaller targets, gather operations, or point filtering where acceptable | Texture sample count, bandwidth counters where available, timestamps, and image diff |
| Hardcoded unprofiled compute workgroup sizes | Bad group sizes reduce occupancy/cache locality or create launch overhead | Start with conservative sizes and sweep per backend using override constants where practical | Workgroup-size sweep with GPU time and occupancy proxies |
| Overdraw, `discard`, fragment depth writes, or storage writes on opaque hot paths | Fragment work often dominates, and these features can disable early tests | Sort opaque front-to-back, use depth/stencil tests, and isolate special fragment paths | Overdraw heatmap/readback and fragment workload timestamps |
| Excessive pipeline switches or redundant state changes | Switches and validation add CPU overhead and can disrupt GPU scheduling | Sort by pipeline/material, share layouts, reduce redundant binds, and instance compatible primitives | Draw/pipeline/bind counters and encoder CPU time |

## Local Candidates To Watch

These are not automatic bug claims. They are local patterns worth measuring
before future speed work changes them.

- Renderer upload paths currently include fresh byte-vector conversion from
  `Vec<f32>` and `Vec<u32>` data and multiple buffer writes. A future
  experiment should compare this against POD instance structs plus
  `bytemuck`/`zerocopy`-style byte views and batched upload staging.
- Text reporting must be detailed enough to distinguish shaped runs from text
  cache hits, misses, evictions, failures, and glyph atlas uploads. A count of
  visible text runs is not enough to prove steady-scroll reuse.
- Blocking readback, PNG encoding, and artifact hashing must stay isolated to
  proof/verifier paths. Normal preview interaction should not wait on those
  steps.
- Full document lowering, full layout artifact writes, and full runtime summary
  serialization should be bailout or verifier behavior, not ordinary hover,
  scroll, drag, cursor, or small-edit behavior.
- Legacy ACK paths that include runtime summaries or layout proof payloads can
  smuggle expensive work into synchronization. Newer source-project ACK paths
  should keep heavy proof and runtime summaries out of hot ACKs.
- Synthetic IPC stress reports should be labeled as synthetic and should not
  replace real preview frame behavior, queue-depth, stall, and kill-proof data.
- Source-path and example-specific branches are risky exceptions. They should
  not become architecture for NovyWave performance.
- Any `CopyToPresent` or other scaffold present report with zero draw metrics
  and no acquired surface texture must not be used as final acceptance evidence
  for native GPU handoff.

## Adoption Checklist

Before accepting a speedup inspired by this file:

- name the bad pattern and the path where it is hot;
- show the old and new measurements on the same representative workload;
- record allocation, clone, lookup, draw, upload, cache, queue, or readback
  counters as appropriate;
- preserve generic Boon semantics and the native GPU contract;
- keep proof-mode readback separate from interaction mode;
- keep full recompute or generic interpreter paths as test oracles where a
  specialized path is introduced;
- add or update verifier/report fields only when they do not make the hot path
  slower.

## Source Appendix

Rust and measurement:

- Rust Performance Book, heap allocations:
  <https://nnethercote.github.io/perf-book/heap-allocations.html>
- Rust Performance Book, collections:
  <https://nnethercote.github.io/perf-book/collections.html>
- Rust Performance Book, type sizes and cache effects:
  <https://nnethercote.github.io/perf-book/type-sizes.html>
- Rust `Cow`:
  <https://doc.rust-lang.org/std/borrow/enum.Cow.html>
- Rust `Path`:
  <https://doc.rust-lang.org/std/path/struct.Path.html>
- Rust `HashMap`:
  <https://doc.rust-lang.org/std/collections/struct.HashMap.html>
- Rust `Entry` API:
  <https://doc.rust-lang.org/std/collections/hash_map/enum.Entry.html>
- Rust `Arc`:
  <https://doc.rust-lang.org/std/sync/struct.Arc.html>
- Criterion:
  <https://bheisler.github.io/criterion.rs/book/>
- Rust `black_box`:
  <https://doc.rust-lang.org/std/hint/fn.black_box.html>
- rustc PGO:
  <https://doc.rust-lang.org/rustc/profile-guided-optimization.html>

WGPU, WebGPU, and WGSL:

- wgpu queue:
  <https://docs.rs/wgpu/latest/wgpu/struct.Queue.html>
- wgpu buffer:
  <https://docs.rs/wgpu/latest/wgpu/struct.Buffer.html>
- wgpu staging belt:
  <https://docs.rs/wgpu/latest/wgpu/util/struct.StagingBelt.html>
- wgpu pipeline cache:
  <https://docs.rs/wgpu/latest/wgpu/struct.PipelineCache.html>
- wgpu timestamp queries:
  <https://docs.rs/wgpu/latest/wgpu/enum.QueryType.html>
- WGSL specification:
  <https://www.w3.org/TR/WGSL/>
- WebGPU specification:
  <https://www.w3.org/TR/webgpu/>
- WebGPU Fundamentals, memory layout:
  <https://webgpufundamentals.org/webgpu/lessons/webgpu-memory-layout.html>
- Learn Wgpu, memory layout:
  <https://sotrh.github.io/learn-wgpu/showcase/alignment/>

Shader and GPU performance:

- NVIDIA, Advanced API Performance:
  <https://developer.nvidia.com/blog/tag/advanced-api-performance/>
- NVIDIA, Thinking Parallel:
  <https://developer.nvidia.com/blog/thinking-parallel-part-i-collision-detection-gpu/>
- Microsoft, dynamic branching:
  <https://learn.microsoft.com/en-us/windows/win32/direct3dhlsl/dx-graphics-hlsl-flow-control>
- Android Frame Pacing:
  <https://developer.android.com/games/sdk/frame-pacing>
- Vulkan tutorial, frames in flight:
  <https://docs.vulkan.org/tutorial/latest/03_Drawing_a_triangle/03_Drawing/03_Frames_in_flight.html>
- Vulkan samples, timestamp queries:
  <https://docs.vulkan.org/samples/latest/samples/api/timestamp_queries/README.html>

Renderer and UI systems:

- Chromium RenderingNG:
  <https://developer.chrome.com/docs/chromium/renderingng>
- Chromium compositor overview:
  <https://chromium.googlesource.com/chromium/src/+/lkgr/docs/how_cc_works.md>
- web.dev, avoid layout thrashing:
  <https://web.dev/articles/avoid-large-complex-layouts-and-layout-thrashing>
- WebRender picture caching:
  <https://mozillagfx.wordpress.com/2018/11/02/webrender-picture-caching/>
- WebRender capture infrastructure:
  <https://kvark.github.io/webrender/debug/ron/2018/01/23/wr-capture-infra.html>
- Dear ImGui paradigm:
  <https://github.com/ocornut/imgui/wiki/About-the-IMGUI-paradigm>
- Dear ImGui debug tools:
  <https://github.com/ocornut/imgui/wiki/Debug-Tools>
- Dear ImGui font notes:
  <https://skia.googlesource.com/external/github.com/ocornut/imgui/+/master/docs/FONTS.md>
- egui repaint requests:
  <https://docs.rs/egui/latest/egui/struct.Context.html#method.request_repaint>
- egui fonts:
  <https://docs.rs/egui/latest/egui/text/struct.Fonts.html>
- egui texture deltas:
  <https://docs.rs/egui/latest/egui/struct.TexturesDelta.html>
- Unity UI optimization tips:
  <https://unity.com/how-to/unity-ui-optimization-tips>
- Unreal UMG optimization guidelines:
  <https://dev.epicgames.com/documentation/unreal-engine/optimization-guidelines-for-umg-in-unreal-engine>
- Unreal Slate Insights:
  <https://dev.epicgames.com/documentation/unreal-engine/slate-insights-in-unreal-engine>
- Godot CanvasItem redraw:
  <https://docs.godotengine.org/en/3.6/classes/class_canvasitem.html>
- Godot GPU optimization:
  <https://docs.godotengine.org/en/3.3/tutorials/optimization/gpu_optimization.html>
- Perfetto FrameTimeline:
  <https://perfetto.dev/docs/data-sources/frametimeline>

Local docs:

- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/plans/speedup/01-inspiration.md`
- `docs/plans/speedup/02-novel-research-ideas.md`
- `docs/plans/speedup/03-rust-speed-libraries.md`
- `budgets/native-gpu.toml`
