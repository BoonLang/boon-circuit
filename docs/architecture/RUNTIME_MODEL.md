# Runtime Model

Boon Circuit is a static scheduled runtime over value slots, state cells, source
ports, and keyed list memories.

## Data Structures

Illustrative Rust shape:

```rust
struct Runtime {
    values: TypedSlots,
    changed_at: Vec<TickSeq>,
    dirty: BitSet,
    dirty_keys: DirtyKeySets,
    schedule: Vec<NodeId>,
    state: StateStore,
    lists: ListStore,
    sources: SourceStore,
    deltas: DeltaBuffer,
}
```

The first implementation must use typed indexes, arenas, and specialized storage
for hot paths. Generic `Value` trees are acceptable at parser/debug boundaries,
but normal ticks must not clone whole records, lists, or text buffers just to
detect changes.

Hot-path storage requirements:

```text
Bool/Text/Int/etc. columns are typed.
List order, `BitVec` valid bits, generations, and source bindings are separate
columns.
Text is bounded or interned according to the runtime profile.
Record/list snapshots are not the unit of change detection.
Release verification reports heap allocations and graph rebuilds.
```

## Compile-Time IR

The compiler should lower Boon into a typed equation graph:

```text
NodeId
ExprId
ScopeId
SourceId
StateId
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
6. Evaluate append initializer subgraphs for newly allocated list keys.
7. Commit state cells and list structural changes.
8. Bind sources for newly live row scopes.
9. Emit semantic deltas.
10. Lower semantic deltas to render/network/persistence deltas as needed.

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

The engine must have a keyed-work contract, not only a best-effort dirty bit.
Compile-time lowering builds dependency indexes from:

```text
(source_id, scope_kind) -> affected static operators
(list_id, field_id, scope_kind) -> affected static operators
(aggregate/view id) -> declared input fields/key sets
```

Runtime dirty data is split into scalar dirty nodes and per-list dirty keysets:

```text
dirty_nodes: BitSet<NodeId>
dirty_keys[list_id][field_id]: KeySet
bulk_ops[list_id]: BulkWorkQueue
```

Row templates run only for changed keys, newly inserted keys, removed keys that
need cleanup, or keys explicitly scheduled by a declared bulk operation.
Renderer visibility can limit drawing work, but it must not determine semantic
recomputation. Broadcast events such as `ClearCompleted` or `ToggleAll` either
use maintained indexes or run as explicit bulk work with bounded per-tick
latency.

## State Cells

A scalar `HOLD` has one current value and one pending write slot.

```text
current: typed slot
pending: typed candidate slot
changed_at: TickSeq
```

An indexed `HOLD` has column storage:

```text
current[key]: typed column slot
pending[key]: typed candidate slot
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

For keyed data, the canonical runtime identity tuple is:

```text
(runtime_id, program_hash, list_id, parent_scope, item_key, generation)
```

`scope_path` is the human/debug rendering of that tuple. Generation is not
embedded ambiguously in a path string for protocol logic; it travels as a
separate typed field in source events and deltas.

The generation prevents stale events from mutating a deleted row whose storage
slot was reused.

Scope paths and generations are not Boon values. They cannot be read, compared,
stored, or pattern-matched by Boon code. They exist only below the language
boundary.

## Conflict Handling

Multiple candidate writes to the same cell in one tick are not silently
undefined.

The first pass must implement:

- `LATEST`: choose the candidate with the greatest monotonic source event
  sequence.
- hard errors for equal-sequence ambiguous writes unless an explicit policy wraps
  the candidates.

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

Release builds do not need unbounded history. Keep:

```text
last_writer per state cell/key
last_source_event sequence/hash
static possible-cause table
bounded recent dirty trace when debug mode is enabled
```

Full trace history is a debug/report artifact, not mandatory always-on release
state.
