# Owner questions from the compiler rewrite review

Written 2026-09-30 for `docs/plans/BOON_COMPILER_REWRITE_PLAN.md` (reviewed at
a60a11d6). Status: **questions and recommendations only.** Nothing here changes
a decision; D1-D36 stand until the owner answers. Every question cites the
confirmed findings it comes from (ids as in `REVIEW.md` §3), in the refuter's
version wherever a refuter reframed, narrowed or merged it.

**§A** holds ten questions that change what the compiler accepts or how the
runtime schedules; P0 cannot write "static semantics v2" without them, and each
ends with a one-sentence answer to adopt or change (A1-A3 are the executive
summary's three). **§B** holds semantics questions P0 can carry as open items,
tagged with the phase that needs them. **§C** holds process questions whose
default applies unless the owner objects, as in plan §11 Q13-Q20. **§D** lists
dropped and merged questions.

Snippets use today's syntax; comments show what the developer writes and sees
under each option (process questions carry one only where that changes).
`Value/distinct`, `List/fold`, `Interrupted` and `next_source` are proposals;
`Http/send` is the D36 name. Recommendations weigh the owner's values:
**consistency**, **strictness**, **simplicity** and **everything is change**.

| # | question | findings |
| --- | --- | --- |
| A1 | Does a WHEN take a value at activation? | SEM-007, MIG-005, SEM-008, SEM-010 |
| A2 | What is a "step"? | SEM-004, SEM-005, SEM-002, SEM-003, MIS-003, ARC-001 |
| A3 | What does "exactly the fields the function consumes" mean? | SEM-035, MIG-011, SEM-036, EVI-003 |
| A4 | Is a stale copy an error or a hint? | SEM-012, SEM-011, MIG-007, MIG-008, SEM-013 |
| A5 | Which edges may close a cycle? | SEM-001, SEM-002, MIG-002 |
| A6 | May a LATEST or any stored value hold a collection? | EVI-008, SEM-024 |
| A7 | When does a live query restart? | SEM-016, SEM-018, SEM-043 |
| A8 | What persists, and what happens to query results at restore? | SEM-008, SEM-026, MIS-003, MIS-007, SEM-024 |
| A9 | How does plain-Boon Cells iterate? | MIG-001, SEM-034, MIG-002 |
| A10 | Where may state live below a copy? | EVI-005, SEM-014 |

---

## A. Decisions that need a second answer before P0 can exit

### A1. Does a WHEN take a value at activation?

D30 runs a copy body "once each time the input updates", D32 says starting is
not an update, and R8 says copy ops "evaluate only when the input updates".
Literally, a WHEN over a selector that has a value at start (a HOLD, a theme
mode, a literal argument) has no value until the selector updates: themed views
render nothing, and a WHEN over a literal is "never runs" (SEM-007, MIG-005).
The same gap decides restore (SEM-008) and commands in such arms (SEM-010).
plan:409-411 and WHEN_VS_WHILE.md:34-37 already assume the WHEN answers at
start; the refuters ask for that sentence and an owner choice on commands.

```boon
width: filter |> WHEN { All => 32, Active => 56, Completed => 88 }          -- `filter` is a literal argument
text: PASSED.theme_options.mode |> WHEN { Light => TEXT { Dark mode }, Dark => TEXT { Light mode } }
doubled: count |> THEN { count * 2 }                                         -- `count` is a HOLD starting at 5
saved: mode |> WHEN { Dark => Http/send(url: TEXT { /theme }, body: TEXT { dark }), Light => SKIP }
-- (a)  width 32 and "Dark mode" from the first frame; `doubled` waits for an update;
--      `saved` is an error "this arm would run at start; use THEN"  ((a'): compiles, send skipped at start)
-- (b)  width and text stay empty until the first update; `filter |> WHEN` is "never runs"
-- (c)  as (a), and `doubled` is 10 at start; the send is suppressed at start
```

- **(a) Activation copy for WHEN, never for THEN; a command under an activated
  WHEN is an error.** Not an update, so nothing downstream fires. Pro: views
  work as today; D31's "never runs on start" needs no special case; restore is
  "activate again". (a') lets the command arm compile, but the arm then has no
  value until the first update, a silent gap.
- **(b) No activation evaluation.** Con: about 1,070 WHENs become WHILE, and WHEN
  is left for decoding occurrences.
- **(c) WHEN and THEN both evaluate at activation.** Con: THEN stops meaning "on
  update".

**Recommendation: (a).** One rule (a copy answers the current value when its
input has one), strict for commands, D32 intact; a literal argument counts as a
value from the start. **Suggested answer:** "A WHEN whose selector has a value
from the start evaluates once at activation, which is not an update; THEN never
does; a command in such a WHEN arm is an error with the fix-it 'use THEN'."

*Reviewer note after critique.* Option (a) is the plan's own implied reading (plan lines 409-411 speak of "a WHEN over a value with a start value"; MIG-005) and needs one sentence in §4.3; (b) and (c) are kept only to show the alternatives. The genuinely open part is the rule for a command inside an arm that is evaluated at activation (SEM-007).

### A2. What is a "step"?

D21, D32 ("fires at most once per step") and R8 ("follow-up microsteps within a
tick") never define a step. Under microsteps a value derived over a diamond
through a HOLD fires twice per press, once with a state that never exists, and
THENs and commands over it run on that transient (SEM-004, probe pX). The same
definition settles the LATEST tie rule (SEM-005; SEM-002: persons_pro
`publish_state` is benign under microsteps, an error per tick), whether a HOLD's
piped reset or its body wins (SEM-003, MIS-003), and whether a step ingests
several host occurrences (MIS-003). Every option rewrites the scheduler
(ARC-001). Lustre and Esterel evaluate each flow once per instant; VHDL delta
cycles glitch (`research/copy_vs_live_synchronous_languages.md` items 2, 9).

```boon
store: [
    press: SOURCE
    a: 0 |> HOLD a { press |> THEN { a + 1 } }
    c: 0 |> HOLD c { a |> THEN { a * 10 } }
    x: a + c
    x_updates: 0 |> HOLD x_updates { x |> THEN { x_updates + 1 } }
    seed: a |> HOLD seed { press |> THEN { seed + 100 } }
]
-- one press. (a) rank: x goes 0 -> 11, x_updates +1; `seed` is reset and updated by the same press:
--     error. (b) microsteps: x goes 0 -> 1 -> 11, x_updates +2; seed takes 100, then the reset
--     lands one microstep later and seed ends at 1. (c) next tick: as (b), over settled ticks
```

- **(a) Rank scheduling.** One host occurrence per tick; each value updates at
  most once per tick in the fire graph's order (Phase A computes it); a copy body
  reads the pre-tick snapshot plus its input's new value (D21 as written); render
  and command staging follow the tick; two writers of one LATEST, or a HOLD's
  piped input and body, from one occurrence are a static error; each
  `Stream/pulses` pulse is a pass under the target's ceiling (MIS-002). Pro:
  glitch-free. Con: `publish_state` is restructured; Elm and Reflex use
  "leftmost" for ties (`research/change_semantics_frp_ui_excel.md` item 4).
- **(b) R8 microsteps as written.** Con: THENs and commands can run twice per
  press, once on a transient; a same-press reset silently beats the body.
- **(c) Next tick (Lustre `pre`).** Like (b) in software; differs on hardware (B11).

**Recommendation: (a),** with the reset/body collision an error like a LATEST
tie, since the owner already chose "error" for ties; MIS-003's "reset dominates"
is the fallback. **Suggested answer:** "A step is a tick: one host occurrence per
tick, every value updates at most once per tick in dependency order, and two
writers of one LATEST or one HOLD (piped input or body) from one occurrence are
an error."

### A3. What does "exactly the fields the function consumes" mean?

D23 makes record parameters exact ("reads, or forwards into another exact
position"). Taken at every hop it has no principal scheme: `h` below cannot be
typed, and a TodoMVC row cannot be passed to `todo_title_element` (SEM-035).
That breaks about 93-140 call sites, mostly NovyWave, a scanner upper bound
(MIG-011); two exact consumers of one value are uninhabitable (SEM-036); the
verdict must run after solving (EVI-003). No surveyed checker infers exactness
from usage; Roc gets D23's rationale from declared closed aliases
(`research/structural_inference_exact_records.md` I2, I10). The refuter adds D.

```boon
FUNCTION f(r) { r.a }
FUNCTION h(r) { f(r: r) + r.b }
ok: h(r: [a: 1, b: 2])
extra: h(r: [a: 1, b: 2, c: 3])
titles: todos |> List/map(item, new: todo_title_element(events: events, todo: item))  -- callee reads 2 of 6 fields
-- A: `h` is rejected at f(r: r) ("extra field b"); `extra` and `titles` are errors
-- B: `ok` compiles; `extra` is "c is never consumed by h"; `titles` is an error with the fix-it
--    todo: [completed: item.completed, title: item.title]
-- C: `extra` is an error only when h is called from a root field;  D: `titles` compiles
```

- **A. Strict per call.** Con: no principal scheme, a projection at every hop,
  and a callee reading one more field forces edits in every caller.
- **B. Consumed set, checked where a value enters.** A parameter's set is its
  reads plus the demands of everything it is forwarded to (a fixpoint over the
  call DAG), checked where a literal, root value, row or call result meets a
  parameter; forwarding narrows implicitly. Pro: principal, order-free, every
  entering field consumed below. Con: a callee edit can make a distant call an
  error, which is D23's "when a function changes later" case working.
- **C. Exact only at root call sites.** Con: helpers pass extras silently.
- **D. Exact only for record literals.** Pro: smallest migration. Con: rows reach
  helpers with unread fields, the "by accident" case D23 exists to stop.

**Recommendation: B,** with the "narrow the argument" fix-it in P2a and the S4
census counting sites under B and D. **Suggested answer:** "A value entering a
FUNCTION from outside may carry only fields consumed somewhere below that
function; forwarding a parameter narrows it implicitly."

*Reviewer note after critique.* Two refuters took different positions. The MIG-011 refuter recommends keeping D23 as written (close to option A) with an explicit narrowing fix-it at the forwarding site, for example `file_tree_row_label(row: [scope_key: row.scope_key, expanded_label: row.expanded_label, collapsed_label: row.collapsed_label])`, shipped in P2a. The SEM-035 refuter keeps the question open and adds option D. So the owner is choosing between the smallest rule with more edits (A plus fix-it) and fewer edits with a consumed-set analysis (B).

### A4. Is a stale copy an error or a hint?

Under D30 a WHEN over a state selector copies whatever else its arms read. The
census finds about 290-335 WHENs whose arms read a value that updates
independently, and at most 35 of 37 hand-checked must become WHILE (SEM-011,
MIG-007). The plan's Theme draft copies `mode` when `name` updates, so the
light/dark toggle would stop working (MIG-008); Cells `display_text` stops
following other cells (SEM-012); D28's example has the same shape (SEM-013).
React and Vue call this their main bug class, and React's linter flags it
(`research/change_semantics_frp_ui_excel.md` item 7).

```boon
display_text: editing |> WHEN { True => editing_text, False => value }                  -- cells/cell.bn:56-60
tokens: name |> WHEN { Classic => Classic/tokens(mode: mode), Professional => Professional/tokens(mode: mode) }
title_to_save: new_todo_input.events.key_down.key |> WHEN { TEXT { Enter } => new_todo_text, __ => SKIP }
-- A: all compile, hover says "copies `value` when `editing` updates", and cell B1 goes stale
-- B: display_text and tokens: "copies `value`, which updates on its own; use WHILE" (fix-it);
--    title_to_save stays legal because its selector is a key press
-- C: all are live, and title_to_save no longer copies the text at the press
```

- **A. Hover hint, as planned.** Con: about 300 silently stale sites, and the
  same trap for every later developer.
- **B. Error with the fix-it "use WHILE"** when a WHEN over a selector with a
  start value reads (directly, through callees or PASSED) a value that updates
  independently; effects in arms are exempt. Pro: decidable from the update-kind
  pass; the migration becomes fix-its. Con: a deliberate state snapshot is
  written `init |> HOLD s { x |> THEN { ... } }` (no example needs one).
- **C. Live WHEN over state, WHILE as an alias.** Con: reverses D30.

**Recommendation: B.** With A1, a WHEN over state is legal exactly when WHEN and
WHILE behave the same; either way, plan §11 L3 should read "model C revised by
D30". **Suggested answer:** "It is an error, with the fix-it 'use WHILE', when a
WHEN over a selector that has a start value reads, directly or through callees
and PASSED, a value that updates independently of that selector."

### A5. Which edges may close a cycle?

§4.3's two cycle texts disagree: plan:405-408 rejects the canonical counter, and
plan:420-421 admits a loop through a HOLD's piped input that fires forever under
D32+D33 (SEM-001, both probed). D26 makes Bool/toggle a library HOLD, so
TodoMVC's `completed` / `all_completed` cycle now enters it through its piped
input (SEM-002). Nodes for collection element fields are undefined (MIG-002,
needed by A9). The refuter's rule keeps D9 as written; letting a THEN body
close a cycle is a question against D9, listed as (b).

```boon
count: 0 |> HOLD count { press |> THEN { count + 1 } }     -- legal under every option
a: b |> HOLD a { press |> THEN { a + 1 } }                 -- with b: a + 1, an error under every option
completed: LATEST {                                        -- TodoMVC RUN.bn:138-144 today
    initial_completed
    store.elements.toggle_all_checkbox.events.click |> THEN { store.all_completed |> Bool/not() }
} |> Bool/toggle(when: todo_elements.todo_checkbox.events.click)
-- (a) error "the cycle enters a HOLD through its piped input"; the fix-it moves both updates into one body:
completed: initial_completed |> HOLD completed { LATEST {
    store.elements.toggle_all_checkbox.events.click |> THEN { store.all_completed |> Bool/not() }
    todo_elements.todo_checkbox.events.click |> THEN { completed |> Bool/not() }
} }
-- (b) today's form compiles: the THEN copies all_completed at the click, so it terminates
```

- **(a) D9 in two clauses.** The fire graph (THEN inputs, WHEN selectors, live
  reads, piped inputs, update candidates) is acyclic, and every cycle passes
  through a HOLD body, an event-driven collection update or a catalog-stateful
  builtin. A piped input never closes a cycle; D26 library HOLDs are ordinary
  HOLDs. Pro: one rule, a mechanical fix-it. Con: the rewrite above.
- **(b) Fire graph acyclic only.** Pro: the shortest terminating rule. Con:
  reverses D9's "not THEN bodies", so state can hide in a THEN.
- **(c) (a), with stateful-builtin status for D26 library functions.** Con: a
  special case user code cannot write.

**Recommendation: (a):** it keeps D9 and treats library HOLDs like user HOLDs,
D26's aim. **Suggested answer:** "The fire graph must be acyclic and every cycle
must pass through a HOLD body, a collection update or a catalog-stateful
builtin; a piped input never closes a cycle, and library HOLDs get no special
status."

### A6. May a LATEST or any stored value hold a collection?

D9 forbids collections (and records or tagged objects holding them) as HOLD
state, D15 lets a LATEST keep its last value, and D12 persists LATEST values. So
NovyWave keeps `HierarchyPage[rows: LIST]` in a persisted LATEST while the same
data in a HOLD is rejected (EVI-008, SEM-024). D17's `List/replace_all` exists
to give effect-returned lists an authority. Host handles such as a Wellen
`artifact` belong here: if they do not survive a restart, they must not be
persisted (SEM-024 correction).

```boon
rows: LIST {} |> HOLD rows { page_event |> THEN { page_event.rows } }        -- rejected by D9
page: LATEST {                                                              -- NovyWave RUN.bn:80-92 shape
    NotStarted
    opened |> WHEN { Opened[artifact] => Wellen/hierarchy_page(artifact: artifact, offset: 0, limit: 256), __ => SKIP }
}
-- 1: `page` is an error "a LATEST may not hold a LIST"; the fix-it writes
--    rows: LIST {} |> List/replace_all(with: page_result.rows)
-- 2: the HOLD form becomes legal and is stored by value;  3: `page` is stored, the HOLD form stays illegal
```

- **1. The ban covers every stored value** (HOLD, LATEST, inferred last value);
  collections live only in authorities. Pro: one rule; D17 is the one way to
  keep an effect-returned list. Con: NovyWave's page and signal paths change
  (already a §5.1 row).
- **2. Lift the HOLD ban** in the P0 HOLD-alternative note. Con: reopens D9.
- **3. As written.** Con: LATEST becomes the way around D9.

**Recommendation: 1:** D9, D12 and D17 agree, and big things stay out of state.
**Suggested answer:** "No stored value (HOLD, LATEST or inferred last value) may
contain a LIST, SET or MAP; collections live only in collection authorities, and
effect-returned lists go through `List/replace_all`."

### A7. When does a live query restart?

D31 restarts a live query "when its arguments change"; D32 says every write
fires, equal or not. Either an equal rewrite restarts the query (NovyWave rewrites
`real_signal_page_offset` to 0 on every zoom and pan press, RUN.bn:106-125) or the engine compares arguments, which D32
rules out (SEM-016). Today HOLD dedup hides this. Every every-write system
surveyed shipped an equality filter on day one
(`research/change_semantics_frp_ui_excel.md` item 1); Elm compares effect
descriptors (`research/query_command_effect_systems.md` item 1). Under a
comparing rule, D34's `request |> THEN { Loading }` can stick at `Loading`
(SEM-043). "At most once per tick" (SEM-018) follows from A2.

```boon
page_offset: 0 |> HOLD page_offset { key_down.key |> WHEN { TEXT { PageDown } => page_offset + 256, __ => page_offset } }
page: Wellen/hierarchy_page(artifact: artifact, offset: page_offset, limit: 256)
-- (a) every key press restarts `page`; the developer writes `__ => SKIP` or
--     offset: page_offset |> Value/distinct(start: 0); hover: "restarts on every update of page_offset"
-- (b) only PageDown restarts `page`, although `page_offset` still fires on every key
```

- **(a) Restart on every argument update,** with `Value/distinct` (a library
  function over HOLD; the owner picks the name) in the P0 catalog and D31
  reworded. Pro: one rule; the loading idiom stays correct;
  `request_fingerprint` gets a named replacement. Con: equal rewrites re-fetch
  until SKIP or `Value/distinct` is added.
- **(b) Restart only on a structurally different argument tuple.** Con: the one
  exception to D32, an equality for records and BYTES, a broken loading idiom.
- **(c) Restart only when the argument is a copy output.** Con: moving an
  expression changes restart behaviour.

**Recommendation: (a),** checked by a small P0 count (argument updates against
distinct tuples per site, plus a held-key scenario); revisit (b) if it shows
starvation. **Suggested answer:** "A live query restarts on every update of any
argument, at most once per tick, and the P0 catalog ships `Value/distinct` for
value-based restarts."

### A8. What persists, and what happens to query results at restore?

D12's "last value of an app-computed value that code reads outside its own
update" is never defined; §4.5 omits LATEST and inferred last values and keeps a
reachability rule D35 removed (SEM-008, SEM-026, MIS-003). SEM-008 names readings
R1-R3. With a reader-dependent set, removing a reader silently deletes data
(MIS-007). D12 says query results "re-run on restore", yet an answer written
into a LATEST or an authority is app memory (SEM-024), and the re-run's first
answer is an update that resets a HOLD piped from it
(`research/state_restore_persistence.md` item 5).

```boon
store: [
    toggle: SOURCE
    press: SOURCE
    mode: Light |> HOLD mode { toggle |> THEN { mode |> WHEN { Light => Dark, Dark => Light } } }
    mode_label: mode |> WHEN { Light => TEXT { Dark mode }, Dark => TEXT { Light mode } }
    last_action: press |> THEN { TEXT { pressed } }
    rows: LIST {} |> List/replace_all(with: page_result.rows)
]
-- after a restart. R1: mode and last_action restored; mode_label recomputed at activation (A1);
--     rows restored, then refreshed by row key when the re-run query answers.
-- R2: mode_label stored too (old code's text after an edit). R3: a LATEST with no history arm shows its start arm.
```

- **R1. Store history, recompute the rest:** HOLD state, LATEST values,
  collection rows, and the last value of any THEN or WHEN output read outside
  its own update. Pro: matches today's measured TodoMVC durable set; restore and
  FPGA power-on are one operation. Con: needs MIS-007's dev warning "stored data
  for `x` will be deleted".
- **R2. Store every copy output that feeds the document.** Con: more storage,
  stale copies of edited code.
- **R3. R1, recomputing a LATEST without a history arm.** Con: it shows its start
  arm instead of its last arm.

Query results under R1: live answers are never stored and re-run; state a query
wrote is restored, then refreshed, rows matched by key (SEM-024's alternatives:
"empty until the answer", "restore, never re-run"). Hover on a HOLD piped from a
live query says "reset by every answer, including after a restart".

**Recommendation: R1** with the warning and restore-then-refresh: D12's text made
precise. **Suggested answer:** "R1: store HOLD, LATEST,
collection rows and the last value of any history node read outside its own
update; recompute the rest at activation; live-query answers are never stored,
and state a query wrote is restored, then refreshed by key."

*Reviewer note after critique.* The MIS-007 refuter names a further option, R1': every touched last value of a THEN or WHEN output, or of a SOURCE-derived copy, is durable regardless of which code reads it. It keeps D35's "no reachability rule" literally, since R1's "read outside its own update" is a reachability rule in all but name, at the cost of storing values that nothing reads after restore.

### A9. How does plain-Boon Cells iterate?

D16 wants Excel-like Cells in plain Boon, cycles shown as an error value, no
cycle support. The §5.1 row cannot be written: no recursion, the foundations
plan says "Do not add List/fold", a THEN input on a cycle is an error, a HOLD's
own writes do not re-trigger it (D33), and a per-cell HOLD sees the committed
snapshot, lagging one edit (D21) (MIG-001, SEM-034). The refuter adds counted
`Stream/pulses` passes, which the foundations plan sanctions. Whether a row
reading another row is a cycle depends on undefined nodes (`cells[*].result` vs
`cells[#]`, MIG-002). `cells.scn`'s `expect_recomputed` encodes smart
recalculation (`research/spreadsheet_recalculation_cycles.md` items 1, 2, 6).

```boon
store: [                                         -- (a0) shape for the spike: one pulse per formula cell
    values: LIST {} |> List/replace_all(with: store.relaxed)
    relaxed: store.edit |> THEN { store.cells |> List/count() } |> Stream/pulses()
        |> THEN { relax(parsed: store.parsed, prev: store.values) }
]
sheet: List/range(from: 1, to: parsed |> List/count())             -- (a) needs List/fold and a `__` binder
    |> List/fold(init: start, __, acc: relax(parsed: parsed, prev: acc))
-- both: A1 = B1 + 1 updates within the same edit; a loop shows cycle_error in every member cell
```

- **(a0) Counted `Stream/pulses` passes over an authority.** Pro: no new builtin,
  exact, legal under A5(a). Con: the spike must show the passes settle in one
  tick and fit the 3 ms Cells budget.
- **(a) A pure `List/fold`.** Pro: exact, stateless. Con: overrides the
  foundations plan.
- **(b) K unrolled passes.** Works today (probed). Con: chains deeper than K show
  as cycles; K x N work per edit.
- **(c) A typed `Cycle` result on a same-collection lookup.** Pro: O(dependents);
  `expect_recomputed` holds. Con: cycle support, against D16 and D9.
- **(d) Recursion with a depth bound.** Con: breaks the call-graph DAG.

**Recommendation: (a0), with (a) as fallback;** reject (c) and (d) under D16/D9;
accept full recalculation per edit (Excel's volatile path) and move
`expect_recomputed` to the performance gate. A P0 spike must pass `cells.scn`'s
cycle steps before P3 sizes the row. **Suggested answer:** "Cells iterates with
counted `Stream/pulses` passes over a collection authority, falling back to a
pure `List/fold` only if pulses cannot settle within one tick; there is no cycle
primitive, and `expect_recomputed` becomes a performance check."

### A10. Where may state live below a copy?

L6 puts state and SOURCE in WHILE arms only, so D25's "every WHEN arm's state"
can only mean WHILE arms (SEM-014). The placement rule is syntactic: a FUNCTION
that creates state, called from a WHEN arm, slips through, and a WHILE nested
inside a THEN body (fibonacci, NovyWave's selected-row path) is undefined
(EVI-005). Transitivity through calls follows from L6 and D26 (a creates-state
bit in each scheme); only the nested case is a question. The foundations plan's
fibonacci intends "one activation-local state scope".

```boon
FUNCTION fibonacci(position) {
    position |> THEN {
        position |> WHILE {
            1 => 1
            n => [previous: 0, current: 1]
                |> HOLD state { n - 1 |> Stream/pulses() |> THEN { [previous: state.current, current: state.previous + state.current] } }
                |> Stream/skip(count: n - 1) |> .current
        }
    }
}
FUNCTION row_widget(label) { [label: label, press: SOURCE, open: False |> HOLD open { press |> THEN { open |> Bool/not() } }] }
view: store.kind |> WHEN { Row => row_widget(label: store.row.label), __ => SKIP }
-- (a) fibonacci compiles (a live scope per position); `view` is an error through the call: "use WHILE"
-- (b) fibonacci is an error and its HOLD must move out of the THEN; `view` is the same error
```

- **(a) A WHILE nested in a copy is a per-activation live scope,** from one run
  of the enclosing copy to the next. Pro: keeps the documented fibonacci. Con:
  one more lifetime to show in hover.
- **(b) No state anywhere below a copy.** Con: fibonacci, NovyWave's row path and
  the foundations plan's canonical example break.

**Recommendation: (a):** the original intent and L6's own words, applied through
calls. **Suggested answer:** "D25 means WHILE arms; placement applies through
calls; a WHILE nested in a THEN body or WHEN arm is a live scope that lasts
until that copy runs again."

---

## B. Semantics questions that can wait for P0

**B1. Queries in copy contexts: supersede, and echo** [P0 catalog; SEM-017,
SEM-006, SEM-043]. Superseding drops per-press answers in copy contexts. (Plan
text, no owner needed: commands are never cancelled; completions are updates in
completion order.) Options: (a) supersede as written; (b) never in copy
contexts; (c) (b) plus a per-entry `superseding` flag (Timer/deadline) shown in
hover. **Recommend (c),** plus an echo convention (a result carries the arguments
it answers), which keeps "loading" correct under A7.

```boon
answer: ask.press |> THEN { Secret/verify(secret: secret_input.text) }   -- two quick presses: (a) one answer, (c) two
page_is_current: page.offset == page_offset                             -- possible only with echoed arguments
```

*Reviewer note after critique.* The SEM-017 refuter asked not to recommend reversing D31's supersede rule by default. Treat (c) as an optional variant; the default is (a), D31 as written, unless the owner wants per-press results kept.

**B2. A command in flight when the process dies** [P4/R9; SEM-025]. D12 restores
`Sending`, D31 never re-runs the command, nothing completes. Options: (a) the app
handles the host "restored" occurrence (D35); (b) the runtime delivers one
synthetic `Interrupted` completion per run in flight; (c) the outbox re-issues
it, against D31. **Recommend (b):** every command run gets exactly one completion.

```boon
sent: buy.press |> THEN { Http/send(method: Post, url: TEXT { /orders }, body: order) }
status: LATEST { Idle, buy.press |> THEN { Sending }, sent |> WHEN { HttpSucceeded => Sent, __ => Failed } }   -- (b): Failed
```

**B3. FLUSH inside a HOLD update** [P0 spec; EVI-004]. Proposed text: FLUSH lands
at the field (D29), the HOLD's state is unchanged and not persisted, the binder
reads the stored state, and FLUSH in a HOLD initializer or LATEST start arm is
an error. Open: (i) does the field's error fire and count as its last value?
(ii) may field and binder differ? **Recommend yes to both:** an error is a value
like any other; hover explains the split.

```boon
hold_state: Ready |> HOLD hold_state { LATEST { fail |> THEN { FLUSH { HoldError } }, recover |> THEN { Recovered } } }
```

**B4. Effect classes beyond query and command** [P2b catalog; EVI-012, SEM-019,
MIS-026]. L2's default ("Log/* allowed anywhere, logs every update") is a third
class D31 lacks; Timer/interval, Router, streams and level ports are
unclassified. Options for Log: (a) a named "observer" class (no result, allowed
anywhere, never replayed); (b) a command; (c) a query. **Recommend (a)** in D31,
with Timer/interval and Router as host SOURCE ports and File/read_stream a query.

```boon
logged: TEXT { Included {count} icons } |> Log/info()   -- BUILD.bn:38, in a WHEN arm; (a) also legal in a plain field
```

**B5. Trigger identity across instances** [P2a schemes; EVI-002]. For the LATEST
tie rule, are two instances of one FUNCTION's SOURCE one trigger? **Recommend
per instance (generative)** over per syntactic site: it matches the runtime.

```boon
FUNCTION row(label) { [label: label, press: SOURCE] }
last: LATEST { Idle, row(label: TEXT { A }).press |> THEN { First }, row(label: TEXT { B }).press |> THEN { Second } }
-- per instance: legal; per site: "two arms update from the same trigger"
```

**B6. What crosses the wire between roles** [P4 distributed link; MIS-001].
Under A2(a) a producer writes an export at most once per tick. Options: (A)
every tick's write crosses; (B) coalesce per network frame; (C) coalesce only
where no transitive consumer is in a fire position. **Recommend A as the
semantics with C as an unobservable optimization,** proven by a Client+Session
O2 scenario.

```boon
saves: 0 |> HOLD saves { remote_draft |> THEN { saves + 1 } }   -- a fire position: B would undercount
```

**B7. Memory in multi-user roles** [P4 persistence; SEM-029, MIS-009]. Server:
(1) global memory only, so per-request scratch persists; (2) global plus
host-keyed transient connection rows, wire inputs as host SOURCEs; (3) a new
per-connection scope. Session after a user leaves: (a) keep until deleted; (b)
deployment-configured retention plus deletion and export; (c) in-memory.
**Recommend (2) and (b):** the deployment picks the store (D35); no syntax.

```boon
request_count: 0 |> HOLD request_count { request |> THEN { request_count + 1 } }   -- global memory: durable
```

**B8. Sensitive inputs** [P0 reconcile, R4; MIS-011]. Under D35 a typed password
becomes durable, logged and inspectable. Options: (b) keep today's host-owned
buffer, so element data holds a host reference; (a) a sensitivity class with a
taint rule; (c) document it. **Recommend (b) now** (BOON_PERSISTENCE_ARCHITECTURE_PLAN.md
"Sensitive Input And Credentials"), (a) later; browser session restore, the closest
precedent, hard-excludes password fields.

```boon
verified: submit.press |> THEN { Secret/verify(secret: password_input.text) }   -- (b): an opaque host reference
```

**B9. A released app's previous schema** [P4 persistence; MIS-007]. Options: (a)
predecessor sources compiled in `persistence_only` mode forever, so every
strictness change must keep old stages compiling; (b) frozen lowered fragments
written at release; (c) sources for dev and hot reload, fragments in packages.
**Recommend (c):** the fragments have a designed home
(`apps/*/migrations/catalog.toml`).

```boon
status: DRAIN { completed } |> WHEN { True => Done, False => Open }   -- migrations/todo/v6.bn:64, compiled forever under (a)
```

**B10. Stored copies after edits, and moved state** [P4 identity_v1; SEM-031,
SEM-028]. (i) When the producing code of an inferred durable copy changes, drop
the copy (producer fingerprint in its identity) or keep it? (ii) With identity =
named owner path + state name, does moving a HOLD into or out of a WHILE arm
reset it unless DRAIN names it? **Recommend drop, and yes,** with A8's warning
offering the DRAIN fix-it: a copy is not authority.

```boon
label_text: press |> WHEN { __ => TEXT { clicked } }   -- was TEXT { pressed }: drop shows nothing, keep shows "pressed"
```

*Also under B10 (SEM-032, PLAN_EDITS.md §1 D35 clause).* D35 says "dev hot reload always keeps state", which cannot hold when a HOLD's type changes or a leaf disappears; today restore fails on any schema mismatch. Options: (a) keep every leaf whose identity and type are unchanged, reset a changed leaf in dev with a dev-window note, and make it a migration error in release (the clause PLAN_EDITS.md proposes; recommended, because it keeps the common edit cheap and never restores a value into a type it no longer fits); (b) reset all state on any schema change in dev (simplest, but every edit to a store field loses the session); (c) keep D35 literally and refuse the reload on mismatch (strict, but the developer is stuck until the schema is restored). The developer sees (a) as "your `count` HOLD changed type; it restarted at 0", (b) as a full restart, (c) as a blocked reload.

**B11. Stores and commits on FPGA and console** [P0 reconcile; hardware plan;
MIS-006, MIS-002]. D35 calls FPGA in-memory, but console app.wasm has a flash
journal. Options: (a) only RTL registers reset at power-on, and a console
profile with a journal has a store; (b) all in-memory; (c) the board profile
picks. **Recommend (a) through (c)'s principle,** restore keyed on identity_v1
schema compatibility. Whether an RTL same-tick chain is one edge or one cycle
per stage: **defer to the hardware plan in writing,** one edge as default.

```boon
c: 0 |> HOLD c { a |> THEN { a * 10 } }   -- RTL: the same edge as `a`, or one cycle later
```

*Reviewer note after critique.* The "one edge" default follows A2's rank scheduling. If A2 is answered with per-microstep semantics instead, the MIS-002 finder's "one cycle per stage" becomes the natural hardware mapping.

**B12. BYTES capacity in HOLD** [P2a; MIS-016, narrowed]. Is a HOLD that starts
at `BYTES[N]` fixed at N, or does it widen to dynamic BYTES (spec.md:44, today)?
**Recommend fixed:** strict, and it matches the RISC-V plan's register widths.

```boon
payload: BYTES[4] { 16u01, 16u02, 16u03, 16u04 } |> HOLD payload { load |> THEN { load.bytes } }   -- fixed: 5 bytes is an error
```

**B13. Recursive data types** [P0 spec; SEM-041]. Boon code cannot build them,
but host values (JSON, file trees) could. Options: out for v1 (a positioned
error and a flat-row idiom) or iso-recursive types (cycle handling in
hash-consing, display, fingerprints). **Recommend out for v1,** stated in §4.3.

```boon
node: [label: TEXT { root }, children: LIST { [label: TEXT { leaf }, children: LIST {}] }]   -- finite: fine either way
```

**B14. Forwarding one matched variant** [P0 spec; SEM-038]. D10 does not refine
the subject, so forwarding a variant means rebuilding its payload. Options: (a)
rebuild, with a fix-it; (b) a whole-variant binder typed as that variant; (c) a
rest binder `HierarchyPage[rows, ...rest]`. **Recommend (b)** with D10; it is
this file's only syntax addition, so (a) if syntax stays frozen.

```boon
rows: result |> WHEN { HierarchyPage[rows, offset] => summarize(page: HierarchyPage[rows: rows, offset: offset]), __ => SKIP }
```

**B15. Theme data under closed contracts** [P2b, R-RENDER; MIG-010, EVI-010].
(i) The draft's `None` sentinels: (a) the contract admits `None` as "unset" for
colour-like keys; (b) Optional `color?:` fields, losing the same-shape check;
(c) literal defaults in Base. (ii) Does D22 also cover Scene lights and
geometry (size L), or only glow, shadows and family (lights flagged)?
**Recommend (a), and style keys only** once S4 lists them (R-RENDER stays 1-2 weeks).

```boon
material: [color: None, glow: None]    -- (a): legal, means "not drawn"
```

**B16. Multi-line TEXT indentation** [P1a; MIS-012]. Recovery needs column-0
lines as resync points. Options: A, no TEXT content line at column 0 (all 5
blocks comply); B, content at the closing `}` column + 4 (breaks BUILD.bn:28-30
in both apps); C, no rule (heuristic recovery). **Recommend A.**

```boon
summary: TEXT {
    Included {count} icons
}
```

**B17. Diagnostics shape and position** [P2a, P5; MIS-013, ARC-019]. (i)
persons_pro's `rejected` payload: (A) today's first-error fields plus `code` and
`count`; (B) a `diagnostics: LIST {...}`; (C) both. (ii) A `PASSED.store.x` typo
in a view function is found at the PASS site (RUN.bn:4949): report it at the
read or at the PASS? **Recommend A (no decision before P5) and "at the read".**

```boon
color: PASSED.store.theme_optons.mode   -- the error points here, not at the PASS in RUN.bn:4949
```

---

## C. Process and plan-shape questions

**C1. Where next-only example sources live before P7** [MIG-020, EVI-007,
EVI-006, MIG-030]. The compatibility rule keeps every next-only migration off
main until P7, while the P3b, P4 and P5 exits need them. Options: (a) a
per-example engine flip (amends the verify-all rule, exposes unhardened gates);
(b) the big bang, P7 resized to 1-3 weeks; (c) a long-lived branch; (d) a next
overlay: a per-example next source for `engine=next` runs, promoted at P7.
**Recommend (d):** one default engine, old gates untouched, real inputs for O2
and P5. The engine selector must reach child-program and migration-stage
compiles (EVI-006); a gate red at P0 is recorded, not blocking (MIG-030).

```toml
[[example]]
id = "cells"
source = "examples/cells.bn"
next_source = "examples/next/cells.bn"   # proposed: read only by engine=next runs, swapped in at P7
```

**C2. Old-engine plans on the new scheduler** [ARC-002, MIG-022, ARC-015,
ARC-001]. ARC-002 recommends one scheduler, MIG-022 today's reads for old plans.
Options: (1) one scheduler; old WHEN becomes a live op; old plans carry a flag
keeping HOLD dedup and disabling piped resets; R7 shared only if S5 finds no
affected gate; (2) two regimes until P8; (3) R7-R9 for `engine=next` only.
**Recommend (1),** deciding v11+delta against a v12 core with a compat converter
at S1 exit: it honours D21's "unconditional" and fits the line cap best.

```boon
v: 1 |> HOLD v { press |> THEN { 1 } }
fired: 0 |> HOLD fired { v |> THEN { fired + 1 } }   -- new plans: counts every press (D32); flagged old plans: 0
```

**C3. An oracle for the change rules** [MIG-024, MIG-025]. O4 is exact on two
static pages, so nothing independent checks D30-D34. Options: (d) the owner
signs expected traces for every `change_probes` scenario, written in D30 terms
by a non-implementer, as O2 fixtures; (a) a reference interpreter O7 kept after
cutover; (b) O7 deleted at P8; (c) nothing. **Recommend (d) now, O7 as an
optional time-boxed spike** (2-4 k lines is optimistic). O1 fixtures come from
static semantics v2, never from the checker's implementer.

```boon
-- change_probes/diamond.bn (the A2 program), signed trace: press 1 -> a = 1, c = 10, x = 11, x_updates = 1
```

**C4. The S2 re-baseline band, and committed reuse** [ARC-020, ARC-011, ARC-022,
ARC-023]. Only the front end is measured; the check targets are 5x and 4.4x
checker.md's model; the ≤8 ms warm goal and ≤1 ms no-op gate contradict the 10.7 ms
full-recheck sum; no surveyed editor reaches sub-frame edits without reuse
(`research/incremental_compilation_subframe_latency.md` item 1). Options: (a) S2
exits on a compile-bench measurement with three bands (pass; an owner
re-baseline band, proposed up to 2x target; a representation fix), and reuse is
a committed P6 item, moved to P5 if TodoMVC's lane exceeds 12 ms p95; (b) keep
extrapolated exits and "reuse if needed". **Recommend (a),** with a provenance
column in §6.

**C5. NovyWave root-group floor** [ARC-009]. Reuse cannot reach the root group
(about 36% of NovyWave), so a `store` edit re-solves it (about 8-15 ms, pending
S2). Options: (a) accept, gating only FUNCTION-body edits; (b) a per-root memo
with a session TypeStore; (c) an SCC split (no help: one SCC). **Recommend (a),
with (b) in P6** if the root share exceeds 5 ms; add `store` edits to the corpus.

```boon
store: [page_size: 256]   -- an edit here re-solves the whole root group under (a)
```

**C6. Where the diagnostics lane runs** [ARC-014]. The snapshot travels two
hops with full decode and re-encode and grows under D13. Options: (a) the
preview process, with deltas and hover on request; (b) an editor-local check
session (no IPC; double check and memory). **Recommend (a),** switching to (b)
if P5 measures snapshots above 256 KB or 1 ms per hop.

**C7. Budgets v4 loosenings** [MIG-028]. §9.3 and the draft disagree on every
limit, v4 loosens limits that the plan's rule sends to the owner, and the
loadavg guard retries forever under load. Options: (a) a v3-to-v4 mapping table,
each row kept, tightened or owner-approved, the draft non-authoritative, the
measurement lock replacing loadavg; (b) regenerate the draft from the prose;
(c) keep v3 until P6. **Recommend (a),** at P1b with the plan archive.

**C8. Line caps for dual-path runtime code** [MIG-026]. The executor has 895
lines of headroom, and R2-R5, R8 and R9 stay dual until P8. Options: (a) a
temporary allowance ratcheted back at P8; (b) a new crate outside the capped
prefixes; (c) deletions now. **Recommend (a),** sized from S1 and S3: (b) meets
the cap only on paper, and (c) has no source of deletable lines.

**C9. Effort and P0 staffing** [MIG-023, MIG-021, MIG-017]. P0 holds six spikes
and the spec in 2 weeks with one engineer; R8 and R9 hide in P4; P7 is not
"days"; the migration is about 1,050 edits. The reviewer estimates 24-34 weeks (24-35 with MIG-017's 3-4 weeks for P3b, the figure PLAN_EDITS.md uses)
critical path and 50-70 engineer-weeks, against 20-27 and 32-45. Options: (a)
spikes as parallel agents in the engineer-week sum, a scheduler row (R-CHG), P7
at 1-3 weeks, all estimates re-baselined by S1's measured-to-estimated ratio;
(b) a longer P0. **Recommend (a):** short in calendar time, falsifiable at S1.

---

## D. Questions dropped or merged

Dropped because a source already answers them, or a refuter made them plan text:

- **SEM-037 option 2, the residual binder after tag arms:** already in
  ERROR_HANDLING.md:333-344 and spec.md:373; adopt it (a binder is a new name,
  per D10). Kind patterns go to the owner only if S4 finds code needing them.
- **SEM-048, the zero-arm LATEST:** as a HOLD body it already means "keep the
  piped value" (65 sites, 14 DRAINING). A bodyless form would be a syntax change.
- **MIS-016 parts (1) and (2):** checker.md:75 (a BitsWidthEq residual
  predicate), spec.md:44 and checker.md:319 answer them; B12 remains.
- **SEM-006, clearing a restarting query's value:** D34 decides it; B1 keeps the
  echo convention.
- **MIS-002, software half:** microsteps and next-tick agree in software once
  rendering waits for the settled tick; A2 and B11 cover the rest.
- **SEM-017, cancelling commands:** change_and_effects.md:51 and D36 already say
  never (R9 plan text); B1 keeps the copy-context supersede.
- **EVI-004 option (a), FLUSH as SKIP:** conflicts with D29; B3 keeps the rest.
- **EVI-005, transitivity through calls:** follows from L6 and D26; A10 keeps
  the nested case.
- **MIS-003, PRIORITY and EXCLUSIVE:** unused; P0 retires them in the spec.

Merged: MIG-005 into A1 (the plan's §4.3 and WHEN_VS_WHILE.md:34-37 imply the
answer; only the command rule is new); SEM-003, SEM-005, SEM-002 part 1 and
MIS-003's tie question into A2; MIG-011 and SEM-035's extra question into A3
(option D); SEM-024's questions into A6 and A8; SEM-016's extra question into
A7; MIS-006 and MIS-002 into B11; MIG-010 and EVI-010 into B15; MIS-013 and
ARC-019 into B17; MIG-020, EVI-007 and EVI-006 into C1; ARC-015 and MIG-022
into C2.
