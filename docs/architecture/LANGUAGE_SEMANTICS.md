# Language Semantics

This file captures the intended operational meaning of the Boon constructs that
matter for the circuit engine.

## Data Equality

Boon values are data. Equality compares data structurally.

There is no Boon-level object identity, reference identity, row identity, slot
identity, or pointer identity. Runtime keys may exist below the language for
retention, source routing, stale-event rejection, rendering, and network deltas,
but Boon code cannot read or compare them.

If input data contains a field named `id`, it is still ordinary data. Comparing
that field compares its value, not a hidden runtime object.

## Number

`Number` is one finite IEEE-754 binary64 semantic type. Integer and decimal
literals do not create different value kinds. NaN and infinities are rejected,
and negative zero is normalized to positive zero for equality, ordering,
hashing, persistence, and protocol identity.

Arithmetic operators produce `Number`. In particular, `/` is real division:
`1 / 2` is `0.5`. Division or remainder by zero and arithmetic that would
produce a non-finite result produce a typed error instead of a non-finite
value.

Whole-number operations are explicit:

- `Number/floor()` returns the greatest whole Number no greater than its input.
- `Number/ceil()` returns the least whole Number no less than its input.
- `Number/round()` rounds to the nearest whole Number, with exact halves away
  from zero.
- `Number/truncate()` rounds toward zero.

List positions, byte offsets, byte counts, and other bounded indices accept
only an already-whole, non-negative, in-range Number. They do not round
implicitly. Code that derives an index from real arithmetic must select the
intended rounding operation first.

`Number/to_text()` keeps the same single Number type and never infers or exposes
an integer storage type. With no formatting options it accepts every finite
Number and produces its ordinary decimal representation:

```boon
label: 12.5 |> Number/to_text()
```

Integer-oriented formatting is explicit:

```boon
bits: 42 |> Number/to_text(radix: 2, min_width: 8, group_size: 4)
hex: 42 |> Number/to_text(radix: 16, prefix: True)
signed: 255 |> Number/to_text(signed_width: 8)
```

These produce `0010 1010`, `0x2a`, and `-1`. Supplying any integer-oriented
option requires an exactly whole Number; the builtin does not round. `radix`
is `2..=36`, `min_width` is `0..=4096`, `group_size` is `1..=4096`, and
`signed_width` is `1..=63`. A signed width interprets a non-negative Number as
a two's-complement bit pattern of exactly that width and rejects values that do
not fit. Prefixes are available only for radix 2, 8, and 16. Invalid options,
non-whole inputs, and output requests beyond these bounds are typed errors, not
implicit conversions or unbounded allocations.

## Functions, Calls, And OUT

All functions, including standard-library functions and user-defined wrappers,
use one call model. Every call has parentheses. Ordinary inputs are named with
their exact declared parameter names and appear in declaration order. There are
no ordinary positional arguments and callers cannot rename parameters. A pipe
supplies only the first declared ordinary parameter; it does not create a
second receiver convention.

An output parameter is declared with `OUT` in a function signature:

```boon
FUNCTION map(list, item: OUT, new) {
    ...
}
```

A bare call entry creates the fresh output with that exact canonical name:

```boon
items
|> List/map(
    item
    new: item.value * 2
)
```

Bare entries are reserved for fresh `OUT` bindings; they are never ordinary
values. The equivalent non-piped form is:

```boon
List/map(
    list: items
    item
    new: item.value * 2
)
```

A wrapper forwards an existing output with a named connection. The formal name
remains the callee's canonical parameter name, while the expression on the
right resolves to the wrapper's output:

```boon
FUNCTION map_values(dictionary, entry: OUT, new) {
    Dictionary[
        ...dictionary
        entries:
            dictionary.entries
            |> List/map(
                item: entry
                new: [
                    key: entry.key
                    value: new
                ]
            )
    ]
}

dictionary
|> Dictionary/map_values(
    entry
    new: entry.value * 2
)
```

`item: item` forwards an output with the same local name and `item: entry`
forwards a compatible output with a different local name. Neither form renames
the formal parameter. `item: OUT` is declaration syntax and is invalid at a
call site. Unknown, missing, duplicated, renamed, or out-of-order entries are
compile errors, as is forwarding an ordinary value to an `OUT` formal.

`OUT` is static contextual wiring supplied by the called function. It is not
mutable storage, a `SOURCE`, a stream, a nominal value, a runtime handle, or a
serializable field. User-defined direct, one-wrapper, and multi-wrapper forms
must elaborate to equivalent executable operations and ownership. The compiler
rejects zero-driver, multiple-driver, incompatible, and cyclic output nets.

## PASS Context

`PASS` is the sole reserved call-context clause:

```boon
Components/button(
    source: store.submit
    label: TEXT { Submit }
    PASS: [store: PASSED.store]
)
```

`PASS:` may occur at most once and must be the final call clause, including
when preceding argument expressions read `PASSED` values. It is represented
separately from the declared value and `OUT` parameter list and therefore does
not affect function arity or parameter order. `PASS` cannot be a function
parameter, pipe receiver, output, persisted field, serialized value, or runtime
value. `PASS` and `PASSED` are reserved language context names.

## Order-Independent Lexical Bindings

Declarations are collected before references are resolved within a lexical
scope. Functions, modules, `BLOCK` variables, explicit record fields, and fresh
call outputs are visible throughout their scope independently of textual order:

```boon
BLOCK {
    result: doubled + 1
    doubled: input * 2
}
```

The compiler resolves this dependency graph and schedules `doubled` before
`result`. This does not relax call syntax: call entries still use exact names
and declaration order because position is part of the API and diagnostic
contract, not an evaluation-order mechanism.

A local declaration shadows an outer declaration throughout its entire scope.
Explicit record fields are sibling lexical declarations, so `[item: item]` is
self-referential; copying an outer same-name value requires an explicit outer
alias. Spread fields do not introduce lexical bindings. Instantaneous type,
output-alias, value, temporal, or distributed cycles are rejected. A temporal
cycle is valid only when a real boundary such as `SOURCE`, `HOLD`, publication,
or asynchronous effect completion breaks it.

## Checked And Erased Programs

`boon_typecheck` produces the authoritative `CheckedProgram`: stable lexical
declaration identities, resolved callable identities, exact typed call entries,
contextual signatures, semantic occurrences, scope effects, correlations, and
typed collection predicates and projections.

`boon_ir` elaborates contextual calls, validates and unifies the static output
net, expands transparent user wrappers in their declaring runtime island, and
produces the authoritative `ErasedProgram`. The erased program has canonical
executable operations and structural ownership anchors, with no runtime `OUT`,
`PASS`, transparent wrapper call, positional binder, or parser-level call
ambiguity. Machine, document, distributed, persistence, native host, and
verifier backends consume only `ErasedProgram`; they do not rediscover call or
context semantics from parser AST or function-name strings.

## Standard Runtime Namespaces

The parser owns one canonical registry of standard-library, runtime, and
program-role roots. Application modules, root fields, and function namespaces
cannot shadow a registered root such as `List`, `File`, `Element`,
`SessionInfo`, `Client`, `Session`, or `Server`.

`Client`, `Session`, and `Server` are program-role roots. A qualified value uses
the role root followed by `/` and an ordinary dotted value path. A qualified
function uses the same `/` function namespace syntax as every other Boon call.
For example, Session may reference its two adjacent islands:

```boon
query: Client/store.query
results: Server/search(query: query)
```

The old `Client.store.query` spelling is invalid. Same-role qualification is
also invalid; local declarations are referenced without a role root.

`SessionInfo` exposes current runtime context through ordinary mandatory-call
syntax:

```boon
status: SessionInfo/status()
principal: SessionInfo/principal()
```

`SessionInfo/status()` returns `Connecting`, `Current`, `Stale`, or
`Failed[code]`. It is available in Client and Session. Server may read it only
while evaluating a branch scoped to one originating Session.

`SessionInfo/principal()` returns `Anonymous` or
`Authenticated[subject, roles]`, where `roles` is a bounded canonical list of
text role names. It is available to Session and to a Session-scoped Server
branch, but never directly to Client. A Session application must explicitly
derive and expose any account-facing data that Client is allowed to observe.

These values are generated from host-private runtime context. They are not
application state, host-injected `SOURCE` values, or durable records. Session
IDs, connection IDs, resume tokens, credentials, correlation IDs, hidden keys,
and generations are never Boon values and never appear in these results.

A distributed application consists of one Client graph in each browser tab,
one resumable Session island owned by that tab, and one shared Server graph.
Only adjacent role edges are legal: Client and Session may depend on each other,
and Session and Server may depend on each other. Direct Client/Server edges are
rejected. Session is compiled once as an indexed template and instantiated with
isolated state, hidden ownership, generation, and bounded scheduling for each
tab.

A consumer reads a qualified value or calls a qualified pure function with
ordinary Boon syntax. Exports, imports, call sites, replies, and shared demand
subscriptions are derived by the compiler and host; they are not declared in
Boon source. In-process Session/Server edges pass immutable runtime values
directly. Client/Session network events use an exact bounded positional CBOR
frame containing protocol version, graph hash, graph-schema hash, edge ID,
graph revision, sequence, hidden Session generation, and one canonical Boon
wire value. The decoder rejects indefinite, noncanonical, trailing, oversized,
wrong-graph, wrong-schema, stale-generation, unknown-edge, replayed, and
out-of-order frames before dispatch. HTTP, JSON, RPC, serialization, retry
envelopes, and transport handles are not exposed in Boon source.

A distributed combinational cycle is invalid. A cycle is legal only when a
real temporal boundary such as `SOURCE`, `HOLD`, publication, or asynchronous
effect completion breaks it.

Cross-role failure is represented by the declared imported result type and the
runtime's typed transport error result. It is not modeled by changing every
local producer value into an error union inside its owning graph.

## SOURCE

`SOURCE` declares an input port.

Examples:

```boon
click: SOURCE
change: SOURCE
key_down: SOURCE
```

Inside a list item, a source is bound by hidden item scope:

```text
source expr id + /todos:42 + generation
```

The host renderer binds concrete UI events to these structural source ids. Those
ids are not Boon values.

## SKIP

`SKIP` means no value/event is present for this branch in the current tick.

It is not `null`, not `False`, and not an empty list. It is absence.

## THEN

`THEN` gates a block on input presence.

```boon
sources.checkbox.click |> THEN {
    completed |> Bool/not()
}
```

If the input is `SKIP`, the result is `SKIP`. If the input is present, the body
is evaluated against the current tick snapshot.

Hardware analogy:

```boon
PASSED.clk |> THEN {
    next_value
}
```

can lower to edge-triggered logic when the target profile marks `PASSED.clk` as
a clock impulse.

## HOLD

`HOLD` stores the last committed value and updates only at commit time.

```boon
False |> HOLD completed {
    sources.checkbox.click |> THEN {
        completed |> Bool/not()
    }
}
```

For the first interpreter, the piped expression is the initialization value for
the state cell or row field. Dynamic resets are expressed as ordinary update
candidates inside the body. The body defines a next-state equation. The name
after `HOLD` is the previous committed state value visible inside the equation.

For list items, `HOLD` becomes a field memory:

```text
completed[key]
```

## LATEST

`LATEST` merges several candidate values:

```boon
LATEST {
    source_a |> THEN { value_a }
    source_b |> THEN { value_b }
}
```

Rules:

- branches that produce `SKIP` are ignored.
- if exactly one branch produces a value, use it.
- if multiple branches produce values, choose the branch whose value carries the
  greatest monotonic source event sequence.
- pure expressions derived from a source event inherit that source event
  sequence.
- constants and stored values have no event sequence unless they are selected by
  an event/presence gate.
- if two candidates have the same greatest event sequence, that is a hard error
  unless the source uses explicit `PRIORITY` or proven `EXCLUSIVE`.

## WHEN

`WHEN` is pattern matching. It has two typed modes:

- `Value<T> |> WHEN { ... }` is continuous pure selection and recomputes when
  the matched value or branch dependencies change.
- `Event<T> |> WHEN { ... }` is presence-gated event decoding. If the event is
  absent in the current tick, the result is `SKIP`.

```boon
selected_filter |> WHEN {
    All => True
    Active => completed |> Bool/not()
    Completed => completed
}
```

On an absent event input, event-style `WHEN` returns `SKIP`. Value-style `WHEN`
does not have absence unless the value being matched is itself an event/optional
value.

## WHILE

`WHILE` is continuous conditional selection, not an imperative loop.

It is appropriate for combinational conditions that should remain true while
their dependencies remain true.

Cycles through `WHILE` or pure expressions must be rejected unless there is a
`HOLD` in the cycle. The first Rust interpreter enforces this during IR
lowering, before runtime execution.

## LIST

`LIST` is a collection value. In the circuit engine it lowers to hidden keyed
memory plus structural deltas.

Dynamic software:

```boon
LIST {
    ...
}
```

Fixed/profiled:

```boon
LIST[10000] {
    ...
}
```

The syntax should stay close to original Boon. Capacity is a target/profile
constraint, not a reason to force a new app-level collection syntax.

## Canonical List Operations

Per-row collection operations use the same typed `OUT` calls as user-defined
functions. `List/map(item, new: ...)` and `List/filter(item, if: ...)` do not
have a hardcoded template syntax, compiler-only item binder, or caller-selected
output name. A user wrapper forwards the output with ordinary named `OUT`
connection syntax.

Mapping over a dynamic list does not clone semantic graph nodes per row.

It creates a static row-template operator evaluated over active hidden keys:

```text
for each changed key:
    evaluate row template in scope /list:key
```

Semantic `List/map` and `List/retain` never depend on renderer visibility.
Renderer objects may be created/deleted/windowed in the host, but the Boon
equation graph does not change and semantic recomputation is driven by dirty
keys, not by visible rows.

`List/find` performs a typed lookup:

```boon
result:
    cells
    |> List/find(
        cell
        if: cell.address == target_address
    )
```

Its result is `Found[value: CELL] | NotFound` and is handled with ordinary
`WHEN` matching. `List/find_value`, reflective quoted
`field:`/`target:` arguments, and an embedded `fallback:` are not language
forms. Typed predicate equality may select a compatible compiler-owned index;
otherwise bounded scan work remains explicit and measurable.

`List/chunk` has one canonical result shape:

```boon
rows:
    cells
    |> List/chunk(size: 26)
```

Each chunk exposes `.items` and `.label`. These fields are supplied by the
operator and cannot be renamed by caller arguments. `.items` is a lazy keyed
slice preserving source item identity; `.label` is the canonical chunk label
or index.

## Compiler-Owned Indexed Queries

`List/query` declares a bounded query over one keyed `LIST`. The declaration is
closed metadata: index fields, normalization, multiplicity, order, selection,
residual kind, and limit are known when the machine plan is compiled. A host or
application cannot inject an index name or an unplanned query shape.

```boon
page:
    catalog
    |> List/query(
        fields: TEXT { city,name }
        normalization: TEXT { TrimLowercase,TrimLowercase }
        select: Prefix
        leading: city_key
        prefix: name_prefix
        limit: 20
        unique: False
        order: Ascending
        residual: None
        cursor: previous_page.cursor
    )
```

The result is a closed page record:

```text
[rows: LIST<Row>, cursor: BYTES]
```

An empty cursor means there is no next page. A non-empty cursor is opaque. It
binds the collection identity, recursive row-schema hash, index identity, query
fingerprint, collection epoch, last ordered key, and stable row identity.
Changing authority, schema, index projection, query bounds, residual, order, or
limit makes an old cursor fail explicitly; it never restarts silently.

Index declaration arguments:

- `fields` is a comma-separated, ordered list of closed row-field paths. One to
  eight fields form a compound tuple key.
- `normalization` is one entry per field, or one shared entry: `Exact`,
  `TrimLowercase`, or `Tokens`. Non-Text fields require `Exact`.
- `multi_value` is an optional comma-separated subset of `fields`. A list-valued
  projection or one `Tokens` projection creates several index keys without
  duplicating authority rows. At most one field may expand.
- `unique` defaults to `False`. Conflicting inserts or updates fail atomically.
- `order` is `Ascending` or `Descending`. Every order ends with hidden stable
  row identity as its deterministic tie-break.

Supported key scalars are `Bool`, finite `Number`, `Text`, and closed fieldless
tags. A compound key is supplied as a list or record in declared field order.
Selection contracts are:

- `select: Exact`, with `key`;
- `select: Prefix`, with optional compound `leading` and Text `prefix`;
- `select: Range`, with optional `lower`/`upper` and static
  `lower_inclusive`/`upper_inclusive` flags;
- `select: Union` or `Intersection`, with bounded `keys`.

One optional bounded pure residual may further test only index-selected
candidates: `FieldEqual`, `TextContains`, `NumberRange`, or `Wgs84Radius`. Its
arguments use `residual_field`/`residual_value`, `needle`, `minimum`/`maximum`,
or latitude/longitude field paths plus center and radius values. Residual work
does not authorize a full collection scan.

`limit` is mandatory and must be `1..=10000`. Query execution has a separate
bounded candidate budget. A declared indexed query fails on an unknown index,
stale cursor, invalid key, excessive expansion, excessive candidates, or
corrupt index authority. It never falls back to `List/filter` or a full scan.
Metrics identify the selected index and report ranges, keys visited, candidates,
rows examined, residual evaluations, returned rows, cursor production, elapsed
time, and full scans. The full-scan count for `List/query` must remain zero.

Inserts, field updates, removals, restore, migration, and retention update or
rebuild derived index state from the same canonical row authority. In-memory
and redb execution use the same index projection and query engine. Interactive
sessions query current committed in-memory authority; redb is not touched by a
render or input frame.

`List/query_prefix` remains source-compatible shorthand for a single ascending
Text-prefix index and returns only the row list. It lowers through the same
compiler-owned collection/index plan and canonical query engine; it is not a
second executor implementation.
