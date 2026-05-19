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
todos.alive        BitSet/TodoKey -> Bool
todos.title        TodoKey -> Text
todos.completed    TodoKey -> Bool
todos.editing      TodoKey -> Bool
todos.generation   TodoKey -> Generation
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
3. initialize stateful fields in the item scope.
4. insert key into order/index memory.
5. emit `ListInsert` and initial `Field` deltas.

No graph nodes are allocated.

## Remove

Removal marks the item dead and unbinds sources.

Tick behavior:

1. set alive/valid bit to false or remove key from order.
2. emit `ListRemove`.
3. emit `SourceUnbind` for item sources.
4. keep storage until safe GC or reuse with a new generation.

Stale source events must include generation and be ignored if the generation no
longer matches.

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
active_count depends on todo.alive and todo.completed
all_completed depends on count(alive) and count(completed && alive)
```

The first interpreter may scan on broad events. The architecture should still
represent enough dependency information to replace scans with maintained
indexes later.

## Hardware Profiles

For hardware or allocation-free codegen, the target profile provides bounds:

```text
todos capacity = 10000
title max bytes = 128
nesting max depth = profile-defined
```

The Boon source should not need to change except where the program genuinely
requires unbounded behavior that the target cannot synthesize.
