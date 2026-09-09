# K0 + K1 execution evidence — 2026-09-07

This is an execution log, not a replacement goal or acceptance contract.
The bounded [K0 + K1 goal](BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md)
remains active. Starting HEAD: `9901854231ffc248e5bd3db79b582e86bc57b71c`;
starting compiler implementation: `37a576b6`. No pushes are authorized.

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
