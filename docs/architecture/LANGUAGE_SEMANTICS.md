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

## List/map

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

## Compiler-Owned Indexed Queries

`List/query` declares a bounded query over one keyed `LIST`. The declaration is
closed metadata: index fields, normalization, multiplicity, order, selection,
residual kind, and limit are known when the machine plan is compiled. A host or
application cannot inject an index name or an unplanned query shape.

```boon
page:
    List/query(
        catalog
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
