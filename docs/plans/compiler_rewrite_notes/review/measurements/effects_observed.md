# Effects observed: re-run and startup behaviour of transient effects today

Measurement for the independent review of `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`.
Date 2026-09-30. Tree: branch `compiler-rewrite-plan`, HEAD `defb103c`, clean
working tree; the runtime under test is `crates/boon_plan_executor` and
`crates/boon_runtime` as checked in (built fresh in the `test` profile for this
measurement). Machine: i7-9700K, Linux 6.18.7, rustc 1.97.1.

The plan's notes (`compiler_rewrite_notes/change_and_effects.md`, "Open
issues") list two claims that were read from code and never observed, because
`boon_cli run` has no host-service runner:

1. a THEN-triggered transient effect re-runs when a HOLD read by its intent
   changes, cancelling the call still in flight;
2. whether an effect inside a WHEN or WHILE arm fires at startup when the
   selector's initial value already opens the gate.

Both are now observed. **(1) Yes, exactly as claimed, and also after the call
has completed. (2) No: with the gate open from the start the effect never
fires, at startup or later, until the selector actually transitions.**

## Method

Harness: a standalone Cargo package outside the repo
(`tools/effects_harness/`, files stored with `.txt` suffixes) that depends on
the workspace crates by absolute path. It follows
`crates/boon_host_runtime/tests/host_services.rs`:

- compiles the Boon source with `boon_program_runtime::compile_program_artifact`
  (`ProgramRole::Server`, `ProgramCapabilityProfile::TrustedServer`, one unit);
- starts `boon_runtime::LiveRuntime::from_machine_template` so the **initial
  turn is visible** (the `ProgramSession::start` wrapper hides it, and refuses
  any startup turn with host work, `program_core.rs:759-777`; the harness also
  reports that verdict);
- dispatches SOURCE events with `source_event_for_path` + `dispatch` and prints
  every `RuntimeTurn.transient_effects` entry (effect id, trigger sequence,
  intent record) and every `cancelled_transient_effects` entry;
- produces Random/bytes completions with the real
  `boon_host_runtime::HostServiceEffectAdapter` and delivers them with
  `LiveRuntime::complete_transient_effect` only when the step says so, which
  makes "still in flight" a controlled condition.

Call ids are opaque (`TransientEffectCallId` redacts both fields in `Debug`,
`machine.rs:1592-1610`), so the harness numbers calls `#1, #2, …` in issue
order, exactly as `boon_behavior_harness` does with its logical call ids.

Why not `boon_cli run --scenario`: the scenario expectation set
(`crates/boon_runtime/src/lib.rs:1894-1932`, `ScenarioExpectation`) has no
transient-effect kind, the CLI prints only turn/state counts
(`crates/boon_cli/src/lib.rs:96-116`), and there is no host to complete calls.
`boon_cli check` also rejects these server-role fixtures ("client programs must
expose one retained document or scene root"), so the fixtures could only be
typechecked through the harness compile.

Effect used: `Random/bytes` (`ReplaySpec::ReadOnly`, transient, single
delivery, `crates/boon_effect_schema/src/lib.rs:214-232`). All six
host-service effects and `Http/request`, `File/read_*`, `Timer/deadline`,
`Wellen/*` share that class, so the staging behaviour observed here is the
class behaviour for transient effects. Durable/outbox effects
(`File/write_text`, `DevelopmentPasskey/*`) were **not** observed.

Commands (run from the harness directory; `EFFECTS_DIR` holds the `.bn` files):

```bash
# dependency build, 2026-09-30 13:41 (uptime before: load 1.31 1.46 1.56)
CARGO_TARGET_DIR=/home/martinkavik/repos/boon-circuit/target cargo test --no-run
#   Finished `test` profile in 1m 31s   (real 1m31.353s, user 5m40.184s, sys 0m14.950s)
# observation run 3 (canonical), 13:45:48 -> 13:45:55
#   uptime before: load 2.81 3.30 2.43 ; after: load 3.97 3.52 2.51
EFFECTS_DIR=…/measure/effects CARGO_TARGET_DIR=/home/martinkavik/repos/boon-circuit/target \
  cargo test -- --nocapture --test-threads=1
#   test result: ok. 6 passed; finished in 0.41s
```

Raw output: `raw/effects_observed_run3.txt` (canonical; runs 1 and 2 are the
same programs before call numbering and the WHEN variant were added, and agree
on every count). No timing is claimed here: the runs are deterministic, the
probes take 0.4 s in total, and the load-average noise is irrelevant to
invocation counts.

## Programs

`q1_then_effect_reads_hold.bn` (question 1):

```boon
store: [
    go: SOURCE
    bump: SOURCE
    size:
        8 |> HOLD size {
            bump |> THEN { size + 1 }
        }
    result:
        NotRequested |> HOLD result {
            go |> THEN { Random/bytes(byte_count: size) }
        }
    result_ready:
        result |> WHEN {
            RandomBytesReady => 1
            __ => 0
        }
]

outputs: [
    size: store.size
    result_ready: store.result_ready
]
```

`q2_while_initially_open.bn` (question 2; `q2_when_initially_open.bn` is the
same with `WHEN` in place of `WHILE`):

```boon
store: [
    bump: SOURCE
    stop: SOURCE
    mode:
        Active |> HOLD mode {
            stop |> THEN { Inactive }
        }
    size:
        8 |> HOLD size {
            bump |> THEN { size + 1 }
        }
    result:
        NotRequested |> HOLD result {
            mode |> WHILE {
                Active => Random/bytes(byte_count: size)
                Inactive => SKIP
            }
        }
    result_ready:
        result |> WHEN {
            RandomBytesReady => 1
            __ => 0
        }
]

outputs: [
    size: store.size
    result_ready: store.result_ready
]
```

`q3_while_initially_closed.bn` / `q3_when_initially_closed.bn`: as q2 but
`mode` starts `Inactive` and a `go` SOURCE switches it to `Active` through a
`LATEST { go |> THEN { Active }  stop |> THEN { Inactive } }`. These are the
baseline that shows the arm effect does fire once the selector transitions.

## Observations

Notation: `INVOKE #n (byte_count=k)` is one entry in `turn.transient_effects`;
`CANCEL #n` is one entry in `turn.cancelled_transient_effects`; both are read
from the turn returned by the dispatch or completion named in the row.

### Q1a: THEN effect; HOLD `size` changes while the call is in flight

| Step | Action | Turn result | `size` | `result_ready` |
|---|---|---|---|---|
| start | initial turn | 0 invocations, 0 cancellations; `ProgramSession::start` Ok | 8 | 0 |
| 1 | dispatch `store.go` | INVOKE #1 (byte_count=8), trigger_sequence 1 | 8 | 0 |
| 2 | dispatch `store.bump` (#1 in flight) | **INVOKE #2 (byte_count=9), CANCEL #1**, trigger_sequence 2 | 9 | 0 |
| 3 | dispatch `store.bump` (#2 in flight) | **INVOKE #3 (byte_count=10), CANCEL #2**, trigger_sequence 3 | 10 | 0 |
| 4 | deliver #1, #2, #3 | #1 and #2 rejected: "unknown, cancelled, or already completed"; #3 accepted, 0 new invocations | 10 | 1 |
| 5 | dispatch `store.bump` (nothing in flight) | **INVOKE #4 (byte_count=11)**, 0 cancellations | 11 | 1 |

One `go` plus three `bump`s produced four invocations. Pending count after
each turn was 1 (one live call), 0 after step 4.

### Q1b: same program; the call is completed before `size` changes

| Step | Action | Turn result | `result_ready` |
|---|---|---|---|
| 1 | dispatch `store.go` | INVOKE #1 (byte_count=8) | 0 |
| 2 | deliver #1 | accepted; 0 invocations | 1 |
| 3 | dispatch `store.bump` (nothing in flight) | **INVOKE #2 (byte_count=9)**, 0 cancellations | 1 |
| 4 | dispatch `store.bump` (#2 in flight) | **INVOKE #3 (byte_count=10), CANCEL #2** | 1 |
| 5 | deliver #2, #3 | #2 rejected (cancelled); #3 accepted | 1 |
| 6 | dispatch `store.go` again | INVOKE #4 (byte_count=10), 0 cancellations | 1 |

The activation stays live after the completion: a later change of the HOLD
read by the intent re-issues the call without any new `go`. A second `go`
re-issues too (the activation is re-captured with the same intent value).

### Q2: WHILE arm and WHEN arm, selector open from the start

Identical for `q2_while_initially_open` and `q2_when_initially_open`:

| Step | Action | Turn result | `result_ready` |
|---|---|---|---|
| start | initial turn | **0 invocations**; `ProgramSession::start` Ok | 0 |
| 1 | dispatch `store.bump` (gate open, size 8→9) | **0 invocations** | 0 |
| 3 | dispatch `store.bump` (size 9→10) | 0 invocations | 0 |
| 4 | dispatch `store.stop` (mode Active→Inactive) | 0 invocations, 0 cancellations | 0 |

Total invocations: 0. The effect in an arm whose gate is open from the initial
value never runs: not at startup, not when its intent input changes, and there
is nothing to cancel when the gate closes.

### Q3 baseline: WHILE arm and WHEN arm, selector starts closed, `go` opens it

Identical for the WHILE and WHEN variants:

| Step | Action | Turn result | `result_ready` |
|---|---|---|---|
| start | initial turn | 0 invocations | 0 |
| 1 | dispatch `store.bump` (gate closed, size 8→9) | 0 invocations | 0 |
| 2 | dispatch `store.go` (Inactive→Active) | **INVOKE #1 (byte_count=9)** | 0 |
| 3 | dispatch `store.bump` (#1 in flight, size 9→10) | **INVOKE #2 (byte_count=10), CANCEL #1** | 0 |
| 4 | deliver #1, #2 | #1 rejected (cancelled); #2 accepted | 1 |
| 5 | dispatch `store.bump` (nothing in flight, size 10→11) | **INVOKE #3 (byte_count=11)** | 1 |
| 6 | dispatch `store.stop` (gate closes) | **CANCEL #3**, 0 invocations | 1 |
| 7 | dispatch `store.go` (re-opens) | INVOKE #4 (byte_count=11) | 1 |

So an arm effect is staged by the selector's **transition into** the arm, then
behaves exactly like the THEN-body effect of Q1: re-run on an intent input
change (in flight → cancel + re-issue; completed → re-issue), and cancelled
when the gate closes. WHEN and WHILE arms are indistinguishable today in this
respect.

## What the code does (corroboration of the observed path)

- Intent and gate evaluation register the effect as a dependency consumer
  (`stage_effect_invocation`, `machine.rs:20089-20255`; `Consumer::Effect`
  passed into `eval_row_expression` and `eval_typed_effect_expression`).
- A dirty dependency re-schedules any **active** effect consumer
  (`machine.rs:22980-22990`: `if self.effect_activations.contains_key(&effect)
  { work.pending_effect_reconciliations.insert(effect) }`); the activation is
  inserted at first staging and is not removed on completion
  (`machine.rs:20233-20240`), which is why Q1b step 3 re-runs.
- `reconcile_dirty_effects` (`machine.rs:19970-20020`) re-stages the same op
  with `TriggerCause::Effect` and the source event's sequence (the observed
  `trigger_sequence` equals the `bump`'s sequence).
- `commit_transient_effects` (`machine.rs:19184-19215`) keeps the last staged
  call per `(invocation_id, owner)` and calls
  `cancel_pending_transient_effect_owner` (`machine.rs:19268-19291`), which
  pushes the older in-flight call into `cancelled_transient_effects`: the
  observed same-turn `INVOKE #n+1` + `CANCEL #n`.
- Gate closed at re-stage → `cancel_pending_transient_effect_owner` directly
  (`machine.rs:20240-20247`): the observed `CANCEL #3` on `stop`.
- Nothing stages an effect at startup: staging only happens inside a triggered
  update; with no state transition there is no activation, so the Q2
  reconciliation path has nothing to reconcile.

## Bearing on the plan

**D31 / R9, "commands staged once with copied arguments and never re-staged
when inputs update".** This *is* a change from today. A `Random/bytes` call in
a THEN body (a *command* under D31) is re-staged today whenever a HOLD read by
its intent updates: the in-flight call is cancelled and a new call is issued
(Q1a steps 2-3), and even a completed call is re-issued on a later input change
(Q1b step 3). With `Clock/wall`, `Random/bytes`, `Http/request` (POST included,
per the notes) and `DevelopmentPasskey/*` in the command class, R9 removes
observable behaviour, and the notes' "latent re-run and duplicate risk" is a
real, reproduced behaviour rather than a reading of the code. The plan's
breakage census should count THEN-body and WHEN-arm effects whose arguments
read a HOLD or a derived value that can update independently of the trigger,
because those sites change behaviour under R9. The same applies to WHEN arms
(D30 calls them copy contexts): today they re-run like WHILE arms (Q3 WHEN).

**D31 queries in a live context (WHILE arm): "starts when its scope activates,
restarts when its arguments change, cancels the superseded run and stops when
its scope ends".** Three of the four clauses already hold today (Q3 steps 3, 5,
6). The first does not: a WHILE arm whose selector is open from the initial
value never starts its effect (Q2, 0 invocations), and nothing starts at
startup at all. So "starts when its scope activates" is new behaviour for
initially-open arms, app start, and (by the same mechanism) restore and hot
reload; the NovyWave-style pattern "state starts Active, effect in the Active
arm" is silently dead today and would come alive under D31.

**D31 commands "never run on start, restore or hot reload".** Matches today's
observable startup behaviour (0 invocations in every initial turn, and the
embedded `ProgramSession::start` even refuses a program whose startup turn
carries host work), but for a different reason: today nothing runs at start,
query or command alike.

**D34 "restarts driven by argument updates" / hand-written
`request_fingerprint` unnecessary.** Today's runtime already restarts every
transient effect on an argument update and cancels the superseded call, once
the effect has been staged at least once. The Wellen fingerprint is therefore
already redundant for restarts *after* the first trigger; what D31/D34 add is
the query/command split and the activation-time start.

**Notes' open issue "possible duplicate command for durable DevelopmentPasskey
effects"**: not observed here (durable outbox effects were out of scope), but
the transient re-stage that would feed it is confirmed.

## Caveats

- One effect (`Random/bytes`), Server role, trusted-server profile, no
  document. The staging code is effect-agnostic for the transient class, but
  outbox/durable effects and stream effects (`File/read_stream`,
  `Content/import`) were not exercised.
- The `size` HOLD changes value on every `bump`; a same-value rewrite of the
  HOLD would not re-run (HOLD dedupes at `machine.rs:21770`), and a derived
  intent argument was not probed here. The notes' probe3 covers the derived
  case for THEN triggers, not for effect intents.
- Startup was observed only through `LiveRuntime::from_machine_template`'s
  initial turn; restore-from-persistence and hot reload were not run.
- The harness lives outside the workspace and reuses the shared
  `target/debug` directory; the dependency build was 1m31s with load ~3-7 from
  the build itself. No repo file outside
  `docs/plans/compiler_rewrite_notes/review/measurements/` was changed.
