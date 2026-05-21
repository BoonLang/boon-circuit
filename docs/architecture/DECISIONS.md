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
the second physical row through the hidden render/source binding. The generic
runtime validates the hidden binding, confirms that the visible title still
matches the bound row, and derives the visible occurrence used by the surface
driver. The Boon source still sees only `[title, completed, editing]`; the
hidden key appears only in semantic deltas, source bindings, stale-event guards,
and debug/protocol data. There is also a stale-generation test: an event
addressed to a deleted row's old `(key, generation)` is ignored before any Boon
equation runs.

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

Implementation note: the IR has an explicit `While` node kind. Lowering rejects
combinational field cycles through pure expressions or `WHILE` unless a `HOLD`
state cell breaks the dependency path. The runtime still has a `while_value`
primitive and tests cover conditional selection. The current TodoMVC and Cells
examples do not use `WHILE` directly.

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
and the Todo runtime no longer keeps separate root mirrors for those `HOLD`
values. TodoMVC row fields, hidden row identities, and source-bind epochs now
live only in generic keyed-list storage; render patches, semantic deltas,
scenario assertions, and state summaries read those facts back from the generic
runtime instead of from a Todo row cache. TodoMVC field deltas and render
targets now also read row key/generation identity from generic storage for
normal row updates. Cells now uses the same generic keyed list storage for
`cell.formula_text`, `cell.editing_text`, and `cell.editing`; those committed
Cells fields are no longer mirrored in the `Cell` cache. Cells state summaries
and scenario assertions read those fields and hidden key/generation facts from
generic storage, while formula value/error/dependency vectors remain derived
caches.
Root text `HOLD` commits and indexed text/bool `HOLD` field commits now go
through generic runtime commit helpers. TodoMVC list append/remove operations
now also enter through generic runtime structural helpers that check the
IR-derived append trigger and remove predicates before emitting current
protocol/render deltas. Appending a keyed row and binding that row's source
ports now happens in one generic list/source-store helper, so row-local source
identity is attached at the same runtime boundary as the structural insert
instead of in the TodoMVC adapter.
Generic source-action commits now carry their list/key/generation/field identity
and can emit keyed semantic deltas directly for root text, indexed text,
indexed bool, list append, list remove, list move, and source unbind mutations.
Formula-derived value/error commits use the same generic keyed field commit path
for their semantic deltas. Source bind facts emitted after list append now also
flow through a generic `SourceBind` mutation and use the appended list's
identity instead of a TodoMVC-specific `todos` helper.
Cells formula-derived row fields (`value`, `error`, `dependencies`) are seeded
into each generic row when the static grid is materialized instead of being
inserted lazily during the first recompute. The loaded Cells formula state keeps
a reusable dependency-text buffer per cell and writes dependency addresses into
that buffer without allocating, so formula recompute can satisfy the release
speed gate's zero post-warmup allocation budget. The remaining final handoff
pieces are fresh human reports and aggregate all reports, not example behavior
adapters in the semantic interpreter.
Source-event routing is also moving into the generic layer: the compiled
`SourceRoutePlan` can now turn a normalized source event into a
`GenericSourceActionInput` by deriving the root/list/indexed action shape from
the static route table. Surface code still supplies the observed human target
label for visible Todo titles or Cells addresses, while duplicate-title
occurrence recovery and hidden source binding validation are handled by the
generic runtime. Surface code no longer decides whether the source is a root
scalar, list append/remove, indexed text, or indexed bool action in runtime
execution. The loaded runtime shell now owns TodoMVC and Cells scenario
execution, including visible row/cell target resolution, render context passed
into the generic lowering table, assertions, summaries, and speed stress
orchestration.
Report-facing summaries have also started moving behind generic storage
projection helpers. TodoMVC summary rows are now projected from generic keyed
list storage, and Cells summary fields such as address, formula text, editing
text, editing state, formula value, formula error, and dependency projection use
generic row fields. The Cells formula evaluator still uses a runtime cache for
parsing, dependency fanout, cycle detection, and recompute metrics, but scenario
assertions and report summaries no longer read value/error directly from that
cache. `LoadedRuntime::state_summary` now dispatches directly to
`GenericScheduledRuntime` projection helpers, so executable reports no longer
reconstruct the TodoMVC or Cells surface runtimes just to summarize final state.
TodoMVC scenario preparation also now runs directly on `GenericScheduledRuntime`
from the loaded runtime shell: the scenario scan reserves root text, append row
storage, source bindings, spare rows, and row text buffers without constructing
the TodoMVC surface driver. Cells preparation also now runs from the loaded
runtime shell: formula text interning, formula-cache sizing, and generic `cells`
row text storage reservation are derived from the same scenario text/dependency
requirements without reconstructing the Cells surface driver.
`LoadedRuntime::apply_step` now also executes TodoMVC root-level SOURCE steps
directly through `GenericScheduledRuntime`: new-todo input changes/appends,
filter changes, clear-completed, and toggle-all no longer reconstruct the
TodoMVC surface driver. Row-addressed TodoMVC checkbox toggles, delete button
presses, and edit/open/change/commit events also execute directly through the
loaded generic runtime after the generic hidden source binding check resolves
the visible title occurrence to a current keyed row. Render-only TodoMVC hover
patches, post-step assertions, state summaries, and speed stress profiles now
also run without borrowing the TodoMVC surface driver. Cells change, commit,
cancel, formula dependency/evaluation, assertions, summaries, and speed stress
profiles likewise run from `LoadedRuntime` over `GenericScheduledRuntime` plus
loaded formula cache state, without reconstructing the Cells surface driver.
Scenario assertions are following the same path: TodoMVC title/filter/count/edit
checks now enter through `GenericScheduledRuntime`, which evaluates the expected
facts through generic root/list assertion helpers. Cells formula/editing,
value/error, and recomputation expectations also enter through
`GenericScheduledRuntime`; recomputed cell indexes are projected back to visible
addresses from generic keyed `cells` storage before comparison. `LoadedRuntime`
now decides when to invoke both TodoMVC and Cells assertions after each scenario
tick, so expectation reads and assertion orchestration are no longer owned by
the TodoMVC or Cells adapters.
The row source paths themselves are compiled from typed IR source ports and the
list's row scope into a generic `ListSourceBindingPlan`; runtime surface
validation now checks TodoMVC and Cells row-source requirements against that
compiled plan instead of re-scanning parsed source text.
Removing a keyed row and unbinding its source ports now follows the same
boundary: generic storage checks the IR-derived predicate, removes the row,
exposes each bound source for protocol/render lowering, and then unbinds those
sources before returning the removed row for storage reuse.
Scenario `expected_source_event` records are now normalized into a generic
source-event object before TodoMVC or Cells consumes source path, text, key,
address, or target row data. The per-step execution loop for timing, allocation
measurement, delta expectation checks, dirty-key counting, and report row
generation is also shared; the remaining example-specific boundary is the
equation-application method behind that loop. Indexed text/bool branch
evaluation for row-scoped `HOLD` fields now goes through generic runtime helpers
for `SourceText`, `PreviousValue`, `TextTrimOrPrevious`, constants, and
`Bool/not`. Generic runtime helpers now also commit indexed text/bool scalar
branches into keyed storage and return the committed key, generation, field, and
value as one fact; TodoMVC and Cells no longer split branch evaluation from the
state write for normal indexed scalar updates. The toggle-all path now uses a
generic bulk indexed-bool commit helper that applies one compiled row equation
across the static list and streams the resulting keyed commits back to the
TodoMVC renderer adapter. TodoMVC draft editing now lowers
the `changed |> Text/trim |> WHEN { TEXT {} => draft; trimmed => trimmed }`
shape into the same `TextTrimOrPrevious` IR as title commit/blur, so draft text
updates no longer need a Todo-only trim/write path. Hidden row source bindings,
source ids, bind epochs, and stale binding checks now live in
`GenericCircuitRuntime`; TodoMVC only reads them back to emit current
protocol/render deltas. Generic runtime helpers now construct the keyed semantic
facts for field, list, and source changes; example-specific code still lowers
those facts into current TodoMVC/Cells render patches. TodoMVC
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
edit events by field names. The compiled route table now stores separate
indexed-text and indexed-bool target indexes, so row-scoped route classification
does not rescan all scalar targets to find the relevant equation family. Cells
editor routes use the same indexed target partitions for `editing_text`,
`formula_text`, and `editing` commits.
Toggle-all and row-checkbox events now carry the
source and row selection only; a route-selected indexed bool commit chooses the
matched `Bool/not` target at application time instead of applying through a
fixed `todo.completed` target. Todo edit-open/change/Enter/Escape/blur events now
carry only source payload and row selection data. Route-selected indexed text
and bool commits choose the matched title, edit-text, and editing targets from
the compiled source route instead of applying through fixed `todo.title`,
`todo.edit_text`, or `todo.editing` paths.
Root scalar `HOLD` dispatch also uses that same compiled route index to find the
single root target for a source. TodoMVC root events now carry only source
payload data; `GenericCircuitRuntime` applies the route-selected root `HOLD`
branch and returns the committed root target/value fact to the renderer adapter.
The runtime object used by TodoMVC and Cells is now a scheduled generic runtime:
it owns generic storage plus the compiled scalar, derived, list, formula,
source-route, and list-source-binding tables together. Adapters no longer carry
parallel copies of those plan tables or pass them back into generic storage on
every commit; they ask the scheduled runtime to apply a source action and then
translate the resulting generic facts into the current semantic/render protocol.
Route classification in the adapters also goes through that scheduled runtime
boundary, so TodoMVC and Cells no longer reach into the route plan directly to
ask whether a source is append/remove/root/indexed-text/indexed-bool capable.
Route target selection is exposed through `SourceRoutePlan` helpers rather than
through example-specific direct reads of route internals. Generic runtime
helpers now accept source route actions for indexed text and bool commits, so
TodoMVC and Cells pass source plus row/address/payload and the helper selects
the compiled `HOLD` target before evaluating the branch. The remaining adapters
still interpret committed facts into example-specific render patches, and list
removal still asks the route plan for its compiled predicate. The source route
plan now also materializes route capability actions (`RootScalar`,
`DerivedText`, `ListAppend`, `ListRemove`, `IndexedText`, `IndexedBool`) from
the compiled target tables, so adapters can classify source events by
precomputed route capabilities instead of repeatedly inspecting scalar
expression lists.
`List/remove` predicates for clear-completed and row delete are carried on the
same source-route entries. TodoMVC remove events now carry only source and row
selection data; generic source-routed list removal selects the compiled
predicate inside `GenericCircuitRuntime`, unbinds row sources, recycles the row
through the generic spare-row pool, and reports the removed key/generation back
to the renderer adapter. Cells edit, commit, and cancel events now carry only
source payload and grid address data.
Route-selected indexed text/bool commits choose the compiled `HOLD` targets
(`cell.editing_text`, `cell.formula_text`, `cell.editing`, or renamed
equivalents) at the application boundary instead of carrying those targets in
the event enum. Cells commit now groups the formula text, editing text, and
editing bool writes through one generic source-routed helper before the generic
formula dependency cache updates reverse edges for the changed cell. Cells
cancel now uses the same
generic indexed text action path: the `PreviousValue` expression copies the
compiled previous field into the target field inside `GenericCircuitRuntime`,
instead of hardcoding `formula_text -> editing_text` in the Cells adapter.
Example runtimes still adapt generic facts into their current protocol/render
outputs; TodoMVC no longer mirrors committed row or root values for render/test
checks, and Cells no longer mirrors committed formula/editing fields.
`List/append` row construction now lowers the `THEN { [field: source] }` record
into typed append seed fields. `GenericCircuitRuntime` stores a row template per
list from indexed `HOLD` initializers and materializes appended rows from that
template rather than from a TodoMVC-specific row constructor. The runtime keeps
a generic per-list spare-row pool and resets spare rows from the typed append
seed fields, preserving the zero-allocation speed budget after warmup without a
Todo-specific pool. Append routing is also part of the source-route action
table: the TodoMVC key-down path asks the compiled route for its `ListAppend`
trigger instead of comparing derived text target names outside the route plan.
The append text transform and row insertion now run as one generic source-routed
list append helper, returning only the inserted key/generation and trigger text
for the renderer adapter.
Source-route action execution is now a generic runtime boundary, not only a set
of classification helpers. The scheduled runtime can walk the precomputed
`RootScalar`, `DerivedText`, `ListAppend`, `ListRemove`, `IndexedText`, and
`IndexedBool` actions for a source, apply them through generic storage, and
stream typed mutations back to the current surface drivers. TodoMVC root,
append, toggle, edit-open, edit-change, edit-commit, edit-cancel, blur, row
delete, and clear-completed source effects now use that generic action executor;
Cells change, commit, and cancel paths use the same executor for indexed
text/bool source actions. The remaining adapter boundary is no longer branch
selection for those source effects; it is the surface driver that routes
scenario/user actions to source contexts and lowers generic mutations into the
current render protocol. The formula dependency and evaluation caches have moved
behind generic formula cache types, and derived `value`/`error`/`dependencies`
storage synchronization now uses the compiled formula-field table through a
generic helper. Cells source events now route as `GenericRoutedSourceEvent`
values carrying the normalized source payload plus the compiled route kind, so
the Cells driver no longer has a separate app-specific `CellEvent` enum for
change/commit/cancel classification. TodoMVC root-level source events now use
the same routed source event for append, input text, filter, clear-completed,
and toggle-all dispatch instead of Todo-specific event variants. TodoMVC
row-level source events now also carry the generic routed source event plus
visible row occurrence evidence; checkbox, edit-open, edit-change, edit-key,
edit-blur, and remove dispatch by compiled route kind instead of separate
Todo-specific source event variants. Formula display mutation construction and
the value/error render patch decision also use the compiled formula-field table.
Generic scheduled runtime can now apply a source action and return an
allocation-free mutation batch keyed by compiled field names; Cells edit,
cancel, and commit use that batch instead of locally pattern-matching every
returned text/bool mutation. TodoMVC edit-open, edit-key, and blur paths use
the same batch helper for their returned title/editing mutations, and TodoMVC
append now reads the inserted row identity plus root input reset from the same
batch instead of local `ListAppend`/`RootText` scans. Residual step
orchestration still remains in the Cells and TodoMVC drivers until the complete
equation/tick executor replaces the
TodoMVC/Cells driver layer.
TodoMVC mutation-to-protocol lowering has moved behind a shared render lowering
plan for root text commits, indexed text/bool field commits, appends, source
bind/unbind, row removes, edit-open/close, show-delete, and list move patches.
Cells uses the same pattern for source text/bool commits, identity text copies,
and formula-derived value/error field commits; the helper chooses the current
cell editor/display patch while the source effect itself remains a generic
mutation. Cells derived `value`, `error`, and `dependencies` storage fields are
read from the compiled `Formula/*` operation targets instead of fixed Rust
literals. Formula parsing and dependency extraction now enter through the
compiled `FormulaEquationPlan` before updating the generic formula dependency
cache, so dependency edges are derived from the parsed formula primitive rather
than a second ad hoc text scan. Formula evaluation now runs through a generic
evaluation cache that owns cycle-detection state, per-tick result cache, and
eval-call metrics. Derived formula storage writes, display semantic mutations,
and value/error render patch policy are selected by the `Formula/*` operation
table rather than fixed Cells field names. Reports now mark the semantic
interpreter as adapter-free because visible scenario event mapping and residual
step orchestration have moved into `LoadedRuntime`.
Source route shape classification and hidden source target validation have now
moved into the generic runtime. `GenericScheduledRuntime` classifies a source
event from the compiled `SourceRoutePlan` as root text, root scalar, list
append/remove, indexed text, or indexed bool work. When a scenario or headed
observation carries a target key, generation, source id, or bind epoch, the same
runtime verifies the row-source binding and resolves the current list index
before the TodoMVC surface applies a row-local source action. Cells now uses the
same routed source event for row-addressed edit, commit, and cancel source
events; commit is detected from the compiled `formula_text` target instead of a
concrete adapter lookup. The action input for Cells resolves row context
generically by looking up the `address` field in the keyed `cells` list, so the
runtime no longer converts `A1`/`B1` to list indexes in a Cells surface driver
before applying source actions.
TodoMVC row source paths now use a generic list-index action-input helper that
validates the compiled source route targets the `todos` list before applying
row-local actions. The same generic runtime now resolves bound row source
events into visible row occurrences by checking key/generation/source-id/bind
epoch, verifying that the observed title still matches the bound row, and
counting duplicate visible titles from generic list storage. The surface still
supplies the visible target label because that is UI observation evidence, but
stale key/generation/bind-epoch rejection and duplicate-title occurrence
resolution are no longer owned by the TodoMVC adapter. Checkbox toggles, row
delete presses, edit open, edit text changes, key commits/cancels, blur
commits, and render-only delete-button hover patches now stay in
`LoadedRuntime` after this resolution and call generic execution/lowering
helpers directly. Cells edit/change/commit/cancel routes also stay in
`LoadedRuntime`; formula parsing, dependencies, recompute, and display mutation
emission use the compiled `Formula/*` operation table and loaded formula cache
state.
Common semantic mutation to render patch lowering now goes through a
`GenericRenderLoweringPlan`. TodoMVC root text, title/edit text, checkbox,
hide-edit, show-edit, remove, insert, bind, and unbind patches, plus Cells
editor/value text patches, are selected by this runtime-owned lowering table
from generic mutations and explicit render context. TodoMVC append and the row
source binds that follow it now emit through the same generic mutation-to-patch
path as ordinary row edits/removes. Edit-open is also lowered generically by
passing the committed `edit_text` as render context to the `editing = True`
mutation. TodoMVC render-only row affordances such as show-delete and list move
position patches now also go through the same lowering plan instead of direct
surface-driver patch construction. Cells formula display synchronization now
uses the formula operation table for semantic/display patch emission; the
loaded runtime supplies the current visible render context directly to the
generic lowering table.
TodoMVC
`List/count`, `List/retain`, completed-title projections, editing-row lookups,
and whole-title projections now execute through generic list scan helpers over
IR-derived predicates instead of Todo-specific loops. Those runtime predicates
now carry the IR selector and row-field paths as data (`FieldBool`,
`FieldBoolNot`, `SelectorVisibility`) instead of using Rust enum variants like
"row completed" or "row active"; row paths are resolved to the current list row
field at evaluation time. Example runtimes assert that required generic fields
and hidden row identities are present after scenario steps; Cells still keeps
formula dependency/evaluation caches beside generic keyed storage because those
caches are runtime acceleration state, not Boon-visible values.
Executable reports also include `compiled_schedule`, a typed-IR-derived schedule
summary that rejects unknown initializers, unsupported update branches,
unsupported list predicates, and per-row graph clones before the example runtime
starts. The compiled plan also infers the executable surface profile from IR
shape (`todos` plus TodoMVC state cells, or `cells` plus Cells state/formula
tables) rather than trusting the parser's example marker during runtime
dispatch and report generation. `run_loaded_scenario` now enters one shared
`run_generic_scenario` loop through a `LoadedRuntime` shell. That shell owns the
scheduled generic storage between ticks and no longer lends that storage to
TodoMVC or Cells surface drivers for scenario execution. The remaining
acceptance blockers are fresh human reports, aggregate all reports, and any
future verification gaps found by the audit, not a semantic example adapter.

Implementation note: the current IR cause table is still source-derived in a
prototype sense. It derives row scopes from `List/map(... new: function(...))`,
then scans field equation bodies and local derived-field references such as
`new_todo_text -> title_to_add` to build `possible_causes`. That is useful
evidence for the shipped TodoMVC and Cells scenarios, but it is not the final
typed AST-to-IR lowering promised by the runtime model. The structural gate for
that distinction is `cargo xtask verify-runtime-finality`, and
`audit-goal-readiness` must fail while this gate reports blockers.

Executable reports may identify the runtime as `static_graph_interpreter` and
may include `generic_interpreter_complete` / `example_behavior_adapter` fields.
Those fields are not accepted as finality proof unless they are derived from
static/runtime coverage instead of hardcoded booleans. Any remaining
TodoMVC/Cells shell or report/assertion glue must stay visible in reports until
removed.

Headed Ply evidence also has two categories. Focusless headed reports are
synthetic/focusless evidence. Full OS pointer/keyboard claims require canonical
`target/reports/todomvc-headed-ply.json` and
`target/reports/cells-headed-ply.json` reports with
`input_injection_method = "os_pointer_keyboard_to_visible_window"`, no
`os_input_limitation`, current hashes, and real OS pointer/keyboard backend
coverage for every user-action step.

Headed Ply verification runs the native playground in release mode through
`xtask`, because the debug build can take minutes to replay TodoMVC's visible
control probes and can hit the wrapper timeout before writing the final report.
It records three intermediate OS-input slices. First, it focuses one real
visible application text control in the preview
(`todo_new_input` for TodoMVC or `cell_editor_A1` for Cells), sends real OS
keyboard text through `wtype`, observes the text through Ply state, captures the
control screenshot, and stores the control bounds and artifact hash. Second,
visible controls emit observed Boon `SOURCE` events and the headed report
records the observed payloads, bounds, screenshots, and runtime mutation result.
When `BOON_ALLOW_OS_POINTER_PROBE=1` is set, click/press-style source probes use
the real pointer backend against their visible target bounds instead of keyboard
activation; the report records `input_backend = "os_pointer"` and the selected
pointer backend for each such probe. Text and submit probes use the same pointer
backend to focus the visible text input, then send real keyboard events through
`wtype`; edit fields are cleared by frame-spaced Backspace key presses rather
than direct text replacement. Blur is driven by a real pointer click on a visible
non-text target. Render-only TodoMVC hover is driven by a real pointer move
without clicking the delete button. The current opt-in headed reports prove all
TodoMVC and Cells scenario user actions through OS pointer/keyboard input:
TodoMVC has 22 `os_input_steps`, including 11 pointer-click source probes, 8
pointer-focus-plus-keyboard source probes, 1 pointer blur, and 1 pointer hover;
Cells has 15 `os_input_steps`, including 11 pointer-focus-plus-keyboard source
probes and assertion-only checkpoints for non-input steps.
Covered prefix events are applied through `boon_runtime::LiveRuntime` against
the real scenario step, so expected source fields, semantic deltas, render
patches, and state assertions must pass. The headed command now fails if a
scenario-tagged visible SOURCE probe does not pass the runtime expectation
checks; it cannot false-green on "SOURCE observed" alone. Third, it focuses the
visible Step control, sends real OS keyboard activation, advances each scenario
prefix through the playground, captures per-step screenshots, and stores the
Step control bounds in the headed report. When every scenario user action is
covered, the headed report sets
`input_injection_method = "os_pointer_keyboard_to_visible_window"` and writes a
schema-checked `os_input_steps` entry for every scenario step.

Visible manual launches should be routed through COSMIC's background launcher on
this machine:

```sh
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example todomvc
```

That keeps the native playground on the `boon-circuit` workspace while the user
continues other work. It does not make real input focus-free: human-like
keyboard and mouse testing must eventually target the visible playground window,
because the compositor routes those events to the active surface. Verifiers that
send real desktop input must stay explicit: keyboard probes use `wtype`, and the
pointer probe is opt-in with
`BOON_ALLOW_OS_POINTER_PROBE=1` because it moves and clicks the real pointer.
Reports may include `os_pointer_probe.status = "skip"` for normal headed runs;
that is evidence that the run avoided pointer injection, not a pass for the
final full-OS-input gate. When the pointer probe is attempted, a failed hit is a
schema failure rather than an accepted warning.

The current reliable pointer backend on this COSMIC/Wayland machine is
X11/XWayland XTest. The Ply process loads X11/XWayland libraries, and the
headed probe now tries XTest absolute screen coordinates first and records
`os_pointer_probe.click_target.backend = "x11_xtest"` when Ply observes the
button click. The older `ydotool` path remains as a fallback but exited
successfully without producing a Ply-observed hit with either relative-delta or
absolute-screen-coordinate attempts; `ydotoold` is not available as a user
service here.

`cosmic-background-launch --workspace boon-circuit -- cargo xtask
verify-todomvc-headed-ply` is not a valid full headed-verifier mode today. A
direct lower-level attempt with `cosmic-background-launch --workspace
boon-circuit -- cargo run --release -p boon_ply_playground -- --verify-headed
--example todomvc` also left the verifier process alive for 120 seconds without
creating its report. That is a failed verification, not a pass. Background
launch is appropriate for manual playground surfaces and bounded smoke launches;
automated headed verification still needs a directly controlled process and a
real focused input route.

The bounded background smoke should be run through:

```sh
cargo xtask verify-playground-background-launch --report target/reports/playground-background-launch.json
```

That wrapper invokes `cosmic-background-launch`, records the launcher output and
child PIDs, waits for fresh TodoMVC and Cells smoke reports, validates those
reports, and verifies the bounded child processes have exited. Raw
`cosmic-background-launch -- ... --smoke-launch` commands are still useful for
diagnosis, but the wrapper is the acceptance evidence for background launch.

`cargo xtask verify-playground-launch` is the bounded launch smoke for the native
Ply surface. It opens the real playground for TodoMVC and Cells in release mode,
draws several frames, captures nonblank screenshots, and records display/window
metadata plus the expected surface controls. This proves startup/rendering and
the manual-test surface are available, but it is intentionally separate from the
headed OS-input and human-report gates.

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

## D15. Stress Evidence Must Use The Loaded IR Runtime

Decision: TodoMVC and Cells speed stress profiles are part of the language
proof, so they must construct their runtime from `CompiledProgram::from_ir` and
`GenericScheduledRuntime::new`. Cells then initializes through the same loaded
Cells path as normal scenario execution; TodoMVC stress rows are materialized
from the IR-derived row template and compiled row-source bindings.

The stress profiles must not instantiate old default Rust TodoMVC or Cells
tables. This matters because a fast 10,000-row TodoMVC interaction or 26x100
spreadsheet proves little if the measured path is not the Boon source path.
Reports therefore include `ir_runtime_proof` for every TodoMVC and Cells stress
profile, covering the inferred surface, schedule node count, operation counts,
source route count, and source binding counts. Cells also records grid
dimensions; TodoMVC records the row source binding count used for hidden source
identity.

The readiness audit enforces both sides:

- source shape: TodoMVC and Cells stress profiles must use
  `CompiledProgram::from_ir` and `GenericScheduledRuntime::new`; Cells must use
  `initialize_loaded_cells_generic`, and TodoMVC must materialize rows through
  the IR row template instead of a default source-binding helper;
- report shape: TodoMVC and Cells stress profiles must carry IR-derived runtime
  proof with operation counts and source routes.
