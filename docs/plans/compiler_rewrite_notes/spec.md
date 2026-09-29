> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

# Typing spec: Boon static semantics v2 for the strict, greenfield checker

## area
Typing spec: Boon static semantics v2 for the strict, greenfield checker

## summary
This is the typing contract for the new strict checker, written to follow decisions.md exactly: no static narrowing, sound unions, strictness, and record identity that ignores field order. Every type is a flow (continuous / present-or-absent / absent) paired with a data type. Flow is tracked at every nested field, because records such as `store` mix SOURCE fields and HOLD fields. The union model is kind-partitioned. Each kind (NUMBER, TEXT, BYTES, BITS, tag set, record, LIST, SET, MAP, port, element) contributes at most one member. The record member is a single record whose fields are Required or Optional. The tag member is one tag set, and each tagged variant carries its own payload record. Boon records are untagged and cannot be matched by pattern, presence test or runtime type test, so a TypeScript-style union of record shapes would add precision no program could use; the chosen join is instead a cheap, canonical, hash-consable semilattice (associative, commutative, idempotent). FUNCTIONs are the only polymorphic entities. Each is checked once, bottom-up over the acyclic call graph, into a scheme: predicates on its parameters plus an implicit PASSED row, the OUT scope effects, and a result term. The scheme is instantiated at each call site, and root values are monomorphic outer variables solved per small SCC. WHEN and WHILE results are always the join of all arms and requirements come from all arms. Only the matched stable path (name.field...) is refined inside an arm; NovyWave needs this (RUN.bn:94-103). Exhaustiveness and reachability are checked only when the selector type is concrete, so an error never depends on which caller is being checked. SOURCE payload types come from the provider contract: the element event group or host port. This replaces the name-spelling heuristic at boon_typecheck lib.rs:39159-39213. Cycles are legal only through state cells (HOLD, and stateful LATEST if the owner agrees), SOURCE, async effects, publication across roles, or Dependency/catch_cycle; the check uses separate steady-state and initialization graphs. The spec lists 60+ diagnostic codes that the checker owns, most of which today are either accepted silently or fail later with internal errors. It also lists which example code breaks: the Theme dispatchers, 110 `[x: x]` field copies, 80 one-arm LATEST blocks, 18 THEN-over-state sites, the TodoMVC append that leaves `completed` Optional, and union field reads in persons_pro. Nine owner questions cover genuinely open semantics; the largest is whether `[x: x]` stays self-referential as documented.

## design
# Boon static semantics v2: typing spec for the strict checker

Status: proposed normative spec for the greenfield checker. Binding inputs are decisions.md 1-8. This spec fixes several things the docs left to the old kernel: joins, narrowing, cycle boundaries and SOURCE payload typing. Items marked **[Qn]** need the owner (see owner_questions); the text shows the recommended default. Syntax is unchanged.

---

## 0. Principles

- **P1. Types never depend on runtime values.** Compile-time constants are the only values that reach types: BITS widths, BYTES[N] lengths taken from literals, and static range checks. They are computed by constant folding, and constant folding never crosses a FUNCTION parameter.
- **P2. One type per definition.** FUNCTION is the only polymorphic entity: let-polymorphism, checked once, instantiated at each call site. Every value binding is monomorphic: root field, record field, BLOCK variable, HOLD binder, pattern binding, OUT binding, SOURCE payload. Recursion is rejected, so the call graph is a DAG and schemes are built bottom-up without a fixpoint among functions.
- **P3. Monotone constraints only.** Joins only grow and requirements only accumulate. The join is associative, commutative and idempotent. There is therefore one least solution, it does not depend on processing order, and nothing is ever retracted. This makes the old requirement withdrawal, order receipts and reuse-key mismatches unnecessary.
- **P4. The checker owns every user rule.** Anything later phases rely on is a positioned diagnostic here; downstream code only asserts it. The C8 census found about 90 user-rule sites scattered through semantic, IR and backend code. Their owner is now the checker (§15).
- **P5. Identity ignores order.** Record identity ignores field order, and tag-set identity ignores tag order. Display order is a presentation side table (§1.8, decision 7).

---

## 1. Types

### 1.1 Shape

Every expression has a type `τ = ⟨φ, δ⟩`: a flow `φ` and a data type `δ`. Flow is recorded at **every nested field**, because one record can mix SOURCE fields (E) and HOLD fields (C). The `store` record in every example does this. C2 found the old kernel resolving per-field modes by walking ancestor graphs; here flow is simply a component of the type.

`δ ::= ⊥ | U | α`, where `U` is a union with **at most one member per kind**:

| kind | member | display | join | subtyping |
|---|---|---|---|---|
| NUM | NUMBER (exact rational) | `NUMBER` | identity | = |
| TXT | TEXT | `TEXT` | identity | = |
| BYT | BYTES(n or dyn) | `BYTES[4]`, `BYTES` | BYTES[n]⊔BYTES[m] = BYTES when n≠m | BYTES[n] ≤ BYTES |
| BIT | BITS(n) | `BITS[8]` | different widths → T13 | = |
| TAG | {name ↦ Bare or Payload(R)} | `Active \| All`, `Found[value: T] \| NotFound` | union of names; payloads join | names ⊆, payloads ≤ |
| REC | R = {field ↦ (req or opt, τ)} | `[title: TEXT, done?: True \| False]` | §1.3 | §1.3 |
| LST | LIST(ε, ord) | `LIST<T>` | unify ε; ord ⊔ | ε equal |
| SET | SET(ε) | `SET<T>` | unify ε | ε equal |
| MAP | MAP(κ, ε) | `MAP<K, V>` | unify | equal |
| PRT | SOURCE(π) | `SOURCE` | unify π | equal |
| ELM | ELEMENT(root, kinds) | `[kind: Button \| Text]` | union of kinds; different roots → D3 | kinds ⊆ |

- **α** is a variable. There are four sorts:
  - parameter variables: rigid inside a body, generalized in the scheme;
  - accumulator variables: collection element types, SOURCE payloads, state cells and root values, all monomorphic;
  - field variables created by requirements such as `p.x`;
  - flow variables at every position.
- **⊥** is the empty union. It is the type of SKIP and of FLUSH on the normal path, and it is the identity for join.
- **poison** is internal. It is the type of an expression whose error has already been reported; it satisfies every predicate, which prevents cascades.

### 1.2 Flow

`φ ∈ {C continuous, E present-or-absent, A always absent}`. TickPresent survives only as an internal refinement of E.

Three monotone combinators:

- **combine** (pure call, operator, field read, TEXT interpolation): A if any input is A, else E if any is E, else C.
- **branch** (WHEN/WHILE arms, and the join of the lower bounds of one variable): A if every arm is A; E if the selector is E or any arm is E or A; else C.
- **latest** (LATEST arms): C if any arm is C (a *stateful LATEST*), else E if any arm is E, else A.

Flow requirements are exact, and each is checked against the joined lower bound. The check is monotone: once a value is E it stays E.

| requirement | positions |
|---|---|
| must be E | THEN input; HOLD update candidates; `List/append item:` |
| must be C | WHILE selector; HOLD init; render-slot values; LIST literal items; `List/retain if:`, `List/map new:`, `sort_by key:` |

`sample(τ)` is applied where a present value is stored: HOLD state, the value of a stateful LATEST, a `List/append` item. It turns every data field flow into C, while ports keep E.

### 1.3 Records and presence

- **Literal.** A record literal yields exact fields, all `req`.
- **Join** `R1 ⊔ R2`:
  - a field present in both is `req` only if it is `req` in both, and its type is the join of the two field types;
  - a field present in only one is `opt`.
- **Subtyping** `R1 ≤ R2` (arguments, contracts):
  - every `req` field of R2 must be `req` in R1, with `≤`;
  - every `opt` field of R2 is either absent from R1 or present with `≤`;
  - extra fields in R1 are allowed (width subtyping), except against a **closed contract**: render, event and style contracts (§9).
- **Field read.** `e.f` needs `f` to be `req` in every member that has fields (§2.3).
- **Using an opt field.** Boon has no presence test: there is no `?` syntax (grep of boon_parser finds none), no record pattern and no runtime type pattern. An `opt` field can therefore only be:
  - (a) passed to a contract that accepts it as `opt`;
  - (b) defaulted with a spread, `[f: default, ...r]`, which gives `f: req (default ⊔ r.f)`;
  - (c) not read at all.

### 1.4 Tag sets

- **Literals.** `A` has type `{A ↦ Bare}`. `A[f: e]` has type `{A ↦ Payload([f: τe])}`. `True` and `False` are ordinary tags; `True | False` is displayed that way and there is no BOOL.
- **Join.** Take the union of names. For a name present on both sides:
  - Bare ⊔ Bare = Bare;
  - Payload(R1) ⊔ Payload(R2) = Payload(R1 ⊔ R2);
  - Bare ⊔ Payload → T12. A name has one arity within a type, and `A[]` normalizes to `A`.
- **Upper bounds.** When a tag set comes from a parameter used in a WHEN without a catch-all, it becomes an upper bound: an OCaml-style closed polymorphic variant `[< A | B]`.

### 1.5 Collections are invariant accumulators

- **Element variable.** `LIST(ε, ord)`. ε belongs to the authority, meaning the construction site times its dynamic scope. Literal items and every write (`List/append`, insert, update) add lower bounds to ε, and every alias sees the same ε.
- **Why invariance.** It is required for soundness: writes through any alias are visible through all of them (foundations:985-994, 'submit an operation to that authority and return its live public view'). A covariant `LIST<A> ⊔ LIST<B> = LIST<A⊔B>` would let one alias be typed narrower than the items it actually contains.
- **Joins.** Joining two collection types unifies their accumulators. Example: `WHEN { A => list_a, B => list_b }` merges the two element types.
- **`ord` (order-chain provenance), covariant:**
  - set by `sort_by`;
  - preserved by filter/retain, one-to-one map and take;
  - cleared by everything else;
  - different `ord` values join to `none` (LANGUAGE_SEMANTICS.md:645-651).

### 1.6 Ports

- **Type.** `SOURCE` has type `⟨E, SOURCE(π)⟩`. π is a fresh monomorphic payload variable per SOURCE occurrence (§7).
- **Reads.** `p.f` reads payload field `f`, with flow E.
- **Allowed places.** Ports are structural. They may live in record fields, function parameters and results, and LIST rows produced by `List/map` templates.
- **Forbidden places** (S2): HOLD state, results of WHEN/WHILE/LATEST/THEN, MAP/SET keys, FLUSH payloads, equality, TEXT interpolation, and role boundaries.

### 1.7 Elements [Q6]

- **Type.** Render constructors (Element/*, Scene/Element/*) return `⟨C, ELEMENT(root, {kind})⟩`, an opaque kind.
- **Slots.** A render slot accepts ELEMENT of its root, plus the tags that slot documents: `NoElement`, `Hidden[text]`, `Reference[element]`.
- **Restrictions.** Programs cannot read fields of an element or build one from a record. No example does either: grep finds no hand-built `kind: Text|Button|...` record and no field read on a constructor result.
- **Effect on joins.** Element joins become trivial: one member instead of differently shaped element records. The mig_unions census counts 94 such LIST joins in NovyWave, 16 in TodoMVC and 18 in persons_pro.
- **Display.** Unchanged: `[kind: Button]` (BOON_TYPE_NOTATION_AND_INSPECTOR.md, registry table).

### 1.8 Identity and display (decision 7)

- **Identity.**
  - A record is a sorted slice of (field SymbolId, presence, TypeId). A tag set is a sorted slice of names with payload TypeIds.
  - Types are hash-consed, so equal shapes share one TypeId.
  - Today's order-sensitive identity goes away: term.rs:309-321, boon_checked lib.rs:310-316, requirements.rs:906-915.
  - This fixes the C3 rejections (probes app_diff and latest_mixed_list).
- **Display order:**
  - a literal shows fields in written order;
  - a join or spread shows first-appearance order, merging left to right in source order (arms top to bottom, spread operands left to right);
  - a parameter requirement row shows the order of first use in the body;
  - tags follow the same first-appearance rule, except that `True | False` is fixed.
- **Where order lives.** Order is kept in a presentation side table filled during checking, keyed by expression or declaration; never in the TypeId. One printer serves hints, the inspector and diagnostics.
- **Notation.** Optional fields display as `field?: T` [Q7]. Unions display as `A | B`.
- **Not displayed.** Flows are never shown, except that ports appear as `SOURCE`. VALUE, Unknown and TypeVar are never shown; an unresolved variable is a diagnostic.

---

## 2. The union model (decision)

### 2.1 Choice

Unions are **kind-partitioned**: at most one member per kind.

- The record member is a **single record** with req/opt presence.
- The tag member is **one tag set** whose tagged variants carry their own payload records.

### 2.2 Rationale

1. **Records cannot be told apart in Boon.** No construct can discriminate record shapes: patterns cover only `__`, a binder, NUMBER/TEXT/BITS literals, `Tag` and `Tag[fields]` (LANGUAGE_SEMANTICS.md:426-436; foundations:268-351). A union of distinct record shapes (TypeScript's `{a} | {b}`) therefore gives programs nothing they can use that a single record with `opt` fields doesn't. It would also cost non-canonical forms and cross-product spreads.
   - Theme.bn:7-10 spreads two 5-way unions, which would mean up to 25 combinations per spread.
2. **The join is a semilattice.** It is O(#kinds + #fields), canonical and hash-consable, which is what P3 requires. Today's join is not: it returns a union while any candidate is still a variable, then a field-union widen with an open-object top (solver.rs:2016-2034, term.rs:1580-1682).
3. **Analogies.**
   - **Roc, Elm and Rust** make sums out of tags and keep records as products. Boon's TAG member is Roc's tag union, including open and closed bounds; the REC member is Roc's structural record.
   - **TypeScript** contributes control-flow narrowing, restricted here to pattern arms over tag and literal kinds (§5.3), and its 'property exists in all members' rule for reading.
   - The opt normalization matches what TypeScript users write by hand as `{a: T; b?: U}`.
4. **Heterogeneous business cases stay explicit.** They use tags (`Found[value] | NotFound`), which is already the documented style (foundations:1135-1137).

### 2.3 Operations on unions

- **Field read `e.f`.** Every member must be one of:
  - REC with `f` req;
  - TAG whose variants are all tagged, with `f` req in every payload.

  Otherwise:
  - T4: f is missing;
  - T5: f is opt;
  - T6: a member kind has no fields, e.g. `TEXT | [a]`, or the tag set has a bare variant.

  The diagnostic names the arm or origin that contributed the offending member.
- **Spread `...e`.** The operand must be exactly one REC member, or exactly one tagged variant (its payload is spread). Otherwise T16.
- **Arithmetic and ordering.** Every member must be NUM (T2).
- **Equality `== !=`.** The operands must overlap: share a kind, and for TAG share a name (T3). Ports and elements are not comparable (S2).
- **DISPLAYABLE.** Used by TEXT interpolation, `Text/concat`, and the label/child display shorthand. Every member must be NUM, TXT, or a TAG with only bare variants (T20).
  - BYTES and BITS never become TEXT implicitly (BYTES_SEMANTICS.md:78-81; LANGUAGE_SEMANTICS.md:450-452). Today's runtime converts BYTES by UTF-8 (machine.rs:35435); v2 rejects it.
- **Argument or contract.** Checked member by member (§1.3, §1.4).
- **LATEST and HOLD.** The join must stay within **one kind** (T17). Decision 6 names 'LATEST over NUMBER vs TEXT' as an error, and TYPE_INFERENCE:482-484 lists LATEST and HOLD incompatibility as negative fixtures. WHEN/WHILE results, LIST literals and FUNCTION results may form multi-kind unions (decision 5).
- **MAP keys and SET elements.** Single kind (§8.3).

---

## 3. Expressions

Notation: `Γ; ψ; Ω ⊢ e : τ | Φ`.

| symbol | meaning |
|---|---|
| Γ | lexical scope |
| ψ | type of the current PASSED context; none at root level |
| Ω | OUT and contextual binders in scope |
| Φ | FLUSH payload set escaping e |

### 3.1 Literals

- **NUMBER.** `⟨C, NUMBER⟩`. The literal is parsed once into a normalized rational, which is also its constant value.
- **TEXT.** `TEXT { ... }` and `"..."` give `⟨combine(parts), TEXT⟩`. Every `{e}` part must be DISPLAYABLE.
  - Probe z2 (a record interpolated into TEXT) compiles today and fails only at runtime (machine.rs:35441-35450).
- **BYTES.**
  - `BYTES {}` is dynamic.
  - `BYTES[N] {}` is N zero bytes; a non-empty `BYTES[N]` must contain exactly N bytes.
  - `BYTES[__]` sums fixed item lengths; any dynamic item is T21.
  - Items are byte literals (length 1) or BYTES values, which flatten. A TEXT item is T20, with a `Text/to_bytes` hint (BYTES_SEMANTICS.md:78-83).
- **BITS.** In `BITS[N] { lit }`, N must be a positive constant ≤ 1,048,576 and the literal must fit exactly; the type is BITS(N) (T21).
- **SKIP.** `⟨A, ⊥⟩`.
- **Tags and tagged objects.** See §1.4.

### 3.2 Record literal

Fields fold left to right; at runtime the later value wins (machine.rs:27115-27140).

- **Explicit `f: e`.** Sets `f := (req, τe)`. A second explicit field with the same name is N4 (already a diagnostic, kernel_oracle.rs:174).
- **Spread `...e`.** For each field `g` of the operand:
  - `req g` overrides;
  - `opt g` becomes `req (earlier ⊔ g)` if an earlier field `g` was req, and `opt (earlier ⊔ g)` otherwise.
- **Scope.** Explicit fields are sibling lexical declarations; spreads bind no names (LANGUAGE_SEMANTICS.md:216-219).
- **Spreading a parameter.** A parameter has an open row. Spreading it is allowed and produces a symbolic spread in the scheme, normalized at instantiation. A field read through it that no explicit field decides becomes a residual predicate, checked at instantiation.
  - Examples spread 209 call results and 8 closed local records and never a parameter; the documented `Dictionary/map_values` does spread one.

### 3.3 Field access

`e.f`, and its pipe form `e |> .f` (fibonacci), follows §2.3. The result flow is `combine(φe, φf)`.

### 3.4 Operators

| operator | operands | result |
|---|---|---|
| `+ - * / %`, unary `-` | NUMBER | NUMBER |
| `< > <= >=` | NUMBER | `True \| False` |
| `== !=` | must overlap | `True \| False` |

- Static `/ 0`, `% 0` and a non-whole `%` operand are T21.
- Today the kernel unifies arithmetic operands with NUMBER but reports nothing (owner.rs:22222-22227), and equality has no operand constraint at all (owner.rs:21876-21878). Probes q3 (`TEXT - 1`) and z3 (`1 == TEXT`) compile today.

### 3.5 Names

- **Lexical names.** Visible across their whole scope; a local shadows the outer name throughout that scope. Registered roots cannot be shadowed (LANGUAGE_SEMANTICS.md:216-222, 242-245).
- **Unique-suffix fallback [Q4].** A bare name with no lexical binding resolves to the unique root record field path ending in that name (boon_typecheck lib.rs:14385-14393, C9). More than one match is N2. TodoMVC (`visible_todos`, `selected_filter`) and NovyWave RUN.bn:4506 depend on this.
- **PASSED.** `PASSED.path` projects ψ (§4.4). PASSED outside any FUNCTION is N6.
- **`element`.** Inside a render-constructor argument it is that constructor's ElementState binder (§9).
- **OUT names.** A fresh or forwarded OUT name has the OUT's value type (§4.5).
- **Field self-reference [Q1].** Under the documented rule, the bare name `f` inside the initializer of explicit field `f` is the field itself. A self-reference that is not broken by a state cell is Y4 (§13.3).

---

## 4. FUNCTION, calls, PASS, OUT

### 4.1 Scheme

```
scheme(f) = ∀ᾱ. ⟨ordinary params, ψ, OUT formals⟩ ⇒ result  |  P, S, Φ, N, eff
```

- **P: predicates on parameter variables.** Created by uses in the body:
  - `HasField(α, f, β)`;
  - `IsNumber(α)`;
  - `TagsWithin(α, {name: payload predicates})`;
  - `Displayable(α)`;
  - `KeySafe(α)`;
  - `Flow(α@path, E|C)`;
  - `Contract(α, K)`;
  - `Overlap(α, β)`;
  - residual predicates on symbolic spreads and tag differences.
- **ψ: the implicit PASSED row variable.** Gets HasField predicates from every PASSED read, and from every callee called without PASS.
- **S: scope effects.** Which ordinary parameters are evaluated under which OUT formal. This is the 'argument under OUT' rank-1 rule (OUT plan:538-546, 801-809).
- **result.** A type term over ᾱ: joins, projections, spreads, differences and collection accumulators.
- **Φ.** Payloads that surface at the function boundary (§6.5).
- **N.** OutNet producer summary (§4.5).
- **eff.** Bits saying whether the function creates state, sources or effects. Used by distributed and backend phases.

### 4.2 Checking a FUNCTION

1. **Order.** Bodies are checked once, in reverse topological order of the call graph, callees first (Y1 guarantees a DAG).
2. **Typing the body.** Boon is first-order: there are no lambdas, and a function used as a value is an error (kernel_oracle.rs:183-187). So the body is typed **forward**: each expression's type is a function of its subexpressions, the parameter variables, root variables (C9 remedy) and callee schemes. Only state cells in the body need a small fixpoint (§6.4); its height is finite and there is no retraction.
3. **Generalization.** Generalize over every variable not reachable from root variables. Root values are monomorphic outer variables, so a store/function cycle needs no extra pass (C9: function-only SCCs are all singletons in both fixtures).
4. **Simplification.** Merge the predicates on each variable; drop variables that occur only positively or only negatively; hash-cons the result.
5. **Early errors.** An unsatisfiable predicate set is reported at the definition, e.g. `p.x` used both as NUMBER and as TEXT.

### 4.3 Call `f(entries)` and `a |> f(entries)`

1. **Call discipline** (reported by the resolver if it has not already been):
   - every call has parentheses;
   - entries use declared names, in declaration order, one per parameter;
   - only builtin parameters may be omitted, and only when the catalog registers a default;
   - a bare entry is allowed only for an OUT formal, and its name must equal the formal (C2, C4);
   - `formal: name` on an OUT formal must name an OUT formal of the enclosing FUNCTION (C9; O7 if it names an already-driven fresh output);
   - `item: OUT` at a call site is C3;
   - an ordinary value passed to an OUT formal is C8;
   - a pipe fills the first ordinary parameter (C5 if there is none);
   - PASS appears at most once, last, and only on user FUNCTION calls (C6; kernel_oracle.rs:222-230).
2. **Parent-scope arguments.** Instantiate the scheme with fresh variables. For each argument evaluated in the parent scope, check `actual ≤ param` by evaluating the predicates on the actual's lower bound. Predicates that mention the caller's own parameters become residuals in the caller's scheme.
3. **OUT arguments.** OUT value types are computed from parent-scope arguments only. An OUT type that depends on a contextual argument is Y6. Contextual arguments are then checked with the fresh OUT bindings in Ω.
4. **Context.**
   - Without PASS, the callee's ψ is the caller's ψ.
   - At root level the context is the closed empty record, so any callee requirement is P1 ('root call to f requires PASS').
   - With `PASS: e`, the callee's ψ is `type(e)`, where `e` is evaluated in the caller's context and must be a record (C7).
5. **Result.** The instantiated result term, normalized. Memoized on (scheme id, actual TypeIds, ψ TypeId).

Where errors go:
- An error inside a body is reported **once, at the definition**.
- A call-site error is reported **once per call site**, at the offending argument, with a secondary span at the callee use that created the predicate ('need_x reads r.x here').
- There is no per-path reporting.

### 4.4 PASS / PASSED

- **Scope.** PASSED is lexically the enclosing FUNCTION's context. Argument expressions, including contextual ones evaluated under a callee OUT, see the caller's PASSED (LANGUAGE_SEMANTICS.md:191-196; OUT plan:327-337).
- **Replacement.** PASS replaces the callee's context completely. Extending it is explicit: `PASS: [...PASSED, theme: t]`.
- **Errors.**
  - A missing field is P2, reported at the PASS clause or root call, naming the chain of functions that read it.
  - A field of the wrong type is an ordinary T-family error that carries the PASS provenance.
  - Today a missing PASSED field compiles in diagnostics mode and dies later at storage_contract.rs:5390; the fact is already known at requirements.rs:461 (C8, probe s3).

### 4.5 OUT

- **Scope effects.** From the body, each ordinary parameter is classified as either parent-scope or evaluated under OUT `o`. It is under `o` when it is passed to a callee argument that the callee's S evaluates under an OUT forwarded from `o`, or to a builtin contextual argument (`new:`, `if:`, `key:`). A parameter needed both inside and outside a scope, or under two different OUTs, is O4 (OUT plan:548-549).
- **OutNet.** Each OUT formal of a user FUNCTION must be forwarded along exactly one chain ending at exactly one builtin producer (OUT plan:603-628). Violations:
  - O1: zero producers (the wrapper typo that writes a fresh `item` instead of `item: item`, OUT plan:305-309);
  - O2: more than one producer;
  - O3: forwarding inside a WHEN/WHILE/LATEST arm (branch-local producers are not in v1);
  - Y6: an alias cycle;
  - O5: a correlated group forwarded only partly.

  Today these fail either with kernel crashes (solver.rs:245; owner.rs:22446, which also rejects the valid record-wrapped producer o8) or only in elaboration (out_net.rs:2608-2613).
- **Values versus identity.** An OUT value may be read and copied into data. The OUT identity may not be compared, stored in HOLD or a LIST, persisted, or sent across a role (O6).
- **Unused parameters.** A fresh output may go unread. An unused **ordinary** parameter is C10 (OUT plan:639-641).

### 4.6 Recursion and nesting

- Any cycle in the call graph is Y1, with the full cycle path. This includes self-calls and cycles through contextual calls (OUT plan:1332-1334).
- Today recursion is accepted in diagnostics mode and rejected only at machine_plan_backend.rs:14051 (C2, C9).
- A FUNCTION nested inside BLOCK is rejected in v1 [Q9]: today it crashes the kernel, and no example uses it.

---

## 5. WHEN and WHILE

### 5.1 Patterns [Q5]

The allowed patterns are `__`, a lowercase binder, a NUMBER/TEXT/BITS[N] literal, a bare `T`, and `T[f1..fk]`.

- A bare `T` matches the variant named T **whether it is bare or tagged**.
- `T[f..]` matches only a tagged T, and binds each `fi` to the payload field `fi`, which must be req (T10).
- NovyWave depends on bare patterns matching tagged values: `HierarchyPage =>` over `HierarchyPage[rows, ...]` (RUN.bn:94-103).

### 5.2 Result

`⟨branch(φs, φ1..φn), ⊔ δi over all arms⟩`.

- Arms are never pruned by the value of the argument (decision 4).
- SKIP arms add ⊥ and make the flow E.
- FLUSH arms add ⊥ to the result and their payload to Φ.

### 5.3 Selector narrowing (pattern refinement)

This is not the dropped static narrowing.

A **stable path** is a lexical name, `PASSED`, a root-qualified path or an OUT binding, followed by zero or more `.field`. If the selector is a stable path, then inside arm i every occurrence of the selector path, or of any extension of it, gets a refined type:

| arm pattern | refined type of the path |
|---|---|
| `T` or `T[..]` | the TAG member restricted to variant T |
| NUMBER/TEXT/BITS literal | the member of that kind; there are no singleton literal types |
| `__` or binder | residual(i): δs minus the tag names matched by earlier tag arms; literal arms remove nothing; a binder gets residual(i) |

- **Why this is sound.** Values are immutable within an evaluation, and an arm is re-evaluated whenever its selector changes.
- **Limit.** Narrowing never crosses a call.
- **Why it is needed.** NovyWave uses it at RUN.bn:94-97 (`real_hierarchy_page_result.rows`) and RUN.bn:69-72 (`real_file_stream_result.retained.content` inside nested arms).
- **Per-variant requirements.** Requirements on the selector's own payload stay per variant, so probe x05 (`r |> WHEN { A[a] => r.a  B[b] => r.b }`) remains accepted. Requirements on other parameters come from all arms, so p1 remains rejected.

### 5.4 Exhaustiveness and reachability

**Concrete selector** (its type mentions no parameter variable):
- **T8.** The residual after the last arm must be ⊥ unless there is a catch-all. The message lists the uncovered tags and kinds; NUM, TXT, BYT, BIT, REC, LST and ELM members can only be covered by a catch-all.
- **T9.** An explicit pattern that can never match δs (absent tag, literal of an absent kind, `T[f]` where f is absent), a duplicate pattern, or any arm after a catch-all.
- **Warning only.** A catch-all whose residual is already ⊥. flush_error_propagation.bn:18-22 and TodoMVC RUN.bn:63 use defensive `__`.
- **r4.** Probe r4 (a non-exhaustive WHEN) fails today with an internal error at contextual_expansion.rs:277; it becomes T8.

**Polymorphic selector:**
- Without a catch-all, the arms become the scheme predicate `TagsWithin(δs, listed names with payload predicates)`.
- Literal arms without a catch-all are T8 at the definition, because literals cannot exhaust NUMBER or TEXT.
- With a catch-all, only the payload predicates of listed tagged patterns remain.
- Reachability is **not** re-checked per instantiation, so errors never depend on the caller.
- A `__` residual over a parameter is the symbolic difference `α∖S`. The predicate `α∖S ≤ U` is rewritten as `α ≤ U ∪ S` (tags only).
  - Example: TodoMVC Theme.bn:2-11 passes `of` from its `__` arm into a theme's closed `material` WHEN.

### 5.5 WHILE

- Same typing as WHEN.
- The selector must be C (F2); probe z6 (`event |> WHILE`) compiles today.
- Arms are activation scopes for state and effects. This has no typing effect.

---

## 6. Temporal constructs

### 6.1 THEN

`s |> THEN { b }`
- `φs` must be E; an A input gives an A result. A C input is F1 [Q3].
- The result is `⟨E, δb⟩`, or A if b is A. Inside b, s is present.
- The kernel today silently drops C through `Eventful` (owner.rs:15583-15585), so probe r3 (`5 |> THEN {6}`) compiles.

### 6.2 SKIP

`⟨A, ⊥⟩`.
- Allowed as: a WHEN/WHILE arm, a LATEST arm, a THEN body, a HOLD update candidate.
- F5 anywhere a C value is required: HOLD init, render slot, LIST literal item, or a binding that is always absent.

### 6.3 LATEST

- At least 2 arms; one-input LATEST is F6 (OUT plan:1051-1052). Probe z1 compiles today.
- The data type is the join of the arms and must stay within one kind (T17). Probe r5 compiles today.
- Flow is `latest(...)`.
- A LATEST with a C arm is a **state cell** [Q2]. Its value is `sample(join)`, and it follows the state-cell cycle rules (§13). Evidence:
  - counter_latest.bn:5-6 says 'An initial arm makes this field stateful';
  - reactive.rs:2463 treats the 'initial LATEST' as a state authority.

### 6.4 HOLD

`i |> HOLD s { b }`
- **Init.** `i` must be C (F3), must not flush (F9), and must not be SKIP. Probe z7 (an event as init) compiles today.
- **State type.** `σ = sample(δi ⊔ δ(candidates))`, and:
  - one kind (T17);
  - no LIST, SET or MAP anywhere, including inside union members and opt fields (S1, foundations:1309-1343);
  - no ports or elements (S2);
  - not infinitely nested (T15).
- **Binder.** Inside b, `s : ⟨C, σ⟩`.
- **Update candidates.** Must be E or A (F4).
- **HOLD expression.** `⟨C, σ⟩`.
- **Today.** The kernel switches between the initializer-only type and a StructuralWiden(init+update) depending on the call path (owner.rs:21361-21390). v2 has one rule.
- **Fixpoint.** The state type is a small monotone fixpoint: a candidate may mention s, and joins only grow.

### 6.5 FLUSH [Q8]

`FLUSH { e }`
- `e` must be a closed tag set (bare and/or tagged) with no collections, ports or flow values (F8).
- On the normal path the type is ⊥; `e`'s type is added to Φ.
- **Boundaries:**
  - the initializer of any record field (root or inside a record literal);
  - FUNCTION return;
  - BLOCK final result;
  - root result.

  At a boundary the type becomes `normal ⊔ Φ` and Φ is cleared.
- **Not boundaries.** BLOCK variables, collection callbacks (`new:`, `if:`) and HOLD bodies: the payload propagates to the consumer or owner (foundations:1583-1625). cells/formula.bn:19-25 relies on BLOCK variables not being boundaries.

### 6.6 BLOCK

- Local bindings are order-independent and monomorphic.
- The value is the final expression, which is a FLUSH boundary.

### 6.7 Flow table (normative)

| construct | inputs | output |
|---|---|---|
| literal, constant | – | C |
| SOURCE | – | E (port) |
| field read, operator, pure builtin, interpolation | any | combine |
| user FUNCTION call | per the scheme's flow variables | per scheme |
| THEN | E (A) | E (A) |
| WHEN | selector C or E | branch |
| WHILE | selector C | branch |
| LATEST | ≥ 2 arms | latest; C means a state cell |
| HOLD | C init, E/A candidates | C |
| Bool/toggle | C input, E `when` | C (state) |
| List/append | C list, E item | C |
| List/map, retain, find, sort_by | C list; `new`/`if`/`key` C per row | C |
| host effect call | per signature | E (completion outcome) |
| SKIP | – | A |

---

## 7. SOURCE payload typing

Today payload fields are typed from path spelling: `text` is TEXT, `bytes` is BYTES, and `press|click|double_click|blur|change|key_down` are `[]` (boon_typecheck lib.rs:39159-39213, `source_payload_access_for_suffix` and `source_payload_field_type`). Source paths are matched by suffix (lib.rs:39228-39243). The principled rule:

1. **Ports.** Each SOURCE occurrence creates `SOURCE(π)` with a fresh monomorphic π. Inside a FUNCTION, π generalizes with the result. This is sound because ports are generative: each call-site instantiation (each List/map row) owns new ports.
2. **Providers fix π, by equality with a contract payload:**
   - **Element binding.** A record passed as `element: [events: G]` to a constructor of kind K must satisfy the closed contract `EventGroup(root, K)`, e.g. `[press?: SOURCE([]), change?: SOURCE([text: TEXT]), key_down?: SOURCE([key: TEXT]), blur?: SOURCE([]), ...]`.
     - Each port in G is unified with its contract payload.
     - An unknown event name is R2; a value that is not a port is R3 (PASS_PASSED doc 'Unknown events and non-SOURCE event fields are errors').
   - **Host binding.** The `host_ports:` root record (server_http_echo.bn:31-36) and host builtins (Timer/interval, File/*, HTTP) take their payloads from the catalog.
   - **Several providers** must agree (R1).
3. **Consumers.** Payload field reads are predicates on π, checked against the provider's payload.
4. **No provider.** The payload is `[]`, a pure pulse. Reading any payload field is R4.
5. **Placement.** SOURCE may occur only in structural positions: root records, and FUNCTION bodies used as templates, including List/map `new:`. It may not occur in THEN bodies, HOLD bodies or WHEN/WHILE/LATEST arms (S4). Per-row ports come from List/map templates (TodoMVC RUN.bn:98-112).

---

## 8. Collections and the builtin catalog

### 8.1 Literals

- **LIST.** `LIST { items }` is `LIST(ε, none)` with ε ≥ each item.
  - Items must be C (F5).
  - A LIST literal mixing element kinds is allowed; `NoElement` alongside elements is normal.
  - In `LIST[N]`, N must be a constant.
- **SET.** `SET { items }` is `SET(ε)`; ε must be key-safe and of one kind.
- **MAP.** `MAP { k => v }` is `MAP(κ, ε)`.
  - Keys must be key-safe and of one kind.
  - Statically equal duplicate keys are T18 (foundations:1188-1189).

### 8.2 Representative signatures

Flow defaults to combine.

- `List/append(list: LIST<T>, item: E T') -> LIST<T>`, with `T ≥ sample(T')`. The accumulator effect is visible through every alias of the authority.
- `List/map(list: LIST<T>, item: OUT T, new: (under item) C U) -> LIST<U>`. This creates a derived authority. `new` may be SKIP only where the render slot accepts NoElement (TYPE_INFERENCE:188).
- `List/retain | List/filter(list, item: OUT T, if: (under item) C True|False) -> LIST<T>`. `ord` is preserved.
- `List/find(list, item: OUT T, if: ...) -> Found[value: T] | NotFound`.
- Sizes and ranges:
  - `List/count`, `List/length` return NUMBER; `List/is_empty` returns `True|False`;
  - `List/get(position: whole >= 1) -> Found[value: T] | NotFound`;
  - `List/range(from, to: whole) -> LIST<NUMBER>`.
- `List/sort_by(list, item: OUT T, key: (under item) K, direction: Ascending|Descending = Ascending) -> LIST<T, sorted>`. K is NUMBER, TEXT or a closed bare tag set, of one kind (T19).
- `List/then_by(...)` requires `ord = sorted` (T19). Today this is enforced only on the verified path (kernel_oracle.rs:3048, probe s5).
- `List/take(count: whole)` preserves `ord`.
- `List/page(size, after: Start | Cursor[value: BYTES]) -> Page[items: LIST<T>, next: End | Cursor[value: BYTES]] | PageExpired | InvalidPageCursor | InvalidPageSize | PageWorkLimitExceeded`. A literal `size` must be in `1..=10000`.
- `List/chunk(size) -> LIST<[items: LIST<T>, label: ...]>`.
- MAP and SET:
  - `Map/upsert(entry: [key: K, value: V])` accumulates K and V;
  - `Map/get(key) -> Found[value: V] | NotFound`;
  - `Map/remove`, `Set/add`;
  - `Set/contains -> True|False`.
- Bool:
  - `Bool/not`, `Bool/and(right:)`, `Bool/or` take and return `True|False`;
  - `Bool/toggle(input: C True|False, when: E any) -> C True|False` is a state cell.
- Text:
  - `Text/*` inputs are TEXT;
  - `Text/concat(input: DISPLAYABLE, with: DISPLAYABLE, separator: TEXT)`, which covers NovyWave RUN.bn:136 and cells/formula.bn:25 passing a NUMBER `with`; today the parameters are `Unknown` (lib.rs:30683-30692);
  - `Text/to_number -> Parsed[value: NUMBER] | InvalidNumber[reason: TEXT, position: NUMBER]`.
- `Number/*`: option ranges are checked statically when constant. `Number/to_bits(width: constant) -> Converted[value: BITS[w]] | NotWhole | OutOfRange`.
- `Bits/*`: width arithmetic per LANGUAGE_SEMANTICS.md:479-483; widths must be constant (T14).
- `Bytes/*`: rules per BYTES_SEMANTICS.md.
- `Stream/pulses(count: whole) -> E`; `Stream/skip(count)`.
- `Dependency/catch_cycle(value: T, on_cycle: T') -> T ⊔ T'`. Its `value:` argument is a runtime-cycle boundary (§13).
- `SessionInfo/status`, `SessionInfo/principal`: availability depends on the role (§11).
- Host effects return typed outcome unions with flow E.

### 8.3 Key safety

Applies to MAP keys and SET elements.
- **Allowed:** NUMBER, TEXT, BYTES, BITS, bare tag sets, and tagged payloads or records built only from key-safe fields.
- **Forbidden:** collections, ports, elements, and any flow other than C.
- **Single kind** (T18). This follows foundations:1153-1177 ('Heterogeneous business cases use explicit Tags').

### 8.4 Authorities

- Collection inside HOLD is S1.
- A nested authority must have a single parent and must not escape its owner (S3). This is checked by an origin analysis over collection construction sites after typing; today there is an authority graph at lib.rs:39102.

### 8.5 Catalog as data

- One static table, compiled in. It is not rebuilt or hashed per compile, as the builtin ABI is today.
- Each entry gives:
  - name;
  - ordered parameters, each with name, ordinary or OUT, type scheme, flow requirement, default, static-argument check and contextual scope;
  - result scheme;
  - flow class: pure, state, event-consuming or effect;
  - render root, if it is a constructor;
  - role availability.
- User FUNCTIONs and builtins share one scheme representation.
- This replaces BuiltinSignatureRegistry (lib.rs:30651, where many parameters are `Unknown`) and the kernel's builtin ABI.

---

## 9. Render roots and contracts

- **Root.** A client program has exactly one retained root: `document: Document/new(root:)` or `scene: Scene/new(root:, lights:, geometry:)` (D1). Today this is enforced only in the backend (machine_plan_backend.rs:6403-6407, probe b1).
- **Constructors.** Catalog builtins with **closed** contracts per (root, kind) [Q7]:
  - `element: ElementConfig` = `[events?: EventGroup, tag?: tags, ...]`;
  - `style: Style(root, kind)`: every field opt and typed; colors are `Oklch[lightness?, chroma?, hue?, alpha?: NUMBER] | TEXT`;
  - `child`, `root`, `label`: one ELEMENT, or the DISPLAYABLE shorthand where documented (counter.bn:107 `label: store.count`);
  - `items`, `children`: `LIST<ELEMENT | NoElement>`;
  - `visible`, `checked`, `focus`, `selected`: C `True|False`.
- **Unknown fields.** An unknown field in a closed contract is D4.
- **`element` binder.** Each constructor supplies `element : ElementState(kind) = [hovered: C True|False, ...]`.
  - It is visible in every argument except the `element:` entry, which is evaluated in the parent scope.
  - Examples read `element.hovered` 110 times.
  - Using it outside a constructor argument is D5.
- **Nested lists.** `items: LIST { header(), todos |> List/map(...) }` stays invalid (TYPE_INFERENCE:347-351) as D2.
- **No result narrowing.** `Element/stripe` returns kinds {Row, Stack} whatever `direction` is. The narrowing at BOON_TYPE_NOTATION:217 is dropped under decision 4.

---

## 10. DRAIN / DRAINING

Source: BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1106-1290. Today every one of these rules lives only in memory_contract.rs (C8).

- **`e |> DRAINING`.**
  - Allowed only as the outermost stage of a named binding's initializer: a root field path, or a row field inside a List/map template.
  - The value must be authoritative state: HOLD, stateful LATEST, a LIST authority, or a record of those (M1).
  - The type is e's type.
  - Any ordinary read of the binding is M2. Today probe d2 (reading a DRAINING sibling inside a HOLD in the same record) is accepted by every stage.
- **`DRAIN { path }`.**
  - Takes exactly one static storage path (binding, field path or static PASSED path) that resolves to a draining binding or a leaf or subtree of one (M4).
  - The type is the source's state type.
  - Allowed where a destination's state is initialized: a HOLD init, a LIST source, or a field of a record that initializes a HOLD.
  - Everything between DRAIN and the destination must be pure: no SOURCE, no state read, no effect, no time (M5).
- **Coverage.**
  - M3/M4: each authoritative leaf of a draining region is consumed exactly once; no ancestor/descendant overlap; no self drain; no double drain; no conflicting destinations.
  - M6: row-field drains only within the same list owner.
  - The migration-cycle validator (memory_contract.rs:2619-2649) is unreachable from source and becomes a debug assert.
- **Rules that stay outside the checker.** Rules that need the predecessor catalog, such as schema evolution without DRAIN (machine_plan_backend.rs:3794-3943), stay in activation. They must still come out as positioned diagnostics.

---

## 11. Distributed roles

- **Checking scope.** Client, Session and Server are separate programs checked together as one bundle.
- **Edges.** `Role/store.path` and `Role/fn(...)` are allowed only on adjacent edges, Client↔Session and Session↔Server (X1). Same-role qualification is X2 (LANGUAGE_SEMANTICS.md:247-258, 283-289).
- **Cross-role values.** Carry the exporting role's monomorphic type and flow. Cross-role edges are publication boundaries (§13).
- **Cross-role calls.** Must target pure FUNCTIONs: `eff` shows no state, source or effect, and there is no OUT or PASS (X5).
- **Never cross a boundary.** PASS, OUT identities, ports and FLUSH status (X4). Today these are enforced in boon_semantic lib.rs:1558, 1581, 5779-5786.
- **SessionInfo** (X3):
  - `status()` in Client and Session, and in Server only inside a Session-scoped branch;
  - `principal()` in Session and a Session-scoped Server, never in Client.

---

## 12. Static (constant) checks

- **Where constants come from.** Constant folding runs per definition, over literals and root-level constant bindings. It never goes through a FUNCTION parameter (P1).
- **T21 checks** apply when a builtin argument is constant:
  - `Number/round` `to:` must be > 0;
  - `Number/to_text` options must be in range;
  - a literal `List/page` size must be in `1..=10000`;
  - `List/range` bounds must be whole;
  - List and Bytes positions must be whole and ≥ 1;
  - `byte_count` must be in {1, 2, 4, 8};
  - no out-of-bounds access on fixed-size BYTES;
  - BITS widths, positions and literal fit;
  - no static division or remainder by zero.
- **BYTES[N] refinement.** A static TEXT input refines `Text/to_bytes`, `Bytes/from_hex` and `Bytes/from_base64` to BYTES[N], and malformed static data is rejected (BYTES_SEMANTICS.md:88-104).
- **Widths must be constant.** A BITS/BYTES width that is not constant is T14. Only language_surface/current/bits_fixed_width_literals.bn uses BITS at all.
- **Everything else** is checked at runtime.

---

## 13. Dependency graph and cycles

### 13.1 Graphs

- **(a) Call graph.** Must be a DAG (Y1).
- **(b) Value graph.** Nodes are root field paths (C9 granularity: the largest SCC has 5 nodes in TodoMVC and 22 in NovyWave), local nodes (record fields, BLOCK variables) and **anonymous state cells** (any HOLD, stateful LATEST or Bool/toggle expression).

### 13.2 State-cell rule [Q2]

A state cell's value during a tick is its committed value.

- **Steady-state graph.** Remove these edges:
  - edges from a state cell to the dependencies of its update inputs (a HOLD body; every LATEST arm);
  - edges into SOURCE;
  - async host-effect completion edges;
  - cross-role publication edges;
  - edges through the `value:` argument of `Dependency/catch_cycle`. Runtime-checked row cycles need this: cells/formula.bn:38-49 (`cells → result → cells`).
- **Initialization graph.** Edges from a HOLD to its init dependencies, and from a stateful LATEST to the dependencies of its C arms, since those arms define the initial value.
- **Both graphs must be acyclic.**
  - Y3 is a steady-state cycle and Y5 an initialization cycle.
  - The diagnostic shows the declarations and edges of one cycle (OUT plan:511-512).
  - Today probe c1 (`a: b + 1`, `b: a + 1`) passes **every** stage (C8).

### 13.3 Self-reference

- **Documented rule [Q1].** `[x: x]` is Y4, with a fix hint to introduce an outer alias (OUT plan:465-487).
- **Init cycle.** `x: LATEST { x, e |> THEN {...} }`, where a C arm reads x itself, is Y5. Probe z4 fails with an internal 'provenance cycle' error today.
- **Legal self-reads.** These do not form an init cycle:
  - `count: LATEST { 0, inc |> THEN { count + 1 } }` (counter_latest.bn:7-12, interval_latest.bn, NovyWave value_format RUN.bn:3019-3022);
  - the HOLD self-read `store.details_visible` inside its own HOLD (flow_operators.bn:17-20).
- **TodoMVC `new_todo`.** Its title_to_update ↔ edited_title ↔ draft_title cycle (RUN.bn:120-160) is legal because edited_title and draft_title are stateful LATESTs.

### 13.4 Other cycles

- Y6: OUT alias cycles.
- T15: type recursion, e.g. an update that nests the state inside itself; found by an occurs check on accumulator lower bounds.
- Y7: distributed combinational cycles, handled by the same graph with role nodes.

---

## 14. Checking algorithm (normative outline, built for speed)

1. **Resolve** (per project, after the per-unit parse). Order-independent declaration collection and name resolution: lexical names, module/function tables, the suffix rule [Q4], the `element` binder, OUT bindings, PASSED. This also builds the call graph and the value graph with labelled edges.
2. **Structure checks.** Call-graph SCCs (Y1), then the steady-state and initialization value graphs (Y3-Y5, §13), and the call discipline (C1-C9). Each is O(V+E).
3. **Constant folding.** Bottom-up, per definition.
4. **FUNCTION schemes.** In reverse topological order: forward typing of the body, scheme construction, simplification and hash-consing (§4.2). All FUNCTIONs with no path between them in the DAG can be checked in parallel.
5. **Root values.** Every root field path gets a monomorphic variable up front. Root SCCs are processed in topological order; inside an SCC, lower bounds are iterated to the monotone fixpoint, which has finite height.
6. **Predicate discharge.** Every predicate on a root variable, and every call-site residual, is evaluated on the final lower bounds.
7. **Whole-program checks on final types:**
   - render contracts and roots;
   - SOURCE providers (R1-R4);
   - OutNet (O1-O7);
   - authority origins (S3);
   - DRAIN coverage (M1-M6);
   - roles (X1-X5).
8. **Diagnostics.** Sorted by (unit, span). Each carries a stable code and Boon-vocabulary text, rendered lazily from the constraint's span and provenance tag. There is one solve and no second 'rich' solve.

**Complexity and determinism.**
- Cost is O(nodes + Σ call sites × simplified scheme size), with union-find/hash-cons operations at near-constant cost.
- Nothing is retracted.
- The result does not depend on processing order, because joins are ACI and predicates only accumulate.

**Incrementality.**
- A scheme's reuse key is (body hash, callee scheme ids, ids of the root variables it reads). This is sound because a scheme is a pure function of those inputs.
- Early cutoff applies when a re-inferred scheme gets the same hash-consed id.
- A root SCC is re-solved only when an input scheme or a member body changes.

---

## 15. Diagnostic catalog (owned by the checker)

The 'today' column shows current behaviour. Legend: **acc** = accepted with 0 diagnostics; **int** = internal/late error; **dx** = already a diagnostic; **elab** = found only in semantic/backend.

| code | rule | today |
|---|---|---|
| N1 | unknown identifier or function | dx |
| N2 | ambiguous identifier (suffix rule) | dx |
| N3 | function used as a value | dx |
| N4 | duplicate explicit record field, BLOCK binding or FUNCTION | dx (field) |
| N5 | shadowing a registered root | partial |
| N6 | PASSED outside a FUNCTION | dx |
| N7 | nested FUNCTION in BLOCK [Q9] | int |
| C1 | missing, unexpected or misordered call entry | dx |
| C2 | bare entry for an ordinary input | dx |
| C3 | `item: OUT` at a call site | parser |
| C4 | bare name does not match the OUT formal | partial |
| C5 | pipe into a function with no ordinary input | dx |
| C6 | PASS not final, duplicated, or on a builtin | dx |
| C7 | PASS value is not a record | int (s04 crash) |
| C8 | ordinary value passed to an OUT formal | legacy only |
| C9 | no enclosing output to forward | legacy only |
| C10 | unused ordinary parameter | acc |
| T1 | argument type mismatch at a builtin or user parameter (e.g. `TEXT \|> Bool/not()`) | acc (q1, q2) |
| T2 | operator operand kinds (`TEXT - 1`, `<` on TEXT) | acc (q3) |
| T3 | equality between disjoint types | acc (z3) |
| T4 | missing required field | dx for parameters; int for direct reads (u02) |
| T5 | field may be absent (opt) | acc (z5, t2, p5) |
| T6 | field read on a member without fields, e.g. a union with TEXT or a bare tag | acc (q5) |
| T7 | unknown field in a closed contract | partial |
| T8 | non-exhaustive WHEN/WHILE | int (r4) |
| T9 | unreachable, duplicate or post-catch-all arm | acc |
| T10 | pattern payload field is not in the variant | partial |
| T11 | literal pattern kind mismatch | acc |
| T12 | tag used both bare and with a payload | – |
| T13 | BITS width mismatch or join | partial |
| T14 | width is not a compile-time constant | partial |
| T15 | infinitely nested type | – |
| T16 | spread operand is not a single record or tagged variant | acc |
| T17 | LATEST/HOLD join mixes kinds | acc (r5) |
| T18 | MAP/SET key not key-safe, mixed-kind key, or duplicate static MAP key | partial |
| T19 | sort key type; then_by without sort_by | verified-only (s5) |
| T20 | interpolating a non-displayable value; implicit TEXT↔BYTES | runtime (z2) |
| T21 | static range and constant checks | partial |
| F1 | THEN on a continuous input [Q3] | acc (r3) |
| F2 | WHILE on an event selector | acc (z6) |
| F3 | HOLD init not continuous | acc (z7) |
| F4 | HOLD update candidate is continuous | acc |
| F5 | SKIP where data is required | partial |
| F6 | one-input LATEST | acc (z1) |
| F7 | List/append item is not an event | acc |
| F8 | FLUSH payload not tag-kind, or contains collections or ports | partial |
| F9 | HOLD init may flush | – |
| S1 | collection inside HOLD state | partial |
| S2 | port or element in HOLD, a key, a FLUSH payload, equality or interpolation | – |
| S3 | nested authority attached to a second parent, or escaping its owner | partial |
| S4 | SOURCE in a non-structural position | – |
| Y1 | recursion | elab (backend 14051) |
| Y3 | instantaneous value cycle | acc (c1) |
| Y4 | field self-reference | int (r8); acc (v3) |
| Y5 | initialization cycle | int (z4) |
| Y6 | OUT alias cycle / OUT type depends on a contextual argument | elab |
| Y7 | distributed combinational cycle | elab |
| P1 | missing PASS at a root call | dx |
| P2 | missing PASSED field | int (s3) |
| O1 | undriven OUT | elab (o3) |
| O2 | multiple producers | int (o10) |
| O3 | branch-local producer | – |
| O4 | incompatible output scopes | – |
| O5 | partial correlated forwarding | – |
| O6 | OUT identity used as a value | – |
| O7 | forwarding an already-driven output | – |
| R1 | SOURCE provider payload conflict | – |
| R2 | unknown event for the element kind | partial |
| R3 | non-SOURCE value in an event group | partial |
| R4 | payload field read with no provider | heuristic |
| D1 | no document/scene root | elab (b1) |
| D2 | slot value is not renderable (LIST of LIST, data list) | partial |
| D3 | element from the wrong root | partial |
| D4 | unknown or ill-typed style field | partial |
| D5 | `element` used outside a constructor argument | unresolved name |
| M1-M6 | DRAIN/DRAINING rules (§10) | elab (d1, d3, d2b); acc (d2) |
| X1-X5 | role rules (§11) | elab |

Rules that stay downstream but must come out as positioned diagnostics, not raw PlanError strings:
- schema evolution against the persistence catalog;
- index capacity against the target profile;
- bounded-access proofs for List/take and List/page;
- GPU/FPGA eligibility.

---

## 16. Behaviour changes against today, and example breakage

Counts come from scratchpad scripts: design/selfref2.py, mig_flow.py, mig_unions.py, the one-arm LATEST scan and the unused-parameter scan. Line numbers were checked by hand.

| # | change (source) | example sites that break | fix |
|---|---|---|---|
| 1 | no static tag narrowing (dec. 4) | TodoMVC `Theme/get(request)`, Theme.bn:86-94, plus each theme's `get` (e.g. Classic.bn:4-17). Results NUMBER \| REC \| LIST \| Fully reach NUMBER slots (`gap:`, `depth:`, `height:`) at about 94 `Theme/*` call sites | a per-kind Theme API: `material`, `font`, `text`, `depth`, `elevation`, `corners`, `sizing`, `spacing`, `spring_range`, `lights`, `geometry`, each dispatching on the theme. One complete record per kind, built as `[...defaults, overrides]`, so every field is req |
| 2 | sound unions: reads need req (dec. 5) | TodoMVC RUN.bn:201, 250-251, 300-301, 517-519, 829, 1049 (`Theme/material(of: X).color`, `Theme/font(of: X).size/.weight/.color`) | covered by #1 (complete records) |
| 3 | opt field read (dec. 5) | TodoMVC RUN.bn:56-63: appending `[title: ...]` makes `completed` opt; `item.completed` becomes T5 and its `__` arm redundant | append `[title: title_to_save, completed: False]` and pass `initial_completed: item.completed` |
| 4 | union field reads without narrowing (dec. 5) | persons_pro RUN.bn:142, 257-258, 267-268: `passkey.registration_succeeded.workspace_grant_bound` on a tag union | `WHEN { RegistrationSucceeded[workspace_grant_bound, ...] => ...  __ => SKIP }` |
| 5 | field self-reference (dec. 6, [Q1] default) | 110 exact `[x: x]` copies, e.g. counter.bn:130, todomvc.bn:199, server_http_echo.bn:33 (`response: response`, where the intended value is `outputs.response`), TodoMVC RUN.bn ×10 `element: [events: events]` (510, 601, 637, 682, 701, 720, 813, ...), fjordpulse ×~30, Theme.bn ×10. About 21 more indirect hits need review; NovyWave RUN.bn:4407 is already the documented alias idiom. TodoMVC RUN.bn:132-135 `title: LATEST { title ...}` is also an init cycle | outer aliases. Under the [Q1] alternative only TodoMVC's title becomes legal as intended, and counter_latest.bn:10, interval_latest.bn and NovyWave RUN.bn:3022 switch to `store.`-qualified self-reads |
| 6 | one-input LATEST (OUT plan) | 80 blocks: 79 in the bytes_*_plan_ops.bn fixtures (inside HOLD bodies), and language_surface/current/reactive_temporal_operators.bn:6 | remove the LATEST wrapper |
| 7 | THEN over continuous (flow table, [Q3] default) | persons_pro, 14 sites (RUN.bn:141, 147, 160, 256, 265, 284, 285, 311-313, ...); NovyWave, 4 (RUN.bn:109, 166, 247 and the comparison equivalent) | give the effect outcome or page request a name as an event value, and THEN on that |
| 8 | unused ordinary parameters (OUT plan) | heuristic scan, about 8: NovyWave RUN.bn `selected_cursor_value_for_signal(format)`, `new_marker(store)`; NovyView.bn ×4; kavik_cz KavikHome `skill(accent)`; persons_pro Components `text(of)` | remove the parameter |
| 9 | recursion, instantaneous cycles | none found in examples (function-only SCCs are empty, C9). The value SCCs pass through HOLD or stateful LATEST, but TodoMVC `new_todo` is legal only under [Q2] | – |
| 10 | stricter parameter kinds | NovyWave RUN.bn:136 and cells/formula.bn:25 pass a NUMBER to `Text/concat with:`. They stay legal only because `with` is DISPLAYABLE | none |
| 11 | language_surface fixtures | structured_out_and_pass.bn: `PASS: [theme: theme]` has an unresolved `theme` and a self-reference; `List/sort(item, by:)` (also in typed_list_pipelines.bn) is not a catalog builtin | mark these as parser-only fixtures, or fix them |
| 12 | explicit unreachable arms (T9) | not yet counted; expected in the Theme split | delete the arms |
| 13 | became acceptances | C3 app_diff and latest_mixed_list (field order); o8 (a valid record-wrapped OUT producer) | – |
| 14 | became consistent | t1 (static selector) and t2 (dynamic) are **both** rejected: the result is `[x?, y?]` and `need_x` needs x req. v12/v13 no longer depend on summary eligibility | – |

---

## 17. Conformance probe set (expected v2 outcome)

Taken from verify_all C8/C10 and the probes in this panel (design/tspec, design/tspec2).

| outcome | probes |
|---|---|
| Rejected | q1 (T1), q2 (T1), q3 (T2), r3 (F1), r5 (T17), z1 (F6), z2 (T20), z3 (T3), z5 (T5), z6 (F2), z7 (F3), t1 (T5), t2 (T5), p1 (T4), p5 (T5), q5 (T6), c1 and cyc1-4 (Y3), r4 (T8), r7 (Y1), r8 (Y4), v3 (Y4 under the documented rule), s3 (P2), u02 (T4), s5 (T19 in both intents), o3 (O1), o10 (O2), d1/d2/d2b/d3 (M-family) |
| Accepted | x05, counter_latest, interval_latest, app_diff, latest_mixed_list, o8 |
| Accepted only under [Q2] | the NovyWave page-result narrowing pattern (RUN.bn:94-103) and TodoMVC `new_todo` |

Each probe is kept as a fixture with its expected diagnostic **code and primary span**, not message bytes.

## owner_questions
- **question**: Q1. Field self-reference: `[x: x]`. The documented rule makes the field refer to itself, so this is an error (LANGUAGE_SEMANTICS.md:216-219). Examples copy a parameter into a same-name field 110 times, e.g. `element: [events: events]` in counter.bn:130 and 10 times in TodoMVC RUN.bn. Keep the documented rule, or say instead that in the initializer of explicit field `f` the bare name `f` resolves outward (the field is not in scope for its own initializer), while qualified paths such as store.f still reach the field? | **recommendation**: Adopt own-name exclusion. It makes all 110 copies legal and makes TodoMVC `title: LATEST { title, title_to_update }` mean what was intended. Only counter_latest.bn:10, interval_latest.bn:8 and NovyWave RUN.bn:3022 would switch to a qualified `store.` self-read. Accidental self-reference is still caught, either as an unknown identifier or as a Y3/Y5 cycle. If you decline, the default is the documented rule plus migrating 110 sites to outer aliases. | **why**: This is the largest example-breakage item for any strict checker. It also decides whether counter_latest's bare self-read or `[x: x]` punning is the idiom. Decision 6 lists field self-reference as an error, but it probably did not account for the 110 copy sites. | **options**: (a) Documented rule: self-reference is an error and 110 sites are migrated. (b) Own-name exclusion. (c) Documented rule plus a quick-fix tool.
- **question**: Q2. Is a LATEST with a continuous (initial) arm a state cell, i.e. a temporal boundary with register semantics where reads see the committed value within a tick, exactly like HOLD? The docs list only SOURCE, HOLD, publication and async effects as boundaries (LANGUAGE_SEMANTICS.md:219-222). | **recommendation**: Yes. counter_latest.bn:5-6 documents it ('An initial arm makes this field stateful'), and the current code already treats 'initial LATEST' as state (reactive.rs:2463; typecheck test then_accepts_transitions_from_initial_latest_state). The self-read counter pattern (counter_latest, interval_latest, NovyWave value_format) and TodoMVC new_todo's title_to_update/edited_title/draft_title cycle are legal only under this rule. The runtime must implement the same semantics; this needs checking with scenario differentials. | **why**: It decides which cycles are instantaneous (Y3) and which programs are legal. The checker's cycle rule has to match the runtime evaluation order. | **options**: (a) Stateful LATEST is a state cell (recommended). (b) Only a self-read inside its own event arms is delayed (TodoMVC new_todo then needs HOLD). (c) LATEST is never a boundary (counter_latest must use HOLD).
- **question**: Q3. THEN over a continuous value. The flow table (TYPE_INFERENCE:183) and decision 6 (`5 |> THEN {6}`) make this an error. persons_pro (14 sites, e.g. RUN.bn:141 `passkey.registration_succeeded |> THEN`) and NovyWave (4 sites, RUN.bn:109, 166, 247) use it to mean 'when this state changes'. Keep it an error, or add a change-event meaning for non-constant continuous inputs? | **recommendation**: Keep it an error and rewrite the 18 sites to name the event-valued effect outcome (e.g. `registration_result: store.elements.register_passkey |> THEN { DevelopmentPasskey/register(...) }`) and THEN on that. Events stay explicit and the flow lattice stays at three points. | **why**: A change-event rule needs a constant-versus-varying distinction in flows and a runtime change detector. The documented table forbids it. | **options**: (a) Error, rewrite 18 sites (recommended). (b) THEN over a varying continuous value fires on committed change; constants are rejected.
- **question**: Q4. Unique-suffix bare-name resolution. A bare name with no lexical binding resolves to the unique root record field path ending in that name (boon_typecheck lib.rs:14385-14393). This is undocumented, but TodoMVC (`visible_todos`, `selected_filter`) and NovyWave RUN.bn:4506 depend on it. Codify it, or remove it and require `store.` or PASSED? | **recommendation**: Codify it for now: lexical names first, then a unique root field path within the same role program, and ambiguity is error N2. The resolver keeps a global name index so edits that add a colliding name invalidate exactly the affected references. Revisit removal later. | **why**: It is existing semantics that examples rely on. It is non-local, which matters for warm edits and diagnostics, so it should be decided explicitly rather than inherited. | **options**: (a) Codify (recommended). (b) Remove and qualify the example references.
- **question**: Q5. Does a bare tag pattern `T` match a tagged value `T[...]`, and may one type contain both a bare `A` and a tagged `A[...]`? The docs say bare and tagged variants are distinct in v1 (TYPE_INFERENCE:136-139), and NovyWave matches `HierarchyPage =>` against `HierarchyPage[rows, ...]` values (RUN.bn:94-103). | **recommendation**: The bare pattern `T` matches variant T with or without a payload. A tag name has a single arity within one type: joining `A` with `A[...]` is T12, and `A[]` normalizes to `A`. | **why**: This fixes pattern semantics, exhaustiveness and selector narrowing for the most common NovyWave idiom. | **options**: (a) Recommended. (b) Keep them fully distinct: NovyWave arms must list payload fields.
- **question**: Q6. Should render constructor results be an opaque ELEMENT kind? Slots would accept only constructor results plus documented tags (NoElement, Hidden[text], Reference[element]); programs could not read an element's fields or build one from a record. | **recommendation**: Yes. No example builds or inspects element records. LIST joins of differently shaped element records (94 in NovyWave, 16 in TodoMVC, 18 in persons_pro) collapse to one member, and contract checking gets simpler. Display stays `[kind: Button]`. | **why**: The type-notation doc describes elements as structural objects, so this is a change to static semantics even though no program observes it. | **options**: (a) Opaque ELEMENT (recommended). (b) Structural records with Optional-field joins.
- **question**: Q7. Closed render contracts, and how Optional fields are displayed. Is an unknown field in element, event or style records an error (today these are partly lenient, BOON_TYPE_NOTATION 'direct render fields are not rejected'), and should an Optional field display as `field?: T` in hints and diagnostics? | **recommendation**: Yes to both. Closed contracts catch typos in style and material records under strictness, but the catalog must list every key the renderer consumes; generate it from the renderer and test that it is complete. Show Optional presence as `?` next to the field name. This is display notation only, not source syntax, just as `LIST<T>` is. | **why**: Strictness and decision 7 display rules need an answer. The doc has no notation for absence-capable fields. | **options**: (a) Closed contracts plus `f?:` display (recommended). (b) Open style records. (c) A different display spelling.
- **question**: Q8. FLUSH boundaries. Is a BLOCK variable initializer a FLUSH boundary ('named binding initializer', foundations:1583-1600), or only record fields, FUNCTION return, BLOCK final result and root result? | **recommendation**: BLOCK variables are not boundaries. Their payload propagates to the consumers and is unwrapped at the BLOCK final result. Record fields remain boundaries. cells/formula.bn:14-26 relies on this: `column` would otherwise be typed `record | InvalidColumnPosition` and `column.label` would be an error. | **why**: It changes the types of local bindings and decides whether Cells code is rejected. | **options**: (a) Only record fields, FUNCTION, BLOCK final and root are boundaries (recommended). (b) Every named binding, including BLOCK variables, is a boundary.
- **question**: Q9. FUNCTION declarations nested inside BLOCK. They parse today but crash the kernel, and no example uses them. | **recommendation**: Reject them in v1 with diagnostic N7. Revisit only if needed; they would close over monomorphic locals, which the scheme design supports. | **why**: This keeps the scope and scheme model to module-level functions. | **options**: (a) Reject (recommended). (b) Support them as closures over monomorphic locals.

## risks
- **risk**: Migrating the examples is a large job: 110 field self-reference copies (if Q1 keeps the documented rule), 80 one-arm LATEST blocks, the Theme refactor across 6 TodoMVC files and NovyTheme, 18 THEN-over-state sites, and scattered union field reads. The first strict checker run will likely surface more. | **mitigation**: Check the examples against the spec before measuring. A parser-aware rewrite tool (deleted after use, OUT plan:1344-1346) handles the mechanical cases (one-arm LATEST, outer aliases); Q1's alternative removes the biggest class. Track the migration as its own work item with a per-example pass list.
- **risk**: The checker's cycle and flow rules (stateful LATEST as a register, THEN input rules, SKIP-arm presence) may disagree with the runtime's actual tick semantics. A program the checker accepts would then behave differently from what the rules promise. | **mitigation**: Resolve Q2 and Q3 first. Add scenario differentials on the runtime for each temporal rule (counter_latest, new_todo edit and blur, NovyWave page offset reset) before cutover. This panel could not run scenarios outside the repository.
- **risk**: Closed render and style contracts need a complete catalog of every key the renderer consumes (scene materials use dozens: glass_highlight, frosted_blur, checkbox_*). Keys missing from the catalog show up as false D4 errors. | **mitigation**: Generate the style and event contracts from the renderer's own key tables, and add a test that every key the renderer reads appears in the catalog and the reverse.
- **risk**: Symbolic spreads, tag differences and residual predicates in schemes could grow for heavily layered helpers, so per-call-site instantiation cost returns. | **mitigation**: Simplify schemes (merge predicates per variable, remove polar variables), hash-cons them, and memoize instantiation on (scheme, actual TypeIds). Measure scheme sizes on NovyWave. Cap residual size and report a diagnostic asking for a concrete operand if the cap is exceeded; examples spread no parameters today.
- **risk**: Unique-suffix name resolution (Q4) is non-local. Adding a same-named root field anywhere can make an existing reference ambiguous, which hurts warm-edit invalidation. | **mitigation**: Keep a global name-to-root-path index with reverse dependencies per bare name, so an edit re-resolves only references with that name. Or take Q4's removal option.
- **risk**: Opt presence makes reads of joined records fail where the old widen accepted them. Users may find `field may be absent` diagnostics surprising, because Boon has no presence test. | **mitigation**: The diagnostic names the arm that lacks the field and suggests the spread-default idiom `[f: default, ...r]` or completing the record. The Theme refactor shows the complete-record style as the house pattern.
- **risk**: SOURCE payload typing now depends on providers, so examples with unbound sources whose payload fields are read (for example scenario-injected sources) would get R4 errors. | **mitigation**: Scan examples for unbound SOURCEs with payload reads during migration, and bind them through host_ports or element events; add a catalog entry for any legitimate injected-source profile.

## work_items
- **id**: TS1 | **title**: Write 'Static semantics v2' into LANGUAGE_SEMANTICS.md | **description**: Turn this spec into normative text: types and flows, the union model, the rules per construct, cycles, SOURCE providers, FLUSH boundaries, display rules, and the owner answers to Q1-Q9. Explicitly supersede TYPE_INFERENCE plan sections, TENS_OF_MILLISECONDS plan:115 (order in identity) and BOON_TYPE_NOTATION:217 (stripe narrowing). | **size**: M | **acceptance**: The owner signs off. Every diagnostic code in §15 maps to one rule paragraph, and no rule is left for the implementation to define.
- **id**: TS2 | **title**: Conformance fixture suite | **description**: Add small .bn fixtures, one positive and one negative for every rule and diagnostic code (about 150), plus the verify_all probe set (q1-q5, r3-r8, s1-s5, t1, t2, p1, p5, u01-u05, x01-x06, c1, o3-o11, d1-d5, v3, v12, v13) and this panel's z1-z7 and cyc1-5. Each fixture carries its expected code and primary span. | **size**: M | **depends_on**: TS1 | **acceptance**: A fixture runner compares (code, span) sets, not message bytes. All fixtures pass on the new checker, and each rule has at least one negative fixture.
- **id**: TS3 | **title**: Builtin signature catalog as data | **description**: Build one static table of typed builtin signatures: parameters (ordinary or OUT), type schemes, flow requirements, defaults, static-argument checks, contextual scopes, flow class, render roots and role availability. It covers List, Map, Set, Text, Number, Bits, Bytes, Bool, Stream, Timer, File, Http, Dependency and SessionInfo, and replaces BuiltinSignatureRegistry (lib.rs:30651) and the kernel builtin ABI. User FUNCTION schemes use the same representation. | **size**: L | **depends_on**: TS1 | **acceptance**: No parameter is typed Unknown. The catalog is built once, never per compile. Every builtin used in examples/ resolves. The typed signature tests pass.
- **id**: TS4 | **title**: Render and host-port contract registry | **description**: Build closed contracts per (root, element kind): ElementConfig, EventGroup payloads, Style records, slot rules and the ElementState binder, plus host_ports contracts (http, websocket, timer, file). Generate them from the renderer and host key tables. | **size**: M | **depends_on**: TS3 | **acceptance**: A two-way completeness test against the renderer and host keys passes. The SOURCE payload heuristic at lib.rs:39159-39243 has no remaining consumers.
- **id**: TS5 | **title**: Example migration to the strict spec | **description**: Apply the §16 fixes. Theme per-kind refactor with complete records for TodoMVC and NovyTheme; TodoMVC append and title fixes; persons_pro and NovyWave union reads and THEN-over-state (per Q3); one-arm LATEST removal (80 sites); field self-reference (per Q1: aliases or the 3 qualified self-reads); unused parameters; language_surface fixture status. | **size**: L | **depends_on**: TS1, TS2 | **acceptance**: All examples in examples/manifest.toml check with 0 diagnostics under the new checker. Scenario runs for counter, TodoMVC, Cells and NovyWave still pass on the runtime.
- **id**: TS6 | **title**: Diagnostic catalog with codes and wording | **description**: Write stable codes N/C/T/F/S/Y/P/O/R/D/M/X with Boon-vocabulary templates, a primary span plus secondary spans (callee use site, PASS provenance, cycle edges), rules for suppressing cascades (poison type), and the per-definition and per-call-site reporting policy. | **size**: S | **depends_on**: TS1 | **acceptance**: Every message avoids internal terms (TypeVar, VALUE, Record, Bool, Event). Every code has a fixture.
- **id**: TS7 | **title**: Runtime semantic alignment for temporal rules | **description**: Confirm or align the runtime's tick semantics for stateful LATEST (register reads, Q2), THEN inputs (Q3), SKIP-arm presence in WHEN, and FLUSH boundaries (Q8). Add scenario differentials for counter_latest, the TodoMVC new_todo edit and blur flow, and NovyWave page offset resets. | **size**: M | **depends_on**: TS1 | **acceptance**: Every temporal rule in the spec has a runtime scenario whose result matches the rule.
- **id**: TS8 | **title**: Display-order side table and printer | **description**: Implement decision 7: order-free TypeIds, plus a per-expression presentation order (written order; first appearance for joins and spreads; first use for parameter rows), `f?:` for Optional fields, `True | False`, and ELEMENT shown as `[kind: ...]`. One printer serves hints, the inspector and diagnostics. | **size**: S | **depends_on**: TS1 | **acceptance**: Golden hint and diagnostic snapshots show written order for literals and first-appearance order for joins. Hash-consing gives the same TypeId for records whose fields are written in different orders.

## evidence
- decisions.md 4-7: no narrowing, sound unions, strict, order-free identity (scratchpad/decisions.md)
- LANGUAGE_SEMANTICS.md:216-222 whole-scope shadowing, `[item: item]` is self-referential, cycles need SOURCE/HOLD/publication/effect boundaries
- LANGUAGE_SEMANTICS.md:426-442 pattern surface; foundations:268-351 rejected patterns (no record/list/runtime-type patterns)
- TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:177-188 flow table (THEN on continuous is an error; WHILE needs a continuous selector; HOLD continuous init); :468-488 negative fixtures (LATEST/HOLD incompatible, recursion)
- OUT plan:465-487 shadowing and outer alias; :497-513 cycles; :538-553 scope effects; :603-641 producers and unused parameters; :1051-1052 one-input LATEST rejected
- foundations:985-994 collection aliases share one authority (basis for invariant accumulator element types); :1153-1177 key safety; :1309-1343 collection-in-HOLD; :1583-1640 FLUSH boundaries
- BYTES_SEMANTICS.md:78-104 no implicit TEXT/BYTES conversion; static literal refinement
- counter_latest.bn:5-12 and interval_latest.bn document stateful LATEST self-read; reactive.rs:2463 treats 'initial LATEST' as a state authority
- Probes (design/tspec, design/tspec2) via boon_cli check: z1 one-arm LATEST, z2 record interpolation, z3 1==TEXT, z5 opt read after append, z6 WHILE on event, z7 HOLD event init, cyc1-cyc4 cycles incl. `a: b+1, b: a+1`, cyc5 `[title: title]` in a function: all pass today; z4 self-read in a LATEST continuous arm dies with an internal provenance-cycle error
- owner.rs:15583-15585 THEN mode silently drops continuous input (no diagnostic); owner.rs:15611-15631 latest_mode ignores Absent arms; owner.rs:21876-21878 and 22222-22227 infix typing (equality unconstrained); owner.rs:21361-21390 HOLD init-only vs StructuralWiden per path
- boon_typecheck lib.rs:39159-39213 SOURCE payload typed by path spelling (text/bytes/press...); lib.rs:39228-39243 suffix matching of source paths; lib.rs:30651-30692 builtin registry with Unknown parameters (Text/concat)
- boon_typecheck lib.rs:14385-14393 unique-suffix bare-name resolution (C9); kernel_oracle.rs:156-243 current kernel diagnostic catalog (PASS on builtin, missing PASS, duplicate field, call-entry shape)
- machine.rs:35431-35455 runtime TEXT conversion: NUMBER, TEXT, bare tag, BYTES via UTF-8; records, lists and tagged objects rejected at runtime
- mig_flow.py over dump-ir: genuine THEN over continuous state in persons_pro (14, store.passkey.*) and NovyWave (4, page results); the todomvc and NovyWave store.elements.* hits are qualified SOURCE reads misclassified as continuous by the kernel
- mig_unions.py census: TodoMVC when:mixed-kind 50, list:record-shapes 16, open-object-fallback 38; NovyWave list:record-shapes 94, list:mixed-kind 42; persons_pro list:record-shapes 18
- selfref2.py: 110 exact `[x: x]` copies plus 21 indirect hits across examples; TodoMVC RUN.bn has 10 `element: [events: events]` (510, 601, 637, 682, 701, 720, 813, ...) and `title: LATEST { title ... }` at 132-135
- One-arm LATEST scan: 80 blocks, 79 in bytes_*_plan_ops.bn fixtures plus language_surface/current/reactive_temporal_operators.bn:6
- NovyWave RUN.bn:69-76 and 94-103 read fields through the WHEN selector path inside tag arms (needs selector narrowing and bare-pattern-over-tagged); RUN.bn:3019-3022 value_format stateful LATEST self-read; RUN.bn:136 Text/concat with a NUMBER `with`
- TodoMVC RUN.bn:56-63 append of `[title]` makes completed Optional; 201, 250-251, 300-301, 517-519, 829, 1049 read fields of Theme results; Theme.bn:86-94 mixed-kind get dispatcher
- cells/formula.bn:14-26 FLUSH inside a BLOCK variable then `column.label`; :38-49 Dependency/catch_cycle around a list self-dependency
- server_http_echo.bn:31-36 host_ports binding; :33 `response: response` self-reference
- verify_all C8: missing producer and DRAIN rules only in elaboration; c1 accepted by every stage; s3/r7/r8 fail at storage_contract.rs:5390. C10: type-directed singleton narrowing at solver.rs:1446-1465 and 2641-2672; x05 accepted, p1 rejected. C3: order observable in app_diff and latest_mixed_list. C9: function-only SCCs empty; value SCCs at most 22 nodes

## perf_targets
- **metric**: TodoMVC typing: resolve + schemes + roots + whole-program checks, cold | **target**: <= 8 ms | **basis**: About 4.1k checked expressions and about 480 syntactic calls. With once-per-definition schemes and hash-consed types at 0.1-1 us per node (HM-family checkers), the a_language estimate is 3-8 ms for inference. Today typecheck is about 370 ms.
- **metric**: NovyWave typing, cold | **target**: <= 25 ms | **basis**: 15.6k checked expressions, 1.8k checked calls, root SCCs of at most 22 nodes (C9); a_language estimate 10-25 ms
- **metric**: Warm edit that preserves a scheme (body edit whose scheme stays the same) | **target**: <= 1 ms re-check before cutoff | **basis**: Re-infer one FUNCTION body, then compare hash-consed scheme ids for early cutoff. No retraction is possible under monotone constraints.
- **metric**: Retractions or withdrawn requirement sites | **target**: 0 by construction | **basis**: The join is associative, commutative and idempotent and predicates only accumulate (P3). Today TodoMVC withdraws 150 of 10,640 requirement sites.
- **metric**: Call-site instantiations per syntactic call | **target**: 1 (memoized), versus today's roughly 30x per-path amplification | **basis**: C2: 10,537 compiled call sites for about 350 checked calls on TodoMVC

## deletions
- **what**: Name-spelling SOURCE payload heuristic and suffix matching of source paths (boon_typecheck lib.rs:39141-39243, SourcePayloadAccess at 37164); replaced by provider contracts (§7) | **size**: about 1-2k lines incl. collectors
- **what**: structural_widen join with open-object top and the union-while-variable rule (term.rs:1580-1682, solver.rs:2016-2034); replaced by the kind-partitioned semilattice join | **size**: about 300 lines
- **what**: Type-directed singleton narrowing and static_variants/SpecializationKey pruning (solver.rs:1446-1465, 2641-2700; owner.rs:15457-15462, 16139-16260, 21838-21874), plus syntax_selected provenance (artifact.rs:81-93; owner_body.rs:4428-4434, 4950-4970) | **size**: several thousand lines
- **what**: HOLD initial_state_surface switch between initializer-only and widened state types (owner.rs:21361-21390, 20642-20646) | **size**: about 100 lines
- **what**: Field order in type identity and order receipts (term.rs:309-321 semantic_order; requirements.rs:906-915; boon_checked lib.rs:310-316 field_order in Eq/Hash; storage_contract.rs:5404-5412 authored-first ordinals) | **size**: about 500 lines
- **what**: Duplicated DRAIN/migration legality (memory_contract.rs:2377-2552 vs machine_plan_backend.rs:3200-3486) and the unreachable validate_migration_cycles (memory_contract.rs:2619-2649); the checker owns the rules once and downstream code keeps debug asserts | **size**: about 700 lines
- **what**: Legacy test-only checker validators that never reached production (validate_output_producers lib.rs:14493-14615, owner_diagnostics project_diagnostic_facts 8448 etc.), superseded by checker rules O1-O7 and Y1 | **size**: part of the about 58k-line legacy oracle

---

# Adversarial review

## area
spec (Boon static semantics v2: typing contract for the strict checker)

## verdict
needs_changes

## major_issues
- **issue**: The temporal and flow model does not match what the runtime does. (a) Q3 says there is no runtime change detector, which is false: THEN over continuous state fires today, on every write, even when the value is unchanged. (b) A stateful LATEST's continuous arm is live, not initial-only, so the §13.2 rule that puts C-arm edges only in the initialization graph is wrong. (c) Bool/toggle's continuous input is live. (d) The 3-point lattice treats a WHEN with a SKIP arm over a continuous selector as an event (branch rule 'E if any arm is A'), so it passes THEN, HOLD-candidate and List/append checks with no defined firing rule. That makes F1 porous: `x |> WHEN { A => v, __ => SKIP } |> THEN` is accepted while `x |> THEN` is rejected. | **evidence**: Release boon_cli run probes in scratchpad/specrev (all use multi-line HOLDs): then1m.bn — `c: 0 |> HOLD c { store.s |> THEN { c + 1 } }` over a HOLD gives c=1 after bump and c=2 after a pick that rewrites s to the same value; then2m.bn (stateful LATEST s) passes c=1,2,3. lat1m.bn — `x: LATEST { store.base, pick |> THEN {1} }` follows the HOLD base 0→10, then goes to 1, then 20. tog.bn — `store.base |> Bool/toggle(when: bump)` becomes True when base changes and there is no bump. part2.bn — HOLD candidate `store.sel |> WHEN { On => store.v, __ => SKIP }` fires when sel changes (h=1) but not when v changes (v=2, h=1). Spec: §1.2 branch rule, §6.1, §13.2 'Initialization graph ... C arms', and Q3's 'why'. | **fix**: Add an explicit presence model as an owner question: which continuous changes or writes count as present for THEN, HOLD candidates, LATEST arms and Bool/toggle input. Either (a) keep THEN-over-C an error and also reject partially-present continuous values (a 4th flow point P) where an event is required, or (b) define write pulses and allow them uniformly. Reword Q3's options to describe today's actual fire-on-write behaviour. Keep live C-arm and toggle-input edges in the steady-state graph.
- **issue**: §13.2 cuts cycle edges at state cells on the assumption that reads see the committed value (snapshot semantics). The runtime does not do this: HOLD bodies read the same-tick new values of other state cells, and cross-cell cycles give results that depend on declaration order. The checker would accept, as legal, programs whose behaviour depends on source text order, which violates LANGUAGE_SEMANTICS.md:198-210 (order-independent bindings) and RUNTIME_MODEL.md:91-93 (snapshot). | **evidence**: specrev/cyc.bn: `a: 0 |> HOLD a { bump |> THEN { store.b + 1 } }`, `b: 0 |> HOLD b { bump |> THEN { store.a + 10 } }` gives a=1, b=11; cyc2.bn (the same program with a and b swapped) gives a=11, b=10. `boon_cli check` passes both. lat6m.bn: after the bump, `x: HOLD { bump |> THEN { store.base } }` reads 10 (new) rather than 0 (committed). Spec §13.2: 'A state cell's value during a tick is its committed value'. | **fix**: Make snapshot reads a hard runtime prerequisite: TS7 becomes a blocker and must pass cyc/cyc2/lat6m as differentials. Otherwise keep edges between state cells gated by the same event in the steady-state graph (report them as Y3) until the runtime is snapshot-correct. Q2 should state this explicitly.
- **issue**: The spec rejects event lists, which both flagship examples use in 27 places. §1.2 and §6.7 require `List/map new:` to be continuous (C). NovyWave and Cells map rows to events and merge them with `List/latest()`. List/latest is missing from the catalog and from the §16 breakage table. Today's checker gives List/map the flow of `new` and gives List/latest the flow of its list. | **evidence**: examples/cells/store.bn:6-21 (`cells |> List/map(item, new: item.sources.editor.select.address) |> List/latest()` ×5); examples/novywave/RUN.bn:1096-1139, 2083-2087 (×22 List/map with THEN in `new` + List/latest); boon_typecheck lib.rs:7183-7192 (mode of List/map = mode of `new`; List/latest = mode of `list`), lib.rs:27861-27863. | **fix**: Add derived event lists: a List/map with an event-valued `new` yields LIST<E T>, which may only be consumed by List/latest (plus any reducers you choose) and never stored, persisted, rendered or published. Add the catalog entry `List/latest(list: LIST<E T>) -> E T` and fixtures, and list the 27 sites.
- **issue**: The SOURCE provider model and the closed element and style contracts (§7, §9, Q7) do not match example usage. The spec knows only `element: [events: G]`. The examples use `event` (142 records), `hovered: <SOURCE port>` (137, which contradicts the documented `hovered: True` flag and would be an R1 payload conflict when the same port is also `event.press`), and `address`/`key`/`text`/`target` config keys. A press payload carries the element's `address`. Style records contain private or non-render keys. None of this is in §16, so TS5 ('0 diagnostics on all examples') is badly undercounted. | **evidence**: Census of top-level keys in `element: [...]`: event 142, hovered 137 (bound to ports, e.g. novywave/View/NovyView.bn:3388-3392 `event: [press: p], hovered: p`), events 59, tag 25, target 14, address 14. cells/view.bn:252-258 binds `address:`, and store.bn:8 reads `select.address` from the press payload. cells/view.bn:263-268 has `__selected_background`, `__selected_border` and `selected` inside style; fjordpulse/View/FjordPulseView.bn:28 has `language:` in style. The kernel accepts both spellings (owner.rs:6489 `"event" | "events"`). PASS_PASSED_AND_TODOMVC_UI_MODEL.md:146-148 documents `hovered: True`. | **fix**: Before fixing Q7, generate ElementConfig/EventGroup/Style contracts from the renderer and host tables and run them against examples/ to get the real breakage. Define `hovered` (a flag or a hover port provider), `event` versus `events`, and payload fields derived from the config (address). Add these counts to §16.
- **issue**: The stateful-LATEST rule is underspecified and differs from today. The spec makes a LATEST a state cell if any arm is continuous; today only a continuous, startup-safe first arm counts. Nothing covers several continuous arms: the second one is silently dead. The documented LATEST rules describe a stateless fallback and have an equal-event-sequence hard error with PRIORITY/EXCLUSIVE, which the spec never mentions. | **evidence**: boon_typecheck lib.rs:13006-13019 (InitialLatest requires branches.first() Continuous && startup-safe, and excludes hold_update_mergers). specrev/lat2.bn (continuous arm last): check passes, run fails with 'value target store.x is not current'. lat3.bn (`LATEST { store.base, 7, ... }`): the value 7 is never observed. LANGUAGE_SEMANTICS.md:393-407; TYPE_INFERENCE plan:183 ('continuous if it has a continuous fallback'); RUNTIME_MODEL.md:84, 193-205. | **fix**: Normative rule: at most one continuous arm, and it must be first (new F-code), otherwise reject. Say whether it is initial-only or live (see issue 1). Say whether equal-sequence conflicts are runtime-only, or add a static diagnostic when two arms are gated by the same SOURCE path. Put the stateless-fallback reading into Q2's options.
- **issue**: Q1 and Q4 interact, and the spec does not say how. Under own-name exclusion, a bare `count` inside `count`'s own initializer falls through to the unique-suffix rule and resolves to `store.count`, which is the field itself. So counter_latest, interval_latest and NovyWave need no migration, which contradicts §16 row 5, and accidental root-level `[x: x]` stays silently self-referential. It also contradicts Q1's claim that such cases are caught as unknown identifiers. server_http_echo only works if the suffix index skips the field being initialized: otherwise it is N2, because both outputs.response and host_ports.http.response match. | **evidence**: examples/server_http_echo.bn:20-36 (`host_ports: [http: [response: response]]`, `outputs.response`); examples/counter_latest.bn:7-12; boon_typecheck lib.rs:14369-14393 (a same-name initializer resolves to the field; suffix fallback via declaration_expr_for_path); verify_all.md:290 (v3 `FUNCTION f(title){[title: title]}` is accepted and TodoMVC relies on it). | **fix**: Specify the suffix index's exclusion set: the field being initialized, and whether its enclosing fields are included. List the consequence for every Q1 option in the Q1 text. Record that function-parameter copies behave as outer bindings today (v3), so option (b) codifies current behaviour.
- **issue**: §5.3 selector-path narrowing, together with Q5(a) (a bare pattern matches a tagged value), is a semantic extension. Decision 8 requires such extensions to be owner questions, and the spec presents it as settled. Decision 4 names the Rust/Roc/Elm model, and those languages do not refine the scrutinee. TYPE_INFERENCE:136-139 says bare and tagged variants are distinct. §17 also attributes the NovyWave narrowing pattern to Q2 when it belongs to §5.3/Q5. | **evidence**: Heuristic scan: about 74 arms read `<selector>.field` under a bare tag pattern (novywave/RUN.bn 61 e.g. 94-97, 4430-4437; host_service_effects.bn 12; server_effect_chain.bn 1). TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:136-139. | **fix**: Add this as an owner question. Recommended simpler default: binder patterns only (`HierarchyPage[rows] => rows`), with a mechanical rewrite of about 74 arms. This removes the stable-path identity rules, conditional per-variant predicates on parameters, and path narrowing's interaction with suffix-resolved names. Keep only residual typing for `__` and binders.
- **issue**: Collection invariance by unification causes false rejections far from their cause, and the scheme does not capture writes. (a) Unifying accumulators on a WHEN join widens both source lists everywhere: after `zs: WHEN { A => xs, B => ys }`, `xs |> List/sum()` fails if ys holds TEXT. (b) LIST subtyping 'ε equal' at read-only contracts (`items: LIST<ELEMENT|NoElement>`) forces the argument's accumulator to widen. (c) Function arguments forward authorities and mutations write through them, so a body that does `List/append` on a parameter list adds lower bounds to the caller's element type. §4.1 P lists only upper-bound predicates, and the §4.2 step-4 'drop polar variables' rule can erase these writes. | **evidence**: Spec §1.5 'Joining two collection types unifies their accumulators', §1 table LST subtyping 'ε equal', §4.2 step 4. BOON_LANGUAGE_FOUNDATIONS_PLAN.md:988-994 (function arguments forward the same authority; mutation ops submit to it). | **fix**: Split reads from writes. A read-only position, such as a contract or a join result used only for reading, is covariant (ε_src ≤ ε_view). A write through any alias adds its lower bound to every authority that can reach it. Add a `Writes(α.elem, τ)` scheme effect that survives simplification, and add fixtures for the xs/ys case.
- **issue**: The algorithm is not implementable in the order §14 gives. Step 2 runs the cycle checks (Y3-Y5) before typing, but deciding which LATEST is stateful, which arms are continuous, and whether a call's result depends instantaneously on its arguments all need flow inference, including polymorphic user FUNCTION flow schemes. The §4.1 scheme has no instantaneous-dependency summary (which params or root reads reach the result or OUT without crossing a state cell). Without one, value-graph edges through calls are either unsound or over-conservative, e.g. `a: f(x: a)` where f wraps a HOLD. | **evidence**: Spec §14 steps 2 vs 4-5; §13.1 nodes are only root paths, local nodes and anonymous state cells; §4.1 scheme fields P,S,Φ,N,eff (eff is only has-state/source/effect bits). | **fix**: Add a per-scheme dependency signature, with a steady and an initialization edge set from params, PASSED and root reads to the result and OUT positions, and run the cycle checks after flow inference. Alternatively, make stateful LATEST syntactic (first arm not event-gated) so the graph can be built during resolve.
- **issue**: Two claims do not hold for the warm edit path: that a scheme is a pure function of (body hash, callee ids, root ids read), and that nothing is ever retracted. Function bodies push effects into root variables: List/append lower bounds on root lists, WHEN-join unification of root collections, and port-payload unification with provider contracts. Editing or deleting such a body means retracting contributions to root union-find classes, and union-find cannot split. | **evidence**: Spec §14 'Incrementality' and P3; §1.5 unification; §7.2 'Each port in G is unified with its contract payload'. | **fix**: Treat effects on root variables as explicit scheme outputs included in the early-cutoff comparison. Re-solve an affected root component from its recorded inputs when those effects change. Prefer bound propagation over union-find merges for root-level variables.
- **issue**: The rules for where ports and state may be created are incomplete and contradict the examples. S4 is purely syntactic, but NovyWave creates SOURCE ports and a HOLD inside a WHEN arm through a call and returns ports from a WHEN, which §1.6 forbids. The joined row record then makes `row_elements` Optional, so View reads become T5. fibonacci creates a HOLD and Stream/pulses inside a WHILE arm inside a THEN body and applies Stream/skip to a continuous HOLD output. The spec does not say whether state may be created in THEN bodies or WHEN arms, or what that means. `sample` ('ports keep E') lets List/append store ports in list authorities, which are persisted. | **evidence**: novywave/RUN.bn:4538-4543 (`VariableRow => new_selected_signal(...)` with SOURCE and HOLD at 4736-4750+), 2083-2087 (reads item.row_elements.remove_signal); examples/fibonacci.bn:55-72; spec §1.2 sample, §1.6, §7.5. | **fix**: Apply S4 and a new state-creation rule transitively through `eff`. Decide explicitly (owner question) on per-activation state in THEN bodies and WHEN/WHILE arms. Ban ports in appended items. Add the NovyWave and fibonacci sites to §16.
- **issue**: Several product contracts are missing. Persistence needs recursive type fingerprints for semantic_schema_hash, but the spec never maps Optional presence or multi-kind unions into them. Identity sorted by SymbolId depends on the session, so any persisted or cached fingerprint must use a name-sorted canonical form. The cross-role typed transport error result has no typing. 'Session-scoped Server branch' (X3) is undefined. The console profile admits only 'fixed records', and Optional fields break that. D1 ('exactly one root') ignores BUILD.bn programs and headless programs such as server_http_echo. | **evidence**: BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:489-518; spec §1.8 'sorted slice of (field SymbolId...)'; LANGUAGE_SEMANTICS.md:307-310 and 270-276; BOON_CONSOLE.md:269-277; examples/manifest.toml:178-180 build_files; server_http_echo.bn has no document. | **fix**: Add sections on (1) type→fingerprint mapping with name-sorted canonical form, (2) cross-role result typing, (3) a static definition of a Session-scoped branch, (4) program kinds (client-UI, headless/server, build) with per-kind root rules and builtin availability, and (5) console eligibility, which rejects Optional fields or defines their layout.
- **issue**: §2.3 allows multi-kind unions in LIST literals and collection element types and attributes this to decision 5. Decision 5 covers WHEN joins only. TYPE_INFERENCE lists 'heterogeneous collection shape' as a negative fixture, and foundations says a MAP value has one shape, possibly a closed tagged union. Under Q6, render lists are already two kinds (ELEMENT | NoElement). | **evidence**: TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:480-481; BOON_LANGUAGE_FOUNDATIONS_PLAN.md:1138-1145; spec §2.3, §8.1. | **fix**: Add this as an owner question. Simplest: fold NoElement, Hidden[text] and Reference[element] into the ELM kind, so render lists stay single-kind, and require single-kind element and MAP-value types elsewhere (tags for heterogeneous data).

## missing
- Catalog and flow signatures for the temporal builtins the examples use: List/latest (27 sites), Stream/skip over a continuous HOLD output (fibonacci.bn:71), Stream/pulses, Timer/interval/deadline, and Bool/toggle's live-input semantics. Bool/toggle is undocumented anywhere in docs/.
- How FLUSH payloads (Φ) compose through calls and builtins. The spec needs: Φ(call) ⊇ ∪Φ(args), including contextual callbacks (flush_error_propagation.bn:92-104 List/map→List/count). A HOLD body that flushes makes the field type σ ⊔ Φ (flush_error_propagation.bn:68-81 `hold_handled` matches HoldError), but §6.4 types the HOLD expression as ⟨C, σ⟩ with no Φ.
- Sort keys must be pure, row-local and total (LANGUAGE_SEMANTICS.md:652-655). T19 checks only the key's type.
- Handling of the LATEST/HOLD same-event-sequence conflict (PRIORITY/EXCLUSIVE are documented but not parsed; RUNTIME_MODEL.md:193-205).
- Precedence of contextual (OUT) names over outer lexical names, except when the lexical name is declared inside the call context (boon_typecheck lib.rs:14374-14383).
- An unconstrained accumulator such as a never-written `LIST {}` should default to ⊥. §1.8's 'an unresolved variable is a diagnostic' would reject it.
- Missing predicate kinds: StateSafe (no collections or ports) and OneKind for LATEST/HOLD over parameter variables, conditional per-variant HasField for bare patterns on polymorphic selectors (e.g. novywave RUN.bn:4430-4437), and Writes effects.
- The memo key (scheme id, actual TypeIds, ψ) is invalid while actuals still contain unsolved root variables or growing accumulators (during the step-5 fixpoint).
- Catalog-level semantic choices that need owner sign-off: Text/concat accepting any DISPLAYABLE value, Bool/toggle input override, List/latest, and the payload field lists for each event.
- A conformance fixture for layout-sensitive HOLD. A single-line `x: 0 |> HOLD x { e |> THEN {..} }` compiles today to 3 operations and never updates; the multi-line form compiles to 5 (specrev/tog4.bn vs tog6.bn). The previous panel's then_state.bn probe relied on this form and fails to typecheck anyway.
- Gate wiring: nothing says how the TS2 conformance fixtures run in xtask verify-all or verify-compiler-interactions.

## simplifications
- Use one cycle graph. As constructed, the steady-state graph is a subset of the initialization graph, because HOLD init edges are never removed from it. The separate steady-state check is therefore redundant and reports initialization cycles as Y3. Keep one labelled graph and derive Y3/Y5 only for message wording.
- Drop Φ from FUNCTION schemes. FUNCTION return is a FLUSH boundary and foundations:1621 says 'no implicit cross-function exception effect', so the scheme Φ is always empty.
- Drop §5.3 path narrowing and Q5(a) in favour of binder patterns, as decision 4 (the Rust/Roc/Elm model) suggests. This removes the stable-path machinery, per-variant conditional predicates and the narrowing/suffix-resolution interaction, at the cost of rewriting about 74 arms.
- Fold NoElement, Hidden and Reference into the ELM kind so render lists and slots stay single-kind.
- Define stateful LATEST syntactically: exactly one non-event-gated first arm. State-cell identification and the cycle graph then no longer depend on flow inference, which fixes the §14 step-2 ordering.
- If ports are banned from appended items, `sample` reduces to 'set all flows to C' and needs no port special case.

## unrealistic
- '0 retractions by construction' holds within one solve, not for warm edits that remove a body's contributions to root accumulators or unification classes.
- TS5's acceptance ('all examples check with 0 diagnostics') is sized from a breakage table that leaves out about 137 `hovered: port` sites, about 142 `event:` configs under closed contracts, 27 event-list sites, the NovyWave WHEN-arm ports/state and Optional row_elements reads, Cells' style `__selected_*` keys and press `address` payload, and about 74 narrowing arms if §5.3 is dropped.
- TS7 is sized M, but it must make the runtime snapshot-correct (cyc/cyc2 are order-dependent today) and define presence for continuous values. That is a runtime semantics change, not a check.
- §17 lists 'Accepted only under [Q2]: the NovyWave page-result narrowing pattern'. That pattern depends on §5.3/Q5, not Q2.
- Q3's rationale that the runtime has no change detector is wrong. The runtime already fires THEN, LATEST continuous arms and Bool/toggle on writes, so option (b) as written ('fires on committed change') also misdescribes today's behaviour, which fires on writes even when the value is unchanged.

## extra_owner_questions
- Presence of continuous values. Does a write or change to continuous state count as 'present' for THEN, HOLD update candidates, LATEST arms and Bool/toggle input? Today THEN fires on every write, including same-value writes (then1m.bn). A WHEN with a SKIP arm over a continuous selector fires only when the selector changes (part2.bn).
- Snapshot semantics. Must a HOLD or stateful LATEST read of another cell in the same tick see the committed value (RUNTIME_MODEL.md:91-93)? Today it sees the new value, and cross-cell cycles depend on declaration order (cyc.bn a=1,b=11 vs cyc2.bn a=11,b=10).
- Stateful LATEST continuous arm: initial-only or live? Today it is live (lat1m.bn), and only a first continuous arm counts (typecheck lib.rs:13006-13019).
- Selector-path narrowing (§5.3, a TypeScript-style extension) versus binder-only patterns (Rust/Roc/Elm, as decision 4 names)?
- Event lists. Keep `List/map(new: <event>) |> List/latest()` (27 sites in Cells and NovyWave) as a language feature?
- State and SOURCE creation inside THEN bodies and WHEN/WHILE arms, directly or through calls (fibonacci.bn:57-72, novywave RUN.bn:4538-4543): allowed, and with what activation semantics?
- Collection joins: unify accumulators (sound, but widens both lists everywhere) or read-only covariant views with writes propagated to all joined authorities?
- Multi-kind collection element and MAP-value types: allowed, given the 'heterogeneous collection shape' negative fixture?
- Element config: what do `hovered: <port>` (137 sites) and `event` versus `events` mean, and do press payloads carry config fields such as `address` (Cells)?
