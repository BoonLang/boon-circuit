# Typed List Pipelines And Query Removal Plan

Status: proposed architecture and implementation plan. No implementation is
claimed by this document.

## Summary

Remove `List/query` and `List/query_prefix` from Boon. They expose a partial
database planner as one reflective standard-library call and conflict with the
typed, compositional list model.

Replace them with ordinary typed list pipelines:

- `List/filter(item, if:)` and `List/find(item, if:)` for predicates;
- `List/sort_by(item, key:, direction:)` for the primary order;
- `List/then_by(item, key:, direction:)` for additional lexicographic keys;
- `List/take(count:)` for a bounded list;
- `List/page(size:, after:)` for revision-bound keyset pagination.

The compiler derives logical access requirements from those typed expressions
and chooses physical indexes. Index declarations, residual plans, query modes,
field paths, normalization policy, and selected physical indexes are not Boon
arguments. One canonical keyed LIST remains the row authority in memory and in
persistence.

This plan is a clean replacement, not a compatibility migration. The final
implementation must delete the old syntax, special lowering, runtime path, and
tests. It must not retain aliases, deprecated forms, fallback parsing, or an
example-specific branch.

## Why The Existing Surface Must Go

The current `List/query` signature combines the list receiver with 27 query
arguments. One call controls:

- reflected field paths;
- compound-key order;
- normalization and token expansion;
- uniqueness;
- exact, prefix, range, union, and intersection selection;
- residual filtering;
- result order;
- limit and cursor behavior;
- WGS84 distance filtering.

This creates invalid argument combinations that the normal type system cannot
exclude. Fields and normalizations are encoded as comma-separated `TEXT`, and
residual fields are encoded as dotted paths. IR lowering reparses checked source
instead of consuming authoritative typed field identities.

The executor also maintains two representations:

1. canonical PlanExecutor rows and typed equality indexes;
2. a separate query collection containing reconstructed, string-keyed records
   and another set of indexes.

Every relevant row update synchronizes both structures. The persistent query
backend validates redb authority but loads rows into the in-memory query engine
for execution. This is useful reference machinery, but it is not one unified
LIST runtime or a scalable persistent query path.

The current API is also incomplete as a database abstraction. It has no general
joins, groups, aggregates, Boolean planner, collation model, full-text ranking,
or spatial index. WGS84 is a Haversine residual, not a spatial access method.
The API is therefore both too large for `List` and too narrow to be a database.

## Architectural Decision

Application code describes list semantics. Compiler and runtime code own the
physical execution plan.

The semantic pipeline is independent of whether execution uses:

- a direct keyed lookup;
- an ordered in-memory index;
- an index union or intersection;
- a redb range;
- an incremental filtered view;
- a bounded scan for a statically small collection.

Changing the physical index must not change a Boon value, invalidate a cursor
whose semantic view and revision are still available, or require source edits.

The compiler may reject a server or persistent query when it cannot prove a
bounded access path under the target profile. That diagnostic is an execution
budget failure, not a request for the developer to spell a physical index in
Boon.

## Canonical Boon API

The examples in this section define the intended shape. Exact typechecker syntax
must follow the canonical OUT and named-argument rules in
`BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md`.

### Typed Filtering

Exact, compound, range, prefix, union, and intersection behavior is expressed
with ordinary predicates:

```boon
oslo_stations:
    stations
    |> List/filter(station, if:
        station.city == selected_city
    )

matching_stations:
    oslo_stations
    |> List/filter(station, if:
        station.name
        |> Text/trim()
        |> Text/to_lowercase()
        |> Text/starts_with(prefix: normalized_search)
    )
```

Repeated filters express conjunction. `Bool/and` and `Bool/or` express explicit
Boolean composition. A range is two comparisons. Token membership is an
ordinary typed token-list predicate. There is no `select:` mode and no user
visible residual mode.

### Typed Ordering

`List/sort_by` is the canonical primary-order operator. The repository already
uses that spelling in BUILD programs, so the replacement must finish one generic
implementation rather than introduce a competing `List/order_by` name.

```boon
ordered_stations:
    matching_stations
    |> List/sort_by(
        station
        key: station.city |> Text/to_lowercase()
        direction: Ascending
    )
    |> List/then_by(
        station
        key: station.name |> Text/to_lowercase()
        direction: Ascending
    )
```

`List/then_by` refines equal groups from the preceding order. A chain forms a
typed lexicographic key without tuples, positional field names, or
comma-separated metadata. Hidden stable row identity is always the final
tie-breaker, so every ordered view is total and deterministic.

The ordering key must be pure, deterministic, finite, and composed from closed
typed values supported by the target profile. Ascending and descending are
semantic directions, not separate physical indexes; an ordered index may be
traversed in either direction.

### Bounded Results

```boon
preview:
    ordered_stations
    |> List/take(count: 20)
```

`List/take` is a lazy bounded view preserving item type and source row identity.
It must stop upstream work when enough matching rows are available. It must not
materialize or sort the complete logical list merely to return the first page.

### Pagination

```boon
station_page:
    ordered_stations
    |> List/page(
        size: 20
        after: requested_position
    )
```

`requested_position` has the structural variant type:

```text
Start | Cursor[value: BYTES]
```

The result preserves the row type:

```text
Page[items: LIST<T>, next: End | Cursor[value: BYTES]]
| PageExpired
| InvalidPageCursor[code: TEXT]
```

An empty byte string is not an end-of-page sentinel. Callers match `End` or
`Cursor`. Cursor bytes are opaque application data: they may be stored and sent
back, but modifying them can only produce `InvalidPageCursor`.

`size` is bounded by the target profile and must be statically provable to be
within that bound. The initial native/server maximum remains 10,000, but product
code should use much smaller values. No page evaluation may inspect more than
the candidate budget.

## Pagination Alternatives Considered

### Offset Or Page Number

Pros:

- simplest API;
- supports direct requests for an arbitrary page;
- adequate for immutable small lists.

Cons:

- later pages may require walking all preceding rows;
- concurrent inserts and deletes cause duplicates and omissions;
- stable random access requires rank indexes or materialized snapshots;
- it encourages UI pagination to leak into storage execution.

Decision: keep offset/range operations only as ordinary finite-list slicing or
layout demand. Do not use offsets as the durable `List/page` contract.

### Current-Epoch Cursor

Pros:

- deterministic while the list does not change;
- bounded keyset seek;
- no historical state or server-side cursor resource;
- straightforward to implement with current authority revisions.

Cons:

- any mutation expires every cursor for that view;
- poor behavior for rapidly changing feeds;
- callers may need to restart from `Start` frequently.

Decision: this is the minimum implementation policy, but it is described as a
revision-bound snapshot cursor with explicit expiration. It is not presented as
silently live pagination.

### Retained Snapshot Cursor

Pros:

- every page observes one consistent result set;
- concurrent writes do not create duplicates or omissions;
- best database semantics for reports and catalog browsing.

Cons:

- requires MVCC, persistent versions, or retained copy-on-write roots;
- needs bounded lifetime, cleanup, memory/disk accounting, and restart policy;
- serialized redb requests cannot retain an arbitrary read transaction forever;
- substantially expands the persistence project.

Decision: the semantic cursor binds a revision and permits a backend to retain
that revision. Historical snapshot retention is outside this clean-cut task.
The first complete implementation retains only the current revision and returns
`PageExpired` once it is unavailable. A future MVCC backend may make more old
cursors succeed without changing Boon syntax or cursor meaning.

### Live Keyset Continuation

Pros:

- inexpensive and stateless;
- continues through a changing collection without global expiry;
- useful for timelines and best-effort feeds.

Cons:

- updates that move a row across the cursor can duplicate or omit it;
- inserts before the cursor never appear;
- it does not represent one coherent result set;
- calling it a page without exposing those semantics is misleading.

Decision: reject as the default `List/page` contract. Truly live feeds should
use SOURCE/stream semantics. A future explicitly named live continuation API
would be a separate design, not a `mode:` argument added to `List/page`.

### Server-Side Iterator Handle

Pros:

- can preserve exact execution state;
- avoids reparsing or replanning later pages.

Cons:

- introduces hidden mutable resources and session affinity;
- requires timeout, cancellation, memory limits, reconnection, and failover;
- cannot be freely persisted or resumed after deployment;
- conflicts with ordinary immutable Boon data.

Decision: reject for the core API. Host capabilities may use such handles
internally, but Boon observes only revision-bound cursor bytes and explicit
terminal variants.

## Chosen Cursor Contract

The selected model is revision-bound keyset pagination with explicit expiry.

Every cursor binds:

- cursor format version;
- semantic source-list memory identity;
- recursive row-schema identity;
- canonical semantic pipeline fingerprint;
- source authority revision;
- last ordered key components;
- hidden stable row identity;
- order directions.

It does not bind:

- a physical index identity;
- an executor implementation;
- an in-memory versus redb backend;
- the page size;
- a process-local pointer or iterator.

The semantic fingerprint is derived from typed filter and ordering expressions,
not source formatting, field-name strings, or physical-plan IDs. The same
cursor remains meaningful after a physical index change when the semantic view,
schema, and bound revision remain available.

The next page performs a direct ordered seek after `(last_key, row_identity)`.
It must not regenerate the complete candidate set and discard earlier rows.

For untrusted external transport, the host seals or authenticates the cursor.
A plain checksum detects corruption but does not prevent a client from editing
and recomputing the checksum. Authentication keys never enter Boon state,
reports, persistence, or cursor-visible fields.

UI virtualization does not use `List/page`. Layout requests visible logical
ranges with bounded overscan from a retained lazy list. Pagination is for an
application-visible bounded continuation such as a server response.

## Compiler Architecture

### Typed Logical Views

Lower list pipelines from the authoritative checked/erased expression graph
into a typed logical view graph:

```text
SourceList
Filter(predicate)
SortBy(key, direction)
ThenBy(key, direction)
Take(count)
Page(size, position)
Map(projection)
```

Each row use carries static owner, local, `ListId`, and `FieldId` identities.
No backend may rediscover row fields from source text, debug labels, object
geometry, or string paths.

### Predicate Analysis

Recognize physical opportunities from typed pure expressions:

- row field or pure computed key equal to a row-independent value;
- lower and upper comparisons;
- `Text/starts_with` over a deterministic normalized key;
- conjunction across repeated filters or `Bool/and`;
- disjunction through `Bool/or` and compatible index unions;
- token-list membership and compatible index intersections;
- ordered prefix plus later ordering keys;
- a remaining typed predicate evaluated only against selected candidates.

The final item above is an internal residual expression. It remains the original
typed Boon expression and is never reduced to a small user-visible enum such as
`FieldEqual`, `TextContains`, or `Wgs84Radius`.

Normalization is explicit source composition:

```boon
item.name |> Text/trim() |> Text/to_lowercase()
```

The compiler may index that deterministic computed key. It must not request a
parallel normalization string. Locale collation, stemming, fuzzy matching, and
spatial indexes belong to separately typed libraries/backends.

### Logical And Physical Identity

Maintain two distinct hashes:

- semantic view identity, used by currentness and cursors;
- physical access-plan identity, used by generated index storage and rebuilds.

A new physical index or scan direction changes only the physical identity.
Changing predicates, key functions, ordering, source memory, or schema changes
the semantic identity and invalidates old cursors explicitly.

### Boundedness

The compiler records a target-profile candidate and output budget. For a large
server/persistent list, `take` or `page` must have a provably bounded upstream
access plan. A small closed LIST may use a measured scan. No optimizer miss may
silently become an unbounded server scan.

Diagnostics explain the semantic predicate that lacked a bounded access path.
They do not tell the user to provide string field names or an index ID.

## Runtime Index Kernel

Replace the parallel query collection with one generic index kernel over the
canonical PlanExecutor LIST authority.

An internal index plan contains typed IDs and compiled key expressions. It does
not contain application-facing field paths:

```text
IndexPlanId
source ListId
typed key-expression IDs
key value types
key directions
multiplicity policy
physical schema/version identity
```

The index maps ordered structural keys to stable `RowId`s. Row values remain in
the canonical list storage. Index entries never duplicate complete records.

Required operations are:

- exact seek;
- lower/upper range seek;
- text-prefix range seek over an explicitly normalized Text key;
- forward and reverse traversal;
- bounded union and intersection of row identities;
- direct seek after a page cursor key;
- incremental insert, remove, and old-key/new-key update;
- deterministic hidden-row tie-breaking;
- index rebuild and integrity validation.

The runtime registers precise dynamic dependencies for the list structure,
queried key range, and row fields evaluated by the residual predicate. A change
outside the demanded range must not force a full result rebuild.

Metrics report logical rows, index seeks, key ranges, candidates, residual
evaluations, returned rows, cursor seeks, index updates, rebuilds, bytes, and
full scans. They must distinguish semantic list size from materialized and
examined rows.

## Persistence Architecture

Canonical LIST rows are stored once. Generated indexes are derived storage and
may be rebuilt from canonical rows after schema or physical-plan changes.

Refactor useful `boon_query_redb` behavior into the canonical persistence path:

- atomic row and index updates;
- authority revision tracking;
- physical index plan validation;
- index rebuild policy;
- corruption detection;
- bounded direct index-range reads;
- restart and migration tests.

The persistent server path must not load every row into a duplicate in-memory
query collection merely to execute a page. Hot in-memory indexes are valid for
interactive sessions, but cold/persistent execution must be able to seek redb
index entries directly.

`unique` is not a read argument. Application-level uniqueness can be expressed
as an indexed typed existence check and append within one authoritative Server
turn. The runtime transaction makes the check and mutation atomic. A future
schema-level unique constraint, if needed, belongs to persistence/schema design
and is not part of this replacement API.

## Optional Domain Libraries

Do not put the following into `List/page`, `List/filter`, or compiler syntax:

- locale-aware collation;
- Unicode normalization profiles;
- stemming and token ranking;
- typo/fuzzy matching;
- geohash, H3, R-tree, or other spatial access methods;
- coordinate reference system conversion.

These are typed pure libraries plus optional specialized physical backends.
For example, a Boon predicate may call `Geo/distance(...)`; semantics remain a
normal Boolean expression. A compiler/backend may recognize a supported spatial
function and select a spatial index, but correctness never depends on that
optimization.

## Clean Implementation Sequence

### 1. Freeze Replacement Semantics

- Add canonical signatures and flow types for typed `List/sort_by`,
  `List/then_by`, `List/take`, and `List/page`.
- Specify stable ordering, key types, page variants, cursor validation, and
  expiry in `LANGUAGE_SEMANTICS.md`.
- Add compile-time negative tests for wrong OUT use, impure keys, unsupported
  directions, unbounded page sizes, and invalid cursor variants.

### 2. Build Typed Logical List Plans

- Lower filter/order/take/page pipelines from ErasedProgram only.
- Preserve generic row type through every operator and through `Page.items`.
- Add semantic pipeline hashing independent of source formatting and physical
  indexes.
- Extend the existing typed equality lookup inference instead of creating a
  second query extractor.

### 3. Unify Index Storage

- Generalize the existing typed equality index into ordered typed keys.
- Add exact, range, prefix, forward/reverse, union/intersection, and cursor
  seek operations.
- Remove whole-record query mirroring and string-key reconstruction.
- Keep one source-list authority and one row identity.

### 4. Implement Lazy Bounded Operators

- Make filter/order chains lazy keyed views.
- Make `take` stop upstream work after enough accepted rows.
- Make `page` seek after the cursor and read at most `size + 1` accepted rows.
- Return `PageExpired` after a bound revision is unavailable.
- Return `InvalidPageCursor` for malformed, foreign, wrong-schema, or
  wrong-semantic-view cursors.

### 5. Integrate Persistence

- Store canonical rows once in redb.
- Maintain or rebuild derived typed indexes transactionally.
- Execute bounded redb index ranges directly.
- Verify restart, physical-plan replacement, authority revision, corruption,
  and current-only cursor expiry.

### 6. Migrate Boon Source And Tests

- Replace FjordPulse station prefix search with typed normalization, filter,
  order, and take/page operators.
- Replace compound, number-range, tag, union, intersection, mutation, and page
  fixtures with ordinary typed pipelines.
- Replace query-time uniqueness tests with atomic typed existence-check plus
  append tests.
- Update wrappers to prove that generic Boon functions can compose these
  operators without compiler-only source spellings.
- Update language, runtime, persistence, FjordPulse, and unified-goal docs.

### 7. Delete The Old World In One Cut

Delete, rather than rename or quarantine:

- the `List/query` and `List/query_prefix` typechecker registrations;
- parser/operator recognition for both names;
- CSV/path parsing and AST query rediscovery;
- `ListProjectionKind::TextPrefix` and `IndexedQuery` source-level variants;
- public `ListQuery*`, `PlanQuery*`, and query-projection contracts that exist
  only for the removed syntax;
- executor `QueryCollectionState` and whole-record synchronization;
- compatibility shorthand and fallback branches;
- obsolete examples, fixtures, reports, and tests.

Retain only code that serves the new generic physical index and persistence
architecture. Rename retained internal concepts to `AccessPlan`, `IndexPlan`,
`IndexCursor`, or another physical name so user query syntax is not recreated
inside the compiler.

Do not leave both execution paths available behind a feature flag. Temporary
noncompiling intermediate commits are acceptable during the cut; the final
tree has one path.

## Verification

### Language And Type Tests

- `filter`, `sort_by`, `then_by`, `take`, and `page` preserve exact row type.
- wrappers around each operator preserve OUT binding and type identity.
- renaming a row field updates typed references without query metadata edits.
- string field paths and comma-separated field declarations are rejected.
- page items preserve `T`; no open-object fallback is accepted.
- unsupported or impure ordering/normalization expressions fail clearly.

### Semantic Equivalence

For small deterministic fixtures, compare optimized execution with a simple
reference implementation of:

- equality and compound equality;
- numeric ranges;
- normalized text prefix;
- conjunction and disjunction;
- token membership union and intersection;
- ascending, descending, and multi-key order;
- take and every page boundary.

The reference evaluator is test-only and must not become a runtime fallback.

### Index And Currentness Tests

- recognized equality, range, and prefix predicates use typed indexes;
- no field-name reflection occurs in runtime execution;
- one row update changes only affected index entries and dependent views;
- unrelated row changes do not rebuild the complete result;
- indexed and reference results remain exactly equal after inserts, updates,
  removes, restore, and migration;
- one canonical list owns every row value and hidden identity.

### Pagination Tests

- first and subsequent pages have deterministic, nonoverlapping rows;
- every order includes the hidden row-identity tie-breaker;
- second-page work begins at the cursor seek and does not revisit earlier keys;
- `End` and `Cursor` are explicit variants;
- same semantic view and current revision accept a cursor;
- mutation with current-only retention returns `PageExpired`;
- another list, schema, predicate, order, or direction returns
  `InvalidPageCursor`;
- a physical index replacement alone does not alter semantic cursor identity;
- malformed and tampered external cursors fail authentication/validation;
- page work and memory stay within configured budgets.

### Persistence Tests

- row and index updates commit atomically;
- restart preserves canonical rows, authority revision, and compatible indexes;
- incompatible physical indexes rebuild without changing semantic rows;
- corrupt authority fails closed;
- redb page execution performs bounded index reads without loading all rows;
- no process handle, iterator pointer, credential, or authentication key enters
  Boon-visible state or reports.

### Product And Performance Tests

- FjordPulse executes search over the required 58,500-station fixture through
  typed filter/order/take/page source;
- prefix and compound lookup report zero full scans;
- page latency and candidate work remain bounded under mutation and restart;
- Cells typed address lookup remains zero-scan and unaffected by this change;
- normal UI visible-range materialization remains separate from pagination;
- no compiler/runtime/document/renderer/verifier branch uses an example name.

## Deletion Audits

Final executable source and examples must contain no removed query syntax or
reflection machinery. The implementation should use parser-aware scans and
normal compiler tests, not regex rewriting alone.

Required audit concepts include:

- removed `List` query call names;
- removed `ListQuery` and `PlanQuery` source contracts;
- query field CSV parsing;
- residual field-path strings;
- compatibility prefix-query lowering;
- duplicate query collection authority;
- user-visible physical index names or IDs;
- empty-BYTES end-of-page sentinels.

This plan file may retain the removed spellings as the migration record. Active
language and architecture contracts must not.

## Clear End Condition

This plan is complete only when all of the following are true:

1. Boon source uses only typed compositional list operators for the migrated
   exact, range, prefix, compound, ordering, bounded-result, and page cases.
2. Both removed query functions fail as unknown functions and have no parser,
   typechecker, IR, compiler, plan, executor, runtime, persistence, example, or
   compatibility implementation.
3. One canonical LIST authority owns rows; all indexes reference typed keys and
   stable row identities without reconstructed string-keyed records.
4. Pagination follows the revision-bound keyset contract, returns explicit
   terminal/error variants, and seeks directly after the cursor.
5. In-memory and redb execution pass equivalence, mutation, restart, corruption,
   bounded-work, and cursor tests.
6. FjordPulse's full station fixture and Cells address lookup pass their typed
   zero-scan gates without example-specific code.
7. The relevant compiler, plan, executor, persistence, example, and aggregate
   verification suites pass from final source with fresh artifacts.
8. Independent reviews find no reflective query metadata, duplicate authority,
   hidden runtime fallback, compatibility alias, or stale report used as proof.

Do not mark the larger unified goal complete merely because this replacement
plan passes. Distributed/session work, streaming, NovyWave, Cells frame budgets,
and FjordPulse's remaining acceptance conditions retain their own end gates.
