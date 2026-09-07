# Boon Compiler: Definition-Owned Work and Responsive Revisions

Updated: 2026-09-06. Status: active compiler architecture and execution contract.
Audited starting implementation: `37a576b6`; future work starts at the actual
current HEAD, preserving later valid checkpoints.

## Authority and Scope

This is the single active compiler cut sequence. It replaces the former
M0--M6 sequence and its whole-pipeline allocation prerequisites, not the landed
kernel or its semantics. Historical refactor/research documents are evidence,
not competing resumption instructions.

- [Performance contract](BOON_COMPILER_PERFORMANCE_PLAN.md) owns measurement,
  correctness-preserving acceptance and the full-program end condition.
- [budgets/compiler.toml](../../budgets/compiler.toml) is the sole executable
  compiler budget/protocol/oracle authority. Proposed harder targets below are
  not silently substituted for its gates.
- [2026-09-06 evidence](BOON_COMPILER_REASSESSMENT_2026_09_06.md) records the
  source audit, report identities, measurements, corrections and uncertainty.
- [Goal prompt](BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md) selects the
  next bounded tranche. It does not authorize every item in this roadmap.
- Language, exact values, type inference, persistence, formal verification and
  native GPU contracts keep their semantic authority. In particular,
  [the WHERE plan](BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md) owns proof meaning
  and feature rollout; this plan must neither weaken nor pretend to implement it.

The 2026-09-06 plan update changed documentation only. K0--K5 and F1 below
are not marked implemented by writing this plan. Live K0+K1 progress is recorded
in the [2026-09-07 execution log](BOON_COMPILER_K0_K1_EXECUTION_2026_09_07.md).
Do not resume an old attachment containing
"implement M0--M6 completely" or the retired unified-product goal.

## Decision

Keep Rust, the permanent dense kernel and the existing repository. Prioritize
Linux IDE/playground responsiveness. Change the unit of reusable computation
from invocation occurrences and whole-phase images to definition transfers,
compatible code variants and exact revision dependencies.

Maintain an automated kernel dependency firewall: only stable lower-level
models such as boon_contract, boon_data, boon_syntax, boon_checked,
boon_effect_schema and boon_document_model may be dependencies. No production
dependency on boon_typecheck, boon_semantic, boon_ir, boon_plan, boon_compiler,
parser implementation details or old owner DTOs may enter the kernel. Add no
crate until a measured one-way ownership/rebuild seam justifies it.

The immediate priorities are:

1. identify and remove repeated type-transfer evaluation across equivalent
   invocation contexts, starting with the measured TodoMVC amplification;
2. carry reusable definition code and occurrence-specific data through semantic
   construction and executable linking, deleting repeated reconstruction;
3. retain packed revision state for demands, errors and cancellation, then
   recompute only exact dirty definition/SCC cones;
4. finish direct packed input and consuming image construction;
5. retain only measured allocator/toolchain/parallel refinements.

Do not finish every string/vector cleanup before crossing to its consuming
phase. Publication-key cleanup is real but verified-only and not the next
independent milestone. Bundle it with deletion of the checked compatibility
owner that requires it.

## Current State, Not Historical Assumptions

The stored non-acceptance NovyWave results are approximately 500 ms diagnostics,
1.81 s verified output, 2.67 million diagnostics allocations and 7.73 million
verified allocations. Across earlier checkpoints, verified allocations fell
about 45% while verified time fell about 11%. This is directional history, not
an isolated A/B attribution or current clean-HEAD acceptance.

The largest verified phases remain checked construction (about 671 ms) and
semantics (773 ms), followed by backend (206 ms) and plan validation (66 ms).
Removing one of them cannot by itself meet a 40--85 ms cold target.

TodoMVC is the sharper kernel counterexample: 4,858 expressions, 10,939 stored
residual operations, 8,194 frames, 282,393 logical operations and 620,555
activations. Five module variants account for about half the logical work.
The operation bytes are ALREADY shared. Reuse of semantic transfer evaluation,
not another layer of shared instruction storage, is the next hypothesis.

Preserve and build on completed work:

- packed production definition/type facts and one checked topology;
- moved prepared payloads and test-gated rich DefinitionArtifact projection;
- lean public diagnostics and existing same-revision kernel demand promotion;
- shared residual modules and existing direct summaries;
- unit-native syntax and parent-linked frame infrastructure;
- the uninstrumented CLI product and exact mimalloc 3.5 integration.

Remaining gaps are explicitly mapped in the evidence record. Do not describe
its former 1.96-million cumulative ancestry counter as 1.96 million current
parent traversals. Do not claim the actual EditorDiagnostics-to-preview path
always checks twice: it can retain a rich checked result. The lean path lacks
shared packed promotion, and cross-revision reuse is still incomplete.

## Target Representation and Ownership

### Stable Identity, Local Storage

Use one logical SymbolId/PathId/packed-type authority across phases. Authored
text remains in immutable source snapshots; intern synthetic/cross-unit names
once; hot rows hold IDs and spans rather than owned names or recursive types.

One authority does not require one monolithic physical slab. Prefer immutable
unit/definition chunks, session-stable indexes and explicit relocations:

- stable external definitions/occurrences own persistence and request identity;
- dense coordinates and source spans are local to their revision/chunk;
- a TypeRef carries its definition/module scope; local variable ordinals never
  become global identities accidentally;
- preserve semantic object field order in equality, hashing and receipts;
- normalize local variables with touched-term remaps and generation stamps,
  not repeated recursive rich-type construction or whole-array clearing;
- maintain deterministic publication independently of hash-table iteration;
- never repack or renumber every unrelated chunk for a small edit.

Borrow references/slices inside passes; share immutable chunks at ownership
boundaries. A derived lookup table or CSR index is not a second semantic
producer. Eliminate duplicate derivation, not useful indexes by decree.

### Definition Transfers Versus Invocation Instances

A definition has three distinct products:

1. a parametric type/requirement transfer over formal inputs and exact semantic
   dependencies, including projections, modes and capture requirements;
2. compatible executable code, specialized only by facts affecting code or
   layout, such as execution domain, static selector/control shape and capability;
3. per-occurrence bindings, captures, state/resources and stable identity.

Code sharing does not merge state. Two HOLD instances can share instructions
while retaining distinct state cells. Conversely, equal currently resolved
result types are insufficient proof that two mutable inference contexts can
share a solver result. Late providers, backflow, alpha scope, captured formals,
whole/nested projections and recursive SCCs must remain explicit.

Keep sound generic residual equations for cases not expressible by a transfer;
these are part of the one kernel, not an old-checker fallback. Do not add a
second solver or a selectable legacy backend. Expand transfer support only
with a proved dependency basis and differential tests for the full supported
construct, never a named fixture/owner fast path.

Keep dense union-find/type-term operations, explicit capture-forward and
requirement-backflow equations, CSR reverse consumers and generation-stamped
deterministic work queues. Queue exhaustion establishes convergence; explicit
oscillation/cycle handling replaces arbitrary round limits. Do not reintroduce
provider-wide scans or separate duplicate body solves during transfer reuse.

### Normalized Facts and Thin Linking

DefinitionCode owns normalized checked, call, effect, state, resource,
execution and proof facts. Later phases consume typed views and relocations
rather than reconstructing source-shaped meaning or whole rich images.

Evaluation/currentness edges and proof/link-relocation edges have distinct
meanings and cycle rules. They may share physical records with tagged planes
and separate CSR views; one forced global graph is not the goal. A runtime
proof SCC must not automatically become an indivisible compiler invalidation
SCC.

Produce compatible plan code once; occurrences point to code through compact
frames. Remove occurrence IDs from code keys ONLY by representing their
semantic data explicitly elsewhere. Inlining is an optional measured runtime
optimization, not a prerequisite for producing a correct interactive preview.
Any deferred inlining/shared-code change must retain native runtime budgets.

### One Revision Authority and Consuming Seal

CompilerSession/CompilationDb retains immutable source units, packed solved
chunks, exact dependency spans, semantic-result fingerprints and currentness
receipts. Keep source/presentation changes separate from semantic changes.

- Cold revision zero and edits use the same algorithms, with no previous
  memo state in either normative cold mode.
- Diagnostics, editor projections and preview promote the same solved revision.
- Reparse changed units and recheck dirty definitions or explicit recursive
  SCCs; backdate dependents when their semantic input result is unchanged.
- A definition demand does not first finalize the whole project.
- Valid, invalid and incomplete edits retain usable facts; diagnostics do not
  trigger a second rich checker solve solely for presentation.
- Cooperative checks inside bounded work batches allow supersession, not just
  cancellation before entry or between multi-hundred-millisecond phases.
- Canceled generations publish nothing. Preserve last-good preview and exact
  source revision identities; reclaim superseded chunks with bounded retention.

Typed builders append and locally validate sections, compute compositional
receipts and link relocations. One independent dense cross-table integrity and
obligation audit remains mandatory before an opaque SealedExecutableImage can
reach the runtime. Exhaustive verification remains mandatory for untrusted
input. Rich checked/semantic/IR/legacy-plan exports are explicit projections,
not permanent alternate production compilers. Consumed export work is timed;
unrequested pretty JSON is not imposed on preview.

## Bugs, Missing Features and Cleanup Register

Statuses here mean observed missing behavior or a documented risk, not a new
claim that all correctness tests currently fail.

| ID | Work and owning cut | Required evidence |
| --- | --- | --- |
| B1 | Error/ordering-diagnostic path can rerun checking; remove it in K3, or pull its bounded fix forward if K1 touches the owner | invalid/incomplete/order-error parity; one solve per revision; stable spans |
| B2 | Lean diagnostics and preview lack retained packed promotion; K3 | demand-sequence counters; EditorDiagnostics parity; no duplicate semantic authority |
| B3 | Whole-project invalidation/finalization defeats definition demands; K3/K4 | exact dirty cones; private/public/unrelated/add/remove/rename revisions; no global repack |
| B4 | In-flight cancellation is not demonstrated inside long kernel/semantic work; K3 | live cancellation races at every long phase; measured stop latency; zero stale publication |
| B5 | CLI allocator choice is not integrated into the native compiler process; C1 | actual native product identity, latency, concurrent activity and bounded RSS soak |
| B6 | Hardcoded effective profile metadata and HEAD/diff freshness confound experiments; K0 | real profile/build-input identities; stale-binary and configuration-mismatch negative tests |
| B7 | Budget prose/manifest drift and missing compiler closure aggregate; K0 records policy, K5 implements final closure | no hidden gate relaxation; registered fail-closed aggregate and stale/missing-sidecar negatives |
| F1 | Authored WHERE is rejected, not a fast implemented feature | feature track below; nonzero source-generated obligations and success/failure/unknown tests |
| D1 | Rich checked publication keys, adapter/SOURCE ABI dependency, repeated finalization | K2/K4: delete the obsolete owner after every consumer has a packed route |
| D2 | Semantic core/manifest and backend reconstruct overlapping facts | K2/K5: one definition fact producer, one compatible code body, consuming link/seal |
| D3 | Historical resumption instructions and arbitrary allocation prerequisites | this documentation update retires their execution authority; git preserves history |

Do not invent a Boon-source workaround, alter NoElement semantics, relax a
verifier, add a timeout, or suppress diagnostics to close an item.

## Cut Sequence and Bounded Exits

K0 is a small prerequisite. K1 is the next default implementation objective.
K2 is the next major cold-output cut. K3's bounded promotion/error/cancellation
work may be pulled forward once its ownership boundary is available; warm
responsiveness is not held hostage to the final cold stretch target. K4/K5
complete the same architecture. A goal names its allowed cuts explicitly.

### K0 — Baseline and Attribution Integrity

Read the live implementation, preserve existing work and record HEAD, branch,
dirty state, producer hashes, allocator, toolchain and effective profile. Build
one current release product/evidence pair. Fix only identity/counter defects
needed to evaluate K1 now; do not build every future harness before K1.

Separate clocks and endpoints: parsing/checking, checked publication,
semantics/proof preparation, source WHERE discharge, independent image audit,
backend, optional export, runtime installation and first presented frame.
Report obligation counts/status: unavailable WHERE is not a measured zero-cost
proof engine. Budget values come from the manifest; retain tighter memory
ambitions separately rather than silently increasing or reducing a gate.

Attribute TodoMVC's ranked residual variants to source definitions and record
first summary rejection, new-frame reason, actual variable/mode tuples,
equivalent semantic input tuples and dependency epochs. Distinguish physical
operations, logical operations, solver activations and summary-node visits.

Exit: current reproducible baseline, explicit counter meanings, generic owner
attribution and a reviewed semantic reuse hypothesis. K0 alone is not a speed
milestone. Missing final closure tooling is recorded, not repeatedly invoked.

### K1 — Reuse Definition Type Transfers Without Merging Instances

Implement the largest sound generic transfer/reuse cut exposed by K0. Begin
with the dominant unsupported/repeated construct, not an owner-ID shortcut.
Keep parametric requirements and their dependencies explicit; reuse only on a
sound semantic basis and re-activate consumers when that basis changes.

The intended deletion is the duplicate per-invocation evaluation/substitution
machinery for that supported construct. Existing shared module bytes remain.
A prototype must not survive as a permanent extra cache around unchanged
occurrence replay. Cold within-request common-work sharing is allowed; prior-
request caching cannot establish this cold improvement.

Before implementation, freeze the target work counters and comparison cohort.
Planning hypotheses are at least 25% less targeted TodoMVC expanded evaluation
and a clearly repeatable end-to-end diagnostics win; they are not measured
promises or replacement absolute budgets. Count work moved into summaries,
key construction and validation too.

Required checkpoint evidence:

- source-level reproduction and generic scaling fixture, independent of
  TodoMVC names, matching the attributed amplification;
- differential diagnostics, expression/declaration flows, calls, captures,
  effects, state/resources and exact dependency behavior;
- late providers, requirement backflow, generic HOLD instance isolation,
  alpha scope, nested/empty/disappearing projections, PASSED, pattern reads,
  recursive calls/SCCs, long WHEN chains, collections and ABI tests as relevant;
- reduced actual owning work and a latency win outside run-to-run noise on
  fresh single-threaded/cache-disabled producers; no shifted hidden work;
- Counter/NovyWave and invalid/stateful holdouts retain correctness, budgets
  already passing, and no material time/RSS regression against the fresh baseline;
- one independent read-only semantic/measurement review, exact deletion audit
  and an authorized local checkpoint; no push.

K1 has two honest decision outcomes: a verified production cut, or a reviewed
rejection of the reuse hypothesis backed by source-level counterexamples and
measured attribution. For rejection, remove only the agent's experimental
changes, preserve user work, record the next owning architectural decision and
request the next scope. One failed experiment is not evidence of impossibility.
Do not describe that outcome as implemented optimization or full performance
completion. The bounded prompt states this decision boundary explicitly.

### K2 — Shared Definition Code Through Semantic and Executable Consumers

Separate code compatibility from occurrence state/capture/resource identity.
Represent contextual/stateful code with formal frame slots and relocations
instead of rejecting sharing for whole call chains. Re-key compatible variants
by genuine code/layout dependencies, not incidental call IDs or cloned maps.

Carry normalized facts through one representative cross-phase vertical slice,
then cover every owner kind before flag-day consumer cutover. Remove duplicate
execution/core/manifest reconstruction for the converted ownership domain;
finish the whole domain, rather than leaving a packed sidecar plus rich owner.

Exit: independent generic repeated-call/stateful fixtures show code-body counts
proportional to compatible variants, occurrence data proportional to instances,
and materially lower NovyWave semantic/backend time. Distinct state, event
ordering, captures, migration identity and runtime performance remain exact.
Delete superseded production owners after parity, not by retaining a legacy
mode. The current ~219 ms core/receipts and ~104 ms manifest subspans are
attribution hints, not additive forecasts of guaranteed savings.

### K3 — Responsive Revision Demands, Errors and Cancellation

Retain the existing KernelSession solved facts in the facade; expose lazy rich
editor projections. Fix B1/B2 before calling this demand promotion complete.
Then make cancellation cooperate inside measured long work batches and extend
to exact public/private result dependencies and backdating across revisions.

This is not a cache of one whole checked project. Scope the work into coherent
subcuts: same-revision promotion, error presentation, bounded cancellation,
then inter-revision definition/SCC invalidation. Do not claim the first subcut
implements all four. Coordinate per-definition storage with K4 so warm reuse
never triggers whole-project type/code finalization.

Exit: clean-full parity for each valid/invalid edit and intent order, each dirty
owner solved once, unchanged cones untouched, generation-safe publication,
bounded retained revisions, real in-flight cancellation and separate warm
compiler/native presentation evidence. Existing warm manifest budgets remain
required for the full program; stronger targets are hypotheses until tested.

### K4 — Direct Packed Input and Definition-Local Finalization

Make compile-mode parsing emit immutable unit-local packed syntax and source
spans with construction-owned indexes. Name/project resolution is an overlay,
not AST rewriting, repeated validation or rich prepared-owner duplication.
Retain recovery and editor navigation as lazy projections over the same source.

Remove production PreparedKernelProjectProjection, old SOURCE ABI checker
calls and rich checked reconstruction after their unique facts have an owner
in the lower-level model/kernel. Remove boon_typecheck only when no legitimate
producer remains; enforce the kernel dependency firewall automatically.

Finalize only demanded definition chunks; import/remap only touched terms and
keep stable identities across edits. Bundle publication-key/string/route cleanup
with this consumer deletion. Exit: one syntax/fact authority, no production
compatibility replay, no unrelated finalization and measured parse/publication
and warm-cone improvements without diagnostic regressions.

### K5 — Consuming Image, Full Verification and Product Closure

Link compatible definition code, assign final image IDs once, validate local
sections as constructed and transfer columns into an opaque executable image.
Eliminate full-plan clones, duplicate rich canonicalization and retrospective
compaction on the consumed preview path. Keep the independent packed audit
and exhaustive untrusted-input verification. Version internal formats when
needed while proving stable contracts, determinism and migration/restart parity.

Implement the currently missing manifest-backed compiler-performance closure
command and three-review sidecars; do not label planned tooling available.
Refresh all affected compiler/session/runtime evidence and applicable native
handoff reports. Unrelated native renderer/compositor/console work requires its
own scope. Exit: the full performance contract, not merely a successful K1--K4
checkpoint. Future WHERE claims additionally require F1's real feature evidence.

## F1 — Authored WHERE Feature Track

Current production syntax rejects WHERE; verify_explicit_contracts binds the
bootstrap artifact/dependency contract with no source-generated obligations.
Preserve that fail-closed behavior until a supported feature slice exists.

Track implementation explicitly under the formal plan, not as a sub-millisecond
feature already completed. The next formal vertical slice must establish:

1. both specified application-facing forms, complete token consumption, source
   spans/recovery and negative parser cases;
2. checked contract ownership, lexical/PASSED substitution and required
   obligation discovery from source and imported requirements;
3. nonempty completeness-checked manifests, supported pure-value/function
   discharge and explicit unsupported/unknown failure with no executable bypass;
4. header caller checks, returned guarantees, successful and failing proofs,
   missing/extra/tampered evidence, deterministic diagnostics and proof erasure;
5. real nonzero-obligation wall/work/memory measures, followed by the formal
   plan's HOLD, list, migration and tooling phases rather than declaring V1 done
   after the pure slice.

Check the formal plan's language-foundation prerequisites before implementation.
Record missing prerequisites; do not silently add public booleans, change exact
NUMBER, introduce runtime assertion fallbacks or weaken assurance meaning.
K1 does not authorize the full feature. A later explicitly selected F1 goal
must own it. Proof complexity for arbitrary future obligations is not bounded
by today's empty-obligation NovyWave timer; label cold/warm proof workloads and
resource-limit outcomes independently, and never call pending proof verified.

## Configuration and Allocation Policy

C1 is a bounded optional product experiment, not another prerequisite marathon.
After K0 identity fixes, compare one factor at a time using the actual consumed
product, including the native in-process compiler:

- retain exact mimalloc 3.5 and a System comparison lane; do not add another
  allocator override or mix FFI allocation domains;
- test cross-crate ThinLTO, then codegen-unit changes, then local
  target-cpu=native; measure producer build cost separately from Boon runtime;
- A/B toolchain updates rather than assume newer stable/nightly is faster;
- train stable instrumentation PGO after architecture settles, using separate
  representative training and holdout projects, edits and invalid inputs;
- consider BOLT/sample PGO only with supported exact tools and evidence of
  instruction-layout pressure; discover current flags instead of copying dated
  nightly syntax;
- consider portable SIMD only for a measured flat scan/hash/bitmap loop;
  allocator_api only for a proved phase/session allocator boundary. Neither
  requires making every compiler container allocator-generic;
- async/type-system ergonomics are not a demonstrated hot-path opportunity.

Retain options only for reproducible product wins and semantic/operational
parity. A nightly default needs at least a five-percent representative holdout
runtime win or a specifically justified architecture-enabling need with its
own evidence; faster Rust workspace compilation alone is insufficient. Do not
change panic/unwind policy just for flags without reviewing IDE failure isolation.

Allocation choices, in order: delete unnecessary ownership; borrow; intern IDs;
share immutable chunks; use dense columns; use scratch arenas where lifetime
and Drop behavior are explicit. Persistent maps can suit snapshot indexes but
not replace Vec columns mechanically. Small strings/Cow belong at remaining
text boundaries when profiling supports them. Avoid per-row reference counting,
one project-wide bump lifetime and exact-sizing passes that cost more than
amortized bounded growth. Measure calls, bytes, capacity, retained revisions,
peak RSS and work; no universal 2,000/5,000-allocation gate is justified yet.

Optional bounded parallelism may be investigated after deterministic
per-definition/SCC dependencies and per-worker evidence exist, without waiting
for the unproven final cold stretch envelope. It is a separately labeled
product; the normative single-thread cold gates remain unchanged. Do not add
threads around duplicated passes or count warming/precompiled libraries as
cold full-source compilation. Retain dependency-package reuse as an explicitly
separate product option. A bytecode/typed-graph preview tier or annotations as
inference firewalls require separate design and runtime/language evidence,
not an immediate second backend or language restriction for benchmarks.

## Targets, Measurement and Goal Discipline

The manifest contains current executable acceptance budgets. The older prose
384 MiB NovyWave memory target and the proposed 128 MiB target are not its
current 512 MiB gate. Record all observed memory; any future normative budget
change must be explicit, justified and versioned, never auto-derived from a
candidate. Likewise 100 ms warm preview acceptance and 50 ms stretch are not
the same target.

Retain these as research envelopes, not forecasts already established by code:

| Product | Unvalidated envelope |
| --- | ---: |
| Warm complete diagnostics, ordinary local edit | 5--15 ms p50, 16.7 ms p95 |
| Warm verified preview, unchanged public interface | 15--40 ms p50, 50 ms p95 |
| Cold in-process diagnostics, empty database/resident bytes | 20--45 ms p50, 30--70 ms p95 |
| Cold in-process verified image | 40--85 ms p50, 60--120 ms p95; sub-100 ms p95 stretch |

The cold verified envelope requires about 21--45x improvement from the stored
1.81-second result. Warm small-cone responsiveness is a more credible near-term
route. No allocation count, allocator percentage, compiler flag or single owner
removal proves either endpoint. Do not add overlapping speedup estimates.

Use the performance contract's edit/preflight/acceptance ladder. Run one Cargo
process at a time with --jobs 2; no parallel benchmark producers. Rebuild fresh
release product/evidence binaries for coherent cuts, not every tiny edit. Use
occasional debug NovyWave samples for development feedback, labeled separately.
All candidates include Counter, TodoMVC, NovyWave, invalid/incomplete code and
the owner's generic/stateful holdouts. Changed-owner actual work counters and
whole-product wall time both matter.

Each checkpoint records: targeted owner/deletion, code and binary identity,
commands, relevant tests, measured work/time/RSS/allocations, surviving inherited
failures, review findings and the next decision. Keep this status concise; do
not append another thousand-line historical imperative log. Full program
acceptance still needs three setup plus 30 scored observations in both cold
modes, interaction/scaling closure and three independent final reviews.

A bounded K1 goal can finish while later whole-program latency gates remain
red; it cannot claim those gates passed. If the same blocker repeats, or a
hypothesis misses by more than 25%, re-evaluate ownership and change the plan
before further tuning. A low-impact string/map change is not the substitute.
Do not mark implementation complete for documentation, unsupported claims or
one favorable sample. Commit only authorized coherent checkpoints with focused
passing correctness gates and honest performance status. Never push without an
explicit request, and never expand into unrelated product plans automatically.
