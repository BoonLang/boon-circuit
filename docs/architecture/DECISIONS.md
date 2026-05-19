# Architecture Decisions

This file records the decisions for the Boon Circuit engine. It is intentionally
opinionated: the point is to prevent the new experiment from drifting back into
either the old actor runtime or a reducer-style app model.

## D1. Build A Static Circuit-Like Engine

Decision: Boon Circuit uses a static equation graph plus indexed state storage.

The semantic graph is fixed after compile/elaboration. Dynamic application data,
such as todos, users, rows, and cells, lives in memories keyed by stable item
identity. Runtime work scales with changed keys and affected dependencies, not
with dynamically instantiated actors.

```text
Boon source
  -> parsed AST
  -> resolved typed equations
  -> static equation graph
  -> schedule
  -> runtime slots, cells, list memories, source ports
```

Rationale:

- The old actor engine preserved local causality but paid high runtime overhead.
- Reducers are fast but hide which values can change each field.
- Hardware already solves this shape: fixed logic around registers and memories.

## D2. Preserve Local Causality

Decision: state changes must remain local to the value being defined.

This shape is rejected:

```boon
FUNCTION update(state, event) {
    event.source |> WHEN {
        ToggleTodo(id) => state |> TodoTable/update(id: id, completed: not completed)
        DeleteTodo(id) => state |> TodoTable/delete(id: id)
        ...
    }
}
```

It is too global. A reader cannot inspect `todo.completed` and see all causes
that may change it.

Preferred shape:

```boon
completed: False |> HOLD completed {
    LATEST {
        sources.checkbox.click |> THEN { completed |> Bool/not() }
        store.sources.toggle_all_checkbox.click |> THEN { store.all_completed |> Bool/not() }
    }
}
```

The runtime may lower this to column mutation, but the source semantics stay
equational.

## D3. Dynamic Rows Are Data, Not Graph Topology

Decision: appending a todo does not create new semantic graph nodes.

Logical view:

```text
todo #42 has title, completed, editing, alive
```

Physical view:

```text
title[42]
completed[42]
editing[42]
alive[42]
```

The compiler/runtime may show per-item dependency explanations, but internally
these are rows in register banks or memory columns.

## D4. Use Existing LIST Syntax First

Decision: do not introduce mandatory user-facing `KEYED LIST Todo BY id
CAPACITY 10000` syntax as the main model.

Boon already planned this distinction:

```boon
LIST { ... }      # dynamic software collection
LIST[10000] { ... }  # fixed-size/profiled collection
```

The circuit engine should make ordinary `LIST` lower to stable keyed storage in
software. Capacity belongs to target profiles and hardware-oriented compilation,
not necessarily to the source app.

Open design issue: source-level syntax may eventually need explicit key policy,
but it should be minimal and compatible with the original TodoMVC style.

## D5. LIST Keys Are Internal By Default

Decision: ordinary `LIST` items have stable runtime keys, but user code should
not see or compare those keys. Boon has data equality, not object identity.

The original plain TodoMVC source has no todo `id`; each row's `title`,
`editing`, `completed`, and element sources are local to the list item. This is
the desired source shape.

The runtime still keeps identity:

```text
list id
item key
generation
scope path
```

Indexes are not identity. They are positions and may change after filtering,
sorting, deletion, or compaction.

These are implementation facts, not Boon values. The Boon developer cannot ask
for the current item key, compare it, or store it as app state.

If input data contains a field named `id`, it remains ordinary data. Equality on
that field compares data, not references or hidden runtime identity.

## D6. SOURCE Is Canonical

Decision: new examples should use `SOURCE` as explicit input ports. Legacy
`LINK` can be supported later as compatibility sugar, but the new engine should
not depend on late-bound actor links.

Source binding must be structural below the language:

```text
program_hash
source_expr_id
scope_path
generation
```

String event names such as `"toggle:3"` are acceptable only as temporary
prototyping glue.

## D7. HOLD Is Storage

Decision: `HOLD` is the only ordinary way to introduce persistent state.

Scalar `HOLD` lowers to a register/cell. A `HOLD` inside a list item lowers to a
field memory indexed by the item scope.

```text
HOLD at root               -> Cell(expr_id)
HOLD inside todos item     -> Cell(expr_id, /todos:key)
HOLD inside nested comment -> Cell(expr_id, /todos:key/comments:key)
```

Commit happens deterministically at the end of a tick.

## D8. THEN Is Event Gating, Not Arbitrary Mutation

Decision: `THEN` evaluates its body only when the input event/value is present.
It does not mutate state by itself.

For software UI events:

```text
SOURCE click |> THEN { next_value }
```

For hardware clocks:

```text
PASSED.clk |> THEN { next_register_value }
```

Clocked hardware lowering treats an impulse source as an edge trigger. The same
semantic operator can remain valid in software.

## D9. LATEST Is A Deterministic Merge

Decision: `LATEST` merges candidate updates by event presence and tick order,
with deterministic tie-breaking.

Rules:

- `SKIP` means no candidate value.
- choose the candidate with the greatest `changed_at` sequence.
- if two candidates have the same sequence, use source order or require an
  explicit conflict policy.
- ambiguous same-tick writes should be diagnosable.

Potential future forms:

```boon
EXCLUSIVE { ... } # compiler/runtime proves only one arm can fire
PRIORITY { ... }  # source order is intentional
LATEST { ... }    # most recent event wins deterministically
```

## D10. WHILE Is Continuous Selection

Decision: `WHILE` is a continuous combinational gate/mux. It is not a loop.

It chooses an output while a condition or selected arm is true, and it recomputes
when dependencies change. Cycles through `WHILE` or pure expressions are errors
unless broken by `HOLD`.

## D11. LIST Deltas Are First-Class

Decision: `LIST` changes propagate as keyed deltas, not whole list snapshots.

Renderer updates, server sync, persistence, and debugger views consume the same
semantic change facts:

```text
Insert(scope, key, fields)
Remove(scope, key, generation)
Move(scope, key, position)
Field(scope, key, path, value)
SourceBind(scope, key, source_path, source_id)
SourceUnbind(source_id)
```

The renderer may lower those semantic facts to direct render patches, but no
layer should need a full DOM or list diff to know what changed.

## D12. Differential Dataflow Is Optional, Not Core

Decision: do not use Differential Dataflow as the primary runtime unless a later
benchmark proves the local engine cannot handle derived relational workloads.

DD may still be useful for:

- large joins
- indexed filters
- transitive dependency queries in Cells
- optional backend experiments

But Boon state ownership remains explicit `HOLD`/field-memory equations.

## D13. FPGA Lowering Uses Profiles, Not App-Level Reducers

Decision: an FPGA target should compile the same local TodoMVC equations with a
hardware profile. It should not require rewriting TodoMVC into a central event
handler or adding app-visible ids only for hardware identity.

The profile supplies:

```text
clock/reset
LIST capacities
text widths and encodings
event FIFO depth
delta FIFO depth
bulk operation latency policy
```

The compiler lowers:

```text
SOURCE       -> input event ports or event bus decoder
HOLD         -> registers/register files
LIST         -> valid bits, order memory, free list, generations
List/append  -> allocation state machine
List/remove  -> valid-bit/order update
bulk ops     -> scan or parallel update engines
deltas       -> output FIFO
```

The source-level values still declare their own next-state equations.

## D14. Start With Rust Static-Graph Interpreter

Decision: the first implementation should be a Rust interpreter over the static
equation graph, not Rust codegen, Zig codegen, or DD.

Rationale:

- Fast enough to validate semantics.
- Easier to inspect and debug than generated code.
- Good fit for a native Ply playground.
- Can still use static schedules, arrays, dirty sets, and keyed memories.

Future targets:

1. Rust static-graph interpreter.
2. Rust codegen from the same typed equation IR.
3. Zig codegen.
4. Hardware-oriented lowering or HDL generation for fixed profiles.
