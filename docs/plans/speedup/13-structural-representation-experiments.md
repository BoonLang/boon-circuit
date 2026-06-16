# Structural Representation Experiments

## Purpose

This file turns the current representation-level speed ideas into bounded
experiments for Boon and NovyWave. The theme is simple: stop paying text,
JSON, tree-map, and whole-list costs in places where the compiler or runtime
can know the shape more precisely.

These experiments are intentionally below the Boon user-facing surface. They
should make existing Boon programs faster and more reliable without forcing
manual type annotations, selectors, index declarations, or NovyWave-specific
source rewrites.

## Ground Rules

- Do not add Boon syntax for speed unless a runtime/compiler-only path is
  proven impossible and documented in the master checklist.
- Treat ambiguity as a compiler/typechecker error with concrete repair tips.
- Keep JSON for human-facing reports, schemas, and external artifacts.
- Avoid JSON as the internal runtime representation for known typed values.
- Keep every optimized representation convertible back to the current summaries
  and reports so full recompute remains the correctness oracle.
- Promote an experiment only when the bridge proof is green and the official
  speed report moves materially toward the failing interaction budget.
- Kill and revert experiments that merely shift work into another root,
  diagnostic path, or bridge materialization.

## Current Local Evidence

- `BoonValue` has scalar, record, list, row-ref, list-ref, and list-selection
  variants, but no bytes/binary value shape yet.
- `FieldValue` stores text, bool, enum, or `serde_json::Value`; nested records,
  lists, row refs, selections, and numbers commonly fall through JSON when
  stored as runtime fields.
- `ValueColumns` already separates text, bool, enum, and JSON columns, but
  structured field fallback still serializes through JSON for storage, nested
  dependency collection, summaries, and cache fragments.
- `GenericEvalFrame` and `GenericDerivedState` use many `BTreeSet` and
  `BTreeMap` structures for reads, dependents, dirty roots, free names, numeric
  stability guards, and root list view caches.
- NovyWave uses many constant text descriptors, page labels, bridge contract
  labels, file names, route labels, and repeated `Text/concat` chains around
  fingerprints and page descriptors.
- List execution already has `ListRef`, `RowRef`, `ListSelection`, text/numeric
  indexes, map/join fusion, and numeric retain stability guards, but still
  materializes many intermediate `Vec<BoonValue>` values for generic list
  operations.

## BYTES Design Anchor

The sibling Boon design docs already define `BYTES`, so this work should restore
or implement a designed type rather than invent a one-off runtime blob.

Useful source design points from `~/repos/boon`:

- `docs/language/BYTES.md` defines byte-oriented data for buffers, files,
  protocols, and serialization.
- Literals include fixed or inferred sizes, such as `BYTES[4] { ... }` and
  `BYTES[__] { ... }`.
- Empty `BYTES {}` is meaningful for dynamic or streaming byte data.
- Operations include `Bytes/length`, `Bytes/get`, `Bytes/slice`,
  `Bytes/concat`, `Bytes/find`, `Bytes/equal`, `Bytes/to_text`,
  `Bytes/from_text`, and typed reads with explicit endianness.
- `docs/language/TEXT_SYNTAX.md` makes TEXT to BYTES conversion explicit at
  I/O boundaries, with compile-time TEXT constants eligible for zero-runtime
  byte conversion where a byte boundary requires it.
- `docs/language/storage/TABLE_BYTES_RESEARCH.md` describes streaming BYTES and
  chunked file processing with `Stream<Bytes>`-like behavior.

For this repo, the first BYTES step should be an internal value and bridge
representation. Parser syntax can come later only if a scenario needs literal
BYTES source.

## Experiment Families

### STRUCT-JSON-001 Internal Binary Structured Fields

Hypothesis:
Known internal structured field values can use a compact typed/binary
representation instead of `serde_json::Value` until a report or external API
actually needs JSON.

Candidate implementation:

- Add an internal structured field payload beside `FieldValue::Json`.
- Preserve current JSON summaries by converting only at report boundaries.
- Start with row-local structured values that are repeatedly stored/read but not
  exposed as external JSON in the hot interaction path.
- Keep root-read-key collection aware of the structured shape instead of first
  converting to JSON.

Metric to improve:

- Lower `store.selected_signal_lane_rows` total and selected-lane
  `eval_minus_field`, `list_map_total`, `user_body`, or `record_loop`.
- Reduce JSON conversion/count/bytes if counters are added.

Correctness oracle:

- Existing runtime state summaries and NovyWave bridge reports are unchanged.
- Full recompute and incremental turn output match for selected-lane scenarios.

Verification:

```bash
cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture
cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture
cargo check -p boon_runtime -p boon_native_playground
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
cargo xtask verify-report-schema
```

Kill criteria:

- Any summary/report shape changes without an explicit schema migration.
- Bridge proof failure.
- Click/input p95 does not improve materially or max regresses.
- Work shifts into `store.bridge_cursor_values.rows`.

### STRUCT-BYTES-001 Runtime BYTES Value

Hypothesis:
`BYTES` should be a first-class runtime value for opaque binary payloads, bridge
data, encoded descriptors, and future file/page chunks. This avoids pretending
binary data is text or JSON.

Candidate implementation:

- Add `BoonValue::Bytes` and `FieldValue::Bytes` backed by a cheap shared byte
  buffer such as `Arc<[u8]>` or the `bytes` crate if dependency review accepts
  it.
- Add `FieldValueRef::Bytes`.
- Add summary conversion that reports bytes as stable metadata, not as large
  inline arrays, unless a test explicitly asks for inline bytes.
- Keep equality/hash/cache fragments deterministic and bounded.
- Implement only internal construction first; defer parser literal support.

Metric to improve:

- Lower bridge/file/page descriptor storage cost once binary descriptors are
  introduced.
- Reduce accidental JSON/string allocation for opaque blobs.

Correctness oracle:

- Existing non-BYTES examples produce identical summaries.
- BYTES round-trip tests prove equality, summary truncation, and cache-fragment
  stability.

Verification:

```bash
cargo test -p boon_runtime --lib bytes_ -- --nocapture
cargo test -p boon_runtime --lib value_columns_ -- --nocapture
cargo check -p boon_runtime -p boon_native_playground
```

Kill criteria:

- BYTES leaks giant byte arrays into normal JSON reports.
- Cache fragments become unbounded for large byte values.
- Any text/enum behavior changes.

### STRUCT-BYTES-002 Bridge And Example BYTES Refactor

Hypothesis:
NovyWave bridge artifacts, page descriptors, fixture descriptors, encoded SVG
assets, and future waveform chunks should use BYTES where they are actually
binary or encoded payloads.

Candidate implementation:

- Identify bridge fields that are data blobs, hashes, digests, byte lengths, or
  encoded payloads.
- Keep labels, statuses, page kinds, and human-visible fields as TEXT or enum.
- Prefer inferred BYTES from bridge/Rust requirements or typed API metadata.
- Use explicit TEXT to BYTES conversion only at real byte boundaries.

Metric to improve:

- Reduced bridge payload conversion cost.
- Cleaner type contracts for future real wellen/waveform data.

Correctness oracle:

- NovyWave bridge proof still passes.
- The UI-visible labels remain unchanged.

Verification:

```bash
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
cargo xtask verify-report-schema
```

Kill criteria:

- Boon examples get manual BYTES annotations only to satisfy the current engine.
- Human-visible fields become binary-looking or unreadable.
- External reports lose stable JSON compatibility.

### STRUCT-SET-001 Replace Transient BTreeSet Membership

Hypothesis:
Some hot `BTreeSet` usage is only transient membership testing over small or
already sorted numeric sets. A sorted/deduped `Vec` with `binary_search`, or a
specialized index set later, can remove tree-node allocation while preserving
determinism.

Candidate implementation:

- Start with local conversions that are not stored in public state.
- Prefer sorted `Vec` for numeric membership where output order is already
  deterministic.
- Consider `IndexSet` only where insertion order is semantically wanted and the
  crate is already justified by measurement.
- Consider bitsets only after list/row IDs are dense and stable.

Metric to improve:

- Lower numeric retain/filter/list pipeline overhead.
- Lower selected-lane cursor-value materialization where retain pipelines are
  hot.

Correctness oracle:

- Numeric retain stability guard tests still skip same-interval recomputes and
  recompute boundary-crossing changes.
- Fused filter/retain/map/join tests still report zero scanned rows where
  indexes should be used.

Verification:

```bash
cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture
cargo test -p boon_runtime --lib numeric_retain_stability_guard_skips_same_interval_row_candidate -- --nocapture
cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture
cargo check -p boon_runtime
```

Kill criteria:

- Recompute candidates change incorrectly.
- Retain/index counters regress.
- A local replacement makes the code less clear without measurable benefit.

### STRUCT-SET-002 Dense Read Keys And Dirty Sets

Hypothesis:
The real long-term fix for read/dependency sets is not swapping `BTreeSet` for
another string-keyed container. It is compiler/runtime IDs for roots, lists,
fields, source routes, and derived values.

Candidate implementation:

- Intern `GenericReadKey` parts into dense IDs.
- Store read sets as sorted small vectors or bitsets keyed by those IDs.
- Keep a reverse label table only for diagnostics and reports.
- Replace string path variants with canonical IDs plus explicit alias metadata.

Metric to improve:

- Lower source apply/recompute dirty-set time.
- Lower memory and clone costs in `GenericEvalFrame` and `GenericDerivedState`.

Correctness oracle:

- Recompute samples and dependency reports remain readable.
- Duplicate labels and stale rows route correctly.

Verification:

```bash
cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture
cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture
cargo check -p boon_runtime -p boon_native_playground
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
```

Kill criteria:

- Diagnostics lose path labels.
- Ambiguous route behavior becomes silent.
- Dirty fanout changes without a full recompute oracle comparison.

### STRUCT-LIST-001 List Shape Classification

Hypothesis:
The compiler can classify list expressions and initializers into storage shapes:
fixed arrays, appendable vectors, row-store lists, selections, virtual ranges,
and incremental views.

Candidate implementation:

- Add IR metadata for list shape and allowed mutations.
- Keep user syntax unchanged.
- `LIST { ... }` with compile-time rows can lower to a fixed array/template.
- `List/range` can remain virtual until an operation requires materialization.
- `List/filter_*` and `List/retain` over row stores should produce selections,
  not copied lists.
- `List/map` should update one mapped row when one source row changes.

Metric to improve:

- Lower `list_map_total`, `list_map_row`, and materialized row counts in
  NovyWave.
- Lower rows-scanned counters in large synthetic list tests.

Correctness oracle:

- Full recompute output equals incremental output after append, remove, reorder,
  row edit, source change, and viewport change.

Verification:

```bash
cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture
cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture
cargo test -p boon_runtime --lib root_list_view_materialization_samples_include_phase_profile -- --nocapture
cargo check -p boon_runtime -p boon_ir
```

Kill criteria:

- A list shape shortcut assumes append-only behavior for a list that can remove
  or reorder.
- The optimization only works for Cells or NovyWave by name.
- It hides an engine limitation behind Boon-level source changes.

### STRUCT-LIST-002 Incremental And Virtual Collections

Hypothesis:
Virtual and incremental lists should be a generic engine protocol, not a
NovyWave/cells-specific widget hack.

Candidate implementation:

- Represent row count, row key range, viewport range, overscan, row height
  policy, and materialized binding lifecycle as engine metadata.
- Keep source events tied to stable row keys and generations, not visible row
  indices alone.
- Use full recompute as the oracle until incremental patches prove equivalent.

Metric to improve:

- Fewer materialized document nodes and source bindings for large views.
- Faster scroll/hover/click on Cells and NovyWave.

Correctness oracle:

- Source events for recycled rows reject stale generations.
- Full and virtual materialization agree for visible rows.

Verification:

```bash
cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json
cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
```

Kill criteria:

- Virtualization changes visible content or hit routing.
- It works only for a hardcoded example.

### STRUCT-CONST-001 Constant Interning And Folding

Hypothesis:
Many NovyWave constants are recomputed, rehashed, cloned, and concatenated as
ordinary runtime values. The compiler can fold and intern pure constants and
precompute route/page descriptor fragments.

Candidate implementation:

- Intern text, enum tags, field names, function names, source names, page
  kinds, file names, style keys, and bridge labels.
- Fold pure `TEXT`, enum, number, bool, and literal record/list fragments.
- Precompute pure `Text/concat` chains when all inputs are constant.
- Precompute stable parts of page/digest/fingerprint labels and append only the
  changing fields at runtime.
- Store constant IDs in IR/runtime plans instead of rehashing labels through
  `FieldSlotId::from_path` and string maps.

Metric to improve:

- Lower repeated string allocation, hashing, and cache-fragment construction.
- Lower selected-lane and bridge descriptor root totals.

Correctness oracle:

- Dynamic source-driven branches still update correctly.
- Constants do not freeze values that depend on `SOURCE`, `HOLD`, row fields, or
  bridge payloads.

Verification:

```bash
cargo test -p boon_ir --lib source_payload_concat_inside_latest_lowers_without_then_wrapper -- --nocapture
cargo test -p boon_runtime --lib source_payload_concat_inside_latest_updates_without_then_wrapper -- --nocapture
cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture
cargo check -p boon_runtime -p boon_ir
```

Kill criteria:

- Any dynamic expression is folded as if constant.
- Compiler diagnostics become less clear.
- Speed does not move after folding the measured hot constants.

### STRUCT-CONST-002 Constant-Aware Cache Fragments

Hypothesis:
Cache keys and dependency fingerprints currently stringify structured values in
places where a stable typed hash or interned constant ID would be enough.

Candidate implementation:

- Replace JSON string cache fragments for `RecordColumns` with typed field/value
  traversal.
- Hash typed scalar values directly.
- Hash structured fields with stable IDs and bounded binary encoding.
- Keep human-readable fragments only in diagnostics.

Metric to improve:

- Lower cache-key construction time for root list view field caching and
  user-function value caching.

Correctness oracle:

- Cache invalidation tests still prove stale values are not reused.
- Collision diagnostics exist for any compact hash use.

Verification:

```bash
cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture
cargo test -p boon_runtime --lib root_list_view_field_cache_ -- --nocapture
cargo check -p boon_runtime
```

Kill criteria:

- Cache collisions can silently produce wrong values.
- Debug output becomes impossible to interpret.

## First Slice

Start with `STRUCT-SET-001` because it is local, reversible, and directly tests
the user's `BTreeSet` concern without changing Boon syntax or public report
schemas.

The first code change should replace the transient `BTreeSet` in numeric retain
stability guard membership with a sorted/deduped `Vec<usize>` and
`binary_search`. This is intentionally small. It will not finish TASK-0804 by
itself, but it creates a disciplined template for representation experiments:

1. Make one local representation replacement.
2. Run the focused correctness tests.
3. Run the bridge/speed oracle only if the focused tests pass.
4. Keep the patch only if it improves or is neutral locally and does not make
   the speed oracle worse.

If this slice is too small to move the official p95, continue with
`STRUCT-CONST-001` and `STRUCT-JSON-001`, because the current selected-lane
profiles still point at repeated row body/materialization work rather than one
isolated set lookup.
