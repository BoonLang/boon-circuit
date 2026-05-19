# `/goal` Prompt

Use this prompt when starting an unattended implementation pass for this repo.

```text
Continue implementing the full Boon Circuit plan in /home/martinkavik/repos/boon-circuit and verify it honestly end to end. Do not restart the repo or replace the current direction. Treat the repo markdown as the source of truth, especially docs/plans/IMPLEMENTATION_PLAN.md, docs/plans/EXAMPLE_VERIFICATION_PLAN.md, docs/plans/TODOMVC_E2E_TEST_PLAN.md, docs/examples/TODOMVC_CIRCUIT_STYLE.md, docs/examples/CELLS_CIRCUIT_STYLE.md, and docs/architecture/*.md.

Do not commit or push unless I explicitly ask later.

Build the small, fast Rust static-graph interpreter and native Ply playground described in the docs. Keep the implementation deliberately scoped:
- no actor runtime
- no async-per-value or channels-per-value model
- no Differential Dataflow core
- no reducer-style TodoMVC
- no hardcoded Rust TodoMVC or Cells app behavior
- no bytecode VM
- no Rust or Zig codegen in this phase
- no per-row or per-cell runtime graph cloning

Implement:
1. Rust workspace skeleton and crates from the plan.
2. Parser and diagnostics for the required Boon subset.
3. Typed equation IR with static schedules, hidden runtime identity, and IR-level checks that runtime keys, generations, source ids, and bind epochs are not Boon values.
4. Static runtime core with typed storage, keyed dirty sets, deterministic ticks, HOLD, THEN, WHEN, WHILE, LATEST, LIST, append/remove/move, stale source rejection, and keyed semantic deltas.
5. Delta lowering to Ply patches without virtual DOM or list diffing.
6. Native Ply playground with example selector, code editor, run/reset/step controls, render preview, semantic delta log, selected value inspector, dependency explanation panel, and headed/manual test support.
7. TodoMVC in Boon source, close to the original local field-equation style, with no app-visible todo id and no global reducer.
8. Cells in Boon source, with generic formula primitives only, real edit/commit/cancel state in Boon, dependency tracking, cycle errors, and no hardcoded app behavior.
9. Shared example verification harness and all documented xtask commands.

Start by running or reading the current `cargo xtask audit-goal-readiness` report. As of the current state, the known blockers are real implementation blockers:
- reports still say `static_graph_interpreter_adapter_backed`
- `LoadedRuntime` owns generic storage now, but residual TodoMVC/Cells surface drivers still handle event classification, render lowering, and formula behavior
- headed Ply checks still prove only an OS keyboard probe plus scenario replay, not every step through real OS pointer/keyboard hit testing
- aggregate all reports and fresh human reports are still missing

Do not make these blockers green by weakening reports, schemas, docs, or audits. Make them green only by completing the implementation and verification they describe.

Verification must be honest and runnable:
- cargo test -p boon_parser
- cargo test -p boon_ir
- cargo test -p boon_runtime
- cargo test --workspace
- cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn --report target/reports/todomvc-cli-run.json
- cargo xtask verify-todomvc-all --report target/reports/todomvc-all.json
- cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/cells-cli-run.json
- cargo xtask verify-cells-all --report target/reports/cells-all.json
- cargo xtask verify-examples-all
- cargo xtask verify-report-schema
- cargo xtask audit-goal-readiness

The headed Ply checks must open a real native window and prove real OS input, focus, screenshots or video artifacts, display backend and scale metadata, window pid/title, nonblank screenshots, and no direct source-event injection. Do not mark headed verification complete if it only does an OS keyboard probe plus scenario replay. It must drive each scenario step through real OS pointer/keyboard interaction with visible controls, or report that as a blocker.

Manual report checks must reject stale, hand-written, scripted, placeholder, or fake reports. They must require observer, artifact hashes, source/scenario hash matches, display/window metadata, and per-scenario-label pass/fail.

Performance is part of correctness. Normal interactions should complete in a couple of milliseconds in release mode, with bounded RAM/VRAM growth, zero graph rebuilds per interaction, no post-warmup allocations in bounded profiles, and proportional dirty-key/render-patch counts. Large TodoMVC and Cells stress scenarios must prove the graph topology stays static and only affected keys/cells are recomputed.

When finished, leave the repo ready for manual testing:
- all verification commands pass
- reports are written under target/reports
- the native Ply playground can be launched for TodoMVC and Cells
- `cargo xtask audit-goal-readiness` passes without suppressing any blocker
- provide the exact commands I should run for manual testing
- clearly report any remaining blocker or unimplemented planned item instead of treating it as a pass
```
