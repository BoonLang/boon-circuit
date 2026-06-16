# Binary, BYTES, LIST, And Constant Representation Experiments

This file collects representation-level experiments that can make Boon and the
NovyWave path faster without adding Boon syntax or hard-coding one example. The
theme is to stop forcing every value through the most general representation
when the compiler or runtime can prove a narrower one.

## Rules

- Do not add Boon syntax for these experiments unless a later design document
  explicitly proves it is unavoidable.
- Keep public JSON reports and scenario fixtures stable unless a task explicitly
  changes a report schema with migration notes.
- Run one experiment at a time. Keep it only if focused correctness tests pass
  and the relevant NovyWave speed or phase profile improves or remains neutral.
- Prefer typed, collision-resistant encodings over ad hoc strings. If compact
  hashing is introduced, add collision diagnostics or debug shadow keys first.
- Treat any Boon-level workaround as temporary evidence for an engine/compiler
  bug. The final fix belongs in the compiler, runtime, bridge, or renderer.

## Experiment Families

### REPRESENT-BINARY-001 Typed Internal Encoding Instead Of JSON Strings

Hypothesis:

Some hot paths use JSON as an internal interchange format even when the value
never needs to leave the process. Cache keys, dependency fingerprints, bridge
payload staging, and renderer command streams can use typed binary or
length-prefixed encodings instead.

Candidate slices:

- Replace `RecordColumns` cache-fragment JSON stringification with direct typed
  traversal.
- Stream existing canonical JSON bytes directly into hashers where a public
  JSON-based digest contract must remain byte-for-byte stable.
- Add a reusable `ValueEncoder` that can write stable typed fragments to
  `String`, `Vec<u8>`, or a hasher depending on the caller.
- Keep JSON only at the external report/debug boundary.
- Measure bridge scenario payloads and replace repeated JSON serialization with
  binary payloads where the producer and consumer are both Rust.
- Treat a true binary bridge canonical format as a versioned schema migration,
  not a silent implementation detail.

Correctness oracle:

- Text `"true"` must not collide with bool `true`.
- Enum `"A"` must not collide with text `"A"`.
- Nested object/list encodings must be deterministic independent of insertion
  order.
- Existing report JSON must remain readable and schema-compatible.

Kill criteria:

- Cache keys can silently collide.
- Debuggability disappears before shadow diagnostics exist.
- Binary conversion cost is higher than the JSON path it replaces.
- Existing canonical bridge digests change without an explicit
  `CANONICAL_SCHEMA_VERSION` migration and golden-vector update.

### REPRESENT-SET-001 Replace Ordered Sets Where Ordering Is Accidental

Hypothesis:

Some `BTreeSet` and `BTreeMap` use exists only for deterministic iteration or
deduplication, but the hot operation is membership, insertion, or indexed
lookup. Those sites may be faster as sorted `Vec`, `IndexSet`, `FxHashSet`, or
small stack-backed sets.

Candidate slices:

- Audit every `BTreeSet` in the NovyWave runtime path and classify it as
  deterministic-order, membership-only, range-query, or indexed-access.
- Use sorted `Vec` only when the set is built once and queried many times.
- Use `IndexSet` when deterministic order and index access are both needed.
- Use hash sets only where deterministic report order is not externally
  visible, or sort only at the report boundary.

Correctness oracle:

- Public reports remain deterministic.
- Candidate/recompute order changes only where order is unobservable.
- Membership replacements are measured against real hot-path profiles, not just
  microbenchmarks.

Kill criteria:

- A replacement improves a microbenchmark but regresses the NovyWave interaction
  speed gate.
- The new container makes invalidation order or diagnostics nondeterministic.

### REPRESENT-BYTES-001 Native BYTES Runtime Value

Hypothesis:

Boon needs an engine-native byte value for waveforms, bridge payloads, decoded
blocks, file chunks, and renderer uploads. Treating these as `TEXT`, JSON arrays,
or generic records creates unnecessary allocations and parsing.

Design direction:

- Add an internal `BYTES` value equivalent to `bytes::Bytes` for shared slices
  and cheap clones.
- Support streaming byte chunks for large files as `Stream<Bytes>` or a runtime
  list of chunk refs, without requiring the Boon programmer to manage chunking.
- Let bridge functions accept and return bytes when Rust/Boon bridge contracts
  require it; infer the Boon type from bridge requirements and usage.
- Keep user-facing syntax unchanged for now. File and bridge APIs should expose
  bytes through typed contracts, not new surface syntax.

Alignment with the upstream Boon BYTES design:

- The upstream language design already specifies `BYTES[__] { ... }` and
  `BYTES[n] { ... }`, explicit byte bases such as `16uFF`, fixed-size checking,
  nested-byte flattening, byte-aligned `BITS` conversion, and functions such as
  `Bytes/from_text`, `Bytes/from_base64`, `Bytes/from_hex`, and `Bytes/zeros`.
- It also requires explicit endianness for multi-byte typed reads/writes.
- The storage research extends BYTES toward chunk-based streaming for files and
  cloud/object-store payloads.
- This repo should not invent a competing surface. The speedup path is to add
  the internal runtime/bridge representation first, infer it from bridge/file
  contracts where possible, and later connect it to the upstream syntax once the
  parser/typechecker/runtime plan is explicit.

NovyWave candidates:

- Waveform file loading and decoded block caches.
- FST/VCD/GHW raw data ingestion.
- Renderer upload buffers for dense signal spans or text atlas data.
- Bridge IPC payloads currently represented as JSON strings or nested arrays.

Correctness oracle:

- Byte payloads do not accidentally become UTF-8 text.
- Large files can be sliced, cached, and dropped without copying the entire
  content repeatedly.
- Bridge errors show meaningful type diagnostics when text/bytes are mixed.

Kill criteria:

- BYTES leaks into user syntax before inference and bridge typing are ready.
- Byte lifetimes require unsafe ownership shortcuts.
- Streaming changes make deterministic scenario replay harder.

### REPRESENT-LIST-001 Proven LIST Storage Modes

Hypothesis:

`LIST` can often be represented more specifically than a generic row list. The
compiler/runtime can choose storage based on usage: fixed arrays for constants,
`Vec` for dense mutable lists, virtual/incremental collections for viewport
queries, and indexed lists for field lookup.

Candidate storage modes:

- Constant array: literal list with no source dependencies.
- Dense mutable vector: append/edit-heavy row lists.
- Incremental row list: derived list where only a subset of rows/fields changes.
- Selection view: filtered/retained list represented as source list plus indices.
- Virtual list: large or infinite list where only visible ranges materialize.
- Indexed list: field equality/range lookups backed by runtime indexes.

Compiler responsibilities:

- Infer storage mode from list construction, downstream operations, and bridge
  contracts.
- Emit a compiler error only when ambiguity would change semantics or make
  invalidation unsound.
- Preserve the generic `LIST` model for users; storage mode is an engine detail.

Correctness oracle:

- `List/map`, `List/filter_field_equal`, `List/retain`, `List/join_field`, and
  root list-view materialization produce identical values.
- Scenario replay and state summaries remain deterministic.
- Virtual lists are generic and reusable across apps, not hard-coded for cells,
  todomvc, or NovyWave.

Kill criteria:

- A storage mode requires example-specific code.
- Incremental reuse hides a dynamic dependency change.
- Virtualization changes user-visible row identity.

### REPRESENT-CONST-001 Compiler And Runtime Constants

Hypothesis:

Boon programs contain many literal labels, separators, selectors, record field
names, enum values, and static row fragments. Recomputing and rehashing them on
every event wastes time.

Candidate slices:

- Constant-fold pure literal records/lists/text concatenations.
- Intern field names, enum tags, source names, and function names in IR.
- Precompute stable parts of cache keys and renderer command labels.
- Store typed constant IDs in plans rather than reconstructing
  `FieldSlotId::from_path` and strings repeatedly.
- Hoist static row template fields out of per-row evaluation.

Correctness oracle:

- Any dependency on `SOURCE`, `HOLD`, row fields, bridge payloads, or runtime
  file data prevents folding.
- Ambiguous dynamic/static classification is a compiler error with actionable
  context.
- Constant IDs survive module loading and bridge imports generically.

Kill criteria:

- A dynamic value freezes because it was misclassified as constant.
- Error messages point at lowered internals instead of the original Boon code.

## Initial Execution Order

1. Start with `REPRESENT-BINARY-001` by replacing `RecordColumns` cache-fragment
   JSON stringification with direct typed traversal.
2. Use the result as the template for future binary/hash encoders. The first
   bridge-side follow-up should preserve existing digest bytes by streaming
   JSON into the hasher; a true binary bridge format comes later with a version
   bump.
3. Revisit `REPRESENT-SET-001` only with a concrete profile target. A previous
   transient-set replacement regressed the NovyWave speed gate, so broad set
   rewrites are not allowed.
4. Plan `REPRESENT-BYTES-001` as a typed bridge/runtime design task before code
   changes. It is likely a multi-crate change.
5. Prototype `REPRESENT-LIST-001` in a root list-view hot path after cache and
   constant slices prove their measurement workflow.
6. Add constant folding only with compiler/runtime tests that prove dynamic
   dependencies are not frozen.
