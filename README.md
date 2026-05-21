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
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-todomvc-headed-ply
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply
cargo xtask verify-todomvc-speed
cargo xtask verify-cells-speed
cargo xtask verify-todomvc-negative
cargo xtask verify-cells-negative
cargo xtask verify-playground-custom-source
cargo xtask verify-playground-background-launch --report target/reports/playground-background-launch.json
cargo bench -p boon_runtime --bench todomvc -- --report target/reports/todomvc-bench.json --speed-report target/reports/todomvc-bench-speed.json
cargo xtask bench-todomvc
cargo xtask bench-example cells
cargo xtask verify-report-schema
cargo xtask verify-runtime-finality
cargo xtask audit-goal-readiness
cargo xtask verify-todomvc-human --write-template
cargo xtask verify-cells-human --write-template
```

The speed aliases re-exec themselves through `cargo run --release -p xtask` and
reports must contain `build_profile: "release"`.
Executable reports contain `runtime_execution` metadata, but those fields are
not finality proof by themselves. Runtime reports also include
`generic_runtime_slice_evidence`, computed from typed IR plus the compiled
program and checked against `compiled_schedule`, so derived-completeness claims
are tied to concrete route/action/list/formula counts rather than free-form
booleans. The same evidence now includes typed storage-layout counts for root
slots, list memories, row-template columns, and hidden key/generation storage,
all checked against `compiled_schedule`. They also include
`expression_coverage`, computed from parser AST plus
typed IR; executable reports are rejected if parser/lowering coverage reaches
any `Unknown` expression, initializer, update, or predicate fallback.
Accepted executable reports also mirror `runtime_profile`,
`runtime_profile_detail`, `capacities`, and `expression_coverage` inside
`runtime_execution`; schema checks reject reports where the runtime block drifts
from the top-level profile/capacity/coverage contract.
The public IR uses transparent typed IDs such as `ExprId`, `NodeId`,
`ScopeId`, `SourceId`, `StateId`, `ListId`, and `FieldId` for lowering/report
boundaries while preserving numeric JSON output. Row scopes discovered from
`List/map` functions are emitted as typed `row_scopes` entries, and scoped
sources/state/derived values reference them by `ScopeId`.
Each `SourcePort` also carries a typed payload schema derived from AST source
references and match-arm destructuring, so `text`, `key`, and row-address
payload requirements are reportable IR data rather than runtime folklore.
`VIEW` data/source/target bindings are lowered into typed IR as `view_bindings`;
the playground still parses layout lines to draw Ply controls, but the binding
contract is visible in compiler reports.
List storage keeps row slots, visible order, `BitVec` valid bits, and a
free-list separate from key/generation lookup, so deletes do not physically
shift the stored row array even though Boon still observes ordinary data/list
order.
Runtime steps keep reusable dirty keyset storage for keyed list work; reports
derive dirty counts from those keysets instead of only scanning emitted deltas
after the fact.
Compiled source routes carry typed `SourceId` values from IR through runtime
dispatch; string labels remain only as debug/report labels and sorted fallback
metadata.
Hardware explanation reports also use the same honest profile contract:
`target/reports/todomvc-hardware.json` must say `hardware_bounded` and include
effective capacities, capacity source, and overflow behavior from the selected
FPGA target profile.
`cargo xtask verify-runtime-finality` is the structural gate for the
current parser, IR lowering, runtime storage, source-route indexes,
RuntimeProfile/capacity reporting, report-claim derivation, headed-test honesty,
and genericity-scan coverage. If that command or
`cargo xtask audit-goal-readiness` fails, the implementation must still be
treated as prototype-shaped even when TodoMVC and Cells behavior reports pass.
The native playground sidebar shows the scenario checklist labels used by the
manual-report templates.
Launch smoke reports run in isolated Xvfb/X11 and must use macroquad
framebuffer screenshots, not whole-desktop COSMIC screenshots. Interactive
manual playground launches still use the normal Wayland desktop.
Focusless headed reports are synthetic/focusless evidence. Full OS-input
evidence requires canonical `target/reports/todomvc-headed-ply.json` and
`target/reports/cells-headed-ply.json` reports from runs with
`BOON_ALLOW_OS_POINTER_PROBE=1`. The xtask headed aliases run those checks in
isolated Xvfb/X11 by default and remove `WAYLAND_DISPLAY`, so verifier
pointer/keyboard events are real OS events against the test window but cannot
type into the live desktop. Live desktop injection is opt-in only with
both `BOON_ALLOW_LIVE_DESKTOP_INPUT=1` and
`BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS=1`. The lower-level
`boon_ply_playground --verify-headed` and `--verify-os-input-probe` paths
enforce the same rule unless `BOON_OS_INPUT_ISOLATED=xvfb` is present from the
isolated xtask wrapper. Passing full OS reports use
`input_injection_method = "os_pointer_keyboard_to_visible_window"`, have no
`os_input_limitation`, record per-step visible targets and screenshots, and are
checked by `audit-goal-readiness`.

On this COSMIC desktop, open the manual playground surface without stealing
unrelated focus by keeping the wrapper directly around the native window
creator:

```bash
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example todomvc
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example cells
```

Background launch is startup/focus-routing evidence only. Full headed OS input
verification is isolated by default; real manual reports still require a
focused visible playground session.

`cargo xtask audit-goal-readiness` is the strict handoff gate. It writes
`target/reports/goal-readiness.json` by default and exits non-zero while any final
acceptance blocker remains, including adapter-backed runtime execution, partial
headed input, missing aggregate reports, or missing fresh human reports.
Executable reports also expose `remaining_example_specific_shells`; those entries
must stay classified as scenario/assertion/report glue until that residual
TodoMVC/Cells shell is removed.
`cargo xtask audit-machine-readiness` writes
`target/reports/debug/machine-readiness.json` and checks the automated side
without accepting missing human reports as done; it records those artifacts as
deferred to the strict goal gate. It also requires the core machine evidence
reports `target/reports/runtime-finality.json`,
`target/reports/playground-genericity.json`, and their debug mirrors to carry
the current git commit, so an old pass report cannot satisfy a new checkout.
`cargo xtask verify-report-schema` is only a report-shape and artifact-hash
gate. It accepts failing readiness/finality audit reports when those reports
have nonzero `exit_status`, explicit blockers, and failing checklist items, so
schema-valid does not mean handoff-ready.
The readiness audits refresh the recursive schema summary before they inspect
it, so the documented command order remains deterministic after earlier report
commands rewrite artifacts.

After a real manual pass, fill:

```text
target/reports/todomvc-human.json
target/reports/cells-human.json
```

Use the manual helper from `docs/plans/MANUAL_TESTING_RUNBOOK.md`; it requires
the visible manual playground process id and a short focus proof, not the older
headed verifier process id:

```bash
cargo xtask prepare-todomvc-human-report --observer <real-name> --started <unix-start> --finished <unix-finish> --window-pid <visible-playground-pid> --focused-window-proof <how-focus-was-confirmed> --display-server <wayland-or-x11> --display-connection <socket-or-display> --display-scale <scale> --window-backend <backend> --notes <visual-notes> --capture-method <tool-used> --artifact <manual-png-or-video> --pass-label <each-todomvc-scenario-label> --report target/reports/todomvc-human.json
cargo xtask prepare-cells-human-report --observer <real-name> --started <unix-start> --finished <unix-finish> --window-pid <visible-playground-pid> --focused-window-proof <how-focus-was-confirmed> --display-server <wayland-or-x11> --display-connection <socket-or-display> --display-scale <scale> --window-backend <backend> --notes <visual-notes> --capture-method <tool-used> --artifact <manual-png-or-video> --pass-label <each-cells-scenario-label> --report target/reports/cells-human.json
```

The helper writes `manual_report_prepared_by`,
`manual_report_template_path`, and `manual_report_template_sha256`; checker mode
rejects hand-written JSON that skips the prepared-template path.

Then check them with:

```bash
cargo xtask verify-todomvc-human --check --report target/reports/todomvc-human.json
cargo xtask verify-cells-human --check --report target/reports/cells-human.json
cargo xtask verify-todomvc-all --check-existing --report target/reports/todomvc-all.json
cargo xtask verify-cells-all --check-existing --report target/reports/cells-all.json
cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json
cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
```
