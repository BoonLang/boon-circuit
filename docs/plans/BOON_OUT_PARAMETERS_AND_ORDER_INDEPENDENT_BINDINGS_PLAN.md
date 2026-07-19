# Boon OUT Parameters And Order-Independent Bindings Plan

## Status

This document records an agreed language and compiler direction. It is an
implementation plan, not a description of syntax currently accepted by every
compiler path.

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

Runtime identity is based on structural provenance:

```text
parent scope
+ authoritative operator/list identity
+ hidden row key
+ generation
+ event binding epoch where applicable
```

Output names and parameter ordinals are diagnostics, not runtime identity.

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
runtime identity.

General stateful wrappers must not ship while generic list materialization uses
unstable positional identity or snapshots an entire logical list merely to
render a visible window. Keyed incremental materialization and bounded
virtualization are prerequisites.

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
- one-input `LATEST` is rejected because it performs no merge or selection;
- user documentation does not expose compiler-internal contextual-template or
  item-binder terminology.

## Implementation Order

1. Add parser, typechecker, compiler, runtime, and document negative fixtures
   that encode this contract before changing accepted syntax.
2. Replace string-only function parameters with structured `Value` and `Out`
   declarations and preserve source spans.
3. Implement exact named inputs, bare output bindings, ordered call binding,
   pipe desugaring, required parentheses, and removal of positional and
   argument-renaming fallbacks.
4. Centralize declaration collection and order-independent resolution for
   functions, modules, `BLOCK`, explicit record fields, and call outputs.
5. Separate type constraints, output aliases, value dependencies, temporal
   dependencies, and distributed dependencies, with graph-specific SCC
   diagnostics.
6. Add typed function signatures carrying output ports, correlation, and
   parameter scope effects.
7. Add one contextual-call elaborator and output-net unifier before machine,
   document, and distributed backend lowering.
8. Fix generic keyed list identity, incremental materialization, and bounded
   visible-window virtualization where current positional or full-snapshot
   behavior violates this contract.
9. Erase wrappers and outputs before canonical plan-ID assignment, then compare
   direct and wrapped plans.
10. Move built-in collection operators to the same typed contextual-function
    model and migrate all examples and fixtures.
11. Add semantic tokens, matching-reference ranges, hover/navigation data, and
    inline diagnostics for fresh and forwarded outputs to the dev editor.
12. Verify state, effect, event, persistence, Session, and distributed
    invariants under nested and forwarded outputs.
13. Remove superseded contextual-operator branches, positional call fallbacks,
    compatibility syntax, and stale tests rather than retaining two models.

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
- Semantic-token, hover, reference-range, and diagnostic snapshots distinguish
  fresh output binding, output forwarding, and ordinary value references.

### Plan And Runtime

- Direct `List/map` and one-wrapper forms normalize to equivalent executable
  operations.
- One-wrapper and multi-wrapper forms have identical dirty-set and allocation
  bounds.
- Two calls to the same wrapper retain distinct call-site ownership.
- Nested rows with the same child key under different parents remain distinct.
- Reorder, delete, reinsert, and key reuse preserve generation safety.
- Stale input events are rejected after replacement.
- Offscreen rows rematerialize without duplicate state or effects.
- Constant per-row expressions still receive keyed ownership.
- Repeated reads in one scope share one graph node.
- No wrapper causes a full-list scan, full document relower, or full render
  rebuild.
- Currentness barriers expose derived values before rendering or publication.

### Persistence And Distribution

- Plans and reports contain no runtime `OUT` value or serializable output ID.
- Persisted state is keyed by structural ownership, not parameter names.
- Client, Session, and Server boundaries carry ordinary values/events only.
- Role mismatch and stale Session generation are rejected statically or at the
  host boundary as appropriate.
- Correlated event routes retain parent key, row key, generation, program
  revision, and binding epoch.

### Genericity And Cleanup

- Use unrelated custom list/dictionary fixtures in addition to existing
  examples.
- Scan compiler, runtime, document, renderer, host, and verifier code for
  branches on example or component identity.
- Scan for the removed positional/renaming call fallbacks and hardcoded
  contextual `List/map` handling.
- Compare normalized plans rather than accepting output-only behavioral tests.

## Clear End Condition

This plan is complete only when all of the following are true from final source:

1. The documented fresh and forwarded `OUT` syntax compiles, including
   cross-name forwarding.
2. Exact ordinary argument names, bare output-binding names, declaration
   positions, pipe semantics, and order-independent lexical resolution are
   enforced consistently by every compiler backend.
3. Generic user-defined wrappers can express the collection examples without
   built-in-only contextual syntax.
4. Direct and transparently wrapped forms normalize to equivalent executable
   plans and measured work bounds.
5. Keyed identity, currentness, virtualization, state/effect ownership, stale
   event rejection, persistence, and distributed-role tests pass.
6. All invalid ownership, forwarding, cycle, and correlation cases fail with
   deterministic language-level diagnostics.
7. Superseded contextual-operator branches, argument-renaming behavior,
   compatibility fallbacks, and stale fixtures are deleted.
8. No generic layer contains an example-specific or `List/map`-specific
   shortcut after contextual signatures have been migrated.
9. Relevant workspace and manifest-backed verification gates pass from fresh
   artifacts.
10. The Boon editor visibly distinguishes fresh output bindings, traces
    forwarded outputs, and reports structural output errors without requiring
    runtime execution.

The work must not be marked complete because syntax parses, one example works,
or output values happen to match. Structural plan equivalence and runtime
identity/work evidence are required.

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
- **Textual evaluation order:** it conflicts with Boon's declarative graph and
  makes harmless reordering change meaning.
- **Type-only forwarding compatibility:** equal value types do not prove equal
  identity, cardinality, role, generation, or event correlation.
