# Boon Compiler Macro-Architecture Research

Status: selected post-`e510726` architecture refinement; subordinate to
`BOON_COMPILER_PERFORMANCE_PLAN.md` and additive to
`BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md`.

This document records the required high-level audit after structural occurrence
identity landed. It ranks whole-owner deletions and representation changes above
local map, allocation, hashing, route-storage, or fixed-point tuning. It does not
weaken any cold, warm, cancellation, RSS, determinism, migration, native, or
adversarial-review gate.

## Decision

Keep the previously selected definition-artifact and thin-link direction, but
broaden it into one **snapshot -> normalized facts -> compositional seals**
compiler architecture:

1. source remains a set of immutable unit snapshots; the production compiler
   stops assembling one global `ParsedProgram`;
2. interface and checked-body requests publish immutable definition shards;
   diagnostics aggregate those shards without building a runtime handoff;
3. semantic builders publish normalized fact sections and typed relocations
   exactly once instead of retaining a graph-of-graphs and then rescanning it;
4. each definition owns one target-neutral plan-code module, while occurrences
   own compact invocation frames;
5. a thin linker consumes summaries and relocations, verification seals the
   linked image, and a consuming builder publishes the runnable machine;
6. semantic, verified, erased, and runnable boundaries are opaque typed seals
   over immutable owned sections, not repeated whole-program materializations;
7. rich graphs, pretty JSON, exhaustive human-readable reports, and legacy
   `MachinePlan` export are explicit debug/evidence requests rather than the
   normal preview path.

The immediate implementation order changes accordingly. Do not build checked
definition shards on top of revision-global expression ids that will be deleted
later. First remove global syntax assembly from the persistent compiler route,
then implement typed request currentness and definition shards, then delete the
checked handoff and downstream whole owners.

## Fresh Current-Tree Evidence

All measurements below are directional debug observations from the current
`e510726` source and prebuilt compiler. They rank owners; they are not release
acceptance and their costs are not assumed to add independently.

### Whole request

| Phase | Current observation |
| --- | ---: |
| verified request | 4,251.545 ms |
| peak RSS | 282,756 KiB |
| parse | 120.640 ms |
| typecheck | 708.498 ms |
| semantic | 2,266.805 ms |
| contract verification | 0.500 ms |
| IR erasure | 43.910 ms |
| backend | 917.666 ms |
| plan validation | 106.305 ms |
| pretty serialization/hash in the scored producer | 535.917 ms |

The canonical plan hash remains
`db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`.

### Syntax ownership

The eight unit-local AST parses total roughly 45 ms in the current parser trace.
After they finish, canonical bundle validation takes another 18.000 ms and unit
assembly takes 36.017 ms. `CompilerSession` retains each `ParsedSourceUnit`, but
`parse_project_snapshot` still clones every retained unit, rebases every dense
id/span/line into a global arena, reconstructs one source string, and validates
the complete bundle for every request.

The structural occurrence route checkpoint adds about 16--24 ms because routes
are currently derived by a post-parse tree traversal. That cost is visible debt,
but tuning its vectors and maps is lower leverage than deleting global assembly:
the target unit representation can emit compact parent/slot metadata while
parsing and never allocate revision-global routes at all.

### Checked ownership

The current diagnostics trace divides typechecking into two very different
owners:

| Checked owner | Current observation |
| --- | ---: |
| actual checked-program construction and validation | about 210.149 ms |
| ordered diagnostic projection | 26.724 ms |
| `assemble_report` | 408.603 ms |

`assemble_report` calls `checked_image_handoff` after the checked scopes,
declarations, statements, expressions, callables, calls, resources, and
metadata already exist. That scanner canonical-serializes a second checked
projection with 63,657 rows. `DiagnosticsOwned` still builds it so a later
verified request can consume the same whole `CheckedProgram`; therefore the
diagnostics intent is coupled to runtime publication even when the user only
needs diagnostics.

The replacement is not a lazy wrapper around the same scanner. Checker requests
publish `CheckedDefinitionShard`s and definition-level receipts while they own
construction. A diagnostics request aggregates their diagnostics. A verified
request reuses the same shards and asks for executable artifacts. Once parity
passes, `checked_image_handoff` and its row projection schema leave production.

### Semantic ownership

The current semantic artifact retains a sealed semantic image beside resolved
OUT, resource, reactive, lowering, view, storage, memory, canonical-core,
Manifest, and request-graph products. The trace exposes the multiplier:

| Semantic owner | Current observation |
| --- | ---: |
| verified intent | 20.349 ms |
| OUT graph plus resolve/validate | 151.038 ms |
| contextual materializations | 138.697 ms |
| semantic execution graph | 521.630 ms |
| execution normalization/finalization | 50.003 ms |
| resource graph | 183.945 ms |
| reactive graph | 94.308 ms |
| lowering contract | 42.772 ms |
| storage graph | 190.920 ms |
| view plus memory graphs | 18.792 ms |
| canonical core | 43.110 ms |
| execution receipt rescan | 286.195 ms |
| dependency Manifest/request graph | 422.191 ms |
| semantic whole-artifact digest | 99.994 ms |

The execution image contains 16,525 expressions and 3,494 call occurrences.
Its handoff emits 30,771 rows and 9,037 relocations. Manifest then combines
63,657 checked rows, those 30,771 execution rows, 2,620 construction rows, and
35,448 remaining-domain rows into 8,315 request/proof nodes and 29,131 edges.
These counts prove that the compiler is hashing and classifying overlapping
representations, not merely running one expensive algorithm.

### Backend and publication ownership

Backend tracing reports 764.193 ms before document lowering, 103.572 ms in the
document backend, and 29.743 ms in finalization. Production contains three
independent recursive ordinary-call lowerers:

- `ExecutableMigrationExpressionLowerer`;
- `ExecutableRowLowerer`;
- the document executable backend's ordinary-call compiler.

The document path alone creates 33,576 expressions, 46,459 expression-cache
entries, 2,320 ordinary-call scopes, and 12,701 cache scopes. Those are symptoms
of per-occurrence recompilation. A shared plan-code module per definition plus
resolved invocation frames must replace all matching recursive body owners in
one vertical tranche.

`seal_machine_plan` then calls `verify_plan`, which scans the completed plan and
encodes its full canonical binary to compute `plan_sha256`. The scored producer
also serializes pretty JSON solely to hash/compare the artifact. Trusted
in-memory preview therefore pays work that belongs to construction or explicit
export.

## Root Architectural Problems

### 1. Retained units still feed a batch-shaped global AST

`ParsedSourceUnit` is now reusable, but every semantic consumer still addresses
nodes by revision-global dense ids. Reassembly is therefore unavoidable and a
one-unit edit invalidates every downstream key. This prevents the retained
syntax work from becoming true incremental compilation.

### 2. Request intents meet too late

Diagnostics, editor, and runtime requests differ mostly at report assembly,
after the checker has already built a whole checked product. The intent should
select request roots at the beginning:

```text
Diagnostics -> all required interfaces/bodies -> ordered DiagnosticSet
Editor      -> cursor/visible definitions + requested sidecars
Verified    -> verified roots -> demanded executable artifacts -> thin link
Export      -> runnable image -> explicit canonical/debug serialization
```

All intents share memoized definition results. They do not share one eagerly
completed whole-program artifact.

### 3. Semantic domains own representations instead of facts

Each domain builds a rich graph, validates it, hashes it, and later projects it
into another owner. This duplicates indexes and lifetime. The canonical
production owner should instead be typed normalized tables plus relocations.
Domain-specific graph objects become optional materialized views for tests,
debugging, and formal evidence.

### 4. Phase proof is retrospective

Checked handoff, execution handoff, Manifest, semantic digest, plan validation,
and artifact serialization repeatedly prove a completed snapshot after its
builder already knew the invariant. Trusted construction should publish a typed
section seal at the moment the final row span closes. Untrusted persisted input
still receives complete independent validation.

### 5. Lowering follows occurrences rather than definitions

OUT/contextual expansion and three backends rediscover call bindings and walk
ordinary bodies. This makes compile work scale with call occurrences and
context paths. Only semantic specialization dimensions should create a new
definition variant; an ordinary occurrence should be a small frame.

### 6. Rust crate boundaries still mix stable models with volatile builders

`boon_semantic` mixes the semantic model with more than 80 kLOC of builders and
domain logic, so any builder change recompiles `boon_verify`, `boon_ir`, and the
compiler backend. `boon_plan` similarly mixes a large public model with global
validation. `boon_compiler` contains the session, distributed solver, three
lowering worlds, and publication. Splitting by today's files would preserve the
cycles; model/builder/link seams must be created first.

## Selected Target Architecture

```text
ProjectSnapshot
  UnitSyntaxSnapshot[SourceUnitId]              immutable, unit-local arenas
  ProjectItemIndex                              body-insensitive interfaces
  SourceMapIndex                                diagnostics only

CompilationDb
  InterfaceSccResult[InterfaceSccKey]
  CheckedDefinitionShard[StableDefinitionKey]
  DefinitionExecutableArtifact[DefinitionVariantKey]
  DomainFactSection[AuthorityKey]
  ThinLinkResult[IntentRoots + summary fingerprints]
  VerifiedLinkedImage[link digest + proof seal]
  SealedRunnableMachine[runnable digest]

ArtifactStore
  immutable typed sections
  stable-key -> section/span index
  evaluation/currentness dependency spans
  proof/link relocation spans
  construction receipts and source-map spans
  revision delta journal
```

`ProjectSnapshot` never concatenates source or rebases unit-local arenas.
Project-wide ordering is an index over `(SourceUnitId, local route)`, not a new
copy of every syntax node. A dense id exists only inside one immutable unit or
one linked image.

`ArtifactStore` is not a dynamically typed cache and is not a second semantic
authority. Language-owned builders append typed rows to immutable sections and
publish one final receipt. The same receipt covers the evaluation dependency
span and proof/link relocation span while keeping those edge planes distinct.

The required public artifact order remains intact:

```text
Parsed -> Checked -> Semantic -> ContractVerified -> Erased -> Runnable
```

Each arrow consumes or extends an opaque typed seal over immutable sections.
Having shared immutable storage does not permit a backend to obtain an erased
or runnable capability before `ContractVerified` exists. The phase type carries
the authority; the section store carries bytes once.

## Macro Refactor Opportunities, Ranked

| Rank | Refactor | Whole owner deleted | Measured envelope affected |
| ---: | --- | --- | ---: |
| 1 | unit-native `ProjectSyntaxSnapshot` | canonical bundle rebuild/rebase and revision-global syntax ids | 54 ms explicit assembly/validation plus warm invalidation |
| 2 | intent-rooted checked definition requests | whole checked rebuild and `checked_image_handoff` | 409 ms rescan plus unrelated warm checks |
| 3 | normalized semantic fact sections | execution/domain graphs followed by receipt/Manifest rescans | 286 ms handoff + 422 ms Manifest + overlapping domain work |
| 4 | definition plan-code modules | three recursive ordinary-body lowerers and occurrence cache scopes | much of the 764 ms pre-document backend plus semantic occurrence expansion |
| 5 | thin link and compositional phase seals | canonical-core remap/hash, full plan validation, normal-path export | 100 ms semantic digest + 106 ms plan validation + 536 ms scored serialization path |
| 6 | distributed summary/delta link | full three-role re-elaboration and confirmation pass | distributed fixtures; not represented by the single-role sample |
| 7 | model/builder/link crate extraction | broad Rust recompilation/relink cones | current release rebuild reported around 95 s |

The table is a prioritization, not a promise that times subtract linearly.
Every exit requires fresh measured samples and exact semantic/proof parity.

## Normalized Facts And Incremental Views

The semantic store should begin with deterministic typed tables, not a general
incremental-Datalog dependency. Example base sections include:

- definition interfaces and executable rows;
- invocation frames and type/OUT substitutions;
- source, state, list, effect, storage, view, and migration authorities;
- evaluation dependencies;
- proof/link relocations;
- source-map and diagnostic provenance.

Each table has one stable-key index, one dense row span, and one revision delta
journal. Domain builders consume indexed spans and publish delta rows. Shared
indexes are constructed once and borrowed by all domain views; each graph may
not rebuild its own owner/path/call/resource map.

Differential-dataflow and DBSP research is relevant because both maintain
derived collections from input changes and share indexed arrangements. Boon
should adopt the invariants before the machinery: explicit positive/negative
deltas, revision timestamps, shared arrangements, and deterministic fixed-point
worklists. Do not add a heavy dataflow runtime until a normalized hand-written
delta implementation exists and a measured recursive global domain proves that
it cannot meet the gates. The compiler remains single-threaded by default and
bounded to two independent workers later.

## Compositional Seals And Artifact Hashing

Every section builder must:

1. reserve and append its canonical typed rows;
2. validate local references while appending;
3. close the row and relocation spans exactly once;
4. compute a versioned section digest over all fields;
5. publish a typed construction receipt;
6. become immutable.

The link seal commits to ordered section identities, section digests,
relocations, demand roots, and proof coverage. A runnable seal commits to the
verified link plus final dense layouts and executor indexes. An unchanged
section is reused without decoding, revalidating, or rehashing its rich source
representation.

Changing the public plan digest is not an incidental optimization. A sectioned
Merkle/compositional digest requires a versioned flag-day artifact format and
semantic/migration parity. Until that cut lands, explicit legacy export may
still reproduce the exact historical canonical `MachinePlan` bytes and hash;
normal in-memory preview must not invoke that exporter. Untrusted persisted
artifacts validate section schemas, hashes, relocations, and proof coverage
before they obtain a runnable seal.

## Crate And Rebuild Architecture

Accept crate splits only after these one-way seams exist:

1. `boon_syntax` remains the unit syntax/identity model; `boon_parser` builds
   unit snapshots without knowing compiler sessions.
2. `boon_checked` remains the checked interface/shard model;
   `boon_typecheck` is the sole builder and issuer.
3. extract a small semantic/artifact model below the volatile semantic builders;
   downstream verification and linking depend on the model, not builder code.
4. extract thin-link model and linker below compiler adapters.
5. extract runnable model below its consuming builder and executor.
6. keep compiler service/session and CLI/native/web adapters above every model.
7. keep migration/debug exporters outside runtime cores.

For each split, capture the normal dependency graph and a touched-file Rust
rebuild trace before and after. A split fails if it merely moves source, adds a
compatibility re-export, preserves a cyclic dependency, or does not reduce the
affected rebuild/relink cone. Rust build speed and Boon compilation latency are
reported separately.

## Implementation Sequence

### M1. Delete production global syntax assembly

- Add opaque `ProjectSyntaxSnapshot` over `Arc<UnitSyntaxSnapshot>` plus a
  body-insensitive `ProjectItemIndex`.
- Give every syntax node a unit-local key; remove revision-global expression,
  statement, line, byte, and file inference from compiler identities.
- Migrate the typechecker's read boundary and diagnostic source mapping to unit
  snapshots and stable routes.
- Delete `assemble_parsed_source_units` from `CompilerSession` production
  requests. It may remain only as an independent parser parity oracle until the
  unit-native typechecker gate passes.
- Fuse compact occurrence parent/slot emission into unit parsing while removing
  the post-parse route traversal; do not create another route owner.

Exit: one-unit warm edits parse only that unit, perform zero cloning/rebasing or
whole-bundle validation, preserve ordered diagnostics and the exact verified
artifact, and expose zero global syntax ids to later request keys.

### M2. Install typed request currentness and definition checking

- Implement typed request slots, generations, evaluation edges, reverse cones,
  backdating, cancellation, and publication counters in `CompilationDb`.
- Build body-insensitive interface SCC requests and immutable checked-definition
  requests keyed by stable definition identity.
- Make diagnostics aggregate definition-local diagnostics without constructing
  executable handoff rows.
- Emit definition receipts during checking and delete production
  `checked_image_handoff` plus its 63,657-row projection.

Exit: a body/constant edit with an unchanged public interface checks only its
affected definition cone; diagnostics report zero runtime artifact rows; a
verified request reuses those exact checked shards; `assemble_report` no longer
contains a whole-program handoff scan.

### M3. Carry one definition through normalized semantic facts and plan code

- Publish one `DefinitionExecutableArtifact` containing checked body receipt,
  semantic rows, plan-code module, source map, evaluation dependencies, and
  proof/link relocations.
- Replace the matching OUT/contextual occurrence body expansion with compact
  invocation frames.
- Delete that definition's document, row/scalar, and migration recursive body
  lowering paths in the same tranche.
- Keep the old path only behind an independent test oracle, then remove it when
  exact artifact/behavior/proof parity passes.

Exit: the migrated definition body is compiled once per justified variant and
every occurrence is a resolved frame; no production fallback can recursively
lower that body.

### M4. Replace graph-of-graphs with domain fact sections

- Migrate resource, reactive, lowering, storage, view, memory, migration, and
  distributed facts in dependency order.
- Share stable indexes/arrangements and publish deltas from changed authorities.
- Materialize rich graphs only for explicit test/debug requests.
- Delete each production graph, receipt rescan, and Manifest inventory as its
  omission/mutation/cone oracle passes.

Exit: each semantic fact has one production owner and one construction hash;
warm edits execute only affected domain worklists.

### M5. Thin link, verification seal, and consuming runnable builder

- Compute demand closure from verified roots over definition/domain summaries.
- Resolve relocations and explicit semantic SCCs once.
- Bind proof completeness into `ContractVerifiedLinkedImage`.
- Consume it into `SealedRunnableMachine`, assigning final dense ids and
  executor indexes once.
- Move legacy plan export and pretty JSON to explicit requests; adopt the
  versioned sectioned artifact only with full migration/parity evidence.

Exit: normal preview performs no rich semantic materialization, whole-plan
clone/rewrite, retrospective trusted validation, binary re-encode solely for a
hash, pretty serialization, or per-consumer executor rebuild.

### M6. Distributed deltas and measured crate extraction

- Publish per-role summaries and changed relocation/authority deltas.
- Delete full three-role re-elaboration and the confirmation rebuild.
- Split model, builder, link, runnable, and service crates only after their
  one-way boundaries and measured Rust rebuild reductions exist.
- Enable at most two workers only for proven independent request cones.

Exit: role-local edits relink only their affected distributed cone, and both
Rust rebuild and Boon compilation reports prove the claimed improvements.

## Required Harness Changes

Add counters and hard assertions for:

- unit parses, unit clones/rebases, and global syntax materializations;
- interface/definition requests executed, reused, backdated, canceled, and
  superseded;
- diagnostics-only executable rows (must be zero);
- definition artifacts and invocation frames built/reused;
- domain base rows, input deltas, output deltas, and rich graph materializations;
- section bytes, simultaneously live artifact bytes, section rehashes, and
  whole-artifact encodes;
- plan-code modules lowered and recursive legacy body-lowering entries;
- thin-link summaries/relocations/SCCs visited;
- runnable sections rebuilt and executor indexes reconstructed;
- explicit debug/export materializations;
- Rust crates rebuilt/relinked for representative touched files.

The cold path uses the same requests with an empty database. The warm gate may
not pass by retaining the final runnable artifact, skipping correctness work,
or hiding work from counters.

## Rejection Rules

Reject an implementation that:

- wraps the global `ParsedProgram` in another snapshot while still assembling
  it for every request;
- introduces stable keys but translates them immediately to revision-global ids
  before checking;
- makes diagnostics fast by discarding checked work and repeating it for
  verified preview;
- retains a mutable whole checker across revisions;
- keeps row-level checked/execution rescans behind a new artifact API;
- stores both rich domain graphs and normalized fact tables in production;
- adds a general differential-dataflow runtime before simple typed deltas are
  measured;
- calls shared `Arc` storage a phase boundary without opaque checked/semantic/
  verified/erased capabilities;
- changes the canonical artifact digest without a versioned migration and
  semantic parity gate;
- keeps any recursive ordinary-body lowerer beside its migrated plan-code
  module;
- splits crates before deleting the dependency edges the split is meant to
  remove;
- uses more compiler threads to hide duplicated work;
- claims the 16.7 ms/100 ms warm gates from a faster cold whole rebuild.

## Primary Architecture References

- [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html)
  keeps syntax trees per file, uses a body-insensitive item tree, and makes
  “typing inside one function never invalidates facts about another” an
  explicit invariant.
- [Salsa red/green evaluation](https://salsa-rs.github.io/salsa/reference/algorithm.html)
  records actual request dependencies and backdates unchanged results across
  revisions.
- [Swift request evaluation](https://www.swift.org/blog/swift-5.2-released/)
  replaced coarse eager declaration validation with immutable declarations and
  fine-grained lazy cached requests shared with code generation.
- [LLVM ThinLTO](https://blog.llvm.org/2016/06/thinlto-scalable-and-incremental-lto.html)
  performs a small summary-only thin link and redoes a backend only when its
  module, imports/exports, imported bodies, or relevant global result changes.
- [Go compiler export data](https://go.dev/src/cmd/compile/README)
  uses an indexed representation that can lazily decode only demanded parts of
  a larger object graph.
- [Roslyn overview](https://github.com/dotnet/roslyn/blob/main/docs/wiki/Roslyn-Overview.md)
  uses immutable per-document syntax snapshots with structural reuse.
- [Differential Dataflow arrangements](https://timelydataflow.github.io/differential-dataflow/chapter_5/chapter_5.html)
  motivate maintaining one shared indexed arrangement instead of rebuilding an
  equivalent index in every consumer.
- [DBSP incremental view maintenance](https://www.vldb.org/pvldb/vol16/p1601-budiu.pdf)
  gives a systematic model for turning snapshot computations into delta
  computations, including recursion, while making its space costs explicit.

These references justify architecture choices, not Boon performance claims.
Only current Boon harness reports and independent adversarial review can close
the plan.
