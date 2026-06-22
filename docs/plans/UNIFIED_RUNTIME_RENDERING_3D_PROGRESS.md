# Unified Runtime, Rendering, 3D, and Manufacturing Progress

This ledger tracks the active unified `/goal` that continues the existing
BYTES/MachinePlan migration into retained runtime/document/layout/rendering,
shared native/browser WGPU, accessibility, 3D, and manufacturing.

It does not replace `docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md`. The
BYTES/MachinePlan ledger remains authoritative for that migration; this ledger
links to it and records unified-phase progress and blockers.

## Contract

- Goal prompt: `docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md`
- Unified architecture: `docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md`
- Active BYTES/MachinePlan ledger:
  `docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md`
- Current HEAD at unified start:
  `271d295a6169ef020570fbb96429c1a8e0d5e296`
- Branch at unified start: `main`
- Dirty tree at unified start: clean
- Start timestamp: `2026-06-23T00:32:38+02:00`
- Toolchain:
  - `rustc 1.96.0 (ac68faa20 2026-05-25)`
  - `cargo 1.96.0 (30a34c682 2026-05-25)`

## Status Definitions

- `not-started`: no implementation work recorded in this unified ledger.
- `in-progress`: implementation or evidence gathering has started, but exit
  gates are not satisfied.
- `blocked`: useful work remains but a specific current prerequisite prevents
  this phase from advancing further in its current shape.
- `implemented`: code/doc/report changes exist for the phase, but full proof is
  not complete.
- `verified`: phase exit gates pass with fresh evidence.

## Phase Status

| Phase | Status | Current Evidence |
| --- | --- | --- |
| U0 - Baselines and Task Graph | in-progress | This ledger exists and records the first current-state probes. Full release/debug baselines for TodoMVC, Cells, native idle, interaction paths, GPU bytes/writes/cache behavior, and browser/WASM artifacts are not yet complete. |
| U1 - Complete BYTES/MachinePlan Half-Migration | in-progress | BYTES aggregate, runtime hardening/finality, machine readiness, and recursive report-schema freshness are current and passing for start HEAD `271d295`. `audit-goal-readiness` still fails on partial phases, missing Cells release benchmark, and default legacy execution. |
| U2 - Transactional Document Changes and Hot Retained Model | in-progress | First document transaction slices added: `DocumentChangeBatch`, `DocumentChangeSet`, atomic rollback, merged dirty facts, precise `InsertChild`/`RemoveChild`/`MoveChild` patches, native runtime-render-patch target lowering through typed document batches for incremental layout fast paths, and deterministic numeric hot node IDs with debug-name tables. Interned styles, broader typed bindings, retained layout integration, and protocol migration remain open. |
| U3 - Retained Incremental Layout and Shared Text | not-started | Pending U2. |
| U4 - Canonical Retained Render Model | not-started | Pending U2/U3. |
| U5 - WGPU Retained Resources, Owned Targets, Demand Scheduling | not-started | Pending U4 and native GPU baseline refresh. |
| U6 - Shared Native/Web Visual Path and Semantic Accessibility | not-started | Pending canonical render contract and WGPU retained resource work. |
| U7 - World Scene and Basic 3D | not-started | Pending app output ports and render contract. |
| U8 - SolidGraph, AssemblyGraph, Visual Compilation | not-started | Pending WorldScene basics. |
| U9 - Manufacturing Compiler and 3MF | not-started | Pending SolidGraph/AssemblyGraph. |
| U10 - Parametric Car Assembly | not-started | Pending manufacturing and 3D foundations. |
| U11 - Cleanup, Defaults, Documentation | not-started | Pending prior phase verification and soak evidence. |

## Initial Dependency Graph

```text
U0 baselines/task graph
  -> U1 BYTES/MachinePlan closure or bounded blocker disposition
     -> U2 transactional document hot model
        -> U3 retained layout/text
           -> U4 canonical render scene/patch contract
              -> U5 retained WGPU resources/readback/demand scheduling
                 -> U6 shared native/web visual + SemanticScene/accessibility
                    -> U7 WorldScene/basic 3D
                       -> U8 SolidGraph/AssemblyGraph/visual compile
                          -> U9 manufacturing/3MF
                             -> U10 parametric car
                                -> U11 cleanup/defaults/docs
```

`TASK-0804A` and related Cells benchmark work may block the old default-switch
readiness gate, but the unified goal permits forward progress into U2-U5 after
bounded diagnosis if the blocker is plausibly architectural and remains
recorded honestly.

## 2026-06-23 - U0 Current-State Baseline Start

Status: U0 in-progress; U1 in-progress by inherited BYTES/MachinePlan state.

Files changed:

- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`

Docs inspected for startup:

- `AGENTS.md`
- `docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md`
- `docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md`
- `docs/plans/BYTES_AND_MACHINE_PLAN_IMPLEMENTATION.md`
- `docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md`
- `docs/plans/RUNTIME_FINALITY_HONESTY_PLAN.md`
- `docs/plans/REMOVE_VIEW_DOCUMENT_UI_GOAL.md`
- `docs/plans/NATIVE_DEMAND_DRIVEN_RENDER_LOOP_PLAN.md`
- `docs/plans/GOAL_PROMPT.md`
- `docs/architecture/RUNTIME_MODEL.md`
- `docs/architecture/LIST_MODEL.md`
- `docs/architecture/DELTA_PROTOCOL.md`
- `docs/architecture/NATIVE_GPU_PIPELINE.md`

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `date -Iseconds && rustc --version && cargo --version && git status --short && git rev-parse HEAD` | Pass | Start timestamp `2026-06-23T00:32:38+02:00`; current HEAD `271d295a6169ef020570fbb96429c1a8e0d5e296`; clean tree before this ledger was created. |
| `cargo xtask --help` | Pass | Listed current xtask surface, including BYTES/MachinePlan gates, `audit-goal-readiness`, native GPU gates, and `audit-machine-readiness`. Existing `boon_native_gpu` dead-code warnings only. |
| `cargo fmt --all -- --check` | Pass | Formatting was clean before this ledger was added. |
| `target/debug/xtask verify-bytes-machine-plan-all --check-existing --report target/reports/bytes-plan/bytes-machine-plan-all.json` | Expected Fail | Wrote `target/reports/bytes-plan/bytes-machine-plan-all.json`; `status=fail`, `checked_report_count=56`, `required_report_count=56`, `proof_report_count=46`, `diagnostic_report_count=10`. No failed child status, stale child schema, or stale child artifact hash was reported, but 55 required child reports were not generated for current commit. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Wrote `target/reports/bytes-plan/goal-readiness.json`; readiness still fails on partial/not-started BYTES phases, stale aggregate/worktree evidence, missing Cells release benchmark wrapper, stale runtime/readiness/schema reports, and default legacy execution. |
| `target/debug/xtask verify-report-schema target/reports/bytes-plan/bytes-machine-plan-all.json target/reports/bytes-plan/goal-readiness.json` | Pass | The refreshed aggregate/readiness report shapes schema-validate even though both reports have `status=fail`. |

Initial BYTES aggregate blockers:

- `55` required BYTES/MachinePlan child reports were not generated for current
  commit after the latest documentation/checkpoint commits.
- The first stale child reports include:
  - `target/reports/bytes-plan/bytes-initial-dump-plan.json`
  - `target/reports/bytes-plan/bytes-initial-run-plan.json`
  - `target/reports/bytes-plan/root-scalar-plan-ops-dump-plan.json`
  - `target/reports/bytes-plan/root-scalar-plan-ops-scenario-run-plan.json`
  - `target/reports/bytes-plan/bytes-length-plan-ops-scenario-run-plan.json`
  - `target/reports/bytes-plan/bytes-is-empty-plan-ops-scenario-run-plan.json`
  - `target/reports/bytes-plan/bytes-get-plan-ops-scenario-run-plan.json`
  - `target/reports/bytes-plan/bytes-equal-plan-ops-scenario-run-plan.json`

Initial readiness blockers:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `BYTES/MachinePlan aggregate is stale for current HEAD`
- `BYTES/MachinePlan aggregate is stale for current worktree fingerprint`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `target/reports/runtime-production-hardening.json` is stale for current HEAD
- `target/reports/runtime-finality.json` is stale for current HEAD
- `target/reports/debug/machine-readiness.json` is stale for current HEAD
- `target/reports/schema.json` is stale for current HEAD
- `boon_cli run` still defaults to legacy, so Phase 10 default switch has not
  happened

Decision notes:

- The first unified implementation task is evidence freshness, not a semantic
  code change: refresh or regenerate the stale required BYTES/MachinePlan child
  reports for the current commit, then rerun aggregate/schema/readiness.
- Do not claim current-goal closure while the readiness report has
  implementation blockers.
- Do not spend unbounded time on `TASK-0804A`; after report freshness is
  restored, run bounded diagnosis and either fix it, record a reviewed blocker
  disposition, or supersede it with replacement evidence.

Next executable task:

1. Replay current `command_argv` entries for stale BYTES/MachinePlan child
   reports where available.
2. Rerun `verify-bytes-machine-plan-all --check-existing`.
3. Rerun `verify-report-schema` for the aggregate/readiness reports.
4. Rerun `audit-goal-readiness`.
5. Update this ledger with the refreshed evidence and any still-stale child
   reports that require manual regeneration.

## 2026-06-23 - U1 BYTES Aggregate Freshness Restored

Status: U1 in-progress; evidence freshness slice complete for the
BYTES/MachinePlan aggregate.

Files changed:

- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`
- regenerated ignored reports under `target/reports/bytes-plan/`
- replay logs under `target/reports/unified/logs/`

What changed:

- Replayed all 55 stale required BYTES/MachinePlan child report commands from
  their embedded `command_argv` values.
- Refreshed the adversarial aggregate-tamper report after the aggregate changed,
  because it intentionally binds to the aggregate artifact.
- Reran the aggregate after replay.

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| replay loop over stale `target/reports/bytes-plan/*.json` child reports | Pass | `stale_report_count=55`, `replay_failed=0`; logs in `target/reports/unified/logs/replay-*.log`. |
| `target/debug/xtask verify-bytes-machine-plan-adversarial --report target/reports/bytes-plan/bytes-machine-plan-adversarial.json` | Pass | Refreshed the adversarial report after aggregate/replay churn; wrote `target/reports/bytes-plan/bytes-machine-plan-adversarial.json`. |
| `target/debug/xtask verify-bytes-machine-plan-all --check-existing --report target/reports/bytes-plan/bytes-machine-plan-all.json` | Pass | Wrote a fresh aggregate with `status=pass`, `checked_report_count=56`, `required_report_count=56`, `proof_report_count=46`, `diagnostic_report_count=10`, no blockers, no failed child reports, no stale child schemas, and no stale child artifact hashes. |
| `target/debug/xtask verify-report-schema target/reports/bytes-plan/bytes-machine-plan-all.json target/reports/bytes-plan/bytes-machine-plan-adversarial.json target/reports/bytes-plan/goal-readiness.json` | Pass | Current aggregate/adversarial/readiness report shapes schema-validate. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Aggregate freshness blocker is gone. Readiness still fails on real roadmap/default-switch blockers and stale support reports. |

Current aggregate summary:

```json
{
  "status": "pass",
  "git_commit": "271d295",
  "checked_report_count": 56,
  "required_report_count": 56,
  "proof_report_count": 46,
  "diagnostic_report_count": 10
}
```

Current readiness blockers after aggregate refresh:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `target/reports/runtime-production-hardening.json` is stale for current HEAD
- `target/reports/runtime-finality.json` is stale for current HEAD
- `target/reports/debug/machine-readiness.json` is stale for current HEAD
- `target/reports/schema.json` is stale for current HEAD
- `boon_cli run` still defaults to legacy, so Phase 10 default switch has not
  happened

Decision notes:

- The stale aggregate was a report-freshness problem caused by later commits,
  not evidence of a new BYTES semantic regression.
- U1 is still not complete. The old BYTES/MachinePlan readiness gate continues
  to fail for partial phases, Cells benchmark/TASK-0804A, stale support
  reports, and the default-engine switch.

Next executable task:

1. Refresh the stale readiness-support reports:
   - `target/reports/runtime-production-hardening.json`
   - `target/reports/runtime-finality.json`
   - `target/reports/debug/machine-readiness.json`
   - `target/reports/schema.json`
2. Rerun `audit-goal-readiness`.
3. Decide whether the next U1 task is bounded `TASK-0804A` diagnosis or a
   reviewed blocker disposition that allows U2 runtime/document work to begin.

## 2026-06-23 - U0/U1 Readiness Support Refresh

Status: U0 in-progress; U1 in-progress; report-freshness work partially
complete.

Files changed:

- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`
- regenerated ignored reports:
  - `target/reports/runtime-production-hardening.json`
  - `target/reports/runtime-finality.json`
  - `target/reports/debug/machine-readiness.json`
  - `target/reports/bytecode-counter.json`
  - `target/reports/bytes-length-audit.json`
  - `target/reports/bytes-plan/cells-plan-compare-after-runtime-cache-key.json`
  - `target/reports/bytes-plan/phase1-bytes-initial-dump-plan.json`
- archived stale ignored diagnostic reports out of `target/reports/`:
  - `target/diagnostics/bytes-plan-negative-probes/_negative-*.json`
  - `target/diagnostics/bytes-plan-cells-benchmarks/cells-release-benchmark-*-speed.json`
  - `target/diagnostics/bytes-plan-archived-reports/review-bytes-negative.json`

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `target/debug/xtask verify-runtime-production-hardening --report target/reports/runtime-production-hardening.json` | Pass | Refreshed runtime production hardening for current HEAD. |
| `target/debug/xtask verify-runtime-finality --report target/reports/runtime-finality.json` | Pass | Refreshed runtime finality for current HEAD. |
| `target/debug/xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json` | Expected Fail | Report is fresh for current HEAD, but fails because five native/playground reports are stale. |
| `target/debug/xtask verify-bytecode counter --report target/reports/bytecode-counter.json` | Pass | Refreshed stale top-level bytecode report required by the recursive schema scan. |
| `target/debug/boon_cli run-plan-root-scalar-scenario examples/bytes_length_plan_ops.bn --scenario examples/bytes_length_plan_ops.scn --steps measure-bytes --compare-legacy --report target/reports/bytes-length-audit.json` | Pass | Refreshed stale focused BYTES length audit report. |
| `target/debug/boon_cli run examples/cells.bn --scenario examples/cells.scn --engine compare --report target/reports/bytes-plan/cells-plan-compare-after-runtime-cache-key.json` | Pass | Refreshed full Cells compare diagnostic; command printed a large passing report to stdout and wrote the report file. |
| `target/debug/boon_cli dump-plan examples/bytes_initial.bn --report target/reports/bytes-plan/phase1-bytes-initial-dump-plan.json` | Pass | Refreshed older focused Phase 1 BYTES dump-plan report. |
| `target/debug/xtask verify-report-schema` | Fail | The recursive schema scan progressed through several stale reports after each refresh/archive, but still has historical stale artifacts under `target/reports/`. Latest explicitly observed stale artifacts were resolved or archived; the scan has not yet reached a final pass in this slice. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Readiness now fails on partial/not-started BYTES phases, missing Cells release benchmark, failing machine-readiness report, stale `target/reports/schema.json`, and default legacy execution. |

Current machine-readiness blockers:

- `target/reports/native-gpu/preview-e2e-todomvc.json` is stale for current git
  commit
- `target/reports/native-gpu/todomvc-two-window-content.json` is stale for
  current git commit
- `target/reports/native-gpu/todomvc-reference-parity.json` is stale for
  current git commit
- `target/reports/native-gpu/todomvc-input-parity.json` is stale for current git
  commit
- `target/reports/playground-genericity.json` is stale for current git commit

Current goal-readiness blockers:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `target/reports/debug/machine-readiness.json` did not pass
- `target/reports/schema.json` is stale for current HEAD
- `boon_cli run` still defaults to legacy, so Phase 10 default switch has not
  happened

Decision notes:

- The current BYTES/MachinePlan aggregate is no longer the blocker; it is fresh
  and passing at 56/56.
- Runtime production hardening and runtime finality are fresh and passing.
- Machine readiness is now the main stale-support-report blocker before the
  old readiness gate can expose only semantic/default-switch blockers.
- Several stale diagnostic artifacts under `target/reports/` were historical
  proof/negative/benchmark artifacts, not active readiness inputs. They were
  moved under `target/diagnostics/` so recursive schema scans do not confuse
  stale diagnostics with current proof.
- The Cells release benchmark remains unresolved. Diagnostic benchmark speed
  reports were archived from the active report tree; this does not solve
  `TASK-0804A` or create the missing wrapper report.

Next executable task:

1. Refresh the native/playground reports required by
   `audit-machine-readiness`, or record why any must remain blocked:
   - `verify-native-gpu-preview-e2e --example todomvc`
   - the current producer for `todomvc-two-window-content.json`
   - `verify-native-todomvc-reference-parity`
   - `verify-native-todomvc-input-parity`
   - `verify-playground-genericity`
2. Continue recursive schema cleanup until `target/reports/schema.json` is
   fresh or the remaining stale reports are explicitly classified.
3. Then rerun `audit-machine-readiness` and `audit-goal-readiness`.

## 2026-06-23 - Machine Readiness Fresh For Current HEAD

Status: U0 in-progress; U1 in-progress; native/playground readiness support
freshness restored.

Files changed:

- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`
- regenerated ignored reports and artifacts under `target/reports/native-gpu/`,
  `target/artifacts/native-gpu/`, and `target/reports/playground-genericity.json`
- archived stale historical subagent JSON reports from
  `target/reports/bytes-plan/subagents/` to
  `target/diagnostics/bytes-plan-subagent-reports/`

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `target/debug/xtask verify-playground-genericity --report target/reports/playground-genericity.json` | Pass | Refreshed playground genericity report for current HEAD. |
| `target/debug/xtask verify-native-two-window-content --report target/reports/native-gpu/todomvc-two-window-content.json` | Fail, then Pass | First run failed because preview E2E/readback evidence was stale; after refreshing preview E2E, rerun passed. |
| `target/debug/xtask verify-native-todomvc-reference-parity --report target/reports/native-gpu/todomvc-reference-parity.json` | Fail, then Pass | First run failed because preview E2E/readback evidence was stale; after refreshing preview E2E, rerun passed. |
| `target/debug/xtask verify-native-todomvc-input-parity --report target/reports/native-gpu/todomvc-input-parity.json` | Pass | Refreshed input parity report for current HEAD. |
| `target/debug/xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json` | Pass | Compiled native crates, launched app-owned preview/dev E2E probe, wrote fresh passing preview E2E report. Existing native GPU/playground dead-code warnings only. No leftover matching `boon_native_playground` preview/dev processes were found afterward. |
| `target/debug/xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json` | Pass | Machine readiness now passes for current HEAD after refreshing the required native/playground reports. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Machine-readiness blocker is gone; goal readiness still fails on partial BYTES phases, missing Cells release benchmark, stale schema report, and default legacy execution. |
| `target/debug/xtask verify-report-schema` | Fail | Recursive schema scan progressed beyond stale bytecode, BYTES length audit, stale forged diagnostics, stale Cells diagnostic benchmarks, phase1 dump-plan, stale review report, and stale subagent JSON reports. The current next observed stale report is `target/reports/bytes-plan/todomvc-current-state-probe.json`. |

Current machine-readiness summary:

```json
{
  "status": "pass",
  "git_commit": "271d295"
}
```

Current goal-readiness blockers:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `target/reports/schema.json` is stale for current HEAD
- `boon_cli run` still defaults to legacy, so Phase 10 default switch has not
  happened

Current schema blocker:

- `target/reports/bytes-plan/todomvc-current-state-probe.json` has a stale
  `boon_cli` binary hash. It is replayable with:

```bash
target/debug/boon_cli run examples/todomvc.bn \
  --report target/reports/bytes-plan/todomvc-current-state-probe.json
```

Decision notes:

- Native/playground report freshness was a real readiness dependency and is now
  restored for current HEAD.
- The remaining schema blocker is not the BYTES aggregate or machine-readiness
  reports; those are current and passing.
- Recursive schema cleanup is still not complete. Continue iterating stale
  historical reports, refreshing replayable reports and archiving obsolete
  diagnostics outside `target/reports/`.

Next executable task:

1. Refresh `target/reports/bytes-plan/todomvc-current-state-probe.json`.
2. Continue `verify-report-schema` until `target/reports/schema.json` is fresh
   or the remaining stale reports are classified.
3. Rerun `audit-goal-readiness`.
4. Once schema freshness is resolved, proceed to bounded `TASK-0804A`/Cells
   benchmark diagnosis or a reviewed blocker disposition that allows U2 to
   begin without claiming current-goal completion.

## 2026-06-23 - Recursive Schema Freshness Restored

Status: U0 in-progress; U1 in-progress; recursive report-schema freshness is
current for start HEAD `271d295`.

Files changed:

- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`
- regenerated ignored reports:
  - `target/reports/bytes-plan/todomvc-current-state-probe.json`
  - `target/reports/cells-cli-run.json`
  - `target/reports/todomvc-cli-run.json`
  - `target/reports/schema.json`
  - `target/reports/debug/machine-readiness.json`
  - `target/reports/bytes-plan/goal-readiness.json`
- wrote command logs under `target/reports/unified/logs/`

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `target/debug/boon_cli run examples/todomvc.bn --report target/reports/bytes-plan/todomvc-current-state-probe.json` | Pass | Refreshed the TodoMVC current-state probe after schema reported a stale `boon_cli` binary hash. |
| `target/debug/xtask verify-report-schema` | Fail | Progressed past the TodoMVC probe and stopped on stale `target/reports/cells-cli-run.json`. |
| `target/debug/boon_cli run examples/cells.bn --scenario examples/cells.scn --report target/reports/cells-cli-run.json` | Pass | Refreshed the Cells CLI smoke report and recorded the report path in `command_argv`. |
| `target/debug/xtask verify-report-schema` | Fail | Progressed past Cells and stopped on stale `target/reports/todomvc-cli-run.json`. |
| `target/debug/boon_cli run examples/todomvc.bn --scenario examples/todomvc.scn --report target/reports/todomvc-cli-run.json` | Pass | Refreshed the TodoMVC CLI smoke report and recorded the report path in `command_argv`. |
| `target/debug/xtask verify-report-schema` | Pass | Recursive schema scan now passes and refreshed `target/reports/schema.json` for `271d295`. |
| `target/debug/xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json` | Pass | Machine readiness remains passing with fresh native/playground reports. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Schema freshness blocker is gone; remaining blockers are semantic/progress blockers listed below. |

Current evidence summaries:

```json
{
  "schema": {
    "status": "pass",
    "git_commit": "271d295"
  },
  "machine_readiness": {
    "status": "pass",
    "git_commit": "271d295",
    "blockers": null
  },
  "goal_readiness": {
    "status": "fail",
    "git_commit": "271d295"
  }
}
```

Current goal-readiness blockers:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `Phase 10 default switch has not happened; boon_cli run still defaults to legacy`

Decision notes:

- The recursive schema blocker is resolved. Continuing to debug stale artifacts
  would now be lower value than addressing the real U1/U2 blockers.
- The old readiness gate still cannot pass because `TASK-0804A` lacks a release
  benchmark wrapper and the CLI default still points at the legacy engine.
- The unified goal explicitly allows forward progress after bounded diagnosis
  when `TASK-0804A` is plausibly architectural. The next step should either
  create a bounded, honest disposition for `TASK-0804A` or start the U2 hot
  document model work that should make the benchmark failure structurally
  easier to solve later.

Next executable task:

1. Re-read the current `TASK-0804A` notes and Cells benchmark wrapper contract.
2. Decide whether a short, bounded `TASK-0804A` disposition is enough to unblock
   U2 without claiming readiness.
3. If yes, record that disposition and begin U2 transactional document/change
   model implementation work.

## 2026-06-23 - U2 Document Transaction Boundary Slice

Status: U2 in-progress.

Files changed:

- `crates/boon_document/src/lib.rs`
- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`

What changed:

- Added `DocumentChangeBatch` as the first public batch boundary over
  `DocumentPatch` values.
- Added `DocumentChangeSet` as a serialized runtime-output summary containing:
  patch count, per-patch reports, unique targets, merged invalidation facts,
  removed nodes, before/after node counts, and materialization reports.
- Reworked `DocumentState::apply_patch` to call the batch path for one-patch
  compatibility, so the old API remains available while the transaction path is
  centralized.
- Added `DocumentState::apply_batch` with atomic commit behavior:
  - validates the current frame once before the batch;
  - applies patches to a cloned candidate frame;
  - validates the candidate once after the batch;
  - commits the candidate only after every patch and final validation pass.
- Added tests proving:
  - merged dirty facts across structural, text, style, layout, paint, and hit
    invalidation;
  - rollback when a later patch in a batch fails.

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `cargo fmt -p boon_document` | Pass | Applied standard formatting after the document transaction edit. |
| `cargo test -p boon_document document_batch -- --nocapture` | Pass | New focused batch tests passed: `2 passed`. |
| `cargo test -p boon_document -- --nocapture` | Pass | Full `boon_document` test suite passed: `36 passed`. |
| `cargo check -p boon_document -p xtask -p boon_native_playground -p boon_native_app_window` | Pass | Downstream compile passed. Existing `boon_native_gpu` and `boon_native_playground` dead-code warnings only. |
| `git diff --check` | Pass | No whitespace errors after the U2 document transaction edit. |
| `cargo fmt --all -- --check` | Pass | Workspace formatting check passed after the U2 document transaction edit. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Readiness still fails. The U2 code edit makes the BYTES aggregate stale for the dirty worktree fingerprint; the remaining blockers are partial phases, missing Cells release benchmark wrapper, and Phase 10 default legacy execution. |

Evidence classification:

- This is implementation evidence for the first U2 batch boundary.
- It is not evidence that U2 is complete. The runtime still does not feed
  `UiSemanticChange` batches into `DocumentState::apply_batch`, hot numeric
  document IDs are not introduced, style/text/material interning is not
  implemented, and the structural `InsertChild`/`RemoveChild`/`MoveChild`
  patches still are not used by runtime lowering.
- It does not resolve `TASK-0804A`, the Cells release benchmark wrapper, or the
  Phase 10 default switch.
- Because this slice changes Rust code and remains uncommitted, the previously
  refreshed BYTES aggregate is now stale for the dirty worktree fingerprint.
  This is an evidence freshness blocker, not a new semantic failure.

Current goal-readiness blockers after this slice:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `BYTES/MachinePlan aggregate is stale for current worktree fingerprint`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `Phase 10 default switch has not happened; boon_cli run still defaults to legacy`

Next executable task:

1. Continue U2 by connecting runtime semantic deltas to the document batch
   boundary or by adding precise structural child operations if that is the
   smaller dependency-ready slice.
2. Refresh the BYTES aggregate only after the next U2 slice or before a
   checkpoint that needs fresh aggregate evidence.

## 2026-06-23 - U2 Precise Structural Child Patch Slice

Status: U2 in-progress.

Files changed:

- `crates/boon_document_model/src/lib.rs`
- `crates/boon_document/src/lib.rs`
- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`

What changed:

- Added precise structural `DocumentPatch` variants:
  - `InsertChild { parent, child, index }`
  - `RemoveChild { parent, child }`
  - `MoveChild { child, new_parent, index }`
- Implemented `InsertChild` as an in-parent reorder with explicit parent-child
  validation and index bounds.
- Implemented `MoveChild` with old-parent detach, new-parent insert, index
  bounds, root protection, and cycle prevention.
- Implemented `RemoveChild` as parent-checked subtree removal, so callers can
  express child deletion without a broad node upsert or ambiguous detach.
- Added `PatchApplyError::ChildIndexOutOfBounds` so invalid structural patches
  fail closed.
- Kept structural child invalidation precise: structure/list/layout/hit-region
  facts are reported without forcing `FullDocument` for valid child reorder and
  move patches.

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `cargo fmt -p boon_document -p boon_document_model` | Pass | Applied standard formatting after protocol/model edits. |
| `cargo test -p boon_document structural_child -- --nocapture` | Pass | Focused structural child tests passed: `2 passed`. |
| `cargo test -p boon_document -- --nocapture` | Pass | Full `boon_document` suite passed after protocol changes: `38 passed`. |
| `cargo check -p boon_document_model -p boon_document -p xtask -p boon_native_playground -p boon_native_app_window` | Pass | Downstream compile passed. Existing `boon_native_gpu` and `boon_native_playground` dead-code warnings only. |
| `git diff --check` | Pass | No whitespace errors after the structural patch slice. |
| `cargo fmt --all -- --check` | Pass | Workspace formatting check passed after the structural patch slice. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Readiness still fails on partial phases, stale BYTES aggregate for the dirty worktree fingerprint, missing Cells release benchmark wrapper, and Phase 10 default legacy execution. |

Evidence classification:

- This satisfies the first U2 protocol requirement for precise structural child
  changes in the document model.
- This is still not retained layout. Layout currently still consumes full
  `DocumentFrame` input.
- Runtime semantic deltas still are not lowered into `DocumentChangeBatch`.

Current goal-readiness blockers after this slice:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `BYTES/MachinePlan aggregate is stale for current worktree fingerprint`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `Phase 10 default switch has not happened; boon_cli run still defaults to legacy`

Next executable task:

1. Continue U2 with runtime semantic-delta-to-document-batch lowering, or start
   numeric hot document IDs if that turns out to be the smaller dependency-ready
   slice.
2. Refresh BYTES aggregate evidence before claiming any readiness state, because
   the current code edits make the old aggregate stale for the worktree.

## 2026-06-23 - U2 Native Patch Fast Path Uses Document Batch Boundary

Status: U2 in-progress.

Files changed:

- `crates/boon_document/src/lib.rs`
- `crates/boon_native_playground/src/main.rs`
- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`

What changed:

- Added `DocumentState::from_frame` and `DocumentState::into_frame` so retained
  cached document frames can enter and leave the typed transaction API.
- Added native data-binding target lowering from runtime render patches to
  typed document patches:
  - text/label/value/display-value targets lower to `DocumentPatch::SetText`;
  - style targets lower to `DocumentPatch::SetStyle`;
  - `size` keeps the existing `box_size` normalization for button/checkbox/
    stack/table-cell nodes;
  - source-intent targets lower to `DocumentPatch::SetBinding` while preserving
    the existing binding ID and intent.
- Reworked both incremental native document-frame patch paths to collect
  `DocumentPatch` values and apply them through `DocumentState::apply_batch`
  before direct layout patching or fallback layout:
  - paint-space root-delta patch path;
  - general patched-document-frame path.
- Kept the direct layout patch target list for the already-existing layout
  fast path, but the retained `DocumentFrame` mutation now goes through the
  typed transaction boundary instead of direct field mutation.
- Aligned the sparse document patch gate with its existing tests: targetless
  paint/layout patches are allowed when they do not overlap structural data
  reads; targetless structural overlaps still reject.

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `cargo fmt -p boon_document -p boon_document_model -p boon_native_playground` | Pass | Applied formatting after bridge edits. |
| `cargo check -p boon_document_model -p boon_document -p boon_native_playground` | Pass | Initial affected-crate compile passed. Existing native GPU/playground dead-code warnings only. |
| `cargo test -p boon_native_playground data_binding_targets_lower_to_atomic_document_change_batch -- --nocapture` | Pass | New focused typed-lowering test passed. |
| `cargo test -p boon_native_playground sparse_document_patch_gate -- --nocapture` | Fail, then Pass | First run exposed the existing gate/test inconsistency for targetless nonstructural patches. After fixing the gate, all `13` sparse patch gate tests passed. |
| `cargo test -p boon_native_playground direct_layout_patch -- --nocapture` | Pass | Existing direct layout patch tests passed: `2 passed`. |
| `cargo test -p boon_document -- --nocapture` | Pass | Full `boon_document` suite still passed: `38 passed`. |
| `cargo check -p boon_document_model -p boon_document -p xtask -p boon_native_playground -p boon_native_app_window` | Pass | Downstream compile passed. Existing native GPU/playground dead-code warnings only. |
| `cargo fmt --all -- --check` | Pass | Workspace formatting check passed. |
| `git diff --check` | Pass | No whitespace errors. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Readiness still fails on partial phases, stale BYTES aggregate for the dirty worktree fingerprint, missing Cells release benchmark wrapper, and Phase 10 default legacy execution. |

Evidence classification:

- This is the first concrete runtime/native-to-document batch integration
  evidence for U2. The runtime still emits `RenderPatch`/`SemanticDelta`, but
  supported native data-binding target updates now cross into the retained
  document model through `DocumentChangeBatch`.
- This is not yet a full `UiSemanticChange` batch boundary from runtime tick to
  document state. The lowerer is native-side and data-binding-target based.
- Layout is still not retained incremental layout. The existing direct layout
  patch path remains a fast path over `LayoutFrame`, with fallback full layout.
- This does not resolve `TASK-0804A`, the Cells release benchmark wrapper, or
  the Phase 10 default switch.

Current goal-readiness blockers after this slice:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `BYTES/MachinePlan aggregate is stale for current worktree fingerprint`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `Phase 10 default switch has not happened; boon_cli run still defaults to legacy`

Next executable task:

1. Continue U2 with hot numeric document IDs/debug-name tables or move toward a
   runtime-owned `UiSemanticChange` batch type, depending on which can be
   introduced without duplicating the existing `RenderPatch`/`SemanticDelta`
   protocol.
2. Refresh BYTES aggregate evidence before any checkpoint that claims readiness.

## 2026-06-23 - U2 Hot Document Node ID Table Slice

Status: U2 in-progress.

Files changed:

- `crates/boon_document/src/lib.rs`
- `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`

What changed:

- Added `DocumentHotNodeId(u32)` as the first internal numeric document node ID.
- Added `DocumentDebugNameTable` to keep human-readable `DocumentNodeId` names
  available for reports/debugging.
- Added `DocumentHotIdTable` with:
  - deterministic root hot ID `0`;
  - stable sorted assignment for non-root node IDs;
  - forward lookup from `DocumentNodeId` to `DocumentHotNodeId`;
  - reverse debug-name lookup from `DocumentHotNodeId` to `DocumentNodeId`.
- Kept the public serialized `DocumentFrame` protocol unchanged. The hot table
  is a derived internal index, not a wire-format replacement.

Commands run:

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `cargo fmt -p boon_document` | Pass | Applied formatting after the hot-ID table edit. |
| `cargo test -p boon_document document_hot_id_table -- --nocapture` | Pass | Focused hot-ID/debug-name test passed. |
| `cargo test -p boon_document -- --nocapture` | Pass | Full `boon_document` suite passed after hot-ID work: `39 passed`. |
| `cargo check -p boon_document_model -p boon_document -p xtask -p boon_native_playground -p boon_native_app_window` | Pass | Downstream compile passed. Existing native GPU/playground dead-code warnings only. |
| `cargo fmt --all -- --check` | Pass | Workspace formatting check passed. |
| `git diff --check` | Pass | No whitespace errors. |
| `target/debug/xtask audit-goal-readiness --report target/reports/bytes-plan/goal-readiness.json` | Expected Fail | Readiness still fails on partial phases, stale BYTES aggregate for the dirty worktree fingerprint, missing Cells release benchmark wrapper, and Phase 10 default legacy execution. |

Evidence classification:

- This satisfies the first narrow U2 requirement for numeric generational-hot-ID
  groundwork, with a debug-name table for human-readable proof/reporting.
- It is not yet a full hot retained document store. IDs are derived from the
  current `DocumentFrame` and do not yet carry generations or drive retained
  layout/render caches.
- It does not resolve `TASK-0804A`, the Cells release benchmark wrapper, or the
  Phase 10 default switch.

Current goal-readiness blockers after this slice:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `BYTES/MachinePlan aggregate is stale for current worktree fingerprint`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `Phase 10 default switch has not happened; boon_cli run still defaults to legacy`

Next executable task:

1. Continue U2 by adding generation tracking to hot document IDs or by moving
   style/text/material interning into the retained document boundary.
2. Refresh BYTES aggregate evidence before any checkpoint that claims readiness.
