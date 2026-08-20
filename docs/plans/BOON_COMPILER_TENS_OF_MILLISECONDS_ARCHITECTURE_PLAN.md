# Boon Compiler Tens-of-Milliseconds Architecture and Linux Hyperoptimization Plan

Date: 2026-08-21

Status: proposed implementation contract and evidence refresh. This plan is
subordinate to
[`BOON_COMPILER_PERFORMANCE_PLAN.md`](BOON_COMPILER_PERFORMANCE_PLAN.md) for
language correctness, cold/no-cache acceptance, determinism, memory reporting,
and final verification meaning. It refines the implementation order in
[`BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md`](BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md)
and
[`BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md`](BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md)
for a substantially harder latency target. It does not weaken an existing
gate. Where the older architecture documents describe the same owner, this
document supplies the newer evidence, packed representation, deletion order,
and Linux-native producer policy.

Execution prompt:
[`BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md`](BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md).

## Decision

Boon will optimize its native compiler, IDE, and playground for current Linux
machines first. Portable native and Wasm producers remain future work; they do
not constrain the first high-performance Linux representation.

The Linux producer will use **Microsoft mimalloc 3.5.0** if the exact integrated
version preserves correctness and the measured speed win survives an
uninstrumented product benchmark and a long-lived-session soak. The current
speed-first decision accepts mimalloc's measured RSS increase. RSS remains
reported and bounded, but a generic five-percent allocator-RSS rejection rule
does not override this explicit product decision.

This is a bridge, not the architecture. The measured allocator gain is about
23% for the current allocation-heavy verified build and about 28% for current
diagnostics. It cannot turn the present representation into a tens-of-
milliseconds compiler. The working hypothesis is that allocator choice will
account for less than two percent after the packed architecture removes per-row
and solver-loop allocation. This is a target to remeasure after M1--M5, not an
assumed property: large columns, scratch buffers, revision retirement, and
lookup tables can keep allocator policy material.

Rust nightly is not selected as the default producer merely because a previous
nightly built the Rust workspace about 11% faster. That number measured the
time to build the compiler, not the time for the resulting compiler to compile
Boon. The measured same-source Boon runtime change was only about 1--3%, with
diagnostics effectively flat. Stable ThinLTO, `target-cpu=native`, and
instrumentation PGO already provide the most relevant code-generation tools.
Nightly remains allowed for a named A/B experiment and becomes the default only
if the final candidate binary wins a representative holdout by at least five
percent without correctness or operational regressions.

The main work is a flag-day end-to-end representation cut:

```text
source byte snapshots + stable source spans
  -> packed unit syntax + module/name overlay
  -> persistent revision database + packed type/definition code
  -> compact invocation frames + normalized facts/tagged edge planes
  -> consuming verified executable-image linker and sealer
  -> runtime-owned sealed image

explicit, untimed-or-separately-timed views only:
  rich checked/editor DTOs | exhaustive proof report | legacy plan export
  pretty JSON | human-readable route and type expansion
```

The production path must stop translating a dense kernel back into recursive
strings and types, expanding those DTOs into several semantic graphs, and then
repacking the result for the runtime. One compact fact must have one owner and
one representation from checking through execution.

## What “Tens of Milliseconds” Means

A single number would hide four materially different products. They must be
measured and reported separately:

| Product | End point | Final target on the reference Linux machine |
| --- | --- | ---: |
| warm editor diagnostics | changed text acknowledged through complete diagnostics for the new revision | 5--15 ms p50, no more than 16.7 ms p95 for an ordinary local edit |
| warm verified preview | same edit through a new in-memory verified executable image or exact image patch | 15--40 ms p50, no more than 50 ms p95 for an unchanged-public-interface edit |
| cold in-process diagnostics | empty compiler database, resident source bytes, complete project diagnostics | 20--45 ms p50, 30--70 ms p95 |
| cold in-process verified image | empty compiler database, resident source bytes, runnable verified image | 40--85 ms p50, 60--120 ms p95, with sub-100 ms p95 as the stretch exit |

Process startup, cold filesystem I/O, and explicit export serialization are
separate budgets. They must not be silently excluded from a CLI number, but
they also must not make an editor preview serialize pretty JSON it does not
consume. A genuinely cold process report includes all three components. A
resident IDE report includes neither process startup nor unrelated export.

For the current roughly 866 KiB NovyWave project, a 20 ms full cold verified
compile is not a credible immediate promise without a generated snapshot or
cache. A 50--100 ms in-process result is a credible architectural hypothesis
only after the rich compatibility pipeline is deleted. The existing 250 ms
diagnostics and one-second verified gates remain mandatory intermediate gates;
they are no longer the desired stopping point.

## Fresh Evidence

The following samples use checkout `dfdb20c14209a5a3cee53b31e99e8dcdedb580be`.
They are directional medians from three fresh invocations of the existing
release producer unless stated otherwise. Exact acceptance still requires the
full p50/p95 protocol. The canonical plan digest remained
`e2d673e3116f03622731f695cf2fb598d3313077845013f776930d38cc2acbea`.

### Cold End-to-End Results

| Mode | System allocator | exact mimalloc 3.5 preload | Change |
| --- | ---: | ---: | ---: |
| verified wall time | 2,664.015 ms | 2,071.005 ms | -22.3% |
| verified peak RSS | 357,784 KiB | 417,152 KiB | +16.6% |
| diagnostics wall time | 707.350 ms | 510.200 ms | -27.9% |
| diagnostics peak RSS | about 207,068 KiB | about 231,436 KiB | about +11.8% |
| verified allocations | 13,956,187 / 1,674,810,172 bytes | same compiler work counters | unchanged |
| diagnostics allocations | 5,781,266 / 584,854,099 bytes | same compiler work counters | unchanged |

The same Rust allocation/work counters accompanied lower wall time and higher
RSS. The A/B does not isolate allocator call overhead from placement, cache
locality, page commitment, huge-page behavior, or page-fault effects. It is
strong evidence for using mimalloc now and equally strong evidence that
mimalloc does not remove the algorithm's 14 million allocation events or its
work amplification.

The system-allocator verified sample divides approximately as follows:

| Current phase | Median time | Interpretation |
| --- | ---: | --- |
| parse and project syntax | 72.50 ms | already too large for the final cold budget, but not the primary owner |
| typecheck and checked publication | 1,154.13 ms | 43% of wall time; the dense solver is a minority of this span |
| semantic elaboration and proof construction | 1,016.15 ms | 38%; repeated representations and occurrence expansion dominate |
| WHERE verification | 0.392 ms | not a present performance problem |
| IR lower plus validation | 59.06 ms | mostly another compatibility boundary |
| backend | 252.39 ms | plan specialization, reconstruction, cloning, and sealing |
| plan validation | 77.25 ms | repeated trusted-output traversal |
| explicit pretty serialization and hash | 69.23 ms | measured outside elapsed; not needed for an in-memory preview |

With mimalloc 3.5, typecheck/checked publication remains about 845 ms,
semantics about 844 ms, backend about 215 ms, plan validation about 68 ms,
and explicit serialization about 68 ms. Faster allocation does not change the
phase ranking.

### Warm Session Failure

A one-line TodoMVC edit in an existing mimalloc-backed session reported:

| Step | Wall time | Allocations |
| --- | ---: | ---: |
| update acknowledgement | 0.041 ms | negligible |
| complete diagnostics | 1,034.574 ms | 5,979,817 / 539,969,052 bytes |
| verified preview after diagnostics | 1,398.010 ms | 7,951,442 / 905,900,788 bytes |
| edit through preview | 2,432.627 ms | 13,931,259 combined calls |

Unit parser caches are working: parse took about 5.76 ms for diagnostics and
0.98 ms for preview. The checker nevertheless visited all roughly 4,080 checked
rows twice. `CompilerSession::apply_updates` clears both diagnostics and the
checked result; diagnostics then retains only a presentation result. The
following verified request has no promotable checked revision and checks the
project again. There is no exact dirty definition/SCC cone and no in-flight
supersession.

The intent boundary is also inconsistent outside the session: the convenience
`check_diagnostics_source` path calls the full checked-source route, while only
`CompilerSession::request(Diagnostics)` reaches the lean diagnostic projection.
Diagnostics demand must select the compact product at the public compiler
boundary, not after an expensive rich result has already been built.

This is the most direct route from seconds to tens of milliseconds for an IDE:
retain compact checked facts for the revision, promote diagnostics into
verified work, and recompute only the dirty owner cone. It is not acceptable as
a way to claim the cold target; the same database must execute revision zero
without cache reuse for cold scoring.

### Allocation and Live-Memory Evidence

NovyWave has about 17,721 parsed expressions, yet verified compilation performs
about 788 allocation calls per expression. Sampled allocation leaves include:

| Allocation source | Sampled calls | Share of all calls |
| --- | ---: | ---: |
| `String::clone` | 4.839 million | 34.4% |
| `Vec<String>::clone` | 0.901 million | 6.5% |
| generic `RawVec` growth | 2.188 million | 15.7% |
| rich object-type construction | 0.257 million | 1.8% |
| `Box<str>` cloning | 0.236 million | 1.7% |

These five leaves explain about 60% of allocation calls. At the sampled peak,
about 1.317 million live `String` clones and 229,000 live `Vec<String>` clones
accounted for roughly 70% of live object count. About 350 MiB of requested
objects were genuinely live near the peak, close to measured RSS. The problem
is therefore not mainly system-allocator fragmentation. The compiler retains
several valid but redundant rich images at once.

The CLI measurement binary itself wraps the process allocator and performs two
thread-local counter updates for each allocation or deallocation event. The
verified request causes roughly 55.8 million such counter updates. Allocations
and bytes remain required evidence, but the product binary and timed product
lane must not carry this instrumentation. An instrumented evidence binary must
report the same semantic digest and work counters separately.

## The Real Bottlenecks

### 1. The Greenfield Kernel Still Exits Through a Rich Compatibility Pipeline

The greenfield direction remains correct, but the production cutover did not
finish the intended representation cut. `kernel_oracle.rs` is still a roughly
20,991-line production adapter. It retains prepared DTOs and final kernel input,
uses tree maps/sets while pruning dependencies, clones compact owners and
definition-fact graphs, and still invokes the old `boon_typecheck` SOURCE ABI
path. `compiler_checked_from_kernel` then constructs the old rich checked
image.

The dense solver itself is no longer the primary checked cost. A detailed
mimalloc trace measured about 198 ms for component solving, but about 763 ms
for the enclosing production checked projection. Within that path, checked-
image construction was about 527 ms, row materialization about 94 ms, source
ABI projection about 9 ms, and additional projection/layout/seal work made up
the remainder. These spans are nested evidence and must not be added to the
end-to-end phase a second time.

`DefinitionArtifact` still carries rich recursive `FlowType` values and many
boxed row families beside its type-term sidecar. Receipt generation recursively
alpha-normalizes those rich values, cloning field names and order. Linking
reconstructs them again for relocation. Materialization borrows the snapshot,
so compact input, artifact, and rich checked output overlap in memory.

Required change: the packed type and definition tables become the production
API. Rich checked structures become an explicit editor/oracle/export view, not
the input to the next compiler phase.

### 2. Semantic Elaboration Multiplies Occurrences and Authorities

The semantic path constructs OUT, contextual materializations, execution,
resource, reactive, lowering, storage, view, memory, canonical core, and the
dependency manifest in sequence. Checked, OUT, and execution fields in the
final `SemanticProgram` are now test-only, so production does not retain all of
them indefinitely. They still overlap as live construction locals while later
domain graphs are built, and the production value retains several independent
domain graphs until erasure reduces the result to canonical data and digests.

The measured project contains 5,061 local substitutions in an existing parent-
linked OUT frame model, but consumers perform 1,957,896 cumulative ancestry
substitution visits for 3,494 call instances, with one instance reaching 833
substitutions. Consumers still reconstruct local `BTreeMap<TypeVar, Type>`
values, and recursive types, provenance, and paths are repeatedly walked or
represented by owned vectors and strings. The final dependency graph has only
7,974 nodes and 28,484 edges, while its collectors walk tens of thousands of
checked, execution, and construction rows and preallocate from an arbitrary
four-times estimate.

Required change: keep the good parent-frame ownership already present in OUT,
change its values to compact type/symbol/path references, and carry that model
directly through normalized definition modules and the linker. Downstream
consumers must not reconstruct cumulative rich maps. One typed edge table
serves scheduling, currentness, proof coverage, and verification.

### 3. Canonicalization and Proof Are Retrospective Reconstruction

Receipt construction rebuilds rich type trees solely to alpha-normalize and
hash them. Semantic publication then scans expression, call, execution, and
resource rows again. Plan verification eventually serializes the whole plan
into a new byte vector for its SHA. These operations preserve important
semantics, but their current representation makes verification proportional to
the number of expanded DTO occurrences rather than canonical definitions and
edges.

WHERE itself is currently sub-millisecond. Removing or weakening WHERE would
not matter and is forbidden. Required change: canonical IDs and local type-
variable ordinals make alpha-normalization an integer remap. Sections hash and
validate rows while they are appended. Definition and section receipts combine
into the program root. An independent dense verifier performs one final ID,
edge, and obligation audit; the exhaustive rich verifier remains for untrusted
or deserialized legacy input and differential testing.

### 4. The Backend Re-specializes Instead of Linking Shared Definition Code

The traced backend handled about 31,872 expressions, 304 functions, 1,444
templates, and 31,449 expression-cache entries. It retained only about 280
shared functions while generating about 1,150 inlined one-off variants and
6,460 cache scopes. It later clones the complete `MachinePlan` to refresh
fingerprints, rebuilds row expressions, compacts reachability, validates, and
hashes again.

Required change: every compatible definition variant owns one shared target-
neutral plan-code module. Its key includes the definition, execution domain,
resolved layout, overlay/control shape, and capability contract; semantically
required variants remain distinct. Occurrences within a compatible variant own
compact invocation frames rather than duplicate bodies. A thin target linker
resolves relocations into final dense IDs. A consuming
builder assigns IDs, seals local sections, records fingerprints, and transfers
the completed columns to the runtime without a whole-plan clone.

### 5. Parser and Source Models Still Own Compiler-Unnecessary Richness

The parser copies source, token lexemes, declared-function names, stable routes,
and nested string paths. A source unit retains source, tokens, lines, items,
statements, and expressions. This is not yet the largest wall-time owner, but it
sets an allocation and identity tax for every later phase.

Required change: compile mode retains `Arc<[u8]>`, tokens as kind/start/end,
packed expression and statement headers, flat operand spans, definition
headers, and construction-time parent/item indexes. Project linking publishes a
small module/name-resolution overlay and never rewrites or repacks the AST.
Line tables, lexeme strings, rich routes, and editor navigation data are lazy
sidecars demanded by the editor, not mandatory compiler input.

### 6. Global Allocation Is a Symptom and an Avoidable Tax

Mimalloc proves that servicing millions of tiny allocations is expensive. It
does not explain why the allocations exist. `Vec<BTreeSet<_>>` dependency
staging, one `Box` for operation outputs, one `Arc` per operation, boxed string
interner entries, collision vectors, recursive type clones, and owned path
vectors turn compact language facts into pointer graphs.

TigerBeetle's useful lesson is not that every variable-size program can use a
literal compile-time fixed array. TigerBeetle computes worst-case bounds at
startup, owns its service memory, batches work, and performs no allocation
after startup. Boon should apply the same ownership discipline at compiler
phase boundaries: perform one `CompileShape` sizing pass or conservative
capacity plan, reserve definition/revision slabs once, append packed rows, and
bulk-reset scratch. Persistent definition slabs are required for incremental
reuse; one monolithic bump arena for the whole project would make every edit
invalidate everything.

## Required Packed Authorities

The following are one-way production types, not adapters layered under the old
DTOs.

### `SymbolTable`

- `SymbolId(u32)` indexes one byte slab plus offset/length rows.
- Authored identifiers refer to source spans until a stable symbol is needed.
- Synthetic and cross-unit names are interned once.
- Hot compiler rows contain no `String`, `Box<str>`, or `Arc<str>`.

### `PathTable`

- `PathId(u32)` identifies an interned `(parent_path, SymbolId)` row or a span
  of `SymbolId` values.
- Each path stores its stable digest once.
- No hot row owns `Vec<String>` and no phase reconstructs a string path for a
  lookup.

### `TypeTable`

```text
TypeHeader { tag, flags, child_start, child_len, payload }
ObjectField { name: SymbolId, ty: TypeId }
ObjectShapeRow { fields: Span32, semantic_field_order: Span32 }
FlowRef { mode, ty: TypeId }
```

- Headers, children, object fields, and variant payloads are flat columns.
- Union-find variables store `TypeId`; no recursive cloned `Type` is stored in
  a solver cell.
- A cross-definition `TypeRef` is `{module_or_definition, term}`. A raw local
  variable ordinal never becomes a global type identity.
- Object lookup may use canonical lexical field order, but `TypeId` equality,
  hashing, and receipts also include the explicit semantic/authored field-order
  span required by the current language contract. It is not a presentation-
  only sidecar.
- Definition-local canonical variable ordinals make receipt normalization a
  compact ID mapping.
- Cross-definition substitutions store IDs/spans, not reconstructed types.
- Interning uses an exact key/open-addressed table or an indexed collision
  chain, not a hash-to-`Vec` bucket of boxed terms.
- Freezing/import uses one reusable generation-stamped
  `source TypeTermId -> frozen TypeId` remap array. It neither clears a huge
  dense array on each definition nor allocates a new map for each import.

The existing `ArtifactTypeModuleV1` is an oracle/transition DTO, not this final
table: it still owns string names and boxed child arrays per term. The earlier
dense importer was rejected because it cleared roughly 85 million slots. A
generation stamp supplies O(touched terms) reset without returning to per-
definition hash/tree maps.

### `DenseSyntaxProject`

- owns immutable source bytes and unit-local packed syntax columns;
- retains revision-local dense unit/definition/node coordinates plus stable
  source-lineage, external-definition, and occurrence identities;
- receives an immutable project-link overlay for resolved names and routes;
- does not build compiler-unnecessary editor DTOs in compile mode.

All dense IDs are revision-local coordinates, never stable external identity.
Fingerprints hash canonical section bytes and stable external definition IDs,
not insertion-order IDs. Hash tables are lookup structures only; source order,
dense construction order, or an explicit deterministic sort owns publication.
Parser route-collision evidence remains available as a lazy source-map sidecar.

### `DefinitionCode`

- points into global checked-op, formal/result, call, effect, state, list,
  source-span, dependency, and diagnostic columns;
- stores compact `Span32 { start, len }` column ranges rather than a separate
  boxed slice for each row family;
- is emitted directly by the checker with its public/private fingerprints and
  local receipt;
- can lazily project a rich `DefinitionArtifact` only for the test oracle or an
  explicit editor/export request.

The present “compact” kernel DTO is not this representation: owner inputs,
edge-role names, statement payloads, record shapes, type-term children, and 13
definition-fact families still use per-row or per-family boxes and owned
strings. M1 must flatten those payloads into global columns; merely selecting
the existing type-term sidecar as the new API would preserve the allocation
problem.

### `InvocationFrame`

```text
InvocationFrame {
  parent: FrameId,
  definition: DefinitionId,
  occurrence: StableOccurrenceId,
  callsite: SourceSpanId,
  actuals: ValueRefSpan,
  local_substitutions: SubstitutionSpan,
  result: ValueRef,
}
```

- `occurrence` is structural identity across revisions; `callsite` is only the
  revision-local presentation span and may shift after an unrelated edit;
- compatible shared definition code is never copied into an occurrence;
- lookup follows frame/definition IDs rather than cloning a cumulative map;
- provenance is an interned `OriginId`/`ProjectionId`, not a path vector.

### `CompilationDb` and `SealedExecutableImage`

- every request key uses stable external definition/occurrence identities for
  `{definition, projection}` or `{definition, invocation-overlay}`;
- each memo stores input/result fingerprints, `changed_at`, `verified_at`, an
  exact dependency span, and work counters;
- one physical tagged edge-storage authority retains distinct
  evaluation/currentness and proof/link-relocation planes with separate CSR
  views; the relations and their cycle rules are never collapsed;
- definition and SCC results are backdated when their output fingerprint is
  unchanged;
- diagnostics retains compact solved facts, so a verified request promotes the
  same revision rather than checking again;
- typed section builders validate and hash while appending, then freeze;
- the final builder consumes scratch/semantic columns and publishes one opaque
  sealed image to the runtime;
- rich `CheckedProgram`, `SemanticProgram`, `ErasedProgram`, and legacy
  `MachinePlan` views are explicit compatibility/oracle exports and are absent
  from the scored native preview path.

## Incremental and Parallel Compilation

Exact incrementality is essential for the warm IDE goal, not a substitute for
the cold architecture:

1. Reparse only the changed unit and preserve unchanged stable syntax lineage,
   definition identity, and occurrence identity; dense coordinates may be
   reassigned within the new revision.
2. Recompute the dirty definition or explicit recursive SCC.
3. If its public fingerprint is unchanged, backdate downstream consumers.
4. Rebuild only demanded semantic/plan projections whose exact dependency
   fingerprints changed.
5. Reuse the diagnostic revision when the preview demand arrives.
6. Check cancellation at unit, owner, and SCC boundaries. A newer edit must
   supersede old unpublished work.
7. Record and assert the exact dirty cone, reused owners, recomputed rows, and
   allocated bytes in every warm benchmark.

The revision-zero database uses exactly the same code without previous memo
state. That preserves the cold/no-cache contract and prevents a separate slow
cold compiler from surviving.

Parallelism comes after packed owner/SCC boundaries. Cold independent SCCs and
target-link sections may use a bounded worker pool when hardware and workload
warrant it. A normal warm edit should touch such a small cone that queueing and
synchronization can make it slower. Background compilation, debounce, and
stale-result cancellation improve perceived behavior but never count as lower
compiler latency. No extra thread may hide duplicate work.

## Linux Allocator and Toolchain Policy

### Exact mimalloc 3.5 Integration

Microsoft's current C release is
[`v3.5.0`](https://github.com/microsoft/mimalloc/releases/tag/v3.5.0), which
upstream describes as the recommended v3 line; v2.5 is the conservative stable
line. The current Rust [`mimalloc` wrapper](https://github.com/purpleprotocol/mimalloc_rust)
is version 0.1.52, but its sys crate still vendors Microsoft mimalloc 3.3.2.
Enabling the wrapper's default “v3” feature is therefore **not** an exact 3.5
integration.

The implementation sequence is:

1. Keep the already-measured out-of-tree official 3.5 `LD_PRELOAD` result as
   evidence because it tests the same Boon binary without a Rust rebuild.
2. Pin an audited exact upstream 3.5 source/tag in the Linux native producer or
   wait for and pin a wrapper/sys release that demonstrably vendors 3.5.
3. Use one allocator override only. Never combine a statically linked
   overriding allocator with a mimalloc preload.
4. Keep allocation/deallocation within the same allocator domain across FFI.
5. Confirm allocator identity at startup in evidence builds. Record the
   library SHA-256, upstream tag and commit, release/debug/secure mode, relevant
   CMake options, absolute preload path when used, every `MIMALLOC_*`
   environment value, and active transparent-huge-page policy.
6. Run compiler tests, invalid-input and panic paths, long-lived incremental
   sessions, cancellation, concurrent playground activity, and leak/RSS soak
   before enabling it by default.
7. Use release defaults initially. Do not confound the first integration with
   secure mode, guarded allocation, huge pages, purge-delay tuning, or custom
   eager-commit settings.

The portable fallback can remain `System` under a target configuration, but it
is not part of this Linux-first performance exit. Stable
`#[global_allocator]` is sufficient. Nightly `allocator_api` is unrelated to
selecting the process-wide allocator.

### Rust and Code-Generation Tournament

The checkout pins Rust 1.97.1 and has no release-profile override; Cargo's
release defaults therefore use 16 codegen units and no cross-crate LTO. Rust
1.98.0 is the current stable release at this plan date. Newer is not assumed
faster: build and runtime are scored independently.

The ranges below are unvalidated experiment priors used to rank measurements;
they are neither upstream guarantees nor current Boon results.

| Candidate | Availability | Unvalidated Boon-runtime prior before overlap | Decision |
| --- | --- | ---: | --- |
| Rust 1.98 versus 1.97.1 | stable | unknown, normally within +/-3% | update only after same-profile A/B |
| ThinLTO | stable | 1--8% | first release-profile experiment |
| `codegen-units=1` | stable | 0--5%, high producer build cost | measure only after ThinLTO |
| `target-cpu=native` | stable, nonportable | 0--5% | select for the Linux-local maximum-speed artifact if it wins |
| instrumentation PGO | stable | 3--15% | highest-priority code-generation experiment |
| BOLT | external LLVM tool | usually 0--5% | only after final PGO/ThinLTO binary and front-end counter evidence |
| sample PGO / AutoFDO | newest Rust/nightly path | same class as PGO, generally less detailed | compare only after instrumentation PGO |
| fat LTO | stable | usually small beyond ThinLTO | low-priority measurement |
| lld or mold | stable/external | runtime approximately 0% | developer Rust-link-time choice only |

These gains overlap. The current planning prior for their combined effect is
roughly 5--20%, or 150--600 ms on a three-second request; it must be replaced by
measured Boon data. The opportunity may shrink when the architecture removes
allocation and pointer chasing. These tools are worthwhile product finishing
work; none removes two seconds of repeated compiler passes by mechanism.

Use separate named Cargo profiles so each increment can be isolated:

```toml
[profile.release-thin]
inherits = "release"
lto = "thin"

[profile.release-max]
inherits = "release-thin"
codegen-units = 1
```

Pass `-Ctarget-cpu=native` only to the machine-local artifact. Train stable
instrumentation PGO on NovyWave diagnostics and verified builds, Counter,
TodoMVC, valid and invalid programs, recursive/generic/effect-heavy fixtures,
and cold and persistent-session requests. Score on a disjoint holdout. The
official [rustc PGO guide](https://doc.rust-lang.org/rustc/profile-guided-optimization.html)
owns the command protocol.

BOLT is attempted only if Linux hardware counters show material instruction-
front-end pressure. It requires an unstripped ELF symbol table and strongly
prefers retained relocations for maximum gains; its
own [README](https://github.com/llvm/llvm-project/blob/main/bolt/README.md) and
[Clang case study](https://github.com/llvm/llvm-project/blob/main/bolt/docs/OptimizingClang.md)
own the exact tooling. Rust's production BOLT experiment reported about a 1.8%
mean cycle reduction, which is a useful reality check, not a Boon forecast.
The reference machine currently has neither `perf` nor the BOLT/profile tools
in `PATH`; installing matching LLVM tools and proving counter permission is an
explicit M6 prerequisite, not work silently assumed by an earlier timing.

### Nightly Feature Decision

| Nightly facility | What it could change | Decision |
| --- | --- | --- |
| `allocator_api` | place selected standard containers in a phase/session allocator and bulk-free them | useful ergonomics only after a proven lifetime boundary; do not design around it because stable custom slabs provide the required architecture |
| sample-profile compiler flags / AutoFDO | guide code layout and inlining from sampled Linux branch profiles | optional comparison after stable instrumentation PGO; not a reason to make all builds nightly |
| `portable_simd` | manually vectorize a measured byte scan, hash, bitmap, or packed validation loop | allowed only for a hot loop proven by counters after packing; no predicted whole-compiler win |
| unstable intrinsics, branch hints, specialization, or unchecked container tricks | shave instructions in a local hot loop | reject until the work-amplification owners are gone and a profile identifies the exact loop |
| async/type-system ergonomics | remove boxes or simplify APIs in asynchronous code | irrelevant to the measured single-threaded compiler hot path |

No nightly language feature removes recursive `Type` reconstruction, cloned
paths, cumulative substitutions, duplicate checking, or whole-plan cloning.
Changing the toolchain without changing those owners would optimize the wrong
algorithm.

At this plan date, stable Rust 1.98 does not contain stable sample-profile use.
`-Cprofile-sample-use` merged on 2026-08-17 after its branch point and is in a
newer nightly. The locally installed 2026-08-15 nightly predates that merge and
still requires `-Zprofile-sample-use`; `-Zdebuginfo-for-profiling` also remains
unstable and is recommended for accurate sample mapping. M6 must record an
exact toolchain commit and discover its supported flags rather than copying a
command between these toolchains.

## Biggest-First Execution Plan

### Live Source-Owner Map

These are the starting ownership seams in the audited checkout. Line numbers
are evidence anchors, not an instruction to preserve the current file split.

| Owner | Current seam |
| --- | --- |
| parser and syntax | `crates/boon_parser/src/lib.rs:2531-2568`; `crates/boon_syntax/src/lib.rs:522-608`, `1053-1121`, `1367-1385`, `1490-1527`, `1711-1718` |
| production kernel adapter | `crates/boon_compiler/src/kernel_oracle.rs:587-603`, `689-834`, `901-1102`, `2479-2480`, `2728-2763` |
| session invalidation/demand | `crates/boon_compiler/src/session.rs:406-488`, `708-807`; `crates/boon_compiler/src/lib.rs:849-879` |
| kernel operations and terms | `crates/boon_compiler_kernel/src/program.rs:697-913`; `term.rs:42-108`, `433-452`, `760-792`; `artifact_terms.rs:62-79` |
| still-boxed kernel rows | `crates/boon_compiler_kernel/src/owner.rs:249-380`, `819-906`, `2199-2240`; `crates/boon_checked/src/type_terms.rs:39-105` |
| type receipt and link reconstruction | `crates/boon_compiler_kernel/src/receipt.rs:800-841`, `941-1011`; `link.rs:1158-1200`, `7029-7098` |
| semantic graph stack | `crates/boon_semantic/src/lib.rs:621-650`, `2746-3068`; `out_net.rs:139-167`, `430-455`; `dependency_manifest.rs:2933-3006` |
| canonical/IR boundary | `crates/boon_semantic/src/program_core.rs:5-53`, `170-241`, `457-650`; `crates/boon_ir/src/lib.rs:30-37`, `297-324` |
| backend clone/rebuild | `crates/boon_compiler/src/machine_plan_backend.rs:7340-7556`; `crates/boon_plan/src/lib.rs:10977-11478`, `14157-14166` |

M1 is the completion of the already-selected type-term cut, not a new side
project. The current expression sidecar demonstrated the identity direction,
but rich `DefinitionArtifact` fields remain authoritative beside it and every
downstream consumer still rematerializes them. The tranche finishes only when
compact `TypeRef` is the sole production authority.

### M0 — Make Product Timing Honest and Land the Linux Allocator Bridge

This is a bounded enabling slice, not a performance milestone by itself.

- separate the uninstrumented Linux product allocator from the exact counting
  allocator used by the performance producer;
- integrate and record exact mimalloc 3.5, not a wrapper that vendors 3.3.2;
- preserve the system-allocator comparison lane;
- split in-memory preview completion from optional pretty JSON/export timing;
- add allocator ID, toolchain commit, target CPU, profile flags, binary hash,
  source hash, and product kind to each report;
- take randomized 20--30-run p50/p95 baselines before combining any codegen
  options.

Exit: exact behavior/digests match, product time has no allocation-counter TLS
tax, mimalloc survives long-session/cancellation tests, and the report cannot
mistake export time or Rust producer build time for Boon compile time.

### M1 — Install the Cross-Phase Identity and Packed-Type Firewall

- implement `SymbolId`, `PathId`, packed `TypeId`, stable unit/definition/local
  IDs, and compact source spans;
- make the kernel consume and emit these IDs directly;
- replace per-operation `Arc`/`Box`, `Vec<BTreeSet<_>>` staging, and boxed term
  buckets with exact-capacity columns and two-pass CSR construction;
- change alpha-normalization to local integer remapping;
- delete production rich-type relocation and materialization;
- make rich types and routes lazy oracle/editor projections.

Exit:

- no `String`, `Vec<String>`, or recursive `Type` occurs in a hot persistent
  row;
- no solver-loop allocation occurs after planned column capacity is reserved;
- no alpha-normalization or link step constructs a rich type tree;
- verified allocation calls are below 3.5 million in the first vertical cut
  and below one million when all owner kinds use the packed authority;
- directional expectation: diagnostics 250--400 ms, verified 0.9--1.4 s.

Missing the latency estimate by more than 25% triggers a new owner/profile
review. It does not trigger a string of local hash-map tweaks.

### M2 — Delete the Parser-to-Kernel and Kernel-to-Checked Compatibility Owners

- make compile-mode parsing emit `DenseSyntaxProject` directly;
- retain project resolution as an immutable link overlay;
- eliminate source/name/path copying that exists only for editor DTOs;
- make `DefinitionCode` the kernel output for every owner kind;
- remove production `PreparedKernelProjectProjection` and the old SOURCE ABI
  typechecker call;
- return `CompilerDiagnostics` directly for diagnostics demand;
- reserve rich checked projection for an explicit editor/oracle/export demand;
- delete `kernel_oracle.rs` production DTO preparation and checked-image
  reconstruction after differential parity.

Exit:

- the production dependency graph contains no old-checker call;
- no `PreparedOwner`, rich checked image, or source-shaped residual is built on
  diagnostics or native preview paths;
- changed units are the only reparsed units;
- directional expectation: cold diagnostics 60--150 ms before exact
  incrementality.

### M3 — Make the Revision Database the Only Compiler Scheduler

- retain `DefinitionCode`, public/private fingerprints, diagnostics, exact
  dependency spans, and reusable slabs by revision;
- use generation-stamped owner/SCC queues for revision zero and later edits;
- promote diagnostics work into verified demand for the same revision;
- implement result backdating and exact public-interface firewalls;
- implement cooperative in-flight supersession;
- delete the one-project-wide checked slot and every second scheduler or
  dependency authority.

Exit:

- an ordinary one-line edit parses one unit and recomputes only its exact dirty
  owner/SCC cone;
- a diagnostics-then-preview sequence checks each dirty owner once;
- all reused and recomputed owners, rows, edges, allocations, and bytes are
  reported;
- warm diagnostics meets 5--15 ms p50 and 16.7 ms p95 for the reference edit;
- cold revision-zero behavior and artifacts remain identical.

### M4 — Replace Semantic Graph Expansion with Definition Modules and Frames

- extend `DefinitionCode` to publish normalized execution, resource, reactive,
  storage, view, effect, proof, and relocation facts once;
- extend the existing parent-linked OUT frame model through semantic and plan
  consumers instead of rebuilding contextual rich maps;
- keep only compact local substitutions and resolve parent frames by ID;
- use one physical tagged edge authority with distinct evaluation/currentness
  and proof/link-relocation planes and separate CSR views;
- seal definition/SCC receipts compositionally while publishing facts;
- delete production OUT/contextual expansion and the separate execution,
  resource, reactive, lowering, storage, view, memory, canonical-core, and
  dependency-manifest reconstructions;
- retain rich V3/exhaustive materialization only as a differential oracle until
  parity, then archive or delete it.

Exit:

- the 5,061 local substitutions remain stored once and the roughly 1.96 million
  cumulative ancestry visits no longer rebuild or walk rich substitution maps;
- final work is proportional to definitions, local rows, frames, and the
  roughly 28,484 real dependency edges;
- each logical edge is stored once in its owning plane and indexed by views
  without reconstructing parallel graphs;
- directional expectation: cold verified 150--350 ms before the final backend
  cut; warm preview 30--100 ms depending on cone.

### M5 — Link and Seal One Packed Executable Image

- compile one shared plan-code module per compatible definition variant keyed
  by definition, execution domain, resolved layout, overlay/control shape, and
  capability contract;
- keep calls as invocation frames/relocations instead of one-off inlined
  specialization;
- replace the current rich `CanonicalProgramCore`/`ErasedProgram` handoff and
  reconstruction backend with a consuming target linker;
- assign final dense IDs once;
- validate local invariants and update digests while appending each section;
- perform one independent packed global ID/edge/WHERE audit;
- transfer completed columns into `SealedExecutableImage` without cloning the
  plan;
- move full legacy `MachinePlan` validation, canonical serialization, and
  pretty JSON behind untrusted-input or explicit export demands;
- update runtime consumers flag-day and delete the superseded 17-thousand-line
  reconstruction backend after parity.

Exit:

- every compatible definition variant has exactly one plan-code body and every
  occurrence references it through a compact frame;
- no trusted compiler path clones, recompacts, fully reserializes, or fully
  revalidates its just-built plan;
- ordinary native preview publishes no rich checked, semantic, IR, or legacy
  plan image;
- cold verified reaches the 40--85 ms p50 / 60--120 ms p95 envelope, with
  sub-100 ms p95 as the stretch gate;
- warm verified preview meets 15--40 ms p50 / 50 ms p95 for an unchanged-
  interface edit.

### M6 — Apply Profile-Guided and Machine-Specific Finishing Work

Only after M1--M5 provide the intended representation:

1. update and A/B the newest stable Rust;
2. test ThinLTO;
3. test the incremental effect of one codegen unit;
4. test `target-cpu=native` for the local Linux artifact;
5. train and score stable instrumentation PGO;
6. install/enable hardware profiling and attempt BOLT only if instruction-
   front-end counters justify it;
7. compare a current nightly sample-PGO build only if a stable candidate still
   leaves measurable code-layout opportunity;
8. use bounded SCC parallelism only if the single-threaded cold gate already
   passes and the workload is large enough.

Exit: retain only independently measured options. A configuration within noise
is deleted. A nightly dependency without at least a five-percent holdout win is
deleted. Producer build-time cost is reported but cannot reject a runtime win
for the speed-first local artifact unless it prevents practical development.

## End-State Allocation and Memory Gates

The following are provisional stretch-policy gates, not predictions derived
from the current trace. They are intentionally aggressive because retaining 14
million calls would reject the intended packed, zero-per-row design even if a
future allocator serviced them unusually quickly. Recalibrate the intermediate
numbers after the first packed vertical prototype while preserving the final
zero-per-row and zero-solver-loop invariants.

| Milestone | Rust global-allocator calls | Cumulative allocated bytes | Peak RSS direction |
| --- | ---: | ---: | ---: |
| current | 13.956 million | 1.675 GB | 349 MiB system / 407 MiB mimalloc |
| packed identity complete | <1 million | <256 MiB | <192 MiB |
| direct semantic modules complete | <100,000 | <192 MiB | <128 MiB |
| final diagnostics | <=2,000 | <=64 MiB | <=96 MiB target |
| final verified image | <=5,000 | <=128 MiB | <=128 MiB hard planning envelope |

These are architecture gates, not allocator-accounting games. One reservation
is an allocation call even if it reserves a large slab; committed and peak
resident bytes remain separately measured. There must be zero per-expression
or per-row heap objects and zero solver-loop global allocations after capacity
planning. Long-lived sessions must demonstrate bounded retained revisions and
must release superseded revision slabs.

## Verification Protocol

Every architecture checkpoint must preserve:

- canonical diagnostics and source locations;
- declaration and expression flow types;
- callable substitutions, captures, projections, calls, effects, states,
  lists, sources, and lexical bindings;
- exact dependency cones and revision currentness;
- recursive calls, late provider epochs, nested/empty/disappearing projections,
  capture backflow, generic HOLD, PASSED, long WHEN chains, heterogeneous
  collections, ABI calls, and cyclic SCC behavior;
- Counter, TodoMVC, NovyWave, invalid-source, migration/restart, and existing
  formal/WHERE behavior;
- deterministic artifacts across fresh processes.

The independent dense cross-table verifier is mandatory for compiler-built
sealed images. Only duplicate rich reconstruction and repeated validation are
deleted. Untrusted/deserialized artifacts continue through the exhaustive
public verifier. Packed symbol or path work must not introduce spelling-based
`NoElement` handling or any other UI-library convention into language/compiler
semantics.

Benchmark variants run in randomized/alternating order with one warm-up and at
least 20--30 scored samples. Reports include p50/p95 wall and CPU time, peak
RSS, allocation calls/bytes, page faults, binary text size, exact work counts,
dirty-cone size, toolchain commit, allocator identity, target CPU, producer
flags, source/worktree fingerprint, binary hash, and output digest. PGO/BOLT
training and scoring workloads are disjoint.

The benchmark suite has two binaries or modes:

- an uninstrumented product producer for user-visible latency;
- an instrumented evidence producer for exact allocation and work counts.

They must produce the same semantic artifacts. Neither may use extra caches,
threads, relaxed verification, example-specific shortcuts, or hidden output
omission. Product intents explicitly decide whether editor data, exhaustive
proof data, legacy plan export, or pretty serialization is required.

Current allocation counts are Rust global-allocator events on the single
compiler thread, not necessarily every allocation performed internally by a
native dependency. Before compiler parallelism, the evidence producer must
aggregate per-worker counters deterministically. If process-wide native
allocation is needed, measure it with a separate allocator/profiler protocol
and label it separately.

## Rejected Shortcuts

- Claiming Microsoft mimalloc 3.5 by depending on a Rust wrapper that still
  vendors 3.3.2.
- Treating an 11% faster Rust workspace build as an 11% faster Boon compiler.
- Switching all development to nightly for `allocator_api` before defining a
  correct phase/revision lifetime.
- Using one project-wide bump arena that destroys incremental ownership.
- Adding threads around duplicated global passes.
- Debouncing the UI and calling the compiler faster.
- Skipping WHERE, exact proof, diagnostics, or deterministic publication.
- Timing diagnostics, then recompiling the same revision for preview.
- Leaving rich checked/semantic/IR/plan materializers in the normal path after
  the packed consumer exists.
- Claiming preview speed by omitting work that the preview consumes, or
  charging preview for an export it does not consume.
- Tuning hashers, small vectors, capacities, branch hints, or SIMD while a
  whole-owner representation multiplier remains.

## Primary Research References

- [Microsoft mimalloc 3.5.0 release](https://github.com/microsoft/mimalloc/releases/tag/v3.5.0)
- [mimalloc 3.5 design and build documentation](https://github.com/microsoft/mimalloc/tree/v3.5.0)
- [Rust mimalloc wrapper and bundled sys source](https://github.com/purpleprotocol/mimalloc_rust)
- [Rust 1.98.0 release](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/)
- [Cargo release-profile behavior](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [rustc code-generation options](https://doc.rust-lang.org/rustc/codegen-options/)
- [rustc instrumentation PGO](https://doc.rust-lang.org/rustc/profile-guided-optimization.html)
- [Rust sample-profile-use stabilization](https://github.com/rust-lang/rust/pull/155942)
- [nightly allocator API tracking](https://doc.rust-lang.org/nightly/unstable-book/library-features/allocator-api.html)
- [LLVM BOLT](https://github.com/llvm/llvm-project/blob/main/bolt/README.md)
- [Rust compiler BOLT deployment](https://github.com/rust-lang/rust/pull/116352)
- [rustc incremental query red/green algorithm](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html)
- [Salsa revisions, dependencies, and backdating](https://salsa-rs.github.io/salsa/reference/algorithm.html)
- [Tree-sitter incremental parsing](https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html)
- [Roslyn immutable shared syntax trees](https://learn.microsoft.com/en-us/dotnet/csharp/roslyn-sdk/work-with-syntax)
- [TigerBeetle memory ownership architecture](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/ARCHITECTURE.md)
- [TigerBeetle without dynamic memory allocation](https://tigerbeetle.com/blog/2022-10-12-a-database-without-dynamic-memory/)

## Final Architectural Test

The work is finished only when the following statement is true:

> One source fact receives a stable compact identity once, is solved once per
> dirty revision, is published once as normalized definition code, is linked
> by compact frames and relocations, contributes through explicit tagged
> proof/currentness planes without graph reconstruction, and moves once into
> the runtime image.

If a profile still shows millions of allocations, a whole-project second
check, cumulative substitution maps, recursive type/path reconstruction,
parallel semantic authorities, or a whole-plan clone, the architecture is not
finished regardless of allocator, nightly, PGO, or LTO results.
