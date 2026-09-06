# Next Compiler /goal — K0 + K1

Updated: 2026-09-06. Status: recommended next bounded compiler goal.
This replaces the former all-M0--M6 goal and the older unified-product prompt.
It does not start, clear, resume or change a live goal by itself.

## Start Command

In a fresh thread for this repository, paste:

```text
/goal Execute the K0+K1 contract in docs/plans/BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md from the current HEAD. Deliver the reviewed kernel-transfer decision and a verified production speedup if the hypothesis holds. Follow its scope, evidence, checkpoint and stopping rules. Do not push or continue into later cuts.
```

If reusing a thread that still holds the superseded goal, clear that objective
with `/goal clear` first; do not resume its old attachment. Starting a goal is a
user action, not part of the documentation checkpoint. The bounded outcome and
explicit verification/stop rules follow the
[official OpenAI Goals guidance](https://developers.openai.com/cookbook/examples/codex/using_goals_in_codex).

## K0+K1 Contract

Read AGENTS.md, this file and
[the active architecture plan](BOON_COMPILER_TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN.md)
completely. Read the
[dated evidence record](BOON_COMPILER_REASSESSMENT_2026_09_06.md), and the
performance plan's authority, measurement/budget, harness and completion-scope
sections. `budgets/compiler.toml` owns executable budgets and oracles. Other
dated plans are historical reasoning, not a request to restart their sequences.

Resolve the actual branch, HEAD and worktree. Preserve all valid later work;
`37a576b6` is the audit anchor, not an instruction to reset. Identify stale
producers/reports before claiming a baseline. Do not edit this objective or
its exit criteria during the run merely to make a candidate pass.

### Outcome

Resolve the largest measured kernel type-transfer amplification and land one
coherent generic production cut when justified. The investigation starts from
TodoMVC's 10,939 stored residual operations expanding through 8,194 frames into
282,393 logical operations and 620,555 activations. Physical module storage is
already shared; duplicating that achievement is not the objective.

K0 must map dominant variants to source definitions, rejection reasons, actual
variable/mode identities, equivalent semantic input tuples and dependency
epochs. K1 must separate reusable parametric type/requirement transfer from
per-occurrence state/resource/capture identity. Counter/NovyWave and independent
stateful/invalid-source workloads must prevent a fixture-specific optimization.

### Allowed Work

- Necessary source/counter/producer-identity fixes and a fresh product/evidence
  baseline for K0. Do not spend the tranche implementing all future closure
  tooling, native product work or configuration tournaments.
- Kernel transfer construction/evaluation, exact dependency tracking, packed
  model/facade changes genuinely required by that ownership cut, generic tests
  and measurement support.
- A known error-presentation bug directly intersecting the changed owner,
  with a reproduction and regression test; no unrelated bug-fix campaign.
- Deletion of the superseded per-invocation machinery for the converted
  construct after semantic parity. Generic residual equations remain for
  semantically distinct cases within the one kernel, not a legacy fallback.
- Concise updates to the plan's current checkpoint/decision and evidence.

Do not start K2--K5, full incremental compilation, authored WHERE, unrelated
runtime/native renderer/input/compositor work, console, Wasm, hardware, game or
portfolio plans. Do not rewrite the kernel, change language semantics or split
crates merely to make the work look greenfield. Do not remove existing changes
belonging to the user.

### Correctness Constraints

Preserve complete diagnostics and source locations; declaration/expression
flows; lexical binding, calls, effects, captures, state, lists and sources;
exact object field order; stable external/persistence identities and scoped
TypeRefs; dependency/currentness meaning; deterministic output and runtime
behavior. NoElement remains a library convention.

Reuse is never justified by current result-type equality alone. Test late
providers, backflow, generic HOLD with distinct state instances, capture
requirements, alpha scoping, whole/nested/empty/disappearing projections,
PASSED, pattern reads, recursive calls and SCCs, long WHEN chains,
heterogeneous collections and ABI calls to the extent the cut affects them.
Record genuine exclusions, not silently missing tests.

Keep the kernel dependency firewall. No old checker/semantic/IR/plan/compiler
production dependencies are allowed inside the kernel. No source-shaped replay,
second solver, persistent legacy mode, relaxed proof or fixture-name shortcut
may substitute for the cut. The current WHERE stage is bootstrap verification
with rejected source syntax; report that honestly and retain fail-closed behavior.

### Measurement and Verification

Use one Cargo process at a time with --jobs 2. Only the main agent runs Cargo,
producers or collectors. Build fresh release `boon_cli` and
`boon_cli_evidence`; invoke prebuilt binaries for repeated observations. Use
focused tests and small directional samples during edits, then the plan's
preflight ladder. Include occasional labeled debug NovyWave measurements
without treating Rust build time as Boon compilation time.

Keep the uninstrumented product separate from allocation/work evidence. Count
residual operations, activations, summary-node evaluations, key/dependency work,
materializations, allocations/bytes and RSS; work moved into a summary does not
disappear. Freeze the comparison cohort and target counters before changing the
algorithm. A targeted work reduction without end-to-end benefit is not yet a
production speedup.

The 25% targeted-work reduction in K1 is a planning hypothesis, not a benchmark
claim. Use repeated alternating baseline/candidate observations to distinguish
a product gain from noise. For the accepted decision checkpoint, use three
setup plus 30 scored observations per required fixture/intent in both cold
modes and preserve source, binary, profile and allocator identity. The existing
collector can remain red for inherited absolute limits; report them rather
than weakening the manifest or pretending to close the full compiler program.
No previously passing gate or semantic oracle may regress. Explain any apparent
small regression with repeatable evidence; do not bury it in aggregate scores.

Ordinary same-format output remains deterministic and matches accepted semantic
oracles. An intentional format change requires explicit version/contract/
migration proof; do not auto-bless a new hash from the candidate being scored.

Use independent read-only subagents for bounded source/reuse-soundness and
measurement/adversarial reviews where useful. Do not ask them to run heavy
commands. The main agent must read controlling contracts itself. At least one
independent fresh-context review must audit the final K1 decision, correctness,
work ownership and measurement before accepting the checkpoint. This does not
substitute for the later three-review full-program closure.

### Checkpoint and Stop

Commit coherent local milestones with exact staging only after focused
correctness gates pass. Keep intermediate diagnostic evidence separate from
acceptance reports. Record measured results, inherited red gates, deleted
owners, unresolved risks and next action. Do not push.

The goal has two explicitly different completion outcomes:

1. **Implemented:** a generic production transfer cut removes the targeted
   repeated work, improves end-to-end latency outside measured noise, passes
   relevant correctness/determinism/scaling and holdout checks, has an
   independent passing review and is committed with reproducible evidence.
2. **Hypothesis rejected:** source-level attribution and counterexamples or a
   measured generic prototype demonstrate why the proposed reuse cannot
   produce a sound worthwhile cut under these constraints. Review that result,
   remove only this run's disposable experimental code, and commit the evidence
   and a precise next architectural decision. Label this research outcome
   explicitly; it is NOT an implemented speedup or full performance success.

Documentation alone, counters alone, one favorable sample, one failed
experiment, inconvenience or budget exhaustion satisfies neither outcome. Do
not enter an endless sequence of small map/string changes when the hypothesis
misses. Reassess the owning work; if completion needs new scope, authority or
external input, report the exact evidence and request it. Follow the product's
goal-status rules rather than marking unfinished implementation complete.

After either evidenced outcome, stop. K2 and later work need a new explicitly
selected goal. Hand off the checkpoint hash, measurements, remaining failures
and recommended next cut; do not automatically expand this goal to the entire
roadmap or the speculative tens-of-milliseconds envelopes.
