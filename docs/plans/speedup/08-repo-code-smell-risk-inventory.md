# Repo Code Smell And Risk Inventory

Date: 2026-06-12

## Purpose And Caveat

This file collects performance, reliability, and verification risks found by
reading code in this repository. It is intentionally broader than the
NovyWave-specific notes. It covers parser, IR, typecheck, runtime, document,
native GPU, native playground, verifier, and report-generation code.

This is not a measured performance report. Treat every item as one of:

- **confirmed by reading:** the code shape exists at the cited location;
- **risk or hypothesis:** the code shape is likely to become slow, flaky, or
  unreliable, but still needs measurement or a focused correctness test;
- **remediation direction:** a likely way to remove the risk after confirming
  its impact and intended behavior.

The goal is to create a high-signal backlog for future audits. Do not use this
file as permission to weaken contracts, skip native GPU evidence, or replace
generic engine fixes with example-specific shortcuts.

## Repo Baseline

The repository has several large single-file implementation surfaces:

- `crates/boon_native_playground/src/main.rs`: about 53,067 lines.
- `crates/xtask/src/main.rs`: about 26,942 lines.
- `crates/boon_runtime/src/lib.rs`: about 24,807 lines.
- `crates/boon_ir/src/lib.rs`: about 10,083 lines.
- `crates/boon_typecheck/src/lib.rs`: about 8,875 lines.
- `crates/boon_native_gpu/src/lib.rs`: about 6,227 lines.
- `crates/boon_parser/src/lib.rs`: about 4,726 lines.

Large files are not automatically bugs, but here they hide multiple ownership
domains in the same place:

- protocol, launcher, preview IPC, dev shell, renderer handoff, verifier
  helpers, and scenario tests in the native playground;
- all native GPU verifier gates and report policy in one `xtask` file;
- compiler/runtime storage, source routing, evaluation, scenarios, reports, and
  tests in one runtime file;
- text, style, editor details, asset rasterization, primitive expansion, GPU
  upload, and proof rendering in one native GPU file.

This makes it easy for a performance shortcut, proof-mode workaround, or
example-shaped behavior to become a hidden architectural dependency.

## Critical Reliability Risks

### Row Source Unbind Can Drop A Live Slot Before Validation

Confirmed by reading:

- `crates/boon_runtime/src/lib.rs:5293` takes a row slot with
  `Option::take()` before checking that the slot matches `list_id`, `key`, and
  `generation`.

Risk or hypothesis:

- A stale or mismatched unbind can remove a live row-source binding slot and
  return early without restoring it.
- This is correctness-sensitive because row source bindings are the route from
  user-visible list rows back to generic source events.

Remediation direction:

- Check the slot by immutable reference before taking it.
- Add invariant tests for stale key, stale generation, wrong list id, repeated
  unbind, remove-then-reinsert, and interleaved row key reuse.
- Add debug assertions that active binding counts, row slots, source slots, and
  row generations agree after every bind/unbind mutation.

### Generic Runtime Still Contains TodoMVC-Shaped Behavior

Confirmed by reading:

- `crates/boon_runtime/src/lib.rs:2424` and `:2573` route Enter-key behavior
  through draft-title commit paths.
- `crates/boon_runtime/src/lib.rs:11398` hardcodes draft fields such as
  `edit_text` and `edited_title` into `title`.

Risk or hypothesis:

- The generic runtime can accidentally preserve example-specific behavior that
  should be expressed in Boon source or typed source routes.
- Future examples may pass because the runtime recognizes a TodoMVC-like shape,
  not because the engine implements the general semantics correctly.

Remediation direction:

- Move draft/commit behavior into generic typed routes or Boon source.
- Add negative tests where similar field names appear in non-TodoMVC examples.
- Keep an `xtask` audit that fails on remaining example-shaped runtime policy.

### Document Patching Silently Ignores Missing Targets

Confirmed by reading:

- `crates/boon_document/src/lib.rs:136` has `DocumentState::apply_patch`
  return `()`.
- Missing-node targets are ignored.
- `RemoveNode` removes only the single node id, not a verified subtree or
  parent/child relation.

Risk or hypothesis:

- Render/document state can drift while the caller still believes a patch
  applied.
- Removing one node without tree cleanup can leave orphaned children or stale
  hit/style/layout references.

Remediation direction:

- Make patch application return `Result<PatchApplyReport, PatchApplyError>`.
- Distinguish `RemoveNode` from `RemoveSubtree`.
- Add document invariants after patch application: every child has a parent,
  every parent contains the child, hit/style/layout references target existing
  nodes, and missing targets are reported.

### Native GPU Proof Can Report Scaffold Success

Confirmed by reading:

- `crates/boon_native_gpu/src/lib.rs:38` exposes `PresentSurface` as metadata
  rather than real acquire/present behavior.
- `crates/boon_native_gpu/src/lib.rs:254` returns proof values such as
  `acquired_surface_texture: false`, `not-presented-scaffold`, zero draw/upload
  metrics, and `scaffold-no-surface`.
- `crates/boon_native_playground/src/main.rs:4263` can wrap this with
  `status: pass` while `visible_surface_rendered` is false.

Risk or hypothesis:

- A report can look green while proving a scaffold path rather than real
  visible rendering.

Remediation direction:

- Rename scaffold proofs to dry-run diagnostics and prevent them from
  satisfying native GPU readiness gates.
- Require real acquire/render/copy/present proof or an explicitly labeled
  app-owned offscreen proof tier.
- Make reports fail if `status: pass` conflicts with
  `visible_surface_rendered: false` for visible-surface gates.

### Synthetic IPC Backpressure Evidence

Confirmed by reading:

- `crates/boon_native_playground/src/main.rs:33623` builds
  `bounded_ipc_stress_response` from a local in-process queue.
- It reports hard-coded values such as `preview_blocked_on_ipc_count: 0`.
- `crates/xtask/src/main.rs:2757` and `:3131` consume those values as native
  GPU backpressure/observability evidence.

Risk or hypothesis:

- The verifier can pass without proving real preview/dev IPC pressure,
  heartbeat gaps, blocked sends, frame gaps, or queue saturation.

Remediation direction:

- Replace synthetic counters with real counters from the preview/dev bridge:
  bounded queue depth, dropped messages, blocked writes, request latency,
  heartbeat gaps, frame gaps, and response size histograms.
- Keep any synthetic stress helper, but label it as a unit-level model, not
  live IPC evidence.

### Hard-Coded "Not Blocked" Counters

Confirmed by reading:

- `crates/boon_native_playground/src/main.rs:31697`, `:31745`, `:32386`, and
  `:33669` report several blocked/backpressure counters as fixed zeros.

Risk or hypothesis:

- A readiness report can claim no blocking without instrumentation.

Remediation direction:

- Replace fixed zeros with measured counters or omit the fields.
- Add schema rules: a field named like `*_count` that affects readiness must
  include provenance, collection window, and source subsystem.

### Readback Paths Can Wait Indefinitely

Confirmed by reading:

- `crates/boon_native_app_window/src/lib.rs:2024` waits on WGPU readback poll.
- `crates/boon_native_gpu/src/lib.rs:1018` uses map/poll readback behavior for
  app-owned pixels.

Risk or hypothesis:

- A GPU/driver/device failure can hang a verifier or interactive proof path
  instead of producing a timeout artifact.

Remediation direction:

- Put explicit deadlines around readback/map waits.
- Return timeout artifacts with device, queue, frame, requested rectangle,
  pending submission, and report context.
- Keep proof readbacks out of normal interaction paths.

### Hard Process Termination In App Paths

Confirmed by reading:

- `crates/boon_native_app_window/src/lib.rs:902` exits from render-thread
  error handling.
- `crates/boon_native_playground/src/main.rs:31315` exits for one probe path.

Risk or hypothesis:

- Abrupt exit can skip report finalization, worker shutdown, buffered logs,
  temp cleanup, and useful diagnostics.

Remediation direction:

- Route fatal errors through supervisor-visible shutdown results.
- Emit a final structured failure report before exit where possible.
- Reserve `process::exit` for top-level command entrypoints after reports have
  been written.

## Likely Performance Risks

### Parser And IR Are Heuristic And Multi-Pass

Confirmed by reading:

- `crates/boon_parser/src/lib.rs:822` begins line/token handling after
  tokenization.
- `crates/boon_parser/src/lib.rs:1240` expression parsing is an ordered chain
  of special cases.
- `crates/boon_parser/src/lib.rs:1625` scans infix operators without a normal
  precedence table.
- `crates/boon_ir/src/lib.rs:581` and `:777` perform many independent passes
  and recompute document binding facts.

Risk or hypothesis:

- Parser correctness will become fragile as syntax grows.
- Mixed arithmetic/comparison expressions can misparse when users expect normal
  precedence.
- Lowering can spend avoidable time rediscovering field, source, row-scope, and
  document-binding facts.

Remediation direction:

- Move toward a typed grammar/AST with explicit precedence.
- Build one semantic index for parser/IR facts and share it across lowering
  passes.
- Keep parser policy separate from repo/example policy.

### Parser Validation Mixes Language Rules With Example Policy

Confirmed by reading:

- `crates/boon_parser/src/lib.rs:1957` and `:2146` enforce policy that every
  source includes `SOURCE`, `HOLD`, and `LATEST`.

Risk or hypothesis:

- Valid future source shapes may be rejected because the parser carries
  example-readiness policy.
- Parser tests become coupled to current documentation goals instead of syntax
  correctness.

Remediation direction:

- Move project/example readiness checks into `xtask` or a separate linter.
- Let the parser report syntax/structure errors only.

### Source Binding Lowering Uses Text And Naming Heuristics

Confirmed by reading:

- `crates/boon_ir/src/lib.rs:1530` falls back to raw source slicing for
  `SOURCE { ... }`.
- `crates/boon_ir/src/lib.rs:2672` normalizes away `events`.
- `crates/boon_ir/src/lib.rs:2685` maps `key_down` to `submit`.
- `crates/boon_parser/src/lib.rs:3327` drops path segments named `events`.
- `crates/boon_parser/src/lib.rs:3347` infers row scope by naming heuristics
  such as singularizing `todos` to `todo`.

Risk or hypothesis:

- Formatting, comments, nested source forms, or legitimate paths containing
  `events` can break source binding semantics.
- Keyboard semantics can be collapsed too early.

Remediation direction:

- Lower source bindings from AST nodes and typed event definitions.
- Preserve event path segments unless a typed route explicitly aliases them.
- Replace naming heuristics with declared row scopes or typecheck-provided
  binding metadata.

### Typecheck Dynamic Fallbacks Can Hide Contract Gaps

Confirmed by reading:

- `crates/boon_typecheck/src/lib.rs:641` reports dynamic fallback counts.
- `crates/boon_typecheck/src/lib.rs:766`, `:1405`, and `:2186` still use broad
  `open_object_type()` fallbacks in function args, user-function returns,
  list-map results, and boolean checks.
- `crates/boon_typecheck/src/lib.rs:656` only checks `document_root` for
  document coverage.

Risk or hypothesis:

- `Unknown` and open-object fallbacks can make typecheck appear healthier than
  the runtime contract really is.
- Scene-only programs can appear fully covered because there is no
  `document_root`.

Remediation direction:

- Treat dynamic fallback counts as warnings or phase failures for readiness
  gates.
- Tighten type coverage for both document and scene roots.
- Add diagnostics that name the expression and fallback reason.

### Runtime Routing Still Relies On Recognizers

Confirmed by reading:

- `crates/boon_runtime/src/lib.rs:5909` classifies source events through
  ordered heuristics.
- `crates/boon_runtime/src/lib.rs:6169` can produce ambiguous or missing list
  targets for top-level events.
- `crates/boon_runtime/src/lib.rs:6288` clones and dispatches a route action
  vector.

Risk or hypothesis:

- One source driving multiple action kinds or multiple lists can be fragile.
- Hot interactions can pay unnecessary cloning and recognizer costs.

Remediation direction:

- Generate source route contracts from typed IR.
- Key routes by typed source id and row binding identity.
- Use compact action-plan storage instead of cloning route action vectors on
  each event.

### Large-List Update Paths Are Linear

Confirmed by reading:

- `crates/boon_runtime/src/lib.rs:4741` refreshes order slots after list
  remove/move operations.
- `crates/boon_runtime/src/lib.rs:6574` loops over rows for non-row-context
  indexed actions.
- `crates/boon_runtime/src/lib.rs:6853` scans visible rows by text/occurrence
  for event routing.

Risk or hypothesis:

- Large lists will spend time in row scans, occurrence matching, and order
  vector maintenance before document/rendering costs are visible.

Remediation direction:

- Add indexed row lookup for key/generation and occurrence targets.
- Add materialized visible-row and text-occurrence indexes for interaction
  routing.
- Record row scan counts and max rows touched per source event.

### Dense Runtime Vectors And FNV Field IDs Have Longevity Risks

Confirmed by reading:

- `crates/boon_runtime/src/lib.rs:5141` and `:5242` index vectors by
  `key as usize` and source id.
- `crates/boon_runtime/src/lib.rs:5498` derives field IDs with an FNV hash and
  a 20-bit mask.

Risk or hypothesis:

- Dense vectors work for current monotonic IDs, but can become expensive or
  sparse in long-lived sessions or external identity systems.
- A 20-bit field-id mask creates collision risk as examples and generated
  fields grow.

Remediation direction:

- Make identity allocation domains explicit.
- Add collision detection and report field-id collisions.
- Consider interned field symbols or typed field tables instead of masked hash
  IDs.

### Global Caches Are Coarse And Sometimes Clear All

Confirmed by reading:

- `crates/boon_runtime/src/lib.rs:805` and `:810` define global runtime plan
  and initialized runtime caches behind `Mutex<BTreeMap<...>>`.
- `crates/boon_runtime/src/lib.rs:1142` clears a cache when it exceeds 16
  entries.
- `crates/boon_native_playground/src/main.rs:3948`, `:3985`, and `:4026`
  define parse/static-analysis caches with similar clear-all behavior.
- `crates/boon_native_playground/src/main.rs:24645` and `:24715` clear
  evaluator caches after 4096 entries.

Risk or hypothesis:

- Coarse mutexes can block hot paths.
- Clear-all eviction can cause latency cliffs.
- Cache behavior is difficult to reason about without hit/miss/eviction
  counters.

Remediation direction:

- Add named cache metrics: hits, misses, evictions, clear-all count, max size,
  lock wait time, and memory estimate.
- Prefer bounded LRU/generation caches over clear-all eviction.
- Keep proof/report caches separate from interaction caches.

### Renderer Caches Are Whole-Frame Or Whole-Buffer

Confirmed by reading:

- `crates/boon_native_gpu/src/lib.rs:726` requires full `LayoutFrame` equality
  for prepared quad reuse.
- `crates/boon_native_gpu/src/lib.rs:761` hashes full position/color/uv arrays
  for quad batches.
- `crates/boon_native_gpu/src/lib.rs:768` clears the entire quad-buffer cache
  at 64 entries.
- `crates/boon_native_gpu/src/lib.rs:6213` hashes whole frames through JSON
  serialization.
- `crates/boon_native_gpu/src/lib.rs:120` has text measurement cached by full
  text/style without a visible bound.

Risk or hypothesis:

- Small editor, row, hover, scroll, or grid changes can miss caches and rebuild
  or upload too much.
- Text cache growth can be unbounded under generated labels or code editing.

Remediation direction:

- Key caches by stable primitive IDs, dirty regions, glyph runs, and texture
  assets.
- Add bounded eviction and hit/miss metrics.
- Hash stable binary frame structures instead of JSON serialization in hot
  paths.

### CPU-Expanded Visual Primitives

Confirmed by reading:

- `crates/boon_native_gpu/src/lib.rs:3893` and `:3945` emit 1x1 rects for
  checkbox/circle primitives.
- `crates/boon_native_gpu/src/lib.rs:3406` and `:3556` build shadow/frosted
  effects as CPU-emitted layered geometry.

Risk or hypothesis:

- Checkbox-heavy, rounded-heavy, shadow-heavy, or material-heavy scenes can
  generate excessive CPU geometry and GPU uploads.

Remediation direction:

- Move common primitives to GPU-friendly geometry or shaders.
- Add primitive expansion counters to reports.
- Add budgets for CPU-emitted quads per frame and upload bytes per frame.

### Synchronous Asset Rasterization And Upload

Confirmed by reading:

- `crates/boon_native_gpu/src/lib.rs:475` rasterizes/uploads SVG data URLs
  during batch preparation.
- `crates/boon_native_gpu/src/lib.rs:571` only accepts non-base64
  `data:image/svg+xml` URLs.
- `crates/boon_native_gpu/src/lib.rs:2922` keys assets by URL plus render size.

Risk or hypothesis:

- Asset-heavy scenes can block the render path.
- Equivalent assets at slightly different sizes may duplicate raster/upload
  work.

Remediation direction:

- Add an asset pipeline with async decode/raster/upload, digest-based identity,
  deduplication, broader data URL support, and bounded eviction.
- Keep asset load failures as visible diagnostics, not panics or silent
  placeholders.

### Fixed Sleeps And Polling Loops

Confirmed by reading:

- `crates/boon_native_app_window/src/lib.rs:16` defines passive input polling
  at 100ms.
- `crates/boon_native_app_window/src/lib.rs:1753` sleeps 5ms in demand-driven
  mode after rendering.
- `crates/boon_native_playground/src/main.rs:33936` polls JSON/report
  readiness.
- `crates/xtask/src/main.rs:23552` uses fixed sleeps in driver harness paths.

Risk or hypothesis:

- Fixed sleeps can add latency, waste CPU, or make tests flaky depending on
  scheduler timing.

Remediation direction:

- Replace sleeps with event waits on loop revisions, readback hashes, driver
  ACKs, socket handshakes, or host-event counters.
- Where polling remains necessary, report poll interval, total wait, wake
  reason, and timeout reason.

## Verification And Report Integrity Risks

### Visible And Proof Rendering Are Separate Paths

Confirmed by reading:

- `crates/boon_native_playground/src/main.rs:4430` uses a persistent visible
  `VisibleLayoutRenderer`.
- `crates/boon_native_playground/src/main.rs:4446` can call
  `render_app_owned_pixels` with a separate fresh renderer.
- `crates/boon_native_gpu/src/lib.rs:943` builds offscreen textures, blocks on
  `device.poll`, writes PNG, and returns `frame_seq: 1`.

Risk or hypothesis:

- App-owned proof can pass while the visible surface differs.
- Proof work can add overhead if it leaks into normal interaction paths.

Remediation direction:

- Prefer render-once proof: render to an app-owned target, then copy/present
  and read back from that same frame.
- If separate proof is unavoidable, label it as offscreen proof and report the
  visible frame hash/source frame it corresponds to.

### Hard Viewport And Readback Clamps

Confirmed by reading:

- `crates/boon_native_gpu/src/lib.rs:721` clamps layout surface encoding to
  `1920x1080`.
- `crates/boon_native_gpu/src/lib.rs:952` clamps app-owned proof rendering to
  `1920x1080`.
- `crates/boon_native_app_window/src/lib.rs:1691` samples preview readback at
  `480x260`.

Risk or hypothesis:

- Large or high-DPI windows can be cropped or logically smaller in reports
  while reports appear to describe the full surface.

Remediation direction:

- Remove clamps from core render/proof paths.
- If a report intentionally samples or crops, name it as `sample_crop`, include
  source surface size, crop rect, scale, and coverage percentage.

### Readiness Can Mean Appearance Rather Than Responsiveness

Confirmed by reading:

- `crates/boon_native_playground/src/main.rs:2024` waits for a socket path
  before starting dev.
- `crates/boon_native_playground/src/main.rs:33962` and
  `crates/xtask/src/main.rs:23239` use JSON polling helpers.

Risk or hypothesis:

- A socket/report file can exist before the endpoint is responsive, schema
  valid, or tied to the expected PID/source hash.

Remediation direction:

- Use role-ready ACKs, socket handshake responses, schema-validated reports,
  PID/cmdline validation, and source/project hash matching.

### Artifact Freshness Uses Mtime

Confirmed by reading:

- `crates/xtask/src/main.rs:24877` uses artifact modified times to judge
  freshness against source and binary mtimes.

Risk or hypothesis:

- Touched or copied artifacts can appear fresh without proving they were
  generated by the current binary/source.

Remediation direction:

- Make artifact proof hash-based and provenance-based: worktree fingerprint,
  binary hash, command argv, source/project hash, artifact hash, schema version,
  and generated timestamp.
- Keep mtime only as a diagnostic hint.

### Dead Or Inconsistent Launcher Branches

Confirmed by reading:

- `crates/xtask/src/main.rs:2205`, `:2647`, and `:2999` contain dead
  `else if false` COSMIC launcher branches.
- `crates/xtask/src/main.rs:7097` still selects launcher behavior through a
  mix of default and environment-dependent paths.

Risk or hypothesis:

- Harness behavior differs by environment in ways that are hard to audit.

Remediation direction:

- Remove dead launcher branches.
- Define one primary launch strategy per gate and report why any fallback was
  chosen.

### Hardcoded Dev Geometry In Verifiers

Confirmed by reading:

- `crates/xtask/src/main.rs:12829` hardcodes a dev editor scroll region.
- `crates/boon_native_playground/src/main.rs:28333` transforms layout frames
  with fixed row/column assumptions such as `/26.0` and `/80.0`.

Risk or hypothesis:

- Verifiers can pass geometry that no longer matches the actual dev layout.
- Render, hit regions, demands, and hashes can desynchronize after layout
  changes.

Remediation direction:

- Derive scroll/input geometry from actual layout reports.
- Make scroll/editor deltas first-class layout/runtime outputs instead of
  downstream `LayoutFrame` mutations.

## Architecture Boundary Smells

### Generic GPU Crate Contains App And Editor Semantics

Confirmed by reading:

- `crates/boon_native_gpu/src/lib.rs:1790` and `:1906` parse editor type hints
  and syntax-related spans.
- `crates/boon_native_gpu/src/lib.rs:2449` handles checkbox/checked behavior.
- `crates/boon_native_gpu/src/lib.rs:4209` includes document-kind default
  fills.

Risk or hypothesis:

- Renderer code becomes hard to cache, test, and reuse because it understands
  app/editor semantics instead of drawing generic display primitives.

Remediation direction:

- Move editor/app semantics to document/display-list generation.
- Keep `boon_native_gpu` focused on primitives: rectangles, paths, text runs,
  glyphs, images, clips, borders, shadows, and surfaces.

### RenderCapabilities Are Not A Real Contract

Confirmed by reading:

- `crates/boon_document/src/lib.rs:49` carries `RenderCapabilities` in layout
  input.
- `crates/boon_document/src/lib.rs:173` does not use them in layout.
- `crates/boon_native_gpu/src/lib.rs:255` claims instancing and clip rect
  support.
- `crates/boon_native_gpu/src/lib.rs:649` and `:849` draw vertex buffers with
  `draw(..., 0..1)` rather than actual instancing.

Risk or hypothesis:

- Reports can claim capability support that layout/rendering do not consume or
  exercise.

Remediation direction:

- Either make layout consume capabilities or remove them from readiness claims.
- Add capability negative tests that fail if unsupported capability claims do
  not change layout/render behavior.

### Text Measurement And Rendering Contracts Diverge

Confirmed by reading:

- `crates/boon_native_gpu/src/lib.rs:158` accepts measured font sizes down to
  `font_size.max(1.0)`.
- `crates/boon_native_gpu/src/lib.rs:1664` clamps shaped rendering sizes to
  `8.0..120.0`.
- `crates/boon_native_gpu/src/lib.rs:1745` and `:2024` create or load fresh
  font systems in editor metric helpers.

Risk or hypothesis:

- Text layout can differ from text rendering for small/large font sizes.
- Fresh font-system creation can add latency or inconsistency.

Remediation direction:

- Define one text contract for font size, line height, fallback fonts, rich
  spans, caret metrics, and selection metrics.
- Share font systems and caches at the app/renderer boundary with explicit
  invalidation.

### Legacy Ply State Still Leaks Labels

Confirmed by reading:

- `crates/boon_ply_playground/src/lib.rs:7728` uses `Box::leak` for render id
  labels.
- The same legacy crate has thread-local focus/hover/button state near
  `crates/boon_ply_playground/src/lib.rs:28`.

Risk or hypothesis:

- This is less important for the active native GPU path, but it remains a
  pattern to avoid in long-lived embedding or future host layers.

Remediation direction:

- Keep legacy Ply out of native GPU evidence.
- If the crate stays, replace leaked labels with owned/interned storage that
  can be cleared per runtime/session.

### Stringly IDs And Style Keys Are Everywhere

Confirmed by reading:

- Many public IDs and style accesses are `String` or string-keyed maps.
- Examples include document model IDs/styles and renderer style keys.

Risk or hypothesis:

- Stringly IDs increase allocation and make semantic distinctions weaker.
- Style lookups through formatted keys such as hover/focus/shadow fields are
  difficult to cache and validate.

Remediation direction:

- Use newtypes for boundary IDs and interned symbols for hot style/source/node
  keys.
- Move pseudo-state and style variants into typed structures before renderer
  lowering.

## Prior Art Checklist

Use these sources as lenses for future audits. They are supporting context, not
proof that a local code path is slow.

- Rust Performance Book: heap-backed `clone`, `String`, allocation, and hashing
  costs should be treated as profile-guided targets, not blanket refactors.
  Source: <https://nnethercote.github.io/perf-book/heap-allocations.html>
- Rust API Guidelines: newtypes provide static distinctions and should replace
  plain strings where IDs cross subsystem boundaries.
  Source: <https://rust-lang.github.io/api-guidelines/type-safety.html>
- `std::thread::sleep`: sleeps are blocking and may overshoot; fixed sleeps in
  verifiers should be replaced by event/report waits where possible.
  Source: <https://doc.rust-lang.org/std/thread/fn.sleep.html>
- Clippy `unwrap_used` and `expect_used`: panic paths should be audited in
  app/runtime/window/proof code and separated from test assertions.
  Source: <https://rust-lang.github.io/rust-clippy/master/>
- winit `EventLoop`: event-loop ownership and dispatch constraints make
  blocking locks/sleeps in event paths especially suspicious.
  Source: <https://docs.rs/winit/latest/winit/event_loop/struct.EventLoop.html>
- wgpu queue/buffer docs: writes use staging behavior and map/readback waits
  require explicit polling; upload/readback metrics and timeouts matter.
  Sources: <https://docs.rs/wgpu/latest/wgpu/struct.Queue.html>,
  <https://docs.rs/wgpu/latest/wgpu/struct.Buffer.html>
- cosmic-text: `FontSystem` and `SwashCache` are intended as app-level shared
  text infrastructure; repeated fresh font-system creation should be treated as
  suspicious in hot paths.
  Source: <https://docs.rs/cosmic-text>
- Vello `Scene`: retained scene/caches need explicit reset/eviction discipline;
  retained render structures without bounds or epochs are risky.
  Source: <https://docs.rs/vello/latest/vello/struct.Scene.html>
- Salsa: query-based incremental recomputation is a useful model for parser,
  lowering, typecheck, and static analysis reuse.
  Source: <https://salsa-rs.github.io/salsa/overview.html>
- Differential Dataflow: large dataflow-style workloads should update from
  differences rather than recomputing whole collections.
  Source: <https://docs.rs/differential-dataflow>

## Follow-Up Audit Order

1. Fix correctness hazards first.
   - `SourceStore::unbind_row` invariants.
   - Generic runtime TodoMVC-shaped behavior.
   - Document patch error handling and tree invariants.

2. Fix report/proof integrity next.
   - Scaffold proof cannot satisfy visible rendering gates.
   - Synthetic IPC stress cannot count as live backpressure proof.
   - Hard-coded zero counters need real instrumentation or removal.
   - Artifact freshness should be hash/provenance based, not mtime based.

3. Bound waits and shutdown.
   - WGPU readback/map waits need deadlines.
   - Fixed sleeps should become event/report waits.
   - Hard `process::exit` should move to top-level supervised exits.

4. Make hot-path work visible before refactoring for speed.
   - Add scan counts, cache hit/miss/eviction counts, upload bytes,
     primitive expansion counts, row counts, text shaping counts, and lock wait
     times.
   - Keep proof-mode readbacks and report serialization out of interaction
     budgets.

5. Reduce full recomputation.
   - Parser/IR semantic index.
   - Typed source route contracts.
   - Row lookup indexes.
   - Dirty-region and primitive-ID renderer caches.

6. Split architecture boundaries.
   - Extract native playground protocol/IPC/worker/dev-shell/verifier modules.
   - Extract xtask native GPU gates into smaller verifier modules.
   - Keep renderer primitives separate from app/editor semantics.

7. Replace stringly hot-path identity.
   - Intern field/source/style/node keys.
   - Use boundary newtypes for IDs.
   - Add collision and sparse-ID diagnostics.

## Acceptance Criteria For This Inventory

This document is useful only if future work preserves the distinction between
observation and proof:

- a local file/line anchor shows the code shape;
- hypotheses are not worded as measured facts;
- remediation does not weaken native GPU or scenario evidence;
- follow-up work adds targeted tests or metrics before large refactors;
- proof/report changes make evidence harder to fake, not easier to pass.

