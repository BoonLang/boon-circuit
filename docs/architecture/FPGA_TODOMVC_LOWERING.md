# FPGA TodoMVC Lowering

This document describes how the ordinary circuit-style TodoMVC should lower to
FPGA hardware without requiring a central reducer or a user-visible todo `id`.

## Why This Matters

The original plain TodoMVC in `~/repos/boon` does not store `id` in each todo
record. Each todo owns local values and local UI event links:

```text
title
editing
completed
todo_elements.remove_todo_button
todo_elements.editing_todo_title_element
todo_elements.todo_title_element
todo_elements.todo_checkbox
```

That is the property to keep. If FPGA support forced every app list to expose an
`id`, then hardware identity would leak into app data. The language would no
longer feel like the same Boon source across software and hardware.

So the rule is:

```text
User code sees todo fields.
Boon equality compares data.
The runtime/compiler hides list keys, slots, generations, and source bindings.
```

## Source-Level Shape

The ordinary TodoMVC source should stay close to this:

```boon
todos:
    LIST {
        [title: TodoTitle { Buy groceries }]
        [title: TodoTitle { Clean room }]
    }
    |> List/append(title_to_add |> THEN {
        [title: title_to_add]
    })
    |> List/map(new_todo)

FUNCTION new_todo(seed) {
    sources: [
        remove_todo_button: [press: SOURCE]
        editing_todo_title_element: [change: SOURCE, key_down: SOURCE, blur: SOURCE]
        todo_title_element: [double_click: SOURCE]
        todo_checkbox: [click: SOURCE]
    ]

    [
        alive: True |> HOLD alive { ... }
        title: seed.title |> HOLD title { ... }
        completed: False |> HOLD completed { ... }
        editing: False |> HOLD editing { ... }
    ]
}
```

There is no `[id: ...]` field. `List/map(new_todo)` creates a row scope for each
item. The compiler maps that scope to hardware storage.

## Hardware Profile

FPGA compilation needs bounds. They should come from a profile or from fixed-list
syntax, not from a different app architecture.

Example profile:

```boon
PROFILE fpga_todomvc {
    clock: PASSED.clk
    reset: PASSED.reset

    todos.capacity: 256
    todo_title: TEXT[64, ascii]

    input_event_fifo.capacity: 16
    output_delta_fifo.capacity: 64

    list_bulk_ops: sequential
}
```

Equivalent fixed syntax may be accepted:

```boon
todos: LIST[256] { ... }
```

The preferred app source can still use plain `LIST`; target profiles decide
whether an unbounded software list is allowed.

## Internal Storage

The compiler lowers `todos` to bounded memories:

```text
valid[256]             Bool
generation[256]        UInt[generation_bits]
title[256][64]         fixed text storage
completed[256]         Bool
editing[256]           Bool
order[256]             slot index
free_list[256]         slot index
count                  UInt[9]
```

`slot` is the internal runtime address. `generation` prevents stale events from
mutating a reused slot. Neither is visible to Boon code.

The row scope:

```text
/todos:<slot>:<generation>
```

is the generated-code address used for state, source binding, and deltas.

## Input Events

A software renderer or host receives source bindings from the runtime:

```text
Bind /todos:42:7/todo_checkbox.click -> source_id 9001
```

When the user clicks the checkbox, the host sends a compact hardware event:

```text
event_valid = 1
event_kind  = TodoCheckboxClick
slot        = 42
generation  = 7
payload     = none
```

The FPGA decoder checks:

```text
valid[slot] && generation[slot] == event.generation
```

Then it produces a one-cycle source pulse for the row-local source:

```text
/todos:42/todo_checkbox.click
```

The Boon source still says `sources.todo_checkbox.click`. It does not need to
read or store `slot`.

## Per-Field Next-State Logic

Each row field becomes a register-file next-state equation.

Source:

```boon
completed:
    False |> HOLD completed {
        LATEST {
            sources.todo_checkbox.click |> THEN {
                completed |> Bool/not()
            }

            store.sources.toggle_all_checkbox.click |> THEN {
                store.all_completed |> Bool/not()
            }
        }
    }
```

Hardware shape:

```text
if reset:
    completed[slot] := false
else if checkbox_click(slot):
    completed[slot] := !completed[slot]
else if toggle_all_active_for_slot:
    completed[slot] := !all_completed
```

The source remains local. The generated HDL may use imperative assignments, but
those assignments are just the implementation of the declared equation.

## Append

`List/append` lowers to allocation logic:

```text
on title_to_add:
    slot = free_list.pop()
    generation[slot] += 1
    valid[slot] = true
    title[slot] = title_to_add
    completed[slot] = false
    editing[slot] = false
    order[count] = slot
    count += 1
    emit ListInsert(slot, generation, initial fields, source bindings)
```

If the list is full, the target profile decides the behavior:

```text
drop event
set overflow error
backpressure input event FIFO
```

The first hardware profile should prefer an explicit overflow error.

## Remove

Row-local remove:

```text
remove_todo_button.press for slot 42
```

lowers to:

```text
valid[42] = false
remove slot 42 from order memory
push slot 42 to free list
emit ListRemove(42, generation[42])
emit SourceUnbind for row sources
```

Storage can be reused later only after generation changes.

## Bulk Operations

`ToggleAll` and `ClearCompleted` are not free in FPGA. They need a latency
policy.

Sequential policy:

```text
on clear_completed:
    scan_index = 0
    busy = true

while busy:
    slot = order[scan_index]
    if valid[slot] && completed[slot]:
        remove slot
        emit ListRemove
    scan_index += 1
    if scan_index == count:
        busy = false
```

Parallel policy:

```text
generate N lanes and update many/all slots in one cycle
```

Sequential is cheaper and should be the first target. The source code does not
change; the profile chooses the lowering.

## Derived Values

Counts can be maintained incrementally:

```text
todos_count
completed_todos_count
active_todos_count
all_completed
```

For small first hardware profiles, scanning on broad operations is acceptable.
The IR should still represent dependencies so maintained counters can replace
scans later.

## Output Deltas

The FPGA should not output whole TodoMVC state after every event. It emits
semantic/render deltas:

```text
ListInsert(slot, generation, fields)
ListRemove(slot, generation)
FieldSet(slot, completed, true)
FieldSet(slot, title, "Buy milk")
SourceBind(slot, generation, source_path, source_id)
SourceUnbind(source_id)
```

A browser, server, or Ply renderer consumes these deltas and updates only the
affected elements.

## Direct Display Option

If the FPGA drives a display directly, the renderer is another bounded hardware
module:

```text
order memory -> visible row scan -> text renderer/framebuffer/HDMI
pointer/key input -> row hit test -> slot/generation event
```

Even in that mode, the Boon TodoMVC program should not gain a user-visible
`id`. The display pipeline resolves physical row position to internal slot
before emitting a source event.

## Old Selected Id Workaround

The physical TodoMVC experiment in `~/repos/boon` added `TodoId` because it
stored a global `selected_todo_id`.

That should not be copied into the new design. It exposes identity-like data only
to recover row-local behavior. The circuit TodoMVC should keep editing as
row-local state driven by row-local sources.

If external input data already contains an `id` field, Boon treats it as ordinary
data. Equality compares its value like any other field. It is not a reference to
a row, a list slot, or a runtime object.

TodoMVC should not need that in its ordinary form; row-local `editing` is enough.
