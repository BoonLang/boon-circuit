# Lens 4: migration and execution: all findings

Generated from the verified finder output for `../REVIEW.md`. Severity is the severity after refutation; each finding keeps its refuter's corrections.

### MIG-001 [high] D16 Cells cannot be written as the plan's row describes; the smallest fix is a pure List/fold (or accepting K unrolled passes)

*Decisions: D9, D16, D21, D32, D33; Plan: §5.1 Cells row (line 750); §4.3 lines 367, 384-387, 405-407, 418-421; D9, D16, D21, D33; R6; Sources: M2; verdict: confirmed; severity critical -> high after refutation*

The §5.1 row says computed values live in HOLD or collection state updated on edit events, recalculation follows dependency order, and a visited/depth guard marks cycles. Under the plan's own rules none of that can be written. (1) A per-cell HOLD updated on the commit event reads the committed snapshot of other cells (D21). So after A1 changes, B1 = A1+1 lags one edit behind. (2) A second pass would need a re-trigger, but D33 says a HOLD's own writes do not re-trigger it. §4.3:405-407 makes 'a THEN input, WHEN selector or update candidate on the cycle' an error. So a self-driven multi-tick relaxation is illegal. That includes the notes' Sketch B, which re-triggers on `values |> THEN`. (3) Evaluating in one tick in dependency order needs unbounded iteration. §4.3:367 forbids recursion, the catalog has no fold (List/* builtins: map, filter, find, sum, range, get, chunk, latest, ...) and BOON_LANGUAGE_FOUNDATIONS_PLAN.md:1379-1385 says 'Do not add List/fold'. The only design that is expressible and legal is K unrolled Jacobi passes in one tick, in which a cell still Pending after K passes is shown as the cycle error. I wrote and ran that design on today's runtime: 4 passes resolve a chain of depth 3 and mark a 2-cell cycle, with no recursion and no HOLD. Its costs: a chain deeper than K is misreported as a cycle (Excel has no such limit), the evaluator is instantiated K times, and every edit costs K×N work. With a pure `List/fold` (documented in the original Boon, LIST.md:160-170 and 330-377), the pass count can be the number of formula cells, which is exact. For comparison, the original Boon's own Cells uses mutual recursion (compute_value → expression_value → compute_value, cells.bn:79-80, 93, 103) and a LIST inside HOLD (cells.bn:389). The plan forbids both.

Evidence:
- [file, verified] `examples/cells/formula.bn:38-49` — cell_result reads another row's derived `result` through `cells |> List/find`, wrapped in Dependency/catch_cycle
- [file, verified] `crates/boon_plan_executor/src/machine.rs:25984-26003` — CatchCycle pushes a catch frame around demand (pull) evaluation. Today's dependent recalculation is lazy row evaluation hosted by the engine
- [measurement, verified] `cd <scratch>/M2 && target/release/boon_cli run fixpoint_probe.bn --scenario fixpoint_probe.scn -> 'pass: 1 turn(s)'; negative control r2=99 -> 'root `r2` expected `99`, got `17`'` — The unrolled 4-pass evaluator resolves A2=A1+A3 with A3=A4+A1 (depth 3) to 17 and leaves the A5/A6 cycle Pending (shown as -1)
- [measurement, verified] `rg -o 'List/[a-z_]+' crates/boon_typecheck/src | sort | uniq -c` — No List/fold, List/reduce, List/iterate or List/chain in today's catalog
- [file, verified] `docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md:1379-1385` — 'Do not add: Loop/*; List/fold; List/scan; List/reduce'
- [file, verified] `~/repos/boon/docs/language/LIST.md:160-170,330-377,720` — The original Boon documents List/fold and List/chain (a runtime fold in software, an unrolled chain in hardware)
- [file, verified] `~/repos/boon/playground/frontend/src/examples/cells/cells.bn:79-80,93,103,389` — The original Cells uses recursion and `overrides: LIST {} |> HOLD state`. Both are illegal under §4.3 and D9
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:405-407` — The fire-edge rule excludes any self-re-triggering relaxation
- (1 more evidence entries in the finder output)

```boon
-- expressible today and legal under the plan: K unrolled passes (verified probe shape)
p0: formulas |> List/map(item, new: Pending)
p1: relax(prev: p0)
-- ... p2 .. pK, one line per pass
FUNCTION relax(prev) { formulas |> List/map(item, new: eval_node(node: item, prev: prev)) }
FUNCTION read(prev, index) {
    List/get(list: prev, position: index) |> WHEN { Found[value] => value, NotFound => Pending }
}
```

```boon
-- with the proposed pure List/fold: exact, one evaluator instance
sheet_values: BLOCK {
    parsed: cells |> List/map(item, new: item.parsed)
    start: parsed |> List/map(item, new: Pending)
    List/range(from: 1, to: parsed |> List/count())
    |> List/fold(init: start, __, acc: relax(parsed: parsed, prev: acc))
    |> List/map(item, new: item |> WHILE { Pending => CycleError, __ => item })
}
-- needs: List/fold with a LIST accumulator, and a `__` binder for the unused element (D24)
```

**Proposed plan change.** Replace the §5.1 Cells row text with the design and its required feature. Cells becomes 'parse each formula_text into a tagged node (Lit/Ref[position]/Binary/Sum/ParseError); sheet_values = fixed point over the parsed list; a cell still Pending after the last pass is CycleError; each row's value reads sheet_values by position'. Name the iteration mechanism the owner picks, and remove 'HOLD/collection state updated on edit events' and 'visited/depth guard'. Add a P0 spike 'Cells in plain Boon' before P3b is sized. Acceptance: cells.scn passes (except the recompute assertions, see the next finding), and formula_edit_input_to_idle p95 stays within cells.budget.toml (3.0 ms) on 2,600 rows. Size: L (about 2 weeks) only with List/fold and the scenario change; XL and uncertain with K unrolling (K evaluator instances, a wrong answer for chains deeper than K, K×2,600 evaluations per edit).

**Refuter correction (high tier, medium confidence).** Keep the core point: plan:750 describes no expressible mechanism, so Cells needs an owner-chosen iteration mechanism and a P0 spike before it is sized L. Remove 'the only legal design is K unrolling'. Add option (a0): counted iteration with Stream/pulses driving a collection-authority update (pulse count = number of formula cells, exact, no new builtin, consistent with the foundations plan). The spike must answer: pulses within one tick vs across frames (hide intermediate frames); legality under the D9 graph granularity (MIG-002); the F x F cost against cells.budget.toml 3.0 ms. Offer List/fold only as the fallback if pulses cannot run in one tick. Lower severity from critical to high, because an existing primitive plausibly covers it.

**Owner question.** How may plain-Boon Cells iterate to a fixed point within one tick, now that recursion, self-re-triggering cycles and Dependency/catch_cycle are all gone?
- (a) Add a pure `List/fold(init:, item, acc:)` (plus a `__` element binder) to the catalog, as the original Boon's LIST.md documents. Pros: exact results; one evaluator instance; no state, no cycles, no persistence impact; lowers to an N-stage chain for fixed-size lists (hardware story unchanged). Cons: overrides the foundations plan's 'Do not add List/fold'; adds one catalog builtin plus typing and lowering work (S-M).
- (b) No new feature: K unrolled passes (e.g. K=26). Pros: works under the rules as written (verified probe). Cons: chains deeper than K show cycle_error, which is not Excel behaviour; K instances of the evaluator in the plan; K×N work per edit against a 3 ms budget.
- (c) Relax D9 for collections: a derived read of a row field through a lookup on the same collection (List/find or List/get over `cells`, reading `.result`) may close a row-level cycle. The runtime evaluates it on demand and yields a typed `Cycle` variant at the lookup (`Found[value] | NotFound | Cycle`). Phase A treats the lookup edge as cut. Pros: O(dependents) recalculation; keeps expect_recomputed and the budget; this is today's mechanism. Cons: it is language support for cycles, which contradicts D16 and D9's 'duct tape' ruling.
- (d) Recursion with a static depth bound. Pros: textbook algorithm. Cons: breaks §4.3's call-graph DAG and scheme checking.
Recommendation: (a). It is the smallest change that keeps D16's 'no language support for cycles' and gives Excel-exact results. Use (c) only if the spike shows the full-recalculation cost cannot meet the Cells latency budget.

---

### MIG-002 [high] Cycle-graph granularity for collection element fields is unspecified, and it decides whether any Cells design type-checks

*Decisions: D9, D16; Plan: §4.3 'Cycles (D9)' lines 416-424; §4.1 line 228; Sources: M2; verdict: confirmed*

Every legal Cells design has row fields that read a sheet-level value derived from other row fields of the same list: row.value ← sheet_values ← `cells |> List/map(item, new: item.parsed)` ← row.formula_text. The harness forces row.value to stay a field of `cells` (lib.rs:2200-2216 reads cells[address].value and display_text). If §4.3's 'root-field-path granularity' treats `cells` as one node, this is the cycle cells → sheet_values → cells with no HOLD-update edge, and it is rejected. If element fields are separate nodes (`cells[*].value`, `cells[*].formula_text`, plus a membership node `cells[#]`), it is acyclic. The same choice decides whether today's TodoMVC all_completed/toggle_all pattern needs the Bool/toggle exemption. Today's direct read of another row's derived `result` (formula.bn:38-49) is a self-loop on cells[*].result at any granularity, so rejecting it is correct.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:416-424` — 'one labelled dependency graph, at root-field-path granularity, including local nodes and state cells'. Element fields are not mentioned
- [file, verified] `crates/boon_runtime/src/lib.rs:2200-2216`
- [file, verified] `docs/plans/compiler_rewrite_notes/review/research/spreadsheet_recalculation_cycles.md implication 5` — taken-from-notes: raises the same granularity question for Sketch A. The evaluator shape and the harness constraint are my additions

**Proposed plan change.** Specify in §4.3: 'Collection element fields are graph nodes `L[*].f`. A list operation that reads `item.f` adds an edge from `L[*].f` and from the membership node `L[#]`. `L[#]` depends only on the list's construction and its event-driven updates.' Add fixtures: cells_evaluator_reads_sheet_values.bn (accept), cells_row_reads_other_row_derived_result.bn (reject, with the path), and cells_rows_read_other_rows_hold.bn (accept).

**Refuter correction (high tier, medium confidence).** Wording: the gap is that 'root-field-path' is undefined for collection element fields, not that the plan picks the wrong granularity. Proposed change is fine as written: define L[*].f and a membership node L[#] as graph nodes in §4.3, and add the three fixtures. Link it to the pulse or fold trigger question in MIG-001, since the pulse count must come from L[#] or formula_text, not from values.

---

### MIG-020 [high] P7 is a big-bang switch, not 'days': the old-engine compatibility rule pushes every next-only migration of 4 of the 5 native-gate examples, plus ~9 consumer ports, into one change set

*Decisions: D1, D6, D17, D30, D31, D35, D36; Plan: 5.1 Old-engine compatibility rule (plan:766-770); P3a (plan:960-973, esp. :969); 4.8 Porting other consumers (plan:698-701); P7 (plan:1043-1049); Sources: M3; verdict: confirmed; severity critical -> high after refutation*

Three rules taken together leave only one legal path for a migration the old engine cannot run: 'lands with the P7 switch'. The rules are the compat rule (plan:766), the new AGENTS block's 'change the old pipeline only to keep it building / feed the oracle / delete it' (plan:1086-1090), and the AGENTS.md:16-19 'no Boon workaround' rule, which stays. Migrations that CAN land early, because the old engine accepts them and they keep today's behaviour: the Theme refactor (the draft checks on the old compiler, but only with a Light/* workaround, examples.md:483), binder patterns (the syntax exists today, e.g. examples/fibonacci.bn:83 `Found[value] => value`), one-input LATEST removal, LATEST->HOLD, WHEN->WHILE conversions (behaviour-preserving: today WHEN is already live, see the probe), suffix qualification, parentheses, unused parameters, exact records. Migrations that CANNOT land early: List/replace_all (R5), Http/get and Http/send (R9), the host `focused` port and 'restored' occurrence (R9), removing request_fingerprint (needs R9 supersession), L5 List/latest, the Cells redesign (`boon_cli check examples/cells.bn` already fails on the old engine), and every scenario step that asserts D30/D32/D33 behaviour. Examples: a WHEN kept to copy on purpose, like title_to_add, or a THEN over a HOLD that is written the same value twice. By site count most of the migration lands early. By example, NovyWave's effect layer (748 WHEN / 750 THEN), FjordPulse's Http, TodoMVC's focus and Cells all land at P7, which is also where §4.8 ports editor, program_runtime, app_package, host_runtime, phase0, the behaviour harness, xtask, the web host and boon_runtime. There is no CI (plan:847), so this one change set has to pass 7 native gates plus verify-all by hand. P3a even lists 'List/replace_all rewrites' (plan:969), which the compat rule forbids.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:766-770` — compat rule; 'A change the old engine cannot handle lands with the P7 switch'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:698-701` — all other consumers ported in the P7 switch change set
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:969` — P3a lists List/replace_all rewrites under the compat rule
- [measurement, verified] `rg -n 'replace_all|"focused"|Http/get' crates/boon_plan_executor/src crates/boon_effect_schema/src; rg -c -g '*.bn' 'List/replace_all|Http/get|Http/send' examples crates` — no hits: none of these exist in today's runtime, catalog or examples
- [measurement, verified] `scratchpad M3/live.bn + live.scn; target/release/boon_cli run live.bn --scenario live.scn` — after go then bump: via_when=1 and via_while=1, so today's WHEN is live and WHEN->WHILE keeps old-engine behaviour; under D30 via_when stays 0
- [measurement, verified] `git ls-files examples/novywave | grep .bn | xargs grep -o -w WHEN|THEN | wc -l` — NovyWave WHEN=748, THEN=750; corpus WHEN=1422, WHILE=47
- [file, from notes] `docs/plans/compiler_rewrite_notes/review/measurements/oracle.md (cells row)` — cells fails check: compact ABI slice lacks Dependency/catch_cycle, List/range, Text/find
- [file, from notes] `docs/plans/compiler_rewrite_notes/examples.md:483,504,510` — Theme draft keeps Light/* out of tokens only because the old backend cannot lower it; critique recommended dropping old-compiler acceptance

```boon
FUNCTION pick(m, n) { m |> WHEN { Off => 0  On => n } }  -- mode goes to On, then n updates: today 1 (live), D30 0 (copied at the mode update); the WHILE form gives 1 on both engines
```

**Proposed plan change.** Replace the compat rule (plan:766-770) with per-example engine flips: "Each manifest example carries `engine = "old" | "next"` (default old). The verifier passes it to the compile service and stamps it into the report, and verify-all checks each report against its example's declared engine. An example flips to next in its own change set once its native gate and O2 scenarios pass on next. The compatibility rule applies only to examples still on old. Migrations the old engine cannot run land in that example's flip change set, never as Boon workarounds. P7 is the flip of the last example plus removal of the default." This needs the engine selector and the ports of host_runtime (MigrationScenarioRunner), boon_runtime, the behaviour harness and phase0 by P4, not P7 (see the dependency finding). Delete 'List/replace_all rewrites' from P3a. Resize P7 to 1-3 weeks if the owner keeps the big bang.

**Refuter correction (high tier, high confidence).** Lower severity to high. Lead with the P3b-exit / P5-exit vs compat-rule conflict (plan:766-770 vs P3b exit vs P5 exit) and the vacuous Cells case, not the site counts. Option (a) must also amend plan:689-690 ('verify-all refuses reports from a non-default engine'). It must also say that a per-example flip before P6 exposes that example's performance gates to an engine that has not been hardened yet. So gate the flip on the P5 compile service and on per-example performance, or accept that the gate stays report-only until P6. A cheaper alternative to offer: (d) keep one default, but host the next-only migrated sources as the next revision of each example (for example a per-example next source path in the manifest), which O2 and P5 run against, with P7 swapping the paths. Keep the P7 resize (1-3 weeks) as the fallback.

**Owner question.** How should examples whose migration the old engine cannot run reach main before cutover?
- (a) Per-example engine flip in examples/manifest.toml; P7 flips the last one
- (b) Keep the big bang: all next-only migrations and consumer ports land together at P7 (resize P7 to 1-3 weeks)
- (c) A long-lived migration branch rebased until P7
Recommendation: (a): it removes the single high-risk change set, and it lets each native gate go green on next as evidence accumulates. 'No code path may depend on an example's name' still holds, because the engine is manifest data.

---

### MIG-025 [high] O1 has no author, size or independence rule, and the notes' spec it would draw on predates D14-D36 and contradicts D9, D10 and D15

*Plan: 7 O1 (plan:836); P0 semantics; P2a/P2b exits ('O1 green'/'O1 fully green'); Sources: M3; verdict: confirmed; severity medium -> high after refutation*

O1 expectations are 'written from the spec'. The spec is written in P0, and O1 must be fully green at P2b exit, but no phase owns writing the fixtures and the effort table does not size them. The notes' spec.md is framed against D1-D13 and never mentions D30-D36 or copy contexts. It still refines 'the matched stable path inside an arm' (conflicts with D10), lists cycles through Dependency/catch_cycle (conflicts with D9) and 'stateful LATEST' (conflicts with D15). Its catalog has 77 codes before any change-model or effect diagnostics are added. At a positive plus a negative fixture per rule, that is at least 160-200 files. If the checker's author also writes the fixtures, O1 turns into a blessing of the checker's own output, which is the old byte-hash failure in a new form.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:1-13` — 'disagrees with the plan's decision table (D1-D13)'; refinement of matched path; catch_cycle cycles; stateful LATEST
- [measurement, verified] `grep -n -i 'D30\|D31\|D35\|WHILE is live\|copy context' docs/plans/compiler_rewrite_notes/spec.md` — no hits
- [measurement, verified] `sed -n 719,870p docs/plans/compiler_rewrite_notes/spec.md | grep -c -E '^\| *`?[A-Z]+[0-9]+'` — 77 code rows
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:836` — 'a negative and a positive fixture per rule'

**Proposed plan change.** Add to P0: "Write static semantics v2 from D1-D36 and §4.3, not from notes/spec.md; give every rule an id." Add to P1b (with the effort-table line 'O1 authoring 1.5-2 engineer-weeks'): "O1 fixtures are written from the spec by an agent that does not write boonc_check. Every `.expect` line cites a rule id. The runner has no bless mode, and a changed `.expect` must cite the rule or decision that changed. The owner reviews fixtures in batches of families before P2a exit."

**Refuter correction (low tier, high confidence).** The bundled 'no author/size/independence rule' framing is fine, but the real teeth is the same-author self-certification risk on an oracle the plan requires 'fully green' at a phase exit with zero fixture-authorship guardrail anywhere in the plan text -- that is closer to high than medium given O1's gating role. Keep the proposed P0/P1b text; add explicitly that spec.md must be re-derived post-D30 before any O1 fixture is written from it, since using it as-is would encode now-superseded semantics into a supposedly authoritative oracle.

---

### MIG-028 [high] Budgets v4: §9.3 and the draft disagree, and the move from v3 includes loosenings that the plan's own rule sends to the owner

*Plan: 6 (supersession p95 ≤1 ms, max ≤8 ms); 9.3 (plan:1147-1171); drafts/compiler.v4.toml; Sources: M3; verdict: confirmed; severity medium -> high after refutation*

Where §9.3 and the draft disagree: (1) Supersession: §6 says p95 ≤1 ms and max ≤8 ms, §9.3 keeps the old 8 ms max, and the draft has only supersede_stop_p95_ms = 2.0 with no max (v3 has cancellation_max_ms = 8.0). (2) Bundle lookup: §9.3 keeps the old 1 ms limit; the draft has no lookup key (v3 loaded_bundle_lookup_max_ms = 1.0). (3) RSS: §9.3 says 'absolute peak RSS from a lean producer'; the draft gates rss_delta 8/64/192 MiB, where v3 has absolute 32/128/512. (4) Scaling: the draft raises max_doubling_ratio from v3's 2.2 to 2.3, which process.md:426 itself calls an owner item. (5) The draft discards checkpoint samples above max_loadavg_1m 1.5. This machine read 1.82-2.09 during this review and 3.1-3.6 in the oracle run, so checkpoints would retry indefinitely while any agent works. Plan text says 'Tightening is free; loosening … needs the owner', and without a mapping nobody can check it.

Evidence:
- [file, verified] `budgets/compiler.toml:40-84` — v3: p99 25, lookup 1.0, cancellation_max 8.0, doubling 2.2, peak RSS 32/128/512
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/compiler.v4.toml:26,101,114,127,172,178,205` — loadavg 1.5; rss_delta 8/64/192; p99 25; supersede p95 2.0; ratio 2.3; no lookup/cancel max
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1151-1166 and §6 supersession row` — keeps 8 ms max and 1 ms lookup; absolute RSS
- [file, from notes] `docs/plans/compiler_rewrite_notes/process.md:426` — 2.2->2.3 flagged as needing owner approval
- [measurement, verified] `uptime` — load average 1.82, 2.09, 2.00

**Proposed plan change.** Add to §9.3 a table mapping each v3 key to its v4 key, marked kept / tightened / changed metric / removed / loosened. Mark 2.2->2.3 and any removed max or lookup limit as owner items. Say the draft is not authoritative. Replace the loadavg guard with 'samples are taken only while the measurement lock is held and no cargo process runs'.

**Refuter correction (low tier, high confidence).** Raise from medium to high: this is not just a documentation-consistency nit, it's a would-be gating tool where the two candidate authorities (plan prose vs. implementing draft) numerically disagree on every listed limit, and the loadavg design is independently demonstrated (not just claimed) to be operationally broken on the reviewer's own machine, which multiplies into 'plan uses stale metric, xtask uses different metric, checkpoints silently retry forever under any concurrent load' if shipped as drafted. The proposed v3->v4 mapping table and 'not authoritative' framing for the draft, plus replacing the loadavg guard with a measurement-lock-based check, are the right fix.

---

### MIG-004 [medium] Cells work outside the formula engine is missing from 'Size L': D30 WHILE conversions, D13 private keys, D24 binders

*Decisions: D13, D24, D30; Plan: §5.1 Cells, closed-contract and unused-binder rows; P3b; Sources: M2; verdict: confirmed*

Beyond the evaluator: (1) Under D30, cell.bn:56 `display_text: editing |> WHEN { True => editing_text, False => value }` stops following typing and recalculation, and view.bn:25 `store.selected_input |> WHEN { Found[value] => selected_formula_bar(cell: value) }` freezes the formula bar's editing_text. Both must become WHILE. (2) Under D13, view.bn:264-266 `__selected_background` and `__selected_border` are a private renderer convention (render_scene.rs:3756, boon_native_gpu lib.rs:6003, boon_document lib.rs:7034 prefix filter), and view.bn:251-257 puts address, key and text in the element record. Closed contracts need a public selected-state style API or a WHILE on `selected`. (3) Under D24, `formulas |> List/map(item, new: Pending)` and a fold's unused element binder are errors, so the catalog needs a `__` binder in list operations. (4) The 2,600-row list is rebuilt by index every edit; the evaluator must read by position (List/get), not List/find by address as today, or each pass costs O(N²).

Evidence:
- [file, verified] `examples/cells/cell.bn:56-63; examples/cells/view.bn:25-28,251-270`
- [file, verified] `crates/boon_document/src/render_scene.rs:3756; crates/boon_native_gpu/src/lib.rs:6003; crates/boon_document/src/lib.rs:7034`

**Proposed plan change.** List these as separate Cells work items in P3b and cover them in the spike. Suggested total for D16: L (about 2 weeks) with List/fold: formula.bn (366 lines) rewritten as parse-to-data plus an evaluator of about 300-350 lines, the view and contract fixes, and the scenario rework. Plus the S-M engine work for List/fold and `__` binders, done in P2b/P4 and not by the example author.

**Refuter correction (low tier, high confidence).** None needed beyond what's proposed; the four sub-items are all independently verified in the current code.

---

### MIG-005 [medium] The plan never states whether a WHEN over a value with a start value evaluates at activation, which decides the size of the D30 migration (~335 vs ~1,070+ sites) and whether every constant-tag theme dispatch becomes a 'never runs' error

*Decisions: D30, D31, D32, D8; Plan: §4.3 Change rules, lines 381-387 and 409-411; §5.1 D30 row; D30, D31, D32, D8; Sources: M1, M2; verdict: confirmed; severity high -> medium after refutation*

§4.3 (plan:382-387) says a WHEN arm runs 'once each time the input updates'. It also says start is not an update (D32, plan:101 and :382) and that 'THEN or WHEN over something that never updates is an error ("never runs")'. Read literally, `PASSED.mode |> WHEN { Light => Oklch[...], Dark => ... }` has no value until mode first updates, because mode's start value is not an update. And `of |> WHEN {...}` inside a theme function called as `Theme/material(of: FilterSelected)` is a 'never runs' error. Measured over the .bn files: 1,096 WHENs select on a value outside a copy context. 335 of them read something that updates independently (see the WHEN→WHILE finding). The other 736 are pure mappings, whose result matches WHILE only if the WHEN is evaluated at start. 429 WHENs select on a FUNCTION parameter: for 27, every call site passes a literal, and 70 more are mixed. plan:409-411 ('a WHEN over a value with a start value that copies …') quietly assumes WHEN evaluates at start, but no rule says so. D31 then needs a second rule for commands in WHEN arms at start: the arm gives its value, but the command is skipped. That rule is not written either.

Separately confirmed: NovyTheme keeps its tag API with literal tags at 84 material and 60 font call sites (`NovyTheme/material(mode:, of: PanelSurface)` → `of |> WHEN {…}` at NovyTheme.bn:3); today's TodoMVC themes contain about 187 `mode |> WHEN` or `request |> WHEN` sites, and the draft themes 27 each. Cells formula WHENs also switch on values that have a start value. D30's own 'never runs' example is a THEN; the WHEN case is unspecified.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:382-387` — copy contexts run on updates; start is not an update; 'never runs' error
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:409-411` — stale-copy hint assumes a WHEN over a value with a start value
- [measurement, verified] `python3 docs/plans/compiler_rewrite_notes/review/census/census.py -> summary.when_by_sel_kind_and_ctx, class when_value_pure_mapping, when_param_selector_call_literalness` — .bn: value|None 1,096; pure-mapping 736 (theme 293, view 268, state 175); param selectors all_literal 31 (27 .bn), mixed 70, all_nonliteral 309
- [file, verified] `examples/todo_mvc_physical/Theme/Neobrutalism.bn:183` — `colors: PASSED.mode |> WHEN {` is the typical start-valued selector
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:381-387`
- [measurement, verified] `rg -o 'NovyTheme/[a-z_]+' examples/novywave --glob '*.bn' | sort | uniq -c` — 84 NovyTheme/material, 60 NovyTheme/font (the plan's counts are confirmed)
- [measurement, verified] `grep -c WHEN examples/todo_mvc_physical/Theme/*.bn` — 40+41+32+36+38 WHEN in the 5 theme files, 18 in Theme.bn
- [file, verified] `examples/novywave/Theme/NovyTheme.bn:3,305` — material(mode, of) and font(mode, of) dispatch on `of`

**Proposed plan change.** Add to §4.3 Change rules: "Start. A copy context whose input has a value from the start runs once at start, against the start snapshot. Commands inside it do not run then (D31). 'Never runs' is reported for THEN, and for a WHEN whose input has no start value and never updates. Per instance, a FUNCTION parameter bound to a constant is a value from the start." List it as a P0 semantics deliverable. Add a §5.1 row: 'WHEN over a value with pure arms: 736 sites; no change under A, WHILE under B.' Additionally add the NovyTheme dispatch shape and `mode |> WHEN` leaf tokens as positive fixtures.

**Refuter correction (high tier, medium confidence).** Lower severity to medium. Present option A as the plan's implied reading that needs one explicit sentence, not as an open question. Keep the proposed §4.3 'Start' sentence, including 'commands in a WHEN arm do not run at start (D31)'. Also state per instance whether a FUNCTION parameter bound to a literal counts as 'never updates' for the error at :386, or as a start value. Cite WHEN_VS_WHILE.md:34-37 as original intent. Mark the census numbers taken-from-notes.

**Owner question.** When a WHEN's input has a value from the start, does the WHEN take a value at start?
- A: Yes. A copy context whose input has a start value runs once at start, against the start snapshot. Commands inside it do not run then (D31). 'Never runs' applies only to THEN, and to a WHEN whose input has no start value and never updates.
- B: No, as strictly written. WHEN runs only on updates, so every WHEN over a value (~1,070 .bn sites) must become WHILE, and WHEN is left for decoding events only.
- C: As A, but a WHEN over a selector that never updates is folded to a constant and gets a warning instead of an error.
Recommendation: A. It is what plan:409-411 already assumes, it keeps the 736 pure mappings unchanged, and it is what today's runtime does (plan §5.1 says 'Today's runtime runs WHEN over a value live').

---

### MIG-007 [medium] The WHEN→WHILE migration under D30 is measurable now: about 335 sites, of which 36 of 37 sampled must change

*Plan: §5.1 D30 row; §4.3 stale-copy hint; S4; P3a; Sources: M1; verdict: confirmed; severity high -> medium after refutation*

§5.1 leaves this row as 'census in P2b'. A static heuristic, described in RESULTS.md, finds 335 refined candidates in the .bn files (+15 in Rust strings): view 142, state 132, theme 61. The heuristic takes a value selector outside a copy context whose arms read something the selector does not read, or call a user FUNCTION that transitively reads PASSED. By example: NovyWave 192 (state 111, view 74, theme 7), FjordPulse 57, TodoMVC 48 (37 in theme files that D8 rewrites anyway), persons_pro 23, cells 6, kavik_cz 5. I hand-classified a random sample of 40 raw candidates (seed 20260930). 36 must become WHILE, 3 stay WHEN, and 1 is unclear. The refined rule drops exactly the 3 that stay, so its precision is 36/37 (95% CI about 0.86-0.99). Also:
- 27 more candidates select on an effect result and need a person to judge.
- 7 sites create state inside a copy context. One of them, NovyWave RUN.bn:4540, only becomes legal once its WHEN is a WHILE.

The sample also exposed a same-step hazard: `fjordpulse/Server/RUN.bn:269 public_http_status(path, method, query)` switches on `path` and reads `method` in its arms. Under 'committed snapshot plus the input's new value' (plan:384-386), when both fields update in the same step, the arm sees the previous request's method. So a missed conversion is wrong within a step, not just stale later. Today's runtime runs WHEN live, so converting these sites to WHILE keeps today's behaviour.

Evidence:
- [measurement, verified] `python3 docs/plans/compiler_rewrite_notes/review/census/census.py; classes when_while_candidate_refined, when_while_candidate_effect_selector, state_in_copy_context` — 335 .bn refined, 27 effect-selector, 7 state-in-copy (.bn)
- [file, verified] `docs/plans/compiler_rewrite_notes/review/census/sample40_classified.json` — hand classification of 40 sampled candidates
- [file, verified] `examples/todo_mvc_physical/RUN.bn:218-228` — theme_switcher_button_material: `selected |> WHEN { True => [...Theme/material(of: FilterSelected), gloss: hovered |> WHEN …] }`. The theme and hovered go stale under copy.
- [file, verified] `examples/novywave/View/NovyView.bn:39-41` — app_shell_for_dialog_state: `workspace_dialog == Open |> WHEN { True => modal_app_shell() False => app_shell() }`. Under copy, the whole app shell would freeze until the dialog toggles.
- [file, verified] `examples/fjordpulse/Server/RUN.bn:269` — path/method/query same-step hazard
- [file, verified] `examples/novywave/RUN.bn:4538-4543` — `row.item_kind |> WHEN { VariableRow => new_selected_signal(selected_signal: row) }`. The callee declares SOURCE and HOLD (RUN.bn:4736-4743), so this is state inside a copy context.

**Proposed plan change.** Replace the §5.1 D30 row's 'census in P2b' with: '~335 .bn sites (view 142, state 132, theme 61; NovyWave 192, FjordPulse 57, TodoMVC 48, persons_pro 23), plus 27 WHENs over effect results to review by hand and 7 with state inside a copy context. Sampled precision 36/37.' In §4.3, make the stale-copy hint (plan:409-411) a **default-on warning with the fix-it "use WHILE"** wherever the WHEN's result flows into a document, a FUNCTION result or a derived field. The corpus says such copies are almost never intended. Add 'WHEN→WHILE fix-it pass' to P3a, and add this heuristic to S4 as the calibration target for the checker's own list. Document the same-step snapshot behaviour with the public_http_status example.

**Refuter correction (high tier, medium confidence).** Keep the count ('~290-335 sites, NovyWave ~190') in §5.1 as a scanner estimate. Remove the public_http_status same-step hazard, or reword it as an example of a co-triggered WHEN that does NOT need WHILE. Revise the precision to at most 35/37 and say the heuristic must exclude selectors co-triggered with the arm reads, a check that needs instance-level update provenance (Phase A). Severity medium: this is sizing plus warning policy, and the plan already delegates the list to the checker.

---

### MIG-008 [medium] The Theme draft keeps copying WHEN where the arm must follow mode updates: under D30, toggling mode without switching theme leaves the old colours, and no scenario would catch it

*Decisions: D8, D30, D25; Plan: §5.1 Theme and NovyTheme rows (lines 745-746) and the 'WHEN that must follow live updates' row; §5.2 step 3; D8, D30; Sources: M2; verdict: confirmed; severity high -> medium after refutation*

The draft Theme.bn.txt is `name |> WHEN { Classic => Classic/tokens(mode: mode) … }`. Under D30 the arm, and every `mode |> WHEN` inside Classic/tokens (the §4.3 placement rule treats a copy context's body as evaluated once), runs only when `name` updates. So the toggle-light-dark-mode step, taken while the theme stays Neumorphism, leaves every Light token in place. The draft's RUN.bn call-site table has the same flaw: `selected |> WHEN { True => theme.material.filter_selected, False => [] }`, `checked |> WHEN { True => theme.font.icon_checked … }` and the interactive_material helper all go stale on a theme or mode switch. NovyTheme's retained API `of |> WHEN { TraceHigh => [..., color: mode |> WHEN {…}] }` never recolours, because `of` is a constant. The TodoMVC scenario asserts only root text for these steps ('theme_options.name' and 'theme_options.mode', todo_mvc_physical.scn:141-185), so the .scn would pass while the pixels are wrong. Inside the draft's per-theme files the leaf WHENs have constant arms and are fine: 0 of 27 per theme read non-constants. The fix is small in lines but decides whether theme switching works.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/migration/todo_theme_new/Theme.bn.txt:4-12`
- [file, verified] `docs/plans/compiler_rewrite_notes/examples.md:292-321 (§4.3 call-site table)` — call-site WHENs whose arms read theme.*
- [file, verified] `examples/todo_mvc_physical.scn:141-185` — theme steps assert only theme_options.name and theme_options.mode
- [file, verified] `examples/manifest.toml:220-226` — switch-theme-* and toggle-light-dark-mode are native-gated input scenarios; visual_artifacts include theme-switcher-crop and html-theme-fixtures
- [file, verified] `docs/plans/compiler_rewrite_notes/examples.md:344` — NovyTheme: 'the tag API stays', while plan line 746 says 'Same idiom'

```boon
-- draft (plan: frozen on mode toggle; today's live WHEN hides this)
theme: Theme/tokens(name: theme_options.name, mode: theme_options.mode)
FUNCTION tokens(name, mode) {
    name |> WHEN { Classic => Classic/tokens(mode: mode), Professional => Professional/tokens(mode: mode) }
}
-- required under D30
FUNCTION tokens(name, mode) {
    name |> WHILE { Classic => Classic/tokens(mode: mode), Professional => Professional/tokens(mode: mode) }
}
```

**Proposed plan change.** Update the Theme row and examples.md §4: Theme/tokens and every call-site selector whose arms read `theme.*` use WHILE. For NovyTheme, either `of |> WHILE` or convert to `tokens(mode)` records, and make plan line 746 and notes §5 agree. Add to the §5.2 step-3 matrix scenario checkpoints in both orders ('switch theme → toggle mode' and 'toggle mode → switch theme') that assert a themed style value, not only root text. Have S4 report these sites explicitly. By D25, a WHILE over 5 themes still instantiates all 5 arms' structure, so include that in the S2 checker-cost probe.

**Refuter correction (high tier, high confidence).** Reword: 'the notes draft that plan:745 points to (examples.md §4, Theme.bn.txt) violates the plan's own D30 row; fix the draft and add transition checkpoints'. Lower severity to medium. Keep the proposed matrix checkpoints (switch theme then toggle mode, and the reverse) that assert a themed style value or readback crop. Also resolve plan:746 'Same idiom' vs the notes' 'tag API stays' for NovyTheme.

---

### MIG-009 [medium] The Theme draft covers only 2 of 5 themes, contradicts D22 and D5, and fails today's lowering, so '2,443 → ~1,110 lines, ~20 → ~8 types' is an extrapolation

*Decisions: D5, D8, D13, D22; Plan: §5.1 Theme row (line 745), NovyTheme row (746), D22/D13 row; P3a; R-RENDER; Sources: M2; verdict: confirmed*

Verified: the 6 theme files total 2,444 lines, and all 94 Theme/ call sites are in RUN.bn. 'Six files' describes the theme modules, not where the calls are. The draft (Base 31 + Theme 12 + Classic 210 + Professional 203 = 456 lines) has no Glassmorphism, Neobrutalism or Neumorphism: these are the largest today (492/434/464 lines) and carry the glass and shadow specifics. Defects: (1) D22: examples.md §4.4 drops 'glow and shadows inside materials … the delete-button glow' as dead fields, yet D22 keeps them. Today there are 5 glow sites (one each in Glassmorphism, Neobrutalism, Neumorphism and Professional, plus RUN.bn:199) and 14 'shadows' sites. Base/material has no glow or shadows field, so Material needs +2 fields and focus- and hover-glow variants (editing, delete-hovered). (2) D5: Professional.bn.txt:37 puts `lights: lights(mode: mode)` in tokens, while Classic.bn.txt has no lights at all and Theme.bn.txt has no lights dispatcher (examples.md §4.2 shows one). `theme.lights` would therefore be Optional, and reading it is an error. (3) The old-engine compatibility rule (§5.1): Light/* inside a root record fails today's lowering, and examples.md:126 itself notes this. The notes' 'checked with today's compiler' refers to two_theme_probe, which has neither lights in tokens nor glow.

Evidence:
- [measurement, verified] `wc -l examples/todo_mvc_physical/Theme/*.bn; grep -o 'Theme/[a-z_]*(' RUN.bn | wc -l; grep -rl 'Theme/' examples/todo_mvc_physical` — 2,444 lines; 94 calls; only RUN.bn calls Theme/
- [measurement, verified] `target/release/boon_cli check <scratch>/M2/lights_in_record.bn (Professional draft lights() in a root record, used by Scene/new)` — 'derived value `theme.lights` failed executable lowering: … call `Light/directional` has no typed PlanExecutor row operation'
- [measurement, verified] `target/release/boon_cli check two_theme_probe.bn / theme_probe.bn / prof_probe.bn` — all pass (7/5/4 operations): today's parser accepts the draft syntax
- [measurement, verified] `grep -n 'lights\|glow' todo_theme_new/Classic.bn.txt Professional.bn.txt; grep -c glow/shadows Theme/*.bn RUN.bn`
- [file, verified] `docs/plans/compiler_rewrite_notes/examples.md:126,335`

**Proposed plan change.** In the Theme row, cite the draft as 'Classic and Professional only; the other three not drafted'. Require lights to be the same in every theme: keep `Theme/lights(name, mode)` as a WHILE dispatcher until the new engine lowers Light/* in records. Add glow and shadows to Base/material and the focus- and hover-glow variants, as D22 requires. Re-estimate at about 1,250-1,400 lines and about 9-10 role types (Glow added). Effort: TodoMVC theme M-L, about 5-7 engineer-days including the 5×2 matrix verification; the notes say about 3. NovyTheme S-M (487 lines, 144 calls, with the D30 fix).

**Refuter correction (low tier, high confidence).** Adjust phrasing: 'Classic.bn.txt defines a dead, uncalled `lights` function; its `tokens()` record has no `lights` field at all', rather than 'has no lights at all.' Otherwise the finding and proposed re-estimate stand.

---

### MIG-010 [medium] The draft's neutral 'None' sentinels (Base/material color: None, Base/frame border: None) need an explicit closed-contract rule under D13

*Decisions: D5, D13; Plan: §4.3 Contracts; §5.1 closed-contract row; D5, D13; Sources: M2; verdict: confirmed*

Uniform role shapes rest on neutral records in which 'None' works only because 'background "None" fails to parse → the per-kind default fill' (examples.md §4.4). No example uses `color: None` or `border: None` today, and render_scene has no 'None' handling. Under D13 'a value the contract does not accept is an error', so every material and frame spread would be a contract error unless the contract defines None as 'unset'. The glass neutrals (frosted_saturate 1, glass_highlight_color #ffffff, others 0) equal the CPU renderer defaults (render_scene.rs:2525-2540, 3588-3595). They are now written as keys on every material, so the native GPU path must use the same defaults, or the readback A/B will show differences.

Evidence:
- [measurement, verified] `rg -n 'color: None|border: None' examples --glob '*.bn'; rg -n '"None"' crates/boon_document/src/render_scene.rs` — 0 hits for both
- [file, verified] `crates/boon_document/src/render_scene.rs:2525-2540,3588-3595` — defaults 0 / 1.0 / white. Native GPU defaults not checked
- [file, verified] `docs/plans/compiler_rewrite_notes/examples.md:323-335 (§4.4)`

**Proposed plan change.** Make it a P0 contract item: color-typed and border-typed style keys (and glow) accept the tag `None`, meaning 'unset'. The renderer treats it as absent explicitly instead of through a parse failure. The generated contract documents it. Add a readback check that one material with and without the neutral keys renders the same on the native path.

**Refuter correction (low tier, high confidence).** None; the owner-question framing (a/b/c options with recommendation (a)) is appropriate and actionable as written.

**Owner question.** How should theme role types express 'no colour / no border' under closed contracts?
- (a) The contract admits `None` = unset for colour-like keys (the draft works; one documented rule).
- (b) Optional fields `color?:` in role types. Honest about absence, but gives up the 'every theme has the same shape' completeness check.
- (c) Write the renderer's per-kind defaults as literal values in Base. No contract change, but the defaults are duplicated and brittle.
Recommendation: (a).

---

### MIG-011 [medium] D23 exact record parameters break about 93-140 call sites, mostly in NovyWave, and the plan gives no size or fix-it

*Plan: §5.1 'Exact record parameters (D23)'; §4.3 Contracts; P3b; Sources: M1; verdict: confirmed; severity high -> medium after refutation*

§5.1 lists D23 as 'census'. 140 .bn call sites (+53 in Rust strings) pass a record variable, not a literal, to a user FUNCTION parameter whose body reads fields of it. NovyWave has 104 of them. For 93, the callee never forwards the whole parameter, so its exact type is only the fields it reads. Callees read 1 field in 49 cases, 2 in 38, 3 in 22 and 4 in 19. Checked by hand: `NovyView.bn:1531 file_tree_row_label(row: row)`. The callee reads only `row.expanded_label` and `row.collapsed_label`. The caller's `row` has at least `indent_width` and `scope_key` as well (NovyView.bn:1633), so under D23 this is an 'extra fields' error. The usual Boon idiom `List/map(item, new: row_widget(item: item))` hits the same rule. No literal record argument carries extra fields (0 found), so the whole cost is in variable arguments. None of them has a mechanical fix: each needs either a narrowed literal `[a: row.a, b: row.b]` or a changed signature.

Evidence:
- [measurement, verified] `census.py classes d23_variable_record_arg, d23_literal_record_arg` — 140 .bn (NovyWave 104); 47 where the callee also forwards the parameter; 0 literal records with extras
- [file, verified] `examples/novywave/View/NovyView.bn:1531,1633,1655` — file_tree_row_label(row: row) reads 2 fields; the caller's row has more
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:88 (D23), :451-456` — extra fields are an error; how exactness composes through forwarding is left to P0

**Proposed plan change.** Replace the §5.1 D23 row's 'census' with: '140 .bn call sites pass a record variable (NovyWave 104, Rust strings 53). In 93 the callee never forwards the whole parameter, so each is an error unless the passed record is already exact. Fix-it: narrow the argument to a literal of the consumed fields.' Add the fix-it to P2a and the edits to P3b.

**Refuter correction (high tier, medium confidence).** Correct the example's field list to scope_key, expanded_label, collapsed_label and label. Add the composition point: a FUNCTION that reads field f of a record and also forwards the whole record to a callee that does not consume f has no valid exact type under plan:454-455. P0 should state that the fix-it narrows at the forwarding site (`file_tree_row_label(row: [scope_key: row.scope_key, ...])`), and P2a should ship that fix-it. Keep the recommendation to keep D23 as written. Mark 140/93 as scanner upper bounds taken from notes.

**Owner question.** Is a whole collection row or record variable passed to a helper FUNCTION meant to fail D23 when the helper reads only some of its fields?
- Yes, as written. The checker ships a fix-it that rewrites the argument as a literal of the consumed fields. Budget about 93-140 edits, mostly in NovyWave.
- Yes, and the fix-it instead changes the callee to take scalar parameters, which touches every caller of that helper.
- Clarify D23 so it applies only to record literals and to records built at the call site (this changes D23).
Recommendation: Keep D23 as written, but make the 'narrow the argument' fix-it a P2a deliverable. Add the measured count to §5.1, and specify in P0 how exactness composes for List/map binders.

---

### MIG-012 [medium] Name rules: 67 names resolved by suffix, and 110 `element.hovered` reads rely on a constructor binder that §4.3 does not list

*Plan: §4.3 Names; §5.1 'Suffix fallback removed (D14)'; Sources: M1; verdict: confirmed*

The §4.3 Names lookup order (plan:427-433) lists record-literal siblings, BLOCK locals, WHEN binders, 'OUT/call-context binders' and FUNCTION parameters. It does not name the constructor `element` binder that the notes define (spec.md:587-590: visible in every argument except `element:`). The corpus reads a sibling call argument 119 times, 110 of them as `element.hovered`, usually `style: f(hovered: element.hovered)`. If that binder is not written down, removing the suffix fallback under D14 turns all 110 into 'unknown name'. Separately, 67 bare names in .bn files resolve only through today's suffix fallback. The plan names 3 of them:
- NovyWave 44: 41 are `elements.*` inside functions such as `new_signal`, RUN.bn:4490-4853, which have no lexical `elements`;
- TodoMVC RUN.bn:552 `visible_todos` and RUN.bn:810 `selected_filter`;
- `response` / `http_response` in 5 server examples;
- 10 fixture sites.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:427-433` — lookup order; no constructor element binder
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:587-590` — element binder rule and the 110 count (taken from notes, re-measured)
- [measurement, verified] `git grep -c -P 'element\.hovered' -- '*.bn' => 110; census.py class call_sibling_read => 119`
- [measurement, verified] `census.py class suffix_resolved_name => 67 .bn` — examples/novywave/RUN.bn:4490-4853 (elements x41), todo_mvc_physical/RUN.bn:552,810

**Proposed plan change.** Add to the §4.3 Names list: "the constructor `element` binder (ElementState of the kind), visible in every argument of an element constructor except `element:`." Name it in the §4.2 resolver and in the S4 scanner. Replace the §5.1 suffix row's '(census)' with '67 sites: NovyWave 44 (41 `elements.*` in row-constructor functions, fixed by passing `elements` in as a parameter or through PASSED), TodoMVC 2, server examples 5, fixtures 16.'

**Refuter correction (low tier, high confidence).** None; proposed plan-text addition is precise and directly actionable.

---

### MIG-014 [medium] testdata and the Rust-embedded sources are missing from the scanner counts behind D10, THEN-over-state and the embedded-source scope

*Plan: §5 preamble; §5.1 D10 and D30 rows; P3b 'Embedded sources and inline test sources'; Sources: M1; verdict: confirmed*

The plan's counts come from scans that skipped testdata/ and the Boon strings inside Rust files. Measured:
- **D10** subject reads in arms: 82 in .bn files (+10 in Rust) against the plan's 68. The difference is `testdata/typed_passkey_effects.bn` with 14.
- **THEN over HOLD state** in the same file: 35 in .bn files, against the plan's 18 and the notes' 25. They are persons_pro 14, `testdata/typed_passkey_effects.bn` 14, host_service_effects 6 and server_effect_chain 1. NovyWave adds 4 THENs over a LATEST with a fallback, and 93 .bn THENs have a non-event input.
- **Boon in Rust:** 48 files hold 191 Boon-like string literals (3,690 lines), not 'about 12 Rust test files'. Only 15 of those files (47 snippets) are in crates that survive P8. The other 144 snippets are in old-compiler crates (boon_semantic 56, boon_typecheck 35, boon_compiler 32, boon_parser 14, boon_ir 6, boon_verify 1), which are deleted at P8 and should not be migrated.

Evidence:
- [measurement, verified] `census.py summary: rust_files_with_boon=48, units_rust=191, lines_rust=3690; rust snippets by crate`
- [measurement, verified] `census.py class when_subject_read_arm restricted to field reads through the subject: 82 .bn (novywave 55, testdata 14, host_service_effects 12, server_effect_chain 1)`
- [measurement, verified] `census.py class then_over_hold_same_file: 35 .bn`
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:71,474` — notes claim 25 (taken from notes; refuted as incomplete)
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:729-737` — 'Boon inside about 12 Rust test files'

**Proposed plan change.** In §5, replace 'Boon inside about 12 Rust test files' with: '191 Boon snippets in 48 Rust files. Only 47 snippets in 15 files belong to crates that survive P8 (plan_executor 16, server_runtime 16, wellen_host 5, and one or two each in document_model, editor, web_host, host_runtime, behavior_harness and cli). They are migrated in P3b. Snippets in old-compiler crates stay with the old engine and are deleted at P8.' Update the D10 row to '82 (NovyWave 55, host_service_effects 12, testdata/typed_passkey_effects 14, server_effect_chain 1), +10 in Rust'. Update the THEN-over-state sentence to '35 over a HOLD, plus NovyWave 4 over a LATEST with a fallback'.

**Refuter correction (low tier, medium confidence).** Treat the precise per-crate snippet counts (191, 3690 lines, 47 surviving) as taken-from-notes/re-derived-by-script rather than independently hand-verified in this pass; the file-count order of magnitude (48 vs '~12') is independently confirmed.

---

### MIG-015 [medium] D24 unused names: 43, not about 12 (36 are pattern binders)

*Plan: §5.1 D24 row; D24; Sources: M1; verdict: confirmed*

The ~12 in §5.1 comes from the notes, which counted only FUNCTION parameters (examples.md: 'about 12 candidates'). D24 also makes unused pattern binders an error. The .bn files have:
- 7 unused FUNCTION parameters, plus 4 unused `OUT` parameters whose status the plan should settle;
- **36 unused WHEN pattern binders**, for example `InvalidNumber[reason, position] => …` with neither binder used (cells/formula.bn 16, NovyWave 21, TodoMVC 5 such as Classic.bn:97 `InteractiveRecessed[focus]`).

The Rust strings add 26 more.

Evidence:
- [measurement, verified] `census.py classes unused_param (11 .bn incl. 4 OUT), unused_pattern_binder (36 .bn, 19 Rust)`
- [file, verified] `examples/novywave/Model/NovyModel.bn:218,226,251; examples/cells/formula.bn:73,348`

**Proposed plan change.** Change the §5.1 D24 row to: '43 .bn (7 parameters, 36 pattern binders), +26 in Rust. Fix-it: drop the binder (`Tag[a, b] => e` becomes `Tag => e` when none is used).' Decide in P0 whether an unused `OUT` parameter counts as unused.

**Refuter correction (low tier, high confidence).** None; the fix-it description (drop the binder, `Tag[a,b] => e` becomes `Tag => e`) is correct and matches the Classic.bn:97 example.

---

### MIG-016 [medium] Missing census classes: state inside copy contexts (7), tags both bare and tagged (3), and commands in live contexts (2, both BUILD.bn)

*Plan: §5.1; S4 scope; Sources: M1; verdict: confirmed*

The plan's placement rule makes HOLD, SOURCE and stateful calls inside a copy context an error (plan:388-392). The .bn corpus has 7 such sites:
- NovyWave RUN.bn:4540: a stateful row constructor called inside a WHEN arm;
- fibonacci.bn:61: a HOLD inside a THEN;
- 3 in testdata/compiler_*_pulses.bn;
- 2 in crates/boon_plan_executor/testdata/reactive_rows_vertical.bn.

D18.4 (a tag is either bare or tagged within one type) catches 3 units: `Ok` in todo_mvc_physical/BUILD.bn and novywave/BUILD.bn (1 bare use, 4 tagged), and `StringValue` in NovyWave RUN.bn. Commands in live contexts (D31) appear at exactly 2 sites, `File/write_text` at todo_mvc_physical/BUILD.bn:33 and novywave/BUILD.bn:33. That matches the plan's 'BUILD files'. None of these classes has its own §5.1 row.

Evidence:
- [measurement, verified] `census.py classes state_in_copy_context (7 .bn), tag_bare_and_tagged_same_unit (3 .bn), command_calls ctx=live (2)`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:388-392` — placement rule

**Proposed plan change.** Add these rows to §5.1:
- 'State inside copy contexts (D30 placement): 7 sites. NovyWave RUN.bn:4540 is fixed by the WHEN→WHILE pass. fibonacci.bn:61 and 5 fixtures are rewritten.'
- 'Bare and tagged tag in one type (D18.4): BUILD.bn `Ok` ×2, NovyWave `StringValue`.'

Add both classes to the S4 scanner scope.

**Refuter correction (low tier, medium confidence).** None major; recommend the proposed §5.1 rows be added largely as written.

---

### MIG-017 [medium] P3a/P3b sizing from the measured counts: about 1,050 .bn edits, 56% in NovyWave, with P3b likely 3-4 weeks rather than 2-3

*Plan: §8 P2a, P3a, P3b, Effort; Sources: M1; verdict: confirmed*

The measured classes add up to about 1,056 .bn edit sites:
- WHEN→WHILE 335
- D23 93-140
- theme call sites 94 + 144
- one-input LATEST 91
- D10 82
- suffix 67
- D24 43
- List/latest 28
- parentheses 12
- state in copy contexts 7
- self-referential LATEST 5
- Http 2, commands 2, D18.4 3, catch_cycle 1

About 630 of them (60%) are fix-it-shaped: one-input LATEST, D24, D10, parentheses, suffix qualification, and WHEN→WHILE once the checker lints it. The rest need judgment: D23 narrowing, the Theme refactors, List/latest rewrites, self-references and state placement. NovyWave carries about 589 (56%): WHEN→WHILE 192, D23 104, NovyTheme 144, D10 55, suffix 44, List/latest 22, D24 21, parentheses 7. There are also 47 Rust-embedded snippets in surviving crates. Estimated effort:
- P3a's mechanical list: about 1 week of fix-it application, if the fix-its exist.
- The TodoMVC Theme refactor: about 3 days (the notes' own estimate).
- NovyTheme: 2-3 days.
- WHEN→WHILE review and the 27 effect-selector cases: 2 days.
- D23 narrowing: 3-5 days, mostly NovyWave, with a render diff.

That fits the plan's P3a (3-4 weeks) only if the fix-its ship in P2a. It pushes P3b to about 3-4 weeks once D23 and the Rust snippets are included. Cells, closed contracts and scenario triage remain the unknowns.

Evidence:
- [measurement, verified] `docs/plans/compiler_rewrite_notes/review/census/census_results.json (all classes)`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:960-986` — P3a 3-4 weeks, P3b 2-3 weeks
- [file, from notes] `docs/plans/compiler_rewrite_notes/examples.md:381-393` — notes' 2.5-4 engineer-weeks example work excluding the D23, WHEN→WHILE and Rust snippets (taken from notes)

**Proposed plan change.** In §8 P2a, add the deliverable 'fix-its for D10 binders, D24, one-input LATEST, parentheses, suffix qualification, WHEN→WHILE (stale-copy lint) and D23 argument narrowing'. In P3a, list the measured counts per class. Re-estimate P3b at 3-4 weeks, adding D23 (93-140 sites) and the 47 Rust snippets in surviving crates. Record in §12 that NovyWave holds more than half the edits, so the novywave gate is the migration's critical path.

**Refuter correction (low tier, medium confidence).** Fix the evidence citation to point at docs/plans/compiler_rewrite_notes/review/census/RESULTS.md (and census.py as the source script) instead of a nonexistent census_results.json. The estimate itself (about 1,056 edits, NovyWave ~56%, P3b re-estimate to 3-4 weeks) is not contradicted by anything found.

---

### MIG-019 [medium] 182 `a == b |> f()` sites rely on binary operators binding tighter than `|>` on its left, which the plan never states

*Plan: §4.2 Expressions; D11; P1b; Sources: M1; verdict: confirmed; severity low -> medium after refutation*

§4.2 (plan:292-296) says only that 'a pipe form is the only thing allowed on the right of `|>`', and D11 calls `|>` 'structural'. 182 .bn sites (NovyWave 160) have the shape `x == 0 |> Bool/and(right: y == 0)` or `a + b |> f()`. Their meaning depends on binary operators binding tighter than `|>` on its left, for example TodoMVC RUN.bn:79-80 `has_todos`. If the new parser differs, they change meaning silently. The tree-equivalence check in P1b catches this only if the old parser agrees on these sites.

Evidence:
- [measurement, verified] `census.py class binop_then_pipe: 182 .bn (novywave 160)`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:292-296`
- [file, verified] `examples/todo_mvc_physical/RUN.bn:79-80`

**Proposed plan change.** Add one sentence to §4.2 Expressions: 'On the left of `|>`, a whole binary expression is the pipe input: `a == 0 |> f()` is `(a == 0) |> f()`. This does not count as mixing operators under D11.' Add the 182 sites as a named corpus in the P1b tree-equivalence test.

**Refuter correction (low tier, high confidence).** Severity is understated: this silently changes meaning at 182 real call sites (160 in NovyWave alone, a flagship example), not a handful of edge cases, and the failure mode is silent (wrong runtime behavior, not a compile error) unless the P1b tree-equivalence corpus happens to include these exact shapes. The proposed one-sentence fix is right; also make the named corpus a required (not just incidental) part of P1b's exit rather than leaving it to 'catches this only if the old parser agrees.'

---

### MIG-021 [medium] The critical-path diagram hides real dependencies: S1 needs P4/P5/R-DOC pieces, S5 needs S6, R-DOC's exit needs P4 output, and the O2 runner is ported only in P7

*Plan: 8 critical path (plan:855-860); S1/S2/S5/S6 (plan:908-913); R-DOC (plan:988-994); P4 exit; P5 (plan:1017-1028); 4.8 (plan:698-701); P6 (plan:1035); Sources: M3; verdict: confirmed; severity high -> medium after refutation*

The diagram shows one serial chain with R-DOC as a side track, but: (1) S1's exit (plan:908) needs a shadow native run with engine=next, which needs the engine selector (§4.8, built in P5). It needs 'restart restores state under D12', which needs persistence lowering (P4). It needs 'an unrelated edit keeps focus', which needs element identity (R-DOC/S3) plus a warm session (P5). So S1 is a mini-P1-to-P5, not a 2-week spike. (2) S2 needs 'TodoMVC view functions (theme refactored)', which is P3a's Theme work; only drafts exist. (3) S5 counts same-tick reads 'across all runnable scenarios', but only 5 of 20 manifest scenarios run in boon_cli today (minimal, hello_world, counter_latest, flow_operators, pages), so S5 depends on S6's host-service runner. (4) R-DOC's exit gate (§4.6) measures TodoMVC, NovyWave and Cells per-frame work on element records. Those records come only from the new lowering (P4) run on migrated sources (P3b, including the Cells redesign). So R-DOC's final 1-2 weeks run serially after P4 and before P5 exit: it is on the critical path, not parallel to it. (5) §4.8 ports host_runtime (the MigrationScenarioRunner that S6 reuses for O2), boon_runtime (boon_cli run), phase0 and the behaviour harness in P7. Yet P4 exit requires 'O2 passes where the scenarios were triaged' on next, and P5 runs 'server, http, host, wellen, web and plan_executor crate tests with next' (plan:1024). (6) P6's 'per-definition reuse, if needed (size L)' conflicts with Q20, which makes NovyWave warm report-only at P7. It should be explicitly post-P8, not a latent L on the critical path. What does hold: P2b needs P2a (the census is the new checker's output), P3b needs P2b, and P3a from week 1 of P2a works for scanner-driven classes only.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:908` — S1 exit: restart under D12, focus on unrelated edit, shadow counter-dev native run with engine=next
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:684-691` — engine selector lives in the shared compile service (P5)
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:698-701 vs :1024 and P4 exit` — ports in P7 vs next-engine crate tests in P5 and O2 at P4 exit
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:648-651,988-994` — R-DOC gate on TodoMVC/NovyWave/Cells; both representations until P8
- [file, from notes] `docs/plans/compiler_rewrite_notes/review/measurements/oracle.md Totals` — 5/20 scenarios pass in boon_cli run
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:413-440 (rows 15,16,18,19,20)` — consumer matrix: boon_runtime, host_runtime migration_scenario, behaviour harness, phase0, plan_executor tests all use the old compile API
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md Q20, :1035` — NovyWave warm report-only at P7 vs P6 reuse 'if needed'

**Proposed plan change.** Redraw §8 'Critical path' as: `P1a -> P0 (spec, S4, S6, gates; S1 reduced) -> P2a -> P2b -> P3b -> P4 [∥ R-CHG: R7+R8+R9 runtime, must finish by P4 exit] -> R-DOC integration on P4 output (1-2 wk) -> P5 -> P6 -> P7 -> P8`. Rewrite S1's exit as: "verify_plan passes; counter scenarios pass on the runtime; every needed runtime/format change is listed and sized." Move 'restart under D12' to P4 exit and 'focus survives an unrelated edit' plus the native shadow run to P5 exit. Order S6 before S5. Move the engine selector (a CLI/env stub) to P1b, and the ports of host_runtime, boon_runtime, the behaviour harness and phase0 to P4. Change P6 to "Per-definition reuse is not in P6; it is post-P8 work gated by Q20."

**Refuter correction (high tier, medium confidence).** Drop point (3) and soften (6) to a wording note: say P6 reuse is optional and Q20 governs. Reframe (1): S1's restart and warm-focus exits make it a thin P1-P5 slice. Either scope them down or say explicitly that S1 may use the v11 document path and a throwaway engine switch. Keep (5) as the headline, with the concrete fix: port host_runtime's MigrationScenarioRunner, boon_runtime (for boon_cli run), the behaviour harness and phase0 (or give them an engine switch) by P4, and the plan_executor/server/http tests by P5. Keep (4) as: R-DOC's performance gate can close only after P4 emits element records for the migrated sources, so show that join on the critical-path diagram.

---

### MIG-022 [medium] Landing R7 (snapshot reads) on the shared executor in P0/P1 changes old-engine behaviour, has no microstep model yet, and contradicts R8's versioning and P4's 'R7 if scheduled'

*Decisions: D21, D30, D32, D33; Plan: 4.5 R7/R8 (plan:612-613); S5 (plan:912); P4 (plan:1008); Sources: M3; verdict: confirmed; severity high -> medium after refutation*

R7 is 'landed early on the shared executor (P0/P1); old-engine gates re-run afterwards', and S5's exit is 'Fix landed on the shared executor; affected examples and scenarios listed and adjusted'. But R8 keeps today's behaviour for old-engine plans 'through a plan semantics version until P8, because today's examples rely on live WHEN', and P4 lists 'plus R7 if scheduled'. Those three statements disagree. Today a THEN body sees another HOLD's same-tick new value: in the p6 probe, after one press b_before=1, b_after=1 and c_chain=10. Under D21 alone, without R8's follow-up microsteps, `a |> THEN { a * 10 }` must either read committed a=0 (c_chain=0, a regression) or depend on the microstep and payload rules, and those are R8's design. So R7 cannot be designed separately from R8's tick model. Landing it early means designing the tick twice. It also changes old-engine gate behaviour (the notes expect NovyWave's effect chains to change) in P0, before any new-engine output exists to compare against, so examples get 'adjusted' toward semantics nothing can check yet.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:612,613,912,1008` — R7 early/unconditional vs R8 versioned vs 'R7 if scheduled'
- [measurement, verified] `cp change_probes/p6_snapshot.bn.txt scratchpad/M3/p6.bn; target/release/boon_cli run p6.bn --scenario p6.scn` — press1: store.b_before=1, store.b_after=1, store.c_chain=10 (today); D21 snapshot reading gives 0 for b_before/b_after
- [measurement, verified] `scratchpad M3/p1.bn from change_probes/p1_then_over_hold; boon_cli run p1.bn --scenario p1.scn` — passes with fires=1 after set_one twice: today's HOLD deduplicates equal writes (D32 changes this, part of R8)
- [file, from notes] `docs/plans/compiler_rewrite_notes/change_and_effects.md:471,507,514` — same-tick vs microstep is a design choice; 'would change NovyWave's effect chains'; the runner cannot show microsteps

```boon
a: 0 |> HOLD a { press |> THEN { a + 1 } }
b: 0 |> HOLD b { press |> THEN { a } }
c: 0 |> HOLD c { a |> THEN { a * 10 } }
-- today after one press: a=1, b=1, c=10. D21: b=0 (committed a); c=10 only if a follow-up microstep delivers a's update (R8).
```

**Proposed plan change.** Change R7's row to: "Snapshot-read semantics (D21), implemented together with R8's microstep model under the same plan semantics version; old-engine plans keep today's reads until P8." Change S5 to: "Instrument the executor to count same-tick reads of another cell's new value over the S6 runner's scenarios and the native gates, and list the affected sites. No behaviour change in P0." Delete 'plus R7 if scheduled' from P4 and put R7+R8+R9 into one runtime track (R-CHG) with its own exit: the change_probes scenarios pass on next with the reference or oracle traces.

**Refuter correction (high tier, high confidence).** Lower severity to medium. Keep the owner question, but make option (b) conditional: 'Change the shared executor only if S5's instrumentation finds zero same-tick cross-cell reads in the native gates and triaged scenarios; otherwise version R7 with R8.' Fix the plan text at plan:1008 ('plus R7 if scheduled') so it matches plan:612 in any case. Cite RUNTIME_MODEL.md:91-93 as the source of the microstep ambiguity.

**Owner question.** D21 says 'Unconditional'. Does that require the OLD engine's runtime to switch to snapshot reads before cutover, or is it enough that the new engine has them from its first plan?
- (a) New-engine plans only, versioned with R8; the old engine keeps today's reads until P8
- (b) Change the shared executor for both engines in P0/P1, as the plan says now
Recommendation: (a): the old engine is deleted at P8 and its examples rely on today's reads. (b) costs gate churn with nothing to compare against and forces the tick model to be designed before R8.

---

### MIG-023 [medium] Effort table: P0 is counted as 2 weeks but holds six spikes plus a spec rewrite; R8/R9 hide inside P4; lines are not the binding constraint. My range is 24-34 weeks of critical path and 50-70 engineer-weeks

*Plan: 8 P0 (plan:863-916); Effort (plan:1061-1071); 10 (replacement ~45 k lines); Sources: M3; verdict: confirmed; severity high -> medium after refutation*

The phases in the effort table sum to 32.5-43.5 engineer-weeks, which matches '~32-45'. That sum treats P0 as 2 engineer-weeks. P0 actually contains: static semantics v2 for D4-D36 including the change model and a diagnostic catalog (the notes' spec.md predates D14-D36); reconciliation of 8+ documents; a HOLD options note; S1, a vertical slice through every phase (see the dependency finding); S2; S3, an R-DOC prototype on TodoMVC, Cells and NovyWave; S4; S5; S6, a host-service runner, bisection and triage of 15 failing scenarios. That is realistically 11-17 engineer-weeks and 4-6 calendar weeks. R8 (a rewrite of the executor's tick: copy vs live ops, every write fires, HOLD reset, microsteps, versioned semantics) and R9 (effects, Http split, host ports, effect log) sit inside P4's 5-7 weeks next to all of lowering. I'd size them at 4-7 engineer-weeks on their own. Lines are not what limits this repo. Since June it added 65-205 k Rust lines a week in crates. The kernel (81 k lines) was written in two weeks (W33-W34, +94 k added). boon_semantic (97 k) took four. Only 40% of all Rust ever added survives. Then W35-W40 produced about 6 k lines of code and 84 k1pp A/B artefacts without meeting the targets. Since 2026-09-06, docs grew +39.8 k lines against +6.1 k in crates, and 55 of 91 commits were docs. So 45 k lines is under a week of historical output. The real costs are convergence to gates, owner decisions, one 8-core machine shared by builds and measurements, and native gates that need the launch-scoped COSMIC seat. Those serialize work, so '12-18 calendar weeks with 2-3 parallel agents per phase' is not credible.

Evidence:
- [measurement, verified] `git log --since=2026-06-01 --numstat --format='C %ad' --date=format:'%G-W%V' -- 'crates/*.rs' | awk (sum per ISO week)` — added: W25 186k, W26 103k, W27 145k, W29 187k, W30 121k, W31 205k, W32 77k, W33 139k, W34 85k, W37 5.1k, W38-W40 <0.3k
- [measurement, verified] `git log --numstat -- crates/boon_compiler_kernel | awk per week; crate first commit 2026-08-14` — kernel +38k (W33), +56k (W34), then +5k (W37); 80,807 rs lines now
- [measurement, verified] `git log --numstat --format= -- 'crates/*.rs' | awk; git ls-files 'crates/*.rs' | xargs cat | wc -l` — all-time +1,689,929 / -1,016,303; 673,626 lines survive (40%)
- [measurement, verified] `git log --since=2026-09-06 --numstat --format= -- docs | awk; same for crates; git log --since=2026-09-06 --oneline | grep -c ' docs'` — docs +39,821/-3,066 vs crates +6,094/-818; 55 of 91 commits are docs
- [measurement, verified] `ls target/reports/compiler-performance | wc -l; prefix count` — 121 files, 84 k1pp-* A/B artefacts from the W35-W40 measurement loop
- [reasoning, verified] `plan:1061-1067 phase sum` — P0 2 + P1a 2 + P1b 2 + P2a 3-4 + P2b 2 + P3a 3-4 + P3b 2-3 + R-DOC 4-8 + R-RENDER 1-2 + P4 5-7 + P5 3-4 + P6 2 + P7 0.5 + P8 1 = 32.5-43.5
- [measurement, verified] `nproc; uptime` — 8 cores; load 1.82/2.09/2.00 during this review

**Proposed plan change.** Replace the Effort table with: "Critical path ~24-34 weeks (P0 4-6, P2a 3-4, P2b 2, P3b 2-3, P4 5-7, R-DOC integration 1-2, P5 3-4, P6 2, P7 1-3, P8 1). Sum ~50-70 engineer-weeks (adds: P0 spikes and spec +9-15, runtime track R-CHG = R7+R8+R9 4-7, O1 fixture authoring 1.5-2, P7 1-3, optional O7 reference interpreter 1.5-2.5). Calendar with parallel agents ~20-28 weeks: native gates and the measurement lock serialize on one machine." Split P0 into P0a (spec, S4, S6, gate run; 2-3 wk) and P0b (S1 reduced, S2, S3, S5 instrumentation; 2-3 wk). Add: "After S1, measure its code-complete-to-gates-green time and re-baseline every estimate from that ratio, not from line counts."

**Refuter correction (high tier, medium confidence).** Lower severity to medium. Keep the concrete, checkable parts: (1) P0's 2 weeks conflicts with 'one engineer per phase' given six spikes and the spec, so either say the spikes run as parallel agents (and count them as engineer-weeks in the sum) or lengthen P0; (2) size R8+R9 separately instead of hiding them in P4; (3) make P7 more than 'days' because of the consumer ports. Label the 24-34/50-70 figures as reviewer estimates. Add the S1-ratio re-baselining rule as the main proposal rather than a new table.

---

### MIG-024 [medium] O4 is exact on two static pages (55 lines); the change model has no oracle independent of the new compiler. Add O7, an executable reference semantics

*Decisions: D21, D29, D30, D31, D32, D33, D34, D15; Plan: 4.5 'exact for dataflow parts that use none of R2-R9' (plan:616-619); 7 O4 (plan:839); 12.1 change-rule risk; Sources: M3; verdict: confirmed; severity high -> medium after refutation*

O4 is exact only where a program uses none of R2-R9. R4 (document model v2) applies to every program with a document. R8 applies to every HOLD, THEN and WHEN. The old engine passes 5 of 20 manifest scenarios. Of those, only minimal (17 lines) and hello_world (38 lines) use no HOLD/THEN/WHEN. counter_latest is a self-referential LATEST that D15 makes illegal, and flow_operators and pages use HOLD+THEN+WHEN/WHILE. I checked two probes on today's runtime: WHEN is live (D30 copies) and equal HOLD writes do not fire (D32 fires). So O4 is a divergence ledger with near-zero exact coverage. The §4.3 change rules, R7-R9 and the §4.6 identity rules are then checked only by the new checker and lowering plus hand-written scenarios, all written by the same agents from the same spec: a common-mode failure. A small executable reference semantics would give an independent oracle for exactly the parts O4 cannot cover. It would also let the owner run change_probes during P0 and read the answers in D30's vocabulary before the spec is signed.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:602-619,839` — R4 covers every document; exact only without R2-R9; O4 only where old passes
- [measurement, verified] `for f in minimal hello_world counter_latest flow_operators pages; grep -o -E '\b(HOLD|WHEN|WHILE|THEN|LATEST|SOURCE)\b' examples/$f.bn | sort | uniq -c` — minimal/hello_world: none; counter_latest: HOLD 2, LATEST 2, THEN 2; flow_operators: HOLD 2, LATEST 3, THEN 5, WHEN 3, WHILE 3; pages: HOLD 1, LATEST 2, THEN 4, WHILE 2
- [file, from notes] `docs/plans/compiler_rewrite_notes/review/measurements/oracle.md Totals` — the 5 passing scenarios: minimal, hello_world, counter_latest, flow_operators, pages
- [measurement, verified] `scratchpad M3 live.bn/p1.bn probes with target/release/boon_cli run` — WHEN live today; HOLD equal-write dedup today
- [measurement, verified] `find ~/repos/boon/crates -path '*engine*' -name '*.rs' | xargs wc -l; ~/repos/boon/crates/boon-cli/src/main.rs:3-4` — the original Boon engines are 24k-194k lines and browser-bound, and boon-cli is a stub, so they are not a cheap oracle for D30

**Proposed plan change.** Add to §7: "O7 reference semantics: `boon_spec_interp`, a test-only AST interpreter over boonc_syntax output. Dynamic values, no types, no performance. It implements no-value/last-value, HOLD (binder, D32, D33), LATEST (D15), THEN/WHEN copy, WHILE live with scope activation (L6), SKIP, FLUSH boundaries (D29), per-microstep committed snapshots (D21), lists including replace_all (D17), elements as tagged objects (D20), and effects against a scripted fake host (query supersession, command once, D31/D34/D36) with an effect log. O7 compares per-step root values, effect logs and element trees modulo ids between the reference and next on change_probes, O1 positive fixtures, small examples and scenario prefixes. It is written in P0/P1 together with the spec (~2-4 k lines, 1.5-2.5 engineer-weeks, estimate), by a different agent from the R-CHG and P4 authors." Rewrite O4 as "divergence ledger (informational; no exit criterion depends on it)".

**Refuter correction (high tier, medium confidence).** Drop the R4 argument, and say O4 is already a ledger by design. Say instead: 'plan:617 overstates exactness; the change rules' only oracle is the spec-derived scenarios, which no phase owns or sizes.' Offer a cheaper option first: in P0 the owner reviews and signs the expected traces for every change_probes scenario (written by an agent that is not an implementer) in D30 vocabulary, and they become O2 fixtures with a named owner and size. Offer O7 as the optional stronger step. Its 2-4 k line / 1.5-2.5 engineer-week estimate is optimistic once lists, elements and scripted effects are included; say so and time-box it as a spike.

**Owner question.** Should the spec get an executable reference interpreter (O7) as the oracle for the change and effect rules?
- (a) Yes: core plus the builtins the probes and small examples use, kept after cutover as the executable spec
- (b) Yes, but deleted at P8
- (c) No: rely on hand-written scenarios plus O2
Recommendation: (a). Pros: an independent oracle where O4 is empty; spec ambiguities surface in P0 rather than P4; it triages lowering bugs versus runtime bugs. Cons: a second implementation to keep in sync; a long tail of builtins; no rendering or layout coverage; common-mode risk if the same agent writes it.

---

### MIG-026 [medium] The runtime line cap has 895 lines of headroom, but R2-R5, R8 and R9 land in the capped executor and keep dual paths until P8. The plan only plans deletions for R1-R7

*Plan: P0 Process (plan:895-900); 4.5 R-table; R-DOC 'both document representations run until P8'; Sources: M3; verdict: confirmed*

I recomputed the caps with the gate's own method (git ls-files including untracked; production lines = total minus everything from the last '#[cfg(test)]' + 'mod … {' pair; test paths excluded): xtask 25,023 of 25,000 (red), playground 31,082 of 32,000, runtime+executor 41,105 of 42,000. The runtime cap counts only crates/boon_runtime and crates/boon_plan_executor. boon_plan_executor holds 142 references to DocumentPlan/materialization, so R4's executor side lands there too. The same crate must carry R2, R3, R5, R8 (with old-engine behaviour kept behind a plan semantics version until P8), R9 (effects, host ports, effect log) and the v11 document path until P8. Nothing can be deleted from the old paths before P8, so a few thousand added lines (estimate) cannot fit in 895. The playground (918 lines headroom) also has to run both document representations and the engine selector.

Evidence:
- [measurement, verified] `python3 scratchpad/M3/loc.py (replica of crates/xtask/src/architecture.rs:2823-2890)` — {'playground': 31082, 'runtime_executor': 41105, 'xtask': 25023}
- [file, verified] `crates/xtask/src/architecture.rs:8-10,130-150,2843-2846` — caps 32,000 / 25,000 / 42,000; runtime cap prefixes boon_runtime and boon_plan_executor only
- [measurement, verified] `rg -c 'DocumentPlan|document_plan|DocumentTemplate|Materializ' crates/boon_plan_executor/src crates/boon_document/src crates/boon_native_playground/src` — plan_executor 142, boon_document 126, playground 1
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:613,900,994` — R8 dual semantics until P8; compensating deletions planned only for R1-R7; both doc paths until P8

**Proposed plan change.** Replace 'Plan compensating deletions for R1-R7' with: "Measure the growth of R2-R5, R8, R9 and R-DOC in S1/S3. The owner decides whether the runtime and playground caps get a temporary allowance for dual-path code, recorded in architecture.rs with an expiry at P8, when the cap returns to its current value or lower."

**Refuter correction (low tier, high confidence).** Severity as stated is reasonable. One caveat for the owner: the estimate of 'a few thousand added lines' for R2-R5/R8/R9/R-DOC is admittedly a guess, not measured -- the proposed fix (measure growth in S1/S3 before deciding a) vs b) vs c)) correctly treats it as unmeasured, so keep the recommendation conditional on that spike rather than asserting the cap will definitely be blown.

**Owner question.** How should dual-path runtime code fit under the architecture line caps until P8?
- (a) A temporary cap allowance recorded in architecture.rs, ratcheted back at P8
- (b) Put next-only runtime code in a new crate outside the capped prefixes
- (c) Find compensating deletions now
Recommendation: (a). (b) meets the cap only on paper, and (c) has no realistic source of thousands of deletable lines while the old engine must keep working.

---

### MIG-027 [medium] P0's archive and 'evidence lane' deletion break xtask and gate 0 unless they land with budgets v4. Deleting compiler_allocator.rs alone clears the cap

*Plan: P0 Process (plan:893-897); 9.2 Documents; 9.3 (budgets v4 lands in P1b); Sources: M3; verdict: confirmed*

budgets/compiler.toml:2 names docs/plans/BOON_COMPILER_PERFORMANCE_PLAN.md as owner_plan, and compiler_performance.rs:2373-2376 fails if that file is missing. So archiving the plans in P0 breaks verify-compiler-performance until budgets v4 lands in P1b. validate_budget also requires a non-empty evidence_producer different from the product producer, so removing the evidence lane also means editing the v3 budgets. Gate 0 reads compiler_performance.rs and compiler_interactions.rs and requires exactly 1 and 3 env_remove(BOON_KERNEL_EXPERIMENTAL_PARALLEL) calls, so editing or deleting those files turns the handoff architecture gate red. Deleting compiler_allocator.rs (919 lines) alone clears the 23-line overage. The re-point list in §9.2 also misses files: rg finds BOON_COMPILER_PERFORMANCE_PLAN/TENS/REQUIREMENT_AGGREGATION referenced from README.md, AGENTS.md, budgets/compiler.toml, TYPE_INFERENCE, OUT_PARAMETERS, CIRCUIT_SIMPLIFICATION, GOAL_PROMPT, PERSISTENCE, steps.md, LANGUAGE_FOUNDATIONS, PACKED_DATA, FORMAL_VERIFICATION and an evidence JSON.

Evidence:
- [file, verified] `budgets/compiler.toml:2,9,12` — owner_plan, evidence_producer, evidence_kind
- [file, verified] `crates/xtask/src/compiler_performance.rs:2365-2385` — owner plan must exist; evidence producer checks
- [file, verified] `crates/xtask/src/architecture.rs:218-279` — gate 0 reads compiler_performance.rs/compiler_interactions.rs; env_remove counts 1 and 3
- [measurement, verified] `wc -l crates/xtask/src/compiler_*.rs` — allocator 919, interactions 2,612, performance 2,732, producer 330, work_sample 435
- [measurement, verified] `rg -l 'BOON_COMPILER_PERFORMANCE_PLAN|BOON_COMPILER_TENS_OF|REQUIREMENT_AGGREGATION' --glob '!docs/plans/compiler_rewrite_notes/**' --glob '!docs/plans/BOON_COMPILER_*' .` — 13 files incl. README.md:40-41 (TENS plans)

**Proposed plan change.** Change P0 to: "Delete compiler_allocator.rs and its main.rs dispatch; nothing else in xtask." Add to P1b: "One change set lands budgets v4, deletes the evidence lane and the old verifiers, updates architecture.rs:218-279, archives the 12 plans and 44 evidence files, and re-points the 13 referencing files listed by `rg -l`." Until then BOON_COMPILER_PERFORMANCE_PLAN.md stays as the budgets owner_plan, marked history.

**Refuter correction (low tier, high confidence).** Correct as stated. The proposed narrowing of P0 to 'delete compiler_allocator.rs and its main.rs dispatch; nothing else in xtask' plus deferring the archive/re-point/budgets-v4 bundle to one P1b change set is the right shape of fix; make sure that change set is also explicitly gated on updating architecture.rs:218-280's file/count checks if compiler_performance.rs or compiler_interactions.rs are touched, since those exact-count checks are brittle to any refactor of the env_remove call sites.

---

### MIG-029 [medium] AGENTS.md block: wrong line references, three contradictions with rules that stay, one rule that cannot be followed before P1b, and missing rules

*Plan: 9.1 (plan:1076-1130); Sources: M3; verdict: confirmed*

(1) plan:1078 says 'Lines 10-14, the console/CPU authority paragraph'. That paragraph is AGENTS.md:8-12, and line 14 is 'Do not commit or push unless the user explicitly asks', so a literal edit could drop the commit rule. (2) 'Boon syntax is frozen' contradicts D11/D18, which change what parses; it should say syntax changes only by owner decision. (3) 'Delete superseded code in the same change set' contradicts keeping the old engine, the v11 document path and R8's old-semantics version until P8. (4) The block's 'change the old pipeline only to keep it building…' plus the compat rule plus AGENTS.md:16-19 (no Boon workaround, which stays) together give no legal action when the old engine blocks a migration. The Theme draft's Light/* split is exactly such a workaround. (5) 'Develop with cargo xtask compiler-ab' cannot be followed until harness v4 exists (P1b). (6) The calendar estimate assumes 2-3 parallel agents per phase, while AGENTS.md:46 allows subagents only when the user asks. Missing rules: Boon probes under docs/ must be .bn.txt, because xtask language_surface parses every .bn in the workspace; parallel agents must not build or test while the measurement lock is held; a .expect change must cite a rule id (no blessing); an old-engine limitation is logged, never worked around.

Evidence:
- [file, verified] `AGENTS.md:8-14` — console paragraph 8-12; commit rule 14
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1078,1086-1092,1108,1120` — line refs; old-pipeline rule; syntax frozen; compiler-ab; delete superseded
- [file, verified] `AGENTS.md:16-19,46-50` — fix-engine rule; subagents only when permitted
- [file, verified] `crates/xtask/src/language_surface.rs:466` — scans workspace files with extension bn
- [file, from notes] `docs/plans/compiler_rewrite_notes/examples.md:483` — Light/* kept out of tokens because the old backend cannot lower it

**Proposed plan change.** In §9.1 change 'Lines 10-14' to 'Lines 8-12 (line 14, the commit rule, stays)'. Change 'Boon syntax is frozen' to 'Boon syntax changes only by owner decision (D11 and D18 are settled)'. Append to 'Delete superseded code…': 'except the old engine, the document v11 path and old-plan semantics, which are deleted at P8'. Add: "- An old-engine limitation never justifies changing Boon source: record it in tests/compiler/divergences.toml, and the example change lands when that example flips to next." Add: "- Until harness v4 exists, make no performance claims." Add: "- Parallel agents share one machine: no builds, tests or timing runs while the measurement lock is held." Add: "- Every `.expect` change cites a rule id; there is no bless mode." Add: "- Boon files under docs/ use the `.bn.txt` extension; scratch probes live outside the repo."

**Refuter correction (low tier, medium confidence).** Split this finding's confidence: item (1), the line-range error, is fully confirmed and should be fixed regardless (trivial, unambiguous). Items (2)-(6) are directionally right but softer -- rephrase the proposed fix for 'Boon syntax is frozen' to something like 'Boon syntax changes only through D11/D18 and future owner decisions, never ad hoc' rather than asserting outright contradiction, and for 'delete superseded code' add the R8/R9/P8 carve-out only if a spike shows agents actually try to delete the dual-path code early (this may be self-evident enough in context that no agent would misread it, in which case downgrade this sub-item from 'contradiction' to 'ambiguity worth a clarifying clause').

---

### MIG-030 [medium] No CI and no native-gate report since 2026-08-23, so the compatibility rule has no known-green baseline

*Plan: 7 'The repo has no CI' (plan:847); P0 'Run all 7 handoff gates once'; 5.1 compat rule; Sources: M3; verdict: confirmed*

The compat rule requires the old engine to pass each example's gate, but the last report in target/reports/report-v2 is architecture.json from 2026-08-23. No counter-dev, todomvc-physical, cells, novywave, persons-pro or negative report exists. The architecture gate is red today (xtask 25,023/25,000). In boon_cli, todo_mvc_physical and persons_pro fail before their first scenario step and cells fails check (from the notes). If a gate is red at P0, the rule is either vacuous or impossible for that example, and the plan does not say which. With no CI and gates that need the launch-scoped seat, gates run only when someone remembers to run them, and regressions from parallel agents surface late.

Evidence:
- [measurement, verified] `ls -la --time-style=long-iso target/reports/report-v2/` — only architecture.json, 2026-08-23 14:07
- [measurement, verified] `ls -a .github; git ls-files | grep -i -E 'ci\.yml|workflow'` — no CI configuration
- [file, verified] `docs/architecture/native_gpu_handoff_manifest.json gates[0..6]` — architecture, counter-dev, todomvc-physical, cells, novywave, persons-pro, negative
- [file, from notes] `docs/plans/compiler_rewrite_notes/process_alternative.md:352` — cells check fails; confirm the playground path before treating the old engine as the baseline

**Proposed plan change.** Add to P0: "Run the 7 gates and record them. An example whose old-engine gate is red at P0 is exempt from the compatibility rule; it is verified only on next and flips when its next gate passes." Add to §7: "A local nightly job (systemd user timer) runs `cargo xtask compiler-checkpoint --nightly` and the native gates in the launch-scoped workspace, while holding the measurement lock, and writes a one-line summary per night. This is the plan's CI."

**Refuter correction (low tier, high confidence).** The plan already partially addresses the 'no baseline' framing via P0's 'run all 7 gates once and record which are red,' so don't present this as though the plan does nothing about it -- narrow the finding to what's actually missing: (a) the plan never states what the old-engine compatibility rule means for an example whose native gate is ALREADY red at P0 (vacuous vs. blocking), and (b) there is no repeated/automated re-verification after that one-time P0 recording, so gate regressions from parallel agents between P0 and the next manual run go undetected. Both proposed additions (exemption rule for already-red gates; a local nightly-timer pseudo-CI) remain valid and actionable with that narrower framing.

---

### MIG-013 [low] Self-referential LATEST: 5 real sites, 2 of them in the persons-pro gate; the plan lists 2

*Plan: §5.1 Self-referential LATEST row; Sources: M1; verdict: confirmed; severity medium -> low after refutation*

§5.1 lists counter_latest and interval_latest. The scan finds 5 LATEST blocks that name their own field with no HOLD binder:
- `counter_latest.bn:8`
- `interval_latest.bn:6`
- `novywave/RUN.bn:3020` `value_format`
- `persons_pro/RUN.bn:333` `publish_request_sequence` (`publish_request_sequence + 1`)
- `persons_pro/RUN.bn:456` `mode` (`mode |> WHEN { Light => Dark … }` inside a THEN)

Both persons_pro sites are in a handoff-gate example, and neither the plan nor the notes list them. TodoMVC `RUN.bn:132` `title: LATEST { title, title_to_update }` is not a self-reference under D14: its own name is hidden, so `title` copies the `new_todo(title)` parameter. Fixing it is a D14 behaviour change, not a LATEST fix.

Evidence:
- [measurement, verified] `census.py class latest_self_reference` — 6 .bn hits including TodoMVC title, +1 Rust
- [file, verified] `examples/persons_pro/RUN.bn:331-336, 454-460`
- [file, verified] `examples/todo_mvc_physical/RUN.bn:132-135` — inside FUNCTION new_todo(title, …)
- [file, verified] `docs/plans/compiler_rewrite_notes/examples.md:20` — the notes list counter_latest, interval_latest and value_format only

**Proposed plan change.** Change the §5.1 self-referential LATEST cell to: 'counter_latest.bn:8, interval_latest.bn:6, novywave RUN.bn:3020 value_format, persons_pro RUN.bn:333 publish_request_sequence and :456 mode (persons-pro gate).' Add a note: 'TodoMVC RUN.bn:132 is a D14 outer copy, not a self-reference.' Add the two persons_pro sites to the persons-pro gate's old-engine compatibility check.

**Refuter correction (low tier, medium confidence).** Reframe as 'filling in the census the plan already asks for' rather than 'the plan lists 2'; keep the concrete site list and the persons-pro-gate flag, which are the valuable part.

---

### MIG-018 [low] Wrong or empty §5.1 cells: collections in HOLD are 0, Http/request and new_todo_focused point at the wrong files, and several counts are off

*Plan: §5.1 rows; S4; Sources: M1; verdict: confirmed*

Measured corrections:
- **Collections in HOLD (D9):** 0 sites. The scanner flagged 7 HOLDs; all hold scalar state and only use `List/` operations in their bodies. No HOLD's initial value or body builds a LIST, SET or MAP.
- **`Http/request`:** the sites are `outbound_http_effect.bn:7` and `server_effect_chain.bn:11`, plus 2 in Rust. host_service_effects has none.
- **Host value mirrored in a HOLD:** `new_todo_focused` is in `examples/todomvc.bn:100`, not todo_mvc_physical. The only other mirror is `fjordpulse/Client/RUN.bn:499 focus_state`. 7 more HOLDs end an edit on blur, which is business logic and stays.
- **One-input LATEST:** 91 (87 in bytes_*), against ~80.
- **`List/map(new:) |> List/latest()`:** 28, against 27.
- **Pony precedence:** 12 mixed-operator expressions and 0 comparison chains, against ≤19.
- **`event` vs `events`:** `event: [` is 142 and `events: [` is 147, of which `element: [events:` is 54. The plan's 59 is undefined.
- **`hovered:`:** 243 lines, of which 96 are `hovered: element.hovered`, against 137.
- **LATEST with two starting arms:** at most 15 (NovyWave 13); a hand check of 6 found 1-2 real ones. This class and 'collections in state' need update kinds, so S4 cannot measure them exactly.
- **Confirmed as stated:** Theme 94 calls; NovyTheme 84/60; LATEST with a constant arm 41; catch_cycle 1 (cells/formula.bn:39); WHEN/WHILE 1,418/45 in .bn.

Evidence:
- [measurement, verified] `census.py classes collection_in_hold, http_request, host_value_mirrored_in_hold, latest_one_input, list_latest, mixed_operators, comparison_chain, latest_two_starting_arms, latest_constant_arm; summary key_* and NovyTheme/*`
- [measurement, verified] `git grep -c -P '\bevent: \[' -- '*.bn' (142); '\bevents: \[' (147); 'element: \[events:' (54); 'hovered: ' (243)`
- [measurement, verified] `git grep -n 'Http/request' -- '*.bn' => examples/server_effect_chain.bn:11, examples/outbound_http_effect.bn:7`

**Proposed plan change.** Apply the corrected table in docs/plans/compiler_rewrite_notes/review/census/RESULTS.md. In particular:
- Collections in HOLD: '0 today; the diagnostic stays; no P3 work.'
- Http/request: 'outbound_http_effect.bn, server_effect_chain.bn.'
- Host mirrors: 'todomvc.bn:100 new_todo_focused, fjordpulse Client/RUN.bn:499.'
- One-input LATEST: 91.
- Precedence: '12 mixed, 0 chains.'
- Give the regex behind every contract count, so P2b can reproduce it.

In S4, mark 'two starting arms' as needing update kinds, to be measured by the P2b checker and not by the scanner.

**Refuter correction (low tier, high confidence).** Correct the new_todo_focused line number to :99, not :100 (trivial, does not change the substance).

---
