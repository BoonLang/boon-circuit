# Next Compiler /goal — Requirement Aggregation (K1″)

Updated: 2026-09-11. Status: recommended next bounded compiler goal. It
replaces the blocked
[K1′ transfer-cost prompt](BOON_COMPILER_K1_TRANSFER_COST_GOAL_PROMPT.md).
This document does not start, clear, resume or change a live goal by itself.

## Why this goal exists

K1′ ended blocked on scope after five measured attempts: memoization has no
sound tuple key (only 35% of summary calls have closed inputs, and the
requirement half of an identical-input class differs 84 times on TodoMVC), the
residual fallback is 33-212% slower than the summary path, halving interpreted
node visits (domain fold, 1,017,465 to 483,995) changed nothing, a scaffold
cache cut interning by 1% while latency rose, and removing the in-pass
requirement refresh broke the withdrawn-evidence invariant. The full record is
in the 2026-09-11 evidence files listed below.

The tree is currently slower than the preserved K0 producer on every large
fixture (best candidate pass: TodoMVC diagnostics 1198 vs 930 ms, verified
1597 vs 1281; NovyWave diagnostics 904 vs 497, verified 2266 vs 1845). The
measured cause is the requirement-aggregation fold, not the transfer
interpreter: TodoMVC performs 18,792 aggregate evaluations over 722,579 fact
visits from 11,553 changed sites, and term intern requests rise from K0's
342,922 to 1,645,514 (non-empty object interning 213,235 to 1,189,597) with
scratch reuses 1,494,269 to 5,923,303. Every refresh re-merges a destination's
base and all of its contribution facts, and the refresh must happen before the
next read for correctness.

## Start Command

In a fresh thread for this repository, paste:

```text
/goal Execute the requirement-aggregation contract in docs/plans/BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md from current HEAD. Make destination aggregation incremental so a committed site change updates a destination without re-merging unchanged contributors, preserving refresh-before-read, exact withdrawal and A-to-B-to-A equivalence. Follow its scope, evidence, checkpoint and stopping rules. Do not push, do not edit budgets/compiler.toml, and do not start the successor goals (warm retention or linked code) inside this goal.
```

Clear a superseded goal with `/goal clear` first if one is still attached.

## Contract to Read

Read AGENTS.md, this file, the
[architecture plan](BOON_COMPILER_TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN.md),
the [performance plan](BOON_COMPILER_PERFORMANCE_PLAN.md) and
[budgets/compiler.toml](../../budgets/compiler.toml) completely, then the
2026-09-11 evidence set —
[warm baseline](evidence/compiler-k1p-warm-baseline-2026-09-11.json),
[reuse ceiling](evidence/compiler-k1p-reuse-ceiling-2026-09-11.json),
[planner cost](evidence/compiler-k1p-planner-cost-2026-09-11.json),
[domain fold and rejections](evidence/compiler-k1p-domain-fold-2026-09-11.json)
— and the 2026-09-11 sections of the
[K0+K1 execution log](BOON_COMPILER_K0_K1_EXECUTION_2026_09_07.md).
`budgets/compiler.toml` remains the sole executable budget and oracle
authority.

## Outcome

Two honest endings:

1. **Implemented:** the component solver maintains each requirement
   destination's aggregate incrementally. Committing a site change applies only
   that change; unchanged contributors are not re-merged. Withdrawal, replacement
   and alias reordering keep exact semantics, refresh-before-read still holds,
   and cold TodoMVC/NovyWave return to K0-class latency with byte-identical
   diagnostics and a large fall in object interning and scratch churn.
2. **Reviewed rejection:** source-level evidence that incremental aggregation
   cannot be made sound or beneficial under the kernel's constraint model,
   with a precise next architectural decision and the same evidence discipline
   K1′ used. A rejection must not be claimed from one failed prototype.

## Allowed Work

- The requirement contribution table and the component solver's aggregation
  path: per-site deltas, exact removal, ordering receipts, alias handling and
  dependency updates that stop re-merging unchanged contributors.
- The refresh-before-read contract: it must be preserved. Do not batch it away
  (measured: `projection_does_not_reimport_a_withdrawn_requirement_as_base`
  fails), weaken it, or add a second solver.
- Generic tests, counters and measurement support needed to prove aggregation
  soundness and cost, including the existing `KernelRequirementWork` block.
- Concise updates to this plan's checkpoint/decision text and the evidence
  record.

## Correctness Constraints

- `projection_does_not_reimport_a_withdrawn_requirement_as_base` and the other
  contribution/withdrawal tests stay enabled and passing; do not weaken, skip,
  delete or rewrite them.
- A to B to A replacement must equal a fresh A solve; another occurrence's
  still-active contribution must survive withdrawal; alias ordering must stay
  deterministic.
- The two `whole_selector` regressions, the tagged-payload regressions and
  NovyWave verified stay green.
- No second solver, no result-type-equality cache, no fixture-name shortcut,
  no budget, schema or gate change.

## Measurement and Verification

- One Cargo process at a time with `--jobs 2`; build fresh release
  `boon_cli` and `boon_cli_evidence`; invoke prebuilt binaries for repeated
  observations.
- Interleaved A/B/A 3 setup + 30 scored observations per fixture and intent in
  both cold modes, product and evidence lanes, against the preserved K0
  producer; store the raw report JSON (the K1′ reviewer flagged its absence).
- Record latency, RSS, allocations and every kernel work counter, especially
  aggregate evaluations, fact visits, changed sites, staged writes, object and
  term intern requests, and scratch reuses.
- Run the warm interaction collector on each checkpoint and record it; warm is
  the product metric even though its fix is the successor goal.
- Keep budgets untouched; inherited red limits are reported, never weakened.
- At least one independent fresh-context read-only review must audit the final
  decision before acceptance.

## Checkpoint and Stop

Commit coherent local milestones with exact staging only after focused gates
pass; keep diagnostic evidence separate from acceptance reports. Do not push.
Stop at one of the two endings and hand off the checkpoint hash, measurements,
remaining failures and the successor goal. Do not start the successor goals
inside this goal.

## Successor Goals — Implement Next, Do Not Start Here

These are declared so they are not lost. Each needs its own `/goal` selection
after this goal ends.

### Goal B — Warm Revision Retention (K3/K4-lite)

The product-critical cut. Warm TodoMVC edit-to-ready is 1189 ms p50 / 1213 ms
p95 against a 16.7 ms budget and verified preview is 2802/2891 ms against
100 ms, because every edit performs the producer's full cold solve; there is no
cross-edit reuse at all. Scope: retain the kernel's packed solved revision
across edits, solve only demanded definitions (sparse demand already exists),
exact dirty cones with backdating, lazy rich projections and last-good
publication. First gates: warm diagnostics p95 at or below 16.7 ms and preview
p95 at or below 100 ms with the interactions collector at 3+30, with no cold
regression and no stale publication.

Start command:

```text
/goal Execute the warm-revision-retention contract in docs/plans/BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md from current HEAD (Goal B section). Retain the kernel's packed solved revision across edits, solve only demanded definitions, and recompute exact dirty cones with backdating and last-good publication. Follow the scope, evidence, checkpoint and stopping rules of that section. Do not push, do not edit budgets/compiler.toml, and do not start linked code.
```

### Goal C — Conditional Linked Transfer Code (K2-lite)

Open this only if, after Goals A and B, fresh phase profiles still show transfer
evaluation dominating cold first-solve cost. The original premise for linked
code was measured false at the dispatch level: halving interpreted node visits
changed latency by roughly nothing, so the expensive work is the semantics
inside nodes, which linked operations would still execute. Scope if opened:
compile the transfer to scheduled operations, bind occurrence frames, and
delete the interpreter for the converted construct. If the profiles instead
show the aggregation and warm cuts closed the gap, record linked code as
unnecessary and close it with that evidence.

Start command (only when the condition above holds):

```text
/goal Execute the conditional linked-transfer-code contract in docs/plans/BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md from current HEAD (Goal C section). Compile the definition transfer to scheduled operations with per-occurrence frames and delete the interpreted walk for the converted construct. Open this goal only if fresh phase profiles still show transfer evaluation dominating cold first-solve cost. Follow its scope, evidence, checkpoint and stopping rules. Do not push and do not edit budgets/compiler.toml.
```

## Non-Goals

- Re-running memoization, the residual fallback, scaffold caching or refresh
  batching; all were measured and rejected in K1′.
- Changing language semantics, budgets, schemas or gates to make a candidate
  pass.
- Starting warm retention or linked code inside this goal.
