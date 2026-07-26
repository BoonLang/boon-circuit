# Boon Packed Data And Dense Internals Plan

Status: proposed implementation contract.

This plan is the universal substrate work that should happen before the first
processor project. It is useful on native CPU, Wasm, servers, GPU-oriented
kernels, and FPGA targets. It does not design a RISC-V core or any other
processor.

The central change is:

```text
today
  recursive generic values
  string-keyed records
  ID-keyed tree maps and tree sets
  pointer-heavy runtime metadata
  repeated cloning and materialization

becomes
  dense typed IDs
  compact arenas
  shape-specialized cells and columns
  bitsets and sparse sets
  flat dependency storage
  explicit collection/index kernels
  recursive values only at real boundaries
```

The result must preserve Boon semantics exactly. Physical layout is private
compiler/runtime machinery, not a second public data model.

## Authority And Scope

The following documents remain authoritative for their existing domains:

- [`BOON_SELF_HOSTING_LANGUAGE_FOUNDATIONS_PLAN.md`](BOON_SELF_HOSTING_LANGUAGE_FOUNDATIONS_PLAN.md)
  owns the public value algebra, exact `NUMBER`, Tags, `BITS[N]`, and
  `LIST`/`SET`/`MAP` authority semantics.
- [`BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md`](BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md)
  owns checked/elaborated programs, erased execution, ownership, and calls.
- [`TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md`](TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md)
  owns typed logical list access, ordering, cursors, currentness, and index
  behavior.
- [`BOON_PERSISTENCE_ARCHITECTURE_PLAN.md`](BOON_PERSISTENCE_ARCHITECTURE_PLAN.md)
  owns stable semantic identity, atomic turns, migration, and durable DTOs.
- [`../architecture/RUNTIME_MODEL.md`](../architecture/RUNTIME_MODEL.md) owns
  the static graph, typed-slot, dirty-key, candidate/commit, and delta model.
- [`../architecture/LIST_MODEL.md`](../architecture/LIST_MODEL.md) owns hidden
  row keys, generations, list equality, and software/hardware list behavior.

This plan owns:

- executable physical layouts;
- packed runtime cells and handles;
- dense compiler and runtime tables;
- scheduler/currentness/dependency storage;
- target-neutral collection storage primitives;
- boundary materialization rules;
- allocation, clone, lookup, and memory-density evidence.

If this plan conflicts with a public semantic decision in the foundations
plan, the foundations plan wins. In particular, this work must not optimize
today's legacy `Bool`/`Null`/`Error`/binary64 representation and then require a
second migration. Each semantic cut lands directly in the final packed
substrate.

Implementation is flag-day by ownership slice. A temporary reference engine
may exist while differential tests are being built, but completion requires
deleting the superseded execution path and its compatibility materializers.

## Goals

1. Make the normal execution unit a fixed-size typed cell or handle rather than
   a recursively owned Rust value tree.
2. Make dense plan IDs index arrays directly.
3. Replace hot-path `BTreeMap`, `BTreeSet`, `HashMap`, and `HashSet` machinery
   with structures chosen for the actual domain: arrays, bitsets, sparse sets,
   arenas, adjacency slabs, packed ordered pages, or explicit keyed stores.
4. Store record and row fields by checked shape and field offset, never by
   runtime string lookup.
5. Store list rows as stable occurrence-key-plus-generation identity mapped to
   physical structure-of-arrays slots.
6. Keep current, pending, valid, dirty, touched, and generation state in
   separate compact storage.
7. Produce a target-specific physical plan without contaminating portable
   `MachinePlan` or persisted semantic identity.
8. Make native and Wasm use the same deterministic storage/currentness model.
9. Provide the bounded widths, ports, capacities, and layout evidence later
   required by hardware lowering.
10. Preserve sparse work: one changed row must not become a whole-column or
    whole-list scan merely because storage is packed.

## Explicit Non-Goals

- No RISC-V ISA, CPU microarchitecture, board shell, RTL, or FPGA toolchain.
- No self-hosting requirement.
- No new public collection or storage type.
- No public `MEMORY` keyword. `MEMORY` is not an alias, compatibility spelling,
  or deferred requirement of this plan.
- No user annotations that expose host containers or physical addresses.
- No persistence-policy redesign.
- No UI or renderer rewrite except where they consume a corrected packed
  boundary.
- No global ban on an ordered container in cold tooling when canonical ordering
  is the actual requirement. Every remaining use must be classified and
  allowlisted; hot execution gets no such exemption.
- No example-specific shortcut for Cells, TodoMVC, FjordPulse, or the later
  processor.

## Current Baseline

The repository already points toward this architecture, but the implementation
is incomplete.

### Existing Foundations

`RUNTIME_MODEL.md` already requires:

- typed indexes and arenas;
- specialized hot-path storage;
- separate validity, generation, and source-binding columns;
- dirty scalar nodes and per-list dirty keysets;
- no whole-record, whole-list, or whole-text cloning for change detection;
- allocation and graph-rebuild evidence in release verification.

`MachinePlan` already has dense typed IDs and a
`PlanRowExpressionArena`. This is valuable progress and must be finished rather
than replaced with another IR.

### Remaining Generic Trees

At this snapshot:

- `boon_data::Value` is still a recursive owned enum containing `String`,
  `Vec<Value>`, and `BTreeMap<String, Value>`;
- the executor has a second recursive `Value` with string- and ID-keyed
  `BTreeMap` records;
- row state, provenance, bindings, currentness, dependencies, undo data,
  deltas, and authorities still use many tree maps and tree sets;
- ordered list access duplicates encoded keys into tree containers;
- runtime evaluation still contains hundreds of clone sites.

A repository-wide inventory found `BTreeMap` in 107 Rust files and `BTreeSet`
in 77 Rust files. That count is not itself a deletion target: it mixes hot
engine code, cold compilers, canonical boundaries, tests, and unrelated host
code. Phase 0 must classify every use instead of blindly swapping tree maps for
hash maps.

### Current Partial Dense Work

The expression arena is not the finish line. Its nodes may have dense IDs while
interning, normalization, runtime lookup, and materialization still use trees,
strings, recursive values, and cloned vectors. Dense IDs are useful only when
the full consumer chain keeps them dense.

## Non-Negotiable Semantic Boundary

The portable pipeline is:

```text
Boon source
  -> parsed syntax
  -> checked program
  -> erased semantic program
  -> semantic MachinePlan
  -> target/profile specialization
  -> PhysicalPlan
  -> packed executor or target lowering
```

The semantic `MachinePlan` says what the program means. `PhysicalPlan` says how
one target will store and execute it.

The current `MachinePlan` already mixes semantic and physical-looking fields.
The migration must classify them explicitly:

| Current `MachinePlan` field | Final owner |
| --- | --- |
| semantic plan version | Semantic plan |
| `program_role`, application and distributed contracts | Semantic plan |
| persistence, effects, outputs, and host-port contracts | Semantic plan |
| producer function instances and source routes | Semantic plan |
| constants and normalized expression arena | Semantic plan |
| logical demand, authority, commit, and delta semantics | Semantic plan |
| logical typed-list access/index requirements | Semantic plan |
| semantic dependency/currentness relationships | Semantic plan |
| hashed semantic schema names and identities | Semantic plan |
| `target_profile` | Physical-plan input and recorded profile digest |
| concrete `storage_layout` | Physical plan |
| physical kernel/placement regions | Physical plan |
| concrete index pages/encodings/storage class | Physical plan |
| dense adjacency arrays, queue capacities, and scheduler layout | Physical plan |
| resource, eligibility, latency, and capability realization | Physical plan/report |
| source spans and presentation labels | Detachable debug sidecar |

Any current field that contains both concerns is split. For example,
`list_indexes` retains a semantic access/order requirement in the semantic plan
while its key encoding, page shape, and storage bytes move to `PhysicalPlan`;
`regions` retains semantic operation ownership while fused/placed kernels move
to `PhysicalPlan`; `capability_summary` splits required semantic capabilities
from realized target support.

There are two hashes:

```text
semantic_plan_hash
  same program meaning across native, Wasm, and every eligible target

physical_plan_hash
  semantic_plan_hash + layout version + profile/capability/budget digests
```

During migration there must never be two authoritative layout sources.
Consumers cut over field by field, then the old target/layout field is deleted
from semantic `MachinePlan`. A final schema/deletion gate rejects target
profile, concrete storage layout, physical regions, and realized resource data
in the semantic artifact.

The following never depend on physical slot numbers or host addresses:

- public equality and ordering;
- authority identity;
- persistent memory identity;
- source routing identity;
- wire schema and canonical encoding;
- cursor identity;
- debug/source identity;
- migration identity.

The following may vary by target/profile:

- immediate versus arena representation;
- column width and alignment;
- bit packing;
- local versus shared storage;
- direct-address versus indexed `MAP`;
- index page size;
- scalar versus fused execution;
- register, distributed RAM, BRAM, linear memory, or native heap placement.

No packed pointer, arena offset, row slot, intern ID, or physical column number
is a persistent ABI.

## Shape And Layout Model

### Checked Shapes

Every concrete executable object and tagged payload receives a checked
`ShapeId`.

```text
ShapeId
  fields: [FieldId]
  semantic_field_names: [SemanticNameId]
  field_types: [SemanticTypeId]
  tag_set: optional closed semantic tags
  collection_authority_fields: [FieldId]
  canonical_field_order: [FieldId]
```

Open structural typing remains a compiler concern. Once a region is executable,
field access is resolved to `(ShapeId, FieldOffset)` or a typed column ID.
Normal execution never searches a string-keyed record.

Semantic field and Tag names are not debug metadata. They determine structural
type fingerprints, canonical object ordering, migration paths, persistence,
wire encoding, and public inspection. They live in a hashed semantic schema
table. `canonical_field_order` is derived from semantic names and then
translated to dense offsets for execution.

Only source spans, comments, presentation labels, and diagnostic rendering
live in a detachable debug sidecar. Removing that sidecar must not remove
semantic names or change runtime results, canonical encoding, persistence,
migration, target eligibility, or hashes that deliberately exclude
presentation-only metadata.

### Physical Layouts

A `LayoutId` describes one selected representation:

```text
LayoutId
  semantic_type
  storage_class
  width/alignment
  validity representation
  current/pending policy
  arena kind, if any
  canonical boundary visitor
  target eligibility facts
```

Layout construction is deterministic for:

```text
semantic_plan_hash
+ semantic contract version
+ physical-layout version
+ target capability digest
+ execution-budget digest
```

Two targets may choose different layouts and still execute the same semantic
`MachinePlan`. The versioned report must make the choice visible.

## Packed Value Representation

### Cells And Handles

The runtime has disjoint storage categories:

```text
DataCell
  private representation of an ordinary Boon data value

EngineRef
  typed reference to fields, sources, states, rows, owners, operations, or plans

AuthorityRef
  private reference to a LIST/MAP/SET authority

HostBinding
  private capability/resource binding, redacted from ordinary data
```

Only `DataCell` is accepted by Boon equality, canonical data visitors,
persistence/wire/effect payloads, and ordinary inspector values. The other
categories cannot be read, compared, matched, serialized, or forged by Boon
code.

A `DataCell` is a compact tagged representation or a statically typed column
element. It is not the public value algebra.

Eligible immediate forms include:

- one-bit representation of the closed `True | False` Tag set;
- closed Tag discriminants;
- bounded small whole Numbers proved exact;
- narrow `BITS[N]`;
- validity/presence bits kept out of application data.

Handle-backed forms include:

- arbitrary exact rationals;
- large `BITS[N]`;
- `TEXT`;
- `BYTES`;
- compound immutable values;
- large immutable content-addressed objects.

Absence is validity metadata, not a public `Null` value. Runtime/compiler faults
travel out of band, not as a public `Error` value. Large values use typed,
generational handles so stale references fail closed.

A compound value may physically contain an `AuthorityRef` in an authority-owned
field, but public equality, canonical encoding, persistence, wire, effects, and
inspection follow the referenced authority's public contents. They never expose
or compare the handle itself.

### Exact NUMBER

The packed substrate must be compatible with the foundations plan's one exact
`NUMBER` domain.

Possible private representations include:

- bounded signed integer;
- bounded unsigned integer;
- fixed-scale rational;
- dyadic rational;
- normalized small numerator/denominator;
- handle to an arbitrary-precision normalized rational.

Representation selection requires a proof. Overflow or loss of exactness
promotes to an exact representation or rejects an ineligible target; it never
falls back silently to `f64`.

### TEXT And BYTES

`TEXT` and `BYTES` use immutable or copy-on-write arena slices and content
handles where safe.

Requirements:

- equality is by content;
- `TEXT` ordering is canonical UTF-8 byte ordering where specified;
- intern ID order is never semantic order;
- substring/slice operations may share storage only while lifetime and memory
  retention remain bounded;
- appending or editing one value must not clone unrelated buffers;
- target profiles charge bytes and arena growth explicitly.

Short inline forms are optional optimizations. They may not create observable
differences.

### Arena Ownership And Reclamation

Generational handles detect stale reuse; they do not by themselves define
lifetime. Every arena allocation has one ownership class:

```text
plan-static
authority generation
committed state
turn-staged candidate
outbound delta/effect/persistence lease
temporary boundary materialization
```

Rules:

- current and pending cells hold explicit typed leases;
- successful commit transfers or replaces leases atomically;
- failed/`FLUSH`ed turns release all newly staged leases;
- overwrite, row removal, authority tombstone, and migration retire old leases;
- delta, effect, persistence, and debug queues retain payload leases until
  settle/acknowledgement or deterministic cancellation;
- storage is not physically reused until all readers before the retirement
  epoch have quiesced;
- compaction updates a private handle table and never changes semantic
  identity;
- a small slice may not pin an arbitrarily large backing buffer without that
  retained capacity being charged; the runtime copies/detaches when the
  profile's retention ratio is exceeded;
- host resources, secret material, and credentials live in the separate
  redacted `HostBinding` store and never in a data arena.

Reports distinguish live, staged, leased, retired, pinned, and reclaimable
bytes. Stress tests repeatedly overwrite and remove values, abort turns, queue
deltas/effects, compact arenas, and reuse physical slots while proving bounded
live-plus-retired memory and stale-handle rejection.

### Objects And Tags

Concrete objects use shape-specialized storage:

```text
object cell
  ShapeId
  fixed inline fields and/or typed handles
```

Rows and repeated objects use structure-of-arrays columns:

```text
field A column
field B column
field C column
valid bits
changed-at column
```

A closed tagged union uses a compact discriminant plus the payload layout for
that arm. Open or boundary-only data may use a slower canonical DTO, but it
must not force every checked object through a recursive record tree.

## Dense IDs, Arenas, And Static Tables

All plan-local IDs must satisfy:

```text
0 <= id < owning_table.len
```

The validator rejects holes, duplicate ownership, out-of-range references, and
cross-table ID confusion before execution.

Use:

- `Vec<T>` or boxed slices for dense immutable tables;
- typed generational arenas for dynamic owner instances;
- compact small vectors for statically tiny arity;
- offset-plus-edge arrays for static adjacency;
- bitsets for dense membership;
- sparse sets or generation-stamped vectors for sparse membership;
- ring/slab queues with declared capacity for bounded work.

Do not use:

- `BTreeMap<NodeId, ...>` for a dense node table;
- `BTreeSet<FieldId>` for a dense dirty set;
- string paths to rediscover checked fields;
- recursive Rust calls for arbitrarily deep valid expression DAGs;
- host pointer identity as semantic ownership.

Compiler symbol tables may use a construction-time lookup table when names are
truly the key. Before an artifact crosses into checked IR, all references become
typed IDs and all externally visible iteration is canonicalized explicitly.

## Packed Runtime Store

The target-neutral store separates logical concerns:

```text
ScalarStore
  layouts
  current columns
  pending candidate columns
  valid bits
  pending-valid bits
  changed-at columns

OwnerStore
  static owner ID
  parent owner instance
  occurrence key
  generation
  current physical row slot

SourceStore
  route table
  bindings
  pending events/signals

ListStore
  occurrence key + generation -> physical slot
  typed field columns
  order storage
  free storage
  mutation staging

DeltaStore
  changed scalar IDs
  changed row-field keys
  structural mutations
  bounded payload references
```

Current and pending storage are separate where Boon's atomic-turn semantics
require it. Committing a turn copies or swaps only marked slots. A failed turn
discards staged bits/entries without reconstructing the entire previous state.

## Scheduler, Currentness, And Dependencies

### Static Dependencies

Compile static dependencies into CSR-like tables:

```text
dependency_offsets[node_id]
dependency_edges[offset..next_offset]
```

Equivalent tables exist for:

- source to operation;
- state to operation;
- field to row expression;
- list structure to consumers;
- output demand to roots;
- commit result to delta lowering.

The schedule is a dense array. Static readiness and dirty state are bitsets.

### Dynamic Owner-Scoped Dependencies

Dynamic dependencies arise from row keys, owner instances, evaluated access
selections, and distributed call instances. They use:

- generational owner arenas;
- bounded adjacency slabs;
- typed composite keys interned to dense runtime IDs;
- per-owner bitsets or sparse sets;
- explicit detach/tombstone operations.

Deleting an occurrence tombstones its `(OccurrenceKey, Generation)`. Reusing
its physical slot or compacting the store cannot change another occurrence's
semantic identity. No stale source, dependency, effect, cursor, persistence
record, or delta can retarget a new owner.

### Work Queues

Queues expose:

- capacity;
- high-water mark;
- number of pushes, coalesces, and drops/rejections;
- deterministic overflow behavior;
- whether order is semantically significant.

Coalescible current-value signals may use dirty bits. Repeated events must use
ordered storage. A generic set cannot silently collapse semantically distinct
events.

## LIST, MAP, And SET Physical Storage

### Public Surface

The only public authorities remain:

```text
LIST
SET
MAP
```

There is no public `TABLE`, `ARENA`, `INTERNER`, or `MEMORY` value kind.

### LIST

A list uses:

- hidden occurrence key;
- generation;
- a private mapping from `(OccurrenceKey, Generation)` to physical row slot;
- validity bit;
- typed field columns;
- source-order/order token;
- explicit order storage;
- bounded or dynamic free-slot storage;
- per-field dirty/touched bits.

One row-field update touches that field column, declared dependents/indexes, and
the resulting delta. It does not clone a row record or materialize the list.

Compaction may change only the physical slot mapping. Cursors, persistence,
source routes, equality, and deltas use occurrence identity, never the slot. A
profile may choose `OccurrenceKey == PhysicalSlot` only when it proves fixed
address identity for the complete lifetime and still keeps the concepts typed
separately.

### MAP And SET

`MAP` and `SET` share an internal keyed-authority interface while preserving
their distinct public semantics.

Each physical class has an eligibility and exhaustion contract:

| Physical class | Eligibility and mutations | Required worst-case contract |
| --- | --- | --- |
| Dense direct address | Finite affordable complete key domain; arbitrary upsert/remove | Constant address work; capacity fixed by domain; validity bits distinguish absence; canonical traversal scans the bounded domain or a proved occupancy index |
| Collision-checked hash slots | Bounded capacity and probe/load bound; arbitrary upsert/remove | Complete-key comparison; deterministic maximum probes; full table rejects without eviction; canonical traversal uses a separate semantic order |
| Perfect hash | Compile-time closed/immutable keyset; value replacement only unless rebuilt outside the live turn | Bounded lookup; no unproved dynamic insertion; rebuild changes only physical plan; canonical key table survives restore |
| CAM | Small bounded hardware keyset; arbitrary bounded update/remove | Declared parallel/serial lookup latency and ports; full capacity rejects; complete-key match |
| Flat sorted array | Small bounded or mostly immutable authority; arbitrary update with staged movement | Binary lookup plus bounded linear insertion/removal; atomic commit; no partial shifted state |
| Sorted run plus update segment | Read-heavy authority with bounded mutable segment | Lookup checks both structures; merge threshold/work is bounded and atomic; tombstones and cursors survive merge |
| Page/B+tree | Dynamic ordered authority with a proved maximum height/page count | Bounded lookup/update/split/merge; copy-on-write or staged atomic commit; canonical pages rebuild from semantic contents |
| Radix pages | Fixed-width canonical bit/byte keys with bounded depth | Work bounded by key width/radix; empty branches reclaimed safely; canonical traversal independent of allocation |

The choice depends on key type, capacity, mutation rate, lookup/range needs,
target, restore behavior, cursor contract, and worst-case proof. Every profile
states its supported operations; selecting a class that cannot implement a
possible mutation is a compile-time eligibility error.

Canonical enumeration is an explicit semantic traversal. It never inherits
hash order, allocation order, tree-node layout, or physical slot order.
Collision tests always compare complete keys.

A dense bounded `MAP<BITS[N], V>` may become direct-address storage. This is the
intended way to express register files and bounded memories later; it does not
require a `MEMORY` keyword.

### Ordered Access Kernel

The typed-list plan's seek, range, prefix, stable order, cursor, candidate, and
currentness contracts remain unchanged.

Replace `std::collections` trees in the hot ordered access kernel with an
explicit implementation whose layout and bounds are visible:

- immutable packed sorted run plus bounded mutable segment;
- page-oriented B+tree;
- radix pages for suitable keys;
- flat array for small bounded profiles.

Every implementation must expose:

- key and payload bytes;
- page/segment count;
- lookup and mutation work;
- rebuild/merge work;
- affected-index fanout;
- cursor stability rules;
- deterministic iteration.

Swapping `BTreeMap` for `HashMap` is not completion.

## Pure Kernel IR And Fusion

After packed storage exists, add a small target-neutral `KernelIR` for pure,
bounded regions.

Initial operations:

```text
load typed column/cell
load constant
BITS operations
exact proved Number operations
closed-tag compare/select
field projection
boolean-mask operations over True/False tags
store pending cell/column
```

Initial eligible regions:

- scalar pure expression chains;
- row-local `List/map`;
- row-local filter predicates;
- comparisons and mask construction;
- dirty-bit scanning and key compaction;
- pure candidate computation and staging.

Fusion stops at:

- all `HOLD`/authority arbitration and semantic commit boundaries;
- `LATEST` arbitration;
- effects and I/O;
- structural list settlement;
- persistence/distribution boundaries;
- observable ordering;
- unsupported exact arithmetic;
- unbounded text/byte work;
- `FLUSH` paths not proved equivalent.

A separate internal commit-copy kernel may move already accepted marked slots
only after preparation and arbitration succeed. It is not a pure semantic
`KernelIR` region. It must be infallible for admitted bounds, preserve the
declared commit/delta order, produce the same changed set, and release staged
handles correctly on abort. No optimization may make a candidate visible
before the complete turn succeeds.

Every kernel has a scalar implementation. This is the ordinary implementation
for small dirty sets, not a compatibility escape hatch. Backend selection may
choose:

```text
tiny dirty set     -> scalar packed loop
sparse medium set  -> compact IDs plus gather/masks
dense set          -> contiguous SIMD/vector/kernel path
```

Thresholds come from target-specific measurement and are recorded in the
physical-plan report.

`KernelIR` describes pure computations without clocks or ports. The later
processor plan's `CoreHardwareIR`/`TargetHardwareIR` are different layers with
registers, cycles, ports, target elaboration, and assertions.

## Boundary Materialization

Recursive canonical values may exist only at deliberate boundaries:

- parser literals and diagnostics;
- public inspector/debug snapshots;
- persistence DTOs;
- canonical wire/effect payloads;
- golden semantic tests;
- import/export APIs.

Prefer streaming visitors:

```text
packed store -> canonical encoder
packed store -> persistence batch
packed store -> inspector tree for selected value only
packed store -> wire frame
```

Do not reconstruct a complete recursive application snapshot merely to encode
one changed value or render one inspector selection.

Boundary reports count:

- recursive values materialized;
- bytes materialized;
- strings allocated;
- large buffers shared or copied;
- reason and caller category.

## Persistence And Wire Compatibility

Persistence stores semantic values, authority operations, stable IDs, and
generations. It never stores:

- arena addresses;
- physical row slots or slot mappings as semantic identity;
- intern IDs as text identity;
- layout IDs as schema identity;
- native pointer widths;
- target-specific padding;
- host hash values.

Restore builds a fresh packed store through validated boundary visitors and
publishes it only after all semantic and capacity checks pass.

The persistence plan contains older wording around legacy value variants. Before
the packed cutover reaches those DTOs, reconcile that wording with the
foundations plan so the project implements one final value algebra.

## Target Profiles And Reports

`PhysicalPlan` reports, at minimum:

```text
semantic plan hash
semantic contract version
layout/compiler version
target capability digest
execution budget digest

table/arena counts and bytes
cell and column layouts
BITS widths
NUMBER representation proofs
TEXT/BYTES bounds
LIST/MAP/SET capacities and physical classes
index layouts and worst-case work
current/pending/dirty storage bytes
queue capacities
kernel regions and scalar paths
rejected/ineligible regions with reasons
```

Native and Wasm may use dynamic arenas under explicit budgets. Hardware
eligibility requires concrete bounds, widths, ports, and latency. An unbounded
layout is rejected; it does not quietly become a host container in generated
hardware.

## Foundation Phase Interlocks

Packed implementation starts only after its semantic owner is final:

| Packed work | Required semantic prerequisite |
| --- | --- |
| closed Tag cells and presence/fault channels | foundations Tag/absence/fault flag-day cut |
| exact Number layouts and promotion | foundations exact `NUMBER` semantics |
| bit cells, columns, and bitwise kernels | end-to-end `BITS[N]` semantics |
| keyed `MAP`/`SET` storage classes | final authority, equality, conflict, and canonical-order semantics |
| object/Tag shape tables | final checked structural type/fingerprint rules |
| executable expression and owner arenas | authoritative checked-to-erased program cut |
| persistence/wire visitors | final semantic DTO/schema and identity decisions |

Reference oracles for a landed packed slice implement only the final algebra.
They may not preserve legacy `Bool`, `Null`, privileged `Error`, or binary64
branches as a second compatibility semantics.

## Implementation Phases

### Phase 0: Inventory And Baseline

- Classify every map/set use as dense-ID table, membership, ordered index,
  name lookup, canonical boundary, test oracle, or unrelated host code.
- Inventory recursive values, runtime string lookup, clones, allocations,
  snapshots, and materialization sites.
- Record baseline memory and work for Counter, TodoMVC, Cells, a large
  FjordPulse-shaped dataset, and a synthetic million-row fixture.
- Add counters before changing representations.
- Create and freeze `budgets/packed-data.toml` before Phase 1. It records the
  exact target/profile/fixture, warmup and measured turn interval, allocator
  instrumentation scope, bytes per row/store, live/staged/leased/retired arena
  ceilings, boundary materialization bytes, queue high-water limits, index
  work, sparse and dense latency/throughput ceilings, and allowed regression
  ratchets.
- Reconcile active semantic documents where legacy values conflict with the
  foundations plan.

Exit: a machine-readable inventory, fresh baseline, and checked numeric budget
manifest exist; no category is hidden inside an aggregate "map count."

### Phase 1: Dense Semantic Artifacts

- Freeze dense-ID invariants and validation.
- Finish expression, type, shape, constant, source, state, owner, and field
  arenas.
- Replace recursive expression ownership and deep-cloning transforms.
- Keep semantic field/Tag names in the hashed schema; move only source spans,
  comments, and presentation/diagnostic labels into debug sidecars.
- Make all executable consumers use typed IDs rather than rediscovery by name.

Exit: the checked/erased/plan pipeline has one compact expression arena and
validated dense tables.

### Phase 2: Packed Cells And Typed Arenas

- Implement `ShapeId`, `LayoutId`, representation selection, and target
  capability inputs.
- Add typed scalar, exact Number, BITS, Tag, text, bytes, and compound arenas.
- Add generational handles and boundary visitors.
- Ensure semantic migrations land directly in these stores.

Exit: ordinary scalar/object evaluation does not require a recursive runtime
value.

### Phase 3: Dense Scalar Runtime

- Move root state, sources, constants, operation metadata, currentness, dirty
  sets, candidates, commits, and deltas to dense storage.
- Replace static dependency maps with flat adjacency arrays.
- Use bounded explicit work stacks and queues.
- Preserve rollback and exact changed-at behavior.

Exit: scalar turns execute without string lookup, tree containers, or
recursive-value clones.

### Phase 4: Columnar Rows And Authorities

- Replace row maps with slot/generation ownership and typed field columns.
- Implement valid/touched/dirty/current/pending columns.
- Replace order and free-slot machinery with explicit packed structures.
- Preserve hidden identity, stale-event rejection, nested ownership, and
  atomic structural mutations.

Exit: one-row edits remain one-row/one-column work.

### Phase 5: Currentness, Dependencies, And Delta Staging

- Replace dynamic dependency trees with owner arenas and adjacency slabs.
- Replace membership/tree work sets with bitsets or sparse sets.
- Replace undo maps with typed staged-write logs.
- Emit deltas directly from touched storage.

Exit: no ordinary turn materializes a full state/list snapshot.

### Phase 6: Explicit Collection And Index Kernels

- Implement the target-neutral keyed-authority storage interface.
- Add direct-address, packed-flat, and page-oriented implementations as
  justified by profiles.
- Move the typed ordered-access kernel off standard tree containers.
- Prove canonical order, cursor behavior, collisions, and bounded work.

Exit: `LIST`/`MAP`/`SET` semantics no longer depend on host container behavior.

### Phase 7: Kernel IR And Density-Aware Execution

- Add the minimal pure `KernelIR`.
- Discover and fuse compatible regions from checked/erased semantics.
- Add packed scalar execution first.
- Measure auto-vectorization and density thresholds.
- Add additional backends only against the same differential fixtures.

Exit: the same physical plan can choose sparse scalar or dense batch execution
without changing semantics.

### Phase 8: Boundaries, Native/Wasm, And Hardware Eligibility

- Stream persistence, wire, effects, and inspector data from packed stores.
- Restore through validated builders.
- Run native/Wasm differential suites.
- Emit bounded layout reports for hardware fixtures.
- Re-run product-scale interaction/currentness reports.

Exit: all boundary encodings and target reports come from the final packed
store.

### Phase 9: Flag-Day Deletion

- Delete the old executor value tree, row maps, dependency trees, compatibility
  materializers, and dual-store switches.
- Delete superseded DTOs and stale active documentation.
- Install dependency/lookup/allocation audits as ordinary CI gates.
- Keep reference semantics as fixtures/traces, not as a production engine.

Exit: there is one execution world.

## Verification

### Semantic Differential Tests

For every fixture, compare:

- committed scalar and collection values;
- authority structure and generations;
- source/event order;
- `LATEST` outcomes;
- pending/commit behavior;
- emitted semantic deltas;
- failures and rollback;
- effects and persistence batches;
- typed-list order, cursors, and budgets.

Native and Wasm must produce identical canonical values, deltas, failures, and
digests under the same profile.

### Representation Tests

- exact Number operations never lose precision;
- narrow-to-wide promotion preserves value;
- BITS width and displayed order remain exact;
- closed Tags keep canonical identity;
- intern IDs never affect equality or ordering;
- injected key collisions cannot change `MAP`/`SET` behavior;
- deleted/reused occurrence generations reject stale references even if a
  physical slot is reused or moved;
- debug-sidecar removal does not alter execution.

### Bounded-Work Tests

- one root scalar edit touches only declared dependents;
- one row-field edit touches one column, declared dependents/index entries, and
  bounded deltas;
- one list insertion does not rebuild unrelated rows or indexes;
- no-op turns perform no semantic work after source ingestion;
- deep valid DAGs and dependency chains pass on the default stack;
- queues fail deterministically at declared bounds.

### Kernel Equivalence Tests

For every enabled fused region:

- disabled and enabled traces have identical operand evaluation order where
  observable;
- currentness reads and dependency subscriptions are identical;
- work charging is exact and does not disappear inside a kernel;
- unrelated fields/arms are not evaluated eagerly;
- `FLUSH` aborts before semantic commit and releases all staged handles;
- arbitration and commit happen outside pure kernels;
- sparse/dense dispatch thresholds come from a recorded benchmark, not a
  source constant chosen by intuition;
- scalar and every enabled backend emit the same candidates, commits, deltas,
  failures, and final digest.

### Allocation And Lookup Tests

`budgets/packed-data.toml` defines the warmup count, measured-turn interval,
allocator hook scope, included engine/boundary code, target/profile, and
fixture. Within that exact interval, warm fixed-capacity scalar and row-field
turns require:

- zero heap allocations;
- zero queue growth;
- zero runtime string-key field lookup;
- zero recursive value-tree clone;
- zero full snapshot materialization.

Variable-size text/bytes growth is charged separately and must identify the
owning value and declared budget.

Overwrite/remove/failed-turn/queued-delta stress tests additionally prove that
live plus retired arena bytes return below the frozen ceiling after the
declared quiescence/acknowledgement point.

### Scale Matrix

The million-row proof fixture contains a mix of:

- exact numeric or bounded bit fields;
- one-bit closed tags;
- another closed Tag;
- derived formulas;
- filter predicates;
- ordered/range access;
- paged output.

Run:

| Dirty population | Purpose |
| --- | --- |
| One row | Preserve interactive sparse latency |
| Approximately one percent | Exercise compaction/gather behavior |
| All rows | Exercise packed sequential/vector throughput |

Cells' 2,600-row interaction gates and FjordPulse's large indexed-access gates
remain product regressions. First interaction may not build an index or scan a
full collection.

Every scale report validates the checked budget manifest. Raising a ceiling
requires an explicit reviewed budget change with before/after evidence; reports
cannot silently learn a new baseline.

### Required Metrics

Every performance report includes:

- allocations and allocated bytes;
- packed bytes by store/layout kind;
- recursive boundary materializations;
- recursive clones;
- runtime string lookups;
- tree-container lookups;
- dense slot reads/writes;
- dirty/touched population;
- dependency edges visited;
- queue high-water marks;
- index pages/segments touched;
- delta items and bytes;
- elapsed time split by ingest/evaluate/commit/delta/boundary.

## Deletion Audits

Production code in:

- executable tables in `boon_typecheck`, `boon_ir`, `boon_compiler`, and
  `boon_plan`;
- `boon_plan_executor`;
- hot `boon_runtime` execution/currentness paths;
- `boon_list_access`;

must contain no dense-ID-keyed tree/hash maps or sets after the final cut.
Checked/erased/plan execution performs no post-resolution name lookup and no
recursive arena clone. Construction-time symbol lookup and semantic boundary
maps require the same narrow allowlist as runtime boundary code.

The audit detects direct imports, fully qualified paths, type aliases, wrapper
types, and equivalent third-party containers. It does not pass merely because a
`std::collections` import was renamed.

Remaining uses elsewhere require an audited allowlist with:

- file and owner;
- cold/boundary reason;
- ordering or lookup requirement;
- why a dense/packed representation is inappropriate;
- proof that the container is outside normal execution.

Tests may retain a simple reference oracle. Production code may not call it.

Additional deletion scans reject:

- recursive executor `Value`;
- string-key runtime row fields;
- ID-to-tree expansion;
- backend AST execution;
- runtime field-name rediscovery;
- whole-state/list snapshot comparison in ordinary turns;
- dual packed/reference production switches;
- target profile, concrete storage layout, physical kernel regions, or realized
  resource data remaining authoritative in semantic `MachinePlan`;
- a second physical layout source outside versioned `PhysicalPlan`.

## Acceptance Criteria

This plan is complete only when:

1. Public semantics still match the authoritative foundations plan.
2. There is no `MEMORY` keyword or public physical-storage type.
3. Executable objects and rows use checked shapes and field offsets.
4. Dense plan IDs index arrays directly.
5. Static dependencies use flat adjacency storage.
6. Dirty/current/touched membership uses bitsets or sparse sets.
7. Dynamic ownership/dependencies use typed generational arenas.
8. Root and row state use typed current/pending columns.
9. `LIST` rows use stable occurrence-key-plus-generation identity mapped to
   private columnar slots; compaction cannot change semantic identity.
10. `MAP`/`SET` physical choices preserve complete-key equality and canonical
    enumeration.
11. The ordered access kernel no longer relies on standard tree containers.
12. Ordinary turns perform no recursive value cloning or whole-snapshot
    materialization.
13. Warm bounded scalar/row turns allocate nothing.
14. Native and Wasm differential suites agree.
15. Persistence and wire identity remain semantic and target-neutral.
16. Bounded hardware reports contain concrete widths, capacities, ports,
    latency, and storage estimates.
17. Product-scale sparse interaction gates remain passing.
18. The old production value/container execution path is deleted.
19. No compatibility fallback remains.
20. Hashed semantic schemas retain canonical field and Tag names independently
    of detachable debug metadata.
21. `DataCell`, `EngineRef`, `AuthorityRef`, and `HostBinding` cannot be
    confused or cross a forbidden boundary.
22. Arena overwrite/remove/abort/queue stress stays within live-plus-retired
    byte budgets and rejects stale handles.
23. Semantic and physical plan hashes are distinct, versioned, and have only
    one authoritative owner each.
24. Compiler/IR/plan executable tables satisfy dense-ID and post-resolution
    lookup deletion audits.
25. Every report passes the frozen numeric budget manifest.
26. Enabled/disabled `KernelIR` execution preserves order, currentness, work
    charging, candidate/commit behavior, and exact semantic traces; thresholds
    are measured.

The minimum dependency gate for the processor plan is narrower than completion
of every phase here. Processor planning and isolated generic hardware fixtures
may begin earlier. Canonical processor implementation may begin only once:

- `BITS[N]` works end to end;
- bounded `MAP` can lower to a proved dense direct-address layout;
- checked shapes, dense IDs, and packed cells are stable;
- hardware profiles can state widths, capacities, ports, and latency;
- processor-cycle execution needs no recursive value, string lookup, or
  tree-map/set hot path;
- native and Wasm execute the same bounded artifact deterministically;
- semantic/physical plan formats used by the processor are stable and
  separately hashed;
- every compiler/runtime/layout path used by the processor has completed its
  flag-day deletion, with no reference/legacy production switch or
  compatibility materializer remaining.

## Risks And Mitigations

### Packing The Wrong Semantics

Risk: making today's legacy values fast creates a second migration later.

Mitigation: semantic phases from the foundations plan land directly in the
packed stores; legacy variants are not treated as the target model.

### Replacing Trees With Nondeterminism

Risk: casual hash-map replacement changes order, collision behavior, or
reproducibility.

Mitigation: canonical order is explicit, complete keys are compared, and
ordered access uses a specified physical kernel.

### Dense Storage Makes Sparse Work Dense

Risk: columnar storage tempts whole-column loops for tiny edits.

Mitigation: dirty key compaction and sparse/dense selection are mandatory; the
one-row gate is equal in importance to the all-row throughput gate.

### Internal Layout Leaks Into Persistence

Risk: slot IDs, intern IDs, or arena offsets become durable by convenience.

Mitigation: one-way boundary visitors and golden cross-layout restore tests;
internal IDs are rejected by persisted/wire schemas.

### Dual Engines Become Permanent

Risk: the reference and packed engines diverge and double maintenance.

Mitigation: differential execution is phase-scoped; the final phase deletes the
old production engine and retains only canonical traces and small test oracles.

### Generic Hardware Needs Create New Syntax

Risk: a processor experiment introduces storage keywords or target-specific
annotations.

Mitigation: public `MAP` plus checked bounds/access patterns and target profiles
select physical storage. Missing capability is fixed below the language
surface unless it represents a genuinely universal semantic need.

## End State

Boon source still speaks in values, authorities, dependencies, and turns.

Internally:

```text
semantic graph
  -> dense typed tables
  -> target-selected packed layouts
  -> sparse dirty execution
  -> optional fused dense kernels
  -> atomic commit
  -> direct semantic deltas
```

The runtime is compact enough for Wasm, predictable enough for servers, dense
enough for vectorization, and explicit enough for FPGA lowering. The first
Boon-designed processor then becomes a demanding consumer of a general
architecture rather than the reason for a one-off hardware path.
