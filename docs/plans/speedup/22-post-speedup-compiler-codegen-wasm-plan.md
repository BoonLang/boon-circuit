# Boon Circuit: Post-Speedup Compiler, Native Codegen, Wasm, and Query-Compilation Plan

**Status:** Proposed implementation roadmap after the current BYTES + MachinePlan migration  
**Repository:** <https://github.com/BoonLang/boon-circuit>  
**Audited revision:** `95f86d265de7585ee1bc6d04cddf356d6cc16ae3` (`Advance BYTES MachinePlan speedup roadmap`, 2026-06-22)  
**Primary current context:** `docs/plans/speedup/21-speedup-execution-goal-start-context.md`  
**Live migration ledger:** `docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md`  
**Audience:** Codex and human maintainers implementing the next architecture in small, independently verified slices

---

## Executive decision

Do **not** begin production Rust, Zig, or Wasm code generation directly from either:

1. the current overloaded `boon_ir::TypedProgram`, or
2. the current `boon_plan::MachinePlan` as it exists in the audited revision.

Both are valuable migration artifacts, but neither is yet the correct permanent native-code interface:

- `TypedProgram` still mixes parser AST, semantic information, report/debug data, and execution-oriented summaries.
- `MachinePlan` has a sound versioned shell, deterministic hashing, typed IDs, verification, storage descriptions, source routes, and typed BYTES expressions, but several core sections remain descriptive rather than executable. In particular, dirty scheduling and commit planning are still largely counts, regions are mostly semantic-category buckets, and some executable shapes retain strings or AST-like expression trees.
- `boon_runtime` still depends directly on parser, typechecker, IR, plan, bridge, and report crates, so the compiler/runtime dependency direction is not yet clean.

The intended end state is:

```text
Source files + project manifest + target profile + backend options
                              │
                              ▼
                     incremental compiler DB
                              │
             ┌────────────────┴─────────────────┐
             │                                  │
             ▼                                  ▼
   resilient syntax / HIR               diagnostics / tooling
             │
             ▼
      TypedSemanticProgram
             │
             ▼
        EquationGraph
             │
       ┌─────┴──────────────────────────┐
       │                                │
       ▼                                ▼
 MachinePlan v2                   NativeRegionIR
 interpreter/tile ABI             optimized native ABI
       │                                │
       ├─ PlanExecutor                  ├─ generated Rust
       ├─ software-tile simulator       ├─ generated Zig
       └─ FPGA/tile lowering later      ├─ direct WebAssembly
                                        ├─ Rust → Wasm validation
                                        └─ Zig → Wasm validation
```

The existing migration must be **finished or cleanly contained**, not abandoned. The plan therefore has two coordinated tracks:

- **Track A — close and stabilize the current BYTES + MachinePlan goal.** Preserve all current proof reports, finish the current default-switch contract honestly, and retain legacy comparison temporarily.
- **Track B — extract the next compiler architecture behind compatibility adapters.** Build query compilation, clean semantic IR, `MachinePlan v2`, `NativeRegionIR`, and code generators without making the half-migrated runtime even more entangled.

Production backend promotion is forbidden until its differential, performance, artifact-integrity, and no-fallback gates pass.

---

# 1. Current repository state

## 1.1 What is already good and must remain authoritative

The current architecture is built around the right semantic thesis:

```text
Boon source
  → static equations
  → fixed semantic graph
  → indexed dynamic storage
  → SOURCE-triggered local work
  → HOLD/LIST commit
  → semantic deltas
```

Keep these invariants:

- no central reducer;
- no runtime graph cloning;
- no per-row graph instance;
- `SOURCE` is structural and typed;
- hidden list identity and generation stay below user code;
- `HOLD` and keyed `LIST` storage are explicit;
- normal work scales with changed values/keys and affected dependencies;
- semantic deltas, rather than whole-state snapshots or DOM diffs, are the canonical output;
- GPU/window/browser integration remains outside the host-neutral execution core;
- hardware and backend limits belong in target profiles, not in ordinary Boon source;
- verification reports are evidence, not marketing summaries.

The following existing architecture documents remain normative unless a later ADR explicitly supersedes them:

- `docs/architecture/DECISIONS.md`
- `docs/architecture/LANGUAGE_SEMANTICS.md`
- `docs/architecture/RUNTIME_MODEL.md`
- `docs/architecture/LIST_MODEL.md`
- `docs/architecture/DELTA_PROTOCOL.md`
- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/architecture/BYTES_SEMANTICS.md`

## 1.2 Current live migration status

At the audited revision, the live progress ledger reports:

| Current phase | Status |
|---|---|
| Phase 0 — Baseline | Partial |
| Phase 1 — Plan Boundary | Complete |
| Phase 2 — BYTES Parser | Complete |
| Phase 3 — BYTES Type System | Partial |
| Phase 4 — Semantic IR | Partial |
| Phase 5 — Real MachinePlan Lowering | Partial |
| Phase 6 — CPU PlanExecutor and Parity | Partial |
| Phase 7 — BYTES Runtime/Storage | Partial |
| Phase 8 — Examples | Partial |
| Phase 9 — Verification/Performance | Partial |
| Phase 10 — Default Switch | Not started |

The aggregate verifier is strong but must not be confused with goal completion:

```text
verify-bytes-machine-plan-all: 56/56 reports pass
  46 proof reports
  10 diagnostic reports

audit-goal-readiness: expected failure
  8 phases partial
  1 phase not started
  Cells release benchmark wrapper missing
  TASK-0804A remains constrained by speed budgets
  default CLI execution still legacy
```

This is a healthy state for a half-migration: much behavior is proven, but the repository correctly refuses to claim the transition is complete.

## 1.3 Current `MachinePlan` strengths

The existing `boon_plan` crate already provides valuable permanent concepts:

- `PlanVersion`;
- deterministic plan hashing;
- `TargetProfile`;
- typed plan/source/storage/region/op/delta IDs;
- typed constants including fixed/dynamic BYTES summaries;
- source payload schemas;
- storage layout;
- operation regions;
- a verifier;
- capability and fallback counters;
- debug maps;
- `dump-plan` and report-schema integration.

Preserve these concepts and their existing evidence. Evolve them through versioning rather than replacing them silently.

## 1.4 Current `MachinePlan` limitations

The current plan is not yet a complete permanent execution ABI.

### Category regions rather than fused executable regions

Current region kinds group operations by semantic category:

```text
SourceRouting
StateInitialization
DerivedEvaluation
UpdateBranches
ListOperations
ListProjections
DependencyEdges
```

A native or tile executor instead needs regions such as:

```text
source handler
row initializer
row-local update
aggregate maintenance
bulk key-range operation
host-effect continuation
delta encoder
```

Those regions must contain an explicit executable schedule or control-flow graph.

### Descriptive dirty and commit plans

Current forms are approximately:

```rust
DirtyPlan {
    dependency_edges: usize,
    unresolved_dependency_edges: usize,
}

CommitPlan {
    update_branch_count: usize,
    unresolved_update_branch_count: usize,
}
```

The permanent plan needs actual tables:

```text
changed storage → dirty regions/keys
source route → entry region
candidate group → merge policy
state group → commit barrier
region output → delta encoder/channel
```

### Remaining string or AST-shaped execution data

Examples include:

- path strings in source/debug-adjacent structures;
- operator strings;
- field names in list projections;
- generic builtin names;
- nested `PlanRowExpression` trees resembling a source expression tree;
- `compile_typed_program` inside `boon_plan`, importing both `boon_ir` and parser AST types.

Strings remain useful in debug maps, reports, diagnostics, and source maps. They should not remain the accepted execution identity.

### Target profiles include an application-specific variant

`FpgaTodomvc` was useful as a proof. The permanent design should represent generic target constraints such as capacities, text widths, queue depths, supported operations, overflow policy, and device capabilities. Keep compatibility for the current profile while migrating it to data-driven configuration.

## 1.5 Current `TypedProgram` limitations

`boon_ir::TypedProgram` currently combines:

- parser `AstExpr` and `AstStatement` values;
- expression coverage;
- semantic symbols/indexes;
- graph nodes;
- scopes;
- source ports;
- state cells;
- lists and list operations;
- derived values;
- dependency edges;
- possible causes;
- update branches;
- functions;
- view bindings;
- typecheck reports;
- verification flags.

It also contains executable-looking paths represented as strings, for example:

```text
DependencyEdge.from/to
UpdateBranch.target/source
UpdateExpression paths and operators
BytesScalarArg::Path
FileBytesPath::StatePath
```

This was reasonable during discovery. It must now be split into source-facing semantic/debug data and execution-facing typed data.

## 1.6 Current dependency-direction problem

`boon_runtime` currently depends directly on:

```text
boon_parser
boon_typecheck
boon_ir
boon_plan
boon_bridge
boon_report_schema
```

The permanent host-neutral executor must not know how to parse or typecheck source. Desired direction:

```text
parser → typecheck → semantic IR → compiler/lowering → plan/native IR
                                                       │
                                                       ▼
                                         runtime core / code generators
```

Not:

```text
runtime → parser + typechecker + semantic IR + plan + reports
```

## 1.7 Existing speedup/codegen proofs that must be reused

Earlier speedup work has already proved several important ideas:

- deterministic `.boonc` artifact emission and source-free loading;
- Counter, TodoMVC, and Cells artifact parity surfaces;
- a scalar micro-op/bytecode proof;
- a generated Rust-enum-kernel proof with parity;
- incremental local count over a large keyed list;
- no graph cloning on accepted paths;
- structured performance and stale/tamper verification.

Do not build an unrelated second codegen system. Migrate these proofs into the new compiler architecture:

```text
old artifact proof      → .boonc v2 compatibility and artifact tests
old scalar micro-ops    → MachinePlan v2 op/schedule tests
old Rust-enum kernel    → NativeRegionIR scalar lowering/codegen tests
old incremental count   → aggregate optimizer + software-tile tests
```

## 1.8 Current performance blocker

`docs/plans/speedup/20-task-0804-root-flush-resolution-plan.md` identifies a remaining CPU runtime root-flush/fanout/currentness/materialization cost. This must not be relabeled as “solved by future codegen.”

Generated direct native handlers may eventually bypass generic root-flush work for covered routes, but the current migration contract must be handled honestly:

- fix the blocker under the current goal; or
- create an explicit, reviewed ADR that moves the gate to a replacement path with equivalent semantic and performance proof.

Never make readiness pass merely by deleting or weakening the gate.

---

# 2. Non-negotiable architectural rules

## 2.1 One semantic language, multiple execution representations

Boon semantics remain single and backend-independent. Different backends may use different physical representations:

- `MachinePlan` for interpretation, software tiles, and future hardware;
- `NativeRegionIR` for direct Rust, Zig, and Wasm code;
- GPU/tensor/tile IRs later as lowerings from region metadata.

No backend may invent different `SOURCE`, `HOLD`, `LATEST`, `LIST`, BYTES, ordering, or delta semantics.

## 2.2 No production codegen from AST or debug tables

Generated code may refer to source maps for diagnostics, but executable behavior must come from verified typed IR.

Forbidden accepted path:

```text
AST/string path → code generator → native code
```

Required path:

```text
typed semantic IR → verified lowering → verified NativeRegionIR → code generator
```

## 2.3 Preserve the scalar fast path

A small source event must compile to a direct function call or compact PlanExecutor route. Do not impose:

- one task per equation;
- one queue hop per equation;
- one graph token per primitive operation;
- a thread pool for tiny UI state changes;
- a global barrier for a local event;
- GPU dispatch for scalar work.

## 2.4 Use coarse regions

The scheduling/codegen unit is a fused semantic region:

```text
SOURCE handler
row-local update
aggregate maintenance
row initialization
bulk list transform
stream stage
host-effect continuation
```

It is not an individual `add`, `get`, or field projection.

## 2.5 No silent fallback

A selected backend must either:

- compile and execute the requested program; or
- reject it with a structured unsupported-capability diagnostic.

It must not silently execute through the legacy runtime or interpreter while reporting Rust, Zig, Wasm, GPU, or tile success.

## 2.6 Version every durable boundary

Version:

- `MachinePlan`;
- `NativeRegionIR` serialization if persisted;
- `.boonc` artifacts;
- runtime host ABI;
- source-event packet ABI;
- delta/effect packet ABI;
- generated runtime support library ABI.

## 2.7 Keep the runtime host-neutral

The execution core must not depend on:

- GPU/window crates;
- browser APIs;
- process management;
- parser/typechecker;
- report rendering.

Host effects use explicit requests and completions.

## 2.8 Verification is a product feature

Every backend must emit evidence including:

- input source and semantic hashes;
- plan/native-IR hashes;
- compiler version;
- target/toolchain version;
- command argv as an array;
- capability coverage;
- fallback counters;
- parity results;
- performance measurements where claimed;
- stale/tamper checks.

---

# 3. Target compiler architecture

## 3.1 Queries and representations

```text
Compiler inputs
  SourceText(FileId)
  ProjectManifest(ProjectId)
  TargetProfile(ProfileId)
  BackendOptions(BackendId)
  OptimizationMode
  ToolchainManifest
        │
        ▼
Compiler query facade (`boon_compiler`)
        │
        ├─ parse_snapshot(file)
        ├─ lower_hir(file)
        ├─ resolve_project(project)
        ├─ typecheck_project(project)
        ├─ semantic_program(project)
        ├─ equation_graph(project)
        ├─ machine_plan(project, profile)
        ├─ native_region_ir(project, profile)
        ├─ optimized_native_ir(project, profile, mode)
        ├─ rust_source(artifact_key)
        ├─ zig_source(artifact_key)
        └─ wasm_module(artifact_key)
```

Separate hashes should exist for:

```text
source snapshot hash
syntax hash
semantic hash
MachinePlan hash
NativeRegionIR hash
backend artifact hash
```

A comment-only change may change source/syntax hashes but should not necessarily invalidate the semantic or codegen artifact.

## 3.2 Representation responsibilities

### Lossless syntax / parsed snapshot

Purpose:

- exact source spans;
- comments/whitespace for tooling;
- error recovery;
- incomplete-program diagnostics;
- formatter/editor support.

This can be introduced gradually. Query-wrap the current parser first; do not block the architecture on an immediate parser rewrite.

### HIR

Purpose:

- desugared source constructs;
- resolved lexical structure;
- explicit holes/error nodes;
- stable source maps;
- no backend-specific layout.

### `TypedSemanticProgram`

Purpose:

- canonical language meaning;
- typed sources, state, lists, functions, effects, scopes, equations;
- debug/explanation graph;
- no parser AST embedded in accepted execution data;
- no executable path strings.

### `EquationGraph`

Purpose:

- typed dependencies;
- state/list read-write sets;
- source reachability;
- possible causes;
- row/key domains;
- cycles and commit boundaries;
- candidate/`LATEST` relationships;
- region-formation input.

### `MachinePlan v2`

Purpose:

- deterministic interpreter/tile executable plan;
- direct source-route entry tables;
- typed physical storage refs;
- explicit dirty schedules;
- explicit candidate/commit groups;
- concrete delta/effect routes;
- bounded target profile;
- software-tile and hardware-ready representation.

### `NativeRegionIR`

Purpose:

- native direct execution;
- SSA-like values and basic blocks;
- explicit typed storage accesses;
- direct calls, loops, branches, and effect barriers;
- region fusion and specialization;
- Rust/Zig/Wasm code generation;
- CPU/GPU/tile placement metadata.

It must not be a generic stack bytecode written as source code.

---

# 4. Proposed crate and dependency structure

Avoid creating many tiny crates immediately. Begin with four clear ownership changes.

```text
crates/boon_parser
  source tokens, spans, parsed/lossless syntax

crates/boon_typecheck
  type/effect facts and diagnostics

crates/boon_ir
  HIR, TypedSemanticProgram, EquationGraph, debug/source maps

crates/boon_compiler                 NEW
  query facade
  orchestration
  semantic → plan lowering
  semantic → native IR lowering
  target profiles
  artifact keys/cache policy

crates/boon_plan
  MachinePlan schema only
  verifier
  serializer/hash/version adapters
  no parser or boon_ir dependency after migration

crates/boon_native_ir                NEW
  NativeRegionIR schema
  verifier
  optimization passes

crates/boon_codegen                  NEW
  rust/
  zig/
  wasm/
  shared ABI/layout/source-map utilities

crates/boon_runtime
  temporary compatibility facade

crates/boon_runtime_core             NEW or extracted module
  PlanExecutor
  typed storage
  source/delta/effect ABI
  no parser/typecheck/IR/GPU/browser dependency

crates/boon_artifact                 optional after ABI stabilizes
  .boonc v2 reader/writer
  v1 compatibility adapter

crates/boon_native_gpu
  rendering/surface pipeline only
  compute backend remains separate
```

## 4.1 Transitional rule

Do not move thousands of lines merely to satisfy a diagram. Use adapters:

```text
legacy TypedProgram
   → compatibility semantic adapter
   → current MachinePlan v1 / PlanExecutor
```

Then move one ownership boundary at a time, with unchanged parity reports.

## 4.2 Required final dependency direction

```text
boon_parser ─┐
             ├─> boon_ir/typecheck ─> boon_compiler ─┬─> boon_plan
boon_typecheck┘                                      ├─> boon_native_ir
                                                     └─> boon_codegen

boon_plan ─> boon_runtime_core
boon_native_ir ─> boon_codegen
boon_runtime_core ─> bridge/host-neutral ABI
```

Forbidden final dependencies:

```text
boon_plan → boon_parser
boon_plan → boon_ir
boon_runtime_core → boon_parser/typecheck/boon_ir
boon_codegen → boon_runtime legacy internals
```

---

# 5. Core data contracts

## 5.1 Typed identities

Introduce or stabilize distinct identity domains:

```rust
SourceId
StateId
ListId
FieldId
FunctionId
ScopeId
EquationId
RegionId
StorageId
LayoutId
CommitGroupId
DeltaSchemaId
EffectSchemaId
DiagnosticSpanId
```

Source-facing names live only in symbol/debug tables.

## 5.2 Typed storage place

A code generator should consume a concrete storage place, not a path string:

```rust
pub enum StoragePlace {
    RootScalar {
        storage: StorageId,
        value_type: ValueType,
    },
    ListField {
        list: ListId,
        layout: LayoutId,
        field: FieldId,
        field_index: u16,
        key: KeyOperand,
        value_type: ValueType,
    },
    ListMeta {
        list: ListId,
        kind: ListMetaKind,
        key: Option<KeyOperand>,
    },
    ByteBank {
        storage: StorageId,
        width: ByteWidth,
        offset: OffsetOperand,
    },
    Constant(ConstantId),
    SourcePayload {
        source: SourceId,
        field: PayloadFieldId,
    },
}
```

## 5.3 `MachinePlan v2`

Proposed shape:

```rust
pub struct MachinePlanV2 {
    pub version: PlanVersion,
    pub target: ResolvedTargetProfile,
    pub constants: ConstantPool,
    pub layouts: LayoutTable,
    pub storage: StorageLayout,
    pub sources: SourceRouteTable,
    pub regions: Vec<PlanRegion>,
    pub dirty: DirtySchedule,
    pub commits: CommitSchedule,
    pub deltas: DeltaSchedule,
    pub effects: EffectSchedule,
    pub capabilities: CapabilitySummary,
    pub debug: DebugMap,
}
```

### Source route table

```rust
pub struct SourceRoute {
    pub source: SourceId,
    pub scope: SourceScope,
    pub payload: PayloadSchemaId,
    pub entries: Vec<RegionEntry>,
}
```

### Executable dirty schedule

```rust
pub struct DirtySchedule {
    pub on_source: Vec<Vec<RegionEntry>>,
    pub on_storage_change: Vec<Vec<DirtyTarget>>,
    pub indexed_frontiers: Vec<IndexedFrontier>,
    pub bulk_frontiers: Vec<BulkFrontier>,
}
```

### Commit schedule

```rust
pub struct CommitGroup {
    pub id: CommitGroupId,
    pub policy: CommitPolicy,
    pub candidates: Vec<CandidateSlot>,
    pub writes: Vec<StoragePlace>,
    pub downstream: Vec<DirtyTarget>,
}

pub enum CommitPolicy {
    DirectSingleWriter,
    SnapshotAtTickEnd,
    LatestByEventSequence,
    ErrorOnConcurrentCandidate,
    Reduction(ReductionKind),
}
```

### Regions

```rust
pub enum PlanRegionKind {
    SourceHandler,
    RowInitializer,
    RowUpdate,
    AggregateUpdate,
    BulkKeys,
    HostEffectContinuation,
    DeltaEncoder,
}
```

A region may use compact typed micro-ops, but the op schema must be explicit and verified. Generic builtin-by-string is diagnostic-only and cannot be executable.

## 5.4 `NativeRegionIR`

Proposed shape:

```rust
pub struct NativeProgram {
    pub version: NativeIrVersion,
    pub layouts: LayoutTable,
    pub globals: Vec<GlobalStorage>,
    pub regions: Vec<NativeRegion>,
    pub source_entries: Vec<NativeSourceEntry>,
    pub delta_schemas: Vec<DeltaSchema>,
    pub effect_schemas: Vec<EffectSchema>,
    pub debug: NativeDebugMap,
}

pub struct NativeRegion {
    pub id: RegionId,
    pub entry: RegionEntryKind,
    pub params: Vec<TypedParam>,
    pub blocks: Vec<BasicBlock>,
    pub reads: Vec<StorageAccess>,
    pub writes: Vec<StorageAccess>,
    pub effects: EffectSummary,
    pub commit: CommitPolicy,
    pub shape: RegionShape,
    pub placement: PlacementHints,
}
```

Supported region shapes:

```rust
pub enum RegionShape {
    ScalarEvent,
    RowLocal,
    KeyedBatch,
    StreamPipeline,
    Reduction,
    Grid,
    Tensor,
    HostEffect,
}
```

Essential instructions:

```text
constant
load/store typed scalar
load/store list field by key
generation validation
BYTES fixed/dynamic operations
TEXT boundary operations
branch/switch
loop over key range
call direct region/function
emit candidate
resolve candidate group
emit semantic delta
request host effect
return effect continuation token
```

The IR verifier must prove:

- all blocks terminate;
- value types agree;
- storage references match layouts;
- source payload fields exist;
- no unresolved strings or AST nodes remain;
- effects obey declared capability and order;
- commit boundaries preserve Boon snapshot semantics;
- delta values are initialized and typed;
- fixed `BYTES[N]` widths agree;
- list field IDs belong to the referenced list/layout;
- all backend-required capabilities are explicit.

## 5.5 Host-neutral event/effect ABI

```rust
pub struct SourceEventHeader {
    pub abi_version: u16,
    pub source_id: u32,
    pub scope_kind: u8,
    pub list_id: u32,
    pub key: u32,
    pub generation: u32,
    pub event_sequence: u64,
    pub payload_schema: u32,
    pub payload_offset: u32,
    pub payload_len: u32,
}
```

Effects use a request/completion loop:

```text
Boon region
  → EffectRequest(id, kind, payload)
  → host performs file/network/timer/window operation
  → EffectCompletion arrives as a typed SOURCE event
```

This supports native Rust, native Zig, Wasm/browser, and future tile hardware without embedding OS calls into semantic execution.

---

# 6. Incremental/query compilation

## 6.1 Introduce a facade before adopting a framework

Create `boon_compiler::CompilerDb` as the repository-owned API. Salsa may implement it internally, but no other crate should depend directly on Salsa types.

Reason:

- Salsa is a strong fit for memoized, dependency-tracked compiler queries.
- Its public repository still describes the project as evolving.
- A facade allows a coarse custom cache or a future replacement without rewriting all compiler consumers.

Official project: <https://github.com/salsa-rs/salsa>

## 6.2 First query granularity

Begin coarse:

```text
one query per source file parse
one query per project typecheck
one query per project semantic program
one query per target plan/native IR
one query per backend artifact
```

Do not begin with one query per expression. Fine-grained query overhead and unstable IDs can cost more than the work saved.

## 6.3 Query inputs

```text
SourceSnapshot { file_id, bytes/text, content_hash }
ProjectManifest
TargetProfile
BackendKind
OptimizationMode
CompilerBuildId
SemanticAbiVersion
PlanAbiVersion
NativeIrVersion
HostAbiVersion
ToolchainId
```

## 6.4 Query outputs

Return immutable `Arc`-backed values with deterministic equality where practical.

```text
ParseSnapshot
HirProgram
TypecheckResult
TypedSemanticProgram
EquationGraph
MachinePlanV2
NativeProgram
OptimizedNativeProgram
GeneratedSource
CompiledArtifact
Diagnostics
PerformanceExplanation
```

## 6.5 Semantic invalidation

Use two layers of hashes:

- source/syntax hash for editor and diagnostics;
- semantic hash for plan/codegen reuse.

Examples:

```text
whitespace/comment change
  → parser/tooling queries change
  → semantic hash may remain stable
  → codegen artifact reused

SOURCE payload type change
  → type/semantic hash changes
  → plan/native IR/codegen invalidated

only renderer document/layout source changes
  → runtime plan may or may not change according to semantic facts
  → rendering artifact invalidated separately
```

## 6.6 Disk artifact cache

Use content-addressed directories:

```text
target/boon-cache/<artifact-key>/
  manifest.json
  semantic.bin or hash record
  plan-v2.bin
  native-ir.bin
  generated/
  output/
  reports/
```

Artifact key includes:

```text
semantic hash
backend
optimization mode
target triple
compiler build ID
IR/ABI versions
toolchain ID
runtime support library hash
relevant target-profile hash
```

Never key only by source path or modification time.

## 6.7 Resilient syntax is a later frontend slice

Do not rewrite the parser during the current PlanExecutor default switch. Sequence:

1. query-wrap existing parser and typechecker;
2. make diagnostics/query reuse measurable;
3. add `ParseSnapshot { tree, diagnostics }` rather than single-failure output;
4. introduce lossless CST/error nodes;
5. add HIR holes and IDE-grade partial typechecking.

---

# 7. Optimization strategy

All native backends share one optimizer over `NativeRegionIR`.

## 7.1 Initial passes

Implement in this order:

1. verify input IR;
2. constant folding;
3. dead block/value/equation elimination;
4. source-route specialization;
5. direct storage-place resolution;
6. single-use temporary elimination;
7. pure region inlining;
8. source-handler/row-update fusion;
9. redundant load/store elimination;
10. direct-commit proof;
11. monomorphization;
12. incremental aggregate rewriting;
13. storage layout selection;
14. loop formation for key ranges;
15. backend capability/legalization;
16. verify after each transformation family.

## 7.2 Monomorphization dimensions

Specialize by:

```text
function argument and result types
record/row layout
LIST schema
SOURCE payload schema
root vs indexed scope
fixed BYTES width
TEXT representation boundary
target profile
device/placement class where relevant
```

## 7.3 Effect barriers

Never move or fuse across these boundaries without a proof:

```text
SOURCE ordering
HOLD snapshot/commit
LATEST event-sequence merge
LIST structural mutation
host-effect request/completion
semantic delta ordering when externally observable
```

## 7.4 Incremental aggregate transformation

Recognize common aggregates:

```text
count where predicate
sum of row field
min/max with suitable index strategy
membership/view count
bitset population count
```

Transform a row update into a constant-time aggregate delta when safe. Keep a fallback scan implementation only as an explicitly reported capability, not a hidden path.

---

# 8. Rust code generation

## 8.1 Purpose

Rust codegen is the first native performance backend because it provides:

- readable generated code;
- mature optimization through `rustc`/LLVM;
- easy comparison with hand-written Rust;
- straightforward host integration;
- a route to Wasm for differential testing;
- a practical debugging oracle for direct Wasm and Zig.

## 8.2 Generated project layout

```text
generated/<artifact-key>/rust/
  Cargo.toml
  src/
    lib.rs
    boon_core.rs
    storage.rs
    sources.rs
    regions.rs
    deltas.rs
    effects.rs
    debug_map.rs       optional
    host_native.rs     optional template
```

`boon_core` must remain deterministic and host-neutral.

## 8.3 Required generated style

Generate:

```text
typed structs and columns
direct source handler functions
direct list-field indexing
concrete BYTES widths
fused region bodies
explicit effect requests
compact typed delta writers
```

Do not generate:

```text
a generic interpreter loop
generic Value enums on hot paths
string path resolution
AST walkers
hash-map-based fields
a match over every semantic graph node per tick
```

## 8.4 Runtime support library

Keep a small versioned support library for:

- list allocation/free-list/generation logic;
- TEXT/BYTES arenas where dynamic storage is needed;
- event/delta/effect ABI structs;
- bounded queues;
- error codes;
- optional profiling hooks.

Generated application logic must remain visible and specialized.

## 8.5 First accepted Rust subset

Begin with the exact proven PlanExecutor surface:

```text
Counter
root scalar SOURCE routes
root HOLD state
TodoMVC root scalar subset
one list append/remove/toggle route
current typed row expressions
current fixed/dynamic BYTES proof fixtures
semantic deltas
```

Then expand by capability table. Unsupported constructs cause a compiler diagnostic.

## 8.6 Rust verification

For every accepted fixture compare:

```text
PlanExecutor
legacy runtime oracle while retained
generated Rust debug build
generated Rust release build
```

Compare:

- final typed state summary;
- semantic delta stream;
- source bind/unbind stream;
- effect requests/completions;
- errors;
- event ordering;
- document/runtime-turn output where applicable.

Performance acceptance begins with truth, not parity claims:

```text
no AST execution
no string lookup
no generic Value on covered hot routes
zero warm allocations where the operation itself adds no persistent data
no whole-list scan for proven incremental routes
```

Then compare against equivalent hand-written Rust.

---

# 9. Zig code generation

## 9.1 Toolchain policy

Pin an exact stable Zig toolchain in a repo-owned manifest. At the time of this plan, the official stable release is Zig `0.16.0`.

Official downloads: <https://ziglang.org/download/>

Do not write generated code against development snapshots in CI.

## 9.2 Generated project layout

```text
generated/<artifact-key>/zig/
  build.zig
  build.zig.zon
  src/
    boon_core.zig
    storage.zig
    sources.zig
    regions.zig
    deltas.zig
    effects.zig
    host_native.zig
    host_wasm.zig
```

## 9.3 `std.Io` and concurrency

Zig 0.16’s `std.Io` is useful for host effects. It must not define Boon’s semantic execution model.

Rules:

- pass host I/O capability explicitly;
- use `std.Io` in host adapters, not the pure core;
- do not emit one async/concurrent task per equation or source branch;
- use concurrency only for coarse host operations or proven large parallel regions;
- remember that `io.async` may legally execute synchronously;
- use required concurrency APIs only when the program explicitly requires it and handle unavailable concurrency honestly.

Release notes: <https://ziglang.org/download/0.16.0/release-notes.html>

## 9.4 Memory policy

Generated core receives an allocator or bounded arena explicitly. For warm scalar routes:

```text
no allocator call
no format allocation
no dynamic dispatch
```

Dynamic TEXT/BYTES, append, or host-read operations may allocate only for persistent result data or explicitly bounded scratch arenas.

## 9.5 Zig differential role

Zig is not merely a second syntax printer. It validates that:

- the native IR is backend-neutral;
- layout assumptions are explicit;
- runtime support is not accidentally Rust-specific;
- host effects are separated cleanly;
- generated code can target native and browser-compatible Wasm environments.

---

# 10. Direct WebAssembly backend

## 10.1 Why direct Wasm

Do not make browser execution depend on embedding complete Rust or Zig compilers in the browser.

Add:

```text
NativeRegionIR → direct Wasm
```

Retain:

```text
Rust → Wasm
Zig → Wasm
```

as differential and portability backends.

A direct backend is practical because WebAssembly handles structured control flow, local variables, linear memory, and validation without requiring a complete native object-file/register-allocation toolchain.

Use Bytecode Alliance tooling:

- `wasm-encoder`: <https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasm-encoder>
- `wasm-tools` validation: <https://github.com/bytecodealliance/wasm-tools>

## 10.2 Initial Wasm ABI

Exports:

```text
boon_abi_version() -> i32
boon_init(config_ptr, config_len) -> status
boon_alloc_input(len) -> ptr
boon_dispatch_source(event_ptr, event_len) -> status
boon_step() -> status
boon_delta_count() -> i32
boon_delta_ptr(index) -> ptr
boon_delta_len(index) -> i32
boon_effect_count() -> i32
boon_effect_ptr(index) -> ptr
boon_reset() -> status
```

Start with 32-bit linear-memory offsets and one worker per preview. Do not add threads or shared memory until deterministic single-worker parity is complete.

## 10.3 Browser host effects

```text
Wasm core emits EffectRequest bytes
  → worker posts request to JavaScript host
  → JS performs Promise-based operation
  → completion is posted back to worker
  → worker converts completion to SOURCE event
```

No filesystem, thread spawning, or DOM access exists in the core. Rust’s `wasm32-unknown-unknown` similarly has minimal OS facilities, which is the desired boundary for differential builds.

## 10.4 Wasm verification

Validate every module structurally before running it. Then compare direct Wasm with:

```text
PlanExecutor
generated Rust native
generated Zig native
Rust → Wasm
Zig → Wasm
```

Do not claim backend parity from final state only; compare ordered semantic deltas and effects.

---

# 11. `.boonc` artifact v2

## 11.1 Preserve v1 evidence

The repository already has source-free artifact proofs. Keep a v1 reader and test fixtures while introducing v2.

## 11.2 Proposed v2 container

```text
manifest
  magic/version
  compiler build ID
  semantic ABI
  plan ABI
  native IR ABI
  host ABI
  target profile hash
  source/semantic hashes
  section table
  section hashes

sections
  typed metadata/debug maps           optional
  MachinePlan v2                      optional/required for interpreter
  NativeRegionIR                      optional
  generated Rust source              optional
  generated Zig source               optional
  direct Wasm module                  optional
  native executable/shared library   optional external reference
  assets/BYTES constants              optional
```

Every section is individually hashed. Verification rejects:

- stale compiler/runtime ABI;
- modified section content;
- mismatched manifest hash;
- wrong target/toolchain;
- missing required capability section.

## 11.3 Artifact policy

Use v2 artifacts for reproducibility and cache exchange, but never make generated native binaries trusted merely because they are inside a `.boonc` file. Native execution still follows sandbox and signature/trust policy.

---

# 12. Playground and preview lifecycle

## 12.1 Backend selector

Eventually expose:

```text
Interpreter
Rust native
Zig native
Direct Wasm
```

Optional developer-only differential modes later:

```text
Rust Wasm
Zig Wasm
Legacy oracle
```

Do not expose a backend selector before the selected backend genuinely compiles and runs the current program. An unsupported program must show capability diagnostics, not run through the interpreter.

## 12.2 Session abstraction

```rust
trait PreviewSession {
    fn run_id(&self) -> RunId;
    fn send_source(&mut self, event: SourceEventPacket) -> Result<()>;
    fn poll_turns(&mut self) -> Result<Vec<RuntimeTurn>>;
    fn shutdown(&mut self, deadline: Instant) -> Result<()>;
}
```

Implementations:

```text
PlanSession
  current PlanExecutor path

NativeChildSession
  generated Rust or Zig process/library host

WasmWorkerSession
  browser worker with fresh Wasm instance
```

## 12.3 Native Rust/Zig Run behavior

When the user presses Run with Rust or Zig selected:

```text
1. Allocate a new RunId.
2. Snapshot source/project/profile/backend/options.
3. Compile through query DB into a content-addressed artifact.
4. Validate generated source, artifact hashes, ABI, and toolchain.
5. If compilation fails:
     - launch nothing;
     - do not kill the healthy old preview immediately;
     - mark old preview visibly stale;
     - present structured diagnostics.
6. If compilation succeeds:
     - ask old preview child to shut down;
     - terminate its full process group after a bounded deadline;
     - launch a fresh generated preview process;
     - require Hello/Ready handshake containing RunId, ABI, artifact hash;
     - require first valid frame/runtime turn;
     - atomically attach the dev window to the new session.
7. Discard all messages from older RunIds.
```

Every successful generated-code run starts with fresh Boon state. No implicit `HOLD`/`LIST` state migration.

## 12.4 Browser Run behavior

```text
compile direct Wasm in compiler worker
  → validate module
  → terminate old preview worker
  → create fresh worker
  → instantiate module
  → Hello/Ready handshake
  → attach preview
```

A compile error preserves the previous preview only as clearly stale.

## 12.5 Existing native GPU process model

The repository already separates developer and preview processes/windows. Preserve that model. Generated code should replace the execution payload/session, not merge compiler, runtime, window, and renderer into one process.

## 12.6 Sandbox requirements

Generated native compilation/execution is untrusted work:

- never invoke a shell with interpolated source/path text;
- use argv arrays;
- build only in content-addressed owned directories;
- enforce source/artifact size limits;
- enforce compiler CPU/time/memory/process limits;
- kill process groups, not only parent PIDs;
- whitelist host effects;
- deny arbitrary file/network access unless capabilities are granted;
- sanitize environment variables;
- prevent generated crate/build scripts from injecting arbitrary code;
- generated Rust/Zig projects should use fixed templates and no user-controlled dependencies;
- browser work runs in workers with bounded memory and message sizes.

---

# 13. CPU execution strategy

## 13.1 Three execution modes from one region model

```text
Scalar direct
  one source event → one direct native region function

CPU sharded
  large keyed range → owner workers/software tiles

Accelerated
  large regular region → GPU/future tile backend
```

## 13.2 Direct scalar route

A typical Todo toggle should become approximately:

```rust
fn on_todo_completed_click(rt: &mut Runtime, key: Key, generation: u32, seq: u64) {
    if !rt.todos.valid_generation(key, generation) {
        return;
    }

    let old = rt.todos.completed[key];
    let new = !old;
    rt.todos.completed[key] = new;
    rt.completed_count += i64::from(new) - i64::from(old);
    rt.active_count -= i64::from(new) - i64::from(old);
    rt.deltas.push_completed(key, generation, new, seq);
}
```

No graph traversal, string lookup, generic `Value`, task submission, or whole-list scan.

## 13.3 Software tiles only when large enough

A software tile owns:

```text
key range/list shard
columnar state
source inbox
compiled region handlers
outgoing queues
local delta buffer
```

Stateful work routes to its owner. Work stealing applies only to pure/read-only bulk regions unless the compiler proves safe ownership transfer.

## 13.4 Scoped commits

Use:

```text
direct commit
  one writer, no conflicting snapshot read

local commit group
  several candidates/fields on one worker

distributed epoch commit
  only when multiple owners participate
```

Do not impose global bulk-synchronous execution on every source event.

## 13.5 Performance explanation

Add:

```text
boon explain-performance <program>
```

Example output:

```text
Route: todo.completed.click
Backend: Rust native
Region: source_handler_14
Affected keys: 1
Storage reads: 2
Storage writes: 3
List scans: 0
Heap allocations: 0 warm
Dynamic dispatches: 0
Commit: direct_single_writer
Delta count: 1
Parallel/GPU rejected: launch cost exceeds estimated work
```

---

# 14. Future GPU and tile/hardware compatibility

Do not implement this during the native-code milestone, but preserve metadata.

`NativeRegionIR` region shapes and placement hints should support:

```text
CPU direct
CPU shard
GPU/WebGPU macro-region
Tenstorrent/Graphcore-like local-memory tile
Versal/FPGA pipeline
future Boon Region Fabric
```

Key principles retained from earlier research:

- Tenstorrent: explicit movement, local SRAM, reader/compute/writer separation;
- Graphcore: programmable local-memory tiles and compiled graph placement;
- SambaNova: fuse and place whole regions with memory and routes;
- Cerebras: incoming data/event tags trigger local resident work;
- TRIPS/EDGE: bounded dataflow inside a coarse region;
- Boon: explicit sources, storage, dependencies, scopes, and deltas let the compiler choose the substrate.

`MachinePlan` remains the better path for software-tile and hardware interpretation. `NativeRegionIR` remains the better path for direct CPU/Wasm code and accelerator region formation.

---

# 15. Staged implementation roadmap

The stages below are deliberately ordered around the half-migrated repository. A later stage may begin experimentally behind a non-default feature after its prerequisite ABI is stable, but no backend may be promoted early.

## Milestone 0 — Reconcile and close the current speedup goal

### Purpose

Finish or explicitly supersede the current BYTES + MachinePlan contract before layering production codegen on top.

### Required work

- Re-read `21-speedup-execution-goal-start-context.md` and the live ledger at current HEAD.
- Complete the remaining supported BYTES surface required by current examples, or mark unsupported operations with explicit diagnostics and update the contract through an ADR.
- Close broad PlanExecutor parity gaps relevant to Counter, TodoMVC, and Cells.
- Resolve the non-ASCII Cells formula policy.
- Produce the missing Cells release benchmark wrapper.
- Resolve `TASK-0804A/C`, or create a reviewed superseding decision with replacement evidence.
- Make Phase 10 default switch only when readiness reports pass.
- Keep legacy compare mode for a defined soak period.

### Allowed parallel work

Documentation, interface-only crate skeletons, and dependency audits may proceed. Production codegen and playground backend selectors may not.

### Acceptance

```text
cargo xtask verify-bytes-machine-plan-all --check-existing ...  passes
cargo xtask audit-goal-readiness ...                            passes
boon_cli run defaults to PlanExecutor
legacy compare mode still available explicitly
no hidden fallback counters on accepted default scenarios
```

### Required report

`target/reports/compiler/m0-current-goal-closure.json`

Record every old phase’s final disposition. Do not overwrite historical reports.

### Kill criterion

If default PlanExecutor is materially slower or semantically incomplete, do not delete legacy. Keep the new default switch blocked and open explicit defects.

---

## Milestone 1 — Extract compiler/runtime boundaries

### Purpose

Stop deepening the current dependency inversion.

### Changes

- Add `boon_compiler`.
- Move `compile_typed_program` orchestration out of `boon_plan` behind a compatibility facade.
- Make `boon_plan` schema/verifier/serialization-focused.
- Extract `boon_runtime_core` or an equivalent module containing PlanExecutor/storage/event/delta logic.
- Move report construction to adapters outside the hot executor where practical.
- Preserve public CLI behavior and report hashes where schema versions do not change.

### Likely files/crates

```text
Cargo.toml
crates/boon_compiler/
crates/boon_plan/src/lib.rs
crates/boon_runtime/src/
crates/boon_runtime_core/       optional new crate
crates/boon_cli/
crates/xtask/
```

### Acceptance

- `boon_plan` no longer imports parser AST.
- Final target: `boon_plan` does not depend on `boon_ir`; an intermediate adapter may remain in `boon_compiler`.
- PlanExecutor core does not parse/typecheck source.
- All current parity and plan verification reports remain equivalent.
- A dependency audit rejects forbidden crate edges.

### Proposed command

```text
cargo xtask verify-compiler-boundaries \
  --report target/reports/compiler/m1-boundaries.json
```

---

## Milestone 2 — Split semantic IR from AST/debug data

### Purpose

Create a durable typed semantic input for both plans and native regions.

### Changes

- Introduce `TypedSemanticProgram` and `EquationGraph`.
- Keep source AST/spans in a separate source map/debug object.
- Resolve executable paths, fields, operators, encodings, and endianness to typed IDs/enums.
- Remove `AstExpr`/`AstStatement` from accepted execution structures.
- Keep an adapter that derives current reports until report consumers migrate.

### Acceptance

```text
executable_string_path_count = 0
runtime_ast_dependency_count = 0
unknown_semantic_op_count = 0 on accepted fixtures
```

- Typed semantic serialization/hash is deterministic.
- Counter/TodoMVC/Cells semantic summary and possible-cause reports remain explainable.
- Tampering an ID/layout relationship causes verifier failure.

---

## Milestone 3 — `MachinePlan v2`

### Purpose

Turn the plan from a typed semantic mirror into a complete deterministic interpreter/tile ABI.

### Changes

- Add source route → entry-region tables.
- Add physical storage/layout references.
- Replace category-bucket execution with fused region entries.
- Add concrete dirty adjacency/frontier tables.
- Add concrete candidate/commit groups and policies.
- Add explicit delta/effect encoders and queues.
- Replace generic builtin execution with typed ops.
- Generalize target profiles while retaining a v1 compatibility adapter.
- Version plan as v2; retain v1 reader/tests.

### Acceptance

- PlanExecutor runs accepted scenarios from serialized v2 plan with no source/AST/IR loaded.
- A plan report can explain exactly which source routes, regions, writes, commits, and deltas execute.
- Dirty work is proportional to affected dependencies/keys on proof fixtures.
- v1 artifacts either migrate deterministically or fail with a clear version diagnostic.

### Proposed commands

```text
cargo xtask verify-machine-plan-v2
cargo xtask verify-plan-v1-compat
cargo xtask verify-plan-v2-adversarial
```

---

## Milestone 4 — Query compilation and content-addressed caching

### Purpose

Make compiler work incremental in the playground and reproducible across backends.

### Changes

- Add `CompilerDb` facade.
- Implement coarse queries around current parser/typechecker/semantic lowering first.
- Add query metrics and invalidation reports.
- Add semantic hashes separate from source/syntax hashes.
- Add content-addressed disk cache and toolchain-aware artifact keys.
- Evaluate Salsa behind the facade; pin exact version if adopted.

### Acceptance

Tests demonstrate:

```text
same source/options → cache hit and identical artifact hash
comment-only edit → parse changes; semantic/codegen artifact reused where valid
one function edit → only dependent semantic/artifact queries invalidated
backend switch → semantic queries reused; backend artifact rebuilt
profile change → appropriate plan/native IR invalidation
compiler/toolchain change → artifact cache miss
```

### Reports

```text
target/reports/compiler/query-cold.json
target/reports/compiler/query-warm.json
target/reports/compiler/query-edit-matrix.json
```

Report query counts, durations, cache hits, invalidations, and hashes.

---

## Milestone 5 — Verified `NativeRegionIR`

### Purpose

Create the common optimizing input for Rust, Zig, and direct Wasm.

### Changes

- Add `boon_native_ir`.
- Lower source handlers, row initializers/updates, aggregates, pure functions, bulk regions, effects, and delta encoders.
- Add typed storage places, blocks, values, and effects.
- Add region/source maps.
- Add verifier and pretty printer.
- Migrate the old scalar bytecode and Rust-enum proof cases into IR tests.

### Acceptance

- Accepted regions contain no AST, path strings, generic builtin names, or unresolved types/layouts.
- Verifier catches type, CFG, storage, effect, commit, and delta tampering.
- PlanExecutor and a simple NativeIR reference interpreter produce identical results for the first subset.

---

## Milestone 6 — Shared native optimizer

### Purpose

Ensure generated Rust/Zig/Wasm express direct specialized computation rather than a serialized interpreter.

### Changes

Implement the pass pipeline from Section 7 with verifier checkpoints and pass reports.

### Acceptance

For Counter and selected TodoMVC routes:

```text
one source event → one fused source handler region
no generic dispatch
no string lookup
no dead graph nodes
single-writer state commits directly when proven
```

For the large-list aggregate proof:

```text
one changed row → O(1) aggregate maintenance
no full scan
```

### Report

`target/reports/compiler/native-ir-optimization.json`

Include before/after region/op/load/store/scan counts and semantic hash equality proof.

---

## Milestone 7 — Generated Rust AOT

### Purpose

Reach the first credible Rust-class performance path.

### Changes

- Add Rust emitter and support library.
- Generate readable source and source maps.
- Build in content-addressed directories.
- Add native host adapter and stable ABI.
- Add differential runner.
- Add benchmark pairs with equivalent hand-written Rust.

### Initial acceptance subset

```text
Counter
BYTES scalar fixtures
selected TodoMVC root routes
one append/toggle/remove list route
current Cells scanner/evaluation subset only if supported without fallback
```

### Acceptance

- State/delta/effect parity with PlanExecutor.
- Generated release executable runs without parser/typechecker/IR.
- No warm allocation for scalar routes that create no persistent data.
- No generic `Value` or string-path dispatch in generated hot handlers.
- Performance report includes hand-written Rust comparison and does not claim universal parity.

### Proposed commands

```text
cargo xtask verify-codegen-rust
cargo xtask verify-codegen-rust-adversarial
cargo xtask bench-codegen-rust
```

---

## Milestone 8 — Generated Zig AOT

### Purpose

Validate backend neutrality and provide a second native/embedded/Wasm-oriented code generator.

### Changes

- Add Zig emitter.
- Pin Zig toolchain.
- Generate pure core and separate native/Wasm hosts.
- Use `std.Io` only in host effect integration.
- Add Zig compilation and test harnesses.

### Acceptance

- Same differential matrix as Rust.
- Generated Zig contains no per-equation async/task creation.
- Warm scalar route allocation and dispatch counters meet the same policy.
- Toolchain absence yields a structured skipped/unsupported development diagnostic, not a false pass.

### Proposed commands

```text
cargo xtask verify-codegen-zig
cargo xtask verify-codegen-zig-adversarial
cargo xtask bench-codegen-zig
```

---

## Milestone 9 — Direct Wasm

### Purpose

Provide fast browser execution and compiler-in-browser deployment without bundling rustc/Zig.

### Changes

- Add direct Wasm emitter.
- Add Wasm ABI and linear-memory layout.
- Add module validator.
- Add worker host and effect bridge.
- Add Rust-Wasm and Zig-Wasm differential builds for selected fixtures.

### Acceptance

- Direct Wasm produces ordered state/delta/effect parity.
- Module instantiates in a dedicated worker.
- Fresh worker reset is deterministic.
- No DOM/filesystem/thread dependency in module.
- Memory and message size limits are enforced.

### Proposed commands

```text
cargo xtask verify-codegen-wasm
cargo xtask verify-codegen-wasm-browser
cargo xtask verify-codegen-wasm-differential
```

---

## Milestone 10 — `.boonc` v2

### Purpose

Unify reproducible plans, native IR, generated source, and compiled modules without discarding existing artifact work.

### Changes

- Implement sectioned v2 container.
- Add v1 reader/migrator tests.
- Embed or link v2 plan/native/Wasm sections.
- Add section hashes and ABI/toolchain metadata.

### Acceptance

- Counter/TodoMVC/Cells accepted subsets run source-free from v2 artifacts.
- Tampering any required section fails.
- v1 fixtures remain readable or produce explicit migration diagnostics.

---

## Milestone 11 — Playground generated-backend sessions

### Purpose

Expose real Rust/Zig/Wasm execution with honest restart behavior.

### Changes

- Add backend selector only for passing backends.
- Add Build/Run protocol with `RunId`, backend, source/semantic hash, artifact hash, and diagnostics.
- Implement native child session restart.
- Implement Wasm worker restart.
- Mark old preview stale on compile failure.
- Discard late messages from older runs.

### Acceptance

Automated tests prove:

```text
successful Run starts fresh state
old process/worker terminates
failed compile launches nothing
old preview is visibly stale
late old RunId messages are ignored
Ready handshake hashes match artifact
rapid repeated Run leaves no orphan process/worker
```

---

## Milestone 12 — Compiler inside Wasm/browser

### Purpose

Run parse/typecheck/semantic/plan/direct-Wasm generation in the playground worker.

### Changes

- Compile compiler core to Wasm.
- Remove host filesystem/process assumptions from compiler core.
- Provide virtual project/source store.
- Use in-memory query DB and bounded persistent artifact cache.
- Stream structured diagnostics and progress.

### Acceptance

- Browser compiler generates the same semantic/plan/native hashes as native compiler for fixed fixtures.
- Direct Wasm module hash is deterministic across native/browser compiler implementations where deterministic tooling permits.
- Memory limits and cancellation work.

---

## Milestone 13 — Performance promotion

### Purpose

Decide which backend is default for which workflow based on measured behavior.

### Measurements

```text
cold compile time
warm/comment-edit compile time
semantic edit compile time
artifact-cache hit latency
preview restart latency
event p50/p95/p99
throughput
warm allocations
peak memory
bytes moved
list scans
region count/code size
binary/module size
```

### Promotion rules

- Interpreter remains default development backend until generated restart latency and diagnostics are acceptable.
- Direct Wasm may become browser preview default when it beats or matches PlanExecutor experience and parity is complete.
- Rust/Zig native become explicit production build targets first.
- No backend is declared “as fast as Rust” without equivalent-algorithm comparison and disclosed toolchain/flags.

---

## Milestone 14 — Parallel/GPU/tile continuation

### Purpose

Use proven region shapes and cost data to add selective parallelism and future hardware.

### Sequence

```text
CPU direct regions
  → CPU key-range sharding
  → GPU/WebGPU macro-regions
  → software-tile simulator
  → Versal/FPGA prototype
  → custom Boon Region Fabric only after evidence
```

No one-equation-per-task/tile model.

---

# 16. Verification and subagent protocol

Each milestone uses independent subagents with non-overlapping responsibilities. The implementing agent must not grade its own work alone.

## 16.1 Required roles

### Migration architecture reviewer

Checks:

- dependency direction;
- compatibility adapters;
- no duplicate execution ABI;
- no accidental legacy deletion;
- milestone scope.

### Semantic parity reviewer

Checks:

- `SOURCE`, `HOLD`, `LATEST`, `LIST`, BYTES;
- event ordering;
- source binding/generation;
- semantic deltas;
- host effects.

### Plan/IR verifier reviewer

Attempts to tamper:

- IDs/layouts;
- value types;
- source payload fields;
- commit groups;
- dirty schedules;
- delta routes;
- BYTES widths;
- CFGs.

### Rust backend reviewer

Inspects generated source and assembly/IR where useful. Searches for:

- generic dispatch;
- string lookups;
- unnecessary allocation;
- whole-list scans;
- accidental interpreter emission.

### Zig backend reviewer

Checks:

- pinned-toolchain compatibility;
- host/core separation;
- allocator behavior;
- misuse of `std.Io`/concurrency;
- native/Wasm parity.

### Wasm/browser reviewer

Checks:

- module validity;
- ABI bounds;
- linear-memory safety;
- worker lifecycle;
- effect protocol;
- cancellation and stale RunIds.

### Security reviewer

Checks:

- command construction;
- sandboxing;
- process groups;
- cache path traversal;
- artifact tampering;
- generated dependency injection;
- resource limits.

### Performance reviewer

Checks:

- benchmark fairness;
- equivalent algorithms;
- warm-up methodology;
- percentiles/sample counts;
- compile vs run costs;
- movement/allocation/scans;
- unsupported claims.

### Report/adversarial reviewer

Checks:

- stale hashes;
- missing required reports;
- forged `status=pass`;
- command replay;
- diagnostic/proof distinction;
- hidden fallback.

### Documentation/examples reviewer

Checks:

- examples use semantic TEXT vs BYTES correctly;
- docs match implementation;
- no backend selectors precede capability;
- migration ledger reflects reality.

## 16.2 Differential matrix

During migration:

| Engine/backend | Role |
|---|---|
| Legacy runtime | Temporary oracle only |
| PlanExecutor v1/v2 | Canonical interpreter |
| NativeIR reference evaluator | Compiler-lowering oracle |
| Generated Rust | Native backend |
| Generated Zig | Independent native backend |
| Direct Wasm | Primary browser backend |
| Rust → Wasm | Differential backend |
| Zig → Wasm | Differential backend |

Compare more than final state:

```text
ordered source consumption
ordered semantic deltas
source bind/unbind events
host-effect requests/completions
errors
event sequence/candidate resolution
state summaries
document/runtime turns where applicable
```

## 16.3 Required report properties

Every proof report contains:

```text
schema version
status
proof vs diagnostic classification
commit hash
dirty working-tree state
command argv array
compiler build ID
toolchain IDs
source hash
semantic hash
plan/native IR hash
artifact hash
child report hashes
capability/fallback counters
measurement environment where relevant
```

Aggregate verifiers must reject stale or unexpected report shapes.

---

# 17. Proposed xtask/report surface

These names are proposed and do not yet exist:

```text
cargo xtask verify-compiler-boundaries
cargo xtask verify-semantic-ir
cargo xtask verify-machine-plan-v2
cargo xtask verify-native-ir
cargo xtask verify-query-incrementality
cargo xtask verify-codegen-rust
cargo xtask verify-codegen-zig
cargo xtask verify-codegen-wasm
cargo xtask verify-codegen-differential-all
cargo xtask verify-artifact-v2
cargo xtask verify-playground-backend-restart
cargo xtask verify-compiler-security
cargo xtask verify-compiler-adversarial
cargo xtask audit-post-speedup-readiness
```

Report tree:

```text
target/reports/compiler/
  current-goal-closure.json
  boundaries.json
  semantic-ir.json
  query-*.json
  machine-plan-v2.json
  native-ir.json
  optimization.json

target/reports/codegen/
  rust/
  zig/
  wasm/
  differential/
  artifact-v2/

target/reports/playground-codegen/
  native-restart.json
  wasm-worker-restart.json
  stale-runid.json
  sandbox.json
```

---

# 18. Documentation updates

Add:

```text
docs/architecture/COMPILER_PIPELINE.md
docs/architecture/SEMANTIC_IR.md
docs/architecture/MACHINE_PLAN_V2.md
docs/architecture/NATIVE_REGION_IR.md
docs/architecture/QUERY_COMPILATION.md
docs/architecture/CODEGEN_RUST.md
docs/architecture/CODEGEN_ZIG.md
docs/architecture/CODEGEN_WASM.md
docs/architecture/HOST_EFFECT_ABI.md
docs/architecture/PLAYGROUND_BACKEND_LIFECYCLE.md
docs/architecture/ARTIFACT_V2.md

docs/plans/speedup/24-post-speedup-compiler-progress.md
```

The new progress ledger must:

- link to this plan;
- record current commit and plan hash;
- list milestone statuses;
- record adaptations as ADRs;
- distinguish proof from diagnostic reports;
- never infer completion from code existence alone;
- include exact commands and artifact hashes.

---

# 19. Risks and explicit non-goals

## 19.1 Risks

### Building codegen too early

If Rust/Zig emit directly from current AST-shaped tables, the project will need a second migration later. Prevent this with the semantic/NativeIR gates.

### Over-generalizing `MachinePlan`

One IR rarely serves interpreter, native optimizer, browser, GPU, and hardware equally well. Maintain two lowerings rather than making the plan an unstructured universal IR.

### Query-system complexity

Fine-grained queries and unstable IDs can consume more time than they save. Start coarse, measure, and hide the framework.

### Generated-code restart latency

Rust/Zig builds are not suitable for every interactive Run. Use content-addressed cache, direct Wasm, and interpreter hot-reload. Do not promise instant native rebuilds prematurely.

### Divergent backend runtime libraries

Keep event/delta/effect ABI and storage semantics shared and versioned. Differential tests are mandatory.

### Report maintenance burden

The repository already has extensive evidence machinery. Reuse shared report envelopes and generators rather than hand-building unrelated JSON formats.

## 19.2 Non-goals for the first post-speedup implementation

- replacing Rust/Zig compilers with custom native machine-code generation;
- embedding rustc or Zig in the browser;
- general escaping closures if Boon does not yet require them;
- automatic GPU execution for UI-scale events;
- one task per equation;
- cache-coherent software tiles;
- custom ASIC/photonic/magnetic hardware;
- deleting legacy before default-switch soak evidence;
- full language support in the first codegen slice;
- claiming universal Rust parity.

---

# 20. Definition of completion

This roadmap is complete only when all of the following are true:

1. The current BYTES + MachinePlan goal has an honest final disposition.
2. Parser/typechecker/semantic compiler and runtime dependencies point in the correct direction.
3. Accepted execution objects contain no AST or executable string paths.
4. `MachinePlan v2` is a complete source-free interpreter/tile ABI.
5. `NativeRegionIR` is typed, verified, and backend-neutral.
6. Query compilation demonstrates measured warm reuse and deterministic cache keys.
7. Generated Rust and Zig execute accepted programs with no silent fallback.
8. Direct Wasm runs accepted programs in a fresh worker with parity.
9. `.boonc` v2 preserves and extends source-free artifact evidence.
10. Playground native/Wasm sessions restart cleanly and reject stale runs.
11. Differential reports compare state, ordered deltas, source bindings, effects, and errors.
12. Security/resource limits cover generated compilation and execution.
13. Performance reports disclose compile, startup, movement, allocation, scan, and execution costs.
14. Unsupported constructs produce structured capability diagnostics.
15. Legacy execution is removed only after a defined soak period and all required replacement gates pass.

---

# Appendix A — Suggested first implementation slice after the current goal

The safest first post-goal pull request is **not Rust codegen**. It is:

```text
1. Add boon_compiler crate.
2. Move compile_typed_program orchestration there.
3. Make boon_plan stop importing parser AST.
4. Add dependency-direction verifier.
5. Keep generated plan bytes/hash and all current parity reports unchanged.
6. Add a small TypedSemanticProgram adapter for Counter only.
7. Prove Counter can lower through both old and new semantic paths to identical plans.
```

Only after this bridge is stable should `NativeRegionIR` begin.

# Appendix B — Suggested first codegen vertical slice

```text
Counter source
  → current parser/typechecker
  → TypedSemanticProgram
  → EquationGraph
  → NativeRegionIR
  → optimize
  ├─ generated Rust
  ├─ generated Zig
  └─ direct Wasm
```

Required Counter operations:

```text
one root SOURCE payload
one root HOLD number
one add/subtract/update handler
one semantic field delta
reset/init
```

The three backends must produce byte-for-byte-equivalent canonical delta packets and identical final state. This vertical slice validates the entire new architecture while remaining small enough to audit manually.

# Appendix C — Research references

Repository and current plans:

- <https://github.com/BoonLang/boon-circuit>
- <https://github.com/BoonLang/boon-circuit/blob/main/docs/plans/speedup/21-speedup-execution-goal-start-context.md>
- <https://github.com/BoonLang/boon-circuit/blob/main/docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md>
- <https://github.com/BoonLang/boon-circuit/blob/main/docs/plans/speedup/20-task-0804-root-flush-resolution-plan.md>
- <https://github.com/BoonLang/boon-circuit/blob/main/docs/architecture/DECISIONS.md>
- <https://github.com/BoonLang/boon-circuit/blob/main/docs/architecture/RUNTIME_MODEL.md>
- <https://github.com/BoonLang/boon-circuit/blob/main/docs/architecture/DELTA_PROTOCOL.md>
- <https://github.com/BoonLang/boon-circuit/blob/main/docs/architecture/NATIVE_GPU_PIPELINE.md>

Compiler/toolchain references:

- Salsa: <https://github.com/salsa-rs/salsa>
- Zig stable downloads: <https://ziglang.org/download/>
- Zig 0.16 release notes: <https://ziglang.org/download/0.16.0/release-notes.html>
- Rust browser/minimal Wasm target: <https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html>
- wasm-encoder: <https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasm-encoder>
- wasm-tools: <https://github.com/bytecodealliance/wasm-tools>

---

**End of plan.**
