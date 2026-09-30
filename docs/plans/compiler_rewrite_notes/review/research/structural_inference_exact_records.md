Research note, 2026-09-29, for review/REVIEW.md; not authority.

# Structural inference with exact records and principal types

Question: does prior art support the checker in BOON_COMPILER_REWRITE_PLAN.md §4.3-4.4, and at
what cost? The checker in question combines Simple-sub-family biunification over
kind-partitioned types, sound unions (D5), exact record parameters (D23) and elements as tagged
records (D13/D20).

## Summary

1. **Nobody infers exact parameters.** No surveyed system infers a closed (exact) record type for
   a parameter from its uses. A field read infers an open row or a width-subtyped record in Elm,
   Roc, OCaml objects, PureScript, Simple-sub and MLstruct alike. A consumer becomes exact only
   through an annotation (Roc, Elm, Flow).
2. **Closed bounds are inferred only for variants.** OCaml's `[< ...]` comes from a match, because
   a match really can handle only the tags it lists. Reading a record needs no field to be absent,
   so D23 is a policy on top of inference, not something inference yields. Its cleanest encoding
   is a deterministic step after inference, and that step is not principal in the usual sense.
3. **Conflicting exact consumers are silent at the definition.** Two exact consumers of one record
   with different field sets have an uninhabitable meet; Flow's docs say so outright. Neither
   algebraic-subtyping checkers nor OCaml's variant inference report this at the definition: they
   only report producer→consumer flows. The error would appear at every call site and blame the
   caller.
4. **Forwarding breaks under strict exactness.** With invariant exactness, "forward the record to
   `g` and also read a field that `g` does not read" is always an error. The review census counts
   47 call sites that forward a whole record parameter (140 − 93).
5. **Spread and return need row variables.** Spreading or returning a parameter needs a real row
   variable, because width subtyping loses the caller's fields (Marques et al. 2024). The plan
   specifies lattice bounds only and says "the solver has no union-find".
6. **Precedents exist for the record join.** MLstruct collapses record unions field-wise by design
   and uses class tags to tell cases apart. Flow's documented fix for exponential spreads is a
   single type with optional fields. TypeScript 2.7 normalizes unions of object literals. D5's
   record member and D20's tagged elements follow the same pattern.
7. **Cost evidence is thin.**
   - Simple-sub was evaluated only on random terms of size 1-23.
   - Dolan's biunification is O((n+m)²) in the worst case and linear on trees.
   - One MLstruct stress case is commented out as "Too long!" (55,374 constraint calls).
   - Roc's checker spends about 2.5 s on about 41k lines with "very large tag-union and record
     types".
   - MLscript's current compiler, hkmc2, has not reimplemented type checking.
   - tsgo's 10x came from a port that kept the same algorithms.
8. **Recommendation.** Take D23 out of the solver lattice and make it a separate monotone
   "consumed-fields" analysis (I2). Spike S2 must measure wide and deep element records (I6).

## 1. Simple-sub (Parreaux, ICFP 2020)

**What it does.** It infers MLsub's principal types, with subtyping, unions and intersections,
using an algorithm close to algorithm W. Type variables carry mutable lists of lower and upper
bounds.

**Mechanism.**
- `constrain(lhs, rhs)` adds a bound and then checks it against each existing opposite bound.
- A cache of (lhs, rhs) pairs prevents both looping and repeated work.
- Records use width and depth subtyping, so a read `x.u` generates `x <: {u: α}`, which is open.
- Let-polymorphism uses levels plus extrusion.
- Simplification uses co-occurrence analysis (removing polar variables, unifying
  indistinguishable ones, flattening "sandwiches") plus hash consing.

**Evidence.**
- [verified: Simple-sub paper PDF v1.9 [S1], §3.2.2] The cache "avoids repeating identical work
  ... which is important to avoid making the algorithm exponential in complexity". The
  Record/Record case reports `missing field` and otherwise constrains field by field, with the
  usual width and depth subtyping.
- [verified: [S1] §4.3.1] The simplifications "preserve principality" because each result is
  equivalent to its input. Example: a variable that occurs only negatively is removed, so
  `α ⊓ int → int` becomes `int → int`.
- [verified: [S1] §6]
  - The evaluation checks correctness only, not speed: 1,313,832 random expressions of size
    1-23, "at most three different field names per expression", checked against MLsub by
    mutual subsumption.
  - It reports no throughput.
  - It found an MLsub simplifier bug, so MLsub's simplifier had to be disabled.
- [verified: [S2] `Typer.scala`, master] Field access is
  `constrain(obj_ty, Record((name, res) :: Nil))`. There is no closed-record type form.
- [verified: [S3], Dolan's thesis §7.4.1] Biunification of scheme automata is O((n+m)²) in the
  worst case and O(n+m) when the automaton is a tree. The only speed claim is that "our online
  demo retypes the input program on each keystroke, without noticeable delay".
- [verified: [S4], Bhanuka, Parreaux et al. 2024, §2.2-2.3]
  - "Algebraic subtyping algorithms like MLsub usually only report Level-0 errors", meaning
    direct producer→consumer flows.
  - A value used as both Int and Bool is not rejected; it can be typed "(Int ⊓ Bool) → (Bool,
    Int)".
  - The paper renders errors as flow paths through source locations. Its user study found no
    quantitative improvement in error localization.

## 2. Simple-sub with rows (Marques, Florido, Vasconcelos 2024)

**What it does.** It adds Rémy-style rows (`l: θ, ρ | Abs | α`, fields `Pre τ | Abs | α`) and
row expansion to Simple-sub.

**Evidence.** [verified: [S5]]
- With width subtyping alone, `λr. r with {y = 1}` gets `{} → {y: int}`, "which loses all
  information about any existing fields of r".
- With rows it infers `{y: β, ρ} → {y: int, ρ}`.
- The work is unfinished: "This is ongoing work". Soundness and completeness proofs are "not ...
  yet done".

**Relevance.** A Boon parameter that is spread or returned whole needs row variables. The only
Simple-sub-with-rows design is an unproven work in progress. spec.md:227 already needs "a
symbolic spread in the scheme". spec.md:228 says no example spreads a parameter.

## 3. MLstruct (Parreaux & Chau, OOPSLA 2022) and MLscript today

**What it does.** Principal inference in a Boolean algebra of types (union, intersection,
negation), with width-subtyped records, class tags and instance matching. Constraints are solved
over normal forms.

**Evidence.**
- [verified: [S6] §2.3.2 "Simplified treatment of unions"]
  - MLstruct identifies `{x: τ1} ∨ {y: τ2}` (x ≠ y) with ⊤, and `{x: Int, y: Int} ∨ {x: Str,
    y: Str}` with `{x: Int ∨ Str, y: Int ∨ Str}`, in order "to keep the expressiveness of unions
    in check".
  - "To make unions of different fields useful, one needs to 'tag' the different cases with
    class types."
- [verified: [S6] §3.5] The implementation is about 5,000 lines of Scala. Its test suite of more
  than 4,000 lines runs in about 2 s in parallel on an 8-core i7. That is the only
  throughput-like figure found for this family.
- [verified: artifact [S7], `TyperHelpers.scala` `recordUnion`] The union keeps only the shared
  fields.
- [verified: [S7] `Simple.mls`]
  - `def f x = x.u` gives `{u: 'u} -> 'u`, which is open.
  - `if true then { u = 1; v = 2 } else { u = 2 }` gives `{u: 1 | 2}`, so `v` is dropped.
- [verified: [S7] `Stress.mls`]
  - An 8-way class match applied to unions of 2 to 8 classes costs 38 to 122 constraint calls,
    growing linearly.
  - A retained, commented-out case records "constrain calls: 55374, annoying calls: 423405" and
    "Too long!".
  - [not verified: whether that case is still slow.]
- [verified: [S8], MLscript README; default branch `hkmc2`, pushed 2026-09-30]
  - "`hkmc2` does not yet reimplement type checking".
  - The authors are "working on a new type checker that will be more expressive and more
    efficient than the old one".

## 4. OCaml objects and polymorphic variants

**Evidence.** [verified: [S9] objects chapter, [S10] polymorphic variants chapter, manual 5.3]
- Using a method infers an open type, and requirements union across uses:
  `get_succ_x : < get_x : int; .. > -> int` and
  `incr : < get_x : int; set_x : int -> 'a; .. > -> 'a`.
- Closed object types of different shapes are incompatible without an explicit coercion ("since
  a point has no method color").
- A match infers a closed upper bound: `f : [< \`Number of int | \`Off | \`On ] -> int`.
- Two closed consumers are intersected without an error:
  - `f1 : [< \`A of int | \`B | \`C ]` and `f2 : [< \`A of string | \`B ]` give
    `f x = f1 x && f2 x : [< \`A of string & int | \`B ]`.
  - `C` silently disappears, and `A` gets an uninhabited payload.
- The "Weaknesses" section:
  - It calls polymorphic variants "a weaker type discipline".
  - It advises annotating the definition, because an inferred bound can absorb a typo (`` `As ``).

## 5. Elm and PureScript

**Evidence.**
- [verified: [S11], Elm `Type/Constrain/Expression.hs`]
  - `Can.Access` builds `RecordN (Map.singleton field fieldType) extType` with a fresh
    `extVar`, so a read is open.
  - `constrainRecord` uses `EmptyRecordN`, so a literal is closed.
  - Record update also gets a fresh extension variable.
- [verified: [S12], PureScript Types.md "Row Polymorphism"]
  - `addProps o = o.foo + o.bar + 1` infers `forall r. { foo :: Int, bar :: Int | r } -> Int`.
  - `addProps { foo: 1, bar: 2, baz: 3 }` compiles.

## 6. Roc

**Evidence.**
- [verified: [S13], Roc tutorial source at git sha 80438f4c. www.roc-lang.org/tutorial now redirects
  to the new compiler's mini-tutorial, which does not cover this.]
  - "Open records are what the compiler infers when you use a record as an argument". Closed
    records are what it infers "when you create a new record".
  - It motivates closed type aliases with: "it may be that the extra fields were included due to
    a mistake rather than on purpose, and accepting an open record could prevent the compiler
    from raising an error that would have revealed the mistake".
- [verified: [S14], Roc langref records.md at main 00cab95a (2026-09-30)]
  - Open records are written `{ name : Str, .. }` or `..rest`.
  - Record update "can only replace fields that already exist".
  - Optional fields `label ?: Str` must be read with `.?label`, which returns
    `Try(Str, [MissingField])`.
- [verified: [S15], Roc issue #11615, opened 2026-09-23]
  - The new Zig compiler on a 27-package app spends about 2,500 ms in Type Inference and about
    2,080 ms in Monotype Lowering, for 4.5 s wall time.
  - "Almost all of the time is roc-pdf": about 41k lines "with very large tag-union and record
    types". Without that package the app builds in 0.58 s.
  - Suspects: row operations that copy or sort fields per unification.
  - Roc's checker rules forbid "give-up-after-N counters".

**Relevance.** Roc has D23's motivation but gets exactness from annotations. Its measured
superlinear pain point is large record and tag types, which is exactly D13's structural element
trees.

## 7. Flow: exact by default

**Evidence.**
- [verified: [S16], Flow Changelog.md]
  - 0.105.0 added the exact-by-default option and "Fixed an exponential-blowup issue with a
    combined use of spreads and unions".
  - 0.122.0 "Fixed an unsoundness in union and intersection spreads".
  - 0.202.0 made `exact_by_default=true` the default.
  - 0.205.0 made errors for spreading nested unions consistent.
  - 0.314.0 deprecated `exact_by_default=false`.
- [verified: [S17], Flow docs, objects.md]
  - "Exact object types are the default".
  - "Intersections of exact object types are usually impossible types ... uninhabitable as soon
    as A and B differ at all". Exact types combine through type spread, where "exactness
    propagates".
  - "Exponential type spread": conditional spreads multiply the cases, and Flow errors beyond a
    limit. The documented fix is "a single type that covers all possibilities using optional
    properties".
- [verified: [S18], unions.md] Disjoint unions require exact objects.
- [verified: [S19], annotation-requirement.md] "Flow requires that function parameters are
  annotated" under Local Type Inference.

## 8. TypeScript and tsgo

**Evidence.**
- [verified: [S20], TS 2.7 release notes]
  - A union of object literals is normalized "such that all properties are present in each
    constituent" (`?: undefined`).
  - Before 2.7 such a union was inferred as `{}`, the same common-fields join as MLstruct.
- [verified: [S21], issue #12936 "Exact Types"] Open since 2016-12-15, labelled "Awaiting More
  Feedback", 312 comments.
- [verified: [S17]] TypeScript's excess-property check "only fires on direct literal assignment".
- [verified: [S22], 2025-03-11] VS Code (1,505k LOC) goes from 77.8 s to 7.5 s, and Playwright
  from 11.1 s to 1.1 s.
- [verified: [S23]] The Go version is "a port that maintains the existing behavior and critical
  optimizations".
- [not verified, secondary: a commenter in [S23] says the speedup is about 3x from native code
  times 3x from concurrency.]
- [verified: [S24], `checkerpool.go`]
  - Files are partitioned across N checkers, each "with its own symbol, type, and instantiation
    caches".
  - With 4 checkers, completion times were very unequal: MUI docs took about 0.6 / 2.8 / 15.3 /
    19.0 s.

## 9. Answers to the plan's questions

**Q1. Can an exact parameter that stays polymorphic in its field types be inferred without
annotations, is it principal, and does it compose?**

- *Can it be inferred?* No system does this. It can be defined as
  `close(principal open scheme)`:
  1. Infer the usual open row `{a: α | ρ}`.
  2. After generalization, set every row variable that occurs only negatively to the empty row.
- *Is it principal?* The result is unique, order-independent and hash-consable. It is **not**
  principal in the Simple-sub sense. Simple-sub's removal of polar variables preserves
  equivalence ([S1] §4.3.1). Closing deliberately rejects callers that the open scheme would
  accept.
- *Forwarding.* Instantiating `g`'s closed row inside `f` makes `f`'s row equal to `g`'s field
  set. Any extra read in `f`, or a second exact callee, then contradicts it.
- *Spread or return.* A row that flows into the result must stay open, which needs row variables
  ([S5]). Otherwise D23 read literally gives `id(r) = r` zero consumed fields.

**Q2. Do two exact consumers with different field sets conflict, and how is the conflict
reported?**

- *Does it conflict?* Yes: the meet is uninhabitable ([S17]).
- *Simple-sub, MLsub and OCaml variants* accept the definition with an uninhabitable
  conjunction ([S4], [S10]) and fail later, at a producer.
- *MLstruct* reports Level-0 flows. The pair of consumer sites is never named unless the checker
  adds that check itself. checker.md:136 plans "an empty meet of requirements, with both
  requirement sites". Plan §4.4, however, says conflicts are "(consumer, producer) pairs".

**Q3. What are the costs?**

- *Measured per-instantiation or per-node costs:* none published for this family.
- *What exists:*
  - Dolan's bounds (O((n+m)²) worst case, linear on trees).
  - The MLstruct test-suite time and one retained blowup case.
  - Roc's roughly 2.5 s on about 41k lines of large records and tags.
  - tsgo: about 200k LOC/s in parallel (1,505k LOC in 7.5 s).
- *Implication:* the plan's unit costs (checker.md:188: 60 ns per ground expression, 0.6 µs per
  general instantiation) are estimates with no external anchor.

**Q4. Is "at most one record member, records join to optional presence" a known design?**

- *Known:* yes. MLstruct §2.3.2 collapses record unions field-wise and needs tags to tell the
  cases apart ([S6]). Flow's fix for spread blowup is optional properties ([S17]). TypeScript
  2.7 normalizes unions to optional-`undefined` fields ([S20]).
- *Boon's variant:* Boon keeps non-shared fields as Optional instead of dropping them, and makes
  them unreadable (D5). Flow reads them as `void | T`, and Roc needs `.?`.
- *Consequence:* Optional fields in Boon can only be forwarded, spread or defaulted. How they
  interact with D23 is unspecified.

## Implications for the plan

1. **I1. D23 and §4.3 "Contracts" (medium, suggests change).** The line "A scheme's record
   parameter lists exactly the fields the body consumes" has no inference precedent.
   - Proposal: specify exactness as `close()` on the principal open scheme, applied at
     compaction before hash-consing, so that the SchemeId encodes it.
   - A row that flows positively (returned whole, spread into the result) stays open.
   - Alternatively, P0 bans parameter pass-through in v1 with a positioned diagnostic.
     spec.md:228 finds no example that spreads a parameter.
2. **I2. D23 composition (critical, suggests change).** "Whole records forwarded to a callee ...
   take that destination's exact type" makes two cases illegal: forward-and-read, and passing
   one record to two exact callees.
   - Proposal: model D23 as a monotone *consumed-field set* on parameter variables. The set is
     the parameter's own reads ∪ the consumed sets of the destinations it is forwarded to.
   - Check it only where a non-parameter producer (literal, root value, row, call result) meets
     a parameter: `fields(arg) ⊆ consumed(param)`, plus the normal Required-field checks.
   - The solver stays width-subtyped, principal and order-free. The attribute sub-solver
     (checker.md:74) already fits this shape.
   - Owner decision: exactness "at every hop" (literal D23) or "consumed somewhere in the call
     tree" (this proposal).
   - Spike: re-run the census on the 47 forwarding sites under both rules.
3. **I3. §4.4 "Diagnostics" (high, suggests change).** "Conflicts are order-independent
   (consumer, producer) pairs" should add a (consumer, consumer) kind.
   - It fires at the definition when requirement meets on one variable go empty: different
     exact field sets, or TEXT versus NUMBER.
   - It names both spans, as checker.md:136 already intends.
   - The call-site errors derived from it are suppressed.
   - Evidence that nothing else catches this: [S4] (Level-0 only), [S10] (`f1 && f2` accepted)
     and [S17] (the meet is uninhabitable).
4. **I4. §4.4 "Representation" (medium, suggests spike).** "The solver has no union-find" plus
   lattice-only bounds cannot pass a caller's row through a spread or a return.
   - Add to S4: count parameters that are returned whole or spread into a result.
   - If the count is 0: forbid for now.
   - Otherwise: add Rémy `Pre/Abs` row variables on the record head only. The only
     Simple-sub-with-rows work, [S5], is unproven, so budget this separately.
5. **I5. D5 and §4.3 "Record member" (medium, supports, with a gap).** `[a: 1] ⊔ [b: 2]` is
   `[a?, b?]` matches MLstruct §2.3.2, Flow's optional-property fix and TypeScript 2.7. It also
   avoids Flow's 2ⁿ spread blowup (spec.md:162 counts up to 25 combinations per Theme spread).
   - Gap: P0 must specify Optional × D23. Proposal: an Optional field that the destination does
     not consume is an error ("may be extra"). A destination that consumes it must accept
     absence, which only contracts with Optional keys do.
   - Document `[f: default, ...r]` as the only way to read an Optional field.
6. **I6. Spike S2 and §4.4 "Model" (high, suggests spike).** S2's generator should include the
   shapes that hurt Roc ([S15]):
   - records of 50-200 fields;
   - the full union of element kinds;
   - depth 22;
   - WHEN arms that join element records with differing Optional fields;
   - chains of four or more forwarded exact parameters.
   Report constraint calls per expression and the largest scheme, not only time. Gate:
   near-linear growth when each dimension is doubled.
7. **I7. §4.4 "Oracles" (high, suggests change).** "A reference checker that inlines every call
   must give the same accept/reject result" cannot hold for D23. After inlining there are no
   parameters left to be exact. The oracle must keep call boundaries for the consumed-field
   check, or D23 must be excluded from the comparison and covered by its own fixtures. Also,
   `close()` is not a Simple-sub simplification, so the confluence test must cover it too.
8. **I8. D13/D20 and §4.3 "Contracts" (low, supports).** Closed, catalog-declared contracts are
   annotation-style exactness, and every surveyed system handles those as ordinary
   producer→contract checks (Flow, Roc). Tagged elements with per-kind payloads are MLstruct's
   "tag the cases" rule. The risk is confined to *inferred* exactness (I1-I3).
9. **I9. §4.4 "Approach" (medium, suggests change).** "HM with bolted-on joins was rejected" is
   reasonable, but every production structural checker surveyed uses row unification (Elm, Roc,
   OCaml, PureScript). The subtyping family has no production deployment, and hkmc2 dropped
   type checking [S8].
   - Budget for error explanation explicitly. Make the "explain mode" output a flow path from
     producer to consumer, as in [S4].
   - Keep a fallback note: if S2 fails, drop to unification with presence rows, since D4 has
     already removed narrowing.
10. **I10. D23 rationale (low, suggests owner option).** Roc gives D23's exact rationale, "extra
    fields were included due to a mistake", but gets it from declared closed aliases [S13].
    Offer the owner an alternative: exactness for declared shapes and contracts, width for
    inferred parameters. This changes semantics, so it is the owner's decision.
11. **I11. Checker hard caps (checker.md:329) (low, supports, with a caveat).** Flow keeps a hard
    limit for spread cases and Roc forbids give-up counters. Boon's single record member makes
    spreads linear by construction. Keep the cap only as a guard, and have S2 show that it never
    fires on the corpus.
12. **I12. §0 "Why the gap is so large" (low, supports).** tsgo shows about 10x from the same
    algorithms, using native code plus duplicated per-checker caches [S22-S24]. Boon is already
    native, so its claimed gain must come from generalize-once. Parallel per-function solving is
    a later lever, and tsgo's imbalance data shows it needs load balancing.

## Sources

1. [S1] Parreaux 2020, Simple-sub, ICFP. https://infoscience.epfl.ch/server/api/core/bitstreams/afe084e0-0050-4542-99c7-c499d2fe1620/content (PDF fetched; ACM DL returned 403)
2. [S2] https://github.com/LPTK/simple-sub (README, Typer.scala fetched)
3. [S3] Dolan 2016, *Algebraic Subtyping* (PhD thesis, Cambridge), §7.4.1. PDF downloaded earlier in this session; source URL not recorded [not re-verified]
4. [S4] Bhanuka, Parreaux, Binder, Brachthäuser 2024. https://arxiv.org/abs/2402.12637 (PDF downloaded earlier in this session)
5. [S5] Marques, Florido, Vasconcelos 2024. https://arxiv.org/abs/2407.06747 (PDF downloaded earlier in this session)
6. [S6] Parreaux & Chau 2022, MLstruct. https://lptk.github.io/files/%5Bv6.1%5D%20mlstruct.pdf (fetched; ACM PDF returned 403)
7. [S7] MLstruct artifact. https://zenodo.org/records/7121838 (zip fetched; Simple.mls, Stress.mls, TyperHelpers.scala read)
8. [S8] https://github.com/hkust-taco/mlscript (README via the GitHub API)
9. [S9] https://ocaml.org/manual/5.3/objectexamples.html (fetched)
10. [S10] https://ocaml.org/manual/5.3/polyvariant.html (fetched)
11. [S11] https://raw.githubusercontent.com/elm/compiler/master/compiler/src/Type/Constrain/Expression.hs (fetched)
12. [S12] https://raw.githubusercontent.com/purescript/documentation/master/language/Types.md (fetched)
13. [S13] Roc tutorial source at git sha 80438f4c (downloaded earlier in this session). https://www.roc-lang.org/tutorial now redirects to docs/mini-tutorial-new-compiler.md, which was also fetched
14. [S14] https://raw.githubusercontent.com/roc-lang/roc/main/docs/langref/records.md (fetched)
15. [S15] https://github.com/roc-lang/roc/issues/11615 (via the GitHub API)
16. [S16] https://raw.githubusercontent.com/facebook/flow/main/Changelog.md (fetched)
17. [S17] https://raw.githubusercontent.com/facebook/flow/main/website/docs/types/objects.md (fetched; the rendered flow.org page was also fetched)
18. [S18] https://raw.githubusercontent.com/facebook/flow/main/website/docs/types/unions.md (fetched)
19. [S19] https://raw.githubusercontent.com/facebook/flow/main/website/docs/lang/annotation-requirement.md (fetched)
20. [S20] TypeScript 2.7 release notes, microsoft/TypeScript-Website `packages/documentation/copy/en/release-notes/TypeScript 2.7.md` (fetched)
21. [S21] https://github.com/microsoft/TypeScript/issues/12936 (via the GitHub API)
22. [S22] https://devblogs.microsoft.com/typescript/typescript-native-port/ (fetched)
23. [S23] https://github.com/microsoft/typescript-go/discussions/411 (fetched and summarized)
24. [S24] https://raw.githubusercontent.com/microsoft/typescript-go/main/internal/compiler/checkerpool.go (fetched)
25. Not fetched: the Flow blog post "Exact object types by default, by default" on medium.com (403); the ACM DL pages for Simple-sub and MLstruct (403); the rendered elm-lang.org records page (no usable content).
26. Repository inputs: BOON_COMPILER_REWRITE_PLAN.md §1, §4.3, §4.4, §8 P0; compiler_rewrite_notes/checker.md:43, :74, :136, :188, :329; spec.md:162, :227-228; review/census/RESULTS.md:49.
