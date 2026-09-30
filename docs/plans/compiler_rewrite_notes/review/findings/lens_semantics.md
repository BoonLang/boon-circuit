# Lens 2: language semantics: all findings

Generated from the verified finder output for `../REVIEW.md`. Severity is the severity after refutation; each finding keeps its refuter's corrections.

### SEM-007 [critical] WHEN over a constant or start-valued selector has no defined start evaluation; view WHENs render nothing and constant-selector WHEN reads as an error

*Decisions: D25, D30, D31, D32; Plan: §4.3 Change rules (lines 380-393), D25 (line 94), R8 (line 613); Sources: S1, S5; verdict: confirmed*

§4.3 says a copy context (THEN body, WHEN arm) 'runs once each time the input updates', D32/§4.3 say start and activation are not updates, R8 says copy ops 'evaluate only when the input updates', and §4.3:386-387 says 'THEN or WHEN over something that never updates is an error'. Read literally: (a) `filter |> WHEN { All => 32 … }` where `filter` is bound to a literal at the call site (TodoMVC filter_button, theme_name_button, every `Theme/material(of: Surface)` dispatcher) is a 'never runs' error; (b) a WHEN over a HOLD (`mode |> WHEN { Light => …, Dark => … }`, `todo.completed |> WHEN {…}`) has no value until the selector's first later update, so the document renders nothing (L3c) at start. The plan elsewhere assumes WHEN results have a start value (D30 lists THEN outputs but not WHEN outputs among 'first value later'; the stale-copy hint bullet speaks of 'a WHEN over a value with a start value'; D25 presupposes WHEN over compile-time constants is legal) but never states when the arm is first evaluated. If activation evaluation is added, it must also say that a command (D31) inside such an arm does not run at activation.

Also (S1-03): §4.3: a copy body 'runs once each time the input updates' and 'Start, restore, hot reload ... are not updates'. Nothing says that a WHEN or THEN whose input has a value from the start is evaluated at activation. Read literally, `visible: filter |> WHEN { All => True  Active => completed |> Bool/not()  __ => completed }` has no value until `filter` first updates, so under L3c the document shows nothing at start - which cannot be intended for the 1,400 WHENs in the examples. Today the two copy constructs already differ observably: WHEN over a HOLD has a value at start (probe pG: visible=True at init) while THEN over a HOLD does not (probe pC: doubled_then absent at init, doubled_live=10). If the plan adds an activation copy for WHEN, D31 ('commands ... allowed only in copy contexts (THEN bodies and WHEN arms) ... never run on start') conflicts: a command in an arm over a start-valued selector would run at activation. The update-kind inference ('has a value from the start' vs 'first value later') is likewise unspecified for WHEN/THEN outputs.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:384-387` — Copy contexts: body runs once each time the input updates; 'THEN or WHEN over something that never updates is an error (never runs)'.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:380-383` — Start, restore, hot reload, entering a WHILE arm and creating a row are not updates.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:613` — R8: copy ops for THEN bodies and WHEN arms (evaluate only when the input updates); activation is not an update.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:94` — D25: every WHEN arm's state exists even when a compile-time constant rules the arm out; compiler may warn about unreachable arms — presupposes WHEN over constants is legal.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:99,410-411` — D30 lists SOURCE payloads, effect results, THEN outputs as 'first value later' (not WHEN); hint bullet: 'a WHEN over a value with a start value'.
- [file, verified] `examples/todo_mvc_physical/RUN.bn:801-803,808-819,838-842,1008-1012,1035-1041,1062-1068,1073-1079` — WHEN over FUNCTION parameters bound to literal tags at every call site.
- [file, verified] `examples/todo_mvc_physical/Theme/Theme.bn:1-12` — `of |> WHEN {…}` dispatcher; all callers pass a literal `of` (RUN.bn:350,464,548,749).
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/migration/todo_theme_new/Professional.bn.txt:21,27,46` — The plan's own theme draft is built from `mode |> WHEN { Light => …, Dark => … }` over a HOLD-derived value and relies on it having a start value.
- (6 more evidence entries in the finder output)

```boon
-- examples/todo_mvc_physical/RUN.bn:808-819; `filter` is a literal at 801-803
FUNCTION filter_button(events, filter) {
    ... width: filter |> WHEN { All => 32, Active => 56, Completed => 88 }
-- plan §4.3 as written: `filter` never updates → error 'never runs' (or no width at start)
-- today: 32 from the start (LANGUAGE_SEMANTICS.md:409-416)
-- developer expects: 32 from the start, never changes
```

```boon
-- examples/todo_mvc_physical/RUN.bn:1111-1114
text: PASSED.theme_options.mode |> WHEN { Light => TEXT { Dark mode }, Dark => TEXT { Light mode } }
-- plan as written: mode has a value from the start but the arm runs only on the first toggle → button text empty until first press
-- today and expected: 'Dark mode' at start
```

```boon
-- S5 WHEN over a value with a starting value (probe pG)
store: [
    press: SOURCE
    toggle: SOURCE
    filter: All |> HOLD filter { press |> THEN { Active } }
    completed: False |> HOLD completed { toggle |> THEN { completed |> Bool/not() } }
    visible: filter |> WHEN { All => True  Active => completed |> Bool/not()  __ => completed }
]
-- plan: ambiguous - 'start is not an update' and the arm 'runs once each time the input updates' => no value at start
-- today: visible=True at start (live WHEN); after press then toggle: True (live)
-- expected: True at start (a copy taken at activation, not an update); after press then toggle: stays False (copy) - hover warns 'use WHILE'
```

**Proposed plan change.** Add to §4.3 'Copy contexts': "A WHEN whose input has a value (a constant, or a value from the start) is evaluated once when its scope activates and again on every update of the input. The activation evaluation is not an update (D32) and does not run commands (D31): an arm containing a command has no value until the input updates. The result has a value from the start; if the input never updates the result never updates, and unreachable arms are a warning (D25), not an error. THEN never evaluates on activation, and THEN over a never-updating input is an error." Delete 'or WHEN' from §4.3:386-387 and align R8 (§4.5:613). Add a change probe (`const_when.bn.txt`, `hold_when_start.bn.txt`) under compiler_rewrite_notes/change_probes/ and an O5 scenario asserting the start value.

[also from S1-03] Add to §4.3: 'Activation copy. A WHEN whose selector has a value from the start is evaluated once at activation (and at row creation and on entering a WHILE arm). That copy is not an update: nothing downstream fires and no command is issued. A WHEN over a value from the start therefore has a value from the start; a THEN never has: it runs only on updates of its input. Commands are allowed in THEN bodies and in WHEN arms whose selector gets its first value later (SOURCE payloads, effect results, THEN outputs); a command in a WHEN arm over a start-valued selector is an error with the fix-it "use THEN".' Extend the update-kind pass accordingly and add S5-S7 as checker/runtime tests.

**Refuter correction (high tier, high confidence).** Keep critical, since R8's explicit 'evaluate only when the input updates; activation is not an update' would be implemented literally. Merge the activation half of SEM-008 into this finding (same missing rule). Keep the proposed §4.3 text. Also fix D30's wording 'the body runs once each time the input updates' so it adds 'and once at activation for WHEN when the selector has a value (not an update)'. Put the command-in-activated-WHEN-arm rule (error or no value until first update) to the owner as part of the same question, since the current proposal contains two slightly different variants of it (D31 conflict).

**Owner question.** Does a copy construct over an input that has a value from the start evaluate at activation?
- (a) WHEN yes (an activation copy that is not an update; commands only in THEN or in WHEN over first-value-later selectors), THEN no.
- (b) WHEN and THEN both evaluate at activation, with commands suppressed on the activation copy.
- (c) Neither; every view over a start-valued selector must be WHILE.
Recommendation: (a). It matches what today's runtime already shows for WHEN vs THEN, keeps 'activation is not an update' and D31 intact with one static rule, and keeps WHEN usable for views.

---

### SEM-001 [high] Cycle rule is ill-defined: rejects the canonical counter and admits an unbounded loop through a HOLD's piped input; FIRE vs COPY edges never classified

*Decisions: D9, D33, D32; Plan: §4.3 'Change rules' bullets 'Fire edges never close a cycle' and 'Cycles (D9)'; D9; D33; Sources: S1; verdict: confirmed; severity critical -> high after refutation*

§4.3 says a legal cycle 'must pass through a HOLD's committed-value read, a collection update or a stateful builtin' and that 'a THEN input, WHEN selector or update candidate on the cycle is an error'. Neither half is a usable rule under D32/D33. (i) Under D32 every read of a HOLD from outside its own body is a FIRE edge (the reader fires whenever the HOLD fires), so `a: b |> HOLD a { press |> THEN { a + 1 } }  b: a + 1` passes through a HOLD's committed-value read and is still an unbounded loop (a fires -> b fires -> piped reset of a fires -> ...); today `boon_cli check` passes it and `run` dies at dispatch (probe pO). (ii) The canonical counter `count: 0 |> HOLD count { press |> THEN { count + 1 } }` has an update candidate on its only cycle, so the literal sentence rejects it. The root cause: the plan never says which edge of which construct is a FIRE edge (an update of the source re-runs the reader) and which is a COPY edge (the reader only samples the committed value when something else runs it), although the change rules, the cycle rule, LATEST ties, the L6 placement rule and R8's 'copy ops' all rest on that classification. Termination is guaranteed exactly when the graph of FIRE edges is acyclic; the only non-fire edges the plan implies are the HOLD binder read inside its own body ('its own writes do not re-trigger it'), THEN-body and WHEN-arm reads other than the input ('copies the current values of everything else it reads'), and stateful-builtin state reads. The operational model I reconstructed (verify each row against owner intent): FIRE = plain/derived operand reads (incl. record fields, TEXT interpolation, LIST items, collection reads such as List/count and row fields under List/map, document slots), THEN input, WHEN selector, WHILE selector, WHILE arm reads while selected, HOLD piped input (D33 reset), HOLD body result into the state, reads of a HOLD/collection from outside, LATEST arms, List/append item and List/replace_all with, SOURCE payload reads, effect-result reads, live-query arguments; COPY = HOLD binder read in its body, THEN-body reads, WHEN-arm reads, command/copy-context effect arguments; UNSTATED = collection reads, row-field reads, stateful-builtin inputs after D26, FUNCTION argument/body reads (F7), BLOCK locals, nested THEN/WHEN inside copies (F4). Proof sketch: with G_fire acyclic, give each node rank = longest FIRE path from the tick's occurrence; a tick performs at most (number of HOLD/collection commits on the longest path)+1 microsteps and each node fires at most once per rank it is reachable at; ranks are per definition, so list rows and dynamic instances do not change the bound; effect results arrive in later ticks; LATEST/SKIP only remove fires; WHILE arm switching only changes which FIRE edges are active, all of which are in G_fire. Counterexamples when G_fire has a cycle: pO above, and `a: 0 |> HOLD a { b |> THEN { a + 1 } }  b: a |> THEN { a }` (THEN input both ways).

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:405-408` — 'Fire edges never close a cycle ... a THEN input, WHEN selector or update candidate on the cycle is an error'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:78` — D9: 'legal only through a HOLD update, an event-driven collection update or a stateful builtin'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:381-384,396-398` — D32 'a derived value fires at most once per step when any input fires'; D33 'its own writes do not re-trigger it' - the only non-fire read the plan names
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pO_piped_cycle.bn "[('init',None,{}),('p1','store.press',{})]" store.a,store.b ; target/release/boon_cli check pO_piped_cycle.bn` — check: 'pass: MachinePlan 11.0, 4 operation(s)'; run: 'evaluation failed: root state 0 has no private presence' - today's compiler accepts the piped-input cycle and the runtime cannot execute it
- [file, verified] `examples/counter.bn:27-34` — canonical counter whose cycle contains an update candidate
- [source, verified] `~/repos/boon/docs/language/HOLD.md:41-61` — original 'Non-Self-Reactive Semantics': the binder read is the non-reactive edge
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:520-527` — today's rule: cycles through WHILE/pure expressions rejected unless a HOLD is in the cycle - also written in terms of nodes, not edges

```boon
-- S1 piped-input cycle (probe pO)
store: [
    press: SOURCE
    a: b |> HOLD a { press |> THEN { a + 1 } }
    b: a + 1
]
-- plan: 'passes through a HOLD's committed-value read' -> literally legal; under D32+D33 it is an infinite fire loop
-- today: check passes; run fails 'root state 0 has no private presence'
-- expected: positioned error 'cycle of updates: a -> b -> a (HOLD input)'
```

```boon
-- S2 canonical counter (examples/counter.bn:27-34)
count: 0 |> HOLD count { press |> THEN { count + 1 } }
-- plan: 'an update candidate on the cycle is an error' -> literally rejected
-- today: works (counter.scn passes)
-- expected: legal; the THEN-body read of `count` is the copy edge that breaks the cycle
```

```boon
-- S3 all-fire cycle through two THEN inputs
store: [
    press: SOURCE
    a: 0 |> HOLD a { b |> THEN { a + 1 } }
    b: a |> THEN { a }
]
-- plan: error (THEN input on the cycle) - correct, but only because both edges are THEN inputs
-- expected: error listing the path a -> b -> a
```

**Proposed plan change.** Replace the two cycle bullets in §4.3 with: (1) an edge table that classifies every construct's reads as FIRE or COPY (the table in the claim, corrected by the owner where intent differs); (2) the rule 'The graph of FIRE edges must be acyclic. A cycle is legal only if it contains a COPY edge: a HOLD's binder read inside its body, a THEN-body or WHEN-arm read, or a stateful-builtin state read. Every other cycle - including one closed through a HOLD's piped input, a derived read of a HOLD, a WHILE arm, a LATEST arm or a collection read - is a positioned error listing the path.' (3) Rewrite D9's 'legal only through a HOLD update...' as 'legal only through a copy read of a HOLD, a collection or a stateful builtin'. Add S1-S3 as checker tests (S1 and S3 must be errors, S2 legal) and make the edge table the input to the Phase A Tarjan pass (edges labelled FIRE/COPY; SCCs computed over FIRE edges only).

**Refuter correction (high tier, high confidence).** Severity critical -> high. Reword the claim: the two cycle texts (405-408 vs 421-423) disagree and neither defines FIRE vs COPY. The literal 405-408 rejects the counter; the literal 421-423 admits the piped-input loop pO. Fix the proposed rule so it does not contradict D9: 'The FIRE-edge graph must be acyclic, and every dependency cycle must contain a D9 edge: a HOLD binder read inside that HOLD's body, an event-driven collection update, or a stateful-builtin state read. A HOLD's piped input (D33 reset) is a FIRE edge and never closes a cycle. A cycle whose only COPY edge is a THEN-body or WHEN-arm read outside a HOLD body is an error (D9: not THEN bodies).' If the reviewer wants THEN-body copies to close cycles, that is an owner question against D9, not a wording fix. Keep S1-S3 as checker tests. Also resolve SEM-002(3) with the same table: TodoMVC completed <-> all_completed goes through a THEN-body copy of all_completed (RUN.bn:138-140) and the library Bool/toggle HOLD's piped input. Under the corrected rule it must be legal through the Bool/toggle HOLD's body binder or rejected, and the plan must say which.

---

### SEM-004 [high] R8/D21 microstep snapshots glitch: derived values fire more than once per tick, once with a state that never exists

*Decisions: D21, D32, D31; Plan: §4.3 'Updates' bullet; §4.5 R8 'follow-up microsteps within a tick, each reading its own committed snapshot'; D21; D31; D32; Sources: S1; verdict: confirmed*

'A derived value fires at most once per step' and 'follow-up microsteps within a tick, each reading its own committed snapshot' together mean a node with inputs at different fire ranks fires once per rank. For `c: 0 |> HOLD c { a |> THEN { a * 10 } }  x: a + c`, one press gives, in microstep 1, x = a_new + c_old (a state that never exists: c is always a*10 after settling) and, in microstep 2, x = a_new + c_new. Under D32 both firings reach `x |> THEN { ... }`, and under D31 a command in that body runs twice per press, once with the glitch value; a HOLD candidate `x |> THEN { x }` writes twice. Today's runtime already fires x twice per press (probe pX: x_fires 2 per press) but every firing is consistent because HOLDs commit immediately (probe pX: consistent 2 per press); D21 removes the immediate commit and thereby introduces the inconsistent firing. No oracle (O1-O6), spike (S5) or change_probe scenario tests glitch freedom, so this would ship silently. It is the classic delta-cycle glitch; every glitch-free FRP system (Elm signals, Reflex, spreadsheets) avoids it by evaluating each node once per transaction in topological order, which the acyclic fire graph of F1 makes possible.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:381-384` — 'A derived value fires at most once per step when any input fires'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:613` — R8: microsteps each reading its own committed snapshot
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pX_glitch.bn "[('init',None,{}),('p1','store.press',{}),('p2','store.press',{})]" store.a,store.c,store.x,store.x_fires,store.x_seen,store.consistent` — p1: a=1 c=10 x=11 x_fires=2 consistent=2; p2: a=2 c=20 x=22 x_fires=4 consistent=4 -> today double-fires per press but every firing sees c == a*10
- [reasoning, verified] `microstep trace under R8` — ms0: press -> a-body -> commit a=1. ms1: a fired -> c-body fires (reads a=1 -> candidate 10) and x fires reading S1={a:1,c:0} -> x=1 (glitch) -> x |> THEN runs. ms2: c committed -> x fires again with {a:1,c:10} -> x=11.
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:203-210` — Model C's tick description (source of the plan's microsteps) claims only bounded depth, never glitch freedom (taken from notes)

```boon
-- S4 diamond through a HOLD (probe pX)
store: [
    press: SOURCE
    a: 0 |> HOLD a { press |> THEN { a + 1 } }
    c: 0 |> HOLD c { a |> THEN { a * 10 } }
    x: a + c
    x_fires: 0 |> HOLD x_fires { x |> THEN { x_fires + 1 } }
    consistent: 0 |> HOLD consistent {
        x |> THEN { c == a * 10 |> WHEN { True => consistent + 1, False => consistent } }
    }
]
-- plan (R8 microsteps): per press x_fires +2, consistent +1, x passes through 1 then 11
-- today: per press x_fires +2, consistent +2 (immediate commits, no glitch)
-- expected by a developer: x_fires +1, consistent +1, x never equals 1; a command in `x |> THEN` runs once with 11
```

**Proposed plan change.** Change §4.3 'Updates' and R8 to rank scheduling over the acyclic fire graph: 'Each node evaluates at most once per tick, in topological order of FIRE edges (rank = longest FIRE path from the tick's occurrence; HOLD and collection commits are ordinary ranked nodes). A node evaluates only after every FIRE input has settled for the tick. A COPY read sees the value committed at a strictly lower rank in this tick, otherwise the pre-tick value.' This preserves D21's guarantees (no declaration-order dependence; the swap `x: 1 |> HOLD x { press |> THEN { y } }  y: 2 |> HOLD y { press |> THEN { x } }` still gives (2,1) because both bodies are rank 1 and each reads the other's pre-tick value), removes microsteps as a user-visible concept, makes 'fires at most once per step' into 'at most once per tick', and gives LATEST a causal order (F6). Add S4 to the O-oracles and to spike S5's exit criteria (x_fires +1 per press, consistent +1, no observation of c != a*10).

**Refuter correction (high tier, high confidence).** Tighten the claim: the glitch reaches THEN bodies and commands. The document is spared if rendering waits for quiescence (notes change_and_effects.md:407), so say that to avoid overstatement. Also point out that SEM-002(1) and SEM-003(i) depend on the same 'step' definition. One decision about rank or microstep settles the LATEST tie rule, the D33 reset precedence and glitch freedom together, so the owner question should be phrased as that one decision. Keep the proposed rank-scheduling text and add probe pX to the S5 exit criteria.

**Owner question.** How should reactions to same-tick HOLD writes be ordered?
- (a) Rank scheduling: one evaluation per node per tick in topological order of the fire graph; copy reads see lower-rank commits of this tick, else pre-tick values.
- (b) Keep R8 delta-cycle microsteps and document the glitch (VHDL-style), plus a checker warning when a THEN/command input has inputs at different ranks.
- (c) Reactions to a HOLD write run in the next tick (Lustre 'pre'); intermediate states are rendered and no microsteps exist.
Recommendation: (a). It is the only option that is both consistent ('everything is change', one evaluation per change) and strict (no observable intermediate states); it costs one static rank per node, which the Phase A DAG already yields.

---

### SEM-008 [high] D12's inferred durable clause is undefined, and WHEN results are not recomputable at restore unless activation evaluation is spelled out

*Decisions: D12, D30, D32; Plan: D12 (plan:81), D30 (plan:99), D32 (plan:101), §4.3 Change rules (plan:378-416, esp. 380-388 and 413-416), §4.5 Persistence (plan:568-578), O2/persistence oracle (plan:837, 843), S1 (plan:908); Sources: S4; verdict: confirmed; severity critical -> high after refutation*

The plan says a copy context 'runs once each time the input updates' and that 'start, restore, hot reload ... are not updates' (plan:380-386, D32). Taken literally, after a restart `store.selected_filter |> WHEN { All => …, Active => …, Completed => … }` (RUN.bn:87-91) and every theme-token WHEN over the restored `theme_options.name` HOLD (RUN.bn:310) have no value until the user clicks again, so either the document is blank after restore or every such WHEN result must be stored. D30 only lists 'SOURCE payloads, effect results, THEN outputs' as values that get their first value later (plan:99) and plan:413 talks about 'a WHEN over a value with a start value', which implies WHEN is evaluated at activation while THEN is not, but nowhere is that written as a rule, and the D12 clause 'the last value of an app-computed value that code reads outside its own update' is never defined. Two readings: (R1) activation evaluation exists: every live expression and every WHEN whose selector has a value is evaluated (copied once) at start/restore/hot reload/row creation/arm entry; THEN bodies, commands and occurrences do not run. Then the only unrecomputable ('history') values are THEN outputs, WHEN outputs over selectors with no start value (key_down.key), occurrence payloads, command results and every LATEST (its 'most recently updated arm' is history even when all arms are recomputable), and D12 stores such a node only when a reader is not its own copy consumer (a live read, or a THEN/WHEN whose input is something else). Applied to TodoMVC this yields exactly today's measured set (selected_filter, new_todo_text, todos + title/completed/editing/edited_title/draft_title, theme name/mode; 10 leaves + 1 list) plus nothing new: `title_to_save` is read only as a THEN input (RUN.bn:60, 43) so it stays transient. NovyWave adds at most the 9 named THEN-only fields plus WHEN-over-occurrence outputs to today's 92 leaves + 5 lists. (R2) no activation evaluation: every WHEN/THEN output that feeds the document over a restored selector must be stored (TodoMVC: 94 theme call sites, filter/theme button WHENs at RUN.bn:815/838/1035-1073 per element instance; NovyWave: 342 WHENs), each copy needs a per-instance identity (row key + element route), and code edits to an arm body do not take effect until the selector updates again. Today (measured) a THEN or WHEN output over an occurrence read live by the document is NOT durable (`memory: []` for the probe), so the D12 clause is a real extension of today's schema even under R1.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:99,101,380-388,413-416,568-578` — D30/D32 text; copy-context rule; 'WHEN over a value with a start value' hint; durable leaf list
- [file, verified] `examples/todo_mvc_physical/RUN.bn:39-53,60,85-91,113-160,310,815,838,1035-1073` — title_to_save WHEN over key_down.key; visible_todos WHEN over selected_filter; row LATESTs; theme WHENs over name
- [measurement, verified] `target/release/boon_cli dump-plan examples/todo_mvc_physical/RUN.bn --out <scratch>/todo_plan.txt; python3 json walk of plan['persistence']` — today's TodoMVC durable set: memory = store.selected_filter, store.new_todo_text, store.todos.{edited_title,title,completed,state_0,editing,edited_title.draft_title}, theme_options.{mode,name}; lists = store.todos
- [measurement, verified] `target/release/boon_cli dump-plan examples/novywave/RUN.bn --out <scratch>/nw.txt` — today: 92 memory leaves (86 scalar, 6 indexed_field) + 5 lists; python count of named fields: 1140 named, 76 with HOLD, 9 THEN-only, 38 LATEST-headed
- [measurement, verified] `target/release/boon_cli dump-plan /tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/S4/then_probe.bn --out tp.txt` — probe `last_press_count: press |> THEN { 1 }` and `label_text: press |> WHEN { __ => TEXT { pressed } }` both read by the document: persistence.memory == [] today
- [measurement, verified] `target/release/boon_cli dump-plan examples/counter_latest.bn` — LATEST { 0, increment |> THEN {count+1}, reset |> THEN {0} } is persisted today as scalar store.count

```boon
-- TodoMVC RUN.bn:85-91. Plan (literal): WHEN runs only when selected_filter updates; restore is not an update -> no value after restart. Today: live, recomputed. Developer expects: the filter view is back immediately after restart.
visible_todos:
    todos
    |> List/retain(item, if: selected_filter |> WHEN {
        All => True
        Active => item.completed |> Bool/not()
        Completed => item.completed
    })
```

```boon
-- probe (scratchpad/S4/then_probe.bn). Plan D12: `last_press_count` is read live by the document -> durable. Today: not durable (measured), shows nothing after restart. Developer expects: whatever the plan says, stated once.
store: [
    press: SOURCE
    last_press_count: press |> THEN { 1 }
    label_text: press |> WHEN { __ => TEXT { pressed } }
]
```

**Proposed plan change.** Add to §4.3 an explicit 'Activation' rule and to §4.5 a precise D12 rule. Proposed text: (a) 'Activation (start, restore, hot reload, row creation, entering a WHILE arm) is not an update. At activation every live expression is evaluated from the current values of its inputs, and every WHEN whose selector has a value evaluates its selected arm once, copying the current snapshot. THEN bodies do not run, commands do not run, and occurrences are not replayed. A WHEN over a selector that has a value from the start therefore has a value from the start; a THEN output gets its first value later.' (b) 'A node is history when its current value cannot be produced by activation evaluation: every LATEST (which arm updated last is history), every THEN output, every WHEN output whose selector has no start value, every occurrence payload and every command result, and anything computed from these by live operators without a HOLD or collection in between. A history node is a durable leaf iff it has a reader that is not its own copy consumer: a live read, a LATEST arm of a durable LATEST does not count, a THEN/WHEN whose input is that node does not count, a HOLD whose piped input is that node does not count (the consumer is the leaf). Everything else is recomputed at activation and never stored.' (c) Add to the persistence oracle (plan:843) a per-example durable-set golden file emitted by the checker, and to S1's exit (plan:908) two cases: a WHEN over a restored HOLD selector is re-evaluated at restore, and a THEN output read live is restored from the store. (d) Record the measured baselines above so the expansion over today (THEN/WHEN-over-occurrence outputs read live) is visible.

**Refuter correction (high tier, medium confidence).** Severity critical -> high. Move the activation-rule half to SEM-007 and reference it. Lead with the verified contradiction: §4.5 (plan:569-574) omits D12's LATEST current values and the inferred app-computed values, and plan:577-578 keeps an L13 reachability rule that D35 removed. Rewrite §4.5 from D12/D35, and give 'reads outside its own update' the finder's precise history-node definition (R1). Keep the owner question R1/R2/R3 with R1 recommended. Add the per-example durable-set golden file to the persistence oracle.

**Owner question.** Which reading of D12 + D30 at activation is intended?
- R1: activation evaluation (WHEN over a valued selector evaluates once at activation; THEN never); only history nodes read outside their own update are stored
- R2: no activation evaluation; every copy-context output that feeds the document is stored per instance
- R3: R1 but LATEST is stored only when it has an arm that is history (otherwise recompute and pick the start arm)
Recommendation: R1. It matches today's measured durable set for TodoMVC, keeps storage proportional to app memory, makes FPGA power-on (mux evaluated from reset values) and restore the same operation, and avoids stale copies of code that was edited. R3 changes observable behaviour (a recomputed LATEST would show its start arm instead of its last arm) and should be rejected.

---

### SEM-011 [high] WHEN-to-WHILE migration is not a P2b census item: 300+ store/view WHENs go stale under D30, theme draft is wrong

*Decisions: D30, D8; Plan: 5.1 rows 'TodoMVC Theme' (743) and 'WHEN that must follow live updates' (752); P2b (950-959); P3b (977-987); Sources: S5; verdict: confirmed; severity critical -> high after refutation*

Today a WHEN over a value is live (LANGUAGE_SEMANTICS.md:409-416; the plan admits this at 5.1:752). D30 turns it into a copy at the moment the selector updates. Counting the tracked .bn files gives 1,418 WHEN vs 45 WHILE. A crude classifier over those sites finds 61 WHENs that decode an occurrence (selector is an event/key/THEN output), 314 whose arms read a dotted or outer name (stale-copy candidates), and 1,043 with literal-looking arms — but that last class over-counts safety because arms such as `Theme/material(of: X)` read PASSED.mode inside the callee. Concrete breakages: NovyWave `compare_result_label` copies `comparison_signal_page_result` when the mode changes, so the label says 'opening comparison' forever; `file_status_detail` copies labels; Cells `display_text` copies `value`, so B1 stops following A1 (dependent recalculation is broken by the change model, not by the formula engine); the theme-refactor draft the plan cites as the D8 fix uses `name |> WHEN { Classic => Classic/tokens(mode: mode) … }`, which copies `mode` when `name` updates, so the Light/Dark toggle stops working. 5.1 defers the census to P2b and sizes P3b at 2-3 weeks on top of it.

Evidence:
- [measurement, verified] `cd repo && git ls-files '*.bn' | xargs grep -c '|> WHEN\b' | awk -F: '{s+=$2} END {print s}'  (and the same for WHILE, THEN, 'LATEST {', '|> HOLD')` — WHEN 1418, WHILE 45, THEN 1357, LATEST 342, HOLD 459.
- [measurement, verified] `python3 heuristic over git ls-files '*.bn' (scratchpad S5): selector containing .events./.key/.press/THEN → decode; arms with dotted/outer reads → value_reads; else const_arms` — decode 61, value_reads 314, const_arms 1043; per example: novywave 184 value_reads, todo_mvc_physical 36, fjordpulse 34, persons_pro 22. Heuristic; const_arms over-counts safe sites.
- [file, verified] `examples/novywave/RUN.bn:1784-1799` — compare_result_label: outer WHEN on comparison_waveform_mode; inner reads comparison_signal_page_result, active_file, comparison_transition_count, all of which update independently.
- [file, verified] `examples/novywave/RUN.bn:1800-1804` — file_status_detail copies diagnostic_label / compare_result_label when diagnostic_state changes.
- [file, verified] `examples/cells/cell.bn:56-60` — display_text: editing |> WHEN { True => editing_text, False => value }; value follows other cells.
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/migration/todo_theme_new/Theme.bn.txt:4-12` — tokens(name, mode) dispatches with WHEN over name and reads mode inside the arms; plan 5.1:743 cites this draft as the fix.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:752` — 5.1: 'census in P2b; 1,400 WHEN vs 40 WHILE today … Every WHEN whose arm reads something that can update independently of its selector … becomes WHILE.'
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:409-416` — Today: Value WHEN 'recomputes when the matched value or branch dependencies change'.
- (1 more evidence entries in the finder output)

```boon
-- examples/novywave/RUN.bn:1784-1799 (abridged)
compare_result_label:
    comparison_waveform_mode |> WHEN {
        Active => comparison_signal_page_result |> WHEN {
            SignalPage => TEXT { real comparison } |> Text/concat(with: active_file, separator: ": ")
            __ => TEXT { opening comparison }
        }
        Inactive => TEXT { no compare page }
    }
-- plan (D30): copied when mode flips to Active; the page arrives later → 'opening comparison' forever
-- today: follows the page result
-- developer expects: follows → must become WHILE
```

**Proposed plan change.** Move the WHEN classification into P0: run a resolver-backed classifier (fe_proto.rs plus a name-resolution pass, or extend the S4 census scripts) over the 177 files and report per site: decode / constant-arm / stale-copy (arm reads a name that may update independently of the selector, including through callees). Re-size P3b from that number. Rewrite the theme draft so the dispatch on a live parameter is WHILE (Theme.bn.txt:5 and any tokens(mode) chain that must follow mode), and state in 5.1 that theme dispatch on `mode`/`name` is WHILE. Combine with the E-STALE error rule (next finding) so the migration is compiler-driven rather than manual.

**Refuter correction (high tier, medium confidence).** Lower severity to high: the plan acknowledges the class, and what is wrong is the draft and the ordering. Reword around three concrete changes. (1) Correct examples.md §4 and Theme.bn.txt so the name dispatch uses WHILE (or make tokens a plain record choice with no WHEN on name). Add to 5.1:743 that a theme dispatch on `name`/`mode` is WHILE. (2) Make the P3a theme refactor depend on a stale-copy lister: either a P0 spike built on fe_proto.rs plus name resolution, or move the P2b stale-WHEN census ahead of P3a. (3) Re-size P3b only after that list exists. Drop the heuristic 314 as a headline number, or label it an unresolved upper bound.

---

### SEM-012 [high] WHEN/WHILE under D30 is the event/value split in disguise; make stale-copy an error, not a hover hint

*Decisions: D30, D6; Plan: §4.3 'Stale-copy hint' (409-413); §11 L3 (1201); Sources: S5; verdict: confirmed*

After D30 the WHENs that must remain WHEN are: (a) decoders over an occurrence (key_down.key, effect results, THEN outputs — 61 sites in my count; FjordPulse Client/RUN.bn is almost entirely this class), (b) constant-arm lookups over a state value (behave identically under WHILE), and (c) a deliberate snapshot when a *state* value updates — I found no such site in the examples (TodoMVC title_to_save snapshots `new_todo_text` on `key_down.key`, an occurrence). So the developer's working rule collapses to 'WHILE unless the selector is an occurrence', i.e. the inferred update kind of the selector decides. That is the event/value distinction, inferred rather than declared, which is consistent with D30 ('the compiler still infers, per expression'). The plan's only guard is a hover hint (§4.3:409-413) gated on 'into the document', which misses Cells display_text (a store value). Original WHEN_VS_WHILE.md already had the same decision tree ('Does pattern matching access dependencies? YES → WHILE'), and the notes' model C (which plan §11 L3 says was adopted) defined WHEN-over-value as live, so the decision surface is not stable. Assessment: keep the two keywords, but let the checker enforce the one case where they differ observably.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:409-413` — Stale-copy hint: hover always; P0 decides a warning only for WHENs that copy 'into the document'.
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:166` — Model C: 'WHEN over a value selects live, like today … WHEN over a moment decodes each occurrence'.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1201` — L3 'Answered: model C (D30)' — but D30 diverges from model C on exactly the WHEN-over-value point.
- [file, verified] `/home/martinkavik/repos/boon/docs/language/WHEN_VS_WHILE.md (Decision Tree, Anti-Patterns sections)` — 'Does pattern matching access dependencies (record fields or outer scope)? YES → WHILE'; 'DON'T use WHEN when branches access dependencies'.
- [file, verified] `examples/fjordpulse/Client/RUN.bn:178-186,218-229,262-265,281-285,320-326` — WHENs over key_down.key nesting value WHENs — decode class, stays WHEN.
- [file, verified] `examples/todo_mvc_physical/RUN.bn:46-53` — title_to_save: WHEN over key_down.key copying new_todo_text on purpose (selector is an occurrence).
- [file, verified] `examples/cells/cell.bn:43-51,56-60` — `editing` is rewritten True on every change event; under D32 that re-fires display_text's WHEN per keystroke, so the copy accidentally tracks editing_text while typing but never tracks `value` — a confusing half-working state.

```boon
-- examples/cells/cell.bn:56-60
display_text: editing |> WHEN {
    True => editing_text
    False => value
}
-- plan (D30): copies `value` when `editing` flips (and on every keystroke, D32); A1 edit no longer updates B1's display
-- today: live
-- developer expects: live. Under option B: error with fix-it 'use WHILE'
```

```boon
-- option B leaves this legal (selector is an occurrence): examples/todo_mvc_physical/RUN.bn:46-53
title_to_save: elements.new_todo_title_text_input.events.key_down.key |> WHEN {
    TEXT { Enter } => new_todo_text |> Text/trim() |> WHEN { TEXT {} => SKIP, title => title }
    __ => SKIP
}
```

**Proposed plan change.** Add to §4.3 an error E-STALE: "`x |> WHEN {…}` where `x` has a value from the start and an arm reads a name that may update independently of `x` (not derived from `x`, the arm's binders or constants; reads through callees count) is an error: 'this WHEN copies `mode` when `name` updates; use WHILE to follow `mode`'. A WHEN whose input gets its first value later may read anything (it decodes/copies on purpose). Hover keeps the copy list in both cases." Drop the 'into the document' qualifier. Record in the plan what the two keywords mean after D30: WHEN answers an update (a sip of current values); WHILE follows a state.

**Refuter correction (high tier, medium confidence).** Keep high, framed as an owner question for P0 rather than a gap. Add one plan text fix regardless of the answer: change L3 to "model C revised by D30 (WHEN copies)" so the notes' live-WHEN reading is marked superseded. In option B, state explicitly that reads through callees and PASSED count, and that effects in arms are exempt. Also state the cost: a deliberate state-update snapshot must be written as `init |> HOLD s { x |> THEN {...} }`.

**Owner question.** When a WHEN over a from-start (state) selector reads a value that updates independently of the selector, should the checker (A) only show a hover hint as planned, (B) report an error with the fix-it 'use WHILE', or (C) keep today's behaviour (WHEN over a from-start value is live; WHILE becomes an alias) and reserve copying for occurrence selectors?
- A: as decided — hover hint, optional document-only warning. Pro: no new rule. Con: ~300+ silently stale sites now and the same footgun for every future developer; the original author already mixed WHEN/WHILE on identical shapes.
- B: error E-STALE with fix-it WHILE; WHEN over an occurrence stays free. Pro: decidable from the update-kind pass the checker runs anyway; matches D6 strictness and 'everything is change'; migration becomes fix-its; keeps WHEN meaningful (answer an update). Con: a deliberate snapshot on a *state* update needs `initial |> HOLD s { x |> THEN {…} }`; no example needs it today.
- C: WHEN over a from-start value is live (today's rule, notes model C), WHILE an alias. Pro: no migration of the 1,400 sites; one construct. Con: contradicts the owner's D30 and the original WHEN_VS_WHILE.md; loses the 'sip' semantics on state updates; two keywords for one behaviour.
Recommendation: B. It is the smallest rule that makes the WHEN/WHILE choice checkable, it is consistent with 'everything is change' because it uses only the inferred update kind, and it turns the largest migration class into compiler-guided edits.

---

### SEM-035 [high] D23 exact record parameters are ill-defined for forwarding/shared records and break passing whole rows to view functions

*Decisions: D13, D20, D23, D28; Plan: §1 D23; §4.3 Contracts (lines 445-450); §5.1 'Exact record parameters'; Sources: S2, S5; verdict: confirmed*

D23 says a record passed to a user FUNCTION must contain *exactly* the fields the function consumes, where consumes = reads or forwards into an exact position (callee parameter, state, collection, contract). Worked through the DAG this rule is not well-defined for pass-through functions and produces contradictions for shared records: (1) `h(r) = f(r: r) + r.b` with `f(r) = r.a + 0` — h consumes {a} (via f) ∪ {b}, so r must be exactly {a, b}, but the inner call `f(r: r)` then passes the extra field b to f, whose exact set is {a}: a hard error, so every shared record must be projected at every call (`f(r: [a: r.a])`), and every later change to f's reads ripples into every caller's projection. (2) `id(r) = r` and `keep(r) = [color: Red, ...r]` (the TodoMVC Theme idiom) consume nothing directly, so under the literal rule every call with a non-empty record is an 'extra fields' error; the only sensible exact set for such parameters is the *demand of the consumers of the result*, which is known per call site, not per definition. (3) `save(r) = rows |> List/append(item: r)`: the exact set is whatever the collection's readers project anywhere in the program (consumer-directed, whole-program). (4) A value passed to two callees with exact {a} and {a, b} (`[f(r: v), k(r: v)]` at the root) cannot satisfy both. (5) PASSED: D23 says context flows through PASSED but the plan never says whether the implicit PASSED row is exact; if it were, `main_scene(PASS: [store: store, theme_options: theme_options])` (TodoMVC RUN.bn:325) is an extra-field error in every callee chain that reads only part of it. (6) Tagged-object payloads (D20 elements, `HierarchyPage[...]` with 10 fields) — D23 speaks only of records; if payloads were exact, `label_of(el) = el |> WHEN { Button[label] => label … }` could never receive a real Button. The plan defers 'how exactness composes through spreads and forwarding' to P0 (line 449-450), but the decision itself is what needs a rule, and the examples give it zero coverage today: today's compiler crashes on a record parameter forwarded to a callee (probe u14), and a census finds 580 argument sites that forward an enclosing FUNCTION parameter by name.

Also (S5-07): D23 makes a user FUNCTION's record parameter exactly the fields it consumes (reads, or forwards into another exact position). TodoMVC passes whole rows: `visible_todos |> List/map(item, new: item |> todo_element())`; todo_element reads `editing` and `todo_elements.*` and forwards `todo` to todo_checkbox (reads todo_elements.todo_title_element, completed, title), todo_title_element (completed, title) and editing_todo_title_element (edited_title). The row also has `title_to_update`, which no view reads, so passing the row is an 'extra field' error. The same holds for every row-shaped call (NovyWave real_selected_signal_row reads 4 fields of signal_row) and, with D13/D20, for element helpers (`FUNCTION with_width(el) { [...el, style: [...el.style, width: 200]] }` cannot take a full Button). The only escapes are PASSED (meant for context) or destructuring at every call site. §4.3 defers 'how exactness composes through spreads and forwarding' to P0, but the problem is the base rule for named record values.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:92` — D23 text: 'exactly the fields the function consumes… reads, or forwards into another exact position'; result flow is not listed
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:445-450` — 'Whole records forwarded … take that destination's exact type. P0 specifies how exactness composes through spreads and forwarding.'
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:258-280,314-322` — Scheme predicates are only HasField/open rows; PASSED gets HasField predicates (open); no exactness predicate exists in the notes at all
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:225-229` — 'A parameter has an open row. Spreading it is allowed and produces a symbolic spread' — contradicts exact parameters
- [measurement, verified] `./target/release/boon_cli dump-ir <scratchpad>/S2/u14_forward.bn (FUNCTION f(r){[value: r.a]}; FUNCTION g(r){[inner: f(r: r), b: r.b]})` — today: 'dense kernel checked construction does not cover the complete project … owner has no direct or structural result' — today's compiler cannot even compile record-parameter forwarding, so no example exercises D23 composition
- [measurement, verified] `python3 census over examples/**/*.bn (scratchpad S2): 580 call-site arguments forward an enclosing FUNCTION parameter by name (e.g. examples/fjordpulse/Model/FjordPulseModel.bn:19-21 `language: language`)` — all kinds, not only records; shows the composition rule has wide exposure
- [file, verified] `examples/todo_mvc_physical/RUN.bn:325; examples/todo_mvc_physical/Theme/Theme.bn:73-74` — PASS record with two fields, callee chains read PASSED.theme_options only
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:92,452-457` — D23 text and §4.3 Contracts: 'Passing extra fields is an error'.
- (4 more evidence entries in the finder output)

```boon
FUNCTION f(r) { r.a + 0 }
FUNCTION h(r) { f(r: r) + r.b }
-- plan (literal D23): h's r is exactly {a, b}; f(r: r) passes extra b -> error, must write f(r: [a: r.a])
-- today: crashes the kernel on record forwarding (probe u14)
-- developer expects: h(r: [a: 1, b: 2]) compiles; passing [a: 1, b: 2, c: 3] to h is the error
```

```boon
FUNCTION keep(r) { [color: Red, ...r] }
-- plan (literal D23): keep consumes nothing directly -> every call with a non-empty record is 'extra fields'
-- developer expects: r's exact set is what the callers of keep(...) read from the result
```

```boon
-- examples/todo_mvc_physical/RUN.bn:552-553 and 699-712
items: visible_todos |> List/map(item, new: item |> todo_element())
...
FUNCTION todo_title_element(events, todo) {
    Scene/Element/label(element: [events: events], style: [...],
        label: Scene/Element/text(element: [], style: todo_title_text(completed: todo.completed), text: todo.title))
}
-- plan (D23): `todo` consumes {completed, title} → passing the 6-field row is an error
-- today: fine; developer expects: pass the row
```

**Proposed plan change.** Add to §4.3 'Contracts' a normative exactness rule (recommended option B): (1) exact(f, p) = direct reads of p ∪ ⋃ exact(c, q) for every whole-record forward of p into parameter q of callee c ∪ (for `[…, ...p]` into an exact position with set S: S minus the explicitly written fields) ∪ the contract's key set for forwards into contracts ∪ the demand of every consumer the value reaches through f's result, state or a collection. (2) Exactness is checked at *entry points*: where a non-parameter value (literal, root value, state, row, effect result, spread result) enters an exact position, its field set must equal that position's exact set. A whole-record forward of a parameter (or a spread of one) into a smaller exact set is an implicit projection, not an error. (3) The implicit PASSED row is open (width subtyping), never exact. (4) Tag payloads are open; only REC members are exact. (5) Because result flow makes the exact set consumer-dependent, exactness is a deferred `Exact(value, position)` predicate discharged when both sides are ground (in the caller's solve or the root group), never a Closed tail bound in the solver (see the lattice finding). Add an S4 census class 'record parameters forwarded to callees / spread into results' with counts.

[also from S5-07] Owner question (below). Whatever the answer, add to 5.1 a census class 'FUNCTION reads a strict subset of a passed record value' (the S4 scripts can count it) and specify in §4.3 how a row from List/map or a FUNCTION result (new_todo) is typed at a call site.

**Refuter correction (high tier, high confidence).** Keep it high and keep the owner question, but trim it. Drop sub-point (6) (tag payloads): D20 (plan:89) says each element kind 'keeps its exact payload', and a label_of helper that reads one field is a question for the same answer, not a separate contradiction. Add a fourth option to the question, taken from the extra question: exactness applies only to record literals written at the call site (the 'data by accident' case) and width subtyping applies to named values (rows, function results). It is the smallest rule that meets the owner's stated motivation, and it should be compared with B. Require the P0 rule before CK5 (schemes) starts, and require the S4 census to count 'whole row/record passed to a FUNCTION that reads a strict subset' so the owner sees the migration size under each option. Cite the u14 reproduction as evidence that today's compiler provides no differential oracle for this.

**Owner question.** Which exactness discipline should D23 mean?
- A. Strict per call: every argument's field set equals the callee parameter's exact set, parameters included; callers project at every call (`f(r: [a: r.a])`).
- B. Entry-point exact with implicit projection: exact sets are computed over the DAG as the union of reads and forwarded demands (result flow included); only non-parameter values entering an exact position are checked for equality; forwarding a parameter narrows implicitly; PASSED and tag payloads stay open.
- C. Exact only at root-level call sites; FUNCTION-to-FUNCTION calls use width subtyping.
Recommendation: B. It keeps the owner's intent (no record reaches a function by accident: every field of a value entering a function is consumed somewhere below it), gives a principal scheme (the exact set is a least fixpoint over the acyclic call graph plus a deferred predicate for result flow), and avoids projection boilerplate that A would force on every helper call. A should be rejected because it makes `h(r) = f(r: r) + r.b` untypable and turns every callee read change into caller edits, which is the opposite of the stated motivation.

---

### SEM-002 [medium] D15 starting arms that update from the same trigger collide with D32; D33 reset edges and D26 library-HOLD edges are unclassified for D9

*Decisions: D9, D15, D26, D32, D33, D14; Plan: §4.3 LATEST/HOLD/Cycles (394-401, 417-425); 5.1 cycles row (749); Sources: S5; verdict: confirmed; severity high -> medium after refutation*

(1) persons_pro `publish_state: LATEST { has_published_revision |> WHEN { True => Published, False => Draft }, …, compiled |> THEN { Published }, … }`: the first arm is derived from the `published_revision` HOLD, which is written on every `compiled` result (D32 fires even if equal), so arm 1 and the `compiled |> THEN` arm update from the same trigger in the same step → §4.3 'Two arms that can update from the same trigger in the same step are an error'. The fix is a restructure, not the D15 fix-it. (2) The D15 fix-it `initial |> HOLD s { LATEST {…} }` combined with D33 means any live `initial` resets `s` on every rewrite, equal or not (D32) — persons_pro's self-naming LATESTs (`publish_request_sequence + 1`, `mode |> WHEN`) get the fix-it and are fine because their initial is a literal, but the pattern is a trap for migrators who pick a derived initial. (3) D26 rewrites `Bool/toggle(value, when)` as `value |> HOLD s { when |> THEN { s |> Bool/not() } }`; TodoMVC's completed → all_completed → toggle_all THEN → completed cycle then closes through HOLD's *piped reset edge*, which §4.3's list of cycle-closing edges (HOLD update edges, collection updates, stateful builtins) does not name; 5.1 calls it legal 'through Bool/toggle' assuming a builtin.

Evidence:
- [file, verified] `examples/persons_pro/RUN.bn:359-369,380-382,452-453` — publish_state LATEST with a derived starting arm; published_revision HOLD written on every compiled; has_published_revision derived from it.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:394-397` — LATEST: at most one arm from the start; two arms updating from the same trigger in the same step are an error.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:101-102,95` — D32 every write fires; D33 piped update resets HOLD; D26 stateful builtins become Boon over HOLD.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:421-423,749` — Only three edge kinds may close a cycle; 5.1: 'completed ↔ all_completed passes through Bool/toggle (legal)'.
- [file, verified] `examples/todo_mvc_physical/RUN.bn:136-143,83-84` — completed: LATEST{initial_completed, toggle_all.click |> THEN { store.all_completed |> Bool/not() }} |> Bool/toggle(when: checkbox.click); all_completed derived from todos.
- [file, verified] `examples/persons_pro/RUN.bn:332-336,455-464` — Self-naming LATESTs (publish_request_sequence, mode) — D14 makes them 'unknown name'; D15 fix-it applies cleanly.
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:397-406` — Today: greatest source sequence wins; a tie is a hard error unless PRIORITY/EXCLUSIVE.

```boon
-- examples/persons_pro/RUN.bn:359-369
publish_state:
    LATEST {
        has_published_revision |> WHEN { True => Published, False => Draft }
        elements.source_editor.event.change |> THEN { Draft }
        elements.publish.event.press |> THEN { Building }
        elements.publish_candidate_program.compiled |> THEN { Published }
        elements.publish_candidate_program.rejected |> THEN { Failed }
    }
-- plan: `compiled` writes published_revision (D32 fires) → arm 1 updates in the same step as arm 4 → error
-- today: runs (sequence rule); developer expects Published
```

**Proposed plan change.** §4.3 LATEST: state whether a starting arm may update after start (persons_pro:361 does). Either (a) require the starting arm to be never-updating (fix-it: move it into `initial |> HOLD`), or (b) allow it and define ordering for same-step ties as 'textually later arm wins' (the original 'last event wins'). §4.3 Cycles: add 'a HOLD's piped-input (reset, D33) edge is a HOLD update edge' and re-verify the 5.1 cycles row against the library Bool/toggle. 5.1: add census classes 'LATEST whose starting arm can update' and 'HOLD whose piped input is live'.

**Refuter correction (high tier, medium confidence).** Severity high -> medium. Drop part (2). Recast part (1) as a dependency on SEM-004: the tie rule's 'same step' must be defined (microstep or tick/rank). persons_pro publish_state is the worked example: under microsteps, arm 1 updates one microstep after arm 4 (benign, same value); under tick semantics it is an error that needs a restructure. Keep part (3) as a concrete addition to the 5.1 cycles row: re-derive the TodoMVC completed cycle against the library Bool/toggle, and state whether the HOLD piped-input edge may close a cycle. Under SEM-001's corrected rule it may not; the cycle is then legal only if the Bool/toggle body binder is on it, which it is not, so the row would need a rewrite.

---

### SEM-003 [medium] D33 piped-input resets: no same-step precedence, no starting-value rule, no migration census; breaks the seed-once idiom at 49 sites

*Decisions: D33, D32, D9, D12; Plan: §4.3 'HOLD' bullet; D33; D32; §5.1 (no D33 row); S4 census list; 'Update kinds are ... seeded at ... HOLD'; Sources: S1; verdict: confirmed; severity high -> medium after refutation*

(i) Same step: `seed: count |> HOLD seed { press |> THEN { seed + 100 } }` - on one press the piped reset (count fires) and the body candidate arrive together; the plan gives no precedence (today piped is init-only: probe pK gives seed 100, 200). (ii) No value at start: `h: reset_val |> HOLD h { … }` with `reset_val: reset |> THEN { 100 }` - §4.3 seeds update kinds 'at HOLD' but this HOLD has no value until the first reset; today such a HOLD is dead forever (probe pB: h 'privately absent' even after reset fires). (iii) Migration: 49 HOLD sites pipe a name path (todomvc.bn:126,146,166 row fields; novywave RUN.bn:1248,1374,1885,1926,2283,2376,2393,2414,2478,4483,4769,4929; cells/cell.bn:26,36; migrations/todo v1-v7; 8 bytes_* fixtures). §5.1 has no D33 row and S4 does not census 'piped inputs that update after start'. Concrete behaviour change: the three NovyWave zoom-center HOLDs (RUN.bn:2375-2430) are piped from `selected_timeline_zoom_center_default` <- `selected_timeline_cursor_default` <- `selected_timeline_range_start` <- `real_waveform_open_result` (RUN.bn:2255-2259, 2336-2355); under D33 the arm `elements.load_default_file |> THEN { 50 }` is overwritten by a reset when the waveform open completes in a later tick, and the hand-written `select_*_file |> THEN { …default }` reset arms become redundant. (iv) D33 removes the only 'seed once from a live value, then own my updates' idiom (today's init-only piped input, LANGUAGE_SEMANTICS.md:374-377); in particular the D32 escape hatch 'a real-changes filter is a library function over HOLD' cannot be `x |> HOLD last { … }` (the piped x resets and fires on every write) and needs an explicit start argument. TodoMVC's `completed: LATEST { initial_completed, toggle_all.click |> THEN {…} } |> Bool/toggle(when: click)` (RUN.bn:136-143) is the positive case: with D26+D33 the piped LATEST correctly resets the toggle HOLD - so D33 is right for that site but must be documented as a breakage class.

Evidence:
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pK_own_name.bn "[('init',None,{}),('p1','store.press',{}),('p2','store.press',{})]" store.count,store.shadow,store.seed` — seed: 0, 100, 200 while count: 0, 1, 2 -> today piped is init-only; under D33 both a reset and a candidate arrive on each press
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pB_piped_reset.bn "[('init',None,{}),('p1','store.press',{}),('r1','store.reset',{}),('p2','store.press',{})]" store.reset_val,store.h,store.h2,store.base,store.h3` — h: privately absent at every step (piped from a no-value THEN); h2 (LATEST {0, reset_val} piped) and h3 (HOLD base piped) are NOT reset at r1 today (stay 1) - under D33 they become 100 then 101
- [measurement, verified] `python3 census over examples/**/*.bn of `<name path> |> HOLD` sites (script in this session)` — 49 sites; list in the claim
- [file, verified] `examples/novywave/RUN.bn:2255-2259,2336-2355,2375-2430` — zoom-center HOLDs piped from a value derived from real_waveform_open_result; arm `load_default_file |> THEN { 50 }`
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:374-377` — today: 'the piped expression is the initialization value ... Dynamic resets are expressed as ordinary update candidates inside the body'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:741-769,911` — §5.1 and S4 have no D33 class
- [file, verified] `examples/todo_mvc_physical/RUN.bn:136-143` — LATEST piped into Bool/toggle: the site where D33 semantics are needed

```boon
-- S10 piped reset and body candidate in the same step (probe pK)
count: 0 |> HOLD count { press |> THEN { count + 1 } }
seed: count |> HOLD seed { press |> THEN { seed + 100 } }
-- plan: undefined (D33: 'takes every value that arrives, from its piped input or its body')
-- today: seed 100, 200 (piped is init-only)
-- expected: an error like a LATEST tie (both writers fire from `press`), or a documented 'reset wins'
```

```boon
-- S11 HOLD piped from a value with no value at start (probe pB)
reset_val: reset |> THEN { 100 }
h: reset_val |> HOLD h { press |> THEN { h + 1 } }
-- plan: update kinds 'seeded at HOLD' - but h has no value until reset
-- today: h is 'privately absent' forever, even after reset
-- expected: error 'HOLD needs a starting value; reset_val gets its first value later' (fix-it: LATEST { 0, reset_val } |> HOLD h)
```

```boon
-- S12 seed-once idiom lost (D32 'real changes' filter)
last: x |> HOLD last { x |> THEN { x == last |> WHEN { True => SKIP  False => x } } }
-- plan: the piped x resets `last` (and fires) on every write of x, so this never filters
-- expected: FUNCTION Value/distinct(value, start) { start |> HOLD last { value |> THEN { value == last |> WHEN { True => SKIP  False => value } } } }
```

**Proposed plan change.** Add to §4.3 HOLD: 'The piped input must have a value from the start (error otherwise, fix-it: wrap in LATEST with a constant). If the piped input and a body candidate update in the same tick [same rank], it is an error, like a LATEST tie, with the fix-it "move the reset into the body's LATEST".' Add a §5.1 row 'HOLD piped inputs that update after start (D33)' with the 49-site census and the NovyWave zoom chain as the worked example, and add the class to S4. State in D32 that the real-changes filter is `Value/distinct(value, start)` (definition above) and put it in the P0 catalog.

**Refuter correction (high tier, medium confidence).** Severity high -> medium. Retitle: 'D33 has no migration class, and the seed-once idiom has no replacement'. Split the census: piped inputs that never update after start or row creation (no change) versus live piped inputs such as the NovyWave zoom chain (behaviour change, list them). Replace the same-step example with one that really collides in one microstep (`press.value |> HOLD s { press |> THEN {...} }`). Also note the answer depends on SEM-004's step/rank definition: under R8 microsteps a derived reset lands one microstep later and wins. Drop the proposed 'piped input must have a value from the start' error. The plan's No-value-yet rule already defines that case. Keep the Value/distinct(value, start) catalog note for D32's 'real changes' filter.

**Owner question.** When the piped input and a body candidate of a HOLD update in the same tick, which wins?
- (a) Error, like a LATEST tie (fix-it: merge the reset into the body's LATEST).
- (b) The piped reset wins (hardware synchronous-reset precedence).
- (c) The body candidate wins.
Recommendation: (a): it is the same situation as two LATEST arms from one trigger, and the owner chose 'error' there; one rule for both keeps the model small.

---

### SEM-005 [medium] LATEST tie rule: 'same step' undefined, deciding analysis dropped, assumes one occurrence per tick

*Decisions: D15, D32, D21; Plan: §4.3 'LATEST' bullet; D15; R8; Sources: S1; verdict: confirmed*

'Two arms that can update from the same trigger in the same step are an error' leaves three things open. (1) 'step': under R8 microsteps, `LATEST { press |> THEN { 1 }  a |> THEN { 2 } }` with `a` written by press fires at microsteps 0 and 1 - no tie and 2 wins if step = microstep, an error if step = tick; today the press arm wins (probe pD: tie_press_hold = 1, tie_microstep = 1), and `LATEST { a |> THEN {1}  b |> THEN {2} }` with a and b both written by press silently gives 2 (pD: tie_two_holds = 2) where LANGUAGE_SEMANTICS.md:405-407 demands a hard error. (2) The analysis: 'can update from the same trigger' needs, per arm, the set of (occurrence site, rank) pairs it can fire from - the clock sets of Model C that the plan dropped; with F2's rank scheduling the pairs are exact and the check is a set intersection. (3) Soundness of a static check requires that a tick ingests exactly one external occurrence; today's scenario runner does (boon_runtime/src/lib.rs:1964 `expected_source_event: Option<…>`, lib.rs:1395-1409 one dispatch per step), but RUNTIME_MODEL.md:78 says 'Ingest source events' (plural) and the plan never states the rule for hosts.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:392-395` — LATEST rule text
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pD_latest_tie.bn "[('init',None,{}),('p1','store.press',{}),('p2','store.press',{})]" store.a,store.b,store.c,store.tie_two_holds,store.tie_press_hold,store.tie_microstep` — p1: tie_two_holds=2 (silent last-arm win), tie_press_hold=1, tie_microstep=1 (source arm beats state arm today)
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:395-407` — today's documented rule: greatest source sequence wins; equal sequence is a hard error
- [file, verified] `crates/boon_runtime/src/lib.rs:1964,1395-1409` — one optional source event per scenario step, dispatched once
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:141-148` — Model C's clock sets ('the set of roots it can happen on') - dropped by the plan (taken from notes)

```boon
-- S13 LATEST arms from one press at different ranks (probe pD)
a: 0 |> HOLD a { press |> THEN { a + 1 } }
c: 0 |> HOLD c { a |> THEN { c + 1 } }
v: 0 |> HOLD v { LATEST { press |> THEN { 1 }  a |> THEN { 2 } } }
w: 0 |> HOLD w { LATEST { a |> THEN { 1 }  c |> THEN { 2 } } }
-- plan: 'same step' undefined -> either 2/2 (microstep) or error (tick)
-- today: v=1, w=1 (press arm and earlier state arm win)
-- expected: causally later wins (v=2, w=2); equal rank from the same occurrence -> error
```

```boon
-- S14 same-rank tie (probe pD)
b: 0 |> HOLD b { press |> THEN { b + 1 } }
t: 0 |> HOLD t { LATEST { a |> THEN { 1 }  b |> THEN { 2 } } }
-- plan: error (correct); today: silently 2
```

**Proposed plan change.** Rewrite the LATEST bullet: 'Each arm has a fire set: the (occurrence site, rank) pairs it can fire from, computed in the forward pass. Two arms whose fire sets intersect are an error (S14). Arms fired by the same occurrence at different ranks: the higher rank (causally later) wins (S13). A tick ingests exactly one external occurrence (SOURCE occurrence, effect completion or timer tick); hosts queue further occurrences for later ticks.' Put the one-occurrence rule into RUNTIME_MODEL.md and NATIVE_GPU_PIPELINE.md and add S13/S14 to the O-oracles.

**Refuter correction (low tier, high confidence).** Finding is well-evidenced and actionable as written (define 'step' for LATEST relative to R8 microsteps, and state the host-ingestion invariant the static check depends on). No downgrade needed; arguably this could be rated high rather than medium since it undermines the soundness of a flagship 'hard error' guarantee (D15/tie-is-error), but medium is defensible given it is a spec-completeness gap rather than a demonstrated runtime crash.

---

### SEM-006 [medium] D34 loading idiom shows the previous arguments' answer during restarts; tie rule needs effect results as their own trigger roots

*Decisions: D30, D34, D12; Plan: §1 D34; §4.3 LATEST (394-397), update kinds seed sentence (417-418); Sources: S3; verdict: confirmed; severity high -> medium after refutation*

With 'every value keeps its last value', a restarted live query keeps the answer for the OLD arguments while the new run is in flight (page 0's rows under page 1's offset). `LATEST { Loading, request |> THEN { Loading }, query }` returns to Loading only when `request` fires in the same step as the argument update; restarts caused by anything else (restore re-run, another trigger writing the argument, hot reload) show stale data silently: stale-while-revalidate by accident, with no way to tell a fresh answer from a stale one because the plan drops `request_fingerprint` and defines no argument echo. Also, the §4.3 LATEST tie rule ('two arms that can update from the same trigger in the same step are an error') rejects the command form of the idiom (two THEN arms on `press`) unless the checker models an effect result as a fresh root that never updates in the step of its trigger; the plan does not say this (the notes did, via clocks).

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:103` — D34 idiom and 'while a restarted query runs, the value keeps its last answer'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:394-397` — LATEST: 'Two arms that can update from the same trigger in the same step are an error'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:81` — D12 persists LATEST current values → after restore the LATEST shows the old answer while the live query re-runs
- [file, verified] `crates/boon_wellen_host/src/lib.rs:560-575` — HierarchyPage result echoes artifact, request_fingerprint and offset — today an app can detect a mismatch; the plan removes the fingerprint and does not require the echo
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:150-154` — notes' Model C gave every effect call site its own clock root; the plan's §4.3 update-kind pass ('seeded at … effect results') does not state the never-same-step property

```boon
page: LATEST {
    Loading
    next.press |> THEN { Loading }
    Wellen/hierarchy_page(artifact: artifact, offset: page_offset, limit: 256)
}
-- page_offset also written by restore re-run / keyboard handler that does not go through next.press:
-- plan: `page` keeps page 0's HierarchyPage while page 1 loads; no Loading, no way to tell
-- today: same staleness but the fingerprint echo lets NovyWave compare
-- developer expects: either Loading, or rows that belong to page_offset
```

```boon
status: LATEST {
    Idle
    send.press |> THEN { Sending }
    send.press |> THEN { Http/send(...) |> WHEN { HttpSucceeded => Sent, __ => Failed } }
}
-- plan §4.3 tie rule: both arms 'can update from the same trigger' unless effect results are their own roots
```

**Proposed plan change.** (1) §4.3: 'An effect result is its own update root: it never updates in the step that started the run. LATEST tie analysis and cycle analysis treat it as independent of the run's trigger.' (2) Catalog convention in D31/§4.3 Contracts: every query result record echoes the arguments that keyed the run (Wellen already does: artifact, offset), so `page.offset == page_offset |> WHILE { True => page.rows, False => LIST {} }` distinguishes stale from fresh without a fingerprint. (3) Hover on a live query: 'keeps the answer for the previous arguments while restarting'. (4) Document the two safe idioms in LANGUAGE_SEMANTICS v2: loading keyed on the request trigger, and echo comparison.

**Refuter correction (high tier, medium confidence).** Severity high -> medium. Reframe: the stale-while-restarting behaviour is owner-decided by D34 and is not the finding. The findings are (1) the plan must state that an effect result, including the output of a THEN whose body is a command, updates only when the answer arrives, never in the trigger's step, and the tie and cycle analyses must treat it as its own root; (2) optionally, a catalog convention that query results echo their keying arguments so apps can compare, as an option for the owner with the recommendation to adopt it. Replace the Http/send snippet with `sent: send.press |> THEN { Http/send(...) }` plus `status: LATEST { Idle, send.press |> THEN { Sending }, sent |> WHEN {...} }`.

**Owner question.** Should a live query's value be cleared when it restarts?
- (a) No: keeps the previous arguments' answer (D30 consistent; stale mismatch by default; needs the echo convention)
- (b) Yes: back to 'no value yet' until the new answer (renders nothing, L3c; flicker; breaks 'every value keeps its last value')
- (c) No, but every query result echoes its arguments so the app can compare (a) + convention
Recommendation: (c).

---

### SEM-009 [medium] Nested THEN/WHEN/WHILE inside copy contexts is undefined; literal 'never updates' rule rejects 675 existing sites

*Decisions: D30, D32; Plan: §4.3 'Copy contexts' bullet ('THEN or WHEN over something that never updates is an error') and 'Placement'; Sources: S1; verdict: confirmed; severity high -> medium after refutation*

Inside a THEN body or WHEN arm every read is a copy, so a nested `x |> WHEN {...}` has a selector that never updates within the copy; the plan's only applicable sentence makes that an error ('never runs'), and no sentence says what a nested THEN means. The census over examples/**/*.bn finds 161 THEN>WHEN, 514 WHEN>WHEN, 10 (THEN|WHEN)>WHILE and 1 THEN>THEN nestings, including TodoMVC RUN.bn:46-53 (WHEN over `new_todo_text |> Text/trim()` inside a WHEN arm), RUN.bn:181-186 (`state |> WHEN { Light => Dark  Dark => Light }` inside a THEN inside a HOLD) and 31 more HOLD bodies where the binder feeds a WHEN/THEN (fjordpulse Client/RUN.bn:141, novywave RUN.bn:763,818,871,921,950,1926,2641,2999,3266,4837, migrations/todo v3-v7, kavik_cz RUN.bn:41). Today a nested WHEN is a one-shot match (probe pL: inner = zero then nonzero; mode toggles Light/Dark), but a nested THEN over a SOURCE fires without its input ever occurring (pL: inner_then = 7 after a press with no bump) - a today-bug the new rule must not inherit. The original SNAPSHOT_VS_STREAM.md made the whole body 'snapshot context' and let user functions inherit it, which is the rule the plan needs to write down.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:385-388` — 'THEN or WHEN over something that never updates is an error ("never runs")'
- [measurement, verified] `python3 census over examples/**/*.bn counting `|> (WHEN|WHILE|THEN) {` inside brace-matched THEN bodies / WHEN arms (script in this session)` — {'THEN': {'WHEN': 161, 'WHILE': 2, 'THEN': 1}, 'WHEN': {'WHEN': 514, 'WHILE': 8, 'THEN': 0}}; top files novywave/RUN.bn (109 WHEN>WHEN, 94 THEN>WHEN), NovyView.bn 94, NovyTheme.bn 62
- [measurement, verified] `python3 census of HOLD bodies whose binder is a WHEN/WHILE/THEN input (script in this session)` — 32 sites, listed in the claim
- [file, verified] `examples/todo_mvc_physical/RUN.bn:46-53,181-186` — nested WHEN in a WHEN arm; `state |> WHEN` inside a THEN body inside HOLD
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pL_nested_when.bn "[('init',None,{}),('p1','store.press',{}),('b1','store.bump',{}),('p2','store.press',{})]" store.mode,store.x,store.inner,store.inner_then` — p1: mode=Dark inner=zero inner_then=7 (bump never fired!); b1: x=1 inner unchanged; p2: mode=Light inner=nonzero
- [source, verified] `~/repos/boon/docs/language/SNAPSHOT_VS_STREAM.md:11-14,110-137` — THEN/WHEN bodies are snapshot context; user functions inherit the caller's context

```boon
-- S8 nested WHEN in a copy body (probe pL; shape of TodoMVC RUN.bn:181-186)
mode: Light |> HOLD mode { press |> THEN { mode |> WHEN { Light => Dark  Dark => Light } } }
inner: press |> THEN { x |> WHEN { 0 => TEXT { zero }  __ => TEXT { nonzero } } }
-- plan: literally an error ('WHEN over something that never updates')
-- today: one-shot match on the copied value (works)
-- expected: one-shot selection over copied values; no update-kind rule applies to a nested selector
```

```boon
-- S9 nested THEN in a copy body (probe pL)
inner_then: press |> THEN { bump |> THEN { 7 } }
-- plan: undefined
-- today: 7 after the first press although bump never occurred (bug)
-- expected: error 'THEN inside a copy would run at most once; move it outside or merge with LATEST'
```

**Proposed plan change.** Add to §4.3 'Copy contexts': 'Inside a copy context, a nested WHEN or WHILE is a one-shot selection over copied values (its arms are copies too), and the "never updates" rule applies only to the outermost input of the copy. A nested THEN is an error ("this THEN is inside a copy and can never run again; move it outside or use LATEST"). HOLD, SOURCE and live queries inside a copy are errors (L6).' Add the nested-THEN class to the S4 census and S8/S9 as checker tests; add the 32 HOLD-binder sites to the O4 differential corpus.

**Refuter correction (high tier, medium confidence).** Severity high -> medium. Drop the '675 sites rejected' claim and treat the census only as the size of the nested-copy test corpus. Retitle: 'Nested THEN and WHILE inside copy contexts are undefined; nested WHEN is implied one-shot but unstated'. Keep the proposed text for nested THEN, an error with a fix-it. That text must not say the never-updates rule 'applies only to the outermost input', because update kinds are global, and a nested `TEXT {x} |> WHEN` inside a copy should still get SEM-007's constant-selector treatment. Add pL's inner_then as a checker test that fixes today's bug.

---

### SEM-013 [medium] D28's read-only view example uses WHEN, which is a copy under D30; copying collections in copy contexts unspecified

*Decisions: D28, D30; Plan: D28 (97); §4.3 copy contexts (384-387); Sources: S5; verdict: confirmed*

D28 defines `zs: WHEN { A => xs, B => ys }` as 'a read-only view whose element type is the union'. Under D30 a WHEN copies the current values of everything it reads, so `zs` is a snapshot of `xs` taken when the selector updated; rows appended to `xs` later are not visible. The intended view is `sel |> WHILE { A => xs, B => ys }`. The plan also does not say what 'copies' costs for a LIST read inside a THEN/WHEN body (`todos |> List/count()` in a THEN): a by-reference snapshot of the committed revision, or an O(n) copy per update.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:97,99,384-387` — D28 wording; D30 'WHEN and THEN copy'; §4.3 copy contexts.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:629-631` — R-DOC: structural sharing (Arc) — suggests by-reference snapshots are intended but only for elements.
- [file, verified] `examples/todo_mvc_physical/RUN.bn:54-91` — todos authority pipeline; visible_todos is a retain, not a WHEN — D28's example has no live instance in TodoMVC.

```boon
-- D28 as written
zs: sel |> WHEN { A => xs, B => ys }
-- plan (D30): zs copied when sel updates; List/append to xs invisible in zs
-- intended: zs: sel |> WHILE { A => xs, B => ys }
```

**Proposed plan change.** Change D28's example to `sel |> WHILE { A => xs, B => ys }` (or say the WHEN form is a snapshot copy). Add to §4.3: 'a collection read in a copy context is a by-reference snapshot of its committed revision (O(1)); the copy never becomes an authority'.

**Refuter correction (low tier, medium confidence).** Keep as written; the finding's own recommendation (use `sel |> WHILE { A => xs, B => ys }` for a live view, and add a plan sentence on collection-copy cost in copy contexts) is the right actionable fix. Consider noting explicitly that this is a documentation/example fix (D28's illustrative snippet), not a change to D28's decision itself.

---

### SEM-015 [medium] Missing small rules: no-value reads in copy bodies/commands, SKIP vs no-value, HOLD binder in D14 scope list

*Decisions: D30, D31, D14; Plan: §4.3 'No value yet', 'SKIP', 'Names (D14)'; D31; Sources: S1; verdict: confirmed*

(i) `reads_ev: press |> THEN { ev }` with `ev: other |> THEN { 5 }`: the plan says the result 'has no value yet' but not whether the body runs at all, whether that is an update, or what a command in that body does with a missing argument. Today the downstream THEN fires although the value is absent (probe pE: reads_ev_fires = 1 while reads_ev is 'privately absent'; the HOLD candidate does not write). The checker can prove statically that `ev`'s fire set (root `other`) is disjoint from `press`'s, i.e. `ev` may have no value at the first press. (ii) SKIP 'means no update' and 'no value yet' propagate identically; the plan should say a copy body that yields no value is a SKIP (no update, no command). (iii) The lexical scope list ('record-literal siblings, BLOCK locals, WHEN binders, OUT/call-context binders, FUNCTION parameters') omits HOLD binders although D14 makes the binder the only self-reference; today a binder with a different name than the field works (probe pK: `shadow: 0 |> HOLD other { … other + 1 }`).

Evidence:
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pE_no_value_read.bn "[('init',None,{}),('p1','store.press',{}),('o1','store.other',{}),('p2','store.press',{})]" store.ev,store.reads_ev,store.reads_ev_hold,store.reads_ev_fires,store.presses` — p1: reads_ev absent, reads_ev_hold=0, reads_ev_fires=1 (fires on an absent value); o1: ev=5; p2: reads_ev=5 reads_ev_hold=5 reads_ev_fires=2
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:398-404,426-430` — 'No value yet' and SKIP bullets; the scope list without HOLD binders
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pK_own_name.bn ... store.shadow` — shadow: 0,1,2 with binder name `other`

```boon
-- S17 copy body reading a value that has no value yet (probe pE)
ev: other |> THEN { 5 }
reads_ev: press |> THEN { ev }
send: press |> THEN { Http/send(url: TEXT { /x }, body: ev) }
-- plan: reads_ev 'has no value yet'; whether `send` issues a command is unstated
-- today: reads_ev absent but a THEN over it fires
-- expected: no update and no command; warning 'ev may have no value when press first fires' (fire sets disjoint)
```

**Proposed plan change.** Add to §4.3: 'A copy body that reads a value with no value yet produces no update (as SKIP): nothing downstream fires and no command is issued. The checker warns when the read value's fire set is disjoint from the trigger's ("`ev` may have no value when `press` fires").' Add 'HOLD binders' to the D14 scope list between record-literal siblings and BLOCK locals, and state that the binder may differ from the field name.

**Refuter correction (low tier, high confidence).** None needed to severity; this is a good catch-all of small but real spec gaps. Consider splitting into the HOLD-binder-in-scope-list omission (a one-line fix, low severity) versus the no-value-propagation-in-copy-bodies question (needs a design answer, medium severity) since they have different fix costs.

---

### SEM-016 [medium] D31 'restarts when arguments change' vs D32 'every write fires': live queries restart on equal writes, with livelock risk and no writable dedupe filter

*Decisions: D30, D31, D32, D33, D34, D36; Plan: §1 D31/D32/D34; §4.3 Placement (line 390); R9 (line 614); Sources: S1, S3, S5; verdict: confirmed; severity high -> medium after refutation*

D31 says a live query "restarts when its arguments change"; D32 says every write fires and equality is never engine business. Read literally, a live query restarts on every argument update even when the value is rewritten equal, so `Wellen/hierarchy_page(artifact, offset: page_offset)` with `page_offset` a HOLD written on every key press cancels and re-fetches on every key. Today's runtime dedupes equal HOLD writes (verified: probe p1 passes), so migration changes restart counts. D34's claim that Wellen's hand-written `request_fingerprint` becomes unnecessary is only half true: the host does not cache by it (it only echoes it), but its dedupe role in NovyWave (rounded cursor ticks, derived labels) moves to a hand-written HOLD+SKIP, it does not disappear.

Also (S1-08): D31 and D36 use 'change' ('restarts when its arguments change', 're-fetches when its arguments update'), but D32 defines the only notion of update as 'every write fires, equal or not'. So a live query restarts and cancels its in-flight run whenever any argument is rewritten to an equal value; a query keyed on a HOLD rewritten at a higher rate than its latency (e.g. a NovyWave signal page keyed on viewport HOLDs rewritten per pointer move, or an argument derived from Timer/interval) never completes. D32's escape hatch 'a real-changes filter is a library function over HOLD' is not writable as `x |> HOLD last {…}` under D33 (F5) and the plan gives no signature.

Also (S5-06): D31 restarts a live query 'when its arguments change'; D32 says every write fires, equal or not, and the only dedup is a library over HOLD. NovyWave rewrites `real_waveform_asset` with the same PackageAsset on every `load_default_file` press and feeds it to `File/read_stream(file: real_waveform_asset …)` in a WHILE arm, followed by `Wellen/open`; under D32-as-'change' each press re-streams the file and re-opens the waveform; today HOLD dedups (the plan's R8 says so). TodoMVC: an equal `selected_filter` press re-fires `visible_todos` retain and every filter_button material; `has_completed` rewritten True on every completed_count write raises the question whether `store.has_completed |> WHILE { True => remove_completed_button(...) }` re-enters the arm (element re-creation, hover/focus loss). §4.3 says entering an arm is not an update but not whether an equal rewrite of the selector re-selects the same arm.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100-101` — D31 'restarts when its arguments change'; D32 'every write fires, equal or not… A "only real changes" filter … is a library function over HOLD, not engine support'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:103` — D34: 'Wellen's hand-written request_fingerprint is unnecessary once restarts are driven by argument updates'
- [measurement, verified] `target/release/boon_cli run p1.bn --scenario p1.scn (scratchpad/S3, copies of change_probes/p1_then_over_hold.*)` — pass: step 'one-again-same-value' expects fires=1 after a second equal write, i.e. today's HOLD dedupes equal writes
- [file, verified] `crates/boon_wellen_host/src/lib.rs:502-575` — try_hierarchy_page only validates request_fingerprint and echoes it in the result (line 565); no caching keyed on it
- [file, verified] `examples/novywave/RUN.bn:237-243` — real_cursor_time_tick = cursor |> Number/round(to: 1): pointer moves produce equal rounded values; today the fingerprint text HOLD dedupes, under D32 each move is a write
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:190-193` — notes' Model C table says live queries are 'keyed by argument equality. Restarts when the key changes' — this is the equality filter D32 forbids; the plan adopted D32 but kept D31's 'change' wording
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100,101,105` — D31 'restarts when its arguments change'; D32 'every write fires ... a filter ... is a library function over HOLD'; D36 're-fetches when its arguments update'
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pI_equal_doc.bn "[('init',None,{}),('p1','store.press',{}),('p2','store.press',{})]" store.v,store.v_fires,store.derived,store.derived_fires` — today an equal HOLD write fires nothing (v_fires 0, derived_fires 0); under D32 both fire per press - the same edge that would restart a live query
- (5 more evidence entries in the finder output)

```boon
page_offset: 0 |> HOLD page_offset {
    key_down.key |> WHEN { PageDown => page_offset + 256, __ => page_offset }
}
page: Wellen/hierarchy_page(artifact: artifact, offset: page_offset, limit: 256)
-- plan (D32 literal): every key press writes page_offset, so `page` cancels and re-fetches on every key
-- today: HOLD dedupes the equal write (p1 probe), no restart on non-PageDown keys
-- developer expects: a fetch only when the offset actually changes; fix under (a): `__ => SKIP`
```

```boon
-- S16 equal writes reach a live query
v: 1 |> HOLD v { press |> THEN { 1 } }
page: Wellen/signal_page(artifact: art, offset: v, limit: 256)
-- plan: D32 -> every press restarts and cancels the query although v stays 1; D31's wording says 'change'
-- today: equal write fires nothing (probe pI)
-- expected: documented restart-on-every-update plus `Value/distinct(v, start: 1)` for apps that want value-based restarts
```

```boon
-- examples/novywave/RUN.bn:49-59
real_file_stream_result:
    NotStarted |> HOLD real_file_stream_result {
        real_waveform_stream_mode |> WHILE {
            Active => File/read_stream(file: real_waveform_asset, chunk_bytes: 65536, retain_content: True)
            Inactive => SKIP
        }
    }
-- plan: real_waveform_asset is rewritten (equal) on every load_default_file press (RUN.bn:22-34); D32 fires → D31 restarts the stream on every press?
-- today: HOLD dedups, no restart; developer expects no restart for an equal asset
```

**Proposed plan change.** Rewrite D31/§4.3 Placement to: 'A live query restarts on every update of any argument (D32), equal or not; the run in flight is cancelled and the value keeps its last answer.' Add the idiom to hover and to LANGUAGE_SEMANTICS v2: arguments that must not re-fire are gated with SKIP in their HOLD (`__ => SKIP`, not `__ => page_offset`). Add a catalog (library) function over HOLD that forwards only unequal values, e.g. `Value/distinct()` (name is the owner's), so NovyWave's fingerprint dedupe has a named replacement, and correct the D34 sentence to 'request_fingerprint's echo and dedupe roles are replaced by argument echo (see the result-echo convention) and by SKIP/`distinct`'. Add a scenario to O2: equal rewrite of a live query argument → one restart per write.

[also from S1-08] Reword D31/D36 to 'restarts on every update of an argument (D32)'. Add the library function `Value/distinct(value, start)` (definition in F5) to the P0 catalog and a hover note on live queries ('restarts on every update of `offset`; wrap in Value/distinct to restart only on different values'). Add a runtime scenario with an argument rewritten faster than a fake query completes to the effect-log tests (R9).

[also from S5-06] §4.3: add "A live query's arguments 'change' when any argument fires (D32); an app that wants dedup wraps the argument in the library `Change/distinct` over HOLD" — or state the opposite as the single exception to D32. Add: "A WHILE arm stays selected while the selector's new value selects the same arm; re-selecting the same arm is not an activation — per-arm state, elements and in-flight queries are kept." Add change probes for both and an effect-log assertion in the NovyWave scenario (presses of load_default_file must not re-open the file).

**Refuter correction (high tier, medium confidence).** Lower severity to medium. Drop the 'filter not writable' claim, and cite the HOLD+THEN+SKIP form above as the library body for `Value/distinct` (the owner picks the name). Keep the concrete changes. (1) D31 wording: 'restarts on every update of an argument (D32)'. (2) Put the distinct function in the P0 catalog and add a hover note on live queries. (3) Add an O2/R9 scenario: an argument rewritten faster than a fake query completes, with the expected behaviour stated. (4) Rewrite the D34 sentence to say request_fingerprint's dedupe role moves to SKIP or distinct.

**Owner question.** When does a live query restart?
- (a) On every argument update, equal or not (consistent with D32; re-fetch on equal rewrites unless the app gates with SKIP or a `distinct` library function)
- (b) On argument inequality (engine equality filter on effect arguments only; contradicts D32 and needs an equality definition for records/bytes)
- (c) On argument update only when the argument is itself a copy (THEN/WHEN output), never for live derived arguments (hard to explain; makes placement of the argument expression change restart behaviour)
Recommendation: (a), with the wording change, the SKIP idiom in hover text, and a named library dedupe function in the catalog so the migration of NovyWave's fingerprints has a target.

---

### SEM-017 [medium] 'Newer run supersedes older' is wrong for per-press command results; two commands in flight unspecified

*Decisions: D31, D36; Plan: §1 D31; §4.5 R9 (line 614); Sources: S3; verdict: confirmed; severity high -> medium after refutation*

D31 applies supersede/cancel to queries in copy contexts too, so `more.press |> THEN { Http/get(...) }` pressed twice in flight delivers one answer; wrong whenever each press is meant to produce a result (List/append of results, logging, counting completions). For commands the plan says nothing about two runs in flight from one site. Today's executor cancels the older transient run per (invocation, owner) for every transient effect, and the HTTP runtime aborts the in-flight task, which for `Http/request` POST drops the result of a request the server may already have processed. Under D36 `Http/send` must never be cancelled, and completion order must be defined (an older run may complete last and win a HOLD). Original Boon did not drop runs: THEN bodies ran sequentially inside HOLD and in parallel outside.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100` — D31: 'in a copy context it runs once per update. Either way, a newer run from the same call site supersedes an older one still in flight'; the command sentence has no in-flight rule
- [file, verified] `crates/boon_plan_executor/src/machine.rs:19184-19215` — commit_transient_effects keeps only the latest per (invocation_id, owner) within a turn and calls cancel_pending_transient_effect_owner for every committed transient effect regardless of class
- [file, verified] `crates/boon_plan_executor/src/machine.rs:19268-19291` — cancel_pending_transient_effect_owner removes every pending call with the same invocation_id+owner and pushes it to cancelled_transient_effects
- [file, verified] `crates/boon_http_runtime/src/lib.rs:248-258` — route_runtime_turn cancels each cancelled_transient_effects call: cancellation.cancel(); task.abort() — aborts an in-flight HTTP call
- [file, verified] `crates/boon_http_runtime/tests/loopback.rs:248-290` — runtime_owner_replacement_cancels_the_exact_http_call_before_submission: second dispatch of store.request cancels the first call id; only the second completes
- [file, verified] `crates/boon_effect_schema/src/lib.rs:222-233 and 1103-1104` — Http/request is ReplaySpec::ReadOnly (transient, cancellable) although its method field allows Post/Put/Patch/Delete
- [file, verified] `~/repos/boon/docs/language/DATAFLOW.md:100-106` — original THEN processing modes: outside HOLD parallel, inside HOLD sequential/backpressure; no dropping of earlier bodies

```boon
jokes: LIST {} |> List/append(item: more.press |> THEN { Http/get(url: TEXT { /joke }) })
-- plan: a second press while the first is in flight supersedes it → one joke appended
-- today: the older transient call is cancelled (machine.rs:19205-19215) → one joke
-- developer expects: two jokes, one per press
```

```boon
receipt: NoOrder |> HOLD receipt {
    buy.press |> THEN { Http/send(method: Post, url: TEXT { /orders }, body: order) }
}
-- plan: 'exactly once per update', nothing about two in flight
-- today: the first POST task is aborted (boon_http_runtime lib.rs:253-258) although the server may have taken the order
-- developer expects: both POSTs complete; `receipt` ends with whichever finishes last
```

**Proposed plan change.** R9/D31 text: (1) Commands are never superseded or cancelled; every completion is delivered as an update, in completion order; hover says 'two presses in flight give two results, in the order they finish'. (2) Copy-context queries: default no supersede (each run is an independent copy, all completions delivered in completion order); a per-catalog-entry flag `superseding` (shown in hover) marks the entries where the newest run must win, initially Timer/deadline (which makes `text |> THEN { Timer/deadline(delay_ms: 300) }` a debounce). (3) Live-context queries keep D31's cancel-and-restart. (4) The scenario runner asserts the effect log per call site (started/cancelled/completed with run ids) so (1)-(3) are testable. List the today→plan delta explicitly in R9: 'the executor's per-owner cancel of transient effects (machine.rs:19184-19291) is removed for commands and non-superseding queries'.

**Refuter correction (high tier, medium confidence).** Lower severity to medium and split it. (a) Plan text fix: add to R9 'commands are never cancelled or superseded by a newer run; the executor's per-owner transient cancel (machine.rs:19184-19291) is removed for commands; each completion is an update in completion order'. (b) Raise the copy-context query supersede as an optional owner question with the per-press-result use case. Drop the recommendation to reverse D31's query rule by default.

**Owner question.** What happens when a copy-context run from one call site is still in flight and the site runs again?
- (a) Plan as written: the newer run supersedes/cancels the older one for queries; commands unspecified (today: cancelled too, including POST)
- (b) Never supersede in copy contexts: every run completes and is delivered in completion order (per-press results work; debounce needs a dedicated catalog entry or a live-context form)
- (c) (b) plus a per-catalog-entry `superseding` flag for the few entries where newest-wins is the point (Timer/deadline), visible in hover and the effect log
Recommendation: (c). Commands must never be cancelled under D36; per-press query results are the common case; debounce stays expressible through Timer/deadline.

---

### SEM-018 [medium] Scope, restart batching and hot reload for live queries undefined

*Decisions: D31, D35; Plan: §1 D31; §4.3 Placement; §4.7; Sources: S3; verdict: confirmed*

D31 says a live query 'starts when its scope activates … stops when its scope ends' but the plan never lists the scopes. Today the executor cancels per row when a row is removed and per owner on gate close; the plan must name plain-field activation (program start), WHILE arm entry/exit, List/map row creation/removal and FUNCTION instance as scopes. The notes said 'restarts at most once per tick, after quiescence' (two arguments updating in one step give one restart); the plan dropped that sentence. D35 says hot reload keeps state and D31 says commands never run on hot reload, but nothing says whether an in-flight live query survives a hot reload, restarts only when its call site or arguments expression changed (Q18 identities), or restarts everywhere.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100` — D31 scope wording without a scope list
- [file, verified] `crates/boon_plan_executor/src/machine.rs:19293-19300 and 20219-20232` — cancel_pending_transient_effects_for_row; gate closed → cancel_pending_transient_effect_owner
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:190-192 and 236-241` — notes: 'Restarts when the key changes, at most once per tick, after quiescence'; runtime work item 5 'reconcile queries only, once per tick'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:104 and 1231` — D35 'dev hot reload always keeps state'; Q18 element/node identities stable across edits outside the edited definition

```boon
pages: signals |> List/map(item, new: Wellen/signal_page(artifact: a, signal_ids: LIST { item.id }, ...))
-- plan: per-row live query; the row is the scope (start on row creation, cancel on removal) — must be stated
```

**Proposed plan change.** Add to §4.3 Placement/D31: 'Scopes: the program (plain fields), a WHILE arm while selected, a collection row while it exists, a FUNCTION call instance while its caller scope is active. A live query starts when its scope activates, restarts once per step after quiescence when any argument updated in that step, cancels on scope exit. Hot reload: a live query whose call site identity and argument expression are unchanged keeps its run and its last value; a changed or new site activates (starts); a removed site cancels. Commands are untouched by hot reload.' Add O2 scenarios: row removal cancels, two arguments in one step restart once, hot reload of an unrelated definition keeps the run.

**Refuter correction (low tier, medium confidence).** Narrow the finding to the two real gaps: (1) the plan drops the notes' quiescence-batching rule ('restarts at most once per tick') with no replacement statement, and (2) in-flight live-query behavior across hot reload is unspecified. Drop or soften the 'scopes are never listed' framing since D31 does name the two live-context kinds.

---

### SEM-019 [medium] Catalog effect-class table has holes (Timer, Router, host ports, streams, Log, HEAD/OPTIONS)

*Decisions: D31, D36; Plan: §1 D31/D36; §11 L2; §4.3 Contracts; Sources: S3; verdict: confirmed*

D31 names two classes but the catalog needs a third for things that are neither: Timer/interval (used at root in interval_hold.bn), Router/route, and host level ports are live-only sources (never legal in a copy context, not restarted by arguments). Nothing says what `press |> THEN { Timer/interval(...) }` is. Stream effects (File/read_stream, Content/import, Content/save) deliver many result updates from one run, but D30/D31 describe an effect result as a single 'gets its first value later' value. Log/* 'logs every update of its argument' means `TEXT { app started } |> Log/info()` at root never logs (constant, start is not an update) while the same call inside a WHEN arm body logs once per arm run: the rule needs 'including the first value'. D36 assigns GET and POST/PUT/PATCH/DELETE but not HEAD/OPTIONS (both in today's schema). The notes say the class 'comes from ReplaySpec', but ReplaySpec::ReadOnly covers Clock/wall, Random/bytes and Http/request, which D31/D36 make commands: the class must be a new explicit catalog column (query | command | source), orthogonal to replay durability (transient | outbox). Also 'live resource' suggests watching: File/read_text in a plain field re-reads only when `path` updates, never when the file changes, and Http/get at root fetches once per activation (polling needs Timer/interval + THEN).

Evidence:
- [file, verified] `crates/boon_effect_schema/src/lib.rs:203-291` — host_effect_policy: Clock/wall, Random/bytes, Http/request, Secret/verify, Timer/deadline, Wellen/* all ReplaySpec::ReadOnly; File/read_stream, Content/import (ReadOnly) and Content/save (ProcessScoped) are Stream delivery
- [file, verified] `crates/boon_effect_schema/src/lib.rs:1103-1104` — Http/request method tags include Head and Options
- [file, verified] `crates/boon_typecheck/src/lib.rs:31541-31553` — Timer/interval, Router/route, Router/go_to are registered builtins, not host effects
- [file, verified] `examples/interval_hold.bn:2 and examples/todo_mvc_physical/BUILD.bn:36` — Timer/interval at root; Log/info on a constant TEXT inside a BLOCK in a WHEN arm
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1203` — L2: 'Log/* allowed anywhere and logs every update of its argument; Clock/wall and Random/bytes are commands'
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:186` — notes 3.4: 'It comes from ReplaySpec … and never depends on argument values' — contradicted by lib.rs:222-233
- [measurement, verified] `rg -n 'Router/|Timer/interval|Content/|File/read_stream|Log/' docs/plans/BOON_COMPILER_REWRITE_PLAN.md` — only line 1203 (Log/*) matches: the plan never classifies Timer/interval, Router/*, Content/*, File/read_stream

```boon
started: TEXT { app started } |> Log/info()      -- plan (L2 literal): never logs; constant never updates, start is not an update
clock: tick |> THEN { Clock/wall() }             -- required form for a live clock; `clock: Clock/wall()` is an error (command in a live context)
later: press |> THEN { Duration[seconds: 1] |> Timer/interval() }   -- no class covers this; must be an error like SOURCE in a copy context
```

**Proposed plan change.** Add a catalog table to D31/§4.3 (P0 deliverable) with three placement classes and a separate durability column: SOURCE-class (live only, starts on activation, never in copy contexts): Timer/interval, Router/route, host level ports, `restored`. QUERY: File/read_text, read_bytes, read_stream (stream), Directory/entries, Content/import (stream), Wellen/*, Timer/deadline (superseding), Http/get (+ Head/Options), Secret/verify and Crypto/hmac_* (see the copy-only finding). COMMAND: File/write_*, Content/save (stream result), Http/send (Post/Put/Patch/Delete), Router/go_to, Clock/wall, Random/bytes, DevelopmentPasskey/* (outbox), Log/* (any context; runs on every value of its argument including the first, and once per body run in copy contexts). Add: 'One run may deliver several result updates (stream classes); each is an update; restart or cancel ends the stream.' Add hover text 'does not watch the file; re-reads when `path` updates' for read queries. State that the class is a new explicit column in host_effect_policy with the same explicit-match discipline (lib.rs:288 unreachable!).

**Refuter correction (low tier, high confidence).** None; well-supported. Consider slightly narrowing scope per-submission if a reviewer wants to file HEAD/OPTIONS and Timer/Router/streams as separate smaller items, since they have different fixes (HEAD/OPTIONS is a one-line addition to D36; Timer/Router/streams need a new catalog column as the finding proposes).

---

### SEM-020 [medium] FLUSH/DRAIN interplay with effects unspecified

*Decisions: D29, D31; Plan: §1 D29; §4.3 Change rules; Sources: S3; verdict: confirmed*

D29 fixes FLUSH boundaries but says nothing about an effect whose argument (or gate) expression FLUSHes. Today's executor discards the staged effect and propagates the payload (discard_flushed_effect_candidate); the original FLUSH spec has components bypass a FLUSHED input and 'FLUSHED triggers immediate cleanup in streaming contexts'. Under D31 the rule must cover three cases: a command with a flushed argument (skip the run), a live query whose argument becomes flushed (cancel the in-flight run; the field takes the payload; restart when the argument recovers), and a stream mid-flight. DRAIN paths must be effect-free (notes spec) but §4.3 does not carry the rule.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:98` — D29: boundaries only; no effect rule
- [file, verified] `crates/boon_plan_executor/src/machine.rs:19916-19957 and 20160-20215` — stage_effect_invocation routes a Flushed gate or intent field to discard_flushed_effect_candidate: no staging, payload propagated to the state output
- [file, verified] `~/repos/boon/docs/language/FLUSH.md:254-262 and 830-834` — component bypass logic; 'FLUSHED triggers immediate cleanup in streaming contexts'
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:609` — 'Everything between DRAIN and the destination must be pure: no SOURCE, no state read, no effect, no time (M5)'
- [file, verified] `examples/novywave/BUILD.bn:17-30` — FLUSH inside List/map feeding File/write_text: the case the rule must cover

```boon
written: export.press |> THEN {
    encode(content) |> WHEN { Ok[text] => File/write_text(path: out, text: text), error => FLUSH { error } }
}
-- plan: silent. today: the write is not staged and `written` gets the error (machine.rs:19916). Rule text needed.
```

**Proposed plan change.** Add to §4.3 Change rules: 'FLUSH and effects. If an argument or the selecting arm of an effect call FLUSHes, the call does not run and the flushed payload lands at the boundary (D29). A live query whose argument becomes flushed cancels its run in flight and restarts when the argument next updates with a value. The effect log records the skip as `skipped(flush)`.' Add to Placement: 'DRAIN sources and the path to their destination contain no effect calls (checker error).'

**Refuter correction (low tier, medium confidence).** Keep medium severity. Suggest the finding lead with case (2) (in-flight live query cancellation on a later FLUSH) and the DRAIN-must-be-effect-free carryover, since case (1) may already follow from D29's general early-return framing even though the plan doesn't spell it out.

---

### SEM-021 [medium] Effect log and scenario assertions promised by R9 have no shape; scenario runner cannot observe effects, so D31/D34/D36 untestable in O2

*Decisions: D31, D34, D36; Plan: §4.5 R9; §7 O2; §12.1; Sources: S3; verdict: confirmed*

R9, D31 and §12.1 rely on 'an effect log in the dev window and scenario runner' and 'one runtime scenario per rule', but the plan never defines the log record or the assertion syntax. Today's .scn format has only `expected_source_event` and `expect_root_text`, `boon_cli run` has no host-service runner (effects are never dispatched), and today's placement failures are internal errors without positions, so there is no baseline to diff against for effect behaviour.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/change_probes/p1_then_over_hold.scn` — scenario schema: steps with expected_source_event and expect_root_text only
- [measurement, verified] `rg -n -i effect examples/*.scn` — no scenario asserts anything about effects
- [measurement, verified] `target/release/boon_cli check scratchpad/S3/live_effect.bn (Http/request in a plain field)` — fails with an internal error 'owner root statement has no unique local or exact resource declaration authority' — no positioned placement diagnostic today
- [measurement, verified] `target/release/boon_cli check examples/novywave/BUILD.bn` — fails: 'authoritative callable File/write_text is not in the current compact ABI slice' — BUILD files have no old-engine baseline for O4
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:614, 1246` — R9 'an effect log for the dev window and the scenario runner'; 12.1 'one runtime scenario per rule'

**Proposed plan change.** Make the effect log a P0 deliverable with a fixed record: {call_site (definition + path + row key), catalog entry, class, context (live|copy), run id, arguments (typed value), event ∈ started | superseded(by run) | cancelled(scope exit | flush | restore) | completed(outcome tag) | skipped(start | flush | no value), step id}. Extend the scenario format with `expect_effects = [...]` per step and a scripted fake host (`[[step.host_answer]] run = N, outcome = {...}`) so a scenario can drive completions in chosen orders (needed for the supersede/ordering rules). Seed one scenario per rule in this review (equal-argument restart, two runs in flight per class, crash-in-flight restore, FLUSH skip, WHEN-arm command at start).

**Refuter correction (low tier, high confidence).** None; well-supported and independently reproduced. This is a solid, actionable gap (define the effect-log record and .scn assertion syntax before O2/R9 can be executed).

---

### SEM-023 [medium] FUNCTION schemes carry no effect/state row and no rule that a body inherits its caller's copy/live context

*Decisions: D26, D30, D31; Plan: §4.3 'Functions' and 'Placement'; D26; D31; Sources: S1, S3; verdict: confirmed*

The Placement rule makes HOLD/SOURCE/live queries errors in copy contexts and commands errors in live contexts, but a FUNCTION scheme carries only 'parameter predicates, PASSED, OUT scope effects and collection-write effects', and nothing says whether a function body takes the context of its call site (the original said it inherits: SNAPSHOT_VS_STREAM.md:11-14, 'The same function behaves differently based on how it's called'). After D26 `Bool/toggle` is a Boon function over HOLD, so `sel |> WHEN { A => x |> Bool/toggle(when: e) }` must be an error while the same call in a WHILE arm is fine - only possible if the scheme records 'creates state'. Likewise a helper that calls Http/send must be callable only from copy contexts, and a plain read inside a function body must be a FIRE edge at a live call site and a COPY edge at a copy call site. Today's compiler crashes internally on a HOLD-bearing function called once from a WHEN arm and once from a live field (probe pM).

Also (S3-06): D31 promises 'an inferred effect row on every FUNCTION scheme' but §4.3 Functions lists only parameter predicates, PASSED, OUT scope effects and collection-write effects. Placement is decided per context (copy vs live), and a FUNCTION body's root position inherits the caller's context (original SNAPSHOT_VS_STREAM.md), so the same function is a live resource when called from a plain field and a one-shot when called from a THEN body, and a function with a command at a body-live position may only be called from a copy context. Without stating what the row contains and how the body is checked under an inherited context, the checker cannot report `icon_code()`-style functions (BUILD.bn) or cross-role purity (notes spec X5).

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:353-357,389-392` — scheme contents; Placement rule
- [measurement, verified] `cd scratchpad/S1 && python3 step.py pM_fn_hold.bn "[('init',None,{}),('p1','store.press',{})]" store.sel,store.in_arm,store.live,store.presses` — 'invalid executable local bindings: checked state 1 occurrences ... have 2 semantic use copies and no exact semantic declaration statement' at every step
- [source, verified] `~/repos/boon/docs/language/SNAPSHOT_VS_STREAM.md:11-14,110-137` — context flag inherited by user functions
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:95` — D26: stateful builtins become Boon library functions over HOLD
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:362-368` — §4.3 Functions: scheme = parameter predicates, PASSED row, OUT scope effects, collection-write effects; no query/command row
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100` — D31: 'an inferred effect row on every FUNCTION scheme'
- [file, verified] `~/repos/boon/docs/language/SNAPSHOT_VS_STREAM.md:16 and 120-137, 163` — 'User functions → inherit caller's context'; 'The same function behaves differently based on how it's called'
- [file, verified] `examples/novywave/BUILD.bn:53-66` — icon_code(item) calls File/read_bytes (query) at its body root; called per row from List/map inside a live pipeline
- (1 more evidence entries in the finder output)

```boon
-- S15 a FUNCTION with state called from a copy and a live context (probe pM)
FUNCTION toggler() { False |> HOLD t { store.press |> THEN { t |> Bool/not() } } }
in_arm: sel |> WHEN { A => toggler()  B => False }
live: toggler()
-- plan: L6 says state in a WHEN arm is an error, but nothing carries 'creates state' through the call
-- today: internal compiler error
-- expected: error at `in_arm` ('toggler creates state; call it from a live context'), `live` fine
```

```boon
FUNCTION save_all(rows) { rows |> Text/join_lines() |> File/write_text(path: out) }
report: save_all(rows: visible)          -- plan: must be an error (command at a live position); needs the row to say so
saved: export.press |> THEN { save_all(rows: visible) }   -- legal: ctx = copy
```

**Proposed plan change.** Add to §4.3 Functions: 'A scheme also carries an inferred context row: creates state (HOLD, SOURCE, stateful builtin), runs a live query, issues a command. A call site is checked against it: state and live queries need a live call site, commands a copy call site; a function needing both is an error at its definition. Otherwise the body inherits the call site's context: its plain reads are FIRE edges at a live call site and COPY reads at a copy call site.' Show the row in hover (D31's effect row).

[also from S3-06] Add to §4.3 Functions: 'A FUNCTION body is checked once with an abstract context parameter ctx ∈ {live, copy} for its root position; nested THEN/WHEN bodies are copy and WHILE arms live regardless of ctx. The scheme's effect row lists each effect call site with its catalog class and body context (live-under-ctx, copy, or live). Rules at the call site: the instance's ctx is the caller's context; a scheme with a command at a live-under-ctx position requires ctx = copy (error otherwise: "`save_all()` runs the command File/write_text; call it from a THEN body or WHEN arm"); queries at live-under-ctx positions become live resources scoped to the call instance when ctx = live and one-shot runs when ctx = copy. The row also feeds hover, the restore re-run set (live queries only), cross-role purity and the effect log.' Add a list-row and a function-instance to the definition of 'scope' for start/cancel.

---

### SEM-024 [medium] Query results held in LATESTs and replace_all collections become durable memory, contradicting 'query results re-run, never stored'

*Decisions: D12, D15, D17, D31, D34, D35, D9; Plan: D12 (81), D9 (78), D31 (100), §4.3 Updates (380-383); Sources: S4, S5; verdict: confirmed; severity high -> medium after refutation*

D12 stores 'HOLD states, LATEST current values …'. NovyWave keeps query results in both: `real_hierarchy_page_result` is a LATEST whose arm is `Wellen/hierarchy_page(...)` inside a WHEN arm and whose value carries `rows` (a LIST of up to 256 rows); `real_waveform_open_result` is a HOLD updated by `Wellen/open(...)` inside a WHEN arm, holding `WaveformOpened[artifact]` — a host handle. D9 bans LIST/SET/MAP in HOLD but says nothing about LATEST, so LATEST is state in disguise with the same copy and persistence cost HOLD.md gives as the reason for the ban. D31 says query results are re-run on restore, but these queries sit in copy contexts, which 'run once per update', and restore is not an update (D32). After a restart the app therefore either gets a stale list/handle back from the store (contradicting 'never anything that can be recomputed') or stays at NotStarted until the user re-triggers the load. D17's `List/replace_all` authority does not remove the LATEST that still holds the page.

Also (S4-03): Today NovyWave persists `store.real_hierarchy_signal_rows` and `store.signal_catalog` as durable lists (measured), which are populated from Wellen effects. The plan turns these into `List/replace_all` authorities (plan:753, R5) whose rows are 'authoritative collection rows' (durable, plan:572), while D12 says query results are re-run on restore and never stored, and D31 classes Wellen/* as queries that 'start when the scope activates'. So after a restore either (a) the rows are restored from the store and the live query re-runs and `replace_all`s them (row-local state such as selection/expansion HOLDs is lost unless rows are matched by an app key, and there is a visible double render), or (b) the rows are not stored and NovyWave's hierarchy is empty until a possibly multi-second Wellen re-read completes, rendering nothing (L3c). The same ambiguity hits D34's own example `LATEST { Loading, request |> THEN { Loading }, Wellen/hierarchy_page(…) }`: its current value is a query answer, and D12 says LATEST current values are durable and query results are not. Neither the plan nor the persistence plan says which wins or how a re-run result merges with restored row-local state.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:81` — D12: stores HOLD states, LATEST current values, …; query results re-run on restore, commands never.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:78` — D9: LIST, SET and MAP values, and records/tagged objects containing them, may not be HOLD state (no mention of LATEST).
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100,103,381-383` — D31: query in a copy context runs once per update; D34: value keeps its last answer; §4.3: restore is not an update.
- [file, verified] `examples/novywave/RUN.bn:69-78` — HOLD real_waveform_open_result updated by Wellen/open inside nested WHEN arms.
- [file, verified] `examples/novywave/RUN.bn:80-104` — LATEST real_hierarchy_page_result with a query arm inside a WHEN arm; .rows read at 97.
- [file, verified] `/home/martinkavik/repos/boon/docs/language/HOLD.md (Supported Types section)` — The original reason for banning LIST/MAP in HOLD is copy cost and hardware mapping — equally true of a LATEST holding a list.
- [measurement, verified] `target/release/boon_cli dump-plan examples/novywave/RUN.bn --out <scratch>/nw.txt; plan['persistence']['lists']` — lists today: store.selected_signal_defaults, store.real_hierarchy_signal_rows, store.markers, store.signal_catalog, store.groups
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:81,100,103,572-573,610,753` — D12 re-run clause; D31 query semantics; D34 LATEST example; row durability; replace_all migration row
- (1 more evidence entries in the finder output)

```boon
-- examples/novywave/RUN.bn:80-92
real_hierarchy_page_result:
    LATEST {
        NotStarted
        real_waveform_open_result |> WHEN {
            WaveformOpened => Wellen/hierarchy_page(artifact: real_waveform_open_result.artifact, offset: 0, limit: 256)
            __ => SKIP
        }
    }
-- plan: LATEST current value (a HierarchyPage with 256 rows) is app memory (D12) yet 'query results are re-run on restore' (D31) — and this query cannot re-run because its WHEN arm only runs on an update (D32)
-- developer expects: after restart the page is either re-fetched or clearly 'not loaded', never a stale copy
```

```boon
-- D34 example (plan:103). D12: the LATEST is durable, the Wellen answer is not. After restore: restored LATEST = old page (stale), live query re-runs and supersedes. Plan text does not say this. Developer expects: last page shown at once, refreshed when the re-run lands.
hierarchy: LATEST {
    Loading
    request |> THEN { Loading }
    Wellen/hierarchy_page(file: store.active_file, page: store.page)
}
```

**Proposed plan change.** Add to D12/§4.3: "A value whose last update came from a query — directly, or through LATEST or HOLD — is not stored. On restore a live-context query re-runs; a copy-context query leaves its consumer at the starting arm and the compiler warns: 'the result of `Wellen/open` is lost on restart; make it live (a WHILE arm) or a `List/replace_all` authority'." Decide the LATEST/HOLD asymmetry for collections explicitly (owner question) and add a 5.1 census class 'LATEST/HOLD fed by a query in a copy context' (NovyWave has several: 32-39, 61-78, 80-92, 197-235, 302-325).

[also from S4-03] Add to §4.5: 'A query node is never a leaf. A HOLD, LATEST or collection authority that received a query answer is a leaf like any other and is restored to that answer; the live query then starts at activation and its first answer supersedes it. A `List/replace_all` authority restores its rows and their row-local leaves; when the query answers, rows are matched by the authority's row key (an app-visible key field, required for replace_all authorities fed by a query), unmatched rows are removed, and matched rows keep their row-local leaves.' Add an owner question on whether Wellen-fed lists should instead be transient (empty until re-read). Add a NovyWave restart scenario to O2 asserting the hierarchy is visible before the re-read completes and that selection survives it.

**Refuter correction (high tier, medium confidence).** Lower severity to medium. This overlaps SEM-025 on copy-context query restore, so merge that part. Keep two plan changes. (1) State in D12 which query results re-run (live context only) and whether a value last written by a copy-context query is restored or reset. (2) Put the LATEST-vs-HOLD collection asymmetry (D9 vs D15) to the owner. Add a check on whether Wellen `artifact` handles survive a restart, and if not, forbid persisting host handles.

**Owner question.** May LATEST (and persisted values generally) carry collections and host handles when HOLD may not?
- 1: Apply the D9 collection ban to LATEST arms and to anything persisted; effect-returned lists go only through List/replace_all (D17); query results are never persisted.
- 2: Lift the HOLD collection ban (fold into the P0 HOLD-alternative note) and persist by value.
- 3: Keep as written: LATEST lists are stored, HOLD lists are illegal.
Recommendation: 1 — it keeps D9, D12 and D31 consistent and makes 'state in disguise' impossible.

---

### SEM-025 [medium] Command in flight at crash leaves the app stuck at Sending; copy-context query restore undefined

*Decisions: D12, D31, D34, D35, D36; Plan: §1 D12/D34; §4.5 R9; §5.1 'Http/request (D36)'; Sources: S3; verdict: confirmed; severity high -> medium after refutation*

Under D12 LATEST current values persist and 'commands never re-run on restore'; under D34 a 'sending' UI is business logic in a LATEST. A restart between `buy.press` and the completion restores `Sending` with no run to complete it, and no rule delivers a cancelled outcome, so the UI stays at `Sending` forever (or until the user presses again). Non-outbox (transient) commands are exactly the ones this hits: File/write_text, Http/send, Random/bytes, Clock/wall. Separately, D12 says query results are re-run on restore, but a copy-context query has no trigger at restore and its copied arguments are not in D12's persisted set, so 'restore re-runs queries' is only implementable for live-context queries; the plan does not say which.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:81` — D12: persisted set = HOLD states, LATEST current values, …; 'Query results are re-run on restore, commands never'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:103` — D34: loading state is business logic in a LATEST
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1413-1428` — durable outbox with idempotency keys applies to consequential effects only; a transient command has no outbox item to resume
- [file, verified] `crates/boon_plan_executor/src/machine.rs:20240-20262 and 36627-36632` — stage_effect_invocation: ReadOnly/ProcessScoped effects are transient (in-memory pending_transient_effects); only Idempotent replay goes to the outbox
- [file, verified] `crates/boon_effect_schema/src/lib.rs:1136-1145` — HttpFailed already carries `cancelled: truth`, so a synthetic 'interrupted' completion fits the existing result schema
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:197-199` — notes: 'a transient command that is in flight during a crash runs at most once' — says nothing about how the app learns it did not complete

```boon
status: LATEST {
    Idle
    buy.press |> THEN { Sending }
    buy.press |> THEN {
        Http/send(method: Post, url: TEXT { /orders }, body: order)
        |> WHEN { HttpSucceeded => Sent, HttpFailed => Failed }
    }
}
-- crash while the POST is in flight, then restore:
-- plan: `status` is restored as Sending (D12), the command is never re-run, no completion arrives → Sending forever
-- developer expects: Failed (or Idle) after restore
```

**Proposed plan change.** Add to D12/D31/R9: (1) 'Live-context queries re-run on restore. Copy-context query results and command results are restored as last values, like any other value; they are not re-run.' (2) 'Every transient run (query or command) that is in flight at shutdown or crash is recorded in the persisted turn; on restore the runtime delivers that run's catalog `Cancelled`/`Interrupted` outcome as an ordinary completion update, so apps written with the D34 idiom leave `Sending`.' Durable-outbox commands keep the persistence plan's replay/reconcile contract. (3) Add an O2 restart scenario: crash between press and completion, expect the failure outcome after restore. (4) D36 must say whether Http/send is transient (at-most-once, interrupted outcome on restore) or durable-outbox (needs an idempotency key field); recommend transient by default.

**Refuter correction (high tier, medium confidence).** Lower severity to medium. Fix the snippet to the split form. Frame this as an owner question: synthetic Interrupted completion (b) vs a diagnostic that requires handling the 'restored' occurrence for values fed by command results (the D35 path). Drop the copy-context query restore part here and leave it to SEM-024. Keep the O2 crash-between-press-and-completion scenario.

**Owner question.** How does an app learn that a command it was waiting for did not complete because the process died?
- (a) It does not (plan as written): the app resets its own UI on the host 'restored' occurrence (D35) — every app must remember to do this for every command
- (b) The runtime delivers a synthetic cancelled/interrupted completion for each run in flight at restore (engine rule, no syntax)
- (c) All commands go through the durable outbox and are re-issued (contradicts D31 'never runs on restore' and needs idempotency keys for File/write_text, Http/send, …)
Recommendation: (b), with the copy-context restore rule spelled out in D12.

---

### SEM-026 [medium] Plan contradicts itself on durable nodes: D12 lists LATEST, §4.5 does not; §4.5 keeps a reachability rule D35 rejects

*Decisions: D12, D35; Plan: D12 (plan:81), §4.5 durable leaves (plan:569-573), §4.5 view-only transient (plan:577-578), D35 (plan:104), L13 (plan:1217); Sources: S4; verdict: confirmed; severity high -> medium after refutation*

(1) D12 (plan:81) says the durable set is 'HOLD states, LATEST current values, stateful-builtin state, authoritative collection rows, and ... the last value of an app-computed value that code reads outside its own update'. §4.5 (plan:569-573) lists only 'HOLD states; stateful-builtin state; authoritative collection rows, whose fields are those not derived from other durable leaves'. LATEST and the inferred set are missing. Today LATEST with a starting arm is persisted (counter_latest `store.count`; TodoMVC row `title`, `edited_title`, `edited_title.draft_title` are all LATESTs and are in the measured durable set), so §4.5 as written would drop leaves that exist today and that D12 requires. The row-field clause 'not derived from other durable leaves' is also ambiguous: TodoMVC row `title` is derived from `title_to_update` (RUN.bn:132-135), which is not a durable leaf, so the clause neither includes nor excludes it. (2) §4.5 says 'State reachable only from view code, such as element-local hover, is transient (L13)' (plan:577-578), while D35 (plan:104) says 'no reachability rule (moving a HOLD must not change whether it survives)' and L13's recorded answer (plan:1217) repeats D35. Element-local hover is not memory at all under D35 (it is a host port), so the §4.5 bullet is both stale and wrongly motivated. (3) The persistence plan's LATEST-as-state example (BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:229-255) is consistent with revised D15 (a starting arm is legal, plan:84) and with today's behaviour; change_and_effects.md:421 calling it a contradiction is outdated, so the plan's §8 P0 doc rewrite (plan:886) should not delete that example.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:81,104,569-578,1217` — exact lines
- [measurement, verified] `target/release/boon_cli dump-plan examples/counter_latest.bn / examples/todo_mvc_physical/RUN.bn` — LATEST leaves persisted today: store.count; store.todos.title, store.todos.edited_title, store.todos.edited_title.draft_title
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:229-255` — HOLD and LATEST forms 'both lower to authoritative memory'
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:125,421` — note claims LATEST example contradicts D15 and that D12 never persists sticky payloads; both predate revised D12/D15

```boon
-- TodoMVC RUN.bn:132-135. Plan §4.5: not a HOLD, not a stateful builtin; a row field 'derived from' title_to_update -> unclear. D12: LATEST current value -> durable. Today: durable (measured). Developer expects: an edited todo title survives restart.
title: LATEST {
    title
    title_to_update
}
```

**Proposed plan change.** Replace plan:569-578 with one rule: 'Durable leaves are (1) HOLD state, including HOLDs inside library functions (D26); (2) authoritative collection rows with every row field that is itself a durable leaf under (1) or (3); (3) every history node read outside its own update as defined in §4.3 Activation (LATEST current values, THEN outputs, WHEN outputs over selectors without a start value, occurrence payloads and command results that a live reader or a foreign copy context reads). Host SOURCE levels and payloads, wire inputs, query results and everything recomputable at activation are never stored (D35). There is no reachability or view/model rule; element-local hover, focus and pressed are host ports, not memory.' Delete the 'reachable only from view code' bullet. In §8 P0 (plan:886) keep the LATEST-as-state example in BOON_PERSISTENCE_ARCHITECTURE_PLAN.md and mark change_and_effects.md:125/421 as superseded.

**Refuter correction (high tier, high confidence).** Lower severity to medium: it is a documentation contradiction with a one-paragraph fix, though it would be a correctness bug if implemented as written. Adopt the proposed §4.5 replacement text and delete the view-reachability bullet. Treat part (3) as a minor note.

---

### SEM-028 [medium] Structural-route identity is more fragile than named identity and leaks library bodies into user data

*Decisions: D26, D35; Plan: §4.5 identity (plan:574-576), Q18 (plan:1230), D26 (plan:95), D35 'moving a HOLD must not change whether it survives' (plan:104), risks 'Persistence schema drift' (plan:1248); Sources: S4; verdict: confirmed; severity high -> medium after refutation*

The plan defines identity as 'the named data path plus the structural route of the state site inside its definition' (plan:574-576) and the only definition of that route is lowering_alternative.md:105: 'field and argument names, pipe-stage/list-item/arm ordinals'. Consequences the plan does not state: (1) inserting a pipe stage before a HOLD (`0 |> Number/max(..) |> HOLD c {…}`), reordering WHILE arms, or wrapping a HOLD in a BLOCK changes the route and silently resets that leaf (the persistence plan says identity excludes 'declaration order ... derived expressions', :470-478, so this is a regression from today's named_owner_path + semantic_memory_path). (2) D26 rewrites stateful builtins as library FUNCTIONs over HOLD; `completed |> Bool/toggle(when: …)` (RUN.bn:142) then has a leaf whose route runs through the library body, so any refactor of the standard library resets every user's toggles. Today that leaf is `store.todos.state_0`, an ordinal label generated at kernel_oracle.rs:1959 even though the persistence plan (:479-484) forbids ordinal keys; cutover will reset it (allowed by D12 'reset once') but the new rule must not repeat the fragility. (3) 'moving a HOLD must not change whether it survives' (D35) says nothing about whether it keeps its data: moving `count: 0 |> HOLD` into `FUNCTION make_counter()` keeps the named path but changes the route; moving it into a WHILE arm makes it a scoped leaf that DRAIN cannot name (path-only grammar rejects 'WHEN, THEN, loops, event flow, or other runtime conditions', persistence plan:1132-1151), so the data is lost with no migration path. (4) DRAIN/DRAINING and the sequential catalog are kept only implicitly (plan:461, 580, 588, 837); the plan never says the migration tool/UX (persistence plan:1354-1362, playground DEV_MIGRATION_START_OVER at ui.rs:35) survives the rewrite.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:574-576,1230,95,104,461,580,588` — identity rule; Q18; D26; D35; DRAIN mentions
- [file, verified] `docs/plans/compiler_rewrite_notes/lowering_alternative.md:105` — site_key = (unit path, enclosing item route, structural route: field and argument names, pipe-stage/list-item/arm ordinals)
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:456-484,1132-1151,1265-1287` — MemoryId excludes declaration order and derived expressions; anonymous state without a stable path is a compile error; DRAIN path grammar; rename without DRAIN = deletion
- [file, verified] `crates/boon_compiler/src/kernel_oracle.rs:1959` — format!("state_{ordinal}") anonymous state label; TodoMVC durable leaf store.todos.state_0 (dump-plan)
- [file, verified] `crates/boon_native_playground/src/ui.rs:35; crates/boon_native_playground/src/runtime_view.rs:844-850` — DEV_MIGRATION_START_OVER and start_over() exist today

```boon
-- Same named path, different structural route. Plan: identity changes (route includes the FUNCTION body), data resets, no DRAIN needed or possible. Developer expects: same field, same data.
store: [ count: make_counter() ]
FUNCTION make_counter() {
    0 |> HOLD count { store.increment |> THEN { count + 1 } }
}
```

**Proposed plan change.** Rewrite plan:574-576 as: 'A leaf identity is (named owner path, state binder name) where the named owner path is the chain of named fields and row keys through which the leaf's value is bound, crossing FUNCTION calls and BLOCKs without recording them. A leaf inside a library FUNCTION is identified by (call-site named owner path, library function name, its declared state name); the state name is part of the library's API and changing it is a DRAIN. A structural route (arm index, list item index, pipe stage) is used only for a leaf that has no named owner, and that case is a compile error with a fix-it to name it (as BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:479-484 already requires). A leaf created inside a WHILE arm is scoped: it is durable per arm activation and cannot be drained; moving state into or out of an arm resets it.' Add the golden-vector cases: pipe stage inserted, arm reordered, HOLD moved into a FUNCTION, HOLD moved into a WHILE arm, library body refactored. State explicitly that DRAIN/DRAINING, persistence_only predecessor compiles, the sequential catalog and the dev Migration view are kept.

**Refuter correction (high tier, medium confidence).** Reword the claim. The plan does not define 'structural route'. The notes' definition (lowering_alternative.md:105) uses ordinals, which plan:575-576 ('never uses a declaration ordinal') and the persistence plan (:470-484) seem to forbid. P0 must pick one. Drop point (4), or reduce it to 'keep the dev Migration/start-over view (ui.rs:35)'. The proposed rule is sound. Its sentence 'a leaf created inside a WHILE arm ... cannot be drained; moving state into or out of an arm resets it' is new semantics and must go to the owner as a question, not as plan text. Related stale line for the same edit: plan:577-578 says 'State reachable only from view code ... is transient (L13)', but L13 is answered by D35 at plan:1217 ('all app memory persists'), and D35 forbids a reachability rule. Fix both in the same pass. Add golden vectors for: pipe stage inserted, arm reordered, HOLD moved into a FUNCTION, library body refactored.

---

### SEM-029 [medium] Server role and multi-user memory unspecified; per-request HOLDs become durable writes

*Decisions: D35, D12; Plan: D35 (plan:104), D12 (plan:81), Q18 (plan:1230), §4.4 WireEligible (plan:514), R3 (plan:608); Sources: S4; verdict: confirmed*

D35 makes the store 'durable for ... servers' and says 'server connection lists come from the host'. fjordpulse's server role keeps `request_count`, `request_path`, `request_method` in HOLDs and `search_query` as a THEN output read by the response root (Server/RUN.bn:5-30); under D12 all four are durable, so every HTTP request causes a durable write of the last request's path and query, and two concurrent requests share one 'last request'. The deployed FjordPulse already mounts a persistent volume and sets BOON_STATE_NAMESPACE (apps/fjordpulse/app.toml:118,155; deploy/fjordpulse/RUNBOOK.md:35), so this is production behaviour, not a fixture. The plan gives no way to express per-connection or per-user memory: the persistence plan only says 'user, tenant, or workspace isolation is a host namespace component' (:451, :267). State nested in rows of a host-supplied connection list would be keyed by host connection ids that do not survive restart, and D12/D35 say nothing about it. Cross-role values arriving over a wire are neither 'app-computed' nor 'host SOURCE' in D12's words, and Q18 says wire ids are not stable across redeploys, so whether a Session-role document that reads a Server value live stores a copy is undefined (storing it restores a stale server value; not storing it renders nothing until the wire reconnects).

Evidence:
- [file, verified] `examples/fjordpulse/Server/RUN.bn:5-30` — request_count/request_path/request_method HOLDs over http_request; search_query THEN output
- [file, verified] `apps/fjordpulse/app.toml:116-118,155; deploy/fjordpulse/RUNBOOK.md:35,68` — persistence.redb capability, BOON_STATE_NAMESPACE, persistent volume
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:267,451` — user/tenant scope is a host namespace component only
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:104,514,608,1230`

```boon
-- fjordpulse Server/RUN.bn:14-17. Plan D35+D12: durable leaf, written on every request, restored after redeploy. Today: same leaf set, but the server role today runs the deterministic slice. Developer expects: per-request scratch is not disk state; request_count maybe is.
request_path:
    TEXT { / } |> HOLD request_path {
        http_request.method |> THEN { http_request.path }
    }
```

**Proposed plan change.** Add a 'Roles and scopes' paragraph to §4.5: 'Values received over a wire are host inputs for persistence: never stored; a role that needs them after restart copies them into a HOLD. Rows of a host-supplied collection (connections, sessions) are host-keyed: leaves nested in them are transient and die with the row. Per-user or per-tenant memory is a store namespace chosen by the deployment (BOON_STATE_NAMESPACE); the language has no per-user scope, and a server that needs per-user memory keys it in an app collection by a user id it receives.' Add fjordpulse's server role to the S4 census as the sample for request-scoped HOLDs and decide whether `request_path`-style scratch should be rewritten as THEN outputs read only by the response THEN (transient under D12).

**Owner question.** What is the memory model for the server role?
- Global memory only, durable (today's shape; per-request scratch persists)
- Global memory + host-keyed transient rows for connections/sessions (proposed)
- Add a language-level per-connection scope (new construct; out of plan scope)
Recommendation: Option 2, with wire inputs treated as host SOURCE for D12.

---

### SEM-030 [medium] Restore ordering and first tick undefined: host re-supply, restored occurrence, outbox intents, FPGA power-on disagree

*Decisions: D31, D32, D35; Plan: D31 (plan:100), D32 (plan:101), D35 (plan:104), R9 (plan:614), §4.3 (plan:380-382); Sources: S4; verdict: confirmed*

(1) D35 says host level values (focused, hovered, viewport, connections) are 're-supplied after restart' and are SOURCEs; D32 says restore is not an update. If the host's initial supply is an update, `focused |> THEN {…}` fires on every restart, contradicting D32; if it is not, `connected |> THEN { Connected }` in fjordpulse Session/RUN.bn:5-11 never fires and the restored `connection` HOLD shows a stale `Connected` until the next real transition. (2) The 'restored' occurrence (D35, R9) is the only opt-out for UI reset; its timing (before or after the first published frame), whether it fires on hot reload (D35 says hot reload keeps state, so firing there would close menus on every edit) and on a cold start without an image are unspecified. (3) D31 says commands 'never run on start, restore or hot reload'; the persistence plan's durable outbox says 'Restart replays pending idempotent intents or reconciles their remote status' (:1411-1434) and today's executor implements replay policies (machine.rs:16481, 20256-20283, EffectReplay::Idempotent) used by DevelopmentPasskey (persons_pro/RUN.bn:49-58, where the command result feeds a HOLD). A crash between the durable intent and its result leaves the HOLD at `RegistrationNotRequested` forever under D31, or replays under the outbox: the plan must pick. (4) D35 says FPGA power-on 'resets every HOLD to its starting value' while the persistence plan (:1310-1314) runs FPGA migrations 'before the new design owns persistent memory' and BOON_CONSOLE.md:414,429 plans a persistent state journal and SPI flash; the plan should say the FPGA/console store is in-memory 'for now' under the deployment-picks-store rule, not by target.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100,101,104,614,380-382`
- [file, verified] `examples/fjordpulse/Session/RUN.bn:5-11` — connection HOLD driven by connected/disconnected occurrences
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:814-870,1411-1434,1310-1314` — restore builder steps 1-10 (no sources attached, no effects dispatched before publish); outbox replay; FPGA migration tooling
- [file, verified] `crates/boon_plan_executor/src/machine.rs:16481,20256-20283,36627-36631` — effect_replay_is_transient; 'no executable replay policy' for non-Idempotent; outbox states in boon_persistence/src/lib.rs:245,1644-1678
- [file, verified] `docs/architecture/BOON_CONSOLE.md:414,429` — optional persistent app/state journal; SPI flash controller when persistence lands

```boon
-- fjordpulse Session/RUN.bn:5-11. Plan: HOLD is durable, restored as Connected; `connected` is a host occurrence, not replayed; whether the host's start-up level counts as an update is unspecified. Developer expects: connection status is host truth, never stale.
connection:
    Connecting |> HOLD connection {
        LATEST {
            store.connected |> THEN { Connected }
            store.disconnected |> THEN { Disconnected }
        }
    }
```

**Proposed plan change.** Add an 'Activation sequence' list to §4.5: '1. Install restored leaves (no fires). 2. Activation evaluation (§4.3): live expressions and WHEN over valued selectors; THEN bodies, commands and occurrences do not run; live queries start and have no value until they answer. 3. Host level ports are supplied with their current values as part of activation (not updates); later host changes are updates. 4. The first published frame. 5. The first tick: the `restored` occurrence fires exactly once when a store image was restored (not on hot reload, not on a fresh start), before any host occurrence; then pending durable command intents already staged before the crash are completed by the effect worker under the outbox contract (idempotent replay or reconciliation); D31's "never runs on restore" means never re-staged by activation.' State that FPGA and console stores are in-memory by deployment today and may become flash-backed without a language change. Rewrite the fjordpulse `connection` HOLD as a host level port in the D35 census row.

---

### SEM-031 [medium] Inferred durable copies go stale after code edits; no rule for changed producing expressions

*Decisions: D12; Plan: D12 (plan:81), Q18 (plan:1230), §4.5 identity_v1 (plan:579-582), persistence plan 'Automatic Changes And Deletion' (:1265-1287); Sources: S4; verdict: confirmed*

Under D12's inferred clause a THEN or WHEN-over-occurrence output read live is stored (e.g. `label_text: press |> WHEN { __ => TEXT { pressed } }`). Its identity is the named path, and identity_v1 fingerprints types, not producing expressions (plan:579-582; persistence plan :470-478 excludes 'derived expressions'). Editing the arm to `TEXT { clicked }` and redeploying restores `pressed` until the next press; today this staleness exists only across hot reload (the value is in memory and not durable, measured), the plan extends it to cold restarts and production deploys. For HOLDs this is intended (a touched value is authority, persistence plan :311-346), but for an inferred copy the value is not authority, it is the last output of code that no longer exists. The plan must decide whether inferred leaves carry a producer fingerprint (reset on edit: no value until the next occurrence, so the document shows nothing for that part) or stay stale.

Evidence:
- [measurement, verified] `dump-plan of scratchpad/S4/then_probe.bn: memory == [] today`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:81,579-582,1230; docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:311-346,470-478,1265-1287`

```boon
-- After editing `pressed` to `clicked` and restarting. Plan: stored copy `pressed` restored (identity = named path; expression not fingerprinted). Today: not stored; nothing shown until the next press. Developer expects: either fresh code wins, or 'last value' wins, stated once.
label_text: press |> WHEN { __ => TEXT { clicked } }
```

**Proposed plan change.** Add to §4.5: 'An inferred leaf (a THEN/WHEN/LATEST output stored under D12 that is not a HOLD or collection) also fingerprints its producing expression in identity_v1; when the expression changes, the stored copy is dropped at activation and the node has no value until its next update. HOLD and collection leaves keep touched values regardless of code edits (persistence plan, Defaults And Touched Authority).' Record it as a golden-vector case and add a hot-reload scenario (edit the arm, assert the old copy is gone).

**Owner question.** When the code producing an inferred durable copy changes, should the stored copy be dropped or kept?
- Drop (producer fingerprint in identity; part of the document has no value until the next occurrence)
- Keep (last value wins; stale text after deploys until the next occurrence)
- Keep but mark stale in the dev window only
Recommendation: Drop. The copy is not authority; showing output of deleted code after a deploy is the worse surprise.

---

### SEM-032 [medium] 'Dev hot reload always keeps state' cannot hold for incompatible type changes; no dev policy

*Decisions: D35; Plan: D35 (plan:104), §4.5 (plan:586-589), persistence plan 'Automatic Changes And Deletion' (:1283-1285), 'Restore And Hot Reload' (:814-870); Sources: S4; verdict: confirmed*

D35 promises 'dev hot reload always keeps state'. The persistence plan says 'A stored type mismatch for an identity that still exists is not treated as deletion. Activation fails until a compatible type or explicit conversion is provided' (:1283-1285), and today's executor rejects any restore image whose schema_version/schema_hash differ ('migration activation is required', machine.rs:15119-15127). So changing a HOLD's type (say `0 |> HOLD` to `TEXT {} |> HOLD`) or an Optional field's presence (R2 puts presence in identity_v1, plan:607) during development either fails the hot reload or requires the DRAIN/pure-conversion ceremony for a throwaway edit. The plan does not say which, nor whether the dev window offers a per-leaf reset; today's playground has only whole-namespace Start Over (ui.rs:35, runtime_view.rs:844-850). Production behaviour is right (fail closed); development needs an explicit, cheaper policy or D35's sentence is false.

Evidence:
- [file, verified] `crates/boon_plan_executor/src/machine.rs:15111-15129` — validate_durable_restore_header: schema mismatch -> Error::InvalidPlan('... migration activation is required')
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1283-1285,814-870`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:104,586-589,607`
- [file, verified] `crates/boon_native_playground/src/ui.rs:35; crates/boon_native_playground/src/runtime_view.rs:844-850` — Start Over control exists; no per-leaf reset

**Proposed plan change.** Amend D35's sentence to: 'dev hot reload keeps every leaf whose identity and type are unchanged; an incompatible leaf is reset to its starting value in the dev deployment, listed in the dev window with an undo (keep old build), and is a migration error in a release build.' Add this to R2's executor/validator scope (plan:607) and to the S1 exit criteria (an edit that changes a HOLD's type keeps the other leaves).

---

### SEM-033 [medium] D35 census undersized: existing hover/focus/pointer persisted leaves and catalog port list not enumerated

*Decisions: D35; Plan: D35 (plan:104), §5 'Host values mirrored in HOLDs' (plan:762), R9 (plan:614); Sources: S4; verdict: confirmed*

The migration table lists only TodoMVC's focus/blur HOLD for D35 (plan:762, 'census'). Today's NovyWave plan persists `store.waveform_hover_zoom_center_value`, `store.waveform_hover_pointer_x_text`, `store.focused_control_label`, `store.waveform_click_pointer_x_text`, `store.marker_focus_label` and `store.pressed_control_label` (measured), and fjordpulse's Session role mirrors connection status in a HOLD (Session/RUN.bn:5-11). Under D35 these should become host level ports, which means the catalog must provide `hovered`/`focused`/`pressed` levels per element (today `hovered` exists as a port, plan:762 says `focused` is to be added), pointer position levels for waveform canvases, and a connection level; R9 names only `focused` and 'server connection lists'. Without the port list the D35 rewrite cannot be sized, and every leaf that stays a HOLD is stored and restored stale (a restored `pressed_control_label` shows a pressed button after restart).

Evidence:
- [measurement, verified] `dump-plan examples/novywave/RUN.bn; filter semantic_path containing hover/focus/pointer/pressed` — 6 leaves listed above
- [file, verified] `examples/fjordpulse/Session/RUN.bn:5-11; docs/plans/BOON_COMPILER_REWRITE_PLAN.md:104,614,762`

**Proposed plan change.** Extend plan:762 with the measured NovyWave and fjordpulse sites and add to R9 the exact port list D35 needs: per-element `hovered`, `focused`, `pressed`; pointer position and pointer-down levels on canvas/waveform elements; `connection` level for Session roles; `viewport`. Add the S4 census class 'HOLD written only from host level transitions' so the count is real before P3 sizing.

---

### SEM-034 [medium] D16 Cells in plain Boon has no evaluation strategy under D9 and no recursion

*Decisions: D9, D16, D21, D32; Plan: 5.1 Cells row (750); P3b (977-987); Sources: S5; verdict: confirmed*

Cells today reads `cells |> List/find(...).result` from inside a row's own `result` (formula.bn cell_result), a static cycle cells → new_cell → result → cell_result → cells guarded by Dependency/catch_cycle, which D9 removes. 5.1 says computed values will live in HOLD/collection state updated on edit events with dependency-ordered recalculation and a visited/depth guard (size L). But D9 bans MAP/LIST in HOLD; a per-row update that reads sibling rows' committed values (legal via collection-update edges) converges one hop per microstep; and a genuine circular reference (A1=B1, B1=A1) ping-pongs through *legal* edges with nothing static to bound it — the 'guard' must be app-level, carried through a non-recursive evaluator. This is a language-capability question ('can dependent recalculation with cycle detection be written in plan-Boon at all?'), not a formula-engine rewrite, and it is on the P3b critical path.

Evidence:
- [file, verified] `examples/cells/formula.bn:38-49,51-60` — cell_result via Dependency/catch_cycle over cells |> List/find; reference_or_number calls it.
- [file, verified] `examples/cells/cell.bn:53-63` — result: compute_result(formula_text) per row; value/error derived from result.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:78,85,361,421-423,750` — D9 edge kinds; D16; 'There is no recursion, so the call graph is a DAG'; 5.1 Cells row (size L).

```boon
-- examples/cells/formula.bn:38-49
FUNCTION cell_result(target_address) {
    Dependency/catch_cycle(
        value: cells |> List/find(item, if: item.address == target_address) |> WHEN { Found[value] => value.result, NotFound => MissingReference[address: target_address] }
        on_cycle: CycleError)
}
-- plan: catch_cycle removed; the cells→result→cells path is a D9 error; the replacement design is unspecified
```

**Proposed plan change.** Add a P0/P1 spike S7 'Cells under D9/D16 on paper': write a 10-cell sheet with add/sum and one circular pair in plan-Boon; state which edges close each cycle, how many microsteps a 3-deep chain takes, where the CycleError value is produced and by which rule the ping-pong stops. If it needs an engine primitive (a committed-read of sibling rows, or a bounded `List/fixpoint`), add it to the catalog before sizing P3b.

**Owner question.** Is a bounded evaluation depth (e.g. 64 generations per edit, then CycleError) acceptable for 'behaves like Excel', or must recalculation be exact for arbitrary depth?
- Bounded depth in Boon (no engine change)
- Engine primitive for dependency-ordered recalculation of a collection (marked stateful in the catalog)
- Keep Dependency/catch_cycle as the one cycle primitive
Recommendation: Decide after spike S7; bounded depth is the simplest answer consistent with D9.

---

### SEM-037 [medium] Multi-kind unions (D5) have no elimination form under D10

*Decisions: D5, D10, D24; Plan: §1 D5, D10; §4.3 'WHEN and WHILE'; §4.3 'Types'; Sources: S2; verdict: confirmed; severity high -> medium after refutation*

With patterns limited to `__`, binder, literal, `Tag`, `Tag[binders]` (D10, plan §4.3) and 'the subject is never refined inside an arm', there is no construct that narrows a kind-partitioned union to one kind: field read needs every member to have the field (T4/T6), arithmetic needs every member NUM (T2), spread needs exactly one REC member (T16), literal patterns have no singleton types and can never exhaust NUMBER/TEXT (T8, so a catch-all is mandatory and its binder has the whole union), and a binder's type is the full subject type. The only consumers that accept a multi-kind union are equality (overlap rule), DISPLAYABLE positions (interpolation, Text/concat, label shorthand — but only for NUM|TXT|bare tags, so `Oklch[...] | TEXT` is not displayable either), pass-through, and closed contracts that list the union (the style colour contract). So D5's 'sound and fully typed' unions are effectively pass-through types; the D5 example `Oklch[...] | TEXT` works only because the style contract accepts it. A developer who receives a union from a WHEN and wants the number back has no way to get it except re-tagging at the producer. The notes already contain the machinery for a residual: spec §5.4 gives a `__` arm over a parameter the symbolic difference α∖S. The plan should either adopt a residual-binder rule (spec-level, no syntax) or say plainly that multi-kind unions are pass-through and the fix-it is 'wrap the arms in tags'.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:74,79,372-379` — D5, D10, and the WHEN/WHILE pattern rules
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:170-192` — §2.3 operations on unions: field read, spread, arithmetic, equality overlap, DISPLAYABLE (T20 excludes tagged variants)
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:380-395` — §5.4 literal arms without catch-all are T8; `__` residual over a parameter is α∖S (tags only)
- [measurement, verified] `./target/release/boon_cli dump-ir <scratchpad>/S2/u2_mixed_kind.bn and u13_residual.bn` — today accepts `mixed + 1`, `mixed == 1`, `TEXT { {mixed} }` on NUMBER|TEXT and `other => TEXT { {other} }` on Oklch[..]|TEXT with no diagnostics (unsound leniency); the plan rejects `mixed + 1` (T2) and the `other` interpolation (T20)
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:428-436` — today's pattern grammar: no kind or record patterns; `Tag[field, ...]` binds payload fields by name

```boon
mixed: store.flag |> WHEN { True => 1, False => TEXT { one } }
plus: mixed + 1
-- plan: T2 'every member must be NUMBER'; no arm can recover the NUMBER
-- today: accepted, 0 diagnostics (probe u2)
-- developer expects: some way to write 'if it is a number, add 1'
```

```boon
color: store.flag |> WHEN { True => Oklch[lightness: 0.5], False => TEXT { red } }
shown: color |> WHEN { Oklch[lightness] => TEXT { {lightness} }, other => TEXT { {other} } }
-- plan as written: `other : Oklch[lightness] | TEXT` -> T20 (tagged variant not displayable)
-- with the residual binder rule: `other : TEXT` -> accepted
-- today: accepted (probe u13)
```

**Proposed plan change.** Add to §4.3 'WHEN and WHILE': (a) 'Residual binder rule: in an arm whose pattern is a binder or `__`, the binder's type (and the arm's contribution to exhaustiveness) is the subject type minus the tag variants matched by earlier `Tag`/`Tag[...]` arms; literal arms remove nothing. This does not refine the subject (D10).' With it, `color |> WHEN { Oklch[lightness] => …, other => TEXT { {other} } }` types `other: TEXT`. (b) State explicitly: 'Kinds cannot be eliminated. A value of type NUMBER | TEXT can be displayed, compared and forwarded but not computed with; the diagnostic for T2/T4/T6 on a multi-kind union carries the fix-it "give each arm a tag: `Number[value: …] | Text[value: …]`".' (c) If the owner wants kind elimination, that is a syntax decision (kind patterns `NUMBER => n`) and must be recorded as a new D-item.

**Refuter correction (high tier, high confidence).** Downgrade to medium. The owner question can be skipped for option 2: the residual binder rule is the original Boon's documented behaviour (ERROR_HANDLING.md:333-344) and spec.md:373 already states it, so the plan should adopt it in §4.3 and note that a binder is a new name, not a refinement of the subject (consistent with D10). Keep option 1 (kinds are pass-through; the fix-it suggests tags, as ERROR_HANDLING.md 'Ok Tagging' does) as plan text. Put option 3 (kind patterns) to the owner only if the S4 census finds code that needs it.

**Owner question.** How may code consume a multi-kind union?
- 1. Pass-through only (as the plan reads today): display/compare/forward/contract; fix-it says 'use tags'.
- 2. Residual binder rule (spec text only): a binder or `__` after tag arms has the subject type minus the matched variants; kinds still cannot be eliminated.
- 3. Kind patterns (`NUMBER => n`, `TEXT => t`, `[..] => r`): a syntax addition, new decision.
Recommendation: 2 now (cheap, no syntax, matches TypeScript users' expectations and the spec's α∖S residual), and document 1 for kinds. Decide 3 separately only if the S4 census finds real code that needs it; the D8 theme-as-data refactor removes the largest source of multi-kind unions.

---

### SEM-038 [medium] No way to forward a single matched variant without rebuilding every payload field

*Decisions: D10, D20, D24; Plan: §4.3 'WHEN and WHILE'; §5.1 'WHEN subject reads in arms (D10)'; Sources: S2; verdict: confirmed*

Positive selection is missing. To pass 'just the Button' or 'just the HierarchyPage' on to a callee or a contract, an arm must bind every payload field and rebuild the tagged object (`Button[label, style, element, …] => Button[label: label, style: style, …]`), because a bare `Tag` arm binds nothing and the subject keeps the full union (D10), and there is no whole-variant binder (Rust `b @ Button{..}`) or rest-spread in patterns. With D20 elements this hits view code that inspects and forwards elements (the plan says code may read, spread and build them), and with D24 all those binders must be used. The residual-binder rule (previous finding) does not help here because it only serves the negative case. Today's NovyWave relies on subject refinement instead (`StringValue => value.text`, RUN.bn:4431-4437; `HierarchyPage => real_hierarchy_page_result.rows`, RUN.bn:95-97), which D10 removes.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:79,89,93,374-376` — D10, D20, D24 and 'A bare Tag matches the variant… without binding anything'
- [file, verified] `examples/novywave/RUN.bn:4430-4437` — bare-tag arms reading fields through the subject (BinaryValue => value.bits …)
- [file, verified] `crates/boon_effect_schema/src/lib.rs:488-520` — HierarchyPage payload has 10 fields; forwarding it whole after matching needs a 10-binder rebuild
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:363-379` — spec §5.3 'selector narrowing' is the notes' answer; the plan withdrew it with D10 but replaced it with nothing for the forward-whole-variant case

```boon
FUNCTION page_rows(result) {
    result |> WHEN {
        HierarchyPage => summarize(page: result)   -- plan: `result` still has the whole union -> summarize's exact/tag predicate fails
        __ => SKIP
    }
}
-- today: accepted through selector refinement (NovyWave RUN.bn:95-97 idiom)
-- developer expects: to hand the page on without listing its 10 fields
```

**Proposed plan change.** Record the gap in §4.3 and put it to the owner. Text proposal for the census: 'S4 counts arms that forward the matched value whole (subject used as a value inside a `Tag` arm).' If the owner wants a construct, the smallest syntax is a whole-variant binder on a tag pattern, e.g. `page: HierarchyPage => pass(page: page)` (Rust `@`), typed as the single variant `HierarchyPage[...]`; the alternative is to accept the rebuild and give the T4/T6 diagnostic a fix-it that lists the full binder set.

**Owner question.** Should a tag arm be able to bind the matched variant as a whole?
- a. No: rebuild from field binders (verbose; every payload change touches every forwarding arm).
- b. Yes, via a whole-variant binder pattern (new syntax, e.g. `page: HierarchyPage =>`), typed as that single variant.
- c. Yes, via a rest binder inside the bracket (new syntax, `HierarchyPage[rows, ...rest]`).
Recommendation: b, decided in P0 alongside D10; it is one grammar rule and keeps D10's 'subject is not refined'. If declined, choose a and add the fix-it.

---

### SEM-039 [medium] spec §5.3 selector narrowing and per-variant requirements are withdrawn by D10 but the plan does not say so

*Decisions: D10, D24; Plan: §4.3 'WHEN and WHILE'; §5.1 'WHEN subject reads in arms'; Sources: S2; verdict: confirmed*

spec.md §5.3 refines stable paths inside arms and keeps 'requirements on the selector's own payload per variant' (probe x05 `r |> WHEN { A[a] => r.a  B[b] => r.b }` accepted). D10 says the subject is never refined, so under the plan x05 is an error (r.a needs a Required in every tagged variant) and the correct form is `A[a] => a`. §4.3 states D10 but not that §5.3 is withdrawn, and the 68-site breakage count in §5.1 comes from the notes' census, unverified; the spec-only sites that read the subject through *nested* paths (`real_file_stream_result.retained.content` RUN.bn:69-72 per spec §5.3) and the per-variant-requirement idiom are different classes with different fix-its (binder vs restructure).

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:363-379` — §5.3 'Selector narrowing… This is not the dropped static narrowing'; 'probe x05 … remains accepted'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:79,378-379,752` — D10; 'The subject is never refined inside an arm (D10)'; breakage row '68 (NovyWave 55)'
- [measurement, verified] `./target/release/boon_cli dump-ir <scratchpad>/S2/u9_refine.bn (parsed |> WHEN { Parsed => TEXT { {parsed.value} } … })` — today: accepted (selector refinement exists today); plan: T5/T6 error, fix `Parsed[value] => TEXT { {value} }`
- [file, verified] `examples/novywave/RUN.bn:95-97,4430-4437` — subject reads in bare-tag arms

```boon
parsed: TEXT { 12 } |> Text/to_number()
shown: parsed |> WHEN { Parsed => TEXT { {parsed.value} }, InvalidNumber => TEXT { bad } }
-- plan (D10): T5 'value is not present in every variant'; write Parsed[value] => TEXT { {value} }
-- today: accepted (probe u9)
```

**Proposed plan change.** In §4.3 'WHEN and WHILE' add: 'spec §5.3 (stable-path refinement, per-variant selector requirements) is withdrawn. Inside an arm the subject and every path through it keep the full selector type; payload fields are reached only through `Tag[binders]`.' Make S4 count three classes separately: subject field reads in tag arms (fix-it: binder), nested-path reads through the subject (fix-it: nested binder is not available — restructure), and forwarding the subject whole (see the whole-variant finding).

---

### SEM-040 [medium] LIST/SET/MAP/port join undefined; notes contradict (invariant-unify vs covariant)

*Decisions: D28, D5, D9; Plan: §4.3 'Types'; §1 D28; §4.4 Representation; Sources: S2; verdict: confirmed*

Plan §4.3 lists LIST, SET, MAP and SOURCE port as kinds and asserts the join is ACI, but gives a join rule only for records and tags. spec.md §1.5 makes collections invariant accumulators whose join *unifies* both operands' element variables ('every alias sees the same ε'), i.e. a consumer (`WHEN { A => xs, B => ys }`) mutates the producers' element types: after the WHEN, `xs |> List/map(item, new: item.b)` becomes a T5 error although xs never holds a row without b (probe u10 shape). checker.md §2 instead says 'LIST, SET and MAP elements are covariant', and D28 says a collection chosen through WHEN is a read-only *view* whose element type is the union and 'writes must name the real authority'. The plan must pick the D28 reading and give it a type-level form: a view is a covariant join, and `List/append`/`replace_all`/`Map/upsert`/… on a view (or on anything whose authority is not a static root path) is a positioned error. That in turn needs an 'authority' attribute or predicate on LIST types, including for FUNCTION parameters (`FUNCTION add(xs, x) { xs |> List/append(item: x) }`): today no example writes through a parameter (scan), so the cheapest rule is 'writes are legal only on a root-level authority reached by a static path'. Ports: spec §1.6 says 'unify π', the plan says polarized PORT(ρ⁺, ρ⁻); the plan should state the join (⊔ on reads, ⊓ on writes) in §4.3, not only in §4.4.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:340-351` — 'Types' lists the kinds and gives join rules only for record and tag members
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:106-116` — §1.5 'Collections are invariant accumulators… Joining two collection types unifies their accumulators'
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:50` — 'LIST, SET and MAP elements are covariant; PORT is invariant'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:97,500` — D28 read-only views; §4.4 'Ports are polarized (PORT(ρ⁺, ρ⁻))'
- [measurement, verified] `./target/release/boon_cli dump-ir <scratchpad>/S2/u10_elem_list.bn (xs: LIST{[a,b]}, ys: LIST{[a,c]}, zs: WHEN{True=>xs, False=>ys}, zs |> List/map(item, new: item.a))` — today: accepted; under spec §1.5 unification xs's element type would become [a, b?, c?] and any later `item.b` read on xs an error
- [measurement, verified] `python3 scan of examples/**/*.bn for `<param> |> List/append|remove|replace_all|insert|update|Map/upsert|Map/remove|Set/add|Set/remove(` inside FUNCTION bodies: 0 hits (single-line receiver heuristic)` — no example writes through a FUNCTION parameter, so a static-path authority rule costs nothing today
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:557-561` — §8.4 only has S1 (collection in HOLD) and S3 (nested authority escape); no view-vs-authority rule

```boon
xs: LIST { [a: 1, b: 2] }
ys: LIST { [a: 5, c: 6] }
zs: store.flag |> WHEN { True => xs, False => ys }   -- D28: read-only view, element type [a: NUMBER, b?: NUMBER, c?: NUMBER]
bs: xs |> List/map(item, new: item.b)                -- covariant view join: fine; spec §1.5 unification: T5 error
zs |> List/append(item: [a: 9])                       -- D28: error 'writes must name the authority' (plan gives no type rule yet)
```

**Proposed plan change.** Add to §4.3 'Types': 'LIST<E>, SET<E>, MAP<K,V> join covariantly (E ⊔ E′); the result of a join, of a WHEN/WHILE/LATEST arm, of a FUNCTION result or parameter is a *view*. Only a collection authority (a LIST/SET/MAP literal or an effect-returned collection, reached by a static root path) accepts event-driven updates; an update whose receiver is a view is error Sx "writes must name the authority". Element writes (List/append item) are checked against the authority's element type, which is the join of its literal items and all its writes. Ports join as PORT(ρ⁺ ⊔ ρ′⁺, ρ⁻ ⊓ ρ′⁻).' Mark spec §1.5's unification as withdrawn. Add an S4 census class for writes through parameters or views.

---

### SEM-041 [medium] 'No recursive types' is an unstated, uncensused, unenforced language restriction

*Decisions: D13, D20, D9; Plan: §4.3 'Types', 'Functions', 'Contracts'; §8 S4; Sources: S2; verdict: confirmed*

The plan says the call graph is a DAG and has no recursive functions, but says nothing about recursive *data*. checker.md §7 rejects 'a HOLD whose state contains itself' with a 'recursive shape' diagnostic ('there are no μ-types'); spec §6.4 lists T15 'not infinitely nested'. A HOLD that nests its previous state (`[depth: st.depth + 1, inner: Nested[value: st]]`) or a collection whose rows contain the collection are the natural way to build trees, comment threads or JSON-like data, and today's compiler accepts the HOLD form (probe u12, 0 diagnostics). NovyWave avoids trees by flattening scope hierarchies into rows with `indent_width` (RUN.bn:1054-1064), and `Wellen/hierarchy_page` returns flat rows. Separately, D13/D20 make elements tagged objects whose payloads contain `child`/`items: LIST<…>`; a slot *type* `Button[..] | Stripe[items: LIST<Element>] | …` would itself be recursive, so the plan's 'slots check element values against the per-kind contracts' only works if contracts are per-node predicates that are never materialized as a type, and a view function's result type is the finite tree of its own body. Neither point is written down in §4.3; 'Everything is structural data' (D13) silently means 'finite data'.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:124-125` — 'A cycle found in the positive pass (e.g. a HOLD whose state contains itself) produces a "recursive shape" diagnostic; there are no μ-types.'
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:428-442` — §6.4 HOLD state type: 'not infinitely nested (T15)'
- [measurement, verified] `./target/release/boon_cli dump-ir <scratchpad>/S2/u12_recursive_hold.bn` — today: accepted with no diagnostics (st: [depth: 0, inner: Empty] |> HOLD st { press |> THEN { [depth: st.depth + 1, inner: Nested[value: st]] } })
- [file, verified] `examples/novywave/RUN.bn:1054-1064` — tree flattened into rows with indent_width and scope_key
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:82,89,436-444,364-366` — D13, D20, contracts paragraph, 'There is no recursion, so the call graph is a DAG' (functions only)

```boon
st: [depth: 0, inner: Empty] |> HOLD st {
    press |> THEN { [depth: st.depth + 1, inner: Nested[value: st]] }
}
-- plan (checker.md §7): error 'recursive shape'
-- today: accepted (probe u12)
-- developer expects: a tree/linked-list state; needs to know the rule and the flat idiom
```

**Proposed plan change.** Add to §4.3 'Types': 'Types are finite trees. A state cell, collection element or effect payload whose type would contain itself is error Tx "recursive shape", with the path. Trees are written flat (rows with parent keys) or as nested literals of fixed depth.' Add to 'Contracts': 'Slot contracts are per-node predicates (`Contract(α, Slot(root))`), never a materialized union type; a view function's result type is the finite tree of its body.' Add an S4 census class for recursive shapes, and put 'μ-types for trees/JSON-like data' on the §12.3 exploration list with the owner's answer.

**Refuter correction (low tier, high confidence).** Note in the writeup that plan:460 already names 'recursion' as a diagnostic family (so this is a specification gap, not a total blind spot) — the proposed fix (explicit finite-type sentence + idiom + S4 census class) still stands as written.

**Owner question.** Are recursive data types (trees, nested JSON-like values) out of the language for v1?
- Out for v1: positioned error plus documented flat-row idiom.
- In: μ-types with iso-recursive join/subtyping (Simple-sub supports them, but hash-consing, display and persistence fingerprints need cycle handling).
Recommendation: Out for v1, stated explicitly in §4.3 with the diagnostic and idiom; revisit in §12.3.

---

### SEM-042 [medium] Optional fields have one unnamed consumption form (spread default); HOLD partial updates become confusing errors

*Decisions: D5, D10, D33; Plan: §4.3 'Types'; §5.1 'Optional-field reads (D5)'; Sources: S2; verdict: confirmed*

D5 says reading a field that is not guaranteed present is an error, and D10 removes refinement, so a developer who receives `[a: NUMBER, b?: NUMBER]` needs an 'unwrap_or'. The spec has it — `[b: default, ...r]` makes b Required (spec §1.3(b), §3.2 spread rule) — but the plan never mentions it, and no other form exists (no `?` syntax, no record patterns). Two common producers of optional fields are (1) collections appended with partial rows (TodoMVC RUN.bn:55-63, listed in §5.1) and (2) HOLD updates that rebuild the state record without spreading it: `[a: 1, b: 2] |> HOLD st { press |> THEN { [a: st.a + 1] } }` makes `st.b` optional and `store.st.b` a T5 error. That error is *correct* (the update really drops b at runtime; today accepts it silently, probe u11), but it must carry the fix-it 'spread the previous state: `[...st, a: st.a + 1]`' or developers will read it as a checker bug. Also note the join rule cannot distinguish 'b sometimes missing' from 'b removed on purpose'.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:74,341-344` — D5 and the record-member join `[a: 1] ⊔ [b: 2]` is `[a?, b?]`; no consumption form named
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:90-96,218-224` — §1.3 'Using an opt field' (a) contract, (b) `[f: default, ...r]`, (c) not read; §3.2 spread makes opt→req when an earlier explicit field exists
- [measurement, verified] `./target/release/boon_cli dump-ir <scratchpad>/S2/u11_hold_records.bn and u1_when_records.bn` — today accepts both `st.b` after a partial HOLD update and `shape.a` on WHEN{[a:1],[b:2]} with 0 diagnostics; the plan rejects both (T5)
- [file, verified] `examples/todo_mvc_physical/RUN.bn:55-63` — append of `[title: title_to_save]` to rows with `completed`

```boon
st: [a: 1, b: 2] |> HOLD st { press |> THEN { [a: st.a + 1] } }
shown: store.st.b
-- plan: st : [a: NUMBER, b?: NUMBER]; `st.b` is T5 (correct: the update drops b)
-- today: accepted (probe u11); at runtime b disappears after the first press
-- developer expects: an error that says 'update at line N drops `b`; write [...st, a: st.a + 1]'
```

**Proposed plan change.** Add to §4.3 'Types' after the record-member bullet: 'An Optional field is consumed by (1) a contract that lists it as optional, or (2) a defaulting spread `[f: default, ...r]`, which makes f Required with type default ⊔ r.f. There is no presence test.' Give T5 a fix-it that names the producer that lacked the field and, for HOLD/collection state, proposes `[...previous, f: …]`. Extend the S4 census 'Optional fields reaching storage' to HOLD updates that drop fields.

**Refuter correction (low tier, high confidence).** None; finding and proposed change are accurate as stated.

---

### SEM-043 [medium] D18 makes same-name catalog outcome tags with different arity unjoinable; align D31 'change' with D32 'update'

*Decisions: D18, D31, D32, D34; Plan: §1 D18, D31, D34; §4.3 'Change rules'; §4.3 'Contracts' catalog; Sources: S2; verdict: confirmed*

Under D18 and spec §1.4, `Bare ⊔ Payload` for one tag name is error T12. The examples never do this inside one user type (census: the only same-file bare/tagged pairs are `Ok` in BUILD files and NovyWave's `StringValue` used as a bare *pattern* over a tagged value, which spec §5.1 allows). But the catalog does: `File/write*` answers a bare `Ok` while `File/read*`/encode answer `Ok[text]`/`Ok[encoded]` (todo_mvc_physical/BUILD.bn:20,36,60-69), so any LATEST/WHEN/HOLD that joins two such results, or a user constant `Ok` next to a query result, is T12 with no fix-it. The D34 loading idiom itself type-checks: `LATEST { Loading, request |> THEN { Loading }, Wellen/hierarchy_page(…) }` joins tags only (probe u3 shows today also accepts the shape), and the 'two arms updating in the same step' rule is not triggered because the query result always gets its value later. However the idiom only shows the loading state again on a repeated request if the query *restarts on every argument update*: D31 says 'restarts when its arguments change' while D32 says every write fires 'equal or not'; with a same-valued request the THEN fires Loading but a change-based restart would never answer, leaving the app stuck on Loading.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:87,100,101,103` — D18, D31 ('restarts when its arguments change'), D32 ('every write fires, equal or not'), D34 idiom
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:97-105` — §1.4 Bare ⊔ Payload → T12; bare pattern matches tagged variant (§5.1)
- [measurement, verified] `python3 census (scratchpad S2) over examples/**/*.bn: same-file tags used both bare and with `[`: novywave/BUILD.bn [Ok], todo_mvc_physical/BUILD.bn [Ok], novywave/RUN.bn [StringValue] (pattern only), NovyView/RUN `A`,`B` are TEXT literals (false positives)`
- [file, verified] `examples/todo_mvc_physical/BUILD.bn:20,36,60-69` — `Ok[text] => text`, `Ok => BLOCK {`, `Ok[bytes]`, `Ok[encoded]` — catalog effects reuse `Ok` with different arities
- [measurement, verified] `./target/release/boon_cli dump-ir <scratchpad>/S2/u3_latest_tag_union.bn (LATEST { Loading, press |> THEN { Loading }, press |> THEN { TEXT{12} |> Text/to_number() } })` — today: accepted; under the plan it is one tag kind, so homogeneity holds
- [file, verified] `docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:136-139` — current doc: 'Bare tags and tagged-object variants are distinct in v1' — D18 changes this; the plan should list the doc paragraph as superseded

```boon
status: LATEST {
    Idle
    write_done |> THEN { File/write_text(...) }       -- catalog: Ok | WriteError[message]
    read_done  |> THEN { File/read_text(...) }        -- catalog: Ok[text] | ReadError[message]
}
-- plan: T12 (Ok bare ⊔ Ok[text]) with no fix-it
-- developer expects: two results with an `Ok` each can share one status value
```

**Proposed plan change.** (1) Add a P0 catalog rule: outcome tag names are D18-consistent across the whole catalog (a name is bare everywhere or carries a payload everywhere; e.g. `Written` vs `Ok[text]`, or `Ok[]`→`Ok` never coexisting with `Ok[text]`), and S4 counts user code that joins effect results of different effects. (2) Give T12 a fix-it ('`Ok` here is bare, `Ok[text]` at … carries a payload; rename one'). (3) Reword D31/§4.3 'restarts when its arguments change' to 'restarts when an argument updates (D32)', and add the loading idiom as a runtime scenario in O-tests. (4) List TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:136-139 among the superseded paragraphs.

**Refuter correction (low tier, medium confidence).** Split this into two findings of different strength: (a) catalog bare/payload arity risk — downgrade to low/speculative since no real join exists today; (b) D31-vs-D32-vs-D34 loading-idiom retry gap — keep at medium, it is the actionable core and is not addressed elsewhere in the plan.

---

### SEM-045 [medium] D13/D20 elements-as-data: state developer gain and error locations; slot contracts need NoElement unions and Reference[element:]

*Decisions: D13, D20, D22; Plan: §4.3 Contracts (452-457); §4.6 (624-652); S3 (910); Sources: S5; verdict: confirmed*

What a developer gains, concretely: elements can be decorated (`[...button, style: [...button.style, width: 200]]`), passed through user FUNCTIONs and lists (original todo_mvc.bn footer_section(align, child)), and matched (`Button[label] => label`). What moves: a wrong key or value in a hand-built element is reported at the slot that consumes it (items:, child:, label:), not at the literal — the plan should say explicitly that this is still static (closed contracts, §4.3) and that the diagnostic carries the construction span, otherwise readers assume 'template errors become runtime data errors'. Gaps: (1) `WHILE { True => Scene/Element/stripe(...), False => NoElement }` produces `Stripe[...] | NoElement`; the per-kind slot contract must accept unions of element kinds plus NoElement — §4.3 Contracts does not say so. (2) `Reference[element: todo.todo_elements.todo_title_element]` names an events record `[events: [double_click: SOURCE]]`, not an element value; under D20 the contract key is a misnomer and the identity used must be specified (4.6 identity = construction site + instance path + row key, which an events record lacks).

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:82,89,452-457,624-652` — D13, D20, §4.3 Contracts (slots check element values), §4.6 identity rules.
- [file, verified] `examples/todo_mvc_physical/RUN.bn:469-481,583-586,756-759` — WHILE arms yielding element | NoElement into items.
- [file, verified] `examples/todo_mvc_physical/RUN.bn:100-111,688` — Reference[element: todo.todo_elements.todo_title_element] where that value is an events record.
- [file, verified] `/home/martinkavik/repos/boon/playground/frontend/src/examples/todo_mvc/todo_mvc.bn:547-558` — footer_section(align, child) — elements as values through user functions.

**Proposed plan change.** §4.3 Contracts: add 'a slot accepts a union of element kinds and NoElement; contract diagnostics at a slot carry the construction span of the offending element'. §4.6/S3: define what `Reference[element:]` (and label targets) point to under D20 — an element value's identity or its events record — and how a hand-built element obtains an identity. Add one sentence to D13's rationale listing the concrete gains above so migrators know what the change buys.

**Refuter correction (low tier, medium confidence).** Cite Q6 (spec.md:852,939) directly in the finding so the owner sees this is a known open question, not a new one, and frame the ask as 'resolve Q6 and state the answer in §4.6' rather than reporting an entirely fresh gap.

---

### SEM-049 [medium] Decision composition matrix (18 pairs): 9 compose, 6 gap, 3 conflict

*Decisions: D9, D12, D13, D14, D15, D17, D21, D23, D25, D26, D28, D30, D31, D32, D33, D34, D35, D5, D10, D4, D8; Plan: §1 (64-108); §4.3; §11 (1195-1234); Sources: S5; verdict: confirmed*

D9-D15: gap — LATEST may carry lists and query results that HOLD may not (see LATEST finding). D9-D17: compose — `List/replace_all` is a collection-update edge; NovyWave page rows fit. D9-D26: gap — the library Bool/toggle closes TodoMVC's cycle through HOLD's piped reset edge, which §4.3's edge list does not name. D9-D33: same gap (reset edge classification). D12-D30: compose once WHEN start evaluation is defined (a copied value is recomputed on restore); the D12 clause 'read outside its own update' still needs a definition. D12-D35: gap — 'all app memory persists' restores per-row `editing` HOLDs (TodoMVC 112-124, Cells cell.bn:43-51) in edit mode with focus: True; D35 says apps add business logic on a host 'restored' occurrence, but no example has it, 5.1 has no census class for it, and the catalog does not yet define `restored`. D13-D23: conflict — exact reads vs element/row helpers. D14-D15: compose — persons_pro's self-naming LATESTs (335, 459) become 'unknown name' errors with the HOLD fix-it. D15-D32: conflict — a derived starting arm ties with THEN arms (persons_pro 359-369). D21-D30: compose — copy contexts read the committed snapshot plus the input's new value; TodoMVC's title_to_save → todos append → new_todo_text reset chain works across microsteps. D25-L6: wording conflict. D28-D17: compose — writes name `todos`; visible_todos is a retain. D30-D31: gap — a command in a WHEN arm evaluated at activation must not run (covered by the start-evaluation finding). D31-D32: gap — equal rewrites restarting queries. D34-D35: gap — copy-context query results after restart. D5-D10: compose — formula.bn:62-69 `error => error` catch-all binder yields the typed union. D4-D8: compose — but the theme draft's `primary: mode |> WHEN { Light => Oklch[…], Dark => TEXT { #b83f45 } }` is an `Oklch | TEXT` union (D5), so the color contract must accept both (it does today: RUN.bn:317 vs counter.bn:48). D23-D13: see D13-D23.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:64-108` — D1-D36 read in full.
- [file, verified] `examples/todo_mvc_physical/RUN.bn:39-62,112-124,136-143` — D21-D30 chain; per-row editing HOLD; Bool/toggle cycle.
- [file, verified] `examples/cells/cell.bn:43-51` — Per-row editing HOLD in Cells (2,600 rows) persists under D35.
- [file, verified] `examples/cells/formula.bn:62-69` — D5-D10 catch-all binder pattern.
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/migration/todo_theme_new/Classic.bn.txt:14-16` — Mixed-kind token value in the D8 draft.
- [file, verified] `examples/persons_pro/RUN.bn:332-336,355-369,455-464` — D14-D15 and D15-D32 instances.

```boon
-- D12-D35 gap, examples/todo_mvc_physical/RUN.bn:112-124 (per-row)
editing: False |> HOLD editing { LATEST { double_click |> THEN { True }, key_down.key |> WHEN { TEXT { Enter } => False, TEXT { Escape } => False, __ => SKIP }, blur |> THEN { False } } }
-- plan (D35): persists; after restart the todo is still in edit mode with focus: True
-- fix per D35: add `host.restored |> THEN { False }` — the catalog must define `restored`; no example has it yet
```

**Proposed plan change.** Add the matrix (with verdicts) as an appendix to §4.3 or §11 and close each 'gap' row in P0: D9 edge classification for reset edges; D12 'read outside its own update' definition; a `restored` host occurrence in the catalog plus a 5.1 census class 'UI-state HOLDs that need a restored arm'; the query-restart rule; the WHEN activation rule.

**Refuter correction (low tier, medium confidence).** Reword the D12-D35 cell from 'catalog does not yet define restored' to 'restored is named (D35, R9) but has no worked example and no §5.1 census class'; the rest of the matrix was only spot-checked (D12-D35, D31/D32/D34 chain) and not independently reverified cell-by-cell, so treat remaining rows as not-refuted-but-unverified in this pass.

---

### SEM-010 [low] Commands in WHEN arms over derived values run on every keystroke under D32; arm-at-start rule unspecified

*Decisions: D30, D31, D32; Plan: §4.3 Placement (390-393), Copy contexts (384-387); Sources: S3; verdict: confirmed; severity medium -> low after refutation*

A WHEN over a HOLD or derived value is a copy context, so a command arm 'runs exactly once each time the input updates'. With D32, a derived selector fires whenever any input fires, so `can_submit |> WHEN { True => Http/send(...) }` with `can_submit: name != TEXT {} && email != TEXT {}` sends on every keystroke once both fields are non-empty; a HOLD selector rewritten with an equal value (e.g. `mode` set to Save twice) writes twice. Today's HOLD dedupe (probe p1) hides the second case, so migrated code changes write counts. Developers expect `save.press |> THEN {...}` semantics. Separately, a WHEN over a value with a starting value evaluates pure arms at start (the WHEN needs a first value) but 'commands never run on start' (D31): the plan must say the WHEN's first value is 'no value yet' for a command arm, and that a record arm mixing a command with pure fields has no value until the command answers.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:101` — D32: 'a derived value fires (at most once per step) whenever one of its inputs fires. So `count > 5 |> THEN` runs on every count update'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100` — D31: command 'runs exactly once each time the input updates … never runs on start'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:380-386` — §4.3 Updates/Copy contexts: start is not an update; body runs once per input update
- [measurement, verified] `target/release/boon_cli run p1.bn --scenario p1.scn` — today an equal HOLD rewrite does not re-fire THEN (fires stays 1), so today's examples never observed the double write
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:209-222` — notes' diagnostic list (3.6) has no warning for commands in WHEN arms over derived selectors

```boon
can_submit: name.text != TEXT {} && email.text != TEXT {}
submit: can_submit |> WHEN { True => Http/send(method: Post, url: TEXT { /signup }, body: form), False => SKIP }
-- plan: one POST per keystroke once both fields are non-empty (derived fires on every input fire, D32)
-- developer expects: one POST on submit; compare `submit.press |> THEN { Http/send(...) }`
```

```boon
mode: Save |> HOLD mode { cancel.press |> THEN { Idle } }
saved: mode |> WHEN { Save => File/write_text(path: p, text: t), __ => SKIP }
-- plan: start is not an update and commands never run on start → `saved` has no value yet although the selector is Save from the start
```

**Proposed plan change.** Add to §4.3 Placement: 'A command in a WHEN arm over a value runs on every update of the selector, including equal rewrites and every fire of a derived selector. Hover: "runs Http/send each time `can_submit` updates: on every update of name or email". Warning W-cmd-derived when the selector is a derived value or a HOLD whose writers include non-SKIPping equal rewrites, with the fix-it "trigger it from the occurrence: `submit.press |> THEN { … }`".' Add: 'At start, a WHEN over a value with a starting value takes the selected pure arm's value; a command arm has no value yet until its first run completes; an arm value containing a command has no value yet.' Add an O2 scenario for both.

**Refuter correction (low tier, medium confidence).** Downgrade to low and narrow the finding to its second half only: the plan should state explicitly that a WHEN/record arm mixing a command with pure fields has no value until the command answers, and should show the HOLD-dedupe pattern as the intended fix for command-duplication in WHEN arms over derived selectors (rather than leaving developers to rediscover D32's closing sentence).

---

### SEM-014 [low] D25 'every WHEN arm's state exists' contradicts L6 (no state in WHEN arms); must mean WHILE arms, and WHILE-arm state lifetime is unstated

*Decisions: D25, D31, D32, L6; Plan: D25; L6; §4.5 'No static arm pruning (D25)'; D31; D32; Sources: S1, S5; verdict: confirmed*

D25: 'Every WHEN arm's state exists, even when a compile-time constant rules the arm out.' L6: state inside WHEN arms is an error (only WHILE arms are live scopes). So D25 as written is vacuous and must mean WHILE arms. For WHILE arms the combination 'entering a WHILE arm is not an update' (D32) + 'every arm's state exists' (D25) implies that a HOLD in a deselected arm keeps its value and does not re-initialise on re-entry, whereas D31 stops and restarts a query when the arm is left and re-entered; the asymmetry is plausible but nowhere stated, and it decides what persistence (D12) stores for arm-scoped HOLDs.

Also (S5-11): D25 says 'Every WHEN arm's state exists, even when a compile-time constant rules the arm out'. L6/§4.3 Placement make HOLD and SOURCE inside a WHEN arm an error, so a WHEN arm has no state; D25 can only refer to WHILE arms (TodoMVC `todo.editing |> WHILE { True => LIST { editing_todo_title_element(...) } }` creates a text_input with focus: True and its events). The consequence — an unreachable WHILE arm still allocates its state and elements — should be stated.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:94,1221` — D25 text; L6 answer 'WHILE arms only ... an error in WHEN arms and THEN bodies'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:381-384,100` — 'entering a WHILE arm ... not updates'; D31 'stops when its scope ends'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:94,390-393,1210` — D25; §4.3 Placement ('inside a copy context they are an error'); L6 answer.
- [file, verified] `examples/todo_mvc_physical/RUN.bn:575-588,635-657` — WHILE arm constructing a focused text_input.

**Proposed plan change.** Change D25 and §4.5 to 'Every WHILE arm's state exists ...'. Add to §4.3 Live contexts: 'State declared in a WHILE arm exists whether or not the arm is selected; it is not reset on re-entry. Live queries in an arm stop on deselection and restart on re-entry.'

[also from S5-11] D25: 'Every WHILE arm's state exists, even when a compile-time constant rules the arm out; WHEN arms carry no state (L6).'

**Refuter correction (low tier, low confidence).** Keep severity low as already rated. Recommend the plan simply reword D25 to say 'WHILE arm' if that is the intent, or, if the intent is function-instance state reachable from a WHEN arm, add one sentence making that explicit -- either fix resolves the apparent contradiction with L6 without changing behavior.

---

### SEM-022 [low] §5.1 migration table: wrong Http/request sites; BUILD.bn File/write_text placement understated

*Decisions: D31, D36; Plan: §5.1 rows 'Effect-returned lists … effect placement (D31)' and 'Http/request (D36)'; Sources: S3; verdict: confirmed*

The D36 row lists host_service_effects and server_effect_chain as the Http/request sites; host_service_effects.bn uses Clock/Random/Secret/HMAC/Timer only, the real sites are outbound_http_effect.bn and server_effect_chain.bn. The 'Effect placement (D31)' row says BUILD-file commands move into copy contexts; in both BUILD.bn files `File/write_text` is applied to the WHEN's result (`|> WHEN { code => TEXT {…} } |> File/write_text(...)`), i.e. a live position, and BUILD.bn does not compile with today's engine, so the old-engine compatibility rule and O4 cannot apply to it.

Evidence:
- [measurement, verified] `rg -n -l 'Http/request' examples` — examples/outbound_http_effect.bn, examples/server_effect_chain.bn
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:763` — 'Http/request (D36) | host_service_effects, server_effect_chain'
- [file, verified] `examples/novywave/BUILD.bn:15-30 and examples/todo_mvc_physical/BUILD.bn:16-31` — pipeline ends `|> WHEN { code => TEXT {…} } |> File/write_text(path: output_file)`
- [measurement, verified] `target/release/boon_cli check examples/novywave/BUILD.bn` — 'authoritative callable File/write_text is not in the current compact ABI slice'

**Proposed plan change.** Fix the D36 row sites to outbound_http_effect.bn and server_effect_chain.bn (and list host_service_effects.bn under a 'commands under THEN (already compliant)' note). In the D31 row add the concrete BUILD edit: move the `File/write_text` call inside the arm (`code => TEXT {…} |> File/write_text(path: output_file)`), and mark BUILD files as 'new engine only' for O4.

**Refuter correction (low tier, high confidence).** None; both sub-claims independently reproduced. Low severity is appropriate -- a one-line citation fix in §5.1 plus a note that the placement-vs-live-position question for BUILD.bn's File/write_text call needs its own decision since D31's copy/live split doesn't obviously classify a command piped immediately after a closing WHEN.

---

### SEM-036 [low] Exact (closed) parameter rows break the checker's lattice meet; two exact consumers must conflict

*Decisions: D23; Plan: §4.4 Approach; §4.4 Oracles 'Lattice-law property tests'; Sources: S2; verdict: confirmed; severity high -> low after refutation*

checker.md §2 defines the negative meet as pointwise: 'required wins over optional, Closed ∩ Open = Closed, tag sets are intersected'. With D23 exactness encoded as Closed negative rows, Closed{a} ⊓ Closed{a,b} would be Closed{a,b} under that rule, but the set of records that satisfy both consumers is empty (a record cannot have exactly {a} and exactly {a,b}). So a variable flowing to two exact consumers gets a wrong (too permissive) upper bound: `[a: 1, b: 2]` would be accepted by the consumer that demanded exactly {a}. Either the meet must return a conflict (⊥ on the negative side) or exactness must not be a bound at all. This also shows the plan's sentence 'types depend only on producers, so a use never changes a producer's type' is only true of lower bounds; a parameter's exact set is by definition consumer-derived (its negative type), and for root values the exactness check needs the final upper bound too. The plan's lattice-law property tests (§4.4 Oracles) would catch this only if closed rows are in the test space.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:50-53` — '⊓ is the pointwise intersection: required wins over optional, Closed ∩ Open = Closed'; 'Join, meet and subtyping obey the lattice laws'
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:44-45` — 'user parameter requirements are Open (width subtyping)' — written before D23; nothing in the notes handles exact parameter rows
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:474-479` — 'Producers add lower bounds and consumers add upper bounds… Types depend only on producers, so a use never changes a producer's type.'
- [reasoning, verified] `Closed{a} denotes {records with field set = {a}}; Closed{a,b} denotes {field set = {a,b}}; their intersection is ∅, so the greatest lower bound is ⊥, not Closed{a,b}`

```boon
FUNCTION f(r) { r.a + 0 }
FUNCTION k(r) { r.a + r.b }
store: [ v: [a: 1, b: 2], x: f(r: store.v), y: k(r: store.v) ]
-- plan with Closed bounds: hi(v) = Closed{a} ⊓ Closed{a,b} = Closed{a,b} -> f silently accepts b
-- correct: f(r: store.v) is the D23 error (or, under option B of the exactness finding, still an error because v is a root value entering f with an extra field)
```

**Proposed plan change.** In §4.4 state: parameter and PASSED rows are Open in the solver (width subtyping during propagation); exactness (D23) is a deferred predicate `Exact(slot, position)` evaluated on final bounds: fields(lo) must equal the demanded field set (union of Required fields over hi and the deferred demands from result flow). Add the lattice test: for Closed rows, ⊓ of two Closed rows with different field sets is a conflict; either exclude Closed rows from the bound lattice entirely (recommended) or add this case. Reword 'a use never changes a producer's type' to 'a use never changes a value's lower bound; exactness and other consumer-side checks read the final upper bound'.

**Refuter correction (high tier, medium confidence).** Downgrade to low. Change the proposal to: 'checker.md §2: for two Closed rows, a field present in only one is Absent in the other; Required ⊓ Absent is empty (conflict), Optional ⊓ Absent is Absent. Add Closed/Closed rows with differing fields to the CK1 meet-is-glb property test.' Whether D23 is a Closed tail or a deferred Exact predicate follows from the answer to SEM-035 (option B's consumer-dependent sets favour the predicate). List it as a dependency of SEM-035, not as its own high item.

---

### SEM-044 [low] S2 cost model must count non-ground result types over call paths; element trees copied per instantiation

*Decisions: D13, D20; Plan: §4.4 Per function (3-4), Model; §8 P0 spike S2; Sources: S2; verdict: confirmed; severity medium -> low after refutation*

§4.4 instantiates each scheme once per syntactic call site and 'copies only nodes that contain variables'. With D13/D20 a view function's result type is its element subtree, and that subtree is non-ground wherever a parameter (`label: title`), a PASSED read, or a port generalized with the result (spec §7 rule 1: SOURCE payloads 'generalize with the result… fresh at each instantiation') appears — which is nearly every view function. A function that calls a view function twice therefore carries two copies of the callee's non-ground result; over a call chain of depth d with fan-out, the total copied size is the fully inlined element tree (sum over call paths), not '373 constructors × depth'. That is the same quantity today's compiler pays per call path (the notes attribute today's cost to ~30× call-path amplification), only cheaper per node. The plan's S2 exit criterion ('largest scheme and instantiation counts within the model') does not name this sum, and the checker review already flagged the '~6 non-ground nodes per instantiation' assumption as unrealistic.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:486-489,522-528,890-893` — compaction/instantiation rules, the model paragraph, and S2's exit criteria
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:483-489` — §7: ports generalize with the result and are fresh per instantiation (so any result containing an unbound port is non-ground)
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:379-381,419-421` — review: element result types invalidate the '~6 non-ground nodes' and '4,096-node scheme cap' assumptions
- [source, from notes] `docs/plans/compiler_rewrite_notes/audit/language.md (from_scratch_design: 'per-call-path solving… 30x call-path amplification')` — the 30× figure is taken from notes, not re-measured
- [reasoning, verified] `f() = [a: g(), b: g()], g() = [a: h(), b: h()], … with non-ground h: result size doubles per level; ground results are memoized by (SchemeId, arg TypeIds) but fresh variables defeat the memo`

**Proposed plan change.** Rewrite S2's exit criterion: 'Measure Σ over syntactic call sites of the non-ground scheme size actually copied (= fully inlined element tree for view code) on TodoMVC and the NovyWave-shaped generator; report the ratio to the source element count.' Add two mitigations to §4.4: (a) hash-cons non-ground scheme terms modulo variable renaming (entry variables numbered by first occurrence, de Bruijn style) so identical instantiations share nodes; (b) ports bound at their own constructor (`element: [events: [press: SOURCE]]` → contract fixes π) are ground and do not generalize; only ports that escape unbound become scheme variables.

**Refuter correction (low tier, medium confidence).** Downgrade from medium to low/medium: reframe as 'tighten S2's exit criterion wording and add the two mitigations' rather than 'the plan doesn't measure this at all', since a timing-based gate and an explicit representation-fix fallback already exist.

---

### SEM-046 [low] Canonical hash-consed scheme needs a stated normal form for early cutoff and confluence oracle

*Plan: §4.4 Per function (3), Oracles; §4.7 early cutoff; Sources: S2; verdict: confirmed*

§4.4 promises 'linear compaction into a canonical, hash-consed scheme', early cutoff on equal SchemeIds, a confluence oracle, and an inline-equivalence oracle. In the Simple-sub family, simplification (removing polar-only variables, merging co-occurring variables) is a set of rewrites that is not canonical in general; canonical (minimal) forms in MLsub come from automaton minimization. Without recursive types (finite terms) a canonical form is reachable — apply the polar-occurrence and co-occurrence rewrites to a fixpoint, then hash-cons the DAG with entry variables numbered by first occurrence — but the plan must say which rewrites, prove they are confluent, and include the residual predicates (Exact, SameHeads, Comparable, TagsWithin) in the hashed form; otherwise two equal schemes may get different ids (lost cutoff, and a false confluence-oracle failure), and the checker review already found the compaction 'under-specified to the point of being unsound' (parameter roots not marked as entries; var-var edges between entry vars). The plan's 'roughly linear' cost holds for compaction, but instantiation is O(Σ non-ground scheme sizes), see the S2 finding.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:484-489,514-520` — compaction/instantiation steps and the three oracles
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:285-286` — §4.2 step 4: 'drop variables that occur only positively or only negatively; hash-cons the result' — no co-occurrence merging, no predicate normalization
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:114-129,397-398` — §7 compaction; review: 'compaction description is under-specified… unsound as written'
- [source, from notes] `https://dl.acm.org/doi/10.1145/3409006` — Parreaux 2020, The Simple Essence of Algebraic Subtyping: simplification is heuristic, not canonical (taken from literature, not re-verified against the paper text in this session)
- [source, from notes] `https://dl.acm.org/doi/10.1145/3009837.3009882` — Dolan & Mycroft 2017, MLsub: canonical forms via automaton minimization (taken from literature)

**Proposed plan change.** Replace 'canonical, hash-consed scheme' in §4.4 with: 'Schemes are normalized by (1) substituting variables that occur only positively by their lower bounds and only negatively by their upper bounds, (2) merging variables that always co-occur with the same polarity, (3) numbering entry variables by first occurrence in (params, PASSED, outer reads, result), (4) sorting residual predicates by (kind, entry indices), then hash-consed. Equality of SchemeIds is the cutoff key; the confluence oracle compares normalized schemes.' Keep 'parameter roots are entries' and 'var-var edges between retained entries become ⊓/⊔ references' from the review fix.

**Refuter correction (low tier, medium confidence).** None beyond what the finding already discloses (its own literature citations are marked verified_myself: false); keep as low severity as filed.

---

### SEM-047 [low] D7 first-appearance order lives outside TypeId; early cutoff and warm==cold oracle must exclude it

*Decisions: D7; Plan: §4.3 'Display (D7)'; §4.7; §7 O5; Sources: S2; verdict: confirmed*

D7 makes field order presentation-only: TypeIds ignore order and a side table filled during checking holds written or first-appearance order, keyed by expression or declaration. Merged types (a HOLD updated from several arms, a collection appended from several files, a root value joined across roles) take the order of first appearance in source order, which depends on unit order and on the position of each producer. On a warm edit that moves or reorders a producer, the TypeId and every SchemeId stay equal (early cutoff fires) but hover text and any diagnostic that prints the type must change. The plan does not say whether the side table is recomputed on cutoff, whether unit order is fixed by the manifest, or that O5 (warm == cold) compares display text. Without that, either hints go stale after cutoff or O5 fails spuriously.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:76,466-470` — D7 and §4.3 'Display (D7)': side table, one printer
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:132-149` — §1.8 first-appearance order 'merging left to right in source order'; 'Order is kept in a presentation side table… never in the TypeId'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:653-676` — §4.7 early cutoff on scheme ids (display side table not mentioned)

**Proposed plan change.** Add one sentence to §4.3 'Display' and §4.7: 'The presentation side table is recomputed for every checked definition on every compile (it is not part of any reuse key); unit order is the manifest order; the warm==cold oracle compares rendered text, so the printer must be a pure function of (TypeId, side table).'

**Refuter correction (low tier, high confidence).** None; finding accurate as filed.

---

### SEM-048 [low] Migration idiom HOLD x { LATEST {} } (14 sites) is illegal under new LATEST arity/unused-binder rules yet O2 requires the sequences to pass

*Decisions: D24; Plan: §4.3 LATEST rule (plan:388-391), D24/L12 (plan:1216), §5 'One-input LATEST' (plan:748), S4 census (plan:911), O2 (plan:837), §4.5 predecessor stages (plan:588-589); Sources: S4; verdict: confirmed; severity high -> low after refutation*

Every DRAINING declaration and every retiring row field in the migration examples is written as `value |> HOLD name { LATEST {} }` (counter/v2.bn:11-14, todo/v2.bn:31-35, and 14 sites across examples/migrations; 65 sites across examples). The plan makes a one-input LATEST an error (plan:390-391) and unused binders an error with fix-it (D24, plan:1216); a zero-input LATEST and an unused HOLD binder are not mentioned anywhere in the plan, the census classes (plan:911) or the migration table (plan:748 counts only one-input LATEST, '~80, mostly bytes_* fixtures'). Since the DRAINING declaration lives in the current stage (v2 contains both `count … |> DRAINING` and `click_count: DRAIN { count } |> HOLD …`), the current-stage checker, not just the persistence_only predecessor compile, must accept some 'state with no writes' form, or the three migration sequences that O2 (plan:837) and S1 (plan:908) require cannot compile.

Evidence:
- [file, verified] `examples/migrations/counter/v2.bn:11-14; examples/migrations/todo/v2.bn:17-35` — `0 |> HOLD count { LATEST {} } |> DRAINING`; `todo.title |> HOLD title { LATEST {} }`
- [measurement, verified] `rg -c 'LATEST \{\s*\}' examples/migrations --glob '*.bn' (14 sites); rg -c over examples (65 sites in 35 files)`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:388-391,748,837,908,911,1216`

```boon
-- examples/migrations/counter/v2.bn:11-14. Plan: LATEST with zero arms undefined (one-arm is an error), binder `count` unused (D24 error). Today: compiles, drains. Developer expects: a retiring leaf with no writes is expressible.
count:
    0
    |> HOLD count { LATEST {} }
    |> DRAINING
```

**Proposed plan change.** Add to §4.3: 'A LATEST with no arms is an error except as the body of a HOLD whose declaration ends in `|> DRAINING` or whose piped input is a DRAIN, where it means "no writes"; the HOLD binder may be unused there.' Add the class 'zero-arm LATEST / unused HOLD binder in migration stages: 14 sites' to the S4 census and §5 table. Alternatively, if the owner prefers a cleaner form, bring a proposal (text only) for a bodyless retiring HOLD and count it as a syntax decision, since no keyword change is otherwise needed.

**Refuter correction (high tier, medium confidence).** Retitle: 'Zero-arm LATEST (write-less HOLD), used by 65 sites including all 14 migration DRAINING/retiring declarations, is unspecified; the one-input fix-it must not be applied to it.' Remove the claims that the one-input rule and D24 make it illegal. Keep the proposed §4.3 sentence, but simpler: 'A zero-arm LATEST means no writes; it is legal as a HOLD body (the HOLD keeps its piped value), notably in DRAINING declarations.' Add the class to the §5.1 table with its count (65 total / 14 migrations). The owner question is unnecessary unless the owner wants a new bodyless form, which is a syntax change.

**Owner question.** How should a retiring or write-less leaf be written in a migration stage?
- Keep `HOLD name { LATEST {} }` legal only in DRAINING/DRAIN declarations (no syntax change)
- Allow a bodyless `value |> HOLD name` everywhere as 'state with no writes' (syntax change, wider)
- Require the predecessor stage source to be frozen and let the current stage omit the retiring declaration entirely (DRAIN names a path from the predecessor compile)
Recommendation: Option 1 now; it is a checker exception with zero syntax risk, and option 3 can be explored with the DRAIN redesign later.

---
