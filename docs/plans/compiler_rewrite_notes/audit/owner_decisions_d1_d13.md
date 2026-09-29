# Owner decisions (2026-09-29) for the from-scratch Boon compiler speed plan

1. STRATEGY: Greenfield only. Build a new compiler pipeline in new crates. Do not spend effort on
   quick wins in the old pipeline. The old compiler may be used as a test-only differential oracle
   during the migration and is deleted at cutover.
2. FREEDOM: "Change what you want - just don't change Boon syntax/semantics without discussion with me."
   Byte-identical plan/diagnostics oracles, budgets, gates, AGENTS.md rules, plan docs, crate layout,
   MachinePlan format, measurement protocol: all may change. Nobody uses the compiler in production now.
3. SELF-PROOFS: Remove receipts, digests, manifests, seals, construction images, in-process
   re-verification entirely. "I don't know what it is for, so we probably don't need it. Keep it
   as simple and fast as possible." (Verification only where genuinely needed: loading untrusted
   plans/artifacts; debug/CI.)
4. NARROWING: DROP static tag narrowing. One type per function. A WHEN's type is the union of all its
   arms; a function's requirements come from all its arms (Rust/Roc/Elm model: types never depend
   on values). Today's type-based singleton narrowing in the solver goes too.
   Consequence: the TodoMVC Theme `get(request)` mixed-kind dispatchers must be split per kind, and
   the owner additionally asked: "I don't like 20 different shapes in Theme - try to refactor it to
   better Boon code" (TodoMVC Theme/*.bn and NovyWave Theme/NovyTheme.bn).
5. JOINS: Sound, fully typed unions: `WHEN { Light => Oklch[...], Dark => TEXT {...} }` has type
   `Oklch[...] | TEXT` ("think Rust, TypeScript, Roc"). No lenient field-union widening, no
   'open empty object' fallback. Reading a field not guaranteed present is an error.
6. STRICTNESS: Strict compiler. Enforce the documented rules (e.g. TEXT |> Bool/not(), 5 |> THEN {6},
   TEXT - 1, LATEST over NUMBER vs TEXT, missing PASSED fields, recursion, instantaneous cycles,
   field self-reference, OUT producer rules, etc. are proper positioned diagnostics).
   Examples that break get fixed.
7. FIELD ORDER: Type identity ignores record field order. Display (hover, hints, diagnostics) must be
   consistent: show fields in written order; for merged/joined types use first-appearance order
   (merge left to right). Runtime already ignores order.
8. Syntax is unchanged. Any semantic change beyond the above must be listed as a question for the owner.

## Later owner decisions (same day)
9. CYCLES/SELF-REFERENCE: "self-referential should be allowed probably only inside HOLD to prevent cycles;
   self-referencing bigger items like LISTs can be especially problematic, so allow cycles only in HOLD and
   you cannot pass big things like MAP or LIST into HOLD at all; preferably find a HOLD alternative too."
   Follow-up answer: event-driven collection updates (List/append etc.) count as state like HOLD for cycles.
   LATEST, derived fields and [x: x] never break cycles.
10. WHEN payloads: binder patterns only (Rust/Roc): `HierarchyPage[rows] => rows`; no refinement of the WHEN
    subject inside arms.
11. PRECEDENCE: Pony rule - mixing two different binary operators without parentheses is an error (fix-it);
    same-operator chains fine (left-assoc); unary minus tightest. Owner: "I like simplicity, compiler should
    force the developer to add () to make precedence clear".
12. PERSISTENCE: clean schema. "identify all source of truth vs derived values and store only the least amount
    of values that will allow us to restart the app to the previous state" (also mused about a single store).
13. ELEMENTS/CONTRACTS: "all items are data - idea was to have everything structural. But unknown keys should
    not be silently ignored - you can pass only what the functions called expects/can handle." -> element
    constructors return structural records; builtin/render contracts closed.
