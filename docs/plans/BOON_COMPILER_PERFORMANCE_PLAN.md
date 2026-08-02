# Boon Cold-First Compiler Performance Plan

Date: 2026-08-02

Status: authoritative blocking implementation contract for compiler latency,
memory, invalidation, cancellation, and compiler-service ownership.

Under the combined order in [`steps.md`](steps.md), this plan is implemented
before the remaining native-recovery exit and before later language, formal,
packed-runtime, console, product, or game work. Documentation reconciliation is
the first slice; passing the cold compiler gates is the first implementation
exit.

## Purpose And Authority

Boon must feel suitable for interactive authoring and ordinary scripting even
when no previous compiler process, query database, or artifact cache exists.
Switching to a larger checked-in example must not expose graph explosion as a
seconds-long editor stall, and persistent-session optimizations must not hide a
slow source-to-verified-plan compiler.

This plan owns:

- compiler latency and peak-memory budgets;
- source-unit snapshots, compiler-session state, dependency invalidation, and
  cancellation;
- compiler phase instrumentation, work counters, benchmark protocol, and
  immutable compiler-artifact reuse;
- the internal representations needed to meet those budgets without changing
  Boon semantics;
- editor and preview integration with the one compiler service.

It does not own or weaken:

- public language, type, `OUT`, exact-value, `WHERE`, or persistence semantics;
- proof obligations, evidence acceptance, or the meaning of
  `ContractVerifiedProgram`;
- the public verified-artifact spine, packed software-runtime phases, native
  input/WGPU evidence, console/processor/FPGA behavior, or product acceptance;
- Boon Orchard or any other game implementation.

The owning language, formal, packed, persistence, native, and console plans
remain authoritative for those concerns. Where an older plan describes a
compiler representation or execution order that conflicts with the budgets
here, this plan owns the performance mechanism while preserving that plan's
semantics and acceptance meaning.

## Non-Negotiable Outcome

Cold source compilation is normative. Both of these modes must independently
pass with compiler caches disabled:

1. a fresh invocation of the prebuilt `boon` compiler process; and
2. an in-process request against a newly created, empty compiler database.

OS file caching may remain natural to the machine, but no prior parse tree,
typed graph, semantic graph, proof result, lowered plan, serialized artifact,
or compiler daemon state may be reused by either cold mode. Incremental state
and content-addressed artifacts are a second optimization layer; they cannot
be used to satisfy a cold gate.

The default and measured compiler is single-threaded. Parallel compilation is
permitted only after the single-threaded cold targets pass and only when it
improves a separately measured workload without increasing interactive
contention or making correctness depend on scheduling.

## Current Baseline

The current post-reboot debug-profile checkpoint is historical diagnostic
evidence, not an accepted performance ceiling:

| Fixture | Package / compiler-input lines | Source to `MachinePlan` | Peak RSS | Plan SHA-256 |
| --- | ---: | ---: | ---: | --- |
| Counter | 140 / 140 | 0.09 s | 29,992 KiB | `dc1fe51b659d1746a0b0b4ae2dcba21d50a9426499eb2bde28dbed988e6cfb08` |
| Physical TodoMVC | 3,576 / 3,576 | 2.02 s | 146,340 KiB | `c9a12cd0a1bcf748a20e3a072afa09d0f923c2c9dbd664f2343d343494404f96` |
| NovyWave | 11,994 / 11,923 | 20.68 s | 1,000,416 KiB | `4d3c284a9240cdc68c70aff7f30c570367e285cc1e8f823585900829bafd8ff7` |

NovyWave's package count includes its separate 71-line `BUILD.bn`; the compiler
input count names the source bundle actually passed to the Client compiler.

The traced NovyWave path currently attributes approximately 4.84 seconds to
typechecking, 2.84 seconds to semantic materialization/execution, 3.81 seconds
to callable-dependency-manifest construction, 9.51 seconds to the complete
semantic portion, and 0.81 seconds to backend work. `boon_verify` itself is not
the 3.81-second blocker; manifest discovery inside semantic construction is.

The editor currently pays avoidable latency in addition to those compiler
costs: a fixed 90 ms debounce precedes a whole-project parse/check/editor-
semantics pass, and preview publication performs another whole-project
`compile_machine_plan` pass. Parsing concatenates source units into one global
program, checker-owned caches die with each borrowed checker, and an in-flight
compile cannot be canceled.

These observations choose the first architectural work. They do not authorize
fixture-specific shortcuts, reduced diagnostics, skipped verification, longer
timeouts, or altered plan semantics.

## Performance Contract

### Cold Budgets

Each cold mode must pass the following single-threaded p95 limits with compiler
caches disabled. Every scored sample must remain below the listed peak-RSS
limit:

| Fixture | Complete checked diagnostics p95 | Verified runnable `MachinePlan` p95 | Peak RSS |
| --- | ---: | ---: | ---: |
| Counter | 10 ms | 50 ms | 32 MiB |
| Physical TodoMVC | 75 ms | 300 ms | 128 MiB |
| NovyWave | 250 ms | 1,000 ms | 384 MiB |

"Complete checked diagnostics" parses and checks the entire requested source
snapshot and returns every diagnostic required by the typechecking contract. It
is not a syntax-only, visible-range, or first-error response.

"Verified runnable `MachinePlan`" traverses the complete public artifact spine
through proof acceptance and backend lowering. A non-executable internal
semantic artifact cannot satisfy this budget.

### Warm Interaction Budgets

Against an already opened project:

- an edit's complete affected checked diagnostics finish within 16.7 ms p95,
  25 ms p99, and 33.4 ms maximum;
- a valid constant-dependency-cone edit reaches a replacement verified preview
  within 100 ms p95 and 200 ms maximum;
- lookup of an already loaded verified bundle completes within 1 ms;
- an example switch is acknowledged within 16.7 ms and reaches the final
  presented native frame within 100 ms p95 and 200 ms maximum;
- a generation superseded by a newer edit or switch stops consuming compiler
  work within 8 ms;
- no superseded diagnostic, proof, plan, or preview generation is published.

Compiler completion and native presentation are separate measurements. The
native pipeline remains responsible for proving the final presented frame; it
must not subtract compiler time or substitute a compiler acknowledgement for a
readback-proved presentation.

### Scaling Budgets

Checked-in synthetic fixtures independently scale call depth, call-site count,
contextual call sites, static branch count, source-unit count, and dependency-
cone size. After fixed setup overhead, doubling one controlled dimension may
increase the owning phase's counted work and allocation by at most 2.2 times.

Ordinary callable bodies are stored once. A pruned static branch creates no
semantic dependency, proof obligation, or backend node. A constant-cone edit
does not reparse, recheck, reseal, reverify, or relower an unrelated unit or
component. A timeout increase or a larger thread pool cannot satisfy a scaling
gate.

## Compiler Architecture

### Source And Checked Database

- Parse source units independently and retain real spans, source-unit
  revisions, and stable declaration identities across unrelated edits.
- Replace the borrowed `Checker<'a>` plus rebuilt checked-output pass with one
  owned compiler database. The checked representation is the sole structural
  owner consumed by later stages; parser semantic side channels and backend
  source-name rediscovery are forbidden.
- Intern repeated names, type terms, shapes, paths, and immutable values. Store
  hot entities in dense typed arrays or boxed slices and store graph edges in
  compact offset/edge arrays where traversal dominates mutation.
- Use snapshot-local dense IDs for compact execution while maintaining explicit
  maps to stable source-unit and declaration identities. Dense IDs are not
  persistence identities.
- Use one Boon-owned constraint/worklist engine with bounded iterative queues,
  union-find where appropriate, SCC/component indexes, bitsets, and generation-
  stamped scratch vectors. Salsa may later schedule invalidated compiler
  queries, but it is not the Boon type solver and is not required by this plan.
- Keep one compact checked-expression representation through the compiler
  spine. Debug trees, reverse-name maps, and report sidecars are constructed
  only for a request that asks for them and must not remain reachable from the
  normal executable artifact.

### Semantic Construction And Proof Boundary

The public executable spine remains:

```text
ParsedProgram
  -> CheckedProgram
  -> SemanticProgram
  -> ContractVerifiedProgram
  -> ErasedProgram
  -> MachinePlan
```

`ParsedSnapshot`, `CheckedSnapshot`, and `SemanticCore` may exist as private
compiler construction types. They are non-executable and cannot be accepted by
a backend or runtime. Only a sealed `SemanticProgram` with an exact callable-
dependency manifest may cross into verification, and only an accepted
`ContractVerifiedProgram` may cross into erasure and lowering.

Each ordinary callable definition is retained once. Contextual call sites use
small overlays that carry only scope-dependent bindings and effects; they do
not clone a complete body, checked tree, or proof graph. Static conditions are
evaluated and dead branches pruned before dependency-manifest, proof, and
backend graph construction.

Build the exact callable-dependency manifest once at semantic sealing. Give
components deterministic identities and Merkle-style parent identities so an
affected component can be resealed without rediscovering unrelated roots.
Distributed fixed-point discovery may operate on private semantic cores, but it
seals one complete `SemanticProgram` only after the fixed point stabilizes.

`boon_verify` remains the sole owner of canonical obligations, accepted proof
evidence, and `ContractVerifiedProgram`. Compiler invalidation may narrow work
only through a generic dependency rule whose result is equivalent to a full
verification. Cached proof evidence remains immutable, exactly keyed, and
revalidated according to the formal plan.

### Backend, Hashing, And Reports

- Lower directly from the verified compact representation without
  reconstructing source ownership or recursive semantic trees.
- Stream canonical hashing, plan serialization, and report serialization.
  Do not build a second full in-memory tree merely to hash or print it.
- Preserve deterministic artifact order and the three baseline `MachinePlan`
  hashes throughout this semantics-preserving plan. Any intentional future
  format change belongs to its owning plan and must update this invariant
  explicitly.
- Keep debug/provenance sidecars detachable and content-addressed. The runnable
  plan contains the compact identity required to locate them, not duplicated
  diagnostic trees.

### Compiler Service

Provide one compiler service with the following conceptual interface; exact
Rust ownership and error types follow the existing crate boundaries:

```text
open_project(SourceBundle) -> ProjectId
apply_update(ProjectId, UnitUpdate) -> Revision
request(ProjectId, Revision, CompileIntent, CancellationToken) -> CompileResult

CompileIntent = Diagnostics | VerifiedCheck | VerifiedPreview | Handoff
```

- `Diagnostics` produces complete checked diagnostics but no executable
  artifact.
- `VerifiedCheck` follows the mandatory semantic/proof boundary without
  requiring a preview or report sidecar.
- `VerifiedPreview` returns the verified runnable plan needed to replace the
  preview.
- `Handoff` additionally materializes requested reports, provenance, or
  distributable sidecars.

Editor feedback uses `Diagnostics`; `boon check` uses `VerifiedCheck`; preview
and run requests use `VerifiedPreview`; handoff/report commands use `Handoff`.
They share the same project revision and owned compiler database rather than
performing serial whole-project compilations.

Use one foreground compiler worker and at most one low-priority handoff/report
worker. Newer revisions cancel older foreground work. A preview keeps the last
verified runnable plan until its replacement verifies; invalid or canceled
source never replaces it. Cancellation checks occur at bounded worklist,
component, proof, and lowering boundaries sufficient to meet the 8 ms gate.

### Immutable Reuse

Add persistent content-addressed reuse only after both cold modes pass. Never
persist the mutable compiler session or runtime `Session`. Reusable artifacts
are immutable and exactly keyed by at least:

- compiler and artifact schema versions;
- complete source-bundle identity and relevant unit/component identities;
- language, verification, semantic-schema, and persistence-migration catalog
  digests;
- target/profile and compile intent;
- every solver, proof-policy, and backend option that can affect the result.

A missing, malformed, stale, partially written, or mismatched entry is rejected
closed and recomputed. Cache-hit measurements are reported separately and
never mixed into cold results.

## Implementation Order

### Phase 0: Documentation And Measurement Contract

1. Reconcile this plan into `steps.md`, `GOAL_PROMPT.md`, the active recovery
   plan, and the owning older plans without changing current architecture docs
   to claim unimplemented behavior.
2. Add a compiler budget manifest and revision-identified JSON report schema.
3. Add deterministic phase timers, work/allocation counters, cancellation
   counters, source and artifact digests, tree fingerprint, binary hash, and
   peak-RSS capture.
4. Establish the two cache-disabled cold runners and the warm edit/switch
   runner using prebuilt binaries.

Exit: the report exposes all named phases and identities, rejects stale or
cache-enabled evidence, and reproduces the current fixture artifacts.

### Phase 1: Cold Parse And Type Core

1. Introduce independent unit parsing and stable source/declaration identities.
2. Replace borrowed checker state and duplicate checked construction with the
   owned checked database.
3. Introduce compact interned terms, dense tables, dependency indexes, and one
   bounded solver/worklist path.
4. Delete superseded global sweeps, deep copies, parser semantic side channels,
   and backend name rediscovery as their replacements land.

Exit: both cold checked-diagnostics gates and their RSS/scaling gates pass with
complete unchanged diagnostics.

### Phase 2: Semantic Sealing And Verification

1. Store ordinary definitions once and add contextual overlays.
2. Prune static branches before graph expansion.
3. Build indexed semantic components and seal the exact callable manifest once.
4. Integrate component invalidation with canonical proof construction and
   accepted evidence without weakening verification.

Exit: semantic/proof work counters scale within budget, exact dependency
closures and negative proof cases pass, and no unsealed artifact is executable.

### Phase 3: Backend, Hash, And Memory Closure

1. Lower directly from compact verified tables.
2. Stream hashes and serialization and detach optional debug/report data.
3. Remove old recursive/duplicated representations after parity is proven.

Exit: both cold verified-runnable time and RSS gates pass for all fixtures, and
their plan hashes remain unchanged.

### Phase 4: Persistent Session And Editor Cutover

1. Implement the compiler-service interface, dependency-cone invalidation,
   revisioned requests, and bounded cancellation.
2. Remove the fixed editor debounce as a correctness/scheduling boundary and
   eliminate the second preview compile.
3. Route diagnostics, checking, preview, run, and handoff through the one
   service while retaining the last verified preview.

Exit: every warm latency, cancellation, latest-generation, and no-stale-
publication gate passes under edit and example-switch storms.

### Phase 5: Optional Artifact Reuse And Native Presentation

1. Add exactly keyed immutable artifact reuse and fail-closed corruption/stale
   entry handling.
2. Report cache hits independently from cold and warm compiler-session work.
3. Reconcile the compiler-completion and final-presented-frame measures with
   the native verifier; update the native budget only when the verifier proves
   the separate stages.

Exit: disabling or deleting the cache leaves every cold gate green, cache
negative cases pass, and the native verifier proves the loaded-switch
presentation budget without weakening its input or WGPU evidence.

### Phase 6: Final Acceptance

Run the complete protocol from one unchanged revision, audit the code for
duplicate compiler owners and compatibility paths, and then refresh every
downstream report invalidated by compiler changes. Old native, Wasm, proof,
packed, or product evidence is not relabeled as current.

## Measurement And Verification Protocol

- Build the optimized compiler once. Cargo/Rust build time is not compiler
  time. Run only one Cargo build or test suite at a time on the reference
  machine; repeat fixture measurements by invoking the built binary directly.
- Start one producer process for every observation in both cold modes. The
  `empty-session` producer constructs one new in-process `CompilerSession` and
  serves one request; setup samples never share allocator high-water state or
  process-local compiler state with scored samples.
- Record three unscored setup runs followed by 30 sequential scored runs for
  each cold mode and fixture. Do not flush the OS page cache.
- Report Linux process peak RSS in KiB, sampled immediately after the checked
  result or runnable `MachinePlan` exists and before plan-validation, pretty-
  serialization, hashing, or report allocation can raise the high-water mark.
- Report p50, p95, p99 where meaningful, maximum, peak RSS, phase work,
  allocations, cache status, cancellation latency, source digest, compiler
  revision, worktree fingerprint, binary hash, and artifact hash.
- Fail if the worktree fingerprint or binary hash differs from the revision
  named by the report, if a required phase is absent, if a cold run observes a
  compiler cache hit, or if samples were run concurrently.
- Use focused debug checks and small integration tests during implementation.
  Run optimized end-to-end measurements only for coherent milestone
  candidates; do not repeatedly rebuild all crates between local edits.
- Preserve complete malformed-source diagnostics, stable spans and identities,
  recursion rejection, exact dependency closure, proof failures, deterministic
  plans, migration identities, and last-good-preview behavior.
- Add invalidation tests proving unrelated units, callables, semantic
  components, obligations, and backend regions are not recomputed.
- Add cancellation races at parse, constraint, semantic, manifest, proof, and
  lowering boundaries. A canceled generation publishes nothing.
- Compare fresh-process and empty-database results byte-for-byte and compare
  incremental results against a clean full compile of the same revision.

## Clear End Condition

This plan is complete only when all of the following are true on one unchanged
revision:

- both cache-disabled cold modes pass every time, RSS, determinism, and scaling
  gate for Counter, physical TodoMVC, and NovyWave;
- all warm diagnostic, verified-preview, switch, presentation, cancellation,
  and latest-generation gates pass;
- the public verified-artifact spine and proof acceptance remain mandatory, and
  no internal snapshot/core or cached evidence can bypass them;
- one owned checked database and one compiler service replace the duplicate
  whole-project editor/preview paths;
- ordinary definitions are stored once, dead static branches create no later
  work, and unrelated dependency components are not recomputed;
- disabling all compiler caches leaves the cold gates green, while immutable
  cache corruption and mismatch cases fail closed;
- the three baseline fixture plans retain their exact SHA-256 values, all
  affected semantic/proof/compiler/migration tests pass, and fresh reports name
  the final worktree and binaries;
- superseded representations, global-sweep fallbacks, compatibility wrappers,
  duplicate compiler owners, and temporary fixture-specific diagnostics are
  deleted;
- downstream evidence invalidated by the compiler change is rerun rather than
  declared current by documentation.

Passing only a smoke test, increasing a timeout, warming a cache, running more
compilers concurrently, publishing partial diagnostics, skipping proof work,
or preserving the old path behind a fallback does not satisfy this plan.
