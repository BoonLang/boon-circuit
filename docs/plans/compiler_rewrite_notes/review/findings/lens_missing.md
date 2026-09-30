# Lens 5: what is missing: all findings

Generated from the verified finder output for `../REVIEW.md`. Severity is the severity after refutation; each finding keeps its refuter's corrections.

### MIS-001 [high] Distributed programs: the plan specifies checking only; what crosses the wire under D30-D32/D35 is undefined, and today's plan format still encodes the removed event/value split

*Plan: §4.4 Distributed programs; §4.5 delta table; P2b; P4; Sources: X1; verdict: confirmed*

The plan's only distributed text is §4.4 'Distributed programs', 'distributed link' in §4.5/P4 and 'Distributed roles' in P2b. All of it is about checking. Today the plan format and executor split cross-role traffic into Value, Event and Function exports and Current/Invocation calls. That split is chosen from the flow mode (TickPresent/PresentOrAbsent become events), and value exports are sent as the current value, which lets writes coalesce. D30 removes the event type and D32 says every write fires, so the split no longer has a basis. The plan leaves six things open: (1) whether a remote consumer sees every write or only the latest value; (2) what a tick is across two processes (D21 snapshots are per process); (3) whether cross-role calls, which X5 requires to be pure, are D31 queries with supersession that re-run on restore; (4) which store each role uses (D35), given that Session is a per-tab indexed template; (5) closed contracts for the server's `outputs` root, which today exists only as a lowering 'hidden v11 contract'; (6) FjordPulse Server's HOLDs that copy each request (request_path, request_method, request_count), which under D35 become persisted app memory. The R1-R9 delta table has no wire item. I also checked the plan's 'legacy fixed point' claim and it holds. Each role gets one typecheck in parse_role, the interface loop re-checks all three roles each pass and stops after a pass with no progress, and sealing checks each role once. That is 9 whole-program checks with no cross-role interface and 12 or more with any interface.

Evidence:
- [file, verified] `crates/boon_plan/src/lib.rs:448-600` — DistributedExportKind {Value, Event, Function}; DistributedCallMode {Current, Invocation}; separate Value/Event export/import plans
- [file, verified] `crates/boon_compiler/src/distributed_compiler.rs:436-462` — event identities selected by FlowMode::TickPresent | PresentOrAbsent
- [file, verified] `crates/boon_plan_executor/src/machine.rs:12790,13180-13200` — distributed_export_value_current; Current-call demands deduplicated by argument equality
- [file, verified] `crates/boon_compiler/src/distributed_compiler.rs:1008-1014,1245-1262,1395-1400,1440-1452` — per-role check in parse_role, fixed-point loop over all roles with break on !progress, seal per role: 9 minimum, 12 with interfaces
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:283-305` — Session is a per-tab indexed template; network frames carry sequence numbers and reject out-of-order frames
- [file, verified] `examples/fjordpulse/Server/RUN.bn:5-30,246-260` — http_request SOURCE copied into HOLDs; outputs.http_response is a live value derived from them
- [file, verified] `examples/fjordpulse/Session/RUN.bn:1-16` — connection HOLD copies connected/disconnected, a host-known value under D35
- [file, verified] `examples/manifest.toml:550-575` — state_namespace per role (client-v1/session-v1/server-v1)

**Proposed plan change.** Add R10 'Distributed wire (D30-D32, D35)' to the §4.5 delta table (Runtime + format, size M, about 1-2 weeks inside P4). Add a §4.4 paragraph: (1) The compiler derives each import edge's transport from how the consumer uses it. An import that reaches a fire position (THEN input, WHEN selector, HOLD piped input or update, LATEST arm) is sent on every write, in order, never coalesced. An import that is only read live is sent as its latest value and may coalesce. This replaces the Value/Event exports. (2) A cross-role call is a D31 query: live in live contexts, once per update in copy contexts, superseded per call site, and re-run on restore. Current/Invocation map to live/copy. (3) An imported update is applied in the consumer's next tick, never inside the producer's tick. (4) The store per role: Server durable, Session and Client by deployment (D35). (5) The server `outputs` root and host_ports get catalog contracts that the checker enforces. (6) Add FjordPulse's request and connection HOLD copies to the §5.1 D35 census row. Add to O2 one Client+Session scenario through the host-service runner.

**Refuter correction (high tier, high confidence).** Drop or soften sub-point (6). D35 and R9 already move connection lists to host ports. FjordPulse's request_count persisting across restarts is arguably correct app memory under D35, so it is a census note, not a defect. Option C in the owner question needs transitive analysis. An import read 'live only' that feeds a derived value, which later reaches a THEN input or HOLD update, is itself in a fire position (D32: a derived value fires whenever an input fires). Say C is 'every write whenever any transitive consumer is a fire position'. Otherwise C is not observably equal to A. Keep R10 sized as proposed, but tie it to S1 or a Client+Session O2 scenario so it is proven, not only specified.

**Owner question.** A producer writes an exported value twice in one tick (two R8 microsteps). Does a remote consumer see two updates or one?
- A: every write crosses the wire (D32 taken literally; per-keystroke chatter, even when the consumer only displays the value)
- B: coalesce per producer tick (at most one update per producer tick; differs from in-process behaviour)
- C: the compiler chooses per edge: every write when the consumer uses the value in a fire position, latest value otherwise (same observable result as A, cost of B where the difference cannot be seen)
Recommendation: C

---

### MIS-003 [high] Under D30 and D15, THEN/WHEN outputs, SOURCE payloads and LATEST hold implicit state. D26 and §4.5 deny it, and the hardware mapping has no category for it

*Decisions: D26, D12, D15, D30, D33, D9; Plan: D26, D12, D15, D30, D33; §4.5 Persistence 'Durable leaves'; RISC-V 'Combinational And Registered Logic'; Sources: X2; verdict: confirmed*

D30 says every value keeps its last value, and D15 makes `LATEST { Text/empty(), input.text, ... }` keep its text without HOLD. So THEN/WHEN copy outputs, SOURCE payloads and LATEST are all stateful. D12 even lists 'LATEST current values' and inferred last values as persisted. Several places contradict this. D26 says 'HOLD and collection updates are then the only state primitives, which is what the cycle (D9) and persistence (D12) rules are built on'. §4.5 lists durable leaves as only HOLD states, stateful-builtin state and collection rows (:569-575), so identity_v1 has no leaf kind for LATEST or last values. The RISC-V hardware contract maps only 'pure → combinational, HOLD/authority → registers' (:295-297). FPGA_TODOMVC_LOWERING.md:183 makes a SOURCE a 'one-cycle source pulse', but under D30 a payload must persist, so it needs a payload register, a fire strobe and a valid bit for 'no value yet'. Two same-step tie rules are also missing. D33 does not say what happens when a HOLD's piped input and its body both update in the same step, and in RTL that is a priority mux. LATEST is defined only for the same trigger, while hardware samples several SOURCEs at one edge. RISC-V:283-288 requires static exclusivity or real sequences for that case, and RUNTIME_MODEL:86 and LANGUAGE_SEMANTICS:407 offer PRIORITY/EXCLUSIVE, which the plan neither keeps nor drops.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:81,95,99,102` — D12 lists LATEST values and last values; D26 says HOLD and collections are 'the only state primitives'; D33
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:569-575` — §4.5 durable leaves omit LATEST and inferred last values
- [file, verified] `docs/plans/BOON_FIRST_RISCV_PROCESSOR_PLAN.md:283-297` — only two storage categories; multi-candidate rule
- [file, verified] `docs/architecture/FPGA_TODOMVC_LOWERING.md:177-187` — one-cycle source pulse
- [file, verified] `docs/architecture/RUNTIME_MODEL.md:86,188-200` — PRIORITY/EXCLUSIVE conflict step
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:374-377,405-407` — HOLD piped input is initialization only (contradicts D33); PRIORITY/EXCLUSIVE

```boon
last_key: key_down.key |> THEN { key_down.key }
-- D30: keeps its last value, so hardware needs a register plus a valid bit ('no value yet'); not a pure wire
mode: LATEST { Light, toggle.press |> THEN { Dark } }
-- D15: legal and stateful, persisted by D12, yet D26 says only HOLD and collections are state
```

**Proposed plan change.** Add a 'State classes' table to §4.3 (about 15 lines) and make persistence, Phase A, identity_v1, the Wasm emitter and the hardware contract all refer to it. Rows: HOLD; collection authority; stateful builtin; LATEST (stores its current value, or the index of the last-updated arm); copy output of THEN/WHEN over an updating input (stores its last value and a valid bit); SOURCE payload (last payload, fire strobe, valid bit); derived live value (no state). Reword D26's consequence as 'HOLD and collection updates are the only state the user declares; LATEST and copy outputs hold implicit state listed in §4.3'. Add the LATEST and copy-output leaf kinds to §4.5 durable leaves and to the identity_v1 golden vectors. Define the same-step tie rules for a HOLD's piped input against its body, and for LATEST arms fed by different triggers in one step. Then decide PRIORITY/EXCLUSIVE: keep, drop, or leave to a library.

**Refuter correction (high tier, high confidence).** Lead with the D12 versus §4.5 leaf-list inconsistency, which is certain and cheap to fix. The fix is to add LATEST-current and inferred-last-value leaf kinds to §4.5 and the identity_v1 golden vectors. Demote the hardware storage categories and the SOURCE payload register to a note for the hardware-backend reconcile. Present PRIORITY/EXCLUSIVE as 'retire explicitly in P0 (no example uses them)', not as an open design problem. Keep the HOLD reset-versus-body same-step tie as a P0 spec item. Recommendation (a), reset dominates, is reasonable. The LATEST different-triggers-in-one-step question only matters if one step can ingest more than one SOURCE occurrence, so the plan should first say whether a software step ingests one or several.

**Owner question.** When a HOLD's piped input (a reset, D33) and its body update in the same step, which wins? And may two LATEST arms driven by different SOURCEs update in the same step?
- (a) The piped reset wins, and LATEST arms from different triggers in one step are an error unless statically exclusive (the RISC-V rule).
- (b) The body wins, and LATEST arms use a per-step arrival sequence (RUNTIME_MODEL's greatest-sequence rule).
- (c) Both are compile errors unless the program is rewritten, with fix-its.
Recommendation: (a) for the HOLD reset, which matches 'reset dominates' (RISC-V:290). For LATEST, keep RUNTIME_MODEL's sequence rule for software and require static exclusivity on hardware profiles as a target-eligibility check. Retire PRIORITY/EXCLUSIVE in the spec explicitly.

---

### MIS-007 [high] No tooling for evolving a persisted app after cutover: DRAIN does not reach D12's new leaf kinds, and an unrelated edit can silently delete stored data

*Decisions: D12, D35, D36, D19; Plan: §4.5 Persistence ('persistence_only' predecessors, DRAIN recipe ids); D12; D36; §7 O2; Sources: X2; verdict: confirmed*

Today's evolution tooling is DRAIN/DRAINING in source (persistence plan:1106-1290; examples/migrations/todo/v2.bn marks the retiring list DRAINING and seeds the new one with DRAIN { todos }), numbered stages in sequence.toml, and persistence_only predecessor compiles. Activation deletes stored memory whose MemoryId is missing from the target schema (boon_persistence/src/migration.rs:235-248), and it refuses to activate when the target schema removes an effect that still has unfinished outbox work (:204-221). The sequential skip-version catalog is designed (persistence plan:1289-1312) but not implemented: apps/fjordpulse/migrations/catalog.toml has `migrations = []` and no MigrationCatalog type exists in crates/. The plan keeps DRAIN diagnostics and recipe ids and has four gaps. (1) DRAIN's path-only grammar targets HOLD and list authorities, but D12 now persists LATEST values and compiler-inferred last values, and nothing says how to rename or retype those. (2) The inferred set depends on reads elsewhere ('reads outside its own update'), so deleting a view read can remove a durable leaf, and the next activation deletes its data with no DRAIN and no warning outside the dev window. (3) Predecessor stage sources must keep compiling under every future strictness change. (4) A catalog change such as D36 blocks migration when outbox items for Http/request are unfinished. The plan has no rename, retype or split story and no schema-diff report for developers.

Evidence:
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1106-1150,1265-1312` — DRAIN grammar, implicit deletion, skip-version catalog design
- [file, verified] `examples/migrations/todo/sequence.toml; examples/migrations/todo/v2.bn:23-38` — stage mechanics and DRAIN usage
- [file, verified] `crates/boon_persistence/src/migration.rs:198-248` — IncompatibleOutbox; deleted_memory computed from missing MemoryIds
- [file, verified] `apps/fjordpulse/migrations/catalog.toml` — current_schema = 1, migrations = []
- [measurement, verified] `rg -rln 'MigrationCatalog|migration_catalog' crates` — no hits: the catalog is unimplemented
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:81,586-589` — D12 inferred set; persistence_only predecessors

**Proposed plan change.** Add a §4.5 'Schema evolution' paragraph (about 12 lines) and a P4 work item. (1) Phase A emits a durable-schema diff against the predecessor stage, in the diagnostics lane: added, removed and retyped leaves, each with its source path. A removed leaf is a positioned warning in dev ('stored data for `x` will be deleted: add DRAIN or keep a reader'). It is an error in `boon-app build` unless the package manifest acknowledges it. (2) Extend the DRAIN grammar in the P0 spec to every D12 leaf kind (LATEST, inferred last values) and define DRAIN on a computed value. (3) Choose how predecessors are represented (owner question). (4) The catalog-change rule: a removed or renamed effect maps through an alias table, so pending outbox items migrate instead of blocking. (5) O2 adds rename, retype, split and delete scenarios for each new leaf kind.

**Refuter correction (high tier, medium confidence).** Fix the evidence. The plan-level migration catalog (recipes and edges merged from predecessor compiles) exists today. Only the frozen catalog.toml fragments are unimplemented. Option (b) therefore partly exists in the plan format, and the real question is whether to keep compiling predecessor sources or to freeze their lowered recipes. Add the D12 versus D35 conflict explicitly: a read-dependent durable set is a reachability rule in all but name. One option is to make every last value of a THEN/WHEN output or SOURCE-derived copy durable when touched, regardless of readers. That keeps D35's principle, costs some storage and removes the silent-deletion path. Keep the schema-diff warning and O2 rename, retype and delete scenarios.

**Owner question.** After cutover, how is a released app's previous schema represented for migration?
- (a) Predecessor sources, compiled in persistence_only mode forever. Every later language change must keep old stages compiling.
- (b) A frozen, lowered schema-and-recipe fragment written at release (the persistence plan's catalog); only the current source is compiled.
- (c) Both: sources for dev stages and hot reload, frozen fragments inside app packages.
Recommendation: (c). Strictness changes such as D6, D23 and D24 would otherwise break old stages, and the fragments already have a designed home (apps/*/migrations/catalog.toml).

---

### MIS-002 [medium] R8 microsteps conflict with the hardware, console and RUNTIME_MODEL commit contracts, and Stream/pulses makes the microstep count depend on runtime values

*Decisions: D21, D32, D30, D9; Plan: §4.5 R8; §4.3 Change rules; P0 'Reconcile the active contracts'; §4.5 'target-profile limits keyed on Config.target'; Sources: X2; verdict: confirmed; severity high -> medium after refutation*

R8 adds 'follow-up microsteps within a tick, each reading its own committed snapshot', so state commits happen in the middle of a tick. Three active contracts forbid that. RUNTIME_MODEL.md:91 says 'No stateful value should commit in the middle of evaluation', and D21 cites RUNTIME_MODEL as the authority. BOON_FIRST_RISCV_PROCESSOR_PLAN.md:257-268 says one hardware activation 'commits all accepted register writes together at the active clock edge' with 'no observable mid-cycle state commit'. BOON_CONSOLE.md:302-307 says one atomic turn yields one candidate transaction that commits nothing on failure. In RTL terms, a THEN over a HOLD's fire in the same tick reads the register's next-state wire (its D input), not its Q output. A chain of such THENs becomes one long combinational path, so the pipeline-per-HOLD structure hardware designers expect cannot be written. The notes claim that 'depth is at most the longest path in the change-edge graph ... so no runtime guard is needed' (change_and_effects.md:180). Stream/pulses makes that false: it emits a runtime-valued number of pulses inside one activation (examples/fibonacci.bn:61-63, a manifest entry). Today that is capped by a per-target ceiling, TargetProfile::max_pulses_per_activation (boon_plan/src/lib.rs:93-104: 1,000,000 / 65,536 / 65,535 for FPGA), enforced at boon_plan_executor/src/machine.rs:18084. The plan never mentions Stream/pulses, the ceiling, or how microsteps map to clock edges or Wasm turns. RUNTIME_MODEL.md is also missing from the P0 reconcile list.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:613` — R8: 'follow-up microsteps within a tick, each reading its own committed snapshot (D21)'
- [file, verified] `docs/architecture/RUNTIME_MODEL.md:76-93` — Tick phases commit once, at step 7; 'No stateful value should commit in the middle of evaluation'
- [file, verified] `docs/plans/BOON_FIRST_RISCV_PROCESSOR_PLAN.md:257-268` — one activation, one commit at the edge; 'no observable mid-cycle state commit'
- [file, verified] `docs/architecture/BOON_CONSOLE.md:300-307,335` — one event per atomic turn; a trap or capacity failure commits nothing
- [file, verified] `crates/boon_plan/src/lib.rs:93-104` — target-owned pulse microturn ceiling; its comment says static verification cannot always prove the bound
- [file, verified] `crates/boon_plan_executor/src/machine.rs:18084-18090` — executor enforces the ceiling
- [file, verified] `examples/fibonacci.bn:52-72` — `n - 1 |> Stream/pulses() |> THEN {...}` inside a HOLD; microstep count = n-1 at runtime
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:177-181` — claims a static bound and no runtime guard
- (2 more evidence entries in the finder output)

```boon
count: 0 |> HOLD count { press |> THEN { count + 1 } }
parity: count |> THEN { count % 2 }
-- R8: parity updates in the same tick (microstep 1). In RTL that is parity_D = f(count_D), a combinational chain, not parity_D = f(count_Q).
-- RUNTIME_MODEL.md:91 and RISC-V:258: parity sees the previously committed count.
-- A hardware developer expects one register stage per HOLD.
```

```boon
[previous: 0, current: 1] |> HOLD state { n - 1 |> Stream/pulses() |> THEN { [previous: state.current, current: state.previous + state.current] } }
-- n-1 microsteps in one activation, depending on a runtime value: no static bound exists
```

**Proposed plan change.** 1) §4.3: add a 'Tick on every target' paragraph (about 12 lines) that says whether microstep commits are visible to later microsteps in the same tick. It must also say how that is realized on a clock-edge target (combinational next-state chaining within one edge, or one edge per microstep with input backpressure) and on a Wasm turn (microstep commits stay private to the turn and become one kernel transaction). 2) §4.5 Phase A: compute a static microstep bound per program from the fire-edge DAG and write it into the plan header. Stream/pulses and other runtime-valued pulse sources are excluded from that bound and stay under the target's runtime ceiling. 3) §4.5 R8: keep TargetProfile::max_pulses_per_activation, or its successor, as a target-owned runtime guard, and add a debug assertion that microsteps stay within the static bound. 4) P0 reconcile: add RUNTIME_MODEL.md (tick phases, conflict step 5) and the RISC-V plan's 'Boon Hardware Contract' (lines 250-300), not only its hashing prerequisite. 5) S5: extend it to list the scenarios whose results differ between per-microstep and per-tick visibility.

**Refuter correction (high tier, medium confidence).** Reword the finding. The plan mentions pulse membership (plan:457) but not the target-owned runtime ceiling. The static-bound claim is in the notes, not the plan. Keep proposed changes 3 (retain max_pulses_per_activation as a runtime guard in R8) and 4 (add RUNTIME_MODEL.md tick phases and the RISC-V hardware contract to the P0 reconcile list). They are cheap and clearly needed. Reframe the owner question: in software, (a) and (b) with settle-before-render are equivalent. The choice only matters for hardware, whether a chain is combinational within one edge or takes one cycle per stage. It can be deferred to the hardware-backend plan if the plan says so explicitly. Drop the implication that R8 breaks the console turn: say instead that microstep commits must stay private to the turn.

**Owner question.** On targets that commit once per clock edge (Boon RTL) or once per turn (console app.wasm), when does a HOLD write become visible to a THEN over that HOLD?
- (a) R8 as written, same tick: microstep commits are visible within the tick. Hardware realizes microstep chains as combinational next-state chaining within one edge, with a profile cap on chain depth. The console keeps microstep commits private to the turn. RISC-V:268 is reworded to 'no externally observable mid-cycle commit'.
- (b) Next tick, Lustre-shaped: commits become visible at the next tick. Software runs settle ticks back to back before rendering and before staging commands. Hardware maps one tick to one edge, and each THEN-over-HOLD stage costs one cycle.
- (c) Per target, (a) in software and (b) in hardware. Rejected: the same source would mean different things, which breaks the console's PC-reference == SoC equivalence (BOON_CONSOLE.md:42-45).
Recommendation: (b). It is what RUNTIME_MODEL says (and D21 cites RUNTIME_MODEL), it matches the RISC-V and console contracts without special rules, and S5 can measure its software impact. In either case, keep the target-owned runtime ceiling for Stream/pulses.

---

### MIS-006 [medium] D35 makes FPGA deployments in-memory, but three active documents give the console and FPGA targets durable state and migrations

*Decisions: D35, D12, D3; Plan: D35; P0 'Reconcile the active contracts'; §4.5 Persistence; Sources: X2; verdict: confirmed; severity high -> medium after refutation*

D35 says the deployment picks the store and gives 'in-memory for one-shot CLIs, tests and FPGA (power-on resets every HOLD to its starting value)'. Three active documents say otherwise. BOON_CONSOLE.md's reset matrix keeps app state across power loss ('last complete journal commit only', :541), it lists an 'optional persistent app/state journal' (:414), 'persisted state size' is a mandatory board-profile field (:357), and readiness item 9 requires persistent install, corruption and rollback behaviour. BOON_CONSOLE_IMPLEMENTATION_PLAN.md Phase 6A adds a 'wear-aware state journal'. BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1293 says 'FPGA bitstreams can also skip versions', and :1314-1317 says 'For FPGA targets, migration normally runs in deployment/updater tooling ... before the new design owns persistent memory'. The console also requires 'exact app identity or a verified migration' to restore state (BOON_CONSOLE.md:552). D12/identity_v1 instead key state on structural routes that survive unrelated edits, so a recompiled app.wasm with the same schema would lose its state under the console rule but keep it under D12. The P0 reconcile list covers the console only for eligibility, ConsolePort, backends and lineage, not for persistence.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:104` — D35 text including 'FPGA (power-on resets every HOLD...)'
- [file, verified] `docs/architecture/BOON_CONSOLE.md:357,414,535-556` — persisted state size, journal, reset matrix, restore needs exact app identity
- [file, verified] `docs/plans/BOON_CONSOLE_IMPLEMENTATION_PLAN.md:303-308,588-600` — Phase 6 SPI flash state journal
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1293,1314-1317` — FPGA persistent memory and migration tooling
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:874-880` — the reconcile list omits persistence for the console and FPGA

**Proposed plan change.** Add a P0 reconcile bullet (about 4 lines): 'BOON_CONSOLE.md reset/persistence matrix and BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:1290-1317: state which store each hardware deployment uses under D35. Console state restore uses identity_v1 schema compatibility (semantic_schema_hash plus DRAIN edges), not exact app.wasm identity.' In §4.5 Persistence, add one sentence: 'The persistence plan (durable leaves, identity_v1) is emitted for every target; an in-memory deployment simply never opens it.' That keeps the MachinePlan the same across stores, which the console's lineage needs.

**Refuter correction (high tier, high confidence).** Lower severity to medium. Frame it as a one-line P0 reconcile bullet: 'FPGA in D35 means Boon-generated RTL registers; a console board profile with a journal is a deployment with a store'. Also add 'console state restore keys on identity_v1 schema compatibility, not exact app.wasm identity'. The latter is the part most worth an owner confirmation. The proposed §4.5 sentence, that the persistence plan is emitted for every target, is good and cheap.

**Owner question.** D35 lists 'FPGA' among the in-memory deployments. Does that cover console app.wasm deployments, whose kernel has an SPI flash journal, or only Boon-generated RTL state?
- (a) Only Boon-generated RTL state: RTL registers reset at power-on. The console is a deployment with a store once its Phase 6 journal exists, and in-memory before that.
- (b) Every FPGA and console deployment is in-memory. Delete console Phase 6 state persistence and the persistence plan's FPGA paragraphs.
- (c) As D35's principle says, the deployment (board profile) picks the store for any target, RTL included. 'FPGA' in D35 is only the default.
Recommendation: (a), stated through (c)'s principle. It keeps D35's rule that the deployment, never the source, picks the store, and it keeps the console readiness gate. Also replace the console's 'exact app identity' restore rule with identity_v1 schema compatibility.

---

### MIS-009 [medium] Durable by default has no retention, deletion or encryption story for multi-user Server and Session roles

*Decisions: D35, D12; Plan: D35 ('durable for desktop/web apps and servers'); D12; Distributed roles; Sources: X2; verdict: confirmed*

D35 makes all app memory durable on servers. FjordPulse deploys Client, Session and Server roles on one volume. The persistence plan treats user, tenant and workspace scope as host storage namespaces (:267, :451) but defines no namespace lifecycle. Nothing says when a disconnected user's Session memory is deleted, how a user's data is erased on request, whether the store is encrypted at rest, or who can export it. The RUNBOOK says the volume is 'not a backup' and that backup is deferred (RUNBOOK.md:68-70). D35 also removes the 'view state' escape: element-local drafts and choices are now durable. That widens what is stored per user, and nothing adds retention controls to match.

Evidence:
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:262-272,451`
- [file, verified] `deploy/fjordpulse/RUNBOOK.md:35,68-70`
- [file, verified] `examples/fjordpulse/Session/RUN.bn:1-15` — a Session-role HOLD (connection) that D35 would make host-provided
- [measurement, verified] `rg -n -i 'encrypt|retention|erase' docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md docs/plans/BOON_COMPILER_REWRITE_PLAN.md` — no hits for encryption or retention

**Proposed plan change.** Add a D35 note or §4.5 paragraph (about 6 lines): 'Durable memory is stored per host namespace (app, role, user or tenant). The host API provides namespace deletion and export, and a retention policy for Session-role namespaces (default: delete N days after last connection). Encryption at rest is a deployment store option, off in dev and on by default for server deployments.' Add one O2 scenario that deletes a namespace and restarts.

**Refuter correction (low tier, medium confidence).** Note this is more an operations/deployment-layer gap than a compiler-architecture defect (the compiler plan correctly delegates the storage choice to the deployment per D35's 'if the host knows it, ask the host' principle); the actionable fix belongs mostly in BOON_PERSISTENCE_ARCHITECTURE_PLAN.md/RUNBOOK.md rather than blocking the compiler rewrite's decisions. Keep the proposed one-paragraph addition and O2 scenario as the right level of fix.

**Owner question.** What happens to a Session-role namespace after its user disconnects for good?
- (a) It is kept until an explicit host deletion.
- (b) A deployment-configured retention period, plus explicit deletion and export.
- (c) Session-role memory is in-memory by deployment default, and only Server-role memory is durable.
Recommendation: (b). It follows D35's rule that the deployment, never the source, picks the store, and it gives a GDPR-style deletion path without new syntax.

---

### MIS-011 [medium] Nothing in the plan covers privacy: under D13, D30, D31, D35 and R9, typed passwords and secrets can become durable, logged and re-verified on every keystroke

*Decisions: D35, D12, D13, D30, D31, D32; Plan: D35, D12, D13, D31, L2 default (Log/*), R4, R9 effect log; §4.3 Contracts; Sources: X2; verdict: confirmed; severity high -> medium after refutation*

Today there is a working sensitive-input mechanism. A `sensitive` style key on text inputs (boon_document_model/src/lib.rs:12-14, 876-930) redacts text, style and caret. The web host has a sensitive_input module (boon_web_host/src/lib.rs:21, document_runtime.rs:589-630), and the playground has restart_sensitive_inputs (preview.rs:3661-3694). The persistence plan's contract (BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:348-368) keeps password drafts in a host-owned buffer that 'is not copied into ordinary application memory' and redacts them from inspector snapshots, traces, logs, verifier reports, scenario artifacts and the persistence encoding. The rewrite plan never mentions any of this; 'sensitive', 'secret', 'privacy' and 'redact' appear nowhere in it. Several decisions now work against it. D13 makes elements data that code reads and spreads. D30 makes every value keep its last value. D12 persists last values read outside their update, and D35 says all app memory persists. D31 classes Secret/verify as a query, so in a live context it restarts whenever its arguments update, which is every keystroke under D32. L2 lets Log/* log every update of its argument, and R9 adds an effect log that records arguments. The 'restored' occurrence only clears a value after the restart; the value is already durable by then. No example uses `sensitive`, so neither the census nor O2 would notice if R4 dropped the feature.

Evidence:
- [file, verified] `crates/boon_document_model/src/lib.rs:12-14,876-930` — SENSITIVE_INPUT_STYLE_KEY and redaction of text, style and caret
- [file, verified] `crates/boon_web_host/src/document_runtime.rs:172,589-630` — web host sensitive-input routing
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:348-368` — host-owned buffer and redaction contract
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100,104,614,1203` — Secret/verify is a query; all app memory persists; R9 effect log; Log/* logs every update
- [measurement, verified] `rg -n 'sensitive' examples --include=*.bn` — no example uses the sensitive key (the only hit is prose in kavik_cz)
- [measurement, verified] `rg -n -i 'sensitive|secret|privacy|redact' docs/plans/BOON_COMPILER_REWRITE_PLAN.md` — only the D31 catalog list names Secret/verify

```boon
password: LATEST { Text/empty(), password_input.text, submit |> THEN { Text/empty() } }
-- D15: a legal LATEST without HOLD. D12: LATEST current values are durable, so the typed password lands in the store.
ok: Secret/verify(secret: stored, candidate: password)
-- D31: a query in a live context restarts on every argument update, so it verifies on every keystroke (D32).
-- A developer expects neither.
```

**Proposed plan change.** Add a §4.3 'Sensitive values' paragraph (about 10 lines) and an R10 row (about 3 lines). (1) The catalog marks host ports and payloads as sensitive, such as a sensitive text_input's text and Secret/* inputs. This is catalog data, not syntax. (2) Checker rule: a value derived from a sensitive payload may reach only the catalog sinks marked as accepting sensitive values (Secret/verify, authentication commands). It may never reach a HOLD, a LATEST, a collection, outputs, wire edges, Log/* or the arguments of a non-sensitive command. It is never a durable leaf. Violations are positioned errors. (3) The effect log, the inspector, scenario artifacts and native reports redact sensitive arguments. (4) Secret/verify in a live context gets a warning or error ('re-verifies on every update; move into THEN'). (5) R4 keeps the host-owned sensitive buffer: element data carries a host reference, never plaintext. Add an O1 negative fixture for each rule and one O2 scenario with a sensitive input across a restart.

**Refuter correction (high tier, high confidence).** Lower severity to medium. Prefer the smallest change. (1) R4 states that sensitive inputs keep today's host-owned buffer, so element data carries a host reference, never plaintext (option b). (2) P0 lists BOON_PERSISTENCE_ARCHITECTURE_PLAN.md 'Sensitive Input And Credentials' as retained. (3) One O1 fixture and one O2 scenario use a sensitive input across a restart and check the store and effect log for plaintext. The checker taint rule (option a) is a larger design, and the persistence plan itself defers a 'future general sensitivity/taint system' (line 367-368). Keep it optional rather than recommended. The Secret/verify per-keystroke point is ordinary D31 behaviour for any query in a live context. Make it a hint or lint, not part of the privacy finding.

**Owner question.** How should sensitive inputs work under 'all app memory persists'?
- (a) A catalog-level sensitivity class plus a checker taint rule: sensitive values flow only into allowlisted sinks, are never durable, logged or inspected. No syntax change.
- (b) Only today's host-owned buffer: plaintext never becomes a Boon value, and Boon sees an opaque host reference. R4 must keep the text out of element data.
- (c) Nothing: document that everything typed, passwords included, is durable app memory.
Recommendation: (b) as the baseline, since the persistence plan already designed it, plus (a) for any sensitive SOURCE payload that does reach Boon code. Neither needs new syntax, and D35 stays intact for ordinary memory.

---

### MIS-012 [medium] Error recovery is not normative: the prototype runs an unclosed bracket to EOF, unclosed TEXT raw mode and the multi-line TEXT indentation rule are undefined, and today reports only one (often misleading) error

*Plan: §4.2 Layout L4 and Recovery; P1a exit; Sources: X1; verdict: confirmed; severity high -> medium after refutation*

§4.2 'Recovery' has two sentences ('every unit reports all errors; pure function of the text'). The strategy exists only in the notes. frontend_session.md §1.4 describes element-level skip plus an implicit close guided by indentation, which contradicts the plan's L4 'indentation never decides structure'. The prototype behind the 2.6 ms parse number does not implement that implicit close: on an unclosed bracket it errors at EOF, so everything after the opener is swallowed. The typing states an editor sees most often (an unclosed `[`, `(` or `TEXT {`) therefore lose every later definition, and references to them become unknown-name floods. Recovering from an unclosed `TEXT {` needs a rule for multi-line TEXT content. Today column-0 content inside TEXT is accepted, and a rule that always resyncs at column 0 would misparse it. The notes' acceptance test 'typing each example character by character never changes the syntax of definitions outside the edited one' (FE3) was dropped from the plan. Today's behaviour, which the plan should state it improves on, is below.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/fe_proto.rs:220-241` — recover() skips to a depth-0 separator; elements(): `if k == T::Eof { self.err("unclosed bracket"); break; }`. There is no implicit close guided by indentation
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:186,513` — claims implicit close guided by indentation; FE3 acceptance: typing char by char never changes other definitions
- [measurement, verified] `target/release/boon_cli check scratchpad/X1/unclosed_text.bn` — `b: TEXT { unclosed` on line 3 reports only 'unbalanced `]` at line 4, column 1' (wrong location, one error)
- [measurement, verified] `target/release/boon_cli check scratchpad/X1/three_errors.bn` — three independent errors (dangling `+`, `+*`, unknown `othr`); only the line-6 error is reported
- [measurement, verified] `target/release/boon_cli check scratchpad/X1/text_col0.bn` — multi-line TEXT with content `hello:` at column 0 is accepted today
- [measurement, verified] `python scan of multi-line TEXT blocks in git ls-files '*.bn'` — only 5 multi-line TEXT blocks in the corpus; all content lines are at column 8 or deeper (BUILD.bn content sits at its closing-brace column)
- [source, verified] `~/repos/boon/docs/language/TEXT_SYNTAX.md:167-196` — original Boon: multi-line content indented to at least the closing `}` column + 4

**Proposed plan change.** Replace §4.2 'Recovery' with normative text (about 25 lines). Synchronization points: (a) A line at column 0 that starts with `name:` or `FUNCTION` always closes every open construct, including TEXT raw mode. It produces one diagnostic 'unclosed X' anchored at the opener, not at the resync point. (b) Inside brackets, skip to the next separator at the same depth. (c) An implicit close guided by indentation is used for recovery only (say so in L4). (d) A missing operand or call entry becomes an Error node and parsing continues. Add to the P1a exit: the FE3 char-by-char typing test over all examples, a probe corpus with N independent errors that must yield exactly N diagnostics, and an unclosed-opener probe for each bracket kind and for TEXT.

**Refuter correction (high tier, high confidence).** Lower severity to medium. Say explicitly that fenced reparse (frontend_session.md:188) is the notes' existing mitigation for warm edits. It is missing from the plan and should be carried into §4.2 and the P1a/P1b exit together with the FE3 typing test. The column-0 TEXT question (option A) is a sound owner question tied to D18, and the corpus evidence supports A. The test 'N independent errors yield N diagnostics' is good. Phrase it as 'at least the N expected positioned diagnostics, with no cascades outside the affected definitions'.

**Owner question.** Multi-line TEXT content indentation (needed so that column-0 resync is sound; D18 says layout must not break multi-line TEXT)
- A: no line inside a multi-line TEXT may start at column 0 (all 5 current blocks comply)
- B: original Boon rule: content indented at least the closing `}` column + 4 (breaks BUILD.bn lines 28-30 in both apps)
- C: no rule; recovery for unclosed TEXT stays heuristic and is not guaranteed pure/local
Recommendation: A

---

### MIS-013 [medium] Fix-its are on the migration critical path, but there is no diagnostic data model (code, related spans, fix-it edits), no applier, and the catalog seed is pre-D30

*Plan: §4.3 Diagnostics; §4.8; P3a; P3b exit; §7 O1; Sources: X1; verdict: confirmed; severity high -> medium after refutation*

D11, D24, the one-input LATEST row, the LATEST→HOLD row, the 'use WHILE' hint and parenthesization all promise fix-its, and P3a is 'scanner- and fix-it-driven'. §4.8 exposes only `Checked::diagnostics()` with no shape. Today every layer carries only (severity, location, message): no code, no related locations, no edits. Many families need secondary spans: a cycle path, O2 multiple producers, LATEST same-trigger arms, D23 exactness (call-site field versus callee body), and conflicts between a consumer and a producer. No phase budgets an applier: no `boon_cli check --fix`, and no quick fix in the dev window (today it shows diagnostics as one text blob). The plan names spec.md §15 as the catalog's starting point, but its F1-F9 family is written in continuous/event terms that D30 removed. persons_pro turns child-compile diagnostics into Boon data (a `rejected` SOURCE payload with one `diagnostic`, `source_path`, `line` and `column`, all TEXT). A multi-error model with codes therefore changes a catalog contract that Boon code reads. Warnings (D25 unreachable arms, the stale-copy warning) cannot be suppressed, because syntax is frozen, yet the P3b exit requires '0 diagnostics'.

Evidence:
- [file, verified] `crates/boon_editor/src/language.rs:69-73` — SemanticDiagnostic { severity, location, message }
- [file, verified] `crates/boon_compiler/src/lib.rs:189-196` — CompilerDiagnostic { path, line, column, start, end, message }
- [file, verified] `crates/boon_checked/src/lib.rs:754-760` — TypeDiagnostic without code or related spans
- [file, verified] `crates/boon_native_playground/src/ui.rs:788-791` — diagnostics_text() shown as one inspector field
- [file, verified] `crates/boon_host_runtime/src/persistent.rs:3994-4008` — rejected payload: revision/source_path/line/column/diagnostic as TEXT, a single diagnostic
- [file, verified] `examples/persons_pro/RUN.bn:182-205` — Boon code reads rejected.diagnostic/.line/.column
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:719-800` — catalog F1-F9 phrased as continuous/event (e.g. 'F1 THEN on a continuous input')

**Proposed plan change.** §4.3 Diagnostics: define `Diagnostic { code, severity, primary: Span, related: [(Span, label)], message: lazy, fixits: [TextEdit{span, replacement}], meaning_preserving: bool }`, and add it to §4.8. Add a P1b/P2a work item (about 1-1.5 k lines): `boon_cli check --fix` (applies only meaning-preserving fix-its and re-checks), and dev-window quick fix plus a diagnostics list with jump-to. O1: every fix-it fixture is applied, re-checked (the code disappears) and, where a scenario exists, re-run. P0 catalog: rewrite the F family in D30 terms. Severity policy: a warning is emitted only with a meaning-preserving fix-it, or it counts as an error. P3b exit: 0 errors and 0 warnings. Catalog: decide the child-program `rejected` payload (keep the first diagnostic plus a count, or a LIST of [code, path, line, column, message]).

**Refuter correction (high tier, medium confidence).** Lower severity to medium. Drop the 'catalog seed is pre-D30' sub-point, or note that P0 (plan:465, 867-871) already rewrites it. Narrow the proposal to three things. (1) Define Diagnostic {code, severity, primary, related[], fixits[]} in §4.3/§4.8. (2) State P3b's exit as '0 errors; warnings only where the census lists them', or require every warning to carry a meaning-preserving fix-it. (3) Make `boon_cli check --fix` the single applier that P3a scripts use, so the fix-its are exercised, rather than a separate dev-window feature on the critical path. The persons_pro payload question is low priority: option A (keep today's fields, add code and count) needs no decision before P5.

**Owner question.** Shape of child-program diagnostics visible to Boon code (persons_pro `rejected` payload) once compiles report all errors with codes
- A: keep today's single-diagnostic fields (first error by position) and add `code` and `count`
- B: replace with `diagnostics: LIST { [code, source_path, line, column, message] }`
- C: both: A's fields plus the LIST
Recommendation: A (smallest catalog change, no persons_pro rewrite; the LIST can come later)

---

### MIS-014 [medium] Cascading-diagnostic suppression is specified only for 'dependents of a broken definition'; the strict rules (D23, D24, D30 'never runs', exhaustiveness, Phase A) and definitions swallowed by recovery will each cascade

*Plan: §4.3 Diagnostics; §4.4 Diagnostics; §4.7 Errors; Sources: X1; verdict: confirmed*

§4.7 says only that dependents of a broken definition see an absorbing error type. The new strict rules produce new cascades that type absorption does not cover. (a) D24: a parameter used only inside a broken sub-expression looks unused. (b) D23: a function whose body is partly broken consumes fewer fields, so correct call sites get 'extra field' errors. (c) D30: THEN/WHEN over an Error-typed input would report 'never runs' because its update kind is unknown. (d) An exhaustiveness check over an Error selector. (e) Phase A cycle/OUT checks over instances of broken functions. (f) Definitions that recovery swallowed produce unknown-name errors at every reference. (g) One scheme-predicate violation inside a theme-like function called from 94 sites yields 94 diagnostics. §4.4 orders diagnostics by position and name bytes but has no deduplication or grouping rule.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:662-664` — 'Dependents of a broken definition see a deterministic absorbing error type'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:502-506` — conflict pairs, explain mode, ordering; no dedup or grouping
- [reasoning, verified] `D23/D24/D30 rules in §1 and §4.3` — each rule reads a property (consumption, use, update kind) that a broken sub-expression makes unknowable

**Proposed plan change.** Add to §4.3 Diagnostics (about 15 lines): an Error node or type carries 'poisoned' for every derived property: type, update kind, consumed-field set, use of binders, OUT production. Rules that read a poisoned property stay silent. A name that is unresolved only because recovery swallowed a region stays silent. One predicate violation reached through N call sites is one diagnostic at the definition, with a related span for the first call site in position order and the note '(and N-1 more)'. Add O1 'one root cause → exactly one diagnostic' fixtures, one per strict rule.

**Refuter correction (low tier, medium confidence).** None; the proposed §4.3 addition (poisoned derived properties + one-diagnostic-per-root-cause with '(and N-1 more)') is a reasonable, appropriately scoped fix.

---

### MIS-015 [medium] 'format ports format_source_unit' would port a line-based formatter that rewrites the contents of multi-line TEXT

*Decisions: D18; Plan: §4.2 'Tooling' (:334); D18 ('must not break multi-line TEXT'); P1b; Sources: X2; verdict: confirmed*

Today's formatter (boon_parser/src/lib.rs:4155-4370) never looks at tokens. On every line, including multi-line TEXT content, it rounds indentation up to a multiple of 4, trims trailing whitespace, collapses runs of blank lines, and compacts a bracket block with one child onto a single line. A faithful Python replica applied to examples/todo_mvc_physical/BUILD.bn rewrites the TEXT code template's `icon: [ / {code} / ]` into `icon: [{code}]`. By the original visual-indentation rule, a `{code}` alone on its line indents each inserted line, so the rewrite changes the generated Icons file. The same template also has content lines at the closing brace's column, below the original rule's C+4 minimum (~/repos/boon/docs/language/TEXT_SYNTAX.md:181-196). The new lexer must define and fixture that dedent case. D18 requires that TEXT not break, but the plan's only formatter line is a straight port.

Evidence:
- [file, verified] `crates/boon_parser/src/lib.rs:4155-4190,4208-4350` — line-based reindent and bracket compaction with no TEXT awareness
- [measurement, verified] `python3 /tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/X2/fmt_replica.py examples/todo_mvc_physical/BUILD.bn` — diff: '-        icon: [ / -            {code} / -        ]' becomes '+        icon: [{code}]' inside TEXT. A replica, not the Rust binary: boon_cli has no format subcommand.
- [file, verified] `examples/todo_mvc_physical/BUILD.bn:25-31`
- [file, verified] `crates/boon_native_playground/src/dev.rs:1045` — the dev window calls the formatter
- [source, verified] `~/repos/boon/docs/language/TEXT_SYNTAX.md:167-220` — visual indentation algorithm (C+4)

```boon
code => TEXT {
    -- Generated from {icons_directory}

    icon: [
        {code}
    ]
}
-- today's formatter turns the three lines into `icon: [{code}]`, changing the generated file
```

**Proposed plan change.** Replace §4.2 'format ports format_source_unit' with (about 5 lines): '`format` is rebuilt on the boonc_syntax token stream. Raw-mode TEXT spans are preserved byte for byte. It is idempotent, and parse(format(x)) equals parse(x), including evaluated TEXT constants, on every tracked .bn file (O1 fixture). It may apply D11 parenthesis fix-its only on request.' Add the multi-line TEXT dedent algorithm, including content at the closing brace's column, to the §4.2 layout rules and to the O1 multi-line TEXT fixtures. In P1b, `highlight` classes `--` inside TEXT as text.

**Refuter correction (low tier, high confidence).** None; this is a solid, code-verified finding. The proposed replacement text (rebuild format on the token stream, preserve raw-mode TEXT byte-for-byte, add an idempotence oracle) is appropriately scoped.

---

### MIS-016 [medium] BITS[N]/BYTES[N] widths have no place in the kind-partitioned algebra or in FUNCTION schemes, yet the RISC-V hardware gate depends on BITS typing

*Plan: §4.3 Types; §4.4; P0 reconcile list; S2; Sources: X1; verdict: confirmed; severity high -> medium after refutation*

§4.3 lists BITS and BYTES as kinds with 'at most one member per kind' and says nothing about widths. Today's documented semantics compute result widths from constant arguments: concat gives BITS[N+M], slice gives BITS[count] and add_widening gives BITS[N+1]. On its face that contradicts D4's 'types never depend on values'. It also needs either width variables in schemes or per-instantiation width predicates. The plan leaves open: joins of different fixed sizes (BYTES[3] ⊔ BYTES[4]), how a HOLD with a fixed-size initial value treats other sizes, and width-generic user FUNCTIONs. Today all three fail or pass silently. BOON_FIRST_RISCV_PROCESSOR_PLAN.md makes 'BITS[N] parses, typechecks, lowers…' a mandatory hardware-readiness gate and models the register file as MAP BITS[5]→BITS[32]. The plan's P0 reconciles that plan only about hashed stage artifacts.

Evidence:
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:448-498` — width part of type; concat N+M, slice count, add_widening N+1; equal-width ops reject mixed widths
- [measurement, verified] `target/release/boon_cli check scratchpad/X1/bits_poly.bn` — FUNCTION low_nibble(x) { x |> Bits/slice(from: 1, count: 4) } called with BITS[8] and BITS[16]: internal 'dense kernel checked construction does not cover the complete project'
- [measurement, verified] `target/release/boon_cli check scratchpad/X1/bits_mix.bn` — BITS[8] HOLD updated with BITS[16]: 'published state store.reg has an open, unresolved, or non-data memory type Object(...)' (not positioned)
- [measurement, verified] `target/release/boon_cli check scratchpad/X1/bytes_mix.bn` — BYTES[3] HOLD updated with BYTES[4]: accepted, 'pass: MachinePlan 11.0, 3 operation(s)'
- [file, verified] `docs/plans/BOON_FIRST_RISCV_PROCESSOR_PLAN.md:192,638` — hardware gate requires BITS[N] typecheck; register file MAP BITS[5]→BITS[32]
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:404` — the review already listed 'residual predicates for type-dependent BYTES/BITS checks' as missing

**Proposed plan change.** Add §4.3 'Widths' (about 20 lines). The width is a ground attribute of the BITS/BYTES member. A scheme carries width predicates (e.g. 'from+count-1 ≤ N', 'widths equal'), checked at each instantiation like the 'tags within' predicate. Result widths are computed per call site from constant arguments; a width that is not constant is T14. Rewrite D4's wording as 'types never depend on runtime values; widths come from compile-time constants'. Add BITS/BYTES width fixtures to O1 and a BITS scheme case to S2. In P0, reconcile BOON_FIRST_RISCV_PROCESSOR_PLAN's BITS/MAP gate items against §4.3.

**Refuter correction (high tier, medium confidence).** Downgrade to medium. Drop the claim that D4 conflicts with widths: spec.md:24 already words it as 'runtime values; widths from compile-time constants'. Drop the bits_poly measurement or re-derive it; it now fails on missing `Bits/xor`, not on width genericity. Replace the proposed 20-line §4.3 'Widths' with a shorter change: 'The §4.3 width and BYTES rules are spec.md:24, 44-45, 209-214, 543, 639-645 and checker.md:75 (BitsWidthEq as a residual scheme predicate); P0 writes them into static semantics v2.' Keep the proposal to add BITS/BYTES width fixtures (O1) and a BITS residual-predicate case to S2. Keep the P0 reconcile line for the RISC-V plan's BITS/MAP gate items (RISCV plan:190-195, 638). Narrow the owner question to one item, because (1) is already answered by checker.md:75, (2) by spec.md:44, and literal-only static arguments by checker.md:319. The remaining question: does a HOLD whose starting value is BYTES[N] have a fixed capacity, so that a BYTES[M] update with M≠N is an error, or does it widen to dynamic BYTES as spec.md:44 and today's binary do? Recommend fixed capacity, to match the RISC-V plan's fixed register widths.

**Owner question.** (1) Width-generic FUNCTIONs; (2) join of different fixed sizes
- (1)A: widths resolved per instantiation with width predicates in the scheme
- (1)B: width variables with linear constraints solved in the scheme
- (1)C: forbid width-generic FUNCTIONs (every BITS parameter used at one width)
- (2)A: BYTES[N] ⊔ BYTES[M] = dynamic BYTES; BITS widths never join (T13)
- (2)B: any mismatch is an error
Recommendation: (1)A: it fits 'instantiate once per syntactic call site' with no arithmetic solver. (2)A for values, but a HOLD whose initial value is BYTES[N] rejects other sizes (declared capacity).

---

### MIS-017 [medium] BYTES has no working baseline and no spec section: 31 of 32 fixtures fail check today, 0 of 29 scenarios pass, and the negative templates the plan seeds into O1 have been orphans since their tests were deleted

*Plan: §4.3 Types; §7 O1/O4; S6; Sources: X1; verdict: confirmed; severity high -> medium after refutation*

The plan's only BYTES content is the kind list, plus O1 seeding from `bytes_negative_templates`. Those are 10 templates with @PLACEHOLDERS@ (7 are .bn.tmpl) and nothing instantiates them. Commit 9cefceae deleted about 3.1 k lines of BYTES backend/executor/runtime tests, including the include_str! consumers of the templates. Today only one bytes fixture checks, and it is bytes_indexed_duplicate_update_conflict, which §4.3's 'two arms updating from the same trigger in the same step' rule makes an error. bytes_same_event_dependency_plan_ops relies on a same-tick read of `store.patched` that D21 flips. Its .scn has no expectations, like most bytes scenarios. BYTES streams (File/read_stream: multishot Opened/Chunk/Finished with credit) are not classified under D31. The rules the new checker and Phase A must own are listed under proposed_change.

Evidence:
- [measurement, verified] `for f in examples/bytes_*.bn; target/release/boon_cli check $f` — check ok=1 bad=31; mostly 'dense kernel checked construction does not cover the complete project'
- [measurement, verified] `for s in examples/bytes_*.scn; target/release/boon_cli run <source> --scenario $s` — pass=0 fail=29
- [file, verified] `git show 9cefceae --stat; git show 9cefceae | grep '^-.*negative_templates'` — deleted machine_plan_backend_tests/bytes/*.rs (~2.5k lines) and runtime/xtask bytes tests; removed include_str!("../../../examples/bytes_negative_templates/file_write.bn.tmpl") etc.
- [file, verified] `examples/bytes_negative_templates/hold_source.bn.tmpl` — @PAYLOAD_NAME@/@UPDATE_EXPR@ placeholders; no remaining consumer (rg over crates/xtask/tests)
- [file, verified] `examples/bytes_indexed_duplicate_update_conflict_plan_ops.bn:13-18` — two `receive.bytes |> THEN` arms in one LATEST: accepted today, error under §4.3
- [file, verified] `examples/bytes_same_event_dependency_plan_ops.bn:6-24` — patched_byte reads store.patched updated by the same store.patch (D21 flips this)
- [file, verified] `docs/architecture/BYTES_SEMANTICS.md:148-165,183-216` — bounds/conversion rules; streaming is a bounded multishot effect, never HOLD/compared/persisted

**Proposed plan change.** Add §4.3 'BYTES' (about 25 lines, next to 'Widths'), listing the rules the checker or Phase A owns: (1) byte literals only inside BYTES{}; Bytes/set value is BYTES[1]; (2) static positions are whole and ≥1, and static out-of-bounds access on fixed-size BYTES is an error; (3) byte_count ∈ {1,2,4,8}; (4) static TEXT/hex/base64 refines to BYTES[N], and malformed static data is an error; (5) no implicit TEXT↔BYTES interpolation (T20); (6) SOURCE `bytes` payload typing from the provider (bytes_source_payload, bytes_named_payload_rejected); (7) File/write_* only in copy contexts, and File/read_* is a query even with dynamic paths or indexed row fields (the file_* templates); (8) same-trigger duplicate update is an error; (9) declared and typed-index capacity for fixed byte banks. Add a P0 item (S6 scope, about 3-5 days): turn the 10 templates into concrete O1 fixtures with .expect files, give every bytes .scn real expectations under D21/D30-D32, and declare BYTES spec-only in O4 (the old engine cannot serve as an oracle). D31 catalog: File/read_stream is a query; a newer run cancels the stream; each chunk fires (D32).

**Refuter correction (high tier, medium confidence).** Downgrade to medium and merge with MIS-016: both are 'the plan does not adopt the notes' BITS/BYTES rules'. Remove the D31 read_stream sub-claim; File/read_* already covers it. Remove the O4 'spec-only' proposal; plan:839 already implies it. Reword 'no expectations' to 'only expected_source_event routing asserts, no value assertions'. Reword '0 of 29' to '0 of 28, plus one .scn with no source'.

Better proposed change:
1. In §7 O1, say that the 10 bytes_negative_templates must be instantiated into concrete fixtures with .expect files. They have no consumer since commit 9cefceae.
2. Add a line to §5 or S6: the bytes_*_plan_ops fixtures (non-manifest, examples.md:489) get a keep/migrate/delete decision. Kept ones get value assertions written from BYTES_SEMANTICS.md under D21/D30-D32. bytes_indexed_duplicate_update_conflict becomes a negative fixture (plan:395-396). bytes_same_event_dependency is rewritten for D21 committed-snapshot reads.
3. Point §4.3 to spec.md:186, 209-213, 639-645 for the BYTES static rules.

This work fits P2b or P3a (examples migration) better than P0, and is closer to 2-3 days than 3-5, because the rules are already written in the notes.

---

### MIS-020 [medium] The in-process compile of untrusted source (persons_pro PublicClient child programs) needs hostile-input bounds; the plan has no resource limits, depth limits or fuzz oracle, and no Config field for the capability profile

*Plan: §4.8 Config; §4.9; §7; Sources: X1; verdict: confirmed*

persons_pro compiles user-authored Boon in process through `Scene/Element/program(source: …, capability_profile: PublicClient)`. program_host bounds only the source unit count and byte size, and ties the profile to the role. D3 turns load_untrusted/verify_plan into the only guard for received plans. The new front end is recursive descent plus Pratt and must at least match today's tolerance of deep nesting. Hash-consed types keep doubling types linear, but printers, hover and diagnostics that expand trees would not. Today's compiler already fails on such programs: an internal lowering error at n=2 and more than 25 s (timeout) at n=6. §4.8 `Config { entry, target, role, app, … }` has no capability profile, so which effects a child program may use is not a positioned checker diagnostic.

Evidence:
- [file, verified] `examples/persons_pro/View/Preview.bn:72-95` — Scene/Element/program(source: PASSED.store.source_draft, capability_profile: PublicClient, …)
- [file, verified] `crates/boon_program_runtime/src/program_host.rs:229-275` — limits: max_source_units/bytes; profile must match role
- [measurement, verified] `target/release/boon_cli check scratchpad/X1/deep.bn (20,000 nested parens) and deep_list.bn (3,000 nested LIST)` — both pass today
- [measurement, verified] `/usr/bin/time timeout 20 target/release/boon_cli check scratchpad/X1/blowup{2..6}.bn (f_i(x) = f_{i-1}(x: f_{i-1}(x: x)), f0(x) = [a: x, b: x])` — n=2..5 internal 'failed executable lowering' (n=5 0.47 s, 61 MB); n=6..12 timeout at 25 s
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:681,723` — Config has no capability profile; 'no reliance on catch_unwind'

**Proposed plan change.** §4.9: add hostile-input rules. Nesting depth cap (at least 20 k, or an explicit stack) with a positioned diagnostic; caps on type/scheme DAG size, instantiations, solver steps and diagnostic count per compile, each reported as a positioned 'limit exceeded' diagnostic rather than a panic. Printers and hover are DAG-aware and truncating. Add `capability_profile` to Config, with host effects and ports outside the profile reported as checker diagnostics. §7: add O7, fuzzing of the parser/checker (cargo-fuzz corpus from examples) and of `load_untrusted` on mutated plan bytes (no panic, bounded time and memory). Size: about 1 week across P1a/P2a/P4.

**Refuter correction (low tier, high confidence).** None; this is a solid, plan-text-confirmed gap with a concrete, actionable fix (nesting/DAG-size caps with positioned diagnostics, capability_profile in Config, an O7 fuzz oracle).

---

### MIS-021 [medium] The editor feature scope is implicit: decisions promise richer hover (copies, effect rows, update kinds, 'no value until'), but the API and phases budget only hints and occurrences; symbol rename, completion and LSP are neither in nor out

*Plan: §4.7 Messages; §4.8; P5; §12.2; Sources: X1; verdict: confirmed*

Today the dev window offers highlighting (lex_source), an inspector (symbol, static type, detail, current value), go-to-definition, next reference, format and a diagnostics blob. On any parse error the language snapshot drops all hints and occurrences. D30 ('hover always says what a WHEN copies'), D31 ('effects become visible through hover, an inferred effect row on every FUNCTION scheme') and L3c ('hover says no value until…') commit to hover content that §4.8's `hints(file)`/`occurrences()` does not describe, and no phase has a work item for it. The snapshot IPC budget (≤2 ms) was sized without that content. The notes estimate a whole NovyWave snapshot at about 3 MB per keystroke and proposed delta publication (SE5), which the plan dropped. Rename, completion and LSP are not mentioned at all.

Evidence:
- [file, verified] `crates/boon_native_playground/src/dev.rs:1045,1290-1340` — format_source_unit; inspector from semantic_at/hint_at; definition_at/next_reference_at
- [file, verified] `crates/boon_editor/src/language.rs:425-455` — project_parse_error_language: inspector_hints empty, semantics Vec::new(), one diagnostic
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:336,521` — ~3 MB per NovyWave snapshot if sent whole; SE5 delta publication
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:409-410,683` — stale-copy hover promise; API hints/occurrences only

**Proposed plan change.** Add a §4.8 'Editor features' table. In scope at P7: parity with today, plus hints, navigation and hover for every definition not affected by a syntax error (acceptance: a syntax error in definition X keeps hints and navigation elsewhere), plus a hover payload with update kind, copies set and effect row, computed lazily per hovered expression rather than shipped in the snapshot. Out of scope, moved to §12.2: symbol rename (the occurrences with DeclId are already enough), completion, LSP. Put the hover item in P5 (SE3 in the notes, size M), and reinstate SE5 delta publication as the fallback if snapshots exceed about 64 KB.

**Refuter correction (low tier, medium confidence).** Soften the 'SE5 dropped' aside: P5 (line 1022) does keep a 'snapshot deltas' concept in the compile-service description, so the delta-publication idea is not fully absent, just not sized or connected to the hover-payload problem. The rest of the finding (missing hover-payload work item, unscoped rename/completion/LSP) holds as stated; severity medium is reasonable.

---

### MIS-022 [medium] R8 and R9 must be implemented in every host and in the wasm32 runtime build, but the plan sizes consumers only for R-DOC

*Decisions: D31, D34, D35, D36; Plan: §4.5 R8/R9; §4.6 'Consumers to adapt'; §4.9 wasm32 rule; P5; Sources: X2; verdict: confirmed*

R9 ships Http/get and Http/send, query supersession in both context kinds, level host ports ('focused', server connection lists), a 'restored' occurrence and an effect log. Http/request is implemented today in boon_server_runtime, boon_http_runtime, boon_app_server, boon_server_host and boon_http_client, and focus and input handling live in boon_web_host (input.rs, document_runtime.rs, wasm/semantic_dom.rs). §4.6 lists consumers for R-DOC only. For R8 and R9, P5 says only that those crates' tests 'run with next'. The day-one wasm32 check (§4.9) covers only the compile crates, yet the FjordPulse client ships as browser Wasm (apps/fjordpulse/app.toml, target_profile software_bounded, public-webgpu), and R4, R8 and R9 all change boon_plan_executor and boon_document.

Evidence:
- [measurement, verified] `rg -rln 'Http/request|HttpRequest' crates --include=*.rs` — 11 non-test files across server, http, app_server and http_client
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:644-650,711-715,1022-1024`
- [file, verified] `apps/fjordpulse/app.toml`

**Proposed plan change.** Add a 'Consumers of R8/R9' table under §4.5 (about 8 rows: boon_plan_executor, boon_web_host with boon_web_effect_host, boon_server_host, boon_http_runtime and boon_http_client, boon_wellen_host, boon_host_services, boon_distributed_runtime, boon_native_playground dev window), each with a size from S1. Extend §4.9's day-one check to 'boon_plan_executor, boon_document and boon_web_host build for wasm32 in every change set that touches R1-R9'.

**Refuter correction (low tier, high confidence).** None material; the proposed consumer table and broadened §4.9 wasm32 rule are reasonable and proportionate to the evidence. Confirm 'public-webgpu' in app.toml refers to a WebGPU capability profile name, not independently proven to compile to wasm32 in-browser -- minor, does not change the verdict since the core consumer-list gap is verified directly from plan text.

---

### MIS-023 [medium] The scenario format has no vocabulary for restarts, time, effect logs or host level ports, and 113 assertions depend on DocumentPlan deltas

*Decisions: D30, D31, D34, D35; Plan: §7 O2; §5.2 S6; R4; R9 ('effect log for the dev window and the scenario runner'); Sources: X2; verdict: confirmed*

O2 requires 'a restart scenario per persisted example' and a UI-retention scenario. Manifest .scn files offer only user_action, expected_source_event and expect_* keys; start, restart and preview exist only in migration scenarios (examples/migrations/counter/migration.scn:5,88-89,111). Nothing lets a scenario inject host level ports (focused, hovered), fire the 'restored' occurrence, control virtual time for Timer/interval or Timer/deadline, order query completions to test supersession (D31), or assert the R9 effect log. Across examples/*.scn, 57 expect_semantic_delta_contains and 56 expect_render_delta_contains assertions match today's DocumentPlan delta strings, which R4 replaces. The plan mentions scenarios as oracles but has no format item.

Evidence:
- [measurement, verified] `grep -rhn -o '^[a-z_]* *=' examples/*.scn | sort | uniq -c` — 57 expect_semantic_delta_contains, 56 expect_render_delta_contains; no restart key in manifest scenarios
- [file, verified] `examples/migrations/counter/migration.scn:5,88-93,111`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:614,776-784,837`

**Proposed plan change.** Add a 'Scenario format v2' item to S6 (spec at most one page) with step kinds restart, hot_reload, set_host_port, advance_time, complete_query (with ordering) and expect_effect_log, and a rule that render assertions compare R-DOC frame trees, not delta strings. List migrating the 113 delta assertions as P3b work. The format is also the user-facing testing tool, so document it in the same change set as LANGUAGE_SEMANTICS v2.

**Refuter correction (low tier, high confidence).** None; finding is well-evidenced and actionable as stated. Severity medium is appropriate given O2 already depends on this capability.

---

### MIS-024 [medium] The R-DOC oracles compare rendered frames only, so accessibility semantics and sensitive redaction are unchecked

*Decisions: D13, D22; Plan: §4.6 R-DOC; §4.5 differential ('documents are compared by rendered frames, structurally, modulo ids'); §5.2 CPU render diff; D13 closed contracts; D22; Sources: X2; verdict: confirmed*

The document runtime today emits semantic and accessibility attributes: aria-selected, lang, aria-level for headings, tabindex, focus and sensitive-input flags (boon_document/src/lib.rs:1125-1160). NATIVE_GPU_PIPELINE.md:115 routes accessibility actions. R-DOC replaces the pipeline that produces them. Its gates are per-frame work and retained-update counts, the render diff and the native readback A/B, and none of those sees the accessibility tree, the lang attribute or redaction. Under D13's closed contracts these keys must also be listed in the catalog. D22 adds font family; the render contracts say nothing on bidi or RTL shaping. For comparison, BOON_CONSOLE.md:624-635 has an accessibility section.

Evidence:
- [file, verified] `crates/boon_document/src/lib.rs:1125-1160`
- [file, verified] `docs/architecture/NATIVE_GPU_PIPELINE.md:113-116`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:619-621,652-653,786-790`

**Proposed plan change.** Add to R-DOC's exit criteria (§4.6, about 3 lines): 'the semantic/accessibility tree (roles, labels, heading levels, lang, selection, focus, sensitive flags) is equal, modulo ids, between v11 and v2 on every manifest example, and the sensitive redaction tests pass.' Require the P2b catalog completeness test to cover accessibility keys. State that RTL and bidi shaping are out of scope for the rewrite.

**Refuter correction (low tier, medium confidence).** Confidence should be marked low-to-medium rather than presented as a certain gap, since it hinges on an unstated definition of 'rendered frame' that could already include these attributes structurally (e.g. via the element-record/contract system in D13). The proposed fix (explicit exit-criteria line) is still the right, cheap remedy regardless of which way the ambiguity resolves.

---

### MIS-025 [medium] The P0 reconcile list misses several documents the decisions contradict, and moves the console stage chain at P8 instead of P0

*Decisions: D2, D30, D33, D35; Plan: P0 'Reconcile the active contracts' (:874-891); P8 (:1055-1056); Sources: X2; verdict: confirmed*

The reconcile list omits these contradicting texts. RUNTIME_MODEL.md tick phases, conflict resolution and the no-mid-evaluation-commit rule (:76-93, :188-200). LANGUAGE_SEMANTICS.md:374-377 ('the piped expression is the initialization value ... Dynamic resets are ordinary update candidates', which contradicts D33) and the hardware analogy at :351-360; this could fall under 'static semantics v2', but no line says so. FPGA_TODOMVC_LOWERING.md, whose one-cycle pulse contradicts D30 and which the console deletion ledger says to delete (BOON_CONSOLE_IMPLEMENTATION_PLAN.md:750). The persistence plan's stage-chain references (:30-35), its FPGA paragraphs and its sensitive-input section. The console's reset matrix, Timer/Boot inputs and 'mandatory verified compiler spine' (BOON_CONSOLE.md:48). The RISC-V plan's lineage list (:669-678), which names checked/erased program digests. The console stage-chain rewrite is also scheduled twice: P0 says reconcile, while P8 (:1055) says 'Update BOON_CONSOLE's stage chain'. The earlier critique asked to move it to P0 (audit/plan_draft_critique.md:150). AGENTS.md makes those documents authoritative for the whole P0-P7 window. Finally, the static semantics v2 deliverable has no size or effort line. The draft sources predate D30-D36: spec.md (978 lines, §6 'Temporal constructs') and change_and_effects.md, whose model C with 'moments' D30 superseded. P0 is two weeks and also holds six spikes.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:866-891,1055-1056` — reconcile list; P8 stage-chain item
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:351-360,374-377`
- [file, verified] `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:30-35` — stage chain through ContractVerifiedProgram and ErasedProgram
- [file, verified] `docs/plans/BOON_FIRST_RISCV_PROCESSOR_PLAN.md:669-678`
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/plan_draft_critique.md:141-150` — earlier critique asked to move the console update to P0
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:146-160` — model C 'moments', superseded by D30

**Proposed plan change.** Extend the P0 reconcile list (about 8 lines). Add: RUNTIME_MODEL.md (tick and commit model after R8, the conflict step); LANGUAGE_SEMANTICS.md HOLD initialization and hardware analogy (D33, D30); FPGA_TODOMVC_LOWERING.md (delete it now, since its generic constraints move to the §4.3 state-classes table); the persistence plan's :30-35, :348-368 and :1290-1317; BOON_CONSOLE.md :48, :150-170 and :535-556; the RISC-V plan's :250-300 and :669-678. Delete the P8 bullet or turn it into 'verify the P0 console rewrite still holds'. Add a P0 line item 'static semantics v2: target ≤1,200 lines, rewritten from D4-D36, not copied from spec.md §6 or change_and_effects.md' with its own effort (about 1 engineer-week), and re-baseline P0 at three weeks if needed.

**Refuter correction (low tier, medium confidence).** Adjust the 'scheduled twice' framing: P0 already carries the substantive console-lineage redesign; P8's bullet is better described as documentation-sync-after-deletion than a duplicate rewrite. The remaining claims (RUNTIME_MODEL.md and FPGA_TODOMVC_LOWERING.md omitted from the reconcile list, no sizing for 'static semantics v2', P0 likely underestimated) are verified and should be kept; consider slightly lowering severity of the 'scheduled twice' sub-point specifically while keeping the rest at medium.

---

### MIS-008 [low] No catalog versioning, although D13's closed contracts and D36 make every catalog change a breaking change for packages and durable outboxes

*Decisions: D13, D36, D19; Plan: D13, D36; §4.5 Trust (load_untrusted); §4.8 COMPILER_ID; identity_v1 EffectId; Sources: X2; verdict: confirmed; severity medium -> low after refutation*

Under D13 an unknown key is an error, so adding, removing or renaming a catalog entry changes which programs are valid. D36 already removes Http/request. App bundles are loaded through load_untrusted, which decodes and runs verify_plan, but neither the plan nor today's code carries a catalog version. boon_effect_schema has only WELLEN_BRIDGE_SCHEMA_VERSION. EffectIds are durable through identity_v1 and the outbox, and today's migration refuses to activate when the target schema removes an effect that still has unfinished work (boon_persistence/src/migration.rs:204-213). Nothing says how an older bundle meets a newer host, or how a renamed effect keeps its pending outbox items.

Evidence:
- [measurement, verified] `rg -n -i 'version' crates/boon_effect_schema/src/lib.rs` — only WELLEN_BRIDGE_SCHEMA_VERSION and field names
- [file, verified] `crates/boon_persistence/src/migration.rs:204-213`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:106,582,595-600`
- [file, verified] `apps/fjordpulse/app.toml` — capability_profile_id strings such as fjordpulse-public-webgpu-v1, with no catalog version

**Proposed plan change.** Add to §4.5 or §4.8 (about 6 lines): a CATALOG_VERSION (major.minor) stamped in the MachinePlan header and in COMPILER_ID. load_untrusted rejects a newer major than the host. Removed or renamed entries stay in the catalog as errors with a fix-it (`Http/request` becomes `Http/get` or `Http/send`). An alias table maps old EffectIds for outbox migration. The catalog completeness test (P2b) also checks that every removed name has an alias or a deliberate tombstone.

**Refuter correction (low tier, medium confidence).** Reframe as a forward-looking recommendation rather than a cutover-risk finding: drop the fjordpulse/outbox-migration framing (D19 already resets it with no migration), and instead ask only whether the new catalog needs a version field for post-rewrite production use going forward. Lower severity from medium to low given D2/D19 remove the near-term urgency.

---

### MIS-010 [low] D32 'every write fires' with D12 touched-value persistence means a durable write per accepted write, including identical large BYTES; the plan should allow store-level dedup

*Plan: §4.5 Persistence; R8; Sources: X1; verdict: confirmed*

D32 makes a HOLD fire on every accepted write, even an equal one ('HOLD stops deduplicating equal values', R8). D12/D35 persist every touched HOLD whenever a store exists. A value refreshed with identical content, such as a query-driven File/read_bytes that re-runs when its arguments update or a periodic check, would rewrite the whole blob each time. BYTES values are shared by Arc, so the in-memory write costs nothing, but the durable write does not. Skipping a byte-identical durable write is invisible to the semantics, because a restore yields the same value. The plan does not say whether the store may skip it.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:101,613` — D32; R8 'HOLD stops deduplicating equal values'
- [file, verified] `docs/architecture/BYTES_SEMANTICS.md:167-180` — immutable Arc-shared bytes::Bytes; large content via ContentRef

**Proposed plan change.** Add one sentence to §4.5 Persistence: 'Dataflow firing is not store I/O: the persistence layer may skip a durable write whose canonical bytes equal the stored bytes; this is invisible to restore.' Add a counter for durable writes per tick to the R-DOC/runtime performance report.

**Refuter correction (low tier, medium confidence).** None needed beyond what the finding already proposes; it is appropriately scoped as a documentation clarification with a suggested runtime counter.

---

### MIS-018 [low] Typed-index capacity is assigned to Phase A (diagnostics lane), but typed indexes are chosen by the backend; either index selection moves into Phase A or the build lane emits diagnostics

*Plan: §4.5 Phase A; §4.7 lanes; §7 O5; Sources: X1; verdict: confirmed; severity medium -> low after refutation*

§4.5 says Phase A checks 'typed-index capacity and declared capacity' from target-profile limits. Today the backend (the Phase B analogue) chooses the typed indexes. verify_plan checks them from facts only the backend knows: the key multiplicity ListItems{max_items} multiplied by slot capacity, the number of indexes per list, and the fanout per mutation. The plan asks for every diagnostic in the diagnostics lane and for warm==cold per lane, so it has to say where index selection happens.

Evidence:
- [file, verified] `crates/boon_plan/src/lib.rs:11540-11590` — indexes per list, fanout per mutation, ListItems max_items × slot capacity against the profile's max_entries_per_index
- [file, verified] `crates/boon_compiler/src/machine_plan_backend.rs:1726-1800,2923-2932` — list_indexes built during backend storage/topology lowering
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:540-542` — Phase A: target-profile limits; typed-index and declared capacity

**Proposed plan change.** In §4.5 Phase A, add 'list-access planning: selection of typed indexes and key multiplicity (a pure function of the typed program and Config.target)', and add it to the Phase A budget in S1. Otherwise state explicitly that capacity diagnostics come from the build lane, are published with the plan, and that O5 compares both lanes.

**Refuter correction (low tier, low confidence).** This reads more like a real but low-cost ambiguity (one clarifying sentence, as the finding's own proposed change says) than a deep architectural conflict; the finding itself acknowledges this ('If boonc_lower's list access item already lives in Phase A, one sentence fixes this').

---

### MIS-019 [low] Target-profile work is under-specified: the console needs versioned profile documents, value-range eligibility proofs and a board-independent MachinePlan

*Decisions: D4, D13, D25, D3; Plan: §4.5 Phase A 'target-profile limits keyed on Config.target'; §4.8 Config; P0 reconcile (console); Sources: X2; verdict: confirmed; severity medium -> low after refutation*

The plan moves 'target eligibility' to boonc_check/Phase A but names only typed-index capacity and declared capacity. The console plan's Decision 2 replaces the closed TargetProfile enum with a versioned TargetProfileDocument that is 'stored or digest-bound in MachinePlan', with separate app, core-hardware and board schemas (BOON_CONSOLE_IMPLEMENTATION_PLAN.md:116-127). The RISC-V acceptance criteria require the core source, the semantic MachinePlan and the CoreHardwareIR digest to be identical across board profiles (:682-688, :1127-1128). So Config.target must never carry board data, and the plan does not say that. Console eligibility (BOON_CONSOLE.md:272-297) needs proofs that bounded whole Numbers stay in a fixed machine range, static BITS widths, no unbounded TEXT, LIST, MAP or SET, statically bounded control flow, and a bounded transaction size. That is a value-range analysis, which D4 ('types never depend on values') keeps out of the type system. No crate in §4.1's roughly 45k-line estimate owns it, and today's ContractVerifiedProgram obligations are empty (audit/representations.md:77, taken from notes). Element values (D13) are unbounded recursive data and are ineligible on these targets; the plan does not say so.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:540-542,876-879,1165-1167`
- [file, verified] `docs/plans/BOON_CONSOLE_IMPLEMENTATION_PLAN.md:116-127,749` — replace the TargetProfile enum; delete the FpgaTodomvc variant
- [file, verified] `docs/plans/BOON_FIRST_RISCV_PROCESSOR_PLAN.md:682-688,1127-1128`
- [file, verified] `docs/architecture/BOON_CONSOLE.md:272-297` — eligibility envelope
- [file, verified] `crates/boon_plan/src/lib.rs:53-130` — today's closed enum and its limits
- [file, from notes] `docs/plans/compiler_rewrite_notes/audit/representations.md:77` — the notes say all obligation vectors are empty today

**Proposed plan change.** §4.8 Config (about 4 lines): 'target is a validated semantic target profile (software default, software bounded, console app, core hardware). Board and shell profiles never reach the compiler, so the MachinePlan is identical across boards.' §4.5 Phase A: list which limits are static and which remain runtime ceilings (pulses per activation; index key and payload limits on restored values). Add a named work item outside the 45k estimate and off the critical path, 'Target eligibility analysis (value ranges for bounded Number, BITS widths, bounded collections, static microstep bound, no element values)', and state in the reconciled console docs that the rewrite does not provide it.

**Refuter correction (low tier, medium confidence).** The finding's own refutation_hints already concede the console is not on the rewrite's critical path; severity should reflect that this is a documentation/scoping gap for later console integration, not a blocker for the compiler rewrite itself.

---

### MIS-026 [low] Time sources are unclassified: Timer/interval is neither a query nor a command, and restore, D32 and console behaviour are undefined

*Decisions: D31, D32, D35; Plan: D31, D32, D35 ('current time' is host-known); R9; Sources: X2; verdict: confirmed*

D31 lists Timer/deadline as a query and Clock/wall as a command. Timer/interval, used in examples/interval_latest.bn:2, interval_hold.bn:2 and kavik_cz/View/KavikAbout.bn:179, is in neither class and is not a boon_effect_schema entry. The plan does not say whether the interval restarts its phase on activation, restore or hot reload, whether ticks missed during downtime are dropped (D35 never replays occurrences), or how it lowers on hardware (a counter derived from the clock) and on the console. There, time arrives as TimerFired(slot, generation) inputs driven by timer_commands in the committed frame (BOON_CONSOLE.md:157,167). Replacing an occupied slot matches D31's supersession, but no document connects the two.

Evidence:
- [measurement, verified] `rg -n 'Timer/interval|Timer/deadline|Clock/wall' examples --include=*.bn` — 4 interval sites, 1 deadline, 1 Clock/wall
- [file, verified] `crates/boon_effect_schema/src/lib.rs:144-149` — Clock/wall, Random/bytes, Secret/verify, Timer/deadline; no Timer/interval
- [file, verified] `docs/architecture/BOON_CONSOLE.md:150-170`

**Proposed plan change.** Add to §4.3 (about 5 lines): 'Timer/interval is a host occurrence port. Its phase starts when its scope activates, and missed ticks are never replayed (D35). Timer/deadline is a query whose supersession maps onto the console's timer-slot generation.' Add a virtual-time step to the scenario format.

**Refuter correction (low tier, medium confidence).** None material; finding is already appropriately low severity and correctly hedged. The proposed one-line clarification is cheap and directly closes the ambiguity regardless of which classification the owner intends.

---

### MIS-027 [low] Constant folding in the checker and Phase B must use the runtime's exact-rational Number, but the plan does not say so

*Decisions: D11, D6; Plan: §4.4 ('Ground expressions are evaluated directly'); §4.5 Phase B ('partial evaluator ... constant pools'); §4.3 static constant checks; D11; Sources: X1; verdict: confirmed*

NUMBER is an exact arbitrary-precision rational with Euclidean `%` and no NaN or infinity (LANGUAGE_SEMANTICS.md:20-39). The runtime implements it in crates/boon_data/src/number.rs. The new checker evaluates ground expressions, and Phase B's partial evaluator folds constants, but the plan never names the value implementation they use. Folding with a second arithmetic, or with f64, would silently diverge from the runtime, for example on 0.1 + 0.2 == 0.3, and O4 would report it only where the old engine agrees. The engineering rules ('no strings on the success path', §4.9) could push implementers toward a leaner number type.

Evidence:
- [file, verified] `docs/architecture/LANGUAGE_SEMANTICS.md:20-39`
- [file, verified] `crates/boon_data/src/number.rs` — exists; contents not reviewed
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:486-488,595-597`

**Proposed plan change.** Add one §4.9 rule: 'All compile-time evaluation (checker constants, static checks, Phase B folding) goes through boon_data's Number, TEXT and BYTES operations. No compile crate reimplements value arithmetic.' Add O1 fixtures for rational identities folded at compile time.

**Refuter correction (low tier, medium confidence).** None material. Low severity is appropriate; this is a cheap, well-evidenced documentation gap rather than a structural plan flaw.

---
