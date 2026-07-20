# Typed List Pipelines And Query Removal Plan

Status: authoritative replacement architecture and implementation plan. No
implementation is claimed by this document. Its list-access, ordering,
pagination, index, and query-removal contracts supersede conflicting active
`List/query`, `List/query_prefix`, persistent-query-driver, and cursor guidance
in older plans.

## Summary

Remove `List/query` and `List/query_prefix` from Boon. They expose a partial
database planner as one reflective standard-library call and conflict with the
typed, compositional list model.

Replace them with ordinary typed list pipelines:

- `List/filter(item, if:)` and `List/find(item, if:)` for predicates;
- `List/sort_by(item, key:, direction: Ascending)` for the primary order;
- `List/then_by(item, key:, direction: Ascending)` for additional
  lexicographic keys;
- `List/take(count:)` for a bounded list;
- `List/page(size:, after:)` for revision-bound keyset pagination.

The compiler derives logical access requirements from those typed expressions
and chooses physical indexes. Index declarations, residual plans, query modes,
field paths, normalization policy, and selected physical indexes are not Boon
arguments. One canonical keyed LIST remains the row authority. Persistence
stores that authority once; compiler-generated indexes are reconstructable,
bounded runtime machinery rather than a second durable query database.

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
- an incremental filtered view;
- a bounded scan for a statically small collection.

Changing the physical index must not change a Boon value, invalidate a cursor
whose semantic view and revision are still available, or require source edits.

The compiler may reject a server or persistent query when it cannot prove a
bounded access path under the target profile. That diagnostic is an execution
budget failure, not a request for the developer to spell a physical index in
Boon. The first implementation keeps the generated access kernel in hot memory
on native and browser targets. It does not synchronously query redb or
IndexedDB from a request, input, layout, or render path.

## Canonical Boon API

The examples in this section define the intended shape. Exact typechecker syntax
follows the canonical OUT and named-argument rules in
`BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md`. The contextual
formal is named `item` in every operator below. A direct call creates that fresh
binding with bare `item`; a wrapper forwards its own compatible `OUT` with
`item: wrapper_item`.

### Typed Filtering

Exact, compound, range, prefix, union, and intersection behavior is expressed
with ordinary predicates:

```boon
oslo_stations:
    stations
    |> List/filter(item, if:
        item.city == selected_city
    )

matching_stations:
    oslo_stations
    |> List/filter(item, if:
        item.name
        |> Text/trim()
        |> Text/to_lowercase()
        |> Text/starts_with(prefix: normalized_search)
    )
```

Repeated filters express conjunction. `Bool/and` and `Bool/or` express explicit
Boolean composition; `Bool/or` must exist as an ordinary typed standard
function before disjunction examples migrate. A range is two comparisons.
Token membership is an ordinary typed token-list predicate. There is no
`select:` mode and no user-visible residual mode.

### Typed Ordering

`List/sort_by` is the canonical primary-order operator. The repository already
uses that spelling in BUILD programs, so the replacement must finish one generic
implementation rather than introduce a competing `List/order_by` name.

```boon
ordered_stations:
    matching_stations
    |> List/sort_by(
        item
        key: item.city |> Text/to_lowercase()
        direction: Ascending
    )
    |> List/then_by(
        item
        key: item.name |> Text/to_lowercase()
        direction: Ascending
    )
```

The canonical signatures are:

```text
List/sort_by(list, item: OUT, key, direction: Ascending) -> LIST<T>
List/then_by(list, item: OUT, key, direction: Ascending) -> LIST<T>
```

`List/then_by` refines equal groups from the preceding order. A chain forms a
typed lexicographic key without tuples, positional field names, or
comma-separated metadata. Both operations are stable: rows equal under every
declared key preserve their current semantic source-list order. A second
`List/sort_by` starts a new primary order rather than appending a key.

The checked program carries an order-chain qualifier that `sort_by` creates and
`then_by` extends. It is compile-time information, not a Boon value or runtime
wrapper. Direct calls and transparent user wrappers preserve it. Filtering,
one-to-one keyed mapping, and `take` preserve a compatible chain; reordering,
many-to-one/expanding transformations, incompatible branches, and unknown
calls clear it. Calling `then_by` without a compatible preceding chain is a
compile error. The qualifier is erased after access planning.

The initial orderable values are finite `NUMBER`, ordinal `TEXT`, `BOOL`, and
closed fieldless tags. Objects, LIST, BYTES, events, effects, and error-capable
keys are rejected. Text normalization is explicit and its semantics/version
participate in the view fingerprint. Each compound key component owns its
direction. Reversing one physical traversal is valid only when it implements
the complete requested direction vector; `(A ascending, B descending)` is not
implemented by reversing an `(A ascending, B ascending)` index.

### Bounded Results

```boon
preview:
    ordered_stations
    |> List/take(count: 20)
```

`List/take` is a lazy bounded view preserving item type and source row identity.
It stops upstream work only when the chosen access plan proves that the prefix
is complete. An unindexed sort of an allowed small LIST may still inspect the
complete input. Operator order is semantic and may not be changed without a
proof: `sort |> take` is a global top-N, while `take |> sort` sorts only the
source prefix.

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

`List/page` is a terminal operation over a deterministic list view, not another
lazy LIST node. Plain LIST sequence order is deterministic, so sorting is not
required; an ordered chain pages in its declared order. The result preserves
the row type:

```text
Page[items: LIST<T>, next: End | Cursor[value: BYTES]]
| PageExpired
| InvalidPageCursor
| InvalidPageSize
| PageWorkLimitExceeded
```

An empty byte string is not an end-of-page sentinel. Callers match `End` or
`Cursor`. Cursor bytes are opaque application data: they may be stored and sent
back, but modifying them can only produce `InvalidPageCursor`.

`InvalidPageCursor` deliberately exposes no parser, authentication, tenant, or
schema detail to application code. The dev inspector and bounded diagnostics
may report the internal reason without turning it into a public oracle.

`size` is a finite whole `NUMBER`. Invalid literal sizes are compile errors;
dynamic values outside `1..=10000` return `InvalidPageSize`. Products may impose
smaller typed boundary limits, such as FjordPulse's external `1..=50`. No page
evaluation may exceed the target candidate/residual budget; exhaustion returns
`PageWorkLimitExceeded` rather than scanning farther or returning a misleading
partial page. `take |> page` pages only that bounded view, and the evaluated
take count participates in cursor identity.

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
- canonical typed pipeline identity and every transitively called pure function
  semantic version;
- canonical evaluated row-independent captures, including selected values,
  normalized prefixes, range bounds, token sets, dynamic direction, upstream
  `take` count, and normalization inputs;
- source authority revision;
- last ordered key components;
- the source-order position token and hidden stable row identity needed to
  resume duplicate-key groups without changing their visible stable order;
- order directions;
- hidden owner/Session generation, authorization scope, and tenant scope when
  the view is scoped.

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

Together these values form a versioned `ViewInstanceFingerprint`. Evaluated
captures are encoded canonically, so a cursor for Oslo cannot page Bergen and a
cursor from one Session/tenant cannot cross into another. Page size is excluded
so a caller may request a different valid size; every semantic upstream bound
is included. Hidden ownership participates in the host-side fingerprint but is
never exposed as Boon data.

The next page performs a direct ordered seek after
`(last_key, source_order_token, row_identity)`. It must not regenerate the
complete candidate set and discard earlier rows.

For untrusted external transport, the host uses a versioned confidentiality-
preserving authenticated seal; authentication without encryption is
insufficient because hidden row and scope identity must not be disclosed. The
host maps opaque cursor BYTES to bounded base64url only at an external HTTP
boundary. Internal positional CBOR keeps it as bytes. Host keys, hidden IDs,
and decoded cursor contents never enter Boon state, reports, persistence, URLs,
or cursor-visible fields. The host defines bounded token size, key rotation,
restart behavior, and accepted key versions. A browser-local host may use an
ephemeral key when durable secure key storage is unavailable, but then reports
that cursors expire across reload instead of silently accepting them.

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

The typed graph also carries the order-chain qualifier described above and a
closed operation-order boundary. Optimizers may not commute `filter`, `map`,
`sort_by`, `then_by`, `take`, or terminal `page` unless equivalence, stable
order, currentness, and work bounds are proven. Branches must agree on item
type and compatible order provenance or the order qualifier is cleared.

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

Maintain three distinct identities:

- semantic pipeline identity for typed operator/function semantics;
- view-instance fingerprint adding evaluated captures, authority revision, and
  hidden owner/authorization scope, used by currentness and cursors;
- physical access-plan identity, used by generated index storage and rebuilds.

A new physical index or equivalent traversal changes only physical identity.
Changing predicates, key functions, ordering, source memory, schema, evaluated
captures, revision, or hidden scope changes the relevant semantic/view-instance
identity and invalidates old cursors explicitly.

### Boundedness

The compiler records a target-profile candidate and output budget. For a large
server/persistent list, `take` or `page` must have a provably bounded upstream
access plan. A small closed LIST may use a measured scan. No optimizer miss may
silently become an unbounded server scan.

Diagnostics explain the semantic predicate that lacked a bounded access path.
They do not tell the user to provide string field names or an index ID.

Index inventory is static and bounded. The compiler deduplicates equivalent key
projections, reuses compatible compound prefixes, and chooses an index only
when its measured target cost fits profile limits. Recognizing an indexable
predicate does not authorize creating an index for every `List/filter`.
Compilation rejects plans exceeding limits for indexes per list, key parts,
encoded key bytes, expanded keys per row, estimated index bytes, or per-turn
affected-index fanout.

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

The index maps ordered structural keys to source-order tokens and stable
`RowId`s. Row values remain in the canonical list storage. Index entries never
duplicate complete records. Stable equal-key order requires an order-maintenance
token whose update cost is bounded and measured; rebuilding every shifted
position after an insertion is not an accepted large-list implementation.

Required operations are:

- exact seek;
- lower/upper range seek;
- text-prefix range seek over an explicitly normalized Text key;
- direction-vector-aware traversal with one canonical order-preserving key
  codec shared across native and Wasm;
- lazy bounded k-way union and bounded intersection driven by the narrowest
  compatible access range, without materializing complete candidate sets;
- direct seek after a page cursor key;
- incremental insert, remove, and old-key/new-key update;
- deterministic source-order continuation and row-identity disambiguation;
- index rebuild and integrity validation.

Index key expressions are row-only pure expressions over constants and total
pure functions. Effects, host time, error-capable calls, and mutable global
captures are rejected as physical index keys. Demand-current row fields cross
an explicit currentness barrier during build/update and may not cause eager
evaluation of unrelated expensive fields such as Cells values/errors.

The runtime registers precise dynamic dependencies for the list structure,
queried key interval, and row fields evaluated by the residual predicate. A
compiled field-dependency mask updates only indexes and views whose key or
predicate inputs changed. A change outside the demanded interval must not wake
or rebuild the complete result. No index may be created on the first user
interaction.

Execution uses seekable access iterators. Exact/range/prefix paging targets
`O(log E + V)` work and `O(page size + active branch count)` transient memory,
where `E` is index entries and `V` is visited candidates. Union/intersection,
residual work, and dynamic invalidation have hard candidate, memory, fanout,
and per-turn budgets with explicit failure rather than fallback scans.

Metrics report logical rows, index seeks, key ranges, candidates, residual
evaluations, returned rows, cursor seeks, index updates, rebuilds, bytes, and
full scans. They must distinguish semantic list size from materialized and
examined rows. Reports also include allocation bytes, index bytes, expanded
keys, affected-index fanout, dependency wake count, startup rebuild work/time,
and work-limit failures.

## Persistence Architecture

Canonical LIST rows and authority revisions are stored once through the existing
`boon_persistence` redb/IndexedDB tables and transactions. Generated indexes are
derived runtime machinery and are not another durable authority, journal, or
transaction system.

At cold start or activation, the host restores canonical rows, builds every
required generated index into a candidate runtime image, validates it, and only
then publishes readiness. Native rebuild work runs off the render/input thread.
Browser rebuild work is worker-backed or bounded and cooperatively yielded; it
must not freeze the browser main thread. Corrupt authority fails closed.
Failure to build a required bounded access path fails readiness rather than
publishing an unindexed application.

Normal list access then uses the same hot in-memory kernel on native and Wasm.
Neither redb nor IndexedDB is queried from an application request, Session turn,
input frame, layout pass, or render hook. Authority mutations update canonical
runtime rows and affected indexes in one prepared/committed turn, then enqueue
the existing bounded persistence delta. Restore/rebuild equivalence proves that
the derived image can always be reconstructed from canonical authority.

Delete `boon_query_redb`, its index tables, journal, cloned collection state,
and direct persistent query path. Delete the old public `boon_query` crate/name
as well; useful index algorithms move into one generically named list-access
kernel such as `boon_list_access` or the canonical executor ownership layer.
Useful corruption and restart fixtures move to `boon_persistence`.
A future dataset that cannot fit its required indexes in the declared memory
budget needs an explicitly designed Database/domain capability. It does not
justify turning ordinary `List` into a hidden cross-target disk database.

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

"Optional" describes specialized acceleration, not FjordPulse behavior.
FjordPulse must still express and pass exact station ID, normalized/multi-token
prefix, bounded one-edit correction, spatial-cell plus exact WGS84 residual,
time ranges, union/intersection, compound order, atomic uniqueness, and dynamic
`1..=50` paging through these typed operators. Its first implementation may use
typed precomputed spatial/text keys and bounded residuals instead of a dedicated
R-tree or full-text engine, but may not scan the 58,500-row catalog.

## Cross-Target Contract

The logical view, key codec, stable ordering, cursor payload, and reference
results are byte-for-byte deterministic across native and
`wasm32-unknown-unknown`. Persisted/cursor fields use fixed-width integers and
never `usize`.

The deterministic access kernel owns no filesystem, thread, wall-clock, or
`std::time::Instant` dependency. Hosts supply timing/instrumentation around it;
the browser implementation must not call APIs that panic on Wasm. IndexedDB
stores canonical authority only and is accessed asynchronously through the
existing Rust host. Denied or evicted browser storage produces explicit
durability status while the current in-memory application may continue; it
never changes query semantics or silently moves disk work onto the UI thread.

## Clean Implementation Sequence

### 1. Freeze Replacement Semantics

- Add canonical signatures and flow types for typed `List/sort_by`,
  `List/then_by`, `List/take`, and `List/page`.
- Specify stable ordering, order-chain propagation, operator-order semantics,
  key types, dynamic page bounds, closed page variants, evaluated-capture
  fingerprints, cursor sealing, and expiry in `LANGUAGE_SEMANTICS.md`.
- Add compile-time negative tests for wrong OUT use, impure keys, unsupported
  directions, invalid `then_by` chains, invalid literal page sizes, unsupported
  keys, and invalid cursor variants.

### 2. Build Typed Logical List Plans

- Lower filter/order/take/page pipelines from ErasedProgram only.
- Preserve generic row type through every operator and through `Page.items`.
- Add semantic view-instance hashing over typed expressions, transitive pure
  function semantics, evaluated captures, hidden owner scope, and authority
  revision, independent of source formatting and physical indexes.
- Preserve or clear checked order-chain provenance according to the explicit
  operator rules, then erase it after access planning.
- Extend the existing typed equality lookup inference instead of creating a
  second query extractor.

### 3. Unify Index Storage

- Generalize the existing typed equality index into ordered typed keys.
- Add exact, range, prefix, forward/reverse, union/intersection, and cursor
  seek operations.
- Use one canonical direction-aware key codec and seekable lazy access iterator
  on native and Wasm.
- Add bounded source-order maintenance, field-dependency masks, index inventory
  deduplication, compatible prefix reuse, and per-list memory/fanout limits.
- Remove whole-record query mirroring and string-key reconstruction.
- Keep one source-list authority and one row identity.

### 4. Implement Lazy Bounded Operators

- Make filter/order chains lazy keyed views with stable equal-key order.
- Make `take` stop upstream work after enough accepted rows only when the
  access plan proves prefix completeness.
- Make `page` seek after the cursor and read at most `size + 1` accepted rows.
- Return `PageExpired` after a bound revision is unavailable.
- Return the closed page size, work-limit, expiry, and cursor variants without
  arbitrary Text error codes.
- Seal untrusted cursors in the host and prove scope/capture isolation.

### 5. Integrate Restore And Persistence

- Store canonical rows and authority revision once through
  `boon_persistence`; store no duplicate query authority or journal.
- Rebuild and validate required hot indexes before readiness on native and Wasm.
- Update affected hot indexes in the same runtime authority turn and persist
  only canonical bounded deltas through the existing worker.
- Delete the old `boon_query`/`boon_query_redb` crates and prove no redb/
  IndexedDB access occurs on request, Session, input, layout, or render paths.
- Verify restart, physical-plan replacement, authority revision, corruption,
  browser storage failure, and current-only cursor expiry.

### 6. Migrate Boon Source And Tests

- Replace FjordPulse station prefix search with typed normalization, filter,
  order, and take/page operators.
- Replace compound, number-range, tag, union, intersection, mutation, and page
  fixtures with ordinary typed pipelines.
- Replace query-time uniqueness tests with atomic typed existence-check plus
  append tests.
- Add `Bool/or` as an ordinary typed function before migrating disjunctions.
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
- the old `boon_query` crate/public API after useful typed index algorithms move
  into the one canonical list-access kernel;
- the `boon_query_redb` crate, tables, journal, direct range execution, and
  duplicate persistence authority;
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
- direct calls use canonical `item`; wrappers may forward a typed OUT with
  `item: wrapper_item` but cannot rename ordinary arguments.
- `then_by` accepts only a compatible checked order chain; preservation,
  invalidation, wrapper, and branch cases are covered.
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
- stable equal-key source order and every mixed direction vector;
- `sort |> take`, `take |> sort`, `filter |> take`, and `take |> page` without
  illegal operator reordering;
- take and every page boundary.

The reference evaluator is test-only and must not become a runtime fallback.

### Index And Currentness Tests

- recognized equality, range, and prefix predicates use typed indexes;
- no field-name reflection occurs in runtime execution;
- one row update changes only affected index entries and dependent views;
- unrelated field updates touch zero indexes whose dependency masks exclude the
  field;
- unrelated row changes do not rebuild the complete result;
- indexed and reference results remain exactly equal after inserts, updates,
  removes, restore, and migration;
- one canonical list owns every row value and hidden identity.

### Pagination Tests

- first and subsequent pages have deterministic, nonoverlapping rows;
- equal declared keys retain source order and resume through a hidden
  source-order position without changing visible order;
- second-page work begins at the cursor seek and does not revisit earlier keys;
- `End` and `Cursor` are explicit variants;
- invalid literal size fails compilation; invalid dynamic size and work-budget
  exhaustion return their closed variants;
- same semantic view and current revision accept a cursor;
- changed evaluated capture, Session/tenant scope, authorization scope, take
  count, or transitive function semantics rejects a cursor;
- mutation with current-only retention returns `PageExpired`;
- another list, schema, predicate, order, or direction returns
  `InvalidPageCursor`;
- a physical index replacement alone does not alter semantic cursor identity;
- malformed and tampered external cursors fail authentication/validation;
- external tokens disclose no hidden row, owner, Session, tenant, or schema
  identity; key rotation/reload behavior is explicit;
- page work and memory stay within configured budgets.

### Persistence And Cross-Target Tests

- canonical row updates commit atomically and derived index updates observe the
  same runtime authority turn;
- restart preserves canonical rows and authority revision, then rebuilds an
  equivalent hot index image before readiness;
- incompatible physical indexes rebuild without changing semantic rows;
- corrupt authority or failed required-index build fails readiness closed;
- native redb and browser IndexedDB are untouched by normal query/request/input
  execution after readiness;
- native and Wasm produce identical order, cursor payload identity, page
  results, expiry, and work counters for golden fixtures;
- browser rebuild is worker-backed or yielded and does not block the main
  thread; denied/evicted persistence reports durability failure honestly;
- no process handle, iterator pointer, credential, authentication/sealing key,
  or hidden owner identity enters Boon-visible state or reports.

### Product And Performance Tests

- FjordPulse executes search over the required 58,500-station fixture through
  typed filter/order/take/page source;
- exact, prefix, compound, range, union/intersection, spatial-cell residual,
  and deep-page paths report zero full scans;
- first/deep page latency, candidate visits, allocations, index bytes, startup
  rebuild, mutation fanout, and dependency wakes remain within declared native
  and browser budgets;
- Cells typed address lookup remains zero-scan and unaffected by this change;
- Cells startup does not eagerly evaluate value/error or construct an index on
  first click, and its exact click/scroll frame gates remain passing;
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
- `boon_query`/`boon_query_redb`, their public query contracts, persistent
  index tables/journal, and request-time persistent access;
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
   terminal/error variants, binds every evaluated capture and hidden scope,
   preserves stable order, and seeks directly after the cursor.
5. One deterministic hot index kernel serves native and Wasm. Canonical
   redb/IndexedDB authority rebuilds it before readiness; neither persistence
   backend is queried on normal access paths, and both old query crates/names
   are deleted after useful algorithms move to the one list-access kernel.
6. FjordPulse's full station fixture and Cells address lookup pass their typed
   zero-scan, bounded-work, memory, startup, and frame gates without
   example-specific code.
7. The relevant compiler, plan, executor, persistence, example, and aggregate
   native/Wasm/browser verification suites pass from final source with fresh
   artifacts.
8. Independent reviews find no reflective query metadata, duplicate authority,
   hidden runtime fallback, compatibility alias, or stale report used as proof.

Do not mark the larger unified goal complete merely because this replacement
plan passes. Distributed/session work, streaming, NovyWave, Cells frame budgets,
and FjordPulse's remaining acceptance conditions retain their own end gates.
