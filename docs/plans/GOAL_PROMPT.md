# Goal Entry Point

Updated: 2026-09-27.

## Active execution plan

[BOON Compiler Performance Plan — Internal](BOON_COMPILER_PERFORMANCE_PLAN_INTERNAL.md)
is the active compiler execution plan. It is engineering-owned and changes no
language surface, no user-facing behavior and no semantics; the only observable
difference is latency. Read it completely before starting compiler work. Its
ordering is: attribute the unmeasured remainder (M0), delete duplicated passes
(M1), measure before shape-keyed specialization (M3), retain the solved revision
(M4), then make the semantic phase incremental (M5), repair the measurement
harness (M6) and the developer loop (M7). **M2 is retired as unnecessary.**

M0 is complete, M1's term-arena items are partly resolved by rejection, and M2 is
retired. Four measured results now govern the ordering:

1. The typecheck remainder splits into a kernel-**compile** half and a
   kernel-**solve** half whose ratio inverts by fixture (TodoMVC 583/474 ms,
   NovyWave 136/316 ms). Per-call-site work barely touches NovyWave.
2. A per-definition solved-state probe shows **100% of definitions publishing
   byte-identical interface state** across both a type-preserving edit and a real
   type change on both fixtures. The 883 ms warm figure is a missing cache over a
   near-zero delta, not conservative invalidation.
3. The canonical set operations that three verified quadratic claims pointed at
   are unreachable at this scale (unions max 46 members, widened records 43
   fields) and restructuring them measured +1.4% to +3.7% slower.
4. **The compiler has two typecheckers and only one runs.** All 18
   `context_scheme_*`, `wrapper_scheme_*` and `checked_flow_*` counters read 0
   because `boon_typecheck`'s inference engine has no production caller and the
   kernel passes `TypeCheckWorkCounters::default()`. This retires M2, withdraws
   every user-facing proposal that was justified by those counters (declaring
   context formals, lexical-only `PASSED`, `WHERE DEPENDS ON`, declared exports),
   and refutes the external review that motivated them.

**Two rules have emerged from this and are binding on the rest of the plan.**

*Asymptotic argument is not a prioritization method here.* Six candidates have
been rejected by measurement. Measure the size or count of the thing before
deciding to restructure it.

*Verify an external claim against this source before acting on it.* Two of the
three central claims in the research review were refuted by reading the code, and
the third by a counter that read zero. A proposal justified by a
`boon_typecheck` counter is describing the test path until it shows a non-zero
production reading.

It supersedes the *sequencing* of the requirement-aggregation goal's Goal B and
Goal C sections while leaving that document's measurement discipline, evidence
format and correctness constraints in force. Goal C (conditional linked transfer
code) is closed as unnecessary-and-harmful: routing call sites through the
pre-compiled residual-module path measures 52.7% slower on TodoMVC and 433%
slower on NovyWave with byte-identical diagnostics.

## Two questions are reserved for the repository owner

1. Whether to re-establish the stale `machine_plan_sha256` oracles in
   `budgets/compiler.toml` for `todo-mvc-physical` and `novywave`, which do not
   match what HEAD produces. Unmodified HEAD reproduces the same hashes, so this
   predates the active plan. Until it is settled, a plan-hash gate must compare
   against the pre-change HEAD hash rather than the budget file.
2. Anything that would change what a Boon program looks like. Nothing in the
   active plan does; such an item stops and escalates rather than being
   reclassified.

## Historical

The previous active compiler execution prompt was
[Next Compiler /goal — Requirement Aggregation](BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md),
which made requirement aggregation incremental. Its mechanism landed and is
measured at 142 ms (TodoMVC) / 87 ms (NovyWave), i.e. 11-13% of typecheck; the
remainder is addressed by the active plan above rather than by that goal's
successors.

The previous

The previous
[K0 + K1 prompt](BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md) and the
[K1′ transfer-cost prompt](BOON_COMPILER_K1_TRANSFER_COST_GOAL_PROMPT.md) are
closed: K1 was measured as a regression and rejected as implemented, and K1′
ended blocked on scope after five measured rejections. The decisions, report
hashes and attribution are in the 2026-09-11 evidence files. Do not re-run
either objective.

Declared successors, to be selected as their own goals after the aggregation
cut (their contracts live in the active prompt): **Goal B, warm revision
retention** — the product-critical 1189 ms to 16.7 ms warm gap — and **Goal C,
conditional linked transfer code**, which opens only if fresh profiles still
show transfer evaluation dominating cold first-solve cost.

The active roadmap is
[Definition-Owned Work and Responsive Revisions](BOON_COMPILER_TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN.md).
The [2026-09-06 evidence record](BOON_COMPILER_REASSESSMENT_2026_09_06.md)
explains the changed order, actual bottlenecks and missing features.

The old unified-product prompt is retired. Its complete text remains in git
at `37a576b6:docs/plans/GOAL_PROMPT.md`; it must not be pasted, resumed from an
attachment or interpreted as current authorization. The earlier all-M0--M6
compiler prompt is likewise preserved in that revision's compiler goal file.
`steps.md` retains historical portfolio sequencing, not a second active goal.

Editing these files does not clear or replace a live goal. Start the selected
new objective explicitly; do not resume a stale paused or usage-limited goal.
Local checkpoint commits are permitted only by the selected user's request or
goal. Pushing always needs explicit authorization.
