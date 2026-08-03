# Boon Compiler Architecture Refactor Plan

Date: 2026-08-03

Status: active high-leverage execution map, reconciled after activation/effect
checkpoint `32bcf40`, subordinate to
[`BOON_COMPILER_PERFORMANCE_PLAN.md`](BOON_COMPILER_PERFORMANCE_PLAN.md). The
performance plan owns all latency, memory, correctness, and final acceptance
gates. This file owns the architectural sequence first chosen after checkpoint
`968c56a` and strengthened by the post-`32bcf40` source/primary-reference
research below; it does not create a second set of weaker exits.

## Why This Refactor Exists

The retained-definition work removed the first large semantic multiplier, but
the remaining verified compiler is still structurally far from the NovyWave
budget. A live directional run of the existing release producer after the
checkpoint reported:

| Stage or inventory | Current directional result |
| --- | ---: |
| complete checked diagnostics | 214.68 ms, 65,184 KiB |
| parse / typecheck | 56.99 / 157.68 ms |
| verified request | 4,491.72 ms, 317,848 KiB |
| semantic elaboration | 3,803.74 ms |
| callable dependency manifest | 2,350.35 ms |
| manifest records / coverage rows | 159,652 / 208,982 |
| proof graph | 160,316 nodes / 512,314 edges |
| proof SCCs | 128,796, including 1,314 cyclic SCCs |
| semantic owners | 663 callables plus one program root |
| backend / plan validation / serialization | 413.93 / 103.05 / 95.58 ms |
| cumulative allocation | 10,924,939 calls / 1,552,286,611 bytes |

The sample is architecture evidence, not acceptance evidence: the complete
revision/binary cross-binding and scored p95 protocol were not regenerated.
Its phase spans also need to become explicitly non-overlapping before phase
budgets can be added safely. The cardinalities and dominant owner are
nevertheless deterministic across the adjacent traces.

The trace also identifies why one sealing architecture can remove several
costs at once. Post-hoc proof inventory spends about 341.69 ms rescanning the
checked program, 455.87 ms rescanning execution, and 258.97 ms rescanning the
lowering contract; it then spends 356.87 ms hashing coverage and 638.61 ms on
the record graph. `semantic_program_digest` separately spends 109.24 ms hashing
the canonical core. Execution validation is repeated on both sides of resource
construction for about 63.20 ms. These are overlapping ownership passes, not
independent language features.

The warm architecture has a similarly concrete boundary. The same diagnostics
sample has eight units and 17,721 expressions, yet canonical assembly rebases
115,683 nodes and parser validation records 1,049,157 visits. Although the
parser exposes cacheable unit-local artifacts, `CompilerSession` retains none
of them and clears its one checked result after any changed unit. Stable two-
level identities and projection currentness are therefore required; a cache
around the final global `ParsedProgram` would preserve the invalidation
problem.

Removing the 2.35-second manifest without changing anything else would still
leave roughly 2.14 seconds. Packing its existing records more tightly is
therefore insufficient. The compiler needs fewer production authorities,
fewer representation transitions, and a single sealing path shared by cold
proof, backend lowering, and later incremental currentness.

## Target Shape

```text
one CompilationDb, used in one-shot cold and persistent warm modes
  revisioned source-unit snapshots
    -> complete checked-diagnostic requests
    -> stable owner/projection requests
         typed semantic rows + row receipts + exact dependency spans
         changed_at + verified_at + result fingerprints
    -> demand-collected retained definitions and compact invocation frames
    -> one proof/plan seal over the same request graph
    -> one immutable runnable MachinePlan image
```

The executable path has one authority at each arrow. Exhaustive dependency
records, flattened specialized semantic trees, and debug JSON are materialized
only by explicit test/debug requests. Complete diagnostic projections belong
only to a diagnostics request. Construction IR may survive a bounded internal
distributed link or explicit serialized-artifact request, but is not retained
by an ordinary runtime artifact.

## Post-Activation Architecture Research (`32bcf40`)

The activation/effect checkpoint closes the generic construction boundary; it
does not make the full NovyWave migration/restart oracle green. A fresh
source-level audit after that checkpoint exposes four larger multipliers that
must shape the compiler cut before another local optimization:

| Live seam | Current production shape | Required owner-level cut |
| --- | --- | --- |
| cold proof versus later warm invalidation | `dependency_manifest.rs` builds and drops the rich V3 record/coverage graph, while `CompilerSession` still has only one project-wide checked slot and would otherwise need a second exact dependency graph | one typed owner/projection request graph is simultaneously the construction scheduler, proof receipt graph, and revision-currentness graph |
| retained semantics versus expanded plan | retained ordinary definitions reach `ErasedProgram`, but `DocumentCompiler::compile_user_call` allocates a cache scope and recompiles the function root for each exact argument/overlay; `DocumentFunction` currently owns materialization bodies, not ordinary function bodies | retain executable definitions through `MachinePlan`; publish compact parameter/type/owner/resource/render invocation frames and evaluate verified plan functions without an AST fallback |
| construction products versus published artifact | `SemanticProgram` simultaneously owns the checked program, OUT, execution, resource, reactive, lowering, view, storage, and memory graphs plus canonical core and proof; `CompiledMachinePlanFromSource` later publishes both complete `ErasedProgram` and `MachinePlan` although normal hosts consume the plan | construction tables are consumed/dropped as their receipts seal; normal verified publication carries the runnable plan, source/stable digests, and profile only; IR/semantic/debug products require an explicit debug intent |
| plan construction versus publication | typed-list fingerprint refresh clones the complete `MachinePlan`, then reachability compaction, validation, hashing, and pretty-JSON measurement traverse large arenas again | one builder assigns final IDs, fingerprints rows, verifies local invariants, compacts once, and streams the canonical plan digest/image; pretty JSON remains an explicit diagnostic view |

The implementation concentration reinforces the ownership problem rather than
justifying file-local tuning: the checked, semantic/proof, IR, machine-backend,
and document-backend roots total about 79.9 kLOC; the audited files contain
hundreds of explicit clones and many independently owned maps. Line and clone
counts are not acceptance metrics, but they show why another isolated
container edit cannot remove the observed multi-second traversal multiplier.

The key reconciliation is to combine the old proof-seal and future
incremental-session tranches. Introduce a small typed `CompilationDb` kernel
now, not after cold compilation is optimized:

- request keys are stable `{owner, projection}` or
  `{definition, invocation-overlay}` identities, never expression-level rich
  proof records or revision-local dense IDs;
- each request memo owns `changed_at`, `verified_at`, input/result
  fingerprints, a compact exact dependency span, a semantic-row receipt span,
  and work counters;
- rows inside one owner/projection remain dense column entries rather than
  individual general-purpose queries; the request graph must stay near the
  declared owner/projection population, with typed compact exceptions for
  irreducible row-level cycles;
- the one-shot cold compiler uses the same database at revision zero without
  disk-cache lookup or a second scheduler; the persistent service retains the
  graph and backdates unchanged results;
- complete diagnostics request all required checked facts for the current
  revision, while a verified-plan request demand-collects only reachable
  semantic/plan projections from published outputs, effects, persistence,
  views, distributed contracts, and migration roots;
- owner/projection row receipts fold directly into V4 proof roots. The
  test-only V3 materializer independently reconstructs exhaustive facts from
  those receipts; production never builds V3 and never builds a separate warm
  dependency authority.

This is intentionally narrower than importing Salsa or recreating rustc's
query engine. Rustc demonstrates stable fingerprints and projection queries as
invalidation firewalls, Salsa demonstrates result backdating, Swift's request
evaluator demonstrates replacing coarse eager validation with immutable lazy
requests, and ThinLTO demonstrates doing whole-program decisions from a compact
summary before materializing bodies. They support one Boon-owned typed request
graph, not another framework or semantic authority. TypeScript 7 demonstrates
the scale available from a native shared-memory foundation, but Boon is already
native Rust: parallelism is allowed only after the graph removes duplicate
work and the two-job machine budget remains explicit.

Additional primary references:

- [Swift 5.2 centralized request evaluation and immutable declarations](https://www.swift.org/blog/swift-5.2-released/)
- [Swift 5.3 fine-grained dependency and request caching](https://www.swift.org/blog/swift-5.3-released/)
- [TypeScript 7 native compiler announcement](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)

The first implementation is a vertical owner slice, not a database shell
under every old graph. Move the program root plus one representative ordinary
callable through checked projection, semantic rows/receipts, V3 test
materialization, demand collection, retained plan function/frame, verifier, and
runtime. Delete each superseded production owner for that slice before
expanding to the next domain. A database facade that still constructs all nine
rich graphs and the V3 inventory is rejected.

Use these non-overlapping final stage envelopes for NovyWave. They divide the
existing 1,000 ms gate; they do not replace its scored end-to-end p95:

| Non-overlapping envelope | Maximum p95 |
| --- | ---: |
| parse plus complete checked diagnostics | 250 ms |
| semantic construction, exact proof, and verification | 350 ms |
| IR plus backend construction and plan seal/validation | 300 ms |
| deterministic hashing/serialization and publication | 100 ms |

If instrumentation shows that work belongs to a different envelope, move it
without double counting and keep the 1,000 ms total. Do not improve a stage by
silently charging its work to another stage.

## Architectural Decisions

### 1. Activation Is A First-Class Output, Not An Empty Mount

Checkpoint `32bcf40` implements this flag-day boundary. `MachineBuildTask`,
runtime construction, persistent native/web hosts, distributed runtimes, and
product consumers now return and route the exact initial activation turn;
synthetic `LiveRuntime::mount` is deleted. Activation/reset persistence applies
authority and startup outbox changes atomically. Unleased producer-template
work is pruned before host commitment, and startup deltas cross the same public
boundary normalization as ordinary turns.

The landed flag-day build result is one activation product:

```text
MachineActivation { machine, initial_turn }
  -> RuntimeActivation { runtime, initial_runtime_turn }
```

The initial turn carries startup document patches, transient effects,
cancellations, credits, durable/outbox changes, distributed invocations,
metrics, and the exact activation identity. Construction, restore, recovery,
migration, and artifact replacement all use this route. The synthetic empty
`mount` authority is deleted; do not add a second startup effect replay path.

The differential behavior harness now records one real-host effect transcript
and replays it into the other candidate. It compares stable effect intent,
owner, target origin, delivery, cancellation, credit, completion order,
stream-result order, terminality, and outcome while remapping only launch-local
call IDs. Exact turn/revision equality follows from shared external causality.
Only the already documented store-local epoch may be normalized.

Focused transcript, persistence, wasm-target, producer-pruning, and workspace
checks are green at the checkpoint. The complete real-host NovyWave
migration/restart/provenance/negative oracle remains a required bounded closure
before V4 proof migration; this tranche is correctness evidence, not a
compiler-speed claim.

### 2. Make Owner/Projection Requests The Construction And Proof Unit

The current V3 pipeline walks every checked and semantic product after those
products have already validated themselves. It allocates rich dependency and
coverage objects, resolves entity references, builds an entity-level graph,
hashes coverage and SCC closures, retains only compact callable/root digests,
and drops the exhaustive inventory.

Replace that production shape with the `CompilationDb` owner/projection request
and receipt kernel shared by checked, semantic, proof, and plan builders:

- every semantic table row is inserted once with its domain, stable local key,
  exact owner, canonical payload fingerprint, and referenced rows/owners;
- a dense coverage bitset proves that every executable row is assigned exactly
  once and no unknown row is claimed;
- each owner folds its canonically ordered row receipts into one local Merkle
  root;
- stable owner-local projection roots expose independently referenced subsets
  such as public shape, body implementation, effect/resource, storage, view,
  and migration facts without reopening the full row inventory;
- references within one proof region require no graph edge because its local
  root commits every member; inter-region references use the narrowest exact
  projection root;
- SCC condensation and transitive implementation roots run over the owner/
  projection summary graph rather than automatically treating every field
  record as a graph node;
- component, public-shape, source, checked-program, classifier, and owner roots
  are combined into one versioned semantic certificate.
- the exact same inter-request edges and result fingerprints own
  `changed_at`/`verified_at` currentness and unchanged-result backdating; no
  later compiler-session dependency graph may duplicate proof ownership.

The current 664 callable/root owners are the starting aggregation boundary, not
permission to coarsen dependency semantics. If folding a whole owner would add
an unrelated dependency, introduce a stable projection root. If an exceptional
cyclic region cannot yet be summarized without losing exactness, retain its
row identities in a compact typed CSR—not rich cloned records—until a proven
projection replaces it. Report owner, projection, exceptional-row, and edge
counts. The warm invalidation tests must reject both missing and unnecessarily
broadened dependency cones.

This is a flag-day V4 proof schema, not a compatibility wrapper around V3. The
test-only exhaustive V3 materializer remains temporarily as an independent
oracle. It must reconstruct the current dependency/coverage facts from the
same sealed semantic database and prove:

- every V3 subject has exactly one corresponding V4 row receipt;
- owner, projection, and cross-region dependency classifications agree;
- mutations of every payload domain invalidate the owning implementation root;
- unrelated-owner mutations do not invalidate another owner root;
- cycles, missing receipts, duplicate ownership, bad references, and a stale
  outer digest fail closed;
- Counter, TodoMVC, and NovyWave retained/flat behavior and stable contracts
  remain equal.

Once the controlled proof migration passes, production must allocate no
`SemanticDependencyRecordV1` or `SemanticDependencyCoverageV1` inventory. The
test materializer cannot become a production fallback. Directional exit:
manifest/proof work is at most 250 ms on NovyWave, the production graph is
bounded by declared owner/projection regions plus measured compact exceptions
rather than rich serialized-field cardinality, and end-to-end time/RSS both
improve. Final acceptance still uses the performance plan's complete protocol.

### 3. One Sealed Compilation Database, Not Nine Rich Graph Authorities

`SemanticProgram` currently retains the complete checked program, resolved OUT
graph, execution, resource, reactive, lowering, view, storage, and memory
graphs, a canonical core, and the proof manifest simultaneously. Several
builders derive maps, validate, serialize/hash, and later rescan overlapping
rows. IR ultimately consumes only the canonical core and bound digests;
verification additionally reads a narrow reactive projection.

Introduce mutable revision construction and immutable sealed views inside the
one `CompilationDb`:

- definitions, calls, occurrences, resources, reactions, storage, views, and
  memory use typed dense columns with stable owner/local keys;
- one canonical edge arena and shared indexes replace component-local maps
  that describe the same relationship;
- component APIs are read-only typed projections, not separately owned DTO
  graphs;
- row fingerprints are computed once at insertion/seal and feed proof,
  semantic identity, incremental backdating, and plan fingerprints;
- validation is construction-local plus one cross-table seal audit; repeated
  whole-artifact validation/hashing is removed from the hot handoff;
- checked and construction-only state is dropped as soon as diagnostics,
  semantic receipts, and required debug/source metadata have been sealed;
- debug serializers materialize from the immutable database on explicit
  request.

Migrate one component domain at a time behind exact projection/materializer
parity. Delete its prior production graph/index owner in the same tranche. A
new database layered underneath all existing rich graphs without deleting
them is a regression, not a checkpoint.

Demand-driven `PlanReachabilityCollector` belongs at this boundary. Starting from
published roots, effects, storage, views, and migration contracts, it traverses
definition-plus-invocation-overlay keys and emits only reachable retained plan
definitions plus compact invocation frames. It must not rebuild a specialized
semantic tree. Static-pruned branches produce no instance or proof receipt.

Directional exit: semantic plus proof/verification fits its 350 ms envelope,
the sealed artifact retains no superseded rich graph, and live bytes,
expression visits, hashes, stable contracts, and negative proof tests are all
reported.

### 4. Lower And Seal `MachinePlan` Once

The backend currently constructs a large mutable plan, clones the complete
plan to refresh typed-list view fingerprints, compacts, validates, hashes, and
serializes through distinct traversals. Replace this with one
`MachinePlanBuilder`:

- accept only demand-collected verified instance rows;
- retain each ordinary executable function body once and encode call-specific
  parameter sources, type substitutions, PASSED values, owner/resource/effect
  coordinates, and render context in compact invocation frames; extend the
  existing verified `DocumentFunction` mechanism beyond materialization-only
  bodies rather than inlining every ordinary call into a fresh cache scope;
- assign final dense IDs during reachable postorder publication;
- compute list-dataflow, document-expression, persistence, and contract
  fingerprints from shared sealed inputs;
- compact unreachable construction rows before publication, without cloning a
  completed plan;
- perform local invariants while inserting and one final cross-table seal;
- return an immutable `MachinePlan` plus its already computed canonical digest.

The runtime consumes only sealed plan functions/frames and existing typed
kernels; it must not regain a semantic AST interpreter or production flat
fallback. The public verifier remains mandatory, but repeated validation of the
same immutable payload at adjacent ownership handoffs is removed. JSON/debug
output streams from the sealed plan and is not required for an in-memory
preview. Compiler-internal distributed linking may retain construction IR only
until the link seals, then drops it.
The scored producer continues to include whatever serialization the manifest
declares, so no work is hidden from the gate.

Directional exit: backend plus plan seal/validation fits 300 ms, publication
hash/serialization fits 100 ms, no full-plan clone remains, and plan behavior,
persistence identities, deterministic digests, and malformed-plan rejection
remain exact.

### 5. Preserve Unit/Owner Identity Across Revisions In The Same Database

The parser already produces independent `ParsedSourceUnit` values with stable
path-derived `SourceUnitId`, but project assembly rebases every unit into one
global dense `ParsedProgram`. `CompilerSession::apply_updates` then clears one
project-wide checked slot for any changed unit. The substantial dependency and
worklist machinery inside `CheckedProgramDatabase` is reconstructed and
consumed on every request.

The `CompilationDb` introduced for the cold proof cut is the persistent
compiler service at later revisions; do not introduce a second solver or
dependency graph:

- retain immutable parsed units by `(SourceUnitId, content fingerprint)`;
- identify syntax/check rows as `{stable owner, owner-local id}` and build
  snapshot-local dense projections only for algorithms that need them;
- give declarations, callables, semantic components, proof owners, and backend
  regions separate implementation and public-semantic fingerprints;
- record exact forward/reverse dependencies and memo metadata including
  `changed_at`, `verified_at`, input/result fingerprints, and work counters;
- backdate a recomputed result whose public fingerprint is unchanged;
- publish complete diagnostics for the current revision, but build verified
  semantic/plan regions only on demand;
- cancel superseded revisions at bounded checkpoints and never publish a mixed
  or partially verified snapshot.

Every incremental result and diagnostic set is compared with a clean full
compile of that exact revision. Cover private edits, public changes, unrelated
units, transitive cones, rename/delete, error introduction/recovery,
cancellation, and unchanged-result backdating. Parallel component evaluation
may begin only after this graph is explicit and remains under one bounded
worker budget.

The design follows the useful parts of rustc's stable owner/local identities,
red/green projection-query firewalls, and result fingerprints; Salsa's exact
dependencies and backdating; Roslyn's immutable shared syntax snapshots;
Tree-sitter's old-tree structural reuse; TypeScript builder programs' affected
file/dependency updates; and TypeScript project references' explicit public
boundary products. It does not add Salsa or Tree-sitter as a second Boon
parser/type solver by default.

Primary references:

- [rustc incremental compilation and projection queries](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)
- [rustc demand-driven monomorphization collection](https://doc.rust-lang.org/beta/nightly-rustc/rustc_monomorphize/collector/index.html)
- [Salsa algorithm and backdating](https://salsa-rs.github.io/salsa/reference/algorithm.html)
- [Salsa memo currentness fields](https://salsa-rs.github.io/salsa/plumbing/terminology/memo.html)
- [Roslyn immutable syntax snapshots](https://learn.microsoft.com/en-us/dotnet/csharp/roslyn-sdk/compiler-api-model)
- [Tree-sitter incremental tree reuse](https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html)
- [TypeScript builder programs](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API)
- [TypeScript project references](https://www.typescriptlang.org/docs/handbook/project-references.html)
- [LLVM ThinLTO compact summary index](https://clang.llvm.org/docs/ThinLTO.html)

Directional exit becomes the performance plan's warm acceptance: affected
diagnostics meet 16.7 ms p95, verified constant-cone edits meet 100 ms p95,
and all cancellation/latest-generation gates pass.

### 6. Invert Build Dependencies At Execution Boundaries

The normal workspace graph at this checkpoint has 44 crates. The extracted
`boon_behavior_harness` still has a 28-crate forward closure; native playground
has 38. More importantly, `boon_runtime` has a 20-crate forward closure because
source/example convenience APIs make the execution core depend on the complete
compiler spine. A compiler change reaches 19 workspace crates in its normal
reverse closure. This is why focused behavior bodies can take fractions of a
second after linking while ordinary edits trigger long rebuild/relink work.

Use flag-day dependency inversion where it enables the work above:

- make runtime execution consume only verified plans plus document,
  persistence, executor, and host contracts;
- move source loading/example compilation from `boon_runtime` to an outer
  compiler/runtime adapter;
- move compilation functions out of `boon_program_runtime`; its core artifact
  and session types must not depend on `boon_compiler`;
- move `boon_host_runtime::migration_scenario` to a harness/tool owner so the
  persistent host core does not depend on compiler/example manifests;
- make HTTP/Wellen adapters return typed completion values; an outer
  orchestrator applies them to `ProgramSession`, avoiding an adapter-to-program
  dependency;
- move `ContentStore` to a dependency-bottom owner used by hosts without
  importing the entire persistent-host crate;
- relocate cross-layer execution IDs from `boon_document_model` to a small
  dependency-bottom contract owner only when the live graph proves the cut;
- split semantic model/proof crates only after the sealed database establishes
  their one-way API and immediately reduces a measured affected set.

For every cut, publish the before/after normal closure, one controlled rebuild
wall time/RSS, and unchanged Boon diagnostics/artifacts. A file move, a facade
that re-exports the old owner, or a smaller Rust closure without an ownership
improvement is not complete. Rust build speed remains separate from Boon
compiler latency.

## Execution Order

1. Preserve checkpoint `32bcf40`; finish the full real-host NovyWave
   migration/restart/provenance/negative oracle using its exact activation turn
   and single recorded/replayed host transcript.
2. Implement the minimal typed `CompilationDb` request/receipt/currentness
   kernel and one end-to-end program-root plus ordinary-callable vertical
   slice. Independently materialize V3 in tests, demand-collect the plan slice,
   retain its executable function plus invocation frame, verify it, and delete
   every superseded production owner in that slice.
3. Expand that vertical cut across checked/OUT/execution/resource/reactive/
   lowering/view/storage/memory domains. Production proof becomes owner-local/
   projection roots plus the compact exact cross-region request graph; migrate
   V3 to V4 only with the complete controlled proof evidence. No second warm
   dependency graph is allowed.
4. Complete demand-collected retained plan functions/frames and replace backend
   clone/rewrite/compact/hash passes with one `MachinePlan` builder seal. Normal
   in-memory publication does not retain construction IR/semantic products or
   pretty JSON; explicit debug/serialized-artifact intents and bounded pre-seal
   distributed linking own them. Acceptance format migration remains controlled
   and scored.
5. Reprofile the complete cold path. Return to a local optimization only when
   the new trace proves it is now the largest remaining owner.
6. Retain the same source/check/semantic request graph across revisions and
   close all warm, backdating, invalidation-locality, cancellation, and
   latest-generation gates.
7. Pull measured dependency inversions and crate splits at the earliest stable
   seams that shorten the next tranche; never let crate splitting replace
   steps 2--6.
8. Run the full cold and warm acceptance protocol and the three fresh-context
   adversarial reviews required by the performance plan.

Checkpoint commits are phase evidence, not exits. Do not push unless the user
explicitly asks. Do not begin game work.

## Refactor Rejection Rules

Reject a candidate when any of these is true:

- it retains the 160k rich-record proof graph in production under a new
  container, or coarsens exact dependency cones merely to reduce node count;
- it adds a second executable semantic authority or production flat fallback;
- it caches a whole-project product without explicit currentness and exact
  dependencies;
- it uses internal dense IDs as cross-revision, persistence, or oracle
  identities;
- it loses startup effects, normalizes async completion order, or weakens turn
  equality to make the differential harness pass;
- it skips complete diagnostics, exact proof acceptance, plan verification, or
  clean-full incremental parity;
- it counts parallelism, a profile change, a timeout increase, crate movement,
  or a debug-only speedup as satisfying a release Boon latency gate;
- it leaves the old owner/facade alive after the flag-day replacement passes.
