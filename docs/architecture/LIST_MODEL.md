# LIST And Indexed Memory Model

`LIST` is the main pressure point for keeping Boon both dynamic enough for apps
and static enough for circuit-style execution.

## Core Rule

A dynamic item is a key into memories, not a graph instance.

```text
old actor model:
  todo row owns several actors

circuit model:
  todo row is key k in several field memories
```

For TodoMVC:

```text
todos.order        Vec<TodoKey>
todos.valid        BitSet/TodoKey -> Bool
todos.title        TodoKey -> Text
todos.completed    TodoKey -> Bool
todos.editing      TodoKey -> Bool
todos.generation   TodoKey -> Generation
```

`TodoKey` is an internal runtime address. It is not a Boon value and is not the
same thing as a user-visible `id` field.

## Hidden Keys And Data Equality

Ordinal list indexes are positions, not identity. They change when the list is
filtered, sorted, compacted, or when an earlier item is deleted.

```text
before delete: [0:A, 1:B, 2:C]
delete A
after delete:  [0:B, 1:C]
```

If source events or retained state used only ordinal positions, a stale click
from old position `1` could mutate the wrong item after compaction.

The runtime therefore owns hidden stable list keys:

```text
position     visual/order fact, unstable
key          hidden runtime address
generation   reuse guard for stale events
id field      ordinary app data if present
```

Ordinary TodoMVC should not need a user-visible `id`. The original plain
TodoMVC source in `~/repos/boon` uses row-local links/state without an `id`
field. A separate physical TodoMVC experiment added a `TodoId` only because it
stored a global `selected_todo_id` and compared rows against it.

The rule:

```text
Runtime LIST keys are hidden from Boon code.
Boon equality is data equality.
If data contains an id field, it is just data.
Runtime keys, references, slots, and generations are never compared in Boon.
```

## Item Scope

When evaluating a row template for key `k`, the runtime enters:

```text
/todos:k
```

All stateful expressions inside that row use the scope path:

```text
(expr_id, /todos:k)
```

Nested lists extend the path:

```text
/projects:p/todos:t/comments:c
```

## Nested Fields

Records inside list items are structural. Stateful fields become slots/columns.

Example:

```boon
projects: LIST {
    [
        name: "Project A" |> HOLD name { ... }
        todos: LIST {
            [
                title: "Task" |> HOLD title { ... }
                done: False |> HOLD done { ... }
            ]
        }
    ]
}
```

Runtime shape:

```text
project.name[project_key]
project.todos[project_key] -> list memory
todo.title[project_key, todo_key]
todo.done[project_key, todo_key]
```

If `todo.done[7, 42]` changes, only dependencies parameterized by project `7`
and todo `42` are dirty, plus any aggregate that reads the relevant key set.

## Append

`List/append` allocates a stable key and creates an item scope.

Tick behavior:

1. evaluate append event.
2. allocate key and generation.
3. evaluate the static row initializer subgraph for the new key.
4. insert key into order/index memory.
5. emit `ListInsert` and initial `Field` deltas.
6. bind row sources after commit; they cannot fire until the next tick.

No graph nodes are allocated.

## Remove

Removal clears the hidden valid bit, removes the key from the live order/view,
and unbinds sources.

Tick behavior:

1. set hidden `valid[key]` to false and remove key from live order.
2. emit `ListRemove`.
3. emit `SourceUnbind` for item sources.
4. keep storage until an acknowledgement/barrier permits reuse with a new
   generation.

Stale source events must include generation and be ignored if the generation no
longer matches. The runtime should count and report stale-event drops so tests
can prove stale UI/network events did not mutate reused storage.

## Move

Moving an item changes order/index memory only.

The item scope and field memories stay the same:

```text
/todos:42 remains /todos:42
```

This makes render movement deterministic and avoids rebuilding item state.

## Views

`List/retain`, filter, sort, and projection can produce views.

A view should track membership/order deltas:

```text
ViewInsert(key, position)
ViewRemove(key)
ViewMove(key, position)
ViewField(key, path, value)
```

The source list's semantic keys remain stable.

## Aggregates

Aggregates such as count and `all_completed` should be incremental when
possible.

Examples:

```text
active_count depends on live keys and todo.completed
all_completed depends on count(live) and count(completed among live keys)
```

The first interpreter may scan only for explicitly declared broad or bulk
operations. Single-key edits must update maintained aggregate state or dirty only
the changed key plus declared aggregate/view operators. The architecture must
represent enough dependency information to replace bulk scans with maintained
indexes later.

## Software Profiles

Software also needs explicit bounds for honest performance and failure behavior.
The default proof profile should declare:

```text
max rows per list
max nested list depth
max text bytes per field
max source bindings
event queue capacity
delta queue capacity
bulk work per tick
overflow/backpressure policy
```

The dynamic profile can grow storage, but the verification budget still sets
maximum accepted proof sizes. The bounded software profile should report
overflow deterministically instead of reallocating silently in hot paths.

## Hardware Profiles

For hardware or allocation-free codegen, the target profile provides bounds:

```text
todos capacity = 10000
title max bytes = 128
nesting max depth = profile-defined
```

The Boon source should not need to change except where the program genuinely
requires unbounded behavior that the target cannot synthesize.

On FPGA, the internal list key is usually the physical slot address plus a
generation:

```text
TodoKey = slot index
TodoGeneration = reuse counter

valid[slot]
generation[slot]
title[slot]
completed[slot]
editing[slot]
order[position] -> slot
```

This gives stable row retention and event routing without forcing an `id` field
into the Boon todo record.

From Boon's point of view, the slot and generation do not exist. They are only
for generated code, source routing, stale-event rejection, and deterministic
deltas.
