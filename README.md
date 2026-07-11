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

The current target is a PlanExecutor-backed runtime with a native GPU
two-window playground. Rust/Zig codegen, browser/WASM, and hardware-oriented
lowering come after the runtime, document, render, and verifier contracts are
kept generic and fast on TodoMVC and 7GUIs Cells.

## Documents

- [Runtime model](docs/architecture/RUNTIME_MODEL.md)
- [Language semantics](docs/architecture/LANGUAGE_SEMANTICS.md)
- [LIST and indexed memory model](docs/architecture/LIST_MODEL.md)
- [Delta protocol for renderers and runtimes](docs/architecture/DELTA_PROTOCOL.md)
- [FPGA TodoMVC lowering](docs/architecture/FPGA_TODOMVC_LOWERING.md)
- [Relationship to previous Boon attempts](docs/architecture/PREVIOUS_ATTEMPTS.md)
- [TodoMVC target shape](docs/examples/TODOMVC_CIRCUIT_STYLE.md)
- [Cells target shape](docs/examples/CELLS_CIRCUIT_STYLE.md)
- [Implementation plan](docs/plans/IMPLEMENTATION_PLAN.md)
- [Native GPU pipeline contract](docs/architecture/NATIVE_GPU_PIPELINE.md)
- [Native realtime frame-loop and proof plan](docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md)
- [Manual testing runbook](docs/plans/MANUAL_TESTING_RUNBOOK.md)
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
5. LIST changes propagate as keyed deltas to the document/native GPU pipeline,
   not as whole snapshots.
6. Browser/server runtime sync can exchange semantic deltas, not full state.
7. Every stateful value has a visible next-state equation.
8. TodoMVC is accepted through app-owned native GPU host-event evidence, not
   Ply, browser, Xvfb, COSMIC screenshots, or fabricated manual proof.
9. Cells and future examples use the same generic native/document/runtime
   verification contract without example-specific runtime or renderer hacks.
10. Normal interactions complete in a couple of milliseconds in release mode
    without excessive RAM or VRAM growth.

## Verification

The active native contract is
[`docs/architecture/NATIVE_GPU_PIPELINE.md`](docs/architecture/NATIVE_GPU_PIPELINE.md).
The six-gate manifest is
[`docs/architecture/native_gpu_handoff_manifest.json`](docs/architecture/native_gpu_handoff_manifest.json).
Older report-v1 commands and schemas have been removed.

Useful focused commands:

```bash
cargo test -p boon_plan_executor
cargo run -p boon_cli -- run examples/counter.bn --scenario examples/counter.scn
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn
cargo xtask shaders --check
cargo xtask verify-architecture
```

Run fresh native handoff evidence once implementation has stabilized:

```bash
cargo xtask verify-all --report target/reports/report-v2/verify-all.json
```

Validate current reports without rerunning producers:

```bash
cargo xtask verify-all --check-existing \
  --report target/reports/report-v2/verify-all.json
```

Human testing is a separate follow-up after these gates pass. It does not replace
real app-window input, app-owned timing, or exact-frame WGPU proof.
