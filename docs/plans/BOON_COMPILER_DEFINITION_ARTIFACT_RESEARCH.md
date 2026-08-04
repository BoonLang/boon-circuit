# Boon Definition Artifact And Thin-Link Architecture Research

Date: 2026-08-03

Status: selected architecture direction after checkpoint `d177af9`; subordinate
to [`BOON_COMPILER_PERFORMANCE_PLAN.md`](BOON_COMPILER_PERFORMANCE_PLAN.md) and
the sequencing in
[`BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md`](BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md).
This document does not create weaker latency, memory, correctness, or evidence
exits.

## Decision

Replace the remaining phase-owned whole-program pipeline with a persistent
definition-artifact compiler and a thin linker.

The unit of retained compiler work is a stable source unit, interface SCC,
authored definition, definition specialization, domain summary, or link image.
It is not an entire parsed, checked, semantic, or `MachinePlan` phase product.
Cold and warm compilation invoke the same request evaluator; an empty database
is simply its cold initial state.

Checkpoint `d177af9` is a necessary bridge, not the finished database. Its
`SealedRequestGraphSnapshot` is the authoritative revision-zero proof and
currentness snapshot for the semantic rows that already exist. It must not be
mistaken for the complete compiler query graph:

- its nodes are predominantly proof projections rather than parse, interface,
  body, code, and link requests;
- its edges include runtime semantic and reactive cycles rather than only
  compiler evaluation dependencies;
- every memo starts with the same local input/result fingerprint, and
  production has not yet published later revisions through `RequestMemo`;
- `CompilerSession` still invalidates one whole checked result for any source
  edit and does not retain parser or checker results.

The selected architecture therefore keeps one database and one stable identity
registry, but gives each request two typed dependency planes:

1. **evaluation/currentness edges** determine what must execute or reverify for
   a new source revision; and
2. **proof/link relocations** encode executable, resource, reactive, storage,
   migration, and distributed relationships that may legitimately contain
   cycles.

This is not two semantic authorities. One definition artifact publishes both
spans from the same construction. Keeping the planes typed prevents a 296-node
runtime/proof SCC from becoming one indivisible compiler invalidation unit.

## Current-Tree Evidence

The current directional debug evidence after `d177af9` is:

| Owner | Current evidence | Architectural consequence |
| --- | ---: | --- |
| complete verified request | 4,112.475 ms, 260,660 KiB RSS | no local micro-optimization can close the gap |
| parse | 96.450 ms | unit parsing is already plausible; global assembly is the wrong retained owner |
| typecheck | 680.846 ms | the checker must publish reusable definition results instead of being consumed wholesale |
| semantic construction | 2,289.956 ms | repeated whole-program semantic representations remain the largest multiplier |
| backend | 807.949 ms | ordinary bodies are lowered by multiple independent owners |
| plan validation / pretty serialization | 104.371 / 534.427 ms | normal in-memory preview must not pay debug artifact costs |
| retained proof graph | 8,315 nodes / 29,131 edges | useful proof snapshot, but not yet a language request database |

The typechecker trace further isolates a high-leverage ownership cut. Parser,
checker construction/inference, and diagnostic projection finish before
`assemble_report`; `assemble_report` then calls `checked_image_handoff`, which
rescans and canonical-serializes the already-built checked scopes,
declarations, statements, expressions, callables, calls, resources, and
metadata. That post-hoc handoff costs about 392 ms in the current directional
sample. The replacement checker must emit the final checked definition receipts
while it owns the data; speeding up the scanner would preserve the wrong owner.

The rest of the trace shows the same multiplier at later boundaries:

- `SemanticProgram` retains the sealed image plus OUT, resource, reactive,
  lowering, view, storage, memory, canonical-core, Manifest, and request-graph
  products, although verified lowering ultimately consumes a narrow canonical
  core and digests;
- ordinary callable bodies are rebound and recursively lowered by document,
  row/scalar, and migration backends;
- `refresh_typed_list_view_fingerprints` clones and rewrites a complete
  `MachinePlan` before validation;
- pretty JSON is hashed and serialized on the ordinary compilation path;
- distributed convergence elaborates every role again, including a complete
  confirmation pass;
- the native compile worker is persistent, but cancellation can only occur
  around large whole phases rather than inside bounded request worklists.

These are representation and lifetime multipliers. They justify deleting
owners before further map, hash, allocation, or SCC-kernel tuning.

## Target Pipeline

```text
ProjectSnapshot
  -> UnitSyntaxSnapshot[SourceUnitId]
  -> UnitItemIndex[SourceUnitId]                 (body-insensitive)
  -> InterfaceSccResult[InterfaceSccKey]
  -> CheckedDefinitionShard[StableDefinitionKey]
  -> DefinitionExecutableArtifact[DefinitionVariantKey]
       { checked body, diagnostics, semantic rows, plan-code module,
         source map, public/body fingerprints,
         evaluation deps, proof relocations }
  -> DomainArtifact[AuthorityKey]
  -> ThinLink[demand roots + summaries + relocations + explicit SCCs]
  -> ContractVerifiedLinkedImage
  -> consuming RunnableImageBuilder
  -> SealedRunnableMachine
```

Complete diagnostics request all definition shards and aggregate their ordered
immutable diagnostics. A verified preview requests only definitions and domain
artifacts reachable from verified roots. Both routes reuse the same interfaces
and checked definition shards; there is no diagnostics-only checker and no
separate warm compiler.

The normal in-memory result is `SealedRunnableMachine`. Rich semantic graphs,
pretty JSON, exhaustive proof materializations, and human-readable debug views
are explicit requests. They are not retained by the runtime artifact and do not
run merely because the playground needs a replacement preview.

## Stable Identity And Fingerprints

The parser must own structural identities before the checker database becomes
persistent:

```text
StableDefinitionKey = SourceUnitId + parser-owned item route
StableOccurrenceKey = StableDefinitionKey + parser-owned structural body route
DefinitionVariantKey = StableDefinitionKey + execution-domain/layout/control/
                       capability specialization key
```

Cross-revision compiler keys must not contain global dense IDs, byte offsets,
line numbers, revision-local arena IDs, or exact authored source substrings.
The current authored-call identity includes a source substring, so formatting
can unnecessarily rekey a call site; parser-owned structural routes replace
that dependency.

Each definition has separate fingerprints:

- **item-tree fingerprint**: names and body-independent structure;
- **public interface fingerprint**: externally visible type, effect,
  persistence, and capability contract;
- **implementation fingerprint**: checked body and local semantic result;
- **definition-code fingerprint**: linked executable module before invocation
  frames;
- **proof fingerprint**: typed proof rows and relocations;
- **source-map fingerprint**: diagnostics and migration provenance only.

A body edit that backdates the public interface leaves unrelated definitions
green. Persistence semantic identity remains an explicit language/runtime
contract and is never inferred from compiler syntax identity.

## Persistent Request Database

`boon_compilation_db` should own request-slot registration, revisions,
evaluation dependencies, reverse cones, memo currentness, cancellation tokens,
and work accounting. Language crates own typed keys and results. The database
does not become a dynamically typed `Any` cache or a facade around the current
whole phases.

Required request states are:

```text
Vacant -> Computing(generation) -> Published(result, fingerprints,
                                           changed_at, verified_at)
                              \-> Failed(diagnostics, dependencies)
```

Publication is atomic and generation-checked. An older request may finish, but
cannot publish after a newer source revision. Re-execution that produces the
same result fingerprint backdates `changed_at`; dependents can be marked green
without executing. Results may be retained, evicted while keeping dependency
metadata, or explicitly materialized for diagnostics/debug use.

The first evaluator stays deterministic and single-threaded. At most two
workers may be enabled only after request cones prove independence. Every long
parser, checker, linker, proof, and backend worklist contains bounded
cancellation checkpoints; a new editor revision must not wait for a complete
multi-second phase to notice supersession.

## Definition Artifact

`CheckedProgramDatabase` already owns dense indexes, reverse dependencies,
dirty queues, inference caches, and worklists, but its 37 kLOC implementation is
constructed fresh and consumed into a whole `CheckedProgram`. Retaining that
mutable object would make error recovery, revision isolation, and exact
invalidation fragile. Extract these immutable products instead:

1. `UnitItemIndex`: names, definitions, imports, stable item routes, and body
   boundaries; it remains stable across body-only edits.
2. `ProjectInterfaceIndex`: body-insensitive symbol lookup plus explicit
   interface SCCs for mutually dependent signatures.
3. `CheckedDefinitionShard`: checked body, ordered local diagnostics, direct
   dependencies, public/body fingerprints, and source-map routes.
4. `DefinitionExecutableArtifact`: the demanded definition's semantic rows,
   normalized plan-code module, typed relocation span, and proof receipt.

The checker builder may remain mutable inside one request, but it publishes an
immutable result and discards scratch state. It emits checked receipts directly
during construction. When that parity gate passes,
`checked_image_handoff` and its complete post-hoc checked-image scan are deleted
from production.

The executable artifact spans the current semantic/backend phase boundary. An
ordinary definition is lowered once for a verified variant. Each occurrence is
a compact resolved invocation frame containing argument, substitution, PASSED,
OUT, owner/resource/effect/render, and control bindings. Migrated definitions
must delete the corresponding recursive body traversal from all three old
backend lowerers in the same vertical tranche.

## Thin Link And Domain Artifacts

The linker consumes compact definition/domain summaries and typed relocations,
not rich whole-program graphs. Its responsibilities are bounded:

- demand closure from complete verified intent roots;
- interface and explicit semantic SCC closure;
- producer/external-event/distributed authority closure;
- relocation resolution and dense final-ID assignment;
- proof root/coverage commitment;
- bundle and runnable receipt publication.

Resource, reactive, lowering, storage, view, memory, migration, and distributed
facts become construction-owned `DomainArtifact`s keyed by their stable
authority. A domain artifact may contain packed tables and CSR spans; it does
not own a parallel semantic AST. Rich domain graphs remain independent
test/debug materializers until parity passes, then are deleted from production.

This follows the useful part of ThinLTO's architecture: definition modules
publish compact summaries, a thin link computes whole-program decisions from
the summaries, and only demanded modules are materialized. It does not import
ThinLTO's unbounded default parallelism; Boon keeps the two-job machine limit.

Distributed compilation uses the same rule. Each role publishes a stable
summary and delta. A changed role advances the fixed point only through affected
relocations and authorities. Re-elaborating Client, Session, and Server in
every round and once more for confirmation is deleted when the delta/full
parity oracle passes.

## Runnable Publication And Serialization

One consuming `RunnableImageBuilder` owns final dense IDs, plan tables,
fingerprints, executor indexes, and validation. It consumes the linked image so
the compiler cannot accidentally retain a second full executable authority.
Trusted consumers receive one `SealedRunnableMachine` whose executor metadata
is built once.

The canonical persisted artifact should be sectioned and indexed so unchanged
definition/domain sections can be reused and consumers can validate/decode only
required sections. Pretty JSON remains a deliberate debug/export request.
Normal playground compilation neither serializes nor hashes pretty JSON. An
untrusted persisted artifact verifies section receipts and builds runtime
indexes exactly once.

## Crate Boundaries

Crate extraction follows the model, not the current large files. Candidate
one-way seams are:

1. syntax/item identity model below all semantic crates;
2. checked interface/definition model below the checker builder;
3. definition/domain artifact model below semantic construction and proof;
4. thin-link model and linker;
5. runnable model below its consuming builder and executor;
6. compiler service below CLI/native/web adapters;
7. migration/debug tooling outside runtime cores.

A split is accepted only when it removes a dependency edge, reduces a measured
Rust rebuild cone, or enables immediate owner deletion. Compatibility re-exports
or moving the present mutually dependent phase implementations into more crates
are rejected. Rust rebuild speed and Boon compilation latency remain separate
measurements.

## Implementation Sequence

The first part of step 1 is implemented: parser units own body-insensitive item
indexes and stable definition routes, while `CompilerSession` retains them by
`SourceUnitId`, invalidates only changed units, and applies unit topology
changes atomically. Producer format V4 and the warm verifier now expose and
enforce exact attempted/parsed/reused unit counts. Canonical assembly still
rebuilds the global syntax product, stable occurrence routes are not yet
published, and checking remains whole-project; those are the next open parts of
steps 1--3 rather than hidden completion claims.

1. **Stable syntax and item ownership.** Retain `Arc<UnitSyntaxSnapshot>` and a
   body-insensitive `UnitItemIndex` in `CompilerSession`; add structural item
   and occurrence routes plus atomic unit upsert/remove/rename. Prove unchanged
   units are not reparsed.
2. **Real typed request currentness.** Add session-owned request slots and
   evaluation-edge spans; separate evaluation edges from proof relocations;
   implement exact reverse cones, backdating, generation-checked publication,
   work counters, and bounded cancellation.
3. **Interface and definition checking.** Extract interface SCC results and
   immutable checked definition shards. Emit checked receipts during checking
   and delete production `checked_image_handoff`. Prove body-only edits with an
   unchanged interface cause zero unrelated checks.
4. **First end-to-end definition artifact.** Carry one ordinary definition from
   checked shard through semantic rows and plan code. Replace its old OUT/
   contextual and document/row/migration body traversals in the same tranche;
   keep exact behavior/proof parity.
5. **Demand and domain migration.** Make verified intent the sole demand queue.
   Migrate remaining definitions and each semantic domain to construction-owned
   artifacts; delete rich retained graph owners and Manifest inventories as
   their independent materializers pass.
6. **Thin link and proof seal.** Link summaries/relocations once, preserve
   explicit semantic SCCs, publish the contract-verified linked image, and
   remove duplicate canonical-core mapping and hashing.
7. **Consuming runnable image.** Build dense plan/runtime indexes once; remove
   full-plan clone/rewrite and ordinary pretty serialization from the in-memory
   path.
8. **Distributed delta link.** Publish role summaries/deltas and delete full
   role re-elaboration plus confirmation rebuild.
9. **Measured crate extraction and bounded parallelism.** Split only at the
   proven one-way seams, then consider at most two independent workers.
10. **Gate closure.** Regenerate cold, warm, cancellation, invalidation,
    scaling, determinism, RSS, migration, and native reports, followed by the
    performance plan's independent adversarial reviews.

Each tranche is flag-day: new and old owners do not remain as production
fallbacks. A temporary independent old path may exist under tests solely as a
parity oracle and is removed or test-gated when the tranche passes.

## Required Evidence Per Tranche

Record at least:

- executed, reused, backdated, cancelled, and superseded requests;
- parsed/reused units and changed/backdated interfaces;
- checked, demanded, pruned, and linked definitions/variants;
- invocation frames, domain rows, relocations, proof rows, and SCC sizes;
- full-program materializations and rich/debug materializations;
- maximum simultaneously live artifact bytes and process peak RSS;
- source-edit-to-cancellation-observation latency;
- cold and warm phase wall times with non-overlapping spans;
- exact diagnostic, verified-artifact, migration, activation, and runtime
  behavior parity.

A warm constant/body edit must report zero unrelated unit parses, interface
changes, definition checks, semantic artifacts, proof components, plan-code
modules, and runnable sections. A cold empty database must execute the same
requests and still pass the cold budgets without relying on retained state.

## Rejection Rules

Reject a tranche that:

- adds a cache or query API around whole reparse/recheck/re-elaboration;
- uses proof/reactive cycles directly as compiler evaluation SCCs;
- derives stable identity from raw source text, offsets, lines, or dense IDs;
- retains the mutable whole checker as the cross-revision authority;
- emits definition rows and then rescans them into another production image;
- keeps old recursive ordinary-body lowering beside a new definition module;
- hides complete diagnostics behind demand pruning;
- serializes pretty JSON on every in-memory preview request;
- calls a seal wrapper around a completed `MachinePlan` a consuming builder;
- re-elaborates all distributed roles to confirm an unchanged link;
- enables broad parallelism before independent request cones and cancellation
  are proved;
- splits crates without a measured dependency or rebuild-cone improvement;
- claims the 16.7 ms diagnostics or 100 ms preview gates from a faster cold
  rebuild rather than measured warm affected work.

## Primary Architecture References

The selected design is grounded in these primary or project-authored
architecture descriptions:

- [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html):
  per-file syntax values, body-insensitive item data, on-demand derived state,
  and the invariant that typing one function body does not invalidate facts
  about another;
- [rustc incremental query evaluation](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html):
  stable fingerprints and red/green reuse;
- [Salsa's red/green algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html):
  revisions, dependency verification, and result backdating;
- [Swift's request evaluator](https://www.swift.org/blog/swift-5.2-released/):
  self-contained fine-grained requests, immutable declarations, lazy
  evaluation, caching, and dependency tracking;
- [TypeScript builder programs](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API):
  affected-file diagnostic and emit reuse;
- [ThinLTO](https://clang.llvm.org/docs/ThinLTO.html): compact per-module
  summaries and a scalable thin link instead of monolithic whole-program
  merging;
- [Go compiler overview](https://go.dev/src/cmd/compile/README): indexed unified
  export data with lazy partial decoding;
- [Zig 0.16 incremental compilation](https://ziglang.org/download/0.16.0/release-notes.html):
  avoiding over-analysis and keeping the evaluation dependency graph acyclic
  except explicit dependency loops.

These references support the architectural invariants; Boon's latency and
correctness claims still require its own current reports. Jai remains an
outcome comparison only because its implementation is not public enough to be
an engineering contract.
