# Implementation Plan

This plan is for building the first honest Boon Circuit proof in this repo.

## Success Criteria

The implementation is not successful until all of these are true:

1. A native playground can load and run Boon examples.
2. The playground includes a code editor and can run custom Boon source.
3. TodoMVC is written in local field-equation style, not a central reducer.
4. TodoMVC can handle many todos without graph growth per todo.
5. Cells satisfies the 7GUIs behavior without hardcoded Rust app logic.
6. LIST changes emit keyed deltas to the renderer.
7. The debugger can explain causes for a selected field/key.

## Phase 0: Repo Skeleton

Create a Rust workspace:

```text
crates/boon_parser
crates/boon_ir
crates/boon_runtime
crates/boon_ply_playground
examples/
tests/
```

Suggested dependencies:

```text
chumsky       parser
ariadne/miette diagnostics
slotmap       typed ids
rustc-hash    fast hash maps
smallvec      small dependency/candidate lists
bitvec        dirty sets
insta         snapshots
divan         microbenchmarks
ply-engine    native renderer
```

Avoid in the first pass:

- async actor runtime
- channels per value
- Differential Dataflow
- hardcoded TodoMVC or Cells semantics in Rust
- bytecode VM
- Rust/Zig codegen

## Phase 1: Parser And AST

Implement enough Boon syntax for:

```text
records
field access
function calls
pipe calls
SOURCE
HOLD
THEN
WHEN
WHILE
LATEST
LIST
List/map
List/append
List/retain
basic literals
```

Every parsed expression gets a stable `ExprId`.

Hard gate:

```text
cargo test -p boon_parser
```

Snapshot tests should include TodoMVC and Cells snippets.

## Phase 2: Typed Equation IR

Lower AST to typed equations with:

```text
NodeId
ExprId
ScopeId
SourceId
CellId
ListId
FieldId
```

Reject:

- combinational cycles not broken by `HOLD`.
- unknown fields.
- ambiguous source shapes.
- unsupported dynamic behavior for the current profile.

Hard gate:

```text
cargo test -p boon_ir
```

Required debug output:

```text
equation graph dump
source table
cell table
list table
dependency table
```

## Phase 3: Static Runtime Core

Implement:

```text
Runtime
Value
SourceStore
CellStore
ListStore
DeltaBuffer
dirty propagation
tick phases
```

Supported operations:

```text
SOURCE event injection
THEN gating
WHEN matching
LATEST merge
HOLD commit
LIST append/remove/move
List/map row template
List/retain view
```

Hard gate:

```text
cargo test -p boon_runtime
```

Add deterministic tests for:

- same tick conflicting writes.
- stale source generation ignored.
- append initializes row fields.
- remove unbinds row sources.
- keyed field update emits one field delta.

## Phase 4: TodoMVC Proof

Write TodoMVC in `examples/todomvc.bn` using SOURCE and local field equations.

Do not use:

```text
FUNCTION update(state, event)
string event dispatch like "toggle:3"
whole-list replacement for row field changes
```

Hard gates:

```text
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn
cargo bench -p boon_runtime --bench todomvc
```

Performance checks:

- graph node count is stable as todo count grows.
- toggling one todo emits one semantic field delta plus derived render deltas.
- clear completed emits remove deltas for completed keys only.

## Phase 5: Ply Playground

Build a native playground with:

```text
code editor
example selector
run/reset/step controls
render preview
semantic delta log
selected value inspector
dependency explanation panel
```

The playground should prove that custom Boon source is interpreted, not that a
Rust demo app was hand-built.

Hard gate:

```text
cargo run -p boon_ply_playground
```

## Phase 6: Cells Proof

Write Cells in Boon source.

Generic Rust primitives allowed:

```text
Formula/parse
Formula/dependencies
Formula/eval
Grid/cells
```

Hardcoded Rust app behavior is not allowed.

Hard gates:

```text
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn
cargo test -p boon_runtime cells
```

Required scenarios:

- edit literal.
- edit formula.
- formula references another cell.
- chained dependencies recompute.
- cycle produces deterministic error.
- unrelated cell edit does not recompute whole grid.

## Phase 7: Profiles

Introduce runtime profiles:

```text
software_dynamic
software_bounded
hardware_bounded
```

`software_dynamic` can grow vectors/maps.

`software_bounded` uses fixed capacities and reports overflow.

`hardware_bounded` rejects unsupported values such as unbounded text unless a
storage profile is provided.

## Phase 8: Codegen Later

Only after the interpreter proves semantics:

1. Rust codegen from typed equation IR.
2. Zig codegen from typed equation IR.
3. hardware-oriented lowering.

The interpreter should be built as if codegen will follow: explicit schedules,
typed ids, no hidden host semantics, no reducer shortcuts.
