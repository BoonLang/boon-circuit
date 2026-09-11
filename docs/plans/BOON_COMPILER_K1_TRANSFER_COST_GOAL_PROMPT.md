# Next Compiler /goal — K1′ Transfer Cost

Updated: 2026-09-11. Status: recommended next bounded compiler goal. It
replaces the completed
[K0+K1 prompt](BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md), which now
records a reviewed rejection rather than an active objective. This document
does not start, clear, resume or change a live goal by itself.

## Why this goal exists

K1 was executed and measured end to end. The transfer mechanism removes the
targeted replay (TodoMVC linked operations 282,393 to 82,451, activations
620,555 to 181,056), but the replacement re-walks shared summary programs on
every invocation: TodoMVC summary node evaluations rose 32,365 to 1,017,465
(+3044%), term intern requests 342,922 to 1,645,514 (+380%) and summary
definition nodes 236 to 2,325 (+885%). End-to-end latency regressed on every
large fixture and cold mode even in the faster candidate pass (TodoMVC
diagnostics +28.9%, NovyWave diagnostics +81.7%, verified +22.7%). K1 as
implemented is rejected; the reuse idea is not disproven.

The measured decision, report hashes and full attribution are in
[the 2026-09-11 evidence](evidence/compiler-k1-decision-2026-09-11.json). The
same contracts show the headroom: TodoMVC diagnostics is 908.7 ms of
typecheck at K0 out of 929.8 ms total, and NovyWave verified adds 785.0 ms of
semantic and 219.2 ms of backend on top of 682.1 ms of typecheck. Warm
interaction latency, the actual IDE product metric, is still unmeasured.

## Start Command

In a fresh thread for this repository, paste:

```text
/goal Execute the K1′ transfer-cost contract in docs/plans/BOON_COMPILER_K1_TRANSFER_COST_GOAL_PROMPT.md from current HEAD. First measure the warm interaction path as the product baseline, then make definition-transfer evaluation cost-proportional (memoized distinct input tuple + dependency epoch and/or linked compatible code variants), or fall back to linked residual evaluation with shared bytes. Follow its scope, evidence, checkpoint and stopping rules. Do not push, do not edit budgets/compiler.toml, and do not start K2-K5.
```

If a superseded goal is still attached, clear it with `/goal clear` first.
Starting a goal is a user action, not part of the documentation checkpoint.

## Contract to Read

Read AGENTS.md, this file, the
[architecture plan](BOON_COMPILER_TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN.md),
the [performance plan](BOON_COMPILER_PERFORMANCE_PLAN.md) and
[budgets/compiler.toml](../../budgets/compiler.toml) completely, then the
[2026-09-11 decision](evidence/compiler-k1-decision-2026-09-11.json) and the
2026-09-10/11 sections of the
[K0+K1 execution log](BOON_COMPILER_K0_K1_EXECUTION_2026_09_07.md). Resolve
the actual branch, HEAD and worktree first; preserve every valid checkpoint.
`budgets/compiler.toml` remains the sole executable budget and oracle
authority.

## Outcome

Make definition-transfer evaluation cost-proportional, or remove it from the
hot path. Two honest endings:

1. **Implemented:** a generic cut evaluates a definition transfer once per
   distinct quiescent typed input tuple plus dependency epoch (and/or executes
   linked compatible code variants bound to per-occurrence frames) so summary
   evaluation grows with distinct inputs rather than invocations, with exact
   invalidation and no result-type-equality key. It passes the correctness
   gates below, removes the measured regression against the fresh K0
   producer, and reduces targeted summary work toward the distinct-tuple
   count.
2. **Fallback:** a reviewed decision that no sound tuple key exists for the
   measured cohort; then evaluate through linked residual specialization with
   shared physical bytes, restoring K0-class cold latency and keeping the
   whole-selector and requirement correctness fixes. Label this explicitly as
   a fallback decision, not an implemented optimization.

Neither ending requires the cold tens-of-milliseconds envelope; that remains a
later program. A targeted work win without an end-to-end benefit is not an
implemented speedup.

## Step 0 — Warm Product Baseline

Before changing the evaluator, measure the IDE-shaped product path:

```bash
cargo xtask verify-compiler-interactions --setup-samples 3 --scored-samples 30 \
  --report target/reports/compiler-performance/k1p-warm-candidate.json
```

and the same command in the preserved K0 worktree
(`/home/martinkavik/repos/boon-compiler-k0-2d7a5343`) for the baseline. Record
warm diagnostics, preview and switch p50/p95 next to the cold numbers; the
warm budget owns its own thresholds and remains required. Missing
native-presentation or in-flight-supersession evidence is reported, not
invented. If warm interactions already sit far above the manifest thresholds,
say so before optimizing cold paths.

## Allowed Work

- Exact-key transfer memoization: canonical quiescent input tuple, dependency
  epoch, mode and capture/state identity as required for soundness, with
  exact invalidation and backdating when a reused result is unchanged.
- Linked compatible code variants and occurrence frames inside the kernel, so
  evaluation is scheduled work rather than an interpreted per-call walk.
- Deletion of the interpreted summary walk from the hot path when the fallback
  ending is chosen.
- Generic tests, counters and measurement support needed to prove reuse
  soundness and cost, including the existing `KernelRequirementWork` block.
- Concise updates to this plan's checkpoint/decision text and the evidence
  record.

## Correctness Constraints

- The two `whole_selector` regressions and the tagged-payload regressions stay
  enabled and passing; do not weaken, skip, delete or rewrite them.
- NovyWave verified must keep compiling; no previously passing gate or
  semantic oracle may regress.
- No second solver, no result-type-equality reuse key, no persistent legacy
  mode, no relaxed proof, no fixture-name shortcut.
- Final result-type equality is not a reuse key; open roots, late providers,
  backflow, alpha scope, captures and state identity must remain explicit.
  Inactive arms contribute nothing, and A to B to A replacement must equal a
  fresh A solve.

## Measurement and Verification

- One Cargo process at a time with `--jobs 2`; build fresh release
  `boon_cli` and `boon_cli_evidence`; invoke prebuilt binaries for repeated
  observations.
- Interleaved A/B/A 3 setup + 30 scored observations per fixture and intent in
  both cold modes, product and evidence lanes, exactly as the manifest
  protocol requires. The previous run's baseline-then-candidate twice ordering
  is not sufficient for a new claim.
- Report latency, RSS, allocations and work counters per fixture; include
  summary node evaluations, summary call activations, term interning and the
  `KernelRequirementWork` block.
- Keep the untouched budgets; inherited red limits are reported, never
  weakened or re-blessed. Do not auto-bless a new machine-plan hash.
- At least one independent fresh-context read-only review must audit the final
  decision, correctness, work ownership and measurement before acceptance.

## Checkpoint and Stop

Commit coherent local milestones with exact staging only after focused
correctness gates pass; keep diagnostic evidence separate from acceptance
reports; record measured results, inherited failures, deleted owners and the
next action. Do not push. Do not start K2—K5, configuration/allocator
tournaments, native renderer, console, Wasm or hardware work. After either
ending is evidenced and reviewed, stop and hand off the checkpoint hash,
measurements, remaining failures and recommended next cut.

## Non-Goals

- Re-running the same K1 hypothesis without new evidence.
- Tuning the interpreted evaluator with small local changes after the same
  blocker repeats.
- Changing language semantics, budgets, schemas or gates to make a candidate
  pass.
- Promising the cold 40—85 ms envelope from this goal.
