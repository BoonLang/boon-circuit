# Speedup Execution Goal Start Context

This file preserves the start contract for the active BYTES and typed
MachinePlan `/goal` as speedup-roadmap execution context. It is intentionally
separate from the live progress ledger so the future speedup execution goal and
the next plan files can reference a stable baseline without relying on chat
context.

Generated: `2026-06-22T21:54:06+02:00`

## Purpose

Use this file when adding later speedup plan files that build on the current
BYTES and MachinePlan work, especially the future `/goal` that will execute the
combined set of speedup plans. It records what the current goal originally
promised, which evidence counts, what must not be weakened, and which baseline
failures were already known at the start.

Do not treat this as the live status source. Live progress remains in
`docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md`, and executable proof remains
in `target/reports/bytes-plan/`.

## Source Contract

Primary objective:

- Implement the Boon `BYTES` language/runtime feature.
- Implement typed deterministic `MachinePlan` architecture between semantic IR
  and execution.
- Follow `docs/plans/BYTES_AND_MACHINE_PLAN_IMPLEMENTATION.md` as the primary
  contract.
- Preserve `AGENTS.md`, current architecture decisions, runtime/language/LIST
  and delta docs, native GPU contract, report-schema rules, and existing
  verification requirements.
- Do not commit or push unless the user explicitly asks.

Objective and plan hash at the time this file was written:

| File | SHA-256 |
| --- | --- |
| `/home/martinkavik/.codex/attachments/2c841483-c47e-4594-8047-2c564dc04182/pasted-text-1.txt` | `aeeaa336157e948e4681590735739fb68ba1664c35054236165a524e0d513440` |
| `docs/plans/BYTES_AND_MACHINE_PLAN_IMPLEMENTATION.md` | `aeeaa336157e948e4681590735739fb68ba1664c35054236165a524e0d513440` |
| `docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md` | `b14bbfc99c01187fe85e51bc78d09f27e75a1d9ac15aecc41019c352126c2b0b` |

The original goal said to work incrementally from current HEAD, preserve
unrelated user changes, inspect current code before assuming paths or symbols,
and never silently reduce scope or weaken acceptance criteria.

## Goal Start Baseline

Phase 0 recorded the original implementation baseline in the progress ledger.

| Item | Value |
| --- | --- |
| Baseline recorded at | `2026-06-18T14:37:52+02:00` |
| Baseline commit | `0f891e5f5d49508e3f8618d03913741e58215e11` |
| Baseline report | `target/reports/bytes-plan/phase0-baseline.json` |
| Rust toolchain | `rustc 1.96.0 (ac68faa20 2026-05-25)` |
| Cargo toolchain | `cargo 1.96.0 (30a34c682 2026-05-25)` |

Baseline failures were not BYTES/MachinePlan completion failures by
themselves. They were recorded so later work could separate new regressions
from pre-existing issues.

Known baseline failures:

- `cargo test --workspace --no-fail-fast` failed in several crates, including
  `boon_cli`, `boon_ir`, `boon_native_playground`, `boon_parser`, and
  `boon_runtime`.
- `cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn`
  failed on `commit-c0-formula-bar-sum-through-a3`, expecting `["C0"]` and
  getting `[]`.
- `cargo xtask verify-report-schema` failed because an existing headed scenario
  report under `target/reports/` was not an accepted report shape.
- The old `cargo xtask audit-goal-readiness` behavior conflicted with the new
  goal. The active BYTES/MachinePlan readiness preflight added later supersedes
  that historical baseline conflict.
- TodoMVC had a usable linked speed report, but its benchmark wrapper was
  schema-invalid at baseline.
- Cells had no usable release benchmark because the scenario failed before
  measurement completed.

## Primary Outcomes

The goal is not just to add syntax. Completion requires all of these outcomes:

1. A real Boon `BYTES` type with dynamic, inferred-fixed, and explicit-fixed
   forms.
2. Explicit-base byte literals with values in `0..255`.
3. Nested BYTES composition/flattening.
4. Exact fixed-size checking and zero-filled fixed BYTES constructors.
5. Explicit `TEXT` / `BYTES` encoding boundaries.
6. Explicit endian handling for multi-byte numeric reads and writes.
7. A deterministic verified `crates/boon_plan` MachinePlan layer.
8. Replacement of accepted string-path / AST-based execution with typed IDs,
   typed storage, source routes, dirty plans, operation regions, deterministic
   commit rules, and semantic delta plans.
9. A CPU PlanExecutor in dual-path mode until concrete TodoMVC and Cells
   parity is proven.
10. Semantic example refactors for TodoMVC physical assets, Cells formula
    scanning, and focused BYTES fixtures.
11. Honest report-schema and xtask verification, negative fixtures, release
    benchmarks, allocation/copy counters, and independent subagent audits.

## Non-Negotiable Rules

Future plans must preserve these rules:

- No central reducer, runtime graph cloning, per-row graph construction,
  actor/channel-per-value engine, or Differential Dataflow core.
- No app-specific Rust behavior for TodoMVC, Cells, NovyWave, or BYTES
  examples.
- `SOURCE` remains the structural external-input abstraction.
- Hidden keys and generations remain below Boon source and cannot become Boon
  values.
- Renderers consume semantic deltas; renderer-specific patches do not enter
  semantic IR or MachinePlan.
- Native GPU rendering/surface work is separate from future GPU compute and
  must not be falsely completed by BYTES/MachinePlan work.
- Human-readable strings and AST spans may exist in `DebugMap` and reports, but
  executable MachinePlan operands must be typed IDs/refs.
- Accepted plan runs must prove `runtime_ast_eval_count = 0`,
  `executable_string_path_count = 0`, `unknown_plan_op_count = 0`,
  `graph_rebuild_count = 0`, and `graph_clones_per_item = 0`.
- Do not delete the legacy path in the same milestone that introduces
  PlanExecutor.
- Do not weaken budgets or report gates to obtain green output.
- Do not fabricate visible Wayland, manual, GPU, hardware, or operator
  evidence. Mark unavailable checks blocked with exact reasons.
- Fix real parser/typechecker/runtime/engine limitations in the engine instead
  of leaving Boon-level workarounds.

## Phase Boundaries

The original goal defined this phase sequence:

| Phase | Meaning | Completion shape |
| --- | --- | --- |
| 0 | Baseline | Current failures, toolchain, semantic outputs, reports, and release benchmarks recorded without misclassification. |
| 1 | Plan Boundary | `crates/boon_plan`, typed IDs, plan structures, deterministic hashing, verifier, compile-to-plan, and dump-plan exist while legacy remains default. |
| 2 | BYTES Parser | Byte/BYTES AST and syntax parse with exact spans and targeted malformed-input diagnostics. |
| 3 | BYTES Type System | Fixed/dynamic BYTES semantics, builtin signatures, encoding/endian checks, and diagnostics are enforced. |
| 4 | Semantic IR | Byte constants, initial values, typed byte operations, source-payload typing, spans, and constant folding enter semantic IR without executable debug strings. |
| 5 | MachinePlan Lowering | Current Boon semantics lower into typed MachinePlan structures with zero string/AST/fallback counters on supported surfaces. |
| 6 | CPU PlanExecutor and Parity | PlanExecutor runs routing, dirty regions, candidate resolution, commit, and semantic deltas with TodoMVC/Cells parity evidence. |
| 7 | BYTES Runtime/Storage | Fixed and dynamic byte storage, host byte IO, source payloads, byte deltas, no-panic behavior, and warm-tick allocation evidence are proven. |
| 8 | Examples | TodoMVC physical, Cells formula scanning, and focused BYTES examples use BYTES semantically without converting ordinary UI text just for coverage. |
| 9 | Verification and Performance | Native repo gates prove language/runtime, negative fixtures, parity, no-fallback counters, storage, stale/tampered rejection, and reproducible release measurements. |
| 10 | Default Switch | Default execution switches to PlanExecutor only after parity, negative, no-fallback, performance, schema, readiness, and example gates pass. |

## Verification Contract

The goal named these required baseline/final checks at minimum:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --no-fail-fast
cargo run -p boon_cli -- dump-ir examples/todomvc.bn
cargo run -p boon_cli -- dump-ir examples/cells.bn
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn
cargo xtask verify-report-schema
cargo xtask audit-goal-readiness
```

The implementation must also run the BYTES/MachinePlan-specific gates from the
implementation plan, including a fresh aggregate `--check-existing` gate and
negative/tamper behavior.

Performance claims require release measurements against Phase 0 baseline data.
Reports must include p50/p95/p99/max, sample count, warm-up, allocations,
copies, rows/regions touched, compile/lower time, runtime tick time, and
artifact freshness.

## Subagent Contract

The original goal required independent subagents throughout the work, not only
at the end.

Required review roles:

- architecture auditor
- BYTES language auditor
- plan/runtime parity auditor
- performance/storage auditor
- adversarial verification auditor
- example/docs auditor

Reviewer reports belong under `target/reports/bytes-plan/subagents/`.
Critical/high findings must be resolved. Medium findings must be resolved or
explicitly deferred with rationale, owner, and milestone.

The adversarial reviewer must prove aggregate gates fail for stale hashes,
edited success flags, missing reports, unknown/fallback execution, AST/string
execution, unbounded bytes reported as hardware-safe, and fake benchmark
profiles.

## Snapshot When This File Was Added

This is not the goal start baseline. It is the repo snapshot when this
standalone context file was created.

| Item | Value |
| --- | --- |
| Snapshot commit | `6494b54e488dc63cfb39d0e1d50830f1283b80dc` |
| `verify-bytes-machine-plan-all --check-existing` | `pass`, `56/56` reports checked |
| Proof reports | `46` |
| Diagnostic reports | `10` |
| Stale child schemas | `0` |
| Stale child artifact hashes | `0` |
| `audit-goal-readiness` | `fail` |

Current readiness blockers at this snapshot:

- `8 phases are still partial in the progress ledger`
- `1 phases are still not started in the progress ledger`
- `Cells release benchmark wrapper report is missing because TASK-0804A remains blocked by speed budgets`
- `Phase 10 default switch has not happened; boon_cli run still defaults to legacy`

Current worktree was dirty when this context file was added. Do not use this
file as a cleanliness checkpoint.

## How Future Speedup Plans Should Use This

Every later speedup plan file that depends on this goal should state:

- which original phase or phases it advances;
- which acceptance criteria from this file it satisfies or leaves open;
- which report paths and commands will prove the work;
- which fallback counters must remain zero;
- whether the work changes language semantics, API, MachinePlan shape,
  PlanExecutor behavior, report schema, examples, or performance gates;
- whether any existing partial phase can become complete, and why;
- what must be added to `docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md` after
  implementation.

A later plan must not claim completion just because the aggregate report is
green. The final goal remains blocked until the progress ledger has no partial
or not-started phases, Cells release benchmark evidence exists, and Phase 10
has switched the default execution path with the required proof.
