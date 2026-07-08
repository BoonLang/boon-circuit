# Engine Runtime Renderer Speed Plan

Date: 2026-06-12

## Purpose

This plan turns the prior speedup research into an engine-only implementation
roadmap. It covers Rust code in the compiler, runtime, document/layout,
renderer, host/window, bridge/effects, driver, report, and verifier layers.

This file deliberately does not propose Boon syntax changes, NovyWave Boon
source rewrites, or user-facing API redesign. It says how the Rust engine
should become correct, reliable, measurable, and fast enough to support that
source shape later.

## Boundaries

In scope:

- Rust compiler/runtime internals: parser, IR, typecheck, runtime dispatch,
  typed slots, row/source identity, dirty sets, list storage, and scheduling.
- Rust document/layout/rendering internals: document patches, layout demands,
  materialization, hit testing, scroll state, display chunks, text/assets, GPU
  uploads, app-window readbacks, and frame pacing.
- Rust bridge/effects internals: bridge SDK, schema registry, canonical
  encodings, request/completion scheduling, replay, cancellation, capabilities,
  and page/blob payload handling.
- Rust verification internals: BoonDriver, scenario manifest integrity,
  report freshness, anti-cheating checks, native GPU gates, performance
  measurement, and negative tests.
- Carefully measured internal dependencies such as symbol interning, dense IDs,
  generational storage, inline vectors, bytemuck/zerocopy upload paths,
  tracing, and histograms.

Out of scope:

- Required Boon syntax changes.
- NovyWave Boon source rewrites.
- Example-specific runtime, renderer, verifier, or driver branches.
- Human-observation, browser, Xvfb, desktop screenshot, COSMIC toplevel, or
  global OS-input evidence for native GPU readiness.
- Broad dependency swaps, global allocator swaps, parser replacement,
  renderer replacement, or bridge/Wellen integration before the generic engine
  path is measurable and trustworthy.

## Source Context

Read these before changing code:

- `docs/plans/speedup/01-inspiration.md`
- `docs/plans/speedup/02-novel-research-ideas.md`
- `docs/plans/speedup/03-rust-speed-libraries.md`
- `docs/plans/speedup/05-rust-wgpu-performance-measurement.md`
- `docs/plans/speedup/06-human-like-scenario-testing.md`
- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/architecture/BOON_RUST_BRIDGE.md`
- `docs/architecture/BOON_DRIVER.md`

Relevant current crates:

- `crates/boon_parser`
- `crates/boon_ir`
- `crates/boon_typecheck`
- `crates/boon_runtime`
- `crates/boon_document_model`
- `crates/boon_document`
- `crates/boon_host`
- `crates/boon_driver`
- `crates/boon_native_gpu`
- `crates/boon_native_app_window`
- `crates/boon_native_playground`
- `crates/boon_report_schema`
- `crates/xtask`

There is no separate `boon_layout` crate yet. Layout and materialization work
currently sits across `boon_document` and `boon_native_playground`.

## Target Engine Path

The desired steady interaction path is:

```text
HostInputEvent or SourceIntent
-> public source batch
-> precompiled runtime action plan
-> row/source generation checks
-> keyed dirty set
-> typed runtime/list patches
-> document patch apply report
-> layout/materialization demand or patch
-> retained display chunks
-> bounded renderer upload plan
-> queue submit / present
-> optional proof-mode readback
```

Ordinary hover, cursor movement, scroll, drag, resize, pan, zoom, and small
text edits must not do full parse, full IR lowering, full runtime summary
serialization, full document lowering, full layout rebuild, full display-list
rebuild, full renderer cache rebuild, proof readback, PNG writes, or dev IPC
waits.

## Current Substrate And Gaps

Existing substrate:

- `boon_ir` already has `SourcePayloadSchema`.
- `boon_host` already has `SourceIntent`.
- `boon_runtime` already has source IDs, source route plans, list rows, dense
  slots, dirty sets, and tick sequencing.
- `boon_document_model` already has `MaterializedRange`.
- `boon_document` already has document patches and layout demands.
- `boon_native_gpu` already has a WGPU renderer, text/asset paths, readback
  artifacts, and renderer metrics.
- `boon_native_app_window` already has visible-surface readback and app-window
  frame loops.
- `boon_driver`, `boon_report_schema`, and `xtask` already provide partial
  report validation and native GPU/BoonDriver gates.

Critical gaps:

- `SourceStore::unbind_row` can remove a row binding before validating
  list/key/generation.
- Generic runtime paths still contain TodoMVC-shaped behavior and event-name
  recognizers.
- Parser/IR still rely on ordered expression special cases, raw source slices,
  repeated passes, and source path normalization.
- Typecheck has useful fallback counters, but open-object fallback still hides
  route-critical ambiguity.
- `DocumentState::apply_patch` returns `()` and can silently ignore missing
  targets.
- Native GPU scaffold proof paths must not satisfy visible readiness.
- Readback/map waits need deadlines and timeout artifacts.
- Some IPC/backpressure counters are synthetic or hardcoded.
- BoonDriver is still partly a proof wrapper over existing native reports, not
  a complete app-owned scenario engine.
- Bridge/effects remain architecture-level: no complete bridge SDK crate,
  `Boon.toml` project loader, `check-bridge`, bridge executor, completion as
  source, canonical schema hash layer, or golden encoding vectors.
- Existing reports can be stale; freshness must be proven by hashes and current
  binaries, not assumed from file presence.

## Phase 0: Fail Closed Before Speed Work

Goal: make correctness and proof failures visible before optimizing hot paths.

Steps:

1. Fix `SourceStore::unbind_row` so stale key, stale generation, wrong list ID,
   repeated unbind, remove/reinsert, and interleaved row key reuse cannot drop
   live bindings or mutate new rows.
2. Replace `DocumentState::apply_patch -> ()` with a structured patch result:
   `PatchApplyReport`/`PatchApplyError` or equivalent. Missing targets, stale
   targets, orphaned children, invalid parent/child links, and stale hit/style/
   layout references must fail closed.
3. Demote scaffold native GPU proof paths to explicitly labeled diagnostics.
   A `CopyToPresent` proof with no acquired surface texture or no real visible
   render must not pass visible native readiness.
4. Add deadlines to WGPU readback/map waits in `boon_native_gpu` and
   `boon_native_app_window`. Timeouts must produce artifacts with backend,
   adapter, frame ID, surface, requested rect, pending submission, and report
   context.
5. Add or wire `verify-scenario-manifest-integrity`. It should reject duplicate
   scenario step IDs, missing manifest step refs, duplicate manifest refs
   unless explicitly phased, raw-coordinate selectors, text-only ambiguous
   selectors, stale scenario hashes, unlinked proof artifacts, and tier drift.
6. Fix or classify known scenario inventory problems before relying on those
   scenarios as proof: NovyWave duplicate `select-primary-file`, Cells manifest
   scroll labels that are not `.scn` step IDs, and TodoMVC
   `reject-empty-todo` manifest/story drift.
7. Add the minimal BoonDriver/report freshness skeleton needed by early gates:
   scenario IDs, source/manifest/budget hashes, binary hash, worktree
   fingerprint, action provenance, source intent provenance, and readback
   artifact hashes. Full BoonDriver automation remains Phase 9 work.
8. Audit fixed sleeps, hard process exits, viewport/readback clamps, and
   forced-success probe paths. They must be removed, bounded, or explicitly
   labeled as diagnostic before speed/readiness claims depend on them.

Acceptance:

- Stale row-source events cannot mutate removed or reused rows.
- Document patch failures are structured failures, not successful-looking
  frames.
- Native visible readiness rejects scaffold proof and missing acquired surface
  texture.
- Readback failures terminate with diagnostic artifacts, not hung verifier
  processes.
- Scenario reports are not accepted while manifest/scenario integrity is known
  broken.
- Freshness and provenance fields exist early enough that later speed reports
  cannot pass by reusing stale artifacts.

## Phase 1: Measurement And Provenance Foundation

Goal: make every later optimization explainable in release mode.

Steps:

1. Define a common `FrameFlowId`/`InteractionFlowId` that links host input,
   source intent, runtime turn, document patch, layout/materialization, scene
   build, GPU upload, encode, submit, present, and optional proof readback.
2. Add release-mode stage histograms for:
   input queue, source intent resolution, runtime dispatch, row/list lookup,
   dirty propagation, document patch apply, layout/materialization, scene build,
   text shaping, asset decode/raster/upload, GPU upload, command encode,
   submit, present, IPC, and proof readback.
3. Add counters for:
   allocation count/bytes, rows scanned/touched, route actions visited, dirty
   entries, dirty fanout, cache hit/miss/eviction, lock wait, draw calls, queue
   writes, upload bytes, pipeline switches, text shape hits/misses, glyph atlas
   uploads, asset cache status, IPC queue depth, dropped/coalesced messages,
   blocked sends, and dev lag.
4. Keep interaction, proof, and diagnostic modes separate. Interaction budgets
   must exclude proof readback, PNG writes, full report serialization, verbose
   tracing, and profiler overhead unless the report explicitly measures that
   overhead.
5. Record binary hash, worktree fingerprint, budget hash, source hash,
   scenario hash, fixture hash, backend, adapter, surface format, present mode,
   display refresh, warmup policy, sample count, and profiler/tool overhead
   state in every speed report.
6. Add cheap counters before adding heavy profilers. Use `tracing`, Tracy,
   Criterion/Divan/IAI, heaptrack/DHAT, RenderDoc, timestamp queries, or
   external profilers only for targeted questions and keep them out of normal
   acceptance hot paths.

Acceptance:

- NovyWave, Cells, TodoMVC, and dev editor reports have non-empty internal
  stage timings, not only outer pass/fail shells.
- Performance reports include p50/p95/p99/max and sample counts for key stages.
- Hot-path PNG writes, report writes, proof readbacks, and dev IPC waits are
  explicitly zero in interaction-mode budget runs.
- A/B speed claims include old/new measurements from the same workload,
  binary/worktree identity, and profiler overhead status.

## Phase 2: Parser, IR, And Typecheck Substrate

Goal: remove repeated heuristic compiler work from runtime readiness paths
without changing Boon syntax.

Steps:

1. Build a single parser/IR semantic index inside the current crates before
   considering Tree-sitter, Rowan, Salsa, or a parser replacement.
2. Preserve spans and semantic nodes for records, tags, tagged objects, source
   payload field access, row scopes, document bindings, list maps, and render
   contracts.
3. Replace raw `SOURCE` slice fallback and `events` path normalization with
   AST/IR-backed source inventory and source payload schema facts.
4. Move parser policy checks out of syntax parsing where possible. Parser
   correctness should not depend on example policy or renderer policy.
5. Turn typecheck fallback into readiness evidence. Route-critical payloads,
   row scopes, render contracts, selectors, and bridge/page descriptors must
   fail readiness when inferred type shape is unknown or open.
6. Add compiler report fields for dynamic fallback count, source payload schema
   coverage, route-critical unknowns, row-scope ambiguity, selector/index
   ambiguity, render slot fallback, and semantic-index reuse.

Acceptance:

- No user-facing syntax change is required.
- Parser/IR/typecheck reports can explain why a route, selector, or render slot
  is not ready for incremental runtime execution.
- Route-critical examples report typed source payload schemas and zero
  readiness-blocking dynamic fallbacks.
- Full parse/lower/typecheck remains source-edit or proof behavior, not normal
  interaction behavior.

## Phase 3: Runtime Source Routing And Scheduling

Goal: compile source events into generic action plans and remove example-shaped
runtime behavior.

Steps:

1. Remove or fence TodoMVC-shaped runtime behavior from readiness paths:
   Enter-key draft-title commits, field-name mappings such as
   `edit_text`/`edited_title` to `title`, and similar app-shaped recognizers.
2. Replace event-time source classification with precompiled action plans:
   `SourceId + inferred payload schema + row binding identity + generation ->
   borrowed action op stream`.
3. Avoid per-event cloning of action vectors. Use borrowed slices, compact
   action op arrays, or tiny inline vectors only after measurement proves the
   path.
4. Define public source batch execution as the runtime boundary used by host
   and BoonDriver layers. Private runtime mutation APIs must not be accepted as
   UI proof.
5. Make deterministic scheduling explicit. Scenario and driver paths must use
   monotonic source/event sequence IDs, stable replay, and clear rejection of
   equal-sequence `LATEST` conflicts.
6. Report route ID, action op count, rows scanned/touched, dirty keys, dirty
   fanout, recompute candidates, allocation counts, and fallback/deopt reasons
   per source event or per sampled event.

Acceptance:

- Generic runtime readiness paths do not branch on example names, source file
  names, TodoMVC field names, Cells field names, or NovyWave signal labels.
- Negative tests with TodoMVC-like field names outside TodoMVC do not trigger
  TodoMVC behavior.
- Source events route through source IDs, payload schemas, row bindings, and
  generations.
- Runtime-only evidence remains semantic support, not interactive UI proof.

## Phase 4: Runtime Storage, Dirty Sets, And List Indexes

Goal: make runtime data access bounded and measurable before promising large
example speed.

Steps:

1. Add collision detection and diagnostics for field/list/source slot IDs.
   Hash-derived IDs such as masked FNV field IDs must not silently collide.
2. Intern field names, style attrs, source labels, module paths, operator names,
   tags, and document attrs into dense symbols at parse/lower boundaries. Keep
   readable strings at diagnostics/report boundaries.
3. Add scan counters before replacing scans:
   list rows scanned, row occurrences scanned, order slots refreshed, summary
   fields scanned, dirty entries deduplicated, and route candidates visited.
4. Add row/generation indexes for list rows, row source bindings, visible row
   occurrences, selected IDs, and address/signal/page lookups.
5. Replace string-heavy dirty sets with dense ID sets where IDs are dense.
   Compare `fixedbitset`, existing `bitvec`, `SmallVec`/tiny arrays, or
   `roaring` only after counters show the data shape.
6. Separate semantic state lifetimes from scratch lifetimes. Use arenas or
   bump allocation only for parse/lower/layout/render scratch phases with clear
   reset points. Deleted rows/nodes need generation checks, not broad bump
   lifetime shortcuts.
7. Make runtime caches bounded and observable. Report hit/miss/eviction,
   clear-all, lock wait, stale generation rejection, and cache memory estimates
   before changing policy.

Acceptance:

- Stale keys and hash collisions are detected or impossible by construction.
- Large-list paths report rows scanned/touched and visible occurrence counts.
- Dirty propagation reports fanout histograms and top recompute causes.
- Cache clear-all cliffs and lock waits are visible before cache replacement.

## Phase 5: Document, Layout, Materialization, And Passive Scroll

Goal: make document/layout incremental and portable before renderer-specific
optimizations.

Steps:

1. Keep `DocumentFrame`, `DocumentPatch`, `LayoutDemand`, `LayoutFrame`, hit
   regions, scroll regions, and materialized ranges renderer-neutral.
2. Add stable document node IDs, fragment IDs, scroll root IDs, hit region IDs,
   style/material IDs, and invalidation reasons.
3. Add `PatchApplyReport` and document invariants:
   every child has a parent, every parent contains its children, hit/style/
   layout refs target existing nodes, subtree removal is explicit, missing
   targets fail, and stale target revisions are rejected.
4. Implement layout/materialization demand as a closed loop:
   layout computes visible range and overscan, runtime/document returns stable
   keyed materialized rows/pages, and reports distinguish logical item count
   from materialized item count.
5. Make passive scroll update scroll/layout/display state without runtime
   dispatch when no semantic source binding is hit.
6. Move hard-coded playground geometry/scroll transforms into generic document
   and layout contracts. The dev editor fast path is useful evidence, not the
   final architecture.
7. Add invalidation classes:
   paint-only, layout-only, hit-region, source-binding, list-structure,
   conditional-structure, scroll-offset-only, materialization-only, and full
   document.

Acceptance:

- Passive scroll reports `runtime_dispatch_count_for_passive_scroll=0`,
  `graph_rebuild_count=0`, and bounded materialized ranges.
- Cells reports the logical 2600-cell grid separately from visible plus
  overscan materialization.
- Missing document patch targets fail with structured errors.
- Layout demand fulfillment is generic and does not mention NovyWave, Cells,
  TodoMVC, or dev editor as special cases.

## Phase 6: Renderer, Text, Assets, And GPU Uploads

Goal: make the renderer retain stable chunks and bound CPU/GPU work.

Steps:

1. Keep renderer proof tied to real rendered frames. Scaffold `CopyToPresent`,
   missing acquired surface texture, or zero-draw proof cannot satisfy visible
   readiness.
2. Introduce retained primitive/display chunks with identity shaped like:
   document node or fragment ID, primitive kind, style/material ID, property
   tree state, clip/scroll state, content hash, and revision.
3. Split property state from primitive data:
   scroll, transform, clip, effect, opacity, and dirty rects should not require
   rebuilding unchanged primitive content.
4. Add stable render bins for quads, borders, text, images/assets, waveform
   segments, overlays, clips, and debug artifacts.
5. Replace fresh per-frame byte vector conversion and many small uploads with
   POD instance structs, explicit host/WGSL layout tests, `bytemuck` or
   `zerocopy` experiments, upload plans, staging/ring buffers, and upload
   counters.
6. Make text measurement and rendering share deterministic layout contracts.
   Text geometry, line breaks, caret positions, and hit regions must be decided
   before rendering. Renderer text cache pressure is diagnostics, not a reason
   to mutate layout.
7. Add text cache metrics:
   shaped-run hits/misses/evictions, glyph atlas uploads/evictions, missing
   glyphs, visible text runs, shaped text runs, and text cache memory.
8. Move asset decode/raster/upload out of interaction hot paths where possible.
   Use digest identities, cache status, async decode/raster/upload, byte caps,
   and proof artifacts for failures.
9. Keep proof readbacks in proof mode. Normal interaction must not wait for
   readback, PNG encoding, artifact hashing, or verifier serialization.
10. Preserve the generated WESL/WGSL/bindgen shader pipeline. Shader freshness
    checks must fail on stale generated outputs, manual generated-WGSL loading,
    or duplicated bind group, pipeline layout, entry point, or shader constant
    definitions.

Acceptance:

- Draw calls, queue writes, upload bytes, pipeline switches, text cache stats,
  and asset cache stats are recorded per interaction workload.
- GPU upload work is bounded by changed primitives/pages, not whole frames.
- Renderer optimizations are driven by document/layout/display identities, not
  example names or source file names.
- Readback deadlines produce failure artifacts instead of hangs.
- Shader freshness/manual-WGSL checks remain part of native GPU readiness.

## Phase 7: Host Event Loop, IPC, And Dev/Preview Separation

Goal: keep preview interaction independent from dev tooling and report work.

Steps:

1. Route native input through host events, document hit/scroll/focus
   resolution, source intents, and public source batches. Verifiers must not
   call private runtime mutation APIs when claiming UI interaction.
2. Coalesce high-frequency hover, wheel, drag, resize, and dev telemetry where
   semantics permit latest-wins behavior.
3. Add bounded IPC lanes for preview commands, source/project replacement,
   debug telemetry, dev queries, readback/proof responses, and heartbeat/status.
4. Replace synthetic/hardcoded blocked counters with live counters:
   queue depth, dropped/coalesced messages, blocked sends, blocked duration,
   bytes, heartbeat gaps, dev lag, preview frame gaps, and stale/discarded
   revisions.
5. Ensure preview never blocks on dev window IPC, debug summaries, report
   writes, PNG writes, or proof-only readbacks.
6. Preserve process/window evidence from `NATIVE_GPU_PIPELINE.md`, but keep
   verification evidence app-owned: reports, process IDs, host events, frame
   IDs, and WGPU readbacks.

Acceptance:

- Preview interaction p95/max is unchanged or improved under dev window stress.
- `preview_blocked_on_ipc_count` comes from live process counters.
- Latest-wins coalescing reports input count, coalesced count, dropped count,
  and semantic reason.
- No desktop screenshots or compositor scraping are used as native proof.

## Phase 8: Bridge And Effects Kernel

Goal: implement the Rust bridge/effects substrate under the existing bridge
architecture without exposing Rust handles to Boon values.

Steps:

1. Create the bridge SDK/registry surface, or an equivalent internal bridge
   module boundary, with module names, export kinds, schema versions, schema
   hashes, capabilities, ABI version, and provider metadata.
2. Add canonical schema encoding and golden vectors for records, tagged
   variants, lists, refs, pages, blobs, diagnostics, and effect completions.
3. Add internal/dev-fixture loading for bridge-enabled packages and a
   `check-bridge`-style verifier that checks SDK version, schema hashes, crate
   package ID, enabled features, target triple, and lockfile digest. Public
   `Boon.toml` workflow design remains owned by the bridge/API plan; this
   engine plan only requires the Rust validation kernel and fixture harness.
4. Implement request/completion scheduling as source-compatible data:
   request ID, generation/epoch, schema hash, input digest, request key,
   status, diagnostic, completion payload, cancellation, dedup, stale rejection,
   and replay.
5. Implement capability/grant denial, payload caps, no-handle checks, and
   host-resource leak rejection before real Wellen integration.
6. Add page/blob storage with byte limits and pure descriptors. Rust may own
   parser/cache/file/mmap handles internally, but Boon-visible values must be
   descriptor refs, pages, blobs, diagnostics, and statuses only.
7. Add bridge scenario proof using deterministic fixture providers before
   using real `wellen`.

Acceptance:

- Missing bridge module, changed schema, wrong effect kind, stale completion,
  duplicate completion, cancellation, grant denial, payload cap, replay, and
  no-Rust-handle cases are tested.
- Bridge completions can be replayed deterministically without re-running host
  effects.
- Real Wellen integration is deferred until the generic bridge kernel and page
  contracts pass.

## Phase 9: BoonDriver, Reports, And Anti-Cheating

Goal: turn verification into an engine-owned automation protocol, not wrappers
around stale reports.

Steps:

1. Complete BoonDriver as an app-owned scenario engine:
   scenario parse, selector resolution, waits, action dispatch, host input,
   hit/focus/scroll routing, source intent evidence, runtime dispatch evidence,
   document/render patch evidence, assertions, and report generation.
2. Keep evidence tiers explicit:
   runtime, boon-driver, real-window, and human. BoonDriver must not claim
   real-window or human observation.
3. Add scenario/manifest integrity as a first-class `xtask` gate and make it a
   prerequisite for scenario acceptance.
4. Harden report freshness with hashes as authority:
   source, source files, scenario, manifest, budget, fixtures, artifacts,
   binary, worktree, and bridge schemas. Mtime is diagnostic only.
5. Add negative/fabricated-report cases for every honesty boolean used in
   readiness:
   private runtime dispatch, preview received scenario data, source-event-only
   IPC shortcut, fake real OS input, fake human observation, full waveform
   payload entering Boon, scaffold rendering, copied pixel hashes, stale
   binaries, reduced fixtures, and model-only timing.
6. Add hidden/metamorphic fixture contracts later:
   generator seed, source reformat, source path move, fixture path/ID changes,
   label renames, declaration order changes, viewport/theme changes, expected
   semantic invariants, visual crop invariants, and allowlisted report-only
   strings.

Acceptance:

- `verify-boon-driver-e2e` proves BoonDriver action flow, not just report
  wrapping.
- Report schema validation rejects stale, fabricated, shortcut, scaffold, and
  tier-inflated evidence.
- Current native/driver reports are treated as stale unless hashes and binaries
  prove freshness.
- Scenario integrity is allowed to fail initially while known drift is fixed or
  explicitly classified, but downstream scenario acceptance must depend on it.

## Phase 10: Low-Level Rust Performance Experiments

Goal: adopt crates and low-level patterns only after local measurements show a
specific bottleneck.

Rules:

1. Add one direct dependency per experiment and keep the write scope narrow.
2. Do not swap the global allocator until allocation design, string identity,
   caches, and upload staging have been fixed and measured.
3. Do not replace parser, runtime, renderer, or bridge wholesale with
   Tree-sitter, Salsa, Vello, differential dataflow, Arrow, or Polars.
4. Do not expose parser/cache/mmap/GPU/Rust handles as Boon-visible values.
5. Keep full recompute or generic interpreter paths as oracles when adding
   optimized paths.

Likely early experiments:

- `bytemuck` or `zerocopy` for GPU upload/page structs after host/WGSL layout
  tests exist.
- `smallvec` or `arrayvec` for tiny patch lists, dirty causes, route actions,
  dependency edges, and invalidation classes after allocation counters prove
  benefit.
- `lasso` or `string-interner` for field/style/source/module/tag symbols after
  symbol lifetime and diagnostics boundaries are clear.
- `slotmap` or `generational-arena` for hidden row/node/source-binding handles
  after stale-key tests define semantics.
- `fixedbitset`, `roaring`, or existing `bitvec` for dirty sets depending on
  measured density and fanout.
- `tracing`, `hdrhistogram`, Criterion/Divan/IAI, or profiler adapters for
  measurement, gated by overhead checks.

Acceptance:

- Every dependency-backed speedup has an A/B report on a representative
  workload with old/new timings, allocation deltas, memory deltas, cache
  counters, and fallback/deopt counters.
- Reports remain deterministic and stable across runs.
- The optimized path is generic across examples.

## MVP Order

The first credible MVP is:

1. Scenario/manifest integrity gate that catches the known drift.
2. Row/source unbind correctness and stale row-source tests.
3. Document patch result reporting and document invariants.
4. Scaffold proof demotion and readback deadlines.
5. Release-mode stage timing and counter foundation.
6. Runtime route action plans, source batches, row identity/generation, and
   removal/fencing of example-shaped runtime behavior.
7. Row/list scan counters and first row lookup indexes.
8. Generic document materialization and passive scroll for Cells/dev editor.
9. Retained renderer chunks and bounded upload counters.
10. BoonDriver core that drives actions instead of wrapping reports.
11. Bridge schema/effect skeleton with canonical encoding and fake provider
    tests, not real Wellen yet.

Cut from MVP:

- Boon source/API changes.
- NovyWave source rewrite.
- Real Wellen integration.
- Dynamic bridge loading.
- Human or real-window tier upgrades.
- Global allocator swaps.
- Full hidden/metamorphic fixture matrix.
- Browser/WASM driver adapters.
- Parser or renderer replacement.

## Acceptance Matrix

Compiler/runtime:

- no readiness-blocking dynamic fallbacks on route-critical examples;
- no example-shaped runtime behavior in readiness paths;
- source events report route ID, action op count, rows scanned/touched,
  dirty keys, recompute candidates, and allocations;
- deterministic replay uses monotonic source/event sequence IDs.

Document/layout:

- patch failures are structured and fail closed;
- passive scroll is runtime-free where semantic source binding is not hit;
- materialized item count is bounded to visible plus overscan;
- layout demand reports distinguish logical size from rendered range.

Renderer/native GPU:

- scaffold proof cannot pass visible readiness;
- readbacks have deadlines and timeout artifacts;
- draw calls, queue writes, upload bytes, pipeline switches, text cache, asset
  cache, and dirty chunk reuse are reported;
- proof overhead is excluded from interaction-mode budgets.

Bridge/effects:

- schema hashes and golden vectors exist;
- request/completion/replay/cancel/stale/duplicate/grant/payload cap cases are
  tested;
- no Rust handles or process-local resource IDs enter Boon-visible values.

BoonDriver/reports:

- scenario integrity is enforced before acceptance;
- BoonDriver proves action dispatch through host/document/source/runtime/render
  evidence;
- report freshness uses hashes and current binaries;
- negative gates reject fake, stale, shortcut, scaffold, and tier-inflated
  evidence.

## Do Not Overclaim

Do not claim:

- NovyWave is fast because an outer report passed while internal stage timings
  are empty.
- Runtime-only evidence proves UI interaction.
- A stale target report is current proof.
- A scaffold render path proves native GPU readiness.
- Synthetic IPC stress proves live preview/dev backpressure behavior.
- Bridge fixture descriptors prove real Wellen file loading.
- Boon source workarounds are acceptable final fixes for engine limitations.

The intended result is a generic Rust engine path that is correct first,
measurable second, and fast third. Once that path exists, NovyWave can become a
large honest workload instead of a collection of special cases.
