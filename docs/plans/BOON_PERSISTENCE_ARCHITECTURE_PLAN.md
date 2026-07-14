# Boon Persistence Architecture Plan

Date: 2026-07-13

Status: implemented architecture and canonical acceptance contract. Durable
semantic memory, restore, migration, effects, native redb, browser IndexedDB,
and development tooling are present in the worktree. Completion is determined
by fresh tests and the manifest-backed native handoff reports, not by a second
hand-maintained pass list in this document.

The language decision is recorded and implemented: authoritative memory is
durable by default, and `DRAIN` / `DRAINING` is the explicit state-evolution
syntax. There are no persistence lifetime keywords.

This plan replaces the earlier opt-in persistence-boundary design. There is no
second active persistence plan.

## Implementation Map

The implementation is split along existing ownership boundaries:

- `boon_ir` and `boon_compiler` produce MachinePlan v3 semantic memory,
  recursive type fingerprints, stable storage identities, output roots,
  migration edges, and effect contracts.
- `boon_plan_executor::Session` owns prepare/commit/settle, sparse authority
  deltas, restore-before-publication, demand currentness, list identity, and
  cycle-safe dependency evaluation.
- `boon_persistence` owns the versioned canonical CBOR protocol, deterministic
  in-memory backend, redb backend, large-blob lifecycle, export/import, repair,
  compaction, and bounded checkpoint worker.
- `boon_runtime` owns durable activation, migration sequencing, typed effects,
  outbox dispatch/reconciliation, browser Rexie/IndexedDB integration, and
  host-vault references for sensitive inputs.
- The native playground exposes authority, durable/pending state, migrations,
  outbox state, clear/start-over, import/export preview, and persistence timing
  without querying storage from render hooks.
- Versioned Counter and Todo migration fixtures exercise incremental and
  skipped-version activation, `DRAINING` finalization, list/row identity, and
  deterministic TEST playback.

The implementation remains generic: no parser, compiler, runtime,
persistence, document, renderer, playground, or verifier behavior is selected
by an example name. Native product evidence is governed solely by
`docs/architecture/native_gpu_handoff_manifest.json`.

## Executive Decision

Boon persistence follows one simple language rule:

> Every compiler-created authoritative application memory node is durable by
> default. Values that are not authoritative memory are supplied again or
> reconstructed.

Persistence is attached to the semantic memory plan after lowering. It is not
attached to `HOLD`, `LATEST`, variable-name conventions, source layout, or an
author-written persistence block.

The implementation uses a Rust-only stack:

- native storage: [`redb`](https://github.com/cberner/redb);
- browser/Wasm storage: Rust using [`rexie`](https://docs.rs/rexie) over
  IndexedDB;
- durable codec: [`minicbor`](https://docs.rs/minicbor) with an explicitly
  versioned Boon-owned schema;
- test storage: a deterministic in-memory Rust driver with fault injection;
- hashing and content identity: SHA-256;
- no SQLite C library, `rusqlite`, LocalStorage, handwritten JavaScript storage
  implementation, JSON product-state path, or Python tooling.

`redb` is selected because Boon needs a compact transactional key-value store,
not a SQL compatibility layer. It is pure Rust, stable, maintained, ACID, and
supports explicit durability, savepoints, repair, and custom storage backends.
Turso remains interesting for a future SQL-facing product but is not the first
runtime-state backend.

The browser implementation remains Rust application code. IndexedDB is a host
platform service, like files and WGPU. The shared persistence protocol prevents
native and browser backend mechanics from changing Boon semantics.

## Product Contract

A Boon application should be able to stop, restart, refresh, load compatible
new code, and continue from its latest acknowledged authoritative state without
serializing the full dataflow graph.

The architecture must preserve these properties:

- The compiler owns the static graph and semantic memory metadata.
- `Session` remains the sole runtime execution owner.
- A Boon turn is prepared and committed atomically in memory.
- Derived data is recomputed through explicit currentness barriers.
- Mutable lists retain hidden row keys, generations, order, and allocator
  state.
- Restore completes before any observable output is published.
- A schema migration or deletion does not mutate the durable store until a
  candidate Session has restored and settled successfully.
- Rendering remains retained and independent from durable storage.
- Interactive frames never perform database, full-state encoding, proof, or
  report work.
- Buffered persistence may coalesce complete turns off-thread.
- Critical external effects cannot escape before their required durable
  barriers.
- Persistence remains generic. Parser, compiler, runtime, document, renderer,
  playground, and verifier code may not branch on an example name.

This contract applies equally to native, browser/Wasm, server, mobile, and
future deployment targets. A target can provide different storage mechanics or
durability guarantees, but not different Boon state semantics.

## Baseline Before Implementation

This section records the gaps that motivated the implementation. It is kept as
design rationale; the implementation map above describes the current worktree.

### In-Memory Execution Exists

`boon_plan_executor::Session` currently owns:

- root state cells;
- stateful expressions lowered from selected `HOLD`, `LATEST`, and standard
  operators;
- logical lists, row order, hidden row keys, and generations;
- indexed row state;
- derived fields and currentness;
- indexes and dynamic dependencies;
- source sequence and binding identity;
- document/output demand.

This is the right place to own execution. It is not yet a safe durable-state
boundary.

### Runtime Values Need A Storage DTO Boundary

The current runtime `Value` enum contains ordinary language data and internal
row representations. Persistence must not derive its file schema directly from
that implementation enum.

Define a Boon-owned recursive `StoredValue` format for language-level data and
separate storage DTOs for list/row authority. Internal row views, runtime IDs,
evaluation stacks, source bindings, document objects, and errors internal to
the engine are never serialized as runtime snapshots.

All language-level data must have a deterministic encoding. The existence of a
codec does not mean every computed value is written. Only authoritative memory
uses it.

### State Discovery Is Too Syntax-Driven

Current parser/IR paths identify memory through special cases such as `HOLD`,
selected stateful `LATEST` shapes, and stateful standard operators. Persistence
cannot duplicate that syntax list.

The compiler must first lower all stateful constructs into generic semantic
memory nodes. Persistence, inspection, hot reload, and migration consume those
nodes. Adding a future stateful standard operator must not require another
persistence-specific branch.

### Durable Identity Does Not Exist

Current `StateId`, `ListId`, `FieldId`, and `PlanStorageId` values are local
execution indexes. Anonymous state discovery can also depend on source line
numbers. Neither is durable identity.

Whitespace, comments, formatting, unrelated declaration insertion, and sibling
reordering must not change a memory identity. Rename, move, and incompatible
type change must be explicit migrations rather than accidental key reuse.

### Turn Failure Can Leave Partial Mutation

The current execution path can mutate state before all fallible currentness,
document, or output work has completed. Persistence would make that ambiguity
permanent.

Runtime execution must gain prepare, commit, and settle phases. No fallible
operation after commit may retroactively invalidate the committed authority.
Preparation failure publishes no authority delta and changes no current state.

### Restore Has No Publication Barrier

Startup currently constructs defaults and can demand derived values immediately.
A durable runtime needs an explicit restore-aware builder. No default-derived
frame, server output, source subscription, or effect may escape before restored
authority and migrations are installed.

### Mutable Runtime Caching Can Fork State

Any cache that stores a mutable runtime/session instance across source
replacement can create competing state owners. Cache immutable compiler
artifacts only. Hot reload constructs one candidate Session, restores into it,
settles it, and swaps it atomically after success.

### Playground Identity Needs To Be Stable

Sequential labels such as `custom-1` are not sufficient application identity
because they may be reused. Built-in examples, custom examples, production
packages, and test runs require explicit stable application and namespace
identities supplied by manifests or host launch configuration.

## Language Semantics

### Authoritative Memory Is The Boundary

The compiler classifies graph nodes into:

1. **Authoritative memory**: history that cannot be reconstructed from current
   source, current inputs, and other authoritative memory.
2. **Derived data**: pure or demand-current values reconstructed from authority
   and inputs.
3. **Source data**: current host/environment input events or continuous values.
4. **Output data**: document, scene, route, response, build, hardware, or other
   host-demanded roots reconstructed from the graph.
5. **Runtime machinery**: indexes, dependency edges, dirty flags, caches,
   scheduling state, host handles, rendering resources, and instrumentation.

Every authoritative memory node is durable. The other categories are not
durable records because they do not own application history.

This is deliberately independent from surface syntax:

```boon
count:
    0 |> HOLD count {
        increment |> THEN { count + 1 }
    }
```

and:

```boon
count:
    LATEST {
        0
        increment |> THEN { count + 1 }
    }
```

both lower to authoritative memory. Mutable `LIST` ownership and stateful
standard combinators also lower to memory. `HOLD` is not a persistence marker.

`HOLD` should remain a general feedback/memory primitive and may become less
common as reusable loops move into generic standard operators. Persistence must
not make `HOLD` more special than it already is.

### No Persistence Lifetime Keywords

Do not add `PERSIST`, `TRANSIENT`, `SESSION`, `RESET`, or `DEFAULT` syntax.

- Durability is the default for authoritative memory.
- Host/test configuration may run the entire application against an in-memory
  store without changing source semantics.
- Browser, desktop, server, workspace, user, and tenant scope are host storage
  namespaces, not lexical persistence blocks.
- If a real future use case requires mixed lifetime authoritative memory, it
  must receive a precise ownership/lifetime design. It must not be smuggled in
  as an ambiguous generic transient block.

### Boon Values Remain Data

Language-level Boon values are data. Host-native objects do not enter the graph
as opaque language values.

Examples:

- an open file is represented by an application-relative path, content digest,
  or host-issued descriptor plus explicit open status;
- a network connection is represented by configuration and a status `SOURCE`;
- a timer is represented by duration/deadline data plus emitted source ticks;
- a WGPU resource remains renderer-owned while Boon produces document/scene
  data;
- a credential is represented by a host credential reference, not token text.

Derived document or scene values are still data, but they are not durable
because they are reconstructed. Serialization capability and persistence
authority are separate concepts.

### Sources And Deliberate Retention

`SOURCE` declares current input. Event presence, pointer position, hover,
pressed buttons, focus, IME composition, network connection state, and timer
ticks are not persisted as source events.

If application code deliberately copies source data into semantic memory, that
copy becomes durable. The compiler must not infer exceptions based on source
provenance or variable names.

This supports exact logical resume without storing host machinery:

- a selected cell, text draft, logical edit mode, selected map item, or cursor
  time may be retained deliberately;
- a live pointer capture, compositor focus token, active socket, abort handle,
  or timer handle is rebuilt by the host;
- a remembered active-watch descriptor may persist while the WebSocket and
  subscription are recreated.

### Defaults And Touched Authority

Untouched reconstructable program defaults do not require stored records.

For every scalar or indexed memory node:

1. Evaluate the current program default.
2. If no authoritative write has occurred, store no override.
3. On the first authoritative write, create a touched override.
4. Preserve the override even when its value equals the current default.
5. Change or remove it only through normal application updates or an explicit
   host Clear State operation.

Equality with the default is not a deletion signal. If a user intentionally
sets Counter to `0`, changing the program default to `5` in a later release must
not silently rewrite that intent.

The dev-window Clear State action removes selected overrides and reconstructs
them from current defaults. This is host tooling, not Boon syntax.

`InitialProvenance` makes the sparse-default rule explicit:

- **ReconstructableDefault** is deterministic from the current plan,
  immutable host configuration, current restored authority, and explicitly
  supplied startup inputs. It remains sparse until written.
- **MaterializedAuthority** samples history that cannot be reproduced, such as
  an event, time, randomness, or a one-shot host result. Its first
  materialization is an authoritative touch and must enter the pending durable
  turn before dependent consequential effects may escape.

The compiler must not describe a source/event sample as a reconstructable
default merely to avoid a record. If an author wants a new source sample after
restart, it remains a `SOURCE`; if the sample is copied into memory, the copy
is authority. Restore never invokes time, randomness, or a host effect. A
materialized initial value must either arrive as an explicit bounded startup
input or be created by the first normal source turn after activation.

### Sensitive Input And Credentials

Persistence v1 does not introduce a secret language type or `SECRET` block.

Password controls keep editable bytes in a host-owned sensitive buffer. Boon
may submit a host reference to a typed authentication effect and observe the
result, but the plaintext draft is not copied into ordinary application
memory. It intentionally clears on restart.

Sensitive host paths must redact payloads from:

- inspector snapshots;
- runtime traces;
- logs and diagnostics;
- native GPU verifier reports;
- scenario artifacts;
- persistence encoding.

Long-lived credentials use platform credential-store references. This is not
an end-to-end secrecy claim: the host and authentication endpoint necessarily
process the secret. A future general sensitivity/taint system requires its own
language and threat-model plan.

## Semantic Memory And MachinePlan v3

Persistence requires a clean plan break. Introduce MachinePlan v3 and remove
the old plan shape rather than carrying a dual executor or compatibility
fallback.

Illustrative plan types:

```rust
pub struct MachinePlan {
    pub format_version: u32, // exactly 3
    pub application: ApplicationPlan,
    pub storage: StorageLayout,
    pub persistence: PersistencePlan,
    pub effects: Vec<EffectContract>,
    pub outputs: Vec<OutputRootPlan>,
    // Existing operation regions, source routes, constants, and debug map.
}

pub struct PersistencePlan {
    pub format_version: u32,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub memory: Vec<MemoryPlan>,
    pub lists: Vec<ListMemoryPlan>,
    pub migration_edges: Vec<MigrationEdgePlan>,
}

pub struct MemoryPlan {
    pub runtime_slot: PlanStorageId,
    pub memory_id: MemoryId,
    pub kind: MemoryKind,
    pub data_type: DataTypePlan,
    pub type_fingerprint: [u8; 32],
    pub initial_provenance: InitialProvenance,
    pub owner: MemoryOwnerPath,
}

pub enum MemoryKind {
    Scalar,
    IndexedField,
}
```

`PersistencePlan` includes every semantic memory node. It has no durable versus
session versus transient role enum.

### Stable Application Identity

The host supplies:

```text
ApplicationIdentity {
    package_id
    state_namespace
    deployment_domain
}
```

- Built-in examples receive versioned IDs in the example manifest.
- Custom playground examples receive a generated immutable ID in their custom
  manifest.
- Production applications use package/deployment identity.
- Tests use unique temporary namespaces and the in-memory driver.
- Optional user, tenant, or workspace isolation is a host namespace component.

Do not derive identity from example display name, source path alone, process
ID, current binary hash, current program hash, or launch order.

### Stable Memory Identity

Each memory node receives a compiler-owned semantic identity:

```text
MemoryId {
    canonical_module_identity
    named_owner_path
    semantic_memory_path
    memory_kind
}
```

The compact storage key is a SHA-256 digest of the full canonical identity. The
store also retains the readable identity for diagnostics and collision checks.

Identity excludes:

- source spans and line numbers;
- declaration order and runtime numeric IDs;
- current default values;
- derived expressions;
- document/render structure;
- type fingerprints and schema hashes;
- migration markers.

An anonymous stateful operation that cannot receive a unique stable semantic
path under a named owner is a compile error. The diagnostic asks the author to
name or structurally disambiguate the owner; it does not generate a line-based
key.

### Recursive Type Fingerprints

`PlanValueType` must become a recursive canonical data schema capable of
describing:

- null/unit;
- booleans, numbers, bytes, and text;
- variants/tags and their fields;
- records with canonical field identities;
- lists and row constructor authority;
- fixed-length and variable bytes;
- language-level error/result data where exposed as ordinary values.

Type fingerprints are separate from memory identity. Compatible additions and
migrations operate on known schemas. Bytes are never reinterpreted under a
different type merely because a path matches.

### Schema Hash

The persistence schema hash covers:

- application and state namespace format;
- semantic memory identities;
- memory kinds and recursive type fingerprints;
- list ownership and authoritative row fields;
- effect outbox schema.

It excludes source formatting, pure-derived fields, output trees, render
resources, debug labels, unrelated constants, and the historical migration
catalog. Adding an older supported migration recipe must not change the state
schema identity of an already deployed target.

Migration identity is separate and non-circular:

- each current-source `DRAIN` recipe has a canonical `recipe_hash` over stable
  source/destination memory leaves, transfer kinds, and the closed pure
  expression plan;
- a bound historical edge ID hashes the exact source schema reference, target
  schema reference, and `recipe_hash`;
- a bundle may expose a `catalog_hash` over its ordered edge IDs for artifact
  reproducibility, but `catalog_hash` is not part of `schema_hash`;
- an edge ID never depends on a target schema hash that itself includes that
  edge or catalog.

## Durable Data Model

### Stored Values

Define versioned storage DTOs rather than deriving encoding from runtime
structs:

```text
StoredValue
  Null
  Bool
  signed 64-bit Number
  Text
  Bytes or BlobRef
  Variant(tag, canonical fields)
  Record(canonical fields)
  ListValue(values) when the list is ordinary nested data
  ErrorData(code, fields) when it is a language value
```

Mutable semantic list authority uses dedicated list/row records instead of a
single encoded `Value::List` snapshot.

Use `minicbor` with:

- a top-level format version;
- numeric field and variant tags that are never reused;
- canonical field/map ordering;
- bounded lengths and nesting depth;
- explicit unknown-field handling for compatible additions;
- SHA-256 checksums for checkpoint and migration envelopes;
- golden fixtures for every released format version.

Do not derive the durable schema directly on runtime structs whose layout can
change.

### Sparse Scalar And Indexed State

Store only touched authoritative overrides. Untouched root and indexed state is
regenerated from current defaults.

For a Cells-sized logical model:

- addresses and other deterministic fields are derived and not stored;
- untouched cell formulas create no records;
- edited formula/draft state creates sparse row-field overrides;
- computed value, error, formula dependencies, indexes, and render state are
  rebuilt;
- the logical grid remains at least 2,600 cells and is not shrunk for storage.

Persistence must use the same generic list/memory metadata for Cells and every
other application.

### Mutable Lists

A mutable list stores:

- stable list memory identity;
- current ordered hidden row keys;
- row generations;
- monotonic allocator/`next_key` state;
- raw inserted-row constructor authority;
- touched indexed row-state overrides;
- authoritative empty-list state after mutation.

Append then remove must not resurrect initial rows or reuse an old key after
restart. Derived mapped fields, source bindings, lookup indexes, filters,
chunks, and summaries are reconstructed.

### Large Bytes

Small authoritative `BYTES` values stay inline. Values above a measured bounded
threshold are content-addressed by SHA-256 in a blob table. References are
derived from current durable roots and reclaimed during explicit/background
maintenance.

Source files, map tiles, waveform page caches, render assets, and other
reconstructable external data are not copied into the state database merely
because the application displays them. Serializable file/content descriptors
may be durable authority.

## Atomic Runtime Turns

### Prepare, Commit, Settle

Refactor `Session` execution into three explicit phases:

1. **Prepare**
   - ingest source events;
   - evaluate affected equations against the previous committed snapshot;
   - stage scalar/list/row writes;
   - resolve `LATEST` and other write policies;
   - validate list mutations and effect intents;
   - perform every fallible operation required to decide authority.
2. **Commit**
   - atomically install staged authority;
   - advance the runtime turn sequence once;
   - emit ordered authority deltas and output invalidations.
3. **Settle**
   - rebuild affected source bindings and indexes;
   - mark derived targets dirty;
   - demand host-owned outputs through currentness barriers;
   - enqueue persistence and effect work;
   - publish diagnostics.

Preparation failure changes no authority. Settle/output failure is visible but
does not create a half-committed turn.

### Authority Delta Contract

The current broad `Delta::SetValue` shape mixes authority and derived
recomputation. Introduce a dedicated semantic stream:

```rust
pub struct AuthorityTurn {
    pub turn_seq: u64,
    pub changes: Vec<AuthorityChange>,
    pub effect_intents: Vec<EffectIntent>,
}

pub enum AuthorityChange {
    TouchSetScalar { memory: MemoryId, value: DataValue },
    TouchSetRowField { memory: MemoryId, row: StableRowKey, value: DataValue },
    ClearAuthority { memory: MemoryId, scope: AuthorityScope },
    InsertRow { list: MemoryId, row: AuthoritativeRow, position: u64 },
    RemoveRow { list: MemoryId, row: StableRowKey },
    MoveRow { list: MemoryId, row: StableRowKey, position: u64 },
    ReplaceListAuthority { list: MemoryId, state: AuthoritativeList },
}
```

An equal-value first write emits a touch. A later assignment of the same value
may be elided from the durable delta, but it never clears existing touched
authority. `ClearAuthority` is emitted only by explicit host Clear State
tooling and is committed as a normal atomic turn. Derived values, source
bind/unbind operations, currentness transitions, render patches, and proof
events remain separate streams.

### No Mutable Runtime Cache

Compiler caches may retain immutable parse/type/IR/MachinePlan artifacts keyed
by content. They must never retain a mutable `Session` as a reusable compiled
artifact.

Hot reload creates one candidate Session, restores/migrates authority into it,
settles demanded outputs, then atomically swaps ownership. Failure leaves the
old runtime active.

## Restore And Hot Reload

Use an explicit builder. `Load` is read-only and produces a validated store
image; migration and deletion are first staged in memory:

```rust
SessionBuilder::new(plan)
    .with_restore_image(restore)
    .build()
```

Candidate construction phases are fixed:

1. Validate MachinePlan v3, persistence metadata, stable-key uniqueness, type
   fingerprints, migration graph, and effect contracts.
2. Validate the read-only store image and determine sequential migration
   edges.
3. Produce an in-memory migrated `RestoreImage` and an exact
   `ActivationBatch`; do not mutate the store.
4. Allocate raw scalar/list storage and static row identities without demanding
   derived fields.
5. Install compatible scalar, list, row, generation, allocator, and touched
   authority.
6. Evaluate reconstructable defaults only for memory with no restored touched
   authority; materialized initial authority enters a staged turn.
7. Rebuild indexes, dynamic dependencies, and dormant source-route metadata.
8. Mark affected derived values dirty.
9. Demand active output roots through currentness barriers without publishing,
   attaching live sources, or dispatching effects.
10. Mark the candidate ready only after all fallible settling work succeeds.

`RestoreImage` contains canonical authority, not a graph or render snapshot.

Cold start commits any non-empty `ActivationBatch` with Immediate durability
before attaching sources or publishing the first output. Hot reload uses a
short activation barrier:

1. Build the candidate from an authority snapshot while the old Session stays
   active.
2. At a turn boundary, stop accepting new authoritative turns and replay the
   complete tail since that snapshot into the candidate.
3. Settle the candidate again and revalidate the durable base epoch.
4. Atomically commit migrated authority, schema metadata, completed migration
   edges, and removed-record deletion as one Immediate `ActivationBatch`.
5. Perform an infallible Session ownership swap, attach live sources, publish
   current outputs, and resume turn admission.

If any step before durable activation fails, the old Session and old store stay
unchanged. If the process dies after durable activation but before publication,
the next cold start observes the new schema. No effect worker runs candidate
intents before activation.

Compatible hot reload and cold restart use the same stable identity, type,
migration, and deletion rules. Hot reload may source authority from the active
Session while cold restart sources it from storage, but both feed the same
builder.

## Persistence Coordinator

### Target-Neutral Command Protocol

A synchronous `DurableStateStore: Send` trait does not fit both native redb and
asynchronous browser IndexedDB cleanly. Core runtime code communicates through
owned commands and results:

```rust
pub enum PersistenceCommand {
    Load(RestoreRequest),
    Commit(CheckpointBatch),
    Activate(ActivationBatch),
    Barrier(BarrierRequest),
    Inspect(InspectRequest),
    Compact(CompactRequest),
    Shutdown(ShutdownRequest),
}

pub enum PersistenceResult {
    Loaded(Result<RestoreImage, StoreError>),
    Committed(Result<CommitAck, StoreError>),
    Activated(Result<ActivationAck, StoreError>),
    BarrierComplete(Result<BarrierAck, StoreError>),
    Inspected(Result<PersistenceInspectorSnapshot, StoreError>),
    Compacted(Result<CompactAck, StoreError>),
    ShutdownComplete(Result<ShutdownAck, StoreError>),
}
```

- Native runs a synchronous redb driver on a dedicated thread.
- Wasm runs an asynchronous Rexie driver in a browser worker/task boundary.
- Tests run an in-memory deterministic driver.
- The runtime and dev window consume the same bounded result/status shapes.

### Checkpoint Batching

In-memory Boon turns remain individually atomic. Backend transactions may
contain one or more complete contiguous turns:

```rust
pub struct CheckpointBatch {
    pub application: ApplicationIdentity,
    pub schema_hash: [u8; 32],
    pub base_epoch: u64,
    pub next_epoch: u64,
    pub first_turn_seq: u64,
    pub last_turn_seq: u64,
    pub changes: Vec<DurableChange>,
    pub outbox_changes: Vec<DurableOutboxChange>,
    pub checksum: [u8; 32],
}

pub struct ActivationBatch {
    pub application: ApplicationIdentity,
    pub expected_base_epoch: u64,
    pub next_epoch: u64,
    pub source_schema_hash: [u8; 32],
    pub target_schema_version: u64,
    pub target_schema_hash: [u8; 32],
    pub through_turn_seq: u64,
    pub authority_changes: Vec<DurableChange>,
    pub completed_migration_edges: Vec<MigrationEdgeId>,
    pub deleted_memory: Vec<MemoryId>,
    pub checksum: [u8; 32],
}
```

Coalescing rules:

- never split a turn;
- preserve complete turn order;
- scalar/row-field sets may collapse to the final value while retaining first
  touch and final turn sequence;
- list operations may coalesce only when final order, row generation,
  allocator, and delete semantics remain identical;
- migration and outbox transitions may not be dropped;
- acknowledgements cover an explicit contiguous turn/epoch range;
- queue saturation applies visible backpressure at a turn boundary rather than
  dropping durable authority.

Prepare reserves bounded queue capacity before authority commit. If capacity
cannot be reserved after semantic coalescing, the runtime rejects that
authoritative turn promptly with a visible `PersistenceBackpressure` status;
it does not mutate Session authority, wait for database I/O in the input frame,
or silently drop the turn. Critical barriers may intentionally delay external
acknowledgement/effect progress, but they do not turn ordinary rendering into a
storage wait.

### Durability Policies

Durability is host/deployment policy, not Boon source syntax.

**Buffered** is the default interactive policy:

- commit product state to the in-memory Session immediately;
- render without waiting for storage;
- coalesce pending complete turns on the persistence worker;
- perform an Immediate checkpoint within a short bounded interval;
- flush on clean shutdown, suspend, migration, and explicit Save/Flush;
- expose pending turn range, oldest age, and last durable epoch.

**Immediate** is available for strict deployments and barriers:

- the relevant turn range is acknowledged only after the backend's strongest
  supported commit completes;
- migrations and critical effect intents always use an Immediate barrier;
- schema activation uses one Immediate transaction after the candidate settles;
- a server may choose Immediate globally;
- browser reports must not pretend IndexedDB completion equals native
  power-loss guarantees.

It is impossible to guarantee that every already-visible UI change survives
sudden power loss while also never waiting for storage. Reports and the dev
window must state the pending tail honestly.

### Performance Boundary

No render hook or input frame may:

- open redb or IndexedDB;
- encode a full application snapshot;
- scan every state/list row;
- wait for a storage acknowledgement;
- parse persistence JSON;
- issue dev-window IPC queries;
- compact or repair storage.

The hot path emits bounded authority deltas into a preallocated/bounded queue.
Persistence costs are measured separately from product input-to-present
latency.

## Native redb Backend

Use explicit byte-keyed tables:

```text
META        format, app identity, schema, epochs, clean shutdown
SLOTS       MemoryId + scope -> touched scalar/indexed value
LISTS       list MemoryId + parent scope -> order, allocator metadata
ROWS        list MemoryId + scope + row key + generation -> row authority
CHECKPOINTS bounded checkpoint/journal metadata
MIGRATIONS  migration edge completion records
OUTBOX      durable external-effect intents and outcomes
BLOBS       content digest -> bounded large-value data/metadata
```

A write transaction:

1. validates application identity, schema, and current epoch;
2. rejects stale or non-contiguous batches;
3. records checkpoint metadata;
4. applies scalar/list/row changes;
5. applies migration completion and removed-key deletion;
6. applies outbox transitions;
7. advances the durable epoch and acknowledged turn sequence;
8. commits once;
9. returns acknowledgement only after redb reports success.

Schema activation uses the same table set but a distinct transaction contract:
it checks `expected_base_epoch`, writes the migrated candidate authority,
records every completed edge, deletes removed memory, advances schema/epoch,
and commits once. A stale base rejects the complete activation without partial
cleanup.

Use `redb::Durability::Immediate` for barriers. Buffered coalescing occurs before
the redb transaction, not by misreporting a weak commit as durable.

Evaluate `WriteTransaction::set_quick_repair(true)` through measured forced-
crash and startup-repair tests. The final setting must meet explicit recovery
budgets. Repair and compaction never run synchronously from product input.

Production storage uses the platform application-data directory. Playground
storage uses a repository-local ignored path:

```text
playground/state/<application-id>/<namespace>/state.redb
```

Built-in source stays versioned. Runtime databases, fault files, exports, and
repair artifacts remain ignored.

## Browser And Wasm Backend

Implement a Rust Rexie driver over IndexedDB object stores corresponding to the
logical native tables:

```text
meta
slots
lists
rows
checkpoints
migrations
outbox
blobs
```

One IndexedDB transaction applies one complete `CheckpointBatch` or
`ActivationBatch`. A checkpoint may contain multiple contiguous Boon turns.
Completion acknowledges the batch, not stronger power-loss durability than the
browser provides.

Rexie database and transaction objects stay on the browser worker/local
executor that created them; they are not sent across threads. Protocol messages
cross the boundary as owned bounded DTO bytes and scalar status only.

Requirements:

- no synchronous full-state encoding on the Wasm UI thread;
- no handwritten JavaScript storage implementation;
- no LocalStorage fallback;
- request persistent browser storage and report whether it was granted;
- surface quota, eviction risk, private mode, blocked upgrades, transaction
  aborts, and version-change closure;
- use the same codec, checkpoint, migration, deletion, and outbox golden
  fixtures as native;
- keep the old database untouched when migration cannot start safely.

The browser may evict or deny storage. The product must remain usable in memory
where possible while reporting that durable guarantees are unavailable. It may
not silently claim success.

## DRAIN And DRAINING

### Purpose

Formatting and compatible same-identity changes migrate automatically. The
compiler cannot safely infer whether a removed value and a new similar value
represent rename, replacement, split, or unrelated state. Those ownership
changes use the only persistence migration syntax: paired `DRAIN` and
`DRAINING` markers.

### Basic Rename

```boon
counter:
    0 |> HOLD count { ... } |> DRAINING

click_count:
    DRAIN { counter }
    |> HOLD count { ... }
```

`DRAINING` marks the old authoritative owner as retiring. `DRAIN { counter }`
transfers its authority to the destination identity. The destination identity
is durable immediately, so finalizing the source markers later does not rename
storage again.

### Path-Only Grammar

`DRAIN { ... }` accepts exactly one statically resolvable storage path:

- a named binding;
- a field path;
- a statically resolved `PASSED` path.

It rejects:

- calls and operators;
- indexing or list lookup;
- literals and record expressions;
- multiple source paths in one block;
- optional/dynamic access;
- `WHEN`, `THEN`, loops, event flow, or other runtime conditions;
- a derived value, source event, or non-authoritative function argument.

`|> DRAINING` is a terminal declaration marker, not a normal runtime operator.
A draining source cannot be used by ordinary Boon references.

### Pure Conversion

Type conversion uses ordinary compiler-proven pure Boon after the path-only
drain:

```boon
old_count:
    0 |> HOLD count { ... } |> DRAINING

count_text:
    DRAIN { old_count }
    |> Number/to_text()
    |> HOLD text { ... }
```

Conversion may call pure functions and construct pure records. It may not read
time, randomness, sources, host resources, mutable state, or perform effects.
The lowered transform is deterministic and versioned with the migration edge.

### Record Split And Merge

A draining record defines a source region. Every authoritative leaf in that
region is consumed exactly once. Disjoint leaves may go to separate
destinations; several independent sources may merge into one destination.

```boon
settings:
    [theme: Dark, zoom: 100]
    |> HOLD settings { ... }
    |> DRAINING

theme:
    DRAIN { settings.theme }
    |> HOLD theme { ... }

zoom:
    DRAIN { settings.zoom }
    |> HOLD zoom { ... }
```

```boon
old_theme:
    Dark |> HOLD theme { ... } |> DRAINING

old_zoom:
    100 |> HOLD zoom { ... } |> DRAINING

settings:
    [
        theme: DRAIN { old_theme }
        zoom: DRAIN { old_zoom }
    ]
    |> HOLD settings { ... }
```

Ancestor and descendant drains may not overlap. Self-drain, double drain,
partial leaf coverage, cycles, and conflicting destination authority are
compile errors.

### Lists And Indexed Rows

Hidden row keys preserve identity inside a list; they do not preserve identity
when the owning list binding is renamed. Whole-list ownership transfer is
therefore supported:

```boon
todos:
    LIST { ... }
    |> List/map(todo, new: new_todo(todo: todo))
    |> DRAINING

tasks:
    DRAIN { todos }
    |> List/map(task, new: new_task(task: task))
```

The transfer preserves order, hidden keys, generations, allocator state,
constructor authority, and touched row fields. Runtime indexes, filters,
source bindings, and derived rows are rebuilt.

An indexed row-field drain is a migration template applied by hidden row key:

```boon
FUNCTION new_todo(todo) {
    [
        title:
            todo.title |> HOLD title { ... } |> DRAINING

        text:
            DRAIN { title }
            |> HOLD text { ... }
    ]
}
```

Row-field migration is allowed within the same stable list owner. Renaming the
list owner and changing its authoritative row schema happen in two schema
versions so ownership remains unambiguous.

General partition/merge of independent authoritative lists is rejected in v1.
It requires row-owner, key-collision, order, and allocator policies that a
path-only drain cannot express safely. Prefer one authoritative list and pure
derived views:

```boon
active_tasks:
    tasks |> List/retain(task, if: task.completed |> Bool/not())

completed_tasks:
    tasks |> List/retain(task, if: task.completed)
```

### Automatic Changes And Deletion

The compiler automatically accepts:

- formatting/comments;
- unrelated declarations and sibling reordering;
- unchanged semantic identity and compatible type;
- new memory initialized from a current default;
- new derived fields or output changes;
- deletion of an old state definition.

Removing an authoritative state definition without `DRAIN` means intentional
deletion. Schema activation deletes its stored records atomically. Rename or
move must include the pair in the same schema change or the old authority is
deleted and the new memory starts from its default.

The dev window shows pending deletions before activating a hot reload. No extra
Boon delete keyword is introduced. Future host policy may optionally retain
backups, but v1 does not accumulate removed state records.

A stored type mismatch for an identity that still exists is not treated as
deletion. Activation fails until a compatible type or explicit conversion is
provided. Corrupt or unsupported stores fail before any cleanup transaction.

### Sequential Migration Catalog

Users may skip releases. This is common for browser/Wasm applications because
IndexedDB remains in a browser profile while the next visit downloads the
current Wasm directly. Native packages and FPGA bitstreams can also skip
versions.

When a DRAIN release is finalized, tooling writes its already-lowered and
validated pure migration plan into a deterministic source-controlled fragment.
Authors do not write another migration syntax. Current artifacts bundle every
edge from the oldest supported schema:

```text
schema 7 -> 8
schema 8 -> 9
schema 9 -> 10
```

A store at schema 7 opening version 10 applies the three edges one by one in a
single staged activation sequence. Each edge is idempotently recorded.

This catalog is code, not a runtime history or user-data log. A future major
release may raise the minimum supported schema and remove older recipe
fragments. Stores older than that baseline fail visibly and remain untouched.

For FPGA targets, migration normally runs in deployment/updater tooling or a
host controller before the new design owns persistent memory. The same generic
migration plan is used; normal datapath hardware need not interpret a migration
catalog.

### Migration Demonstration Examples

Migration is not complete when it exists only in unit fixtures. Add two
versioned built-in examples that use the same compiler, Session builder,
activation transaction, storage drivers, dev-window controls, and native TEST
path as ordinary applications. Neither runtime nor verifier may branch on the
example IDs.

**Counter Migration** is the small teaching sequence:

1. V1 owns `count` with program default `0`.
2. V2 marks `count` as `DRAINING` and initializes `click_count` from
   `DRAIN { count }`.
3. V3 removes both markers, retains only `click_count`, and changes its program
   default to `10`.

The scenario increments and explicitly writes `0`, checkpoints, restarts,
previews and activates V2/V3, and proves the touched `0` does not become the
new default `10`.

**Todo Migration** is the realistic sequence:

1. V1 owns `todos` rows with `title` and `completed`, a preferences record,
   and obsolete `show_help` state.
2. V2 transfers whole-list ownership from `todos` to `tasks`.
3. V3 finalizes the list rename and adds an untouched `priority` row field.
4. V4 splits preferences into `theme` and `density` through field drains.
5. V5 finalizes that split and drains indexed row field `title` to `text`.
6. V6 finalizes the row rename and purely converts `completed: Bool` into a
   status variant.
7. V7 finalizes conversion and deletes obsolete `show_help` authority.

The scenario preserves order, hidden row keys, generations, allocator
monotonicity, edited text, touched row fields, and preferences. One run applies
each version incrementally. A second fresh namespace jumps from V1 to V7 and
must produce the same canonical final authority by applying every catalog edge
in order.

The main example manifest retains V1 as the normal source and points to an
optional migration-sequence manifest. The sequence owns ordered stage IDs,
labels, schema versions, source units, and one typed migration scenario. Manual
demo namespaces persist normally. TEST uses a launch-scoped temporary namespace
and never clears or rewrites manual demo state.

The dev Migration view provides forward-only target selection plus Preview,
Activate, Restart, and confirmed Start Over commands. Preview compiles,
migrates, and settles a candidate without mutating the store or mounted
preview. Activate keeps the old retained frame visible until the durable schema
transaction succeeds and then performs one current Session swap. Old stage
source may be inspected but is not an implicit downgrade path.

Invalid examples remain compiler/typecheck fixtures rather than selectable
playground entries. They cover missing pairs, double drain, use after drain,
dynamic/conditional paths, overlap, cycles, impure conversion, and simultaneous
list-owner plus row-schema migration.

## Critical External Effects

Durable local state alone is insufficient when a turn triggers an external
action. Persistence v1 includes effect consistency.

### Effect Contracts

Built-in and custom host effects expose typed metadata:

```rust
pub struct EffectContract {
    pub effect_id: EffectId,
    pub replay: EffectReplay,
    pub barrier: EffectBarrier,
    pub result_policy: EffectResultPolicy,
}

pub enum EffectReplay {
    ReadOnly,
    Idempotent { key_type: DataTypePlan },
    NonReplayable,
}

pub enum EffectBarrier {
    None,
    Before,
    BeforeAndAfter,
}
```

The host contract, not a value name or Boon persistence annotation, determines
barrier requirements. The compiler rejects an irreversible workflow that lacks
a safe durable intent/idempotency contract.

### Durable Outbox

Consequential effects use a transactional outbox:

1. A Boon turn stages local pending state and an effect intent with a stable
   idempotency key.
2. An Immediate checkpoint stores both atomically.
3. Only after acknowledgement may the effect worker contact the external
   system.
4. The result enters Boon as a correlated source event.
5. A second Immediate checkpoint stores the authoritative outcome and consumes
   or completes the outbox item.
6. Restart replays pending idempotent intents or reconciles their remote status.

For a bank transfer, the UI may show `Pending` immediately. It may not show
`Completed` before the authoritative remote result is durably recorded. If the
remote system succeeded but the local result commit failed, restart queries by
the same idempotency key and reconciles without issuing a second transfer.

Exactly-once execution across an unreliable network is not generally possible.
Durable intent, idempotency, and reconciliation provide the honest practical
contract. The external bank/shared transactional database remains authoritative
unless Boon is deployed with an appropriate shared/replicated database. Local
redb is not distributed consensus.

### Lifecycle Barriers

Immediate barriers are automatic for:

- migrations and schema activation;
- critical effect intents and outcomes;
- server acknowledgements that promise durable mutation;
- explicit Save/Flush operations;
- bounded clean shutdown/suspend handling.

Read-only effects, repeatable fetches, rendering, and ordinary input do not
force barriers.

## Output Roots And Host Ownership

`document`, `scene`, routes, responses, build outputs, scheduled tasks, and
hardware pins are typed named output roots. They are reconstructed from current
authority and inputs.

Generalize the dedicated document field into:

```rust
pub struct OutputRootPlan {
    pub name: String,
    pub contract: OutputContractKind,
    pub demand: OutputDemandPolicy,
    pub value: ValueRef,
}
```

Hosts demand only roots they own. There is no mandatory imperative `main()`.
`BLOCK` continues to return its final value, while the root registry defines
what the host observes.

The runtime owns retained element identity, active/pending document state,
layout, text resources, hit testing, GPU resources, and render patches. These
are never durable application memory even if a structural document value could
be encoded.

## Dev Window Persistence UX

### Inspector

For authoritative memory, show:

```text
Symbol          store.count
Kind            Authoritative memory
Type            Number
Current         42
Default         0
Touched         yes
Last checkpoint 42 at epoch 18
Pending         turns 37..39, oldest 24 ms
Backend         redb, Buffered
Memory ID       app/store/count
Encoded size    7 bytes
Last writer     increment_button.press, sequence 27
```

For a derived value:

```text
Kind            Derived
Current         3
Stored          no
Reconstructed   from store.todos.completed
Currentness     current
```

For source/resource state:

```text
Kind            Source / host resource
Stored          no
Status          connected
Descriptor      credential-ref or file digest only
```

Sensitive values show only `redacted`; no length, hash, prefix, or payload is
included in reports.

### Persistence Overview

Show a cached scalar summary:

- application identity, namespace, schema version/hash;
- backend and effective durability guarantee;
- runtime turn, queued turn range, and durable turn/epoch;
- touched scalar and indexed counts;
- logical/list row counts, stored rows, and byte size;
- pending batch count and oldest age;
- last encode, commit, barrier, and restore durations;
- checkpoint, outbox, migration, and blob sizes;
- browser persistence permission/quota status;
- matched, added, deleted, incompatible, and migration-pending identities;
- last actionable error.

Large lists use virtualized filtering and bounded row samples. The sidebar must
never enumerate every Cells row during normal inspection.

### Controls

Use icon buttons with tooltips for:

- flush now;
- clear selected authoritative memory/list scope (disabled for derived,
  source, and output values);
- clear all application state with confirmation;
- export/inspect a state artifact;
- import into a separate preview before activation;
- run explicit maintenance/compaction;
- inspect pending outbox work;
- review and activate schema migration/deletion.

The migration preview shows source, destination, conversion, affected row
count, deletion count, required edges, and compatibility baseline before hot
reload commits.

### Hot-Path Isolation

The dev window consumes a cached `PersistenceInspectorSnapshot` updated after
turns and acknowledgements. Footer/render hooks perform zero storage queries,
zero runtime lock acquisition, zero transport calls, and zero JSON report
parsing.

Report these costs separately:

```text
authority_enqueue_us
encode_us
queue_age_ms
checkpoint_ms
barrier_ms
restore_ms
migration_ms
rebuild_derived_ms
stored_bytes
pending_turns
outbox_pending
```

## Implementation Phases

No phase is optional. Do not start a backend first and patch semantics around
it later.

### Phase 0: Canonical Documentation

- Replace the old persistence-boundary design with this document.
- Keep one active persistence plan.
- Lock durable-by-default memory and DRAIN semantics before parser changes.
- Record explicit user approval of the language-facing sections before Phase 1.
- Record explicit non-goals and completion gates.

Exit condition: this document contains no contradictory lifetime-role,
removed-record-retention, or one-turn-per-backend-transaction requirements.

### Phase 1: Semantic Memory And MachinePlan v3

- Lower every stateful construct to generic semantic memory nodes.
- Remove line-number and declaration-order durable identity inputs.
- Add recursive data type plans/fingerprints.
- Add stable application, memory, list, row-field, output, and effect metadata.
- Cut MachinePlan v2 execution compatibility rather than carrying two worlds.
- Add deterministic plan/debug inspection before disk storage.

Exit condition: Counter, Counter without HOLD, TodoMVC, Cells, NovyWave, and
generic fixtures emit deterministic memory plans independent of formatting and
without example-specific branches.

### Phase 2: Atomic Session And Restore Builder

- Implement prepare/commit/settle turn phases.
- Add staged scalar/list/row writes and rollback-on-prepare-failure.
- Split authority deltas from derived/debug/render deltas.
- Preserve equal-value touched writes.
- Implement `SessionBuilder` and restore publication barrier.
- Remove mutable Session/runtime caching from compile/hot-reload caches.

Exit condition: deterministic fault injection cannot expose partial authority,
and no output is published before restored authority is current.

### Phase 3: Persistence Protocol And In-Memory Driver

- Add target-neutral command/result protocol.
- Add checkpoint batching and exact acknowledgement ranges.
- Implement deterministic in-memory load, commit, barrier, inspect, compact,
  and failure behavior.
- Add bounded queue/backpressure and cached inspector snapshots.
- Prove no render/input hook performs storage work.

Exit condition: golden authority turns lower to exact checkpoint batches;
multi-turn coalescing preserves semantics under every injected failure.

### Phase 4: Native redb

- Add target-gated redb dependency and driver.
- Implement metadata, slots, lists, rows, checkpoints, migrations, outbox, and
  blob tables.
- Implement Buffered and Immediate policies.
- Add repository-local ignored playground state path.
- Benchmark checkpoint, barrier, restore, repair, compaction, and file growth.

Exit condition: native restart and forced-crash tests restore the last
acknowledged epoch with bounded startup and no product-frame I/O.

### Phase 5: Sparse Authority And Large Lists

- Implement touched provenance for root and indexed memory.
- Persist dynamic list structure, generations, order, allocators, and raw row
  authority.
- Handle empty-but-mutated and static-then-mutated lists.
- Implement bounded blob storage/reclamation.
- Add Cells-scale logical-versus-stored diagnostics.

Exit condition: one Cells formula edit stores bounded sparse authority while
all 2,600 logical cells and derived formulas restore correctly.

### Phase 6: Hot Reload, DRAIN, And Migration Catalog

- Implement path-only parser/type/IR support.
- Add leaf ownership, purity, cycle, overlap, and list constraints.
- Implement schema deletion and migration preview.
- Transfer scalar, record, whole-list, and indexed row-field authority.
- Generate deterministic finalized migration fragments.
- Apply sequential edges for skipped versions.
- Atomically swap a fully restored/current candidate Session.
- Add the generic version-sequence manifest and migration-scenario runner.
- Add Counter Migration and Todo Migration as mandatory built-in fixtures.

Exit condition: formatting and compatible additions are automatic; rename,
move, conversion, record split/merge, list rename, row-field rename, deletion,
and skipped-version scenarios behave exactly as documented.

### Phase 7: Critical Effects And Outbox

- Add typed host effect contracts.
- Lower effect intents into atomic authority turns.
- Implement before/after barriers, outbox dispatch, idempotent retry, and
  reconciliation.
- Reject unsafe non-replayable workflows.
- Add server acknowledgement integration.

Exit condition: crash-at-every-boundary tests cannot duplicate a consequential
effect when the declared external idempotency/reconciliation contract is
honored, cannot report completion without a durable outcome, and reject effects
that cannot provide such a contract.

### Phase 8: Browser/Wasm

- Implement the Rust Rexie/IndexedDB driver.
- Run storage work outside the Wasm UI hot path.
- Match native logical schema and golden checkpoint behavior.
- Add persistent-storage permission, quota, eviction, private-mode, blocked
  upgrade, and transaction-abort statuses.
- Bundle/apply the same supported migration chain.

Exit condition: browser refresh, skipped deployment, quota failure, and schema
evolution pass without LocalStorage or handwritten JavaScript persistence.

### Phase 9: Dev Window UX

- Extend inspector data with semantic memory/current/checkpoint metadata.
- Add virtualized Persistence, Migration, and Outbox views.
- Add clear, flush, export/import-preview, and maintenance controls.
- Add migration stage inspection plus Preview, Activate, Restart, and Start
  Over controls for any manifest-backed migration sequence.
- Add sensitive-data redaction negative tests.
- Keep render hooks on cached scalar snapshots only.

Exit condition: an operator can explain what is authoritative, what is stored,
what is pending, what will be deleted/migrated, and why an operation failed
without opening backend files.

### Phase 10: Generic Output Roots And Final Cleanup

- Replace dedicated output assumptions with `OutputRootPlan`.
- Keep retained host resources outside semantic memory.
- Add non-UI/server/build fixtures.
- Delete temporary adapters, duplicated schemas, old plan versions, and
  migration compatibility scaffolding not part of the final architecture.
- Run the no-special-case audit across compiler, runtime, document, renderer,
  playground, app window, and verifiers.

Exit condition: one execution architecture remains and every output kind uses
the same restore/currentness boundary.

## Required Verification

### Compiler And Identity

- `HOLD`, stateful `LATEST`, `LIST`, and stateful standard operators lower to
  generic semantic memory.
- Counter and Counter without HOLD produce equivalent persistence behavior.
- Stable identities survive whitespace, comments, unrelated declarations, and
  sibling reordering.
- Line numbers, declaration-order IDs, program hashes, and example names never
  appear in storage keys.
- Recursive type fingerprints are canonical and collision-checked.
- Duplicate/ambiguous semantic memory identities fail compilation.
- MachinePlan v2 is rejected rather than silently routed through another
  executor.

### Atomicity And Currentness

- Failure during prepare changes no authority.
- Commit installs all turn authority or none.
- Equal-value first writes become touched authority.
- Queue saturation rejects before commit and never blocks an interaction frame
  on storage I/O.
- Restore happens before source binding, derived demand, output publication, or
  effects.
- Derived values are not loaded from storage and recompute correctly.
- Indexes, formula dependencies, source bindings, and render state rebuild.
- Cycle/default-stack protection remains active after restore.
- Hot reload failure leaves the previous Session active and unforked.

### Minimal State

- Fresh Counter stores no override.
- Increment stores one override.
- Reset button assigning `0` keeps a touched durable override.
- Dev Clear State removes it and reuses the current program default.
- Cells startup stores no `value`, `error`, index, dependency, or untouched row
  values.
- Editing one cell stores only edited authority and bounded list metadata.
- TodoMVC preserves row order, keys, generations, titles, drafts, editing, and
  completion.
- Removing every list row preserves authoritative empty structure and allocator
  state.

### Migration

- Parser accepts only named, field, and statically resolved `PASSED` paths.
- Calls, indexing, conditions, event flow, derived sources, and impure
  conversion are rejected.
- Missing pair, double drain, uncovered leaf, overlap, cycle, self-drain, and
  conflicting destination fail.
- Scalar rename, module move, conversion, record split/merge, whole-list
  rename, and row-field rename preserve intended authority.
- List rename preserves row order, hidden keys, generations, allocator, and
  nested touched fields.
- General authoritative list split/merge is rejected with a derived-view
  recommendation.
- Removing state deletes its records atomically.
- Candidate settle failure leaves both active Session and durable schema
  unchanged.
- Schema activation commits migration results and deletion in one Immediate
  transaction before Session swap/publication.
- Existing-identity type mismatch fails without deletion.
- Stores skipping multiple supported versions apply each edge in order.
- Unsupported-old or corrupt stores fail before mutation.
- Repeated migration activation is idempotent.

### Checkpoint And Backend Recovery

- Fault before checkpoint changes nothing durable.
- Fault during a backend transaction restores the prior acknowledged epoch.
- Fault after acknowledgement restores the acknowledged turn range.
- Stale, duplicate, non-contiguous, and checksum-invalid batches cannot corrupt
  state.
- Coalesced batches preserve complete turn order and list semantics.
- Queue saturation never drops list, migration, deletion, or outbox changes.
- Native repair preserves the last acknowledged Immediate state.
- IndexedDB abort/quota/version-change failures map to explicit statuses.
- Native, Wasm, and in-memory drivers consume the same golden batches and
  produce equivalent restore images.

### Critical Effects

- Read-only effects require no barrier.
- Consequential intent and local pending state commit atomically.
- Dispatch never occurs before the required acknowledgement.
- Crash before dispatch resumes the pending outbox item.
- Crash after remote success but before local result commit reconciles by
  idempotency key.
- Duplicate retry cannot duplicate the external action.
- Result commit failure cannot display authoritative completion.
- Non-replayable effects without a safe contract are compile/activation errors.

### Sensitive Data

- Password drafts are absent from authority deltas and checkpoints.
- Logs, inspector, reports, scenarios, and errors contain no password payload,
  prefix, length, or hash.
- Credential references encode without exposing credential material.
- Restart clears host-owned sensitive input.
- The plan and product do not claim cryptographic end-to-end secrecy beyond
  tested host/storage/transport boundaries.

### Application Fixtures

- Counter and Interval test scalar updates and high-frequency coalescing.
- TodoMVC tests dynamic lists and row state.
- Cells tests sparse large logical models and formula currentness.
- NovyWave tests workspace/file descriptors, selected signals, groups, markers,
  panel dimensions, zoom, pan, and cursor while excluding waveform caches,
  requests, and render resources.
- A FjordPulse-shaped fixture tests locale, basemap, selection, focus/watch
  descriptors, last-known snapshots, reconnect, TTL/staleness derivation, and
  external authoritative reconciliation.
- A non-UI fixture tests server/output roots and critical effect barriers.
- Counter Migration proves scalar rename, paired-marker finalization, restart,
  and touched-equal-default behavior.
- Todo Migration proves whole-list and indexed-row ownership transfer, record
  split, pure conversion, deletion, incremental activation, and V1-to-V7
  skipped-version activation.

### Performance

- Persistence enqueue/authority encoding performs no file or IndexedDB I/O on
  product threads.
- No full application, document, scene, or JSON snapshot is created per turn;
  unchanged list rows are not re-encoded. A deliberate whole-list replacement
  encodes only its changed authority off the product thread and is subject to
  explicit queue/backpressure budgets.
- Buffered high-frequency state coalesces within bounded memory and time.
- Persistence enabled retains the native 16.7 ms input-to-present and scroll
  p95 targets for existing performance fixtures.
- Product latency excludes asynchronous checkpoint/readback/report completion,
  while those costs remain separately linked and reported.
- Restore, migration, forced-crash repair, and large-store startup have explicit
  release budgets.
- The dev persistence UI cannot freeze on Cells-sized or NovyWave-sized state.

### Dev Window And Native Evidence

- Inspector distinguishes authority, derived, source/resource, and output data.
- Current, touched, pending, and durable values update after turns and acks.
- Migration preview exposes additions, deletions, conversions, and supported
  upgrade edges.
- Large state is virtualized.
- Dev render hooks perform zero storage/runtime queries and zero IPC requests.
- Native visual/functional evidence uses app-owned events and WGPU/readback
  according to `NATIVE_GPU_PIPELINE.md`, never fabricated human observation.
- Migration TEST visibly performs application interactions with the virtual
  cursor, uses the public activation path, shows stage progress in the dev
  window, and leaves the manual namespace unchanged.
- Candidate preparation never blanks or replaces the active preview; the final
  retained-frame swap meets the normal interaction-frame budget while
  preparation and durable activation are reported separately.

### Genericity Audit

Scan parser, typechecker, IR, compiler, plan, executor, runtime, document,
native GPU, playground, app window, and xtask verifiers for:

- example-name branches;
- Cells-specific storage logic;
- source-label/geometry inference used as identity;
- duplicate execution paths or fallback runtimes;
- direct backend dependencies in compiler/runtime core;
- LocalStorage, Python, JSON state snapshots, or render-hook storage access.

Any positive finding blocks completion.

## Acceptance Criteria

The implementation goal is complete only when all conditions below are true:

- Boon has durable-by-default semantic memory with no persistence lifetime
  keywords.
- Every stateful construct lowers through one generic memory plan.
- MachinePlan v3 contains stable identities, recursive data schemas,
  persistence metadata, migration edges, effect contracts, and output roots.
- No line/order/runtime numeric identity reaches durable storage.
- Session turns are atomic and restore precedes first observable output.
- Equal-value touched authority, sparse defaults, lists, rows, generations, and
  allocators restore correctly.
- Native redb, Rust IndexedDB/Wasm, and deterministic in-memory drivers satisfy
  the same semantic contract.
- Buffered and Immediate guarantees are honest and visible.
- DRAIN/DRAINING is the only explicit persistence migration syntax and supports
  the documented scalar, record, list-owner, and row-field cases.
- Removed state is deleted atomically; incompatible/corrupt stores are not
  silently reset.
- Supported skipped versions migrate sequentially.
- Critical external effects use barriers, durable outbox, idempotency, and
  reconciliation.
- Sensitive input does not enter ordinary state, persistence, logs, reports, or
  inspector output.
- Cells proves sparse large-model persistence without losing 60 FPS targets.
- NovyWave and FjordPulse-shaped scenarios prove realistic desktop and
  full-stack state ownership.
- Counter Migration and Todo Migration pass manual controls, deterministic TEST,
  restart, incremental, skipped-version, fault-injection, native visual, redb,
  in-memory, and IndexedDB/Wasm verification.
- The dev window explains current/durable/pending/migration/effect state without
  hot-path queries.
- `document`, `scene`, retained layout, GPU resources, caches, and proof data
  remain reconstructed host/output state.
- All deterministic native, Wasm, restart, crash, migration, outbox, sparse,
  currentness, performance, and dev-window gates pass from fresh artifacts.
- No temporary adapter, duplicate runtime, old plan fallback, or example hack
  remains.

Do not mark the goal complete for documentation alone, a redb proof of concept,
Counter-only restore, stale reports, or passing smoke tests. Completion requires
the entire contract above.

## Explicit Non-Goals

- SQL query semantics or a general relational database.
- Distributed consensus, cross-region replication, or pretending local redb is
  a shared banking database.
- Aggressive guessed renames based on similar source structure.
- A general list repartition/merge migration language in v1.
- A general secret/taint language design inside persistence work.
- Persisting render scenes, GPU buffers, host handles, source events, runtime
  caches, or full graph snapshots.
- Adding a mandatory imperative `main()`.
- Weakening Cells/native GPU performance or proof gates to accommodate storage.

## Source References

Repository contracts and implementation:

- [`../architecture/RUNTIME_MODEL.md`](../architecture/RUNTIME_MODEL.md)
- [`../architecture/DELTA_PROTOCOL.md`](../architecture/DELTA_PROTOCOL.md)
- [`../architecture/LANGUAGE_SEMANTICS.md`](../architecture/LANGUAGE_SEMANTICS.md)
- [`../architecture/NATIVE_GPU_PIPELINE.md`](../architecture/NATIVE_GPU_PIPELINE.md)
- [`../../crates/boon_plan_executor/src/session.rs`](../../crates/boon_plan_executor/src/session.rs)
- [`../../crates/boon_plan/src/lib.rs`](../../crates/boon_plan/src/lib.rs)
- [`../../crates/boon_ir/src/lib.rs`](../../crates/boon_ir/src/lib.rs)
- [`../../examples/novywave/RUN.bn`](../../examples/novywave/RUN.bn)

Prior strict migration design consulted for semantic continuity:

- `~/repos/boon_experiments/docs/new_boon/3.6_STATE_EVOLUTION.md`
- [`../game/idea_1.md`](../game/idea_1.md)

External Rust/storage references:

- [redb repository](https://github.com/cberner/redb)
- [redb crate documentation](https://docs.rs/redb)
- [redb durability](https://docs.rs/redb/latest/redb/enum.Durability.html)
- [redb custom StorageBackend](https://docs.rs/redb/latest/redb/trait.StorageBackend.html)
- [Rexie IndexedDB wrapper](https://docs.rs/rexie)
- [minicbor](https://docs.rs/minicbor)
- [IndexedDB transactions](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB)
- [Browser storage persistence and quotas](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria)
