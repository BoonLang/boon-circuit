# Boon Persistence Architecture Plan

Date: 2026-07-13

Status: architecture decision and implementation plan. Durable Boon application
state is not implemented in `boon-circuit` yet. This document defines the
language, compiler, runtime, storage, development tooling, and verification
contract for implementing it.

## Executive Decision

Use a Rust-only persistence stack:

- Native storage engine: [`redb`](https://github.com/cberner/redb).
- Browser/Wasm storage adapter: Rust code using
  [`rexie`](https://docs.rs/rexie) over the browser's IndexedDB API.
- Durable value codec: [`minicbor`](https://docs.rs/minicbor) with an explicitly
  versioned Boon-owned schema.
- Test backend: an in-memory Rust implementation of the same durable-store
  contract with deterministic fault injection.

No SQLite C library, `rusqlite`, JavaScript-authored storage layer, JSON product
state path, or Python tooling belongs in this implementation.

`redb` is the native choice because it is stable, maintained, pure Rust, ACID,
crash-safe by default, uses copy-on-write B+trees, provides MVCC readers, and
supports explicit transaction durability, savepoints, repair, and custom
storage backends. Its file format is documented as stable with an upgrade path.

Turso Database is not selected. It is an attractive pure-Rust, SQLite-compatible
engine and may become useful for a future SQL-facing Boon data product, but its
own repository currently labels the database beta. Canonical runtime state must
start on the smaller stable key-value abstraction that Boon actually needs.

The browser adapter is still a Rust-only application implementation. IndexedDB
is a host platform service, like files or WGPU, and `rexie` exposes it to Rust
and Wasm. No handwritten JavaScript storage implementation is required.
`redb::StorageBackend` makes a future redb-on-OPFS experiment possible, but that
is not the first browser implementation: its synchronous `Send + Sync` storage
contract must first be proven sound and performant over browser worker/OPFS
semantics. The durable-store contract below prevents that experiment from
changing Boon semantics.

## Product Goal

Boon applications must be able to stop, restart, load compatible updated code,
and continue from their last durable authoritative state without storing or
restoring the entire dataflow graph.

Persistence must preserve Boon's core properties:

- The compiler owns static graph and storage metadata.
- `Session` remains the only runtime execution owner.
- A Boon turn commits atomically.
- Derived data is recomputed through currentness barriers.
- Dynamic lists keep hidden row identity and generation.
- Rendering remains retained and independent from durable storage.
- Normal interaction frames never wait for background storage unless an
  explicitly strict external-effect policy requires a durable acknowledgement.
- Persistence remains generic. No example name or Cells-specific branch may
  appear in parser, compiler, runtime, document, renderer, playground, or
  verifier code.

## Current State

### Implemented Session Memory

`boon_plan_executor::Session` currently owns:

- root `HOLD` state values;
- derived root fields and their currentness;
- logical lists, row order, hidden row keys, and generations;
- raw and derived row fields;
- indexes and dynamic dependencies;
- source sequence and binding identity.

This is valid in-memory reactive state. It is not durable application state.

### Debug Snapshot, Not Restore Format

The current `boon_plan_executor::Snapshot` contains:

```text
states: StateId -> Value
fields: FieldId -> Value
lists: ListId -> rows and fields
```

`Session::snapshot()` clones root states, demanded derived root fields, list
rows, raw row fields, and any materialized current derived fields. It has no
durable serialization contract and there is no inverse restore/hydration API.
It mixes authoritative and derived data, and its numeric IDs are local to one
compiled plan.

The implementation should eventually rename this type to `DebugSnapshot` so it
cannot be mistaken for a persistence checkpoint. Durable restore uses separate
`RestoreImage` and `DurableCommitBatch` types.

### Startup Always Builds Fresh State

`Session::new_shared` currently:

1. constructs plan metadata;
2. initializes scalar state from compiler-provided defaults;
3. initializes all logical lists and rows;
4. initializes indexed state;
5. evaluates root-initial fields;
6. makes demanded derived roots current.

There is no restore barrier between raw state allocation and derived root
evaluation. Persistence requires splitting this sequence so restored values are
installed before derived computation or output publication.

### Sequential Runtime IDs

The IR currently assigns `StateId`, `ListId`, `FieldId`, and `PlanStorageId` by
enumeration. The IR also retains useful semantic paths, but the plan does not
yet have durable storage identity or type fingerprints.

Numeric plan/runtime IDs must never be disk keys. Adding or reordering an
unrelated declaration can change them.

### Process-Local Runtime Cache

The native preview caches up to eight `RuntimeView` values by exact source key.
Switching back to an unchanged source can recover the mounted runtime while the
preview process remains alive. Edited source creates a new runtime, and process
restart loses all state. This cache is useful mounting behavior, not persistence
or migration.

### Unrelated Project File Persistence

`boon_native_playground::workspace::PersistenceWorker` atomically writes custom
example source and manifest files under `playground/custom_examples`. It does
not persist Boon runtime state. Rename it to `ProjectFileWriter` when the durable
runtime store is introduced so the two responsibilities cannot be confused.

### Current Inspector

The dev inspector currently exposes only:

```text
symbol
static type
detail
current runtime value
```

It has no knowledge of state class, durable identity, default value, last saved
value, pending commit, storage backend, schema compatibility, or migration.

## Language Semantics

### Four Value Roles

Every inspectable value has exactly one role:

1. **Durable state**: authoritative application memory that survives process
   restart and compatible deployments.
2. **Session state**: ordinary `HOLD` memory that survives ticks and compatible
   in-process development reload, but not cold restart.
3. **Transient state**: state that intentionally resets during reload, such as
   focus, hover, caret, temporary drag state, timing handles, and secrets.
4. **Derived value**: pure or demand-current data reconstructed from state and
   inputs; never persisted.

`HOLD` continues to mean semantic memory. It does not automatically mean disk
durability.

### One Explicit Durable Boundary

The application author marks a model boundary as durable instead of annotating
every field. Illustrative syntax is:

```boon
state: PERSIST {
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }

    todos: LIST { ... }

    -- Derived from authoritative state. Visible inside the same model, but not
    -- written to storage.
    completed_count: List/count(todos, where: completed)
}

ui: TRANSIENT {
    focused_row: NoRow |> HOLD focused_row { ... }
}

document: app_view(PASS: [state: state, ui: ui])
```

The exact parser spelling is decided during the language phase, but these
semantics are fixed:

- `PERSIST` marks a lexical/model policy boundary, not a request to serialize
  the boundary's resulting record.
- The compiler discovers storage cells and list memories owned by the boundary.
- `SOURCE` ports are not persisted.
- Pure fields and projections are not persisted.
- Render/document/scene values are not persisted.
- Ordinary `HOLD` outside a durable boundary is session state.
- `TRANSIENT` explicitly prevents compatible hot reload transfer as well as
  disk persistence.
- Unsupported runtime handles or sensitive values inside `PERSIST` are compile
  errors, not silently skipped fields.

### Persistence Is Orthogonal To Backend

Boon source states which memory is durable. Deployment/host configuration
selects where and how strongly it is stored. Source must not mention redb,
IndexedDB, filesystem paths, or database table names.

### Secrets And External Resources

Passwords, authentication tokens, cryptographic keys, open file handles,
network sockets, timers, WGPU handles, and host objects are transient by
default. Platform secret stores require a separate typed capability; they must
not be represented as ordinary `PERSIST` values.

An external resource may persist a serializable descriptor, such as a content
digest, URL, or application-relative file reference. Restore reopens the
resource through the host and exposes failure explicitly.

## Minimal Durable State Selection

### Persist Authority, Never Convenience

The compiler emits persistence metadata only for values whose history cannot be
reconstructed from source and other durable state:

- root scalar `HOLD`/state cells inside `PERSIST`;
- indexed `HOLD` columns inside durable list rows;
- mutable list membership and order;
- hidden list key allocator state and row generations;
- raw constructor/input fields required to reconstruct dynamic rows;
- durable source-event deduplication cursors only when the deployment protocol
  can replay those events;
- explicit migration metadata and completion state.

Never persist:

- pure or aggregate derived fields;
- demand-current caches;
- formula dependency graphs or evaluation stacks;
- lookup indexes;
- dirty/currentness flags;
- source bindings rebuilt from live rows;
- document, scene, layout, retained render state, glyph caches, GPU resources,
  proof/readback state, or performance reports;
- parser AST, IR, MachinePlan, operation schedule, or function closures;
- unbounded debugging history.

### Touched Overrides

A conceptual state cell does not require a stored record until it has acquired
user/application-owned history.

For static/root initialization:

1. Compile and evaluate the current default.
2. If no durable update ever committed, store no override.
3. On first durable update, write a touched override.
4. Keep that override even if its value currently equals the default.
5. Remove it only through an explicit reset-to-default operation.

Equality with the current default is not enough to delete an override. The user
may intentionally have stored `0`; if a later code version changes the default
to `5`, restoring `5` would silently rewrite user intent.

### Cells Example As A Generic Acceptance Fixture

For a 2,600-cell logical sheet:

- `address` is deterministic and never stored;
- default formulas are regenerated from source;
- untouched formula/editing state creates no durable row record;
- edited `formula_text`/`editing_text` are sparse touched overrides;
- `value` and `error` are derived and never stored;
- formula dependencies and indexes are rebuilt;
- editing one cell should ordinarily add one or a small bounded number of
  durable records, not 2,600 records.

This behavior must emerge from generic storage provenance and list semantics.

### Static And Dynamic Lists

An untouched static list/range is regenerated from the current plan. It stores
no membership snapshot.

Once a static list is structurally changed, membership becomes authoritative.
The first implementation stores complete current membership/order for that list
plus sparse row-state overrides. This is less clever and safer across code
changes than trying to merge an arbitrary new initializer with old structural
deltas.

A dynamic list stores:

- list durable key;
- current ordered row keys;
- each row generation;
- monotonic `next_key`/allocation state;
- raw inserted-row constructor fields needed for reconstruction;
- touched indexed-state overrides.

An empty list can still require durable metadata after mutation. For example,
append then remove must not resurrect initial rows or reuse a stale key after
restart.

## Stable Identity And Persistence Schema

### Application Identity

The host/project manifest supplies a stable `application_id`. It is not derived
from example name, source path, binary hash, process ID, or current program hash.

Built-in examples receive versioned manifest IDs. Custom playground projects
receive a generated ID stored in their versioned/custom manifest. Production
applications use their deployment/package identity.

### StorageKey

Each durable cell/list receives a compiler-owned identity:

```text
StorageKey {
    format_version
    application_id
    canonical_module_path
    named_lexical_binding_path
    storage_kind
    hold_or_list_name
    row_field_path, if indexed
    sha256_of_the_canonical_identity
}
```

The store keeps both the readable canonical identity and full SHA-256 digest.
The digest is the compact lookup key; the readable path detects impossible-but-
unsafe collisions and supports the inspector.

Do not include source spans, line numbers, parser order, current defaults,
derived expressions, type fingerprints, or program hashes in identity. Those
change for ordinary compatible edits.

### Type Fingerprint

Every storage key has a separate canonical type fingerprint. Restore requires:

- matching storage kind;
- compatible Boon value type;
- compatible scope/list ownership;
- compatible codec/schema version.

A mismatch remains preserved as incompatible/orphaned data until explicit
migration or reset. The runtime must never reinterpret bytes under a new type.

### PersistencePlan

Add a `PersistencePlan` to `MachinePlan` beside `StorageLayout`:

```rust
pub struct PersistencePlan {
    pub format_version: u32,
    pub application_schema_id: String,
    pub schema_hash: [u8; 32],
    pub slots: Vec<DurableSlotPlan>,
    pub lists: Vec<DurableListPlan>,
    pub migrations: Vec<MigrationPlan>,
}

pub struct DurableSlotPlan {
    pub runtime_storage_id: PlanStorageId,
    pub key: StorageKey,
    pub value_type: PlanValueType,
    pub type_fingerprint: [u8; 32],
    pub role: StateRole,
    pub initial_provenance: InitialProvenance,
}
```

Runtime numeric IDs remain fast local indexes. `PersistencePlan` is the only
mapping between local storage and durable identity.

The persistence schema hash covers durable keys, kinds, types, list ownership,
and migration declarations. It does not cover the document tree, renderer,
derived graph, constants unrelated to durable defaults, or source formatting.

## Runtime Initialization And Restore

Split `Session` construction into explicit phases:

1. **Validate plan**: plan version, persistence metadata, key uniqueness, type
   fingerprints, and migration graph.
2. **Allocate raw storage**: scalar columns, list containers, and static row
   identities without demanding derived fields.
3. **Evaluate defaults**: initialize values that have no restored override.
4. **Load restore image**: map durable keys to current runtime IDs and install
   compatible scalar overrides, list structures, rows, generations, and
   allocator state.
5. **Execute migrations**: atomically transfer old storage ownership and record
   completion.
6. **Rebuild runtime-only structures**: indexes, source bindings, dependency
   state, and caches.
7. **Mark derived values dirty**: restored authority invalidates every relevant
   derived cache.
8. **Demand outputs**: run currentness barriers for active named output roots.
9. **Publish first frame/output**: no default-derived frame may escape before
   restore finishes or visibly fails.

Introduce a builder rather than adding optional restore parameters throughout
`Session::new`:

```rust
SessionBuilder::new(plan)
    .with_restore_image(restore)
    .build()
```

The restore image contains only canonical durable state. It is not a full
runtime graph snapshot.

## Delta-Native Persistence

The semantic delta layer is canonical. Persistence lowering filters each
committed `RuntimeTurn` through `PersistencePlan`:

```text
RuntimeTurn
  -> keep authoritative durable State/List/Row changes
  -> map local IDs to StorageKey
  -> coalesce safely inside the same turn
  -> DurableCommitBatch(base_epoch, next_epoch, changes)
  -> background store transaction
```

Do not write all `Delta::SetValue` entries. Derived field deltas are renderer or
debug facts, not durable state. Source bind/unbind deltas are rebuilt and are
not durable.

One Boon turn maps to one atomic durable transaction. Either all state/list
changes from the turn commit or none do.

### Durable Commit Shape

```rust
pub struct DurableCommitBatch {
    pub application_id: ApplicationId,
    pub schema_hash: [u8; 32],
    pub base_epoch: u64,
    pub next_epoch: u64,
    pub changes: Vec<DurableChange>,
    pub checksum: [u8; 32],
}

pub enum DurableChange {
    SetOverride { key: StorageKey, scope: ScopeKey, value: StoredValue },
    ResetOverride { key: StorageKey, scope: ScopeKey },
    ReplaceListStructure { key: StorageKey, rows: Vec<StoredRow> },
    InsertRow { list: StorageKey, row: StoredRow, position: u64 },
    RemoveRow { list: StorageKey, key: u64, generation: u64 },
    MoveRow { list: StorageKey, key: u64, generation: u64, position: u64 },
    CompleteMigration { migration_id: MigrationId },
}
```

Persistence epochs are durable-store epochs, not frame sequence numbers. They
must advance monotonically and reject stale/repeated commit application.

## Durable Store Boundary

Core runtime code must not depend directly on redb, IndexedDB, filesystem APIs,
threads, futures executors, or browser types.

Use a small host-owned interface:

```rust
pub trait DurableStateStore: Send + 'static {
    fn load(&mut self, request: RestoreRequest) -> Result<RestoreImage, StoreError>;
    fn commit(&mut self, batch: DurableCommitBatch) -> Result<CommitAck, StoreError>;
    fn flush(&mut self) -> Result<FlushAck, StoreError>;
    fn inspect(&mut self, request: InspectRequest) -> Result<StoreSummary, StoreError>;
    fn compact(&mut self, request: CompactRequest) -> Result<CompactAck, StoreError>;
}
```

The runtime communicates with a `PersistenceCoordinator`, not the backend:

```text
Session commit
  -> bounded coordinator queue
  -> dedicated native thread or browser worker
  -> DurableStateStore
  -> CommitAck / failure status
  -> cached inspector snapshot
```

Do not use `async_trait` in the compiler/runtime core. Native redb is
synchronous on its dedicated thread. Browser IndexedDB is asynchronous inside a
worker, but messages crossing the coordinator boundary have the same request and
acknowledgement shapes.

## Native redb Backend

Use `redb` tables with byte keys and explicitly encoded values:

```text
META       singleton metadata, format, app id, schema, epoch, clean shutdown
SLOTS      StorageKey + ScopeKey -> StoredSlot
LISTS      StorageKey + parent scope -> StoredListMetadata
ROWS       StorageKey + parent scope + row key + generation -> StoredRow
JOURNAL    durable epoch -> DurableCommitBatch
MIGRATIONS migration id -> completion record
BLOBS      content digest -> bounded large value/blob metadata
```

The write worker applies a commit in one `redb::WriteTransaction`:

1. Read and validate `META.current_epoch`.
2. Reject mismatched schema or stale `base_epoch`.
3. Insert the journal record.
4. Apply slot/list/row materialized-state changes.
5. Record migration completion.
6. Advance metadata to `next_epoch`.
7. Commit once.
8. Send `CommitAck` only after `redb` returns success.

The materialized tables provide bounded startup. The journal is retained only
within a configured debugging/recovery window and compacted after a verified
materialized checkpoint. Normal restore does not replay an unbounded history.

### redb Durability Modes

Expose host/deployment policy, not Boon syntax:

- **Immediate**: `redb::Durability::Immediate`; the commit acknowledgement means
  the transaction is persistent. Use for explicit saves, shutdown barriers,
  migrations, and externally consequential operations.
- **Buffered**: commit/coalesce pending local UI turns on the worker, followed by
  a bounded periodic Immediate barrier. The inspector must display pending and
  last durable epochs. A clean shutdown performs an Immediate flush.

The default desktop playground policy is Buffered for interaction latency, with
a short bounded flush interval. Product state becomes visible from the
in-memory Session immediately. The product frame never waits for redb. A strict
deployment can require an Immediate acknowledgement before executing an
external irreversible effect.

The queue may be bounded, but it may not drop durable commits. It may safely
coalesce scalar overrides only while preserving commit order, list structure,
migration state, and the final epoch. If it cannot coalesce, it applies
backpressure at a runtime turn boundary and reports the stall. It never performs
storage work from a render hook.

Evaluate `WriteTransaction::set_quick_repair(true)` in the redb backend spike.
The final native setting must be selected by measured commit and forced-crash
recovery results. Startup recovery has a hard budget, so accepting very slow
full-file repair is not a valid silent default for large stores.

Compaction runs only on explicit maintenance or bounded size/obsolete-page
thresholds. It is never triggered synchronously from source input or rendering.

### Native Storage Location

Production applications use the platform application-data directory under
their stable application ID.

The playground uses a repository-local ignored directory, for example:

```text
playground/state/<application-id>/state.redb
```

Built-in example source remains versioned. Runtime database files, journals,
exports, and fault-test files remain ignored.

## Browser And Wasm Backend

Implement `IndexedDbDurableStateStore` in Rust using `rexie` and IndexedDB
object stores corresponding to the logical native tables:

```text
meta
slots
lists
rows
journal
migrations
blobs
```

One Boon turn is one IndexedDB read-write transaction across all affected object
stores. The browser worker returns an acknowledgement only after transaction
completion.

The WebAssembly UI/main thread must never synchronously encode a full snapshot
or wait on storage. It sends bounded durable batches to the worker and consumes
acknowledgements/status updates.

Browser storage is normally best-effort and can be quota-limited or evicted.
The host requests persistent storage through the browser Storage API and reports
whether it was granted. Quota failure, transaction abort, private browsing, and
version-change blocking are explicit runtime/dev statuses.

Do not use LocalStorage. It is synchronous, small, string-only, and unsuitable
for atomic list/row commits.

## Durable Codec

Define Boon-owned storage DTOs separate from runtime `Value`, `RowId`, and
MachinePlan types.

Use `minicbor` with:

- numeric field and variant tags that are never reused;
- a top-level format version;
- optional fields for compatible additions;
- canonical map/list ordering;
- explicit maximum lengths and nesting depth;
- SHA-256 checksums for commit/checkpoint envelopes;
- golden fixtures from every released format version;
- no direct derive on internal runtime structs whose layout may change.

`StoredValue` initially supports only serializable Boon data:

```text
Null
Tag/Bool
signed Number
Text
Bytes
List of StoredValue
Record with canonical field ordering
```

Runtime-only `MappedRow`, `Row`, error stacks, handles, sources, and render
objects are rejected or converted by an explicit durable descriptor type. They
are never encoded accidentally.

Large authoritative `BYTES` values use a bounded threshold. Small bytes remain
inline. Large values are content-addressed by SHA-256 in `BLOBS`, with reference
counts/marking derived from durable roots. Derived assets and source files stay
outside the state database.

## Hot Reload And Schema Evolution

### Compatible Reload

Hot reload uses the same stable persistence schema as cold restore:

1. Compile the new plan.
2. Export current authoritative Durable and Session values by stable key.
3. Match unchanged keys and compatible types.
4. Apply declared migrations.
5. Build a new Session through the restore barrier.
6. Recompute derived outputs.
7. Atomically replace the active runtime only after success.

Session values use stable keys in memory but are not written to the durable
backend. Transient values are deliberately omitted.

### DRAIN And DRAINING

Renames, moves, splits, merges, and incompatible type changes require explicit
state evolution. The intended language model is the paired
`DRAIN`/`DRAINING` design:

```boon
counter: 0 |> HOLD value { ... } |> DRAINING
click_count: DRAIN { counter } |> HOLD value { ... }
```

Required invariants:

- every `DRAINING` source has exactly one `DRAIN` destination;
- every `DRAIN` references a valid draining source;
- no double drain, conditional drain, cycle, or use-after-draining;
- destination identity is the durable identity after migration;
- source bytes remain preserved until the destination transaction commits;
- migration completion is durable and idempotent;
- removing migration markers while transfer is incomplete is rejected;
- conversion is explicit for type changes;
- unknown/orphaned data is retained until migration or explicit deletion.

Until `DRAIN` is implemented, compatible stable-key restores may proceed, but
renames/type mismatches must produce visible diagnostics and preserve old data.
They must not silently reset or guess by source similarity.

`FLUSH` remains a separate fail-fast/bypass concept. It is not state migration.
`UNPLUGGED` remains structural-absence fallback, not ownership transfer.

## Failure And Recovery Semantics

Persistence failure must be honest and non-destructive.

- Corrupt or incompatible data remains on disk and is reported.
- A failed restore does not silently start from defaults and overwrite the old
  database.
- A failed commit leaves the previous durable epoch authoritative.
- Partial Boon turns are impossible because one turn is one backend transaction.
- Repeated commit batches are detected by epoch/checksum and are idempotent or
  rejected deterministically.
- A stale schema writer cannot update a newer store.
- Storage queue saturation is observable and never resolved by dropping list or
  migration changes.
- Clean shutdown requests an Immediate flush with a bounded timeout and reports
  failure.
- Forced process termination can lose only commits explicitly reported as
  pending under Buffered mode.
- Power/storage corruption tests preserve the last acknowledged Immediate
  epoch.
- Restore rebuilds all derived indexes and bindings instead of trusting stored
  cache state.

## Document, Scene, And Root Outputs

### Output Ports, Not Stored Variables

`document` and `scene` are named reactive output ports. They are not application
state and are never part of `PERSIST`.

Boon source may call a normal pure view function:

```boon
document: app_view(PASS: [state: state, ui: ui])
```

The compiler lowers this output into a typed document/scene plan. The runtime
owns retained element identity, active/pending document state, layout, text
resources, GPU resources, hit testing, and patch application. Boon functions
produce structural descriptions and bindings, not host-owned widget objects.

### Generalize The Root Registry

The current semantic index recognizes `document` and `scene`, while
`MachinePlan` has a dedicated optional `document` field. Generalize this into a
typed named output-root registry:

```text
outputs: Vec<OutputRootPlan>

OutputRootPlan {
    name
    contract kind
    value target/document plan
    demand policy
}
```

Hosts demand only the roots they own. Future roots may include UI, server
routes, scheduled tasks, build results, network endpoints, or hardware pins.

### No Mandatory main Function

Do not add an imperative mandatory `main()`. A single main return value would
hide independent reactive outputs and introduce unnecessary root unpacking.

`BLOCK` continues to return its final expression/value. A named root may use a
`BLOCK` to calculate and return its value, but the root registry defines host
outputs. Persistence roots remain orthogonal to output roots.

## Dev Window Persistence UX

### Editor And Inspector Roles

Use restrained familiar icons in the editor gutter and inspector:

- database icon: Durable state;
- memory/history icon: Session state;
- short-lived indicator: Transient state;
- function/derived indicator: Derived, not stored;
- warning icon: pending, failed, orphaned, or incompatible state.

When the caret or inspector targets a value, show:

```text
Symbol          store.count
Type            Number
Role            Durable state
Current         42
Last persisted  42
Default         0
Status          Saved at epoch 18, 24 ms ago
Backend         redb, Buffered
Storage key     app/state/store/count
Encoded size    7 bytes
Last writer     increment_button.press, sequence 27
```

For a derived value:

```text
Role            Derived, not stored
Current         3
Reconstructed   from store.todos.completed
Reason          Pure aggregate with currentness barrier
```

For a sparse indexed value:

```text
Logical rows    2,600
Touched rows    1
Stored values   1
Current row     A2 / hidden key 27 / generation 1
```

### Persistence Overview

Add a Persistence view grouped by durable model/list:

- application ID and schema hash;
- backend and durability policy;
- current runtime epoch, queued epoch, and last durable epoch;
- durable roots, scalar overrides, touched rows, dynamic rows, and byte size;
- pending commit count and oldest pending age;
- last commit/flush duration;
- journal and blob sizes;
- persistent browser permission/quota status;
- matched, new, orphaned, incompatible, and migration-pending keys;
- last error without truncating the actionable reason.

Controls use icon buttons with tooltips:

- flush now;
- export durable state;
- import/inspect an export;
- clear/reset selected durable root;
- clear all state with confirmation;
- run compaction/maintenance explicitly.

Do not dump thousands of rows into the normal sidebar. Use filtering,
pagination/virtualization, and bounded samples.

### Migration View

On code replacement, show a migration summary before committing destructive
changes:

```text
Matched             14
New                  2
Draining             1
Orphaned             1
Type incompatible    0
```

Selecting an entry shows old key/type/value summary, destination, conversion,
completion epoch, and whether old data is still retained.

### Performance Boundary

The dev window consumes a cached `PersistenceInspectorSnapshot` sent on state or
acknowledgement changes. It never opens redb, scans IndexedDB, serializes a full
snapshot, performs IPC, or waits for the storage worker from a render hook.

Persistence timings are reported separately:

```text
enqueue_us
encode_us
queue_age_ms
commit_ms
flush_ms
restore_ms
rebuild_derived_ms
stored_bytes
pending_batches
```

## Implementation Phases

### Phase 1: Language And Plan Metadata

- Finalize `PERSIST`/`TRANSIENT` syntax without making backend names language
  concepts.
- Add state-role classification to parser semantic metadata and typechecking.
- Reject unsupported/sensitive durable values.
- Add stable `StorageKey`, type fingerprints, and `PersistencePlan` to IR and
  MachinePlan.
- Add uniqueness, determinism, and compatibility verification.
- Add inspector metadata before any disk backend.

Exit condition: Counter, TodoMVC, Cells, and generic fixtures compile to a
deterministic persistence plan whose durable slots exclude all derived fields.

### Phase 2: Restore-Aware Session Construction

- Split raw initialization from derived publication.
- Add `SessionBuilder`, `RestoreImage`, and state-role-aware export.
- Add deterministic in-memory backend and restart harness.
- Restore root/indexed state and list identity before currentness barriers.
- Rename the existing broad snapshot to `DebugSnapshot`.

Exit condition: deterministic process-local cold-restart tests restore state
without a file/database and derived outputs are recomputed, not copied.

### Phase 3: Delta Lowering And Persistence Coordinator

- Lower committed semantic deltas through `PersistencePlan`.
- Filter derived/render/source-binding deltas.
- Add atomic durable epochs and checksums.
- Add bounded worker protocol, acknowledgements, errors, flush, and shutdown.
- Prove no product render hook or interaction frame performs storage work.

Exit condition: the in-memory backend receives exactly the authoritative
changes for each Boon turn and fault injection cannot expose partial state.

### Phase 4: Native redb Backend

- Add target-gated `redb` dependency and Boon-owned backend crate/module.
- Implement tables, metadata, materialized state, bounded journal, migrations,
  blobs, repair reporting, and compaction.
- Implement Immediate and Buffered policies.
- Add repository-local ignored playground state location.
- Benchmark queueing, commit, flush, restore, repair, and file growth.

Exit condition: native Counter/TodoMVC restart tests, forced-crash tests, and
schema mismatch tests pass with the last acknowledged epoch intact.

### Phase 5: Sparse Lists And Large Logical Models

- Track untouched versus touched scalar/indexed state provenance.
- Store sparse row overrides.
- Store dynamic list structure, generations, order, and allocators.
- Handle empty-but-mutated and static-then-mutated lists.
- Add bounded blob policy.

Exit condition: editing one Cells formula stores bounded sparse state while all
2,600 logical cells and derived formulas restore correctly.

### Phase 6: Compatible Hot Reload And Migration

- Transfer Durable and Session state through stable keys.
- Drop Transient state.
- Add compatibility diagnostics and atomic runtime replacement.
- Implement `DRAIN`/`DRAINING` analysis, transfer, completion, and finalization.
- Retain orphaned/incompatible bytes until explicit action.

Exit condition: whitespace/reordering/view changes preserve state; rename,
move, split, merge, and type conversion scenarios require and correctly execute
explicit migrations.

### Phase 7: Browser/Wasm Backend

- Implement the Rust `rexie`/IndexedDB worker backend.
- Match native logical tables and transaction semantics.
- Request/report persistent browser storage.
- Handle quota, eviction status, private browsing, blocked upgrades, aborts, and
  clean shutdown limitations.
- Run the same backend contract fixtures in Wasm.

Exit condition: browser cold restart and schema evolution match native semantic
results without LocalStorage or handwritten JavaScript persistence code.

### Phase 8: Dev Window UX

- Extend language hints and runtime inspect responses with state roles.
- Add current/default/persisted/pending values and storage identity.
- Add virtualized Persistence and Migration views.
- Add clear, flush, export/import, and maintenance commands.
- Ensure all UI reads cached snapshots only.

Exit condition: an operator can determine exactly what is stored, why it is
stored, how much is stored, whether it is durable, and why a migration failed
without inspecting database files manually.

### Phase 9: Generic Output Root Ownership

- Replace the plan's dedicated output assumption with `OutputRootPlan` registry.
- Keep `document` and `scene` as typed named roots.
- Keep runtime-owned retained output resources outside persistence.
- Add a non-UI root fixture proving no mandatory `main()` is required.

Exit condition: multiple named roots can coexist, hosts demand selected roots,
and no output object appears in a durable state record.

## Required Verification

### Compiler And Identity

- Stable keys survive whitespace, comments, unrelated declarations, and sibling
  reordering.
- Renames and moves do not silently reuse state.
- Numeric `StateId`/`ListId`/`FieldId` values never appear in durable keys.
- Type changes fail compatibility unless explicitly migrated.
- Persistence schema hash ignores render-only and pure-derived changes.
- Duplicate canonical identities fail compilation.

### Runtime And Currentness

- Restored state is installed before any demanded derived read or first output.
- Derived values are absent from stored records and recompute after restore.
- Currentness barriers prevent stale values after restore or migration.
- Indexes, dynamic dependencies, and source bindings are rebuilt.
- Cycles remain protected by the default evaluation stack.

### Minimal State

- Fresh Counter has zero overrides; one increment creates one scalar override.
- Reset-to-default explicitly removes the override.
- Cells startup stores no `value`/`error` and no untouched row state.
- Editing one cell stores only edited authority and bounded list metadata.
- TodoMVC preserves dynamic row order, keys, generations, titles, and completion
  state across restart.
- Removing every row preserves authoritative empty-list and allocator state.

### Atomicity And Recovery

- Fault before commit changes nothing durable.
- Fault during backend transaction restores the previous acknowledged epoch.
- Fault after commit acknowledgement restores the acknowledged epoch.
- Duplicate/stale batches cannot corrupt or double-apply state.
- Corrupt values and incompatible schemas remain preserved and visible.
- Buffered mode reports exactly which epochs are pending.
- Immediate flush semantics match `redb::Durability::Immediate`.

### Backend Parity

- In-memory, redb, and IndexedDB backends consume the same golden commit batches
  and produce semantically identical restore images.
- Versioned `minicbor` golden files remain readable after codec changes.
- Browser quota/abort and native I/O/repair failures map to the same public
  status categories.

### Performance

- Persistence disabled adds no hot-path branches beyond a predictable role
  check.
- Persistence enqueue/encoding does not perform file I/O or IndexedDB work on
  the product thread.
- Normal visible interactions retain the existing 16.7 ms p95 target with
  persistence enabled.
- Product input-to-present excludes asynchronous durable acknowledgement but
  reports queue/commit latency separately.
- No full state, list, document, or JSON snapshot is created per turn.
- Storage queue never drops durable list/migration changes.
- Restore and forced-crash repair have explicit release budgets and bounded
  report evidence.

### Dev Window

- Durable, Session, Transient, and Derived roles are visually distinct.
- Current and last persisted values update after runtime turns and store acks.
- Pending/failure/orphan/migration states are visible.
- Large lists are virtualized and do not freeze the dev window.
- The dev render hook performs zero store queries and zero IPC requests.

### Output Roots

- `document` and `scene` are never durable records.
- View-only source changes do not invalidate compatible durable state.
- Runtime retains and patches output resources after state restore.
- Multiple output roots work without a mandatory `main()`.

## Acceptance Criteria

The persistence goal is complete only when all of the following are true:

- `PERSIST` and `TRANSIENT` have documented, typechecked semantics.
- MachinePlan carries stable, deterministic persistence metadata.
- Session restore occurs before derived currentness/output publication.
- Only authoritative scalar/list/row state is stored.
- Sparse untouched state is proven on Cells-sized logical models.
- Native persistence uses redb and no C/SQLite dependency.
- Browser persistence is implemented in Rust over IndexedDB and uses no
  LocalStorage or handwritten JavaScript storage layer.
- One Boon turn is one atomic durable transaction.
- Immediate and Buffered durability are honest and visible.
- Compatible hot reload and cold restart share the same identity/migration
  rules.
- DRAIN/DRAINING migrations are cycle-safe, atomic, idempotent, and visible.
- Corrupt or incompatible state is preserved rather than silently reset.
- The dev window explains what is stored, why, where, how much, and at which
  durable epoch.
- Normal render/input frames contain no database or proof work and retain their
  performance budgets.
- `document`, `scene`, retained elements, layout, and GPU resources remain
  runtime-owned outputs, never stored application variables.
- Deterministic native, Wasm, restart, crash, migration, sparse-state,
  currentness, and visual/functional dev-window gates all pass from fresh
  artifacts.

## Source References

Current repository contracts and implementation:

- [`../architecture/RUNTIME_MODEL.md`](../architecture/RUNTIME_MODEL.md)
- [`../architecture/DELTA_PROTOCOL.md`](../architecture/DELTA_PROTOCOL.md)
- [`../architecture/LANGUAGE_SEMANTICS.md`](../architecture/LANGUAGE_SEMANTICS.md)
- [`../../crates/boon_plan_executor/src/session.rs`](../../crates/boon_plan_executor/src/session.rs)
- [`../../crates/boon_plan/src/lib.rs`](../../crates/boon_plan/src/lib.rs)
- [`../../crates/boon_ir/src/lib.rs`](../../crates/boon_ir/src/lib.rs)
- [`../../crates/boon_native_playground/src/dev.rs`](../../crates/boon_native_playground/src/dev.rs)
- [`../../crates/boon_native_playground/src/ui.rs`](../../crates/boon_native_playground/src/ui.rs)

External Rust/storage references:

- [redb repository and status](https://github.com/cberner/redb)
- [redb crate documentation](https://docs.rs/redb)
- [redb durability](https://docs.rs/redb/latest/redb/enum.Durability.html)
- [redb custom StorageBackend](https://docs.rs/redb/latest/redb/trait.StorageBackend.html)
- [Turso Database repository and beta status](https://github.com/tursodatabase/turso)
- [rexie IndexedDB wrapper](https://docs.rs/rexie)
- [minicbor](https://docs.rs/minicbor)
- [IndexedDB transactions](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB)
- [Browser storage persistence, quotas, and eviction](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria)
