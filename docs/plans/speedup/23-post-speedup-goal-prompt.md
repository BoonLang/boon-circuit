# `/goal` — Continue the half-migrated Boon Circuit architecture safely

Implement the roadmap in `docs/plans/speedup/22-post-speedup-compiler-codegen-wasm-plan.md` **systematically, honestly, and incrementally** against the current `BoonLang/boon-circuit` repository.

## Current-state rule

Before changing code:

1. Read `AGENTS.md` and all relevant nested agent instructions.
2. Read:
   - `docs/plans/speedup/21-speedup-execution-goal-start-context.md`
   - `docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md`
   - `docs/plans/speedup/20-task-0804-root-flush-resolution-plan.md`
   - the current architecture documents named by the plan.
3. Record current commit, dirty-tree state, toolchains, and hashes.
4. Run the current BYTES/MachinePlan aggregate and readiness commands.
5. Treat the live progress ledger and current executable reports as truth. The start-context document is a baseline, not live completion status.
6. Do not claim the previous goal is complete while readiness still reports blockers.

## Scope rule

Work on **one milestone at a time**. The first milestone is current-goal reconciliation/closure, followed by compiler/runtime boundary extraction. Do not jump directly to production Rust/Zig/Wasm codegen from current AST-shaped or string-shaped tables.

The permanent architecture is:

```text
source/query DB
  → resilient frontend
  → TypedSemanticProgram
  → EquationGraph
  ├─ MachinePlan v2 → PlanExecutor/software tiles/hardware later
  └─ NativeRegionIR → Rust/Zig/direct Wasm
```

## Non-negotiable invariants

- no central reducer;
- no runtime graph cloning;
- no per-row graph instance;
- `SOURCE` remains structural and typed;
- hidden list key/generation remains below user code;
- semantic deltas remain canonical;
- no production execution from AST/debug tables/string paths;
- no silent backend fallback;
- no one-task-per-equation design;
- no global barrier for a provably local source event;
- no GPU/window/browser dependencies in the host-neutral runtime core;
- do not delete legacy in the same slice that first introduces its replacement;
- preserve and extend stale/tamper/adversarial verification.

## Required implementation behavior

For each milestone:

1. Inspect current code and reports before proposing edits.
2. Create/update `docs/plans/speedup/24-post-speedup-compiler-progress.md` with:
   - status;
   - files changed;
   - decisions/ADRs;
   - commands and exact outcomes;
   - proof vs diagnostic reports;
   - open blockers;
   - adaptations from the roadmap.
3. Use compatibility adapters instead of mass rewrites.
4. Keep schemas/versioned artifacts deterministic.
5. Add verifiers before or with new executable representations.
6. Regenerate reports whose schema or producer binary changed.
7. Never convert an expected failure into a pass by removing the assertion or weakening the threshold without an explicit reviewed ADR.

## Independent subagents

Use independent subagents for at least:

- migration/dependency architecture;
- semantic parity;
- plan/IR adversarial verification;
- performance methodology;
- report stale/tamper verification;
- security/sandboxing when code generation begins.

When Rust, Zig, and Wasm work begins, additionally use separate backend reviewers. Implementing agents do not self-certify final readiness.

Subagents must write structured findings under an appropriate report directory and identify:

```text
scope reviewed
commit/hash reviewed
commands run
findings
false-positive risks
unverified assumptions
recommendation: pass/block/diagnostic only
```

## Immediate execution sequence

### Step 0 — current goal closure

- Re-run the current aggregate and readiness reports.
- Finish or explicitly ADR-supercede outstanding BYTES/PlanExecutor/default-switch requirements.
- Resolve or truthfully retain the Cells benchmark and TASK-0804 blockers.
- Switch default execution only when the current readiness gate passes.
- Keep explicit legacy comparison during a soak period.

### Step 1 — compiler boundary

- Add `boon_compiler`.
- Move semantic-to-plan orchestration out of `boon_plan`.
- Make `boon_plan` converge on schema/verifier/serialization only.
- Extract a parser/typechecker-independent PlanExecutor core.
- Add a dependency-direction verification command/report.
- Preserve plan hashes/parity where format is unchanged; version honestly where it changes.

### Step 2 onward

Follow the detailed plan in order:

```text
semantic IR split
MachinePlan v2
query compilation/cache
NativeRegionIR
optimizer
generated Rust
generated Zig
direct Wasm
artifact v2
playground restart sessions
browser compiler
performance promotion
parallel/GPU/tile continuation
```

## Backend gate

A backend is accepted only when it:

- compiles the selected accepted subset;
- runs without parser/typechecker/legacy runtime loaded;
- reports zero silent fallback;
- matches ordered state/delta/source-binding/effect/error behavior;
- passes adversarial artifact/ABI tests;
- emits reproducible reports with source/semantic/IR/artifact hashes;
- discloses unsupported constructs;
- meets its stated performance gate without unfair comparison.

## Playground gate

Do not add a visible Rust/Zig/Wasm selector until that backend really builds and runs. When selected and Run succeeds:

- terminate/restart the previous generated preview session;
- begin with fresh state;
- use a new `RunId` and Ready handshake;
- discard stale messages;
- leave the old preview visibly stale rather than launching broken code when compilation fails;
- leave no orphan processes/workers after repeated runs.

## Honesty rule

Partial progress is valid. Record it precisely. A passing focused test is not whole-phase completion. A diagnostic report is not proof. Existing unrelated failures must be classified, not hidden. Performance claims must include algorithm, toolchain, flags, samples, percentiles, startup/compile costs, allocations, scans, and data movement where relevant.
