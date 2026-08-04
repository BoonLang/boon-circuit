# Boon Compiler Architecture Refactor Plan

Date: 2026-08-03

Status: active high-leverage execution map, reconciled after shared-request-
graph checkpoint `c870358` while preserving compact-proof/sealed-plan checkpoint
`38e6541` and activation/effect checkpoint `32bcf40`, subordinate to
[`BOON_COMPILER_PERFORMANCE_PLAN.md`](BOON_COMPILER_PERFORMANCE_PLAN.md). The
performance plan owns all latency, memory, correctness, and final acceptance
gates. This file owns the architectural sequence first chosen after checkpoint
`968c56a` and strengthened by the post-`32bcf40`, post-`38e6541`, and
post-`c870358` source/
primary-reference research below; it does not create a second set of weaker
exits.

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
  SourceUnitSnapshot -> InterfaceShard + DefinitionShard
    -> demand-collected InvocationShards
    -> ephemeral LinkFixedPoint over relocations and compact summaries
    -> one SealedSemanticImage with exact proof/currentness receipts
    -> shared plan-code linker across document, row, and migration domains
    -> one consuming RunnableMachineImage builder
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
  -> SemanticImageBuilderV2<Local>
  -> compact role/bundle link summaries + relocation fixed point
  -> SemanticImageBuilderV2<Linked>
  -> narrow proof view + verification receipt
  -> SealedSemanticImageV2
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
2. Unify fail-closed revisions and specify stable source/interface/definition/
   invocation/top-level/link keys, small projection kinds, local/linked/image
   fingerprint domains, relocation identities, and `Pending -> Finalized` row
   contracts. Retain immutable parsed-source-unit snapshots; support atomic
   upsert/remove/rename and exact cached project assembly.
3. Narrow the database graph to dense registered projection IDs. Make
   typechecking emit interface and definition shards, then migrate complete
   checked plus execution rows and finalization-time receipts. Independently
   reconstruct source/V3/V4 facts in tests and delete the corresponding
   production post-hoc inventories in the same coherent batch.
4. Introduce invocation shards and expand the same image builder through OUT,
   resource, reactive, lowering, storage, view, and memory in dependency order.
   Delete each superseded rich graph owner as its borrowed view/test
   materializer passes; do not stack the row image underneath every old DTO.
5. Run producer/external-event/distributed link fixed points over compact
   summaries and relocations. Seal one role/bundle `SealedSemanticImage`, give
   verification a narrow proof view, and delete post-seal `SemanticProgram`
   retention plus duplicate canonical mapping/hash.
6. Add one shared plan-code linker across document, row/scalar, and migration
   domains. Demand-collect `(definition, specialization key)` pairs, emit dense
   invocation frames, and delete all corresponding cache scopes, parameter-
   binding stacks, and recursive function-root lowering together.
7. Replace full-plan clone/rewrite/compaction and per-consumer executor metadata
   construction with one consuming runnable-image builder. Normal compilation
   returns `SealedRunnableMachine`; explicit debug/serialization intents own
   their extra products, and untrusted deserialization verifies/builds indexes
   exactly once.
8. Reprofile the complete cold path after each coherent owner deletion. Return
   to a local optimization only when the new trace proves it is the largest
   remaining owner; the scored p95/RSS gates remain the only performance exit.
9. Retain these same snapshots, shards, link results, proof regions, plan-code
   variants, and runnable indexes across revisions. Close interface backdating,
   exact invalidation cones, add/delete/rename, error recovery, cancellation,
   stale/latest races, clean-full parity, and every warm time/RSS gate. Enable
   at most two deterministic workers only for graph-proven independent work.
10. Pull measured dependency inversions and crate splits at the earliest stable
    seams that shorten the next tranche. Priorities are outer compiler adapters
    out of runtime cores, migration tooling out of host core, stable semantic
    image/model versus builder/proof, and runnable image/model versus executor.
    Never let a re-export facade or file-only split replace steps 2--9.
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
