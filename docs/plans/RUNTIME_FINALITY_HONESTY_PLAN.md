# Boon Circuit Honesty And Final Runtime Plan

## Summary

The audit confirms the repo is useful but still prototype-shaped. Current
TodoMVC and Cells behavior, keyed deltas, generic `VIEW` rendering, and speed
reports are real, but several docs and reports overstate finality. The cleanup
must make implementation and claims match: real AST, typed IR, columnar runtime
storage, dense route indexes, honest bounded profiles, and verification that
distinguishes focusless/synthetic checks from real OS-input checks.

This is not another local TodoMVC workaround. It is a core compiler/runtime
cleanup that makes the checked implementation match the architecture promised
by `docs/architecture/RUNTIME_MODEL.md` and
`docs/architecture/LIST_MODEL.md`.

## Key Changes

- Replace text/line parsing with a real `chumsky` AST parser.
  Remove semantic dependence on `ParsedExpression.text`, `contains(...)`,
  path-name example detection, and comment/string-sensitive construct
  detection. Parse existing TodoMVC and Cells syntax plus `VIEW`, and produce
  spans, stable `ExprId`s, structured expression/operator nodes, and
  diagnostics.

- Replace heuristic IR lowering with typed AST lowering.
  Remove `expression_node_kind`, `field.body.contains`,
  `branch_text_for_source`, source-body scans, TodoMVC/Cells substring
  inference, and path-substring indexed detection. Lower into typed tables:
  `NodeId`, `ScopeId`, `SourceId`, `StateId`, `ListId`, `FieldId`, operator
  kinds, source payload schemas, dependencies, update branches, list
  operations, formula operations, and `VIEW` bindings. Keep strings only as
  debug/report labels.

- Replace hot runtime storage with typed columnar storage.
  Remove hot-path `BTreeMap<String, RuntimeValue>` and `GenericRow` maps from
  normal ticks. Use root slot arrays and list memories with separate order,
  valid bit, generation, free-list, field-column, source-binding, and dirty-key
  storage. Runtime keys, generations, source ids, slots, and bind epochs remain
  hidden from Boon; Boon equality stays data equality.

- Replace route/source lookup with dense compiled indexes.
  Events route by `SourceId` plus hidden binding metadata, not visible labels or
  `Vec::iter().find`. Rename any report field that currently says "index" but
  means "IR-routed semantics", or make it a real dense index.

- Make bounded/runtime profiles honest.
  Add `RuntimeProfile` from budget/profile data for dynamic software, bounded
  software, and hardware-style runs. Reports must show effective capacities,
  overflow behavior, dynamic vs bounded mode, and capacity source. Do not claim
  bounded fixed-capacity execution when TodoMVC is running with unbounded list
  capacity.

- Make report claims derived, not self-attested.
  `generic_interpreter_complete`, `example_behavior_adapter = false`, and
  similar fields must be computed from static/runtime coverage, not hardcoded
  booleans. Reports must include `generic_runtime_slice_evidence` derived from
  typed IR plus the compiled program, and schema checks must bind that evidence
  to `compiled_schedule` instead of accepting free-form capability claims. Any
  accepted executable report must also include `expression_coverage`, computed
  from parser AST plus typed IR, with zero `Unknown` expression, initializer,
  update, or predicate fallback counts. The runtime block must mirror the
  top-level `runtime_profile`, `runtime_profile_detail`, `capacities`, and
  `expression_coverage` fields exactly; schema and readiness audits must reject
  drift between those copies.
  remaining TodoMVC/Cells shell must be listed explicitly as an
  allowed scenario/assertion/report shell through
  `remaining_example_specific_shells` until removed.

- Separate verification categories honestly.
  Focusless headed reports must say they are synthetic/focusless. Full OS-input
  claims require canonical `todomvc-headed-ply.json` and `cells-headed-ply.json`,
  current hashes, the current git commit, a generated timestamp no older than
  24 hours, real OS pointer/keyboard backend per user-action step, and no
  synthetic observations. Those canonical headed
  aliases run in isolated
  Xvfb/X11 by default; live desktop injection is not allowed unless explicitly
  requested with both `BOON_ALLOW_LIVE_DESKTOP_INPUT=1` and
  `BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS=1`.
  `verify-report-schema`
  should be renamed or
  supplemented so it means "existing reports are schema-valid", not "readiness
  passed". Failing readiness/finality audit reports are schema-valid only as
  blocker evidence: they must have nonzero `exit_status`, explicit blockers,
  and failing checks.

- Extend genericity gates.
  `verify-playground-genericity` must scan renderer, `VIEW` parser,
  source-event probes, headed probes, and app-control probes. Example-specific
  element ids or TodoMVC/Cells branches are allowed only in examples, scenarios,
  docs, report labels, or explicitly named test fixtures.

## Test Plan

- Parser/IR:

  ```bash
  cargo test -p boon_parser -p boon_ir
  ```

  Include snapshots and negative tests proving comments/strings containing
  operator names do not change IR.

- Runtime:

  ```bash
  cargo test -p boon_runtime
  ```

  Prove dense IDs, hidden identity, duplicate title routing, stale binding
  rejection, append/delete/move, bounded overflow, typed storage, and Cells
  formula fanout.

- Playground:

  ```bash
  cargo test -p boon_ply_playground
  ```

  TodoMVC and Cells must still render from Boon `VIEW` through the generic path.

- Audits/reports:

  ```bash
  cargo xtask verify-playground-genericity --report target/reports/playground-genericity.json
  cargo xtask verify-runtime-finality
  cargo xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json
  cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json
  cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
  cargo xtask verify-report-schema
  ```

  `audit-machine-readiness` is the unattended implementation gate. It must pass
  before handoff and must not count missing real human reports as accepted.
  It also requires `target/reports/runtime-finality.json`,
  `target/reports/playground-genericity.json`, and their debug mirrors to carry
  the current git commit, while `verify-report-schema` remains only a
  shape/hash-validity gate.
  Readiness audits refresh the recursive schema summary before inspecting it, so
  this command order is valid even though earlier commands rewrite report
  artifacts.
  `verify-examples-all` and `audit-goal-readiness` are final acceptance gates:
  they require fresh checked `target/reports/todomvc-human.json` and
  `target/reports/cells-human.json` reports from a real visible manual session,
  plus the dependent `*-all.json` aggregates. If those human reports are
  missing, the correct result is failure with explicit blockers, not a synthetic
  pass or a weakened schema.

  Add negative fixtures for synthetic reports pretending to be full OS-input
  reports.

- Manual launch:

  ```bash
  cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_ply_playground --example todomvc --mode app
  ```

## Assumptions

- Use `chumsky`, `slotmap` or equivalent newtype arenas, `rustc-hash`, and
  `bitvec`.
- Do not introduce Differential Dataflow, actor-per-value runtime, bytecode VM,
  or Rust/Zig codegen in this cleanup.
- Existing TodoMVC/Cells user behavior must not regress.
- If anything remains prototype-only, it must be named as a blocker and
  `audit-goal-readiness` must fail until resolved.

## Copy-Paste `/goal` Prompt

```text
/goal In /home/martinkavik/repos/boon-circuit, implement the honesty/finality cleanup for Boon Circuit. Do not make another local TodoMVC workaround. Make the implementation and reports match docs/architecture/RUNTIME_MODEL.md and docs/architecture/LIST_MODEL.md.

Implement completely:

1. Replace the current text/line parser with a real chumsky AST parser. Parse existing TodoMVC and Cells syntax including SOURCE, HOLD, LATEST, WHEN, THEN, WHILE, LIST, List/map, List/append, List/retain, List/count, Formula/* calls, function calls, pipe calls, records, literals, and VIEW. Semantics must not depend on ParsedExpression.text, contains checks, comments, file path names, or substring markers.

2. Replace heuristic IR lowering with typed AST lowering. Remove expression_node_kind and all semantic lowering based on field.body.contains, branch_text_for_source, source-body scans, hardcoded todo/cell substring checks, and path-substring indexed detection. Lower into typed IR tables with NodeId, ExprId, ScopeId, SourceId, StateId, ListId, FieldId, operator kinds, source payload schemas, dependencies, update branches, list operations, formula operations, and VIEW bindings.

3. Replace hot runtime storage with typed columnar storage. Remove BTreeMap<String, RuntimeValue> and GenericRow field maps from normal tick storage. Implement typed root slots and list memories with separate order, valid bits, generations, free slots, field columns, source bindings, dirty keysets, and hidden runtime identity. Boon code must not see or compare runtime keys, slots, generations, SourceIds, or bind epochs.

4. Replace linear source/route lookup with dense compiled indexes. Runtime event routing must use SourceId and hidden binding metadata. Reports must not use "index" labels for Vec iter().find/position tables unless they are real dense indexes.

5. Add honest RuntimeProfile handling. Support dynamic software, bounded software, and hardware-style profiles from budget/profile data. Reports must show effective capacities, overflow behavior, dynamic vs bounded mode, and capacity source. Do not claim bounded fixed-capacity execution when the runtime is unbounded.

6. Make generic-runtime completeness derived, not self-attested. generic_interpreter_complete, example_behavior_adapter, and similar fields must be computed from static/runtime coverage. Any remaining TodoMVC/Cells shell must be visible in reports as scenario/assertion/report glue until removed.

7. Fix headed-test honesty. Focusless headed reports must remain clearly synthetic/focusless. Full OS-input claims require canonical todomvc-headed-ply.json and cells-headed-ply.json, current hashes, input_injection_method = os_pointer_keyboard_to_visible_window, no limitation field, and real OS pointer/keyboard backend for every user-action step. Add negative fixtures rejecting reports that claim full OS while using synthetic/focusless observations.

8. Extend playground genericity checks. verify-playground-genericity must scan renderer, VIEW parser, source-event probes, headed probes, and app-control probes. Example-specific control ids or TodoMVC/Cells branches are allowed only in examples, scenarios, docs, report labels, or explicitly named test fixtures.

9. Fix schema/readiness naming. verify-report-schema must be clearly "existing reports are schema-valid", or add a separate canonical-required-report gate so schema success cannot be mistaken for readiness.

10. Update docs honestly. Architecture/plans/README must describe the implemented state. If anything remains prototype-only, name it as a blocker and make audit-goal-readiness fail until resolved.

Do not stop until these pass from a clean tree, except that the final
`verify-*-all`, `verify-examples-all`, and `audit-goal-readiness` commands must
remain blocked when no real human TodoMVC/Cells reports exist:
cargo fmt --check
cargo test -p boon_parser -p boon_ir -p boon_runtime -p boon_ply_playground
cargo xtask verify-playground-genericity --report target/reports/playground-genericity.json
cargo xtask verify-runtime-finality
cargo xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json
cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json
cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
cargo xtask verify-report-schema

Then relaunch:
cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_ply_playground --example todomvc --mode app

At the end, report changed files, verification commands/results, report paths, launch PID, and any blockers. If any blocker remains, do not describe the runtime as final, complete, or fully honest.
```
