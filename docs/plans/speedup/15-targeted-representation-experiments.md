# Targeted Representation Experiments From Current NovyWave Evidence

This file turns the current representation ideas into concrete TASK-0804
experiments. It is intentionally narrower than the earlier research plans: each
idea here must either explain how it can move the current NovyWave click/input
budget or record why it should wait.

Current measured baseline:

- `verify-native-gpu-novywave-interaction-speed` still fails click/input p95.
- The latest clean post-revert report is `status=fail` with
  `input_to_visible.p95=22.929ms` and `click_to_cursor.p95=22.929ms` against
  the `16.700ms` budget.
- The top grouped click root totals are still
  `store.selected_signal_lane_rows=24.420ms` and
  `store.bridge_cursor_values.rows=15.095ms`.
- Previous standalone `BTreeSet` replacement, dense read-ID sidecar,
  root-list row-output cache, row-field ID micro-cache, and root read-key alias
  narrowing experiments preserved focused correctness but were rejected by the
  speed oracle.

## Rules

- No new Boon syntax for these experiments.
- Keep public JSON reports, scenario fixtures, and bridge canonical schema
  stable unless a task explicitly declares a schema migration.
- Keep one code experiment at a time. Revert it if the speed oracle rejects it.
- Treat `bytes::Bytes`, compact binary encoders, dense list storage, and
  constants as internal representation choices inferred from compiler/runtime
  facts or bridge contracts.
- Do not retry the killed micro-slices without new counters proving a different
  bottleneck.

## Experiment A: Bridge BYTES Moves From `Vec<u8>` To Shared Bytes

Hypothesis:

`BridgeValue::Bytes` already exists, but the payload is `Vec<u8>`. Replacing
the internal byte payload with a cheap-clone shared byte buffer can make future
waveform/file payloads cheaper to pass through bridge/runtime layers. This is
not expected to solve the current click p95 alone, but it is the safest first
BYTES implementation slice because it exercises the existing bridge type
without adding Boon syntax.

Candidate implementation:

- Add the Rust `bytes` crate with serde support.
- Change only the internal bridge byte payload from `Vec<u8>` to
  `bytes::Bytes`, preserving the serialized JSON shape and canonical hashes.
- Add constructors/helpers so callers do not depend on the concrete storage.
- Keep `BridgeValue::Bytes { digest, byte_len, bytes }` as the public enum
  shape for now.

Acceptance:

- Existing bridge golden vectors and canonical hashes do not change.
- JSON serialization for `BridgeValue::Bytes` remains compatible with existing
  reports and tests.
- `validate_payload_cap` checks logical `byte_len` and actual byte storage
  length.
- Focused bridge tests prove clone/equality/serde round-trip behavior.

Kill criteria:

- The serialized bridge JSON changes without an explicit schema migration.
- `bytes::Bytes` serde changes golden vectors or diagnostics.
- The change requires unsafe lifetime tricks or leaks storage ownership details
  into Boon code.

Verification:

```bash
cargo test -p boon_bridge --lib -- --nocapture
cargo check -p boon_bridge -p boon_runtime -p boon_native_playground
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
cargo xtask verify-report-schema
```

Promotion:

Keep if it is correctness-neutral and preserves public bytes. Do not claim a
TASK-0804 speed win unless the official speed report also improves.

Status note:

- Started as the first implementation slice. It is representation groundwork,
  not the expected click-p95 fix.
- The follow-up BYTES work should target real binary payload movement instead
  of more bridge enum cleanup.

## Experiment A2: Binary Native Source IPC Frame

Hypothesis:

The dev-to-preview source replacement path sends newline-delimited JSON over a
Unix stream. Its `SourceProjectPayload.units[].text` field carries full Boon
source as JSON strings. Producer and consumer are both Rust, and reports already
hash the source bytes, so this is a better binary-encoding target than public
reports.

Candidate code paths:

- `PreviewTransport::replace_source_project`
- `handle_preview_ipc_client`
- `send_preview_ipc_request_with_timeouts`
- `SourceProjectPayload.units[].text`

Candidate implementation:

- Keep a small JSON control envelope for request type, version, source-unit
  metadata, and fallback compatibility.
- Move source text into length-prefixed UTF-8 byte chunks on the Unix socket.
- Decode back into the existing `SourceProjectPayload` before
  `preview_enqueue_source_project`, so parser/runtime still receive `String`.
- Keep the current JSON-lines path as fallback until the binary path proves
  latest-wins, stale-source, and backpressure behavior.

Acceptance:

- Binary and JSON paths decode to identical project payloads: paths, roles,
  active file, text, per-unit SHA-256, project hash, and entrypoint.
- Preview still receives Boon source, not example names.
- Non-UTF-8 source bytes are rejected with a clear transport error.
- `preview_blocked_on_ipc_count=0` remains true.
- Public reports stay JSON-compatible.
- Source-switch request bytes and source-switch round-trip are lower or neutral
  on large multi-file examples.

Kill criteria:

- Binary framing leaks into Boon syntax, parser APIs, scenario files, or public
  report schemas.
- Latest-wins or stale-source behavior changes.
- `sync_ack_payload_bytes_max=16384` is violated.
- Source-switch p95/max regresses.

Verification:

```bash
cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json
cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json
cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json
```

Status note:

- The first implementation slice is kept as internal transport groundwork. The
  preview/dev source-project protocol now supports a JSON control line plus
  length-prefixed UTF-8 source chunks, with JSON payload fallback still
  available.
- This is not a NovyWave click-p95 fix. It only removes an avoidable JSON
  source-transfer cost and gives the verifier an explicit binary transport
  flag.
- The current blocker exposed by this experiment is source-project prewarm and
  replacement work that can still spend too long inside the preview process.

## Experiment A2b: Async Latest-Wins Source Prewarm

Hypothesis:

Binary source IPC is not enough if prewarm still performs parse/lower/runtime
and layout work synchronously on the preview IPC server. Prewarm should accept
source bytes quickly, coalesce stale work, and mark reusable project/runtime
hashes only after a background build succeeds.

Candidate implementation:

- Keep the binary source-project frame from Experiment A2.
- Add a bounded single-slot prewarm worker with latest-wins semantics.
- Return a small `queued` ACK immediately for nontrivial prewarm requests.
- Do not mutate live preview source, runtime, render state, or
  `replace-source-status` from prewarm completion.
- Mark only prewarm cache state after a successful background build.
- Report queue depth, stale drops, input count, and coalescing count.

Acceptance:

- Prewarm ACKs stay small and do not contain runtime summaries or layout proof.
- Binary source-project transport remains true for prewarm and replace probes.
- Repeated prewarm bursts coalesce stale work instead of blocking IPC.
- Replace-source status continues to be owned by the replace worker, not the
  prewarm worker.
- Already-prewarmed hashes can suppress the pending overlay only when the
  preview process actually marked the project/runtime hash prewarmed.

Kill criteria:

- Prewarm completion commits source/runtime/render state.
- Queued prewarm ACKs make verifier reports claim a project was prewarmed
  without an actual preview-side mark.
- The background worker allows unbounded project builds or memory growth.
- Example-switch proof still fails and reports binary transport as false.

Verification:

```bash
cargo test -p boon_native_playground --bin boon_native_playground source_project -- --nocapture
cargo test -p boon_native_playground --bin boon_native_playground example_tab_switch_uses_fast_visual_path_and_async_project_work -- --nocapture
cargo test -p boon_native_playground --bin boon_native_playground replace_source_ack_is_small_and_worker_commits_latest_revision -- --nocapture
cargo test -p boon_native_playground --bin boon_native_playground preview_replace_worker_queue_reports_live_latest_wins_metrics -- --nocapture
cargo check -p boon_native_playground -p xtask
cargo xtask verify-native-example-switch-speed --profile debug --report target/reports/native-gpu/example-switch-speed-debug.json
```

Status note:

- This slice is currently in progress. Focused unit tests pass for queued
  prewarm ACKs and binary source frame round-trips, but the live debug
  example-switch gate still fails after NovyWave replace work remains pending.
  A later verifier fix enabled app-owned preview frame readback, so top-level
  readback hashes are now real, but per-switch final frame binding and
  latest-wins proof still need work.
- The result is useful because it narrows the problem: source transfer and
  prewarm IPC are no longer the only blocker. The next source-switch fix must
  make heavy replace builds interruptible/coalesced at a lower layer or repair
  the final readback/latest-wins proof path.

## Experiment A3: Stop Encoding Dev Render Metadata As JSON Strings

Hypothesis:

Dev editor render metadata is stored inside style values as JSON strings such
as `syntax_spans_json` and `editor_type_hints_json`, then parsed again during
render-scene lowering. This is a repeated native/dev rendering path and a
clearer JSON-removal candidate than verifier reports.

Candidate implementation:

- Replace JSON-in-style strings with typed document/render-scene metadata.
- Keep the public report/debug view JSON at the boundary.
- Ensure renderer-neutral lowering owns the typed metadata before the GPU crate.

Acceptance:

- Dev editor syntax spans and type hints render identically.
- Document/render-scene tests cover typed metadata lowering.
- Native architecture/dependency reports still prove semantic lowering stays
  before the GPU crate.

Kill criteria:

- The typed metadata path duplicates large editor state every frame.
- The change moves editor semantics back into `boon_native_gpu`.
- Public report schema churn is required only to remove an internal JSON parse.

## Experiment B: Internal Binary Encoder Instead Of JSON For Private Keys

Hypothesis:

Some private cache keys still use string fragments that behave like handwritten
JSON. The earlier `RecordColumns` cache-fragment cleanup removed one
`value_columns_json(columns).to_string()` conversion and was kept as hygiene,
but it did not move p95. A real binary encoder should target repeated private
keys that appear in `store.selected_signal_lane_rows` samples, not public
reports.

Candidate implementation:

- Add a small typed `CacheKeyWriter` that can append type tags, lengths, string
  bytes, integers, booleans, and field IDs to either `Vec<u8>` or a hasher.
- Start with `root_list_view_env_fingerprint` and
  `generic_function_cache_key` only if profile evidence shows cache-key
  construction is a meaningful part of `user_function_record_env_fingerprint_ms`
  or user-function cache work.
- Keep existing string key generation as a debug shadow during the experiment
  and assert equality/collision separation in tests.

Acceptance:

- Text `true`, bool `true`, enum `True`, and JSON string `"true"` remain
  distinct.
- Object/record order remains deterministic.
- Public reports remain JSON.
- Debug builds can print or reconstruct a readable key for diagnostics.

Kill criteria:

- Hash/key collisions are possible without debug shadow checks.
- The encoder adds allocation or conversion work that raises runtime p95.
- The implementation hides the original Boon field/function names in errors.

Verification:

```bash
cargo test -p boon_runtime --lib record_columns_cache_fragment -- --nocapture
cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture
cargo test -p boon_runtime --lib root_list_view_ -- --nocapture
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
```

Promotion:

Keep only if click/input p95 or selected-lane root totals move materially
toward budget without moving work into `store.bridge_cursor_values.rows`.

## Experiment C: Container Replacement Only Behind A Hot Boundary

Hypothesis:

Replacing `BTreeSet` or `BTreeMap` blindly is noise or a regression. The prior
transient set swap and dense read-ID sidecar both failed the speed oracle. The
next container work must replace a whole representation boundary, not a local
collection in isolation.

Allowed candidates:

- `IndexSet` only where deterministic order and index access are both required.
- Sorted `Vec` only where the set is built once and queried many times.
- Dense IDs or bitsets only if the canonical hot-path storage is replaced
  end-to-end, not added as a sidecar beside string keys.
- Hash maps/sets only where report order is explicitly sorted at the boundary.

Acceptance:

- The replacement removes an entire conversion or invalidation pass.
- Determinism of reports and diagnostics is preserved.
- Focused tests prove broad list-structure dirty reads still overlap
  `ListField` and `ListColumn` dependencies.

Kill criteria:

- The change only improves a microbenchmark.
- The official speed oracle regresses.
- The replacement adds sidecar synchronization or key materialization overhead.

## Experiment C1: DirtyKeySets Sorted/Dense Boundary

Hypothesis:

Dirty keyed work currently deduplicates through small `Vec` scans and owned
string entries. That is acceptable for tiny updates, but larger semantic
deltas should use a sorted tuple vector or dense keyed representation before
fanout and recompute candidate accounting.

Candidate implementation:

- Keep the current tiny `Vec` path for very small dirty-key counts.
- For larger counts, store `(list_id, field_id, key)` through interned or
  compact IDs and sort/dedup once.
- Preserve the existing dirty-set recommendation report so measurement can
  decide whether bitset, sorted vec, or current vec is best.
- Do not add a second sidecar that must be synchronized with canonical dirty
  entries unless it replaces the hot boundary end to end.

Acceptance:

- Duplicate dirty attempts are counted identically.
- Recompute candidates and top recompute causes are unchanged.
- Existing dirty-set density recommendations still appear in reports.
- Focused semantic/list invalidation tests pass before any speed claim.

Kill criteria:

- Tiny updates get slower because every path pays sorting or interning cost.
- The representation changes recompute candidate order in a user-visible
  report without sorting at the report boundary.
- The official NovyWave speed oracle regresses.

Verification:

```bash
cargo test -p boon_runtime --lib dirty -- --nocapture
cargo test -p boon_runtime --lib root_list_view_ -- --nocapture
cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture
cargo check -p boon_runtime
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
```

## Experiment C2: LIST Selection Complement Without Tree Sets

Hypothesis:

Some `List/filter_field_equal(..., equal: false)` paths convert matching
indices into `BTreeSet` and then scan the whole list. When indexed matches are
already sorted or can be sorted once, complement can use merge/difference over
sorted vectors or dense `BitVec` masks.

Candidate implementation:

- Preserve sorted `Vec<usize>` outputs from indexed lookup where possible.
- For `equal: false`, compute complement by linear merge against list order
  instead of allocating a `BTreeSet`.
- Use `BitVec` only when density makes it cheaper than sparse vectors.
- Keep generic full-scan behavior as fallback/oracle.

Acceptance:

- Equal and not-equal filter outputs are identical for `ListRef`,
  `ListSelection`, and homogeneous row-ref lists.
- List order is preserved.
- Indexed filter/retain/map/join counters still prove index usage where
  expected.

Kill criteria:

- Complement output changes row order or row identity.
- Dense masks allocate more than the previous sparse path on small selections.
- Text/equality filter tests pass but NovyWave bridge or speed proof regresses.

Verification:

```bash
cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture
cargo test -p boon_runtime --lib list_retain_numeric_row_field_compare_updates_from_root_cursor -- --nocapture
cargo test -p boon_runtime --lib root_list_view_ -- --nocapture
cargo check -p boon_runtime
```

## Experiment C3: Incremental LIST Lookup Index Maintenance

Hypothesis:

`ListMemory` already has a strong columnar shape, but text and numeric lookup
indexes are invalidated wholesale on mutations and rebuilt by scanning visible
rows. Maintaining indexes incrementally across append/remove/field update could
remove repeated rebuild work in filter/find-heavy examples.

Candidate implementation:

- Track index updates during append, remove, and typed field writes.
- Keep `BTreeMap` for numeric/range indexes until equality-only text profiles
  justify a hash index.
- Start with text/equality indexes because NovyWave and TodoMVC use many exact
  field lookups.
- Retain full rebuild as a debug/oracle path and for complex structural
  mutations.

Acceptance:

- `List/find_value`, `List/filter_field_equal`, retain/map/join fusion, and
  root-list tests pass.
- Index counters show fewer full index rebuilds on representative scenarios.
- Stale removed rows cannot be returned from an index.
- Numeric range behavior remains deterministic.

Kill criteria:

- Index maintenance adds enough mutation cost to regress append/edit scenarios.
- Removed or reordered rows can be returned by an old index entry.
- The implementation duplicates index state without clear ownership.

Verification:

```bash
cargo test -p boon_runtime --lib List/find_value -- --nocapture
cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture
cargo test -p boon_runtime --lib root_list_view_append_after_remove_does_not_reuse_stale_row_projection -- --nocapture
cargo check -p boon_runtime
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
```

## Experiment C4: Runtime List Name Lookup Index

Hypothesis:

Runtime lists are stored in a vector and list lookup by name still uses linear
search in some paths. A small name-to-slot index may help large programs if
profiles show repeated list-name lookup.

Candidate implementation:

- Add a name-to-slot index only if profile counters show repeated list-name
  lookup cost.
- Keep deterministic list iteration order separate from lookup storage.
- Update the index at list creation/removal only; do not add per-row overhead.

Acceptance:

- Program/list loading order and reports remain deterministic.
- Missing-list diagnostics keep the original list name.
- Large-program lookup counters drop or remain neutral.

Kill criteria:

- No measured repeated list-name lookup exists.
- The index introduces lifetime or synchronization complexity for little gain.

## Experiment D: BYTES In Runtime And NovyWave Examples

Hypothesis:

Waveform file bytes, decoded waveform pages, bridge payloads, and renderer
upload buffers should not flow through `TEXT`, JSON arrays, or generic records.
The upstream Boon design already has `BYTES[__]`, fixed-size `BYTES[n]`,
explicit byte bases, text conversion functions, and streaming research. This
repo should implement the runtime/bridge representation first and let bridge
contracts infer the Boon type.

Candidate implementation:

- Add `BoonValue::Bytes` only after the bridge `Bytes` representation is stable.
- Add `Type::Bytes`, `FieldValue::Bytes`, and byte-aware value columns before
  allowing bytes to flow through ordinary runtime fields.
- Teach Rust/Boon bridge conversion to infer bytes from bridge schema shape.
- Convert NovyWave bridge/file payloads to bytes only where the value is truly
  binary: raw waveform chunks, decoded page bytes, or blob/artifact payloads.
- Keep labels, file names, status strings, and UI text as `TEXT`.
- Prefer page/blob refs for large or streamed data; inline bytes must stay
  capped and deterministic.

Acceptance:

- Text/bytes ambiguity is a compiler or bridge-contract error with a useful
  message.
- Scenario replay remains deterministic.
- Large payloads can be sliced and dropped without copying the entire file.
- No Boon syntax is required for NovyWave to use BYTES through bridge/file
  contracts.

Kill criteria:

- BYTES becomes a user-facing syntax dependency before type inference and
  bridge typing are ready.
- Binary payloads are converted back to JSON or text immediately.
- Large payloads enter hot interaction reports or state summaries.

Status note:

- Bridge bytes already exist through `BridgeSchemaShape::Bytes` and
  `BridgeValue::Bytes`, backed by `bytes::Bytes`.
- Runtime and typechecker bytes are still missing: there is no `Type::Bytes`,
  `BoonValue::Bytes`, or `FieldValue::Bytes` yet.
- Strong NovyWave candidates are future raw waveform chunks, decoded page
  bytes, blob payloads, and asset byte caches. Current labels, file paths,
  digests, statuses, formulas, and visible waveform labels should remain
  semantic text.

## Experiment D2: Typed Blob/Page Boundary For NovyWave

Hypothesis:

NovyWave should not model large waveform data as ordinary nested Boon records
or text fragments. The engine should expose typed `BlobRef`, `ArtifactRef`, and
`PageRef` values at the bridge/runtime boundary, then materialize only the
visible rows and labels needed by the UI.

Candidate implementation:

- Keep visible row labels, path labels, status labels, and digest text stable.
- Convert ad hoc bridge boundary records to typed bridge/runtime values where
  schema contracts already know the shape.
- Store raw or decoded page bytes behind refs, not inline in state summaries.
- For generated SVG/data-url assets, introduce an internal asset/blob cache
  while preserving the current Boon-visible output until the renderer consumes
  typed assets directly.
- Compute private request/page/cache keys through a typed byte writer while
  still exposing digest strings where users or reports need them.

Acceptance:

- NovyWave bridge proof still passes.
- The UI-visible text output is unchanged.
- Large payloads do not appear inline in ordinary JSON summaries.
- Bridge/type diagnostics distinguish text, bytes, blob refs, artifact refs,
  and page refs.

Kill criteria:

- A Boon example has to manually annotate BYTES or refs just to satisfy the
  current engine.
- Visible file names, labels, formulas, or status text become binary-looking.
- The ref layer hides stale page data or makes scenario replay nondeterministic.

## Experiment E: LIST Storage Modes Inferred By The Engine

Hypothesis:

`LIST` should remain the user model, but the compiler/runtime can choose a
storage mode: constant array, dense mutable vector, incremental projection,
selection view, virtual list, or indexed list. The current NovyWave p95 problem
suggests `store.selected_signal_lane_rows` and
`store.bridge_cursor_values.rows` are repeatedly materialized as generic lists
when the engine needs a more explicit list plan.

Candidate implementation:

- Add a list-shape classifier that reports observed construction and use:
  literal constant, source-backed dense rows, filtered selection, map over row
  refs, map/join cursor projection, viewport/materialized window, or unknown.
- Do not change execution in the first slice unless classification is proven
  stable and useful.
- Promote one storage mode only after the classifier shows a repeated hot shape
  and the correctness oracle exists.

Acceptance:

- Reported storage-mode recommendations are generic and contain no example
  names.
- Full generic LIST execution remains the oracle.
- Ambiguous or dynamic shapes stay generic rather than being forced into a
  wrong optimized mode.

Kill criteria:

- The classifier adds hot-path cost without being gated to diagnostic/profile
  sampling.
- A storage mode changes row identity, removal/reorder behavior, or source
  binding generation.

## Experiment E2: Direct Root `List.map` Materialization

Hypothesis:

`materialize_root_list_view_field` currently evaluates a root list expression,
turns list refs/selections into a fresh `Vec<BoonValue::RowRef>`, lets
`List/map` build another `Vec<BoonValue>`, then converts records into
`ValueColumns`/row snapshots. Current TASK-0804 evidence still points at
`store.selected_signal_lane_rows`, `list_map`, user-body, and record-loop work,
so a direct root `List.map` materializer is a stronger LIST experiment than
another container swap.

Candidate implementation:

- Detect root `ListView` fields whose source is `ListRef` or `ListSelection`
  piped into `List/map`.
- Preserve the selection/mapped-selection representation instead of forcing a
  generic `Vec<BoonValue>` result.
- Materialize directly into `ValueColumns` / `RuntimeRowSnapshot`.
- Precompute field IDs and field expression plans only when this removes
  actual per-row body work.
- Keep generic evaluation as oracle/fallback.
- Preserve `RootListViewFieldCacheContext`, reads, numeric guards, dirty
  dependency behavior, same-count reorder, and remove-then-append correctness.

Acceptance:

- Focused tests cover root list-view field cache, same-count reorder,
  remove-then-append/no-stale-row, caller-env separation, indexed
  filter/retain/map/join, and mapped root-list function behavior.
- NovyWave bridge proof passes.
- Official speed report moves `store.selected_signal_lane_rows` or
  click/input p95 materially toward `16.700ms` without raising
  `store.bridge_cursor_values.rows`.

Kill criteria:

- Any stale `current_value`, `page_refs`, selection, collapse, reorder, cursor,
  or file-row behavior.
- `store.bridge_cursor_values.rows` rises materially while selected-lane work
  does not improve.
- The implementation detects NovyWave names instead of a generic root
  `ListView`/`List.map` shape.

Verification:

```bash
cargo test -p boon_runtime --lib root_list_view_ -- --nocapture
cargo test -p boon_runtime --lib user_function_cache_ -- --nocapture
cargo test -p boon_runtime --lib mapped_root_list_function_filters_segments_with_store_cursor -- --nocapture
cargo test -p boon_runtime --lib indexed_filter_retain_map_join_pipeline_fuses_cursor_value_shape -- --nocapture
cargo check -p boon_runtime -p boon_native_playground
timeout 240 cargo xtask verify-novywave-bridge-scenario --report target/reports/novywave-bridge-scenario.json
cargo xtask verify-native-gpu-novywave-interaction-speed --report target/reports/native-gpu/novywave-interaction-speed.json
```

## Experiment F: Compiler And Runtime Constants

Hypothesis:

NovyWave and the examples contain many constants: labels, separators, field
names, enum tags, style tokens, function names, and static row fragments. The
compiler/runtime should intern and hoist these when their dependencies prove
constant. The previous field-ID micro-cache was too small and was rejected, so
the next constants work must eliminate repeated row-body work rather than only
changing string ownership.

Current state:

- IR already has const-oriented forms such as `UpdateExpression::Const`,
  `UpdateValueExpression::Const`, match-const branches, and append const
  lowering.
- That is pattern-specific lowering, not a general post-lowering constness
  analysis. The next pass should classify more expressions only when it can
  prove dependencies and preserve diagnostics.

Candidate implementation:

- Add a constness analysis over lowered IR that classifies expressions as
  literal-constant, module-constant, bridge-contract-constant,
  row-invariant-per-materialization, or dynamic.
- Emit diagnostics when an expression cannot be classified safely and an
  optimization would depend on the classification.
- Use interned IDs for field names, enum tags, source names, and function names
  only when they remove per-row reconstruction or hashing.
- Hoist row-invariant fragments only with a dependency contract that proves
  caller env, row field, `SOURCE`, `HOLD`, bridge payload, and file data
  dependencies cannot be frozen incorrectly.

Acceptance:

- Dynamic values never freeze because of constness.
- Error messages point to original Boon source concepts, not lowered internals.
- The official bridge proof stays green.
- The speed oracle moves selected-lane row-body/materialization work, not just
  a small sub-counter.

Kill criteria:

- The pass depends on NovyWave-specific names or fixtures.
- The speed win comes from hiding semantic deltas or reducing scenario coverage.
- The official speed gate regresses even if a local allocation counter improves.

## User-Suggestion Map

This is the concrete mapping for the current representation ideas:

- Replace JSON with binary encoding where possible:
  - keep public reports and scenario artifacts as JSON;
  - use A2/A2b for private Unix-stream source transfer;
  - use A3 for JSON-in-style dev render metadata;
  - use B only for private cache keys with collision shadows.
- Replace `BTreeSet`/`BTreeMap` where they are accidental:
  - do not retry local swaps that already failed the speed oracle;
  - use C only when the change removes a whole representation boundary;
  - try C1 dirty-key sorted/dense representation before deeper dependency-ID
    rewrites;
  - try C2/C3 LIST selection and lookup-index maintenance before container
    changes in unrelated runtime maps;
  - prefer sorted `Vec`, `IndexSet`, dense IDs, or bitsets based on the
    observed access pattern, not by container fashion.
- Implement BYTES:
  - A is the bridge storage foothold with `bytes::Bytes`;
  - D is the runtime/NovyWave path for real binary waveform chunks, page bytes,
    and blob payloads;
  - labels, file names, statuses, and UI strings stay as `TEXT`.
- Optimize LIST:
  - E adds a generic storage-mode classifier before changing execution;
  - E2 is the likely click-p95 lever because current evidence points at
    selected-lane root `ListView` / `List.map` materialization.
- Identify constants:
  - F should intern or hoist only values proven literal, module-constant,
    bridge-contract-constant, or row-invariant for a materialization;
  - current const handling is pattern-specific, so a general pass belongs after
    dirty/LIST representation experiments unless profiles prove expression eval
    dominates;
  - ambiguous constness is a compiler/runtime diagnostic, not a request for
    manual Boon annotations.

## First Execution Choice

Start with Experiment A. It is unlikely to close the click p95 gap alone, but it
is the cleanest way to begin the BYTES path the language already wants:

1. It touches the existing bridge BYTES shape.
2. It can preserve JSON reports and canonical hashes.
3. It creates the storage primitive needed before runtime `BoonValue::Bytes`.
4. It has a clear correctness oracle and low rollback cost.

After that, choose based on the current bottleneck evidence:

- If the next focus is binary transport and source switching, finish A2b and
  then make heavy replace-source builds interruptible/latest-wins at a lower
  layer.
- If the next focus is the failing NovyWave click p95, try E2 before another
  local container or cache-key micro-optimization.
- If dirty-key metrics show nontrivial duplicate or dense keyed work, try C1.
- If indexed list filters or not-equal complements dominate, try C2 and then
  C3.
- If BYTES stays isolated to bridge tests and NovyWave does not move binary
  payloads through runtime yet, continue with a bridge/runtime BYTES conversion
  task.
- If speed reports still show selected-lane row-body/materialization dominating,
  add LIST-shape classification before any more list execution rewrites.
- If expression evaluation or repeated static row construction dominates after
  LIST work, try F with a debug shadow that proves dynamic dependencies do not
  freeze.

Do not use public report JSON removal as a speed shortcut. Reports are part of
the verifier/debug contract; internal JSON strings and transport payloads are
the right targets.
