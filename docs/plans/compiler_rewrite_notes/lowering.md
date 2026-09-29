> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

# Lowering and plan: typed HIR to MachinePlan (replaces boon_semantic, boon_verify, boon_ir, machine_plan_backend, document_executable_backend, distributed back half, and boon_plan sealing on the compile path)

## area
Lowering and plan: typed HIR to MachinePlan (replaces boon_semantic, boon_verify, boon_ir, machine_plan_backend, document_executable_backend, distributed back half, and boon_plan sealing on the compile path)

## summary
The runtime needs much less from a plan than the current back half produces. At load the executor rebuilds its own dependency indexes and sorts update ops itself (machine.rs:5064-5076), and it ignores SourceRoute and DependencyEdge ops. What it actually consumes is typed ops, storage, row expressions, the persistence plan and a document plan. On TodoMVC the plan is 99% document (9.1 of 9.2 MB compact JSON). That document has 64,482 expressions only because narrowing-driven per-call-site specialization copied Theme helper bodies up to 284 times; 61,691 of the nodes come from 1,663 IR expressions. I recommend emitting today's MachinePlan data model as a small, explicit V12 delta rather than a new format, for three reasons. V11 already has shared DocumentFunction/Call. The executor (37.9k lines) and the document runtime encode subtle semantics that the 55 scenarios and the old compiler can then test differentially. And a new format would save at most a few ms of compile time. A new crate, boon_lower, goes from typed HIR through one hash-consed RIR arena to MachinePlan: demand/classification; lowering with an instance tree only for functions that own runtime identity (one instance per static call path, with list-row regions as static owners), OUT nets by union-find and PASS as static environments; linear analyses (one SCC/topological pass, event-cause sets, update arms, derived values, storage, persistence, list access, pulse batches); and direct emission with no SHA interning, no verify_plan and no plan hash. Pure functions get one shared document body (inlined only when single-use or tiny, always inlined into row expressions). That cuts the document to about 3k expressions on TodoMVC and 6k on NovyWave, and NovyWave's 1,444 templates to about 377. Element-state reads do not force per-instance copies, because the document runtime resolves them against the enclosing constructor. UI node identity for hot reload moves from dense compiler ids to stable source anchors, which needs one runtime line (the call fragment uses a stable site key). Persisted identities (MemoryId, leaf ids, type fingerprints, schema_hash, migration and effect ids) must stay stable across compiler versions, because a stored image with the same schema_version but a different schema_hash is rejected (migration.rs:285-293). Today they are SHA-256 over a serde encoding that bakes in Rust struct, enum and variant names, so the new code freezes an explicit encoder pinned by golden vectors. The durable schema today also contains compiler-made leaves: derived row values, event payloads and ordinal-named anonymous states. Whether to reproduce them byte-for-byte or declare one persistence epoch is the main owner question. Lowering is total on checked programs: every rule the back half enforces today moves to the checker. That includes recursion, anonymous persisted state, closed persisted types, OUT drivers and cycles. Estimated back-half cost is about 1.5-4 ms on TodoMVC and 6-13 ms on NovyWave, against 336 and 1,085 ms today. So a full re-lowering on every keystroke fits the warm budget with no incremental lowering, and swapping in a plan at runtime stops re-running verify_plan + plan_sha256 (boon_runtime lib.rs:550 -> machine.rs:9514).

## design
## 0. Verdict on format

**Emit today's MachinePlan data model first, as PLAN_MAJOR_VERSION 12 with a short, explicit delta (section 16). Do not design a new runtime format now.**

Why:
1. **The cost is not in the format.** TodoMVC plan: document 9,104,205 of ~9.2 MB compact JSON. Everything else is tiny: 73 row expressions, 19 source routes, 28 state updates, 11 scalar slots, 2 list slots, 10 memories. The document is 64,482 expressions because each call site got its own body, not because V11 lacks sharing. V11 already has `DocumentExprOp::Call` + `DocumentFunction` (boon_plan/src/document.rs:170-178, 482-487). Grouping by compiler id: 61,691 of 64,482 nodes come from 1,663 IR expressions duplicated 5 or more times. The top ones are Theme `function_parameter`/`when` nodes copied 284x. Distinct non-constant compiler ids + distinct constants = 2,602 (TodoMVC) and 5,318 (NovyWave). That is the shared-body size in the *current* format.
2. **The runtime surface is large and semantically dense.** Executor machine.rs is 37,874 lines; boon_document runtime.rs is 7,086. They encode authority/value row roles, currentness, list reconciliation, cursors and FLUSH. Rewriting them together with the compiler removes the only strong oracle (same-format plans plus 55 `.scn` scenarios plus old-vs-new plan comparison).
3. **A new format gains little compile time.** A unified DAG or compact encoding would save roughly 1-3 ms. It should be justified later by *runtime activation* measurements, not by compile time.

## 1. What the runtime actually consumes (the contract)

| Consumer | Needs from the plan | Evidence |
| --- | --- | --- |
| MachineTemplate / Metadata::new | ops by kind (DerivedValue, StateUpdate, EffectUpdate, ListMutation, ListProjection); storage slots; list slots with row fields and roles; list_dataflow; list_indexes; source_routes; pulse_batches; persistence memory -> slot maps; constants; debug_map labels. It rebuilds dependencies itself (`build_dependencies`) and sorts update ops itself (`sort_update_ops_by_dependencies`). | machine.rs:4405-4474, 5040-5076, 5506 |
| Ignored by executor | `PlanOpKind::SourceRoute`, `PlanOpKind::DependencyEdge` (878 dead ops on NovyWave) | machine.rs:5064 (no-op arm); only boon_plan verifier/capability code matches them |
| Tie-break order | mutations sorted by op id; pending list mutations by `ordinal` | machine.rs:5076, 33877 |
| Name lookup (hidden contract) | `debug_map.{fields,state_slots,list_slots}` labels (semantic paths, ids formatted `field:N` etc.) build root_field_by_name / root_state_by_name / field_name_index | machine.rs:4704-4707, 4785-4800; runtime.rs:5822-5835 |
| Document runtime | expressions dense (`expressions[id.0]`); templates (only `.node` is read, runtime.rs:3384-3392); Select's `compiler_id` as the matched-selector key (runtime.rs:2007); value_class for static cache and retained scalar bindings (runtime.rs:1702, 1856-1877, 3490-3494) | |
| Node identity | `node:{template.node}:{instance path}` built from `root-{template}`, `call-{function}-{expression}`, `node-{template}`, `materialize-{id}-..`, `/row-{list}-{key}-{gen}` | runtime.rs:1796-1800, 1975, 3489, 3234, 3562-3572, 5999-6000 |
| Hot reload | new plan: runtime rebuilt from the durable image; frame fully replaced (lib.rs:3292-3309); playground keeps hovered/pressed/focused/scroll/text-input state **by node-id string** only if the id still exists | runtime_view.rs:1433-1445 |
| Persistence | adopt image iff schema_version and schema_hash are equal (persistent.rs:760-763); same version with a different hash -> `UnsupportedSource` (migration.rs:285-293); data keyed by MemoryId / MemoryLeafId / DurableRowId; migrations chain on source_schema_hash | boon_persistence lib.rs:285-296; migration.rs:139-330 |
| Program artifacts | compiler-bound: a stored artifact with a different `compiler_id` is rejected and recompiled | program_core.rs:420-428 |
| Child-program profiles | `capability_summary` flags | program_host.rs:306-356 (replace with `derive_capability_summary(plan)`, lib.rs:14572) |
| Distributed | RemoteCallSiteId = H(graph, endpoint, DistributedDeclarationId(role ns, `call:{stable_owner_path}:{fn}:{ordinal}`)); wire_schema_hash equality between endpoints | distributed_compiler.rs:2300-2326; lib.rs:5207-5226; client.rs:26-193 |
| Page cursors | `view_fingerprint` sealed into cursor tokens and compared on use | cursor.rs:165-175; machine.rs:29980 |

Today every runtime activation from `Arc<MachinePlan>` re-verifies and re-hashes the plan. The path is `LiveRuntime::from_shared_machine_plan_with_restore` (boon_runtime lib.rs:545-551) -> `MachineTemplate::new_shared` (machine.rs:9514-9517) -> `seal_shared_machine_plan` -> `verify_plan` -> `plan_sha256`. The persistent playground path uses it (persistent.rs:763). That is about 90 ms per plan swap on TodoMVC, on top of compile time.

## 2. Identity stability classes

**A. Must be stable across compiler versions (persisted user data).** These are functions of source-level names and canonical types only:
- `ApplicationIdentity` and `identity_hash` (lib.rs:246-305)
- `MemoryId = H(MemoryIdentityInput{canonical_module, named_owner_path, semantic_memory_path, memory_kind})` (lib.rs:4935-4941, 5300-5313)
- `MemoryLeafId = H(memory_id, semantic_leaf_path)`
- `type_fingerprint = H(DataTypePlan::canonicalized())` (lib.rs:14335); fields and variants are sorted, so field order is not part of identity (consistent with decision 7)
- `schema_hash = H(CanonicalPersistenceSchema{application, format_version, schema_version, memory[], lists[], collections[], effect_outbox})` (lib.rs:14463-14500). It includes `hidden_key_type` strings such as `Store.todoKey`, a pure string function of the list path (core_lowering.rs:79-98), plus `initial_provenance` and `has_generation`.
- `MigrationRecipeId`, `MigrationEdgeId`, recipe and catalog hashes
- `EffectId(host_operation)`, `EffectInvocationId(effect_id, target_path)` -> OutboxItemId (boon_persistence lib.rs:86-116)
- DurableRowId and DurableCollectionId composition
- ContentArtifactId (SHA-256 of user blobs)

**Encoding hazard.** `canonical_sha256` hashes `binary::encode`, which writes struct names, newtype names, enum names and variant names (binary.rs:142-160, 224-239) and depends on field declaration order. Renaming a Rust struct silently changes every MemoryId and schema_hash. New rule: `boon_plan::persist_id` is a hand-written encoder that emits **the same bytes** as today, pinned by golden vectors taken from all example PersistencePlans. From then on, persisted identity is decoupled from Rust type names.

**B. Deterministic within one compile, across roles (not across versions).** RemoteCallSiteId, ExportId, ImportId, DistributedGraphId, EndpointId, DistributedArgumentId, wire_schema_hash. A mismatch between deploys is already rejected by the wire hash.

**C. Best effort across edits in one session (UI continuity).** DocumentNode ids: template, call-site and materialization anchors.

**D. No stability needed.** All dense ids (StateId, FieldId, ListId, SourceId, ScopeId, PlanStaticOwnerId, PlanStorageId, PlanOpId, DocumentExprId, DocumentFunctionId); plan digest; program artifact bytes. The page-cursor view_fingerprint is soft: tokens fail closed after a recompile.

## 3. Pipeline

```
TypedProgram (checker; borrowed, immutable)
  L1 demand + classification       O(defs + call edges)
  L2 lower to RIR (one recursive walk; instances, static owners, OUT UF, PASS env, inlining policy)
  L3 analyses on RIR side tables   (one iterative Tarjan/topo pass + 5 linear passes)
  L4 emit MachinePlan V12 -> TrustedPlan (Arc)
```
Everything that exists today between checked types and MachinePlan disappears from the compile path: the 7 semantic graphs, receipts, manifests, CanonicalProgramCoreV2, ErasedProgram, boon_verify and the boon_checked Type trees. There are 3 representations: HIR, RIR plus side tables, and the plan.

## 4. Input contract from the checker

```rust
pub struct TypedProgram {
  defs: IndexVec<DefId, Def>,          // FUNCTIONs + root values at field-path granularity (C9)
  nodes: IndexVec<NodeId, HNode>,      // op:u8, flags, ty: TypeId, span, children range, payload
  children: Vec<NodeId>,
  types: TypeStore,                    // hash-consed, order-free identity, display-order side table
  symbols: Interner,                   // SymbolId
  res: Vec<Res>,                       // per node: Local(binder) | Def | Param(i) | Passed(path) | OutBinder | Builtin
  calls: FxHashMap<NodeId, CallInfo>,  // callee, args in declaration order, OUT fresh/forwarded entries, PASS clause
  schemes: IndexVec<DefId, Scheme>,    // generalized type, PASSED requirement row, OUT formals
  inst: FxHashMap<NodeId, SubstId>,    // type args per call to a generic def (needed only at storage/persistence/wire boundaries)
  def_order: Vec<DefId>,               // function DAG topo order + value SCC groups
  def_paths: IndexVec<DefId, (ModuleSym, PathSym)>,  // "$root" / module, "store.todos"
}
```
Lowering never re-infers types. It needs concrete types only for storage slots (PlanValueType), persistence (DataTypePlan), effects and the wire. It gets them by applying an instance's substitution to a node's TypeId, memoized per (TypeId, SubstId). There are no per-expression type trees.

## 5. RIR: one hash-consed arena

```rust
#[repr(C)] struct RNode { op: ROp /*u8*/, flags: u8, _pad: u16, ty: TypeId, a: u32, b: u32, c: u32 } // 20 B
struct Rir { nodes: Vec<RNode>, extra: Vec<u32>, owner: Vec<OwnerId>, inst: Vec<InstId>,
             anchor: Vec<u32> /*HIR NodeId for spans/debug*/, cons: hashbrown::HashTable<RId>, consts: ConstPool }
// operand = u32; high bit set = ConstId (no Constant nodes in RIR)
```
- **Hash-consing.** Key = FxHash of (op, flags, ty, a, b, c, extra slice). The table stores only the RId and compares against the arena on a hit, so no keys are duplicated.
- **Identity-bearing ops are never consed.** These are Hold, Latest, Source, ListAuthority, Stream/pulses, doc Constructor, doc Call site, Materialize, and doc Select (its selector key is read by `Matched`). Their children are consed.
- **ConstPool.** An FxHashMap keyed by canonical value (Text / Number (exact rational) / Tag / Bytes / Bits). At emission it produces 1 `PlanConstant` or `DocumentConstant` per distinct value (439 for TodoMVC, against 29,917 Constant nodes today).
- **Names** use SymbolId throughout. Strings are materialized only at emission.

Ops (sketch): Const, Param(i) (document functions only), Local(binder), RowLocal(owner, local), Record, Tagged, Project(sym), Text(segments), Infix, Builtin(id, args), When(arms), While(arms), Then, Latest, Hold(state), Skip, Flush, FlushBoundary, CatchCycle, SourceRead(src), SourcePayload, StateRead(state), FieldRead(field), ListRef/AuthorityListRef, ListOp{kind, source, owner, row_local, body, captures}, Order, Page, Pulses, EffectCall, Drain(leaf), DocCall{fn, site}, Constructor{template, ctx}, Materialize{id}, ElementState(ctx, proj), NoElement.

## 6. L1: demand and classification

Bottom-up over the function DAG (recursion is a checker error), compute bits per function:
- `owns_identity`: HOLD, LATEST, SOURCE, LIST authority literal, Stream/pulses, host effect, DRAIN, Bool/toggle-style stateful builtins
- `has_out`: has OUT formals
- `reads_passed`
- `renders`: the result type contains elements

Element-state reads (`element.hovered`) are **not** identity: the runtime checks them against `env.element_context` set by the enclosing Constructor (runtime.rs:2357-2374, 3394-3395).

Roots, in source order:
- output roots (document, scene, host values)
- every root-level identity-owning value. Today an unread root HOLD is still persisted (probe counter_unused.bn), so keep that behaviour.
- effect-bearing roots
- distributed exports

Derived root fields are demanded lazily when read. The result is `RootOutputDemand::Selected(sorted)`; it must be sorted because document.rs:638 uses binary_search.

## 7. L2: instances and static owners (fused into the lowering walk)

```rust
struct Instance { def: DefId, parent: InstId, site: SiteKey, owner: OwnerId, passed: PassedEnvId, subst: SubstId, key: u64 }
struct StaticOwner { parent: OwnerId, kind: OwnerKind /* RowRegion{list, scope} | ActivationArm{activation} */, anchor: u64 }
struct Env<'a> { inst: InstId, owner: OwnerId, passed: PassedEnvId, params: &'a [Operand], locals: SmallStack<(BinderId, Operand)>, row: Option<(OwnerId, LocalId)> }
fn lower(&mut self, n: NodeId, env: &Env) -> Operand   // memo per (template context) via Vec<RId> indexed by NodeId
```
- **Call to an `owns_identity` function** creates an Instance: one per static call path, so two wrapper call sites stay distinct (per the OUT plan's "State, Effects, And Lifecycle" section). Its states, sources, lists and effects get fresh dense ids in DFS order. The instance key is a fold of SiteKeys along the path.
- **Row regions** (List/map `new:`, retain/remove predicates, sort keys that contain state) become **static owners**, not instances. State inside them becomes indexed row state (one StateId, per-row storage at runtime). This gives today's forest shape (TodoMVC 23 owners).
- **WHILE arms** holding state become activation owners (`PlanStateLifetime::ActivationLocal`, `PlanActivation`).
- **Pure functions** create no instance.
- Probe: a FUNCTION returning `[count: 0 |> HOLD n {...}]` used at a root field crashes today's semantic phase (design/lowering/hold3.bn, hold5.bn). Root-level per-call-site instances therefore have no working oracle; the new rule above defines them.

## 8. OUT nets (union-find)

Every fresh call-entry binder, every wrapper OUT formal and every builtin row binder (`item`, `entry`) is a UF element. Forwarding (`item: entry`) unions them. OUT wrappers are always **inlined** at the call site: their `new:` argument runs inside the callee's row region, and the language requires "no residual wrapper invocation" (LANGUAGE_SEMANTICS.md:173-176, 226-236). Each class resolves to exactly one driver, a builtin row region `(owner, row_local)`, and every OUT read lowers to `RowLocal(driver)`. Driver-count and cycle rules are checker diagnostics; lowering only debug_asserts them.

## 9. PASS / PASSED

PASS is a compile-time context, not a value (LANGUAGE_SEMANTICS.md:178-194). A PASS clause lowers its record to RIR and interns a `PassedEnvId`, a persistent chain of (sym -> Operand). `PASSED.a.b` resolves statically by projecting the nearest env record, so no broad store record is evaluated at runtime. Functions that read PASSED are specialized per (DefId, PassedEnvId). That is typically 1-2 variants: C9 found each function sees one PASSED context type.

## 10. Call lowering policy

| Callee | Dataflow (row expressions) | Document |
| --- | --- | --- |
| builtin | intrinsic RIR op, row region if it has OUT | same |
| OUT wrapper | inline | inline |
| identity-owning | per-Instance body (ops + storage per instance) | per-Instance body |
| pure | inline (PlanRowExpressionNode has no Call; hash-consing bounds growth) | **shared DocumentFunction** per (DefId, PassedEnvId), unless single reachable call site, body of 8 nodes or fewer, or an argument is a root/record path whose runtime evaluation would read more than the body's projections. In those cases inline by RIR substitution. |

The decision is made *before* emission from the static call graph. There is no compile-then-inline pass (today 1,669 of 1,718 variants are inlined back; document_executable_backend.rs:460).

## 11. L3: analyses (side tables keyed by RId, slot or id; no graphs with their own id spaces)

1. **One iterative Tarjan over "instant" edges.** Reads of HOLD/LATEST state, SOURCE, pulse and effect completion are cut. Outputs:
   - (a) debug_assert that no non-trivial SCC exists; the checker owns the "instantaneous cycle" diagnostic. If one is found, it is an ICE with spans.
   - (b) reverse-topological rank, used for deterministic op id assignment and derived startup order.
   - (c) pulse batches: the forward instant closure from each `Pulses` node inside its activation owner gives `state_update_ops` / `derived_ops` / `list_mutation_ops`. `count` = the Pulses operand; `emission_routes` come from Stream/skip.
2. **Event causes.** One interned sorted set per node (`FxHashMap<Box<[u32]>, SetId>`; a u32 per node). Source -> {src}; state change -> {state}; Then, When and Latest propagate. The sets give StateUpdate triggers (`ValueRef::Source/State/Pulse`), SourceEventTransform arms and EffectUpdate triggers.
3. **State update arms.** Decompose each HOLD/LATEST body into (trigger, gate, value) arms in LATEST branch source order. Op ids follow that order, because the executor tie-breaks by op id (machine.rs:5076).
4. **Derived values.** A named root or row field that is read across owners (by the document, an effect, an output, another owner or a list op) and is not constant becomes a FieldId plus a DerivedValue op. Kinds: SourceEventTransform (event-only, default value), ListView, Aggregate, Pure. The `startup_recompute` rule is ported. Op inputs come from `PlanOp::synchronize_expression_inputs` semantics (lib.rs:6196-6270), computed once at creation.
5. **Storage.** One scalar slot per state (root/instance -> persistent scalar; row owner -> indexed with `indexed_field_id`; activation -> ActivationLocal). One list slot per list authority and per materialized derived list; row fields carry roles Value/Authority/ValueAuthority/Capture. Byte banks for fixed-length BYTES. Initializers are constant or row expression. PlanValueType maps from TypeId (Text, Number, tag-only union -> Tag, Bytes{fixed}, Bits, otherwise Data).
   - The row-field-role, list activation mode (MaterializedAuthority / DerivedDefault), value_list_authorities, authority_source_list and row_field_copies decisions are **ported as pure functions** of RIR side tables from machine_plan_backend (ScalarFieldCatalog, materialized_output_fields). This time the lookups are O(1): a bitset over FieldId plus an FxHashMap<(ListId, SymbolId), FieldId>. Today's version is 57% of the NovyWave backend (a_backend).
   - Validation is structural equality with old plans after id renumbering, plus all scenarios.
6. **Source routes.** Owner (with ancestor rows), path string (the scenario/host contract, e.g. `store.todos.sources.todo_checkbox.events.click`), scope_id and row_projections for row sources, interval_ms, payload schema.
7. **Effects.** EffectContract comes from a process-lifetime `OnceLock` table built from `builtin_effect_contract`. Outbox schemas are sorted by effect_id.

## 12. Persistence

- **Durable schema rule (to be confirmed, owner Q1).**
  - HOLD, and LATEST that holds memory, published at a named data path: root scalar, or `<list path>.<row field>` indexed with owner = list path (today: `store.todos.title`).
  - Anonymous stateful nodes use `<nearest named path>.state_<k>`, k = source pre-order ordinal among anonymous stateful nodes under that named path (today: `store.todos.state_0`).
  - LIST authorities with the row seed fields that are read.
  - MAP/SET authorities under a named field.
  - State with no named data path stays a checker error, as today (machine_plan_backend.rs:2276; memory_contract.rs:559).
- **Identities** come from `persist_id` (byte-identical to today). `DataTypePlan` and `type_fingerprint` are memoized per TypeId, about 600 SHA-256 calls over <300 B inputs on NovyWave (~1 ms).
- **Migrations.**
  - `DRAIN { path }` resolves against the predecessor `PersistencePlan` (MigrationPredecessorBinding, lib.rs:4319) to MemoryLeafIds.
  - The transform lowers to `MigrationExpressionPlan` (a pure subset; `migration_call_is_supported`, lib.rs:4774).
  - `|> DRAINING` marks draining memory.
  - Predecessor persistence plans are cached by stage source digest, so persons_pro stops recompiling every stage per keystroke.
- **Oracle.** PersistencePlan equality against the old compiler on every example (or the documented epoch rules if the owner picks option B).

## 13. List access plans

Pattern pass over RIR list pipelines on authority lists:
- `ListOp{Filter|Retain|Find|Every|Any}` predicates split into conjuncts/disjuncts of `row.key OP expr_independent_of_row`, with OP in {==, Text/starts_with, <, <=, >, >=}. They map to `PlanListAccessSelection` KeyPrefix / TextPrefix / ComponentRange / Union / Intersection.
- `Order{SortBy, ThenBy}` keys and take/page limits produce `PlanListIndex(source_list, keys)`, deduplicated by (list, key RIds, kind, direction). Hash-consed RIds make that dedup exact.
- Target-profile limits apply (8 keys, 16 expanded keys per row, depth 8, 64 leaves; lib.rs:6374-6380). Over the limit means no index, not an error.
- ClosedTag keys use `closed_tag_type_id`.
- `view_fingerprint` uses boon_plan's existing TypedListViewFingerprintContext (small canonical inputs).

## 14. Document plan

- **Functions.** One DocumentFunction per shared (DefId, PassedEnvId). Parameters are `fn*STRIDE+i` as today. Bodies are emitted once, post-order from the root, with a memo `Vec<u32>` from RId to DocumentExprId.
- **Hash-consing.** Inherited from RIR. Constructor, Call, Select and Materialize nodes are unique per site. Two equal Calls under one parent would otherwise produce the same `call-..` fragment and colliding node ids.
- **Value class.** Render / ChildList come from the type. For others, a staticness bit: Static only if the node is independent of state, fields, lists, sources, rows, dynamic locals, `Matched`, `ElementState` and dynamic params. A param's staticness is the AND over all reachable call sites of that function, computed once in call-graph order. This keeps `static_value_cache` sound for shared bodies (runtime.rs:1856-1877).
- **Stable anchors** (xxh3-64 with a fixed seed from a pinned crate, never FxHash):
  - DefKey = H(module, def path)
  - ConstructorAnchor = H(DefKey, constructor pre-order ordinal in the def)
  - SiteKey = H(caller DefKey, callee symbol, ordinal among calls to that callee in the caller)
  - Template id and template.node = ConstructorAnchor for shared bodies; H(anchor, Instance.key) for per-instance bodies
  - Materialization id = H(List/map site anchor, instance)
  - Root template/node = H(output name)
  - Collisions are checked with a FxHashSet at emission and salted deterministically.
  - Result: node ids survive edits outside the edited definition. Today they come from dense executable ids (document_executable_backend.rs:3723-3760, 2099-2110), so any earlier insertion shifts them.
- **Element contexts.** `DocumentElementContextId{call_instance: anchor, ordinal}` is static per constructor anchor. It is valid for shared bodies because it is checked against `env.element_context`.
- **Rest.** Materializations (row identity, template function, VisibleRange), view bindings (template, attribute, target), and `initial_patch_batch` via `DocumentPlan::build_initial_patch_batch` (document.rs:520-567).
- **Expected size.** TodoMVC ~3k expressions (2,602 + calls), 57 templates. NovyWave ~6k expressions (5,318 + ~1k calls), ~377 templates (from 1,444). Constructors copied up to 80x today exist only because element-state bodies were specialized per invocation.

## 15. Distributed

Each role is lowered by the same code. A link step builds exports, imports, remote call sites and producer function instances, then the wire schema and `wire_schema_hash`.
- `stable_owner_path` is the static owner path string without the row suffix (distributed_compiler.rs:2300-2305).
- The ordinal is the source pre-order index among calls to that function under that owner path and role.
- Synthetic invocation SourceIds are allocated after the role's own sources.
- Replace the legacy boon_typecheck fixed point (at least 12 role checks per keystroke) with interface slots from the new checker. That part belongs to the checker designer.

## 16. V12 delta (exact list)

1. `DocumentExprOp::Call { function, site: u64, arguments }`. runtime.rs:1975 formats `call-{site:016x}` (one line).
2. `DocumentExprOp::Select { input, arms, selector: u32 }`. Delete `DocumentExpr.compiler_id`; runtime.rs:2007 reads the op. Delete `DocumentTemplate.compiler_expr_id/owner_function` and `DocumentMaterialization.compiler_expr_id` (the runtime never reads them).
3. Template, materialization and root ids become stable anchors. No runtime change; they are u64 already.
4. Drop `SourceRoute` and `DependencyEdge` ops and their regions, `PlanOp.unresolved_executable_ref_count`, `DocumentPlan.unresolved_op_count`, `MachinePlan.{capability_summary, dirty_plan, commit_plan}` (derive on demand) and `PlanPulseBatch.semantic_slice_digest`.
5. `PlanRowExpressionArena::from_trusted_nodes(Vec<_>)`: no SHA structural index (today every intern SHA-256s, lib.rs:8985-9003). Build an FxHash index lazily only if something interns later.
6. `SealedMachinePlan` becomes `TrustedPlan { plan: Arc<MachinePlan>, identity: OnceLock<PlanIdentity> }`:
   - `from_compiler(plan)`: `debug_assert!(verify_plan(&plan).is_ok())`
   - `from_untrusted(plan) -> Result`: runs verify_plan
   - `identity()`: lazy blake3 over a compact name-free encoding
   - `MachineTemplate::new(TrustedPlan)`
   - `LiveRuntime::from_trusted(..)`
   - The `Arc<MachinePlan>` entry point is renamed `from_untrusted`.
7. `boon_plan::persist_id`: explicit encoder with golden vectors (section 2A).
8. Bump PLAN_MAJOR_VERSION to 12 and COMPILER_ID, so program artifacts recompile.

Later, and only if runtime activation measurements justify it (V13): one expression arena for row and document, frame-relative document functions, compact binary, and node-id row fragments keyed by list MemoryId instead of the dense ListId.

## 17. Invariants the lowering must reproduce

**Structural.** In-process these hold by construction; `verify_plan` checks them for untrusted plans and in debug/CI.
1. Dense unique ids: op ids, storage ids (one space: scalar, then list, then byte banks), `expressions[i].id == i`, view binding ids; unique template, function, materialization and parameter ids.
2. Every ValueRef resolves. Op inputs are exactly the ValueRefs its expressions read, canonically sorted.
3. Source routes: typed ids, structural owners matching the owner forest; scoped routes carry scope_id and row_projections; `interval_ms > 0`.
4. Storage: exactly one scalar slot per state; indexed iff inside a row owner; ActivationLocal refers to an activation that lists the state; value types agree with constant initializers; byte banks match fixed bytes.
5. List slots: every row field has exactly one value owner and one authority owner; initial rows and range bounds resolve.
6. Constants are deduplicated. Byte constants carry sha256 and inline bytes up to 1024.
7. Row expressions: contextual locals only inside their region; list fields resolve; builtin signatures match (lib.rs:12255).
8. Index resource limits hold.
9. `RootOutputDemand::Selected` is sorted, and document reads of root fields are inside it.
10. Outputs: `OutputRootId = H(name)`; contracts match the program role; session-info intrinsics are legal for the role.
11. Effects are canonical and the outbox is sorted.
12. Pulse batches reference existing ops and activations.
13. Document: canonical initial patch batch; root in bounds; no unresolved ops.
14. Persistence: entries sorted by memory_id, leaves by leaf_id; identities and fingerprints recomputable; `runtime_slot` and `runtime_field_id` consistent; list authority fields have leaves; migration catalog acyclic with a source_schema_hash chain.
15. Distributed: endpoint role equals plan role; wire hash equals H(wire schema); remote call sites have owners.

**Behavioural.**
- Snapshot turn semantics.
- Op id order equals source order for same-state arms and same-list mutations; mutation `ordinal` is the pipeline order.
- SourceEventTransform defaults; startup_recompute.
- FLUSH -> FlushBoundary at the owning state activation; CatchCycle rules ported.
- Row identity is hidden key plus generation. value_list_authorities give row identity and state that survive filtering. EffectResult appears only inside its owning update.

**Naming contract.** debug_map labels are semantic paths in the `field:N` / `state:N` / `list:N` / `source:N` formats. The source route `path` strings are what scenarios and host routing use.

**Document identity.** Node-id grammar unchanged except the call fragment. Identity-bearing nodes are never shared. Static classification is sound.

**Persistence.** Section 2A, bit-exact through `persist_id`.

## 18. Diagnostics contract: lowering is total

Lowering returns `Result<TrustedPlan, Ice>`; an Ice is a compiler bug with spans, never a user diagnostic. So the diagnostics lane never needs lowering. These rules must move into the checker as positioned diagnostics (decision 6):
- recursion (today only at machine_plan_backend.rs:14051)
- persisted state without a named data path (machine_plan_backend.rs:2276)
- collection authority not under a named field (memory_contract.rs:559)
- non-closed persisted types (memory_contract.rs:1016)
- duplicate memory identity
- OUT zero, multiple or cyclic drivers
- instantaneous and field self-reference cycles
- non-closed distributed boundary types (distributed_compiler.rs:2217)
- storage-level "no exact field" / "no compatible WHEN arm" failures, which become type errors (C10 u02, u05, x04)

## 19. Warm path

Full re-lowering per revision (section 20: 1.5-13 ms) runs on the preview lane after diagnostics are published, with cancellation polled per root definition. Retained across revisions: the process-lifetime effect-contract table, TypeId -> DataTypePlan/fingerprint memo (valid if the checker's TypeStore is append-only), and predecessor migration plans by stage digest. No per-definition incremental lowering in v1: the budget does not need it. The natural later cache is RIR per (DefKey, instance context), keyed by content hash plus callee interface epochs.

## 20. Performance model and size estimate

| stage | TodoMVC | NovyWave | basis |
| --- | --- | --- | --- |
| L1 demand/classify | ~0.05 ms | ~0.2 ms | 121 / 505 functions, 219 / 881 syntactic calls (grep) |
| L2 lower to RIR | 0.3-0.8 | 1.5-3 | ~5k / ~20k nodes x 60-150 ns (arena push + FxHash cons) |
| L3 analyses | 0.3-1 | 2-4 | SCC + 5 linear passes; 11/102 states, 126/1,433 fields, 28/878 arms |
| persistence ids | <0.1 | 0.5-1 | ~25 / ~600 SHA-256 of <300 B |
| L4 emit | 0.5-1.5 | 2-5 | ~3k / ~6k doc nodes, 73 / 2,071 row nodes, 38 / ~1,266 ops |
| **total** | **~1.5-4 ms** | **~6-13 ms** | today semantic+IR+backend+plan_validation = 158+3+85+90 = 336 ms and 780+25+214+66 = 1,085 ms |

Plan size (compact JSON):
- TodoMVC: 9.2 MB -> ~0.5 MB (document 64,482 -> ~3k expressions; ops 85 -> 38)
- NovyWave: 6.4 MB -> ~2 MB (document 31,331 -> ~6k; templates 1,444 -> ~377; ops 2,265 -> ~1,266)

Lowering peak memory is about 5-10 MB on NovyWave: RIR 20 B/node plus side tables plus the plan. Code size: boon_lower ~20k lines, replacing ~136k (boon_semantic 97k, boon_ir 4.2k, boon_verify 3.4k, boon_checked 8.1k, backends plus distributed back half 25.7k), most of which is proofs and graph plumbing.

## 21. Migration and oracle strategy

1. Old vs new on all 142 `.bn` examples, with a plan canonicalizer that renumbers dense ids and sorts. It compares storage_layout, regions, source routes, row-expression DAGs, persistence and debug_map, and expects the documented diffs (sharing, anchors, dropped fields).
2. All 55 scenarios run on new plans.
3. PersistencePlan byte-identical (or the epoch rules).
4. Document frames equal modulo node-id renaming.
5. Counters: RIR nodes, document expressions, templates, SHA-256 calls, lowering µs.

The old compiler stays test-only and is deleted at cutover.

## owner_questions
- **question**: Persistence continuity at cutover. Should the new compiler reproduce today's durable schema byte-for-byte, or declare one persistence epoch with a schema defined only from source semantics? | **options**: (A) Byte-identical PersistencePlan, oracle-tested against the old compiler on every example. Existing local data keeps loading, but the new compiler must port undocumented backend storage-role logic, including derived leaves such as store.todos.@authority:not_completed (= completed |> Bool/not()), event payload leaves such as @authority:sources/.../change/text, and @authority:title_to_update. (B) One epoch: bump PERSISTENCE_FORMAT_VERSION. Durable = HOLD/LATEST states at named data paths (anonymous ones as <named path>.state_<k> by source pre-order), LIST row seeds, MAP/SET authorities. Nothing derived or transient. Existing local data needs Start Over. Either way the identity encoding is frozen with golden vectors afterwards. | **recommendation**: B. Nobody uses the compiler in production. A would carry accidental backend decisions (persisting recomputable values and event payloads) into the new compiler forever. | **why**: schema_hash covers every leaf. A stored image with the same schema_version but a different schema_hash is rejected outright (boon_persistence migration.rs:285-293), so this choice decides whether existing playground data survives. It is also a question of what 'persisted' means, which may count as semantics.
- **question**: Should lowering prune WHEN/WHILE arms whose selector is a compile-time constant? Today two static-selector evaluators do this (out_net.rs:1924, contextual_expansion.rs:9030), and pruning removes the HOLD/SOURCE/LATEST inside the pruned arms. | **options**: (A) No semantic pruning: every arm is lowered and exists, and constant folding may only skip runtime evaluation of pure nodes. (B) Prune constant selectors, which changes which states exist and so the durable schema. | **recommendation**: A | **why**: With narrowing dropped, types no longer depend on values (decision 4). Pruning would make the instance tree and persistence schema depend on values again, and it needs a spec and a single implementation.
- **question**: Is best-effort UI node identity across hot reload acceptable? Focus, scroll and text-input state would survive edits outside the edited definition. Inside it, nodes whose constructor or call ordinal shifted may reset. This is implemented with stable source anchors plus a one-line runtime change (the call fragment uses a stable site key instead of dense function/expression ids). | **options**: (A) Best effort as described. (B) Stronger guarantees such as explicit keys or structural diffing, which need a runtime redesign. | **recommendation**: A | **why**: The playground keeps hover, focus, scroll and text-input state by node-id string across plan swaps (runtime_view.rs:1433-1445). Today those ids come from dense compiler expression ids, so almost any edit resets them.
- **question**: Must wire identities (RemoteCallSiteId, ExportId, ImportId, wire_schema_hash) and page-cursor view fingerprints stay stable across compiler versions or redeploys, for example rolling server deploys with live clients or cursor tokens stored in state? | **options**: (A) Deterministic within one compile only. A wire hash mismatch forces reconnect and stale cursors fail closed. (B) Cross-version stable, which needs frozen encoders like the persistence ids. | **recommendation**: A | **why**: Deciding this keeps the frozen-encoder burden limited to persisted user data. The runtime already fails closed on a mismatch (client.rs wire_schema_hash; machine.rs:29980 cursor check).

## risks
- **risk**: Porting the executor-facing storage decisions (row-field roles Value/Authority/ValueAuthority/Capture, list activation modes, value_list_authorities, authority_source_list, row_field_copies) is subtle and undocumented. A mismatch gives wrong runtime behaviour, not a compile error. | **mitigation**: Port them as pure functions over RIR side tables. Gate on canonicalized structural equality of storage_layout and regions against old plans on all 142 examples, plus all 55 scenarios, before deleting the old path.
- **risk**: Persisted identity drift: a different durable schema or a renamed Rust struct changes schema_hash, and stored images are rejected. | **mitigation**: Add persist_id, an explicit encoder with golden vectors from every example's PersistencePlan. Gate on PersistencePlan byte equality (or the documented epoch rules if the owner picks B).
- **risk**: Hash-consing or shared document bodies cause node-id collisions or unsound static caching (a value marked Static is cached per expression id). | **mitigation**: Never cons Constructor, Call, Select or Materialize. Param staticness is the AND over all call sites. Debug builds re-evaluate cached static values and assert equality, and assert no duplicate node ids per frame.
- **risk**: Shared document bodies evaluate arguments eagerly at the caller (runtime.rs:1958-1976). A big record argument (for example a whole store) then reads more fields than the body uses, which broadens dependencies and slows the runtime. | **mitigation**: The inlining policy inlines when an argument is a root or record path whose evaluation exceeds the body's projections. PASS is static environments, never values. Track per-scenario runtime evaluation counters.
- **risk**: The checker misses a rule the back half used to enforce, so lowering hits an ICE on a user program. | **mitigation**: The section 18 list becomes checker work items with negative tests. Lowering returns Ice with spans. Fuzz by mutating example programs and assert either a checker diagnostic or a successful lowering.
- **risk**: Root-level per-call-site stateful instances have no working oracle: today's compiler crashes on them (probes hold3.bn, hold5.bn). | **mitigation**: Define the rule explicitly: one instance per static call path, persisted at the published data path. Add scenarios for it before cutover.
- **risk**: Removing the plan hash and verify_plan from trusted paths could let a backend bug reach the runtime unchecked. | **mitigation**: debug_assert verify_plan inside TrustedPlan::from_compiler, a CI lane running verify_plan plus mutation fuzzing on all example plans, and full verification at every untrusted load (decode_program_artifact, bundles, server-received plans).
- **risk**: Stable-anchor ordinals shift when a definition is edited, so some UI state resets on hot reload. | **mitigation**: Accept as best effort (owner Q3). Per-callee ordinals limit churn to calls of the same callee. A scripted-edit test measures node-id survival.
- **risk**: The distributed call-site ordinal scheme differs from the old compiler and breaks linking between roles compiled together. | **mitigation**: Compile all roles in one lowering session with one ordinal allocator. Test wire_schema_hash determinism across two compiles and run the fjordpulse scenarios.

## work_items
- **id**: LP1 | **title**: boon_plan V12 delta and minimal runtime changes | **description**: Add Call.site and Select.selector; remove compiler_id, compiler_expr_id and owner_function; drop SourceRoute/DependencyEdge ops, unresolved counts, capability_summary (derive on demand), dirty/commit plans and semantic_slice_digest. Add PlanRowExpressionArena::from_trusted_nodes. Replace SealedMachinePlan with TrustedPlan (debug-only verify, lazy blake3 identity). Change MachineTemplate/LiveRuntime/program_host to take TrustedPlan; the unverified Arc entry point becomes from_untrusted. Update verify_plan for untrusted loads. One-line runtime.rs call-fragment change. | **size**: M | **acceptance**: All boon_plan, boon_plan_executor and boon_document tests pass on V12. Activating a trusted plan performs zero verify_plan and zero plan hashing (counter). decode_program_artifact still runs full verification. | **depends_on**: 
- **id**: LP2 | **title**: persist_id frozen identity encoder | **description**: Hand-written encoder that reproduces today's canonical_sha256 bytes for ApplicationIdentity, MemoryId, MemoryLeafId, type_fingerprint, schema/recipe/catalog hashes, migration ids, EffectId, EffectInvocationId and OutputRootId, independent of Rust type names. | **size**: S | **acceptance**: Golden vectors taken from every example's current PersistencePlan match exactly. Renaming a boon_plan struct does not change any vector. | **depends_on**: 
- **id**: LP3 | **title**: boon_lower core: TypedProgram contract, RIR arena, hash-consing, constant pool, stable anchors | **description**: Define the checker-to-lowering interface (section 4) with the checker designer. Build the 20-byte RIR node arena with hashbrown HashTable consing, the identity-bearing op set, ConstPool, and xxh3 stable keys (DefKey, ConstructorAnchor, SiteKey, InstanceKey). | **size**: M | **acceptance**: Consing microbench at 10M nodes/s or better. Unit tests prove identity-bearing ops are never shared. Anchors are unchanged under edits outside a definition (test). | **depends_on**: checker TypedProgram
- **id**: LP4 | **title**: Demand, classification, instance tree, static owners, OUT union-find, PASS environments, inlining policy | **description**: L1 bits over the function DAG, roots in source order, instances only for identity-owning functions, row-region and activation owners, UF for OUT nets with inlined wrappers, interned PassedEnv, and the pre-emission share/inline decision. | **size**: L | **acceptance**: The static owner forest matches the old compiler on all examples wherever old behaviour is defined (TodoMVC 23 owners). Root-level per-site state probes (hold5.bn) lower to one instance per call site. | **depends_on**: LP3
- **id**: LP5 | **title**: HIR to RIR expression lowering for all node kinds | **description**: Records, tags, text, infix, builtins, WHEN/WHILE/THEN/LATEST/HOLD/SKIP/FLUSH, list ops with row regions, Stream/pulses, effects, DRAIN/DRAINING, element state, constructors and document calls. | **size**: L | **acceptance**: All 142 example .bn files lower without an Ice. Every HIR op kind is covered by a test. | **depends_on**: LP4
- **id**: LP6 | **title**: Dataflow analyses, storage layout and op emission | **description**: One Tarjan/topological pass, event-cause sets, state update arms in source order, derived values, list mutations and projections, pulse batches and activations, source routes, effects and outbox. Port the row-field role, list activation mode and authority logic with O(1) indexes. Emit regions, storage, row expressions and debug_map labels. | **size**: XL | **acceptance**: For every example, canonicalized storage_layout, regions, source_routes, row expressions and debug_map equal the old plans modulo the documented V12 diffs. All 55 scenarios pass on the new plans. | **depends_on**: LP5, LP1
- **id**: LP7 | **title**: Persistence plan and migrations | **description**: Durable schema derivation (per the owner's Q1 answer), identities through persist_id, TypeId-memoized DataTypePlan and fingerprints, DRAIN/DRAINING migration recipes against cached predecessor plans. | **size**: L | **acceptance**: PersistencePlan byte-identical to the old compiler on all examples (option A), or matching the documented epoch rules (option B). All migration scenarios pass (examples/migrations, persons_pro). Predecessor stages are compiled at most once per stage source digest. | **depends_on**: LP6, LP2
- **id**: LP8 | **title**: List access plans | **description**: Pattern pass that produces PlanListIndex, access selections, pages, bounded pages and view fingerprints within target-profile limits. | **size**: M | **acceptance**: NovyWave's 10 list indexes are reproduced (structurally). Typed-list scenarios pass. View fingerprints equal the old ones where the inputs are equal. | **depends_on**: LP6
- **id**: LP9 | **title**: Document plan emission with shared bodies | **description**: Shared DocumentFunctions per (DefId, PassedEnvId), the inline policy, templates, materializations and bindings with stable anchor ids, value classes with call-site staticness, initial patch batch. | **size**: L | **acceptance**: TodoMVC document has 5,000 expressions or fewer, NovyWave 8,000 or fewer with 400 templates or fewer. Scenario frames are identical modulo node-id renaming. A scripted-edit test shows node ids outside the edited definition survive. | **depends_on**: LP5, LP1
- **id**: LP10 | **title**: Distributed lowering and link | **description**: Per-role lowering, exports and imports, remote call sites with ordinal scheme, producer function instances, wire schema and hash, all in one session. | **size**: L | **acceptance**: fjordpulse and all distributed scenarios pass. wire_schema_hash is identical across two compiles of the same source. | **depends_on**: LP6, LP7
- **id**: LP11 | **title**: Differential harness | **description**: One command compiles every example with old and new, canonicalizes and diffs the plans, runs the scenarios on both, and reports counters (RIR nodes, document expressions, templates, SHA calls, lowering microseconds). | **size**: M | **acceptance**: Runs in CI and locally in under 2 minutes, and is the gate for LP6 to LP10. | **depends_on**: LP3
- **id**: LP12 | **title**: Integration and verification policy | **description**: CompilerSession verified-preview and playground paths call boon_lower and pass TrustedPlan to the runtime. Program artifacts are hashed once, lazily, only when written. Add a CI lane that runs verify_plan and mutation fuzzing on all example plans. | **size**: M | **acceptance**: compiler-sample verified back half: TodoMVC 5 ms or less, NovyWave 20 ms or less, counter 0.5 ms or less. No SHA-256 on the in-process path except persistence identities. The preview plan swap does no re-verification. | **depends_on**: LP6, LP9, LP1
- **id**: LP13 | **title**: Cutover and deletion | **description**: Delete boon_semantic, boon_ir, boon_verify, boon_checked Type trees, machine_plan_backend, document_executable_backend, document_plan_backend and the old distributed back half, the dependency classifier registry, and the xtask semantic-spine checks. | **size**: M | **acceptance**: Workspace builds; scenarios and differential snapshots (frozen before deletion) still pass; the xtask gates are updated. | **depends_on**: LP7, LP8, LP10, LP12

## deletions
- **what**: boon_semantic (7 semantic graphs, receipts, manifest V7, construction image, out_net, contextual expansion, core lowering) | **size**: ~97k lines (15k cfg(test))
- **what**: boon_ir (erase_and_lower is verification plus a move) | **size**: ~4.2k lines
- **what**: boon_verify (empty obligation manifest plus pulse-fusion wrapper) | **size**: ~3.4k lines
- **what**: boon_checked rich Type trees on the lowering path | **size**: ~8.1k lines
- **what**: machine_plan_backend.rs, document_executable_backend.rs, document_plan_backend.rs, distributed_compiler.rs back half | **size**: ~25.7k lines
- **what**: verify_plan and plan_sha256 on in-process seal and runtime activation (MachineTemplate::new_shared) | **size**: ~91 ms TodoMVC / ~66 ms NovyWave per call, 2-3 calls per preview
- **what**: SourceRoute and DependencyEdge ops and regions (ignored by the executor, machine.rs:5064) | **size**: 47 of 85 ops TodoMVC, 999 of 2,265 NovyWave
- **what**: capability_summary, dirty_plan, commit_plan, unresolved_* counts, semantic_slice_digest, DocumentExpr.compiler_id, template compiler_expr_id/owner_function | **size**: plan fields plus their verify_plan re-derivation checks
- **what**: SHA-256 structural interning in PlanRowExpressionArena (lib.rs:8985-9003) | **size**: ~20-25 ms NovyWave
- **what**: Per-call-site document specialization plus inline-back pass | **size**: 64,482 -> ~3k document expressions on TodoMVC
- **what**: dependency_classifier_schema_v1.toml registry and xtask semantic-spine string gates | **size**: 15,805-line TOML plus xtask checks

## evidence
- boon_plan/src/lib.rs:5468-5503 MachinePlan fields; TodoMVC plan (design/lowering/tmvc.plan.json) compact JSON: document 9,104,205 bytes; row_expressions 73 nodes; regions 19/7/28/3/0/28 ops; 11 scalar slots, 2 list slots, 10 memories
- Document duplication (anal_doc.py on the current plans): TodoMVC 64,482 expressions, 29,917 constants over 439 distinct, 61,691 nodes from 1,663 IR ids copied 5+ times (top: Theme function_parameter/when x284). Distinct non-constant compiler ids + constants = 2,602 (TodoMVC), 5,318 (NovyWave). NovyWave templates 1,444 from 377 distinct constructors (max 80 copies)
- boon_plan_executor/src/machine.rs:5040-5076: executor classifies ops itself, sorts update ops by dependencies, ignores SourceRoute and DependencyEdge (5064), sorts mutations by op id (5076); machine.rs:33877 pending mutations sorted by ordinal
- machine.rs:4704-4707, 4785-4800 and boon_document/src/runtime.rs:5822-5835: runtime name lookups built from debug_map labels (hidden contract)
- boon_document/src/runtime.rs:1975 call fragment 'call-{function}-{expression}'; 3489 'node-{template}'; 5999-6000 'node:{plan_id}:{instance}'; 2007 Select compiler_id as matched key; 2357-2374 element state checked against env.element_context; 2906-2937 call_function replaces params, inherits locals, pushes instance
- boon_compiler/src/document_executable_backend.rs:3723-3760, 2099-2110: template/node ids built from dense executable expression ids and invocation indices
- boon_native_playground/src/runtime_view.rs:1433-1445 keeps hover/focus/scroll/text-input state by node-id string across plan swaps; boon_document/src/lib.rs:3292-3309 full replace on a new frame
- boon_plan/src/lib.rs:4935-4941, 5300-5313 MemoryId inputs; 14463-14500 persistence_schema_hash; 14554-14561 canonical_sha256 = SHA-256(binary::encode); binary.rs:142-160, 224-239 encoder writes struct/newtype/enum/variant names
- boon_persistence/src/migration.rs:285-293: equal schema_version with different schema_hash gives UnsupportedSource; boon_host_runtime/src/persistent.rs:760-763 adopts the image only on exact hash equality
- Probe examples/todomvc.bn: durable list leaves include store.todos.@authority:not_completed (derived) and store.todos.@authority:sources/editing_todo_title_element/events/change/text; todo_mvc_physical includes store.todos.state_0 (anonymous LATEST). machine_plan_backend.rs:2740-2755 list_authority_persistence_path
- boon_semantic/src/core_lowering.rs:79-98 hidden_key_type naming ('store.todos' -> 'Store.todoKey'), part of schema_hash
- Probe counter_unused.bn: an unread root HOLD is still persisted (store.unused_total)
- Probes design/lowering/hold3.bn and hold5.bn: a FUNCTION returning [count: HOLD] used at a root field fails in today's semantic phase ('resolves declaration N to no lexically visible owner/frame binding')
- boon_runtime/src/lib.rs:545-551 -> boon_plan_executor/src/machine.rs:9514-9517: each runtime activation from Arc<MachinePlan> re-runs seal (verify_plan + plan_sha256)
- boon_program_runtime/src/program_core.rs:420-428: program artifacts rejected on compiler_id mismatch (compiler-bound, no cross-version need); program_host.rs:306-356 reads capability_summary
- boon_compiler/src/distributed_compiler.rs:2300-2326 RemoteCallSiteId from 'call:{stable_owner_path}:{fn}:{ordinal}'
- boon_plan/src/document.rs:638 RootOutputDemand::Selected must be sorted (binary_search)
- machine_plan_backend.rs:14051 recursion rejected only at lowering; machine_plan_backend.rs:2276 anonymous line-based persisted state; memory_contract.rs:559, 1016 named-field and closed-type persistence rules (all must move to the checker)
- Measured baselines (plan_part1.md, a_backend.md): back half TodoMVC 158+3+85+90 ms, NovyWave 780+25+214+66 ms; plan_sha256 is 80-93% of verify_plan

## perf_targets
- **metric**: Back half (typed program -> TrustedPlan), TodoMVC, release, fresh process | **target**: <= 5 ms (expected 1.5-4) | **basis**: ~5k RIR nodes x 60-150 ns, ~3k document nodes, 38 ops; today 336 ms
- **metric**: Back half, NovyWave | **target**: <= 20 ms (expected 6-13) | **basis**: ~20k RIR nodes, ~6k document nodes, 2,071 row nodes, ~1,266 ops, ~600 persistence SHA-256; today 1,085 ms
- **metric**: Back half, counter | **target**: <= 0.5 ms | **basis**: Tiny program; today ~5.7 ms (semantic 4.8 + backend 0.4 + validation 0.4)
- **metric**: Document expressions | **target**: TodoMVC <= 5,000; NovyWave <= 8,000; NovyWave templates <= 400 | **basis**: Distinct non-constant compiler ids + constants: 2,602 / 5,318; 377 distinct constructors
- **metric**: SHA-256 invocations on an in-process verified compile | **target**: Only persistence identities (<= ~1,000 on NovyWave), 0 plan-level hashes | **basis**: Plan identity is lazy; interning uses FxHash
- **metric**: verify_plan calls on trusted in-process and runtime-activation paths | **target**: 0 in release | **basis**: TrustedPlan; today 2-3 per preview at ~66-91 ms each
- **metric**: Warm full re-lowering after a one-literal TodoMVC edit | **target**: <= 5 ms | **basis**: Same as cold; no incremental lowering needed within the 100 ms preview budget
- **metric**: Lowering peak memory, NovyWave | **target**: <= 16 MB | **basis**: RIR 20 B/node + side tables + ~2 MB-JSON-equivalent plan

---

# Adversarial review

## area
Lowering and plan: typed HIR to MachinePlan V12 (boon_lower), reviewed for runtime-contract compatibility and back-end/runtime performance

## verdict
needs_changes

## major_issues
- **issue**: The rule for Static params in shared document bodies is unsound. The design marks a param Static when every call site passes a static argument (AND over call sites, section 14). But the runtime caches Static results by DocumentExprId alone, for the whole evaluation pass. So Theme/color(Primary) and Theme/color(Secondary) calling one shared body would both get whichever value was cached first. | **evidence**: boon_document/src/runtime.rs:1702 (static_value_cache: BTreeMap<DocumentExprId, EvalValue>) and runtime.rs:1856-1877 (a cache hit on the id alone, with no parameter frame). Today params are never Static: document_executable_backend.rs:1770-1773 classifies them with value_class_for_type (4024-4049), which gives DynamicScalar or DynamicStructure. | **fix**: Any node that depends on a Param or a Local must never be Static in a shared body, or the runtime cache key must include the call frame. To keep today's caching for the dominant Theme pattern, evaluate calls to pure functions with all-constant arguments at compile time, memoized per (DefId, constant args). That is allowed under owner Q2 option A, and it shrinks the document further than sharing does.
- **issue**: The inline policy (inline when single-use or 8 nodes or fewer) can make node ids collide, and it makes them unstable. A constructor, Call or Materialize inside an inlined body gets its anchor from the callee's DefKey and a per-caller ordinal. Two inlined copies under the same parent, for example `LIST { divider(), divider() }`, then get the same template.node and the same instance path. Separately, adding a second call to a render helper flips it from inlined to shared. That adds a `call-` fragment and renames every node under the first call, which loses focus, scroll and text-input state. | **evidence**: runtime.rs:3561-3575 (instance_node_id = env.instance.join("/") + the row suffix), 3489 (node-{template}), 1975 (the call fragment only exists for real Calls). runtime_view.rs:1433-1445 keeps UI state keyed by the node-id string. | **fix**: Functions with the `renders` bit are always emitted as calls, never inlined based on how many times they are used. Otherwise, salt the anchors of Constructor, Call and Materialize nodes in inlined bodies with the inline-site path. Also add an emission-time assertion that node-id templates are unique per parent path.
- **issue**: Sharing moves work into the runtime's call path, and nothing measures that. Every document Call now builds a BTreeMap for the parameter frame and clones the DocumentFunction (`.cloned()`). It clones the EvalEnv, and Arc::make_mut deep-clones the whole instance Vec<String>, because the env clone always leaves the Arc shared. It also formats a string. Node ids are the full joined path. With u64 anchors formatted in decimal (up to 20 digits for each `node-{template}` fragment) plus `call-{016x}` fragments, ids grow several times longer. They are keys in BTreeMaps (frame, retained nodes, playground lookups) whose entries share long common prefixes. Full re-evaluation plus diff_frames runs on every structural change, and params that lose Static also become retained scalar bindings. Each of those stores an EvalEnv clone and a set of dependencies. | **evidence**: runtime.rs:1957-1977 (Call), 2906-2937 (call_function: clone at 2918, env clone at 2931, make_mut push at 2935), 3561-3575, 5997-6002, 1025-1037 (full evaluate + diff_frames), 3493-3500 (a DynamicScalar argument becomes a retained binding), 1658-1669 | **fix**: Add runtime perf targets alongside the compile targets: full document evaluation per frame and retained-binding count on TodoMVC with many rows, NovyWave and Cells, no worse than today. Make the needed runtime changes part of LP1 and budget them honestly, because this is not a one-line change. Calls to non-rendering functions should push no instance fragment (for example `site: Option<u64>`). Make env.instance a persistent parent-linked path, or hash node paths into a u64, instead of an Arc<Vec<String>> that is deep-cloned on every call. Format anchors compactly, or truncate them to 32 bits with the collision check.
- **issue**: The never-consed list is incomplete, and per-node side vectors do not work with hash-consing. EffectCall, Stream/skip, Bool/toggle-style stateful builtins and Drain are missing from the never-consed set, although L1 marks them as owning identity. Consing two identical `File/write(...)` sites would merge them into one EffectInvocation. The RIR also keeps `owner`, `inst` and `anchor` vectors indexed by RId, but the cons key leaves them out. A consed node therefore gets whichever owner, instance or span it was created with first. | **evidence**: Design section 5 ('Identity-bearing ops are never consed: Hold, Latest, Source, ListAuthority, Stream/pulses, doc Constructor/Call/Materialize/Select'). The executor looks up effects by invocation_id (machine.rs:16907-16925). | **fix**: Define never-consed as owns_identity, plus every effect op, plus the identity-bearing document ops, and derive it from the same L1 predicate, not from a hand-written list. Derive a node's owner and instance from its operands (the least common ancestor of the owners it reads). Keep spans only on non-consed nodes, or in a first-occurrence debug table that no analysis reads.
- **issue**: Owner option B contradicts the acceptance criteria and invariants in the design itself. B drops derived and event-payload leaves such as @authority:not_completed and @authority:.../change/text. But those are authority row fields in storage, and verify_plan requires every authority field of a persisted list to have a leaf (the design's invariant 14 repeats this). Meanwhile LP6's acceptance demands canonicalized storage_layout equal to the old plans. So B also changes row-field roles, and possibly snapshot-versus-live semantics for derived row values. The persistence plan also doubles as the executor's source of semantic identity. It provides the cursor row and field ids and bounded-page authority hashes, and name lookups for inspection (inspect_value_current, used by the migration scenarios). | **evidence**: boon_plan lib.rs:11119-11127 (list-authority-fields-have-stable-persistence-leaves); machine.rs:4716-4775 (semantic_list_identities, semantic_row_field_identities, list_fields_by_exact_name from persistence leaves), 9376-9395, 32783-32795 (a bounded page fails without a memory identity); cursor.rs:392-420 (hashing a Row requires a leaf id for every field, otherwise CursorError::Invalid); machine.rs:15598-15602; migration_scenario.rs:1073. Probe of the TodoMVC plan: 9 of the 22 row fields in store.todos already have no leaf. | **fix**: Decide storage roles from source semantics first, and let persistence follow them: persist exactly the authority fields. Under B, LP6 compares storage by semantic path and allows the documented role changes. Give non-persisted row fields a non-durable semantic id for cursors, or change cursor.rs to hash only persisted fields. Also note, as an argument for B, that fewer schema-affecting edits also means fewer hot-reload rejections (persistent.rs:2528-2541 goes to migration_chain, where the same version with a different hash gives UnsupportedSource).
- **issue**: 'Lowering is total' is not realistic with the section 18 list as written. The back half has about 680 PlanError sites (machine_plan_backend 426, document backend 130, distributed 122). Many of them are rules a user can hit, and the list omits them. Several of these rules need dataflow classification (transient vs durable vs row-contextual), so the checker would have to duplicate L1/L3. | **evidence**: machine_plan_backend.rs:723 (a producer function cannot own a durable outbox effect), 1733/1752 (whole-list migration defaults), 3168-3173 (absence or FLUSH as migration data), 3722, 3808, 6412 (role cannot contain document/scene roots), 8287-8351 (list initial fields from transient inputs), 9560, 13223 (element state in dataflow), 13302, 13406, 14919, 1403. distributed_compiler.rs:2990-3010 (a Session scope crossing a global boundary) and 3020-3060 (cross-role immediate cycles). | **fix**: Add a work item that classifies every PlanError site as either an ICE or a user rule. Also allow lowering to return positioned user diagnostics for storage and dataflow rules. At 2-13 ms it can run in the diagnostics lane, which avoids a second classification pass in the checker. Put the distributed session-scope rule, the cross-role cycle rule and the migration rules on the explicit list.
- **issue**: The V12 stable anchors do not fit the runtime id types on wasm32. DocumentElementContextId.call_instance is a usize, and the design puts a 64-bit anchor in it. boon_web_host runs boon_document and boon_runtime on wasm32 and deserializes plans from bundles, so a u64 anchor overflows usize there. | **evidence**: boon_plan/src/document.rs:35-38 (call_instance: usize, ordinal: usize); boon_web_host/Cargo.toml:13-19 (boon_document and boon_runtime in the web host). Templates, materializations and nodes are already u64 (document.rs:42-50). | **fix**: Add DocumentElementContextId.call_instance: u64 to the V12 delta, or use a dense per-plan context id. The element context only has to match within one frame and does not need stability across reloads.
- **issue**: The contract table leaves out host_ports and OutputValueRef::RetainedVisual. Server programs need HostPortPlan (HTTP and WebSocket source/output bindings) and the program host checks it, but no work item produces it. | **evidence**: boon_plan lib.rs:5483 (host_ports); boon_server_runtime/src/lib.rs:3521-3600 resolve_bindings; program_host.rs:420; lib.rs:5424 RetainedVisual{expression} refers to document expression ids. | **fix**: Add host_ports and output emission to LP6/LP10, with fjordpulse_http_contract and the server scenarios as acceptance. Remap RetainedVisual expression ids after document emission.
- **issue**: The reasons given for removing DependencyEdge are inaccurate, although the removal itself looks safe. boon_document's pending_distributed_targets propagates across all ops at runtime. The distributed compiler's session-scope and immediate-cycle checks, and verify_plan's distributed_session_scope_failure, also iterate DependencyEdge ops. | **evidence**: runtime.rs:5919-5976; distributed_compiler.rs:2995-3010, 3033-3045; boon_plan lib.rs:10188-10210. I checked tmvc.plan.json and novy.plan.json: 28/28 and 878/878 DependencyEdge ops have inputs that are a subset of another op with the same output, so they are redundant on those two examples. | **fix**: Keep the drop, but add a CI assertion on the old compiler, before cutover, that every DependencyEdge is implied by another op. Run it on fjordpulse and the distributed examples too. Say explicitly that pending propagation relies on the StateUpdate and DerivedValue inputs.
- **issue**: Assigning op ids by reverse-topological rank is unnecessary and can change observable order. The executor already topologically sorts same-trigger update ops itself, breaking ties by op id. Effect-bearing updates are left out of producer ordering, so op id alone decides effect emission order across different HOLDs that respond to one event. | **evidence**: machine.rs:21575-21615 sort_update_ops_by_dependencies (sorts by op id first, effect branches skipped at 21579), 5066-5076 | **fix**: Assign op ids in source pre-order across the whole program. Delete the rank output of the L3 Tarjan pass: cycle detection belongs to the checker, and ordering belongs to the executor. That leaves pulse-batch closure as the only graph walk.

## missing
- Runtime activation and evaluation perf targets. Metadata::new, DocumentRuntime::new, the first full evaluation and the retained-binding counts all change shape under sharing, and only compile-side targets are given.
- A rule for pure calls with constant arguments (compile-time evaluation). This is the dominant Theme pattern and the only way to keep Static caching with shared bodies.
- The V12 delta omits delta_plan, which is dead: only the plan-hash view reads it (lib.rs:5500, 14211, 14314). It should also drop the Unknown/unresolved plan variants (PlanListProjection::Unknown, ListInitializerKind::Unknown, is_unknown_op), because lowering becomes total.
- DocumentPlan.view_bindings and initial_patch_batch have no runtime caller. Only accessors exist (machine.rs:14225-14260, and document_binding_value_target is never called). The design keeps and emits them.
- host_ports (HTTP and WebSocket) emission, plus remapping RetainedVisual output expression ids.
- The field/state alias contract: runtime.rs:5886-5917 treats a field as the state with the same debug_map label. Scenario FieldSet: matching also resolves through debug_map labels (boon_runtime lib.rs:1732-1760).
- DocumentExprOp::RuntimeExpression (the bridge from document to row expressions) and how RIR decides which subtrees become row expressions and which become document ops.
- Node-id stability under row fragments: `/row-{ListId}-...` uses the dense ListId. Inserting a list earlier in the source renames every row node, which undermines the stable-anchor goal for list UIs (the design defers this to V13).
- Sequencing for the differential oracle. Examples must first be refactored under decisions 4 and 6 (Theme split, strictness fixes) into sources that both compilers accept. Otherwise old-vs-new on TodoMVC and NovyWave compares different programs.
- How debug builds behave: debug_assert!(verify_plan) inside from_compiler runs on every keystroke in the debug playground build that AGENTS.md uses for visible launches. Gate it with a feature or environment variable instead.
- A check that a stored artifact's plan_digest (sha256-hex validated in boon_app_package bundle.rs:216, 502) is still produced when the lazy identity switches to blake3. The digest format and its validation must change together.

## unrealistic
- 'One runtime line' for stable UI identity. It also needs a u64 element context, inline-site salting, and a non-quadratic instance-path representation to avoid the call-overhead regression.
- LP6 acceptance of 'canonicalized storage_layout, regions and row expressions equal to old plans modulo renumbering'. The instance structure, demand, sharing and (under B) authority roles all differ, so this is graph isomorphism with expected diffs. Compare by semantic keys (debug labels, source-route paths, memory paths) and by behaviour (scenarios, frames modulo node ids).
- 'Lowering is total' with the current section 18 list. About 680 back-half error sites need classifying, and several rules need dataflow facts the checker does not have.
- Retaining a TypeId to DataTypePlan memo across revisions depends on an append-only checker TypeStore, which is not established. It saves at most about 1 ms, so drop it.
- The claim that element-state reads are 'resolved against the enclosing constructor'. The runtime only checks that the context matches and then returns constant false for focused, hovered, pressed and selected (runtime.rs:2357-2374). It works for shared bodies only because the read and its constructor sit in the same body.

## simplifications
- Drop the DocumentTemplate table. Let Constructor carry `node: u64` directly: the runtime only reads template.node, through a linear `templates.iter().find` on every constructor evaluation (runtime.rs:3384-3392). This removes both the table and the O(templates) scan.
- Delete view_bindings, initial_patch_batch, delta_plan, capability_summary, dirty_plan and commit_plan, and the Unknown variants, together in V12. The runtime reads none of them. program_host derives capabilities on demand.
- Evaluate pure calls with constant arguments at compile time (memoized per (DefId, constant args), with hash-consed results). Theme lookups become constants, which keeps them Static and makes the document smaller than sharing alone would.
- Always emit render functions as calls, and always inline pure non-render helpers into their callers (or share them without an instance fragment). Node identity then never depends on how many times a function is used.
- Assign op ids in source order and let the executor keep its own dependency sort. The compiler's SCC pass is then needed only for the pulse-batch closure (the checker owns cycle detection).
- Under owner option B, skip LP2's byte-for-byte reproduction of today's struct-name-laden canonical_sha256. Define one small documented encoder now and pin it with golden vectors.
- Drop TrustedPlan's separate lazy blake3 over a new name-free encoding. The only consumers of plan identity are artifact writers and bundle checks, which already serialize, so hash those bytes. The existing MachineTemplate::new_sealed (machine.rs:9523-9527) and PlanRowExpressionArena::from_nodes (lib.rs ~8895-8904) already provide the trusted entry points, so LP1 can extend them instead of adding a new type.
- Make the runtime name contract typed. Both sides change in V12 anyway, so replace debug_map `field:N` string parsing (machine.rs:4704-4800, runtime.rs:5822-5917, boon_runtime lib.rs:1747-1760) with typed (id, semantic path) tables.
- Build the differential harness on semantic keys and behaviour: scenario outcomes, frame trees compared by structure ignoring node ids, sets of memory and leaf semantic paths, source-route path sets and debug-label sets. Do not canonicalize the whole plan graph.

## extra_owner_questions
- Should V12 include document-runtime changes beyond the call fragment: non-render calls that push no instance fragment, a persistent or hashed instance path, compact anchor formatting? Without them, shared bodies may make per-frame evaluation slower than today even though the plan is 20x smaller.
- Can node ids become opaque hashed u64s instead of long path strings? The playground, reports and hit-testing key on these strings today.
- Under persistence option B, what identity should non-persisted row fields of persisted lists (derived values) have for page cursors and bounded-page authority hashes? cursor.rs currently rejects any Row field without a persistence leaf.
- Is the order in which effects are emitted, when several HOLDs react to one event, defined as source order? It is decided by op id today, so the lowering must fix it.
- May lowering report positioned user diagnostics for storage and dataflow rules (list initializers from transient inputs, element state in dataflow, migration data restrictions, distributed session scope)? The alternative is for the checker to replicate the dataflow classification.
- Should a HOLD inside an identity-owning function called from view code, with no published data path, be persisted, stay transient, or be an error? The durable-schema rule currently only covers named data paths and `<named path>.state_<k>`.
