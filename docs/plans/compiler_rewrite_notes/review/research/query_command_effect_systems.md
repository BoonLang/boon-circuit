Research note, 2026-09-29, for review/REVIEW.md; not authority.

Topic: query/command effect systems, for D31, D32, D34, D36, D12/D35, §4.3
"Placement", R9 and the effect log. Systems: Bonsai, Electric v3 (+Missionary),
TanStack Query v5, Elm 0.19, RxJS 7, Koka, SwiftUI `.task(id:)`, React, RFC
9110, Stripe keys, outbox. Sibling notes `change_semantics_frp_ui_excel.md`
and `copy_vs_live_synchronous_languages.md` cover "what counts as a change".

Method: curl (copies in `scratchpad/research/effects/`) or WebFetch; plan
text quoted verbatim; claims marked [verified: …] or [not verified: …].

## Summary

1. Every surveyed system restarts a *live* resource on argument **inequality**:
   Bonsai (mandatory `~equal_input`), TanStack (deterministic key hash), Elm
   (effect manager diffs subscriptions by key), React (`Object.is` per dep),
   SwiftUI (Equatable `id`, secondary source). None restarts on an equal write.
2. Elm shows D32 and equality can coexist: `subscriptions model` is re-run
   after *every* message, and the effect manager keeps any subscription whose
   key is unchanged. Equality lives at the host boundary, not in the dataflow.
3. The reverse also holds: equality-keyed systems need a fresh UUID to make a
   one-shot task fire on an equal press (SwiftUI TaskTrigger). D32 fits
   copy-context triggers and commands; it does not fit live-query restarts.
4. D31's wording "restarts when its arguments change" is ambiguous under D32.
   Read literally, it means restart-on-every-write plus cancel, which can
   starve a query whose argument is rewritten faster than the query answers.
5. Supersession: switch (cancel previous) is the consensus for parameterized
   *reads* and is called unsafe for *writes*. Bonsai does not cancel at all.
   It lets later-scheduled results win and throttles to one run in flight plus
   one queued.
6. Commands: Elm, Electric and React tie them to a discrete interaction.
   TanStack runs repeated mutations in parallel by default, serially by
   opt-in, and replays persisted paused mutations after a reload. D31 says
   nothing about command concurrency or result order.
7. D34's "keep the last answer" matches Electric's default (`Offload-latch`)
   and TanStack's opt-in `keepPreviousData`. Both still expose "this answer is
   for old arguments" (`isPlaceholderData`, Bonsai's response carries its
   query). D34 exposes nothing.
8. Restore: queries re-run everywhere. For commands in flight, practice is
   replay with an idempotency key (outbox, Stripe, TanStack resume). That
   conflicts with D31's "never runs on … restore" unless the plan scopes it
   to *new* runs.
9. HTTP splits two properties: *safe* (GET/HEAD) and *idempotent* (PUT and
   DELETE too). D31's single "safe to re-run" test merges them. The
   persistence plan's three-way `EffectReplay` enum already keeps them apart.
10. Koka's inferred rows make purity checkable at every call. Boon's row should
   drive placement checks through FUNCTION calls, not only hover.

## Bonsai (Jane Street, OCaml)

An `Effect.t` is a value that runs only when the runtime schedules it (an
event handler returns it, or an `Edge` combinator schedules it).
- `Edge.on_change ~equal v ~callback`: "The [equal] function is used to
  determine if a value is the same or not. The callback is _always_ invoked
  the first time that this component is becomes active. The callback is
  invoked at most once per frame and will be called with the latest value."
  `equal` is mandatory. [verified: src/cont.mli (master), module `Edge`]
- Guide: "The `~equal` argument allows us to define what 'change' means".
  Also: "Extensive use of the `Edge` module will make your program less and
  less declarative." [verified: bonsai_web docs/how_to/edge_triggered_effects.md]
- Live query: `Edge.Poll.effect_on_change ~equal_input ?equal_result
  starting input ~effect` runs the effect "every time that the input ...
  changes" and "guarantees that the result from an effect that are scheduled
  later than another effect will take priority, even if the effects complete
  out of order". `Starting.empty` makes the result an `'o option` (no value
  yet); `Starting.initial x` gives a start value. [verified: cont.mli
  `Edge.Poll`]
- No cancellation: "If an effect starts, then it should complete with some
  kind of result - [Effect] does not support cancellation in general."
  `Effect_throttling.poll` keeps at most one run in flight and one queued;
  "Any previously enqueued item gets kicked out ... (this is important so that
  things like RPCs calls don't pile up)". [verified: cont.mli
  `Effect_throttling`]
- Polling RPC: the poller "sends requests whenever the query ... changes"
  (`~equal_query`), "will not send a new request until the previous one has
  completed", and its response state carries the query and
  `last_ok_response`. [verified: bonsai_web docs/how_to/rpcs.md]
- Copy pitfall: a handler's effect "closes over the value of `computed` from
  before the button was clicked", and `Bonsai.peek` exists to read the current
  value. [verified: docs/how_to/effects_and_stale_values.md]

## Electric Clojure v3 (hyperfiddle/electric master, Missionary)

- `e/Token`: a new event creates a token only if none is outstanding
  (`compare-equal-and-set! !x [nil nil] [(->token !x) nil]`), so clicks
  while a command is in flight are **ignored** (exhaust semantics). `(t)` is
  "success, burn token"; `(t err)` is "failed attempt burns token, user must
  interact again" (no automatic retry). [verified: src/hyperfiddle/electric3.cljc
  `Token`]
- The tutorial puts the server command inside `(when-some [t (TxButton …)] …)`
  and disables the button while `?t` is non-nil. The command branch mounts
  once per token. [verified: electric-fiddle
  src/electric_tutorial/token_explainer.cljc]
- `e/Offload` is the default async call: "Buffer the latest result while
  awaiting subsequent values of f, such that intermediate pending states are
  not seen." A source comment explains why latch is the default: "Pending
  model for v3 is still WIP" and "today a naive UI will blink - confusing to
  newcomers". `Offload-reset` is the variant that returns to "no value" while
  pending. [verified: electric3.cljc `Offload`, `Offload-latch`,
  `Offload-reset`]
- Both use Missionary `?<`: "the current processing branch is interrupted when
  the forking flow becomes ready to transfer again" (switch with
  cancellation). [verified: missionary src/missionary/core.cljc `?<`]
- Electric's equality work-skipping in v3 was not confirmed. [not verified:
  not found in electric3.cljc/Readme/CHANGELOG within budget]
- Dev tool `token-trace` prints token trees with added/retracted/changed
  diffs. [verified: src/hyperfiddle/electric_tokens.cljc]

## TanStack Query v5 (TanStack/query main)

- Keys: "Query keys are hashed deterministically", so object key order does not
  matter but "Array item order matters". Include every variable the fetcher
  uses; "any time a variable changes, _queries will be refetched
  automatically_ (depending on your `staleTime` settings)". [verified:
  docs/framework/react/guides/query-keys.md]
- The same key set again fetches nothing. `setOptions` rebuilds the query
  from the cache and fetches only when `shouldFetchOptionally` holds:
  `query !== prevQuery` (a new key hash) and the data is stale. [verified:
  packages/query-core/src/queryObserver.ts]
- In-flight dedup: `fetch` returns the running promise "instead of starting a
  new one, unless `fetchOptions.cancelRefetch` is set and the query already
  has data". [verified: query-core/src/query.ts `fetch`]
- Cancellation: the `AbortSignal` is aborted "When a query becomes
  out-of-date or inactive". However, "By default, queries that unmount or
  become unused before their promises are resolved are _not_ cancelled";
  only a query that consumed the signal is cancelled, and its state is then
  "_reverted_". [verified: guides/query-cancellation.md; `removeObserver` in
  query.ts] An old key's answer lands in the old key's cache entry, so it can
  never overwrite the new key's data.
- Stale-while-revalidate is opt-in: `placeholderData: keepPreviousData`, with
  an `isPlaceholderData` flag so the UI can tell. [verified:
  guides/paginated-queries.md, guides/placeholder-query-data.md]
- Structural sharing: results "are structurally shared to detect if data has
  actually changed and if not, the data reference remains unchanged".
  Stale queries refetch on mount, focus and reconnect. Queries retry 3 times;
  mutations are not retried. [verified: guides/important-defaults.md,
  guides/mutations.md]
- Mutations: "Per default, all mutations run in parallel - even if you invoke
  `.mutate()` of the same mutation multiple times". A `scope.id` makes them
  serial. Paused offline mutations "will be retried in the same order when
  the device reconnects", and persisted ones resume after a reload through
  `resumePausedMutations()`. That needs a default mutation function because
  "functions cannot be serialized". [verified: guides/mutations.md]
- v5 removed `onSuccess`/`onError` from `useQuery`: they run once per
  component, and they do not run when data comes from the cache. Mutation
  callbacks stayed. [verified via WebFetch summary:
  tkdodo.eu/blog/breaking-react-querys-api-on-purpose, 2023-04-16]

## Elm 0.19 (elm/core, elm/time, elm/http)

- "Elm has two kinds of managed effects: commands and subscriptions". Because
  effects are data, the runtime "can do some 'query optimization' before
  actually performing the effect". [verified: elm/core src/Platform/Sub.elm]
- After *every* message the kernel runs `update`, then
  `_Platform_enqueueEffects(managers, pair.b, subscriptions(model))`.
  [verified: elm/core src/Elm/Kernel/Platform.js `sendToApp`]
- The `Time.every` manager reconciles with `Dict.merge` keyed by interval.
  New keys spawn a process, keys present on both sides keep theirs, and
  removed keys are killed. An equal subscription is never restarted.
  [verified: elm/time src/Time.elm `onEffects`]
- `Cmd.batch`: "no ordering guarantees about the results". [verified:
  Platform/Cmd.elm] Commands come only from `init`/`update`, which run on a
  message.
- A new `Http` request with an already-used tracker spawns a second process
  and overwrites the dict entry; the old request is not cancelled.
  Supersession is manual (`Http.cancel`). [verified: elm/http src/Http.elm
  `updateReqs`]

## RxJS 7 and the switchMap debate

- `switchMap` "stops emitting items from the earlier-emitted inner
  Observable". `exhaustMap` "ignores every new projected Observable if the
  previous projected Observable has not yet completed". `concatMap` waits
  "for each one to complete", with a warning about an unbounded buffer.
  [verified: rxjs 7.x src/internal/operators/*.ts doc comments]
- N. Jamieson (RxJS core team, 2018): switchMap on writes aborts requests the
  server may already have processed. Use `concatMap`/`mergeMap` for writes,
  `switchMap` for reads that go stale, `exhaustMap` to ignore repeats; "If
  you are unsure ... use `concatMap`." A lint rule `rxjs-no-unsafe-switchmap`
  exists. [verified via WebFetch summary:
  ncjamieson.com/avoiding-switchmap-related-bugs]

## Koka

- "Koka infers and tracks the effect of every function in its type". It has
  `total`, `exn`, `div`, `pure` (= exn+div), `ndet` (non-deterministic), `st<h>`
  and `io`. [verified: koka doc/spec/why.kk.md]
- Effects are polymorphic: `map : (xs : list<a>, f : (a) -> e b) -> e
  list<b>`. At a call site the rows "are extended automatically until they
  match", which yields their union. A function with only local state is
  inferred `total` ("fib3"). [verified: doc/spec/tour.kk.md "Polymorphic
  effects", "Local Mutable Variables"]
- Relevance: the row is a checked part of the type, not documentation.
  Random numbers are `ndet` and kept apart from I/O, so Koka tells "answers
  differently" apart from "acts on the world". D31 merges the two into
  "command".

## SwiftUI `.task(id:)`

- The Apple page could not be fetched. [not verified: 404 via WebFetch;
  the JSON endpoint returned HTML] Secondary summaries say the task is
  cancelled and recreated when `id` changes, tested with Equatable.
- TaskTrigger (a third-party library) documents the consequence. A one-shot
  action on `.task(id:)` needs manual reset housekeeping ("Otherwise another
  tap wouldn't trigger the `task(id:priority:_:)` again"). The library's
  `trigger(value:)` therefore creates "a new `UUID` whenever the method is
  called", so equal taps fire and "prior operations will get cancelled if they
  are still running". [verified: github.com/lukepistrol/TaskTrigger README]

## React `useEffect`

- "React will compare each dependency with its previous value using the
  `Object.is` comparison." Objects created during render "cause the Effect to
  re-run more often than needed", and the fix is to remove them. The linter
  "will verify that every reactive value is correctly specified as a
  dependency". [verified via WebFetch: react.dev/reference/react/useEffect]
- "If this logic is caused by a particular interaction, keep it in the event
  handler. If it's caused by the user *seeing* the component on the screen,
  keep it in the Effect." A race in data fetching is fixed with a cleanup
  that ignores stale responses. [verified via WebFetch:
  react.dev/learn/you-might-not-need-an-effect]
- Strict Mode runs effects twice in development so that non-idempotent
  effects show up. [verified: same page, "runs twice in development"]

## HTTP, idempotency keys, outbox

- RFC 9110 §9.2.1: GET, HEAD, OPTIONS and TRACE are *safe*, so that
  "pre-fetching" can "work without fear of causing harm". §9.2.2: "PUT,
  DELETE, and safe request methods are idempotent"; a client "SHOULD NOT
  automatically retry a request with a non-idempotent method unless it has
  some means to know" it is idempotent or was never applied. [verified:
  rfc-editor.org/rfc/rfc9110.txt]
- Stripe saves "the resulting status code and body of the first request made
  for any given idempotency key, regardless of whether it succeeds or fails".
  Keys may be pruned after 24 hours, and reuse with different parameters is
  an error. It advises "Don't send idempotency keys in `GET` and `DELETE`
  requests" because they are "idempotent by definition". [verified via
  WebFetch: docs.stripe.com/api/idempotent_requests]
- Transactional outbox (C. Richardson): the relay "might publish a message
  more than once", so consumers must be idempotent. [verified via WebFetch:
  microservices.io/patterns/data/transactional-outbox.html]
- Boon's persistence plan already has `EffectReplay { ReadOnly, Idempotent {
  key_type }, NonReplayable }`, and outbox step 6 reads: "Restart replays
  pending idempotent intents or reconciles their remote status".
  [verified: docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md §Effect
  Contracts, §Durable Outbox]

## Answers to the review's questions

1. **Live-query restart trigger.** Prior art answers "argument inequality",
   and Elm gets there without an equality cutoff in the dataflow.
   Restart-on-every-write has no precedent. React treats it as a bug
   (over-firing on fresh objects).
2. **Commands triggered by always-updating values.** Nothing forbids it
   statically except Elm, where commands come only from `update(msg)`.
   React's docs steer commands to handlers, Electric requires a token from a
   discrete event, and TanStack removed query-completion callbacks.
   Nothing dedupes commands automatically.
3. **Copy-context supersession.** switchMap is the default for reads
   (Jamieson; Missionary `?<`; NgRx blog convention, search summaries only).
   It is wrong for writes, and it starves under a high trigger rate. Bonsai uses
   latest-scheduled-wins without cancel, plus one-in-flight throttling.
4. **Restore.** Queries re-run everywhere. For commands in flight, practice
   is replay with an idempotency key; nobody drops them silently.
5. **Visibility.** Koka puts rows in types. React relies on a lint for
   manual deps, which Boon does not need because the compiler knows the
   inputs. Dev tools show per-request state: Bonsai's poller details
   (queries, responses, timestamps) and Electric's token trace.

## Implications for the plan

1. **D31 × D32: restart rule is undefined** (contradicts / needs owner). D31:
   "restarts when its arguments change"; D32: "Every write fires, equal or
   not: one rule everywhere". Proposed: R9 reconciles live queries the way
   Elm's effect managers do. Every argument update re-evaluates the resource
   descriptor (call site, instance path, argument tuple), and the host
   restarts only when the descriptor differs structurally from the running
   one. Nothing downstream fires differently, so D32 is untouched in the
   dataflow; equality lives only in resource identity. BYTES arguments
   (`Wellen/open(content: …)`) need a pointer-then-hash fast path. Owner
   decision, because D32 says "no equality in the engine".
2. **Spike S-EFF1 (P0).** On the NovyWave and Cells scenario corpus, count,
   per live-query site, argument updates against distinct argument tuples.
   Add one "hold W for 2 s" scenario with `Wellen/signal_page` latency above
   the key-repeat interval. If updates far exceed distinct tuples, or the held
   key yields no answer, literal D32 restarts are ruled out.
3. **Supersession policy (R9, D31 "cancels the superseded run")**
   (suggests-change). Split the policy by class:
   - queries: drop the stale result, abort only when the host supports it
     (TanStack revert, Bonsai no-cancel);
   - if the restart rule stays literal (every write), use Bonsai's one in
     flight plus the latest queued instead of switch, so a churning argument
     cannot starve the answer;
   - commands: never cancelled (as change_and_effects.md line 51 already
     says), and R9 must state it.
4. **Command concurrency and result order (D31, D32, R9)** (suggests-change).
   D31's "runs exactly once each time the input updates" forbids exhaust
   (Electric) but picks neither merge nor concat. With merge (TanStack's
   default), D32's "an effect-result HOLD fires on every completion" lets an
   older `Http/send` that finishes last overwrite a newer result. Proposed
   default: commands run serially per call site and instance (concatMap,
   TanStack `scope`), so results arrive in issue order. The effect log shows
   a "queued" state.
5. **§4.3 "Placement": trigger provenance for commands** (suggests-change).
   Under D32, `store.x |> THEN { Http/send(…) }` sends once per write,
   including equal writes. And a query re-run on restore completes, which is
   an update, so a command in a THEN over that result runs after every
   restart or hot reload. That breaks D31's "never runs on start, restore or
   hot reload" through one hop. Proposed: the checker traces each command's
   trigger back to its roots:
   - a SOURCE occurrence: fine;
   - a live-query completion: warning "runs again after every restart
     because `Wellen/open` re-runs";
   - a HOLD or derived value: hover note;
   - add these classes to the S4 census.
6. **D12 × D31: stuck business-logic state after a crash** (contradicts).
   D12 persists "LATEST current values". D34's own idiom `LATEST { …,
   request |> THEN { Loading }, … }` therefore restores `Loading` or
   `Sending`, and a transient command that was in flight never completes.
   Proposed R9 rule: every call site with a command in flight at crash gets
   an `Interrupted` outcome on its own result path after restore, so
   business logic can leave the pending state.
7. **D31 "never runs on … restore" vs outbox step 6** (contradicts). Proposed
   D31 wording: "never *starts* on start, restore or hot reload. A durable
   command already staged before a crash is resumed with its original
   idempotency key if its contract is Idempotent, or reconciled or
   interrupted otherwise." This matches TanStack's resume, Stripe's keys and
   the persistence plan's `EffectReplay`.
8. **D36: three classes, not two** (suggests-change). RFC 9110 separates
   safe from idempotent. `Http/send` covers POST (non-idempotent) and
   PUT/DELETE (idempotent): the query/command split plus the existing
   `EffectReplay` attribute. R9 generates one idempotency key per command
   run, persisted with the outbox item and sent as a header where the host
   supports it. `Http/get` stays a query (safe, so the runtime may re-run it
   freely).
9. **D34: staleness is invisible** (suggests-change). D34 says the value
   "keeps its last answer", which Electric and TanStack endorse. Both also
   expose that the answer is for old arguments. If implication 1 is adopted,
   the D34 idiom `args |> THEN { Loading }` breaks: it fires on an equal
   write that no longer restarts the query, so the value sticks at
   `Loading`. Proposed catalog change, no syntax: query results carry the
   arguments they answer (Bonsai's response carries `query`). Then "loading"
   is `result.for != current_args`, which is correct under both restart
   rules.
10. **FUNCTION effect rows must check, not just display (D31, §4.3)**
    (suggests-change). D31 lists the "inferred effect row on every FUNCTION
    scheme" under visibility. As in Koka, the row should be the placement
    requirement of a call:
    - a row with a command means the call is legal only in a copy context;
    - a row with a live query makes the call site a live resource owner.
    Otherwise a command wrapped in a FUNCTION escapes "Placement". P0 spec
    item.
11. **Effect log schema (R9)** (suggests-change). Modelled on Bonsai's
    poller details and Electric's token trace. Each record has site,
    instance path, class (query / idempotent command / command), trigger
    (update id and provenance root), argument digest, state (issued, queued,
    superseded, dropped-stale, completed, delivered, interrupted, resumed)
    and tick. Scenario assertions:
    - "site X ran N times";
    - "no command started during restore";
    - "no two runs of a command site with equal arguments in one tick".
12. **Strict-Mode-style scenario mode (§5.2)** (suggests-spike). React runs
    effects twice in development to surface non-idempotent effects. The
    runner should add a mode that restarts every live query mid-flight and
    simulates restore with commands in flight. It checks that answers still
    converge and nothing is issued twice (implications 3, 6 and 7).
13. **§4.3 "Placement" supports the Elm/Electric/React consensus** (supports).
    Commands only in copy contexts and live queries only in live scopes is
    the same split as Cmd/Sub, Token/Offload and handler/Effect. Keep it,
    with implications 5 and 10 closing the transitive gaps.

## Sources

1. https://raw.githubusercontent.com/janestreet/bonsai/master/src/cont.mli — fetched (curl); `Edge`, `Edge.Poll`, `Effect_throttling`, `Clock` read.
2. https://raw.githubusercontent.com/janestreet/bonsai_web/master/docs/how_to/edge_triggered_effects.md — fetched.
3. https://raw.githubusercontent.com/janestreet/bonsai_web/master/docs/how_to/effects_and_stale_values.md — fetched.
4. https://raw.githubusercontent.com/janestreet/bonsai_web/master/docs/how_to/rpcs.md — fetched.
5. https://raw.githubusercontent.com/hyperfiddle/electric/master/src/hyperfiddle/electric3.cljc — fetched; `Token`, `Offload*`.
6. https://raw.githubusercontent.com/hyperfiddle/electric-fiddle/main/src/electric_tutorial/token_explainer.cljc — fetched.
7. https://raw.githubusercontent.com/hyperfiddle/electric/master/src/hyperfiddle/electric_tokens.cljc — fetched.
8. https://raw.githubusercontent.com/leonoel/missionary/master/src/missionary/core.cljc — fetched; `?<`.
9. https://electric.hyperfiddle.net/tutorial/token_explainer — no prose (JS-rendered); not used.
10. https://raw.githubusercontent.com/TanStack/query/main/docs/framework/react/guides/{query-keys,important-defaults,query-cancellation,mutations,paginated-queries,placeholder-query-data,render-optimizations}.md — fetched.
11. https://raw.githubusercontent.com/TanStack/query/main/packages/query-core/src/{query,queryObserver}.ts — fetched.
12. https://tkdodo.eu/blog/breaking-react-querys-api-on-purpose — WebFetch summary.
13. https://raw.githubusercontent.com/elm/core/master/src/Platform/{Sub,Cmd}.elm, src/Platform.elm, src/Elm/Kernel/Platform.js — fetched.
14. https://raw.githubusercontent.com/elm/time/master/src/Time.elm — fetched.
15. https://raw.githubusercontent.com/elm/http/master/src/Http.elm — fetched.
16. https://raw.githubusercontent.com/ReactiveX/rxjs/7.x/src/internal/operators/{switchMap,exhaustMap,concatMap}.ts — fetched.
17. https://ncjamieson.com/avoiding-switchmap-related-bugs/ — WebFetch summary.
18. https://raw.githubusercontent.com/koka-lang/koka/master/doc/spec/{why,tour}.kk.md — fetched. (https://koka-lang.github.io/koka/doc/book.html WebFetch gave only a paraphrase; not quoted.)
19. https://developer.apple.com/documentation/swiftui/view/task(id:priority:_:) — 404 via WebFetch; not verified.
20. https://raw.githubusercontent.com/lukepistrol/TaskTrigger/main/README.md — fetched (third-party).
21. https://react.dev/reference/react/useEffect — WebFetch.
22. https://react.dev/learn/you-might-not-need-an-effect — WebFetch.
23. https://www.rfc-editor.org/rfc/rfc9110.txt — fetched; §9.2.1, §9.2.2.
24. https://docs.stripe.com/api/idempotent_requests — WebFetch.
25. https://microservices.io/patterns/data/transactional-outbox.html — WebFetch summary.
26. Repo: docs/plans/BOON_COMPILER_REWRITE_PLAN.md (§1 D12, D31-D36; §4.3; §4.5 R9), docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md (§Effect Contracts, §Durable Outbox), docs/plans/compiler_rewrite_notes/change_and_effects.md (§3.4, line 51), examples/novywave/RUN.bn (lines 128-215: `real_signal_page_request` LATEST over eight press arms feeding `Wellen/signal_page`).
