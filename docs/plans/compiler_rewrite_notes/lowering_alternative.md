> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

> Earlier, unreviewed variant from the first run of the same design prompt.

# Lowering and plan: from the checker's typed HIR straight to MachinePlan. This replaces boon_semantic, boon_verify, boon_ir, machine_plan_backend, document_executable_backend and the boon_plan sealing on the compile path.

## area
Lowering and plan: from the checker's typed HIR straight to MachinePlan. This replaces boon_semantic, boon_verify, boon_ir, machine_plan_backend, document_executable_backend and the boon_plan sealing on the compile path.

## summary
Recommendation: at cutover, emit today's MachinePlan v11 format through `boon_plan` types, and limit runtime changes to APIs. The trusted template constructor stops verifying, SealedMachinePlan is removed, and plan_digest is computed only where an artifact is encoded. A format change (v12) waits until after cutover. The compile-time budgets do not need a format change: the costs attributed to the format disappear on the compiler side alone. Those costs are the whole-plan SHA-256 inside verify_plan (~88 ms on TodoMVC), the re-verify at every runtime open and hot reload, and the 64,482 document nodes. The new lowering is two phases over the checker's HIR. Phase A (analyze, run in the diagnostics lane, ~1-4 ms) does: definition capability flags, an instance tree only for stateful definitions per static call path, static owners for LIST row regions, static arm selection, slot allocation, OUT nets by union-find, one iterative Tarjan pass for instantaneous-cycle diagnostics and topological order, and publication paths for persistence identity. Phase B (emit, preview lane only) is a partial evaluator that inlines every user call into two hash-consed DAGs (FxHash on compact keys) with constant pools: PlanRowExpressionNode for dataflow and DocumentExpr for the view. It then assembles storage, ops, persistence, list access plans, the debug map and outputs directly into MachinePlan. Nothing is verified, sealed or plan-hashed in process; verify_plan runs only on untrusted loads and in CI. Truly shared document bodies (one body per definition) are not possible in v11 without breaking incremental dependency tracking. The runtime evaluates call arguments eagerly (boon_document runtime.rs:1962-1976), so per-instance state reads passed as hidden parameters would turn targeted retained-scalar updates into structural rebuilds. v11 therefore uses full inlining plus hash-consing. I measured this at 64,482 → 11,049 nodes on TodoMVC and 31,331 → 7,745 on NovyWave. Node identity becomes a hash of the stable site key and the static call-path key, not today's positional expression ids, so focus, caret and scroll survive unrelated edits. I also give an exact v12 spec (runtime instance frames), which brings TodoMVC to about 4k document nodes and removes the per-path emission work. Persistence identities (MemoryId, MemoryLeafId, type_fingerprint, schema_hash, migration ids, EffectId/EffectInvocationId) are all derived by existing `boon_plan` functions from semantic inputs (module, owner path, semantic path, kind, data type). The new lowering must reproduce those inputs bit for bit, including the synthetic `state_N`, `@authority:` and hidden-key names. For the same schema_version, a different schema_hash makes stored user data unloadable (migration.rs:281-291). Performance model: the back half drops from ~343 ms to ≤8 ms on TodoMVC and from ~1,108 ms to ≤15 ms on NovyWave, cold. Full re-lowering on every warm edit fits the 100 ms verified-preview budget, so no incremental lowering is needed. About 127k lines of old pipeline code plus a 15.8k-line classifier registry are replaced by an estimated 14-16k lines.

## design
## 0. Verdict

* **Format: emit MachinePlan v11 (today's `boon_plan` types) at cutover. Move to v12 only after cutover, and only if it is justified.** The new crate `boon_lower` builds `boon_plan::MachinePlan` values directly. Between the checker HIR and the plan there are only side tables and two interners: no SemanticProgram, ErasedProgram, CanonicalCore, receipts or manifests.
* **Runtime changes at cutover are API-level only (R1-R4, §2).** The runtime's code for executing plans stays byte-identical. That makes old-vs-new differential testing meaningful, because both plans run on the same executor.
* **No self-verification.** In process there is no verify_plan, no plan_sha256 and no SealedMachinePlan. Verification happens only at untrusted boundaries (decoded artifacts, app bundles, plans received from a server) and in CI.
* **Document plan: full inlining per static call path into a hash-consed DAG** (measured 64,482 → 11,049 nodes on TodoMVC, 31,331 → 7,745 on NovyWave with `design/hashcons.py`). Template, node and materialization ids come from stable site keys and call-path keys. Truly shared function bodies are specified as v12 (§6), because they need the runtime to resolve instance-relative reads.

## 1. The real runtime contract (what consumers actually read)

| Consumer | Reads from the plan | Evidence |
|---|---|---|
| boon_plan_executor `Metadata::new` | constants; `debug_map.fields` labels, which are **semantic** (name-based root lookups used by scenarios and hosts); storage_layout; persistence (durable maps keyed by MemoryId); list_dataflow (activation order); list_indexes; source_routes; DerivedValue/StateUpdate/EffectUpdate/ListMutation/ListProjection ops; pulse_batches; activations; demand; producer_function_instances; distributed_endpoint. SourceRoute and DependencyEdge ops are **ignored**. The executor builds its own dependency maps and topologically sorts update ops per trigger, using op id as the tie-break; list mutations are sorted strictly by op id. | machine.rs:4405-4474, 4704, 5064, 5171, 15280-15320, 21575-21620 |
| boon_document runtime | Expressions by dense id; functions; templates (linear find, :3387); materializations; names/constants. `value_class` matters in only two places: Static means cached by expr id (:1856) and DynamicScalar means a retained constructor-argument binding (:3405, :3491). Select uses `compiler_id` as the matched-selector key (:2007). Node id is `node:{template.node}:{instance path}` (:3561, :5997); path segments come from Call (`call-{fn}-{expr}`, :1975), Materialize (`materialize-{id}-row-{list}-{key}-{gen}`, :3234) and child nesting (`node-{template}`, :3489). ElementState only checks `env.element_context == context` and returns constant false flags (:2357-2372). | boon_document/src/runtime.rs |
| Nobody at runtime | `document.view_bindings`, `initial_patch_batch`, `DirtyPlan`/`CommitPlan` counters, `capability_summary` (verify only), `semantic_slice_digest` (non-zero check only). `document_binding_value_target` (machine.rs:14229) has no caller. | grep |
| boon_host_runtime persistent.rs | Direct restore only if `schema_version` **and** `schema_hash` are equal; otherwise stage_migration. The same version with a different hash is `UnsupportedSource`, so the data is refused. | persistent.rs:760-800, 2527-2545; boon_persistence migration.rs:139, 281-291 |
| Native playground | Focus, hover, pressed, scroll and text-input caret/buffer are kept across plan replacement **iff the node-id strings are equal**. Scenario files use source paths (`store.sources.increment_button.press`) and root names (`store.count`). | runtime_view.rs:1433-1473; examples/counter.scn |
| program_core / app_package / server | `COMPILER_ID` mismatch rejects stored artifacts, forcing a recompile. plan_digest is stored and compared (bundle.rs:502, server lib.rs:1430). `MachineTemplate::new_shared` verifies again on every open and hot reload (machine.rs:9514). | program_core.rs:420, 441-458 |
| distributed | wire_schema_hash equality between client and session/server. | client.rs:77,182; session.rs:124 |

## 2. Runtime/API changes at cutover (no format change)

* **R1** `MachineTemplate::new_shared(Arc<MachinePlan>)` becomes trusted with no verify_plan, and a new `new_untrusted` does verify + new. The call sites are boon_runtime lib.rs:550 and boon_distributed_runtime session.rs:88. This removes ~92 ms (today's TodoMVC) from every preview open and hot reload.
* **R2** Delete `SealedMachinePlan`/`seal_*`. Add `boon_plan::load_untrusted(bytes) -> Result<MachinePlan>` (decode + verify_plan), used by decode_program_artifact, app-bundle load and server-received plans.
* **R3** plan_digest is computed once at artifact encode, as SHA-256 of the artifact's CBOR bytes (== ContentArtifactId, computed once, not twice as in lib.rs:772-780). Delete whole-plan `plan_sha256` (the name-bearing encoder). Stored artifacts are already invalidated by COMPILER_ID.
* **R4** (optional runtime performance) index templates by id in boon_document (runtime.rs:3387 is O(templates) per constructor).
* Keep the `boon_plan::binary` encoder, but **only** for identity inputs. Move the identity input structs (`MemoryIdentityInput` lib.rs:4937, `CanonicalPersistenceSchema`, the migration inputs, etc.) into a `boon_plan::identity_v1` module marked frozen. The encoding writes struct and field names (binary.rs:224-229, 345-350), so renaming one changes persisted identity. Add golden tests: fixed inputs produce fixed hashes.

## 3. Invariants the new lowering must reproduce

**A. Structural validity.** Every emitted plan passes `boon_plan::verify_plan` (the 41 checks, lib.rs:10999-11504) and `row_expressions.validate()`. CI runs it on every example; release in-process never does.

**B. Executor semantics**
1. `PlanOp.inputs` lists every ValueRef the op's expressions read. The executor derives dependency maps and the update-op topological order from them; a missing input means stale values or a wrong order.
2. Op ids follow authored order (HOLD LATEST-arm order, list mutation site/ordinal), because op id is the executor's tie-break and the sole order for mutations.
3. StateUpdate/ListMutation triggers are only `Source`, `State` or `Pulse` (anything else is rejected at load, machine.rs:5042-5062).
4. Exactly one DerivedValue op per derived root FieldId, one list computation per derived ListId, one row computation per row FieldId. `startup_recompute` and derived kinds follow today's rules.
5. `RootOutputDemand::Selected` is sorted and unique. Every document `Read::Field`, `Field` materialization source and `Field` binding target is inside it (document.rs:674-712).
6. Every **named** top-level field that holds a HOLD, a LIST or a dynamic value gets a FieldId/StateId/ListId whose label in `debug_map` is its semantic path, even if the view never reads it. Scenario `expect_root_text` and host queries resolve by name (machine.rs:15280-15320).
7. list_dataflow has one entry per list slot, topologically ordered, with non-zero semantic_identity and type_fingerprint. DerivedDefault requires a reconstruction_output (lib.rs:3448-3490).
8. Storage: one scalar slot per state; indexed (row) states carry scope and `indexed_field_id`; list slots carry row fields with value/authority/capture roles; byte banks for fixed BYTES.
9. Source routes: one per SourceId. `path` is the source's semantic path (host dispatch and scenarios). Row-scoped sources carry scope_id and row_projections, plus a typed payload schema.
10. Constants are deduplicated. Byte constants carry sha256, with `inline_bytes` ≤ 1024.
11. Effects: `builtin_effect_contract` per host operation, outbox schemas, `EffectInvocationId::from_result_owner(effect, target_path)`.
12. Pulse batches: `semantic_slice_digest` is non-zero; any cheap structural hash will do. Fusion may be `Ineligible` (semantically safe, machine.rs:18730-18745).
13. Output roots: `OutputRootId::from_name`. `RetainedVisual.expression` is the document root.
14. `capability_summary = derive_capability_summary(plan)` (lib.rs:14572).
15. Static-owner forest: `PlanOwner.ancestors` chain (static_owner, scope, list) is consistent with list row scopes. ROOT = usize::MAX.

**C. Document runtime**
1. Live node ids are unique. Identity-bearing ops (Constructor, Materialize, Call) are never merged by hash-consing unless their template/materialization ids are equal, and ids are unique per (site, static call path).
2. `Static` means the value is closed: no Parameter/Local/Matched/Row/State/Field/Source/ElementState read. Only then is caching by expr id sound under DAG sharing.
3. The value-class rules are an exact port of document_executable_backend.rs:4024-4146 (type-based `value_class_for_type`, `record_value_class`, `list_value_class`). The DynamicScalar-vs-DynamicStructure split decides retained-binding granularity, and so incremental performance.
4. Each Select's `compiler_id` (selector key) is unique among lexically nested Selects. Use the WHEN site ordinal; nesting the same site inside itself would need recursion, which is rejected.
5. `ElementState{context}` equals the nearest enclosing Constructor's `element_context`.
6. Expression ids are dense and ordered. Templates exist for every Constructor. The initial patch batch is canonical (derived with `DocumentPlan::build_initial_patch_batch`). `view_bindings` may be empty.
7. Materializations: row_identity and item_scope match the list's scope; the template function takes exactly the item parameter plus template_arguments.
8. A dynamic BLOCK local that is used more than once stays a `LocalBlock` + `Read::Local`, so the runtime evaluates it once. Single-use, static or trivial locals are substituted.

**D. Persistence**, exact inputs to the `boon_plan` identity functions
1. `MemoryId::from_identity({canonical_module, named_owner_path}, semantic_path, kind)` (lib.rs:5300). canonical_module is the source unit module or `$root` (memory_contract.rs:936-968). owner_path is the parent of semantic_path, or the list path for IndexedField (memory_contract.rs:413-429, 970-982). semantic_path is the **publication path**. Synthetic `state_N` covers anonymous published HOLDs (kernel link.rs:7708-7713) and `path.@authority:<route>` covers collection authorities (memory_contract.rs:715-740). Unpublished states (`$state.sN`) are not memory.
2. `MemoryLeafId::from_memory_path` over record-type leaf flattening (memory_contract.rs:984+).
3. `data_type_fingerprint(canonicalized DataTypePlan)`. The mapping from checker types must equal today's for unchanged types.
4. `hidden_key_type` comes from the pluralization rule (core_lowering.rs:79-98, e.g. `store.todos` → `Store.todoKey`) plus has_generation.
5. `initial_provenance` / `ListActivationMode` per list origin.
6. `persistence_schema_hash` (format v5), `migration_recipe_hash`, `migration_catalog_hash`, and MigrationInput/Recipe/Edge ids through the `boon_plan` constructors, with today's predecessor catalog merge.
7. Consequence: for the same source and schema_version, every one of these must be bit-identical between compiles and between compiler versions. Otherwise stored data is refused (migration.rs:281-291).

**E. Other identities (same-compiler consistency only):** `TypedListViewFingerprintContext` view fingerprints (lib.rs:6569-6700), distributed ids (`DistributedDeclarationId::from_semantic_path` and derived Export/Import/RemoteCallSite ids), wire_schema_hash, ApplicationPlan.identity_hash. Compute them all with the existing `boon_plan` functions.

## 4. Identities that must stay stable across compiler versions

* **Must (persisted user data in redb/IndexedDB):** MemoryId, MemoryLeafId, type_fingerprint, schema_hash (including hidden_key_type strings and leaf sets), MigrationInputId/RecipeId/EdgeId (completed_migration_edges are stored), EffectId and EffectInvocationId (the stored outbox), and DurableCollectionId (MemoryId plus runtime row owners). All of these are pure functions of semantic inputs, so the job of lowering is to produce identical inputs, and the job of `boon_plan` is a frozen encoding.
* **Need not:** all dense ids (State/Field/List/Source/Scope/Op/Expr/Template), DocumentNodeId strings (memory only), plan digest and ContentArtifactId (COMPILER_ID forces a recompile), view_fingerprint (cursors are sealed with a per-session key), wire ids and hash (bundles are compiled atomically).
* Under decision 5, HOLDs whose types change (e.g. lenient widening becoming a union) get a new type_fingerprint and therefore a new schema_hash. That is expected and must be listed per example in the differential report.

## 5. Pipeline

### 5.1 Input contract from the checker (to agree with the checker panel)
```rust
pub struct LowerInput<'a> {
    hir: &'a Hir,            // node arena: op u8, flags, ty TypeId, children range, span, site_key u64
    types: &'a TypeArena,    // hash-consed; unions/records/tags/lists/maps/sets/bytes/bits/render
    res: &'a Resolution,     // Ref -> Def | Param | Local | PassedField | OutFormal; Call -> DefId (static)
    sigs: &'a [DefSig],      // params, OUT formals, scope-effect summary (which params run under which OUT)
    project: &'a LowerProject, // app identity, role, target profile, schema_version, predecessors
}
```
`site_key` is a stable 64-bit hash of (unit logical path, enclosing item route, structural route inside the item: field and argument names, pipe-stage/list-item/arm ordinals). It must not change under edits to other items, formatting or sibling subtrees. The checker guarantees no recursion and a well-typed program; lowering must never fail on a checked program (an internal error is a bug). Flow modes are recomputed per inlined use (cheap, bottom-up) and checked against the checker with debug_assert.

### 5.2 Phase A: analyze (runs on every edit, diagnostics lane)
1. **Definition flags**, bottom-up over the call DAG: `HOLD|SOURCE|LIST|EFFECT|OUT_PRODUCER|CONSTRUCTOR|READS_PASSED`. A definition is `stateful` if any flag other than CONSTRUCTOR/READS_PASSED is set, transitively.
2. **Roots**: all root-scope value definitions of the entry program (today's ProgramSchedule roots, verified_intent.rs:79-110), `document`/`scene`/host outputs, external calls and producer functions. Functions are reached only through calls.
3. **Instance tree** (stateful definitions only), from a preorder walk over roots with an env `{inst, owner, params: [Operand], publish: PathId, path_key}`:
```rust
struct Instance { def: DefId, parent: InstanceId, call_site: HirId, owner: OwnerId, path_key: u64, publish: PathId }
struct Owner    { parent: OwnerId, list: ListId, scope: ScopeId, site: HirId } // LIST row region (List/map new:)
```
Pure calls create no instance; they are inlined at emission. `path_key(child) = mix64(path_key(parent), site_key(call))`.
4. **Static arm selection** (owner question Q2): if a WHEN/WHILE selector reduces to a constant after parameter substitution along this path, only the selected arm is walked. No slots or instances exist for the other arms. The same decision is reused by both emitters.
5. **Slot allocation**: per-definition local slot ordinals (states, sources, lists, fields, effects, nets) computed once per definition; per-instance bases; a final dense compaction in walk order (deterministic).
6. **OUT nets**: union-find over `NetId`. A bare fresh OUT at a call creates a net whose producer is the callee's structural producer. `formal: existing_out` does `union(net(callee, formal), net(env, out))`. Checks per root: exactly one producer (errors: no producer or alias cycle, multiple producers), and scope/shape compatibility. Consumers resolve to `find(net).producer` (a row local `(owner, local)`).
7. **Dependency graph + one iterative Tarjan pass** over instance cells (derived fields, states, lists, list views, effects), using instantaneous edges only. Temporal edges (HOLD update arms, SOURCE events, effect completions) are excluded. Any SCC larger than one node, or a self-edge, is a positioned diagnostic listing members and edge sites (decision 6). The reverse postorder gives derived-op emission order, list_dataflow order, and pulse-batch membership (per-pulse reachability over the condensation; pulses are rare).
8. **Publication paths**: record field `name: e` evaluates `e` with `publish = P.name`, and call results inherit the caller's publish path. This yields the state/list semantic paths, kinds and owner paths, synthetic `state_N`, `@authority:` routes, hidden key names and debug labels (rules in §3-D, validated by the schema_hash oracle).

Output: `Analysis` (flat Vecs indexed by InstanceId/OwnerId/NetId/slot) plus diagnostics. Estimated 1-4 ms on NovyWave.

### 5.3 Phase B: emit (preview lane only)
**Interners.** Each DAG is interned with `FxHashMap<Key, u32>`, where `Key = (op u16, class u8, a u32, b u32, extra: range into a scratch u32 pool)`. Children are already interned, so the key is O(arity) with no deep equality. On a miss the `boon_plan` node is built once. Constant pools are `FxHashMap<canonical-bytes, ConstId>`, one for `plan.constants` and one for `document.constants`. The row arena is built with `PlanRowExpressionArena::from_nodes` (lib.rs:8894); no SHA index. Interning order is DFS emission order, so plans are deterministic.

**Dataflow emitter** (partial evaluator; row expressions have no call node, so every user call is inlined):
* `lower_value(e, env) -> RowId` for continuous values.
* `lower_event(e, env) -> SmallVec<[EventArm; 2]>`, where `EventArm { trigger: ValueRef, gate: Option<RowId>, value: RowId /* Absent = SKIP */ }`. LATEST concatenates arms; `src |> THEN {v}` gives `(Source(src), v)`; WHEN on a payload gives `Select(payload, arms)` with SKIP mapped to Absent.
* HOLD: a state slot, an initializer (constant or expression), one StateUpdate per arm in authored order, with the self name bound to `ValueRef::State`.
* A LATEST over events with a continuous default, as a field, becomes `DerivedValue::SourceEventTransform`.
* Named dynamic fields become `DerivedValue::RowExpression`. Lists become slots, initial rows, Append/Remove mutations, contextual collections or materialized views with row-field copies. Effects become `EffectInvocationPlan` and outbox schemas.
* `PlanOp.inputs` is collected while lowering (the set of ValueRefs touched).
* Memoize on `(HirId, env-operand tuple)` for subtrees that are closed over the instance, so repeated pure calls with equal arguments cost O(1).
* Port the specialized derived forms today's backend chooses (SourceKeyTextTrimNonEmpty, NumberCompareConst, BoolNot, ValueCompare, MaterializedRowField) with the same selection predicates.

**List access plans**: port `plan_typed_list_access` and its helpers (machine_plan_backend.rs:9916-10760) as a pure rewrite pass over the interned row DAG. ErasedProgram/ValueIndex lookups become type-table queries, and view fingerprints come from `TypedListViewFingerprintContext`.

**Persistence**: `MemoryPlan::new`, `ListMemoryPlan::new`, `CollectionMemoryPlan::new` and `PersistencePlan::new_with_collections` from the Phase A paths and the type mapping. DRAIN/DRAINING become MigrationTransferPlan/MigrationExpressionPlan. A `persistence_only` mode is used for predecessor stages.

**Document emitter** (full inline):
* `doc(e, env) -> DocExprId`. Parameters are substituted by argument DocExprIds, so no Parameter reads remain outside materialization row templates.
* Constructor: `template = node = mix64(site_key(e), env.path_key)`; `element_context = {call_instance: path ordinal, ordinal: site ordinal}`. Collisions are detected with an FxHashSet and disambiguated deterministically.
* Materialize: `id = mix64(site, path)`, plus one DocumentFunction per materialization (body lowered once with a row env).
* Select `compiler_id` = WHEN site ordinal. Class is computed at intern time by the ported rules. Constant-selector Selects fold (partial evaluation).
* A view subexpression that needs list semantics becomes a `Materialize` (render rows) or a `RuntimeExpression` (row DAG).
* Memo on `(HirId, env-operand tuple)` for subtrees free of identity-bearing ops.
* Demand = the set of fields read by the document, host outputs and derived dependencies.

**Assembly**: storage layout, regions (SourceRoute/DependencyEdge ops can be emitted trivially or empty, since the executor ignores them), debug map (labels = semantic paths), outputs, host ports, activations, pulses (`Ineligible` until the fusion rule is ported), `derive_capability_summary`. No verify, no hash.

### 5.4 Warm path
Full re-lowering on every edit: ≤8 ms on TodoMVC and ≤15 ms on NovyWave, against the 100 ms verified-preview budget, so no incremental lowering. Phase A (≤4 ms) runs in the diagnostics lane so that cycle and OUT rules publish within 16.7 ms. Phase B runs on the preview worker and polls cancellation per definition. The only cache worth keeping is the persistence-identity memo (`FxHashMap<inputs, id>` across revisions; SHA calls drop to ~0 on edits that leave persistence unchanged).

### 5.5 Distributed and migration
Each role gets its own Phase A and Phase B. `boon_lower::link` ports `link_lowered_roles`: declaration ids from (module, semantic path), endpoint/export/import/call-site ids, RemoteCallSitePlan argument DAGs and row bindings, and ProducerFunctionInstancePlan ownership taken directly from the producer instance's slot ranges. Wire schema and hash come from `boon_plan`. Migration predecessors are compiled in `persistence_only` mode.

## 6. v12 spec (post-cutover, optional)
Shared document bodies with instance frames:
* `DocumentExprOp::Call{function, site: u64, arguments, instance: Option<ChildOrdinal>}`, where `EvalEnv.instance: InstanceId` is resolved through `instances[cur].children[k]`.
* `DocumentRead::Instance{slot}` resolves at the **use site** to State/Field/Source/List, and is recorded as the real dependency. That keeps retained-binding granularity, which hidden parameters would break (runtime.rs:1962-1976 evaluates arguments eagerly).
* Templates per site, not per path (the node id already gets the instance path prefix). Row node ids drop the dense ListId. `element_context` becomes dynamic.
* Unify the row and document arenas, make constants tagged operands, move provenance to a side table, drop dead fields (view_bindings, initial_patch_batch, DependencyEdge/SourceRoute ops, counters, capability_summary), rename `debug_map` to a real `names` table, and use one compact versioned encoding for artifacts.
* Result: TodoMVC document ≈ 3.6-4k nodes (distinct compiler ids 3,650 + 439 constants) and emission proportional to HIR size, not call paths. Trigger it if runtime open time or memory, or a UI whose path expansion exceeds ~50k visits, justifies it.

## 7. Crate layout
* `boon_lower` (new): `analyze/`, `emit_flow/`, `emit_doc/`, `list_access/` (port), `persist/`, `link/`, `assemble.rs`. Depends on the checker HIR crate, `boon_plan` and `boon_data`.
* `boon_plan` (kept): the format, frozen identity_v1, verify_plan (untrusted loads and CI), serde.
* Deleted at cutover: boon_semantic (97k lines), boon_verify (3.4k), boon_ir (4.2k), machine_plan_backend.rs (17.7k), document_executable_backend.rs (4.2k), document_plan_backend.rs, the seal/verify code paths, boon_contract (its only users are in the old pipeline), the dependency_classifier_schema_v1.toml registry (15.8k lines), and the xtask receipt-presence gates.

## 8. Performance model and sizes
Unit costs: ~50-100 ns per interned node visit (FxHash plus arena push); ~1-3 µs per `boon_plan` identity hash (small name-bearing encode plus software SHA-256 at 566 MB/s on this CPU, which has no SHA-NI).

| Stage | TodoMVC | NovyWave | Basis |
|---|---|---|---|
| Phase A (flags, instances, OUT, Tarjan, paths) | 0.5-1.5 ms | 1.5-4 ms | 4.5k / 16.3k HIR nodes × 2-3 passes; 318 / 3,494 call occurrences; ~1.6k cells |
| Dataflow emission | 0.3-1 ms | 1.5-3 ms | 73 / 2,071 row nodes; 60 / ~1,400 ops; 126 / 1,433 fields |
| Persistence identities and schema hash | ~0.1 ms | 0.5-1 ms | 12 / 126 memories plus leaves; schema encode ~100 KB |
| Document emission | 2-5 ms | 1.5-3 ms | ~65k / ~32k inline visits (memo cuts TodoMVC further) → 11,049 / 7,745 nodes |
| Assembly | <0.3 ms | <0.5 ms | labels, summary |
| **Total** | **≤8 ms** (today semantic 161 + ir 3.3 + backend 88 + plan_validation 91 ≈ 343 ms) | **≤15 ms** (today 794 + 25.5 + 221 + 67 ≈ 1,108 ms) | |

Plan size: TodoMVC compact JSON 9.17 MB → ~1.7 MB (the document is 9.10 MB today); NovyWave 6.15 MB → ~2.5 MB. In-memory document: ~11k × ~130 B ≈ 1.4 MB. Runtime open saves the verify_plan (~92 ms TodoMVC, ~66 ms NovyWave per open or hot reload). Code: ~14-16k new lines (analysis ~2.5k, flow ~5k, document ~2.5k, persistence and migration ~1.5k, list access ~1.2k, link ~2k) replace ~127k lines.

## 9. Verification strategy (test-only old-compiler oracle until cutover)
Per example (counter, TodoMVC, NovyWave, cells, persons_pro, migrations/*, distributed):
1. verify_plan(new) passes.
2. Persistence equality: schema_hash, the MemoryId/LeafId sets and type fingerprints, except documented decision-5 type changes.
3. Equal source-path sets and debug-label sets.
4. Scenario (.scn) runs on the same runtime give identical root texts and structurally equal DocumentFrames (compared modulo node-id renaming).
5. Runtime work counters (retained-binding re-evaluations, structural rebuilds) show no regression.
6. The plan normalizer maps dense ids to labels for readable diffs.
7. debug_assert that Static document nodes are closed and that template ids are unique.
8. Fuzzing and mutation of decoded plans goes through `load_untrusted`.

## owner_questions
- **question**: Persisted-state compatibility at cutover: should the new compiler reproduce today's persistence identity v1 bit for bit? That covers MemoryId/LeafId/type_fingerprint/schema_hash, the synthetic `state_N` ordinal names, `@authority:` routes, `Store.todoKey` pluralization and the name-bearing SHA-256 encoding. The alternative is a clean v2 now, with a one-time break of existing local playground and app state. | **recommendation**: Reproduce v1 at cutover, freeze its encoding in boon_plan::identity_v1 with golden tests, and use schema_hash equality per example as a differential oracle. Later, schedule an explicit v2 that replaces ordinal-based anonymous state ids with structural-route ids, shipped with a migration edge. | **why**: At the same schema_version, a different schema_hash makes stored data unloadable (boon_persistence migration.rs:281-291 UnsupportedSource). Reproducing v1 is cheap (hundreds of small hashes) and gives a strong correctness check of the persistence-relevant lowering. Ordinal `state_N` ids have a latent hazard: inserting an anonymous HOLD earlier can silently rebind a same-typed stored value to a different HOLD. Decision 2 says nobody depends on production data, but it does not say whether local state may be dropped. | **options**: (a) v1 bit-exact now, v2 later (recommended); (b) define v2 now and accept that existing state resets; (c) v1 for named paths, v2 only for synthetic names now
- **question**: Static arm selection: when a WHEN/WHILE selector is a compile-time constant along a static call path, should lowering instantiate only the selected arm? In that case HOLD, SOURCE, LIST and effects in the unselected arms do not exist for that path, which affects the persisted schema and the source routes. Types are still checked over all arms, per decision 4. | **recommendation**: Yes. Make it an explicit, documented lowering rule, applied identically in the dataflow and document emitters and decided once in Phase A. | **why**: This is semantics-adjacent. It changes which state exists and therefore schema_hash and memory ids. Today's elaboration already prunes this way (contextual_expansion static_selector_value_at; C10). Instantiating all arms would add dead state and sources and change schema hashes against the oracle. | **options**: (a) prune statically selected arms (recommended, matches today); (b) instantiate every arm on every path
- **question**: Plan format sequencing: emit today's MachinePlan v11 at cutover, and do the v12 format (shared document bodies with runtime instance frames, dead-field removal, compact artifact encoding) only after cutover, when runtime open time, memory or UI expansion justify it? | **recommendation**: Yes. v11 at cutover with only API-level runtime changes (trusted template, no seal, plan_digest only at artifact encode); v12 afterwards as a separately measured runtime and format change. | **why**: The compile-time budgets are met without a format change: removing the plan hash and verify, plus hash-consing, is enough. An unchanged runtime keeps old-vs-new scenario differential testing exact. Shared bodies need new runtime semantics (instance-relative reads resolved at the use site) to keep incremental dependency tracking, and doing that alongside the compiler rewrite would double the unknowns. | **options**: (a) v11 now, v12 later (recommended); (b) v12 at cutover, which means changing boon_document evaluation at the same time

## risks
- **risk**: Persistence identity drift: any difference in publication paths, synthetic names, leaf flattening, the type mapping or hidden-key names changes schema_hash, and existing stored data is refused. | **mitigation**: Compute every identity with the existing boon_plan functions. Freeze identity_v1 with golden tests. Gate on per-example schema_hash, MemoryId and LeafId equality against the old compiler; only decision-5 type changes are allowed, each listed explicitly.
- **risk**: The old backend encodes undocumented special cases (specialized derived forms such as SourceKeyTextTrimNonEmpty, NumberCompareConst, router_route, authority/value row-field roles, activation modes) whose absence changes behaviour or runtime performance. | **mitigation**: Port the selection predicates as a table and cite old function names in code comments. Run scenario differential tests on the same runtime plus work-counter comparisons (retained re-evaluations, structural rebuilds) per scenario step.
- **risk**: Misclassified value classes. Static on a node that is not closed gives stale cached values; DynamicStructure where the old compiler emitted DynamicScalar causes structural rebuilds, a 60 FPS regression. | **mitigation**: Exact port of document_executable_backend.rs:4024-4146. debug_assert that Static nodes contain no env/instance reads. Compare per-constructor-argument classes against the old compiler in the differential harness.
- **risk**: Full inlining makes document emission scale with the static UI size per call path. Future deeply nested component libraries could grow to hundreds of thousands of visits. | **mitigation**: Memoize identity-free subtrees on (HirId, operand tuple). Keep an emitted-node budget with a positioned diagnostic. v12 shared bodies remove per-path emission entirely; trigger it when inline visits exceed ~50k.
- **risk**: 64-bit site/path hash collisions for template, node or materialization ids would produce duplicate node ids. | **mitigation**: Detect with an FxHashSet at emission and disambiguate deterministically (a salt plus ordinal). Add a CI assertion over all examples. Collision probability is ~n²/2^65 for n ≈ 10k.
- **risk**: The checker contract may not deliver stable site keys, OUT scope-effect summaries or resolved static calls in the shape lowering needs, which delays integration. | **mitigation**: Agree on the LowerInput contract first (work item W2). Lowering recomputes flow modes and OUT nets itself, so it depends only on resolution, types, spans and site keys.
- **risk**: Hot-reload identity changes: node-id strings differ from today's, and row node ids still embed the dense ListId (runtime.rs:96), so rows reset when list numbering shifts. | **mitigation**: No external consumer uses node-id numbers (grep: only boon_document and GPU tests with synthetic ids). Site-key ids strictly improve retention. Removing ListId from row ids is part of v12.
- **risk**: Pulse fusion emitted as Ineligible could slow pulse-heavy examples such as fjordpulse. | **mitigation**: It is semantically safe (machine.rs:18730-18745). Port the eligibility rule as a small schedule classifier before cutover if the fjordpulse runtime gate regresses.
- **risk**: Decision 6 turns `title: LATEST { title, ... }` style self-references into errors, and the TodoMVC Theme refactor (decision 4) changes the example sources. Old-vs-new plan and persistence comparisons then diverge for reasons that are not lowering bugs. | **mitigation**: Run the differential oracle on the already-refactored examples compiled by both compilers where the old one still accepts them. Keep a per-example allow-list of expected divergences, each tied to a decision number.

## work_items
- **id**: LP1 | **title**: boon_plan trust and identity preparation | **description**: Delete SealedMachinePlan/seal_* from the in-process path. Add MachineTemplate::new_untrusted and make new_shared trusted (boon_runtime lib.rs:550, boon_distributed_runtime session.rs:88). Add load_untrusted(bytes) for decode_program_artifact, app bundles and server-received plans. Compute plan_digest once at artifact encode from the CBOR bytes, and remove the double hash in ContentArtifact::new. Move identity input structs into a frozen identity_v1 module with golden-hash tests. Optionally index templates by id in boon_document. | **size**: M | **acceptance**: Zero verify_plan and zero whole-plan SHA-256 calls on the playground compile, open and hot-reload path, confirmed with a counter or trace. The decode paths still verify. Golden identity tests pass. Existing runtime tests stay green.
- **id**: LP2 | **title**: Checker to lowering contract (LowerInput) | **description**: Define with the checker panel: HIR arena layout, TypeArena queries, resolution (static callee DefIds, params, locals, PASSED, OUT formals), per-definition signatures with OUT scope-effect summaries, spans, and stable 64-bit site keys with their structural-route definition. | **size**: S | **acceptance**: Written contract plus a Rust trait/struct stub that compiles in both crates. Site-key stability test: editing another item or reformatting keeps all keys; editing inside an item changes only keys on the changed routes.
- **id**: LP3 | **title**: Phase A analysis | **description**: Definition capability flags; root set; instance tree for stateful definitions; static owners for LIST row regions; static arm selection; per-definition slot ordinals with instance bases and compaction; OUT union-find with producer checks; instance-cell dependency graph with iterative Tarjan (positioned instantaneous-cycle diagnostics, topological order, pulse membership); publication paths, synthetic names, hidden keys and debug labels. | **size**: L | **depends_on**: LP2 | **acceptance**: On all examples: the set of state, list and source semantic paths equals the old compiler's, and MemoryId inputs are equal. Cycle and OUT negative fixtures give positioned diagnostics. Phase A takes ≤1.5 ms on TodoMVC and ≤4 ms on NovyWave (release, median of 30).
- **id**: LP4 | **title**: Dataflow emitter | **description**: FxHash compact-key interner for PlanRowExpressionNode and the plan constant pool; lower_value/lower_event (event normal form); HOLD, LATEST and implicit-state rules; derived values with the ported specialized forms; lists (slots, initial rows, append/remove, contextual collections, materialized views, row-field roles); effects and outbox; pulses (Ineligible); storage layout; PlanOp.inputs collection; op-id authored order. | **size**: XL | **depends_on**: LP3 | **acceptance**: verify_plan passes on every example plan. All .scn scenarios produce identical root texts on the unchanged runtime. Dataflow emission takes ≤3 ms on NovyWave.
- **id**: LP5 | **title**: Persistence and migration emission | **description**: Build MemoryPlan, ListMemoryPlan, CollectionMemoryPlan and PersistencePlan from Phase A paths plus the checker-type to DataTypePlan mapping; DRAIN/DRAINING to migration recipes and edges; predecessor catalog merge; persistence_only mode for predecessor stages; cross-revision identity memo. | **size**: L | **depends_on**: LP3 | **acceptance**: schema_hash, migration hashes, MemoryId, LeafId and type fingerprints are bit-identical to the old compiler on all examples except documented decision-5 cases. The examples/migrations sequences pass migration.scn.
- **id**: LP6 | **title**: Document emitter (v11, full inline, hash-consed) | **description**: Compact-key interner for DocumentExpr plus the document constant pool; parameter substitution; LocalBlock let-inlining heuristic; constant-selector folding; Constructor, template and node ids from mix64(site_key, path_key) with collision detection; element contexts; Select selector keys; ported value-class rules; materializations with one DocumentFunction each; RuntimeExpression fallback; demand collection; canonical initial patch batch. | **size**: L | **depends_on**: LP3, LP4 | **acceptance**: TodoMVC ≤12k document expressions, NovyWave ≤8k. Scenario DocumentFrames are structurally equal to the old compiler's (modulo node ids). Retained-binding and structural-rebuild counters are no worse. Document emission takes ≤5 ms on TodoMVC.
- **id**: LP7 | **title**: List access planning port | **description**: Port plan_typed_list_access and its helpers (machine_plan_backend.rs:9916-10760) as a rewrite over the interned row DAG, with type-table lookups instead of ErasedProgram/ValueIndex; view fingerprints through TypedListViewFingerprintContext. | **size**: M | **depends_on**: LP4 | **acceptance**: NovyWave list_indexes and list accesses are equivalent to the old plan after normalization (same key kinds, directions, selections); typed-list-index verify checks pass.
- **id**: LP8 | **title**: Assembly, outputs and debug map | **description**: MachinePlan assembly: outputs and host ports, demand, regions, activations, debug map with semantic labels, capability summary, application plan. No verify or hash in release; debug_assert verify_plan under cfg(debug_assertions) and in tests. | **size**: M | **depends_on**: LP4, LP5, LP6 | **acceptance**: The back half takes ≤8 ms on TodoMVC, ≤15 ms on NovyWave and ≤0.3 ms on counter (release, fresh process, median of 30).
- **id**: LP9 | **title**: Distributed linking on new per-role outputs | **description**: Port link_lowered_roles: declaration, endpoint, export, import and call-site ids; RemoteCallSitePlan argument DAGs and row bindings; ProducerFunctionInstancePlan ownership from instance slot ranges; wire schema and hash through boon_plan. | **size**: L | **depends_on**: LP8 | **acceptance**: Distributed example bundles verify and run their scenarios. Wire ids and wire_schema_hash match the old compiler where the role sources are unchanged.
- **id**: LP10 | **title**: Differential harness and plan normalizer | **description**: A test-only old-compiler oracle. Per example: verify_plan, persistence identity equality, source-path and label sets, scenario equivalence including normalized DocumentFrames, runtime work counters, and a readable normalized plan diff. Maintain the expected-divergence allow-list tied to decisions. | **size**: L | **depends_on**: LP4 | **acceptance**: The CI lane runs on all examples. Every divergence is either fixed or listed with a decision reference.
- **id**: LP11 | **title**: Cutover and deletion | **description**: Switch the playground, CLI, program_runtime, app_package and server to boon_lower. Delete boon_semantic, boon_verify, boon_ir, the old backends, boon_contract and the classifier registry. Update the xtask architecture gates, and replace the budgets and plan oracles with the new measurement lanes. | **size**: M | **depends_on**: LP8, LP9, LP10 | **acceptance**: The workspace builds without the deleted crates. The native GPU handoff gates pass. TodoMVC and NovyWave verified cold and warm budgets pass.
- **id**: LP12 | **title**: (Post-cutover, optional) MachinePlan v12 shared document bodies | **description**: Runtime instance frames (Call{site, instance}, Read::Instance{slot} resolved at the use site), per-site templates, row node ids without ListId, a unified expression arena with tagged constant operands, dead-field removal, a names table instead of debug_map, and a compact artifact encoding. | **size**: L | **depends_on**: LP11 | **acceptance**: TodoMVC document ≤4.5k nodes; emission work proportional to HIR size. Scenario and work-counter parity with v11. Measured runtime open time and memory are no worse.

## evidence
- I re-ran conservative hash-consing on today's plans (scratchpad/design/hashcons.py; identity-bearing constructor/materialize/call nodes kept unique, Select keyed by compiler_id): counter 123→79, TodoMVC 64,482→11,049, NovyWave 31,331→7,745 document expressions.
- Document op mix on the TodoMVC plan: constant 29,917 (439 distinct constants), project 9,994, record 9,174, select 6,163, read 5,995 (5,702 of them local reads), call 239, constructor 61. There are only 3,650 distinct compiler_ids. One Select site is duplicated 284 times (compiler_id 2125, the Theme dispatch).
- NovyWave plan: 1,444 templates and 1,442 constructors carrying an element_context (per invocation), 24 materializations, 7,251 distinct compiler_ids.
- Plan sections as compact JSON: TodoMVC total 9.17 MB, of which the document is 9.10 MB. NovyWave total 6.15 MB: document 4.90 MB, regions 635 KB, row_expressions 190 KB (2,071 nodes).
- Op counts. TodoMVC: 19 source routes, 7 derived, 28 state updates, 3 list mutations, 28 dependency edges; 11 scalar and 2 list slots. NovyWave: 121 routes, 378 derived, 878 updates, 9 mutations, 1 projection; 102 scalar and 24 list slots. All three fixtures have zero pulse batches.
- The executor ignores SourceRoute and DependencyEdge ops (machine.rs:5064) and topologically sorts update ops per trigger with an op-id tie-break (machine.rs:21575).
- Document runtime: only Static (runtime.rs:1856) and DynamicScalar (:3405, :3491) value classes affect behaviour. Select uses compiler_id as its key (:2007). Node ids are built from template.node plus the instance path (:3561, :5997). view_bindings and initial_patch_batch have no runtime reader.
- Hot-reload view state (focus, hover, scroll, caret) is kept only when node-id strings match (native playground runtime_view.rs:1433-1473). Today's template ids come from positional (owner, invocation, compiler_id) (document_executable_backend.rs:3723-3773).
- Persistence: MemoryId, schema_hash, type_fingerprint and migration ids are SHA-256 over the name-bearing serde encoding (boon_plan lib.rs:4937, 5300-5318, 14335, 14463, 14554; binary.rs:224-229, 345-350). The same schema_version with a different hash gives UnsupportedSource (boon_persistence migration.rs:281-291).
- Synthetic persistence names come from the kernel (`state_{ordinal}`, link.rs:7708-7713), from memory_contract.rs:715-740 (`@authority:` routes) and from core_lowering.rs:79-98 (hidden-key pluralization). TodoMVC shows them as `store.todos.state_0` and `Store.todoKey`.
- Stored program artifacts are rejected when COMPILER_ID differs (program_core.rs:420). MachineTemplate::new_shared re-verifies on every open and hot reload (machine.rs:9514; verify_all C4: ~92 ms on TodoMVC).
- The overlay rule forces per-invocation document specialization for any function that transitively contains a constructor (document_executable_backend.rs:3617-3700). The runtime evaluates call arguments eagerly (runtime.rs:1962-1976), so hidden-parameter body sharing would move state reads out of retained bindings.
- IR scale: TodoMVC has 4,536 executable expressions, 97 functions, 318 call occurrences and 23 owners. NovyWave has 16,340 expressions, 230 functions, 3,494 call occurrences, 546 owners, 1,433 fields and 591 bindings.
- Size of the code to delete: boon_semantic 96,971 lines, boon_verify 3,375, boon_ir 4,215, boon_contract 1,632, machine_plan_backend.rs 17,730, document_executable_backend.rs 4,221, dependency_classifier_schema_v1.toml 15,805.

## perf_targets
- **metric**: Back half (analysis + emission), cold, release, fresh process: TodoMVC | **target**: ≤8 ms | **basis**: Today semantic 161 + ir 3.3 + backend 88 + plan_validation 91 ≈ 343 ms. Model: ~65k inline visits × 50-100 ns plus linear passes over 4.5k HIR nodes.
- **metric**: Back half, cold: NovyWave | **target**: ≤15 ms | **basis**: Today 794 + 25.5 + 221 + 67 ≈ 1,108 ms. Model: 16.3k HIR nodes, 2,071 row nodes, ~32k document visits, ~500 identity hashes.
- **metric**: Phase A (diagnostics-lane lowering rules) | **target**: ≤1.5 ms TodoMVC, ≤4 ms NovyWave | **basis**: Must fit in the 16.7 ms warm edit-to-diagnostics budget alongside the checker.
- **metric**: Document expressions | **target**: ≤12k TodoMVC, ≤8k NovyWave (v11); ≤4.5k TodoMVC (v12) | **basis**: Measured hash-consing 11,049 / 7,745; 3,650 distinct compiler ids plus 439 constants for v12.
- **metric**: Compact plan JSON size | **target**: ≤2 MB TodoMVC (from 9.17 MB), ≤3 MB NovyWave (from 6.15 MB) | **basis**: The document is 99% of TodoMVC's plan; ~140 B per node.
- **metric**: In-process verify_plan / whole-plan hash calls per compile + open + hot reload | **target**: 0 | **basis**: Today 2 per open plus 1 per reload, ~92 ms each on TodoMVC (verify_all C4).
- **metric**: Warm verified preview, lowering share | **target**: ≤10% of the 100 ms budget with full re-lowering | **basis**: No incremental lowering needed at ≤8 / ≤15 ms.
- **metric**: Persistence identity parity | **target**: 100% of MemoryId/LeafId/schema_hash equal to the old compiler on all examples, except the documented decision-5 cases | **basis**: A different schema_hash at the same schema_version makes stored data unloadable.

## deletions
- **what**: boon_semantic: elaboration, receipts, manifests, six side graphs, CanonicalProgramCoreV2 | **size**: 96,971 lines
- **what**: boon_verify (contract-verified handoff) | **size**: 3,375 lines
- **what**: boon_ir (erase_and_lower, IR verifiers, verify_hidden_identity) | **size**: 4,215 lines
- **what**: boon_compiler machine_plan_backend.rs + document_executable_backend.rs + document_plan_backend.rs | **size**: ~21,955 lines
- **what**: boon_contract (CBOR+SHA-256 row identity), once no longer used | **size**: 1,632 lines
- **what**: SealedMachinePlan / seal_machine_plan / in-process verify_plan calls / whole-plan plan_sha256 / double ContentArtifactId hash | **size**: ~200 lines of code, ~90 ms per call removed
- **what**: docs/architecture/phase1/dependency_classifier_schema_v1.toml plus its xtask digest gate, and the xtask receipt-presence architecture checks | **size**: 15,805-line TOML plus gate code
- **what**: Per-call-site document overlay specialization and the inline_single_use pass (replaced by full inlining plus hash-consing) | **size**: part of document_executable_backend.rs
