# Binary, BYTES, LIST, And Constants Execution Memo

This memo captures the current representation ideas as an execution checklist
for the active speedup work. It overlaps with plans `14`, `15`, `16`, and `17`,
but is intentionally shorter: use it to decide the next experiment, not as a
complete architecture document.

## Rules

- Do not add Boon syntax for these changes. BYTES, LIST storage modes, indexes,
  and constants are engine/compiler choices unless a later design explicitly
  proves a surface change is unavoidable.
- Keep public scenario files, reports, and human-readable diagnostics JSON.
  Replace JSON only at private Rust-to-Rust boundaries or by preserving the
  exact public serialized shape.
- Start with one measurable slice. Revert or kill any experiment that passes
  focused tests but worsens the official NovyWave speed gate.
- Prefer structural fixes over Boon-level workarounds. If a Boon example has to
  be flattened, hard-coded, or manually annotated to make the engine fast, the
  engine still has a bug or missing representation.

## User-Suggested Experiment Families

### 18A. Replace JSON Where It Is Only Internal

Good candidates:

- Typed render metadata such as syntax spans and inline type hints that are
  produced by Rust and consumed immediately by Rust.
- Private cache keys and hashes where a typed encoder can write stable fragments
  directly to a hasher or byte buffer.
- Native IPC frames where the consumer is the paired Rust preview process.

Rules:

- Public reports may still serialize the typed value as the old JSON string.
- Debug builds should keep readable shadow output.
- Bridge canonical hashes must remain byte-for-byte stable unless a versioned
  schema migration is explicitly accepted.

### 18B. BYTES As An Inferred Engine Value

Good candidates:

- Raw waveform file bytes, decoded waveform pages, blob/page refs, and renderer
  upload buffers.
- Bridge contracts that already know a payload is binary.
- Streamed or chunked large-file data where cloning a shared byte slice is
  cheaper and clearer than moving nested text/JSON structures.

Not candidates:

- File names, UI labels, signal labels, formulas, statuses, scenario text, and
  diagnostics remain `TEXT`.

Rules:

- The Boon programmer should not need manual byte annotations for this speedup
  pass.
- State summaries should carry byte refs, digests, or page IDs, not inline
  megabyte arrays.
- Streaming `Bytes` needs deterministic replay and cancellation semantics
  before becoming a broad runtime primitive.

### 18C. Replace Tree Containers Only At Hot Boundaries

Good candidates:

- Temporary `BTreeSet` membership checks fed by already-sorted index hits.
- Dirty/read-key collections where a whole dependency representation can become
  lower allocation, not merely an added sidecar.
- Ordered sets that also need index access, where `IndexSet` or sorted `Vec`
  matches the semantic order.

Rules:

- Deterministic report order must stay deterministic.
- Do not retry known bad one-off sidecar/container experiments without new
  profile evidence.
- A container swap is promoted only if it moves a measured hot boundary.

### 18D. LIST Storage Modes

The user-facing `LIST` stays generic. The compiler/runtime can internally choose
one of several physical modes when usage proves it safe:

- constant array for literal static lists;
- dense `Vec` for ordinary materialized rows;
- selection view for filtered indexed lists;
- incremental projection for derived rows;
- virtual list for large visible windows;
- stream-backed or page-backed list for large binary/file payloads.

Rules:

- Generic LIST execution remains the oracle.
- Compiler errors are allowed only when inference would be ambiguous or unsafe;
  users should not select storage strategies manually.
- Virtual collections must be generic across examples, not Cells/TodoMVC/
  NovyWave-specific.

### 18E. Constants And Hoisting

Good candidates:

- Literal labels, separators, enum tags, field names, source names, static style
  fragments, and bridge-contract metadata.
- Static row templates whose dependency set is empty or row-invariant.
- Interned IR/runtime IDs for field paths and names used in hot loops.

Rules:

- `SOURCE`, `HOLD`, row fields, file data, bridge payloads, and event payloads
  prevent folding unless proven invariant in the current scope.
- Start with diagnostics/classification before broad hoisting.
- Any dynamic value freezing is an immediate kill condition.

## Current Start Choice

Start with typed dev render metadata:

- It is a small private JSON removal.
- It exercises the same representation discipline needed by future BYTES and
  binary/cache-key work.
- It keeps public JSON compatibility by preserving legacy string fallback.
- It does not rely on a speculative LIST/cache hit that the speed oracle has
  already rejected in earlier experiments.

Acceptance:

- Native editor live path stores syntax spans and type hints as typed style
  values, not newly serialized JSON strings.
- Document lowering and native GPU rendering consume the typed values first and
  keep legacy `*_json` string parsing as fallback.
- Existing report/schema verification remains green.
- Focused rich-text/type-hint tests still pass.

Kill criteria:

- The typed values break report serialization/deserialization compatibility.
- Scalar style helpers start treating typed metadata as text/number/bool.
- The change requires Boon source edits or example-specific branches.

Current result:

- Implemented and kept as representation hygiene.
- Live native editor, type inspector, document lowering, and native GPU
  rich-text measurement now carry syntax spans/type hints as typed style values
  first.
- Legacy serialized/report shape is preserved by serializing typed payloads as
  the same JSON strings and by parsing old `Text` values as fallback.
- Focused tests, check, report-schema verification, and the NovyWave
  source-replacement preview-state proof passed.
- This is not a production NovyWave click-p95 win by itself. It is the small
  accidental-JSON slice that makes later binary/BYTES/cache-key changes safer.

## Current Follow-Up Slice: IR Representation Classifier

Implemented the first diagnostic-only LIST/constant classifier in `boon_ir`.

What it reports:

- Expression buckets for literal/static, row-dependent, source/HOLD-sensitive,
  runtime-dynamic, and unknown-dynamic forms.
- LIST storage-mode candidates such as constant arrays, dense vectors,
  selection views, incremental projections, virtual ranges, and unknown
  fallbacks.
- Bounded root-derived samples with source lines and list-storage hints.

Rules preserved:

- No Boon syntax changed.
- No expression is folded or hoisted yet.
- Row fields, `SOURCE`, `HOLD`, event paths, state/list reads, and unknown
  symbols remain blockers rather than optimization permission.
- Inline `List/map` row bindings are detected as row-dependent even for inline
  record projections with no named constructor function.

Verification:

- `cargo test -p boon_ir --lib lower_profile_reports_representation_candidates_without_folding -- --nocapture`
- `cargo test -p boon_ir --lib representation -- --nocapture`
- `cargo check -p boon_ir -p boon_runtime -p boon_native_playground`
- `cargo test -p boon_native_playground --bin boon_native_playground switching_to_novywave_builds_runtime_state_before_preview_commit -- --nocapture`

Current NovyWave evidence:

- The source-replacement proof passed and now exposes
  `live_runtime_profile.plan.lower_profile.representation_analysis`.
- In the debug proof, `representation_analysis_ms` was about `399ms`; keep this
  as measured diagnostic overhead, not as a production hot-path assumption.

Current BYTES note:

- A read-only subagent confirmed `BridgeValue::Bytes` already exists in
  `boon_bridge`, but Boon typechecking/runtime do not yet have byte values.
- The next safe BYTES step is bridge schema validation for bytes/blob/page/
  artifact refs, then measured bridge/runtime integration only where real
  waveform/blob/page payloads move.
- Visible names, labels, statuses, formulas, scenario text, and public reports
  remain `TEXT`/JSON unless a versioned public schema migration is accepted.

## Current Follow-Up Slice: Bridge Value Shape Validator

Implemented the first bridge-boundary BYTES/ref contract slice in
`boon_bridge`.

What changed:

- `validate_bridge_value_shape` checks `BridgeValue` against
  `BridgeSchemaShape`.
- `Bytes` is accepted only as `BridgeValue::Bytes` with a non-empty digest and
  matching declared byte length.
- `BlobRef`, `ArtifactRef`, and `PageRef` are distinct shapes and get basic
  required-field sanity checks.
- Lists, records, tagged values, results, and completion output shapes recurse
  through nested bridge values.
- Accepted values keep the same canonical JSON and canonical hash.

Verification:

- `cargo test -p boon_bridge --lib -- --nocapture`
- `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`
- `cargo xtask check-bridge --report target/reports/check-bridge.json`
- `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
- `cargo xtask verify-report-schema`

Current result:

- Good bridge hygiene, but not a measured speed win.
- No Boon syntax, runtime `Bytes`, scheduler schema fields, bridge schema
  version, or public JSON shape changed.
- The next BYTES experiment should either carry schema shapes through the
  scheduler safely or integrate real waveform/blob/page payload movement.

## Current Follow-Up Slice: Bridge Scheduler Shape Sidecar

Implemented the safe scheduler-carriage step for bridge schema shapes.

What changed:

- Export schema shapes are registered as a non-serialized `BridgeRegistry`
  sidecar after their hashes match public export metadata.
- The effect scheduler validates input shape during scheduling and stores the
  output shape for the live request.
- Accepted and replayed completions must satisfy the registered output shape.
- Fixture open requests and opened responses now match their declared schemas;
  missing options, bare page output, replay shape drift, and OK-without-output
  are explicit `check-bridge` negative cases.

Verification:

- `cargo test -p boon_bridge --lib -- --nocapture`
- `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`
- `cargo xtask check-bridge --report target/reports/check-bridge.json`
- `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`

Current result:

- Good bridge correctness groundwork, but still not a measured speed win.
- No Boon syntax, bridge schema version, public registry JSON shape, or public
  report schema changed.
- The fixture completion golden vector digest changed only because the fixture
  completion now carries a schema-valid `WaveformOpened` output.
- The next BYTES work should target real raw waveform/blob/page bytes or
  deterministic streaming/replay rather than labels, paths, statuses, or
  scenario/report JSON.

## Current Follow-Up Slice: Bridge Completion Payload Sidecars

Implemented the deterministic payload-sidecar step for bridge completions.

What changed:

- `BridgeCompletionPayloads` carries completion-scoped raw blob/page bytes
  behind `BlobRef` and `PageRef` descriptors.
- `BridgeEffectScheduler::complete_with_payloads(...)` validates accepted
  output refs against those sidecar bytes before committing the completion.
- Existing `complete(...)` remains descriptor-only compatible for current
  callers and fixtures.
- A bridge-only fixture export proves both blob and page refs without changing
  Boon syntax or NovyWave source.
- `check-bridge` now has nine proof rows and includes
  `bridge_scheduler_completion_payload_sidecars_validate_refs=true`; missing
  page sidecars and drifted page bytes reject with `SchemaMismatch`.

Verification:

- `cargo test -p boon_bridge --lib -- --nocapture`
- `cargo check -p boon_bridge -p boon_runtime -p boon_native_playground`
- `cargo xtask check-bridge --report target/reports/check-bridge.json`
- `timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json`
- `cargo xtask verify-report-schema`

Current result:

- Good bridge payload hygiene, but not a measured speed win.
- No Boon syntax, runtime `Bytes`, bridge schema version, public report JSON, or
  visible text/status/path representation changed.
- The next BYTES work should connect real waveform/page/blob producers to this
  sidecar path or design deterministic streaming/replay. Do not convert labels,
  diagnostics, scenario text, or public reports to bytes.
