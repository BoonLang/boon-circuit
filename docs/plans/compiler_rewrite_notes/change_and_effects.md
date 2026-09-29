> **Design-panel input, 2026-09-29. Not authority.** Output of the
> change-and-effects analysis that fed open questions L2 (effects) and L3
> (events vs "everything is change") in `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`.
> The plan's decision table wins. Probe numbers (p1-p9) refer to throwaway
> scenario probes run against the old compiler/runtime on 2026-09-29; their sources are in
> `change_probes/` (plan dumps not kept). See also `original_change_model.md`.

# L2/L3 design options: events vs change, and host effects

## 0. What any model has to settle

**The distinction does not go away; only its visibility changes.** "Has a value right now" and "exists only at the moment something happens" are different things. TodoMVC depends on the difference. `title_to_add` (examples/todomvc.bn:33-41) is a WHEN over `key_down.key` that reads `new_todo_text`.
- If the payload of a moment stays readable afterwards (sticky, as today: probe p8) and WHEN is live, then after one Enter every later keystroke changes `new_todo_text`. That re-fires `title_to_add`, and each keystroke appends a todo.
- Every design has to rule this out. It can do so with a user-visible type rule (Model A), by changing how WHEN works (Model B), or with an inferred analysis written in the language of change (Model C).

Every model has to give normative answers to five questions:
1. What counts as a change?
2. Does a moment's value survive past its moment?
3. When do reactions to a HOLD change run, relative to D21 snapshots?
4. Which effects may re-run when their inputs change, and which must be tied to a single occurrence?
5. How does the user see effects and in-flight calls?

Today each input kind answers these differently:
- A SOURCE fires on every occurrence (p3).
- A HOLD fires only when its value differs (machine.rs:21769-21783, p1).
- A derived value fires on any upstream write (p5c: `a > 5` fires while it stays False).
- An effect-result HOLD fires on every completion (machine.rs:17450-17458).
- Event payloads stay readable after their tick (p8).
- Effects are live and latest-wins (machine.rs:19972-20020, 20177-20186).

---

## 1. Model A: "Events and values" (§4.3 as written)

### Rules
- **SOURCE** is an event: present only in the tick it occurs, and every occurrence counts, including equal payloads.
- **THEN** needs an event input. A value input is an error. The body is evaluated against the tick snapshot (D21). The result is an event.
- **WHEN** over a value selects continuously and gives a value. WHEN over an event decodes each occurrence and gives an event.
- **WHILE** needs a value selector.
- **LATEST**: every arm must be an event, and the result is an event (D15). Ties on the same sequence are an error (LANGUAGE_SEMANTICS.md:385-407).
- **HOLD** needs a value as its initial value, and every candidate must be an event. A bare value candidate, or a WHEN over a value inside the HOLD, is an error.
- **SKIP** means absent in this tick (LANGUAGE_SEMANTICS.md:332-336).
- "Change" is not a user concept. Code cannot react to a state change; it reacts to the event that caused it.
- **Tick**: one evaluate/commit phase against snapshot S0, then commit, then publish. There are no follow-up steps, because nothing can be triggered by state.

### Effects
- An effect call is legal only in an event context: a THEN body, or a WHEN over an event, including inside HOLD, LATEST and `List/append` candidates.
- Each occurrence makes exactly one call. Arguments are sampled at the trigger.
- The call expression is a completion event: it arrives later, once per outcome (streams arrive several times).
- There are no resources and no Pending value; users model in-flight state with a HOLD, as persons_pro already does.
- Cancellation: a newer call from the same site and owner supersedes an older ReadOnly call. Commands are never cancelled.
- There is no arm-scoped ownership. Stopping the NovyWave stream on `show_empty` therefore needs a new mechanism: an `Effect/cancel` builtin, or letting the stream finish and ignoring the result. Either one reintroduces something that looks like a resource.

### What the user sees
- "`THEN` needs an event, but `passkey.registration_succeeded` is a value (a HOLD). React to the event that changes it."
- "`File/read_stream` may only run when an event arrives (inside THEN or a WHEN over an event). Here it is inside a WHILE over `real_waveform_stream_mode`."
- "LATEST merges events. `default_file_tree_selected_file` is a value."
- "`title_to_add` is an event. The document needs a value; keep it with HOLD."
- Hover: `store.count : NUMBER, value` and `title_to_add : TEXT, event from new_todo_input.key_down`.

### Checks
- **Can check:** every flow rule, effect placement, reading an event where a value is needed, and WHILE over an event. Duplicate calls are impossible by construction (one per occurrence).
- **Cannot check:** whether the user enumerated every event that changes a value they care about. Adding a new writer silently skips the reaction.

### Runtime
- This is the simplest runtime.
- Delete the State-trigger routing for user updates (updates_by_state, machine.rs:5028-5058, 17769-17865) and the effect re-run on dirty inputs (machine.rs:19972-20020). There are no microsteps.
- Completions arrive as external events in later ticks.

### Migration
- About 25 THEN-over-state sites must become completion events: persons_pro 14, host_service_effects 6, server_effect_chain 1, NovyWave 4. §5.1 counts 18.
- 7 effect sites inside WHEN/WHILE over state must be rechained on events: Wellen ×5, File/read_stream ×2.
- The WHEN-over-state candidates inside NovyWave HOLDs (about half of the 37 WHEN candidates) must change.
- LATEST arms that are derived values (NovyWave `real_waveform_file_request`) must change.
- 41 constant-arm LATEST blocks move to HOLD (D15, the same in every model).

### Pros
- Matches the documents and §4.3 as written.
- Simplest runtime and cheapest checker.
- Commands are safe by construction.
- Precedent: Elm 0.17+ (update returns Cmd).

### Cons
- It is the opposite of the owner's "everything is just change": users must think in two categories and meet both words in diagnostics.
- It loses the live, argument-replacing effects that the NovyWave plan requires (NOVYWAVE_BOON_REWRITE_PLAN.md:26-51, BYTES_SEMANTICS.md:182-205).
- Reactions have to be written against causes, not results. For example, "leave edit mode when a todo becomes completed" has to list `todo_checkbox.click`, `toggle_all_checkbox.click` and every future writer.
- It has the largest NovyWave migration.

---

## 2. Model B: "Pure change" (original Boon)

### Rules
- Every expression is a stream with a latest value. Once a value has arrived it stays readable (sticky).
- **A change is every arrival or write**, with no equality check (Elm `foldp`, Excel dirty marking). A SOURCE changes per occurrence, a HOLD changes on every accepted candidate even when equal, and a derived value changes on every upstream arrival.
- **THEN** fires on every arrival of its input. The body is a snapshot (original SNAPSHOT_VS_STREAM.md:13-40).
- **WHEN** freezes on arrival: its arms run once per arrival of the input, and body reads are samples. **WHILE** is live. This is the original rule (WHEN_VS_WHILE.md:11-25). Without it, TodoMVC appends a todo on every keystroke, as described in section 0.
- **LATEST** gives the value that arrived last. A constant arm is its initial value, so LATEST is state (original LATEST.md:14-35).
- **SKIP** means "don't emit; keep the current value" (original SKIP.md:10-12).
- **HOLD**: any arrival of a candidate writes the HOLD and fires.

### Effects
- Effects are live subscriptions anywhere, including top-level pipeline sinks (novywave/BUILD.bn:11-50).
- A call re-runs on every arrival of any argument and cancels the previous call.
- Inside THEN or WHEN, a call runs once per trigger.
- No command/query split is enforced.

### What the user sees
- Almost no temporal diagnostics; only cycles and types.
- Hover: "latest value; updates on every arrival from X".
- Mistakes show up at runtime: repeated effects, stale payloads read by an unrelated trigger (p8: `other |> THEN { ev }` reads an old `ev`), and the "not current" errors.

### Checks
- **Can check:** cycles.
- **Cannot check:** duplicate commands, reads of stale payloads, or reactions that never fire.

### Runtime
- Every node keeps its last value.
- No equality cutoff, so there is more re-evaluation and more effect churn.
- Change loops still need static detection or a runtime depth guard.
- Today's runtime is closest to B.

### Conflicts with settled decisions
- **D15** (state only via HOLD): sticky moments and LATEST-with-default are hidden state.
- **D12**: sticky payloads are memory that is never persisted, so a restarted app diverges from the running one. `title_to_add` or a press payload is readable before a restart and gone after it.
- **D6** (strictness): there is little left to enforce.
- **Semantics change**: boon-circuit's continuous WHEN becomes a frozen-on-arrival WHEN. Every value-WHEN whose arms read other values has to become WHILE. That covers most of `account_state` (persons_pro/RUN.bn:288-300), `visible_todos` and the NovyWave arms; a census is needed.

### Migration
- Examples mostly run as they are, except the WHEN→WHILE conversion.
- Undoes the D15 fix-its.

### Pros
- Faithful to the original idea.
- Smallest vocabulary.
- An effect-completion HOLD re-fires on every repeat without any extra field.

### Cons
- Violates D15, D12 and D6.
- Effects re-run whenever an argument is rewritten, even with the same value.
- Equal presses of `selected_filter` fire downstream reactions.
- Writes that don't change the value still fire, which does not match hardware edge semantics (FPGA_TODOMVC_LOWERING, LANGUAGE_SEMANTICS.md:351-358).

---

## 3. Model C: "Change with moments" (hybrid, recommended)

The user writes and reads "change". The compiler infers when things change, like Lustre clocks, and reports that in change vocabulary. Effects split into queries and commands by catalog class. No syntax changes.

### 3.1 Kinds (inferred, never written)
- **constant**: never changes.
- **value**: exists from the start and may change. This covers HOLD, stateful builtins, collections and anything derived only from these.
- **moment**: exists only when it happens. This covers a SOURCE occurrence, an effect outcome, a Timer/interval tick, a THEN output, a WHEN over a moment, a pure operation with a moment input, and any expression with a SKIP arm, since it may produce nothing.
- Each moment has a **clock**: the set of roots it can happen on. A root is a SOURCE declaration site, an effect call site, or "change of value v". Clock sets are interned and small.

### 3.2 What counts as a change
- **Arrivals from outside** (SOURCE, effect outcomes, timer ticks): every arrival is a change. Two identical clicks are two changes, and two identical `RegistrationCancelled` results are two changes.
- **Moment-derived expressions**: change on every occurrence that does not SKIP, with no equality check. Adding "milk" twice appends twice.
- **HOLD**: changes when the committed value is structurally unequal to the previous committed value.
- **Derived values**: change when the recomputed value differs (equality cutoff). Collections change when their revision changes.
- **Activation is not a change.** App start, restore, hot reload, entering an arm and creating a row do not fire THEN.

### 3.3 Constructs
- **THEN**: the input must be able to change; a constant input is an error. THEN fires once per change of the input. The body is a when-context: only the input triggers, and every other read samples the committed snapshot. Reading the input gives its new value, which is automatically committed (see Tick below). The result is a moment on the input's clock.
- **WHEN** over a value selects live, like today. It gives a value, or a moment if any arm SKIPs. WHEN over a moment decodes each occurrence (a when-context) and gives a moment.
- **WHILE**: the selector must be a value. Arms are live.
- **LATEST**: every arm must be able to change. A constant arm is an error with the D15 fix-it `initial |> HOLD s { LATEST {...} }`. A value arm contributes its changes. The result is a moment. The latest microstep wins, then the greatest sequence; a tie on the same sequence is an error. This rephrases D15 as "LATEST merges events/changes only".
- **HOLD**: the initial value must exist from the start. A candidate can be anything that can change, and each of its changes is a write. SKIP means no write. The binder is the committed value. `0 |> HOLD h { x |> WHEN { Valid[v] => v  __ => SKIP } }` is the "remember last valid" idiom.
- **Update positions** act on each change: `List/append item:`, `List/replace_all with:` (D17), `List/remove when:`, and `Bool/toggle when:` (D26).
- **SKIP** means "nothing here". In a moment position it means no occurrence; in an update position it means no write. If an expression that may SKIP is used where a value is required (the document, a WHILE selector, a HOLD initial value, a LIST item), that is a compile error, not today's runtime "privately absent".
- **Moment reads** in a when-context:
  - allowed only if the moment is on the same clock as the trigger; if it may be absent, the absence propagates;
  - a disjoint clock is an error, because it would never be present.
- **Cycles** (a refinement of D9): a cycle must pass through a committed-value read of a HOLD, a collection or a stateful builtin. A change-trigger edge (a THEN input or an update candidate) never closes a cycle.
  - probe5's ping-pong, which today fails at dispatch with "state transition cycle re-entered" (machine.rs:17780), becomes a positioned error;
  - so does a HOLD triggered by its own change, which today is silently ignored (probe4).
- **Tick** (D21 applied per microstep):
  1. Microstep 0 evaluates the candidates triggered by external arrivals against S0 and commits S1.
  2. Microstep i evaluates the reactions to changes committed in step i-1 against S_i.
  3. Depth is at most the longest path in the change-edge graph, which the cycle rule makes acyclic and statically bounded, so no runtime guard is needed.
  4. After quiescence: reconcile queries once, dispatch commands, then publish and render.
  - Swapping x and y on one press gives (2,1), not today's (2,2) (p6b).

### 3.4 Effects: two catalog classes

The class is decided by whether a call is safe to re-run, not by read vs write. It comes from ReplaySpec (boon_effect_schema/src/lib.rs:197-306) and never depends on argument values.

- **Query**: re-running with the same arguments is harmless, and it has no outside effect. File/read_text, read_bytes and read_stream, Directory/entries, Wellen/*, Secret/verify, HMAC sign and verify, Timer/deadline, Content/import.
- **Command**: it has an outside effect, or it gives a fresh answer on every run. DevelopmentPasskey/*, File/write_*, Content/save, Http/request, Clock/wall, Random/bytes. Borderline cases are in Q4.

| position | query | command |
| --- | --- | --- |
| Value context: a field, a WHILE arm or WHEN-over-value arm, a derived candidate | **Live resource.** Starts on activation, keyed by argument equality. Restarts when the key changes, at most once per tick, after quiescence. Cancels the superseded run and stops on deactivation (arm left, row removed). Its value is `Pending`, then the latest outcome. | **Error** |
| When-context: a THEN body or a WHEN over a moment | Runs once per trigger with sampled arguments. The result is a moment at completion. | Runs **exactly once** per trigger with sampled arguments, and is never re-run when an argument changes. The result is a moment at completion. |

Consequences:
- **Completions are moments.** A HOLD around a completion keeps the last result and itself changes only when the value differs.
- **Restore and hot reload:** queries restart. Commands are never re-issued. Durable commands go through the outbox (BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1413-1423); a transient command that is in flight during a crash runs at most once.
- **Duplicate commands disappear.** Today the effect re-stages whenever its inputs go dirty (machine.rs:19972-20020). Sampling the arguments removes that, and removes the possible second DevelopmentPasskey outbox item raised as an open issue.

### 3.5 How effects become visible without syntax
- **Hover** shows the catalog class and the timing:
  - "`DevelopmentPasskey/register`, command: runs once each time `store.elements.register_passkey` fires; its result arrives later".
  - "`Wellen/open`, query: runs while this arm is active; restarts when `content` changes; `Pending` until it answers".
- **FUNCTION schemes** carry an inferred effect row (a query/command bitset beside the OUT and collection-write effects, BOON_COMPILER_REWRITE_PLAN.md:353-357). Hover on a user function shows "runs query Wellen/signal_page".
- **Types**: `Pending` appears in the type of a query used as a value, and D5 exhaustiveness makes users handle it.
- **Dev window and scenario runner**: an effect log of issued, cancelled and completed calls per call site, which scenarios can assert on.

### 3.6 What the user sees
- "`TEXT {x} |> THEN {…}` never runs: `TEXT {x}` never changes. THEN runs when its input changes."
- "`File/write_text` is a command: it writes. In this WHILE arm it would run again whenever `path` or `text` changes. Run it at a moment: `save.press |> THEN { File/write_text(…) }`."
- "`title_to_add` exists only when Enter is pressed in new_todo_input, and the document needs something that exists all the time. Keep it with HOLD."
- "`title_to_add` happens on `new_todo_input.key_down`, but this THEN runs on `add_button.press`, so it would never see a value."
- "Change loop: `ping` changes → `pong` updates → `pong` changes → `ping` updates. A HOLD may read another HOLD's value in a loop, but not react to its change in a loop."
- "`press` only happens; it has no current value for WHILE to select on. Use WHEN to handle each press."
- "`0` never changes, so it never wins in LATEST. For a starting value write `0 |> HOLD count { LATEST {…} }`."
- **Warning**: "`passkey.registration_cancelled` keeps the last result of a command. THEN over it runs only when the stored value becomes different, so a second identical `RegistrationCancelled` is missed. React to the completion itself."
- Hover on names:
  - `store.count`: "value; starts as 0; changes when increment.press, decrement.press or reset.press".
  - `title_to_add`: "happens when new_todo_input.key_down (Enter, text not empty)".
  - `real_hierarchy_page_result`: "value (query); Pending until Wellen/hierarchy_page answers; restarts when real_waveform_open_result changes".

### 3.7 Checks
- **Can check:** everything in 3.6, command placement, missing Pending arms, and effect rows through FUNCTION calls.
- **Cannot check:**
  - whether a value actually changes at runtime (`count * 0` never fires once the equality cutoff exists; that is correct but invisible);
  - whether a catalog class is true;
  - whether the user meant "each time" or "on change" for equal results; only the command-result warning covers this;
  - the cost of equality on large BYTES;
  - exactly-once delivery across crashes for commands outside the outbox.

### 3.8 Runtime work
1. Equality cutoff on derived values (today they fire on any upstream write, p5c). Collections use revisions, and BYTES use a hash or pointer fast path. D9 already keeps collections out of HOLDs.
2. A pending/committed split with microsteps (D21).
3. Moments cleared after their microstep; sticky payloads removed (machine.rs:19339-19375).
4. Completions delivered as moments. The forced `route_state_transition` on effect-result HOLD writes goes away (machine.rs:17450-17458).
5. Commands staged once with sampled intent. No re-run on dirty inputs for commands; reconcile_dirty_effects stays for queries only, once per tick.
6. Pending injected for value-context queries.
7. Fix the lowering of bare continuous candidates (p5b, p5).
8. LATEST ties become an error (p9).

---

## 4. Migration, side by side

### Counter
- examples/counter.bn:27-34 needs no change in A, B or C.
- examples/counter_latest.bn:5-11 is legal in B. In A and C, D15 requires:
```boon
count: 0 |> HOLD count { LATEST { increment |> THEN { count + 1 }  reset |> THEN { 0 } } }
```

### TodoMVC add and toggle (todomvc.bn:25-41, 59-61, 165-175)
- **A and C:** no text change.
  - `title_to_add` becomes a moment. Its body samples `new_todo_text` before microstep 0 clears it.
  - Toggle-all rows all read `store.all_completed` from S0, so the result no longer depends on declaration order. Today HOLDs commit immediately and derived values cascade within the same turn (p6). Whether toggle-all is actually order-dependent today was not verified.
- **B:** needs the value-WHENs whose arms read other values rewritten as WHILE (`visible_todos` :68-76 and similar).
- **Where C adds something** (hypothetical feature, "leave edit mode when completed changes"):
```boon
-- C: react to the value, whoever wrote it
editing: False |> HOLD editing { LATEST { …  completed |> THEN { False } } }
-- A: must list every writer, and silently misses future ones
editing: False |> HOLD editing { LATEST { …
    sources.todo_checkbox.events.click |> THEN { False }
    store.sources.toggle_all_checkbox.events.click |> THEN { False } } }
```

### persons_pro passkey (RUN.bn:48-72, 139-148, 301-317)

Before:
```boon
registration_cancelled:
    RegistrationNotCancelled |> HOLD registration_cancelled {
        store.elements.simulate_registration_cancel |> THEN {
            DevelopmentPasskey/register(…, simulation: Cancel) } }
passkey_workflow_state: LATEST { Idle  …  passkey.registration_cancelled |> THEN { Cancelled } … }
```

- **A (required), and the recommended fix in C:**
```boon
registration_cancel_done:
    store.elements.simulate_registration_cancel |> THEN {
        DevelopmentPasskey/register(…, simulation: Cancel) }
registration_cancelled:
    RegistrationNotCancelled |> HOLD registration_cancelled { registration_cancel_done }
passkey_workflow_state:
    Idle |> HOLD passkey_workflow_state { LATEST {
        elements.register_passkey.event.press |> THEN { Registering }
        …
        passkey.registration_cancel_done |> THEN { Cancelled } } }
```
- **C without the fix** compiles, except for D15 (`Idle` arm), and gets the warning from 3.6. That warning points at a real bug: a second "Cancel registration" leaves the workflow in `Registering`, because `RegistrationCancelled` has no payload (effect_schema lib.rs:1022) and the HOLD value does not change.
- **B** keeps the code as it is; repeats fire because every write counts.
- The persons_pro gate asserts these steps, so this is a manual rewrite: 7 completion fields and 14 reactions. It is the same work in A and in C.

### NovyWave open → hierarchy (RUN.bn:41-93)
- **Today and B:** HOLD, then WHILE with `File/read_stream`; HOLD, then WHEN with `Wellen/open`; LATEST `{ NotStarted, … Wellen/hierarchy_page(request_fingerprint: …) }`.
- **A:** chain events and add a cancellation mechanism for `show_empty`. Nested binders follow D10.
```boon
real_file_stream_event: real_waveform_file_request |> THEN {
    File/read_stream(file: real_waveform_asset, chunk_bytes: 65536, retain_content: True) }
real_file_stream_result: NotStarted |> HOLD real_file_stream_result { real_file_stream_event }
real_waveform_open_event: real_file_stream_event |> WHEN {
    Finished[retained] => retained |> WHEN { Retained[content] => Wellen/open(content: content)  __ => SKIP }
    __ => SKIP }
real_hierarchy_page_event: real_waveform_open_event |> WHEN {
    WaveformOpened[artifact] => Wellen/hierarchy_page(artifact: artifact, request_fingerprint: TEXT { hierarchy:default:0 }, offset: 0, limit: 256)
    __ => SKIP }
real_hierarchy_page_result: NotStarted |> HOLD real_hierarchy_page_result { real_hierarchy_page_event }
-- and RUN.bn:108: real_hierarchy_page_event |> THEN { 0 }
```
- **C:** the current code is legal, except the `NotStarted` LATEST arm (D15), which moves to HOLD. It can optionally be simplified to live queries:
```boon
real_file_stream_result: real_waveform_stream_mode |> WHILE {
    Active => File/read_stream(file: real_waveform_asset, chunk_bytes: 65536, retain_content: True)
    Inactive => NotStarted }
real_waveform_open_result: real_file_stream_result |> WHILE {
    Finished[retained] => retained |> WHILE { Retained[content] => Wellen/open(content: content)  __ => NotStarted }
    __ => NotStarted }
real_hierarchy_page_result: real_waveform_open_result |> WHILE {
    WaveformOpened[artifact] => Wellen/hierarchy_page(artifact: artifact, offset: 0, limit: 256)
    __ => NotStarted }
-- RUN.bn:108 `real_hierarchy_page_result |> THEN { 0 }` stays (it also fires on the Pending change, which is harmless)
```
  - The types become `NotStarted | Pending | <outcomes>`.
  - `request_fingerprint` becomes unnecessary if keys are automatic (Q5).
  - Behaviour change: results are dropped when an arm deactivates. Wrap them in a HOLD where they must survive.

### BUILD.bn (novywave/BUILD.bn:11-50)
- **C:** Directory/entries and File/read_bytes stay as value-context queries. `File/write_text` is a command, so it moves under a build-start moment. Log depends on Q4.
- **A:** all of these must be triggered.
- **B:** unchanged.
- Both BUILD files already fail kernel coverage today.

---

## 5. Compile speed

No builds were allowed, so these are asymptotic estimates, to be confirmed with counters in the P-phase.

- **A:** one forward flow pass over the instance graph (Phase A, where D9 cycles are already detected). It uses a three-valued lattice and costs O(V+E). Effect placement is a local context flag. This is the cheapest strict option.
- **C:** the same pass, plus:
  - an interned clock set per moment, usually 1-3 root ids;
  - edge labels in the existing D9 SCC pass (change vs committed read);
  - one effect-class bitset per FUNCTION scheme.
  
  It stays linear with a small constant factor over A. The "changes when" hints are computed on demand by the editor as a backward slice, not in `check`. Flows are recomputed downstream of changed root fields only, so they fit the warm path (`verify-compiler-interactions`).
- **B:** the checker is cheapest, since there is no flow pass. The cost moves to the runtime: last values stored per node, no equality cutoff, more re-evaluation and live effect restarts.

---

## 6. Recommendation: Model C

1. It is what the owner described. Users write and read only change: "changes when…", "happens when…", "never changes". There is no event type and no event keyword.
2. It keeps D6 strictness honestly. The event/value distinction is real (section 0), so C infers it the way Lustre and Esterel do and turns every temporal mistake that reaches the runtime today into a positioned compile error: "privately absent", "not current", dispatch cycle errors, silently ignored self-triggers.
3. It respects the settled decisions: state only via HOLD (moments are not sticky, so there is no hidden state), D9 (tightened), D12, D15 (rephrased as "LATEST merges changes"), D21 (per microstep), and syntax and types independent of values.
4. It unifies today's four definitions of a change into one rule: arrivals always count; computed values count when they differ.
5. It keeps the live, latest-wins resource behaviour that the NovyWave plan and the HTTP and Wellen tests assert, and makes commands exactly-once. That removes the latent re-run and duplicate-outbox risk.
6. Its migration is the smallest that is still consistent. Beyond the D15 fix-its, only the persons_pro completion split is needed, and it fixes a real bug. NovyWave keeps its structure.
7. Compile cost is about the same as Model A.

Changes to the rewrite plan if adopted:
- **§4.3 flow rules** become: "THEN needs an input that can change"; "HOLD needs a starting value that exists from the start; its candidates must be able to change"; "WHILE needs a value"; "every LATEST arm must be able to change".
- **§5.1**: the L3 row becomes "warning plus the persons_pro split", and the L2 row becomes "commands in value context".
- **§11**: L2 and L3 resolved.

---

## 7. Owner questions

**Q1. Which model?**
- (a) A: events and values.
- (b) B: pure change.
- (c) **C: change with moments. Recommended.**

**Q2. When do a HOLD and a derived value "change"?**
- (a) **When the new value differs. Recommended.**
  - Pressing All twice does not re-fire `selected_filter |> THEN`, and edges behave like hardware edges.
  - A repeated equal command result must be reacted to via its completion, not its HOLD (the persons_pro split).
- (b) On every accepted write.
  - The HOLD re-fires on equal results without a split.
  - Equal writes also fire (for example, pressing All again). HOLDs and derived values then use different rules, unless derived values also fire on every upstream write, which is today's p5c behaviour.

**Q3. Does a moment's value survive its moment?**
- (a) **No. Recommended.** `title_to_add` or a press payload is readable only at that moment. Reading it anywhere else is a compile error; keep it with HOLD.
- (b) Yes, sticky (today's runtime and original Boon). This is hidden state that is never persisted (conflicts with D12 and D15). It also needs WHEN frozen on arrival, which means rewriting value-WHENs as WHILE.

**Q4. Effect classes. Approve the query/command split (3.4) and these borderline assignments:**
- **Http/request** (its schema allows POST and DELETE but is classed ReadOnly, lib.rs:1103-1104):
  - (a) **always a command. Recommended.**
  - (b) split into `Http/get` (query) and `Http/send` (command). This is a catalog change, not a syntax change.
- **Clock/wall and Random/bytes:**
  - (a) **command** (tied to a moment, result kept in a HOLD and survives restore). **Recommended.**
  - (b) query (in value context, read once on activation and again after restore).
- **Log/\*:**
  - (a) **allowed anywhere; logs every change of its argument. Recommended.**
  - (b) command, so it must be under THEN, and BUILD.bn changes.

**Q5. What does a query used as a value show while it runs?**
- (a) **`Pending` is part of its type, and the value returns to `Pending` on every key change. Recommended.**
- (b) `Pending` at first, then the last result stays visible while a new key loads (stale while revalidate).
- (c) No Pending: the user wraps the query in a HOLD with an initial value, as today.

Linked to this: should keys be automatic by argument equality, which makes Wellen's `request_fingerprint` unnecessary? Recommended: yes.

**Defaults applied unless the owner objects:**
- Change reactions run in follow-up microsteps of the same tick, and rendering happens after quiescence. The alternative is the next tick, which would render intermediate states.
- Activation is not a change for THEN. Queries start on activation. Commands never run on start, restore or hot reload.
- Commands sample their arguments at the trigger and never re-run.
- Superseded query runs are cancelled (today's behaviour).
- Change loops and self-triggers are compile errors (the D9 refinement).
- THEN over a constant is an error. THEN over a HOLD fed by command results is a warning.

---

## 8. Fix whatever model is chosen
- **Lowering bug:** a bare continuous HOLD candidate or LATEST arm evaluates to the triggering state's value instead of its own (p5b: 1 instead of 1001; p5: 1 instead of 10; p5b.plan.json op 3).
- **LATEST ties** resolve silently to the last arm (p9); the docs require an error.
- **The production compiler emits no flow diagnostics** (`1 |> THEN {2}` passes; the test expects an error at crates/boon_typecheck/src/tests/pulses.rs:235-254).
- **Ping-pong** fails only at dispatch (machine.rs:17768-17782), and a self-trigger is silently ignored (probe4).
- **Persistence plan:** its LATEST-as-state example contradicts D15 (BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:229-255). LANGUAGE_SEMANTICS has no section on effect timing; it should get one in the P0 "static semantics v2".
- **Not verified here** (`boon_cli run` has no host runner):
  - that today a THEN-triggered transient effect re-runs when a HOLD read by its intent changes;
  - whether a WHEN/WHILE-arm effect fires at startup when the initial selector already opens it.

Earlier probe files, including p1-p9 and p5.py, are in `change_probes/` (next to this note). No new probes were written in this pass and the repo was not modified.

---

# Appendix A: how the current tree behaves

Boon today is a hybrid. The docs describe a strict split between events and values, while the implementation is already close to the owner's "everything is change".

**Docs.** LANGUAGE_SEMANTICS, the TYPE_INFERENCE flow table and RUNTIME_MODEL separate events from continuous values. SKIP means "absent in this tick", every tick reads a snapshot, and none of these docs has a section on host effects or FLUSH. The only effect semantics written down are in the NovyWave plan, BYTES_SEMANTICS and the persistence outbox section.

**Checker.** The old checker deliberately allows THEN over a HOLD, over a LATEST with a constant arm, and over a state-writing call. The production compiler that `boon_cli check` and the playground use emits no flow diagnostics at all. It accepts `1 |> THEN {2}`, WHILE over an event and one-arm LATEST, with diagnostic_count 0.

**Runtime.** Updates have three trigger kinds: SOURCE, State and Pulse; effect completion is a fourth, internal cause.
- A SOURCE occurrence fires every time, even when the payload is equal to the last one.
- A HOLD write triggers downstream work only when the value actually changes.
- THEN or WHEN over a derived value is lowered to a trigger on each upstream HOLD. It fires on every upstream change, even when the derived value stays the same or becomes absent.
- A value derived from an event keeps its last value in later ticks.
- LATEST merges event arms and change arms by whichever arrived last.
- HOLD updates commit immediately in dependency order. State changes cascade within the same turn, so swapping two HOLDs gives (2,2) instead of (2,1). This confirms D21.

**Effects.** An effect runs only when a triggered update stages it. Inside a continuous WHEN or WHILE arm, the trigger is a change of the selector's state and the arm acts as the gate. After it is staged, the effect stays active. Any change to its gate or intent inputs re-runs it and cancels the call still in flight (latest call wins); the NovyWave plan documents this as intended. Closing the gate cancels the call. The result comes back in one of three ways:
- a HOLD write that always fires downstream, even when the value is equal;
- a transient result that resumes the original event's dependent updates;
- a durable outbox item, used only by the DevelopmentPasskey effects.

None of this shows up in source or diagnostics. There is no Pending result value and no effect marker. The only placement error is an internal message with no source position.

**Bugs found by the probes.** A HOLD body or LATEST arm that is a bare continuous expression lowers to the triggering state's value, not its own. Two LATEST arms firing on the same event resolve silently to the last arm, where the docs promise a hard error.

**Examples.** 39 host-effect call sites use 20 builtins. About 25 THEN-over-state sites exist, and every one reacts to an effect completion; the rewrite plan counts 18. 41 LATEST blocks have a constant arm, and 32 have bare non-THEN arms.

**Original Boon.** It was explicitly built on streams and value arrival: values "fly in" and every operator reacts to each new incoming value. WHEN freezes on arrival, THEN is WHEN without arms, WHILE flows continuously. LATEST takes default arms, SKIP means "stay at the current value", and effects are pipeline sinks. Queries inside THEN or WHEN run once; queries elsewhere are live subscriptions. boon-circuit narrowed each of these.

Probes and plan dumps are in `change_probes/` (next to this note) (p1-p9 plus p5.py, a scenario stepper). The effect re-run and cancel behaviour comes from reading the code and the HTTP loopback test: `boon_cli run` has no host-service runner, so it could not be observed directly.

## Open issues

- Not observed: the claim that THEN-triggered transient effects re-run when a HOLD read by their intent changes. It comes from reading the code (a transient completion keeps the activation live; state reads register dependencies). `boon_cli run` has no host-service runner, so invocation counts could not be seen. Confirming it needs an S6 host runner or a small executor test.
- Possible duplicate command: for a durable DevelopmentPasskey effect, an intent input (such as `store.credential_count`) changing while the outbox item is still pending would re-stage the effect and enqueue a second item with a new idempotency key, because the turn sequence differs. Not verified.
- Http/request is classed ReadOnly and transient even for POST, PUT and DELETE. It can therefore be re-run when inputs change and cancelled when superseded, and it never goes through the durable outbox.
- Lowering bug to fix or track: a bare continuous HOLD body, or a bare continuous LATEST arm, evaluates to the triggering state's value instead of the expression (p5b, p5).
- LATEST ties on the same event resolve silently to the last arm; RUNTIME_MODEL and LANGUAGE_SEMANTICS require a hard error. A LATEST with event arms plus a continuous arm compiles but fails at runtime with 'not current' until its first arrival.
- Unverified: whether an effect inside a WHEN or WHILE arm runs at startup when the selector's initial value already opens the gate. The trigger is a state transition, and the startup path was not traced.
- For the owner (L3): what counts as a 'change'? Three candidates are value inequality of the observed expression, any upstream write, or every arrival including equal SOURCE payloads and equal effect results. Today all three coexist, depending on whether the input is a HOLD, a derived value, a SOURCE or an effect result.
- For the owner (L3): SKIP currently means 'absent this tick' in the docs, 'stay at current value' in original Boon, and in practice persistence of the last event value in the runtime. Which one should hold?
- For the owner (L3/D21): once reads use snapshots, when does THEN over a HOLD fire? Options: a later microturn in the same tick, the next tick, or the same tick with committed-only reads, which would change NovyWave's effect chains.
- For the owner (L2): should effects stay live queries (the argument changes, so re-run and cancel the old call; the arm is left, so cancel), as the NovyWave plan specifies? Or should they be one-shot per trigger, as original Boon's snapshot rule for THEN/WHEN has it? Should ReadOnly queries (live allowed) be distinguished from commands (trigger only, outbox), and should in-flight state (Pending) be a visible value?
- For the owner (L2): original Boon allowed effects as pipeline sinks, and the BUILD.bn files still use that form. Today effects must be direct named calls inside a triggered HOLD or LATEST-state. Keep that restriction, or allow continuous or top-level effects with subscription semantics?
- The rewrite plan's breakage count of 18 THEN-over-state sites omits host_service_effects.bn (6) and server_effect_chain.bn (1); the total is about 25.


# Appendix B: prior art

I researched 17 systems and ran read-only probes against today's Boon compiler and runtime. The owner's idea that "everything is just change" has clear precedents. The closest are Elm before 0.17 (one Signal type, with foldp working like HOLD), Esterel valued signals (presence in this instant plus a persistent value), Lustre (events are Boolean flows, and clocks are inferred static types), Reflex (a Dynamic is a current value plus its update events) and hardware (an event is a value change, and posedge is a change of a value). Nothing in the prior art forces a user-visible event type. What every successful unified design does have, and Boon still lacks, are normative answers to four questions:
1. What counts as a change: every write, or a different value?
2. Does activation count as a change?
3. Does a HOLD's change fire in the same tick or the next one?
4. Which effects may be keyed by a value change, and which must be tied to a single occurrence?

The probes show Boon already runs a change model, but it is inconsistent and partly unchecked:
- The checker accepts THEN over a HOLD.
- A HOLD counts as changed only when its value differs (machine.rs:21770). A derived value counts as changed whenever anything upstream was written: `count * 0` fires THEN on every click.
- A two-HOLD change cycle passes `check` and then fails at dispatch with "state transition cycle re-entered" (machine.rs:17780).
- A HOLD that triggers on its own change is silently ignored.

Effects split the same way everywhere. Idempotent synchronization/resources (React effects, Bonsai `Edge.on_change`, Solid `createResource`, Elm subscriptions) are safe to key by a value change. Non-idempotent commands (React event handlers, Elm Cmd from update, Reflex `performEvent`, Electric `e/Token`) must be tied to one discrete occurrence.

Recommended model, with no syntax change:
- A SOURCE changes on every occurrence. Every other value changes when it is structurally unequal to its committed value.
- THEN means "on change of its input". The input is the only trigger; body reads are D21 snapshot reads. This already matches Solid 2.0's compute/apply split and MobX `reaction`.
- The continuous/event/absent flow modes become an internal, Lustre-style clock analysis. Hints and diagnostics show it as "changes when: …".
- Refine D9: a HOLD breaks a cycle only through its committed value, never through its change.
- Split catalog effects into resources (keyed, restart when the key changes, latest result wins, allowed in continuous positions) and commands (only in THEN bodies triggered by a SOURCE-derived occurrence; completions are occurrences and are never deduplicated).
- Make effects visible through the catalog class, inferred effect rows in function schemes, hints, an effect log in scenarios and targeted diagnostics.

The probe files are in `change_probes/` (next to this note) (probe2-5 .bn/.scn, then_over_hold_doc.bn). I made no changes to the repo.

## Open issues

- Owner Q1, what counts as a change. Options: every write or occurrence (Elm foldp, Reflex updated, Excel), or a structurally different value (VHDL event, Solid, TC39, Bonsai). Proposal: a SOURCE changes on every occurrence; every other value changes only when its value differs from the committed one. Today's runtime mixes the two: a HOLD dedupes (machine.rs:21770) while a derived value fires on any upstream write (probe3). Consequence of the proposal: a command whose completion repeats with an identical payload must deliver it as an occurrence, not through a HOLD.
- Owner Q2, is activation a change? This covers app start, WHEN/WHILE arm activation, list-row creation, restore from persistence and hot reload. MobX reaction and Reflex say no. Bonsai on_change, React and Solid effects say yes. Proposal: no for THEN; resource effects start on activation; commands never fire on activation or restore.
- Owner Q3, same tick or next tick. When a HOLD commits in tick N, does a THEN over it run in tick N? That is instantaneous, needs Esterel-style causality analysis, and needs a rule that the payload is the new value while plain reads are committed; it is today's behaviour. Or does it run in a follow-up delta tick N+1, as with VHDL delta cycles, the Verilog NBA region or Solid flush? The delta tick is most consistent with D21 but needs a bounded depth and a runaway diagnostic.
- Owner Q4, refine D9: change-triggered edges do not break cycles; only committed-value reads through HOLD do. This makes the probe5 ping-pong a compile error instead of a dispatch-time InvalidPlan, and makes a HOLD triggered by its own change an error instead of silently ignored.
- Owner Q5, approve the resource/command effect split and the assignment of each catalog builtin. Undecided cases: Http/request (GET as a resource, other methods as commands, or always a command?), Timer/interval, Timer/deadline, Clock/wall, Random/bytes, Secret/verify, Crypto/*, File/read_bytes, File/read_stream, File/write_*, Wellen/*, DevelopmentPasskey/register.
- Owner Q6, resource key-change policy: cancel the in-flight request, or let it finish and discard the result (latest wins, as in Bonsai Poll)? Should resources dedupe automatically by argument equality, which would make the hand-written Wellen request_fingerprint unnecessary?
- Owner Q7, visibility: is it acceptable that effects become visible only through the catalog class, inferred effect rows in hints and the inspector, diagnostics and a scenario effect log, with no syntax? Or should a naming convention for commands be allowed, which touches syntax (D2)?
- Owner Q8, LATEST under 'everything is change': a continuous arm contributes only its change occurrences, the result is an occurrence per D15, and a value needs a HOLD around it. Confirm that simultaneous changes are still resolved by greatest source sequence, with ties as errors (LANGUAGE_SEMANTICS.md:385-406).
- Implementation question: detecting changes by structural equality on large records and lists is expensive. Can collections change by authority revision, meaning any row write that changes a value, while scalars and records use equality? Or must one rule cover all values?
- Probe caveat: today's scenario runner cannot show whether a HOLD change is delivered in the same tick or a later microstep. The probes show only that it happens in the same turn and that the body reads the post-commit value. The Q3 decision needs a runtime scenario differential once D21 lands.
