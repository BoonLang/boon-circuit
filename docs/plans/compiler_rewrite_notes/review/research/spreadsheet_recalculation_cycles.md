Research note, 2026-09-29, for review/REVIEW.md; not authority.

Topic: spreadsheet recalculation and cycle handling in plain dataflow (Cells,
D16; cycles, D9; change model, D30/D32; the Cells row of §5.1).

## Summary

1. Every real spreadsheet engine read here (Excel, LibreOffice Calc, IronCalc,
   Sestoft's CoreCalc/Funcalc) evaluates a **runtime-discovered** dependency
   graph with either recursion plus a "being computed" mark, or a worklist;
   none does it inside a static, recursion-free dataflow.
2. Today's Cells already *is* Sestoft's top-down algorithm: the executor's
   `Currentness::{Dirty, Evaluating, Current}` states are his
   AwaitsComputation/BeingComputed/UpToDate table, and `Dependency/catch_cycle`
   is the "report a cycle" step. D9 removes that and calls it duct tape.
3. Under D9 + "no recursion" (§4.3) + "do not add List/fold" (foundations plan),
   dependency-ordered recalculation of a dynamic graph is **not expressible**
   in plain Boon. Only bounded unrolling (K copies of the evaluator) or
   per-tick relaxation over a collection authority are expressible; both are
   full recalculation, neither is "dependents only", and both need a bound.
4. The §5.1 Cells row ("Recalculation follows dependency order. A visited/depth
   guard...") therefore describes code that cannot be written as planned.
5. Smallest fixes, ranked: a principled lazy-evaluation builtin with a cycle
   value (keeps O(N), matches all engines); a per-tick fixed point with a
   static iteration bound (this is literally Excel/Calc/Sheets "iterative
   calculation"); `List/fold`; a depth-bounded recursion allowance.
6. D32 "every write fires" matches Excel exactly: dirty propagation is
   structural, not value-based ("even if the value ... does not change").
7. But spreadsheets pair that with a DAG and an iteration bound. D9's legal
   cycles through collection updates plus D32 give unbounded microstep loops
   unless R8 adds a bound and a non-convergence diagnostic (Excel: 100 /
   0.001; Calc: Steps / Minimum change, Err:523; Sheets: maxIterations).
8. Cycle display: IronCalc and Calc mark **every** member of the loop;
   cells.scn already expects that (A1 and B1 both `cycle_error`).
9. Dynamic graphs: Excel rebuilds the dependency tree on every formula entry;
   Calc re-registers listeners; IronCalc keeps no graph at all and
   re-evaluates everything. A static checker cannot know Cells' graph.
10. cells.scn's `expect_recomputed` steps encode smart recalculation; every
    plain-Boon encoding fails them under D32. Owner call needed.

## 1. Today's Boon Cells (baseline)

**What it does.** 2,600 rows from `List/range |> List/map(new_cell)`
(model.bn). Each row holds `formula_text` in a HOLD and derives `result:
compute_result(formula_text)`. References go through `cell_result`, which
reads *another row's derived `result`* via `cells |> List/find`, wrapped in
`Dependency/catch_cycle(value: ..., on_cycle: CycleError)` (formula.bn:38-49).

**Mechanism.** The executor evaluates row fields on demand. A field being
evaluated is marked `Currentness::Evaluating`; re-entering it raises
`Error::Cycle` (machine.rs:1988-1992, 20392, 21214-21222). `CatchCycle` pushes
a catch frame and, on `Error::Cycle | Error::ListCycle`, unwinds to it and
evaluates `on_cycle` (machine.rs:25984-26003, 32192-32270).
[verified: local files read at the cited lines]

**Scenario contract.** cells.scn expects `expect_recomputed = ["A1","B1","C1"]`
style minimal-recalc sets, both cells of a 2-cycle to show `cycle_error`, and
"replace-b1-formula-removes-stale-cycle-edge" (the graph is dynamic).
[verified: examples/cells.scn steps `change-a1-updates-b1`, `cycle-error`,
`select-b1-for-cycle-break`, `replace-b1-formula-removes-stale-cycle-edge`]

This is Sestoft's algorithm (§2 below) hosted by the engine, with the cycle
report surfaced as a value. D9 makes the edge `cells → result → cells` illegal
(a derived field closes it) and R6 deletes the builtin.

## 2. Sestoft: CoreCalc / Funcalc (academic view)

**Evidence.** ITU-TR-2006-91 "A Spreadsheet Core Implementation in C#",
fetched and text-extracted. [verified: source 18]

- Static vs dynamic dependence: "A static dependence may or may not cause a
  dynamic dependence; it is an approximation" (§1.6). Cycles: "if a cell
  dynamically transitively depends on itself, then there is a dynamic cycle".
- Order: bottom-up (topological) or top-down (demand): "When the value of an
  as yet uncomputed cell is needed, then that cell is computed ... will
  terminate unless there is a dynamic cyclic dependency" (§1.7.1).
- Algorithm (§2.11): flags `visited`/`uptodate`; "if visited is true, then the
  cell depends on itself; stop and report a cyclic dependency"; states
  AwaitsComputation / BeingComputed / UpToDate. Deep recursion is the known
  cost ("This may cause deep recursion if there are long dependency chains").
- Full recalculation: "each recalculation evaluates every formula exactly once
  ... linear in the sum of the sizes of all formulas" (§3.3).
- Minimal recalculation needs an explicit support graph; event listeners
  fail because "a dynamic cyclic dependency will cause an infinite chain of
  events, unless a separate cycle detection scheme is implemented" and
  "one needs a separate mechanism to determine the proper recalculation
  order anyway" (§3.3.1).
- Topological sorting is fragile under edits ("simple edits ... can radically
  change the topological sorting", §3.3.3, figure 3.2).
- Non-strict IF: "They report a cyclic dependency involving the argument of a
  non-strict function only if the argument actually needs to be evaluated.
  ... This is how Excel and OpenOffice work" (§1.7.6).
- "Spreadsheets are functional programs ... recalculation ... is driven by
  availability of input, or forwards" (§1.9).

Bock's literature review (ITU-TR-2016-199, fetched) adds the Funcalc
"standard minimal recalculation": four states dirty / computing / enqueued /
uptodate, Mark then Evaluate phases, recursion for referenced cells, and the
`enqueued` state exists only to avoid false cycle reports. [verified: source 19,
pp. 3-4] The parallel/dynamic-cycle paper (source 20) returned HTTP 403.

## 3. Microsoft Excel

**Evidence.** learn.microsoft.com "Excel Recalculation", "Multithreaded
recalculation in Excel", "Excel performance - Improving calculation
performance"; support.microsoft.com circular-reference page. [verified:
sources 1-4]

- Three stages: "Construction of a dependency tree", "Construction of a
  calculation chain", "Recalculation of cells". "During recalculation, Excel
  revises this chain if it comes across a formula that depends on a cell that
  has not yet been calculated. In this case, the cell that is being
  calculated and its dependents are moved down the chain." (source 1)
- Dynamic graph: "When a structural change is made to a workbook, for
  example, when a new formula is entered, Excel reconstructs the dependency
  tree and calculation chain." (source 1)
- Dirty marking is transitive and structural: "All direct and indirect
  dependents are marked as dirty" (source 1); "Excel continues calculating
  cells that depend on previously calculated cells even if the value of the
  previously calculated cell does not change when it is calculated."
  (source 3). A formula "can be calculated multiple times per recalculation"
  when the chain order is wrong (source 3).
- Cycles: "If a cell depends, directly or indirectly, on itself, Excel
  detects the circular reference and warns the user." (source 1). UI: warning
  dialog, status bar "Circular References" plus one address, cell shows "0 or
  the last calculated value"; iterative calculation off by default, "Excel
  stops after 100 iterations, or when values change by less than 0.001"
  (source 4). "Clear the iteration box so that if you have accidental
  circular references, Excel will warn you and will not try to solve them"
  (source 3). "Calculate" appears in the status bar while iteration is on.
- Volatile: NOW, TODAY, RAND, OFFSET, INDIRECT, CELL, INFO "recalculated at
  each recalculation even if it does not seem to have any changed
  precedents" (source 3). INDIRECT/OFFSET are the dynamic-reference
  functions, and they are single-threaded (source 2).
- MTR: "Excel tries to identify parts of the calculation chain that can be
  recalculated concurrently"; circular references and data tables "always
  calculate single-threaded" (sources 2, 3).
- Excel's own formula language grew a bounded fold (`REDUCE(initial, array,
  lambda)`, source 6) and recursion via LAMBDA ("Excel can return a #NUM!
  error if there are too many recursive calls", source 5). Release dates
  [not verified: not on the fetched pages].

## 4. LibreOffice Calc

**Evidence.** help.libreoffice.org "Calculate" options and error codes;
OpenOffice wiki on formula cell dependence; `sc/inc/recursionhelper.hxx` and
`sc/source/core/data/formulacell.cxx` from the GitHub mirror of core/master.
[verified: sources 7-11]

- Dependence tracking is broadcaster/listener: "the formula cell tries to
  listen the cells which is referenced by it directly when it is inserting in
  the column" (`StartListeningTo`); ranges use `ScBroadcastArea`; on notify
  "it set itself dirty and put itself into a FormulaTrackList"; values are
  computed when requested ("Paint", "Saving"). (source 11) Listeners are
  re-registered when a formula is replaced, which is how the graph is dynamic.
- Cycle detection is a running flag: in `ScFormulaCell::Interpret`,
  `if (bRunning) { if (!rDocument.GetDocOptions().IsIter()) {
  aResult.SetResultError( FormulaError::CircularReference ); return ... } ... }`
  (formulacell.cxx:1815-1831); `MAXRECURSION = 400` (line 231) bounds the
  recursion, above which the cell is treated as running and a "recursion
  return" unwinds. (source 10)
- Iteration: `ScRecursionHelper` keeps `nIteration`, `bConverging`, an
  iteration list and `StartIteration/ResumeIteration/EndIteration`
  (recursionhelper.hxx:45-108). On non-convergence: "If one cell didn't
  converge, all cells of this circular dependency don't"; every member gets
  `FormulaError::NoConvergence` (formulacell.cxx:2050-2062). (sources 9, 10)
- User-facing: Err:522 "Formula refers directly or indirectly to itself and
  the Iterations option is not set"; Err:523 "iterative references do not
  reach the minimum change within the maximum steps that are set"; options
  Iterations / Steps / Minimum change; "If the Iterations box is not marked,
  an iterative reference in the table will cause an error message" and a
  "Circular reference" status-bar message. (sources 7, 8)

## 5. Google Sheets (limited)

**Evidence.** Sheets API v4 `IterativeCalculationSettings`: `maxIterations`
"the maximum number of calculation rounds to perform"; `convergenceThreshold`
"When ... successive results differ by less than this threshold value, the
calculation rounds stop"; on `SpreadsheetProperties`: "Absence of this field
means that circular references result in calculation errors." [verified:
source 12] The user help page on calculation settings (source 13) covers only
Automatic/Manual and locale; the "#REF! Circular dependency detected" cell
text is reported by third-party pages only [not verified: primary page not
found].

## 6. IronCalc (Rust)

**Evidence.** `base/src/evaluation.rs` and `base/src/model.rs` on `main`
(base crate version 0.8.3 per Cargo.toml), PR #1420, README. [verified:
sources 14-17]

- No dependency graph, no dirty set: `pub fn evaluate(&mut self)` "Evaluates
  every formula in the workbook. Runs passes until one completes without a
  restart"; each pass clears `state.cells` and visits "every cell of every
  sheet" (evaluation.rs:1-8, 481-500, 660). Full recalculation, like CoreCalc.
- Demand-driven recursion with a mark: `enum CellState { Evaluating,
  Evaluated }` (line 125); in `evaluate_formula_cell`: `Some(CellState::
  Evaluating) => { self.mark_cycle(cell_reference); return
  circular_reference(cell_reference); }` (lines 790-792); `#CIRC!` is
  `Error::CIRC` "Circular reference detected" (line 1108; model.rs:825-831).
- Whole loop marked: `mark_cycle` walks the evaluation stack from the
  re-entered cell and inserts every member into `circular`; "They all store
  `#CIRC!`, whatever their formula would have made of the error, so that the
  verdict does not depend on where the recursion entered the loop"
  (lines 960-972).
- Deep chains: recursion is bounded "in formulas and in stack"; it unwinds,
  leaves cells `Evaluating` on an explicit stack, and replays "the deepest
  cell first ... about one extra run per cell instead of overflowing the
  stack" (header lines 28-33; `too_deep`, `replay`, lines 829-869). PR #1420
  cites ~700-level chains overflowing before this.
- Dynamic arrays (spills) make the write set dynamic too; contradictions
  restart the pass with a learned ordering fact ("at most n² of them"); anchor
  order "survives across evaluations" (header lines 9-20, 35-37).
- Volatile functions: nothing special is needed because everything is
  re-evaluated each call [verified: no `volatile` identifier in model.rs or
  evaluation.rs; whether RAND/NOW are otherwise handled: not verified].
- README describes goals only; no design documentation of evaluation.

## 7. Encoding a dynamic-graph evaluator in a static dataflow language

**Elm (7GUIs Cells).** The 7GUIs task requires change propagation until "no
more changes in the values of any cell" and "not just recompute the value of
every cell but only of those cells that depend on another cell's changed
value". [verified: source 22] joakin/elm-7guis does it with **general
recursion**: `evaluate get expr` calls `evaluateCell get` which calls
`evaluate` again (Cell.elm:363-411), and `propagateChanges` recursively
refreshes dependents (Main.elm:181-201). No cycle detection appears in the
three files read, so a cycle would recurse without bound. [verified: source
23] Elm has recursion; the encoding is trivial there and says nothing about a
recursion-free language.

**Kahn's algorithm (Atlas).** "everything downstream gets recalculated in
topological order. Any cells left over after that sort are the ones in a
cycle, and they show `#CYCLE!`. It doesn't guess with a recursion limit."
[verified: source 24] Kahn needs a worklist, i.e. a fold or a loop.

**Lustre (synchronous static dataflow).** The compiler checks "Absence of
recursive node call: in view of obtaining automata-like executable programs,
LUSTRE allows up to now only static networks" and "Absence of cyclic
definitions: any cycle in the network should contain at least one pre
operator" (a delay-free loop "can be interpreted as a deadlock. It is
therefore rejected"); it also rejects structural cycles that are not true
deadlocks "because the analysis of such networks is undecidable" (§IV.A).
[verified: source 21] This is D9 in synchronous-language terms: HOLD's
committed read is `pre`. A Lustre-class language cannot host a spreadsheet
interpreter without an iterator extension over statically sized arrays
(later Lustre V6 `map`/`fold` iterators) [not verified: V6 docs not fetched].

**Excel in Excel.** Before REDUCE/LAMBDA, Excel's own formula language could
not evaluate a formula graph stored in cells except through iterative
calculation (a bounded per-recalc fixed point). With them it can (bounded fold
or recursion with `#NUM!` on overflow). Sources 5, 6. So the industry's own
answer to "what minimal primitive" is: a bounded fold, or recursion with a
depth error, or the engine's iteration bound.

**Conclusion.** Evaluating a runtime-discovered graph in dependency order
needs one of: (a) recursion + a "being computed" mark (Sestoft, IronCalc,
Calc, Elm); (b) a worklist/fold (Kahn, REDUCE); (c) a bounded per-tick fixed
point (Excel/Calc/Sheets iterative calculation); (d) an engine builtin that
does (a) for you (today's `catch_cycle` + lazy rows). A static acyclic
dataflow with none of these can only express K-bounded unrolling.

## 8. Answers to the three questions

### (1) Can dependency-ordered recalculation be written in plain Boon under D9 and "no recursion"?

No, not in dependency order. Two things are expressible.

**Sketch A: depth-bounded unrolling (full recalculation from formula texts).**
Rows read other rows' *HOLD state* (`formula_text`), never a derived field,
so no derived-field cycle exists; the cross-row read is a committed-value read
(legal by D9 if Phase A recognises it, see implication 5).

```boon
FUNCTION lookup_text(address) {
    cells |> List/find(item, if: item.address == address) |> WHEN {
        Found[value] => value.formula_text          -- committed HOLD read
        NotFound => FLUSH { MissingReference[address: address] }
    }
}
FUNCTION eval0(text, path) { literal_only(text: text) }           -- refs => TooDeep
FUNCTION ref1(address, path) {
    path |> List/any(item, if: item == address) |> WHILE {
        True => CycleError                                       -- visited mark
        False => eval0(text: lookup_text(address: address)
                       path: path |> List/append(item: address))
    }
}
FUNCTION eval1(text, path) { parse(text) |> WHEN { Ref[a] => ref1(address: a, path: path)  … } }
-- ref2/eval2 … refK/evalK: K copies; the row uses evalK(text: formula_text, path: LIST { address })
```

Properties: exact cycle detection up to depth K (the path is the visited set);
chains deeper than K are an error; K copies of a ~330-line evaluator; cost per
edit is every formula cell re-evaluated as a *tree* (no memo: a diamond DAG
is exponential in K); under D32 every row's `result` fires on any commit,
so cells.scn `expect_recomputed = ["C1"]` fails (all 2,600 recompute).

**Sketch B: per-tick relaxation (Jacobi passes over a collection authority).**
Cycle closed through `List/replace_all` (D9-legal, D17), state in a collection
because LIST may not be HOLD state (D9).

```boon
store: [
    values: LIST {} |> List/replace_all(with: next_pass)      -- authority
    pass: 0 |> HOLD p { LATEST { any_commit |> THEN { 0 }, values |> THEN { p + 1 } } }
    next_pass:
        pass < 64 |> WHILE {                                  -- static bound (Excel: 100)
            True => cells |> List/map(item, new: eval_one(text: item.formula_text, read: values))
            False => SKIP                                     -- stop: no update fires
        }
    -- a cell whose value differs between pass 63 and 64 is on or below a cycle
]
```

Properties: no recursion, no fold; needs the previous pass too (a second
authority) to mark cycles; 64 × 2,600 evaluations and 64 `replace_all`s per
edit, each a microstep (R8) that re-renders under R-DOC; without the SKIP
guard, D32 makes `values → next_pass → values` fire forever. It is Excel's
iterative calculation with threshold 0, which Excel keeps off by default.

### (2) Smallest engine feature that makes it clean

| option | pros | cons |
| --- | --- | --- |
| (i) Principled lazy-row builtin: rows may read other rows' derived fields; the executor evaluates on demand with the existing `Evaluating` mark and yields a typed `Cycle` value (rename/spec `catch_cycle`, e.g. a `WHEN { Cycle => … }` arm on the read) | O(N) minimal recalc today; what every engine does; marks all members if `mark_cycle`-style stack walk is added; no language change; keeps cells.scn `expect_recomputed` | contradicts D9's "duct tape" and R6; needs a spec for what is legal (only through the builtin) and Phase A must model it as a cut edge |
| (ii) Per-tick fixed point with a static bound (Sketch B) plus R8 microstep budget and a non-convergence diagnostic | already needed for D9 collection cycles under D32 (implication 3); mirrors Excel/Calc/Sheets | O(depth × N) per edit; every pass re-renders; cycle detection is indirect (non-convergence), not a path |
| (iii) `List/fold` (bounded, static list) | Kahn's algorithm and IronCalc-style passes become library code; one builtin | foundations plan says "Do not add ... List/fold"; still full recalc unless the visited set is threaded |
| (iv) Recursion with a static depth bound | Sketch A without K copies; Sestoft's algorithm verbatim | §4.3 "There is no recursion, so the call graph is a DAG"; schemes need a fixpoint; inline-equivalence oracle breaks |

Recommendation for the review: (i) or (ii); (i) is the only one that keeps
"dependents only" recomputation.

### (3) What spreadsheets do with a dynamic graph and with equal-value rewrites

- Formula edit changes dependencies: Excel "reconstructs the dependency tree
  and calculation chain" on a new formula and moves misordered cells "down the
  chain" during recalc (source 1); Calc re-registers listeners on insert
  (source 11); IronCalc has no graph and learns anchor order from restarts
  (source 14); CoreCalc rejects incremental topological maintenance as fragile
  (§3.3.3). Non-strict IF and INDIRECT/OFFSET mean even the static graph is
  an approximation (Sestoft §1.6; volatile list, source 3).
- Equal-value rewrites: dirtiness is structural. Excel: dependents recalculate
  "even if the value of the previously calculated cell does not change"
  (source 3). CoreCalc minimal recalc: the support-graph closure, no value
  test (§3.3.2). Calc: `SetDirty` on notify (source 11). Value equality
  appears only as the **iteration stop criterion** (0.001 / Minimum change /
  convergenceThreshold), i.e. exactly where cycles are tolerated.

## Implications for the plan

1. **§5.1 Cells row, D16 (contradicts).** "Recalculation follows dependency
   order. A visited/depth guard marks circular references" is not writable in
   plain Boon under §4.3 "no recursion", D9 and the foundations rule "Do not
   add List/fold" (§7, §8.1). Proposed change: add a P0 spike "Cells in plain
   Boon" that must compile and pass the cycle steps of cells.scn before P3a
   sizes the row; the row's fix text should name the chosen option from §8.2.
2. **D9 / R6 (suggests change).** `Dependency/catch_cycle` is not duct tape
   but the engine hosting the algorithm of CoreCalc (§2.11), IronCalc
   (`CellState::Evaluating`) and Calc (`bRunning`). Proposed: the P0 options
   note (D9's "HOLD alternative") adds option (i): a specified lazy-row read
   whose result type includes a `Cycle` variant, modelled in Phase A as a cut
   edge; delete the *name* `catch_cycle`, keep the mechanism.
3. **D9 + D32 + R8 (suggests change).** A cycle legal through a collection
   update, with every write firing, is an unbounded microstep loop; Sestoft
   §3.3.1 names this failure for listener designs. Every engine bounds it
   (Excel 100 iterations, Calc Steps + Err:523, Sheets `maxIterations`).
   Proposed: R8 adds a per-tick microstep budget and a positioned runtime
   diagnostic ("did not settle after N microsteps: cycle through
   `store.values`"); the checker warns when a collection-update cycle has no
   HOLD/SKIP guard. Spike: encode Sketch B and prove termination.
4. **D32 (supports).** Structural dirty propagation without value tests is
   Excel's rule (source 3) and CoreCalc's (§3.3.2). Keep D32; but §4.3's
   "at most once per step" must mean once per *tick* for derived values, or
   a 2,600-row live `List/map` recomputes once per microstep in Sketch B.
5. **D9 row-level cycles, §4.3 "Cycles" (suggests spike).** Sketch A is legal
   only if Phase A treats a row reading another row's HOLD through
   `List/find` as a HOLD-cut edge rather than a `cells → cells` list cycle.
   §4.3 says cycles are found "on instances in lowering Phase A" at
   "root-field-path granularity"; whether `cells.formula_text` and
   `cells.result` are distinct nodes decides Cells. Proposed: add fixture
   `cells_rows_read_other_rows_hold.bn` (accept) and
   `cells_rows_read_other_rows_derived.bn` (reject with path) to the P0 census.
6. **§4.6 R-DOC gate, §5.2 (contradicts as written).** Full recalculation
   without memoization (Sketch A) or 64 passes (Sketch B) plus D32 re-renders
   every cell per edit; cells.scn `expect_recomputed = ["C1"]`,
   `["A1","C1","D1"]` encode smart recalculation and fail under both. Proposed:
   the owner either accepts dropping `expect_recomputed` from cells.scn (D16
   changes observable behaviour) or picks option (i), which keeps them.
7. **D30 copy/live (supports, with a note).** A cell's `result` must be live
   over other cells' formula texts (a WHEN would freeze it); cells/view.bn
   `display_text: editing |> WHEN {…}` is one of the "WHEN that must follow
   live updates" sites of §5.1 and becomes WHILE. Consistent with the plan;
   list Cells explicitly in that census row.
8. **D16 cycle value (supports).** IronCalc `mark_cycle` and Calc's
   `NoConvergence` mark every loop member; cells.scn expects A1 and B1 both to
   show `cycle_error`. Whatever primitive is chosen must be able to mark all
   members: Sestoft's plain visited mark reports only the re-entered cell, so
   option (i) needs IronCalc's stack walk, and Sketch A needs the path.
9. **D9 "LIST may not be HOLD state" (supports, with cost).** Sketch B stores
   the value vector in a `List/replace_all` authority as D17 intends; note the
   R-DOC identity reconciliation cost of 2,600-row `replace_all` per pass in
   the S3 spike.
10. **D31 volatile analogy (supports).** Excel's volatile functions
    (NOW/RAND/INDIRECT) are "recalculated at each recalculation"; Boon's
    query/command split gives NOW-like values (Clock/wall) command status in
    copy contexts, which is stricter and avoids Sestoft's "volatile functions
    complicate ... recalculation order" problem. No change proposed.

## Sources

1. Excel Recalculation, learn.microsoft.com/en-us/office/client-developer/excel/excel-recalculation — fetched 2026-09-29, full text read.
2. Multithreaded recalculation in Excel, learn.microsoft.com/en-us/office/client-developer/excel/multithreaded-recalculation-in-excel — fetched, full text read.
3. Excel performance: Improving calculation performance, learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-improving-calculation-performance — fetched, full text read.
4. Remove or allow a circular reference, support.microsoft.com/en-us/office/remove-or-allow-a-circular-reference-8540bd0f-6e97-4483-bcf7-1b49cd50d123 — fetched, quoted via summary.
5. LAMBDA function, support.microsoft.com/en-us/office/lambda-function-bd212d27-1cd1-4321-a34a-ccbf254b8b67 — fetched, quoted via summary.
6. REDUCE function, support.microsoft.com/en-gb/excel/functions/reduce-function — fetched, quoted via summary.
7. LibreOffice Help "Calculate" options, help.libreoffice.org/latest/en-US/text/shared/optionen/01060500.html — fetched, quoted via summary.
8. LibreOffice Help "Error Codes in Calc", help.libreoffice.org/latest/en-US/text/scalc/05/02140000.html — fetched, quoted via summary.
9. LibreOffice core `sc/inc/recursionhelper.hxx` (GitHub mirror, master), raw.githubusercontent.com/LibreOffice/core/master/sc/inc/recursionhelper.hxx — fetched with curl, grep-read.
10. LibreOffice core `sc/source/core/data/formulacell.cxx` (master), raw.githubusercontent.com/LibreOffice/core/master/sc/source/core/data/formulacell.cxx — fetched with curl, lines 231, 1808-1848, 2050-2066 read.
11. OpenOffice wiki "Calc/Implementation/Formula cell and cells dependence", wiki.openoffice.org/wiki/Calc/Implementation/Formula_cell_and_cells_dependence — fetched (the wiki.services host failed TLS), quoted via summary.
12. Google Sheets API v4 `IterativeCalculationSettings`, developers.google.com/sheets/api/reference/rest/v4/spreadsheets#IterativeCalculationSettings — fetched, quoted via summary.
13. Google Docs help 58515 "Set a spreadsheet's location and calculation settings", support.google.com/docs/answer/58515 — fetched; contains no iterative-calculation details.
14. IronCalc `base/src/evaluation.rs` (main), raw.githubusercontent.com/ironcalc/IronCalc/main/base/src/evaluation.rs — fetched with curl, lines 1-40, 125-127, 478-500, 783-812, 829-869, 960-985, 1108 read.
15. IronCalc `base/src/model.rs` (main), raw.githubusercontent.com/ironcalc/IronCalc/main/base/src/model.rs — fetched with curl, grep-read (lines 825-831); `base/Cargo.toml` version 0.8.3.
16. IronCalc PR #1420 "Fixes to the general evaluation algorithm", github.com/ironcalc/IronCalc/pull/1420 — fetched, summary only.
17. IronCalc README, github.com/ironcalc/IronCalc — fetched; no evaluation design documented.
18. Sestoft, "A Spreadsheet Core Implementation in C#", ITU-TR-2006-91, raspi.itu.dk/people/sestoft/corecalc/ITU-TR-2006-91.pdf — fetched, pdftotext, sections 1.6-1.9, 2.11-2.13.3, 3.3-3.3.5 read. (The 2014 MIT Press book was not fetched; the report is its basis.)
19. Bock, "A Literature Review of Spreadsheet Technology", ITU-TR-2016-199, researcher.itu.dk/ws/files/81449279/ITU_TR_2016_199.pdf — fetched after redirect, pdftotext, pp. 3-4 read.
20. Bock, Biermann et al., "Parallel spreadsheet evaluation and dynamic cycle detection", onlinelibrary.wiley.com/doi/abs/10.1002/cpe.6218 — HTTP 403, not fetched.
21. Halbwachs, Caspi, Raymond, Pilaud, "The synchronous data flow programming language LUSTRE", Proc. IEEE 79(9), 1991, homepage.cs.uiowa.edu/~tinelli/classes/181/Fall14/Papers/Halb91.pdf — fetched, pdftotext, §IV.A read.
22. 7GUIs tasks (Cells), eugenkiss.github.io/7guis/tasks/ — fetched, quoted via summary.
23. joakin/elm-7guis `src/Tasks/Cells/{Dependencies,Main,Cell}.elm`, raw.githubusercontent.com/joakin/elm-7guis/master/... — fetched with curl, Main.elm:170-233 and Cell.elm:360-420 read.
24. bxzex/atlas README, github.com/bxzex/atlas — fetched, quoted via summary.
25. Local: examples/cells/{model,defaults,store,columns,cell,formula,view}.bn, examples/cells.scn, crates/boon_plan_executor/src/machine.rs (lines 1988-1992, 20384-20396, 21206-21222, 25984-26003, 32185-32300), docs/plans/BOON_COMPILER_REWRITE_PLAN.md (§1 D9/D16/D17/D26/D30-D34, §4.3, §4.5 R6/R8, §4.6, §5.1, §11), docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md:1376-1398 ("Bounded Repetition Without Loop APIs") — read.
