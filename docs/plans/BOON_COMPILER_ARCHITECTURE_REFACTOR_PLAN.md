# Boon Compiler Architecture Refactor Plan

Date: 2026-08-03

Status: active high-leverage execution map, reconciled through unit-native
checkpoint `a48f488` and the post-M1 identity/evaluator/fact-store/
compositional-seal research after the definition-artifact/thin-link and
structural-identity tranches, while preserving whole-program audit
checkpoint `d113544`, compact execution-receipt checkpoint `96b1611`,
shared-request-graph checkpoint `c870358`, compact-proof/sealed-plan checkpoint
`38e6541`, and activation/effect checkpoint `32bcf40`, subordinate to
[`BOON_COMPILER_PERFORMANCE_PLAN.md`](BOON_COMPILER_PERFORMANCE_PLAN.md). The
performance plan owns all latency, memory, correctness, and final acceptance
gates. This file owns the architectural sequence first chosen after checkpoint
`968c56a` and strengthened by the post-`32bcf40`, post-`38e6541`, and
post-`c870358` source/
primary-reference research below and in
[`BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md`](BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md);
it does not create a second set of weaker exits.

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
one CompilationDb and stable identity registry
  evaluation/currentness edges (compiler work; acyclic except explicit SCCs)
  proof/link relocations       (runtime semantics; cycles are allowed)

ProjectSnapshot -> UnitSyntaxSnapshot -> body-insensitive UnitItemIndex
  -> InterfaceSccResult -> CheckedDefinitionShard
  -> demanded DefinitionExecutableArtifact + compact InvocationFrames
  -> DomainArtifacts -> ThinLink(summaries + relocations + explicit SCCs)
  -> ContractVerifiedLinkedImage -> consuming RunnableMachineImage builder
  -> SealedRunnableMachine(plan tables + dense runtime indexes + receipt)
```

The executable path has one authority at each arrow. Exhaustive dependency
records, flattened specialized semantic trees, and debug JSON are materialized
only by explicit test/debug requests. Complete diagnostic projections belong
only to a diagnostics request. Construction IR may survive a bounded internal
distributed link or explicit serialized-artifact request, but is not retained
by an ordinary runtime artifact. `LinkFixedPoint` is an algorithm and bounded
scratch owner, not another retained OUT/resource/reactive/storage/view graph.

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

### Post-Checkpoint Whole-Pipeline Research (`76b93af`)

A bounded direct release trace after the documentation checkpoint, over the
unchanged `32bcf40` production code, completed in 4,401.51 ms at 317,860 KiB
peak RSS. Parse plus typecheck used 55.81 + 156.58 ms, while semantic
elaboration used 3,733.87 ms and its post-hoc dependency manifest alone used
2,329.43 ms. The manifest constructed 159,652 dependency records, 208,982
coverage rows, and a 160,316-node/512,314-edge graph for 664 callable/root
owners. Contract verification itself used only 0.06 ms. The bottleneck is
therefore representation and proof amplification after checking, not authored
proof execution or a generally slow frontend.

The same trace found a second multiplicative boundary after semantic sealing.
The retained execution graph had 16,421 expressions, but document lowering
published 33,916 expressions, 2,320 ordinary-call scopes, and 13,279 cache
scopes because `compile_user_call` recompiles a retained function root for each
argument/overlay context. Backend construction, the separate public plan
validation, and scored serialization then used 402.41, 100.60, and 89.52 ms.
Those are structural ownership passes, not candidates for another collection
micro-tune.

A source-level audit of the normal program-host route exposes work not fully
represented by that direct producer sample:

- `CompiledMachinePlanFromSource` retains the complete `ErasedProgram` beside
  the complete `MachinePlan`, although the ordinary host needs only the source
  digest from the IR;
- artifact encoding computes a plan digest, clones the plan into an owned
  serialization DTO, and serializes it; artifact construction then computes
  the plan digest again;
- `MachineTemplate::new_shared` runs the public plan verifier, whose report
  computes the plan digest again, before constructing executor metadata.

The trusted in-process path first required the now-landed non-forgeable
`SealedMachinePlan`; the post-`c870358` audit extends that boundary to
`SealedRunnableMachine`, adding dense runtime indexes built once while retaining
the immutable plan, canonical digest, successful verification receipt, and
minimal source/semantic/verification provenance. Explicit output intent owns
the remaining products:

```text
Diagnostics        -> complete checked diagnostics, no executable artifact
VerifiedPreview    -> SealedRunnableMachine, no retained semantic/IR/debug tree
SerializedArtifact -> same seal plus one streamed canonical artifact image
DebugIr            -> explicit ErasedProgram materialization
DebugPlan          -> explicit human-readable plan projection
DistributedLink    -> construction IR only until the joint link seals
```

Deserialized or otherwise untrusted plans still pass the complete public
verifier and build runnable indexes exactly once. A trusted token cannot be
constructed by bypassing the builder/verifier, and executor metadata is
derived once per seal rather than once per consumer. This boundary removes
duplicate lifetime and publication work; it does not weaken the mandatory
`SemanticProgram -> ContractVerifiedProgram -> ErasedProgram -> MachinePlan`
construction spine.

The ranked architectural opportunities are now:

| Rank | Refactor | Work owner that must disappear | Primary acceptance effect |
| ---: | --- | --- | --- |
| 1 | definition/invocation shards with finalization-time row receipts in one `CompilationDb` graph | production V3 exhaustive record/coverage allocation, entity-level proof graph, broad program-root dependency targets, and a future second warm graph | remove the dominant proof rescan while preserving exact proof and invalidation cones |
| 2 | one canonical `SealedSemanticImage` plus an ephemeral link fixed point | simultaneously retained OUT/execution/resource/reactive/lowering/storage/view/memory DTO graphs and the later canonical-core copy | remove representation multiplication and shorten peak lifetimes without weakening verifier facts |
| 3 | demand-collected shared plan-code definitions plus compact invocation frames across document, row, and migration domains | per-call ordinary-body recompilation, call cache scopes, and unreachable plan definitions | reduce semantic-to-plan expansion, backend time, and plan bytes |
| 4 | one consuming runnable-image builder and one `SealedRunnableMachine` publication token | full-plan fingerprint clone, repeated compaction/validation/hash passes, retained IR in preview, and executor `Metadata` reconstruction per consumer | reduce backend/publication/runtime-index work and peak live bytes without weakening untrusted-plan checks |
| 5 | retain the same database across revisions | whole-project `CompilerSession` invalidation and clean rebuild of unaffected owner/projection results | meet warm diagnostic, verified-edit, cancellation, and latest-generation gates |
| 6 | dependency inversion and measured crate splits at the new ownership seams | runtime-to-compiler convenience dependencies and broad Rust rebuild/relink closures | shorten implementation feedback without counting Rust build speed as Boon latency |

Ranks are dependency order, not six independent patch queues. The first
vertical slice must establish the rank-1 identities and finalization contract,
then cross ranks 1--4 for one definition plus its invocations, prove V3/source
materializer and runtime parity, and delete its old owners. Otherwise a
database facade, retained-function side table, sealed-plan wrapper, or runtime
index sidecar would merely coexist with the same hot path. Bounded parallel
owner evaluation and lower-level container tuning remain later options only
after a fresh trace shows the structural multipliers have gone.

### First V4 Projection-Proof Slice (2026-08-03)

The first flag-day production proof cut replaces the V3 entity inventory with
stable owner/projection receipts and an exact compact projection graph. V3 is
now a test-only exhaustive oracle. Its independent materializer reconstructs
every V4 row receipt, projection receipt, graph edge, SCC digest, and owner
implementation digest from the V3 inventory. The production graph falls from
159,617 nodes/506,915 edges to 14,518 nodes/43,714 edges. A normal downstream
compiler/runtime/CLI check and all 19 focused dependency-manifest tests pass.

One directional optimized NovyWave sample, not the scored p95 protocol, falls
from the immediately preceding sealed-plan sample's 4,581.206 ms and
317,316 KiB peak RSS to 3,977.806 ms and 247,092 KiB. The manifest itself falls
from 2,321.269 to 1,807.287 ms. The compact proof therefore saves about 603 ms
and 70 MiB while reducing graph nodes and edges by roughly elevenfold. It is a
real architectural cut, but it remains far outside both the 1,000 ms total gate
and the 350 ms semantic/proof envelope.

The residual profile is the important result: checked inventory still uses
367.057 ms, execution inventory 471.067 ms, lowering inventory 269.516 ms, and
final projection-receipt folding 516.468 ms. Production no longer retains rich
V3 rows, but it still walks already-built rich graphs after construction to
rediscover proof facts. Do not spend the next tranche polishing receipt hash
containers. Move row/projection receipt emission into checked, execution, and
lowering construction, make those receipts the shared proof/currentness
authority, and delete each corresponding post-hoc inventory walk. Then measure
whether exhaustive semantic demand itself must shrink. This V4 slice is not
the planned `CompilationDb`, construction-time receipt, or warm-currentness
exit.

The first dependency-bottom kernel is now wired into the production V4 path:
`boon_compilation_db` owns revision/backdating metadata, compact forward and
reverse request edges, deterministic SCC sealing, and implementation-root
digests. `boon_semantic` no longer owns a second V4 SCC implementation or the
old owner-by-all-projections scan. Four database tests and all 19 focused
manifest tests pass, including independent V3/V4 parity. A fresh directional
NovyWave sample is 4,011.485 ms at 250,416 KiB, with 3,265.269 ms semantics,
1,771.603 ms manifest work, and 465.455 ms projection sealing. The roughly
40--50 ms change is useful evidence but deliberately not an exit: the shared
kernel still receives the same post-hoc rows. The next batch must make owner
units produce the rows and delete checked/execution inventory ownership.

The same checkpoint introduces a non-forgeable `SealedMachinePlan` for the
trusted in-process route. It carries the immutable plan, canonical digest, and
successful verification receipt so normal runtime and artifact handoffs do not
clone, rehash, or reverify the same plan. Deserialized plans still use the full
public verifier. This closes the duplicate publication boundary but does not
make backend expansion or the end-to-end performance gate green.

The first post-`c870358` implementation cut replaces generic key-valued graph
edges with registered dense projection IDs, makes stale memo publication fail
closed, and separates callable public interfaces from callable implementation
summaries. Production call/use dependencies now target leaf interface nodes
committed by the existing public-shape digest; the broad callable-owner
reference constructor is test-only. A direct two-job release rebuild takes
2m43s, confirming that Rust fan-out remains red. One directional NovyWave run
is 3,961.669 ms at 250,596 KiB, with 3,223.284 ms semantics and 1,816.404 ms
manifest work. Latency is effectively still red/noisy, but the proof graph's
largest SCC collapses from 4,296 nodes at `c870358` to 85. The graph now has
15,181 nodes, 44,807 edges, and 14,483 components. This is the required
interface firewall for precise invalidation and later bounded parallelism, not
a timing exit. The next flag-day cut remains finalized checked/execution rows
and deletion of their 378/477 ms post-hoc inventories; lowering inventory and
receipt folding remain separately red at 272/502 ms.

### Checked/Execution Sealed-Image Ownership Checkpoint (2026-08-03)

The first complete checked-plus-execution ownership cut is now implemented as
a required architecture checkpoint in commit `174eb4b`, not a performance
exit. `CheckedProgram`
is an opaque `boon_checked` product issued through one audited unsafe seal in
`boon_typecheck`; `boon_semantic` has no production dependency on the
typechecker. The typechecker final seal emits stable checked shard receipts
after lowering/report metadata is complete. Semantic construction owns one
`SemanticImageBuilder<ExecutionPending>`, permits execution mutation only
through resource construction, validates the post-resource state, and consumes
it into `ExecutionFinalized` before sealing the execution handoff.

Outside tests, `SemanticProgram` no longer stores `CheckedProgramFields` or a
second execution graph. Manifest V5 imports the checked and execution handoffs
and has no production caller for `inventory_checked` or
`inventory_execution`; V3/V4 reconstruction remains test-only. Ordinary
invocations are attributed to their concrete program root, producer
invocations to the producer callable, and static occurrence identity owns the
final row while an ancestor frame remains provenance. A test-only independent
owner reconstruction compares every checked/execution owner table against the
construction-owned image routes. The architecture gate, the 19 focused
dependency-manifest tests, the minimal stable-manifest test, the distributed
bundle freeze/mutation oracle, and the ignored NovyWave occurrence oracle pass.
Those oracles exposed and closed a missing OUT-net static-owner route, an
incorrect callee-definition ownership rule, and a checked-solver path that
erased a known sealed cross-role value type to `Unknown`. A fresh-context
adversarial review also confirmed that bundle validation borrows rather than
clones the three sealed images and that expression-origin/frame plus
activation-local state relocations are present before this checkpoint.

The representation used to establish that boundary is deliberately recorded
as red. One current two-job release rebuild takes 4m09s. One direct optimized
NovyWave verified sample, not acceptance evidence, takes 5,665.819 ms at
507,428 KiB peak RSS, including 480.066 ms typecheck, 4,357.397 ms semantic,
1,142.939 ms execution-image finalization, and 1,534.308 ms manifest work. It
performs 18,656,831 allocations totalling 2,989,230,512 bytes. The plan hash
remains the prior current hash
`890eff63ce7eff16c5597093179b6878fc8f8ed3e9f49555e73333d71d7bcb42`,
so this is an ownership/representation regression rather than a semantic
shortcut.

The trace explains why this checkpoint cannot be extended with local hashing
tweaks. It seals 63,657 checked rows and 49,283 execution rows, then imports
full recursively owned stable projection keys and call-path vectors into row
routes and 119,441 graph edges. Final receipt folding still handles 78,336
legacy-domain rows. The resulting graph has 13,261 nodes and a maximum SCC of
156, so SCC explosion is no longer the dominant problem; repeated stable-key
ownership, serialization, and remaining eager demand are. This violates the
target representation in this plan even though the owner-deletion seam is now
real.

The immediate post-checkpoint work is therefore a fresh whole-pipeline design
audit followed by a larger replacement slice: intern stable projection keys
and invocation paths once behind dense IDs and one relocation arena; make
typed builders stream or reuse one final row fingerprint instead of rebuilding
full key trees per row; migrate the remaining OUT/resource/reactive/lowering/
storage/view/memory owners into the image; and move verified-intent demand
before occurrence expansion. Do not optimize `BTreeMap` operations or the
already-small SCC kernel while these representation multipliers remain.

### Post-`174eb4b` Whole-Pipeline Architecture Decision (2026-08-03)

Three fresh-context, read-only audits independently traced projection/image
ownership, demand/occurrence expansion, and the complete artifact lifetime from
typechecking through executor metadata. They agree that V1 is a useful
construction-ownership witness but is not the final image, proof, or
incremental-currentness seam. It still recursively owns projection and call-
path keys, derives invocation identity from owner-local order, hashes post-hoc
DTO scans, seals roles before bundle relocation closure, imports the result into
Manifest V5, retains seven rich semantic graphs plus a canonical-core copy, and
re-lowers ordinary bodies in several backends. Tuning maps, serializers, row
hashes, or SCC sealing inside that shape is rejected.

The selected replacement is one flag-day production architecture, implemented
in internally verifiable tranches without a production fallback:

```text
complete checked diagnostics + stable interfaces/definitions
  -> VerifiedIntent roots
  -> demanded definition specializations + compact invocation frames
  -> SemanticImageBuilder<Local>
  -> compact role/bundle link summaries + relocation fixed point
  -> SemanticImageBuilder<Linked>
  -> narrow proof view + verification receipt
  -> SealedSemanticImage
  -> shared all-domain plan-code linker
  -> SealedRunnableMachine(plan image + runtime indexes + receipt)
```

#### 1. Canonical identity and row storage

Replace the checked/execution V1 DTO families, Manifest V5 key import, and the
generic rich-key projection graph together. One registry owns revision-local
dense `OwnerId`, `SymbolId`, `StablePathId`, `InvocationPathId`,
`ProjectionId`, `RowId`, and `RelocationId` values. Stable paths and invocation
paths are collision-checked parent-pointer tries; full paths are debug views,
not vectors copied into rows. An authored call occurrence receives a parser-
derived structural identity. Inserting an earlier call must not renumber later
occurrences. Revision-local dense IDs and owner-local ordinals never enter a
stable fingerprint, persistence key, or cross-revision request key.

The retained image contains one owner table, path tables, a projection table,
typed domain columns, one row-metadata arena, flat CSR relocation/edge arenas,
projection receipts, and dense entity-owner columns. Do not retain
`Vec<{domain, index, rich key}>` routes or a second serializable proof model.
Every canonical row is finalized once after its last legal mutation and stores
one exact relocation span. Preserve four distinct commitments:

1. stable owner/projection/path key fingerprint;
2. local row payload fingerprint normalized to stable references;
3. linked projection/SCC fingerprint after relocation resolution; and
4. canonical dense image encoding digest.

This tranche deletes `CheckedImageHandoffBuilderV1`,
`ExecutionImageHandoffBuilderV1`, their post-hoc scans, rich V1 route/receipt
keys, `SemanticDependencyProjectionKeyV5`, `PresealedProjectionIndexV5`, and
the key-valued `ProjectionGraphBuilder<K>` production path. Manifest V6 consumes
the image's finalized receipts and dense edges directly; it does not import or
re-inventory the image. The V3/V4/V5 source materializers remain test-only,
independent omission and mutation oracles until the controlled replacement
gate passes.

#### 2. Demand before semantic occurrence expansion

Complete checking and deterministic diagnostics remain eager. Immediately
after the checked seal, `VerifiedIntent` roots published outputs, top-level
executable statements, state/source/effect ownership, persistence and migration
entries, host ports, producer materializations, and distributed imports and
exports. A queue keyed by
`{definition, execution domain, resolved layout, overlay/control shape,
capability contract}` requests canonical definition bodies once. Occurrences
carry compact frames for arguments, substitutions, PASSED, OUT ports, owners,
resources, effects, rendering, and materialization bindings.

Replace eager recursive `OutNetBuilder` instantiation and per-candidate
contextual body cloning with a demanded link solver, one definition-body
builder, and one frame builder. Statically proven dead branches create no
execution/proof/backend rows; dynamic inactivity, empty collections, and
offscreen work are not compile-time unreachability. Distinct authored
occurrences retain distinct state/source/effect identities. OUT remains
compile-time topology, and PASSED/FLUSH/DRAIN/DRAINING/HOLD/LATEST, commit,
deltas, effects, persistence, migration, and currentness retain their existing
semantics. Delete the late backend demand pass when this demand owner is live.

#### 3. One semantic authority and compact bundle link

Migrate OUT/resource/reactive/lowering/storage/view/memory algorithms to write
or finalize typed image columns. Delete each rich production graph immediately
when its independent source-driven oracle and last consumer move. Delete
`SemanticProgram` and `CanonicalProgramCoreV1` as retained authorities rather
than wrapping them behind V2; useful row schemas move into the image.
Verification receives borrowed pulse/arm/effect/crossing projections and a
small receipt, not ownership of the semantic program.

Elaborate each role locally once. Distributed rounds exchange only compact
exports, requirements, producer/external-event facts, relocations, and digests;
they never clone `CheckedProgram` or rerun full semantic elaboration. Apply
relocations once after monotone convergence, then seal the bundle. Confirmation
replays the compact link digest instead of rebuilding the roles.

#### 4. One code linker and one runnable publication

Link ordinary code once per compatible specialization across document,
row/scalar, and migration execution. Delete document call-cache scopes and all
three recursive ordinary-call body lowerers together. Invocation frames contain
resolved dense bindings, never a semantic AST or unresolved substitution.

The consuming linker assigns final IDs, reachability, structural fingerprints,
plan tables, and runtime indexes once and returns `SealedRunnableMachine`.
Normal compilation must not retain semantic graphs, canonical core, IR, and
plan simultaneously; `MachineTemplate::from_runnable` clones one `Arc` and does
not call `Metadata::new`. Untrusted deserialization remains a separate path that
verifies and constructs indexes exactly once.

#### 5. Currentness, bounded parallelism, and crate seams

Retain the same source/interface/definition/invocation/link/proof/plan/runnable
requests across revisions. Body-only edits backdate unchanged interfaces;
public edits invalidate the exact reverse cone; insertion, deletion, rename,
errors, cancellation, and stale generations fail closed; a clean full compile
and every incremental result remain identical. Enable at most two workers only
after dense dependencies prove requests independent. Shared-memory parallelism
is a multiplier after de-duplication, not a substitute for it.

Split crates only after these models stabilize. The intended low-fanout seams
are `boon_semantic_image` for stable semantic-image schemas/receipts/borrowed
views and `boon_machine_image` for the sealed plan/runtime-index contract.
Compiler adapters move outward from runtime cores. Do not split each semantic
domain into a crate, preserve rich DTOs through re-exports, or count Rust rebuild
speed as Boon compile latency. Every split requires before/after reverse-
dependency closure and two-job rebuild evidence and must enable the immediately
following owner deletion.

The external architecture evidence supports these boundaries rather than a
copy of another compiler. [rust-analyzer's architecture](https://rust-analyzer.github.io/book/contributing/architecture.html)
keeps body-local edits from invalidating global derived data and uses compact
IDs; [Salsa](https://salsa-rs.github.io/salsa/overview.html) uses interning and
tracked deterministic queries; [Swift's request evaluator](https://github.com/swiftlang/swift/blob/main/docs/RequestEvaluator.md)
centralizes fine-grained dependency caching and cycle handling; and the
[TypeScript native port](https://devblogs.microsoft.com/typescript/typescript-native-port/)
plus [TypeScript 7](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)
show the benefit and memory tradeoff of parallel checking. For Boon, the
measured 18.66 million allocations and repeated body/image/proof ownership must
fall before parallel compilation is allowed to amplify the remaining work.

#### Flag-day rejection and acceptance gates

Reject the tranche if V2 coexists as a production facade over V1/V5, receipts
scan finished vectors, proof/link views own cloned models, bundle rounds invoke
semantic elaboration, any backend still recursively lowers ordinary bodies,
`SealedRunnableMachine` still triggers executor metadata reconstruction, or a
feature flag/adapter preserves the old production path.

Required evidence includes exact definition/occurrence/specialization/frame and
row/domain counters; unique path nodes versus cumulative logical depth; queue
enqueued/processed/reused/pruned/error accounting; zero unresolved relocations;
zero legacy proof rows, post-hoc inventories, retained rich graph owners, and
recursive backend lowerers; one runtime-index build per runnable seal; stable
digest invariance under allocation-order perturbation; precise edit cones and
clean-full parity; direct/wrapped OUT/PASSED equivalence; independent behavior,
artifact, migration/restart, effect, currentness, and mutation oracles; scaling
fixtures; and fresh NovyWave time/RSS reports. The first implementation batch
must delete an existing V1/V5 scan or owner while carrying one real demanded
definition and occurrence through the dense image. Adding only an interner,
side table, database shell, crate, or new DTO does not qualify.

#### First dense V2/V6 spine: checkpoint candidate, not tranche exit

The first implementation cut after `174eb4b` replaces the public checked and
execution V1 handoff families and Manifest V5 in one flag-day working tree.
Checked V2 owns each stable projection key once, routes and relocations by
canonical `CheckedImageProjectionIdV2`, and stores one flat CSR relocation
arena. Execution V2 references checked projections by dense ID, interns
invocation ancestry as collision-checked parent-pointer nodes, stores each
execution projection identity once, and uses dense route/CSR tables. Manifest
V6 imports the image's stable-key digests, receipts, dense routes, and CSR
edges; it no longer clones full checked owner keys or recursive invocation paths
into its projection namespace. The compilation graph's production API likewise
accepts collision-checked dense projection IDs rather than generic rich keys.
Authored call sites currently use a parser/source-derived snapshot digest plus
an identical-site reverse ordinal. Focused oracles prove that unrelated and
identical earlier calls do not renumber later identical sites. This is only a
snapshot-local checkpoint identity: raw source text, source paths, and rich
owner data still enter the key, so it does not satisfy the target parser-owned
structural identity or cross-revision currentness contract.

This is a coherent representation checkpoint candidate, not completion of the
first demand/image tranche. `checked_image_handoff` and
`execution_image_handoff` still scan finalized rich checked/execution columns;
the checked and execution registries are linked rather than yet one complete
all-domain image builder; OUT and contextual occurrence expansion still happen
before demand; Manifest V6 still inventories 78,336 legacy-domain rows; and
resource/reactive/lowering/storage/view/memory graphs, canonical core, backend
recursive ordinary-call lowerers, distributed re-elaboration, and executor
metadata reconstruction still exist. Therefore the anti-facade exit remains
red even though the old V1/V5 wire shapes are gone.

The required three-way adversarial review found another deliberate red line in
this checkpoint: checked and execution row payload hashes still serialize rich
snapshot DTOs containing dense IDs and spans. Owner-local row ordinals have
now been removed from the row fingerprints, duplicate call sites count from the
end, relocation spans use checked arithmetic, V6 receipt layers have distinct
digest domains, and manifest row accounting fails closed. But these payload
hashes are snapshot receipts, not the normalized stable-reference fingerprints
required for persistent currentness. The next vertical slice must create
parser-owned structural occurrence routes and typed canonical row payloads
while moving their production construction into the demand/image builder; it
must not reinterpret this checkpoint's digests as cross-revision cache keys.

One final two-job release rebuild takes 3m00s. Its direct optimized NovyWave
edit-loop sample, not acceptance evidence, finishes in 3,549.342 ms at 274,896
KiB peak RSS with the unchanged plan hash
`890eff63ce7eff16c5597093179b6878fc8f8ed3e9f49555e73333d71d7bcb42`.
Semantic time is 2,480.719 ms. Execution-image finalization falls from
1,142.939 to 375.894 ms and manifest work from 1,534.308 to 727.061 ms;
allocated bytes fall from 2,989,230,512 to 1,805,377,118. Allocation calls are
12,517,443, so no allocation-count improvement is claimed. Architecture, 19
focused manifest tests, minimal manifest, authored-call insertion, state-
lifetime, bundle freeze/mutation, and the ignored NovyWave occurrence oracle
pass. This evidence authorizes the checkpoint only. The next production cut
must move verified-intent demand before OUT/contextual expansion, carry a real
demanded definition and occurrence through the image, and delete its replaced
scanner/owner; it must then migrate the remaining legacy domains rather than
tune the V2 containers.

### Post-`9540262` Multiplier Audit and Selected Refactor (2026-08-03)

The dense snapshot checkpoint makes the remaining architectural multipliers
measurable. Its final direct release sample is 3,549.342 ms. Semantic
elaboration alone is 2,480.719 ms: OUT takes 194.246 ms, contextual candidate
materialization 69.822 ms, execution expansion 293.237 ms, execution-image
finalization 375.894 ms, reactive/lowering/storage construction 478.702 ms,
and Manifest V6 727.061 ms. The backend adds 382.323 ms, plan validation
92.826 ms, and serialization 83.503 ms. The run still creates 5,147 OUT call
instances, 49,283 execution rows, and 78,336 legacy proof rows and performs
12,517,443 allocations totalling 1,805,377,118 bytes.

This changes the optimization order. Even an impossible zero-cost replacement
for both execution-image finalization and Manifest V6 would leave about 2.45
seconds. Parallelizing the same eager work, changing map implementations,
splitting files, or shaving individual hashes cannot reach the one-second
runnable gate. The next production tranche must remove the occurrence and row
multipliers before those downstream phases execute.

#### Separate four identity planes

The first V2 cut exposed an incorrect assumption in the earlier design: one
digest cannot safely serve deterministic artifacts, edit lineage, persistence,
and dense storage. Indistinguishable duplicate syntax cannot have a
deterministic cross-process identity that survives every arbitrary sibling
insertion without either edit history or an authored semantic anchor. Replace
that requirement with four explicit planes:

1. `CanonicalSyntaxRoute` is deterministic within one source snapshot. It uses
   canonical `SourceUnitId`, grammar roles, named declaration/statement
   anchors, and a snapshot-local duplicate discriminator. It owns diagnostics,
   reproducible artifact bytes, and cold differential tests; it is not a warm
   lineage or persistence key.
2. `SyntaxLineageId` is compiler-session local and is reused only when the
   incremental parser matches an immutable syntax node across revisions. It
   may drive exact warm request reuse, is never serialized, and a missed match
   causes conservative recomputation rather than false currentness.
3. `SemanticIdentity` is the public stable identity for declarations, state,
   sources, effects, resources, persistence, migrations, distributed ports,
   and authored occurrence semantics. It derives from language-owned names and
   explicit semantic structure, never source offsets, raw formatting, or an
   incremental allocator.
4. `DenseId` values are revision-local table indexes. They own image encoding
   and runtime locality and never enter the other three identity planes.

Request keys combine the narrow semantic owner with a session lineage ID when
available; result fingerprints normalize every reference to canonical syntax
or semantic identity. Cold artifacts remain deterministic even though lineage
IDs differ. Identical syntax that lacks an authored semantic anchor may miss a
warm cache after an ambiguous edit, but it must never reuse the wrong state,
effect, migration, or proof row.

This follows the useful separation visible in
[Roslyn's immutable, structurally shared syntax snapshots](https://github.com/dotnet/roslyn/blob/main/docs/wiki/Roslyn-Overview.md),
[rust-analyzer's file-local syntax plus body-stable `ItemTree`](https://rust-analyzer.github.io/book/contributing/architecture.html),
and [Salsa's distinction between identity fields and tracked fields](https://salsa-rs.github.io/salsa/overview.html).
It does not require adopting their public APIs or importing a generic query
framework before Boon's request and row boundaries are correct.

#### Ranked high-level opportunities

1. **Demanded semantic linker and canonical definition bodies.** Immediately
   after complete checking, publish `VerifiedIntent` roots for top-level
   statements, outputs, state/source/resource/effect schedules, host ports,
   persistence/migration entries, producer roots, and distributed crossings.
   One worklist keyed by definition, execution domain, resolved layout,
   overlay/control shape, and capability contract builds each compatible body
   once and represents calls with compact invocation frames. Integrate OUT
   constraints and contextual list operations into that worklist. Delete
   recursive `OutNetBuilder::instantiate_frame`, candidate-local expression
   builders, and the late backend demand pass together. This is first because
   it reduces every downstream domain rather than only its own measured phase.
2. **Construction-owned all-domain image with direct proof sealing.** The same
   linker finalizes typed normalized rows and exact relocation spans. Resource,
   reactive, lowering, storage, view, and memory algorithms append/finalize
   columns in that owner. Register each invocation-path node once. Replace
   `DenseManifestProjectionIndexV6`, legacy inventories, and the second proof
   graph with a borrowed proof view over image rows and CSR edges. This removes
   the measured 375.894 ms post-hoc image scan and most of the 727.061 ms
   manifest replay while preventing a new facade.
3. **One plan-code linker and consuming runnable publication.** Compile each
   demanded ordinary specialization once across document, row/scalar, and
   migration domains. Verification returns a token borrowing the sealed image;
   the consuming linker assigns final plan IDs and runtime indexes and returns
   `SealedRunnableMachine`. Delete the three recursive ordinary-body lowerers,
   `CanonicalProgramCoreV1`/`ErasedProgram` duplicate authority, post-plan
   rewrites, and executor `Metadata::new` reconstruction.
4. **Persistent red/green currentness.** Only after the first three products
   expose normalized request inputs/results, retain parse, summary,
   definition-specialization, proof, plan-fragment, and runnable requests in
   `CompilerSession`. `apply_updates` invalidates exact source-unit/semantic
   cones instead of dropping the whole checked result. Backdate unchanged
   results and prove clean-full parity, cancellation, and stale-generation
   rejection. The existing unused `RequestMemo` is a primitive, not a database.
5. **Compact three-role link fixed point.** Exchange only exports,
   requirements, producer/external-event facts, relocations, and digests; do
   not retain or re-elaborate three full `SemanticProgram` values per round.
6. **Bounded parallelism and crate seams.** After demand and dependencies are
   exact, schedule at most two independent source/body/SCC requests. Extract
   `boon_semantic_image` and `boon_machine_image` only when their schemas stop
   changing, and invert `boon_verify`/`boon_ir` onto those low-fanout contracts.
   TypeScript 7 demonstrates that native shared-memory parallelism can be a
   large multiplier, but Boon's Rust implementation is already native; its
   first problem is duplicated work and ownership, not a missing language port.

#### Selected next vertical tranche

Implement items 1 and the row-emission edge of item 2 as one flag-day slice.
The slice is complete only when a checked definition/effect summary publishes
real `VerifiedIntent` roots; a single worklist carries ordinary and contextual
definitions plus compact frames through demanded OUT topology into typed image
rows; at least one existing OUT/contextual recursive owner and its post-hoc
image scanner are deleted; and the resulting counters show fewer eager call
instances and execution rows on NovyWave. Preserve full diagnostics and all
state/source/effect/OUT/PASSED/FLUSH/DRAIN/currentness semantics. Do not start
with crate splitting, generic query-framework integration, two-worker
scheduling, Manifest map tuning, or another receipt wrapper.

Required new counters are intent roots by kind, definitions discovered,
specializations built/reused, frames built/reused, queue pushes/pops, statically
pruned branches, OUT instances avoided, contextual builders avoided, rows by
domain emitted/reused, and peak live artifact owners. Scaling fixtures vary
call depth, repeated call sites, contextual sites, dead branches, and dependency
cone width independently. A fresh adversarial review must prove both the
deleted-owner ledger and that effectful or stateful work was not pruned merely
because it is absent from a callable result.

#### First demanded-definition and compact-overlay evidence

The first working-tree cut computes the closed pure-definition set once before
OUT and shares it with contextual expansion. OUT no longer recursively copies
context-free bodies per occurrence. A deliberately aggressive prototype fell
from 5,147 to 1,251 OUT calls, but the full compiler correctly rejected it:
`Scene/Element/text` contexts inside retained bodies had lost their concrete
invocation route. The production candidate therefore computes a transitive
overlay-demand set and retains only user-call chains that reach real call-local
host contexts. Focused tests prove that context-free nested calls disappear,
render-context ancestry remains exact, and OUT/resource-owning functions stay
concrete. This is the compact-frame contract, not a Boon-level workaround.

On NovyWave, 312 definitions are retainable and 190 of them require a concrete
context-overlay route. OUT call instances fall from 5,147 to 3,494, cumulative
type substitutions from 80,743 to 39,431, execution rows from 49,283 to 47,296,
legacy proof rows from 78,336 to 73,162, and the projection graph from
13,261/119,671 nodes/edges to 11,608/82,364. The optimized full compiler emits
the same new plan hash twice,
`db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`.
One directional run is 3,451.075 ms at 271,128 KiB and another is 3,462.779 ms
at 270,692 KiB; allocated bytes fall to about 1.782 billion. The independent
retained-versus-flat NovyWave artifact oracle passes the stable contract and
persistence-type comparison, so the changed representation is intentional.
These are edit-loop results, not acceptance evidence.

The modest wall-time gain is itself the architectural result. The remaining
release run still spends about 351 ms rescanning finished execution columns
into the V2 image, 679 ms replaying all rich owners into Manifest V6, 511 ms
constructing reactive/lowering/storage graphs, and 368 ms in separately
recursive backends. The same cut now publishes `VerifiedSemanticIntentV1`
immediately after producer-root discovery. It validates and categorizes program
schedule, retained visual output, host output, source/state/list authority,
state initializer, consequential effect, external-call, and producer-function
roots. OUT consumes its exact schedule roots, while OUT and contextual
expansion share its retained-definition set; NovyWave retains the same 3,494
OUT calls, 47,296 execution rows, and 11,608/82,364 projection graph after this
ownership change. This is a production demand seam, not a trace-only counter,
but the non-schedule categories do not yet independently drive normalized row
construction.

Do not spend the next slice filtering the remaining three currently
unreachable lexical calls or tuning the overlay maps. Make those categorized
intent roots feed the construction-owned row/relocation arena, then replace
Manifest V6 re-import with a borrowed direct proof seal. The shared plan-code
linker follows immediately so render-context overlay routes and ordinary code
are assigned once rather than rediscovered by each backend. The selected
tranche remains incomplete until a scanner/owner is deleted.

#### First construction-owned domain and the lowering round-trip audit

The first row-emission cut now deletes the production `inventory_lowering`
replay owner. Lowering construction emits normalized metadata, output, and host
port proof rows at their owning stages; Manifest V7 resolves those routes and
seals them in an explicit `ConstructionSemantic::Lowering` namespace. V7 is an
intentional schema and digest-domain bump because construction-owned and legacy
semantic projections must never be interpreted under the same cache/proof
contract. The exhaustive lowering inventory remains `cfg(test)` only as an
independent source-driven oracle.

On the focused debug NovyWave oracle, 36,979 rows move to construction ownership
and the legacy count falls from 73,162 to 36,183 while graph topology remains
11,608 nodes and 82,364 edges. Reusing the already sealed lowering-metadata and
lowering-contract digests for their aggregate proof rows removes two duplicate
whole-graph serializations: traced aggregate work falls by roughly 434 ms in
that debug run, with the contract row itself falling from 217.162 ms to 0.019
ms. The architecture gate and exact semantic oracle pass. This is directional
debug evidence, not a release speed gate, and the remaining roughly 737 ms of
metadata-row construction shows why construction callbacks alone are not the
final architecture.

The higher-level consumer audit identifies that remaining multiplier precisely.
`SemanticLoweringMetadataV1` copies the checked expression, function, and named-
value type tables into semantic DTOs; `build_canonical_program_core` then maps
them back into `boon_checked` tables; `ErasedProgram` retains those tables; and
distributed and backend code scans them again for signatures, expression types,
and exportable named values. The expression table also carries occurrence rows
that already exist in the execution image. Preserving one proof row per member
of that round trip would make the historical bridge permanent.

The first flag-day table cut now replaces that round trip with narrow products
owned by the sealed semantic image:

1. diagnostic source-map rows own source units, original-expression coverage,
   and diagnostics without entering the runnable authority unless explicitly
   requested;
2. typed interface rows own callable exports/requirements, named value exports,
   host/output contracts, and distributed crossings by semantic identity; and
3. execution/storage rows remain the sole owner of expression flow types and
   normalized named-value targets.

`SemanticLoweringContractV2` now deletes the full expression/function type
inventories, while `CanonicalProgramCoreV2` deletes all three full checked
tables from the runnable core. The remaining lowering named-value metadata is a
transitional construction owner projected into a narrow interface rather than
reconstructed as a checked table. Distributed value references carry the exact
`ExecutableExprId`; backend export discovery consumes a narrow
`NamedValueInterface`; and remotely demanded functions are linked from the
exact sealed `ProducerFunctionInstance`, which now owns parameter, result, and
effect types. This last detail is essential: a function used only across a role
boundary correctly has no ordinary local-call entry. The focused three-role
regression proves three cross-role values plus a remote call/function export,
so no global function table or compatibility lookup remains reachable.

The fresh focused debug NovyWave oracle builds 1,885 construction-owned
lowering rows rather than 36,979, a 94.9% reduction. Lowering metadata falls
from about 736.6 ms to 120.7 ms, its dependency rows take 36.0 ms, Manifest
ingestion takes 4.4 ms, and final projection sealing takes 529.1 ms. The graph
falls from 11,608/82,364 to 10,640 nodes/80,698 edges while the 36,183
not-yet-migrated legacy rows remain. The whole focused semantic test is 12.67 s
after an incremental build, versus 13.99 s before this owner deletion. These
are directional debug measurements, not release acceptance evidence.

Do not return to tuning the 1,885 rows. The same trace now identifies the next
architectural owners: execution-image finalization is about 1.88 s, reactive
derivation about 1.30 s, and the remaining Manifest build about 1.72 s. Move
resource/reactive/storage/view/memory facts to construction-owned typed tables
and seal borrowed table/CSR spans directly, deleting each production replay
scanner and rich duplicate owner in the same tranche. Move diagnostic-only
source maps out of the runnable core and fold the remaining named-value
interface into its storage/interface owner. Then make `SealedSemanticImage`
the primary retained authority and land the shared plan-code linker. No
compatibility adapter may resurrect the checked tables, preserve the lowering
named-value inventory permanently, or keep rich graphs beside the replacement.

The first reactive owner cut confirms that this ordering must remain
architectural. Read construction already resolves every canonical/local read
to one exact binding, but trigger construction discarded that authority and
repeated lexical binding, owner-ancestry, and call-ancestry searches from each
state arm. `TriggerReadRoute` now carries that exact construction-owned route
into trigger planning. A transaction-local trigger-plan index materializes an
exact `(root expression, terminal boundary)` plan once, rejects cyclic plan
dependencies, and is intentionally discarded with the immutable semantic
build; revision-local dense IDs never enter a persistent cache. The exact
NovyWave semantic oracle remains unchanged while two current-tree samples put
state-update-arm construction at 295.4--309.5 ms, down from about 962.0 ms, and
the complete reactive phase at 496.9--513.2 ms, down from about 1,172.9 ms. This
is a coherent duplicate-owner deletion, not
a phase exit or a reason to tune the residual map/clone cost.

The new trace makes the next larger cut unambiguous: execution-image
finalization still takes about 1,824.5--1,848.1 ms and Manifest V7 still takes
about 1,695.0--1,714.8 ms. Replace their adjacent full-image scans with construction-owned
typed rows, dependency spans, relocation sealing, and a narrow final cross-
table validator. If reactive planning later becomes the largest owner again,
replace the remaining recursive walk with one normalized trigger-dependency
graph plus SCC/worklist publication and shared immutable arm spans; do not add
another layer of expression-specific caches first.

The execution/resource phase-cycle cut is now real. Inline list authorities
are normalized while execution is building, execution seals before resource
construction, and the resource table alone owns materialization row bindings
and predecessor lineage. Resource construction also publishes its 735 final
typed dependency rows, entity routes, and component commitment instead of
making Manifest inventory the rich graph again. A focused debug NovyWave trace
puts resource-row ingestion at 2.688 ms with 35,448 legacy replay rows left;
the exact ignored oracle and architecture verifier pass. Total time does not
materially improve yet because the adjacent owners remain: execution-image
finalization is 1,829.237 ms, resource derivation is 604.621 ms, and Manifest
is 1,701.115 ms. This evidence rejects further resource-row or hash-buffer
micro-tuning. The next architecture tranche decomposes execution into stable
checked-definition receipts and compact occurrence/invocation overlays whose
builders publish final rows and CSR relocations directly, then deletes the
post-hoc full execution-image scan and its Manifest replay.

The following second and third audit sections remain evidence and detailed
design rationale. Where they name a "next" action or staging order, the
post-`9540262` multiplier audit above supersedes it.

### Second Whole-Pipeline Reassessment: Owner Compilation Units

A fresh post-checkpoint directional sample completes in 4,052.379 ms at
247,284 KiB. Parse plus typecheck use 56.748 + 161.384 ms. Semantic elaboration
uses 3,262.360 ms: 1,813.236 ms is the compact V4 manifest, about 1,311.717 ms
is the eager graph-building work before it, and about 137.407 ms builds/hashes
the canonical core and validates the handoff. IR lowering, backend construction,
plan validation, and scored serialization remain separately visible at
37.220, 394.166, 93.479, and 84.037 ms. These are one-sample directional phase
timers, not non-overlapping p95 acceptance evidence.

The arithmetic rules out a receipts-only plan. Even a free manifest would
leave roughly 2.24 seconds end to end. The current semantic pipeline builds a
whole-program OUT graph, execution graph, resource graph, reactive graph,
lowering contract, storage graph, view graph, and memory graph; V4 then scans
them, the canonical-core builder maps them again, and the document backend
expands retained ordinary definitions per invocation. Moving the same 208k
classifications into callbacks without changing those authorities would only
move time between labels.

The post-`c870358` source and adversarial audit corrects one important part of
that first model: a single `OwnerBodyUnit` conflates authored definitions,
occurrence-owned invocation/resource state, and program/bundle-wide link facts.
The implementable production shape is:

```text
SourceUnitSnapshot
  -> InterfaceShard(stable public declarations, schemes, effects)
  -> DefinitionShard(stable authored identity, parametric checked/executable
       rows, local receipts, unresolved stable relocations, optional plan ABI)
  +  InvocationShard(parent invocation, local substitutions/PASSED context,
       OUT ports/nets, static/resource/row/effect/view bindings)
  -> LinkFixedPoint(demand roots, relocation resolution, producer/external-
       event closure, cross-shard summaries, compact SCC/projection graph)
  -> SealedSemanticImage(final dense columns, one edge/relocation arena,
       proof roots, narrow verifier projection)
  -> VerifiedProgramImage
  -> shared plan-code linker(definition, execution domain, resolved layout,
       overlay/control shape, capability contract)
  -> consuming RunnableMachineImage builder(plan code + invocation frames +
       dense runtime indexes)
  -> SealedRunnableMachine(image, receipt, digest, provenance)
```

Definitions and invocation shards are cold compilation and warm invalidation
units. `LinkFixedPoint` is bounded linking scratch over summaries and
relocations; it must not retain a second family of OUT/resource/reactive/
storage/view/migration graphs. The current `ProgramRoot` is a final link sink,
not a dependency target for every top-level fact. Source/module interfaces,
authored callable bodies, top-level authority/definition shards, and the final
program link receive distinct stable identities. Broad `Owner` edges are
rejected because they create the observed 4,296-node SCC and destroy both
invalidation locality and safe owner parallelism.

Construction-time proof means finalization-time proof, not append-time proof.
Resource construction mutates execution bindings and lineage after initial row
creation, and storage resolution adds/reclassifies capture fields. Builders
therefore use typed `Pending -> Finalized` row states. Finalization fingerprints
the canonical local payload exactly once, emits its exact owner/projection and
dependency/relocation span, and marks schema coverage. Three identities stay
separate: a stable local-content fingerprint for backdating, a linked-target
fingerprint after relocations resolve, and the final dense-image encoding
digest. Revision-local dense IDs are never cache or persistence keys.

Only cross-shard projection edges enter
`boon_compilation_db::RequestGraphBuilder`; dense rows remain in a language-
owned typed store. Production deletes `DependencyOwnerIndex`, `inventory_*`,
and `DependencyCollector` domain by domain as builders emit final receipts.
The old source-driven builder remains an independent test oracle during each
migration. A materializer that reads only the new store cannot detect a source
fact that was accidentally never inserted.

`SealedSemanticImage` becomes the sole retained semantic authority. One
`SemanticImageBuilder` owns typed dense columns and a shared CSR edge/
relocation arena. OUT/resource/reactive/lowering/storage/view/memory algorithms
write or finalize those columns directly; their rich graphs become borrowed
views or explicit test/debug materializers. Delete the production canonical-
core remap and drop mutable construction scratch as each shard seals. Bundle
roles seal only after the distributed producer/external-event fixed point;
definition shards may be reused across rounds while link-dependent projections
are memoized by their exact inputs.

Verification receives the sealed image, proof certificate, and narrow tables
for pulse batches, arms, observers, effects, and distributed invocations. The
verifier remains the only authority that approves fusion. Normal IR erasure
moves this image rather than retaining `SemanticProgram` plus eight rich graphs
and a duplicate canonical core. Existing rederivation validators remain test-
only oracles until their domain has exact parity.

Retained definitions must cross every backend domain, not begin and end in the
document backend. OUT currently has 5,147 concrete call instances for 1,821
checked calls; the execution graph retains 312 ordinary bodies, yet document
lowering creates 2,320 ordinary-call scopes and 13,279 cache scopes. Document,
row/scalar, and migration lowering therefore share plan-code definitions keyed
by `{definition, execution domain, resolved layout, overlay/control shape,
capability contract}`. Occurrences become compact frames containing already
resolved argument, PASSED/context, owner/materialization, resource/effect, and
render bindings. Frames contain no compiler AST or unresolved type
substitution, and runtime dispatch uses dense IDs/ranges.

`SealedMachinePlan` removes duplicate trusted verification but is not the final
publication boundary. `MachineTemplate::new_sealed` still constructs a broad,
clone-heavy executor `Metadata` graph for each consumer. A consuming builder
must instead publish `SealedRunnableMachine`: canonical plan tables plus dense
runtime indexes built once, one receipt and digest, and minimal provenance.
Untrusted deserialization still verifies the plan and builds those indexes
exactly once. The crate seam must avoid a plan/executor dependency cycle;
runtime indexes belong in a dependency-bottom runnable-image/model owner, while
execution algorithms remain in the executor.

For warm work, retain existing unit-local parser products and keep interface
summaries stable across body-only edits. This follows
[rust-analyzer's item-tree boundary](https://rust-analyzer.github.io/book/contributing/architecture.html),
[rustc's red/green query graph](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html),
and [Salsa result backdating](https://salsa-rs.github.io/salsa/reference/algorithm.html).
[Swift's request evaluator](https://www.swift.org/blog/swift-5.2-released/) and
[ThinLTO summaries](https://clang.llvm.org/docs/ThinLTO.html) reinforce the same
rule: do whole-program decisions from compact summaries and materialize bodies
only on demand. TypeScript 7's native parallel checker demonstrates that
parallelism can multiply an already shared-memory architecture, but Boon is
already native Rust; its 11.14 million allocation calls and multipass graph
amplification must disappear first. Use at most two compiler workers after the
request graph proves independence.

#### Historical Post-`c870358` Architectural Priority

Items 1 and 2 below led to checkpoint `174eb4b`. The current production order
starts with the dense V2/Manifest V6 and demand-first vertical tranche in the
post-`174eb4b` decision; this list remains the provenance of that decision.

1. Specify stable source/interface/definition/invocation/top-level/link keys,
   local/linked/image fingerprint domains, finalization typestates, and small
   projections. Split `ProgramRoot`; eliminate broad owner dependency targets;
   add row, edge, SCC, and invalidation-cone counters before enabling caching.
2. Make typechecking emit interface and definition shards. Migrate checked and
   execution rows plus finalization-time receipts, and delete their production
   post-hoc inventories in the same tranche.
3. Introduce invocation shards and migrate OUT/resource/reactive/lowering/
   storage/view/memory in dependency order into one semantic image, deleting
   each old DTO graph owner when its borrowed view/test materializer passes.
4. Run the distributed link fixed point over compact summaries, seal the role/
   bundle semantic image, give verification its narrow proof view, and delete
   post-seal `SemanticProgram` retention and duplicate canonical mapping/hash.
5. Add the shared all-domain plan-code linker and frames. Delete document,
   row/scalar, and migration per-call body lowering together; do not preserve a
   production flat fallback.
6. Replace completed-plan clone/rewrite/compaction and per-consumer executor
   metadata construction with one consuming `SealedRunnableMachine` builder.
7. Retain these exact shards and request memos across revisions, add backdating
   and cancellation, then enable deterministic two-worker evaluation only for
   graph-proven independent requests.
8. Split crates only at these durable seams: stable semantic image/model,
   semantic builder/proof implementation, runnable image/model, and outer
   source/compiler adapters. Record the reverse-dependency closure and a
   controlled two-job rebuild before/after; a re-export facade is not a split.

The first coding batch must establish the stable key/fingerprint/finalization
contract and carry one definition plus its invocation through a real vertical
slice. It must delete an old scan or representation owner before checkpointing;
`RequestMemo` currently has no production consumer, so another database shell
or side table is rejected. The Boon latency result comes only from deleted
compiler work; Rust rebuild speed remains a separate developer-speed gate.

Record these structural gates alongside the time/RSS protocol:

- parsed/reused source units and interface/definition/invocation/link shards
  recomputed, reused, or backdated for each request;
- interface, definition, invocation, top-level-authority, and link shard counts;
- finalized rows per shard (max and p95), exact cross-shard edges, largest SCC,
  and broad-owner dependency targets (must be zero);
- production post-hoc inventory rows/passes and retained rich semantic graph
  owners (both must reach zero at their flag-day exits);
- plan-function variants and frames versus old ordinary-call/cache scopes; all
  old recursive call-lowering owners must reach zero;
- trusted executor metadata rebuilds (zero) and runnable-index builds (one per
  seal), plus unchanged turn/delta/currentness/commit/effect/persistence work;
- invalidation seeds/cone visits, proof/backend regions rebuilt or reused,
  cancellation checkpoints/discarded work/publication attempts, and an atomic
  latest-generation commit; every counter family has an accounting equality;
- reverse Rust dependency closure and controlled rebuild time/RSS, reported
  separately from Boon cold and warm compiler latency.

The parser already exposes stable path-derived `SourceUnitId` and context-
independent `ParsedSourceUnit`, but production reparses and globally reassembles
every unit. Expose a parser-owned immutable project-unit snapshot and exact
cached assembly API; do not substitute the public single-unit parser because
its validation policy differs. Session updates become atomic upsert/remove/
rename deltas and tombstone removed stable owners. Unify session revisions with
the database's checked increment so overflow fails closed rather than silently
reusing a revision.

`boon_compilation_db` remains a dependency-bottom graph/currentness kernel, not
a semantic value cache. Narrow `RequestGraphBuilder` toward a private
`ProjectionGraphBuilder`: register each stable owner and fixed small projection
once, return a dense request ID, and accept edges by ID. Its node accounting is
`owners + nonempty projections + link nodes + measured typed exceptions`.
Arbitrary per-expression queries, generic cached semantic return values, and
dynamic row-level dependency tracking are forbidden. `RequestMemo::publish`
must reject non-monotonic or dependency-stale publication before production use.

Persistent compilation uses revision overlays. Work may read the previous
immutable snapshot, but it publishes owner/link memos and a runnable seal only
if every demanded result is current and the generation is still latest. Place
cancellation checks inside owner worklists and link/backend regions, not merely
between large phases. Complete diagnostics deterministically merge current
owner diagnostics in canonical unit/local-span order. Every incremental
revision must match a clean full compile for diagnostics, interface/body/link/
proof/verification digests, plan hash, stable contracts, and behavior.

### Third Whole-Pipeline Audit: Delete Owners, Not Just Inventory Loops

A direct current-HEAD release trace after checkpoint `e475a22` completes in
4,044.712 ms at 250,736 KiB peak RSS. It preserves plan digest
`890eff63ce7eff16c5597093179b6878fc8f8ed3e9f49555e73333d71d7bcb42`
and records 11,045,739 allocation calls / 1,574,453,268 allocated bytes. This
is one directional sample, not scored p95 evidence, but its nested timers make
the ownership decision unambiguous:

| Current owner | Directional time |
| --- | ---: |
| parse plus complete typecheck | 57.480 + 164.446 ms |
| eager semantic graphs before proof | about 1,321 ms |
| post-hoc dependency manifest | 1,819.621 ms |
| checked / execution / lowering inventory | 374.351 / 478.142 / 272.516 ms |
| late row resolution, projection folding, and SCC seal | 505.380 ms |
| duplicate canonical semantic-core digest | 111.129 ms |
| IR erasure plus its internal audits | 39.376 ms |
| backend construction | 362.329 ms |
| plan verification / required serialization | 94.141 / 85.958 ms |

The eager semantic time is not one local loop. It includes a 5,147-occurrence
OUT graph with 80,743 cumulative substitutions, execution construction, two
complete execution validations, resource binding, reactive scheduling,
lowering metadata, storage, views, and memory. In particular, reactive state-
update-arm derivation is about 162 ms, lowering metadata plus its separate
digest is about 149 ms, and storage construction is about 147 ms. Therefore a
faster hash table inside `DependencyCollector`, or moving the same hashes into
callbacks while retaining every graph, cannot reach the 350 ms semantic/proof
envelope.

The next flag-day slice is the complete checked-plus-execution ownership cut:

```text
InterfaceShard + DefinitionShard + InvocationShard
  -> SemanticImageBuilder<ExecutionPending>
  -> resource binding, synthesized rows, and lineage backpatches
  -> SemanticImageBuilder<ExecutionFinalized>
  -> remaining domain views/builders
  -> SealedSemanticImage
```

The execution component cannot seal when contextual expansion first returns.
Resource construction may synthesize expressions/origins/statements, bind
materialization source and target lists/scopes, and finalize predecessor
lineage. Its exact seal is after resource construction and the current
post-resource invariant boundary. Checked interface rows may begin at the
typechecker's first checked seal, but the complete checked shard seals only
after lowering metadata, source/type/host/render tables, and diagnostics are
attached at the typechecker's final seal.

This slice is complete only when all of the following are true:

- `SemanticProgram` no longer owns `CheckedProgram` or a separate
  `SemanticExecutionImageColumnsV1` field outside the sealed image;
- production contextual/resource construction writes callable interfaces,
  definitions, invocations, expressions, statements, scopes, sources, states,
  roots, producer functions, materializations, owners, and origins directly
  into image-owned columns;
- remaining legacy semantic algorithms receive zero-allocation borrowed views
  over those columns, never cloned `Vec` materializations or a second
  serializable graph facade;
- callable interface leaves receive a stable callable key and the finalized
  public-shape digest directly, rather than being rediscovered from dense
  `SemanticCallableId` values inside the manifest;
- `inventory_checked`, `inventory_execution`, the post-hoc callable-interface
  digest loop, and their checked/execution portions of late owner/entity
  resolution have no production caller;
- every actual image row is fingerprinted once with all authoritative fields
  and an ordered stable-relocation span; V3 child-field subjects map to a row
  plus classifier field/domain only in the independent test oracle;
- projection-local roots fold finalized row receipts during image sealing and
  only unique cross-projection relocations enter the request graph. Production
  does not retain one row per historical coverage subject merely to regroup it
  in `finish_compact_v4`;
- the old checked and execution artifacts, inventories, and validators remain
  available only under test configuration as source-driven omission oracles.

Local row identity, linked dependency identity, and dense image identity stay
separate. A local receipt commits the canonical row payload and stable
relocation keys. Link sealing resolves those relocations to projection IDs and
combines dependency roots. Final image encoding assigns revision-local dense
IDs and has its own digest. This permits body-result backdating without
pretending that a different link target or dense encoding is the same object.

The current interface firewall also exposes two correctness gaps that must be
closed before persistent reuse:

- the program-root public-shape digest currently includes the whole source
  bundle, so every edit dirties the root; split top-level authority/interface
  summaries from the final link sink;
- the callable public-shape digest currently commits `context_scheme: None`.
  Move the existing payload unchanged for the first cut, then version the
  interface schema before adding the complete principal context scheme and
  add a mutation oracle for every dependency-relevant interface field.

Demand collection moves before occurrence expansion and backend lowering.
After complete diagnostics and interfaces seal, verified intents seed demand
from published outputs, document/view roots, sources/effects, persistence and
migration contracts, host ports, producer materializations, and distributed
imports/exports. Link summaries collect only reachable definition-
specialization keys and invocation frames. The current backend `demand_plan`
is too late because it walks an already complete `ErasedProgram`; the current
distributed provisional demand set is too narrow and discarded after interface
convergence.

The downstream order is now fixed:

1. delete checked/execution production owners and inventories through the
   image-owned vertical slice above;
2. migrate invocation/OUT/resource/reactive/lowering/storage/view/memory rows
   into the same image and delete each rich graph plus the duplicate canonical
   mapping/hash as its independent oracle passes;
3. demand-collect and link shared plan-code variants across document,
   row/scalar, and migration domains, deleting all recursive function-root
   lowering and ordinary-call/cache-scope owners;
4. land the consuming plan builder and `SealedRunnableMachine` together.
   A standalone seal wrapper is rejected because it would retain completed-
   plan clone/rewrite/compaction and per-consumer `Metadata::new`;
5. retain these exact cold units across revisions, make currentness graph-
   checked and transactional, then allow at most two graph-proven workers.

Warm evidence must be versioned with this cut. The current work-sample
predicate requires every unit to be reparsed and the preview profile repeats
diagnostic parse/typecheck provenance, so a genuine incremental implementation
would paradoxically fail or double-count. Report per-request parsed/reused
units, recomputed/reused/backdated shards, and currentness work directly while
keeping clean-full parity as the correctness oracle.

### Fourth Whole-Pipeline Audit: Break The Phase Cycle And Publish Once

The post-`ac2b234` source audit corrects the third audit's proposed seal point.
Resource construction is not intrinsically entitled to mutate execution. It
currently does so because `synthesize_inline_checked_list_targets` creates
semantic expressions, origins, and statements late, while the binding passes
backpatch source/target list and scope IDs plus predecessor lineage into
`SemanticContextualMaterialization`. The resource graph then copies those same
bindings into `materialization_bindings`, and downstream code reads a mixture of
both owners. `execution_for_resource` therefore exposes a phase cycle, not a
durable semantic boundary.

The replacement boundary is:

```text
checked definitions + demanded invocation frames
  -> ExecutionBuilding
  -> normalize inline list authorities and statement ownership
  -> ExecutionSealed (immutable expressions/statements/materializations)
  -> ResourceTable (sole row/list binding and lineage owner)
  -> construction-owned domain tables and local receipts
  -> one compact relocation/SCC linker
  -> SealedSemanticImage
```

The normalization pass may reuse the current exact checked-list and occurrence
logic initially, but it runs before execution publication and emits ordinary
image rows through the same builder. Once `ExecutionSealed` exists, no resource,
reactive, storage, view, memory, proof, or backend API receives mutable execution
columns. `SemanticContextualMaterialization` retains operation, expression,
owner, type, and local identity only. `SemanticMaterializationResourceBindingV1`
or its sealed-table successor exclusively owns source/target rows and
predecessor lineage. Consumers use a dense materialization-to-binding route;
they never search or cross-check a duplicate execution copy.

This phase cut also removes adjacent repeated work. The current elaborator
validates execution before resources, validates it again after resources, and
`finalize_execution` performs the same post-resource validation a third time.
It then calls `execution_image_handoff`, which rediscovers routes for every
scope, expression, statement, callable, call, occurrence, source, state, root,
function, materialization, and owner; serializes/hashes every rich payload;
allocates relocation-digest vectors; and seals a second projection image.
The target performs local insertion checks, exactly one cross-table execution/
resource seal audit, and construction-owned row hashing into reusable scratch
storage. There is no post-hoc execution handoff scanner.

The same publication rule applies to the remaining domains. Resource,
reactive, lowering, storage, view, and memory builders publish a typed table and
one `DomainSeal`-equivalent product containing the component digest, stable
projection rows, dense entity routes, and typed CSR relocations. Manifest V7's
current `DependencyCollector` still creates `compact_rows`, `compact_records`,
`compact_references`, subject and entity maps, re-registers checked/execution
projections, and inventories the rich domain graphs. Replace it with one linker
over the domain seals. The manifest becomes a signed/materialized view of that
sealed projection graph; it is not a second compiler. A canonical domain row is
serialized and hashed once by its construction owner.

The broader lifetime audit exposes three following owner deletions:

1. `SemanticProgram` still retains six rich domain graphs beside
   `CanonicalProgramCoreV2`, and `semantic_program_digest` serializes/hashes the
   mapped core again. Make the normalized sealed image the backend input; rich
   graphs become explicit debug/test materializers, the canonical core becomes
   a borrowed executable projection or disappears, and the program digest folds
   construction-owned domain/link receipts.
2. Machine-plan finalization clones the complete plan to calculate typed-list
   view fingerprints, rewrites roots, compacts in a second traversal, validates,
   and later makes each trusted `MachineTemplate` rebuild `Metadata`. The shared
   plan-code linker and consuming runnable builder assign final dense IDs in
   reachable postorder, compute fingerprints before publication, and return
   `SealedRunnableMachine { plan, indexes, receipt, digest }` with indexes built
   once.
3. `CompilerSession::apply_updates` currently replaces source strings and sets
   the whole checked slot to `None`; the existing `CompilationDb` is used as a
   one-shot manifest SCC/fingerprint helper rather than retained compiler
   currentness. After the cold owners above are normalized, make that same
   database own immutable source snapshots and stable interface/definition/
   invocation/domain/link/plan/runnable requests with red/green verification,
   unchanged-result backdating, exact reverse cones, cancellation, and atomic
   latest-revision publication.

Crate splitting follows these ownership seams rather than preceding them. The
durable candidates are a dependency-bottom semantic image/model owner, a
semantic build/proof implementation owner, and a dependency-bottom runnable
image/model owner. A split is accepted only when consumers depend on the small
sealed model without pulling the builders and a measured edit rebuilds fewer
crates or relinks. Moving the current mutually dependent rich graphs into new
packages or re-exporting them through a facade is rejected.

The immediate coherent tranche is the complete phase-seal cut, not removal of
one redundant validation in isolation:

1. move inline-list-authority normalization to `ExecutionBuilding` and prove
   exact execution/resource parity;
2. make the resource table the only materialization row-binding/lineage owner
   and migrate every consumer before deleting the execution fields;
3. emit execution and resource routes, row receipts, and relocations while
   constructing their final rows;
4. delete `execution_for_resource`, `execution_image_handoff`, the duplicate
   materialization-binding checks, the repeated whole-execution validations,
   and the execution/resource Manifest inventories in the same flag-day tranche;
5. reprofile the exact NovyWave oracle before selecting the next whole owner.

This supersedes the earlier statement that execution's exact seal belongs
after a mutating resource builder. It preserves the same semantic facts and
oracles while assigning each fact to one phase and publishing it once.

### Fifth Whole-Pipeline Audit: Compile Definitions, Link Overlays, Retain Snapshots

The post-`210b1a9` trace rejects treating execution finalization as a hashing
utility problem. NovyWave has 17,721 checked expressions and 16,525 execution
expressions. Of those execution expressions, 11,257 route directly to stable
checked definitions and only 5,268 route to concrete invocation projections.
Nevertheless `execution_image_handoff` constructs 47,296 row fingerprints,
47,296 entity routes, 6,483 projections, 3,494 invocation paths, and 16,834 raw
relocations after execution has already been built. One projection contains
18,509 rows, which is both a cold serialization hotspot and an unusably coarse
warm invalidation unit.

The measured execution finalization is 1,869.164 ms:

| work | debug NovyWave time |
| --- | ---: |
| expression plus duplicated origin rows | 890.791 ms |
| final projection/path/relocation canonicalization and whole-handoff hash | 340.185 ms |
| calls plus call occurrences | 188.654 ms |
| projection/path setup | 93.477 ms |
| scopes, statements, callables, and remaining domains | 356.057 ms |

This identifies the replacement representation. A stable checked-definition
receipt commits authored opcode, literal/interface shape, source definition,
and dependency-relevant checked facts once. A compact invocation overlay
commits only concrete type/layout substitutions, parameter/PASSED/context
bindings, owner/resource/effect/render coordinates, and dense plan-code
relocations. Diagnostic provenance splits into a definition-owned source map
and a compact occurrence route; it does not clone checked scope/span data into
every execution row. Final executable rows, not rich semantic DTO mirrors, are
the row-fingerprint authority.

The cold pipeline becomes a thin-link shape:

```text
immutable unit snapshots
  -> checked interface + definition receipts
  -> demanded definition variants + compact invocation overlays
  -> construction-owned resource/reactive/storage/view/memory tables
  -> compact summary index + relocation/SCC link
  -> consuming runnable-image builder
  -> SealedRunnableMachine + optional proof/debug materializers
```

The compact summary link may inspect public/effect/resource/storage/view/
migration summaries and exact inter-shard relocations. It must not import all
rich rows into a monolithic Manifest compiler. This follows ThinLTO's useful
separation between per-unit products and a compact combined summary index,
without adopting LLVM IR or deferring Boon's mandatory verifier. The local
receipt/result fingerprint is also the red/green query result: unchanged
definition or interface output is backdated and keeps downstream shards green.

The warm architecture is currently absent, not merely inefficient.
`CompilerSession::apply_updates` replaces source strings and sets the entire
checked slot to `None`; `boon_compilation_db` is not retained by the session and
currently stores no parsed, checked, semantic, or runnable values across
edits. The same flag-day program therefore adds an immutable `ProjectSnapshot`
with structurally shared unit products and a persistent request database. Unit,
interface, definition, invocation, domain, link, and runnable requests retain
values plus exact dependencies, `changed_at`, and `verified_at`. A successful
new revision publishes atomically while the last verified runnable image stays
live. A whole-project artifact cache is not an incremental compiler.

The implementation order is therefore stricter than “remove the scanner”:

1. define checked-definition, invocation-overlay, diagnostic-source-map, and
   compact relocation schemas with independent local/link/image digests;
2. publish expression/origin/call rows at their construction sites and compare
   them against the current full execution handoff under tests;
3. make compact executable rows plus domain seals the production proof input,
   then delete `execution_image_handoff`, its 47,296-row mirror, and Manifest's
   execution re-import in the same cut;
4. replace the remaining Manifest inventories with the compact summary linker
   and fold the semantic program digest from existing domain/link receipts;
5. connect retained immutable unit/definition results and the same dependency
   graph to `CompilerSession`; prove clean-full parity and exact cones before
   enabling bounded parallel evaluation;
6. split model/build crates only at the resulting one-way seams. The likely
   durable cuts are dependency-bottom semantic-image contracts, semantic
   construction/proof, dependency-bottom runnable-image contracts, and the
   persistent compiler service.

Primary architecture references supporting these choices:

- [rustc red/green incremental queries](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html)
- [Salsa result backdating and durability](https://salsa-rs.github.io/salsa/reference/algorithm.html)
- [TypeScript incremental builder programs](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API)
- [Roslyn immutable workspace snapshots](https://learn.microsoft.com/en-us/dotnet/csharp/roslyn-sdk/work-with-workspace)
- [LLVM ThinLTO compact summary index](https://clang.llvm.org/docs/ThinLTO.html)

The trace-only phase/count instrumentation remains in the semantic compiler so
future measurements distinguish row families and representation cardinality.
It is evidence tooling, not an optimization or an acceptance gate.

The first fifth-audit implementation slice now establishes that route spine.
`ExecutionConstructionRoutesV3` is created before semantic execution rows. It
resolves each checked projection to its stable definition/interface owner once
and publishes one dense, parent-linked invocation overlay for every OUT call
occurrence, with one version-stable logical path identity and separate V3
overlay/table digest domains. The
transitional V2 handoff consumes these routes instead of rediscovering call
ancestry and definition ownership, after which the V3 construction table is
discarded rather than retained as a second image. The exact ignored NovyWave
semantic oracle and `boon_semantic --tests` compiler check pass. This is the
identity/route prerequisite for construction-owned executable rows; it is not
yet a cold or warm speed result and the V2 full-row mirror still must be
deleted.

The next route checkpoint binds every dense semantic expression and statement,
plus every static owner, to that construction spine after resource-authority
normalization has emitted its final generated rows. Production handoff now
consumes those bound routes; the old expression, statement, and owner route
rediscovery exists only under `cfg(test)` as an independent parity oracle. This
removes a second ownership decision from production and prevents generated
authority rows from falling outside the route image. It still deliberately
does not count as a performance result: V2 continues to allocate and hash the
47,296-row proof mirror, so the next architectural cut remains construction-
owned compact executable receipts followed by deletion of that mirror.

That compact executable-receipt cut is now implemented. Production seals
`SealedSemanticImageV3` from the final `CanonicalProgramCoreV2` rows, one
parent-linked invocation-overlay table, one compact projection table, and one
CSR relocation arena. The cumulative V2 path arena, rich V2 row mirror, and
duplicated expression-origin receipt family compile only under `cfg(test)` as
an independent parity oracle. Manifest V7 now consumes V3 identities through
direct checked-definition, authored-call-site, and parent-invocation edges; it
does not reconstruct cumulative paths.

One direct debug NovyWave verified sample records 30,771 execution rows, 4,158
projections, and 9,037 relocations, down from 47,296, 6,483, and 16,834. The
production execution seal is 38.109 ms of finalization plus 277.995 ms of V3
receipt publication, versus the preceding 1,829.237 ms V2 finalization trace.
Manifest falls from 1,701.115 ms to 410.015 ms. The complete directional sample
is 4,100.898 ms, with 2,328.883 ms in semantic construction, 258,232 KiB peak
RSS, and unchanged plan hash
`db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`.
The exact ignored V2/V3 NovyWave route-owner oracle and architecture gate pass.
This closes the production V2 mirror, not the persistent-compiler phase:
Manifest still imports compact checked/execution receipts into its own graph,
and all cold products are still discarded after the request.

### Sixth Whole-System Audit: One Persistent Definition-to-Runnable Graph

The construction-route work is a prerequisite, but it is not the final
architecture. A current-tree audit after `addb056` finds five whole-program
boundaries that still multiply the same authored program:

1. `boon_parser` can already issue an independent `ParsedSourceUnit`, but normal
   compilation immediately assembles all units into one rebased
   `ParsedProgram`. The session retains none of the unit products.
2. `VerifiedSemanticIntentV1` now runs before OUT expansion, but normal retained
   compilation seeds it with every ordinary callable. It therefore provides
   useful root classification without yet being the sole demand worklist.
3. `SemanticProgram` simultaneously owns the sealed checked/execution image,
   OUT, resource, reactive, lowering, view, storage, memory, canonical-core,
   and Manifest products. Manifest V7 then imports most of those graphs into a
   second projection graph, and `semantic_program_digest` serializes the full
   canonical core after its facts have already been committed elsewhere.
4. `CompilerSession::apply_updates` clears the complete checked slot on any
   changed unit. `boon_compilation_db::RequestMemo` implements revision,
   verification, and result backdating semantics, but has no production user;
   the cold proof graph is constructed and discarded instead of becoming the
   warm currentness graph.
5. document, migration, and row/scalar backends still lower ordinary callable
   roots independently, plan row-expression finalization clones the complete
   `MachinePlan`, and distributed closure re-elaborates all three roles on each
   round plus a confirmation pass.

These are larger than collection, hashing-buffer, or loop optimizations. They
also explain why the 16.7 ms affected-diagnostics budget cannot be reached by
making the existing cold pipeline merely faster. The replacement is one
persistent request graph whose result cells own the final products:

```text
empty database (cold) OR prior immutable ProjectSnapshot (warm)
  -> UnitSyntaxSnapshot[SourceUnitId]
  -> UnitInterface + DefinitionReceipt[StableDefinitionId]
  -> demanded DefinitionExecutableShard[definition, specialization]
       { code, typed rows, source map, proof receipt, outgoing relocations }
  -> InvocationOverlay[stable call route, specialization, bindings]
  -> DomainSummary[resource/reactive/storage/view/memory]
  -> BundleLinkSummary + relocation/SCC fixed point
  -> consuming RunnableImageBuilder
  -> SealedRunnableMachine + optional diagnostic/debug materializers
```

Cold and warm compilation must execute the same requests. Cold starts with no
memoized values; warm reuses or backdates unchanged results. There is no
whole-program cache beside the production compiler and no second dependency
graph beside proof. A query may retain its compact result or only its receipt,
depending on recomputation cost, but both use the same stable identity,
dependency list, `changed_at`, and `verified_at` data.

#### Architectural cuts and owner deletions

| cut | replacement owner | production owners deleted by the cut | primary benefit |
| --- | --- | --- | --- |
| unit snapshot | structurally shared `ProjectSnapshot` of canonical per-unit syntax and stable structural paths | repeated parsing of unchanged units and monolithic AST rebasing as the warm source authority | makes source edits proportional to changed units and gives call/definition identity a parser-owned route |
| definition artifact | one immutable checked interface plus one body/executable/proof shard per demanded definition variant | post-hoc checked/execution row scanners and repeated rich definition bodies | makes proof and executable construction a byproduct of the same work |
| demand/link | one root-driven work queue and compact invocation overlays | eager ordinary occurrence bodies, candidate-local contextual builders, and cumulative invocation-path expansion | prevents work instead of compressing it after expansion |
| semantic/domain seal | typed construction-owned tables plus compact summaries/CSR relocations | the multi-owner `SemanticProgram` graph set, Manifest re-import index, and full canonical-core rehash | removes the largest cold allocation and hashing multipliers |
| plan-code link | one definition code module linked into document, migration, row/scalar, and role roots | three recursive ordinary-call lowerers and per-occurrence body code | compiles ordinary logic once and bounds specialization explicitly |
| runnable seal | one consuming builder that constructs dense runtime indexes once | IR-plus-plan retention, whole-plan fingerprint clone/rewrite, repeated validation/digest/index construction | lowers peak live memory and eliminates final full-plan passes |
| retained service | the same request graph and immutable snapshots installed in `CompilerSession` | whole checked-slot invalidation and whole-runnable rebuild after a local edit | is the only credible path to the 16.7 ms/100 ms warm gates |
| bundle delta link | persistent role shards plus monotone producer/event deltas | three full semantic elaborations per fixed-point round and the full confirmation rebuild | makes distributed closure proportional to newly discovered crossings |

The unit-snapshot cut is deeper than caching the current assembled
`ParsedProgram`. Stable identities become `(SourceUnitId, structural route)`;
snapshot-local dense IDs are assigned only when a consumer asks for a packed
image. Formatting or an unrelated earlier insertion may change local content
or source-map coordinates without rekeying every later definition/call. The
complete diagnostics path can still materialize a project view, but that view
is not the retained source authority.

The demand cut must also finish the job begun by `VerifiedSemanticIntentV1`.
Each request is keyed by stable definition, type/layout specialization,
control/effect capability, and only the context coordinates that change
semantics. A direct pure ordinary call links the definition shard and supplies
an invocation frame; it does not instantiate another body. Stateful,
resource-owning, and consequential-effect definitions remain demanded, but
their scheduled roots are explicit rather than justified by retaining every
lexical call. Static branch selection happens before child requests are queued.

The compact link follows ThinLTO's useful shape: local artifacts publish small
summaries and relocations, a combined index resolves cross-artifact closure,
and consuming backends receive only demanded imports. It does not adopt LLVM
IR and does not defer Boon's verifier. Boon's proof is cheaper when the final
typed row receipt and dependency span are emitted by the same definition/domain
builder; a later Manifest compiler should have nothing left to rediscover.

The request graph should initially execute deterministically on one thread.
Once definition/domain requests are isolated from siblings, bounded parallel
evaluation may use at most the configured worker and memory budget. MLIR's
operation-pass rules are a useful constraint: a request may read immutable
ancestors and its declared inputs, but may not mutate sibling products or rely
on global pass state. Parallelism is an optional scheduler property, not a way
to hide graph explosion.

#### Crate boundaries after the model stabilizes

The current source inventory is itself a development-latency warning:
`boon_semantic` is about 77.7k Rust lines, `boon_typecheck` 41.8k, and
`boon_compiler` 26.3k; `machine_plan_backend.rs` alone is about 17.3k. Split
only at one-way ownership seams created above:

- a dependency-bottom semantic snapshot/receipt model crate;
- semantic construction and compact-link implementation crates;
- a dependency-bottom runnable-image contract plus a consuming builder;
- a persistent compiler-service crate containing revisions, cancellation, and
  publication policy;
- a plan-code linker separated from host/CLI integration.

Do not split today's mutually dependent algorithms into facade crates. Measure
the reverse Rust dependency closure and controlled rebuild before and after
each seam. A successful split moves stable contracts below volatile builders,
reduces downstream rebuild work, and preserves one runtime/compiler path; a
file move without those effects is rejected.

#### Implementation priority after the compact-receipt checkpoint

1. Consume the existing V3 projection registry into the shared sealed request
   graph without re-registering every owner, projection, and edge in Manifest.
   Derive the compact proof summary from that graph, retain its revision-zero
   request memos, and delete the production Manifest graph-import loop in the
   same tranche.
2. Promote parser unit products, stable structural definition/call routes, the
   checker definition/interface results, and `RequestMemo` into an immutable
   `ProjectSnapshot`. Prove unit reuse and result backdating before expanding
   the retained result surface.
3. Turn verified intent into the sole demand queue and publish one executable
   definition shard plus compact invocation frames. Delete replaced OUT/
   contextual body expansion and all three backend ordinary-body lowerers in
   vertical slices.
4. Move remaining semantic domains to construction-owned tables and replace
   the residual Manifest inventories with the compact summary/relocation
   linker. Fold the semantic digest from existing receipts rather than
   serializing the canonical core.
5. Land the consuming runnable builder, then the bundle delta linker, and only
   then split crates at the proven one-way seams.

Every cut records executed/reused/backdated requests, parsed/reused units,
changed interfaces, demanded/pruned definitions and branches, definition
variants, invocation overlays, linked relocations, proof rows, full-program
materializations, maximum simultaneously live artifact bytes, and cancellation
latency. A warm constant edit must report zero unrelated unit parses,
definition checks, semantic shards, proof components, and plan modules. Clean
full parity, exact dependency cones, stable migration/activation behavior, and
the independent rich/flat materializers remain correctness oracles; they are
not production fallback paths.

Primary architecture references for this whole-system cut:

- [rustc red/green query evaluation and stable fingerprints](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html)
- [Salsa backdating, revisions, and durability](https://salsa-rs.github.io/salsa/reference/algorithm.html)
- [Roslyn immutable solution/document snapshots](https://learn.microsoft.com/en-us/dotnet/csharp/roslyn-sdk/work-with-workspace)
- [TypeScript affected-file builder programs](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API)
- [Swift immutable declarations and fine-grained request evaluation](https://www.swift.org/blog/swift-5.2-released/)
- [LLVM ThinLTO summary-index linking](https://clang.llvm.org/docs/ThinLTO.html)
- [MLIR isolated operation pipelines](https://mlir.llvm.org/docs/PassManagement/)
- [Zig incremental-compilation CI and retained compiler work](https://ziglang.org/devlog/2024/)

Jai's implementation is not publicly documented enough to use as an
engineering contract. The useful comparison is therefore outcome-based:
complete cold diagnostics, cold verified runnable output, and warm affected
updates are measured separately. TypeScript-like affected-file reuse and
Jai-like perceived responsiveness require the persistent request path above;
they cannot be claimed from a faster full rebuild.

### Seventh Current-Tree Audit: Stop Reconstituting The Program

A second direct debug NovyWave sample at `96b1611` separates the remaining
owners after the V3 receipt win. It is directional edit-loop evidence, not a
scored release report:

| Stage | Current directional result |
| --- | ---: |
| complete verified artifact | 4,029.882 ms, 257,892 KiB peak RSS |
| parse / typecheck | 91.054 / 691.933 ms |
| semantic construction | 2,284.212 ms |
| contract verification / IR lowering / IR validation | 0.572 / 45.699 / 10.951 ms |
| backend total | 724.634 ms |
| backend pre-document / document / finalization | 574.313 / 106.577 / 30.042 ms |
| plan validation / serialization | 104.649 / 553.520 ms |
| cumulative allocation | 11,548,451 calls / 1,559,991,583 bytes |

The unchanged plan hash is
`db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`.
The sample confirms that the 82.7% execution-seal and 75.9% Manifest reductions
did not move work into verification or IR; the remaining time is distributed
across owners that each recreate a whole-program view.

The live ownership audit is more specific than the earlier conceptual graph:

1. `ParsedSourceUnit` is already a context-independent unit-local product, but
   canonical project assembly validates every unit, clones and qualifies its
   syntax, rebases every local token/line/item/statement/expression identity,
   and concatenates one global `ParsedProgram`. The NovyWave trace records
   115,683 rebased nodes and 1,049,157 parser-validation visits. Caching only the
   assembled program cannot make an edit proportional to the changed unit.
2. Every typecheck constructs a fresh `CheckedProgramDatabase`, including dense
   call/declaration/scope indexes, reverse inference dependencies, dirty queues,
   worklists, and inference caches, and then consumes it into one
   `CheckedProgram`. `CompilerSession` retains only an optional whole checked
   result and clears it for any changed unit. The checker already contains much
   of the required incremental machinery, but its lifetime is cold-only.
3. Manifest V7 first owns `DenseManifestProjectionIndexV7`, then
   `build_dependency_projection_graph_digests_v7` registers every owner and
   projection in a second `DenseProjectionGraphBuilder`, copies every edge,
   seals SCC/forward/reverse data, extracts owner digests and statistics, and
   drops the graph. `RequestMemo` has correct revision/backdating semantics but
   no production caller. This is the first owner to delete because the retained
   graph is also the currentness backbone required by every later cut.
4. `SemanticProgram` retains the semantic image plus OUT/resource/reactive/
   lowering/view/storage/memory/core/Manifest products, while the verified
   lowering handoff consumes only canonical core and two digests. The semantic
   digest hashes the complete canonical core again. These domain products must
   become request results or borrowed/debug views, not parallel retained
   authorities.
5. Ordinary callable bodies are recursively rebound and lowered independently
   by the document backend, migration expression lowerer, and row/scalar
   lowering path. Backend pre-document work is 574.313 ms. One demanded typed
   definition-code shard with relocations and compact invocation frames must
   replace all three body interpreters; sharing only their lookup cache would
   preserve three semantic authorities.
6. `refresh_typed_list_view_fingerprints` clones the complete `MachinePlan`
   before rewrite/compaction/validation, while distributed linking clones every
   checked role and fully re-elaborates Client/Session/Server on every round and
   again for confirmation. A consuming runnable builder and a delta role linker
   delete those final whole-product loops after definition/domain requests are
   stable.

The architectural unit is therefore a result-owning request cell, not a phase
cache. A cell has a stable semantic identity, declared dependency span, compact
result or receipt, public and implementation fingerprints, `changed_at`,
`verified_at`, and work counters. Unit syntax, interface, definition body,
definition code, invocation overlay, domain summary, bundle link, and runnable
seal are different typed cell kinds in one graph. Revision zero and later
revisions run the same evaluator.

The first tranche is intentionally narrow but must be destructive: make the
V3/remaining-domain projection registry feed one sealed request graph directly,
derive Manifest's root/callable proof summary from it, retain that graph with
the compilation snapshot, and remove
`build_dependency_projection_graph_digests_v7` plus its second registration and
edge arenas. A generic database facade while the import loop survives is a
failed tranche. The next tranche installs structurally shared unit snapshots
and durable checker interface/definition cells in `CompilerSession`; a final-
artifact cache is likewise rejected.

Only after those identities and lifetimes are real should source be split into
smaller crates. The measured seams are model versus builder for semantic
receipts, model versus consuming builder for runnable images, and persistent
compiler service versus one-shot adapters. Splitting the present mutually
dependent 77 kLOC semantic, 38 kLOC typecheck, or 26 kLOC compiler algorithms
before those cuts would add interfaces without reducing either Boon work or the
Rust invalidation cone.

#### First seventh-audit tranche: retain the graph that proof already built

The first owner deletion is now implemented. Manifest V7 registers its compact
projection identities as pending requests in one `DenseProjectionGraphBuilder`,
publishes each finalized local receipt exactly once, adds checked, execution,
remaining-domain, and owner edges to that same graph, and seals it as a
revision-zero `SealedRequestGraphSnapshot`. The snapshot retains stable identity
lookup, exact forward/reverse CSR, SCCs, local digests, and one `RequestMemo` per
request. Missing or duplicate receipt publication fails closed.

The former `build_dependency_projection_graph_digests_v7` registration and
edge-copy pass is deleted. Manifest root and callable implementation digests now
come from the retained graph, `SemanticProgram` cross-checks that graph against
its compact proof, and the unsealed compiler handoff transfers it to
`CompilerSession` before the ordinary sealed plan artifact is published. A
source update invalidates the verified artifact but keeps the last verified
snapshot available until a newer revision publishes atomically; normal sealed
runtime artifacts do not retain compiler currentness state.

A direct current debug NovyWave trace has 8,315 graph nodes, 29,131 edges,
7,992 components, three cyclic components, a maximum SCC of 296 nodes, and
26,218 component edges. Manifest takes 415.329 ms. The corresponding untraced
sample is 4,112.475 ms at 260,660 KiB peak RSS, with the unchanged plan hash
`db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab` and
11,541,264 allocation calls / 1,558,986,278 allocated bytes. Compared with the
adjacent `96b1611` directional sample, allocation calls fall by 7,187 and bytes
by 1,005,305, while total time and RSS are noisy/slightly worse. This is an
authority/currentness checkpoint, not a latency exit and not warm reuse yet.

The next tranche is therefore not another graph-container optimization. Retain
immutable parser-unit products and checker interface/definition result cells in
the same session-owned snapshot, make their exact dependencies and backdating
observable, and prove a changed unit does zero unrelated parse/check work. Only
then turn verified intent into the sole demand queue and delete the repeated
ordinary-body expansion/link owners. The larger architecture must be
re-researched at this boundary before selecting implementation details; a
whole-program cache, query facade, or premature crate split remains rejected.

### Eighth High-Level Audit: Definition Artifacts And Thin Linking

The required post-`d177af9` architecture research is complete. Its detailed
decision record is
[`BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md`](BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md).
The retained graph checkpoint deleted a real owner, but the new audit corrects
one important abstraction: the graph is presently a sealed semantic proof
snapshot, not yet the compiler's complete request evaluator. Its nodes are
mostly proof projections and its edges include runtime/reactive cycles. Using
those cycles directly for compiler currentness would turn the measured
296-node semantic SCC into an unnecessarily coarse invalidation unit.

Keep one `CompilationDb` and one identity registry, but publish two typed edge
planes from every artifact: evaluation/currentness dependencies and proof/link
relocations. The former schedules and backdates compiler work; the latter
preserves executable semantics and may contain cycles. This separation does not
create duplicate semantic authorities because one construction owns both
spans.

The live trace ranks the larger owner deletions above new micro-optimization:

1. `checked_image_handoff` rescans and canonical-serializes the already-built
   checked program during `assemble_report`; this post-hoc handoff accounts for
   about 392 ms in the current directional typechecker trace. Checker requests
   must emit immutable definition receipts directly and delete that scan.
2. The normal semantic product retains a sealed image plus the rich OUT,
   resource, reactive, lowering, view, storage, memory, core, Manifest, and
   graph products. A definition artifact spanning checking, semantic rows,
   proof relocations, and plan code replaces those parallel phase authorities.
3. Document, row/scalar, and migration backends recursively lower ordinary
   bodies independently. One `DefinitionExecutableArtifact` plus compact
   resolved invocation frames must delete all three traversals for each
   migrated definition.
4. A ThinLTO-like link over compact summaries and relocations replaces
   monolithic semantic merge, duplicate canonical-core hashing, and complete
   distributed role re-elaboration. Only demanded definition/domain artifacts
   are materialized.
5. One consuming runnable builder owns final dense IDs, validation, executor
   indexes, and the canonical sectioned artifact. Normal in-memory preview does
   not clone a completed `MachinePlan`, rebuild executor metadata, or serialize
   pretty JSON.

The selected first implementation tranche is now stable syntax/item identity
plus a real typed request database, followed immediately by interface SCC and
per-definition checker results. `StableDefinitionKey` is source-unit identity
plus a parser-owned item route; `StableOccurrenceKey` adds a parser-owned
structural body route. Raw source substrings, lines, offsets, global dense IDs,
and revision-local arena IDs are forbidden as cross-revision keys. Public
interface and implementation fingerprints are separate so a body edit can
backdate its unchanged interface and leave unrelated definitions green.

After that foundation, migrate one ordinary definition end to end through a
definition executable artifact and delete its old checker handoff, OUT/
contextual expansion, and all matching backend body lowerers in the same
vertical slice. Continue across semantic domains, thin link, consuming runnable
publication, distributed deltas, and only then measured crate extraction and
two-worker scheduling. The first warm proof is exact zero unrelated parse,
interface, definition, semantic, proof, plan-code, and runnable work for a
constant/body edit; retaining revision-zero cold memos alone does not count.

#### First eighth-audit tranche: retain unit syntax and parser-owned item routes

The first syntax/session boundary is implemented. Every `ParsedSourceUnit` now
owns a body-insensitive `UnitItemIndex`; authored definitions use
`StableDefinitionKey { SourceUnitId, StableItemRoute }`, whose segments contain
header kind/names and an ordinal only among matching siblings. Function body
statements, offsets, lines, and local dense ids are excluded, so an unrelated
item insertion or body change preserves the definition key even when the local
statement id moves.

`CompilerSession` retains each unit artifact behind `Arc`, reparses cache misses
with the exact project-unit parser boundary, and assembles reused plus changed
units through the ordinary canonical assembler. Content updates evict only the
changed `SourceUnitId`; atomic upsert/remove/rename validates a complete
candidate project before publication and retains every unchanged unit. Parser
work now distinguishes attempted, parsed, and reused units. Producer format V4
and the warm verifier require a one-unit edit to report exactly one attempted/
parsed unit plus `N - 1` reused units; cold reports still require all units
parsed and zero reused.

Focused parser equivalence, stable-key, changed-unit reuse, topology atomicity,
and verified-session reuse tests pass. This is not the warm exit: canonical
assembly still clones/rebases and validates every unit, and typechecking still
rebuilds the whole `CheckedProgramDatabase`. The next tranche below completes
parser-owned structural occurrence routes; typed evaluation request slots,
interface SCC and checked-definition results, and deletion of
`checked_image_handoff` remain next.

#### Second eighth-audit tranche: structural occurrence identity

Parser-owned structural routes now identify every checked call/pipe occurrence
by source-unit identity, nearest stable item owner, statement shape/name route,
and typed expression-child route. The checker no longer hashes raw source
substrings or runs a second identical-authored-site counting pass; argument,
callee-body, formatting, and unrelated-earlier-call edits leave the occurrence
identity stable while their semantic payloads still change. Focused parser and
typechecker stability tests pass, and the verified NovyWave plan hash remains
`db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`.

This tranche is identity infrastructure, not a latency claim. Its initial
post-parse route builder adds roughly 16--24 ms to directional NovyWave debug
parsing; a current verified sample is 4,251.545 ms/282,756 KiB. Do not enter a
container/allocation tuning loop here. First add typed evaluation request slots
and interface/definition shards, emit final checked receipts during checking,
and delete the approximately 392 ms `checked_image_handoff`. Revisit route
storage only after those macro owner deletions, preferably by fusing route
emission into parsing or retaining compact parent/slot metadata rather than a
second identity representation.

### Ninth High-Level Audit: Unit-Native Syntax, Normalized Facts, Phase Seals

The required post-`e510726` macro audit is complete and recorded in
[`BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md`](BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md).
It preserves definition artifacts and thin linking while correcting three
remaining batch assumptions. Retained source units must remain unit-native
through checking rather than feed a reconstructed global `ParsedProgram`;
semantic domains must publish one normalized typed fact/relocation store rather
than overlapping rich graph products; and trusted phase boundaries must close
construction-owned compositional section seals rather than rescan completed
snapshots.

The fresh trace ranks those cuts ahead of route/container work: canonical
bundle validation plus assembly is 54.017 ms, checked `assemble_report` is
408.603 ms, execution receipt projection is 286.195 ms, Manifest/request-graph
construction is 422.191 ms, backend pre-document work is 764.193 ms, plan
validation is 106.305 ms, and the scored explicit export serializer is
535.917 ms. These times overlap and are not additive promises.

The audit's resumption order was therefore: delete production global syntax
assembly and revision-global syntax keys; implement typed interface/definition
requests and delete `checked_image_handoff`; carry a definition through one
normalized semantic/plan-code artifact while deleting all matching recursive
body owners; migrate domains to delta fact sections; thin-link and verify an
opaque linked image; consume it into one runnable machine; then land
distributed deltas and measured model/builder/link crate seams. The first cut
is now landed; the Tenth Audit below refines its identity/request boundary. Do
not split crates or tune structural route storage before the owner boundary it
serves is real.

The first safe intent cut is now landed ahead of M1: diagnostics publishes a
completed `CheckedProgramConstruction`, not a runtime `CheckedProgram`, and
therefore performs no checked-image handoff scan. A later verified request
consumes those exact fields, rebinds their parser source digest, builds the
handoff once, and grants the existing opaque checked capability; no second
parse or type solve occurs. A fresh debug NovyWave empty-session diagnostic is
422.445 ms/92,432 KiB, with `assemble_report` observed at 2.083/2.133 ms rather
than the earlier 408.603 ms. The canonical verified plan hash remains
`db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`.

At that checkpoint the session still constructed global syntax and a whole
checked graph, and verified publication still ran the 63,657-row scanner. M1
has since removed the first owner; the post-M1 audit below supersedes the old
resumption sentence. Do not regress by dropping the checked construction and
re-typechecking after diagnostics.

### Tenth High-Level Audit: Typed Identity, Real Requests, Demand-Owned Images

Checkpoint `a48f488` completes the production unit-native syntax cut. The
persistent session now checks `ProjectSyntaxSnapshot` directly, reports zero
rebased nodes, reuses unchanged `Arc<UnitSyntaxSnapshot>` values, and preserves
exact diagnostics plus the canonical verified NovyWave plan artifact against
the assembled-syntax oracle. M1 is therefore an architecture checkpoint, not a
performance exit: fresh directional diagnostics remains about 467--489 ms and
verified compilation about 4.21--4.24 seconds.

The cut exposed a more important boundary than another parser or solver
micro-optimization. Tagged unit syntax lookup IDs and dense `CheckedExprId`
slots still pass through integer-shaped APIs. `TypecheckExpressionArena::get`
accepts either plane by trying a syntax lookup and then a dense-slot fallback.
During M1, mixing the planes caused a lexical-scope cycle and a false
seven-round inference fixed point; explicit checked-to-syntax conversions
restored the canonical 36-round result and artifact. The next tranche must make
that class of bug unrepresentable before it persists request keys:

- unit syntax expression/statement keys, checked definition-local slots,
  stable definition/occurrence keys, and linked image IDs are distinct types;
- one construction-owned translation table crosses each phase boundary;
- syntax arenas never accept dense checked slots, and checked tables never
  accept packed syntax lookup IDs;
- no revision-local dense value enters evaluation identity, a stable
  fingerprint, proof relocation identity, or persistence identity.

The parser trace also changes the M1 follow-up. Eight NovyWave AST builders take
about 45.5 ms, but the complete parse phase takes about 106.7 ms and reports
1,050,467 validation visits. Project linking currently namespaces functions by
mutating the AST, rebuilds a full validation index, revalidates it, walks it to
pack project IDs, and then walks statements/items again for the project
snapshot. Replace that unit-wide relinking pass with parser-native typed local
IDs and parent/item metadata plus an immutable module/name-resolution and
source-layout overlay. Optimizing the current route maps while retaining those
passes is rejected.

The name `CompilationDb` must not obscure the current state. Production builds
a `SealedRequestGraphSnapshot` only after semantic/Manifest construction,
initializes cold memos, stores it in `CompilerSession`, and never consults it to
decide what to execute after an edit. Any source update clears the complete
checked result; verified compilation reconstructs the whole pipeline. The next
database tranche is therefore a real typed evaluator, not another receipt
graph:

1. request keys and typed result slots exist before evaluation;
2. the evaluator records evaluation dependencies while requests run, detects
   cycles, owns generation-checked publication, and retains values plus stable
   fingerprints across revisions;
3. backdating and reverse cones drive actual parse/interface/body/semantic/
   plan work; proof/link relocations remain a separate typed edge plane;
4. diagnostics, verified preview, debug views, and export enter through
   different roots over the same memoized definition results.

Fresh current-tree traces rank the vertical owner deletions after that
foundation. Diagnostics is 106.694 ms parse plus 350.876 ms typecheck. Checked
construction spends about 45 ms initializing global indexes, 56 ms on
contextual schemes, 88 ms in the 36-round checked fixed point, and 28 ms on
ordered diagnostics. Verified publication adds the 63,657-row checked handoff,
raising the typecheck phase to about 763 ms. Replace this with interface-SCC
requests and definition-body requests under frozen interfaces; construction
emits checked rows/receipts directly and diagnostics only aggregates local
diagnostics.

Semantic construction remains the dominant 2.28-second owner. Verified intent
exists, but production deliberately retains all 312 eligible ordinary
definitions; 1,821 checked call sites become 3,494 OUT call instances, a
16,525-expression execution image, a 30,771-row execution handoff, and an
8,315-node/29,131-edge Manifest graph. `SemanticProgram` simultaneously owns
the sealed image plus OUT, resource, reactive, lowering, storage, view, memory,
canonical-core, Manifest, and request-graph products. It then spends about
515 ms building execution, 277 ms reconstructing execution receipts, 415 ms
re-importing receipts/rich domains into Manifest, and 99 ms hashing the whole
semantic product.

The selected post-M2 vertical slice is one demand-rooted
`DefinitionExecutableArtifact`. It owns a checked-body receipt, semantic fact
rows, definition plan code, source map, evaluation edges, and proof/link
relocations. Occurrences own compact resolved frames. Land it through one
representative ordinary definition and delete that definition's matching OUT/
contextual expansion and document, row/scalar, and migration recursive body
lowerers in the same flag-day tranche. Then expand the normalized fact store by
domain, delete each rich graph and Manifest inventory, thin-link summaries and
relocations, and consume the verified link into one runnable image.

This is the path that can change the asymptotics and the 1.61 GB/approximately
11.9 million allocation churn of one NovyWave verified request. Crate splits
remain valuable only after these owner seams exist: checked model versus
checker/evaluator, semantic artifact model versus builders, thin-link model
versus linker, and runnable model versus builder/executor. Each split still
requires a measured Rust rebuild-cone reduction and cannot stand in for Boon
latency evidence.

### Eleventh High-Level Audit: Greenfield Dense Compiler Kernel

The 2026-08-13 implementation checkpoint starts the replacement engine in this
repository as `boon_compiler_kernel`. A separate repository is rejected: the
new engine needs the same parser contracts, checked type vocabulary, stable
identities, differential fixtures, and flag-day consumers. Keeping it here also
lets each migrated owner be compared against the current compiler before the
old implementation is deleted. The kernel is dependency-bottom and currently
depends only on `boon_checked`; an architecture test forbids dependencies on
the parser, typechecker, semantic, verifier, IR, plan, or compiler crates.

The first vertical slice is deliberately not a wrapper around `TypeUnifier`.
It owns:

- an immutable, hash-consed type-term DAG with dense `u32` IDs;
- compact variables and packed operations instead of source nodes and generic
  edge-role searches during evaluation;
- one reverse-consumer CSR index and a mutation-driven work queue;
- explicit single-writer directional publication (`Union`,
  `StructuralWiden`, and exact `Replace`) plus symmetric `Unify`;
- detached projection equations, including empty-path reads, so provider
  epochs refresh chained consumers without treating old producer holes as
  consumer requirements;
- one component solve spanning cross-owner expression references without
  reconstructing or recursively dispatching an owner evaluator.

The differential harness is test-only and feature-gated as
`test-kernel-oracle`; normal compiler binaries do not contain it and there is no
production fallback. Its first version imported legacy owner constraint seeds.
That bridge has now been deleted. The harness projects its supported subset
straight from borrowed parser owner views into the compact kernel program; it
does not construct `OwnerSyntaxInput`, a lexical plan, `OwnerConstraintSeed`, an
interface result, or a body transfer graph. Closed `SOURCE` payloads remain
explicit ABI inputs, but those inputs are now projected by a separate
parser/host-contract pass without constructing a checker database or running
either old type solver. The old compiler runs only after the candidate path as
the differential oracle and contributes no candidate input. Unsupported owners
are classified explicitly and recursively removed from the supported component
rather than silently falling back.

The direct parser slice also has invocation-local formals and acyclic user-call
composition. Every call allocates a fresh formal frame and compiles the callee
nodes into the caller's component. A recursive backedge may address only the
callee principal frame; the evaluator never recursively dispatches an owner.
A parser-backed differential calls one generic function with Number and Text
and proves that the two result records remain independently specialized.
Formal occurrences and formal requirements live in separate invocation-local
frames: projected reads flow from the supplied actual, while constraints found
inside the callee flow back through the requirement frame without replacing or
sharing the actual's provider root. The compact program now also owns mode
equations, cyclic HOLD structural widening, numeric/boolean infix residuals,
and compiled singleton-pattern selection. These are work-queue operations, not
recursive re-entry into an owner evaluator. Generic match-arm correlation,
OUT formals, block-local lexical bindings, and most builtin ABIs remain explicit
unsupported boundaries.

The ignored NovyWave probe is the repeatable timing and semantic gate:

```bash
cargo test -p boon_compiler --features test-kernel-oracle \
  kernel_oracle::tests::novywave_kernel_timing_probe -- \
  --ignored --exact --nocapture
cargo test --release -p boon_compiler --features test-kernel-oracle \
  kernel_oracle::tests::novywave_kernel_timing_probe -- \
  --ignored --exact --nocapture
```

It times source-bundle loading, unit-native parsing, the independent direct
SOURCE-ABI projection, owner projection, dependency pruning, dense program
compilation, solve, and artifact projection independently. The old compiler
runs afterward only as the differential oracle and is timed outside every
candidate total. The probe also reports owner coverage,
operation/activation/mutation counts, and ranked unsupported classes. Timing
values never enter the deterministic report. Rust test compilation time is
reported by Cargo separately and is not counted as Boon compilation latency.

The first 2026-08-13 release probe exposed the old bridge rather than the new
solver: 294.365 ms of a 298.094 ms kernel request was owner projection, while
dense compile plus solve took 1.244 ms. Splitting that projection showed, in a
debug build, approximately 850 ms in legacy constraint-seed construction and
360 ms in legacy owner-syntax projection. Deleting the constraint seed reduced
the debug kernel request from approximately 1.26 s to 469 ms. Deleting the
owner-syntax artifact and projecting directly from parser views reduced it
again to 25--56 ms in subsequent debug samples, depending on the enabled call
slice and differential checks.

The current call-composition release slice solves 216 of 1,397 NovyWave owners
exactly in 13.473 ms of kernel time: 9.236 ms direct projection, 0.332 ms program
compilation, 1.211 ms solve, and 0.731 ms artifact projection. Unit-native parse
was 78.563 ms and the independent SOURCE ABI pass was 10.439 ms, making the
candidate compile 102.475 ms after source loading or 105.818 ms including the
3.343 ms bundle read. The old compiler differential oracle took 272.626 ms and
is outside the candidate total. A debug sample was 207.833 ms after source
loading versus 1,080.897 ms for the old oracle. The same slice performs 483
operations, 990 activations, and 507 mutations. Every supported NovyWave owner
and expression is compared with the current checked artifact, alpha-normalized
within its owner; an empty-LIST mismatch found by this gate repaired the kernel
before these measurements were accepted. The optimized Rust test rebuild took
1 minute 58 seconds and is intentionally reported separately from every Boon
timing above.

The release breakdown changes the next speed priority. Dense compilation and
solving are only about 1.5 ms combined; cold parsing is roughly 77% of the
candidate path. `CompilerSession` already retains unchanged immutable unit
snapshots, so the probe must not misrepresent that 79 ms as an unavoidable warm
cost. On the retained project snapshot, the still-full SOURCE ABI plus kernel
passes total 23.912 ms. The next cut must install these artifacts in the typed
session request graph and measure one changed unit/affected SCC; merely rerunning
the cold probe would ignore existing parser reuse, while micro-optimizing the
1.2 ms solver would optimize the wrong layer. Cold one-shot workflows can then
attack snapshot construction separately.

Counter and TodoMVC retain exact supported-subgraph parity at 10 of 24 and 13
of 43 owners. Their direct debug kernel requests are now approximately 1--3 ms,
down from the initial seed-bridge observations of approximately 16 ms and 57
ms. All numbers remain directional migration-slice receipts, not a production
compiler speed claim: most NovyWave owners are still explicitly unsupported,
and no production request has cut over.

The next measured boundary, after direct PASSED composition, cyclic HOLD,
infix, and compiled selector residuals, solves 366 of 1,397 NovyWave owners.
The 2026-08-13 release sample is:

- 3.572 ms source-bundle loading;
- 87.699 ms cold unit-native parsing;
- 12.539 ms independent SOURCE-ABI projection;
- 19.067 ms owner projection, of which 18.475 ms is direct syntax projection;
- 3.673 ms dependency pruning;
- 1.304 ms dense program compilation;
- 8.349 ms work-queue solve;
- 1.093 ms artifact projection;
- 39.766 ms total kernel time, 52.305 ms on a retained parsed snapshot, and
  143.576 ms cold including bundle loading;
- 299.932 ms for the separate old-compiler differential oracle.

This is approximately 2.1x faster than the old checker on that cold partial
slice, but it is not a whole-compiler speedup: 1,031 owners are still rejected
or dependency-pruned, and semantic construction, verification, and planning
have not cut over. The optimized Rust rebuild for this checkpoint took about
89 seconds and is reported separately.

An intermediate debug sample after adding parser-native render-constructor residuals
projected 854 owners but still solved 366 because every newly admitted render
owner depended on an unsupported owner. It measured 9.158 ms bundle loading,
98.275 ms parse, 55.052 ms SOURCE ABI, 95.491 ms owner projection, 8.945 ms
dependency pruning, 6.326 ms dense compilation, 60.480 ms solve, 2.892 ms
artifact projection, and 350.299 ms cold candidate total versus 1,084.254 ms
for the old oracle. That checkpoint prevents a misleading coverage claim and
sets the next target: shared builtin/pipe residuals and lexical/block closure,
not additional render-constructor special cases.

The next accepted boundary adds one compiled pure-builtin residual family,
more render constructors, and identity-preserving nested projection. Repeated
PASSES through the same nested path now retain one alpha even when unrelated
sibling projections are interleaved; the solver no longer recursively
materializes an unresolved scaffold before unifying it. The debug NovyWave
sample solves 402 owners and leaves 995 unsupported, with only five direct Call
and five direct Pipe rejections. It measured 9.342 ms bundle loading, 98.844 ms
parse, 54.564 ms SOURCE ABI, 96.624 ms owner projection, 10.373 ms dependency
pruning, 9.484 ms dense compilation, 57.497 ms solve, 3.090 ms artifact
projection, and 353.458 ms cold candidate total versus 1,088.866 ms for the old
oracle.

The corresponding optimized sample solves the same 402 owners with 3.482 ms
bundle loading, 78.139 ms parse, 9.947 ms SOURCE ABI, 17.092 ms owner
projection, 4.273 ms dependency pruning, 1.854 ms dense compilation, 8.474 ms
solve, and 0.931 ms artifact projection. Kernel time is 37.595 ms, the retained
snapshot candidate is 47.542 ms, and the cold candidate including bundle
loading is 129.163 ms. The separate old oracle is 259.449 ms, so this partial
cold path is approximately 2.0x faster. The optimized Rust rebuild took 69
seconds and remains outside the Boon timings. The program has 3,766 compact
operations, 7,598 activations, 4,043 mutations, and 20,256 dynamic dependency
edges. These counts and timings are the new baseline; they are not a production
or whole-compiler speed claim.

The next architecture checkpoint adds replayable authored-order record spreads,
structural widening across union members, and the first call-residual sharing
cuts. Coverage rises from 436 to 583 of 1,397 NovyWave owners. This is a useful
warning against comparing partial slices only by wall time: the newly admitted
call graph expanded the transitional flat component from 7,211 to 69,029
operations. Before simplifying the representation, the accepted release probe
measured 365.454 ms of kernel work: 35.573 ms projection, 67.408 ms program
compilation, 246.881 ms solve, and 2.607 ms artifact projection. The matching
debug solve used 1,827.973 ms. This regression is coverage-driven residual
duplication, not evidence that record overlays themselves are expensive.

The first systematic deletion pass now:

- reuses a definition's principal result when its complete result cone is
  formal-independent;
- aliases invocation formals directly to caller provider roots and memoizes an
  identical `(definition, actual roots, static selector surface)` application,
  while preserving private occurrence and requirement roots for genuinely
  different calls;
- replaces per-operation recursive live-dependency discovery with a compact
  variable-to-dependent-variable graph updated only when a binding changes;
- keeps exact publications, records, projections, and equality constraints as
  lazy immutable term-DAG links, resolving only the outer shape required by an
  operation and materializing recursively at artifact output.

The accepted post-cut release probe keeps all 583 results and differential
checks green while reducing the flat program to 65,044 operations. It measures
3.461 ms bundle loading, 80.984 ms parsing, 10.869 ms SOURCE ABI projection,
29.400 ms owner projection, 5.102 ms dependency pruning, 70.021 ms program
compilation, 135.186 ms solve, and 2.663 ms artifact projection. Kernel work is
248.334 ms; the retained-snapshot candidate is 259.203 ms and the cold candidate
including bundle loading is 343.648 ms. The independent old oracle used
258.089 ms in the same process. The comparable debug checkpoint reports
1,754.848 ms kernel work and a 1,216.810 ms solve. Dynamic dependency storage is
55,814 edges rather than the pre-cut 204,627, although lazy links intentionally
cause 139,263 cheap activations. These timings are still a partial diagnostics
slice, not a production speed claim.

The flat component remains transitional. A 65k-operation program for 583 owner
results still embeds callee operations per distinct invocation frame, and its
70 ms compile plus 135 ms solve already consumes almost the whole final
diagnostics envelope before complete coverage. The next flag-day cut is one
compiled typed residual module per definition/SCC plus compact invocation
frames. Acyclic calls reference modules; they do not clone operation payloads.
Frame-local cells preserve call isolation, recursive edges address the SCC's
principal fallback, and only demanded outputs instantiate work. The lazy term
DAG and binding-dependency graph remain the evaluation substrate. Do not spend
the next tranche tuning BTreeMap lookups inside the flat representation and do
not admit another large semantic family until modular operation/frame counts
and timing are reported.

A follow-up scheduler cut removes the last allocation-heavy work-queue path.
The old `schedule_variable` created a set and vector, traversed transitive
binding dependencies, then sorted and deduplicated operation IDs for every
mutation. The replacement retains generation-stamped variable visitation and
one reusable traversal stack; the existing queued bitset directly deduplicates
operations. Traversal remains deterministic because CSR consumers and binding
dependents are stored in canonical order. Immutable operation payloads are also
shared and evaluated by reference rather than cloning boxed inputs on every
activation.

The accepted release result after that cut is 197.174 ms of kernel work:
31.358 ms owner projection, 5.134 ms pruning, 71.207 ms flat-program compile,
80.946 ms solve, and 2.579 ms artifact projection. SOURCE ABI projection adds
10.890 ms, making the retained-snapshot candidate 208.064 ms. Cold parsing is
81.881 ms and bundle loading 3.400 ms, making the partial cold candidate
293.345 ms. The old parse/typecheck oracle took 250.457 ms in the same process.
The comparable debug solve falls from 1,216.810 ms to 442.356 ms and total
kernel work from 1,754.848 ms to 970.296 ms. Work remains 65,044 operations,
139,210 activations, 80,709 mutations, and 55,814 binding-dependency edges;
the speedup therefore comes from deleting scheduler overhead, not from doing
less semantic work or reducing coverage.

This makes the next boundary unambiguous. Solve is no longer catastrophically
more expensive than compilation; flattened residual construction and
evaluation are now comparable 71/81 ms owners. Shared definition modules and
compact frames must remove both together. A representation that merely makes
the current operation loop a few percent faster is insufficient, because
complete owner coverage and later semantic/plan consumers have not yet entered
the timing envelope.

The shared-module boundary now exists. Each definition principal and each
distinct `(definition, static selector surface)` specialization compiles one
immutable typed residual module. Invocation occurrences retain only compact
variable frames and linker tables; they do not clone the module's operation
payload. Module forward dependencies are computed once and remapped at link
time. The mutable solver state is separated from immutable executable code, so
hot evaluation borrows shared operations directly. Projection instructions can
write directly into their declared occurrence, deleting the former
projection-temporary plus equality/publication adapter pair. The final linker
orders the initial pass from single writers and module forward dependencies;
the ordinary mutation queue handles only subsequent propagation and the nine
instructions outside the acyclic prefix.

The physical/logical counters prove this is architectural sharing rather than
a timing-only change. NovyWave uses 769 residual type modules and 2,283 frames.
Their 13,796 physical operations serve 58,540 linked operations (4.24x reuse),
and 18,381 physical module terms serve 52,087 linked terms (2.83x reuse).
58,531 of 58,540 linked operations receive a provider-before-consumer initial
order. Compared with the accepted flat checkpoint, linked operations fall from
65,044 to 58,540, activations from 139,210 to 109,988, mutations from 80,709 to
58,606, and dynamic binding-dependency edges from 55,814 to 13,303. The
remaining activation mix is 28,358 equality, 50,438 publication, 17,585
projection, 7,682 selection, and 5,925 record activations.

A three-run debug checkpoint keeps all 583 results exact. Its median is
809.282 ms of kernel work: 162.612 ms owner projection, 15.682 ms dependency
pruning, 330.046 ms compile/link, 281.681 ms solve, and 8.139 ms artifact
projection. SOURCE ABI makes the retained-snapshot median 863.889 ms; parse and
bundle loading make the cold median 971.766 ms. The debug numbers are retained
for edit-loop direction, not release acceptance.

The accepted optimized checkpoint measures 3.443 ms bundle loading, 80.787 ms
parsing, 11.578 ms SOURCE ABI, 29.659 ms owner projection, 5.629 ms pruning,
83.134 ms compile/link, 56.238 ms solve, and 2.676 ms artifact projection.
Kernel work is 183.211 ms, the retained-snapshot candidate is 194.789 ms, and
the cold candidate is 279.019 ms. The independent old oracle took 259.238 ms in
the same process and remains outside candidate latency. Against the best flat
checkpoint, modular kernel and retained-snapshot time improve by about 7% and
solve improves by about 31%, while compile/link rises from 71.207 ms to
83.134 ms. That shift is intentional evidence for the next cut: compile one
immutable definition/SCC result summary plus compact residual module instead
of recompiling/relinking equivalent call surfaces inside one cold request.
Cross-revision persistence and currentness remain deliberately later than the
complete cold checker gate. The optimized Rust rebuild took 1 minute 28
seconds and is outside all Boon timings.

The 2026-08-14 semantic expansion reaches 718 of 1,397 NovyWave owners. It also
exposes the next representation multiplier: 947 physical residual modules with
17,336 operations are linked through 4,931 frames into 137,993 executable
operations. Only 17 linked operations are outside the static acyclic prefix.
Before the scheduler cut, a three-run debug median used 2,054 ms of kernel time,
including 751 ms to compile/link and 1,060 ms to solve, with 263,896 operation
activations. This is coverage-driven frame expansion; it must not be compared
to the smaller 583-owner checkpoint as if it were the same workload.

The solver now installs equality/scaffold equations once before directional
providers, never schedules those persistent equations as reactive work, and
does not reschedule the currently executing operation merely because it wrote
its own acyclic output. The 17 cyclic operations retain ordinary self-replay.
This reduces deterministic work to 145,326 activations for 137,993 operations.
A runtime attempt to rebuild the whole DAG through canonicalized nested binding
dependencies was measured and deleted: it increased solve time to 1,431 ms and
dynamic edges to 64,279. The retained split is both smaller and faster.

The accepted three-run debug median after this cut is 1,683 ms kernel time:
194 ms owner projection, 29 ms dependency pruning, 760 ms compile/link, 678 ms
solve, and 7 ms artifact projection. SOURCE ABI makes the retained-snapshot
median 1,741 ms; parsing and bundle loading make the cold median 1,848 ms. At
the same 718-owner coverage this improves kernel time by about 18%, solve by
about 36%, and cold candidate time by about 17%.

The corresponding optimized median is 3.447 ms bundle loading, 77.803 ms
parsing, 10.392 ms SOURCE ABI, 33.898 ms owner projection, 9.012 ms dependency
pruning, 181.839 ms compile/link, 166.426 ms solve, and 2.778 ms artifact
projection. Kernel work is 399.667 ms, the retained-snapshot candidate is
410.031 ms, and the cold candidate is 491.005 ms. The independent old oracle
used 249.099 ms in the same process and remains outside candidate latency. This
partial kernel is therefore not yet a whole-compiler speed win: the next large
cut must eliminate linked frame expansion and make the 17,336 physical module
operations immutable definition/SCC artifacts rather than optimize the
remaining 5% replay. Warm persistence is not an acceptance shortcut for this
cold-path cut.

The first frame-expansion cut reuses every formal-independent expression from
the definition principal, not only a wholly formal-independent result. An
invocation allocates cells and mode variables only for the result cone that can
actually observe one of its actuals; constant and otherwise independent
subexpressions remain immutable shared principal authorities. NovyWave records
43,776 such expression reuses. Residual module operations fall from 17,336 to
15,300, linked operations from 137,993 to 93,044, activations from 145,326 to
100,261, and linked terms from 121,618 to 79,646. A focused two-call test proves
that Number and Text occurrence fields remain isolated while their shared
constant field is computed once.

The accepted post-sharing debug median is 1,462 ms kernel time: 192 ms owner
projection, 29 ms pruning, 595 ms compile/link, 620 ms solve, and 7 ms artifact
projection. The retained-snapshot candidate is 1,516 ms and the cold candidate
is 1,624 ms. Against the first 718-owner baseline, kernel time improves by about
29%, compile/link by about 21%, solve by about 42%, and cold time by about 27%.

The corresponding optimized median is 3.372 ms bundle loading, 78.480 ms
parsing, 10.013 ms SOURCE ABI, 33.697 ms owner projection, 8.921 ms pruning,
137.677 ms compile/link, 146.279 ms solve, and 2.684 ms artifact projection.
Kernel work is 333.824 ms, the retained-snapshot candidate is 343.784 ms, and
the cold candidate is 425.766 ms. The old differential oracle used 249.771 ms.
This remains a partial migration rather than a speed claim: 679 owners are
still unsupported or dependency-pruned, while the new path already carries
4,201 specialized invocation frames and 190,208 live variables. The next large
cut is an immutable result/type summary per definition specialization, so
callers do not instantiate mutable cells for the same result transfer
repeatedly. Persistent revision currentness and backdating begin only after the
complete cold checker passes its parity and performance gates.

A premature cross-revision module cache was measured and deleted. Its retained
full-owner keys improved a second identical run but added cloning/hashing to the
authoritative cold path; the optimized cold median regressed to about 352 ms of
kernel work and 150 ms of compile/link. Warm hits cannot justify that ordering,
and no retained compiler cache remains in this checkpoint. The existing dense
`SpecializationKey` map continues to share one module per definition/static
surface within a cold request without treating structurally identical unrelated
owners as cache entries.

The first immutable definition-summary cut makes specialization reachability an
allocation authority. Invocation frames now allocate expression and mode cells
only for the formal-dependent nodes reachable from that specialized result;
unreachable dependent arms reuse the immutable principal slots and receive no
occurrence state. NovyWave prunes 116,521 such cells, reducing live variables
from 190,208 to 73,687 while keeping the same 93,044 operations, 100,261
activations, and exact 718-owner differential. A focused eight-arm generic call
proves seven unselected dependent arms allocate no occurrence cells. Solved
rows are now published as immutable `DefinitionArtifact`s inside one
`KernelCheckedSnapshot`; global solve work is owned once by the snapshot rather
than repeated on definition rows.

The accepted three-run debug median is 1,440.068 ms kernel time: 193.246 ms
owner projection, 29.615 ms pruning, 580.305 ms compile/link, 613.499 ms solve,
and 7.088 ms artifact projection. SOURCE ABI makes the retained parsed-snapshot
median 1,496.243 ms; parse and bundle loading make the cold median 1,603.788 ms.

The corresponding optimized median is 3.399 ms bundle loading, 77.314 ms
parsing, 10.298 ms SOURCE ABI, 33.756 ms owner projection, 8.724 ms pruning,
130.864 ms compile/link, 140.179 ms solve, and 2.703 ms artifact projection.
Kernel work is 321.768 ms, the retained parsed-snapshot candidate is 332.066 ms,
and the cold candidate is 414.231 ms. Against the preceding accepted release
checkpoint, kernel work improves by about 3.6%, compile/link by about 4.9%, and
the cold candidate by about 2.7%; the larger result is deleting 61% of live
solver variables before complete coverage.

The complete-owner expansion on 2026-08-14 changes the scale again and replaces
the preceding partial-slice timing baseline. The kernel now solves 1,388 of
1,397 NovyWave owners; the remaining nine are explicit unsupported boundaries,
not legacy fallbacks. Before the next normalization cut, a candidate-only
release request used 3,722.8 ms of kernel work: 691 ms to compile/link and
2,965 ms to solve. It contained 380,276 live variables, 488,259 linked
instructions, 85,823 scheduled work items, 996,846 instruction activations,
642,535 mutations, and 1,240,886 dynamic dependency edges. The comparable
debug request used 25,176 ms. These complete-slice receipts supersede every
smaller-coverage result for speed claims.

Two residual scheduling experiments were measured and deleted rather than
allowed to accumulate as cleverness in the hot path. Omitting frame-internal
reverse edges and manually propagating locally reduced an optimized request to
3,382 ms, but it detached equality/binding-dependent flows and changed
`store.active_metadata_format` from `Text` to `Text | Var`. Restoring exact
per-instruction reverse edges recovered parity but increased debug kernel work
to 28,253 ms despite fewer activations. The accepted representation therefore
keeps one coarse scheduled work item for a fully acyclic residual frame and
instruction-grained replay only for the cyclic tail. Queue refinements are not
the current speed owner.

The first complete-slice definition normalization instead removes authored
adapter cells before linking. Invocation-only MATCH arms that are used solely
as WHEN arms alias their exact output; principal frames still materialize every
authored expression for checked artifacts. LIST, SET, and MAP construction now
uses one packed collection instruction that widens item/key/value inputs
directly into the final collection authority. It deletes the intermediate
structural-widen cell and final publication while retaining empty LIST's open
object item and language-neutral Unknown SET/MAP defaults. Focused tests prove
heterogeneous structural widening, precise producer isolation, empty
collections, and one-instruction MAP construction.

The fresh accepted release receipt after those cuts is 3,622.678 ms of kernel
work: 705.996 ms compile/link and 2,850.805 ms solve. The graph contains
355,947 variables, 463,930 linked instructions, 85,777 scheduled work items,
907,302 activations, 599,982 mutations, and 1,227,049 dynamic dependency
edges. Relative to the complete-owner baseline this deletes 24,329 variables
and instructions, removes 89,544 activations, improves solve by about 3.9%, and
improves kernel time by about 2.7%; compile/link is slightly worse and remains
red. The matching debug receipt is 24,759.170 ms of kernel work, including
3,270.068 ms compile/link and 21,156.957 ms solve. The dominant residual is
`NovyView/tree_row`: its 65-instruction module is linked through 419 frames for
27,235 logical instructions, down from 72/30,168 before packed collections.

All 40 kernel tests pass. The full NovyWave differential still stops at the one
known pre-existing mode mismatch: `store.focused_control_label` HOLD expression
3 is `PresentOrAbsent` in the kernel and `Continuous` in the current checker.
No new type, expression, collection, or checked-artifact mismatch was exposed
by the normalization cuts. Candidate-only timing deliberately skips parity and
must never be reported as parity-certified.

The next large cut is a compiled parametric result summary per definition and
static selector surface. Pure acyclic constructors such as `tree_row` must be
represented once as summary bytecode over formal slots, with explicit
projection/backflow outputs, rather than allocating and linking 65 mutable
expression cells for each of 419 calls. Principal checked rows remain complete;
invocations request only the public result, requirements, modes, effects, and
state identities they actually consume. This is the intended bridge from the
current shared residual modules to `DefinitionArtifact` plus compact invocation
frames. Do not claim success by recursively interpreting source nodes at each
call or by caching a fully expanded frame across revisions.

A measured negative prototype locks that last rule. Extending the existing
recursive direct-result compiler across WHEN, collections, render constructors,
and pure builtins reduced residual frames from 26,129 to 4,203, but emitted its
summary operations afresh at every call. Variables rose from 355,947 to
724,442, linked instructions from 463,930 to 1,039,702, and debug kernel time
from 24,759 ms to 40,691 ms. The prototype was deleted immediately. The shared
summary must be an immutable compiled module with compact actual/result and
requirement relocations; a recursive source walk is not an intermediate
implementation of that architecture.

The accepted parametric-summary cut implements that missing ownership seam.
Each eligible definition now owns one immutable `KernelSummaryProgram`; call
occurrences carry only actual input terms and one result publication. Its lazy,
memoized bytecode supports constants, records and spreads, LIST/SET/MAP,
requirements, authored sequencing, MATCH/WHEN selection, render records,
fixed-result pure ABI operations, external authorities, and nested pure user
calls. A singleton WHEN evaluates only its selected arm, so an unselected arm
cannot impose its requirements. Cycles fail closed. Principal checked rows
remain complete, and unsupported summary shapes use the ordinary compiled
frame rather than a recursive source interpreter. The obsolete per-call
recursive summary fallback was deleted in the same cut.

On the complete 1,388/1,397-owner NovyWave surface, 4,127 call occurrences use
shared summaries. Linked instructions fall from 463,930 to 265,751, live
variables from 355,947 to 200,771, activations from 907,302 to 465,696, and
mutations from 599,982 to 349,185. Debug kernel work falls from 24,759.170 ms to
19,285.573 ms: compile/link falls from 3,202.321 ms at the immediate shared-
summary foundation to 1,466.288 ms, and solve falls from 21,493.146 ms to
17,507.556 ms. The optimized kernel falls from 3,622.678 ms to 2,595.897 ms:
296.171 ms compile/link and 2,235.931 ms solve. Parse plus SOURCE ABI plus the
kernel is 2,684.253 ms in that release sample. This is about 28% lower release
kernel wall time and roughly half the mutable graph/work, not yet the final
250 ms diagnostics gate.

All 41 kernel tests pass, including a direct adversarial proof that an
unselected summary arm cannot constrain its input. The complete differential
again reaches only the known `store.focused_control_label` HOLD expression 3
mode mismatch (`PresentOrAbsent` versus the current checker's `Continuous`),
with no earlier type or artifact divergence. The next dominant cost is no
longer residual module linking: solve owns 2,235.931 ms, 235,570 projection
activations, and 1,132,755 dynamic dependency edges. Summary inputs still
materialize standalone projection operations and requirement scaffolds before
the bytecode runs. Move those projections into the lazy summary invocation,
subscribe the invocation directly to provider roots, and delete the separate
projection/unify adapters. This must preserve detached authoritative reads,
nested requirement backflow, late provider replay, and branch laziness.

That lazy-input cut is now accepted. A summary call stores one compact
provider/path program per demanded formal read and allocates only its private
consumer cells. It subscribes directly to the provider root and evaluates a
path only when selected bytecode demands the input. Standalone adapter
projections and requirement unifications disappear. On the same complete
NovyWave surface, variables fall to 148,875, linked instructions to 98,697,
activations to roughly 212,000, and projection activations to 75,085. The
optimized kernel falls to 2,003.834 ms (176.971 ms compile/link and 1,766.403
ms solve); parse plus SOURCE ABI plus kernel is 2,094.665 ms. Focused tests lock
unselected-path laziness and nested requirement backflow.

The following dependency cut replaces append-only inferred edges with an
exact bidirectional binding dependency table. Replacing a binding first
removes its old reverse edges, variable union transfers authority without
copying stale dependents, and queue exhaustion remains the sole convergence
rule. Active dynamic edges fall from 1,063,222 to 100,825 without changing the
checked differential. Summary coverage then expands across pure infix,
transparent THEN/Arrow, and dynamic text sequencing. That deletes the repeated
`NovyView/file_tree_row_label` residual and leaves 127,122 variables, 66,524
linked instructions, 14,410 scheduled work items, 153,727 activations, and
91,072 live dynamic edges. The first release median after that expansion was
about 2,075 ms, slightly slower than the 2,004 ms lazy-input sample despite the
smaller graph, so it was not recorded as a speed win. Telemetry exposed the
reason: 10,582 summary activations still recursively dispatched 1,142,399
summary nodes, while term resolution performed 1,708,489 intern lookups and
91.9% of structural-widen requests hit an existing result.

The accepted dense-term execution cut removes hash/set allocation from this
shared path. Term resolution, cycle checks, and exact dependency extraction
now use reusable generation-stamped arrays. Every immutable term also carries
one derived `has_variable` bit; a closed type bypasses recursive resolution,
occurs traversal, dependency traversal, and reconstruction entirely. This is
term-DAG metadata owned once by the arena, not a cross-request result cache.
Debug kernel work falls first from 13,060.471 ms to 11,411.164 ms with dense
scratch storage, then to 9,170.181 ms with closed-term bypass; solve falls from
11,744.630 ms to 7,834.660 ms. Intern requests fall from 1,708,489 to 1,087,621
with identical operations, activations, mutations, and outputs.

Three fresh no-rebuild optimized samples after the full cut record kernel times
of 1,455.195, 1,420.421, and 1,402.469 ms (median 1,420.421 ms). Their complete
parse plus SOURCE ABI plus kernel times are 1,551.670, 1,506.496, and 1,489.286
ms (median 1,506.496 ms). The median solve is 1,181.592 ms and compile/link is
178.593 ms. Against the first complete optimized kernel receipt of 3,722.8 ms,
the greenfield path is about 62% faster and is now inside the plan's 0.8--2.0 s
first-kernel milestone. This is not checker cutover readiness: nine owners
remain explicitly unsupported and the complete differential still stops at
the same pre-existing `store.focused_control_label` HOLD mode mismatch, with no
earlier type or artifact divergence. All 44 kernel tests pass.

A measured selector-free linear-summary prototype is rejected. Only 829 of
10,582 summary activations contain no lazy selector, and the debug kernel moved
from 9,170 to 9,148 ms, which is noise rather than a speed win. The partial
second evaluator was deleted. A future packed summary plan must represent
guarded selector jumps and preserve unselected-arm requirement laziness; a
selector-free side engine is not enough.

The accepted typed-index cut follows the larger retained counter instead.
Intern requests break down as 732,044 variable terms, 296,335 objects, and only
59,242 variants, unions, containers, functions, and scalars combined. The term
arena now maps `TypeVariableId` directly to its unique `TypeTermId`, so repeated
root resolution never hashes a variable term. Ordered object construction no
longer allocates a temporary field-name map, object lookup compares borrowed
ordered fields before allocating a canonical boxed slice, and the trusted
internal lookup fingerprint uses a fast deterministic mixer. Collision buckets
still require exact equality, so type identity and deterministic output do not
depend on fingerprint uniqueness.

Those changes reduce solve-time intern requests from 1,087,621 to 406,428 and
debug kernel/solve medians to approximately 8,092/6,793 ms. Three optimized
samples record kernel times of 1,300.215, 1,321.255, and 1,312.981 ms (median
1,312.981 ms). Complete parse plus SOURCE ABI plus kernel takes 1,388.803,
1,416.500, and 1,406.673 ms (median 1,406.673 ms); median solve is 1,067.331 ms.
This is another 7.6% below the 1,420.421 ms kernel checkpoint and about 65% below
the original 3,722.8 ms complete-kernel receipt. Work counts and the full
differential are unchanged, including the one known HOLD mode mismatch.

The next definition-summary cut removes a false boundary between nested calls.
A forwarded formal now retains the identity of its occurrence-local projection
input, so a callee read such as `row.expanded_label` composes its field path
against the original actual instead of allocating a complete residual frame.
Computed summary values have one immutable directional `Projection` bytecode
node for cases such as a parsed tagged result followed by a payload read. The
former preserves private requirement backflow; the latter projects an
authoritative derived value without inventing a mutable provider cell. Gated
owner tracing also reports remapped dense and stable call targets rather than
the pre-projection placeholder IDs.

On NovyWave this makes `file_tree_simple_scope_expand_button` and
`waveform_segment_for_signal` shared summaries. Invocation frames fall from
2,115 to 512, residual frames from 3,503 to 1,900, linked instructions from
66,524 to 32,823, activations from 153,727 to 69,754, and live variables from
127,122 to 99,442. Three fresh optimized samples record kernel times of
1,208.321, 1,121.139, and 1,146.833 ms (median 1,146.833 ms). Their complete
parse plus SOURCE ABI plus kernel times are 1,313.259, 1,217.505, and
1,242.646 ms (median 1,242.646 ms); median compile/link is 173.795 ms and
median solve is 907.353 ms. The fresh two-job release rebuild took 1m57s. This
is 12.7% below the preceding 1,312.981 ms
kernel checkpoint and about 69% below the original 3,722.8 ms receipt. Debug
samples remain noisier at roughly 7.30--8.00 seconds of kernel work, while the
same deterministic workload records 69,754 activations, 158,504 mutations,
316,694 intern requests, and 56,114 live dependency edges.

A collection-summary prototype is explicitly rejected and deleted. Adding
directional item extraction plus `List/map` result bytecode reduced invocation
frames again from 512 to 156 and linked instructions from 32,823 to 25,714,
but recursively embedded and reinterpreted the mapped definition graph at each
occurrence. Two debug observations regressed solve from roughly 6.1--6.6
seconds to 9.493/10.431 seconds and kernel work to 11.208/12.326 seconds.
Smaller mutable graph counts therefore do not justify recursively interpreting
large summary trees. A future collection cut must compile the map body once as
compact guarded/loop bytecode or direct typed operations; it cannot inline the
source-shaped callee tree into every summary activation.

The summary evaluator now owns one reusable generation-stamped dense scratch
arena. Each activation advances a stamp and writes only demanded values; it no
longer clears and resizes result and cycle-detection buffers across every node
in the immutable summary program. This is especially important for lazy
selectors, where unselected arm nodes must remain untouched. Two no-rebuild
debug observations after the cut record kernel/solve times of 7.457/6.214 and
7.813/6.480 seconds, compared with the immediate 8.118/6.698-second baseline.
Three optimized samples record kernel times of 1,188.470, 1,102.121, and
1,088.352 ms (median 1,102.121 ms). Complete parse plus SOURCE ABI plus kernel
takes 1,307.426, 1,201.776, and 1,184.231 ms (median 1,201.776 ms); median solve
is 866.832 ms. The two-job optimized rebuild took 2m00s. Work counts remain
identical, and a dedicated regression proves that reused scratch storage cannot
leak one call occurrence's value into the next.

All 47 kernel tests and 77 non-ignored compiler tests pass. The complete
NovyWave differential again reaches only the known
`store.focused_control_label` HOLD expression 3 mode mismatch
(`PresentOrAbsent` versus the current checker's `Continuous`), with no earlier
type or artifact divergence. The next dominant repeated residual is
`NovyView/selected_wave_area` through `waveform_segment_lane` (29 operations
through 27 frames). It is collection/map-shaped and must wait for the compiled
collection-summary design above. In parallel, 961,506 summary-node evaluations
remain the largest direct-dispatch counter, so guarded packed summary
instructions are now more important than removing another small residual
module.

A guarded flat-summary prototype tested that interpreter hypothesis and is
rejected. Flattening every lazy summary into one instruction tape produced
290,200 semantic nodes, 311,659 instructions, and 119,163 guarded blocks. Its
optimized median regressed to roughly 1.179 seconds of kernel work from the
1.102-second generation-stamped checkpoint. Dense code alone did not remove
the repeated definition graphs, and the extra guard dispatch outweighed its
small straight-line benefit. The prototype was deleted rather than retained as
a second evaluator.

The accepted definition-bytecode cut instead removes the source-order boundary
between nested summaries. Eligible callees are compiled in deterministic
callee-first order. A caller either inlines a small definition or emits one
`Invoke` node that references the callee's immutable `Arc<KernelSummaryProgram>`.
Nested input slots resolve mapped parent values lazily through the current
evaluation frame, so an unselected child `WHEN` arm cannot evaluate a formal
projection or impose its requirement. Each nested definition receives a
generation-stamped scratch frame; no source node is redispatched and no
per-invocation argument vector is allocated.

Inlining is a measured code-size/runtime decision rather than an all-or-nothing
mode. Debug NovyWave sweeps at 32, 64, 128, and 256 summary nodes showed the
expected frontier: larger thresholds progressively reduced dynamic node
evaluation while increasing compile work. The 128-node boundary gave the best
stable balance. The retained program has 39,243 definition-summary nodes and
530 static invokes; it evaluates 1,149,635 summary nodes, down from 1,361,998
when nearly every nested definition was shared, while avoiding the 287,965
static nodes produced by selector-safe inlining. The surrounding mutable graph
remains at 99,442 variables, 32,823 linked operations, 69,754 activations, and
56,114 live dependency edges.

Three fresh optimized samples record kernel times of 1,012.901, 1,000.490, and
996.979 ms (median 1,000.490 ms). Complete parse plus SOURCE ABI plus kernel
takes 1,109.775, 1,092.366, and 1,084.842 ms (median 1,092.366 ms); median
compile/link is 109.190 ms and median solve is 832.288 ms. The two-job release
rebuild took 1m49s. This is about 9.2% below the preceding 1,102.121 ms kernel
median and about 9.1% below its 1,201.776 ms complete median. All 50 kernel
tests and 77 non-ignored compiler tests pass. The complete NovyWave
differential again reaches only the same pre-existing
`store.focused_control_label` mode mismatch, with no earlier semantic or
artifact divergence.

This establishes the reusable call instruction needed by the next large cut:
compile collection callback bodies, beginning with `List/map`, as one shared
definition program invoked by the collection operator. Do not embed callback
source trees into caller summaries. A future packed interpreter should operate
on these unique definition programs and their explicit frame stack; it must not
recreate the rejected global guarded tape.

The executable-owner coverage boundary is now closed. Compact edge projection
uses the parser's canonical `linked_input` for every multiline-capable input
role (`DRAINING`, `HOLD`, `WHEN`/`WHILE`, `THEN`, and infix-left), rather than
only for `WHEN` and builtin pipes. This brought the isolated `hold_proof`
definition into the same owner-local expression tree as its authored pipeline
input. The NovyWave receipt now classifies 1,389 executable owners as solved,
eight declaration-less unit roots as explicit container owners, and zero
owners as unsupported. Empty containers are not assigned a fake result, while
a nonempty unit root still fails closed. The added definition costs only three
variables and three operations and leaves the existing performance profile
otherwise unchanged.

A whole-definition `List/map` summary prototype is rejected. It promoted every
definition containing a map into one coarse summary invocation. Although
invocation frames fell from 512 to 156 and linked operations from 32,823 to
25,714, the enclosing definition was reevaluated on every callback-provider
epoch: variables rose above 120,000, summary evaluation rose to 1.531 million
nodes, mutations rose to 216,460, and debug solve time regressed from about
6.0 seconds to more than 11 seconds. Replacing the callback item alone removed
only about 1,400 variables and did not fix the ownership error. The prototype
was deleted. The accepted design must keep the enclosing collection owner
incremental and attach one shared callback program specifically to the
collection operation, with explicit item and capture inputs.

The next accepted cut removes a more fundamental duplicate fact. Work
attribution now records the top residual modules and the top immutable summary
definitions by program and node evaluations. The residual ranking proved that
linking was already small (the largest module contributes only 783 linked
operations), while `NovyTheme/material` alone accounted for 247,124 of
1,149,635 demanded summary nodes. A prototype that externalized every large
nested summary as another dependency-scheduled component was therefore tested
and deleted: it reduced summary evaluation only to 1,041,977 nodes while
expanding variables from 99,445 to 505,413 and slowing debug solve from about
5.93 seconds to 10.89 seconds. Nested definition boundaries are not independent
mutable state by default.

The definition trace exposed the actual ownership error. `material(mode, of)`
compiled 49 occurrence inputs even though they represented only two facts: the
whole `of` formal and the whole `mode` formal repeated in many lazy arms. The
direct-summary compiler now interns every `(formal, complete projection path)`
once. All uses share one immutable `Input` value and one occurrence-local
projection equation; branch outputs and constraints remain independently lazy.
This is ordinary type-flow canonicalization, not a `material`, renderer, UI,
tag, or `NoElement` special case. An exact regression requires two repeated
whole-formal reads to emit one summary input and still produce both record
fields. The existing multi-arm selector tests continue to prove that an
unselected arm cannot impose a requirement.

On complete NovyWave coverage, summary definition nodes fall from 39,243 to
34,453, variables from 99,445 to 39,798, mutations from 158,507 to 57,375,
dynamic dependency edges from 56,114 to 23,827, and term-intern requests from
316,696 to 265,976. Linked operations remain 32,826 and all 1,389 executable
owners plus eight inert containers remain covered. One no-rebuild debug sample
records 1,861.444 ms kernel time, 595.148 ms compile/link, and 945.701 ms solve,
down from roughly 6.87/0.63/5.93 seconds before the cut.

Three fresh optimized samples record kernel times of 378.794, 381.282, and
389.723 ms (median 381.282 ms). Complete parse plus SOURCE ABI plus kernel takes
469.072, 470.206, and 478.447 ms (median 470.206 ms); median compile/link is
97.578 ms and median solve is 226.029 ms. This is approximately 2.62 times
faster than the preceding 1,000.490 ms kernel median and 2.32 times faster than
its 1,092.366 ms complete median. All 51 kernel tests and 79 non-ignored
compiler tests pass. The complete NovyWave differential still reaches exactly
the known `store.focused_control_label` HOLD mode mismatch and no earlier type
or artifact divergence.

The full owner-flow differential is now closed. Instead of stopping at the
first error, the ignored NovyWave oracle collects every owner mismatch in one
run; that exposed nine remaining differences and prevented another serial
one-failure-per-run repair loop. The accepted fixes separate kernel semantics
from old-checker compatibility:

- tagged `WHEN` arms now carry an explicit mode-narrowing equation from the
  matched selector to nested reads of that same provider, so historical
  eventful `LATEST` branches cannot leak their mode into a proven selected arm;
- an empty `LATEST {}` publishes `Unknown` shape evidence rather than claiming
  the explicit `Absent` value type, while structural HOLD widening still keeps
  a concrete initializer;
- the differential alone recognizes the old assembled checker's exact erased
  missing-projection tuple and its call-site-specialized generic-selector
  principals. These allowances are restricted to already-proven legacy cones,
  normalize strict and lossy alpha partitions separately, and do not weaken
  ordinary kernel equality; and
- compatible open generic rows can differ by call-site-added fields in the
  legacy image, while disjoint rows, closed rows, concrete occurrences, modes,
  and all non-generic owners remain strict.

The complete NovyWave run now compares all 1,389 executable owner results and
their stable expression rows with zero mismatches, alongside eight explicit
inert unit containers and zero unsupported owners. This is full parity for the
currently emitted flow surface, not checker cutover readiness: diagnostics,
calls, effects, states, collections, sources, lexical bindings, currentness,
and dependency cones must still become kernel-owned artifacts and pass their
own differential gates before production changes. `NoElement` remains an
ordinary user/library tag; none of these changes gives it language semantics.

The post-parity debug sample records 1,839.611 ms kernel time and 1,999.152 ms
for parse plus SOURCE ABI plus kernel, with 266.089 ms owner projection,
584.405 ms compile/link, and 942.890 ms solve. Three fresh optimized samples
record kernel times of 377.818, 402.059, and 406.314 ms (median 402.059 ms).
Complete parse plus SOURCE ABI plus kernel takes 464.434, 495.098, and
500.039 ms (median 495.098 ms); median compile/link is 98.198 ms and median
solve is 242.824 ms. The optimized rebuild took 1m46s. Mutable work remains
exactly 39,798 variables, 32,826 operations, 69,687 activations, 57,375
mutations, and 23,827 dynamic edges. The current desktop median is about 5%
slower than the preceding receipt while its first sample is slightly faster;
with identical work counts this is recorded as measurement noise rather than a
claimed speed change. The parity cut is accepted for correctness, not speed.

The first non-flow `DefinitionArtifact` cut now makes call and host-effect
inventory kernel-owned. Every source-authored user, renderer, pure builtin, or
host-effect call publishes one compact row containing its definition-local
expression, typed target, ordered input roles, explicit local/external provider
reference, and solved result. An external provider is no longer represented by
an out-of-range local expression ID. Host-effect rows additionally copy replay,
barrier, result, and delivery policy exactly once from the dependency-bottom ABI
registry; downstream construction does not need to rediscover those policies by
walking source or checked call records.

The complete NovyWave differential publishes 1,821 call rows and 11 host-effect
rows. It proves one-to-one stable call occurrence coverage, stable user-call
targets, inherited-context presence, renderer/pure/host target classification,
call-result agreement with the expression artifact, and exact host operation
coverage while retaining the full owner-flow differential. Regular parser-backed
tests prove stable user-call/provider projection and stable host-effect rows; a
kernel test locks every copied effect policy against the ABI registry. Exact
legacy `FreshOut`/`ForwardOut`, contextual substitution, and call-entry binding
parity remains a later artifact sub-cut rather than being inferred from these
rows.

One no-rebuild debug receipt records 1,824.357 ms kernel time, including 582.802
ms compile/link and 934.597 ms solve, with the same 39,798 variables, 32,826
operations, 69,687 activations, 57,375 mutations, and 23,827 dynamic edges.
This table-only artifact publication is not claimed as a speed improvement over
the preceding 1,839.611 ms debug sample, and it does not spend another optimized
rebuild; the last accepted 402.059 ms release median remains the release
baseline. All 54 kernel unit tests, 84 non-ignored compiler unit tests, and 16
compiler integration tests pass; the ignored complete NovyWave call/effect and
flow differential also passes explicitly.

The next artifact cut replaces the flow-only expression vector with compact
solved expression rows. Each row now owns its dense expression ID, authored
kernel kind, ordered typed input edges, and final `FlowType`. Local and external
providers use different enum cases throughout the artifact, and project solve
consumes the pending rows instead of cloning their kinds and edges a second
time. SOURCE remains a distinct expression kind carrying its closed payload ABI
rather than being erased to a generic known value. LIST and BYTES rows retain
their authored capacity/fixed-size metadata; SET and MAP retain their exact
collection kind and structural inputs.

The complete NovyWave artifact now contains 15,575 expression rows. Its 133
collection occurrences match the checked expression inventory exactly in kind,
capacity, ordered edge roles, and stable providers; its 117 literal SOURCE
occurrences match exactly in stable identity, payload, flow, and cardinality.
The existing whole-flow comparison remains the type authority, including its
narrow documented legacy compatibility cones, so collection structure parity
does not accidentally reintroduce a second stricter type oracle. Statement,
declaration, path, interval, state, and persistent list-resource rows are still
the next fact layer; source/list expression parity is not presented as full
resource-table parity.

Three debug samples record 1,848.111, 1,869.499, and 1,886.962 ms kernel time
(median 1,869.499 ms). Parse plus SOURCE ABI plus kernel records a 2,021.778 ms
median. Three fresh optimized samples record 382.407, 385.574, and 391.411 ms
kernel time (median 385.574 ms); complete candidate time is 470.100, 472.884,
and 477.308 ms (median 472.884 ms). Median compile/link is 101.158 ms and median
solve is 221.594 ms. The two-job optimized rebuild took 1m46s. This is roughly
4% below the prior optimized medians, but all solver/work counters are identical,
so it is retained as the new complete-artifact receipt without attributing an
algorithmic speedup. All 54 kernel unit tests, 85 non-ignored compiler unit
tests, and 16 compiler integration tests pass; the full ignored NovyWave
expression/collection/source/call/effect/flow differential passes explicitly.

The next dependency-bottom cut adds one dense authored statement table to every
`DefinitionArtifact`. Each row owns its definition-local statement ID, exact
function/field/SOURCE/HOLD/LIST/block/spread/expression kind, parameter and
capacity metadata, one typed local-or-external value reference, and ordered
local-statement or child-owner edges. The table is carried through the same
project compile and solve as expression rows; it is not reconstructed from
checked statements afterward. Project assembly validates dense statement IDs,
local child bounds, external owner bounds, and value references before solving.

Multiline `WHEN` arms exposed a previously duplicated sequencing fact: the
parser's structural statement surface retains the arm expression while the
checked statement row points at the final pipeline value. The stable parser
owner view now publishes `checked_statement_value_expression` as that exact
syntax authority. The kernel bridge consumes it directly instead of importing
the old owner syntax graph, and a parser regression locks the structural-arm
versus checked-tail distinction. The complete NovyWave artifact contains 5,541
statement rows with zero kind, value, child-topology, or coverage mismatches,
while retaining the 15,575 expression, 133 collection, 117 literal SOURCE,
1,821 call, and 11 host-effect rows from the preceding cut.

Three no-rebuild debug samples record 1,897.640, 1,927.924, and 1,889.030 ms
kernel time (median 1,897.640 ms); complete candidate time is 2,047.757,
2,087.334, and 2,039.583 ms (median 2,047.757 ms). Three optimized samples
record 401.878, 406.279, and 403.857 ms kernel time (median 403.857 ms);
complete candidate time is 490.793, 494.548, and 491.071 ms (median 491.071
ms). Median optimized compile/link is 102.952 ms and solve is 228.180 ms. The
final two-job optimized library-test rebuild took 3m21s; an earlier broad Cargo
test command also spent 4m36s rebuilding unused integration-test targets and is
not a timing sample. Mutable solver work is unchanged at 39,798 variables,
32,826 operations, 69,687 activations, 57,375 mutations, and 23,827 dynamic
edges, so the small timing increase is recorded as statement-artifact
publication plus measurement noise, not solver regression. All 78 parser tests,
55 kernel tests, 85 non-ignored compiler unit tests, and 16 compiler integration
tests pass; the full ignored NovyWave differential passes explicitly.

The declaration/lexical artifact cut is now complete. Every definition carries
one dense declaration table for authored statements, function parameters,
inline record fields, match-pattern bindings, and collection callback outputs,
plus one lexical occurrence table with an exact declaration/value/context
target, authored projection, and read-versus-drain access. Local and imported
authorities are distinct typed references. Fieldless HOLD self-reads follow the
parser-owned containing statement chain to the enclosing state declaration;
they do not alias the initializer/provider expression. DRAIN is compiled as an
ordinary lexical occurrence and has no separate type-flow implementation.

The independent lexical-plan differential covers 4,489 declaration rows and
3,963 lexical-binding rows across all 1,389 executable NovyWave owners, with
eight inert unit containers, zero unsupported owners, and zero declaration,
target, projection, access, or coverage mismatches. Focused parser-backed tests
exercise parameters, BLOCK declarations, statement-backed and inline record
fields, pattern bindings, collection callback bindings, DRAIN, and cross-owner
HOLD authority. Kernel validation rejects incompatible structural declaration
origins and missing lexical declaration targets. The superseded flattened
checked-row declaration/lexical comparison helper is deleted; the remaining
old lexical projection exists only as the explicit test oracle.

Three no-rebuild debug samples record 1,985.619, 2,003.021, and 2,006.467 ms
kernel time (median 2,003.021 ms); complete candidate time is 2,141.893,
2,154.771, and 2,158.413 ms (median 2,154.771 ms). Three fresh optimized
samples record 410.490, 423.980, and 407.671 ms kernel time (median 410.490
ms); complete candidate time is 497.946, 516.275, and 495.394 ms (median
497.946 ms). Median optimized compile/link is 103.188 ms and solve is 217.007
ms; the two-job optimized rebuild took 1m46s. Mutable work remains effectively
unchanged at 39,798 variables, 32,826 operations, 69,688 activations, 57,341
mutations, and 23,827 dynamic edges. The roughly 1--2% optimized increase over
the statement-only checkpoint is recorded as artifact-publication cost and
measurement noise, not a speed win. All 56 kernel tests, 86 non-ignored compiler
unit tests, and 16 compiler integration tests pass, and the full ignored
NovyWave differential passes explicitly.

The persistent-resource artifact cut is now complete. `DefinitionArtifact`
owns SOURCE, HOLD-state, and persistent-LIST rows directly beside its dense
expressions, statements, declarations, and lexical occurrences. Each resource
uses typed local-or-public declaration and statement references plus one
declaration-anchored semantic path; there is no source-shaped resource draft
and no second body solve. SOURCE payload types, HOLD state/initial flow, and
LIST item authorities are materialized from the already solved expression and
public-result tables. A child LIST that supplies a public field through
list-preserving operations is linked to that parent statement by following the
packed project graph and its external-result edge, rather than inferring
authority from parser nesting.

The complete NovyWave differential now covers 117 SOURCE resource rows, 76
HOLD state rows, and 133 persistent LIST rows, in addition to 15,575
expressions, 5,541 statements, 4,489 declarations, 3,963 lexical bindings, 133
collection expressions, 1,821 calls, and 11 host effects. Every declared owner
is supported: 1,389 executable definitions plus eight inert unit containers,
with zero unsupported owners. The resource comparison checks exact
declaration, statement, path, initial/provider, payload/state/item type,
capacity, and key-policy authority. `NoElement` remains an ordinary UI-library
tag in the kernel. One explicitly scoped test-oracle allowance recognizes the
legacy checker's spelling-based structural-widening loss; it changes no kernel
type rule.

Three no-rebuild debug samples record 2,027.397, 2,040.099, and 2,042.011 ms
kernel time (median 2,040.099 ms); complete candidate time is 2,178.085,
2,190.699, and 2,192.895 ms (median 2,190.699 ms). Three optimized samples
record 431.452, 417.595, and 433.662 ms kernel time (median 431.452 ms);
complete candidate time is 520.122, 504.705, and 520.720 ms (median 520.122
ms). Median optimized compile/link is 103.605 ms and solve is 230.725 ms. The
fresh two-job optimized rebuild took 1m48s and is excluded from every Boon
latency. A separate optimized full differential run passes and records 525.603
ms candidate time versus 272.405 ms for the legacy parse/check oracle; the new
candidate is not yet production and the old oracle time is not included in its
total. Resource publication raises optimized candidate latency by roughly 4--5%
from the declaration/lexical checkpoint while adding all 326 persistent
resource rows; this is recorded as artifact cost, not a speed improvement.

All 57 kernel tests, 87 non-ignored compiler unit tests, and 16 compiler
integration tests pass. Focused tests validate dense resource IDs and node
kinds, solved SOURCE/HOLD/LIST surfaces, malformed-resource rejection, a
fieldless function HOLD, and parent LIST authority through `List/map`. The
next checker tranche is therefore diagnostics, callable substitutions, exact
dependency/currentness receipts, and the permanent session/demand API—not
resource micro-optimization. Those tables must be differential-clean before
the checker-wide flag-day cutover and deletion of the old owner solvers.

The dependency/currentness receipt cut is now complete. The kernel owns an
exact typed dependency row for every external expression, result, public
declaration, public statement, call target/input, lexical read, and persistent
resource anchor. A compact CSR index preserves all authored uses in the
forward direction and one deduplicated reverse-consumer edge per definition;
NovyWave contains 7,001 dependency rows and 3,053 reverse-consumer rows. Queue
invalidation can now start from this index instead of rescanning definitions.

Each solved definition publishes five distinct V1 SHA-256 receipts: the exact
compiled basis, alpha-stable public result, alpha-stable complete artifact,
imported dependency authorities, and exact combined currentness. Type-variable
alpha normalization is performed once for the complete definition, preserving
all intra-definition correlations. An implementation-only edit can therefore
change the exact definition receipt while a dependent definition backdates
when its imported public authority is unchanged. The structural hash stream
has an explicit domain and fixed-width numeric encoding; a future persistent
`KernelSession` must include compiler/kernel ABI identity rather than assuming
that Rust's derived `Hash` contract remains stable across compiler upgrades.
Focused tests lock the current byte contract, exact dependency cones, and the
separation between semantic backdating and exact evaluation currentness.

All 60 kernel tests, 87 non-ignored compiler unit tests, and 16 compiler
integration tests pass. The ignored full NovyWave differential also passes
with 1,389 executable definitions, eight inert containers, zero unsupported
owners, and complete receipt coverage. One final optimized differential sample
records 602.791 ms kernel time, 691.581 ms complete candidate time, 110.177 ms
compile/link, and 395.045 ms solve/seal time.

Three no-rebuild debug samples record 2,534.372, 2,451.759, and 2,458.587 ms
kernel time (median 2,458.587 ms); complete candidate time is 2,691.284,
2,604.326, and 2,611.554 ms (median 2,611.554 ms). Three optimized samples
record 608.843, 595.990, and 616.712 ms kernel time (median 608.843 ms);
complete candidate time is 699.815, 684.308, and 710.921 ms (median 699.815
ms). Median optimized compile/link is 111.487 ms and solve/seal is 396.748 ms.

This is deliberately recorded as a performance regression, not a speed win.
Relative to the persistent-resource checkpoint, median optimized kernel time
rose by 177.391 ms (41.1%) and complete candidate time by 179.693 ms (34.5%).
The required ownership review found that dependency collection is small; the
dominant cost is alpha-normalizing and sealing every complete checked artifact.
An earlier CBOR materialization design was slower and was replaced by the
direct structural hash stream, but local hashing changes cannot make full
checked-image publication free. The next architecture tranche is therefore the
permanent `CheckDemand` boundary: diagnostics-only compilation must stop before
complete artifact normalization/currentness sealing, while checked-image and
demanded-definition requests explicitly pay for the products they request.
This demand split precedes further solver micro-optimization.

The first permanent input/session/demand cut is now complete. A
`KernelProjectInput` owns the immutable normalized owner programs and
definition facts, parser-stable definition identities grouped by source unit,
and one collision-checked stable-to-dense project-link overlay. Stable keys are
the request identity; `KernelOwnerId` remains revision-local. The kernel crate
depends on the lower-level `boon_syntax` identity contract but still has no
dependency on `boon_parser`, `boon_typecheck`, or any upper compiler stage.

`KernelSession` owns one revision, its immutable project input, one quiescent
solved graph, and demand-product caches. `CheckDemand::Diagnostics` publishes
only alpha-normalized public interfaces; `CheckDemand::Definitions` resolves
stable definition keys and materializes only the requested artifacts; and
`CheckDemand::CheckedImage` alone builds the complete dependency/currentness
image. Strengthening demand in one revision never recompiles or re-solves the
type graph. Once the complete checked image is published it replaces the large
pre-publication owner tables and answers every weaker demand, so the session
does not retain two full representations. Replacing the project advances the
revision and clears all products; cross-revision red/green reuse remains a
later explicit tranche.

The NovyWave probe now measures the product boundary directly. Three optimized
no-rebuild samples record diagnostics-only candidate time of 539.276, 553.239,
and 549.776 ms (median 549.776 ms), versus complete checked-image candidate
time of 709.950, 727.079, and 725.537 ms (median 725.537 ms). Median kernel time
is 455.632 ms for diagnostics and 631.393 ms for the checked image; graph solve
is 242.705 ms, public-interface projection 7.214 ms, and complete checked-image
publication 162.784 ms. Diagnostics therefore avoids 175.761 ms (24.2%) on the
matched median while publishing zero definition artifacts and zero receipts.
One final optimized differential run passes at 519.082 ms diagnostics-only and
688.510 ms complete candidate time, with all 1,389 definitions and all artifact
tables unchanged. A directional debug sample records 2,275.534 ms diagnostics
and 2,668.092 ms complete candidate time. The two-job optimized rebuild took
1m56s and is excluded from every Boon latency.

All 65 kernel tests, 87 non-ignored compiler unit tests, and 16 compiler
integration tests pass. Focused gates prove stable unit/link ownership,
stable-key demand canonicalization, sparse materialization, zero receipt work
for diagnostics and sparse definitions, public-interface equality across
demands, same-revision graph reuse, checked-image replacement, and revision
invalidation. This closes the input/session/demand ownership milestone. Exact
diagnostic rows must attach to these products without reintroducing a second
solve or making diagnostics pay for a checked image.

The callable-interface slice is now complete. Every definition publishes one
canonical formal scheme from its private requirement surfaces, and every user
call publishes a target-definition-local substitution environment. Parameter
IDs are assigned by the target scheme rather than leaking union-find ordinals,
so they remain meaningful across occurrence frames and fresh processes. Call
actuals now carry separate provider and requirement channels: explicit
arguments can share one occurrence surface, while an inherited `PASSED` formal
reads the caller provider and sends transitive callee requirements to the
caller's private requirement surface. This removes the detached-context bug
without adding another per-call projection graph or specializing concrete
providers.

The differential compares callable schemes and occurrence substitutions in
those canonical namespaces. It keeps one narrow legacy allowance: the old
checker can back-specialize an inherited generic context from a downstream
partial `WHEN` selector, whereas the kernel intentionally keeps that provider
generic. Missing legacy-only selector evidence is ignored; contradictory
substitutions still fail. This is not a tag or UI exception, and `NoElement`
remains an ordinary library tag.

The 2026-08-14 full NovyWave differential passes over all 1,389 executable
owners and 2,123 call sites. A fresh debug diagnostics sample records
2,291.075 ms candidate time, 2,138.140 ms kernel time, and 956.244 ms graph
solve. A fresh optimized sample records 550.265 ms candidate time, 461.382 ms
kernel time, 232.922 ms graph solve, and 7.525 ms public-interface projection;
the full optimized differential body completes in 1.61 seconds. The two-job
release rebuild took 121.34 seconds and is excluded from Boon latency. All 67
kernel tests, 88 non-ignored compiler unit tests, and 16 compiler integration
tests pass. This closes the public callable interface/substitution milestone.
The next measured architecture target is the diagnostics construction path:
roughly 129.5 ms of program compilation and 232.9 ms of graph solve remain
larger than interface projection, so work must reduce compiled operations and
solver activation rather than tune the already-small projection tail.

The first exact-diagnostics slice is now complete. Diagnostics are typed kernel
facts, not formatted legacy strings: each row owns a dense owner, exact call
input site, target definition and formal ordinal, severity, actual and expected
types, and an exact missing/incompatible structural field when one exists.
They are projected from the quiescent graph and public callable schemes without
materializing checked expressions, statements, resources, dependencies, or
currentness receipts. `CheckDemand::Diagnostics` therefore still publishes
zero definition artifacts and zero sealed definitions. A later checked-image
demand reuses both the solved graph and the call substitutions derived during
diagnostic projection; it does not derive the same substitution environment a
second time.

The compiler oracle relocates dense diagnostic sites to parser-stable call and
target identities. Parser-backed differentials cover both ordinary authored
arguments and explicit `PASS` context records, including the exact nested field
failure. Generic call instantiation is applied before assignability testing, so
a definition-local alpha does not produce a false user diagnostic. Diagnostic
rows participate in the alpha-normalized exact artifact/currentness V3 receipt
but deliberately do not change the public-interface fingerprint. Sealing
validates every referenced call, target, formal and input row. This slice does
not yet claim complete source-facing diagnostics: syntax, link, arity, name
resolution, and other non-call-input diagnostic families still need their own
typed facts and presentation relocations before production cutover.

A fresh directional debug NovyWave run remains differential-clean over all
1,389 owners. Diagnostics-only records 2,439.434 ms candidate time,
2,279.199 ms kernel time, 1,056.661 ms graph solve, and 17.623 ms interface plus
diagnostic projection. The complete checked-image path records 2,851.110 ms
candidate time, 2,690.875 ms kernel time, and 387.025 ms checked-image
publication. Compared with the preceding one-sample debug receipt this is a
roughly 3--7% regression/noise band, not an accepted speed win; the new
projection tail is only 17.6 ms and graph solve remains dominant. All 72 kernel
tests, 90 non-ignored compiler unit tests, and 16 compiler integration tests
pass. `NoElement` remains an ordinary tag throughout this diagnostic path; no
library spelling is recognized by the kernel type comparison.

The matching optimized architecture-boundary probe passes in 1.61 seconds
including the legacy differential oracle. Its diagnostics-only candidate is
563.534 ms, including 471.016 ms kernel time, 242.976 ms graph solve, and
7.737 ms interface plus diagnostic projection. The full checked-image
candidate is 733.407 ms, including 640.889 ms kernel time and 160.153 ms
checked-image publication. The two-job release rebuild took 1m59s and is
excluded from every Boon latency. Relative to the preceding single optimized
receipt this is a roughly 2--4% regression/noise band, so it does not change
the architectural conclusion or count as a speed improvement.

The first definition-summary normalization cut is now complete. The kernel
partially evaluates definition-constant projection, sequence, collection,
selection, and record algebra once while compiling the definition. It never
evaluates call inputs, requirements, or nested invocations during this pass, so
unselected arms retain their lazy requirement behavior. Records whose dynamic
fields all use the same selector and identical closed arm domains are fused into
one selector over complete record terms. A final dense relocation removes dead
summary nodes and unused formal inputs. Whether a definition owns shared
summary bytecode is decided from its pre-normalization size, so making a large
definition compact cannot accidentally turn it back into per-call inlining.
These are language-generic term and decision rewrites; no UI constructor or
tag spelling, including `NoElement`, participates in them.

On the full NovyWave graph this cut folds 870 constant nodes, fuses 2,083
same-selector records, removes 13,457 dead nodes and 41 dead inputs, and leaves
25,162 summary nodes. Relative to the typed-diagnostics checkpoint, stored
summary nodes fall from 34,453 to 25,162 (27.0%), summary-node evaluations from
984,821 to 732,728 (25.6%), and term-intern requests from 259,795 to 202,391
(22.1%). The complete differential remains exact across all 1,389 owners.
All 74 kernel tests, 90 non-ignored compiler unit tests, and 16 compiler
integration tests pass.

The matching optimized candidate-only probe records 554.727 ms diagnostics,
including 464.461 ms kernel time and 231.867 ms graph solve, versus 563.534,
471.016, and 242.976 ms at the preceding checkpoint. The complete checked
candidate records 725.659 ms, including 635.393 ms kernel time and 160.537 ms
checked-image publication, versus 733.407, 640.889, and 160.153 ms. This is a
small 1--5% cold-path improvement, not the final architecture-scale speedup;
the two-job release rebuild again took 1m59s and is excluded. The remaining
summary ranking is led by `tree_row_text` at 88,032 evaluations and `material`
at 71,949, so the next large cut should remove repeated occurrence evaluation
or specialize definition decisions in packed form rather than micro-optimize
the public projection tail.

The follow-up definition-DAG cut is also complete. After partial evaluation,
each summary now hash-conses identical pure inputs, terms, projections,
collections, selections, and records. Constraint publications, ordered
sequences, and nested invocations are deliberately excluded, and a pure parent
is eligible only when its complete local dependency graph is pure. Hash buckets
are lookup accelerators only: every hit is checked by exact node equality and
canonical order remains traversal-defined. This makes repeated type algebra one
definition-owned fact without caching a prior compilation or copying code into
callers.

The full NovyWave graph deduplicates another 24,848 nodes and retains 12,856;
only 915 nodes remain for final dead-code pruning. Summary evaluation falls
from 732,728 to 448,132 (38.8%), structural-widen requests from 132,286 to
96,553 (27.0%), `tree_row_text` from 88,032 to 40,143 evaluations, and
`material` from 71,949 to 57,317. From the typed-diagnostics checkpoint, the
combined normalization cuts remove 62.7% of stored summary nodes and 54.5% of
summary evaluation. Exact differential parity still passes for all 1,389
owners; all 75 kernel tests, 90 non-ignored compiler unit tests, and 16 compiler
integration tests pass.

Three source-current optimized samples record diagnostics candidates of
543.546, 535.264, and 538.935 ms (median 538.935 ms), with median kernel time
450.294 ms and graph solve 221.751 ms. Complete checked-image candidates record
716.208, 707.437, and 707.060 ms (median 707.437 ms), with median kernel time
619.730 ms, compilation 131.994 ms, and checked-image publication 162.351 ms.
Against the preceding receipt, complete diagnostics and checked-image latency
improve by 2.8% and 2.5%; this remains incremental progress, not the final
Jai-like result. The two-job optimized rebuild took 2m03s and is excluded.

A controlled alternative was rejected: allowing a callee that was large before
normalization to inline merely because its normalized bytecode became small cut
only 5% of evaluation, grew stored summary code from 25,162 to 41,564 nodes,
raised debug compilation from about 729 to 890 ms, and slowed the candidate by
about 8%. Shared ownership must therefore remain definition-based. The next cut
must reduce repeated shared-program evaluations or replace more of the 48,281
general graph operations with definition-owned packed transfer facts; caller
expansion is not an acceptable shortcut.

A second controlled alternative was also rejected. Extending the DAG pass to
deduplicate ordered `Sequence` nodes and nested `Invoke` nodes reduced stored
summary nodes from 12,856 to 12,508, summary evaluations from 448,132 to
418,146, material evaluations from 10,975 to 7,265, and structural widening
from 96,553 to 89,796. Despite those attractive counters, the optimized median
slowed from 538.935 to 548.545 ms for diagnostics and from 707.437 to
724.852 ms for the complete checked candidate; graph solve was effectively
unchanged. The experiment was fully reverted. Operation-count reduction is
therefore evidence to investigate, not a substitute for end-to-end latency.

The dependency-bottom source-expression diagnostic slice is now complete.
`KernelDefinitionFactsInput` carries typed diagnostic facts for invalid syntax
expressions and patterns, exact `Number` parse failures, invalid `BITS`
literals, and byte literals outside a direct `BYTES` constructor. Exact-number
reason and position remain typed kernel data; lower-level parser details are
retained only as display text where the stable data crate has no finer error
enum. The kernel validates dense expression sites once, publishes these facts
for diagnostics-only demand without constructing checked rows, and reuses the
same facts for checked-image demand. The compiler facade relocates each dense
site through the parser-stable expression key and immutable source-unit layout,
then presents a `TypeDiagnostic` that is exactly equal to the existing checker
authority, including severity and global byte/line coordinates. No legacy
checker database or diagnostic-string parsing participates in the kernel path.
The definition basis receipt is now V2 and artifact/currentness receipts are V4
because diagnostic inputs and serialized diagnostic variants changed.

The full NovyWave differential remains exact for all 1,389 owners with zero
unsupported owners and unchanged graph counts: 12,856 summary nodes, 448,132
summary evaluations, 48,281 graph operations, and 85,159 activations. A fresh
debug sample records 2,365.726 ms diagnostics candidate time, including
2,203.022 ms kernel time, 923.647 ms graph solve, and 17.305 ms interface and
diagnostic projection. The complete checked candidate records 2,777.981 ms,
including 765.765 ms compilation and 387.529 ms checked-image publication.
All 76 kernel tests, 91 non-ignored compiler unit tests, and 16 compiler
integration tests pass.

The optimized timing is recorded as a same-machine A/B receipt because machine
load had moved since the preceding checkpoint. Three source-current samples
record 616.397, 580.237, and 600.195 ms diagnostics (median 600.195 ms) and
812.959, 763.359, and 779.661 ms complete checked candidates (median
779.661 ms). Three immediate replays of the preceding `6ee61b2` release binary
record a 582.174 ms diagnostics median and a 767.410 ms complete median. The
current slice is therefore 3.1% and 1.6% slower in this controlled comparison;
it is not a speed win. The source scan finds no Novy diagnostics and accounts
for roughly 6 ms of the owner-projection delta even after linear/hashed indexing.
The two-job optimized rebuild took 3m50s and is excluded. This milestone is
accepted for ownership and parity; in the permanent input model these syntax
facts should be projected once with the immutable syntax revision rather than
rescanned by a solver demand.

The ordinary user-call target and lexical-shape diagnostic cut is now complete.
The kernel owns one normalized matcher for direct and piped calls: resolved
target kind, ordered VALUE/OUT formals, optionality, authored argument sources,
explicit PASS, and inherited context are compact inputs. It publishes dense
formal/source pairs only for a valid call. Unknown or ambiguous targets,
missing/extra/misordered entries, bare VALUE bindings, pipes without a VALUE
input, authoritative PASS misuse, and missing PASS context are typed diagnostic
facts. An invalid user call remains a supported `Unknown` expression with no
call artifact; it no longer ejects its entire owner from the dense project.
Invalid arity is diagnosed even when the resolved user signature contains OUT
parameters, while a shape-valid OUT or callback-arm frame remains explicitly
unsupported until its lexical declaration equations migrate.

The differential bridge now retains every same-spelling project callable
candidate instead of overwriting one target in a map, so ambiguity is
deterministic and never guessed. It also consults only cheap authoritative-name
ownership from the builtin, render, and host-effect registries. This distinction
is important: a valid authoritative call outside the current compact ABI remains
explicitly unsupported and is not mislabeled as an unknown user function. The
first implementation rebuilt the complete legacy owner ABI merely to obtain
that name set; the measured debug projection cost rejected that design, and it
was replaced with direct registry membership. The permanent
`KernelProjectInput` must carry the same resolved link/ABI overlay without a
dependency on the legacy typechecker.

Parser-backed regressions prove exact production diagnostic equality, including
argument and PASS anchors, for missing, extra, misordered, unresolved,
ambiguous, missing-context, piped, and OUT-arity cases. A separate adversarial
case proves a valid but unmigrated `Text/space` call remains unsupported. Typed
call diagnostics participate in both definition basis and exact artifact
currentness while preserving the public result fingerprint. The definition
basis receipt is therefore V3 and artifact/currentness receipts are V5.

The full NovyWave differential remains exact for all 1,389 owners with zero
unsupported owners. Graph work is unchanged at 12,856 summary nodes, 448,132
summary evaluations, 48,281 graph operations, and 85,159 activations. One fresh
no-rebuild debug receipt records 2,556.920 ms diagnostics candidate time,
including 2,383.917 ms kernel time, 989.565 ms graph solve, and 18.681 ms
interface projection. The complete candidate records 3,001.830 ms, including
2,828.827 ms kernel time, 508.869 ms owner projection, 802.684 ms program
compile, and 417.271 ms checked-image publication. Machine load was also higher
in this sample, so these numbers are not attributed as a call-matcher
regression or a speed improvement. No optimized rebuild was spent on this
semantic subcut; the preceding source-diagnostic checkpoint remains the latest
release receipt. All 78 kernel tests, 93 non-ignored compiler unit tests, and 16
compiler integration tests pass.

The authoritative lexical-call contract cut is now complete. `boon_checked`
owns a versioned, type-free `CheckedAuthoritativeCallableShapeV1` row containing
only the callable name/kind and dense ordered parameter names, VALUE/OUT kinds,
and optionality. A narrow typechecker compatibility adapter projects those rows
from the same builtin, render-constructor, typed-host-effect, and SessionInfo
registries that own the current checked signatures; it does not construct the
legacy owner ABI environment or expose recursive parameter/result types. The
compiler facade converts the rows once into a compact name index. The permanent
kernel remains dependency-firewalled from `boon_typecheck`, and the future
`KernelProjectInput` can receive the same overlay directly from the eventual
lower-level library-contract owner.

Every registry-backed authoritative call now runs through the same kernel
lexical matcher before a render, pure-builtin, or host-effect node is created.
Invalid calls publish typed missing/extra/misordered/PASS diagnostics, an
`Unknown` result, and no call or host-effect artifact. Shape-valid calls outside
the migrated residual slice remain explicitly unsupported. Dynamic `Field/*`
pipelines are the one project-derived language family: the checker creates their
single `input` signature from syntax, so the compact bridge does the same for
an argument-free field pipe rather than inventing a registry entry such as
`Field/color`. The first NovyWave differential exposed this distinction across
three Theme owners; modeling the general rule restored all owners without an
example-specific exception.

Parser-backed regressions prove exact production diagnostic and source-anchor
equality for `Text/slice`, `Scene/new`, `Random/bytes`, `Clock/wall`, and PASS on
`Text/empty`. A lower-layer invariant proves the projected table is uniquely
sorted, has dense ordinals and unique parameter names, and preserves required
versus optional surfaces for representative builtin, render, host, and
SessionInfo entries. No receipt domain changed because this cut populates the
existing typed call-diagnostic variants and fingerprints.

The fresh full NovyWave debug differential is exact for all 1,389 owners with
zero unsupported owners and unchanged graph counts: 12,856 summary nodes,
448,132 summary evaluations, 48,281 operations, and 85,159 activations. It
records 2,635.821 ms diagnostics candidate time, including 2,442.008 ms kernel
time, 985.925 ms graph solve, and 19.284 ms interface projection. The complete
candidate records 3,065.042 ms, including 538.478 ms owner projection,
825.932 ms program compilation, and 405.906 ms checked-image publication. This
single debug receipt is parity and scale evidence, not a claimed speed change;
machine load remains above the prior sample. No optimized rebuild is spent on
this semantic subcut.

The remaining residual-module ranking must keep shrinking, but a smaller
interpreter cannot substitute for compiling each definition once. Release
improvement is accepted only when the complete candidate path improves, not
merely when graph counts fall.

### Stable semantic relocation authority

The checked-image audit exposed one necessary reordering of the greenfield
plan. Exact semantic facts cannot all wait until after checker cutover: the
checked linker needs a definition-local source relocation for every dense
expression and statement, or it must rediscover the same owner graph from the
parser after solving. `KernelDefinitionFactsInput` and `DefinitionArtifact`
therefore now carry `KernelDefinitionRelocations` as first-class immutable
authority.

Expression relocations explicitly distinguish an authored
`StableExpressionKey` from a `SyntheticDefinitionResult`. The latter is needed
when a structural owner publishes a record composed only from child owners and
there is deliberately no parser expression to name that aggregate. The kernel
validates exact dense counts, uniqueness, and source-unit ownership and refuses
to manufacture a fake authored key. Relocations participate in the definition
basis, artifact, and exact-currentness fingerprints; their domains advance to
basis V4 and artifact/currentness V6. Public-result fingerprints remain
unchanged when only source structure moves, preserving semantic backdating.

The full debug NovyWave differential after this cut retains every relocation
and remains exact for 1,389 solved owners plus eight structural containers,
with zero unsupported owners. It records 59,302 operations, 95,483
activations, 2,607.416 ms direct production diagnostics including parse, and a
3,091.634 ms complete checked candidate. The latter includes 420.620 ms of
checked-image publication. This cut adds linker authority rather than claiming
a latency improvement, so no optimized rebuild was spent on it.

This does not make the old checked assembler part of the new design. The next
cut retains exact literal and execution facts beside these relocations, then a
single dense linker projects `KernelCheckedSnapshot` into the existing
`boon_checked` model. Only after that direct projection passes the full parity
and budget gates can production checked-image requests cut over and the old
owner row builders be deleted.

Run the debug probe after each meaningful semantic slice. Run the release probe
at architecture boundaries or when a debug profile changes materially; do not
spend a full optimized rebuild on every small edit. Keep the latest accepted
debug and release receipts in this section, including solved/unsupported counts
and rebuild time, so a faster partial slice cannot be confused with a faster
complete compiler.

The next migration is architecture-first, but production changes only in one
checker-wide flag day:

1. Finish `DefinitionArtifact` from the dependency bottom upward: declaration
   and lexical-binding identities first, then HOLD state, persistent LIST, and
   SOURCE resource rows. Reuse the existing dense expression and statement
   references; never create parallel source-shaped resource drafts.
2. Complete exact diagnostics in `KernelCheckedSnapshot`. Public callable
   substitutions, currentness receipts, dependency-cone rows, and the first
   call-input diagnostic family are complete. Source-expression syntax and
   literal diagnostics plus their exact source presentation are also complete,
   as are ordinary user target resolution and lexical call-shape diagnostics,
   plus authoritative builtin/render/host lexical signatures. Next add valid
   OUT/callback lexical equations, pipeline/link and
   remaining name-resolution/presentation facts. Owners containing an invalid
   call must still produce a bounded artifact with an unknown result instead of
   becoming globally unsupported. Extend Counter, TodoMVC, and NovyWave
   differential gates over each new family before moving to the next one;
   collect whole mismatch inventories instead of repairing one serialized
   failure per run.
3. Freeze each public interface separately from its private compiled definition
   and fingerprint both once. Finish the permanent `KernelProjectInput`,
   one-revision `KernelSession`, and `CheckDemand` boundary without importing
   old owner DTOs or adding a selectable legacy path.
4. Require fresh-process determinism, complete owner and artifact coverage,
   NovyWave cold diagnostics below two seconds, and zero provider-wide scans,
   separate body solves, and source-shaped residual interpretation. Warm caches,
   extra threads, or partial candidate paths do not count toward these gates.
5. Perform one checker-wide production cutover. In the same tranche delete all
   legacy owner interface/body unifier machinery, flow/capture replay,
   source-shaped residual preparation and recursive evaluation, duplicate
   checked-row reconstruction, and superseded session requests/fingerprint
   domains. The old implementation may remain only as explicitly test-gated
   oracle code until the final parity suite is archived; it is never a
   production fallback.
6. Continue the clean-slate path downstream: normalized semantic facts per
   definition, compact invocation frames, one shared plan-code module per
   definition, and a consuming linker/seal. Do not split more crates until a
   measured one-way dependency seam justifies it.

Renderer semantics are a separate ABI migration, not a language special case.
`NoElement` is an ordinary user/library tag. The existing checker currently
recognizes that spelling in structural widening and render validation only as
legacy built-in UI coupling; the dense kernel deliberately does not. Likewise,
`Type::RenderContract` is a compiler-owned wildcard used by the hard-coded
Document/Scene constructor registry, not a source-level data type. Replace both
mechanisms flag-day with an explicit renderer/library ABI that supplies callable
signatures and accepted slot shapes to the typechecker/backend. Then delete
`Type::RenderContract`, the built-in renderable tag/name tables, and all
`NoElement` name tests from the core type algebra. Different UI libraries must
be free to use another absence convention or no absence tag at all.

Based on the measured old call amplification (1,251 requested roots becoming
18,690 owner evaluations, 17,439 nested frames, and 235,484 residual source-node
visits), eliminating recursive acyclic dispatch should make the owner
interface/body portion roughly 3--8x faster. With the remaining parse,
checked-row, and diagnostics work still present, the first whole diagnostics
cut is estimated at 2--5x. These are planning ranges, not acceptance evidence.
The 250 ms diagnostics and one-second verified goals still require the direct
parser projection, real persistent request evaluator, demand-owned definition
artifacts, thin linking, and deletion of the rich semantic/Manifest assembly
described above.

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

- every V3 subject maps to exactly one canonical finalized shard row plus an exact
  classifier field/domain, and every executable database row is covered;
- folding the independently materialized V3 subject classifications by their
  canonical row and projection produces the same owner/projection commitments
  as the production unit seal. Production does not allocate one receipt per
  historical child-field subject: the canonical row fingerprint already binds
  every field, while its typed dependency span binds external reads;
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
bounded by actual sealed database rows and declared owner/projection regions
plus measured compact exceptions rather than rich serialized-field
cardinality, and end-to-end time/RSS both improve. Final acceptance still uses
the performance plan's complete protocol.

### 3. One Sealed Semantic Image, Not Nine Rich Graph Authorities

`SemanticProgram` currently retains the complete checked program, resolved OUT
graph, execution, resource, reactive, lowering, view, storage, and memory
graphs, a canonical core, and the proof manifest simultaneously. Several
builders derive maps, validate, serialize/hash, and later rescan overlapping
rows. IR ultimately consumes only the canonical core and bound digests;
verification additionally reads a narrow reactive projection.

Introduce mutable revision construction and immutable sealed views inside the
language-owned `SemanticImageBuilder`, with `CompilationDb` owning only stable
request graph/currentness metadata:

- definitions, calls, occurrences, resources, reactions, storage, views, and
  memory use typed dense columns with stable owner/local keys;
- one canonical edge arena and shared indexes replace component-local maps
  that describe the same relationship;
- component APIs are read-only typed projections, not separately owned DTO
  graphs;
- row fingerprints are computed once at typed finalization and feed proof,
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

### 4. Link Plan Code And Build The Runnable Image Once

The backend currently constructs a large mutable plan, clones the complete
plan to refresh typed-list view fingerprints, compacts, validates, hashes, and
serializes through distinct traversals. It also compiles ordinary roots
independently in document, row/scalar, and migration paths, after which each
trusted consumer rebuilds executor metadata. Replace these owners with one
shared plan-code linker and one consuming runnable-image builder:

- accept only demand-collected verified instance rows;
- retain each ordinary executable function variant once across document,
  row/scalar, and migration domains, keyed by execution domain, resolved
  layout, overlay/control shape, and capability contract; encode call-specific
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
- return a non-forgeable `SealedRunnableMachine` containing immutable compact
  plan tables, dense executor indexes built exactly once, the already computed
  canonical digest, successful verification receipt, and minimal provenance.

The runtime consumes only sealed plan functions/frames and existing typed
kernels; it must not regain a semantic AST interpreter or production flat
fallback. The public verifier remains mandatory, but repeated validation of the
same immutable payload at adjacent trusted ownership handoffs is removed.
Deserialized/untrusted plans always verify and build runnable indexes once
before receiving a seal. Trusted consumers do not rebuild executor `Metadata`.
JSON/debug output streams from the sealed plan and is not required for an
in-memory preview. Compiler-internal distributed linking may retain
construction IR only until the link seals, then drops it.
The scored producer continues to include whatever serialization the manifest
declares, so no work is hidden from the gate.

Directional exit: backend plus runnable seal/validation fits 300 ms,
publication hash/serialization fits 100 ms, no full-plan clone or trusted
metadata rebuild remains, and plan behavior,
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

1. Preserve activation checkpoint `32bcf40`, compact-proof/sealed-plan
   checkpoint `38e6541`, and shared-request-graph checkpoint `c870358`. Preserve
   exact activation/effect/migration behavior and the independent flat/V3/
   clean-full oracles while changing ownership; do not resume game or later
   product work. The complete real-host migration/restart/provenance matrix is
   still a phase-acceptance gate, not a reason to delay the compiler cut.
2. Preserve `a48f488`'s unit-native production route, then finish its typed
   identity boundary. Retain immutable parser-unit snapshots and a body-
   insensitive item index; emit typed local node keys and compact parent/item
   metadata during parsing; make project module/name resolution an immutable
   overlay rather than an AST rewrite/revalidation pass. Distinguish syntax
   keys, checked definition-local slots, stable owner/occurrence keys, and
   linked IDs at the type level. Raw text, offsets, lines, packed/dense fallback
   IDs, and revision-local arena IDs are not cross-revision keys.
3. Turn `CompilationDb` into a typed request evaluator and currentness service,
   not a post-build graph snapshot. Keep compiler
   evaluation/reverse-cone edges distinct from proof/link relocations, while
   publishing both from one artifact. Add typed result slots, evaluator-owned
   dependency capture/cycle detection, revisions, backdating, generation-
   checked publication, work counters, and bounded cancellation. The next
   revision must actually use these memos to choose work. Do not wrap the old
   whole phases in a query facade.
4. Extract interface SCCs and immutable checked-definition shards from the
   fresh monolithic checker. Converge cross-definition interface constraints at
   SCC granularity and evaluate bodies under frozen interfaces instead of
   rerunning a program-wide fixed point. Emit checked receipts while
   constructing those shards and delete production `checked_image_handoff`;
   prove a body edit with a backdated interface performs zero unrelated checks.
5. Migrate one ordinary definition end to end into a demanded
   `DefinitionExecutableArtifact`: checked body, diagnostics/source map,
   semantic rows, proof relocations, and normalized plan code. Encode calls as
   compact invocation frames and delete the matching OUT/contextual plus
   document, row/scalar, and migration recursive body owners together.
6. Expand definition and construction-owned domain artifacts through OUT,
   resource, reactive, lowering, storage, view, memory, migration, and
   distributed authority. Delete each superseded rich graph/Manifest inventory
   as its independent source materializer passes; never stack the replacement
   underneath every old DTO.
7. Run producer/external-event/distributed closure over compact summaries and
   typed relocations. Thin-link only demanded modules, seal one contract-
   verified linked image, and delete post-seal `SemanticProgram` retention,
   duplicate canonical mapping/hash, and full-role confirmation elaboration.
8. Consume the linked image into one runnable builder. Normal compilation
   returns `SealedRunnableMachine` with final dense tables and executor indexes
   built once; explicit debug/serialization requests own pretty JSON or rich
   views, and untrusted deserialization verifies/builds indexes exactly once.
9. Reprofile the complete cold and warm paths after each coherent owner
   deletion. Return to a local optimization only when the new trace proves it
   is the largest remaining owner; the scored p95/RSS gates remain the only
   performance exit. Close exact cones, add/delete/rename, error recovery,
   cancellation, stale/latest races, clean-full parity, and every warm gate.
10. Pull measured dependency inversions and crate splits only at the stable
    syntax/item, checked model/builder, definition/domain artifact, thin-link,
    runnable model/builder/executor, compiler-service/adapter, and migration-
    tooling seams. Enable at most two deterministic workers only for graph-
    proven independent requests. Never let a re-export facade or file-only
    split replace steps 2--9.
11. Run the full cold and warm acceptance protocol, complete migration/restart/
    provenance negatives, and the three fresh-context adversarial reviews
    required by the performance plan.

Checkpoint commits are phase evidence, not exits. Do not push unless the user
explicitly asks. Do not begin game work.

## Refactor Rejection Rules

Reject a candidate when any of these is true:

- it retains the 160k rich-record proof graph in production under a new
  container, or coarsens exact dependency cones merely to reduce node count;
- it adds a second executable semantic authority or production flat fallback;
- it caches a whole-project product without explicit currentness and exact
  dependencies;
- it makes the program/root owner a dependency target for unrelated top-level
  facts, retains a giant SCC, or treats revision-local dense IDs as stable keys;
- it fingerprints mutable rows before finalization or uses one digest for local
  backdating, linked targets, and final image encoding;
- it retains per-domain recursive ordinary-body lowering or rebuilds executor
  metadata for each trusted consumer beside a nominal shared seal;
- it uses internal dense IDs as cross-revision, persistence, or oracle
  identities;
- it loses startup effects, normalizes async completion order, or weakens turn
  equality to make the differential harness pass;
- it skips complete diagnostics, exact proof acceptance, plan verification, or
  clean-full incremental parity;
- it counts parallelism, a profile change, a timeout increase, crate movement,
  or a debug-only speedup as satisfying a release Boon latency gate;
- it leaves the old owner/facade alive after the flag-day replacement passes.
