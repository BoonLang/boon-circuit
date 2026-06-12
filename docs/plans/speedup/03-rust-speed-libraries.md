# Rust Libraries For Faster Boon

Date: 2026-06-12

## Purpose

This file catalogs Rust libraries that could help speed up Boon and the
NovyWave example. It is deliberately practical: low-level crates for strings,
collections, allocation, ids, dirty sets, and byte storage sit next to
higher-level libraries that could outsource selected algorithms or whole
layers.

This is not approval to add dependencies. Each crate needs a measured local
bottleneck, a narrow experiment, and verifier-backed proof before adoption.

Local constraints:

- `docs/architecture/NATIVE_GPU_PIPELINE.md` remains the active native GPU
  contract.
- Do not use a dependency to hide an engine limitation behind a Boon-level
  workaround.
- Do not add example-specific shortcuts for NovyWave, Cells, or TodoMVC.
- Do not expose external parser, mmap, renderer, or cache handles as Boon
  semantic values.

## Local Baseline

The current workspace already uses a small dependency set:

- parser/type/runtime: `chumsky`, `ena`, `bitvec`;
- editor text: `ropey`, `unicode-segmentation`;
- serialization/reports: `serde`, `serde_json`, `toml`, `sha2`;
- native renderer/proof: `wgpu`, `glyphon`, `image`, `resvg`;
- native app/window: local `app_window`;
- historical/reference path: `ply-engine`.

The main speedup gaps this file targets are:

- string-heavy runtime identity, field paths, style attrs, module names, and
  source labels;
- allocation-heavy parse/lower/runtime/layout/render phases;
- map and set costs in dirty propagation, dependency lookup, document patches,
  and style lookup;
- full or clone-heavy layout/display/render state;
- per-frame byte conversion and GPU upload staging;
- bounded waveform bridge pages and sparse visible-window data.

## Adoption Rules

- Profile first. A crate is only a candidate after a report names the hot cost:
  allocation count/bytes, hash lookup count, clone count, upload bytes,
  text-shape misses, dirty-set fanout, or page decode time.
- Add one dependency per experiment. Keep the write scope narrow enough to
  isolate the win or failure.
- Preserve the generic engine path. The same dependency-backed optimization
  must work from typed runtime/document/render data, not from example names.
- Keep plain Rust fallbacks or full-recompute oracles where semantics are at
  risk.
- Record proof fields in reports: old/new timing, allocation deltas, memory
  deltas, cache hit/miss counts, and fallback/deopt counts.

## Library Map

| Crate | Layer | Candidate use | Adoption tier | Risks | Proof needed |
| --- | --- | --- | --- | --- | --- |
| `lasso` | Parser/IR/runtime | Intern module names, fields, source labels | Early | Intern table lifetime policy | Fewer string clones/lookups |
| `string-interner` | Parser/IR/runtime | Alternative symbol interner | Early | Duplicate with custom IDs | Same as `lasso` comparison |
| `smol_str` | Runtime/document | Small copied labels and attrs | Early | Still allocates when large | Allocation count reduction |
| `compact_str` | Runtime/document | Inline short strings | Early | API churn vs `String` | Allocation and clone profile |
| `smartstring` | Runtime/document | Small-string optimization | Evaluate | Similar overlap with `compact_str` | Microbench against real labels |
| `arrayvec::ArrayString` | Reports/hot labels | Fixed-capacity small strings | Narrow | Panics/truncation policy | No truncation, fewer allocs |
| `arcstr` | Shared text refs | Shared immutable strings | Later | Refcount overhead | Clone-heavy path wins |
| `smallvec` | Patches/edges | Tiny patch lists and dependency edges | Early | Spills can hide costs | Heap allocs after warmup |
| `arrayvec` | Fixed tiny data | Fixed patch causes, stack strings | Early | Capacity errors | No overflow in stress cases |
| `tinyvec` | no-std-ish vecs | Alternative tiny vecs | Later | Less common locally | Beats `smallvec` in benchmark |
| `hashbrown` | Maps | Faster/custom hash maps | Early | Hash choice/security | Lookup timing and memory |
| `rustc-hash` | Internal maps | Fast deterministic-ish hashing | Early | Not DoS-resistant | Internal-only speed profile |
| `ahash` | Internal maps | Fast randomized hashing | Evaluate | Reproducibility concerns | Stable report behavior |
| `indexmap` | Ordered maps | Stable iteration with hash lookup | Narrow | More memory | Removes sort or order hacks |
| `slotmap` | Hidden ids | Generational row/node handles | Early | Key serialization policy | Stale key rejection tests |
| `generational-arena` | Hidden ids | Row/node/source-binding arenas | Early | Free-list behavior | Remove/reinsert correctness |
| `id-arena` | Stable arenas | Append-only compiler/layout nodes | Narrow | No generation guard | Good only for append-only phases |
| `bumpalo` | Scratch allocation | Parse/lower/layout/render scratch | Early | Borrow/lifetime complexity | Phase alloc reset proof |
| `bump-scope` | Scratch allocation | Scoped bump arenas | Evaluate | Smaller ecosystem | Simpler scoped scratch proof |
| `typed-arena` | Arena allocation | Long-lived compiler structures | Later | No individual frees | Leak/lifetime audit |
| `fixedbitset` | Dirty sets | Dense node/list dirty sets | Early | Bad for sparse high ids | Dense dirty-set benchmarks |
| `roaring` | Dirty sets | Sparse or clustered dirty ids | Evaluate | Serialization and overhead | Sparse fanout wins |
| `hibitset` | Dirty sets | Hierarchical sparse bitsets | Later | Less mainstream | Beats `roaring`/`fixedbitset` |
| `bytemuck` | GPU upload | Cast POD instance slices to bytes | Early | Requires correct POD invariants | No per-frame conversion Vec |
| `zerocopy` | Bridge/GPU | Typed bytes and page structs | Early | Layout/version contract | Safe page and upload tests |
| `bytes` | IPC/pages | Shared byte buffers | Narrow | Refcount and slicing semantics | Fewer copies in IPC/page path |
| `memmap2` | Bridge/pages | Memory-map waveform/Arrow pages | Later | File lifetime and platform behavior | Page cache and stale-file tests |
| `rkyv` | Zero-copy reports/pages | Archived debug/page payloads | Later | Format complexity | Faster load without semantic leak |
| `mimalloc` | Global allocator | Benchmark allocator overhead | Defer | Global behavior change | Whole-gate A/B reports |
| `tikv-jemallocator` | Global allocator | Benchmark allocator overhead | Defer | Size/platform effects | Whole-gate A/B reports |
| `snmalloc-rs` | Global allocator | Benchmark allocator overhead | Defer | Platform/ecosystem risk | Whole-gate A/B reports |
| `logos` | Lexer | Fast tokenization | Evaluate | Parser rewrite cost | Is lexing actually hot? |
| `winnow` | Parser | Alternative parser combinators | Later | Rewrite cost | Parser benchmark win |
| `tree-sitter` | Editor parser | Incremental parse trees | Later | Grammar and integration cost | Source-edit latency win |
| `rowan` | Syntax tree | Lossless green/red trees | Later | Larger compiler refactor | Incremental edit reuse |
| `salsa` | Compiler queries | Incremental parse/type/IR queries | Later | Query architecture cost | Reuse proof and invalidation tests |
| `rayon` | CPU parallelism | Parse/lower/page/render prep | Narrow | Scheduling overhead | Parallel workload is large enough |
| `crossbeam-channel` | IPC/work queues | Bounded nonblocking lanes | Early | Queue policy complexity | No preview blocking |
| `flume` | IPC/work queues | Alternative channels | Evaluate | Duplicate with crossbeam | Queue benchmark comparison |
| `moka` | Caches | Page/tile/debug caches | Narrow | Eviction opacity | Hit/miss/eviction report fields |
| `vello` | Rendering | Path-heavy retained renderer reference | Defer | Renderer replacement temptation | Prototype only |
| `lyon` | Geometry | Tessellate paths/rounded shapes | Narrow | Tessellation cost | Shape-heavy frame benchmark |
| `guillotiere` | Atlas | Texture/glyph atlas allocation | Evaluate | Needs atlas metrics first | Fewer atlas evictions |
| `etagere` | Atlas | Alternative atlas allocator | Evaluate | Same as `guillotiere` | A/B atlas churn comparison |
| `cosmic-text` | Text | Direct shaping/layout control | Narrow | Overlaps `glyphon` | Text cache miss reduction |
| `wellen` | Waveforms | VCD/FST/GHW parsing bridge | Early | Bridge contract design | Bounded page correctness |
| `arrow` | Pages/bridge | Columnar waveform pages | Evaluate | Heavy dependency surface | Page copy/decode reduction |
| `polars` | Data pages | Lazy dataframe-style analysis | Defer | Too broad for core runtime | Only for bridge/offline data |
| `datafusion` | Data pages | Query engine for large data | Defer | Too broad for UI hot path | Only if bridge queries need SQL-like plans |
| `differential-dataflow` | Runtime ideas | Delta maintenance reference | Defer | Core semantics replacement risk | Mine ideas before dependency |
| `timely` | Runtime ideas | Dataflow scheduling reference | Defer | Architecture replacement risk | Mine ideas before dependency |

## Shortlist By Layer

### Strings And Symbols

Candidates: `lasso`, `string-interner`, `smol_str`, `compact_str`,
`smartstring`, `arrayvec::ArrayString`, `arcstr`.

Most likely first experiment:

- intern field names, style attrs, source labels, module paths, operator names,
  and document attrs into dense symbols during parse/lower;
- keep human-readable strings at diagnostics/report boundaries;
- compare `lasso` and `string-interner` against a small custom newtype table if
  the adoption surface looks too large.

Use small-string crates only where values remain human-facing but are copied
often: labels, compact style names, debug tags, and short source identifiers.
Do not replace all `String` usage blindly.

Proof:

- allocation counts and bytes before/after warmup;
- string clone counts where instrumentation exists;
- runtime/source-route lookup time;
- report stability and deterministic hashes.

### Inline Collections And Maps

Candidates: `smallvec`, `arrayvec`, `tinyvec`, `hashbrown`, `rustc-hash`,
`ahash`, `indexmap`.

Most likely first experiments:

- use `SmallVec` or `ArrayVec` for tiny document patches, dirty causes,
  source-route outputs, dependency edges, invalidation classes, and render
  fragment lists;
- evaluate `rustc-hash` or `hashbrown` for internal maps keyed by dense ids;
- use `indexmap` only when stable iteration order removes a separate sort or
  duplicate vector.

Do not use faster hashers for untrusted external strings or report-facing hash
identity. Prefer them only for internal, compiler-produced dense keys.

Proof:

- fewer heap allocations after warmup;
- fewer map lookups or lower lookup wall time;
- no nondeterministic report ordering;
- no capacity overflows under Cells and NovyWave stress scenarios.

### IDs, Arenas, And Allocation

Candidates: `slotmap`, `generational-arena`, `id-arena`, `bumpalo`,
`bump-scope`, `typed-arena`.

Most likely first experiments:

- use generational keys for list rows, document nodes, source bindings, retained
  display chunks, and possibly compiled IR nodes;
- use `bumpalo` only for phase-local scratch in parse/lower/layout/render
  staging;
- use append-only arenas for compiler artifacts whose lifetime is exactly the
  compilation or mounted program.

Semantic state should not live in a bump arena unless its lifetime is already
the entire mounted program and explicit reset semantics are proven. Deleted
rows/nodes need generation checks or quarantine so stale source bindings cannot
mutate new state.

Proof:

- stale key rejection tests;
- remove/reinsert list-row tests;
- source-binding stale-event tests;
- allocation profile for scratch phases;
- memory peak/RSS reports for long NovyWave sessions.

### Dirty Sets And Dependency Tracking

Candidates: existing `bitvec`, plus `fixedbitset`, `roaring`, `hibitset`.

Most likely first experiments:

- use `fixedbitset` for dense node ids or list ids;
- compare `roaring` for sparse/high-id dirty fields or document node sets;
- keep `bitvec` where bit-level storage is already useful and measured.

Choice should be data-shaped:

- dense contiguous ids: `fixedbitset`;
- sparse clustered ids: `roaring`;
- tiny fixed sets: `ArrayVec` or bit-packed newtypes;
- hot boolean flags in columns: `bitvec` or packed custom columns.

Proof:

- dirty-set construction time;
- union/intersection/difference time;
- memory footprint;
- fanout histograms;
- equality against full recompute.

### Byte Storage, GPU Uploads, And Zero Copy

Candidates: `bytemuck`, `zerocopy`, `bytes`, `memmap2`, `rkyv`.

Most likely first experiments:

- replace per-frame `Vec<u8>` conversion for GPU instance data with
  `bytemuck::cast_slice` where types can safely be made POD;
- use `zerocopy` for typed waveform page records or bridge buffers where layout
  is explicitly versioned;
- use `bytes` for shared IPC/page payload buffers if clone-heavy payload paths
  show up in profiles.

`memmap2` and `rkyv` are later candidates. They are strongest for waveform page
storage, large fixtures, or cached bridge data, not for core Boon semantic
state.

Proof:

- upload byte conversion allocation count goes down;
- upload wall time or CPU staging time improves;
- safety invariants for POD/page structs are documented;
- stale mmap/page fingerprint tests prevent wrong-file reuse.

### Parser, Compiler, And Query Layers

Candidates: current `chumsky`, plus `logos`, `winnow`, `tree-sitter`, `rowan`,
`syntree`, `salsa`.

Most likely path:

- keep `chumsky` until profiling separates lexing, parsing, lowering, and
  source replacement costs;
- evaluate `logos` only if tokenization is hot;
- evaluate `tree-sitter` and `rowan` for editor/source-incremental workflows,
  not as a quick runtime speed fix;
- evaluate `salsa` only after compiler artifacts have stable query keys and
  invalidation boundaries.

Proof:

- parse/lower/typecheck/source-replace timing split;
- edit-range reuse tests;
- source hash/version correctness;
- diagnostics stay span-rich and deterministic.

### Rendering, Text, And Geometry

Candidates: current `wgpu` and `glyphon`, plus `vello`, `vello_cpu`, `lyon`,
`guillotiere`, `etagere`, `cosmic-text`.

Most likely first experiments:

- use `lyon` only for shape-heavy paths that need robust tessellation;
- evaluate `guillotiere` or `etagere` only after atlas eviction metrics show
  churn;
- use `cosmic-text` directly only if `glyphon`'s abstraction prevents the text
  cache split needed by Boon;
- treat `vello` as a prototype/reference for retained scene and path-heavy
  rendering, not as a native GPU proof shortcut.

Proof:

- glyph shaping/cache hit/miss counts;
- atlas upload and eviction counts;
- draw calls and pipeline switches;
- render encode time;
- app-owned WGPU readback still proves pixels for native gates.

### Waveform Bridge And Data Pages

Candidates: `wellen`, `vcd`, FST-related crates/bindings, `arrow`, `polars`,
`datafusion`, `memmap2`, `bytemuck`, `zerocopy`.

Most likely first experiments:

- use `wellen` behind a typed bridge for VCD/FST/GHW parsing;
- expose Boon-owned requests for visible rows, time windows, pixel-scale
  budgets, and stale-response fingerprints;
- store bridge responses as columnar pages: time deltas, value codes, flags,
  min/max/first/last buckets, and transition metadata;
- use `arrow` only if its columnar and IPC/mmap ecosystem offsets dependency
  size and complexity.

`polars` and `datafusion` are defer candidates. They may be useful for offline
analysis, bridge-side queries, or future large-data tools, but they are too
large for the preview hot path without a specific measured need.

Proof:

- exact-vs-page rendering comparison for bounded fixtures;
- selected-signal sparse extraction tests;
- payload byte caps;
- page cache hit/miss and eviction reports;
- no Rust parser/cache handles leak into Boon values.

### Concurrency, Caches, And IPC

Candidates: `rayon`, `crossbeam-channel`, `flume`, `moka`, `mini-moka`,
`bytes`.

Most likely first experiments:

- use `crossbeam-channel` or `flume` for bounded preview/dev lanes if current
  queues block or over-allocate;
- use `moka`/`mini-moka` for page/tile/debug caches only when eviction and
  cache observability are explicit;
- use `rayon` for large independent batches, not small per-frame tasks.

Proof:

- no preview blocking on dev IPC;
- queue depth, blocked send, dropped/overwritten sample counters;
- cache hit/miss/eviction reports;
- latency does not regress under burst input.

## Do Not Reach For Yet

- Do not swap the global allocator until allocation profiles identify allocator
  overhead and a targeted A/B benchmark exists.
- Do not replace the parser with `tree-sitter` or `rowan` before measuring
  whether current cost is lexing, parsing, lowering, typechecking, or source
  replacement.
- Do not outsource Boon semantics wholesale to `differential-dataflow`,
  `timely`, `datafusion`, or `polars`.
- Do not replace the native renderer with `vello` as a shortcut. Prototype
  against it only when path-heavy retained-scene workloads justify the work.
- Do not expose `wellen`, Arrow, mmap, DataFusion, Polars, or parser handles
  directly to Boon values.
- Do not use small-stack containers where overflow policy is unclear. Any
  fixed capacity needs stress tests and diagnostics.

## Source Links

Strings and symbols:

- `lasso`: <https://docs.rs/lasso>
- `string-interner`: <https://docs.rs/string-interner>
- `smol_str`: <https://docs.rs/smol_str/>
- `compact_str`: <https://docs.rs/compact_str>
- `smartstring`: <https://docs.rs/smartstring>
- `arrayvec::ArrayString`: <https://docs.rs/arrayvec>
- `arcstr`: <https://docs.rs/arcstr>

Inline collections, maps, and hashes:

- `smallvec`: <https://docs.rs/smallvec/>
- `arrayvec`: <https://docs.rs/arrayvec>
- `tinyvec`: <https://docs.rs/tinyvec>
- `hashbrown`: <https://docs.rs/hashbrown>
- `rustc-hash`: <https://docs.rs/rustc-hash/latest/rustc_hash/>
- `ahash`: <https://docs.rs/ahash>
- `indexmap`: <https://docs.rs/indexmap>

IDs, arenas, and allocation:

- `slotmap`: <https://docs.rs/slotmap/>
- `generational-arena`: <https://docs.rs/generational-arena>
- `id-arena`: <https://docs.rs/id-arena>
- `bumpalo`: <https://docs.rs/bumpalo>
- `bump-scope`: <https://docs.rs/bump-scope>
- `typed-arena`: <https://docs.rs/typed-arena>
- `mimalloc`: <https://docs.rs/mimalloc>
- `tikv-jemallocator`: <https://docs.rs/tikv-jemallocator>
- `snmalloc-rs`: <https://docs.rs/snmalloc-rs>

Dirty sets and bytes:

- `fixedbitset`: <https://docs.rs/fixedbitset/>
- `roaring`: <https://docs.rs/roaring>
- `hibitset`: <https://docs.rs/hibitset>
- `bytemuck`: <https://docs.rs/bytemuck>
- `zerocopy`: <https://docs.rs/zerocopy>
- `bytes`: <https://docs.rs/bytes>
- `memmap2`: <https://docs.rs/memmap2>
- `rkyv`: <https://docs.rs/rkyv>

Parser, compiler, and query:

- `chumsky`: <https://docs.rs/chumsky>
- `logos`: <https://docs.rs/logos/latest/logos/>
- `winnow`: <https://docs.rs/winnow>
- `tree-sitter`: <https://docs.rs/tree-sitter>
- `rowan`: <https://docs.rs/rowan>
- `syntree`: <https://docs.rs/syntree>
- `salsa`: <https://docs.rs/salsa>

Rendering, text, and geometry:

- `wgpu`: <https://docs.rs/wgpu>
- `glyphon`: <https://docs.rs/glyphon>
- `cosmic-text`: <https://docs.rs/cosmic-text>
- `vello`: <https://docs.rs/vello>
- `vello_cpu`: <https://docs.rs/vello_cpu>
- `lyon`: <https://docs.rs/lyon/>
- `guillotiere`: <https://docs.rs/guillotiere>
- `etagere`: <https://docs.rs/etagere>

Waveform, data, concurrency, and caches:

- `wellen`: <https://crates.io/crates/wellen>
- `vcd`: <https://docs.rs/vcd>
- `arrow`: <https://docs.rs/arrow/latest>
- `polars`: <https://docs.rs/polars/latest/polars/>
- `datafusion`: <https://docs.rs/datafusion/latest/datafusion/>
- `differential-dataflow`: <https://docs.rs/differential-dataflow>
- `timely`: <https://docs.rs/timely>
- `petgraph`: <https://docs.rs/petgraph>
- `rayon`: <https://docs.rs/rayon>
- `crossbeam-channel`: <https://docs.rs/crossbeam-channel>
- `flume`: <https://docs.rs/flume>
- `moka`: <https://docs.rs/moka/latest/moka/>
- `mini-moka`: <https://docs.rs/mini-moka>
