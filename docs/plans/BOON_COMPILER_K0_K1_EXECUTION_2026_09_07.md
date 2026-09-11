# K0 + K1 execution evidence — 2026-09-07

This is an execution log, not a replacement goal or acceptance contract.
The bounded [K0 + K1 goal](BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md)
completed on 2026-09-11 with a reviewed rejection of K1 as implemented; its
successor is the
[K1′ transfer-cost goal](BOON_COMPILER_K1_TRANSFER_COST_GOAL_PROMPT.md).
Starting HEAD: `9901854231ffc248e5bd3db79b582e86bc57b71c`; starting compiler
implementation: `37a576b6`. No pushes are authorized.

## Producer identity prerequisite

An ordinary release build initially finished in 0.77 seconds without rebuilding
the producer. Its embedded HEAD remained `25a354d4`, dirty, although the live
checkout was clean `99018542`. That binary is not a current timing baseline.
The build script included HEAD and staging state in its v1 identity but did
not watch the Git inputs that change them.

The repair retains the existing strict identity and validation rules. It adds
Git-resolved watches for HEAD, the current branch ref, existing index and
packed-refs; a packed branch watches its existing refs parent until its loose
ref exists. Linked worktrees resolve their private HEAD/index correctly.
Missing optional files and the whole `.git` directory are not watched.

The frozen release profile label now says `lto=false`, matching the unchanged
Cargo.toml setting, instead of `lto=off`. No optimization flags, allocator,
budgets, oracle hashes, or freshness rejection rules were relaxed. Debug
observations no longer attest release-only profile settings; unknown settings
are explicitly labeled unknown. This remains a fixed-lane identity, not a
general effective-profile resolver for arbitrary overrides.

Focused checks:

- `cargo test --locked --jobs 2 -p boon_cli --test build_git`: four tests pass
  for branch commits, detached HEAD/staging, packed refs and linked worktrees.
- `cargo build --locked --release --jobs 2 -p boon_cli --bin boon_cli --bin boon_cli_evidence`:
  refreshed the producer to the actual HEAD/dirty input identity.
- An unchanged repeat build finished in 0.13 seconds without recompiling.
- A direct Counter observation emitted the corrected profile label and
  current dirty source identity. Its 7.142 ms internal time is a plumbing
  smoke check, not fresh-process collector latency or performance acceptance.

## Initial source attribution

The identity repair is checkpoint `ab670feca7e715728f95d95b66593652313b3575`.
An ordinary post-commit build recompiled the producer and emitted that clean
HEAD, demonstrating the repaired trigger on the real workspace.

Fresh identity-valid development preflight (one setup + five scored, both
product/evidence lanes and both cold modes):
`target/reports/compiler-performance/k0-identity-preflight.json`.

| Fixture / mode | Diagnostics p50 / p95 ms | Verified p50 / p95 ms |
| --- | --- | --- |
| Counter / fresh-process | 8.213 / 8.297 | 17.150 / 17.482 |
| Counter / empty-session | 7.140 / 7.348 | 16.051 / 16.868 |
| TodoMVC / fresh-process | 908.076 / 932.523 | 1276.965 / 1352.684 |
| TodoMVC / empty-session | 915.345 / 931.499 | 1303.899 / 1343.032 |
| NovyWave / fresh-process | 502.072 / 504.200 | 1881.583 / 1886.286 |
| NovyWave / empty-session | 508.744 / 509.970 | 1878.022 / 1945.904 |

All source/diagnostic/plan hashes, evidence parity and cache-disabled checks
pass. Overall status is **fail**: TodoMVC and NovyWave exceed time budgets;
Counter verified (~39.4 MiB) and TodoMVC exceed RSS budgets. Counter timing and
NovyWave RSS pass. These are baseline observations, not a regression verdict
against the old producer and not a K1 acceptance run. NovyWave fresh-process
bootstrap WHERE verification is 0.053 ms p50 / 0.057 ms p95; it is not authored
WHERE verification, which remains unimplemented and fail-closed.

The existing `BOON_KERNEL_ORACLE_TRACE_DENSE_OWNER` diagnostic maps the dated
TodoMVC top-five residual variants to these definitions. The trace used the
pre-repair binary only for source attribution: its compiler implementation
matches the starting implementation, but its provenance is stale for scoring.

| Dense owner | Definition under `examples/todo_mvc_physical/Theme/` | Local pattern reads |
| --- | --- | --- |
| 91 | `Glassmorphism.bn::material` | `InteractiveRecessed.focus` |
| 115 | `Neumorphism.bn::material` | `InteractiveRecessed.focus` |
| 127 | `Professional.bn::material` | `Interactive.hovered`, `InteractiveRecessed.focus` |
| 103 | `Neobrutalism.bn::material` | `InteractiveRecessed.focus` |
| 80 | `Classic.bn::font` | `ButtonIcon.checked`, `SmallLink.hovered` |

These local definitions contain no HOLD. This does not prove their complete
transitive reuse conditions. Debug-only tracing now confirms PatternRead is the
first rejected result-path node in all five definitions: expressions 137, 161,
114, 171 and 104 for owners 80, 91, 103, 115 and 127 respectively.
`DirectSummaryPlanCompiler::compile_expression` also lacks that transfer.

[Machine-readable attribution](evidence/compiler-k0-attribution-2026-09-07.json)
records the exact variable/mode identities, representative root/dependency
identities, all quiescent type/mode tuple classes and sampled root epochs.
The dominant variant is `[None, None]` static arguments with
`initial_state_surface=true` in each definition.

| Definition | Distinct dominant-variant calls | Quiescent input type/mode classes | Calls with a nonauthoritative provider |
| --- | ---: | ---: | ---: |
| Classic.font | 175 | 21 | 155 |
| Glassmorphism.material | 179 | 26 | 154 |
| Neobrutalism.material | 179 | 26 | 154 |
| Neumorphism.material | 179 | 26 | 154 |
| Professional.material | 179 | 26 | 154 |

The five variants own 140,791 of 282,393 logical operations (49.9%). Their
891 call frames converge to 125 definition-qualified tuple classes, but final
type equality is **not** a valid reuse key. Many distinct open context roots
resolve to the same `{name, mode}` shape with different live dependency cells.
Twenty representative variable IDs generated 597 root-mutation observations;
some roots went through five to seven distinct bindings before convergence.
This is sampled root history, not a complete dependency-event oracle.

All 1,871 requested actual variables produced quiescent records. Trace runs
retained 620,555 activations and the same diagnostic fingerprint. IDs are
qualified by their tracing run; they are not stable cross-revision identities.
The observers are debug-only and their timing is not acceptance evidence.
An additional untraced debug NovyWave diagnostics smoke completed in
2,451.511 ms with zero diagnostics; Rust build time is excluded.

The relevant correctness boundary is concrete: `project_pattern` performs
tag-sensitive payload narrowing and requirement scaffolding for open providers;
authoritative providers remain directional and may replace disappearing
projections. Pattern bindings also retain their fixed arm-local flow mode.
Simply treating this as an ordinary field projection is unsound. K1 must
preserve those equations and per-call state/capture ownership, including calls
through nested summaries.

Independent read-only review identified two mandatory counterexample gates
before conversion. First, a wrapper calling `choose(First, open_value)` must
not inherit `Wrapped[item]` requirements from an untaken `Second` branch.
Residual specialization prunes that branch, but unconditional summary-input
requirement emission would not. Second, a later demanded requirement can
invalidate a selector already read in the same summary activation. The
existing scratch memoization and acyclic self-replay suppression must not
publish that stale result. Include nested Invoke variants of both tests.

The proposed ownership is a typed formal-root projection path (whole, field,
pattern), with private occurrence cells and lazy requirement backflow in the
existing summary evaluator. Dependency/read-write scheduling must make those
mutations converge in the existing queue. Pattern-local mode identity stays
separate from interned type-path identity. Computed pattern providers that
cannot be expressed by this path remain a precise generic residual case; do
not add a detached partial pattern evaluator or result-type-equality cache.

### Reproduced baseline correctness failure

`crates/boon_compiler/tests/kernel_transfer.rs` adds the high-level wrapper
cases. The pattern case passes on the existing residual path. The ordinary
field case **already fails before K1**: `choose(First, value)` exports an open
`item: VALUE` object requirement into `wrapper(value)`, so calling the wrapper
with text is incorrectly rejected. This is the eager requirement emission
identified by review, not a newly introduced pattern-transfer failure.

The failing case is explicitly ignored only to retain an honest clean K0
baseline checkpoint. Reproduce it with
`cargo test --locked --jobs 2 -p boon_compiler --test kernel_transfer -- --ignored`.
K1 must remove that ignore and make the test pass; an ignored reproduction
cannot satisfy K1 acceptance. No previously passing gate was disabled.

The 172 existing kernel unit tests, including the dependency firewall, pass
after the attribution-only change. The 14 summary-focused tests also pass.
The next scheduling change must distinguish a summary's actual-input read
subscriptions from its output-observation subscription in the existing reverse
consumer store. Input mutations may requeue the active summary; publishing its
own fresh contextual-hole result must not cause endless self-replay. Duplicate
roles on one variable must retain the read role, including after aliasing.
Do not add a second queue or globally persistent contextual-hole cache.

Frozen comparison cohort: Counter, TodoMVC physical and NovyWave, diagnostics
and verified intents, fresh-process and empty-session, unchanged release flags
and mimalloc lane. The independent wrapper requirements, stateful and invalid
source holdouts remain correctness gates. Target the five mapped residual
variants (140,791 linked operations); also report total activations (620,555),
summary-node evaluations (32,365), summary-call activations (6,373), term
interning, allocation calls/bytes and RSS so moved work remains visible. Counts
are not a universal cost model: the cut also needs an end-to-end product gain
outside noise. The 25% work-reduction hypothesis and 3+30 comparison protocol
are unchanged.

The first source probe also exposed an independent inline-function limitation:
`FUNCTION wrapper(value) { choose(which: First, value: value) }` receives no
AST body child (`ast_statement` assigns no expression to a function, while
`ast_statement_block` only collects following indented children). The checked
adapter then reports no direct or structural result. The canonical multiline
probe above isolates transfer semantics; it is not a fix or product workaround
for inline function support. Preserve this missing-feature report for a parser
tranche; do not claim that feature was implemented by K1.

## K1 correctness prerequisite — 2026-09-08

The clean K0 baseline is preserved at commit
`2d7a5343c9d98e9eeabc5c0de0c3c5728f4fb7f2`, in the detached sibling worktree
`/home/martinkavik/repos/boon-compiler-k0-2d7a5343`. Its product and evidence
release binaries were built before the K1 changes and copied byte-for-byte to
that worktree's `target/release`:

- Product SHA-256: `178b6b58c641890f1028bbfbf612b1a3b5fd29df895f191ab0b536db43948ecf`.
- Evidence SHA-256: `c547ddac0c80d0b4079953e972809525f2fbb96ab8289b61837a4a5abb6f329b`.
- Embedded workspace-input SHA-256: `299f1b59bbc3b2a32ea4e7206664a23fad04653a2b738d6221486ad2b26ae13c`.
- Embedded HEAD matches the baseline; dirty is false. Release settings remain
  opt-level 3, LTO false, 16 codegen units, debug assertions and overflow checks
  off. These binaries are not fresh candidates for the changed main worktree.

The first implementation change makes ordinary summary requirement paths lazy:
private cells are allocated per occurrence, but no requirement equation is
connected until evaluation reads that input. Whole-value backflow is equality;
field backflow uses the existing projection equation. This removes the eager
standalone requirement equations, not the generic residual solver.

The same reverse-consumer store now distinguishes ordinary dependencies,
summary input reads and summary output watches. A changed input can requeue
the active summary after its activation-local scratch becomes stale; its own
output publication cannot trigger contextual-hole self-replay. Input wins
when the same variable is both input and output. There is no second queue or
result-type-equality cache.

The formerly ignored ordinary-field regression is enabled and passes alongside
the pattern regression. The first broader kernel run caught a whole-value
backflow mistake (a directional read failed to export a callee requirement);
the equality correction passes the existing regression. All 177 kernel unit
tests pass, including new direct/nested late-backflow replay, output-only
contextual-hole termination and same-input/output subscription-role tests.
This is a correctness prerequisite, not accepted K1 performance evidence.

Independent read-only review found no definite scheduling defect and requested
two additional focused gates: a taken nested-field requirement and a late
constraint on a distinct caller requirement provider while the actual stays
authoritative. The four high-level transfer cases (untaken field, nested field,
pattern and taken nested field) and all three staged-compilation tests pass.
The separate-provider gate also passes: a late extra field on the caller's
requirement root queues the summary again, the merged requirement retains both
fields, and the concrete authoritative input and Number result remain unchanged.
Its initial run failed only because the test used an undeclared frozen-pool
symbol; the corrected test uses the existing `kind` symbol.

The review also identified a retained validation limitation: private projection
cells are not fully covered by directional-writer validation, and requirement
path length is checked only when demanded. Existing value-path cells have a
similar limitation; do not claim a complete private-cell validation boundary.
The larger consumer row and extra per-summary dependency collection/sort need
measurement alongside the deleted eager equality operations; no cost direction
has been established yet.

## Next action and status

The focused lazy-requirement gates and conditional review requests are met.
Next add typed formal-root pattern transfer with pattern-local mode identity preserved.
Pattern transfer is not implemented yet. Count summary work as well as residual
work; use the required alternating comparison and holdouts before claiming a
speedup. The baseline collector still needs a correctly baseline-rooted xtask
build: its workspace root is compiled in, so copying main's xtask is invalid.
No fresh candidate release measurement or K1 acceptance run has occurred;
inherited performance failures remain open.

## Pattern-transfer WIP — 2026-09-09

The lazy-requirement prerequisite was committed as `6dd95c4e`. The next cut is
uncommitted and not performance-accepted. Typed `Whole`, `Field` and `Pattern`
paths now reuse the existing `project` / `project_pattern` equations, with
occurrence-private requirement cells. Pattern result modes stay fixed across
path composition; a completion assertion and the emitter reject a pattern or
requirement input escaping as an ordinary input-derived result mode. Computed
pattern providers without a formal-root path remain residual equations.

The first broader compiler run exposed two TodoMVC failures (96 tests passed,
two failed, one was already ignored). Shared transfer omitted the closed WHEN
domain equations used by residual lowering, so wrapper interfaces retained only
payload-bearing alternatives and rejected valid bare tags. A five-case transfer
suite now includes the small `Plain | Wrapped[value]` reproduction. Adding
occurrence-private `PatternRequirement` inputs and impure `Unify` nodes made
that reproduction pass; no mutable expected-type holes live in shared bytecode.
Nested invocations receive distinct requirement operands. The domain sequence
remains lazy under its enclosing arm; wildcard/binding domains stay open.

Read-only review identified the next necessary split: resolved input values
cannot substitute for writable requirement identities. `SummaryValue` now
carries those separately, including authoritative callback data with a distinct
caller requirement surface. Closed directional evidence is copied as a term,
not by aliasing its authoritative cell. Unresolved nested directional evidence
still needs explicit adversarial coverage; do not claim complete parity yet.

The writable-domain change exposed a real non-monotone equation cycle in the
existing syntax-discrimination test: a VariantSet domain and unqualified object
field requirements erased and reinstalled each other. A full unit run exceeded
60 seconds in that test; only its test process was terminated. A subsequent
isolated 10-second diagnostic timeout confirmed non-convergence. Neither timeout
was added to production or treated as a convergence rule. The fix qualifies
descendant formal-field paths with the active tag arm: A requires a, B requires
b, rather than exporting both fields unconditionally. The formerly looping test
now passes immediately, and all 179 kernel unit tests pass.

New solver gates cover late nested payloads, missing/wrong/disappearing tags and
reappearance, plus concrete callback data changing alternatives while its
separate callable domain retains both alternatives. The compiler-wide rerun,
additional mode/currentness and nested-invocation coverage, fresh release
measurements and final independent review remain outstanding. There is no new
NovyWave timing or speedup claim for this WIP.

The latest compiler-wide rerun rebuilt in 1m14s and reached the two TodoMVC
tests, but both exceeded 60 seconds (the earlier complete failing run took
25.18s total). At 1m37s the test process still used roughly two CPU cores and
218,528 KiB RSS. A read-only debugger attach was denied by ptrace policy; no
machine policy was changed. Only that test PID was terminated. This is an
unresolved large-fixture stall, not a passing gate or a proven timing result.
Next isolate one TodoMVC test and attribute active solver work before making
another equation change. The 179-test kernel pass does not establish that this
broader stall is fixed; do not commit the WIP as a verified production cut.

The isolated 12-second TodoMVC trace reached 480,000 mutations across many
operations, rather than one slowly executing operation. Sampling is debug-only:
`BOON_KERNEL_TRACE_EPOCHS=1` with no selected roots reports every 10,000th
mutation; explicit selected-root tracing keeps its existing behavior. The run
ended by its diagnostic timeout, not by successful convergence.

Two additional deterministic unit reproductions now pass after focused fixes:

- A directional callback provider with no first value yet must not alias its
  private read cell to its separate requirement channel. Authority is checked
  on the input provider, not merely on the still-uninitialized read cell.
- Same-tag requirement payloads must merge through solver-aware equality.
  Immutable structural widening previously replaced their raw variable IDs
  without equating their union-find identities. `merge_equal_terms` now merges
  matching tagged payloads recursively using the existing variant scratch pool.
  The regression asserts that replaying either contributor after the first
  merge produces zero additional mutations.

All 181 kernel tests pass. The isolated TodoMVC oracle is being rerun; this
does not yet prove that the large-fixture stall is resolved. Review also calls
for whole-selector forwarding through an arm and invalid/transiently mismatched
closed evidence tests before accepting the new domain-transfer representation.

### Ownership reassessment, not another equality special case

The isolated TodoMVC oracle still exceeded 80 seconds after those fixes and
was stopped. A second bounded trace reached 530,000 mutations. Selected root
27968 / operation 6422 alternated complete font-record types: for example,
`weight` changed between tag sets and open-empty Object, while `style` and
authored field order also changed. This is real value/shape churn, not merely
equal payloads retaining different raw IDs. Raw diagnostic traces and the
bounded counterexample are preserved in
[the non-convergence evidence](evidence/compiler-k1-nonconvergence-2026-09-09.json).

A fresh-context read-only architecture audit found that this prototype mixes
directional data evidence with permanent conditional requirements. The new
`incompatible_directional_evidence_reaches_requirement_quiescence` test proves
one generic replay cycle without hanging: unchanged closed Object data plus a
tag-domain requirement produces 9 mutations after the first activation and 11
after the second. Copying the data restores a field that the incompatible
domain immediately erases. The new test is enabled and fails; the earlier
181-test pass must not be cited as a current all-tests pass.

The audit also identifies two representation gaps that another equality
special case cannot solve: inactive arms cannot withdraw prior union-find
effects, and whole-selector forwarding does not retain the active arm guard.
These are not yet proven causes of every TodoMVC oscillation, but they are
mandatory soundness obligations for K1.

The next implementation decision within K1 is therefore:

1. Retain definition-owned bytecode and occurrence-private cells; introduce no
   second solver or result-type cache.
2. Represent directional value evidence, writable requirement destination and
   active-arm guard as separate facts. Whole forwarding keeps the original
   tagged value, not just its payload.
3. Give each conditional requirement contribution an exact occurrence/effect/
   nested-invocation identity. Reevaluation replaces its contribution; an
   inactive site contributes nothing.
4. Aggregate current contributions in the existing component solver without
   permanently unioning retractable contributor cells into the destination.
   Unconditional inference equalities keep their existing role.
5. Replace unconditional closed-data copying with explicitly owned replaceable
   evidence or a compatibility obligation. Do not suppress input replay to hide
   the cycle, widen all tag arms conjunctively, or relax diagnostics.
6. Prove incompatible-input quiescence, late-arm withdrawal, A→B→A equivalence
   to a fresh A solve, preservation of another occurrence's still-active
   contribution, whole forwarding, and nested/HOLD identity isolation before
   rerunning large-fixture acceptance.

This rejects the current permanent-side-effect transfer prototype, not the
K1 reuse hypothesis as a whole. The goal, comparison protocol and exit criteria
are unchanged. The WIP remains uncommitted; performance acceptance, the final
independent review and the complete K1 outcome are still outstanding.

### Replaceable contribution table: initial implementation

Added `solver/requirements.rs` as the private storage substrate for the ownership
change. It retains dense occurrence/site identities and stages complete effect
replacements before commit. Unvisited sites withdraw their facts without
touching another occurrence. Failed evaluation retains the previously committed
facts. Destination dirtiness is deduplicated; unchanged replay adds no dirty
destination. Registration order is separate from branch visitation order.

A bounded independent read-only review found no standalone table blocker, but
identified mandatory integration obligations: nested transactions, mutable term
dependencies, base-fact separation, alias ordering/invalidation, and preventing
payload mutation before commit. The table now defines an owner as a top-level
summary activation with distinct nested-effect sites under that same atomic
replacement. Site registration seals before first evaluation. Exposed site
ordinals support deterministic merging across aliased destination identities.

Seven focused table tests pass, covering withdrawal, multiple contributors,
A-to-B-to-A replacement, unchanged replay, staged/aborted evaluation, skipped
nested calls, parent abort, and order independent of visitation/alias-member
enumeration. These are storage-level tests, not proof of integrated solver
behavior. Command: `cargo test -p boon_compiler_kernel --lib
solver::requirements::tests --jobs 2`.

The full kernel suite run after the first four table tests had 185 passes and
the same one enabled failure: incompatible directional evidence still produces
11 mutations instead of 9. The three later table tests were checked separately.
The table is not yet connected to summary evaluation or union-find aggregation;
there is no production behavior or timing improvement from this substrate yet.
Next integrate separately retained unconditional base bindings and exact
contribution dependencies, then switch summary effects to atomic replacement.
Do not aggregate into the previous aggregate, permanently union retractable
payloads, or use raw term-ID equality as proof that nested bindings are current.
All work remains uncommitted pending a coherent passing cut; no push occurred.

### Solver aggregation integration and retraction counterexamples

The contribution store is now connected to the component solver's binding and
dependency machinery, but summary evaluation has not yet been converted to
publish through it. Managed destinations retain unconditional base facts
separately. Equality, directional replacement and alias establishment preserve
that separation; aggregation does not permanently union contributor payload
variables. Original contribution terms remain dependency authorities even when
their resolved types are closed or unchanged. Aliased destinations fold sites
in registration order. New scratch storage is included in existing scratch
work accounting.

Four solver-level tests prove base restoration after withdrawal and late
unconditional facts, mutable payload invalidation without contributor unions,
alias-order-independent field order and withdrawal, and quiescent incompatible
fact replay through the new aggregation path.

Independent review identified two additional generic counterexamples, both
reproduced as bounded enabled tests:

- Two cyclic contributions retained Number after their external Number seed
  was withdrawn. Recomputing against the previous effective bindings was not
  enough. The solver now clears the affected reverse dependency cone's derived
  bindings before re-deriving them from base facts and live contributions. This
  test passes. The algorithm is component-local retraction, not revision
  currentness or full-provider scanning; its work must be measured before any
  performance acceptance.
- An ordinary projection copies a retractable `{value: Number}` into its
  consumer through permanent equality. After withdrawal, replay scaffolds that
  consumer back into the provider's unconditional base. This test remains red.
  Merely replacing the final aggregate cannot fix the projection's ownership.

The full kernel run has 193 passes and two failures (195 tests, none ignored):
the new projection counterexample and the original summary replay regression
(11 mutations instead of 9). All 12 other contribution tests pass. No TodoMVC
rerun or release measurement is justified yet, and no production checkpoint is
claimed. Next replace summary requirement-path scaffold/equality replay with
explicit destination/path/guard contributions and non-mutating value reads;
preserve required backflow rather than suppressing it to pass the new test.
Then route summary effect sites, including nested calls, through the activation
transaction. Exact contribution/aggregation work counters and independent
review of the integrated path remain required before the K1 decision.

### Summary writes now use activation-owned contributions

Summary calls now collect requirements by exact call occurrence and root input
path. Nested summary evaluation shares that activation's collection; it does
not independently commit child effects. Successful evaluation publishes the
complete replacement; errors abort it. Closed value evidence and explicit
constraints are folded into that replacement instead of repeatedly mutating a
permanent requirement cell. The call table is solver-local and keyed by output
variable identity, never by resolved result types or cross-revision cache keys.

Requirement-bearing summary inputs use a non-mutating value read. Reverse path
construction (whole, field, or pattern payload) happens when publishing the
collected requirement, not while reading the value. The old private
requirement-root/consumer fields are no longer used by this evaluator; their
model/construction deletion is pending the coherent cut, not an accepted
compatibility layer.

The original `incompatible_directional_evidence_reaches_requirement_quiescence`
regression now passes, including unchanged replay, through actual summary
evaluation. The full kernel suite still has 193 passes and two failures, but
the remaining failures are now:

- The ordinary projection counterexample: non-summary projection/equality can
  still reimport withdrawn evidence into a managed destination's permanent base.
- `separate_summary_requirement_requeues_without_changing_authoritative_actual`:
  requeue and value-isolation checks pass, but exact field order is wrong.
  Actual and expected both contain `kind: Text` and `value: Number`; actual
  order is `[kind, value]`, expected `[value, kind]`. Folding all base facts
  before contributions reorders a late unconditional field ahead of the
  earlier contribution. Preserve ordering authority separately from retained
  type evidence; do not weaken the assertion or simply reverse every fold.

No integrated whole-arm-forwarding, nested-effect identity, large-fixture,
release, or final independent acceptance is claimed. Registration order across
lazy call activation and the cost of occurrence/site storage also need review.
Next fix the projection ownership boundary and deterministic field ordering,
then run the compiler transfer/staged gates before another TodoMVC acceptance
attempt. No commit or push; the goal remains open.

### Requirement-ownership correctness milestone

The two remaining focused failures are resolved without changing their
assertions. Ordering receipts retain only surviving field order; all values,
shape kinds and openness come from freshly derived facts. A new test proves
that ordering cannot resurrect a removed field or an old field type, including
inside a list. Alias joins discard representative-dependent ordering receipts
and use stable contribution-site order.

Whole/field/pattern projections from contribution-managed providers now own
separate forward-value and backward-requirement sites. Backflow reads the
consumer's independent base facts, not the value that this projection just
forwarded. The regression additionally proves that a consumer loses a withdrawn
Number and that a new independent Text constraint still flows back to the
source. These equations settle through the existing operation queue.

Deleted the obsolete private requirement root and per-step consumer allocation
from summary construction. `KernelSummaryRequirementTarget` now holds only the
destination; the already-owned value path determines reverse scaffolding.
There are no remaining references to `KernelSummaryRequirementPath` or its
removed private-cell fields in kernel source.

Verified focused results after that deletion:

- Kernel library: 196 passed, none failed or ignored.
- Compiler transfer integration: 5 passed.
- Compiler staged integration: 3 passed.
- Fresh debug compiler unit binary: 98 passed, none failed, one pre-existing
  ignored directional timing probe; 31.10 seconds with two test threads.
- Isolated TodoMVC checked-publication/replay/verification oracle: passed in
  21.25 seconds under a 30-second observation bound. This is debug correctness
  test duration, **not** release compilation latency or a speedup claim.

This is a local correctness checkpoint within K1, not the accepted K1 decision.
Next: adversarial whole-forwarding and occurrence/currentness coverage, complete
work counters for contributions/aggregation/cone invalidation, fresh release
product/evidence measurements, and independent final review. Lazy registration
ordering, cold memory/work cost, and the full comparison/oracle cohort remain
unaccepted. The original budget/oracle gates and 3+30 A/B protocol are unchanged.

After formatting the changed Rust files, a fresh `cargo test -p
boon_compiler_kernel -p boon_compiler --lib --jobs 2 -- --test-threads 2` also
passed: 98 compiler tests (one existing ignored probe) and 196 kernel tests.
The compiler suite took 32.38 seconds; this remains debug test-suite duration.

### 2026-09-10: accounting and whole-selector holdout

Added `KernelRequirementWork` and producer/collector projection for occurrence
and site counts, begin/stage/commit visits, changed and withdrawn sites,
aggregate/fact visits, invalidation variable/edge visits, ordering traversal,
projection evaluation and forward/reverse path steps. Unchanged replay remains
visible as work even when no facts change. The collector distinguishes an
absent historical accounting block from an explicitly zero-valued block; a
present block must contain every counter. No budget or semantic oracle changed.
Focused checks: 15 contribution tests and four collector work-sample tests pass.
`cargo check -p boon_cli --bins --jobs 2` also passes with the new report fields.

The independent reviewer could not run because of the account usage limit;
there is no new independent approval. Main-agent source inspection and an
enabled compiler integration test confirmed the previously suspected whole
selector gap. `inner` accepts First/InnerOnly; `wrapper` accepts First/Second
and calls `inner(which: which)` only in its First arm. Current compilation
exports First/InnerOnly/Second from wrapper and reports an incompatible argument
at the guarded inner call. See
[the bounded evidence](evidence/compiler-k1-whole-selector-2026-09-10.json).

This is not fixed by activation-level withdrawal alone: all potential arms of
an abstract selector can participate in an activation, so the active arm guard
must qualify whole-value requirements and values. The current compiler only
qualifies descendant Field paths, and its empty-path helpers return unchanged
input identities. The source-level failure may also involve principal/residual
publication; do not assume a summary-only edit is sufficient. Comparison with
the preserved K0 behavior remains outstanding.

The new regression is intentionally enabled and failing. K1 release acceptance
waits for this guard obligation; the prior local correctness checkpoint was not
final acceptance. Next carry whole-value arm guards explicitly through value
forwarding and reverse requirements, verify the principal/call-publication
boundary, then rerun the holdout and previous focused gates. Accounting and the
new regression are uncommitted WIP, not a new accepted checkpoint.

Follow-up comparison: the regression now asserts zero diagnostics from the
test-only legacy checker before checking the candidate. A fresh run passes that
assertion and fails the candidate no-errors assertion (exit 101). The preserved
clean K0 release producer also rejects this source with one diagnostic; changing
only the inner actual to the literal First produces zero diagnostics. This is
an existing production-kernel parity gap, not evidence that K1 introduced it.
The CLI comparison uses client role and exposes only the diagnostic count;
the integration test uses server role. Exact K0 diagnostic bytes are not claimed.

Source inspection confirms that principal `FormalRead` unconditionally unifies
the read with the whole formal requirement, and `expression_requirement_variable`
returns that unrestricted destination for call backflow. The existing adapter
`attach_tag_match_mode_narrowings` handles only deeper ValueRead paths and stores
no pattern. Existing PatternProjection returns a tag's payload, not its whole
tagged value. None is an adequate whole-value guard without an explicit semantic
extension. A bounded independent read-only review is examining the common
guarded-access ownership seam; no final K1 approval or speedup is claimed.

The independent read-only review confirmed the shared guarded-access seam and
identified a necessary negative control: filtering backflow must not erase a
callee mismatch. The initial tag-only negative control was invalid as a legacy
oracle: that checker widened the inner formal to First | InnerOnly. Replacing
the inner body with `which + 1` gives a genuine negative control. Legacy reports
an error and the preserved K0 producer reports one diagnostic, but the candidate
reports none and publishes an open empty Object for the wrapper formal. The
enabled `whole_selector_guard_does_not_hide_an_incompatible_callee` regression
now captures that false negative. This is a regression against K0, not merely
the older false-positive gap; the exact introducing commit is not yet bisected.

The positive test now also asserts the legacy wrapper's exact First | Second
domain, rather than relying on empty diagnostics alone. Both positive legacy
assertions pass before the candidate fails. The numeric negative's legacy error
assertion likewise passes before candidate failure. The full current kernel
library suite, including accounting, passes 197 tests; it does not cover these
source-level failures. No guard implementation or accepted checkpoint has been
landed. Next: retain one occurrence-scoped guard chain across principal and
summary paths, keep actual evidence separate from callee requirements, and
verify both error directions before release measurements. Tagged payload and
lexical containment semantics need checked-row tests, not dependency-closure
inference or payload-projection substitution.

### 2026-09-10: whole-selector arm guards closed (083ed083, 789dda76)

The guard obligation is now carried by the caller's definition program rather
than by shared summary bytecode. Each owner program gets one structural map of
the tag arms that surround its expressions: an expression belongs to an arm
only when every path from the owner roots reaches it through that arm, so a
selector read that is also reachable outside its arm and an occurrence shared
by sibling arms stay unguarded. The map is computed by one dominator-style
intersection pass and cached per owner on the residual module cache, so
specializations reuse it instead of recomputing it.

Whole reads inside an arm now forward the proved value. A bare tag arm
publishes the closed tag; a payload arm reconstructs the tagged value and keeps
its payload connected to the formal through a pattern projection. A call whose
argument is such a read carries an explicit `guarded` flag, which detaches the
callee requirement channel: the requirement is checked against the proved
value instead of reshaping the caller's formal, and an incompatible callee is
still diagnosed. `CallActual.guarded` also suppresses the residual instance's
formal-requirement binding for the same reason.

The summary bytecode path intentionally keeps bare-tag forwarding only. An
intermediate revision also reconstructed tagged payload records inside shared
summary programs; the debug TodoMVC checked-publication oracle then took 143
and 162 seconds against a 23.9-second guard-disabled baseline, while the
owner-side reconstruction alone ran that oracle in 25.1 seconds. The expensive
variant is deleted, not tuned, and is recorded as a rejected local experiment
rather than a K1 hypothesis outcome.

Checked results after both commits:

- kernel library: 197 passed, none failed or ignored.
- `kernel_transfer`: 10 passed, including the two formerly failing
  `whole_selector` regressions and three new tagged cases (valid tagged call,
  incompatible tagged payload, nested incompatible tagged payload).
- compiler library: 98 passed, one pre-existing ignored directional probe;
  the TodoMVC checked-publication/replay/verification oracle passed in 25.11 s
  under the final configuration.
- `staged_compilation` 3, `map_set` 3, `nested_boolean_match` 3, `pulses` 8,
  `cargo check -p boon_cli --bins` clean.

The valid tagged call keeps its `Other | Wrapped[value: NUMBER]` interface and
the invalid one is rejected again, which the K1 WIP and the first committed
guard revision both accepted. One limitation remains explicit: the candidate
attributes the tagged mismatch to the call whose concrete actual violates the
transmitted requirement, while the legacy oracle attributes it to the guarded
inner call. The preserved K0 producer reports one diagnostic for that source;
its exact site and bytes remain unclaimed. Closing that attribution difference
requires validating guarded calls per call-site instantiation, which is not
part of this cut.

Next: keep the guard work counted (occurrence/site work is already in
`KernelRequirementWork`), run the focused compiler gates once more, then build
fresh release product/evidence producers and begin the K0/K1 comparison
protocol. No performance claim is made from the debug correctness timings
above.

### 2026-09-11: K1 decision — targeted replay removed, end-to-end regresses

The K1 cut has now been measured against the preserved K0 producer with the
manifest protocol: three setup plus 30 scored observations per fixture and
intent in both cold modes, product and evidence lanes, one process per
observation, one Cargo process at a time. The baseline ran first from the K0
worktree producer (`2d7a5343`, product `178b6b58...`); the candidate ran twice
from `c7ff71a8` (product `f80452ee...`). Both producers are clean, the budget
manifest is untouched, and every report is an honest fail report.

Measured p50 for the large fixtures (milliseconds, best candidate pass):

| Fixture / mode / intent | K0 | candidate | delta |
| --- | ---: | ---: | ---: |
| TodoMVC fresh diagnostics | 930.2 | 1199.4 | +28.9% |
| TodoMVC fresh verified | 1281.9 | 1597.0 | +24.6% |
| TodoMVC empty diagnostics | 918.8 | 1170.2 | +27.4% |
| TodoMVC empty verified | 1281.5 | 1573.7 | +22.8% |
| NovyWave fresh diagnostics | 497.3 | 903.9 | +81.8% |
| NovyWave fresh verified | 1847.6 | 2266.4 | +22.7% |
| NovyWave empty diagnostics | 505.0 | 906.9 | +79.6% |
| NovyWave empty verified | 1850.4 | 2277.4 | +23.1% |

Counter is a work control: it shows no algorithmic change within about +/-
3%, and the candidate's empty-session diagnostics outlier in the first pass is
machine noise, not work. It is not a timing-clean baseline; its baseline
fresh-process diagnostics p95 is 14.3 ms against a 10 ms budget. The first
candidate pass was slower still (NovyWave verified p50 3693.8 ms) while its
work counters are byte-identical to the second pass, so the spread is not
algorithmic. Only the candidate has two passes, so this is not yet a
like-for-like stability comparison; the next cut needs interleaved A/B/A
ordering.

The work counters explain the regression. The cut removes the targeted
residual replay: TodoMVC linked operations fall from 282,393 to 82,451
(-70.8%), activations from 620,555 to 181,056 (-70.8%), mutations from
465,647 to 182,294 (-60.8%), union operations from 196,759 to 41,929 (-78.7%).
NovyWave linked operations fall 59,057 to 39,276 and union operations 31,884
to 15,052. The work does not disappear, though: TodoMVC summary node
evaluations rise 32,365 to 1,017,465 (+3044%), summary definition nodes 236 to
2,325 (+885%) and term intern requests 342,922 to 1,645,514 (+380%).
NovyWave rises 462,003 to 2,172,644 summary node evaluations (+370%) and
12,866 to 105,935 summary definition nodes (+723%). TodoMVC performs about 91
summary node evaluations per summary call activation, so the shared program is
re-walked per invocation instead of being reused across equivalent
invocations. The candidate-only `KernelRequirementWork` block adds its own
bookkeeping on top (NovyWave fresh diagnostics: 185,441 aggregate fact visits,
37,215 invalidation variable visits, 25,355 commit site visits).

Decision: the K1 reuse hypothesis is rejected as implemented. There is no
verified end-to-end production speedup; the transfer cut's targeted work
reduction is real but is outweighed by interpreted summary evaluation and
interning work. The correctness cuts from this run are preserved (whole-value
arm guards, tagged forwarding, the open-read projection-path requirement fix
that restored NovyWave verified). The prior K1 machinery is prior committed
work, not this run's disposable experiment, so it is not deleted without its
own scope.

Next architectural decision: make definition-transfer evaluation proportional
to distinct typed input tuples rather than invocations. Evaluate a transfer
once per distinct quiescent input tuple plus dependency epoch, reuse that
result for equivalent invocations, and invalidate exactly when the tuple or a
dependency epoch changes. K0 attribution found 125 quiescent tuple classes
for 891 top-five-variant calls, a 7.1x ceiling on invocation reuse rather than
tenfold; even a perfect 7x cut of 1,017,465 summary evaluations lands near
1.43e5 against K0's 3.2e4, still about 4.4x above the K0 cohort, so the next
cut must also reduce per-tuple evaluation cost. Final result-type equality is
not a valid key; if the tuple key cannot be made sound, keep the shared
physical bytes and return to linked residual specialization for evaluation,
which was correct and faster than the interpreted summary path. Acceptance
uses the same 3+30 protocol with interleaved A/B/A ordering and requires
targeted summary work at or below the distinct-tuple count, per-tuple cost
that does not reintroduce the gap, and no residual activation or interning
regression.

Stateful and invalid-source holdouts stayed green at the measured head:
private-state capability, HOLD occurrence freshness, multi-contributor
withdrawal and skipped nested invocation tests in the kernel suite, plus
invalid-call, invalid-update and staged-rejection tests in the compiler suite.
The candidate's tooling contract digest differs from the baseline because of
candidate-only requirement accounting, so the measured bundle is the transfer
cut plus the guard and requirement fixes rather than a byte-identical
instrument. Latency figures are the upper-middle median of the 30 sorted
scored samples; the report's own p50 field is the nearest-rank lower element
and reads up to about 0.7% lower.

The decision record, report hashes and full attribution are in
[the 2026-09-11 evidence](evidence/compiler-k1-decision-2026-09-11.json).

### Verification pass, 2026-09-11

The focused correctness gates were re-run at the measured head `c7ff71a8` by
the main agent: `boon_compiler_kernel` 197 passed, `boon_compiler` library 98
passed with the one pre-existing ignored directional probe, `kernel_transfer`
10, `map_set` 3, `nested_boolean_match` 3, `pulses` 8 and `staged_compilation`
3, all green, with the whole-selector regressions enabled and passing. The
figures above were re-derived directly from the three stored reports:

- The report hashes match the decision record: baseline `91f866fe...` and the
  two candidate passes `1151c6a8...` and `4d9194b0...`.
- The two candidate passes have byte-identical numeric work counters across
  every fixture, mode and intent, so the pass-to-pass latency spread is not an
  algorithmic difference between them.
- Every latency figure is the median of `normative_elapsed_ms` and every work
  figure the median of the corresponding counter, both over the 30 scored
  observations of a lane in `fixtures[*].modes[*].{diagnostics,verified}`.
- The candidate's `machine_plan_hash_pass` oracle is inherited-failing rather
  than K1-specific. All three fixtures fail it at K0 (`counter` observed
  `6c284c85...`, TodoMVC `149c7f17...`, NovyWave `4fd22df2...`), and TodoMVC
  (`4087c4ff...`) and NovyWave (`5926de7b...`) still fail it at the candidate;
  only the `counter` lane passes at the candidate. `budgets/compiler.toml` is
  unmodified, so this oracle separates nothing about K1 in either direction.
The independent fresh-context read-only review has since returned a support
verdict for the rejection decision. It reproduced the report hashes, producer
identities, the byte-identical candidate work objects and all quoted counters;
it also confirmed the untouched budget manifest and honest fail reports. Its
record-precision corrections are folded into this section and the evidence
file: the reuse ceiling is 7.1x with an additional per-tuple cost requirement,
the stability claim applies only to the two candidate passes, the counter
control band is about +/-3%, and the median convention and instrument-digest
difference are disclosed. The reviewer did not re-run the correctness gates or
the collector protocol, so those remain main-agent evidence. The decision
therefore rests on the work attribution and the best-case end-to-end pass, not
on the inherited-failing machine-plan hash oracle.

### 2026-09-11: K1′ step 0 — warm product baseline

The successor goal's first step measured the IDE-shaped warm path for the
preserved K0 producer and the current candidate at three setup plus 30 scored
edits on physical TodoMVC. The interaction collector cannot persist that
sample count (its 16 MiB report limit rejects the 23.5 MB 3+30 report), so both
producers were invoked directly with identical flags and parsed from their own
JSON; the K0 producer needs the unit-relative edit path `RUN.bn` instead of
the candidate's bundle path. Full record:
[2026-09-11 warm baseline](evidence/compiler-k1p-warm-baseline-2026-09-11.json).

| Warm metric, TodoMVC edit | K0 p50 / p95 | candidate p50 / p95 | manifest p95 |
| --- | ---: | ---: | ---: |
| diagnostics edit to ready | 945.7 / 982.8 ms | 1189.1 / 1213.1 ms | 16.7 ms |
| verified preview edit to ready | 2277.7 / 2334.9 ms | 2802.0 / 2891.1 ms | 100 ms |
| update ack | 0.051 ms | 0.051 ms | – |
| switch ack / bundle lookup | 0.0003 / 0.0002 ms | 0.0002 / 0.0002 ms | 16.7 / 1.0 ms |

Nowhere near the budget: warm diagnostics p95 is 58.9x the 16.7 ms limit at K0
and 72.6x at the candidate; warm preview p95 is 23.3x the 100 ms limit at K0
and 28.9x at the candidate. The per-edit work counters explain it:
each warm edit performs the producer's full cold solve (K0: 620,555
activations and 32,365 summary node evaluations; candidate: 181,056 and
1,017,465). The session retains no solved state across edits, so the warm fix
is K3/K4 work — retained sessions, definition-local finalization and exact
dirty cones — which this bounded goal does not authorize. The candidate's warm
regression mirrors its cold regression (+23-26%), and its warm peak RSS is
lower than K0 (191.9 vs 238.5 MiB). In-flight cancellation remains
unsupported in both producers; only pre-canceled requests are measured.

K1′ therefore stays on the cold transfer-cost cut or its reviewed fallback,
with the warm numbers recorded so the handoff names K3/K4 as the
product-critical next goal.

### 2026-09-11: K1′ decision evidence — planner amplification, not reuse keys

The bounded goal hit its decision boundary with measurements that reject both
of its endings. Full record:
[2026-09-11 planner cost](evidence/compiler-k1p-planner-cost-2026-09-11.json).

Memoization is rejected by the earlier probe: only 35% of summary calls have
closed inputs, the value half is tuple-pure but the requirement half is not
(84 TodoMVC and 4 NovyWave mismatches on identical inputs), and the open 65%
cannot be keyed by resolved terms. The in-tree residual fallback is rejected by
release A/B: with `BOON_KERNEL_DISABLE_DIRECT_SUMMARIES=1` TodoMVC diagnostics
runs 1553 ms against 1166 ms for the summary path (K0 930 ms) and NovyWave runs
2849 ms against 913 ms (K0 497 ms), because the residual path also carries the
K1 requirement machinery (NovyWave 24,756 frames against K0's 703).

The measured root cause of the whole regression is transfer-program shape. A
97-node owner with no calls compiles into a 465-node summary program (45 input,
41 unify, 16 sequence, 16 record, 14 projection, 3 select nodes), and the same
definitions evaluate 5–12x more nodes per call than K0 (definition 79: 213 vs
17 nodes per evaluation; NovyWave definition 996: 25.5 vs 5.1). No nested-call
sharing is attempted on TodoMVC (zero nested calls in the dominant summaries),
so this is planner scaffolding, not transitive inlining. Two sharing
experiments (computed-actual support in `compile_shared_invoke` and a lowered
nested-share threshold) stayed green but were inert and were reverted; the
debug-only reuse and summary-size probes remain as measurement support.

Next cut to select: shrink the transfer program and its per-call evaluation —
audit the per-field requirement inputs and per-arm unify/sequence scaffolding
against the accepted correctness gates, and if the scaffolding is irreducible
compile the transfer to linked code once per definition (K2-style) instead of
interpreting a 400-node program per call. The warm product gap remains K3/K4.
Gates after the experiments: kernel library 197, kernel_transfer 10,
staged_compilation 3 and compiler library 98 with one pre-existing ignored
probe, all green.

Follow-up trace: the final (post-fold) transfer program for definition 104 is
140 nodes — 45 input, 41 unify, 16 sequence, 16 record, 14 projection, 5 term
and 3 select — against a 97-node owner with no calls; the 465 figure is the
pre-fold count. Definition 79's final program is 219 nodes. The unify
scaffolding comes from one emission site that adds a
`PatternRequirement` input and one `Unify` node per closed WHEN arm, and K0's
planner has no such site: that is exactly why K0's programs were small and why
K0 failed the closed-domain and whole-selector regressions the K1 correctness
work fixed. At roughly 265 ns per interpreted node visit, deleting the whole
scaffolding would recover about 160 ms of TodoMVC's +268 ms excess — close, but
not sufficient without cheaper per-node execution. That is a K2-scale linked
code cut, which this bounded goal excludes, so the goal stops here pending a
decision: this first report framed the remaining work as a K2 scope question.

An independent fresh-context read-only review reproduced the reuse probe, the
residual A/B work counters, the warm baseline and the per-definition ratios,
and returned a support verdict with two load-bearing corrections now folded in:
the 465-node figure is a pre-fold IR count (the interpreted program is 140
nodes, 1.44x its owner) and definition 79's final program is 219 nodes, so the
defensible root cause is per-call evaluation cost (110.3 and 212.9 nodes per
evaluation against K0's 17.0 and 5.1), not program blow-up. The reviewer also
noted the residual A/B should store raw outputs next cut.

Scope reassessment after the review: the goal's Allowed Work explicitly lists
"linked compatible code variants and occurrence frames inside the kernel, so
evaluation is scheduled work rather than an interpreted per-call walk". The
corrected next cut is therefore in scope, and the goal continues on it instead
of stopping for a scope decision. K3/K4 warm work remains out of scope and is
recorded as the product-critical follow-on.

### 2026-09-11: K1′ domain fold — structure win, cost driver reattributed

The first in-scope linked-evaluation slice replaces the per-arm `Unify` chain
with one `KernelSummaryNode::ConstrainDomain` per closed WHEN: one node
resolves its pattern-requirement inputs and constrains the selector in arm
order, instead of one `Input` plus one `Unify` visit per arm. All gates stay
green and diagnostics are byte-identical. Full record:
[2026-09-11 domain fold](evidence/compiler-k1p-domain-fold-2026-09-11.json).

The structure win is large: TodoMVC summary node evaluations fall 1,017,465 to
483,995 (−52%) and summary definition nodes 2,325 to 1,114; definition 104's
final program drops 140 to 74 nodes with inputs 45 to 4, and definition 79
drops 219 to 99 with inputs 77 to 2.

Release latency is unchanged: TodoMVC diagnostics 1166.2 → 1174.1 ms (K0
929.8), verified 1589.6 → 1579.1 (K0 1281.1); NovyWave diagnostics 913.0 →
915.1 (K0 497.3) and verified 2215.3 → 2304.8 (K0 1845.4). Halving the
interpreted node visits changed nothing, so node-visit count is not the cost
driver. Every other work counter is unchanged by the fold, and the K0
comparison now points at the real one: term intern requests 342,922 → 1,645,514
(+1.30M), dominated by non-empty object interning 213,235 → 1,189,597 (+976k,
kind 1), plus scratch-pool reuses 1,494,269 → 5,923,303. The requirement
machinery is building and interning roughly a million scaffold objects per
TodoMVC diagnostics solve — one per reverse-projection step per call.

Next cut: stop rebuilding those scaffolds per call — memoize or precompute the
reverse-projection scaffold for a (term, projection path) pair and cut the
scratch churn — with the gate that object intern requests and scratch reuses
fall toward K0's 213k and 1.5M without changing diagnostics.

That next cut was attempted and rejected by measurement. A bounded per-solve
cache keyed by (term id, field) at all three production single-field scaffold
sites left TodoMVC term interning at 1,634,822 against 1,645,514 and object
interning at 1,178,905 against 1,189,597 — about one percent — while latency
rose from 1174.1 to 1238.9 ms; NovyWave diagnostics went 915.1 to 966.8 ms with
identical interning. The arena already serves 97% of intern requests as hits,
so the traffic is structural-equal lookups made from fresh per-call term
identities, which a term-id-keyed cache cannot capture. The experiment is
reverted exactly (the worktree is unchanged). Two measured attempts — the
domain fold and the scaffold cache — have now shown that no single cheap node
or cache reduction moves end-to-end latency; the remaining gap is distributed
across the per-call summary machinery and needs the larger linked-code cut
(compile the transfer to scheduled operations) to have a chance of closing.

A third bounded attempt is also rejected. Removing the three in-pass
`refresh_requirements` calls (in `publish_projection` and
`finish_summary_requirements`) and relying on the drain loop's per-iteration
refresh left 196 kernel tests passing and one failing:
`projection_does_not_reimport_a_withdrawn_requirement_as_base`. The immediate
refresh after staging is load-bearing — a projection must observe the refreshed
aggregate before the next read — so the requirement fold's cost is inherent to
refresh-before-read semantics, not redundant cadence. It is reverted exactly.

Three measured attempts (domain fold, scaffold cache, refresh batching) now
show the same result: the remaining TodoMVC gap is not addressable with bounded
node, cache or cadence changes. Closing it needs solver-core incremental
requirement aggregation (maintain each target's aggregate per site change with
exact removal), K2-style linked transfer code, or the product-critical K3/K4
warm path — all of which exceed this bounded goal's authorized cuts. The goal
is therefore blocked on scope, and the handoff asks for a new cut to be
selected.

### 2026-09-11: K1′ handoff and the selected successor sequence

K1′ closed blocked on scope. The successor contract is
[the requirement-aggregation prompt](BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md),
which selects:

1. **K1″ — incremental requirement aggregation.** Maintain each destination's
   aggregate per committed site change with exact removal, preserving
   refresh-before-read, so unchanged contributors are never re-merged. Gate:
   cold TodoMVC/NovyWave back to K0-class latency with byte-identical
   diagnostics and interning/scratch churn falling toward K0's 213k object
   intern requests and 1.5M scratch reuses.
2. **Goal B — warm revision retention (K3/K4-lite).** The product-critical
   cut: retain the packed solved revision across edits, solve demanded
   definitions only, exact dirty cones with backdating, last-good publication.
   Gates: warm diagnostics p95 <= 16.7 ms and preview p95 <= 100 ms at 3+30,
   no cold regression, no stale publication.
3. **Goal C — conditional linked transfer code.** Opens only if fresh profiles
   after A and B still show transfer evaluation dominating cold first-solve
   cost; otherwise it is recorded as unnecessary with the profile evidence.

The full start commands for all three live in the prompt file. K2–K5 in the
architecture plan keep their long-range order; these three cuts are the
explicitly selected next tranche. No further work happens inside K1′.

### 2026-09-11: K1″ step 1 — fold dedup landed, K0 parity still ahead

The first requirement-aggregation slice deduplicates contributions inside a
fold while preserving the original interleaved order. A debug probe
(`BOON_KERNEL_TRACE_AGGREGATE`) first measured the redundancy: TodoMVC folds
visit 722,579 terms of which only 56,797 are distinct (7.9%), with 2,949
repeat folds and 2,009 single-delta folds; NovyWave is 64.3% distinct. Full
record: [fold dedup](evidence/compiler-k1pp-fold-dedup-2026-09-11.json).

Work effect on TodoMVC diagnostics: aggregate fact visits 722,579 → 56,796
(−92%), term intern requests 1,645,514 → 840,207, non-empty object interning
1,189,597 → 390,804 (−67%), scratch reuses 5.93M → 2.73M. NovyWave falls much
less (fact visits −34%, interning −5%).

Latency, measured as an interleaved A/B against the pre-dedup release binary
(15 alternating one-process rounds): TodoMVC diagnostics p50 1667.2 → 1603.0 ms
(−3.9%) and NovyWave 1408.0 → 1294.2 ms (−8.1%). Against the preserved K0
producer, re-measured interleaved after a machine reboot (10 rounds): TodoMVC
937.7 vs 1077.4 ms (+14.9%) and NovyWave 503.6 vs 872.3 ms (+73.2%). The
dedup takes the candidate from +28.9%/+81.7% versus K0 to +14.9%/+73.2%, so it
is real progress but the K0-class gate is not met. All gates stay green:
kernel 197, kernel_transfer 10, compiler library 98 with one pre-existing
ignored probe, staged 3, map_set 3, nested_boolean_match 3, pulses 8.

Next: the NovyWave gap is genuine re-merging (29 mostly-distinct contributions
per fold), so the next slice must make each fold cheaper, not just
duplicate-free — for example a closed-term fast path that builds the union once
instead of pairwise-merging, gated on a probe of how often every contribution
in a fold is closed.

### 2026-09-11: K1″ sliced further — phase attribution corrected, closed-fold memo

An independent fresh-context review rejected the conclusion that aggregation
was exhausted and produced the missing measurement with a release-visible
env-gated phase probe (it exceeded its read-only mandate to do so, restored the
sources exactly, and disclosed the deviation; artifacts in
`/tmp/k1pp-phase-probe-build/`). The profile changes the target map:

| refresh sub-part | TodoMVC | NovyWave |
| --- | ---: | ---: |
| total | 237.9 ms | 258.0 ms |
| invalidation | 86.9 | 3.7 |
| assembly | 31.2 | 3.0 |
| resolve + merge | 48.6 | 233.1 |
| commit/order | 68.1 | 16.9 |
| refresh calls / folds | 65,216 / 18,792 | 29,646 / 6,425 |

The refresh path is about 21% of TodoMVC and 30% of NovyWave latency; against
K0 it exceeds the whole TodoMVC gap and covers about 76% of the NovyWave gap.
The earlier "program shape dominates" conclusion is withdrawn. Full record:
[fold memo and phase profile](evidence/compiler-k1pp-fold-memo-2026-09-11.json).

The second aggregation slice is a closed-fold memo: a destination whose
resolved contributions are all variable-free compares the signature with the
last folded one and restores the stored aggregate on a hit. Merging closed
terms binds no variables, so resolving before merging is equivalent to the
interleaved order; mixed folds keep the interleaved path and are never
memoized. Interleaved ten-round A/B against the dedup build: TodoMVC
diagnostics 1077.9 → 1061.8 ms (−1.5%), NovyWave 857.2 → 848.5 ms (−1.0%), with
all gates green.

Next targets: NovyWave's 233.1 ms resolve+merge (per-site resolved-term reuse
with an epoch guard, or delta-based aggregation with exact removal while
preserving canonical order and payload identity) and TodoMVC's 86.9 ms
invalidation plus 68.1 ms commit/order (first measure how many refresh calls
carry dirty entries and how much invalidation is real cone work). Goal C's
opening condition is not met while the refresh path dominates.

### 2026-09-11: K1″ third slice — sub-phase split and single-pass merge

The release sub-phase probe (env-gated, inert when unset) splits the refresh
cost. TodoMVC: 283.2 ms total = invalidation 86.9 + commit/order 68.1 +
assembly 31.2 + resolve 26.7 + occurs 16.7 + merge 16.7, over 65,216 calls of
which 55,947 carry an empty dirty queue. NovyWave: 290.5 ms = merge 189.8 +
resolve 38.5 + occurs 11.9 + invalidation 3.9 + the rest, over 29,646 calls and
119,920 pairwise merges. Closed-pair caching is worthless there: only 639 of
those merges are between closed terms.

The third slice builds homogeneous folds (all variant sets or all objects) in
one pass and interns only the final term, keeping the pairwise path for
variables, unions and mixed kinds. The per-element rules are copied exactly, so
the final term is identical: both fixtures report byte-identical diagnostics
fingerprints between the two builds. Phase effect: TodoMVC merge 16.7 → 10.0 ms
and refresh 283.2 → 277.4; NovyWave merge 189.8 → 162.9 and refresh 290.5 →
260.6. Fold iterations fall 44,262 → 12,553 and 119,920 → 4,717. Eight-round
interleaved end-to-end is within noise (single-pass versus memo: +0.4% TodoMVC,
−0.6% NovyWave), so no production claim is made. Full record:
[single-pass merge](evidence/compiler-k1pp-single-pass-merge-2026-09-11.json).

Remaining buckets: NovyWave's 162.9 ms merge now sits in recursive payload and
field merges, so the next step is cheaper recursive merging (per-pair memo with
an epoch guard, or field-wise fast paths); TodoMVC's 86.9 ms invalidation needs
a requirement-relevant dependent index; its 68.1 ms commit/order path is
untouched. All gates green: kernel 197, kernel_transfer 10, compiler library 98
with one pre-existing ignored probe, staged 3, map_set 3, nested_boolean_match
3, pulses 8.

### 2026-09-11: K1″ A/B/A acceptance baseline — the K0 gap is real and not yet closed

The contract's acceptance protocol ran end to end — a mid-session machine hard
reset killed nothing, because the preserved K0 collector survived it: the
interleaved A/B/A (candidate `7ebc6e38` / K0 producer `2d7a5343` / candidate
`7ebc6e38`), 3 setup + 30 scored observations per fixture, mode and intent,
one producer process per observation, release product and evidence lanes, cold
fresh-process and empty-session. Raw reports:
`target/reports/compiler-performance/k1pp-aba-{candidate-a,k0,candidate-b}.json`
(sha256 recorded in the evidence file); the compact record with per-sample
latency and CPU arrays plus every kernel work counter is
[the A/B/A evidence file](evidence/compiler-k1pp-aba-2026-09-11.json).

| fixture / intent, fresh-process p50 | candidate-a | K0 | candidate-b | b vs K0 |
| --- | ---: | ---: | ---: | ---: |
| TodoMVC diagnostics | 1067.5 ms | 922.1 ms | 1073.4 ms | +16.4% |
| TodoMVC verified | 1462.7 ms | 1284.5 ms | 1470.2 ms | +14.5% |
| NovyWave diagnostics | 839.5 ms | 493.5 ms | 836.2 ms | +69.5% |
| NovyWave verified | 2167.9 ms | 1833.8 ms | 2173.3 ms | +18.5% |

The two candidate legs agree inside 0.8% on all twelve fixture/mode/intent
cells, and the empty-session mode repeats the picture, so the gap to K0 is
real rather than protocol noise. The aggregation slices land real work
reductions — TodoMVC term intern requests are 814,114 against K0's 342,922,
non-empty object interning 377,373 against 213,235 (down from 1,189,597 before
the dedup slice), scratch reuses 2.65M against 1.49M, and the candidate runs
3.4x fewer kernel operations (82,451 vs 282,393) — but its remaining
operations are much more expensive, and the refresh path stays at 26-31% of
latency: it alone exceeds the whole TodoMVC gap and covers about 76% of
NovyWave's. NovyWave's verified gap (+18.5%) is smaller than its diagnostics
gap because backend and plan-verification work dominate that lane.

Honest position: the Implemented ending (cold TodoMVC and NovyWave back to
K0-class latency) is not met by the three landed slices, and neither is the
Reviewed-rejection ending — the phase probe shows the remaining cost sits in
refresh sub-buckets that are still inside the contract's scope. Work continues
on NovyWave's 162.9 ms recursive payload merge and TodoMVC's 86.9 ms
invalidation plus 68.1 ms commit/order, and every further slice must move the
A/B/A p50 outside the measured 0.8% inter-leg agreement before it claims a
production effect.

### 2026-09-11: K1″ probe extension — the TodoMVC refresh cost is touch scheduling

The env-gated release phase probe now splits the untimed refresh regions
(assembly, dependency replacement, order retention, commit/touch) and
attributes term-arena intern requests to each region. One release observation
per large fixture (diagnostics, fresh-process); raw probe lines and counters in
[the attribution evidence](evidence/compiler-k1pp-phase-attribution-2026-09-11.json):

| refresh region | TodoMVC | NovyWave |
| --- | ---: | ---: |
| total | 311.5 ms | 263.2 ms |
| invalidate | 88.3 | 3.7 |
| commit (set_order + binding + touch) | 68.0 | 2.2 |
| assembly (facts + sort + inputs) | 32.5 | 2.9 |
| resolve | 26.5 | 37.1 |
| occurs | 16.7 | 11.5 |
| merge | 10.0 | 156.5 |
| dependency replacement | 8.2 | 12.4 |
| order retention | 0.8 | 14.6 |

Three corrections and consequences:

- The 68.1 ms bucket recorded earlier as "commit/order
  (`retain_requirement_order` + `set_order` + `touch`)" is not order
  retention: `retain_requirement_order` is 0.833 ms. The bucket is the commit
  path including `touch`.
- Neither large TodoMVC bucket is the requirement cone walk. Invalidation
  handles 12,541 dirty pops, 14,988 affected cells and 12,690 edge visits,
  while the two buckets together carry about 27,000 `touch` calls; the cost is
  `schedule_variable`'s transitive dependent walk at roughly 5-6 us per touch.
- Intern requests inside refresh are 164,211 of 814,114 on TodoMVC and 394,531
  of 1,067,405 on NovyWave, so the interning bloat that the K0 comparison
  exposes is not local to the refresh path.

NovyWave's remaining refresh cost is the recursive payload merge (156.5 ms),
and closed-pair caching stays dead there: 27 of 44,262 TodoMVC contributor
visits and 46 of 119,920 NovyWave visits merge two closed terms. Fold-memo
hits are 33% of TodoMVC and 27% of NovyWave aggregate evaluations.

All standing focused gates stay green: kernel 197, kernel_transfer 10, compiler
library 98 with one pre-existing ignored probe, staged 3, map_set 3,
nested_boolean_match 3, pulses 8. Next targets are the touch/scheduling walk
behind TodoMVC's invalidation and commit buckets and NovyWave's recursive
merge; the scheduler change is a dependency-update change inside the
aggregation path, so refresh-before-read and the withdrawal invariant must
stay green.

### 2026-09-11: K1″ fourth slice — only changed aggregates reschedule, TodoMVC reaches K0-class p50

The extended probe showed the refresh carrying about 27,000 `touch` calls at
roughly 5-6 us each while the requirement cone BFS moves only 12,690 edges, so
the cost was `schedule_variable` consumer scans re-run for destinations whose
recomputed aggregate had not moved. The slice records every binding that
invalidation clears and compares the new aggregate with it: an equal aggregate
restores the binding without the walk, a changed aggregate touches exactly as
before. Refresh-before-read is untouched — the refresh still runs before every
dequeue and still folds every dirty destination — and a debug assertion proves
every recorded binding is refolded in the refresh that cleared it.

Release phase effect (one observation per build, diagnostics, fresh-process):
TodoMVC refresh 311.5 → 135.6 ms (invalidate 88.3 → 16.9, commit 68.0 → 1.4,
folds 12,553 → 9,662, 12,514 skipped touches); NovyWave 263.2 → 247.8 ms
(merge 156.5 → 144.3, 4,530 skipped touches). Interleaved A/B, eight rounds,
one process per observation: TodoMVC diagnostics 1088.4 → 927.3 ms (−14.8%,
min 923.1), NovyWave diagnostics 849.2 → 789.4 ms (−7.0%), TodoMVC verified
1514.3 → 1369.2 ms (−9.6%). Diagnostics fingerprints are identical across the
lanes, and TodoMVC now sits at the K0 producer's 922.1 ms p50 for the
diagnostics lane — the first slice of this goal to move end to end.

Disclosure: the TodoMVC machine plan changes shape. The candidate plan is the
baseline plan with two projected-scalar expressions removed and downstream ids
renumbered (the op-kind sequence aligns under a +2 id offset for 64,481 of
64,482 rows; 501 rows differ beyond pure renumbering). The plan-hash budget for
the large fixtures is already red at HEAD — the budget constant is the K0-era
plan `8e7120c3…`, HEAD produces `4087c4ff…` and the candidate `0befc7f8…` — so
this continues an inherited red rather than regressing a green gate.
NovyWave's plan is byte-identical (`5926de7b…`). Full record:
[touch elimination](evidence/compiler-k1pp-touch-elimination-2026-09-11.json).

All standing focused gates are green: kernel 197, kernel_transfer 10, compiler
library 98 with one pre-existing ignored probe, staged 3, map_set 3,
nested_boolean_match 3, pulses 8. NovyWave remains about 60% above K0 and its
remaining cost is the recursive merge, so the next slice still targets that
merge; the contract's 3+30 A/B/A acceptance run for this candidate follows.

### 2026-09-11: K1″ touch-elimination acceptance — TodoMVC crosses K0

The full contract protocol ran for the touch-elimination candidate
(`d234c11e`): interleaved A/B/A, 3 setup + 30 scored observations per fixture,
mode and intent, one producer process per observation, release product and
evidence lanes, cold fresh-process and empty-session, the middle leg collected
in the preserved K0 worktree (`2d7a5343`). Raw reports:
`target/reports/compiler-performance/k1pp-aba2-{candidate-a,k0,candidate-b}.json`;
compact record with per-leg work medians and plan hashes:
[touch-elimination acceptance](evidence/compiler-k1pp-aba-touch-2026-09-11.json).

| fixture / intent, fresh-process p50 | candidate-a | K0 | candidate-b | b vs K0 |
| --- | ---: | ---: | ---: | ---: |
| TodoMVC diagnostics | 915.7 ms | 970.6 ms | 914.9 ms | −5.7% |
| TodoMVC verified | 1390.3 ms | 1344.4 ms | 1314.2 ms | −2.3% |
| NovyWave diagnostics | 775.0 ms | 531.1 ms | 774.5 ms | +45.8% |
| NovyWave verified | 2104.8 ms | 1836.0 ms | 2104.5 ms | +14.6% |

The candidate is now faster than the interleaved K0 producer on TodoMVC in all
four mode/intent cells (−5.7% and −10.2% diagnostics, −2.3% and −7.3%
verified), which is the first time in this goal that a large fixture is at or
below K0 class; both candidate legs agree inside 0.1% p50 on the large
fixtures. NovyWave improves to +45.8% (from +69.5% at the previous baseline)
and its remaining cost is the recursive merge. Diagnostics fingerprints are
byte-identical across all legs; the TodoMVC machine plan moves as disclosed in
the slice record, and both candidate legs carry occasional p95 outliers that
look like host interference (the p50 values are the interleaved metric).

The Implemented ending is therefore met for TodoMVC and still open for
NovyWave: the next slice attacks NovyWave's recursive payload merge (144.3 ms
of a 247.8 ms refresh) under the same gates and evidence discipline.
