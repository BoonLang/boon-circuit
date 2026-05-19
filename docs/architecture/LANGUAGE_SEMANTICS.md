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

Cycles through `WHILE` must be rejected unless there is a `HOLD` in the cycle.

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
