# Architecture Decisions

This file records the decisions for the Boon Circuit engine. It is intentionally
opinionated: the point is to prevent the new experiment from drifting back into
either the old actor runtime or a reducer-style app model.

## D1. Build A Static Circuit-Like Engine

Decision: Boon Circuit uses a static equation graph plus indexed state storage.

The semantic graph is fixed after compile/elaboration. Dynamic application data,
such as todos, users, rows, and spreadsheet cells, lives in memories keyed by
hidden stable runtime keys. Runtime work scales with changed keys and affected
dependencies, not with dynamically instantiated actors.

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
todo #42 has title, completed, editing
```

Physical view:

```text
title[42]
completed[42]
editing[42]
valid[42]  # hidden list membership, not Boon record data
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

The runtime still keeps retention/routing data:

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

Implementation note: the current verifier has an explicit duplicate-title test.
Two todos may have the same visible title, and a host action can still route to
the second physical row through the hidden render/source binding. The Boon
source still sees only `[title, completed, editing]`; the hidden key appears only
in semantic deltas, source bindings, stale-event guards, and debug/protocol data.
There is also a stale-generation test: an event addressed to a deleted row's old
`(key, generation)` is ignored before any Boon equation runs.

Implementation note: the runtime now has a reusable `KeyedList<T>` primitive for
hidden keys, generations, append/remove/move, and bound source lookup. TodoMVC
stores only todo field data in its row value; key/generation mechanics are owned
by the list memory layer. Row source bindings are derived from parsed `SOURCE`
ports in the Boon row template, not from a fixed Rust list of TodoMVC element
names. Cells uses the same hidden grid slots for protocol deltas; visible
spreadsheet addresses such as `A1` remain ordinary domain data and are not hashed
into runtime identity.

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

Implementation note: the runtime has an `EventPulse`/`then_value` primitive.
TodoMVC uses it for Enter-triggered input reset and filter button updates, and
tests cover present-event candidate creation and absent-event `SKIP`.

## D9. LATEST Is A Deterministic Merge

Decision: `LATEST` merges candidate updates by event presence and monotonic
source event sequence.

Rules:

- `SKIP` means no candidate value.
- choose the candidate with the greatest source event sequence.
- pure expressions selected by an event inherit that event sequence.
- if two candidates have the same greatest sequence, fail unless the source uses
  explicit `PRIORITY` or proven `EXCLUSIVE`.
- ambiguous same-tick writes are errors, not warnings.

Implementation note: the runtime has a `LatestCandidate<T>` primitive carrying a
monotonic `TickSeq`. Runtime tests cover greatest-sequence selection and
equal-sequence conflict rejection, and TodoMVC uses it for `new_todo_text` and
`selected_filter` `HOLD` updates.

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

Implementation note: the runtime has a `while_value` primitive and tests cover
conditional selection. The current TodoMVC and Cells examples do not use `WHILE`
directly.

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

Every keyed fact also carries generation/bind-epoch data as defined in the delta
protocol. The key is a protocol/runtime fact, not a Boon value.

Current reports expose the compiled source/state/list/dependency tables under
`ir_debug_tables`. That table is the beginning of the debugger contract: it
shows which source ports can write each `HOLD` cell without making runtime keys
or source ids into Boon values. The native Ply playground's `Causes` panel reads
that same `possible_causes` table so the visible debugger surface is backed by
the IR, not by separate UI text.

`ir_debug_tables.update_branches` is the more executable form of that same
contract. It records source-derived update branches for state cells, including
whether the branch is indexed and whether the expression is a source payload,
constant, previous value, `Bool/not`, or still an unknown expression summary.
`ir_debug_tables.list_operations` does the same for list append/remove/view/count
operators. `ir_debug_tables.formula_operations` records the Cells formula
pipeline as `Formula/parse`, `Formula/dependencies`, `Formula/eval`, and
`Formula/error` operators. `ir_debug_tables.state_cells` now includes
source-derived initial values for `HOLD` cells, `ir_debug_tables.lists` includes
record-literal and `Grid/cells` initializers plus optional `LIST[n]` capacity,
and `ir_debug_tables.derived_values` records non-state values such as
`store.title_to_add`, aggregate counts, list views, and formula projections.
These tables are not enough to claim the generic interpreter is complete, but
they are the handoff artifacts the runtime should consume while replacing the
current TodoMVC/Cells adapters.

Current implementation note: TodoMVC root scalar `HOLD` fields such as
`store.new_todo_text` and `store.selected_filter` now execute through those
source-derived update branches. TodoMVC `todo.title` uses IR-derived
trim-or-previous branches for Enter and blur commits. TodoMVC `todo.completed`
also uses IR-derived boolean `Bool/not` branches for row checkbox and toggle-all
sources, and `todo.editing` uses IR-derived constant branches for double-click,
Enter, Escape, and blur sources. TodoMVC `todo.edit_text` uses IR-derived source
payload and previous-title branches for opening an editor, editing the draft,
and cancelling with Escape. TodoMVC append, row delete, and clear-completed
remove operations are checked against IR-derived `List/append` and `List/remove`
operators, including renamed source ports and renamed local append triggers.
TodoMVC active/completed counts and selected-filter visibility are evaluated
through IR-derived `List/count` and `List/retain` predicates.
Cells edit-state `HOLD` fields such as
`cell.editing_text`, `cell.formula_text`, and `cell.editing` use the same branch
table for change/commit/cancel handling. The Cells formula evaluator checks the
IR-derived formula-operation pipeline before parsing, dependency extraction,
evaluation, and error projection. The evaluator itself is still a generic Rust
primitive inside the current runtime, so reports keep
`example_behavior_adapter: true` and the readiness audit still fails.

Implementation note: a generic initialization runtime now materializes root
state cells and keyed list rows from `TypedProgram` initializers. TodoMVC seed
titles, `store.new_todo_text`, and `store.selected_filter` are initialized from
that generic storage rather than by reparsing the source text in the runtime,
and TodoMVC row fields now write that storage first while the Todo mirror is
kept as a checked render/test cache. TodoMVC scenario assertions and state
summaries now read root values, row fields, row identities, counts, and filter
views from that generic storage instead of from the mirror. Cells now uses the
same generic keyed list storage for `cell.formula_text`, `cell.editing_text`,
and `cell.editing`; the formula/value/dependency vectors are derived caches.
Root text `HOLD` commits and indexed text/bool `HOLD` field commits now go
through generic runtime commit helpers before mirrors are updated. TodoMVC list
append/remove operations now also enter through generic runtime structural
helpers that check the IR-derived append trigger and remove predicates before
the render/test mirror is updated.
Scenario `expected_source_event` records are now normalized into a generic
source-event object before TodoMVC or Cells consumes source path, text, key,
address, or target row data. The per-step execution loop for timing, allocation
measurement, delta expectation checks, dirty-key counting, and report row
generation is also shared; the remaining example-specific boundary is the
equation-application method behind that loop. Indexed text/bool branch
evaluation for row-scoped `HOLD` fields now goes through generic runtime helpers
for `SourceText`, `PreviousValue`, `TextTrimOrPrevious`, constants, and
`Bool/not`. Hidden row source bindings, source ids, bind epochs, and stale
binding checks now live in `GenericCircuitRuntime`; TodoMVC only reads them back
to emit current protocol/render deltas. Generic runtime helpers now construct
the keyed semantic facts for field, list, and source changes; example-specific
code still lowers those facts into current TodoMVC/Cells render patches. TodoMVC
root `SOURCE` events are dispatched through the generic branch table to the
single root `HOLD` target they are allowed to drive, so the adapter no longer
names `store.new_todo_text` or `store.selected_filter` at the event dispatch
site. The TodoMVC append title is now evaluated through a generic derived-text
transform selected by the IR `List/append` trigger (`store.title_to_add`, or a
renamed equivalent) instead of by trimming the Enter payload directly in the Todo
adapter. TodoMVC source-producing scenario steps are now routed from the
canonical `SOURCE` event facts and compiled branch/list-operation tables, not
from UI labels such as "Active filter" or "Buy groceries checkbox"; only
hover-only render affordances still use the UI action target because they do not
produce a Boon source event. Cells uses the same boundary: source events carry
the visible address plus optional text, and the compiled branch table
distinguishes edit, commit, and cancel sources without reparsing UI target
labels such as "A1 editor". Those source routes are now precomputed in the
compiled runtime plan from scalar branches, derived text transforms, and list
remove operations, so normal ticks no longer scan those tables just to classify
a source. Source-route scalar targets also carry their compiled branch
expression, so TodoMVC route classification can ask for non-root `Bool/not`,
text-trim, constant, or previous-value branches instead of recognizing toggle or
edit events by field names. Root scalar `HOLD` dispatch also uses that same
compiled route index to find the single root target for a source, and routed
TodoMVC source events carry that root target into the application phase instead
of looking it up again there. `List/remove` predicates for clear-completed and
row delete are carried on the same source-route entries, so row removal uses the
compiled predicate directly instead of looking it up by source during the row
scan. Cells edit, commit, and cancel events now carry the indexed `HOLD` targets
selected by their compiled source route (`cell.editing_text`,
`cell.formula_text`, `cell.editing`, or renamed equivalents) into the
application phase, so the application boundary does not choose those fields by
hardcoded event kind. Example runtimes only mirror committed values for
render/test checks.
TodoMVC
`List/count`, `List/retain`, completed-title projections, editing-row lookups,
and whole-title projections now execute through generic list scan helpers over
IR-derived predicates instead of Todo-specific loops. Those runtime predicates
now carry the IR selector and row-field paths as data (`FieldBool`,
`FieldBoolNot`, `SelectorVisibility`) instead of using Rust enum variants like
"row completed" or "row active"; row paths are resolved to the current list row
field at evaluation time. Both example runtimes assert that their mirrors stay
synchronized with the generic storage after scenario steps.
Executable reports also include `compiled_schedule`, a typed-IR-derived schedule
summary that rejects unknown initializers, unsupported update branches,
unsupported list predicates, and per-row graph clones before the example runtime
starts. The compiled plan also infers the executable surface profile from IR
shape (`todos` plus TodoMVC state cells, or `cells` plus Cells state/formula
tables) rather than trusting the parser's example marker during runtime
dispatch and report generation. `run_loaded_scenario` now enters one shared
`run_generic_scenario` loop through a `LoadedRuntime` shell, but that shell still
selects the TodoMVC or Cells driver by inferred surface profile. The tick
executor remains adapter-backed until all source events execute through one
generic schedule without the TodoMVC/Cells driver layer.

Implementation note: the current IR cause table is source-derived, not a
TodoMVC/Cells-specific Rust lookup table. It derives row scopes from
`List/map(... new: function(...))`, then scans field equation bodies and local
derived-field references such as `new_todo_text -> title_to_add` to build
`possible_causes`. Runtime execution still has example adapters and remains a
known blocker before the full "no hardcoded app behavior" criterion is met.
Executable reports include `runtime_execution` metadata so this blocker is
visible in verification artifacts.

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
