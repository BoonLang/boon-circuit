Research note, 2026-09-29, for review/REVIEW.md; not authority.

Topic: copy vs live and change semantics in synchronous languages (Esterel,
Lustre/Scade, Lucid Synchrone, ReactiveML, Céu, Lingua Franca, VHDL/Verilog
delta cycles), with causality and termination analysis when every write fires.
Plan targets: D30, D32, D21, D9, D31/D34, §4.3 "Change rules", R8 (§4.5), and
`compiler_rewrite_notes/change_and_effects.md` §3.3 "Tick".

Method: primary documents fetched; PDFs extracted with `pdftotext` into the
scratchpad (`scratchpad/research/sync_languages/*.txt`). External sources are
paraphrased with page/section numbers; plan lines are quoted verbatim.

## Summary

1. "A value changed" is defined three ways in prior art: **every write**
   (Esterel emission, LF port presence, Céu emit, VHDL `'active`/`'transaction`),
   **inequality** (VHDL `'event`, Verilog `@` implicit events), and **no notion
   of change** (Lustre/Scade/Lucid: a flow has a value at every instant of its
   clock; "changed" is written `x <> pre x`).
2. VHDL alone carries both definitions side by side and names them; its
   default process wake-up is inequality. D32 picks the Esterel/LF side; that
   is coherent but makes HOLD write frequency user-visible and needs the
   explicit "changed" library function D32 already promises.
3. Every system with a bounded instant uses the same shape of rule as §4.3
   "Fire edges never close a cycle": the only legal back edge is a *delayed,
   non-triggering* read of state (Lustre `pre`/`fby`; Céu "an await cannot be
   awoken in the chain that emitted it"; ReactiveML "value readable next
   instant"). None lets the delayed value re-trigger inside the same instant.
4. The two systems whose back edge is "next microstep of the same instant"
   (LF logical action with 0 delay; VHDL/Verilog zero-delay assignment) have
   **no static bound**: LF documents a reactor rescheduling itself at one
   logical time; VHDL simulators add runtime limits (GHDL 5000, xsim 10000)
   because the LRM has none.
5. R8 says "follow-up microsteps within a tick, each reading its own committed
   snapshot". Whether that is bounded depends on a distinction the plan does
   not state: a HOLD's committed-value read is non-firing only in copy
   contexts; for live readers a HOLD write *fires* (D32). The bound holds iff
   trigger + live-read + update edges form a DAG and only copy-reads may be
   back edges. §4.3 should say exactly that.
6. Per-microstep evaluation exposes intermediate states to live readers and to
   commands (the diamond glitch). Lustre/Scade avoid it by evaluating each
   flow once per instant with state visible next instant; VHDL/LF accept it
   and make it explicit. The plan must define "step".
7. Copy vs live: Esterel's `?S` read at emission copies a persistent value;
   `emit S(?S+1)` is the documented causality cycle fixed with `pre(?S)`,
   which is D14/D15 "self-reference only via HOLD". Lustre's `when` makes
   sampling a typed combinator; its `current` hold was dropped in Scade 6 for
   yielding `nil` before the first sample, a documented stale-sample pitfall.
8. Esterel's single-signal rule (two emits of one signal in one instant is a
   static error) matches §4.3 LATEST "two arms that can update from the same
   trigger in the same step are an error"; Esterel/ReactiveML/LF also offer a
   merge policy (combine function, gather, last reaction wins).
9. Guards: Esterel, Lustre, Lucid, Céu are static-only; LF static plus a
   runtime deadline; VHDL runtime and tool-specific; Verilog none. A plan
   claiming a static bound should still keep a debug-build assertion.
10. No synchronous language bounds cross-instant loops through asynchronous
    results (D31/D34: a query restarted by its own answer). That is outside
    the static rule and needs a dev-window detector, not a type rule.

## Plan lines under test

- D32: "Every write fires, equal or not ... a derived value fires (at most
  once per step) whenever one of its inputs fires."
- §4.3: "Fire edges never close a cycle. A cycle must pass through a HOLD's
  committed-value read, a collection update or a stateful builtin (D9); a
  THEN input, WHEN selector or update candidate on the cycle is an error, not
  a runtime loop."
- R8: "follow-up microsteps within a tick, each reading its own committed
  snapshot (D21)".
- change_and_effects.md §3.3: "Depth is at most the longest path in the
  change-edge graph, which the cycle rule makes acyclic and statically
  bounded, so no runtime guard is needed."
- D21: "reads within a tick see committed values". D30: "WHEN and THEN copy
  ... WHILE is live".

## Systems

### Esterel (v5_91 primer, Berry)

Imperative synchronous language; a program reacts in discrete instants;
signals are broadcast and present or absent per instant.
- A valued signal has a presence status per instant and a *permanent* value:
  `?S` is the value emitted this instant if S was emitted, else the previous
  value; `pre(?S)` is the previous instant's value; the declared initial value
  serves both [verified: primer §3.3, PDF p.22].
- Presence does not persist: a signal is present in a reaction iff emitted or
  supplied by the environment; the survey likens signals to wires, not
  variables [verified: Benveniste et al. 2003, p.67].
- `emit S(?S+1)` is impossible (?S = ?S+1); the primer names it a causality
  cycle and prescribes `pre(?S)` or a variable [verified: primer §3.5, p.26].
- Single valued signals: at most one `emit` per instant, checked statically
  with `-Icheck`; combined signals merge multiple emissions with an
  associative-commutative function, including an input received and emitted
  locally in one instant [verified: primer §4.4.2 pp.49-50; `emit`, p.58].
- Loops: a body must not be *able* to terminate instantaneously; static over
  potential paths even if never taken; `repeat` in a loop needs `positive`
  [verified: primer pp.57-59].
- `pre(S)` is the previous status, false in a first instant; `pre` cannot be
  nested [verified: primer p.53].
- Constructive semantics: three-valued statuses propagated by facts only; a
  program is constructive if every status is determined, uniquely; cyclic but
  constructive programs (symmetric bus arbiter) are accepted, acyclic ones
  preferred [verified: primer §5.2.2 p.87, §5.2.7 p.92].

### Lustre / Scade 6 (Lustre V6 manual; Halbwachs et al. 1991; Colaço, Pagano, Pouzet 2017)

Declarative dataflow; each variable is a flow with one value per cycle of its
clock; no writes, no triggers.
- Causality: an output at an instant cannot depend on future inputs; bounded
  memory [verified: Lustre V6 §1.1.3, p.8].
- `pre(A)` is `(nil, a1, a2, ...)`; a nil first output is rejected or warned
  [verified: Lustre V6 §1.2.1, p.9]. Two integrators in a loop (sincos) are
  reported as a deadlock because sin and cos depend on each other
  instantaneously; the fix is a `pre` in the loop [verified: Lustre V6
  Examples 8-9, pp.13-14].
- Halbwachs 1991: `when` samples (keeps values where the boolean is true),
  `current` interpolates (value at the last true instant); any cycle must
  contain a `pre`; `X = 3*X+1` has the empty sequence as least solution and
  is rejected as a deadlock; structurally cyclic but dynamically acyclic
  programs are rejected too [verified: §II.B and the cyclic-definitions
  paragraph].
- Scade 6: `current` yields an arbitrarily long `nil` prefix unless the clock
  is initially true, so Scade 6 replaced it with Lucid's `merge`, which
  introduces no nil and needs no memory; the clock calculus rejects
  `x + (x when h)`; causality analysis reduces to instantaneous loops in the
  dataflow (Cuoq-Pouzet modular causality); a causal model compiles to
  statically scheduled sequential code (Property 3) [verified: §II, §IV.C].
- Lustre V6 `merge` generalises `current` [verified: Lustre V6 §3.2].

### Lucid Synchrone (manual 3.0, Pouzet; MPRI causality notes)

Higher-order Lustre with inferred clocks.
- Clocks are types and are inferred; a causality analysis rejects programs
  that cannot be statically scheduled; an initialization analysis rejects
  programs depending on uninitialized delays [verified: manual intro, p.4].
- §1.1.9: a recursive definition must not depend on itself instantaneously
  (diagnostic on `nat = m -> nat + 1`); every loop must cross `pre` or `fby`;
  the check is *syntactic*: the semantically fine `o = f o` with
  `f x = 0 -> pre (x+1)` is rejected [verified: manual §1.1.9].
- §1.2.1 `when` sampler with clock type `'a on c`; §1.2.2 `merge`, with the
  Lustre hold written `merge c x ((ydef -> pre y) whenot c)`; warning that
  `merge` is lazy while `if/then/else` is strict and needs all three arguments
  on one clock [verified: manual §§1.2.1-1.2.2].
- Local clocks may not escape their scope; V3's calculus is strictly less
  expressive than V1's, accepted for simplicity [verified: manual, clock-scope
  remark after §1.2].
- Pouzet's notes: Lustre's answer is to reject instantaneous unconditional
  cycles, expressible as a type system; Esterel and Signal allow conditional
  cycles with boolean reasoning, whose modular typed form is open; causality
  analysis is value-independent and separate from the clock calculus; the
  trivial algorithm drops edges to the right of a delay and rejects cycles
  [verified: causality.pdf slides 2-3, 9, 13-14, 25].

### ReactiveML (Mandel and Pouzet, PPDP 2005, extended)

ML plus Boussinot's synchronous reactive model.
- No instantaneous reaction to absence: `present x then () else emit x` is
  rejected by Esterel's causality analysis but valid here; absence takes
  effect next instant; no causality analysis needed [verified: §1, §2.1].
- Multi-emission: a signal is declared with `default` and `gather`; values
  emitted in an instant are combined; `await s(x) in e` delivers the combined
  value at the *next* instant, explicitly to avoid causality problems, so
  `await s(x) in emit s(x+1)` is causal [verified: §2.1, Fig. 1; §3: presence
  decided at end of instant].
- `await` ignores the current instant unless `immediate` [verified: §2.1].
- `pre s`/`pre ?s` [not verified: manual page not fetched].

### Céu (Sant'Anna et al., SenSys 2013; manual v0.30)

Structured imperative synchronous language; trails react to one external
event at a time.
- A reaction chain runs active trails until they await or terminate and
  always runs in bounded time; an event arriving mid-chain is queued
  [verified: paper §3.1].
- Every path through a loop body must contain `await` or `break`, else it is
  refused at compile time as a tight loop; Céu says this makes it unsuitable
  for algorithm-heavy code [verified: paper §3.1; manual "Bounded Execution"].
- Internal events are stack-based (`emit` suspends the emitter, awaiters run
  first); footnote 5 of §3.6: to keep reactions bounded, an `await` cannot be
  awoken within the reaction chain in which it was invoked [verified: paper
  §3.6 fn.5; manual "Internal Reactions"].
- Equal-valued emits: awakening is by occurrence; no equality test documented
  [not verified: neither source discusses it].

### Lingua Franca (official docs)

Deterministic reactors with superdense logical time.
- A tag is (logical time, microstep); simultaneous only if both match; every
  `schedule()` advances the tag by at least one microstep; the documented
  example reschedules its own zero-offset action five times, giving
  microsteps 0..4 at one logical time [verified: "Superdense Time"].
- Zero delay means one microstep later; `min_spacing` policies
  `defer`/`drop`/`replace` [verified: "Actions"].
- Causality loops: the tools refuse to generate code; break with `after 0`
  (one microstep) or by reordering reactions [verified: "Causality Loops"].
- Reactions of one reactor are mutually exclusive, ordered by tag then
  declaration; a later reaction overwrites an earlier same-tag output;
  `is_present` tests presence [verified: "Reactions"].
- Deadlines relate logical to physical time, are checked when the reaction is
  ready to run, replace it with a handler, and make the program
  nondeterministic [verified: "Deadlines"].
- TECS 2021 paper [not verified: ACM 403, NSF PAR refused, eScholarship empty].

### VHDL and Verilog (IEEE 1076 §12.6 mirror; IEEE 1364-2005; simulators)

- VHDL: an event occurs iff updating the signal changes its current value; a
  signal is active when a source is active even with an equal value;
  processes resume on an *event*; a cycle updates active signals then runs
  processes; if Tn = Tc the next cycle is a delta cycle; an error if a
  postponed process causes one [verified: LRM mirror §§12.6.2-12.6.4].
  Attributes `S'EVENT`, `S'ACTIVE`, `S'TRANSACTION` (flips each cycle S is
  active) [verified: UMBC attribute page].
- No LRM bound on consecutive delta cycles. GHDL `--stop-delta` default 5000
  [verified: GHDL docs]; xsim "Iteration limit 10000 is reached. Possible
  zero delay oscillation" [verified: embdev.net thread]; UG900 2025.2 lists
  `-maxdeltaid` default -1 [verified: docs.amd.com]; ModelSim vsim-3601 with
  IterationLimit 5000 [not verified: vendor and forum pages returned 403].
- Benveniste et al. call VHDL/Verilog/Statecharts delta-cycle microstep
  semantics awkward relative to synchronous circuits, a gap Esterel avoids
  [verified: 2003 survey, Esterel section].
- Verilog §9.7.2: an implicit event is detected on any change in the value of
  the expression; an operand change leaving the result unchanged is not an
  event. §9.9: `always areg = ~areg;` is a zero-delay infinite loop /
  simulation deadlock. §11.2-11.3: every value change is an update event;
  processes are sensitive to update events; five regions (active, inactive,
  NBA update, monitor, future); active-event order is free; no iteration
  limit anywhere in §§9.9, 11.3-11.4 [verified: LRM PDF copy].

## Comparison table

| System | "changed" means | Equal write | Legal back edge | Bound per instant | Sampling (copy) | Guard |
| --- | --- | --- | --- | --- | --- | --- |
| Esterel | emitted this instant | present; single signal: 2 emits = static error; combined: merged | `pre`, or constructive cycle | static fixpoint (constructiveness) | `?S` read = copy of sticky value | static (loops, causality) |
| Lustre/Scade | no writes; value per clock tick | n/a; test `x <> pre x` | `pre`/`fby` only (previous instant) | one pass, DAG depth | `when` (typed clock); `current`/`merge` hold | static |
| Lucid Synchrone | as Lustre | as Lustre | `pre`/`fby`, syntactic | as Lustre, modular types | `when`/`merge` | static |
| ReactiveML | emitted this instant | gathered | value/absence readable next instant | one instant, no analysis | `await s(x)` = copy next instant | static structure |
| Céu | event occurrence | awakens (no equality test documented) | internal await cannot re-awaken in the same chain | finite chain by rule | await = copy at wake | static (tight loops) |
| Lingua Franca | port present at tag | present; last reaction wins | logical action = next microstep | **none** at one logical time | reaction reads inputs at its tag | static loops + runtime deadline |
| VHDL | `'event` inequality; `'active` every write | active, no event | none | **none** in LRM | none | runtime limit (5000/10000) |
| Verilog | value change | not an event | none | **none** | none | none |
| Boon plan | every write (D32) | fires | copied read of HOLD/collection/builtin (§4.3) | claimed static (change_and_effects §3.3) | WHEN/THEN copy (D30) | none planned |

## Answers to the plan's four questions

1. **Every write vs inequality.** Esterel, ReactiveML, Céu and LF define
   change as an occurrence, value-blind; Lustre-family languages have no
   change at all; VHDL/Verilog define wake-up as inequality, and VHDL keeps a
   value-blind attribute beside it. Equal writes: present (Esterel/LF),
   merged (Esterel combined, ReactiveML gather), no event (VHDL/Verilog).
2. **Static bound.** "Cycles only through a committed state read" bounds the
   instant exactly when that read is *not itself a trigger* within the
   instant: Lustre `pre` (next instant), Céu (next chain), ReactiveML (next
   instant). LF's logical action and VHDL's zero-delay assignment are
   next-*microstep* triggers and give no bound. The plan's rule is
   Lustre-shaped only if the HOLD read on the cycle is a copy; a live read of
   a HOLD fires under D32, so §4.3 must make the distinction (implication 3).
3. **Copy vs live.** Sampling is a combinator in Lustre/Lucid (`when`, typed
   by a clock), Esterel (`?S` at emission, `pre(?S)`), ReactiveML
   (`await s(x)`), LF (inputs read at the reaction's tag). Documented
   pitfalls: `emit S(?S+1)` (Esterel), `current`'s nil prefix (dropped in
   Scade 6), strict `if` vs lazy `merge` clock mismatch (Lucid), and the
   delta-cycle glitch (VHDL/LF: intermediate microstep values are
   observable). Synchronous dataflow has no diamond glitch because each flow
   is computed once per instant.
4. **Guards.** Static: Esterel, Lustre, Lucid, Céu. Static + runtime: LF.
   Runtime only, tool-specific: VHDL (GHDL 5000, xsim 10000). None: Verilog.

## Implications for the plan

1. **D32 "every write fires" — supports, with a naming change.** Esterel, LF
   and Céu are value-blind, as D32 is. VHDL shows the alternative is not
   exotic: its default is inequality and it *names* both (`'event` vs
   `'active`). Proposed: keep D32; in P0 add the promised "only real
   changes" function to the catalog as a standard function over HOLD (the
   analogue of Lustre `x <> pre x`, Lucid `edge`), and have hover and the dev
   inspector show "fires" and "value changed" as two counters.
2. **D32 "at most once per step" vs R8 "follow-up microsteps" — contradiction
   until "step" is defined.** If step = microstep, a live derived value on a
   diamond (one input direct, one through a HOLD write) fires twice per tick,
   once with a transient value, and any THEN/command over it (D31 "runs
   exactly once each time the input updates") runs on the transient. That is
   the VHDL/LF delta model; Lustre/Scade avoid it by evaluating each flow once
   per instant with state visible next instant. Proposed change to §4.3:
   define a tick as one topological pass in which HOLD, collection and
   stateful-builtin commits become visible to live readers only at the next
   tick boundary (Lustre `pre`); let the runtime run settle ticks back to
   back before rendering and before staging commands (change_and_effects §3.3
   step 4 already stages commands after quiescence; make it normative).
   Spike in S5: count scenarios whose result differs between per-microstep
   and per-tick visibility.
3. **§4.3 "Fire edges never close a cycle" / D9 — suggests a wording change;
   otherwise the bound in change_and_effects §3.3 is unproven.** "A cycle must
   pass through a HOLD's committed-value read" is ambiguous: a *live* read of
   a HOLD is also a committed-value read (D21) yet fires on every write
   (D32). `count |> HOLD c {...}`, `doubled: count * 2`, and a copy body
   triggered elsewhere that writes `count` is fine; `doubled |> THEN {...}`
   writing `count` is the error. The invariant shared by every bounded
   system, and the one the plan needs: trigger edges, live-read edges and
   update edges form a DAG; only a *copied* read (inside a THEN body or WHEN
   arm) of HOLD, collection or stateful-builtin state may be a back edge.
   Then microsteps per tick ≤ state stages on the longest path of that DAG.
   Proposed: reword §4.3 that way, run Phase A's Tarjan pass on exactly that
   edge set, and add a negative-check scenario per edge kind.
4. **R8 microsteps — suggests a debug guard even with a static bound.** Every
   HDL simulator ships a delta limit because the language has none; Céu and
   Lustre need none because the rule is static. The plan is in the second
   camp only if implication 3 also covers collection updates and stateful
   builtins (D26 rewrites most builtins over HOLD, so their edges must be
   classified the same way). Proposed: keep "no runtime guard" for release,
   add a debug-build assertion `microsteps <= static_bound` computed by
   Phase A, and a spike that tries to build a legal-by-D9 program exceeding it.
5. **D31/D34 async loops — suggests a spike, outside the static rule.** A
   live query whose arguments depend on its own result (through a HOLD)
   restarts every time it answers. No synchronous language bounds this: LF
   admits such events only through physical actions and offers a deadline;
   Céu bounds a chain, not the sequence of chains. Proposed: a static warning
   "query argument depends on this call site's result" (computable on the
   §4.3 graph with effect-result nodes) and a dev-window effect-log detector
   (restarts per call site per second).
6. **§4.3 LATEST "two arms ... same trigger in the same step are an error" —
   supports.** Esterel's single-signal rule is the same static error
   (`-Icheck`); combined signals, ReactiveML `gather` and LF last-reaction-wins
   are the escape hatches. Proposed: keep the error; if an example needs
   merging, provide a library combinator, consistent with D32's library stance.
7. **D30 copy semantics and D14/D15 self-reference — supports.** Esterel's
   `?S` at emission copies a sticky value and `emit S(?S+1)` is the documented
   causality cycle whose fix is `pre(?S)`: D15 with the HOLD binder as `pre`.
   Lustre's `when` is sampling as a typed combinator; the plan's update kinds
   (never / from start / later) are a three-valued clock calculus. Proposed:
   name the WHEN/THEN self-reference diagnostic after Esterel's "causality
   cycle" with the HOLD fix-it, and state in §4.3 that update-kind inference
   is value-independent (Pouzet: causality analysis never depends on stream
   values), which D25 already requires.
8. **D32 "entering a WHILE arm ... not updates" — supports, with an
   `immediate` precedent.** Esterel `every S` and ReactiveML `await` ignore
   the starting instant unless `immediate` is written; the plan hard-codes the
   non-immediate behaviour. Proposed: P0's stale-copy hint should also cover
   a WHEN that must react to a value already present at arm entry, with the
   fix-it "use WHILE", and the census should list places that relied on
   today's live WHEN for exactly that.
9. **D21 snapshot reads — supports.** VHDL updates signals only between
   process executions, so every process in a cycle reads the same values;
   Lustre's `pre` reads the previous instant. LF is not a precedent: same-tag
   outputs propagate within the tag in dependency order. Proposed: cite the
   VHDL cycle (update all active signals, then run processes) as the model
   for R7's commit point, and keep D21 per tick, not per microstep, if
   implication 2 is adopted.
10. **P0 stale-copy hint — supports adding it.** Scade 6 removed `current`
    because its nil prefix before the first sample was a real hazard; Lucid
    warns that strict `if` and lazy `merge` differ. The plan's "No value yet"
    rule (§4.3) is the same hazard for a WHEN that copies a value with no
    value yet. Proposed: make the warning cover "copies a value that may have
    no value yet at the trigger", tied to the L3c display decision.
11. **Decidability — supports D25/D9's value-independence.** Lucid forbids
    local clocks from escaping and accepts a less expressive calculus; Pouzet
    notes that modular typing of conditional (constructive) cycles is open.
    The plan's unconditional rules and D16 (Cells without language cycles)
    are the Lustre choice. Proposed: state in §4.3 that dynamically acyclic
    but structurally cyclic programs (Esterel's bus arbiter, Halbwachs's
    if-C example) are rejected on purpose.
12. **Cells (D16, §5.1 "visited/depth guard") — supports keeping the guard in
    app code.** Static-only systems push unbounded algorithms out of the
    language; Céu says so explicitly. The runtime depth guard belongs to the
    Boon spreadsheet program; whether that program is expressible is the
    subject of `spreadsheet_recalculation_cycles.md`.

## Sources

1. Berry, The Esterel v5 Language Primer v5_91 — https://www.college-de-france.fr/media/gerard-berry/UPL8106359781114103786_Esterelv5_primer.pdf — fetched, extracted; §§3.3, 3.5, 4.4.2, 4.7, 5.2 read.
2. Jahier, Raymond, Halbwachs, The Lustre V6 Reference Manual — https://www-verimag.imag.fr/DIST-TOOLS/SYNCHRONE/lustre-v6/doc/lv6-ref-man.pdf — fetched, extracted; §§1.1.3, 1.2.1, Ex. 8-9, 3.2 read.
3. Halbwachs, Caspi, Raymond, Pilaud, The Synchronous Data Flow Programming Language LUSTRE, Proc. IEEE 1991 — text in scratchpad from an earlier session (`halb91.txt`); §II.B and the cyclic-definitions paragraph read; URL not re-fetched.
4. Pouzet, Lucid Synchrone 3.0 Tutorial and Reference Manual — https://www.di.ens.fr/~pouzet/lucid-synchrone/lucid-synchrone-3.0-manual.pdf — fetched, extracted; intro, §§1.1.9, 1.1.10, 1.2.1, 1.2.2, clock-scope remark read.
5. Pouzet, Causality analysis (MPRI notes) — https://www.di.ens.fr/~pouzet/cours/mpri/cours7/causality.pdf — fetched, extracted; slides 2-3, 9, 12-14, 25 read.
6. Colaço, Pagano, Pouzet, SCADE 6: A formal language for embedded critical software development, TASE 2017 — https://www.di.ens.fr/~pouzet/bib/tase17.pdf — text in scratchpad from an earlier session (`scade6.txt`); §§II, III, IV.C read.
7. Mandel, Pouzet, ReactiveML, a Reactive Extension to ML (extended), PPDP 2005 — https://www.lri.fr/~mandel/papers/MandelPouzet-PPDPextended-2005.pdf — fetched, extracted; §§1, 2.1, 3 read.
8. Sant'Anna et al., Safe System-level Concurrency on Resource-Constrained Nodes, SenSys 2013 — https://web.stanford.edu/class/cs240e/papers/ceu.pdf — fetched (`ceu_sensys13.txt`); §§3.1, 3.6, fn.5 read.
9. Céu v0.30 Reference Manual — https://ceu-lang.github.io/ceu/out/manual/v0.30/ — fetched; "Synchronous Execution Model", "Bounded Execution", "Internal Reactions" read.
10. Lingua Franca docs, Superdense Time — https://www.lf-lang.org/docs/0.6.0/writing-reactors/superdense-time/ — fetched.
11. Lingua Franca docs, Actions — https://www.lf-lang.org/docs/writing-reactors/actions/ — fetched.
12. Lingua Franca docs, Causality Loops — https://www.lf-lang.org/docs/0.6.0/writing-reactors/causality-loops/ — fetched.
13. Lingua Franca docs, Reactions — https://www.lf-lang.org/docs/writing-reactors/reactions/ — fetched.
14. Lingua Franca docs, Deadlines — https://www.lf-lang.org/docs/writing-reactors/deadlines/ — fetched.
15. Lohstroh, Menard, Bateni, Lee, Toward a Lingua Franca for Deterministic Concurrent Systems, TECS 2021 — https://dl.acm.org/doi/10.1145/3448128 — NOT fetched (ACM 403; NSF PAR refused; eScholarship returned no body).
16. IEEE 1076 (VHDL) LRM text mirror, §12.6 simulation cycle — https://rti.etf.bg.ac.rs/rti/ri5rvl/tutorial/TUTORIAL/IEEE/HTML/1076_12.HTM — fetched twice; §§12.6.1-12.6.4 read.
17. VHDL predefined attributes (UMBC) — https://portal.cs.umbc.edu/help/VHDL/attribute.html — fetched.
18. GHDL docs, Simulation and runtime — https://ghdl.github.io/ghdl/using/Simulation.html — fetched (`--stop-delta`).
19. AMD UG900 2025.2, xsim Executable Options — https://docs.amd.com/r/en-US/ug900-vivado-logic-simulation/xsim-Executable-Options — fetched (`-maxdeltaid`).
20. embdev.net thread quoting the xsim iteration-limit error — https://embdev.net/topic/389408 — fetched.
21. ModelSim vsim-3601 / IterationLimit 5000 — NOT verified (Intel community, Accellera forum, edaboard returned 403; altera.co.kr refused).
22. IEEE Std 1364-2005 (Verilog), PDF copy — https://www.csie.nuk.edu.tw/~stpan/course/Verilog1964-2005.pdf — fetched, extracted; §§9.7.2, 9.9, 11.2-11.4 read.
23. Benveniste, Caspi, Edwards, Halbwachs, Le Guernic, de Simone, The Synchronous Languages 12 Years Later, Proc. IEEE 2003 — text in scratchpad from an earlier session (`benveniste2003.txt`); Esterel and Signal sections read; URL not re-fetched.
