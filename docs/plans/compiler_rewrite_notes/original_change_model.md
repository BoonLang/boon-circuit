> **Research input, 2026-09-29. Not authority.** A read-only survey of the
> original Boon repository (`~/repos/boon`, language docs from 2025-11/12, the
> actors engine, and the playground `.bn`/`.expected` examples), made to inform
> open questions L2 (effects) and L3 (events vs "everything is change") in
> `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`. The plan's decisions win. Paths
> are relative to `~/repos/boon` unless marked otherwise.

# Original Boon change and effect model

## Summary

Every expression is a `ValueActor`: a stream of emissions plus a cached
current value (`docs/language/DATAFLOW.md:21`). The language has no event
type. A button press, a HOLD state, a constant and a timer are all streams.

- Constants emit once and then stay silent. Their emissions carry a stable
  identity key.
- A change is a new emission with a fresh idempotency key, not a value that
  differs from the previous one.
- The difference between "event" and "value" lives in the combinator, not in
  the input. THEN/WHEN snapshot their body once per input emission; WHILE and
  plain expressions stream continuously.
- Effects are ordinary builtin calls in pipelines (`|> Router/go_to()`,
  `|> Log/info()`, `|> File/write_text(...)`). They run once per emission of
  their input and return or pass through a value. The only visible trace is
  that the result must be bound to a name.
- The runtime had no ticks or transactions. It used asynchronous actors;
  consistency came from a HOLD permit (one update at a time) and, later,
  Lamport timestamps.

## 1. Events vs values

- `README.md:56`: `Math/sum` outputs "on every change, that is, on every new
  incoming value".
- `SNAPSHOT_VS_STREAM.md:5-16`: streaming (the default) vs snapshot is a mode
  carried in the evaluation context. THEN/WHEN bodies are snapshot, WHILE
  bodies stream, and a function inherits its caller's mode. Line 137: "The same
  function behaves differently based on how it's called."
- The version counter increments on every emission (`DATAFLOW.md:34`). A
  constant is "a stream that emits once then hangs forever" (`DATAFLOW.md:956`).
- `PERSISTENCE.md:70-71`: constants have stable keys; events have a unique key
  per emission.
- Objects are containers of per-field actors. A field changing is not an
  emission of the record, which is why WHEN on a record freezes its fields
  (`WHEN_VS_WHILE.md:34-42`).

## 2. Firing rules

| construct | fires on | source |
| --- | --- | --- |
| THEN | every emission of its input, of any kind (state, derived, timer, event) | `DATAFLOW.md:90-95`; `HOLD.md:332-335` builds `Math/sum` from THEN over any stream |
| WHEN | once per input emission, with dependencies frozen | `README.md:81`, `WHEN_VS_WHILE.md:12`, `when.bn` + `when.expected` |
| WHILE | picks an arm per input emission, then forwards every emission of that arm | `DATAFLOW.md:182-184`; canonical `while.bn` feeds it a LATEST of button-press THENs |
| LATEST | merges all inputs (`select_all`); drops only a re-delivery of the same key | `DATAFLOW.md:267-280`; arms may be constants, continuous values or events (`BOON_SYNTAX.md:675-680`) |
| HOLD | any emission of the piped input resets the state; body events update it; its own updates do not re-trigger | `HOLD.md:17, 41-61, 98-103` |
| SKIP | "don't update, stay at current value"; a SKIP result is filtered out of LATEST | `SKIP.md:10`, `BOON_SYNTAX.md:1178-1182` |

LATEST starts UNDEFINED without a default arm (`LATEST.md:36-59`) and was the
stateful form (`0 |> LATEST count {...}`) until 2025-12-01, when that became
HOLD and LATEST lost self-reference (`LATEST.md:10`).

## 3. What counts as a change

Every emission counts, including an equal value. Deduplication is always by
emission identity, never by value: LATEST (`DATAFLOW.md:276-280`), `Math/sum`
(`crates/.../api.rs:1921`), even `Stream/distinct` (`api.rs:4565`).

- `counter.bn:13-18` is `LATEST { 0, press |> THEN { 1 } } |> Math/sum()`;
  `counter.expected:24-31` requires three quick clicks to go from 2 to 5.
- Adding a todo with the same text twice adds it twice.
- Across code edits, identity is by stable source key: `LATEST {1,2}` edited to
  `LATEST {3,2}` gives 1, 2, 3 because "Number `2` hasn't changed"
  (`README.md:52`). This also stops effects re-firing after a reload.
- The engine added a 64-entry `ValueHistory` "preventing message loss"
  (`engine.rs:4142-4144`), which confirms individual emissions matter.

Non-authoritative counterpoints: `THOUGHT_EXPERIMENTS.md:1034-1039` uses
`if self.x != new_x` cutoffs; `boon_3_idea_3.md:124` updates only "if value
differs"; ActorsLite (`docs/plans/actors_lite.md:131-133`, 2026-03) cuts off
scalars by value but never pulses or queues.

## 4. Effects

- Plain function calls on streams, run once per input emission:
  `LATEST {...} |> Router/go_to()` (`todo_mvc.bn:9-15`), `Router/route()` and
  `Timer/interval()` as sources, `|> File/write_text(path: ...)` and
  `logged: ... |> Log/info()` (`todo_mvc_physical/BUILD.bn:32-51`). `Log/info`
  passes its input through; `Router/go_to` emits `[]` per input.
- What the user sees: the result must be bound to a name
  (`BOON_SYNTAX.md:926-948, 1028`). Liveness depends on reachability
  (`pages.bn:7`: "must be inside store to keep actor alive").
- FLUSH suppresses effects downstream (`FLUSH.md:188-193, 231, 863`).
- Reading mode picks query vs subscription: in a THEN body a server list count
  is one RPC; outside THEN it is a persistent subscription
  (`SNAPSHOT_VS_STREAM.md:77-103`).
- DOM events are never replayed after reload (`PERSISTENCE.md:74`).
- Proposals to make effects explicit stayed at the IR level, with "No Boon
  syntax changes" as a goal: a warning for effects inside HOLD bodies
  (`LATEST_COMPILER_RULES.md:236-266`), `Effect` IR nodes
  (`boon_3_idea_1.md:156`), effects queued until after stabilization
  (`boon_3_idea_2.md:258-261`), and FIFO-ordered effect nodes with effects as
  data (`boon_3_idea_4.md:44, 442-457, 566`).
- No HTTP/fetch example exists; the examples use only File, Log, Router,
  Build, Timer and Directory.

## 5. Values that have not emitted yet

They render nothing (`then.expected:9`, `while.expected:9`). `interval_hold.bn`
uses `Stream/skip(count: 1)` to hide HOLD's initial emission. A THEN body
reading a value with no emission yet waits for the first one
(`CurrentValueError::NoValueYet`). The v3 ideas proposed that reading an
unbound LINK yields SKIP (`boon_3_idea_2.md:325-334`).

## 6. Type-level distinction, glitches and ticks

- Against marking events: the engine first hardcoded
  `is_event_link = "click" | "press" | ...`, then replaced it with
  happened-before filtering instead of a `LinkType` Event/State enum, so that
  "No manual marking of event types required"
  (`lamport-clock-stale-event-filtering.md:9-14, 126-132`). THEN snapshots read
  the value from just before the triggering emission
  (`engine.rs:4206-4219`).
- For a distinction, only at the runtime/IR level: event vs state ports
  (`boon_3_idea_1.md:99-108`); "WHILE input should usually be a behavior, not an
  event" (idea 2/3); `EdgeKind::Trigger` vs `EdgeKind::Data`
  (`boon_3_idea_4.md:282-296`); signal vs value for hardware
  (`WHEN_VS_WHILE.md:184-187`).
- Known glitches: LATEST with a constant then a real value blinks the UI
  (`BOON_SYNTAX.md:804-810`); the diamond problem
  (`THOUGHT_EXPERIMENTS.md:1221-1290`); circular reads see new values
  (`LATEST_COMPILER_RULES.md:225-226`). Deterministic ticks with commit phases
  appear only in the v3 ideas, and HOLD commit timing stays open there
  (`boon_3_idea_2.md:644-646`).

## 7. Representative snippets (`playground/frontend/src/examples/`)

```boon
-- counter/counter.bn:13-18
counter: LATEST { 0, increment_button.event.press |> THEN { 1 } } |> Math/sum()

-- counter_hold/counter_hold.bn:13-15
counter: 0 |> HOLD counter { increment_button.event.press |> THEN { counter + 1 } }

-- todo_mvc/todo_mvc.bn:107-114
completed: False |> HOLD state {
    LATEST {
        checkbox.click |> THEN { state |> Bool/not() }
        toggle_all.click |> THEN { store.all_completed |> Bool/not() }
    }
}

-- timer/timer.bn:4, 18-23
tick: Duration[milliseconds: 100] |> Timer/interval()
raw_elapsed: 0 |> HOLD state {
    LATEST { tick |> THEN { state + 0.1 }  reset.press |> THEN { 0 } }
}

-- then/then.bn:1-9 (snapshot of two timer-driven HOLDs)
current_sum: addition_button.event.press |> THEN { input_a + input_b }

-- interval_hold/interval_hold.bn:1-8
0 |> HOLD counter { tick |> THEN { counter + 1 } } |> Stream/skip(count: 1)
```

## Differences from current boon-circuit that bear on L2/L3

1. Flow modes: boon-circuit's `TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:165-188`
   defines `Flow<T>` (continuous, tick-present, absent), and
   `LANGUAGE_SEMANTICS.md:411` gives WHEN two typed modes. The original had one
   stream kind; the mode belonged to the combinator.
2. THEN: boon-circuit makes continuous input a type error (18 "THEN over HOLD
   state" sites to migrate in the rewrite plan's §5). The original allowed THEN
   over any stream.
3. WHILE: boon-circuit rejects an event-style selector; the canonical original
   `while.bn` uses one.
4. LATEST: rewrite-plan D15 allows event arms only. The original allowed
   constants, defaults, continuous values and events, with block order as the
   tie-break.
5. Change detection: boon-circuit uses `changed_at` tick sequences plus
   structural equality; the original counted every emission by identity.
6. HOLD input: boon-circuit expresses dynamic resets inside the body
   (`LANGUAGE_SEMANTICS.md:375`); originally any emission of the piped input
   reset the state.
7. Ticks: boon-circuit has tick phases (`RUNTIME_MODEL.md:76-93`); the original
   had none, which matches the owner's D21 explanation.
8. Event sources: boon-circuit's `SOURCE` declares input ports; the original used
   `LINK` plus the `.event.*` path convention and deliberately avoided typing
   events.
9. SKIP: boon-circuit ties absence to the current tick; originally it meant
   "no value emitted", with no tick.
10. Effects: boon-circuit has effect schemas, asynchronous completion as a
    temporal boundary, and `List/replace_all` (D17). Originally effects were
    untyped pipeline calls firing per emission, possibly in streaming contexts.
11. boon-circuit's `examples/counter_latest.bn` and `interval_latest.bn` use
    self-referential LATEST, which matches neither the original nor D15.

Treat with caution: `latest/latest.expected` (added 2026-03, ActorsLite era)
contradicts accumulate-every-emission semantics, and
`THOUGHT_EXPERIMENTS.md:1108-1160` models LATEST as combine-latest, which
disagrees with `LATEST.md`.
