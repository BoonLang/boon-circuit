# Boon OUT Parameters And Order-Independent Bindings Plan

## Status

**Implementation-ready canonical plan.** This document records the agreed
language surface, compiler cutover, runtime ownership model, collection API
migrations, verification work, and clear end condition. It is not a
description of syntax currently accepted by every compiler path.

The design must remain a general Boon feature. `List/map` is the motivating
case, but compiler, runtime, document, distributed, and verifier code must not
special-case `List/map` or any example.

## Goals

- Let ordinary Boon functions define and wrap contextual operations such as
  `List/map` without exposing compiler-internal templates or item binders.
- Keep function parameter names and positions exact, readable, and stable.
- Let wrappers forward an output under either the same or a different local
  name.
- Make declarations visible throughout their lexical scope independently of
  textual order.
- Preserve keyed identity, currentness, event routing, incremental work, and
  runtime ownership through any number of transparent wrappers.
- Erase `OUT` wiring before executable runtime plans are produced.
- Diagnose ambiguous ownership, cycles, incompatible scopes, and invalid
  forwarding statically.
- Replace reflective collection calls with typed contextual calls that remain
  indexable and virtualizable.
- Make every executable backend consume one checked and elaborated program
  instead of rediscovering call or row semantics independently.
- Materialize only demanded keyed list windows while keeping logical list
  length, state, dependencies, and identity intact.

## Non-Goals

- `OUT` is not mutable storage and does not copy C#, Ada, or C output-parameter
  semantics.
- `OUT` is not a `SOURCE`, stream, event queue, runtime handle, nominal value,
  or object that application code can persist.
- This design does not add callable values, lambda syntax, `new(item)`, or a
  user-visible template concept.
- This design does not make intentionally ordered constructs unordered.
  Pipeline stages, spread override precedence, and chronological selection such
  as `LATEST` retain their own ordering semantics.
- This design does not preserve positional calls, caller-selected output field
  names, `List/find_value`, or compatibility fallbacks. The migration is an
  atomic language cutover.

## SOURCE Versus OUT

From a Boon user's point of view, `SOURCE` and `OUT` do have an important thing
in common: both receive values against the normal function-input direction.

The three directions are:

```text
ordinary argument   caller -> called function
OUT parameter       called function -> dependent call expression
SOURCE              outside world -> running Boon graph
```

The difference is therefore not that one is an input and the other is an
output. The difference is **who supplies the value and when**.

### OUT: Supplied By The Called Function

An `OUT` declaration says that the called function supplies a scoped value for
other expressions in that call.

```boon
FUNCTION map(list, item: OUT, new) {
    ...
}

todos
|> List/map(
    item
    new: item.title
)
```

`List/map` supplies each current todo as `item`; the caller supplies `new:`,
which may use that item. The caller does not push a value into `item`, and
`item` does not receive future events. It is the contextual value provided by
this invocation of `List/map`.

An `OUT` can also be forwarded through a wrapper:

```boon
item: entry
```

This connects the value supplied through the callee's `item` output to the
caller's existing `entry` output.

### SOURCE: Supplied By The Outside World

A `SOURCE` declaration says that something outside the Boon graph may supply a
new value while the program is running. Examples include a button, keyboard,
timer, file stream, network connection, or hardware input.

```boon
increment: SOURCE

count:
    0 |> HOLD count {
        increment |> THEN { count + 1 }
    }
```

A `SOURCE` may supply zero, one, or many values over time. Each arrival is a new
runtime turn. Application code may react to it or deliberately retain its
value in `HOLD`.

### Using Both Together

The two reverse-flow mechanisms compose naturally:

```boon
todos
|> List/map(
    todo
    new: [
        title: todo.title
        events: [remove: SOURCE]
    ]
)
```

`List/map` supplies each `todo` through `OUT`. Later, the outside world may
supply a `remove` event through the `SOURCE` belonging to that concrete todo.

The short user-facing rule is:

> Use `OUT` when the called function provides a contextual value to the call.
> Use `SOURCE` when the outside world provides live values to the application
> over time.

Both go against ordinary `IN` flow, but they are not interchangeable:

- `OUT` is supplied by the function; `SOURCE` is supplied by the environment.
- `OUT` provides a contextual value during the call; `SOURCE` may deliver new
  values repeatedly after the application is running.
- `OUT` can be forwarded by wrapper functions; `SOURCE` is a live endpoint.
- `SOURCE` introduces a temporal boundary; `OUT` does not and cannot make a
  dependency cycle legal.
- A source value may deliberately be copied into `HOLD`; an output port itself
  cannot be stored, compared, persisted, or serialized.

Internally, these user-visible differences imply different lifetimes. `OUT`
wiring is resolved and removed before execution. `SOURCE` remains as a live,
structurally identified endpoint so the host can deliver later values. This is
an implementation consequence, not the primary way the distinction should be
explained to users.

## Surface Syntax

An output parameter is declared with `OUT` in a function signature:

```boon
FUNCTION map(list, item: OUT, new) {
    ...
}
```

A caller creates a fresh output by writing the canonical output name bare:

```boon
items
|> List/map(
    item
    new: item.value * 2
)
```

The bare `item` is an output-binding declaration, not a positional value
argument. Its name must exactly match the declared `item: OUT` parameter. Calls
do not invent aliases for fresh outputs.

A wrapper connects an existing output by naming it as the argument value:

```boon
item: item
```

Different names are valid when the argument resolves to a compatible existing
`OUT`:

```boon
item: entry
```

This is connection, not declaration renaming. The callee still has its
canonical parameter `item`; the caller has an existing output named `entry`.

An ordinary value with a matching value type is not a valid target for an
`OUT` parameter.

## Complete Wrapper Example

```boon
FUNCTION map_values(dictionary, entry: OUT, new) {
    Dictionary[
        ...dictionary
        entries:
            dictionary.entries
            |> List/map(
                item: entry
                new: [
                    key: entry.key
                    value: new
                ]
            )
    ]
}

dictionary
|> Dictionary/map_values(
    entry
    new: entry.value * 2
)
```

`Dictionary/map_values` does not allocate a second runtime row, source, state
slot, or event route. Its `entry` output is unified with `List/map`'s `item`
output during compilation.

## Calls, Names, Positions, And Pipes

All functions use one consistent call model:

- Every call has parentheses.
- An ordinary input is written `name: expression`.
- A fresh output binding is written as its bare canonical name, such as
  `item`.
- An existing output is forwarded as `formal: existing_output`, such as
  `item: entry`.
- Bare call entries are never ordinary positional arguments.
- Every call entry uses the function's declared parameter name and appears in
  declaration order.
- Unknown, missing, duplicated, renamed, or out-of-order entries are errors.
- Parameter names are part of the function contract; callers cannot rename
  them.
- An output may connect to an existing output with a different local name
  because the right-hand side is a lookup, not a renamed formal parameter.
- The pipe supplies the first ordinary parameter and only changes where that
  argument is written.

These are equivalent:

```boon
List/map(
    list: items
    item
    new: item.value * 2
)
```

```boon
items
|> List/map(
    item
    new: item.value * 2
)
```

The pipe does not introduce a second receiver convention. It is syntax for
moving the first declared argument to the left.

A bare identifier is rejected when the corresponding formal parameter is an
ordinary input:

```boon
calculate(value)
```

The author must write `calculate(value: expression)`. Conversely,
`item: OUT` is not valid call syntax: `OUT` marks a function declaration, not a
runtime value passed by the caller.

The canonical distinction is therefore:

```boon
item          -- create the fresh output declared as item: OUT
item: item    -- forward an existing output with the same local name
item: entry   -- forward an existing output with a different local name
```

If a wrapper accidentally creates a fresh output instead of forwarding its own
declared output, the wrapper output remains structurally undriven. That is a
hard compiler error even when the wrapper output is referenced elsewhere; the
language does not rely only on an unused-parameter warning.

## Reserved PASS Context

`PASS` is compile-time context wiring, not an ordinary function parameter or
argument. It remains a separate reserved call clause:

```boon
Components/button(
    source: store.submit
    label: TEXT { Submit }
    PASS: [store: PASSED.store]
)
```

The rules are deliberately narrow:

- `PASS:` may appear at most once and must be the final call clause.
- Its payload is resolved as compile-time context and is not part of the
  function's declared value or `OUT` parameter list.
- Actual value and `OUT` entries before it must exactly match the declaration
  names and order.
- `PASS` cannot be piped, forwarded as an `OUT`, persisted, serialized, or
  exposed as a runtime value.
- A function cannot declare a parameter named `PASS`; `PASS` and `PASSED` are
  reserved language context names.

Keeping `PASS` separate avoids a hidden optional function parameter and
preserves exact call arity. Keeping it last leaves the complete declared
function contract contiguous and then appends orthogonal context wiring. This
is a source-order rule only: `PASS` does not establish evaluation order, and
ordinary arguments may use the resulting `PASSED` context regardless of where
their expressions appear. The parser, resolver, editor, and migration tooling
must represent it separately from ordinary call entries. Calls whose only
clause is `PASS` are trivially both first and last.

## Canonical Collection APIs

Collection operators use ordinary typed calls plus `OUT`; they do not use
quoted field reflection, caller-selected result field names, or a second
template language.

### List/chunk

The canonical call is:

```boon
rows:
    cells
    |> List/chunk(size: 26)
```

Each result row has canonical typed fields:

```boon
row.items
row.label
```

`items:` and `label:` are not caller-provided field-name arguments. Existing
calls such as `List/chunk(cells, size: 26, items: cells, label: row_number)`
must migrate and the old spelling must be deleted. `row.items` is a lazy keyed
slice retaining the source item identities; `row.label` is the canonical chunk
label/index supplied by the operator.

### List/find

The canonical typed lookup is:

```boon
result:
    cells
    |> List/find(
        cell
        if: cell.address == target_address
    )
```

It returns a typed result:

```boon
Found[value: CELL] | NotFound
```

The result is handled with ordinary Boon matching:

```boon
result |> WHEN {
    Found[value] => value
    NotFound => fallback
}
```

`List/find_value`, quoted `field:`/`target:` arguments, and embedded
`fallback:` behavior are removed. A caller selects a field from the returned
typed row after handling `NotFound`.

The typed predicate is semantic, not reflective. Equality of a current row
field with a loop-invariant value, such as
`cell.address == target_address`, is recognized by typed IR and may use the
existing field index. Cells address lookup must therefore report index hits and
zero scans without a Cells-specific branch. Predicates that cannot use an
index remain correct bounded/incremental predicates and report their scan work
honestly.

### Other Collection Helpers

Operators that naturally evaluate per row use typed `OUT` predicates or
projections, for example `List/filter(item, if: ...)` and
`List/map(item, new: ...)`. Only genuine schema/index declarations may retain
compile-time metadata. User data access must not be encoded as quoted field
names merely to help a backend recognize an operation.

`List/filter_field_equal` and `List/filter_field_not_equal` are removed rather
than retained as convenience aliases. Their canonical replacements are typed
predicates such as `List/filter(item, if: item.family == family)` and
`List/filter(item, if: item.family != family)`. The typed IR may infer an
indexed equality lookup from those predicates, but that optimization is never
expressed through a quoted field name in Boon source.

## Order-Independent Lexical Binding

Declarations are collected before references are resolved within a lexical
scope. Textual order controls presentation, not name visibility.

This applies to:

- function declarations and parameters;
- module declarations;
- variables in `BLOCK`;
- explicit fields in object/record literals;
- fresh `OUT` declarations introduced by calls.

Forward references are therefore legal when their dependency graph is valid:

```boon
BLOCK {
    result: doubled + 1
    doubled: input * 2
}
```

The compiler collects both declarations, resolves the references, builds the
dependency graph, and schedules `doubled` before `result`.

Written call argument order remains exact even though reference visibility is
order-independent. Argument position is part of the API and diagnostic
contract; it is not an evaluation-order mechanism.

### Shadowing

A local declaration shadows an outer declaration throughout its entire lexical
scope, regardless of where the local declaration is written. There is no
before-declaration window in which the outer name is visible.

For explicit record fields, sibling fields are lexical declarations. This
makes a same-name field reference self-referential:

```boon
[
    item: item
]
```

To copy an outer value into a field with the same name, introduce an explicit
enclosing alias:

```boon
BLOCK {
    outer_item: item
    [
        item: outer_item
        text: item.title
    ]
}
```

Only explicit fields introduce lexical names. Spread fields do not silently
import names into the lexical scope. Spread override precedence remains ordered
and is independent from declaration visibility.

### Cycles

Order independence does not permit instantaneous cycles. The compiler must
distinguish and validate at least these graphs:

- type constraints;
- `OUT` aliases and unification;
- ordinary value dependencies;
- stateful temporal dependencies;
- distributed Client, Session, and Server dependencies.

An instantaneous strongly connected component is an error. A cycle is legal
only when a real temporal boundary such as `SOURCE`, `HOLD`, publication, or an
asynchronous host effect breaks it.

Diagnostics must show the declarations and edges that form the cycle instead
of reporting a parser-order failure.

## OUT Scope Semantics

Fresh outputs affect only parameters whose expressions are evaluated under
that output's contextual scope. They do not indiscriminately shadow names in
all sibling arguments.

```boon
List/map(
    list: outer_item.children
    item
    new: item.title
)
```

Here:

- `list:` is evaluated in the parent scope and can read `outer_item`;
- bare `item` creates the per-row output declared by the function as
  `item: OUT`;
- `new:` is evaluated under that output and reads the fresh `item`.

The compiler derives this relationship from the function body and stores it in
the typed function signature. No additional user syntax is required.

A typed function signature must record:

- its ordinary and `OUT` parameters in canonical order;
- which ordinary parameters are evaluated in the parent scope;
- which ordinary parameters are evaluated under each output scope;
- output shape, identity, role, and correlation constraints;
- state or effect ownership introduced by the body.

If one ordinary parameter would need to be evaluated simultaneously in
incompatible output scopes, compilation fails with a diagnostic that asks the
author to split the parameter or restructure the function.

Recursive contextual functions are rejected initially unless scope-effect
inference can prove a finite, unambiguous signature. This avoids silently
falling back to runtime interpretation.

## Static OUT Model

`OUT` is a compile-time dataflow port. A useful internal representation is:

```text
OutNet
  stable declaration ID
  canonical formal parameter and ordinal
  value type
  scalar or keyed/repeated shape
  parent and row scope provenance
  hidden key and generation policy
  runtime-island role
  correlation group
  capability or presence mode
  producer
  consumers
```

An `item: OUT` function parameter declares an output formal. A bare `item` in
the corresponding call slot allocates its fresh, initially undriven
compile-time net. It does not allocate a runtime row, list, object, source,
state slot, effect, or queue.

`parameter: existing_output` aliases or unifies the callee port with an
existing net in the enclosing scope. The right-hand side lookup must not fall
back to the callee's own output. If no enclosing `OUT` exists, compilation
fails.

Forwarding compatibility includes more than ordinary value type:

- value type;
- scalar versus keyed/repeated cardinality;
- parent and row scope;
- authoritative identity and hidden-key policy;
- generation semantics;
- Client, Session, or Server island;
- correlation group for related outputs;
- presence/capability mode where applicable;
- exactly-one-producer and acyclic-unification rules.

An output's ordinary value may be copied into records and results. The `OUT`
identity itself cannot be compared, persisted, serialized, placed in `HOLD`,
inserted into `LIST`, or transported between runtime islands. Distributed
edges carry ordinary bounded values or events after `OUT` has been erased.

## Producers, Correlation, And Branches

Every output forwarding chain must end at exactly one structural producer.

The compiler rejects:

- an output with no structural producer;
- more than one producer;
- an alias cycle;
- a forwarded ordinary value;
- incompatible output shape, scope, role, or generation;
- partial forwarding of a correlated output group.

A structural producer with zero current rows is still a valid producer. Empty
cardinality is not the same as an undriven output.

Multiple related outputs from one operator share a correlation group and must
be forwarded atomically. When practical, one structured output is preferable
to several independently forwarded outputs.

Branch-local producers are not part of the first implementation. Code must
select the input or view first and then drive one output once. A future branch
feature may be added only with exact branch-signature equality and explicit
graph selection; accidental multiple producers remain errors.

A fresh output may be unread by the caller. Constant per-row rendering is
valid:

```boon
items
|> List/map(
    item
    new: Button[text: TEXT { Delete }]
)
```

The button is still instantiated under each keyed output scope. An expression
that does not textually reference the output is not automatically global.
Pure work may be hoisted only when ownership, identity, and behavior are
provably unchanged.

Ordinary unused function parameters are errors. An unused `OUT` value is valid
only when the function still structurally drives it; an undriven declared
`OUT` is an error.

## State, Effects, And Lifecycle

Stateful expressions inside a contextual function are owned by the concrete
expanded expression scope: call site, parent scope, authoritative row key, and
generation. The `OUT` port itself owns no state.

The same rule applies to `SOURCE`, `HOLD`, and host effects:

- transparent wrappers add no ownership layer;
- two wrapper call sites remain distinct;
- nested repeated scopes include every parent key;
- row replacement or branch deactivation cancels owned effects;
- stale generations cannot deliver events to replacement rows;
- rematerializing an offscreen row restores the correct current state without
  duplicating effects.

## Identity And Performance Contract

Contextual calls are elaborated and output nets are unified before final dense
`ScopeId`, `SourceId`, machine-plan, document-plan, or distributed-plan IDs are
assigned.

Runtime identity is based on structural provenance and every repeated ancestor,
not only the nearest row:

```text
OwnerInstanceId
  static owner ID
  ancestor instances: [(list ID, hidden row key, generation), ...]
```

Output names and parameter ordinals are diagnostics, not runtime identity.
`OwnerInstanceId` is interned so normal evaluation and event routing compare a
small stable ID rather than repeatedly allocating or hashing the full ancestry.
State, sources, effects, dependencies, persistence ownership, retained
document rows, and currentness caches all use this same owner.

Values materialized from repeated operators retain ownership explicitly:

```text
KeyedItem
  owner: OwnerInstanceId
  value
```

Host events use a complete route token:

```text
EventRoute
  program revision
  owner instance
  source ID
  row generation
  binding epoch
```

Payload matching, list index fallback, inferred generation `1`, and any other
best-effort recovery are forbidden. A stale or incomplete route is rejected.
The same correlation fields survive Client, Session, and Server transport;
`OUT` itself never crosses that boundary.

After elaboration, transparent wrappers must disappear. Direct and wrapped
forms must produce equivalent executable work:

- no residual wrapper invocation;
- no runtime `OUT` object;
- no copied row solely for forwarding;
- no extra state/source/effect owner;
- no broader dirty set;
- no full-list source scan;
- no additional per-interaction allocation;
- no loss of virtualization or demand-current behavior.

Debug provenance may retain the wrapper stack and source spans without changing
runtime identity. Debug-only provenance is excluded from executable hashes,
persistence schema IDs, wire schema IDs, and direct-versus-wrapped equivalence
comparisons.

General stateful wrappers must not ship while generic list materialization uses
unstable positional identity or snapshots an entire logical list merely to
render a visible window. Keyed incremental materialization and bounded
virtualization are prerequisites.

The normal list/document path exposes logical length and demanded ranges. It
must not first create a full `Vec<Value>`, lower all rows, or recover a row with
`list_row_at(index)`. Map, filter, chunk, and find preserve keyed ownership;
document layout requests a visible range plus bounded overscan; retained row
subframes survive scrolling; only document/render caches may evict offscreen
materialization. Runtime state and formula dependencies remain independently
owned and current.

## Compiler Architecture

The current parser and compiler paths must be replaced coherently rather than
adding another `List/map` exception.

### Structured Parameters

The AST must retain parameter kind and source span:

```text
Parameter
  name
  kind: Value | Out
  ordinal
  source span
```

Storing function parameters as only strings is insufficient because it loses
`OUT` before typechecking.

Call syntax must also preserve whether an entry was bare or named:

```text
ParsedCallEntry
  BareBinding { canonical_name, source_span }
  Named { canonical_name, expression, source_span }

TypedCallEntry
  FreshOut { formal, output_net }
  ForwardOut { formal, enclosing_output_net }
  Input { formal, expression }
```

There is no generic unnamed-expression call entry. A parsed bare binding is
valid only when its canonical name and position match an `OUT` formal. A named
entry is classified as an ordinary input or output forwarding only after the
callee signature is resolved.

### Unified Declaration Collection

One declaration-collection and resolution model should serve functions,
modules, `BLOCK`, record fields, and call outputs:

1. Collect declarations and allocate stable declaration IDs.
2. Resolve names and shadowing against those IDs.
3. Build type, alias, value, temporal, and distributed dependency graphs.
4. Validate graph-specific cycles and compatibility.
5. Topologically elaborate valid expressions.

Machine and document compilers must consume the same typed contextual-call
representation. They must not independently rediscover contextual semantics.

### Scope-Effect Inference

The typechecker analyzes each function body and derives which ordinary
parameters depend on each `OUT`. This scope-effect summary becomes part of the
function signature and is instantiated at call sites.

Built-in structural operators may initially provide trusted signatures, but
they must use the same typed representation and verifier as user-defined
functions. The end state has no separately hardcoded contextual-call syntax.

### Elaboration And Erasure

Before backend lowering:

1. Bind exact named inputs and bare outputs in declaration order.
2. Apply the pipe to the first ordinary parameter when present.
3. Allocate fresh output nets.
4. Resolve forwarded outputs in the enclosing lexical scope.
5. Check type, shape, identity, role, generation, and correlation constraints.
6. Infer and validate each argument's evaluation scope.
7. Expand the function in its declaring runtime island.
8. Unify output nets and validate exactly one producer.
9. Erase transparent function and `OUT` wiring.
10. Assign canonical plan IDs from structural provenance.

No fallback may eagerly compile all call arguments in the caller scope when
the typed signature requires a contextual scope.

### Checked And Erased Programs

The cutover uses two authoritative compiler products rather than letting each
backend reinterpret parser AST:

```text
CheckedProgram (owned by boon_typecheck)
  stable DeclId and LexicalScopeId
  resolved callable identity
  exact typed call entries
  polymorphic contextual signatures
  semantic occurrences and source spans
  scope-effect and correlation summaries
  typed collection predicates and projections

ErasedProgram (owned by boon_ir)
  expanded contextual functions
  unified and validated OutNet graph
  structural OwnerInstanceId anchors
  canonical executable operations
  no OUT, PASS, wrapper call, or parser-level call ambiguity
```

No new crate is required. `boon_typecheck` owns declaration resolution and the
checked semantic program; `boon_ir` owns contextual elaboration, output-net
validation, erasure, and canonical executable IR. Machine, document,
distributed, persistence, native host, and verifier backends consume only the
post-erasure representation.

One typed signature registry covers built-ins and user functions. Initially,
built-ins may register trusted generic signatures and lowering capabilities,
but they are checked through the same call binder and `OutNet` verifier. Parser
row-scope heuristics, `ListMapBinding`, backend-specific positional binders,
string matching on contextual function names, and backend rediscovery of
template arguments are deleted after the cutover.

Transparent wrappers are expanded before executable IDs and hashes are
assigned. The authoritative structural owner belongs to the outer fresh-output
operator call; forwarding wrappers add debug provenance only. This guarantees
that direct, one-wrapper, and multi-wrapper forms produce identical executable
ownership and work.

### Incremental Collection Lowering

Typed collection operations lower to lazy keyed views with at least:

- logical length without value materialization;
- stable row IDs for demanded ranges;
- current field projection reads;
- incremental map, filter, chunk, and find state;
- index selection from typed predicate equality;
- precise dirty-key and dependency propagation;
- bounded visible-window materialization.

`List/chunk` stores an internal keyed slice/range and exposes canonical
`.items`/`.label`; it does not copy every child row. `List/find` installs a
dependency on the selected row and predicate inputs, uses a compatible typed
index when available, and returns `Found`/`NotFound` without evaluating an
unrelated projection or fallback branch.

The document backend asks for the visible range before requesting row values.
It retains row fragments by `OwnerInstanceId` and patches transforms, clips,
text, and style data incrementally. A normal selection, edit, formula update,
or scroll must not trigger a full list snapshot, full document lower, full
layout frame rebuild, full host reconciliation, or full render-scene rebuild.

### Current Repository Cut Map

The implementation starts from concrete duplicated paths that exist today:

- `boon_parser::AstCallArg` stores only an optional name and expression, so it
  must be replaced or complemented by structured bare/named/PASS call entries
  and structured function parameters.
- `boon_typecheck::ListMapBinding`, render-slot template fields, and parser
  helpers such as `list_map_binding_name` encode contextual row behavior outside
  the general function model and must disappear into `CheckedProgram`.
- `boon_ir` and both compiler backends repeatedly inspect `AstCallArg` and
  function strings to reconstruct list-map, find, chunk, and template behavior.
  Those consumers must receive typed call/elaboration nodes instead.
- `boon_plan_executor` currently materializes `ListRef` and `ListMap` as full
  vectors and carries special `MappedRow` values. It must operate on lazy
  `KeyedItem` views and preserve owner identity through projections.
- `boon_runtime::document` may recover materialization identity through
  `list_row_at(list, index)`. That positional fallback must be replaced by the
  keyed owner already carried by the demanded item.
- `List/find_value` has dedicated plan, compiler, executor, and example paths.
  They are removed after source migration to typed `Found | NotFound`.
- `SourceEvent` currently lacks complete program/owner/binding identity, and
  runtime/playground paths still contain default-generation and payload lookup
  recovery. Event routing must fail closed on the complete route token.

This is a deletion map, not a compatibility checklist. Once the replacement
representation is connected, remove these paths in the same implementation
slice rather than leaving old and new execution worlds in parallel.

## Diagnostics

Diagnostics must use the language concepts in this document, not compiler
implementation terms such as templates or binders.

Required errors include:

- expected bare output binding `item` or a forwarded existing output for
  parameter `item: OUT`;
- bare `value` cannot fill ordinary input `value`; write `value: expression`;
- `item: OUT` is not call syntax; write bare `item` to create the output;
- bare output name does not match the canonical function parameter;
- `entry` is an ordinary value and cannot drive output parameter `item`;
- no enclosing output named `entry` exists;
- wrapper output `entry` has no structural producer; if forwarding was
  intended, write `item: entry`;
- output `entry` has two structural producers;
- output forwarding creates an alias cycle;
- output shape or runtime island is incompatible;
- correlated outputs must be forwarded together;
- argument `new` is evaluated under incompatible output scopes;
- argument names or positions do not match the function declaration;
- `PASS:` must appear once at most and after all value/output entries;
- `PASS` is reserved context and cannot be declared or used as a value
  parameter;
- `List/find` requires a typed row predicate and returns `Found | NotFound`;
- reflective `field:`, `target:`, or `fallback:` lookup syntax is not
  supported;
- `List/chunk` result fields are canonical `.items` and `.label` and cannot be
  renamed by the caller;
- local declaration shadows the outer name throughout this scope;
- instantaneous dependency cycle with the complete declaration path.

Warnings are not sufficient for invalid ownership, identity, or output
forwarding.

## Editor And Tooling Contract

Bare output bindings are terse but must not be visually indistinguishable from
ordinary references in the Boon editor.

The parser and typed program expose enough ranges for the IDE to provide:

- a distinct semantic style for a fresh output binding;
- matching reference highlighting inside dependent call expressions;
- hover text such as `OUT item, supplied by List/map`;
- hover or connection information such as
  `List/map.item -> Dictionary/map_values.entry` for forwarding;
- jump-to-signature and jump-to-forwarded-output navigation;
- an optional, user-toggleable inline `OUT` hint beside a bare binding;
- inline zero-driver, multiple-driver, incompatible-output, and shadowing
  diagnostics.

Correctness cannot depend on colors, hover state, or the IDE being present.
The grammar is unambiguous in plain text because ordinary positional arguments
do not exist and bare call entries can only create canonical `OUT` bindings.

## Adjacent Language Consistency

The migration should also enforce already-agreed call consistency:

- all function calls require parentheses;
- all ordinary argument labels and bare output-binding names are canonical and
  exact;
- argument renaming is removed from every function, not only collection
  operators;
- ordinary positional arguments are removed; a bare call entry exclusively
  declares a fresh `OUT` binding;
- a pipe supplies the first ordinary parameter;
- `PASS:` is the only reserved context clause, appears last, and is not an
  argument;
- `List/find_value` and reflective `List/find(field:, value:)` are replaced by
  typed `List/find(item, if:) -> Found | NotFound`;
- `List/filter_field_equal` and `List/filter_field_not_equal` are replaced by
  typed `List/filter(item, if:)` predicates;
- `List/chunk` exposes canonical `.items` and `.label` fields rather than
  accepting caller-selected field names;
- one-input `LATEST` is rejected because it performs no merge or selection;
- user documentation does not expose compiler-internal contextual-template or
  item-binder terminology.

## Implementation Order

Implement this as one compiler/runtime cutover. Intermediate commits may be
temporarily uncompilable inside a local branch, but no compatibility mode,
dual execution path, or permanent syntax adapter may ship.

1. **Freeze executable contract fixtures.** Add small unrelated list and
   dictionary programs for direct, one-wrapper, and multi-wrapper forms;
   `PASS`; `List/find`; `List/chunk`; nested keyed state; effect cancellation;
   stale event routing; and visible-window materialization. Record normalized
   executable sections and work counters, not parser AST snapshots alone.
2. **Introduce structured syntax.** Replace string-only function parameters
   with spanned `Value`/`Out` declarations. Parse call entries as
   `BareBinding` or `Named`, and parse `PASS` into its own optional context
   field. Keep syntax errors local and deterministic.
3. **Build the two-pass resolver.** Predeclare functions, modules, `BLOCK`
   bindings, explicit record fields, parameters, and fresh call outputs with
   stable `DeclId`/`LexicalScopeId`; then resolve references independently of
   textual order. Build labeled type, alias, value, temporal, and distributed
   edges and reject illegal SCCs.
4. **Create the authoritative `CheckedProgram`.** Resolve every callable once,
   bind exact call entries against the unified typed signature registry, infer
   output scope effects and correlation, and emit semantic occurrences for
   tooling. Remove parser-level row-scope inference as soon as consumers use
   this representation.
5. **Enforce the new call model atomically.** Require parentheses, named
   ordinary inputs, canonical bare fresh outputs, named compatible forwarding,
   declaration order, a pipe only for the first ordinary input, and the
   separate final `PASS` clause. Delete positional binding, argument
   renaming, first-unused-parameter recovery, and unknown-name fallback.
6. **Implement `OutNet` elaboration in `boon_ir`.** Allocate and unify output
   nets, validate one producer, type/shape/role/generation/correlation
   compatibility, expand contextual functions in their declaring island, and
   erase wrappers/outputs into `ErasedProgram` before executable IDs and hashes
   are assigned.
7. **Cut every backend to `ErasedProgram`.** Convert machine, document,
   distributed, persistence, native host, and verifier lowering together.
   Delete `ListMapBinding`, template-argument rediscovery, string-based
   contextual function switches, backend positional binders, and runtime
   `OUT` representations. Do not leave an AST fallback.
8. **Install structural keyed ownership.** Intern `OwnerInstanceId` from static
   owner plus all ancestor `(list, key, generation)` instances. Key state,
   sources, effects, dependencies, persistence, retained document rows, and
   currentness by that owner. Route events with program revision, source ID,
   owner, generation, and binding epoch; delete payload/index/default-generation
   recovery.
9. **Replace collection APIs.** Migrate `List/map` and related contextual
   helpers to typed signatures, replace `List/find_value` and reflective find
   calls with `List/find(item, if:) -> Found | NotFound`, and replace renamed
   `List/chunk` outputs with canonical `.items`/`.label`. Derive index use from
   typed predicate equality.
10. **Make collection execution lazy and keyed.** Preserve row identity through
    map/filter/chunk/find; provide logical length and demanded ranges; make
    current field reads precise; and remove full-list value snapshots,
    positional reconciliation, discarded mapped row IDs, and
    `list_row_at(index)` ownership recovery from normal execution.
11. **Cut document/render materialization to visible demand.** Request visible
    ranges plus bounded overscan before row evaluation, retain keyed row
    subframes, and patch selection/edit/scroll state without full lower,
    layout, host reconcile, or scene rebuild. Keep offscreen application state
    independent from evictable document/render caches.
12. **Migrate source mechanically.** Use a temporary parser-aware Rust codemod
    to label ordinary arguments, preserve bare outputs, place `PASS` last,
    quote only genuine metadata constants, add explicit outer aliases for
    same-name record fields, and rewrite find/chunk calls. Migrate all examples,
    tests, embedded Boon source, diagnostics fixtures, persistence migrations,
    and docs. Do not use regex rewriting or Python. Delete the codemod after the
    one-time migration.
13. **Finish diagnostics and editor semantics.** Emit structured primary spans,
    notes, and fixes for call/OUT/PASS/cycle errors. Feed semantic occurrences
    into syntax styling, hover, references, F12/Ctrl-click navigation, and an
    optional inline `OUT` hint that defaults off.
14. **Delete superseded code before broad testing.** Run repository scans and
    remove positional binders, parser row heuristics, backend contextual
    rediscovery, runtime output handles, `List/find_value`, reflective find,
    caller-renamed chunk fields, full-snapshot normal paths, stale fixtures,
    and example-specific branches. Do not quarantine, rename, or preserve them
    as fallback code.
15. **Verify in layers once per completed cut.** Run focused parser/typecheck/IR
    tests, then executor/runtime/document tests, then native scenario and
    performance reports. Only after the source and binaries are final, run all
    reports from `native_gpu_handoff_manifest.json` and the manifest-backed
    aggregate. Avoid repeatedly refreshing expensive reports between small
    edits.

## Verification Matrix

### Language And Typechecking

- Fresh output creation with bare `item` for a declared `item: OUT` formal.
- Same-name forwarding with `item: item`.
- Cross-name forwarding with `item: entry`.
- Rejection of `item: OUT` at a call site.
- Rejection of bare entries for ordinary input parameters.
- Rejection of an ordinary value as an output target.
- Rejection when the forwarding target does not exist in the enclosing scope.
- Rejection of unknown, duplicated, missing, renamed, or out-of-order
  arguments.
- Pipe and explicit-first-argument equivalence.
- `PASS` is parsed separately, accepted only once in final position, and does
  not change function arity, parameter order, or executable data values.
- Rejection of a `PASS` parameter, a non-final or duplicated `PASS`, and
  attempts to pipe, forward, persist, or serialize it.
- Forward references in functions, modules, `BLOCK`, and explicit record
  fields.
- Whole-scope shadowing and explicit enclosing aliases.
- Instantaneous-cycle rejection and temporal-cycle acceptance.
- Rejection of incompatible type, shape, role, generation, presence, or
  correlation.
- Rejection of output alias cycles, zero drivers, and multiple drivers.
- Rejection of the wrapper typo where bare `item` creates a new output while
  the wrapper's declared output remains undriven.
- Rejection of incompatible parameter scope effects.
- Rejection of unsupported branch-local producer combinations.
- Rejection of one-input `LATEST`.
- Typed `List/find(item, if:)` returns `Found[value] | NotFound`; reflective
  find arguments and `List/find_value` are rejected.
- `List/chunk(size:)` exposes only canonical `.items` and `.label`; caller
  field renaming is rejected.
- Semantic-token, hover, reference-range, and diagnostic snapshots distinguish
  fresh output binding, output forwarding, and ordinary value references.

### Plan And Runtime

- Direct `List/map` and one-wrapper forms normalize to equivalent executable
  operations.
- Direct, one-wrapper, and multi-wrapper forms have identical executable graph
  sections, persistence/wire schemas, owner IDs, dirty sets, allocation bounds,
  and evaluated-row counts. Debug provenance is excluded from that comparison.
- Two calls to the same wrapper retain distinct call-site ownership.
- Nested rows with the same child key under different parents remain distinct.
- Reorder, delete, reinsert, and key reuse preserve generation safety.
- Stale input events are rejected after replacement.
- A joined ownership scenario proves row-local `HOLD`, source routing, effect
  cancellation, late-completion rejection, reorder, deletion, and reinsertion
  in one executor run.
- Offscreen rows rematerialize without duplicate state or effects.
- Constant per-row expressions still receive keyed ownership.
- Repeated reads in one scope share one graph node.
- No wrapper causes a full-list scan, full document relower, or full render
  rebuild.
- Currentness barriers expose derived values before rendering or publication.
- A 2,600-row fixture materializes only the visible range plus bounded
  overscan; scrolling changes that window without full-list map/filter/chunk
  evaluation or positional row lookup.
- Indexed `List/find` reports index hits, candidate counts, and zero scans for
  compatible typed equality; non-indexed predicates report bounded scan work.
- Normal list/document counters prove zero full-list snapshots, zero full
  document relowers, zero full host reconciles, and zero full scene rebuilds
  for selection, edit, formula update, and passive scroll.

### Persistence And Distribution

- Plans and reports contain no runtime `OUT` value or serializable output ID.
- Plans, persistence schemas, CBOR frames, native reports, and protocol values
  contain no `PASS`, `OutNet`, wrapper-debug identity, or parser call entry.
- Persisted state is keyed by structural ownership, not parameter names.
- Client, Session, and Server boundaries carry ordinary values/events only.
- Role mismatch and stale Session generation are rejected statically or at the
  host boundary as appropriate.
- Correlated event routes retain parent key, row key, generation, program
  revision, and binding epoch.
- Two Session tabs with overlapping local row keys remain isolated because
  their complete owner ancestry and Session generation differ.

### Genericity And Cleanup

- Use unrelated custom list/dictionary fixtures in addition to existing
  examples.
- Scan compiler, runtime, document, renderer, host, and verifier code for
  branches on example or component identity.
- Scan for the removed positional/renaming call fallbacks and hardcoded
  contextual `List/map` handling.
- Scan for `ListMapBinding`, `List/find_value`, reflective `List/find`,
  `List/filter_field_equal`, `List/filter_field_not_equal`, renamed
  `List/chunk` result fields, parser row-scope heuristics, AST-consuming
  executable backends, runtime output handles, positional owner recovery, and
  default generation/event-route fallbacks.
- Scan compiler, runtime, document, renderer, host, and verifier crates for
  branches on Cells, NovyWave, FjordPulse, or another example identity.
- Compare normalized plans rather than accepting output-only behavioral tests.

## Clear End Condition

This plan is complete only when all of the following are true from final source:

1. The documented fresh and forwarded `OUT` syntax compiles, including
   cross-name forwarding.
2. Exact ordinary argument names and positions, canonical bare output names,
   named forwarding, pipe semantics, order-independent lexical resolution, and
   separate final `PASS` context are enforced once in
   `CheckedProgram`, not independently by backends.
3. `OutNet` validation and transparent expansion produce one `ErasedProgram`;
   every executable backend consumes it and no backend reparses contextual
   semantics from AST, names, or strings.
4. Generic user-defined wrappers express the collection examples without
   built-in-only contextual syntax, and built-ins use the same typed signature
   and output verification model.
5. `List/find(item, if:) -> Found | NotFound` and canonical
   `List/chunk(size:)` are the only supported forms; `List/find_value`,
   reflective lookup arguments, and caller-renamed chunk fields are absent
   from source, fixtures, docs, plans, and executable code.
6. Direct, one-wrapper, and multi-wrapper forms normalize to equivalent
   executable plans, persistence/wire schemas, owner identities, dirty work,
   allocations, and evaluated-row bounds.
7. `OwnerInstanceId` includes all ancestor row keys and generations; state,
   sources, effects, dependencies, persistence, currentness, document rows, and
   event routes consistently use it.
8. Keyed incremental list execution and visible-window document demand are in
   the normal path. A 2,600-row fixture materializes only visible rows plus
   bounded overscan and proves no full-list/full-document fallback during
   selection, edit, formula update, or scroll.
9. Indexed typed find proves index hits and zero scans for Cells-style address
   lookup without any example-specific branch.
10. Keyed identity, currentness, state/effect cancellation, stale event
    rejection, persistence, Session isolation, and distributed correlation
    tests all pass, including reorder/delete/reinsert and late completion.
11. All invalid ownership, forwarding, call, `PASS`, cycle, scope, and
    correlation cases fail with deterministic language-level diagnostics and
    source spans.
12. The Boon editor distinguishes fresh output bindings, traces forwarded
    outputs, reports structural errors, and supports hover/references/navigation
    from typed semantic occurrences without runtime execution.
13. Superseded contextual branches, `ListMapBinding`, positional/renaming
    binders, first-unused recovery, parser row heuristics, runtime output
    handles, positional row recovery, event-route fallbacks, compatibility
    syntax, temporary codemod, and stale fixtures are deleted rather than
    hidden or renamed.
14. No compiler, runtime, document, renderer, host, verifier, or migration layer
    branches on `List/map` after typed registration or on Cells, NovyWave,
    FjordPulse, or another example identity.
15. Focused workspace tests pass, every final-source native report named by
    `docs/architecture/native_gpu_handoff_manifest.json` is fresh and passing,
    and `cargo xtask verify-all --check-existing --report
    target/reports/report-v2/verify-all.json` passes against those artifacts.

The work must not be marked complete because syntax parses, one example works,
output values happen to match, or a compatibility path keeps old fixtures
green. Structural plan equivalence, bounded runtime work, deletion scans,
editor evidence, and fresh manifest-backed verification are all mandatory.

## Implementation Constraints And Initial Limits

- User-defined function parameters are required and have no implicit defaults.
  Only explicitly registered standard-library functions may declare defaults,
  and the typed binder still expands them deterministically.
- Recursive contextual functions and branch-local output producers are rejected
  in the first complete implementation. They may be designed later only with a
  finite scope-effect proof and exact branch signatures.
- `PASS` is the sole reserved call-context clause. It is never a user
  parameter, executable value, persisted value, or wire field.
- The implementation uses existing crates: `boon_typecheck` owns
  `CheckedProgram`; `boon_ir` owns elaboration, output-net verification, and
  `ErasedProgram`.
- Source migration is parser-aware Rust code that is deleted after use. No
  Python, regex-only source rewriting, or permanent compatibility translator is
  added.
- Performance counters and normalized debug provenance are diagnostic side
  channels. They cannot alter executable IDs, currentness, scheduling, or
  visible behavior.

## Rejected Alternatives

- **Repeating `item: OUT` at calls:** `OUT` belongs to the function signature;
  repeating it at every call adds noise, makes it resemble a runtime value, and
  creates two possible spellings for fresh output binding.
- **Using `old |> entry` for forwarding:** `|>` means runtime/dataflow
  transformation into a called function. Output forwarding statically connects
  two ports and has no evaluation order, so overloading the pipe would be
  misleading. Use `old: entry`.
- **Allowing bare ordinary positional arguments:** this would make
  `List/map(item, ...)` ambiguous. Bare call entries are reserved exclusively
  for fresh canonical `OUT` bindings.
- **Call-site output renaming:** changing a declared parameter name at a call
  weakens the API contract. Cross-name forwarding already provides composition
  without renaming the formal parameter.
- **Universal output name:** requiring every operator and wrapper to call its
  output `old` prevents expressive custom abstractions.
- **Callable/lambda arguments:** `new(item)` introduces a second programming
  model and is unnecessary for static dataflow expansion.
- **Runtime output handles:** they complicate persistence, distribution,
  ownership, and performance while losing static graph guarantees.
- **Hardcoded collection operators:** they prevent ordinary Boon wrappers and
  duplicate semantics across compiler backends.
- **Reflective `List/find(field:, target:)`:** quoted application field names
  discard type information and force backends to rediscover structure. A typed
  predicate is clearer and still allows compiler-selected indexes.
- **`List/find_value` with embedded fallback:** it combines lookup, projection,
  and control flow, hides absence, and multiplies special lowering paths. A
  typed `Found | NotFound` result composes with ordinary Boon matching.
- **Caller-named `List/chunk` fields:** allowing `items:` and `label:` to name
  result fields makes result types call-site dependent and reflective. Fixed
  `.items` and `.label` keep the operation typed and optimizable.
- **Treating `PASS` as an optional function parameter:** that would make arity,
  ordering, piping, serialization, and diagnostics ambiguous. It remains a
  separately parsed final compile-time context clause.
- **Allowing `PASS` anywhere:** this has no semantic or implementation benefit,
  breaks the contiguous declaration-order argument sequence, and permits
  inconsistent call layouts. The final position is canonical even when earlier
  arguments read `PASSED` values.
- **Textual evaluation order:** it conflicts with Boon's declarative graph and
  makes harmless reordering change meaning.
- **Type-only forwarding compatibility:** equal value types do not prove equal
  identity, cardinality, role, generation, or event correlation.
