Research note, 2026-09-29, for review/REVIEW.md; not authority.

Topic: change semantics in FRP/UI systems and spreadsheets (Elm before 0.17,
Reflex, Solid 1.x/2.0, Svelte 5, Vue 3, MobX, React as the copy/live analogue,
Excel). Plan targets: D30, D32, D33, D15 (LATEST ties), D16, D21, D31 and
§4.3 "Change rules". Sibling notes: `copy_vs_live_synchronous_languages.md`
(Esterel/Lustre/VHDL) and `spreadsheet_recalculation_cycles.md` (engine
algorithms for Cells); this note does not repeat them.

Method: primary docs and source files fetched with WebFetch or curl (copies in
`scratchpad/research/frp_ui/`). Quotes are verbatim unless marked. Every
claim carries [verified: …] or [not verified: …].

## Summary

1. Every-write systems: Elm signals, Reflex `holdDyn`, Excel dirty marking.
   Equality-cutoff systems: Solid, Svelte 5, Vue 3 refs, MobX. D32 joins the
   first camp.
2. Every every-write system shipped the equality filter as day-one library
   API (`Signal.dropRepeats`, `holdUniqDyn`); D32's "if ever wanted"
   understates it.
3. Every cutoff system has a durable complaint class (in-place mutation does
   not fire: Solid #1164, Vue deep-watch note) and an opt-out (`equals: false`).
4. Where cycles are possible a runtime bound is universal: Vue 100 (dev only),
   MobX 100, Svelte 1000, Solid 1.x 1e6 queued, Solid 2.0 dev guard (rc.4 prod
   hung), Excel 100 / 0.001. Reflex bounds structurally and misfires (#516).
5. Elm <0.17 needed no bound (`foldp` is the only back edge, next event).
   Signals went for concept load: they "did not make Elm *easy*".
6. Closest copy/live analogue: Reflex `tag (current d) e` vs `Dynamic`, with
   `tagPromptlyDyn` (same-frame value) warned against in cycles; React Effect
   Event vs Effect, with a linter React says never to suppress.
7. Nobody made copy/live a keyword; all made it a function on a typed value.
   Svelte 4's compiler-decided `$:` was replaced by runes ("hard to refactor").
8. Same-trigger simultaneity is ordinary and deterministic everywhere (Elm
   "left update wins", Reflex `leftmost`/`mergeWith`); D15's error is stricter
   than any system found.
9. Excel orders at run time ("dynamically determines the calculation
   sequence"; a formula "can be calculated multiple times per recalculation"),
   treats unknowable references as volatile, and shows a cycle as a warning
   plus "0 or the last calculated value", not an error value.
10. Net: D32/D33/D30/§4.3 placement supported; D15 ties and D16 "error value"
   contradicted; the plan lacks the bound and loop diagnostic for D9 cycles.

## Elm before 0.17 (core 3.0.0 `Signal`)

What it does: one `Signal a` type, "A *signal* is a value that changes over
time." `foldp` is state (HOLD), `sampleOn` is copy, `map` is live.
Mechanism [verified: raw.githubusercontent.com/elm-lang/core/3.0.0/src/Signal.elm]:
- `foldp`: "Create a past-dependent signal. Each update from the incoming
  signals will be used to step the state forward." No equality test.
- `sampleOn`: "Sample from the second input every time an event occurs on the
  first input." `map2`: "reevaluated whenever *either* signal changes".
- `dropRepeats`: "Drop updates that repeat the current value of the signal."
  Its existence proves the default was every update.
- `merge`: "If an update comes on both signals at the same time, the left
  update wins." `mergeMany`: "the left-most update wins."
Static graph (no signals-of-signals; only `foldp` closes loops) [not verified:
Czaplicki's thesis "Concurrent FRP", not fetched].
Why removed [verified: pages/news/farewell-to-frp.elm in
github.com/elm/elm-lang.org, dated 2016-05-10, author Evan]: "signals are one
of the few stumbling blocks left. They made Elm easier than its peers, but
they did not make Elm *easy*." "all the toughest concepts in Elm (signals,
addresses, and ports) could collapse into simpler concepts". "you could do
almost all your Elm programming without thinking about signals at all." "So
is Elm about FRP anymore? No." Footnote: "I had no idea my thesis had so much
in common with synchronous programming languages … I might argue that Elm was
*never* about FRP." Replacement: commands from `update`, and subscriptions
("The connection is opened if anyone is subscribed to it, and it is closed if
no one needs it anymore"), which is D31's query class in spirit.

## Reflex (0.9.4.1)

What it does: Event: "A stream of occurrences. During any given frame, an
`Event` is either occurring or not occurring". Behavior: "can be sampled at
will, but it is not possible to be notified when they change". Dynamic: "a
combination of a Behavior and an Event, with a rule that the Behavior will
change if and only if the Event fires" [verified: hackage-content.haskell.org
reflex-0.9.4.1 Reflex-Class, Reflex-Dynamic].
Mechanism [verified: github.com/reflex-frp/reflex Quickref.md and the Hackage pages]:
- `holdDyn :: a -> Event a -> m (Dynamic a)` "updated when the Event fires";
  `foldDyn` likewise. Every occurrence, equal or not.
- `holdUniqDyn :: Eq a => Dynamic a -> m (Dynamic a)` "removing updates w/o
  value change"; Hackage: "only signals changes if the values actually
  changed". `holdUniqDynBy` takes a predicate.
- `tag :: Behavior a -> Event b -> Event a` "using its current value when
  another Event occurs"; `attachWith` adds a function. This is copy.
- `tagPromptlyDyn d e` "differs from `tag (current d) e` in the case that `e`
  is firing at the same time that `d` is changing. With `tagPromptlyDyn d e`,
  the **new** value of `d` will replace the value of `e`." Warning: "the
  output Event may not be used to directly change the input Dynamic, because
  that would mean its value depends on itself. When creating cyclic data
  flows, generally `tag (current d) e` is preferred."
- qfpl tutorial [verified: qfpl.io/posts/reflex/basics/dynamics]: "`hold`
  updates the `Behavior` in the next frame rather than the current frame."
  The complaint behind `holdUniqDyn`: "if we clicked the same button over and
  over, we'd trigger updates even when the state wasn't changing."
- Simultaneity: `leftmost` "(picks the first)"; `mergeWith` "using a combining
  function when multiple events occur simultaneously"; `merge` "will contain
  all of the input keys that are occurring simultaneously". No error path.
- `performEvent :: Event (Performable m a) -> m (Event a)` "Run side-effecting
  actions in Event when it occurs; returned Event contains results."
- Loop detection [verified: src/Reflex/Spider/Internal.hs `checkCycle`]: nodes
  carry a topological *height*; a merge at `InvalidHeight` during propagation
  does `throwIO EventLoopException`: "causality loop detected: \n compile
  reflex with flag 'debug-cycles' and compile with profiling enabled for stack
  tree". Structural, no iteration count.
Developer evidence:
- reflex-platform #238 "profiling a reflex-dom app" (2018-02-10) [verified:
  title/date; the comment "reduce the firings by using holdUniqDyn" is a
  search snippet, the fetched page showed no comments].
- reflex #231 "Inconsistent Dynamic values" (2018-09-16, open) [verified]: a
  length Dynamic "appears to get updated twice", old then new length; "the
  behavior here seems wrong, or at least not what I expected." A frame-order
  surprise of the D21 kind.
- reflex #516 "Weird casuality loop detection" (2025-01-02, open) [verified]:
  "I don't think there is any causality loop." False positive with
  `mergeMapIncremental`; #91 similar [not verified: title only].

## Solid 1.x and 2.0

Mechanism:
- `createSignal` `equals` [verified: docs.solidjs.com reference create-signal]:
  "signals use reference equality (`===`) to compare previous and next
  values"; `equals: false`: "the signal will always update regardless of value
  equality, which is useful for creating signals that trigger manual updates".
- 1.x `writeSignal` [verified: raw packages/solid/src/reactive/signal.ts]:
  `if (!node.comparator || !node.comparator(current, value))` guards the whole
  propagation; inside `runUpdates`: `if (Updates!.length > 10e5) { Updates =
  []; if (IS_DEV) throw new Error("Potential Infinite Loop Detected."); throw
  new Error(); }`. A queue-size bound, and production gets a bare `Error`.
- 2.0 split effects [verified: github.com/solidjs/solid
  documentation/solid-2.0/MIGRATION.md]: "Solid 2.0 splits effects into two
  phases: a **compute** function that runs in the reactive tracking phase and
  returns a value" and "an **apply** function that receives that value and
  performs side effects." "Writing to signals/stores inside a reactive scope
  **throws** in dev." Write-back patterns move to derived stores /
  `createProjection` [split and throw verified; createProjection's description
  is from a search snippet of the RC blog, which did not fetch].
- Discussion #2425 [verified: github.com/solidjs/solid/discussions/2425]:
  devagrawal09 (2026-03-10): "Splitting the pure from the effectful is the
  only way to guarantee that the side effect only runs once and runs
  completely." Pushback, ahzvenol: "Switching to the new split-phase API feels
  cumbersome, also making patterns like tracking B or C depending on A harder
  to express"; about 51,200 `createEffect` uses vs 1,300 explicit `on(...)`.
- 2.0 loop guard [verified: release @solidjs/signals@2.0.0-rc.5, 2026-09-01]:
  "dev threw 'Potential Infinite Loop Detected', production has no counter on
  that loop and hung." Cause: `setSignal` "re-opens a node's stamped
  transaction before the value-equal bail"; the guard now "reports what kept
  the loop alive — transition done-state, queue counts, and the last staged
  node." The equality bail itself was part of the bug.
Developer evidence: solid #1164 "Arrays in createSignal not updating in the
view" (2022-08-13) [verified]: "In the console log, I can see the changes, but
in the browser, they don't appear." (in-place mutation, same reference, no
fire). Discussion #1866 [not verified: title only].

## Svelte 5 (runes)

Mechanism [verified: raw packages/svelte/src/internal/client/reactivity/sources.js, batch.js]:
- `internal_set`: `if (!source.equals(value)) { … mark reactions dirty }`
  (`equals`, or `safe_equals` for mutable sources). Cutoff by default.
- `set` throws `state_unsafe_mutation` inside a derived or block effect (the
  §4.3 placement rule, at run time).
- Loop bound: `while (this.#scheduled.length > 0) { if (flush_count++ > 1000)
  { this.#unlink(); infinite_loop_guard(); } …}`; in DEV the guard walks
  `source.updated` stacks to name the sources that kept updating, then calls
  `e.effect_update_depth_exceeded()`.
- Message [verified: svelte.dev/docs/svelte/runtime-errors]: "Maximum update
  depth exceeded. This typically indicates that an effect reads and writes the
  same piece of state", example `$effect(() => { count += 1; })`.
- Why `$:` went [verified: svelte.dev/blog/runes, 2023-09-20]: "the `$: area =
  ...` declaration can only 'see' `width`, it won't be recalculated when
  `height` changes." "code is hard to refactor, and understanding the
  intricacies of when Svelte chooses to update which values can become rather
  tricky beyond a certain level of complexity." Runes "determine the
  dependencies of their expressions when they are evaluated".
Developer evidence: sveltejs/svelte #13192 (2024-09-10) [verified]: a library
author called the error "almost impossible to debug" because the stack showed
only `effect_update_depth_exceeded`, `infinite_loop_guard`,
`flush_queued_root_effects`, `process_deferred`; closed via PR #13231. Seven
further issues with the same error exist [not verified: titles only]; the DEV
culprit listing exists because of reports like #13192.

## Vue 3

Mechanism [verified: raw packages/reactivity/src/ref.ts, packages/runtime-core/src/scheduler.ts]:
- Ref setter: `if (hasChanged(newValue, oldValue)) { … trigger }`; `hasChanged`
  is `Object.is`. Cutoff by default.
- `const RECURSION_LIMIT = 100`; `checkRecursiveUpdates` counts runs per job
  per flush; on overflow: "Maximum recursive updates exceeded … This means you
  have a reactive effect that is mutating its own dependencies and thus
  recursively triggering itself. Possible sources include component template,
  render function, updated hook or watcher source function." The job is
  skipped. Only under `__DEV__`.
- watch vs watchEffect [verified: vuejs.org/guide/essentials/watchers]:
  "`watch` only tracks the explicitly watched source. It won't track anything
  accessed inside the callback. In addition, the callback only triggers when
  the source has actually changed." "`watchEffect` … automatically tracks
  every reactive property accessed during its synchronous execution." Deep
  watch caveat: "`newValue` will be equal to `oldValue` here on nested
  mutations".
Developer evidence: vuejs/core #10214 (2024-01-25) [verified]: computed with
side effects hit the limit after a 3.4.15 change; the reporter concedes
"computed properties should not have side effects" but "all previous versions
of Vue 3 ... supported this"; fixed by #10232, follow-ups #11121, discussion
#11800 [not verified: titles only]. The bound became a compatibility surface.

## MobX

Mechanism [verified: raw packages/mobx/src/core/reaction.ts; mobx.js.org/reactions]:
- `const MAX_REACTION_ITERATIONS = 100`; on overflow: "Reaction doesn't
  converge to a stable state after 100 iterations. Probably there is a cycle
  in the reactive function: [reaction]" (production: "[mobx] cycle in
  reaction: [reaction]"), then `allReactions.splice(0)` drops all pending
  reactions.
- `reaction`: "By default, the result of the *data* function has to change in
  order for the *effect* function to be triggered." Rule: "**Reactions
  shouldn't update other observables**".
Developer evidence: mobx #1325 (2018-01-29) [verified]: the error came from a
reaction created in `componentWillMount` under Nashorn; closed unexplained.

## React (analogue only)

[verified: react.dev/learn/separating-events-from-effects] "Event handlers run
in response to specific interactions." "Effects run whenever synchronization
is needed." "Logic inside event handlers is not reactive … Event handlers can
read reactive values without 'reacting' to their changes." "Logic inside
Effects is reactive." `useEffectEvent`: "The logic inside it is not reactive,
and it always 'sees' the latest values of your props and state." Chat-room
example: with `[roomId, theme]` as dependencies "the chat reconnects
unnecessarily" on a theme change; the fix moves the theme read into an Effect
Event. "We recommend never suppressing the linter." "If you never suppress the
linter, you will never see problems with stale values." In D30 terms: WHEN =
Effect Event, WHILE = Effect.

## Excel

Mechanism [verified: learn.microsoft.com/office/client-developer/excel/excel-recalculation
and learn.microsoft.com/office/vba/excel/concepts/excel-performance/excel-improving-calculation-performance]:
- "When new data or new formulas are entered, Excel marks all the cells that
  depend on that new data as needing recalculation. Cells that are marked in
  this way are known as *dirty*. All direct and indirect dependents are marked
  as dirty". "Excel continues calculating cells that depend on previously
  calculated cells even if the value of the previously calculated cell does
  not change when it is calculated." Entry counts, not inequality.
- Order is runtime-decided: "Excel dynamically determines the calculation
  sequence based on a list of all the formulas to calculate (the calculation
  chain) and the dependency information about each formula." "if a formula
  depends on one or more formulas that have not yet been calculated, the
  formula is sent down the chain to be calculated again later. This means
  that a formula can be calculated multiple times per recalculation."
- Volatile: "one whose value cannot be assumed to be the same from one moment
  to the next even if none of its arguments … has changed. Excel reevaluates
  cells that contain volatile functions, together with all dependents, every
  time that it recalculates." OFFSET and INDIRECT are in the list: a reference
  Excel cannot know statically is handled by recomputing every time.
- Async UDFs: Excel "runs a new calculation pass to re-compute cells that use
  the cell with the reference to the asynchronous function."
- Circular [verified: both pages plus support.microsoft.com "Remove or allow a
  circular reference"]: "Excel detects the circular reference and warns the
  user"; "Clear the iteration box so that if you have accidental circular
  references, Excel will warn you and will not try to solve them"; "The status
  bar can show Circular References and one cell address"; "By default, Excel
  stops after 100 iterations, or when values change by less than 0.001"; with
  iteration off "Excel might display either 0 or the last calculated value in
  the cell." Not an error value (LibreOffice Err:522 and Sheets "#REF!
  Circular dependency detected" are in the sibling note).

## Comparison table

| System | Fires on | Equality filter | Copy vs live | Ties (same frame) | Loop bound; on hit |
|---|---|---|---|---|---|
| Elm <0.17 | every update | `dropRepeats` opt-in | `sampleOn` / `map` | `merge`: left wins | none needed (static graph, `foldp` next event) |
| Reflex | every occurrence | `holdUniqDyn` opt-in | `tag (current d)` / `Dynamic`; `tagPromptlyDyn` same-frame | `leftmost`, `mergeWith`, `merge` | structural height check; "causality loop detected"; false positives |
| Solid 1.x | value `!==` | default; `equals:false` opt-out | `on(deps)`/`untrack` [not verified] | n/a (glitch-free) | 1e6 queued updates; dev message, prod bare Error |
| Solid 2.0 | value `!==` | default | compute/apply split; writes throw in pure scope (dev) | n/a | dev guard names last staged node; rc.4 prod hung |
| Svelte 5 | `!equals` | default (`safe_equals` for mutable) | `$effect` live; `untrack` | n/a | 1000 flushes; error, DEV lists sources |
| Vue 3 | `Object.is` changed | default; `triggerRef`, `flush:'sync'` | `watch` (copy others) / `watchEffect` (live) | n/a | 100 runs per job, dev only; job skipped |
| MobX | data value changed | default comparer | `reaction(data, effect)` / `autorun` | n/a | 100 iterations; queue cleared |
| React | render; deps `Object.is` | deps array | Effect Event (copy) / Effect (live) + lint | n/a | render-loop error [not verified] |
| Excel | every write, structural dirty | none; volatile = always | n/a | chain order | 100 iterations / 0.001; warning, 0 or last value |
| Boon plan | every write (D32) | "library function, if ever wanted" | WHEN/THEN copy, WHILE live (keywords) | same trigger = error (D15) | static: fire edges never close a cycle; D9 cycles unbounded |

## Answers

(1) Every-write: Elm, Reflex, Excel; cutoff: Solid, Svelte, Vue, MobX.
Every-write complaints are "firing too much" and frame-order surprises
(#231); cutoff complaints are silent non-updates after in-place mutation
(#1164, Vue deep watch) and cutoff/loop-guard interactions (Solid rc.5).
(2) Bounds: 100 (Vue dev-only, job skipped; MobX, queue cleared; Excel), 1000
(Svelte, throws, DEV names sources), 1e6 queued (Solid 1.x, prod bare Error).
Reflex bounds structurally and misfires; Elm made loops unwritable.
Developers hit the bound far from the cause (MobX #1325, Vue #10214, Svelte
#13192), so every project later added culprit reporting.
(3) Copy/live: Reflex `tag (current d) e` vs Dynamic, Vue `watch` vs
`watchEffect`, React Effect Event vs Effect: functions on typed values, not
keywords. Svelte 4's compiler-decided `$:` was regretted because it missed
indirect reads; WHEN copies everything it reads, so that does not transfer.
The React material shows the residual risk: a wrong choice is silent without
a lint.
(4) Excel orders during recalculation, makes unknowable references volatile,
re-passes for async results, and bounds cycles at 100 iterations. A Boon Cells
without runtime-ordered evaluation is a sheet where every formula is
volatile: full recalculation per edit plus a bound (sibling note).

## Implications for the plan

1. **D32 (every write fires) — supports, with a change.** Elm `foldp`, Reflex
   `holdDyn` and Excel dirty marking all fire on equal values, so the rule has
   precedent. But each shipped `dropRepeats`/`holdUniqDyn` at launch and
   Reflex's profiling guidance is built on it. Change D32's "if ever wanted, a
   library function over HOLD" to a P0 deliverable: a catalog function (say
   `Value/changed`) plus a hover line "fires on every write". Spike: count the
   census `THEN` sites over HOLDs that today rely on the engine's dedup
   (machine.rs:21770).
2. **D33 (later piped update resets the HOLD) — supports.** Reflex `holdDyn`
   and Elm `foldp` take every arriving occurrence with no self-retrigger;
   "the Behavior will change if and only if the Event fires" matches "its own
   writes do not re-trigger it". No change.
3. **§4.3 copy rule "reading the committed snapshot (D21) plus the input's new
   value" — supports, add a hint.** This is Reflex's `tag (current d) e`, the
   variant Reflex recommends for cycles; `tagPromptlyDyn` (same-frame value)
   is the one it warns about, and #231 shows the surprise when users expect
   the promptly value. Add to the WHEN/THEN hover: "reads `x` as of the start
   of this step, even if `x` also updates now".
4. **D15 (two arms updating from the same trigger in the same step are an
   error) — contradicts.** Elm `merge` documents "the left update wins", Reflex
   offers `leftmost`, `mergeWith` and all-keys `merge`; fan-out from one press
   into two arms is a normal shape in every FRP found. Proposed change: keep
   the diagnostic as a warning with a fix-it that names the rule ("first arm
   wins", as Elm/Reflex `leftmost`) or add an explicit ordered form; have the
   S4 census count same-trigger LATEST arms before P3b (probe p9 has the shape).
5. **§4.3 "Fire edges never close a cycle" — supports; stronger than every JS
   framework**, which detects loops only at run time. But D9 still permits
   cycles through HOLD/collection updates and with D32 those loop (sibling
   note); every dynamic system here carries a bound *and* a culprit report,
   the plan has neither. Proposed change to R8/§4.3: a per-tick microstep
   bound (100 is the conventional figure), enforced in release builds too
   (Solid rc.4 "production hung"; Vue's dev-only check became a compatibility
   surface), and on overflow an error value plus a dev-window report naming
   the HOLDs/collections written in the last microsteps (Svelte's DEV source
   walk, Solid's "last staged node").
6. **D16 (Cells behaves like Excel; circular references shown as an error
   value) — contradicts in detail.** Excel shows a warning, a status-bar
   marker and "0 or the last calculated value"; only iterative mode has a
   bound (100 / 0.001). "Error value" is LibreOffice/Sheets behaviour.
   Proposed change: name the spreadsheet Cells copies, and specify
   recalculation in Excel's terms: every Boon formula is volatile (full recalc
   per edit) unless the language gains dependency-ordered evaluation; the §5.1
   "visited / depth guard" then needs the bound from item 5, not recursion.
7. **D30 (WHEN copies, WHILE is live) — supports, with a default-on warning.**
   React (Effect Event vs Effect) and Vue (watch vs watchEffect) are the same
   two behaviours, and both treat the wrong choice as the main bug class:
   React says "never suppressing the linter" avoids "problems with stale
   values". The §4.3 open item ("P0 decides whether a WHEN … into the document
   also gets a warning") should resolve to yes, on by default, fix-it "use
   WHILE". Svelte's `$:` history warns only about compiler-truncated
   dependency sets; WHEN copies all reads, so that failure does not apply.
8. **D31/§4.3 placement (state and live queries only in live contexts,
   commands only in copy contexts) — supports.** Solid 2.0 dev "throws" on
   writes inside pure scopes, Svelte throws `state_unsafe_mutation`, MobX rules
   "Reactions shouldn't update other observables", Vue's loop message names
   "watcher source function" mutation. Compile errors are the stronger
   position. Expect the Solid 2.0 pushback ("cumbersome", 51,200 vs 1,300
   uses): the WHEN→WHILE census (1,400 vs 40) is the same ratio problem.
9. **D34 (hidden asynchrony, value keeps its last answer) — weakly supported.**
   Elm subscriptions and Reflex `performEvent` results are plain occurrences;
   Solid 2.0 added async with "stale" values [not verified beyond discussion
   snippets]. No system found made "loading" a language-level state.
10. **Spike for §4.3 / R8.** Port the qfpl counter (`holdDyn` vs
    `holdUniqDyn`), Reflex #231 (list length read in the frame of the removal)
    and a two-HOLD ping-pong under D9's collection-update allowance as Boon
    probes under `change_probes/`; they are the smallest reproductions of the
    complaint classes above and exercise the bound from item 5.

## Sources

1. Elm core 3.0.0 `Signal.elm` — https://raw.githubusercontent.com/elm-lang/core/3.0.0/src/Signal.elm — fetched.
2. "A Farewell to FRP", Evan Czaplicki, 2016-05-10 — https://elm-lang.org/news/farewell-to-frp — JS-rendered; text fetched from https://raw.githubusercontent.com/elm/elm-lang.org/master/pages/news/farewell-to-frp.elm (curl).
3. Reflex Quickref — https://github.com/reflex-frp/reflex/blob/develop/Quickref.md — fetched.
4. Reflex.Class / Reflex.Dynamic 0.9.4.1 — https://hackage-content.haskell.org/package/reflex-0.9.4.1/docs/Reflex-Class.html and …/Reflex-Dynamic.html — fetched (redirect from hackage.haskell.org).
5. Reflex Spider internals — https://raw.githubusercontent.com/reflex-frp/reflex/develop/src/Reflex/Spider/Internal.hs — fetched (curl).
6. qfpl "Dynamics" — https://qfpl.io/posts/reflex/basics/dynamics/index.html — fetched.
7. reflex #231 — https://github.com/reflex-frp/reflex/issues/231; reflex #516 — …/issues/516; reflex-platform #238 — https://github.com/reflex-frp/reflex-platform/issues/238 — fetched (issue bodies; #238 comments not shown). reflex #91 — title only.
8. Solid `createSignal` — https://docs.solidjs.com/reference/basic-reactivity/create-signal — fetched.
9. Solid 1.x `signal.ts` — https://raw.githubusercontent.com/solidjs/solid/main/packages/solid/src/reactive/signal.ts — fetched (curl).
10. Solid 2.0 MIGRATION.md — https://github.com/solidjs/solid/blob/next/documentation/solid-2.0/MIGRATION.md — fetched.
11. Solid discussion #2425 — https://github.com/solidjs/solid/discussions/2425 — fetched; #1164 — https://github.com/solidjs/solid/issues/1164 — fetched; discussion #1866 — title only.
12. @solidjs/signals 2.0.0-rc.5 — https://github.com/solidjs/solid/releases/tag/%40solidjs%2Fsignals%402.0.0-rc.5 — fetched.
13. Solid 2.0 RC blog — https://www.solidjs.com/blog/solid-2-0-rc-the-big-reveal — not fetched (client-rendered shell); search snippet only.
14. Svelte runtime errors — https://svelte.dev/docs/svelte/runtime-errors — fetched.
15. Svelte `sources.js` — https://github.com/sveltejs/svelte/blob/main/packages/svelte/src/internal/client/reactivity/sources.js — fetched; `batch.js` — raw.githubusercontent.com …/reactivity/batch.js — fetched (curl).
16. "Introducing runes", 2023-09-20 — https://svelte.dev/blog/runes — fetched; sveltejs/svelte #13192 — https://github.com/sveltejs/svelte/issues/13192 — fetched; other Svelte issues — titles only.
17. Vue `scheduler.ts` — https://github.com/vuejs/core/blob/main/packages/runtime-core/src/scheduler.ts — fetched; `ref.ts` — …/packages/reactivity/src/ref.ts — fetched.
18. Vue watchers guide — https://vuejs.org/guide/essentials/watchers.html — fetched; vuejs/core #10214 — https://github.com/vuejs/core/issues/10214 — fetched; #11121, discussion #11800 — titles only.
19. MobX `reaction.ts` — https://github.com/mobxjs/mobx/blob/main/packages/mobx/src/core/reaction.ts — fetched; https://mobx.js.org/reactions.html — fetched; mobx #1325 — https://github.com/mobxjs/mobx/issues/1325 — fetched.
20. React "Separating Events from Effects" — https://react.dev/learn/separating-events-from-effects — fetched.
21. Excel Recalculation — https://learn.microsoft.com/en-us/office/client-developer/excel/excel-recalculation — fetched.
22. "Excel performance: Improving calculation performance" — https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-improving-calculation-performance — fetched.
23. "Remove or allow a circular reference" — https://support.microsoft.com/en-us/office/remove-or-allow-a-circular-reference-8540bd0f-6e97-4483-bcf7-1b49cd50d123 — fetched (the `…8540bf0f…` variant 404s).
24. Not fetched: Czaplicki's thesis "Concurrent FRP" (static-graph claim), web.archive.org (blocked by the fetch tool), Solid `on` reference (404), React render-loop error docs.
