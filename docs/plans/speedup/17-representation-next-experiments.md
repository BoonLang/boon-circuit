# Representation Experiments: Next Slices

This file captures the current JSON/binary, BYTES, container, LIST, and
constant ideas as near-term experiments. It is intentionally narrower than the
earlier catalogues: each slice must be engine-only, generic, measurable, and
easy to kill if it does not help.

## Ground Rules

- Do not add Boon syntax for these slices.
- Keep public scenario/report JSON unless the task explicitly migrates a schema.
- Replace JSON only where the producer and consumer are both internal Rust code,
  or where the public digest contract can be preserved by streaming the same
  canonical bytes.
- Replace `BTreeSet`/`BTreeMap` only at a hot boundary with a correctness oracle.
- Keep generic `LIST` semantics as the oracle while changing physical storage.
- Treat ambiguous constant or storage-mode inference as a compiler/runtime error,
  not as a request for manual annotations in Boon code.

## EXP-17-001 Binary Encoding Where JSON Is Accidental

Hypothesis:
Internal cache keys, state fingerprints, bridge staging, and renderer command
data should not serialize to JSON when they never cross a public boundary.

First candidates:

- Keep the new structured document-eval cache key and extend the same pattern
  to other in-process cache keys.
- Add a reusable typed encoder that can write deterministic fragments to a
  hasher, `Vec<u8>`, or debug string without round-tripping through
  `serde_json::Value`.
- For bridge canonical hashes, preserve existing JSON digests until a versioned
  ABI migration exists; optimize first by streaming canonical JSON into a hasher
  instead of allocating strings.
- Keep scenario files, reports, and human-readable diagnostics as JSON.

Acceptance:

- Type tags distinguish text, enum, bool, number, bytes, record, and list.
- Object/record field order is deterministic.
- Debug builds can reconstruct or print a readable shadow key.
- Existing public bridge golden vectors do not change unless
  `CANONICAL_SCHEMA_VERSION` changes with migration notes.

Kill criteria:

- The binary path increases total source-replacement time.
- A cache key collision is possible without a debug detector.
- The change makes report or scenario evidence less readable.

## EXP-17-002 BYTES As An Inferred Engine Value

Hypothesis:
Waveform files, decoded pages, blob payloads, bridge results, and renderer
uploads should move as shared byte slices or streams instead of text, JSON
arrays, or generic records.

Current anchor:

- `boon_bridge::BridgeValue::Bytes` already exists and is backed by
  `bytes::Bytes`.
- This repo should not invent a competing surface syntax. Older Boon design
  notes describe explicit `BYTES[...]` forms and conversion functions, but this
  speedup pass should first connect internal bridge/runtime representation and
  inference.

First candidates:

- Add runtime/typechecker `Bytes` value support only where bridge/file contracts
  infer it.
- Add byte/page/blob refs for large waveform payloads so state summaries do not
  inline data.
- Treat native source-project chunks as private bytes internally where the IPC
  frame already sends length-prefixed UTF-8 bytes; keep the Boon source model
  textual.
- Move real NovyWave waveform chunks/pages and bridge blob payloads toward
  `Bytes`, `BlobRef`, or `PageRef`; keep metadata labels and visible signal
  values as `TEXT`.
- Consider generated SVG/data-url assets as internal asset bytes while
  preserving the existing visible API.
- Refactor NovyWave bridge examples only when the contract proves the payload is
  raw bytes; visible labels, paths, statuses, and signal values stay `TEXT`.
- Consider `Stream<Bytes>` or chunk refs for large files after deterministic
  replay and cancellation rules are clear.

Acceptance:

- Byte payloads are not coerced to UTF-8 text.
- Large payloads can be sliced, cloned, cached, and dropped without full copies.
- Scenario replay records stable refs/digests rather than inline byte arrays.
- Type errors explain text-vs-bytes mismatches from bridge contracts.

Kill criteria:

- Boon programmers must add manual byte annotations to make common examples work.
- Byte payloads are converted back to JSON/text immediately after crossing the
  bridge.
- Streaming breaks deterministic replay.

Not BYTES candidates:

- TodoMVC titles, Counter labels, Cells formulas, NovyWave file paths,
  statuses, UI labels, scenario text, and human-readable diagnostics remain
  semantic `TEXT`.

## EXP-17-003 Container Replacement At LIST Boundaries

Hypothesis:
Replacing containers one by one is usually noise. The useful work is to replace
temporary ordered sets on known hot LIST paths with sorted vectors, dense IDs,
bitsets, or index-backed selections.

First slice:

- Indexed `List/filter_field_equal` and `List/filter_field_not_equal` already
  produce sorted `Vec<usize>` hits. Avoid converting those hits into
  `BTreeSet` when computing complements or matching homogeneous row-ref lists.
- This is intentionally a small proving slice. It should not be treated as the
  main LIST-storage win unless the speed oracle shows movement.

Acceptance:

- Existing indexed LIST tests still pass.
- `ListSelection` remains ordered and stable.
- Not-equal filters preserve visible list order.
- No example-specific branch is added.

Kill criteria:

- Order changes for `List/filter_field_not_equal`, `List/retain`, or
  `List/join_field`.
- The NovyWave speed profile regresses outside normal measurement noise.

Current result:

- Implemented as a small hygiene slice, not promoted as a speed win.
- Focused LIST/index tests pass and preserve not-equal order.
- NovyWave source-replacement timing stayed in the same broad band, with reruns
  at `total_ms=15394.1` / `layout_ms=1428.8` and `total_ms=14806.9` /
  `layout_ms=1401.2`; the slice did not materially move the current bottleneck.

## EXP-17-004 LIST Physical Storage Modes

Hypothesis:
User-facing `LIST` should stay generic, but the compiler/runtime can choose a
physical representation: constant array, dense mutable vector, selection view,
incremental projection, virtual list, indexed list, or stream-backed list.

First candidates:

- Keep current `ListSelection { list, indices: Vec<usize> }` and remove
  accidental materialization around it.
- Add diagnostics that classify root list views as literal constant, source
  backed, filtered selection, mapped projection, or virtual viewport.
- Prototype direct root `List/map` materialization for NovyWave selected lanes
  only through a generic root-list-view path.
- Add incremental exact-text lookup-index maintenance before rebuilding all
  text indexes on mutation; leave numeric/range indexes alone until equality
  lookup is proven.
- Add virtual list support only as a reusable component that works for Cells,
  TodoMVC, NovyWave, and future examples.

Acceptance:

- Generic LIST execution remains the oracle.
- Row identity and event routing are stable.
- Virtualization is not hard-coded to an example.

Kill criteria:

- A storage mode hides a dynamic dependency.
- The compiler cannot explain why a list cannot be made incremental.
- User-facing code must change to select a storage strategy.

## EXP-17-005 Constant Detection And Hoisting

Hypothesis:
Examples contain many labels, separators, field names, enum tags, static rows,
and style fragments. Rebuilding them during every event and every row
materialization wastes time.

First candidates:

- Add diagnostics that classify lowered expressions as literal constant,
  module constant, bridge-contract constant, row-invariant, or dynamic.
- Hoist only values whose dependency set is empty or explicitly row-invariant.
- Intern field names, enum tags, source names, and function names in hot IR and
  runtime paths.
- Precompute static parts of renderer commands and document-eval cache keys.

Acceptance:

- `SOURCE`, `HOLD`, row fields, bridge payloads, and runtime file data prevent
  folding unless proven invariant in the current materialization scope.
- Dynamic values never freeze.
- Diagnostics point back to Boon source locations.

Kill criteria:

- Any dynamic value is misclassified as constant.
- The hoisted form makes invalidation less precise.

## Additional Ranked Candidates

1. Direct root `List.map` materialization is the highest-payoff LIST experiment
   for the current NovyWave interaction miss. The existing path still evaluates
   through generic `List/map`, `Vec<BoonValue>`, and record materialization
   before root `ListView` output.
2. Typed dev render metadata is the cleanest remaining accidental-JSON target:
   native playground writes syntax spans and type hints as JSON strings in
   style text, and document lowering parses them back. This should become a
   typed producer/consumer path before any public report JSON rewrite.
3. Thresholded `DirtyKeySets` is a contained container experiment: start with a
   small-vector path and switch to a membership index only when duplicates or
   density justify it.
4. Incremental `ListMemory` exact-text lookup-index maintenance is a plausible
   medium-risk speedup for repeated indexed filters. Start with equality text
   indexes only.
5. Private cache-key writers for hot runtime keys are conditional: keep a debug
   shadow string and proceed only where profile fields show cache-key or record
   env fingerprint cost.
6. Constness work should start as diagnostics only; folding and hoisting come
   after the classifier proves dynamic dependencies are not frozen.
7. Runtime/typechecker BYTES is important architecture work, but it should
   target measured waveform/blob/page payload movement rather than the current
   NovyWave click-p95 budget.

## Current Result: Typed Dev Render Metadata

Implemented `EXP-17-001` for the dev editor render metadata path as a
compatibility-preserving internal representation slice.

What changed:

- `StyleValue` now has typed rich-text span and editor type-hint payload
  variants for in-memory frames.
- Those typed variants serialize as the legacy JSON string values so public
  reports and old artifacts keep the same scalar style shape.
- Document lowering and native GPU rich-text measurement consume typed payloads
  first and parse the old `*_json` text values only as fallback.
- Native editor full render, fast scroll patch, and type-inspector rows now
  write typed payloads instead of serializing JSON strings on the live path.
- Scalar style helpers treat typed payloads as non-scalar, so the new variants
  do not leak into number/text/bool style lookups.

Verification:

- `cargo test -p boon_document_model --lib typed_style_payloads_serialize_as_legacy_json_strings -- --nocapture`
- `cargo test -p boon_document --lib render_text_runs_lower_syntax_spans_and_type_hints_before_gpu -- --nocapture`
- `cargo test -p boon_native_gpu --lib rich_text_spans_preserve_exact_line_text -- --nocapture`
- `cargo test -p boon_native_playground --bin boon_native_playground code_editor_view_renders_mixed_lines_as_colored_segments -- --nocapture`
- `cargo test -p boon_native_playground --bin boon_native_playground code_editor_view_attaches_virtual_type_hint_metadata_without_changing_source_spans -- --nocapture`
- `cargo test -p boon_native_playground --bin boon_native_playground type_inspector_syntax_spans_color_notation_without_changing_text -- --nocapture`
- `cargo test -p boon_native_playground --bin boon_native_playground dev_render_scroll_patch_preserves_rich_spans_for_large_buffers -- --nocapture`
- `cargo check -p boon_document_model -p boon_document -p boon_native_gpu -p boon_native_playground`
- `cargo xtask verify-report-schema`
- `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`

Result:

- Kept as internal representation hygiene and as groundwork for future BYTES /
  binary/cache-key work.
- Not promoted as a NovyWave click-p95 speed win. The focused NovyWave
  source-replacement proof still reports the same broad post-layout-optimization
  shape: `total_ms=15451.5`, `live_runtime_ms=13886.4`,
  `layout_ms=1472.8`, and `document_eval_lower_ms=998.8`.
- The official click/input speed gate was not rerun for this slice because the
  changed path is dev render metadata, not the production NovyWave selected-lane
  interaction path.

## Current Result: IR Representation Classifier

Implemented the first `EXP-17-004` / `EXP-17-005` diagnostic slice in
`boon_ir` lower profiles. This is classification only; it does not fold
expressions, change LIST storage, alter runtime values, or add Boon syntax.

What changed:

- `lower_profile` now includes `representation_analysis_ms` and
  `representation_analysis`.
- The report classifies expressions into literal/static, row-dependent,
  source/HOLD-sensitive, runtime-dynamic, and unknown-dynamic buckets.
- It counts list-literal static/dynamic shape and LIST storage-mode candidates
  such as `constant_array_literal`, `selection_view`,
  `incremental_projection`, `virtual_range`, and dense/unknown fallbacks.
- Bounded root-derived samples include the path, source line, derived kind,
  expression class, and list-storage hints.
- Inline row bindings from `List/map`, `List/retain`, and related list
  operators are treated as row-dependent even when the parser does not expose a
  named row-scope function for an inline record projection.

Verification:

- `cargo fmt -p boon_ir`
- `cargo test -p boon_ir --lib lower_profile_reports_representation_candidates_without_folding -- --nocapture`
- `cargo test -p boon_ir --lib representation -- --nocapture`
- `cargo check -p boon_ir -p boon_runtime -p boon_native_playground`
- `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`

Result:

- Kept as compiler/runtime representation diagnostics, not as a speed win.
- The report gives later hoisting and LIST storage-mode work a source-linked
  candidate list without treating type information as value constness.
- The NovyWave source-replacement proof passed and exposed the report under
  `live_runtime_profile.plan.lower_profile.representation_analysis`; in the
  debug run the diagnostic itself cost about `399ms`, so it must stay a
  measured planning aid rather than hidden hot-path work.
- A broad `cargo test -p boon_ir --lib` run is not currently a clean verifier in
  this checkout: it passed 50 tests and failed 4 existing broader
  parser/typecheck/NovyWave expectation tests unrelated to this classifier.
- The BYTES subagent review found that `BridgeValue::Bytes` already exists, but
  Boon typechecking/runtime do not yet have a `Bytes` value. The next safe
  BYTES slice is bridge schema validation for bytes/blob/page/artifact refs,
  not Boon syntax and not converting visible labels or statuses to bytes.

## Current Result: Bridge Value Shape Validator

Implemented the first `EXP-17-002` bridge-boundary slice in `boon_bridge`.
This is BYTES/ref contract groundwork only; it does not add Boon syntax, add a
runtime `Bytes` value, change bridge JSON shape, change canonical hashes, or
wire schema shapes into the scheduler.

What changed:

- Added `validate_bridge_value_shape` for `BridgeValue` against
  `BridgeSchemaShape`.
- Primitive shapes reject mismatched bridge values with `SchemaMismatch`.
- `Bytes` values must carry a non-empty digest and their declared `byte_len`
  must match the shared byte buffer length.
- `BlobRef`, `ArtifactRef`, and `PageRef` values get shape-level sanity checks
  for required identity/digest fields.
- `List`, `Record`, `Tagged`, `Result`, and `Completion` shapes recurse through
  nested bridge values.
- Tests assert that validation leaves canonical JSON and canonical hashes
  unchanged for the accepted bytes/blob/artifact/page-ref sample.

Verification:

- Read-only bridge validator subagent review `019ec894-fc9d-7452-9d1d-1e142e85eb1b`
- `cargo fmt -p boon_bridge`
- `cargo test -p boon_bridge --lib -- --nocapture`
- `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`
- `cargo xtask check-bridge --report target/reports/check-bridge.json`
- `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
- `cargo xtask verify-report-schema`
- `git diff --check -- crates/boon_bridge/src/lib.rs`

Result:

- Kept as bridge contract hygiene and as a safe first BYTES/ref boundary.
- Not promoted as a speed win. It prevents text/ref kind drift before later
  runtime or scheduler BYTES work, but it does not move a measured NovyWave
  hot path by itself.
- The next bridge slice can decide how to carry schema shapes through the
  registry/scheduler or report checks without changing current public fixtures.

## Current Result: Bridge Scheduler Shape Sidecar

Implemented the next `EXP-17-002` bridge-boundary slice in `boon_bridge`.
Schema shapes now travel through a non-serialized registry sidecar and are
enforced by the effect scheduler when the sidecar is present.

What changed:

- `BridgeRegistry` can register `BridgeExportSchemas` for an export after
  checking that the schema versions and hashes match public metadata.
- The sidecar is skipped during serde, so public registry JSON still contains
  only module metadata, export hashes, versions, capabilities, and provider
  data.
- `BridgeEffectScheduler::schedule` validates request input shape after grant,
  payload-cap, and rust-handle checks preserve their existing error precedence.
- The scheduler stores the registered output shape for each live request and
  validates accepted completions before committing them.
- Replayed completions are validated against the registered output shape before
  replay is accepted.
- The fixture `open` request/output values now satisfy `OpenWaveformRequest`
  and `WaveformOpened`; the old bare page output is a negative shape case.
- `check-bridge` now reports seven proof rows, including schema-valid fixture
  values and scheduler rejection of missing options, bare page outputs, replay
  shape mismatches, and missing OK outputs.

Verification:

- Read-only bridge scheduler/shape review `019ec8a0-b5d6-72c1-8447-4433e38a3bc9`
- `cargo fmt -p boon_bridge`
- `cargo test -p boon_bridge --lib -- --nocapture`
- `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`
- `cargo xtask check-bridge --report target/reports/check-bridge.json`
- `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`

Result:

- Kept as correctness and replay-safety groundwork, not as a NovyWave speed
  win.
- No Boon syntax, bridge schema version, public registry metadata fields, or
  public report schema changed.
- The fixture completion golden vector digest changed because it now hashes a
  schema-valid `WaveformOpened` output instead of the old arbitrary record.
  The canonical hash algorithm and schema hashes are unchanged.

## Current Result: Bridge Completion Payload Sidecars

Implemented the next `EXP-17-002` bridge-boundary slice in `boon_bridge`.
Bridge completions can now validate descriptor refs against deterministic
sidecar bytes when the caller has real payloads available.

What changed:

- Added `BridgeCompletionPayloads`, backed by the existing `BridgePayloadStore`,
  for completion-scoped raw blob/page bytes.
- Added `BridgeEffectScheduler::complete_with_payloads(...)` as a payload-aware
  completion path while keeping `complete(...)` compatible for descriptor-only
  completions and existing replay fixtures.
- Completion payload validation recursively walks accepted output values and
  checks nested `BlobRef` and `PageRef` values against sidecar bytes.
- Missing sidecar bytes, byte-length drift, and digest drift are rejected as
  `SchemaMismatch` before a completion is accepted.
- Added a bridge-only fixture export whose declared output schema contains both
  a `BlobRef` and a `PageRef`, so this proof does not require Boon syntax,
  NovyWave hardcoding, or extra public scenario fields.
- `check-bridge` now reports nine proof rows, including
  `bridge_scheduler_completion_payload_sidecars_validate_refs=true`.

Verification:

- `cargo fmt -p boon_bridge`
- `cargo test -p boon_bridge --lib -- --nocapture`
- `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`
- `cargo xtask check-bridge --report target/reports/check-bridge.json`
- `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
- `cargo xtask verify-report-schema`

Result:

- Kept as deterministic bridge payload groundwork, not as a NovyWave speed win.
- No Boon syntax, bridge schema version, public registry JSON field, public
  report shape, or visible text/status/path representation changed.
- The public JSON descriptor still carries refs; real bytes live out of band
  and are validated only when the engine supplies sidecar payloads.
- This is the first safe step toward real waveform/blob/page payload movement
  without converting labels, diagnostics, scenario text, or reports to BYTES.

## Immediate Order

1. Use the new representation classifier to choose a measured LIST or
   constness optimization instead of guessing from source shape.
2. Treat the bridge value-shape validator, scheduler sidecar, and completion
   payload sidecars as done. The next BYTES slice should move real waveform/
   blob/page payload data or improve deterministic streaming/replay, not add
   Boon syntax or manual type annotations.
3. Prefer direct root `List.map` materialization if the active goal is NovyWave
   interaction p95 and the classifier/profile points to that root.
4. Continue typed/internal binary replacements only where the producer and
   consumer are both Rust and public JSON compatibility is preserved.
5. Do not hoist constants or change LIST physical storage until the diagnostic
   classifier plus runtime read evidence proves dynamic values will not freeze.
