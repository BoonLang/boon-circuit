# Implementation Plan

This plan is for building the first honest Boon Circuit proof in this repo.

## Success Criteria

The implementation is not successful until all of these are true:

1. A native playground can load and run Boon examples.
2. The playground includes a code editor and can run custom Boon source.
3. TodoMVC is written in local field-equation style, not a central reducer.
4. TodoMVC can handle many todos without graph growth per todo.
5. Ordinary TodoMVC does not expose runtime identity, references, or row ids.
6. Cells satisfies the 7GUIs behavior without hardcoded Rust app logic.
7. LIST changes emit keyed deltas to the renderer.
8. The debugger can explain causes for a selected field/key.
9. Every interactive example passes the shared headed/manual/semantic/speed
   verification contract.
10. Normal interactions complete in a couple of milliseconds in release mode
    with bounded RAM and VRAM growth.

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

## Interpreter Scope Control

Keep the first interpreter deliberately small:

```text
parse Boon source
lower to typed equation IR
compile a static schedule
store values in typed arenas/register files
run deterministic ticks
emit semantic deltas
lower deltas to Ply patches
```

Do not add a general VM, actor scheduler, async runtime, DD substrate, per-row
graph builder, or app-specific preview engine to make the examples pass. If an
example needs behavior the core cannot express, either add the smallest generic
primitive needed by the language or mark the example as unsupported. Do not hide
the behavior in Rust demo code.

The release interpreter should be allocation-quiet after warm-up for bounded
profiles. Normal interactions must dirty only the affected nodes/keys plus
declared aggregates/views, and the verification reports must make graph rebuilds,
allocations, latency, RAM, and VRAM visible.

`audit-goal-readiness` must also guard this scope directly. It checks that the
workspace manifests do not pull in a Differential Dataflow/actor/channel-per-
value/codegen/bytecode substrate for this phase, that out-of-phase codegen or
bytecode commands are not exposed, and that speed stress reports keep
`graph_rebuild_count = 0`, `graph_clones_per_item = 0`, stable graph node
counts, and bounded render patch counts.

The same audit parses and lowers `examples/todomvc.bn` and `examples/cells.bn`
directly. It must prove TodoMVC has no global reducer or app-visible identity,
that source tables come from declared `SOURCE` ports, that row-local events stay
row-local in typed IR, and that Cells edit state plus formula primitives are
represented by Boon-derived IR rather than hidden Rust app behavior.

It also requires fresh CLI scenario reports for both examples:
`target/reports/todomvc-cli-run.json` and
`target/reports/cells-cli-run.json`. Those reports must come from the documented
`boon_cli run <source> --scenario <scenario>` commands, match current
source/scenario hashes, and prove the same Boon-source-to-typed-IR-to-static-
runtime path as the harness reports.

The readiness audit also checks scenario label coverage directly from
`examples/todomvc.scn` and `examples/cells.scn`. TodoMVC dynamic-row,
filtered-toggle, clear-all, empty-state, and add-after-clear coverage, and Cells
dependency, fanout, cycle, stale-edge, unrelated-edit, and cancel coverage are
hard gates, not informal checklist items.

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
StateId
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
state table
list table
dependency table
```

Required identity checks:

- hidden runtime keys, slots, generations, scope paths, and source ids are not
  representable as Boon values.
- equality over hidden runtime identity types is impossible in the IR.
- examples with duplicate visible data, such as two todos with the same title,
  still route row-local events to the correct hidden scope.
- string grep checks for `id` are only supplemental; the IR verifier owns this
  rule.

## Phase 3: Static Runtime Core

Implement:

```text
Runtime
TypedSlots
SourceStore
StateStore
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

## Phase 4: TodoMVC Semantic Proof

Write TodoMVC in `examples/todomvc.bn` using SOURCE and local field equations.

The e2e contract is detailed in
[TODOMVC_E2E_TEST_PLAN.md](TODOMVC_E2E_TEST_PLAN.md).

Do not use:

```text
FUNCTION update(state, event)
string event dispatch like "toggle:3"
whole-list replacement for row field changes
user-visible todo id just to make row retention or source routing work
identity/reference comparison in Boon source
```

Hard gates:

```text
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn
cargo xtask verify-todomvc-semantic
cargo xtask verify-todomvc-ply-headless
cargo bench -p boon_runtime --bench todomvc
```

The benchmark command must leave durable evidence, not only terminal output. It
writes `target/reports/todomvc-bench.json`, writes a linked schema-checked
`target/reports/todomvc-bench-speed.json`, and `audit-goal-readiness` fails if
that benchmark evidence is missing or stale. The shared `cargo xtask
bench-example cells` command applies the same report contract to Cells.

Performance checks:

- graph node count is stable as todo count grows.
- todo source does not contain `[id: ...]` or `next_todo_id` unless testing a
  plain data field imported from external domain data.
- toggling one todo emits one semantic field delta plus derived render deltas.
- clear completed emits remove deltas for completed keys only.
- normal interactions satisfy the budget in `examples/todomvc.budget.toml`.
- 1,000 and 10,000 row stress profiles report RAM, VRAM, allocation, and dirty
  key counts, and every stress profile is schema-rejected if it allocates after
  warmup.

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
cargo xtask verify-playground-launch
cargo xtask verify-playground-custom-source
cargo xtask verify-todomvc-all --report target/reports/todomvc-all.json
cargo xtask verify-example-headed-ply cells
```

This gate must open a real native Ply window and verify visible pixels, input,
focus, scaling, and render patches. Headless render checks are useful fast
smokes, but they are not enough to accept the playground.

`verify-playground-custom-source` is the separate editor-path proof. It writes a
modified TodoMVC source text into a report artifact, runs it through the same
source-text execution entry point used by the playground editor, checks a
matching custom scenario, and also proves the modified source fails the original
scenario's initial-state assertions. That prevents the playground from passing
only because bundled example files were hardcoded.

The editor-path proof also writes a modified Cells source/scenario pair. The
Cells variant changes `Grid/cells` dimensions, runs the custom source through the
same source-text entry point, and proves the original full Cells scenario rejects
the smaller grid. `audit-goal-readiness` requires both examples in
`target/reports/playground-custom-source.json`.

`verify-playground-launch` is the bounded native-window smoke proof. It launches
the real Ply playground for TodoMVC and Cells, draws several frames, captures
nonblank screenshots, and writes `target/reports/playground-launch*.json`. It is
startup/render evidence only; it does not replace headed OS-input or human
verification.

Cells has an additional visible-reality gate because its semantic scenario can
exercise only a small subset of a much larger spreadsheet.
`verify-cells-visible-reality` writes
`target/reports/cells-visible-reality.json` and must prove a visible spreadsheet
viewport derived from `Grid/cells(columns: 26, rows: 100)`, including at least
26 columns, 100 rows, 2600 rendered addressed editors, non-A-D address samples,
and nonblank screenshot evidence. Semantic/stress evidence for the 26x100
runtime model is not by itself visible playground parity.

The Cells viewport must be declared in `examples/cells.bn` with generic `VIEW`
components and generic attributes. The playground may interpret those generic
attributes, but it must not render a hardcoded Cells-specific spreadsheet.

The playground also owns the shared example verification harness described in
[EXAMPLE_VERIFICATION_PLAN.md](EXAMPLE_VERIFICATION_PLAN.md):

```text
verify-foundation
verify-playground-launch
verify-example-headed-ply
verify-example-operator-e2e
verify-example-human
verify-cells-visible-reality
verify-example-semantic
verify-example-ply-headless
verify-example-speed
verify-example-negative
verify-example-all
verify-examples-all
verify-report-schema
```

The cross-example command must leave durable evidence too:
`verify-examples-all` writes `target/reports/examples-all.json` after the
TodoMVC and Cells aggregate reports exist. Missing per-example aggregates still
block first, because they include the operator E2E reports.

The final operator E2E and visible/manual follow-up procedures are described in
[MANUAL_TESTING_RUNBOOK.md](MANUAL_TESTING_RUNBOOK.md). Passing aggregate reports
must include fresh checked operator E2E reports bound to current full headed
OS-input evidence. Missing human reports are follow-up items, not final
readiness blockers.

`verify-foundation` is the repo-level parser/IR/runtime gate. It runs
`cargo test -p boon_parser`, `cargo test -p boon_ir`,
`cargo test -p boon_runtime`, and `cargo test --workspace`, then writes
`target/reports/foundation.json`. `audit-goal-readiness` must require this
report so foundational parser/IR/runtime coverage is not only an informal
terminal command.

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
cargo xtask verify-cells-headed-ply
cargo xtask verify-cells-human
cargo xtask verify-cells-visible-reality
cargo xtask verify-cells-semantic
cargo xtask verify-cells-ply-headless
cargo xtask verify-cells-speed
cargo xtask verify-cells-negative
cargo xtask verify-cells-all
```

Required scenarios:

- edit literal.
- edit formula.
- formula references another cell.
- chained dependencies recompute.
- cycle produces deterministic error.
- unrelated cell edit does not recompute whole grid.
- selection, typing, commit, cancel, and focus are tested in the headed Ply
  window.
- large grid edits satisfy `examples/cells.budget.toml`.
- RAM, VRAM, allocation, dirty cell, and recomputed cell counts are reported.

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

## Phase 8: FPGA TodoMVC Contract

Before HDL/codegen work, prove the compiler can produce a hardware plan for the
same no-user-id TodoMVC shape:

```text
LIST capacity
fixed title width
source event bus
hidden slot/generation storage
register-file fields
append/remove state machines
bulk operation scan policy
delta output FIFO
```

Hard gate:

```text
cargo run -p boon_cli -- explain-hardware examples/todomvc.bn --target fpga_todomvc
```

The output should show that row retention and source routing are implemented as
internal slot/generation storage, not as required app-level `id` fields or Boon
identity references.

## Phase 9: Codegen Later

Only after the interpreter proves semantics:

1. Rust codegen from typed equation IR.
2. Zig codegen from typed equation IR.
3. hardware-oriented lowering.

The interpreter should be built as if codegen will follow: explicit schedules,
typed ids, no hidden host semantics, no reducer shortcuts.
