> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

# Checker architecture: type-checker algorithm, data structures, processing order, schemes, diagnostics, catalog, outputs, incrementality and performance model for the greenfield strict Boon compiler

## area
Checker architecture: type-checker algorithm, data structures, processing order, schemes, diagnostics, catalog, outputs, incrementality and performance model for the greenfield strict Boon compiler

## summary
I recommend a flow-directed biunification checker (Simple-sub / algebraic-subtyping family) over a restricted "polar, kind-partitioned" type language, rather than HM unification with bolted-on joins. In that language every union holds at most one member per head (TEXT, NUMBER, BYTES, BITS, record, tag row, LIST, SET, MAP, SOURCE port). Records carry per-field presence: Required/Maybe on values, Required/Optional on requirements. Tag rows are sorted by SymbolId and carry payload rows. Every type node carries a 3-state flow-mode mask. With these types a join is a pointwise merge per head, a meet is a pointwise intersection, and a subtype check dispatches on the head. So bounds are lattice elements, not lists, the solve is monotone and confluent, and nothing is ever retracted. Boon's own shape makes generalization easy: FUNCTIONs exist only at module level (a nested FUNCTION in a BLOCK is rejected today), recursion is rejected, and BLOCK locals stay monomorphic. Reads of root values inside a function are treated as implicit parameters. Given that, levels collapse to "one private arena per FUNCTION, then one monomorphic root group", and generalization becomes a linear compaction of that arena into a canonical, hash-consed scheme (the analogue of Elm's makeCopy / OCaml's generic_level). Most Boon code is ground data flowing forward: 95% of TodoMVC lines are function bodies, many with literal-tag Theme calls. The checker therefore evaluates ground expressions directly through memoized TypeId operations and falls back to inference variables only for parameters, PASSED, HOLD cycles and SOURCE payloads. A call instantiates the callee's scheme exactly once per syntactic call site; all-ground arguments hit an exact memo keyed by (SchemeId, argument TypeIds). That replaces 10,537 compiled call sites, 82,451 constraints and 169,680 unification steps on TodoMVC (measured, 398 ms typecheck) with about 4k constraint-generation visits and about 480 instantiations. Diagnostics are order-independent sets of 24-byte conflicts, each a (consumer, producer) pair of u32-pair expression references. Text, and a display order for fields by first appearance, is computed only on demand. The outputs are dense side tables that lowering monomorphizes directly: expr_type as TypeIds over entry-point variables, per-call Inst argument types, and resource facts. Warm reuse keys each function on its position-free HIR hash plus its callees' SchemeIds, with early cutoff on SchemeId equality, which is sound because a scheme is a pure function of those inputs. The model estimates 1.2 ms (target at most 4 ms) for TodoMVC and 4.5 ms (target at most 12 ms) for NovyWave, cold and single-threaded. The implementation is about 13k lines plus about 2.5k lines of catalog data, replacing about 81k lines of kernel plus most of boon_typecheck's 109k. Because Boon has no recursion, a mechanical "inline every call and check monomorphically" reference checker gives a built-in test oracle for scheme soundness.

## design
# Boon checker: flow biunification over kind-partitioned polar types

## 0. The choice in one paragraph

A type describes the exact set of runtime shapes that can reach a position. Producers (literals, constructors, builtin results) add **lower bounds**. Consumers (field reads, builtin parameters, WHEN patterns, render contracts, THEN/HOLD mode rules) add **upper bounds**. Bounds meet at variables and are checked there. Each union allows at most one member per *head* (kind), and records carry field presence, so both bounds are elements of a lattice with pointwise join and meet. Every FUNCTION is solved once, in a private arena, bottom-up over the acyclic call graph. The arena is then compacted into a canonical scheme, hash-consed and keyed by a SchemeId, which each call site instantiates once. Root values form one monomorphic group, solved last. A value is typed by its producers and consumers only check it, so types never change because of how a value is used. The single exception is SOURCE payloads, which are invariant. That property is what makes the errors predictable and the reuse keys exact.

## 1. Options compared

| | (a) HM + levels + Rémy rows/presence + polymorphic variants + joins by unification | (b) Simple-sub / MLstruct biunification (bound lists, general ∪/∩, coalescing plus simplification) | **(b′) chosen: biunification over kind-partitioned polar types, merged lattice bounds, ground-first evaluation, 2-level compaction** | (c1) checking by memoized specialization (Zig comptime / C++ templates) |
|---|---|---|---|---|
| `Oklch[..] \| TEXT` | Needs a row over kinds on every type (`[> Text \| Tags..]`) plus a presence lattice, i.e. subtyping on atoms added to unification | native (positive ∪) | native: a head bitmask | native (ground join) |
| Field read on `[a]⊔[a,b]` or on `A[x]\|B[x]` | Presence variables must be ≤-constrained (Pre ⊔ Abs = Maybe), which is not unification | native | native: pointwise over members | native |
| Flow modes, order chain, presence | Each needs its own join/transfer machinery anyway | same ≤ propagation | same propagation (a small attribute solver) | ground only |
| Error locality | Blame depends on unification order, and a consumer changes a producer's inferred type | producer→consumer pair | producer→consumer pair, reported from the final bounds, so independent of order | errors appear inside instantiations (template-style), and uncalled functions go unchecked |
| Scheme size | compact by construction | blows up without simplification (it grows with the transitive call tree) | canonical compaction, linear, sized like an HM type | no schemes; cost grows with the number of instantiations |
| Per-op cost | ~10-30 ns union-find | bound lists re-checked pairwise, cubic worst case | ~40 ns lattice merge; ground ops are memoized TypeId lookups | fast when ground |
| Core size | ~3-4k lines, plus presence bolt-ons that approach (b′) | ~0.5k in Scala without our features | ~6k solver+scheme, ~3k generation | smallest solver, but violates decision 4's "one type per function" |

Why not (a): Boon's unions are unions of *kinds* (decision 5), and its records are joined with presence. Both are subtyping. Unifying kind-rows makes every literal an open polymorphic variant and requires OCaml's conjunctive row bounds. Presence and flow modes still need ≤ constraints on top of that, so you pay for two mechanisms. Unification also makes consumers rewrite producers' types. That breaks the "types depend only on producers" property the root group and the incremental keys rely on.

Why not plain (b): unrestricted ∪/∩ and bounds kept as lists force coalescing and co-occurrence simplification, which is costly and not canonical, so there is no hashable scheme. Kind partitioning makes negative unions decidable by dispatching on the head, and lets bounds stay merged lattice elements. MLstruct's negation types exist for narrowing, which decision 4 dropped.

(c1) is rejected as the checker but kept as an optimization: the ground fast path in §7 is exactly memoized specialization, applied only where it is exact.

## 2. Type language (what a TypeId denotes)

- **Heads** (u16 bitmask): TEXT, NUMBER, BYTES, BITS, RECORD, TAGS, LIST, SET, MAP, PORT. A type is `mode × (at most one member per head)`. In scheme and body terms it can also contain entry-variable references (`Var(i)`, meaning "⊔ α_i" on the positive side and "⊓ α_i" on the negative side). The empty head set is **Never** (⊥, the data type of SKIP). A negative type with all heads allowed and an open tail is **Any**, the upper bound for `__` / binder-only patterns.
- **Record member**: fields sorted by SymbolId, `(name, TypeId, presence)`. Positive presence is `Required | Maybe`, where Maybe records the origin of the producer that lacked the field. Negative presence is `Required | Optional`. The tail is `Closed | Open`: positive records are always Closed, meaning exact shapes; user parameter requirements are Open (width subtyping); render and style contracts are Closed. No authored order is stored anywhere (decision 7).
- **Tag member**: entries sorted by SymbolId, `(tag, Bare | Payload(RowId))`, tail Closed/Open. A WHEN without `__` gives a Closed upper bound, and that closed bound *is* the exhaustiveness check. Payload rows follow the record rules. For example, `Oklch[lightness]` ⊔ `Oklch[lightness, chroma, hue, alpha]` gives `Oklch[lightness, chroma?, hue?, alpha?]` (Theme/Classic.bn:22-60).
- `LIST<T, chain>` (the chain attribute is a 2-state lattice {Chained, Unordered} for sort_by/then_by), `SET<T>`, `MAP<K,V>`, `BYTES(Fixed n | Dyn)` with Fixed(n) ≤ Dyn, `BITS(w)` (invariant width; widths are ground in every interned ground type), and `PORT(ρ)` with an invariant payload.
- **Mode mask** (u8), with the states C = continuous, E = tick-present event, A = absent. A lower bound is the set of states a value may be in; an upper bound is the set of allowed states. Transfer functions are monotone and table-driven (§5).
- **Ground rules** (all memoized on TypeId pairs):
  - L ≤ U holds iff, for each head in L: the head is in U; records have every Required field of U present as Required in L with its type ≤, Optional fields satisfied if present, and no extra fields when U is Closed; each tag of L is in U with payload ≤; LIST, SET and MAP elements are covariant; PORT is invariant; Bytes Fixed ≤ Dyn. Also mode(L) ⊆ mode(U).
  - ⊔ is the pointwise union: fields in one side only become Maybe, tags are unioned, payloads are joined, modes are ORed.
  - ⊓ is the pointwise intersection: required wins over optional, Closed ∩ Open = Closed, tag sets are intersected.
  - Join, meet and subtyping obey the lattice laws. Each property is tested.

**Join policies** (decision 6 says to enforce the documented rules):
- *Union* for WHEN, WHILE and FLUSH boundaries: plain ⊔.
- *Compatible* for LATEST, HOLD init/update, and LIST/SET/MAP elements including List/append: ⊔ plus a deferred predicate `SameHeads(args)`, meaning every non-Never argument has the same head set, with any tag mix counting as one head. This rejects `LATEST {1, TEXT}` (TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:479-484). It accepts records that differ in their fields, with Maybe presence. Owner question 1.

## 3. Identity layer (session-persistent, append-only)

- **Interner** `SymbolId(u32)`: one per session, shared with the front end. It is FxHash-indexed over a byte arena. A prefix of builtin paths, contract field names and core tags (True, False, …) is seeded at fixed ids generated by build.rs, so the catalog uses constants. Row order is by SymbolId value, which is enough for identity. Everything users can observe (display, plan field order) uses names or explicit orders, never SymbolId order.
- **TypeStore**: hash-consed and immutable. It holds only *ground* and *scheme or body* terms, never inference variables. (Today the kernel interns terms that contain variables: 266,605 intern requests on TodoMVC; TypeTermArena 130k terms on TodoMVC and 112k on NovyWave.) Nodes are 16 bytes and point to member, field and tag pools. The index is one open-addressing table over structural hashes, with memo tables `join`, `meet`, `sub` (the result carries the first failing path, used for conflict detail), `field(TypeId, SymbolId)` and `subst(TypeId, InstKey)`. The flag bit `HAS_VAR` marks non-ground terms, so instantiation copies only those (OCaml generic_level / Elm noRank analogue). Structures keyed by dense ids use Vec, never hash maps (Roc's lint).

## 4. Inference layer (a private arena per definition solve)

- `Slot(u32)`: the top bit set means `Var`, otherwise a ground `TypeId`. Every expression gets a Slot.
- Var data is stored SoA (structure of arrays): union-find parent (used only to collapse invariant pairs, i.e. PORT payloads, HOLD state/output and BITS widths); `lo: BoundId`; `hi: BoundId`; `mode_lo`, `mode_hi: u8`; `succ`/`pred` edge lists (chunked pool); flags (INVARIANT, ENTRY, PASSED, OUTER); `origin: Prov`.
- A `Bound` is shallow: a head mask plus members whose children are Slots. Record fields carry a `Prov` each: the producer on lower bounds (the origin of absence for Maybe) and the requirer on upper bounds.
- Work items: `Flow(Slot, Slot, Prov) | Lower(Var, BoundId) | Upper(Var, BoundId) | ModeProp(PropId)`, processed LIFO.
  - Ground ≤ Ground: memoized `sub`.
  - Ground ≤ Var: `Lower`. Var ≤ Ground: `Upper`.
  - Var ≤ Var: add an edge, push `lo(v)` to w and `hi(w)` to v. If both are INVARIANT, union them instead.
  - `Lower(v, B)`: `lo' = lo ⊔ B` per head. If nothing changed, stop. Otherwise check the *delta* against `hi` by dispatching on the head (a missing head, missing or Maybe field, unhandled tag, or extra field on a closed record each record a Conflict; matching children push `Flow`s), then forward the delta to `succ`. `Upper` is symmetric.
  - A join of two non-ground child slots allocates a memoized join variable. A meet does the same with a meet variable.
- Termination: each bound only grows, and its height is bounded by the type size (fields can only be added, or change Required→Maybe), so the fixpoint is unique and independent of order. Conflicts go into a set deduplicated on `(consumer, producer, kind, detail)`, so the reported set is also independent of order.
- **Attribute sub-solver**: mode masks and LIST chain attributes live on the vars. `ModeProp { op, inputs: [Var; ≤4], out }` re-applies its table-driven monotone op whenever an input's `mode_lo` grows.
- **Deferred predicates** `Pred { kind, args: [Slot; 3], site }` cover rules that are not lattice bounds: SameHeads, Comparable (the operands of `==` share a head), KeyEligible (MAP/SET keys), NoCollectionInHold, a disjunctive ModeAtLeast (e.g. `(a + b) |> THEN`), SpreadHas (a field read through a spread of two parameters), and BitsWidthEq. They are evaluated on final bounds when the definition's solve ends. If they still mention entry variables they become residual predicates in the scheme, re-checked at instantiation, like Roc's "pending requirements in schemes" or type classes.

## 5. Constraint generation (one post-order pass per definition, interleaved with solving)

Local scopes are visited in the resolver's dependency order, so every referenced local is already final when it is read. A slot is Ground exactly when its value depends only on final ground slots. Ground operations are memoized TypeId lookups that allocate no variables.

| construct | type rule | mode |
|---|---|---|
| number / text / bytes / bits literal | `Ground(NUMBER)` …; `Text/to_bytes` etc. on a *direct literal* are refined by the catalog (BYTES[N]) | C |
| `A`, `A[f: e]`, `[f: e]` | closed tag / record, ground if all children are ground | C (the record's own mode) |
| `[...a, f: e, ...b]` | parts applied left to right. All ground: `store.spread`. Otherwise a Spread propagator: extend forward as `lo(a)` grows; requirements on fields not overridden go backward to a; unresolved cases become `SpreadHas` | Combine |
| `e.f` | ground: `store.field(T, f)` (a conflict unless every member has f Required); var: `e ≤ [f: r, ..Open]` | Combine(e, field) |
| static root path `store.a.b` (through literal root records) | the RootId slot: an implicit parameter inside functions, the root variable in the root group | as solved |
| `PASSED.p…` | the `π` entry variable: `π ≤ [p: r, ..Open]` | — |
| call / pipe | instantiate the scheme (§7). A pipe fills the first ordinary parameter. A PASS-less call passes `π_caller ≤ π_callee`. `PASS: e` passes `e ≤ π_callee`. At root level without PASS the context is the closed empty record, so any requirement produces "missing PASSED field" | callee rule |
| `WHEN` | `s ≤ Pattern` (a closed/open tag row with payload binders as fresh vars; TEXT/NUMBER heads for literal patterns; Any for binder/`__`); `r ≥ armᵢ` (Union); all arms are always generated (decision 4) | When(s, arms) |
| `WHILE` | as WHEN, plus `mode(s) ⊆ {C}` | |
| `THEN` | `mode(in) ⊆ {E,A}`; `r ≥ body` | Then(in, body) |
| `LATEST` | `r ≥ armᵢ`; `SameHeads(arms)` | Latest(arms) |
| `init \|> HOLD s {u}` | `s ≡ r` (invariant); `init ≤ r`; `u ≤ r`; `mode(init) ⊆ {C}`; `mode(u) ⊆ {E,A}`; `SameHeads`; `NoCollectionInHold(r)` | out = {C} |
| `SKIP` | `Ground(Never)` | {A} |
| `SOURCE` | `PORT(ρ)`, ρ a fresh INVARIANT var | {E}; reading through a PORT yields ρ at mode E |
| `LIST{…}`, SET, MAP | `LIST<α>`, `α ≥ elems`, `SameHeads`; SET and MAP add `KeyEligible` | |
| `TEXT{… {e} …}` | `e ≤` the catalog's Interpolable type | Combine |
| operators | catalog signatures (`-` is NUMBER × NUMBER → NUMBER, so `TEXT - 1` is rejected); `==` adds `Comparable` | Combine |
| BLOCK locals | monomorphic slots (no local generalization) | |
| `FLUSH e` | boundary result `≥ e`; the expression is Never | |

The same pass enforces the syntactic strict rules: unused ordinary parameters, the OUT rule of exactly one producer per OUT binding, and the scope-effect summary (which ordinary parameters are evaluated under which OUT, as a u64 bitset per OUT; OUT plan:539-553). Recursion and root-level instantaneous cycles are handled by the definition graph (§6). Local instantaneous cycles, including `[title: title]`, are reported by the resolver.

## 6. Definition graph, order, and the root group (C9 remedy)

- **Nodes.** One per FUNCTION (FnId). One per root field path at every depth, plus root statements such as `document` and `scene` (RootId); this is the kernel's granularity (155 definitions on TodoMVC, 1,389 on NovyWave). Reads through literal root records resolve statically to the deepest RootId.
- **Edges.** fn→fn calls give the order and the recursion check: any function SCC larger than one, or a self edge, produces a positioned "recursion" diagnostic that lists the cycle. root→root reads are labelled instantaneous or temporal; Tarjan over them reports instantaneous root cycles. Distributed role adjacency is an edge check on qualified reads.
- **Order.** Kahn levels over fn→fn edges (callees first), then the root group. The function-only graph is acyclic on both fixtures. The only function-containing SCCs (5 nodes on TodoMVC, 22 on NovyWave) exist through root reads, and those are broken because
- **root reads inside a function are implicit parameters.** A function body never touches root variables. Each distinct RootId it reads becomes an OUTER entry point with a requirement. Callers forward outer entries upward, the same way they forward PASSED. In the root group, an outer entry instantiates to the shared root variable itself, not a fresh copy. So function solves are independent (they can run in parallel per level), function summaries are self-contained and hashable, and root values stay monomorphic with nothing to extrude. Functions never write root values. The only flow from a function into a root is the SOURCE binding of an invariant PORT, which receives ground element payloads.
- **Root group.** One arena at level 0 holds a variable per RootId and per root SOURCE payload. All root definitions are generated, with function calls instantiating schemes, and solved together. Root code is almost entirely ground forward evaluation. The root variables are then zonked to ground TypeIds. An unbound, unread PORT payload defaults to the closed empty record; reading a payload field of a never-bound SOURCE is an error (owner question 6). The group is not ordered by SCC because PORT payloads come from consumers through invariance. Re-solving it is cheap (§13).
- **SOURCE payloads end to end.** A SOURCE inside a function, e.g. new_todo's per-item elements (RUN.bn:98-160), is an invariant variable reachable from the result. It therefore becomes a scheme variable, fresh at each instantiation, carrying the body's read requirements (e.g. `{key: TEXT}`). The rendering function binds it through its parameter's contract `PORT(={…})`. Two elements binding one source with different payloads give a conflict.

## 7. Generalization = compaction; instantiation

**Compaction** runs at the end of a FUNCTION solve and is linear in the arena.
1. *Negative pass.* Roots are the parameters in declaration order, then π, then outer reads (sorted by stable root key). Compute `neg(v) = hi(v) ⊓ ⋂ neg(succ)` with memoization. Every var reached as a child position is marked ENTRY and numbered in first-visit order.
2. *Positive pass.* Roots are the result, then OUT outputs, then INVARIANT vars reachable from them (SOURCE payloads). Compute `pos(v) = lo(v) ⊔ ⋃ pos(pred)`. An ENTRY var emits `Var(i)` together with any lower bound of its own.
3. Entry points referenced from positive roots become **scheme variables** carrying their bounds (bounded quantification, `∀(α ≤ neg)`), renumbered 0..k in first-reference order. The other entry points are inlined as plain requirements. Mode entry points are handled the same way (ModeTerm trees over parameter modes, hash-consed).
4. Residual predicates are rewritten over scheme variables. Ground predicates were already checked.
5. The scheme is hash-consed into a `SchemeId`. Identity excludes origins, which live in a side `SchemeOrigins` keyed by (parameter, path) → definition-relative ExprId and are used only when rendering.

For example: `f(p) = p.a` gives `∀β. (p: [a: β, ..]) → β`. `f(p, q) = BLOCK{ t: LATEST{p, q}, t.a }` gives `∀β. ([a: β, ..], [a: β, ..]) → β`. `g(p) = p.a + 1` gives `([a: NUMBER, ..]) → NUMBER`, fully ground. A cycle found in the positive pass (e.g. a HOLD whose state contains itself) produces a "recursive shape" diagnostic; there are no μ-types.

**Instantiation** (once per syntactic call site):
- *Ground fast path.* All argument slots are Ground, so check the memo `(SchemeId, arg TypeIds) → (result, conflicts)`. On a miss, match the parameter terms against the arguments to bind each `Var(i) := ⊔` of the argument subterms at its entry positions, in OUT-scope order: a scoped argument is checked with its OUT binding already ground. Then check `arg ≤ subst(param)` and return `Ground(subst(result))`. This is exact for first-order schemes.
- *General path.* Create a fresh var per scheme variable with its instantiated bounds, then `Flow(arg_j, inst(param_j))`. PASSED and outer entries are wired as in §6, residual predicates are pushed into the caller, and the result is `inst(result)`. Copying touches only `HAS_VAR` nodes.
- Cost is O(non-ground scheme size). It never re-walks the callee body. The number of instantiations per syntactic call is exactly 1.

## 8. Diagnostics

- `Conflict { kind: u8, detail: u32 (field or tag SymbolId, or head mask), consumer: Prov, producer: Prov }`, 24 bytes with the ExprRefs inline.
- `Prov` is an index into a per-arena table of `ProvEntry { site: ExprRef{def: u32, local: u32}, via: u32 }`. `via` points to `Via { callee, param, path }`, which is resolved only at render time against the callee's *current* SchemeOrigins, so origins are never stale after a backdated edit.
- Reporting happens after the fixpoint, from the final state:
  - a producer member whose head, field or tag fails the consumer requirement ("`Text/trim` expects TEXT; NUMBER comes from `5` at line 3");
  - an empty meet of requirements, with both requirement sites ("`t` is used as TEXT at line 4 and as NUMBER at line 5");
  - a failed deferred predicate;
  - call-site errors with a note pointing to the callee requirement ("argument `p` of `f` is missing field `a`, needed by `p.a` in f.bn:12").
  Errors are sorted by (unit, span), deduplicated and capped per consumer. Errors inside a function are reported once, at the definition.
- **Lazy rendering.** Nothing is formatted on the success path. Text uses Boon vocabulary; no TypeVar, no rows.
- **Display order** (decision 7) is a lazy structural query over the HIR, `display::field_order(def, expr)`: a literal gives its authored order, a spread concatenates its parts in order, a WHEN/LATEST/LIST join merges arms left to right, and a call derives from the callee's result expression. It is never stored with types, so it adds no cost on the hot path. Hover re-solves the single definition when asked (about 10-50 µs).

## 9. Builtin and render-contract catalog

- It is a DSL file (`catalog/*.sig`). build.rs generates static tables: seeded SymbolIds, a TypeStore seed prefix loaded by one memcpy at session start (about 5k nodes; hash index built at startup, about 50 µs), and one `BuiltinSig` per entry.
- Each entry holds a scheme in the user-scheme format; per-parameter kind (Ordinary, Out, `Scoped{under}`) plus an optional default (today's registry has optional parameters with defaults, boon_typecheck/src/lib.rs:30437-30447); a mode rule (Pure = Combine, or a custom op); static-literal checks (page size 1..=10000, round quantum > 0, byte_count ∈ {1,2,4,8}, BITS count and width); an optional literal refiner function; and an effect class.
- Render constructors take closed contract records with Optional fields and PORT event fields, and return tagged render values.
- It replaces today's `BuiltinSignatureRegistry { entries: BTreeMap<&str, …> }`, which is rebuilt on every compile (boon_typecheck/src/lib.rs:30402-30410, about 150 builtin names).

## 10. Outputs consumed by lowering (dense side tables, no rich Type trees)

```rust
pub struct CheckedDef {
    pub scheme: Option<SchemeId>,        // FUNCTIONs
    pub entries: Box<[EntryPath]>,       // (param | PASSED | outer | scoped-result, field/elem path) per entry var
    pub expr_type: Box<[TypeId]>,        // by def-local ExprId; function bodies may reference Var(entry)
    pub call_inst: Box<[Inst]>,          // by def-local call index
    pub inst_args: Box<[TypeId]>,        // pool: per call, actual types of params, PASSED, scoped results (caller vocabulary)
    pub resources: Box<[Resource]>,      // HOLD/SOURCE/LIST/effect sites: (ExprId, TypeId)
    pub out_scopes: Box<[u64]>,          // scope-effect bitsets
    pub conflicts: Box<[Conflict]>,
}
pub struct Inst { pub callee: Callee /* Fn(FnId) | Builtin(BuiltinId) */, pub args: u32, pub n: u16 }
pub struct CheckedProgram { pub defs: IndexVec<DefId, CheckedDef>, pub root_types: IndexVec<RootId, TypeId>, pub store: Arc<TypeStore> }
```

Lowering monomorphizes from the roots. The key `(FnId, ground arg TypeIds)` determines every entry point by structural projection, and body types come from `store.subst(expr_type[e], entry_actuals)` (memoized). On TodoMVC the M3 probe found 3,382 closed calls collapsing to 168 classes, which suggests about that many specializations. Modes live inside TypeIds, so lowering reads event versus continuous from them. Typed storage gets exact shapes: Maybe gives an optional slot, and a multi-head type gives a tagged-union slot.

## 11. Incrementality, cancellation, parallelism

- **Function reuse key**: `(xxh3(position-free HIR of the body), xxh3(sorted (callee, SchemeId) list), catalog_version)`. On a hit, the retained CheckedDef is reused; spans are definition-relative and rebased at render. On a miss, re-solve. If the new SchemeId equals the old one, backdate: callers' keys do not change. This is sound because a scheme is a pure function of the key inputs, given a private arena and a confluent solve. No epochs are needed. This is early cutoff on result equality, which AGENTS.md currently forbids for the retracting kernel; the rule does not apply here.
- **Root group key**: a hash over the root HIR hashes, the SchemeIds of functions called from roots, and the outer-requirement sets. When it changes, the whole group is re-solved.
- **Warm body-only edit**: one function re-solve (about 10-50 µs), then a SchemeId compare, then stop.
- **Cancellation**: a `CancelToken` generation is checked before each definition and every 4,096 work items. Results are committed by an atomic swap at the end, so a cancelled solve drops its arenas and retained state is never half-updated.
- **Parallelism** (optional, off by default): functions within a topological level have no shared mutable state. Workers own arenas and intern into per-worker pending tables that are merged in FnId order at the level barrier, so TypeIds stay deterministic. Enable it only if measurements show more than about 5 ms of checker time.

## 12. Invariants and test oracles

- **Inline-equivalence oracle.** There is no recursion, so a reference checker that inlines every call and checks monomorphically must give the same accept/reject result and, after substitution, the same ground expression types as the scheme checker. This is the scheme-soundness oracle and replaces byte parity with the old checker.
- **Confluence test**: shuffle the order of the work items and constraint generation; schemes and conflict sets must stay identical.
- **Lattice-law property tests** for join, meet, subtype and the mode ops.
- **Conformance corpus**: the tspec z1-z7 probes, the verify/c10 probes t1/t2/p1/s01-s17/v12/v13/x01-x06, and the language probes q/r/s, with outcomes set by decisions.md.
- **Caps**: at most 2^20 vars per arena and at most 4,096 scheme nodes. Exceeding either is a "definition too complex" diagnostic, never a hang.
- **Counters**: ground-slot ratio, work items per expression, join vars per expression, instantiation memo hits, largest scheme.

## 13. Performance model (single thread, i7-9700K; estimates, not measurements)

Unit costs: ground expression 60 ns (visit plus 1-2 memo lookups); var expression 250 ns (allocation, about 2 flows, about 3 propagation steps); ground instantiation 80 ns; general instantiation 0.6 µs (about 6 non-ground nodes); compaction 80 ns per arena var; zonk and side tables 50 ns per expression. Assumed share of var-dependent expressions: 40% TodoMVC, 35% NovyWave, driven by PASSED and parameters; the ground-slot counter checks this. Inputs: TodoMVC 4,081 checked expressions, 121 FUNCTIONs, about 480 syntactic calls; NovyWave 15,650 checked expressions, 505 FUNCTIONs, about 2,408 calls.

| phase | TodoMVC | NovyWave |
|---|---|---|
| definition graph + levels | 0.02 ms | 0.1 ms |
| ground expressions | 2,450 × 60 ns = 0.15 ms | 10,170 × 60 ns = 0.61 ms |
| var expressions | 1,630 × 250 ns = 0.41 ms | 5,480 × 250 ns = 1.37 ms |
| instantiation (50% ground) | 0.16 ms | 0.82 ms |
| compaction | ~2,100 vars → 0.17 ms | ~7,000 vars → 0.56 ms |
| zonk + side tables | 0.20 ms | 0.78 ms |
| root group extra, predicates | 0.1 ms | 0.3 ms |
| **model total** | **≈1.2 ms** | **≈4.5 ms** |
| **target (3× margin)** | **≤4 ms** | **≤12 ms** |
| today (measured typecheck) | 398 ms | 457-475 ms |

- Warm: a body-only edit costs 0.05-0.3 ms of checker time. A root edit re-solves the root group: TodoMVC about 0.1 ms; NovyWave about 1-3 ms (3,792 of 11,997 non-comment lines are root code).
- Memory: arenas are reset between definitions, so peak arena size is bounded by the largest definition or the root group (about 2 MB on NovyWave). The TypeStore holds an estimated 10-20k nodes, about 1-2 MB. Retained side tables are about 0.3 MB. Checker heap should stay at or below 8 MB.
- Allocations: pooled arenas, at most about 2k allocations per compile.

## 14. Crate layout and size

| crate / module | contents | est. lines |
|---|---|---|
| `boon_symbols` (shared with the front end) | Interner, SymbolId, seeded prefix | 400 |
| `boon_types` | TypeStore, rows, tag rows, ModeRef/ModeTerm, widths, ground join/meet/sub/subst/field with memos, canonical printer (order supplied by the caller) | 2,500 |
| `boon_catalog` | DSL, build.rs generator, `BuiltinSig`, refiners, static checks, mode tables | 800 code + ~2,500 data |
| `boon_check::graph` | DefGraph, recursion and root-cycle diagnostics, levels | 600 |
| `boon_check::gen` | constraint generation (expr, call, when, record/spread, flow, out/pass, source) | 3,000 |
| `boon_check::solve` | arena SoA, bounds pool, propagation, invariance union-find, attribute solver, predicates, conflicts | 3,000 |
| `boon_check::scheme` | compaction, canonical numbering, hashing, instantiation with the ground fast path | 1,200 |
| `boon_check::root` | root group | 400 |
| `boon_check::output` | zonk, CheckedDef, subst API for lowering | 500 |
| `boon_check::diag` | conflict to Diagnostic, provenance/via, lazy text, display order | 1,500 |
| `boon_check::session` | reuse keys, backdating, cancellation, optional level parallelism | 700 |
| **total** | | **≈14.6k code + 2.5k catalog data** (vs 80,807 kernel + 109,295 boon_typecheck + 8,100 boon_checked + 24,234 kernel_oracle.rs) |

Input contract from the front end (boon_hir): a per-definition node arena with u32 ExprIds in source pre-order; resolved references (Local, Param, Root, Fn, Builtin, PassedRead, OutBinding); the local dependency order and local cycle diagnostics; definition-relative spans; a position-free content hash per definition; and literal values (exact rationals, text) for static checks.

## 15. Core Rust sketches

```rust
// boon_types
pub struct SymbolId(pub u32); pub struct TypeId(pub u32); pub struct RowId(pub u32);
pub struct TagRowId(pub u32); pub struct SchemeId(pub u32);
bitflags! { pub struct Heads: u16 { const TEXT=1; const NUMBER=2; const BYTES=4; const BITS=8;
    const RECORD=16; const TAGS=32; const LIST=64; const SET=128; const MAP=256; const PORT=512; } }
bitflags! { pub struct Mode: u8 { const C=1; const E=2; const A=4; } }
pub struct ModeRef(u32);                       // 0..=7 ground mask; else ModeTermId + 8
#[repr(C)] struct TypeNode { heads: Heads, flags: u8, nvars: u8, mode: ModeRef, members: u32, hash: u32 } // 16 B
enum Member { Text, Number, Bytes { width: u32 /*MAX=dyn*/ }, Bits { width: WidthRef }, Record(RowId),
    Tags(TagRowId), List { elem: TypeId, chain: AttrRef }, Set(TypeId), Map(TypeId, TypeId),
    Port(TypeId), Spread(SpreadId) /* schemes only */ }
#[repr(C)] pub struct Field { pub name: SymbolId, pub ty: TypeId, pub presence: Presence, pub _p: [u8; 3] }
#[repr(u8)] pub enum Presence { Required, Maybe, Optional }
#[repr(u8)] pub enum Tail { Closed, Open }
pub struct TagEntry { pub tag: SymbolId, pub payload: Option<RowId> }
pub struct TypeStore { nodes: Vec<TypeNode>, members: Vec<Member>, fields: Vec<Field>, tags: Vec<TagEntry>,
    index: hashbrown::HashTable<u32>, join: FxHashMap<(TypeId,TypeId),TypeId>, meet: FxHashMap<(TypeId,TypeId),TypeId>,
    sub: FxHashMap<(TypeId,TypeId),SubResult>, subst: FxHashMap<(TypeId,InstKey),TypeId> }

// boon_check::solve
#[derive(Copy, Clone)] pub struct Slot(u32);  // hi bit = Var
pub struct Var(u32);
pub struct Arena {
    uf: Vec<u32>, lo: Vec<BoundId>, hi: Vec<BoundId>, mode_lo: Vec<Mode>, mode_hi: Vec<Mode>,
    succ: Vec<EdgeList>, pred: Vec<EdgeList>, flags: Vec<VarFlags>, origin: Vec<Prov>,
    bounds: Vec<Bound>, bmembers: Vec<BMember>, bfields: Vec<BField>,
    work: Vec<Work>, props: Vec<ModeProp>, preds: Vec<Pred>, conflicts: ConflictSet,
    join_vars: FxHashMap<(Slot,Slot),Var>, meet_vars: FxHashMap<(Slot,Slot),Var>, prov: Vec<ProvEntry>, steps: u32 }
pub struct Bound { heads: Heads, members: u32 }
pub struct BField { name: SymbolId, slot: Slot, presence: Presence, prov: Prov }
enum Work { Flow(Slot, Slot, Prov), Lower(Var, BoundId), Upper(Var, BoundId), Mode(PropId) }
pub struct ModeProp { op: ModeOp, inputs: [Var; 4], n: u8, out: Var }
pub struct Pred { kind: PredKind, args: [Slot; 3], site: Prov }
pub struct Conflict { kind: ConflictKind, detail: u32, consumer: ExprRef, producer: ExprRef, via: u32 }
pub struct ExprRef { def: u32, local: u32 }     // the "u32 pair"

// boon_check::scheme
pub struct Scheme { params: Box<[TypeId]>, passed: Option<TypeId>, outer: Box<[(RootKey, TypeId)]>,
    outs: Box<[TypeId]>, scoped: Box<[u64]>, result: TypeId, vars: Box<[VarBounds]>, preds: Box<[SchemePred]> }

// boon_catalog
pub struct BuiltinSig { pub path: SymbolId, pub scheme: SchemeId, pub params: &'static [ParamSig],
    pub mode: ModeRule, pub statics: &'static [StaticCheck],
    pub refine: Option<fn(&LiteralArgs, &mut TypeStore) -> Option<TypeId>>, pub class: EffectClass }

// boon_check::session
pub struct ReuseKey { hir: u64, callees: u64, catalog: u32 }
pub fn check(s: &mut CheckSession, hir: &Hir, cancel: &CancelToken) -> Result<&CheckedProgram, Cancelled>;
```

Propagation core:

```rust
fn lower(&mut self, v: Var, b: BoundId) {
    let v = self.find(v);
    let Some(delta) = self.merge_lo(v, b) else { return };   // lattice join; None = unchanged
    if let Some(u) = self.hi[v] { self.check(delta, u) }     // head dispatch -> Flows or Conflicts
    for w in self.succ(v) { self.work.push(Work::Lower(w, delta)) }
}
```

## 16. What this deletes

The per-call-path kernel: InvocationKey on the caller's TypeVariableIds (owner.rs:15390-15396), residual frames, summary bytecode, the non-monotone `join_select_candidates` (solver.rs:2015-2034), `structural_widen`'s field union and open-object fallback, requirement staging and withdrawal with order receipts, the syntax_selected and contextual-hole machinery, and `Object{fields, open: bool}` without a row variable (term.rs:278-281). Also: interning of terms that contain variables, the per-compile builtin ABI, the EditorRich/RuntimePacked double path, rich `boon_checked::Type` export, and kernel_oracle's side tables.

## evidence
- Measured today (release binary, fresh process, diagnostics intent) on examples/todo_mvc_physical/RUN.bn: typecheck_ms 398.4, checked_expressions 4079, checked_calls 350, compiled_call_sites 10537, invocation_frames 5789 (302 reused), owner_local_constraints 82451, owner_unification_steps 169680, definition_modules 155, max_call_depth 12. Raw output: drafts/checker/todo_diag.json
- crates/boon_compiler_kernel/src/solver.rs:2015-2034: join_select_candidates returns union() while any candidate still has a variable, and a structural_widen fold otherwise. The join depends on solve state, so it is non-monotone.
- crates/boon_compiler_kernel/src/term.rs:278-281: `Object { fields, open: bool }` has no row variable, which forces deferred projections.
- crates/boon_compiler_kernel/src/owner.rs:15390-15396: InvocationKey includes actuals: [(TypeVariableId, ModeVariableId)], so callee bodies are instantiated per call path.
- `grep -rn generaliz crates/boon_compiler_kernel/src` returns 0 matches: the kernel has no generalization step.
- Probe design/checker/nested.bn (a FUNCTION declared inside a BLOCK) is rejected by today's `boon_cli check` with 'depends on unsupported owner ... Function inner'. FUNCTIONs are therefore module-level only, which justifies 2-level generalization.
- Probe design/checker/tagfield.bn (`c: Oklch[lightness: 1]`, `l: c.lightness + 1`) passes today, so field reads on tagged objects are existing semantics (relevant to owner question 2).
- Probe design/checker/poly2.bn passes today: one FUNCTION is used at NUMBER and at TEXT. Its body `[value: value]` resolves to the parameter, contrary to the documented shadowing rule (OUT plan:471-486). The new resolver must flag it (decision 6: field self-reference).
- verify_all.md C9: the function-only call graph has 0 cyclic SCCs on both fixtures. Function-containing SCCs exist only through root reads (5 nodes on TodoMVC, 22 on NovyWave). Store-reachable functions that read PASSED: 0/1 and 0/71. This supports root reads as implicit parameters, with roots solved as one monomorphic group.
- Source statistics (grep/awk over examples): TodoMVC has 3,647 lines, 121 FUNCTIONs, ~480 call sites, 235 WHEN, 157 PASSED, 54 spreads; function-body lines 3,050 vs root lines 151. NovyWave has 11,997 lines, 505 FUNCTIONs, ~2,408 call sites (824 to builtin namespaces), 748 WHEN, 748 PASSED, 117 SOURCE, 76 HOLD, 104 LATEST; function-body lines 7,246 vs root lines 3,792.
- examples/todo_mvc_physical/Theme/Theme.bn:89-93 uses `PASS: PASSED.theme_options`, so PASS replaces the context. RUN.bn:325 `scene: main_scene(PASS: [store: store, theme_options: theme_options])` is the root context.
- RUN.bn:55-63 appends `[title: title_to_save]` to a list of `[title, completed]` and then reads `item.completed`. Under decision 5 this is a Maybe-field read, i.e. an error, and the same pattern appears in the tspec probe z5_optional_read.bn.
- Theme/Classic.bn:22-60: Oklch payloads with different field sets are joined under WHEN arms (`Oklch[lightness: 0.97]` vs `Oklch[lightness, chroma, hue, alpha]`), so payload rows need Maybe presence and contracts need Optional fields.
- RUN.bn:39 `elements.new_todo_title_text_input.events.change.text`: the SOURCE payload is fixed by the element the source is bound to, which calls for an invariant PORT payload variable.
- docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:161-186 has the flow-mode table (Presence and SKIP); :479-484 lists the negative fixtures: heterogeneous collections, incompatible LATEST branches, incompatible HOLD update shape, THEN on a continuous value.
- docs/plans/BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md:539-553: typed signatures must record which ordinary parameters are evaluated under each OUT scope. This maps to per-OUT scope bitsets in schemes.
- crates/boon_typecheck/src/lib.rs:30402-30410: BuiltinSignatureRegistry is a BTreeMap<&'static str, entry> rebuilt per compile, with optional parameters and defaults at :30437-30447 (about 150 builtin names in the crate).
- crates/boon_data/src/lib.rs:229: Value::Object(BTreeMap<String, Value>). The runtime ignores field order, so order-free type identity is safe (decision 7).
- a_representations.md: TypeTermArena holds 129,998 terms (TodoMVC) and 112,006 (NovyWave), with 266k-310k intern requests, including terms that contain variables. The new store interns only ground and scheme terms.
- Line counts replaced: boon_compiler_kernel 80,807; boon_typecheck 109,295; boon_checked 8,100; boon_compiler/src/kernel_oracle.rs 24,234.

## owner_questions
- **question**: What does 'compatible' mean for the documented LATEST, HOLD and LIST/SET/MAP rules, which decision 6 says to enforce? Is it the same head set with records allowed to differ in fields, or identical record field sets? | **options**: (A) Every non-SKIP branch or element has the same head set. Any mix of tags or tagged objects counts as one head. Records with different field sets join with Maybe presence. (B) Records must also have identical field sets. (C) Same union rule as WHEN, with no extra check. | **recommendation**: A. `LATEST {1, TEXT}` and `LIST {1, TEXT}` are rejected. `LIST {[title, completed]} |> List/append(item: [title])` is accepted, and reading `.completed` afterwards is the error. | **why**: Decision 6 names 'LATEST over NUMBER vs TEXT', but the docs never define compatibility for records or tag unions. The rule is one deferred predicate (SameHeads), so it costs nothing to change. It does decide which examples break.
- **question**: Are field reads and spreads allowed on tagged objects and on unions of record-like members, e.g. `x.a` where x is `A[a] | B[a]` or `[a] | A[a]`? | **options**: (A) Allowed when every member has the field Required (TypeScript-like). (B) Only on a single plain record or a single tag. (C) Only on plain records. | **recommendation**: A. It is sound, costs nothing with kind partitioning, and today `Oklch[lightness: 1].lightness` already type-checks (probe tagfield.bn). | **why**: Decision 5 says reading a field that is not guaranteed present is an error, but it does not say whether a union of tagged objects that all share the field counts as guaranteed.
- **question**: Must static builtin arguments (BITS widths and counts, page size, round quantum, byte_count, Bytes/to_bits width) be direct literals at the builtin call, or may they come through FUNCTION parameters? | **options**: (A) Direct literals only, with literal-only arithmetic folded. (B) Any statically known value, which requires specializing per value. | **recommendation**: A. It matches decision 4 ('types never depend on values'). A value passed through a parameter gets 'must be a compile-time literal'. | **why**: B brings back per-value specialization of checking, the class of work being removed. BYTES[N] refinement from literal TEXT follows the same rule.
- **question**: Is `==` or `!=` between operands with no head in common (e.g. NUMBER == TEXT) an error? | **options**: (A) Error (Comparable predicate). (B) Allowed, always False. | **recommendation**: A. The result would be constant, so it is almost certainly a bug. Strictness is in the spirit of decision 6. | **why**: Probe z3_eq_disjoint exists, but the rule is not written down.
- **question**: Should one tag be allowed both bare and with a payload in the same union, e.g. `Panel` and `Panel[x: 1]`? | **options**: (A) Diagnostic: a tag is either bare or tagged within one union. (B) Two distinct variants. | **recommendation**: A. It keeps tag rows keyed by SymbolId alone, and the docs already call them distinct variants. | **why**: It decides the TagRow key and the behaviour of WHEN patterns.
- **question**: What happens with a SOURCE that no element ever binds, and with types that end up unconstrained (e.g. an empty `LIST {}` never appended to)? | **options**: (A) Reading a payload field of an unbound SOURCE is an error. Using it only as an event is fine (empty payload). Unconstrained types default to Never. (B) Any unconstrained type is an error. | **recommendation**: A | **why**: The spec asks for zero unresolved type variables but not for how they are reported. Defaulting to Never is sound and needs no fallback types.
- **question**: Are render and style contracts closed, so an unknown field is an error, while user FUNCTION parameters stay open (width subtyping)? | **options**: (A) Contracts closed, user parameters open. (B) Both open. | **recommendation**: A. A misspelt style field is otherwise silently ignored. | **why**: It decides the Tail of catalog contract records. The documented negative fixture 'wrong style field type' covers only field types.
- **question**: How should editor hints display a polymorphic FUNCTION type, given the rule not to expose TypeVar in user-facing text? | **options**: (A) Show requirement structure with 'same as `item.title`' references. (B) Lowercase placeholders (a, b). (C) Show only call-site instantiated types. | **recommendation**: A for signatures, plus instantiated types on hover over a call site. | **why**: With one type per function (decision 4), hints need some notation for scheme variables. The notation affects only rendering, not the checker.
- **question**: Please confirm the flow-mode transfer table the checker will encode. In particular: a continuous-input WHEN with a SKIP arm yields {continuous, absent}, which THEN rejects; LATEST is continuous only when some branch is always present; a List/map template body may not be SKIP. | **options**: (A) Encode TYPE_INFERENCE plan:161-186 literally, with the refinements above. (B) Owner supplies an amended table. | **recommendation**: A, with the table kept as data in boon_catalog so an amendment needs no solver change. | **why**: The docs specify modes per construct but not their combination under joins. Decision 6's '5 |> THEN {6}' needs exact rules to avoid rejecting valid code such as the TodoMVC title_to_save chain.
- **question**: Does PASS always replace the context, with extension written explicitly as `PASS: [...PASSED, x: e]`? | **options**: (A) Replace; extend via spread. (B) Implicit merge with the outer PASSED. | **recommendation**: A. The examples already use replacement (`PASS: PASSED.theme_options`, Theme/Theme.bn:89-93), and the doc's 'introduce, replace, narrow, or extend' is all expressible with explicit records. | **why**: It decides whether PASSED is an ordinary implicit parameter or needs a row-concatenation rule at every PASS site.

## risks
- **risk**: Biunification propagation churn or large schemes on pathological code (many joins of non-ground slots, long var-var chains). Plain Simple-sub needs expensive simplification to avoid scheme blowup. | **mitigation**: Kind-partitioned lattice bounds (merged, not listed); ground-first evaluation; memoized join and meet vars; canonical linear compaction; hard caps with a 'definition too complex' diagnostic; counters for work items per expression and largest scheme as gates.
- **risk**: A compaction or instantiation bug yields unsound or incomplete schemes. These are the hardest bugs to see because tests only exercise the call sites that exist. | **mitigation**: Inline-equivalence oracle: without recursion, a reference checker that inlines every call must give identical accept/reject and identical substituted ground expression types. Plus confluence tests that shuffle constraint order, and lattice-law property tests.
- **risk**: Flow-mode semantics are underspecified, so the strict checker rejects valid examples or accepts invalid ones. | **mitigation**: Mode ops are table data in boon_catalog. The owner confirms the table (owner question 9). The tspec probes z1/z4/z6/z7 are conformance tests. Monotonicity is property-tested.
- **risk**: The strict rules (Maybe-field reads, SameHeads, no narrowing) break TodoMVC and NovyWave: Theme get(request), item.completed, Oklch optional fields. | **mitigation**: Example migration is its own work item, scheduled before perf gates. Perf is measured on the migrated fixtures. The old checker is used only as a differential oracle for probes whose semantics did not change.
- **risk**: Error messages for failures deep in generic call chains are confusing: the requirement comes from a callee several levels down. | **mitigation**: The via chain stores (callee, param, path) and renders one hop by default ('needed by p.a in f.bn:12'), expandable on request. Errors inside a function are reported once, at the definition.
- **risk**: SOURCE payload invariance or the root group create order dependencies that break incremental assumptions. | **mitigation**: The root group is always re-solved as a whole (estimated 1-3 ms on NovyWave, well inside 16.7 ms). Only function schemes use early cutoff.
- **risk**: Parallel level checking makes TypeId numbering nondeterministic and plan bytes flaky. | **mitigation**: Off by default. When enabled, per-worker pending interners merge at the barrier in FnId order. A test requires identical plans across thread counts.
- **risk**: Dependence on the front-end HIR contract (resolved names, local dependency order, position-free hashes, definition-relative spans) and on lowering's handling of Maybe fields and multi-head unions in typed storage. | **mitigation**: Freeze the boon_hir interface (section 14) early with the front-end panel. Give lowering precise types and a subst API, and agree storage for Maybe (an optional slot) and multi-head types (a tagged slot) with the backend panel.

## work_items
- **id**: CK1 | **title**: boon_types: TypeStore and ground algebra | **description**: Hash-consed TypeNode, rows, tag rows, ModeRef/ModeTerm, widths; ground join, meet, sub (with failure path), field and subst with FxHash memos; seed-prefix loading; canonical printer that takes an order callback. | **size**: M | **depends_on**: boon_symbols interner (front-end panel) | **acceptance**: Property tests pass for join/meet associativity, commutativity and idempotence, join as least upper bound, and subtyping reflexivity and transitivity on random types. Interning is order-free ([a,b] and [b,a] give the same TypeId). A memo hit costs 25 ns or less in a microbenchmark.
- **id**: CK2 | **title**: boon_catalog DSL and generator | **description**: Signature DSL, build.rs generating seeded symbols, TypeStore prefix and BuiltinSig tables. Port all current builtin and render-constructor signatures (about 150 builtins plus Element/Scene contracts), including OUT, Scoped and optional parameters, mode rules, static-literal checks and refiners. | **size**: L | **depends_on**: CK1 | **acceptance**: Every builtin referenced by examples/ resolves. Catalog load is under 0.1 ms per process. Nothing is rebuilt per compile.
- **id**: CK3 | **title**: Inference arena and propagation | **description**: SoA vars; shallow bounds pool; Flow/Lower/Upper work loop with delta propagation; invariance union-find; memoized join and meet vars; attribute sub-solver (modes, list chain); deferred predicates; order-independent conflict set; cancellation polling. | **size**: L | **depends_on**: CK1 | **acceptance**: A confluence test (random shuffles of constraint order) yields identical final bounds and conflict sets. No TypeStore interning of var-containing terms. Work items per constraint stay at 4 or fewer on the fixtures.
- **id**: CK4 | **title**: Constraint generation for the whole language | **description**: Every construct in the section 5 table, including spreads, WHEN patterns with payload binders, THEN/HOLD/LATEST/WHILE modes, SOURCE ports, LIST/SET/MAP, interpolation, operators, calls with pipe, PASS and OUT, BLOCK, and FLUSH. Also the syntactic strict rules: unused parameters, one OUT producer, OUT scope-effect bitsets. | **size**: L | **depends_on**: CK2, CK3 | **acceptance**: tspec z1-z7 and the language probes (q/r/s) produce the outcomes set by decisions.md, with positioned diagnostics. The ground-slot ratio is reported.
- **id**: CK5 | **title**: Schemes: compaction and instantiation | **description**: Negative and positive compaction passes, canonical numbering, SchemeId hash-consing with origins kept aside, residual predicates, general instantiation, and the ground fast path memoized on (SchemeId, argument TypeIds). | **size**: M | **depends_on**: CK3 | **acceptance**: The inline-equivalence oracle (reference checker that inlines all calls) agrees on accept/reject and on substituted expression types for all examples and probes. Exactly one instantiation per syntactic call.
- **id**: CK6 | **title**: Definition graph, driver and root group | **description**: DefGraph at root-field-path granularity; recursion and root instantaneous-cycle diagnostics; Kahn levels; root reads as implicit parameters with forwarding; PASSED forwarding; the root group arena; PORT payload defaulting. | **size**: M | **depends_on**: CK4, CK5 | **acceptance**: Checks TodoMVC and NovyWave (migrated) end to end. The C9 SCCs need no special casing. Recursive probes give a positioned recursion diagnostic.
- **id**: CK7 | **title**: Diagnostics and display | **description**: Conflict-to-diagnostic mapping in Boon vocabulary; provenance and via rendering; lazy text; sorting, deduplication and capping; lazy structural display order following decision 7. | **size**: M | **depends_on**: CK6 | **acceptance**: No string formatting on the success path (allocation counter). Record types display in written order, and merged types in left-to-right first-appearance order, for a golden corpus.
- **id**: CK8 | **title**: Lowering side tables | **description**: CheckedDef (expr_type over entry vars, call_inst, inst_args, resources, out_scopes), root_types, and the subst API with a memo. | **size**: S | **depends_on**: CK6 | **acceptance**: The lowering panel's monomorphizer consumes the tables with no other checker queries. The number of distinct specializations on TodoMVC is reported.
- **id**: CK9 | **title**: Session: reuse keys, early cutoff, cancellation | **description**: Position-free HIR hash plus callee SchemeIds as the key; backdating on an equal SchemeId; root-group key; definition-relative diagnostics rebased at render; atomic commit; cancellation tokens. | **size**: M | **depends_on**: CK6, CK7 | **acceptance**: A body-only edit re-solves exactly one definition, with checker time of 0.3 ms or less. Warm results equal a cold check on an edit corpus (differential test). A cancelled check leaves retained state unchanged.
- **id**: CK10 | **title**: Conformance corpus and example migration | **description**: Encode the verify/c10, tspec and language probes with expected outcomes under decisions.md and the answers to the owner questions. Migrate the examples that break: Theme get(request) split per kind and refactored per decision 4, the item.completed append, and others found by the checker. | **size**: L | **depends_on**: CK4 (the corpus can start earlier) | **acceptance**: All examples compile with zero diagnostics under the new checker. Every probe has a pinned expected result (a set of span and kind pairs, not bytes).
- **id**: CK11 | **title**: Perf counters and gates | **description**: Release-visible counters: ground-slot ratio, work items per expression, join vars per expression, instantiation memo hits, largest scheme, arena peak. A bench of cold and warm runs on counter, TodoMVC and NovyWave. | **size**: S | **depends_on**: CK6 | **acceptance**: Checker time is at most 4 ms on TodoMVC and 12 ms on NovyWave cold, single thread, on the reference machine.
- **id**: CK12 | **title**: Optional level-parallel checking | **description**: Per-level worker arenas, per-worker pending interners merged at the barrier in FnId order. | **size**: S | **depends_on**: CK9 | **acceptance**: Identical CheckedProgram and plan bytes at 1 and N threads. Enabled only if CK11 shows more than 5 ms of checker time.

## deletions
- **what**: boon_compiler_kernel: per-call-path owner compilation (InvocationKey, residual frames, summary bytecode), the retracting solver, requirement staging and withdrawal with order receipts, structural_widen and the open-object fallback, syntax_selected and contextual-hole machinery, interning of terms that contain variables | **size**: 80,807 lines (whole crate)
- **what**: boon_typecheck: owner orchestration, DTO projection and repacking, the per-compile BuiltinSignatureRegistry and RenderContractRegistry (replaced by the static boon_catalog), and the cfg-gated legacy owner oracle | **size**: 109,295 lines (about 58k of them cfg-gated legacy)
- **what**: boon_checked rich Type trees (BTreeMap<String, Type> fields, field_order-inclusive Eq/Hash) as a checker output | **size**: 8,100 lines
- **what**: boon_compiler/src/kernel_oracle.rs side tables and the EditorRich/RuntimePacked double construction path | **size**: 24,234 lines

## perf_targets
- **metric**: Cold checker time (constraint generation + solve + generalization + side tables), TodoMVC, single thread | **target**: 4 ms or less (model 1.2 ms) | **basis**: 4,081 expressions; 40% var-dependent at 250 ns, 60% ground at 60 ns; 480 instantiations; 3x margin. Today 398 ms typecheck (measured).
- **metric**: Cold checker time, NovyWave, single thread | **target**: 12 ms or less (model 4.5 ms) | **basis**: 15,650 expressions, 2,408 calls, 505 functions; same unit costs; 3x margin. Today 457-475 ms.
- **metric**: Cold checker time, counter.bn | **target**: 0.2 ms or less | **basis**: 130 expressions; fixed costs are only the catalog seed memcpy and the hash index build.
- **metric**: Warm body-only function edit (checker share) | **target**: 0.3 ms or less | **basis**: One definition re-solve (about 10-50 µs), a SchemeId compare, diagnostics rebase.
- **metric**: Warm root-value edit, NovyWave (checker share) | **target**: 3 ms or less | **basis**: Root group re-solve of 3,792 of 11,997 non-comment lines, mostly ground forward evaluation.
- **metric**: Instantiations per syntactic call | **target**: exactly 1 | **basis**: Today 10,537 compiled call sites vs 350 checked calls on TodoMVC (30x or more).
- **metric**: Solver work items per checked expression | **target**: 4 or fewer | **basis**: Today 82,451 constraints and 169,680 unification steps for 4,079 expressions (about 20 and 41 per expression).
- **metric**: TypeStore intern requests per checked expression | **target**: 1.5 or fewer, with zero var-containing terms interned | **basis**: Today about 65 per expression (266,605 on TodoMVC).
- **metric**: Checker heap peak, NovyWave | **target**: 8 MB or less; 2k allocations or fewer | **basis**: Arenas reset per definition; store estimated at 10-20k nodes. Today 7.46M allocations per verified compile.

---

# Adversarial review

## area
checker

## verdict
needs_changes

## major_issues
- **issue**: Render values are left unspecified in a way that breaks both the semantics and the performance model. The design says render constructors 'return tagged render values'. The documented and implemented shape is different: an untagged record `[kind: Button]` (BOON_TYPE_NOTATION_AND_INSPECTOR.md, runtime shape table), built today as a closed record of all constructor arguments plus `kind` (crates/boon_typecheck/src/lib.rs:31887-31909). Switching to tags is a semantic and display change that was never raised with the owner. There is a second problem: if a render value's type carries its arguments, whether as a record or as a tag payload, every view function's result type holds its entire element subtree. Every mixed `items: LIST { a(), b(), c() }` then joins deep trees, and the general instantiation path copies those HAS_VAR subtrees at each call level (NovyWave has 373 Element constructor calls, 252 style records and call depth 22). That invalidates the model's '~6 non-ground nodes per general instantiation' and '50 ns zonk per expression'. It also makes the 4,096-node scheme cap likely to reject valid view functions with 'definition too complex'. | **evidence**: crates/boon_typecheck/src/lib.rs:31887-31909 (constructor_shape pushes every argument field plus kind); :31879-31885 (renderable = RenderContract | [kind: registered] | NoElement); :31691-31694 (renderable kinds depend on active_root); :31745/:32239-32244 (Element/stripe kind narrowed by the direction literal). Counts from grep: NovyView.bn+RUN.bn have 373 `Element/*(` calls; todo_mvc_physical/RUN.bn:295-330 nests stripes inside LIST items. | **fix**: Add an owner question and a catalog decision. Recommended: an opaque RENDER head (an 11th head) whose only visible attribute is `kind` (displayed as `[kind: Button]`). Argument contracts are checked at the constructor call, including PORT binding through `element:`, and are never carried into the result. Slot contracts become `RENDER | NoElement` (plus TEXT|NUMBER for `child:`), with the renderable kind set chosen per active root. Replace stripe direction narrowing with `kind: Row | Stack` unless the owner keeps a literal refiner. This keeps element types O(1) and makes mixed element LISTs trivially SameHeads.
- **issue**: The mode sub-solver is claimed to be 'monotone and table-driven', and the confluence and unique-fixpoint arguments depend on that. The documented LATEST rule ('continuous if it has a continuous fallback', TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:185) is anti-monotone in the lower-bound set lattice. With ∅-initialised mode_lo, out([∅,{E}]) = {E,A} while out([{C},{E}]) = {C}. That is not an inclusion even though ∅ ⊆ {C}. In a cyclic group the solve can therefore stop at a least fixpoint that depends on the order of work. The worked example `g(p) = p.a + 1 → NUMBER, fully ground` is also wrong: its result mode is Combine(mode(p.a), C), so almost every scheme is mode-polymorphic. | **evidence**: TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:176-187 (flow table); design §4 'ModeProp re-applies its monotone op'; BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md:508-510 (only SOURCE/HOLD/publication/async effects break cycles) | **fix**: Compute modes producer-first in one forward pass in dependency order. Legal cycles pass only through HOLD, whose output is the constant {C}, or through SOURCE/effects, which are constant {E}. Seed those nodes and evaluate the rest in topological order, so the non-monotone LATEST op is harmless. If a fixpoint is kept, split the mode into a may-absent bit (least fixpoint) and an always-present bit (greatest fixpoint) with explicit directions. In either case, restrict the property tests to the ops that really are monotone, and state that scheme result modes are ModeTerms over entry modes.
- **issue**: Provenance conflicts with ground-first hash-consing and with the order-independence claim. Ground TypeIds are hash-consed without origins. A ground join such as `[title, completed] ⊔ [title]` therefore cannot say which producer lacked `completed`, yet §8 promises messages like 'NUMBER comes from `5` at line 3' and 'the origin of absence'. For var bounds, 'Maybe records the origin of the producer that lacked the field' and the per-field `Prov` keep whichever producer arrived first, so the conflict tuple (consumer, producer, …) depends on the order of work. §4 also records conflicts during Lower/Upper delta propagation, while §8 says reporting happens only from the final state. | **evidence**: Design §3 (the TypeStore holds no provenance), §4 BField{prov} and 'Maybe records the origin', §8 'Reporting happens after the fixpoint, from the final state'; the TodoMVC case RUN.bn:55-63 is exactly a ground join whose error needs the append site. | **fix**: Keep provenance off the hot path entirely: no Prov in BField or TypeStore. On failure, re-solve only the failing definition (or the root group) in an explain mode that tracks provenance as sets, reduced to the minimum ExprRef so the result is canonical, and render from its final state. Propagation may detect conflicts but must never choose which pair to blame. This removes 4-8 bytes per bound field and makes the confluence test meaningful.
- **issue**: The checker's output contract does not match what the lowering design expects, and the two designs duplicate each other's checks. d_lowering's LowerInput expects one `ty: TypeId` per HIR node, recomputes flow modes per inlined use, and emits MachinePlan v11. v11 cannot represent the checker's precise types: DataTypeFieldPlan has no presence flag, and DataTypePlan::Union allows several records, whereas the checker merges records into one with Maybe fields. d_lowering also requires persistence type_fingerprint and schema_hash to stay bit-identical for unchanged types. Separately, both designs claim the OUT one-producer rule and the instantaneous-cycle Tarjan pass. | **evidence**: crates/boon_plan/src/lib.rs:413-416 (DataTypeFieldPlan {name, data_type}, no presence); :345-347 (Union{members}); d_lowering.md:83-93 (LowerInput has no inst_args/subst), :67 (fingerprint must equal today's), Phase A steps 6-7 (OUT nets and Tarjan); checker design §5 ('the OUT rule of exactly one producer') and §6 (root instantaneous cycles). | **fix**: Agree the checker→lowering contract before CK8. Either the checker exports ground per-node types per specialization, or LowerInput gains `call_inst`, `inst_args` and a memoized `subst` that lowering applies along each inlined path. Decide with the owner how Maybe fields and merged records map to v11 DataTypePlan and what that does to schema_hash. Give each check a single owner: lowering Phase A already builds OUT nets and does Tarjan over inlined instances, so drop those from the checker and keep only resolver-level local cycle checks and recursion.
- **issue**: Distributed checking is reduced to 'an edge check on qualified reads'. The product has cross-role value reads, cross-role SOURCE event forwarding, cross-role function calls and bidirectional references. The legacy checker handles these today with a fixed-point loop of at least 9-12 full role checks per compile. The new checker must replace that, or it cannot be deleted. | **evidence**: crates/boon_compiler/src/distributed_compiler.rs:3660-3700 (fixture: `increment: Client/store.increment`, `doubled: Server/double(value: count)`, Server reads Session and Session reads Server); :1245-1268 (interface fixed point); boon_syntax/src/lib.rs:278-282 (distributed_role_paths: 'values and calls'); verify_all C7 (≥9 legacy checks per distributed compile). | **fix**: Check all roles in one session. FnIds are role-qualified, so a cross-role call simply instantiates the producer role's scheme. Cross-role reads become variables in a single combined root group, which removes the fixed point. Add predicates: WireEligible for values that cross roles (no PORT except event forwarding, no FLUSH status, none of the host capabilities the FLUSH/persistence docs forbid on the wire), mode agreement across the wire, and role adjacency. Add a work item and a fjordpulse-based conformance test.
- **issue**: Language constructs and documented rules are missing from the §5 generation table and the predicate list. | **evidence**: examples/migrations/counter/v2.bn:13-16 and BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1134-1168 (DRAIN/DRAINING; conversion 'may not read time, randomness, sources, host resources, mutable state, or perform effects'); BOON_LANGUAGE_FOUNDATIONS_PLAN.md:1547-1552 (FLUSH payload must be a closed tag union with no collections or flow state), :1596 (new:/if:/HOLD bodies are not boundaries), :1628 ('A potentially flushing HOLD initializer is invalid'); examples/server_http_echo.bn and boon_typecheck lib.rs:20539 (host_ports/outputs root contracts); TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md:184 ('incompatible branches' clear the chain), :192-198 (orderable keys); TYPE_INFERENCE plan:280-281 and :479-480 (nested-authority ownership); BYTES_SEMANTICS.md:111-125 and :157 (type-dependent static checks). | **fix**: Add rows and predicates for:
- DRAIN/DRAINING (the type of the drained path), plus a per-function effect/purity summary in schemes to enforce pure conversions.
- A may-flush bit per expression; FlushPayloadEligible; rejection of HOLD initialisers that may flush; a boundary search that skips new:/if:/HOLD bodies.
- Root contracts for document/scene/outputs/host_ports, with host-port SOURCE payload types.
- An Orderable predicate for sort_by/then_by keys and `<`/`>`, and a chain attribute that carries key identity (not just two states) so incompatible joins clear it.
- Nested collection-authority rules, or an explicit handoff of them to lowering.
- Residual predicates for type-dependent BYTES/BITS checks.
- Non-exhaustiveness for WHEN over TEXT/NUMBER with only literal patterns: a closed TEXT head cannot express it.
- **issue**: The claim that 'every referenced local is already final when it is read' ignores cyclic local scopes. Record-literal siblings inside function bodies form cycles; in new_todo, title_to_update ↔ edited_title ↔ title. Any cycle that is legal (through HOLD) needs variable slots and SCC grouping at the local level. Detecting instantaneous cycles that cross a function (store.todos → new_todo → store.all_completed, where stateful builtins such as Bool/toggle may break the cycle) needs a per-function instantaneous input→output summary. §6's Tarjan over root→root reads has no such summary. | **evidence**: examples/todo_mvc_physical/RUN.bn:120-160 (title_to_update, title, edited_title/draft_title); verify_all C9 (TodoMVC SCC {new_todo, store.todos, active_count, completed_count, all_completed}); a local mutual cycle probe (scratchpad rev_checker/local_cycle.bn) fails today with an internal error. | **fix**: Specify local SCC handling: the resolver reports instantaneous SCCs; temporal SCCs are generated with variables and solved together. Assign cross-function instantaneous-cycle detection to lowering Phase A, which inlines and already runs Tarjan, instead of inventing function dependency summaries in the checker. Send the THEN-trigger/body question (tspec2 cyc1/cyc2) to the language panel.
- **issue**: A session-persistent, append-only TypeStore and interner conflicts with the proposed warm gates. Memo tables, symbols and types grow for the life of a playground session, which can break 'Session RSS growth ≤ 8 MiB'. The 'sub' memo returns the first failing path in SymbolId order, and compaction numbers entries by first visit over SymbolId-ordered rows. SymbolIds depend on session history, so warm diagnostics (which missing field gets reported) can differ from cold, and that fails the warm==cold oracle. | **evidence**: d_process.md:175 (RSS growth ≤ 8 MiB), :206 (O5 warm==cold); design §3 ('Row order is by SymbolId value'; sub memo 'carries the first failing path'). | **fix**: Use a per-compile TypeStore overlaid on a shared immutable catalog base; at 1-5 ms cold this costs nothing and makes TypeIds deterministic per compile. If a persistent store is kept, add generation GC. In either case, anything that reaches diagnostics must be ordered by name bytes or source position, never by SymbolId.
- **issue**: The design's own numbers do not justify the incrementality and parallelism layers (CK9, CK12), and the reuse key as specified is incomplete. Cold checking is modelled at 1.2 ms (TodoMVC) and 4.5 ms (NovyWave) against a 16.7 ms warm budget, and lowering Phase A re-runs in full anyway. CK9 would add stable DefIds (retained ExprRef{def,local} uses dense DefIds), span rebasing, SchemeId hash-consing and a warm==cold differential. Its key, (position-free HIR hash, callee SchemeIds, catalog), omits resolution inputs: bare names resolve by a global unique-suffix rule, so adding a root field elsewhere can re-resolve or break a function whose text did not change. | **evidence**: boon_typecheck/src/lib.rs:14385-14393 (unique-suffix bare-name resolution); verify_all C9 (NovyWave's only function-containing SCC exists because of it); design §11 ReuseKey{hir, callees, catalog}. | **fix**: Ship cold-only checking with cancellation first. Keep the property that a scheme is a pure function of (resolved HIR, callee schemes), so CK9 can be added if measurements demand it. If CK9 is built, hash the resolved HIR with stable root/function keys and make definition identity stable across edits. Remove CK12 from the plan.
- **issue**: Three recommendations in the owner questions conflict with the docs or are driven by representation convenience. Q1-A (records with different fields join with Maybe in LIST/LATEST/HOLD) contradicts the documented 'homogeneous element/key/value shapes, including closed tagged unions' and the negative fixture 'heterogeneous collection shape'. Q5-A (forbid bare and payload forms of one tag in a union) is justified by keeping tag rows keyed by SymbolId alone, but the docs say bare and tagged forms are distinct variants, which suggests they may coexist. PORT invariance via union-find leaves PORT ⊔ PORT with different payload variables undefined: for `WHEN { True => a.events.press, False => b.events.change }`, unification forces the payloads to be equal and produces a false conflict. | **evidence**: TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:272-273, :479-480, :136-139; design §2 (TagEntry keyed by SymbolId), §4 (union-find for PORT/HOLD/BITS). | **fix**: Present Q1 with the doc evidence. Option B puts the TodoMVC error at the append site (RUN.bn:55-63) and keeps list rows and HOLD state free of optional slots, which matters for v11 storage, but B depends on the render-value decision because mixed element LISTs must stay homogeneous. For Q5, supporting both forms costs one bit per TagEntry; let the owner decide on semantics alone. Replace union-find with the standard polarised encoding PORT(ρ⁺, ρ⁻), where join is (⊔, ⊓), and make HOLD's state binder alias the output slot. That removes union-find from the solver entirely and gives PORT joins a defined meaning.
- **issue**: The compaction description is under-specified to the point of being unsound as written. Negative roots are the parameters, but only vars 'reached as a child position' are marked ENTRY. For `f(x) = BLOCK { t: x, t }` the positive pass computes pos(t) = lo(t) ⊔ pos(x) = Never unless the parameter roots are entries themselves. Nor does the text say how var-var edges between two entry vars (α ≤ β) survive compaction. | **evidence**: Design §7 steps 1-3. | **fix**: State the polarity rule explicitly: a variable is quantified iff it occurs both positively and negatively, parameter roots included, and var-var edges between retained vars become negative ⊓/positive ⊔ references. Keep the inline-equivalence oracle, but restrict its accept/reject equality to called functions, because errors are reported at the definition even for functions that are never called.

## missing
- The render value representation and renderable slot contracts: NoElement, a renderable kind set that depends on the active root (RenderContractRegistry::active_root, boon_typecheck lib.rs:31691), and replacing the Element/stripe direction narrowing (lib.rs:31745).
- The root contracts document/scene/outputs/host_ports, and SOURCE payload types supplied by host ports (host_port_table, lib.rs:20539; examples/server_http_echo.bn).
- DRAIN/DRAINING typing, the path-only source rule, and a per-function purity/effect summary to enforce pure drain conversions (BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1134-1168).
- The FLUSH may-flush effect bit, payload eligibility, rejection of HOLD initialisers that may flush, and a boundary search that skips new:/if:/HOLD bodies (FOUNDATIONS:1547-1628).
- Multi-role checking: cross-role calls, forwarding of cross-role SOURCE events, a WireEligible predicate, mode agreement across the wire, and deleting the fixed point at distributed_compiler.rs:1245-1268.
- An editor type-hint table API to replace boon_typecheck::project_type_hints*, which boon_editor calls in production (language.rs:360-363). It needs whole-file inline hints (parameters, returns, pipeline results, tag-union fields), a source payload shape table and render slot data, not only a lazy per-definition hover.
- A source-payload ABI export for hosts and runtime (today project_source_payload_abi_paths/types).
- Deriving the host-effect catalog from boon_effect_schema, which 12 crates use (intent defaults/constraints, Variant results), instead of a second hand-written DSL.
- An Orderable predicate for sort keys and ordering operators, and a chain attribute that can detect incompatible branches (TYPED_LIST plan:184-198).
- Rules for nested collection-authority ownership, or an explicit handoff to lowering (TYPE_INFERENCE plan:280-281, :479-480).
- Residual predicates for type-dependent BYTES/BITS static checks, e.g. Bytes/to_bits width versus the fixed input length, and statically out-of-range positions on fixed BYTES.
- WHEN over TEXT/NUMBER with only literal patterns must be reported as non-exhaustive.
- Handling of local temporal SCCs inside function bodies, and ownership of cross-function instantaneous-cycle detection.
- Checking migration predecessor stages (the persistence_only compile of predecessor sources used for migration edges).
- Mapping checker types to v11 DataTypePlan and persistence type_fingerprint/schema_hash (no presence bit in DataTypeFieldPlan).
- An explicit, positioned rule for a FUNCTION nested inside a BLOCK. Today it fails with an internal 'dense kernel checked construction' error, not a language rule, so it must be listed for the owner under decision 8.
- Display notation for Maybe (value side) and Optional (requirement side) fields. BOON_TYPE_NOTATION_AND_INSPECTOR.md has no spelling for them, and decision 7 display depends on it.
- Display order for requirement types and for spreads of parameters. The order at a call site depends on the argument expression, but display::field_order only follows 'the callee's result expression'.

## unrealistic
- The ground-share assumptions (60% ground expressions, 50% ground instantiations) and the 'literal-tag Theme calls' rationale. Any callee that transitively reads PASSED or a root can never take the ground fast path, because π_caller and outer entries are always vars and the root PASS record contains root vars. Every TodoMVC Theme function reads PASSED.theme_options (Theme/Theme.bn:73-94; Classic.bn:22+). The totals still probably fit (even at 90% var, NovyWave is about 5-7 ms), but the fast path and instantiation memo pay off far less than claimed.
- '~6 non-ground nodes per general instantiation', '50 ns per expression zonk' and a hard 4,096-node scheme cap, given that element-tree result types grow with the view subtree (NovyWave: 373 constructor calls, depth 22).
- The 'monotone, table-driven' mode transfer functions (LATEST's continuous-fallback rule is anti-monotone) and therefore the stated confluence of the attribute solver.
- 'Order-independent conflict sets' while Prov records whichever producer arrived first, for Maybe origins and field producers.
- Warm==cold equal diagnostics with a session-persistent SymbolId order that decides the reported first failing path.
- Input counts: the design uses about 480 and 2,408 calls and 121 functions, while measured checked_calls are 350 and 1,821 (todo_diag.json; a_kernel_solver.md) and TodoMVC has 120 FUNCTIONs. The summary's '≈13k lines' contradicts §14's '≈14.6k'.
- 'Same accept/reject' for the inline-equivalence oracle, which cannot hold for uncalled functions or for errors reported at the definition.

## simplifications
- Make render values an opaque RENDER head carrying only `kind`. Element types become O(1), mixed item LISTs join trivially, and schemes stay small.
- Drop CK9 (session reuse, backdating, SchemeId-keyed cutoff) and CK12 (parallelism) from the initial plan and build the per-compile store. That also fixes RSS growth and determinism. Add CK9 only if a measured cold time exceeds the warm budget.
- Build only the general biunification path first. Add the ground fast path and the (SchemeId, arg TypeIds) memo only if a profile shows instantiation is hot. SchemeId hash-consing is needed only for cutoff.
- Remove union-find from the solver: use a polarised PORT(ρ⁺, ρ⁻), alias HOLD's state binder to the output slot, and treat BITS width as a flat attribute lattice.
- Take provenance out of bounds and TypeStore entirely, and re-solve the failing definition in an explain mode only when there are diagnostics.
- Compute modes in one forward pass in dependency order, with HOLD/SOURCE as constant seeds, instead of a general attribute fixpoint solver with ModeProp propagators.
- Build the catalog as plain Rust tables in a OnceLock (about 50 µs once per process), with host effects generated from boon_effect_schema. Drop the .sig DSL, the build.rs codegen and the memcpy seed image.
- Give each diagnostic class a single owner: leave OUT-producer and instantaneous-cycle checks to lowering Phase A, which already inlines and runs Tarjan, and keep only the resolver-level local checks and recursion in the checker.
- Turn the scheme and arena caps into telemetry and debug assertions rather than user-facing 'definition too complex' errors on valid programs.

## extra_owner_questions
- What is a render constructor's value type: (a) today's closed record of arguments plus `kind` (documented `[kind: Button]`), (b) an opaque renderable showing only `kind`, or (c) a tag with a payload? And does Element/stripe keep its direction-dependent `kind: Row` or `Stack`, or always return `Row | Stack`?
- How should Maybe (may be absent) and Optional (may be omitted) fields be displayed in hints and diagnostics? No notation exists today.
- What should BYTES[n] ⊔ BYTES[m] with n ≠ m in WHEN arms do: an error, a union, or silent widening to dynamic BYTES? Widening changes the storage class (fixed byte banks).
- May WHEN or LATEST arms produce two different SOURCE ports, e.g. `a.events.press` or `b.events.change`? If so, is the payload type the join of both payloads?
- Should a FUNCTION declared inside a BLOCK be a positioned language error? Today it is an internal compiler error.
- Which values may cross roles (wire eligibility: SOURCE events, collections, tags with payloads), and does a cross-role call such as `Server/double(value: count)` execute on the producer role?
- For cycle legality, are the THEN trigger and body dependencies instantaneous? The OUT plan (:508-510) lists only SOURCE/HOLD/publication/async effects as boundaries, which makes TodoMVC new_todo's title_to_update/edited_title cycle illegal.
