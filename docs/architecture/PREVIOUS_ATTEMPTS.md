# Relationship To Previous Boon Attempts

This document explains how Boon Circuit differs from the earlier local
experiments. It is not a criticism of those experiments; each one clarified a
constraint that this design keeps.

## Original Boon Actor Engine

What it got right:

- local causality in examples such as TodoMVC.
- values could be written close to the UI/data that affected them.
- `HOLD`, `THEN`, `LATEST`, and list-local state were expressive.

What hurt:

- per-value actors and subscriptions made the runtime heavy.
- dynamic rows tended toward dynamic graph shape.
- event/state propagation involved many runtime objects and queues.
- debugging performance issues became a runtime scheduling problem.

Boon Circuit keeps:

- local field equations.
- `HOLD` as persistent state.
- event-gated updates through `THEN`.
- deterministic merges through `LATEST`.

Boon Circuit changes:

- no actor per value.
- no channel/subscriber graph per todo row.
- no graph cloning for dynamic list items.
- row-local values lower to indexed memories.

## boon-interpreter-rust

What it got right:

- Rust is the right first implementation language.
- `SOURCE` should be canonical for new examples.
- a native playground with an editor is the right proof surface.
- Chumsky-style parser plus diagnostics is a reasonable path.

What was not enough:

- a tree-walking interpreter can accidentally become app-specific.
- reducer-like TodoMVC shape loses the original local dependency style.
- hardcoded Rust behavior for Cells is not an honest language/runtime proof.

Boon Circuit keeps:

- Rust first.
- native playground first.
- parser/IR/runtime split.
- SOURCE-based examples.

Boon Circuit changes:

- compile to a static equation graph before runtime execution.
- require TodoMVC and Cells behavior to live in Boon source.
- use keyed list memories and deltas as core runtime concepts.

## boon-dd

What it got right:

- static graph thinking.
- explicit source injection and output draining.
- recognition that lists, holds, and derived views need a real semantic model.
- good pressure toward deterministic long-lived graphs.

What is too heavy for the next proof:

- Differential Dataflow becomes the central runtime dependency.
- simple UI/state examples risk being explained in DD terms instead of Boon
  terms.
- field ownership can become relational/database-like instead of local
  next-state equations.

Boon Circuit keeps:

- static graph requirement.
- deterministic source/input boundary.
- derived relations and indexes as important concepts.

Boon Circuit changes:

- DD is optional later, not the core runtime.
- explicit `HOLD`/field memories own state.
- simple dirty propagation and indexed memories come first.

## boon-zig And boon-pony

What they clarified:

- SOURCE-only examples are viable.
- codegen/native runtimes are attractive once semantics are stable.
- multiple backends need a shared semantic contract, not per-backend app logic.

Boon Circuit keeps:

- SOURCE as canonical.
- future Rust/Zig/native backend path.
- backend-independent examples.

Boon Circuit changes:

- delay codegen until the interpreter proves the semantics.
- make static equation IR the shared contract before backend work.

## The New Boundary

The core difference is this:

```text
previous actor shape:
  dynamic graph objects own app behavior

previous reducer shape:
  one global state transition owns app behavior

DD shape:
  relational dataflow owns app behavior

Boon Circuit shape:
  local equations own app behavior
  static runtime operators execute those equations over indexed memories
```

This is the intended unification:

```text
source-level experience: original Boon-style local definitions
runtime implementation: hardware-style fixed logic around memory
renderer/network behavior: deterministic keyed deltas
future codegen: static schedules and bounded storage profiles
```
