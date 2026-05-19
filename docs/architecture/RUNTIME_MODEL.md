# Runtime Model

Boon Circuit is a static scheduled runtime over value slots, state cells, source
ports, and keyed list memories.

## Data Structures

Illustrative Rust shape:

```rust
struct Runtime {
    values: Vec<Value>,
    changed_at: Vec<TickSeq>,
    dirty: BitSet,
    schedule: Vec<NodeId>,
    cells: CellStore,
    lists: ListStore,
    sources: SourceStore,
    deltas: DeltaBuffer,
}
```

The exact implementation may use typed indexes, arenas, and specialized value
storage, but the conceptual boundary should stay this small.

## Compile-Time IR

The compiler should lower Boon into a typed equation graph:

```text
NodeId
ExprId
ScopeId
SourceId
CellId
ListId
FieldId
```

Important node kinds:

```text
Const
SourceRead
PureCall
Record
FieldAccess
When
Then
While
Latest
Hold
ListLiteral
ListAppend
ListRetain
ListMap
RenderForEach
```

The graph is static. Dynamic list rows are runtime keys, not node ids.

## Tick Phases

Each runtime tick follows deterministic phases:

1. Ingest source events.
2. Mark directly affected nodes and keyed scopes dirty.
3. Evaluate scheduled pure/event nodes.
4. Collect candidate writes for `HOLD` cells and list memories.
5. Resolve conflicts with `LATEST`, `PRIORITY`, or `EXCLUSIVE` semantics.
6. Commit state cells and list structural changes.
7. Emit semantic deltas.
8. Lower semantic deltas to render/network/persistence deltas as needed.

No stateful value should commit in the middle of evaluation. This gives
snapshot-style semantics: all next-state equations see the previous committed
state plus current tick inputs.

## Dirty Propagation

Scalar dependencies use ordinary dependency edges:

```text
source -> expression -> hold_next -> hold_commit -> derived expression
```

Indexed dependencies carry scope/key information:

```text
todo.completed[t] changed
  -> todo.visible[t]
  -> active_count
  -> footer.render
```

The engine should avoid marking all rows dirty when a keyed event only affects
one row. Broadcast events such as `ClearCompleted` or `ToggleAll` may scan or
use indexes.

## State Cells

A scalar `HOLD` has one current value and one pending value.

```text
current: Value
pending: Option<Value>
changed_at: TickSeq
```

An indexed `HOLD` has column storage:

```text
current[key]: Value
pending[key]: Option<Value>
changed_at[key]: TickSeq
```

The compiler gets indexed cells by seeing a `HOLD` inside a list item scope.

## Scope Paths

Scope paths are hidden runtime addresses for state inside dynamic data:

```text
/                                           root
/todos:42                                  todo item
/projects:7/todos:42                       nested todo
/projects:7/todos:42/comments:3            nested comment
```

A stateful expression is identified by:

```text
(program_hash, expr_id, scope_path, generation)
```

The generation prevents stale events from mutating a deleted row whose storage
slot was reused.

Scope paths and generations are not Boon values. They cannot be read, compared,
stored, or pattern-matched by Boon code. They exist only below the language
boundary.

## Conflict Handling

Multiple candidate writes to the same cell in one tick are not silently
undefined.

The first pass should implement:

- `LATEST`: choose by event sequence, tie-break by source order.
- diagnostics for equal-priority ambiguous writes.

Later:

- `EXCLUSIVE`: runtime or compile-time proof that at most one arm fires.
- `PRIORITY`: source order is explicit and intentional.

## Debugging Requirements

The runtime must be able to answer:

```text
What is the current value of todo.completed[42]?
What changed it last?
Which source event caused that?
Which equations can change it?
Which derived values became dirty because of it?
```

This is required to preserve the original actor-engine readability.
