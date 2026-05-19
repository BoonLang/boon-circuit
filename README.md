# Boon Circuit

This repository is for the next Boon runtime experiment: a static, circuit-like
engine that keeps Boon's local dataflow style while avoiding the cost of the old
actor engine.

The core direction is:

```text
No central reducer.
No runtime graph cloning.
Values are equations.
Collections are indexed equations over memory.
```

The first target is a Rust static-graph interpreter with a native Ply
playground. Rust/Zig codegen and hardware-oriented lowering come later, after
the semantics are proven on TodoMVC and 7GUIs Cells.

## Documents

- [Architecture decisions](docs/architecture/DECISIONS.md)
- [Runtime model](docs/architecture/RUNTIME_MODEL.md)
- [Language semantics](docs/architecture/LANGUAGE_SEMANTICS.md)
- [LIST and indexed memory model](docs/architecture/LIST_MODEL.md)
- [Delta protocol for renderers and runtimes](docs/architecture/DELTA_PROTOCOL.md)
- [FPGA TodoMVC lowering](docs/architecture/FPGA_TODOMVC_LOWERING.md)
- [Relationship to previous Boon attempts](docs/architecture/PREVIOUS_ATTEMPTS.md)
- [TodoMVC target shape](docs/examples/TODOMVC_CIRCUIT_STYLE.md)
- [Cells target shape](docs/examples/CELLS_CIRCUIT_STYLE.md)
- [Implementation plan](docs/plans/IMPLEMENTATION_PLAN.md)
- [Example verification plan](docs/plans/EXAMPLE_VERIFICATION_PLAN.md)
- [TodoMVC e2e test plan](docs/plans/TODOMVC_E2E_TEST_PLAN.md)
- [`/goal` prompt](docs/plans/GOAL_PROMPT.md)

## Non-Goals For The First Pass

- Do not optimize or repair the original actor runtime.
- Do not use Differential Dataflow as the core engine.
- Do not make TodoMVC a reducer-style `event -> state -> update(state, event)`
  program.
- Do not clone a runtime subgraph for each todo row or spreadsheet cell.
- Do not require user-facing `KEYED LIST` syntax before proving that ordinary
  Boon `LIST` can lower to indexed storage.

## Proof Targets

The first implementation is only convincing if these are true:

1. TodoMVC source keeps the original local field-equation feel.
2. TodoMVC with many rows does not grow runtime graph topology per row.
3. Ordinary TodoMVC does not expose runtime identity, references, or row ids.
4. Cells satisfies the 7GUIs behavior without hardcoded Rust app logic.
5. LIST changes propagate as keyed deltas to Ply, not as whole snapshots.
6. Browser/server runtime sync can exchange semantic deltas, not full state.
7. Every stateful value has a visible next-state equation.
8. TodoMVC is accepted through a headed native Ply replay and manual pass, not
   only a semantic or headless test.
9. Cells and future examples use the same headed/manual/semantic/speed/resource
   verification contract.
10. Normal interactions complete in a couple of milliseconds in release mode
    without excessive RAM or VRAM growth.

## Current Verification Shape

The repo intentionally keeps the final aggregate gate honest. Semantic,
headless, headed Ply, speed, negative, and report-schema checks can be generated
by automation, but `verify-*-all` must still fail until a real human fills a
fresh manual report from a visible headed session.

Useful commands while iterating:

```bash
cargo test --workspace
cargo run -p boon_cli -- dump-ir examples/todomvc.bn
cargo run -p boon_cli -- dump-ir examples/cells.bn
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn
cargo xtask verify-todomvc-headed-ply
cargo xtask verify-cells-headed-ply
cargo xtask verify-todomvc-speed
cargo xtask verify-cells-speed
cargo xtask verify-report-schema
cargo xtask audit-goal-readiness
cargo xtask verify-todomvc-human --write-template
cargo xtask verify-cells-human --write-template
```

The speed aliases re-exec themselves through `cargo run --release -p xtask` and
reports must contain `build_profile: "release"`.
Executable reports contain `runtime_execution` metadata. At the current
prototype stage it explicitly records that source and typed IR are loaded, but
that runtime behavior is still adapter-backed until the generic static-graph
interpreter replaces the TodoMVC/Cells execution adapters.
The native playground sidebar shows the scenario checklist labels used by the
manual-report templates.
The current headed Ply verifier proves a real OS keyboard event reaches the Ply
window, captures nonblank screenshots, and then routes scenario `user_action`
records through the runtime. It intentionally records this as an
`os_input_limitation` until every scenario step is driven by real OS
pointer/keyboard hit testing.

`cargo xtask audit-goal-readiness` is the strict handoff gate. It writes
`target/reports/debug/goal-readiness.json` and exits non-zero while any final
acceptance blocker remains, including adapter-backed runtime execution, hybrid
headed input, missing aggregate reports, or missing fresh human reports.

After a real manual pass, fill:

```text
target/reports/todomvc-human.json
target/reports/cells-human.json
```

and check them with:

```bash
cargo xtask verify-todomvc-human --check --report target/reports/todomvc-human.json
cargo xtask verify-cells-human --check --report target/reports/cells-human.json
cargo xtask verify-examples-all
```
