# Lens 1: evidence (causal claims and unaddressed objections): all findings

Generated from the verified finder output for `../REVIEW.md`. Severity is the severity after refutation; each finding keeps its refuter's corrections.

### EVI-005 [high] The placement rule for state and SOURCE is syntactic, while schemes carry no 'creates state' effect; calls through functions (including D26 Bool/toggle) and WHILE nested inside copies are undefined

*Decisions: D26, D30, D31, L6; Plan: §4.3 Functions, Placement; §8 S4, P2b; Sources: E345; verdict: confirmed*

§4.3 Placement makes a HOLD, SOURCE, stateful builtin or live query inside a copy context an error. A FUNCTION scheme lists only 'parameter predicates, PASSED, OUT scope effects and collection-write effects' (plan:363-366); D31 adds an effect row for host effects. Nothing tells a call site in a WHEN arm that the callee creates SOURCE ports or HOLD state. NovyWave does exactly this: `row.item_kind |> WHEN { VariableRow => new_selected_signal(...) }`, and new_selected_signal declares roughly nine SOURCE ports plus HOLDs. D26 turns Bool/toggle and similar builtins into Boon functions over HOLD, so after P3 every stateful-builtin call becomes an ordinary call that a syntactic rule cannot see. fibonacci.bn creates a HOLD with Stream/pulses inside a WHILE arm inside a THEN body. The plan does not say whether a WHILE nested in a copy is a live context (per-activation state) or still 'inside a copy'. Stream/pulses, Stream/skip and Timer/interval appear nowhere in the plan. The spec.md review asked for the rule to apply transitively and for an owner decision on per-activation state; the plan's L6 answer ('WHILE arms only') does not cover either case.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:363-366,390-393,95,1210`
- [file, verified] `examples/novywave/RUN.bn:4538-4541,4736-4750`
- [file, verified] `examples/fibonacci.bn:52-72`
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:937` — review: apply S4 transitively; owner question on per-activation state
- [measurement, verified] `rg -n 'Stream/pulses|Timer/interval' docs/plans/BOON_COMPILER_REWRITE_PLAN.md -> no matches`

```boon
FUNCTION row_widget(row) {
    [press: SOURCE, open: False |> HOLD open { press |> THEN { open |> Bool/not() } }]
}
view: store.kind |> WHEN { Row => row_widget(row: store.row), __ => SKIP }
-- plan: syntactic rule sees no HOLD/SOURCE in the arm -> accepted; the arm copies once,
-- so `open` never updates. A developer expects either an error or a live row.
```

**Proposed plan change.** In §4.3 Functions, extend the scheme with an activation row: {creates state, creates SOURCE, live query, command} (a union over the body and callees). In Placement, add: 'the rule applies at call sites through the activation row, so calling a state-creating FUNCTION (including D26 library functions such as Bool/toggle) from a copy context is the same error'. Add a nesting rule for WHILE inside THEN/WHEN, and ask the owner. Add Stream/*, Timer/interval and Build/* to the P2b catalog classification list and the S4 census (fibonacci, NovyWave new_selected_visible_item).

**Refuter correction (high tier, high confidence).** Keep high, because NovyWave's main row path and fibonacci hit this directly. Narrow the owner question to the nested case only (WHILE under THEN/WHEN). Treat transitivity as following from L6 and D26 rather than as a new decision: state in the plan that Placement applies through calls via a creates-state/creates-SOURCE bit in the scheme (or Phase A definition flags). Add NovyWave new_selected_visible_item/new_selected_signal and fibonacci to the §5.1 migration census, because under a transitive rule they become errors that need restructuring (for example, moving the SOURCEs to a WHILE or to row creation). I did not verify whether the §5.1 table already lists them. Add Stream/*, Timer/interval and Build/* to the P2b catalog classification list.

**Owner question.** Is a WHILE arm nested inside a THEN body or WHEN arm a live scope that owns state for one activation (from the copy until the next update of the copy's input)?
- (a) yes: per-activation live scope (fibonacci stays)
- (b) no: any state below a copy is an error (fibonacci must be restructured)
Recommendation: (a), with the scope's lifetime stated, because original Boon runs queries inside THEN once per activation, and fibonacci's per-request iteration needs it. The rule must be transitive through calls either way.

---

### EVI-002 [medium] D30 update kinds and trigger identity are the per-call fact that survives; the plan never says whether schemes carry them

*Decisions: D30, D32, D15, D25; Plan: 4.3 Types (update kind 'tracked at every nested field'), Change rules, line 414 'Update kinds are computed in one forward pass'; 4.4 Per function; 4.5 Phase A 'instance tree only for stateful definitions'; Sources: E12; verdict: confirmed; severity high -> medium after refutation*

Today's per-path key carries a flow-mode variable for each actual (InvocationKey.actuals: (TypeVariableId, ModeVariableId), owner.rs:15393). A formal's field modes are resolved by walking ancestor expression graphs (ModeSource owner.rs:15318-15326; FormalRead mode owner.rs:21610-21639; projected_mode_variable owner.rs:22488-22557). The constructs that force frames, LATEST, HOLD and THEN-carried effects, are exactly the change constructs. D30 keeps an argument-dependent property at every nested field, the update kind. Several 4.3 rules therefore depend on the call site when they appear inside a FUNCTION: 'THEN or WHEN over something that never updates is an error', 'Two arms that can update from the same trigger in the same step are an error', and 'at most one arm has a value from the start'. The result depends on the actual's update kind at a path and on trigger identity. The plan does not say where these are evaluated. 4.3 says the checker owns change rules and computes kinds 'in one forward pass in dependency order'. 4.5 Phase A builds instances only for stateful definitions. change_and_effects.md §5 puts the pass 'over the instance graph (Phase A)'. checker.md:381 notes that 'almost every scheme is mode-polymorphic'. Trigger identity for a SOURCE declared inside a function is per instance: TodoMVC's new_todo declares 6 SOURCEs (RUN.bn:101-110). If update kinds or triggers are computed on instances, the diagnostics lane iterates call paths again. On latest_12 that is 12,261 instances for 23 syntactic calls; on TodoMVC it is on the order of today's 10,537 compiled call sites. Warm per-definition early cutoff is also lost.

Evidence:
- [file, verified] `crates/boon_compiler_kernel/src/owner.rs:15391-15396, 15318-15326, 15518-15530 (ModeEquation Fixed/Copy/Eventful/Latest/Call), 21610-21639, 22488-22557`
- [file, verified] `crates/boon_checked/src/lib.rs:713-718 (FlowMode Continuous/TickPresent/PresentOrAbsent/Absent), 624-627 (FlowType {mode, ty})`
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:339-340, 385-397, 414-415, 434-446, 533-537`
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:154 (clock = set of roots incl. SOURCE declaration and effect call sites), :340-347 (pass over the instance graph)`
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:381 (LATEST rule anti-monotone; 'almost every scheme is mode-polymorphic')`
- [file, verified] `examples/todo_mvc_physical/RUN.bn:101-110 (SOURCE inside FUNCTION new_todo)`
- [source, from notes] `docs/plans/compiler_rewrite_notes/audit/claim_verification.md:270 (today '5 |> THEN {6}' accepted by both intents)` — taken from notes, not re-run

```boon
FUNCTION stamp(x) { x |> THEN { TEXT { fired } } }
a: stamp(x: 5)                                  -- plan: 'never runs' error, but the scheme or the instance must know x's update kind
b: stamp(x: store.sources.btn.events.press)     -- fine
-- today: 5 |> THEN {..} is accepted (per notes); a developer expects the error at stamp(x: 5)
```

**Proposed plan change.** Add to 4.3/4.4 the following. (1) Scheme-level update-kind terms. Every parameter path read gets a kind variable. Result kinds are join-normal forms over those variables in the 3-valued lattice. 'Must update' and 'has no start value' become scheme predicates, checked at instantiation and reported at the call site with a note into the callee. (2) Clock terms. Parameters get clock variables, and SOURCE declarations and effect sites inside a FUNCTION become generative roots, freshened per instantiation like type variables. The LATEST same-trigger rule is then decided on the root group without an instance tree. (3) Add latest_K and a same-trigger fixture to S2, and report the term counts. If the team prefers instance-level evaluation, it must remove 'the checker owns change rules' and re-budget Phase A (1.5 ms TodoMVC) against the instance count, not against stateful definitions only.

**Refuter correction (high tier, medium confidence).** Lower to medium. Split the proposed change. (1) Add one sentence to 4.4 'Per function': 'Schemes carry update-kind terms per parameter path; never-runs and start-value rules on parameters become scheme predicates reported at the call site with a note into the callee (the same mechanism as polymorphic exhaustiveness).' (2) Keep the owner question on trigger identity, since that is the genuinely missing part. Recommendation (a), generative roots per instantiation, is reasonable. Also add a same-trigger fixture to O1 and S2. Remove the TodoMVC '10,537 instances' extrapolation unless it is measured.

**Owner question.** For LATEST's 'two arms that can update from the same trigger' rule, are two call instances of the same FUNCTION that each declare a SOURCE (or run the same query call site) the same trigger or different triggers?
- (a) Different: triggers are per instance, as at runtime. The checker needs generative clock roots per instantiation.
- (b) The same: triggers are per syntactic site. This is simpler, but `LATEST { row(a).press, row(b).press }` would be rejected even though the presses come from different buttons.
Recommendation: (a). It matches the runtime and developer expectation, and generative roots keep the check at scheme level.

---

### EVI-003 [medium] A non-retracting solver is allowed by D4/D5 only if verdicts that can flip run after solving; D23 destination exactness ties schemes to the root group

*Decisions: D4, D5, D23, D24; Plan: 3.1 bullet 4; 4.3 Join ('nothing is ever retracted'), Contracts (D23); 4.4 Root group ('each function solves independently'); Sources: E12; verdict: confirmed*

Confirmed in code: today's solver retracts and never generalizes. Requirement sites belong to one invocation occurrence. 'Commit withdraws sites not visited this time', and 'skipping a child withdraws its effects' (requirements.rs:1-13, commit/withdraw 159-178). invalidate_requirement_cone performs 'component-local retraction' (requirements.rs:1266-1272). Authoritative providers get replace_binding instead of a join (solver.rs:1688-1690, 1703-1705), and principal results publish with PublishMode::Replace (owner.rs:20681). Withdrawal is driven by which arms a summary visits for the current input types, which is narrowing and is removed by D4, and by Replace publishes, which are non-ACI and removed by D5. So a bound-only solver is what D4/D5 permit. Several new rules, however, have verdicts that flip as bounds grow. D23 'extra fields are an error': discovering more consumption removes the error. Exhaustiveness is checked 'when the selector type is concrete'. D24 unused parameters. The LATEST start-value rule, which checker.md:381 shows is anti-monotone. Encoding any of these as a bound during solving brings retraction back. Separately, D23 says whole records forwarded 'into state or into a collection take that destination's exact type'. That destination is a root-group join, solved after the function schemes are instantiated, and 4.4 only turns root reads into outer parameters. Root writes and forwarding are not covered, so 'each function solves independently' and P6 scheme reuse are weaker than stated.

Evidence:
- [file, verified] `crates/boon_compiler_kernel/src/solver/requirements.rs:1-13, 159-178, 1266-1272`
- [file, verified] `crates/boon_compiler_kernel/src/solver.rs:1652-1715 (project: authority deferral, replace_binding)`
- [file, verified] `crates/boon_compiler_kernel/src/owner.rs:20676-20682 (PublishMode::Replace)`
- [reasoning, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:344-351, 421-425 (exhaustiveness when concrete), 461-466 (D23 exactness incl. state/collection destinations), 486-495 (root group)`

**Proposed plan change.** Add to 4.4: 'The solver only adds bounds. Verdicts that can flip as bounds grow (D23 exactness, concreteness and exhaustiveness, D24 unused, the LATEST start-value rule) are computed in a post-solve pass per definition, and the root group runs last.' Also: 'A root destination that a FUNCTION writes or forwards into enters its scheme as an outer monomorphic variable, like a root read. Exactness against it is checked after the root group solves. The scheme's reuse key therefore includes those outer variables.' Add a confluence property test that inserts producers and consumers in random order and asserts verdicts only after the fixpoint.

**Refuter correction (low tier, high confidence).** Severity is fair at medium; this is a real architectural gap (root-write/forwarding destinations aren't folded into per-function schemes) rather than a misreading. Note in the writeup that plan:448 already flags exactness-composition-through-forwarding as P0 work, so the actionable ask should be phrased as 'resolve this named-but-unspecified P0 item with the concrete rule below' rather than as a wholly new discovery.

---

### EVI-004 [medium] FLUSH under the change model is undefined, and a manifest example asserts a split between a HOLD field's value and its stored state that the plan's HOLD rule contradicts

*Decisions: D29, D30, D32, D33, D12; Plan: §1 D29; §4.3 Change rules; §8 P0 spec; Sources: E345; verdict: confirmed; severity high -> medium after refutation*

D29 makes a record field (store fields included) the boundary for FLUSH, and D30/D33 say 'HOLD takes every value that arrives, from its piped input or its body'. The manifest example flush_error_propagation, which carries a scenario, specifies something else. In it a FLUSH inside a HOLD body's THEN makes the field read the error (`hold_boundary = HoldError`), 'never enters HOLD storage' (a later copy of hold_state reads `Ready`), and 'recovers on a later independent activation'. No rule in §4.3 covers these questions. Does a FLUSHed update count as a write that fires downstream (D32)? Does it replace the HOLD's state (homogeneity, persistence under D12)? Is the field's error value its 'last value' (D30)? What happens with FLUSH inside a THEN body, a WHEN arm, a List/map new: body or a HOLD initializer? The checker review's 'missing' list asked for 'rejection of HOLD initialisers that may flush' and a may-flush effect bit. The plan's FLUSH text is only the boundary list. Today's release compiler fails to lower this example with an internal 'invalid executable local bindings' error, so no old-engine behaviour settles it.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:98,101-102,1211` — D29, D32, D33, L7
- [file, verified] `examples/flush_error_propagation.bn:1-3,69-89`
- [file, verified] `examples/flush_error_propagation.scn:17-39` — hold-flush expects HoldError; inspect-preserved-state expects observed Ready
- [file, verified] `examples/manifest.toml:720-723`
- [measurement, verified] `target/release/boon_cli run examples/flush_error_propagation.bn --scenario examples/flush_error_propagation.scn -> 'invalid executable local bindings ... incompatible with checked declaration type'`
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:403` — review 'missing': FLUSH may-flush bit, HOLD initialisers that may flush
- [file, verified] `~/repos/boon/docs/language/FLUSH.md:103-134` — original: FLUSHED[value] propagates to a boundary; no HOLD rule

```boon
hold_state: Ready |> HOLD hold_state {
    fail_hold |> THEN { FLUSH { HoldError } |> WHEN { __ => ShouldNotRun } }
    recover_hold |> THEN { Recovered }
}
hold_boundary: hold_state
-- scenario today: after fail_hold, hold_boundary = HoldError, but a later
-- `inspect |> THEN { hold_state }` reads Ready. Plan: undefined.
```

**Proposed plan change.** Add a FLUSH bullet to §4.3 'Change rules' and to the P0 spec list. Proposed text: 'FLUSH inside a copy body or HOLD update is a SKIP for that update: the HOLD keeps its state and does not fire. The error is delivered only at the enclosing FUNCTION/BLOCK/field boundary of the copy body itself. FLUSH in a HOLD initializer or a LATEST starting arm is an error. FLUSH in a List/map new: body flushes the whole list value at the nearest boundary. A FLUSHed value is never persisted.' Then either adjust flush_error_propagation.scn (hold_boundary stays Ready) or ask the owner for option (b).

**Refuter correction (high tier, medium confidence).** Retitle to 'FLUSH × change model (D30/D32) is unspecified'. Drop the claim that the example contradicts the plan, and drop owner option (a) as written, since it conflicts with D29. Proposed P0 text: 'A FLUSH inside a HOLD update lands at the enclosing field boundary (D29). The HOLD's state is unchanged and not persisted (persistence plan, FLUSH atomicity). The field shows the error value and fires once as an update (or: does not fire; owner choice). The HOLD binder always reads the stored state. FLUSH in a HOLD initializer or a LATEST starting arm is an error.' Keep the owner question, narrowed to (i) whether the field's error value fires and counts as its last value under D30, and (ii) whether the binder and field views may differ. Keep flush_error_propagation.scn as the O1/O2 fixture.

**Owner question.** What does FLUSH inside a HOLD update do under D30/D32/D33?
- (a) acts as SKIP for the state; the error surfaces only at the copy body's own boundary
- (b) writes the error into the HOLD (it fires, is persisted, and the HOLD's type includes the error tag)
- (c) keeps today's documented split: the field shows the error, storage keeps the old state; needs a separate 'field value vs HOLD state' concept
Recommendation: (a): it keeps 'HOLD takes every value' true for real values, keeps errors out of D12 storage, and needs no new concept. Option (c) contradicts 'every value keeps its last value' for the HOLD binder.

---

### EVI-006 [medium] The engine selector misses child-program and migration-stage compiles, so engine=next runs in S1 and P5 compile with both engines

*Decisions: D1, D12; Plan: §4.8 Engine selection; §8 S1, P5; §5.2 Gate coupling; Sources: E345; verdict: confirmed; severity high -> medium after refutation*

The plan puts BOON_COMPILER_ENGINE 'only in the shared compile service (playground preview and dev lanes)' and compile-bench, and ports program_runtime only in P7 (plan:690-701). Two playground compile paths never go through that service. (1) Child programs: ProgramCompileWorker ('boon-program-compile') calls boon_program_runtime::compile_program_artifact, which calls boon_compiler::compile_sealed_machine_plan directly. (2) Migration stages: Preview and Activate call compile_migration_stage on the preview main loop. So the S1 'shadow counter-dev native run with engine=next' compiles counter_migration's stages with the old engine, because counter-dev switches between counter and counter_migration (plan:795-797). The P5 exit 'native preflight runs with next pass the product gates' passes persons-pro while its child compiles (starter_source, the 4 ms p95 budget) and migration stages still run on the old engine. The process.md review raised exactly this ('does not reach every compile path'). Its fix was a process-global choice in the facade, and the plan narrowed it instead. COMPILER_ID is also a `const` today, and persons_pro stores `compiled.compiler` in a HOLD, so 'COMPILER_ID includes the engine' changes a value Boon code can see and that D12 persists.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:690-701` — selector lives only in shared compile service; program_runtime ported at P7
- [file, verified] `crates/boon_native_playground/src/compile.rs:21,228,324` — boon-program-compile worker calls compile_program_artifact
- [file, verified] `crates/boon_program_runtime/src/program_core.rs:482-517,420-425` — calls compile_sealed_machine_plan directly; rejects stored artifacts whose compiler_id differs from the host const
- [file, verified] `crates/boon_native_playground/src/preview.rs:3332,3356,4144-4145,4568` — migration Preview/Activate and child compiles outside the compile service
- [file, verified] `crates/boon_compiler/src/lib.rs:49` — pub const COMPILER_ID
- [file, verified] `examples/persons_pro/RUN.bn:407-410,439` — HOLD published_compiler stores compiled.compiler
- [file, verified] `docs/plans/compiler_rewrite_notes/process.md:520` — review issue and fix: one process-global engine choice read in the facade
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/claim_verification.md:66` — C5 caveat listing the extra compile triggers

**Proposed plan change.** Replace the §4.8 bullet with: 'The engine is one process-global choice, read once by the boon_compiler facade and carried in Config. Every compile entry point honours it from P5 on: the shared compile service, compile_program_artifact (child programs), compile_migration_stage (Preview/Activate/TEST) and distributed packages. COMPILER_ID becomes compiler_id(engine). Every report records the count of compiles per engine and fails if the counts are mixed.' Add to the S1 exit: 'the counter_migration stages compile with next'. Add to the P5 exit: 'the persons-pro child-compile p95 is measured on next'. Add to O4's divergence list that persons_pro's published_compiler value differs by engine by design.

**Refuter correction (high tier, high confidence).** Lower to medium. The proposed text is good. Add that the P5 compile service must either absorb ProgramCompileWorker and compile_migration_stage, or the facade reads the process-global engine. Add to the S1 exit that counter_migration stages compile with next, and to the P5 exit that the persons-pro child-compile p95 is measured on next. Record persons_pro.published_compiler as an expected O4 divergence.

---

### EVI-007 [medium] The old-engine compatibility rule keeps the new-form examples off main until P7, but the P3b, P4 and P5 exits need them

*Decisions: D1, D17, D34, D36; Plan: §5.1 compatibility rule; §8 P3a/P3b/P4/P5 exits; Sources: E345; verdict: confirmed; severity high -> medium after refutation*

§5.1 says a migration lands on main only if the old engine still passes that example's gate. 'A change the old engine cannot handle lands with the P7 switch.' Yet the P3b exit requires every tracked source to check with 0 diagnostics on the new checker, the P4 exit requires O2, and the P5 exit requires native preflight with next to pass the product gates, NovyWave included. NovyWave's core data path needs D17 List/replace_all, D34 removal of request_fingerprint and D36 Http/get/Http/send. None of these exist in today's shared catalog. boon_effect_schema requires request_fingerprint on the Wellen effects, and neither boon_effect_schema nor boon_typecheck mentions Http/get, Http/send or replace_all. boon_effect_schema is shared by both engines, and P2b generates the new catalog from it, so the new catalog cannot change there without changing the old engine. Neither the plan nor the notes say where the new-form sources live between P3b and P7. The plan_draft_critique 'hidden circular dependency' item (audit/plan_draft_critique.md:278) is therefore only moved, not fixed. M3 recorded the narrower P3a replace_all case.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:766-770` — compatibility rule
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:985-987,1016-1020,1027` — P3b, P4 and P5 exits
- [file, verified] `crates/boon_effect_schema/src/lib.rs:494,504,560,578,646,661` — request_fingerprint is a required Wellen field
- [measurement, verified] `rg -n 'Http/get|Http/send|replace_all' crates/boon_effect_schema/src crates/boon_typecheck/src/lib.rs -> no matches`
- [file, verified] `examples/novywave/RUN.bn:80-92` — request_fingerprint in a live Wellen call
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/plan_draft_critique.md:278,286`

**Proposed plan change.** Add a subsection to §5.1, 'Where migrated sources live before P7': (a) a per-example `next` overlay (e.g. manifest key `next_source`, or examples/<name>/next/) that engine=next preflight, O2 and P3b-P6 use, promoted in the P7 change set; or (b) a versioned catalog, where boon_effect_schema carries both the old and new entries (request_fingerprint optional, Http/request beside Http/get/send, List/replace_all) until P8, so that more migrations pass on the old engine. State that O4 runs on the pre-overlay revision. Put the question to the owner, recommending (a), because (b) changes the old engine's catalog and so its gates.

**Refuter correction (high tier, high confidence).** Lower to medium. Keep the proposed '§5.1 Where migrated sources live before P7' subsection and the owner question; recommendation (a), a next overlay consumed by engine=next runs, is sound. Also fix the internal contradiction in P3a: move 'List/replace_all rewrites' (and any D34/D36 rewrites) out of the list that 'follows the old-engine compatibility rule', or state that they go to the overlay. State that O4 compares on the pre-overlay revision, consistent with plan:769-770.

**Owner question.** Where do example sources that only the new engine accepts live between P3b and P7?
- (a) next-overlay sources used by engine=next runs, promoted at P7
- (b) dual-entry shared catalog until P8
- (c) relax the P3b-P5 exits to 'sources the old engine also accepts'
Recommendation: (a): it keeps the old gates untouched and gives O2 and the P5 preflight real inputs.

---

### EVI-008 [medium] D9's ban on collections in HOLD is trivially bypassed by LATEST and sticky values, which D12 persists; NovyWave keeps HierarchyPage[rows: LIST] in a LATEST

*Decisions: D9, D12, D15, D17, D30; Plan: §1 D9, D12, D17; §4.3 Homogeneity/Cycles; §5.1 rows 753/756; §8 S4; Sources: E345; verdict: confirmed*

The owner's words were 'you cannot pass big things like MAP or LIST into HOLD at all', and D9 bans LIST/SET/MAP and records containing them as HOLD state. Under D15/D30, a LATEST with a starting arm keeps its last value, D12 persists 'LATEST current values' and inferred last values, and §4.3 homogeneity rules say nothing about collections in LATEST. NovyWave's real_hierarchy_page_result is exactly such a LATEST, `LATEST { NotStarted, open |> WHEN { WaveformOpened => Wellen/hierarchy_page(...) } }`, holding HierarchyPage[rows: LIST]. The rule the plan writes therefore does not force the §5.1 'Collections in HOLD → collection authorities' and D17 'effect-returned lists → List/replace_all' rewrites: the author can keep the LATEST, or move a HOLD's list into a LATEST, and pass the checker. Persisted lists then exist outside collection authorities, which is what D9 and D17 were meant to prevent.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/owner_decisions_d1_d13.md:32-35` — owner quote item 9
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:78,81,84,86,753,756`
- [file, verified] `examples/novywave/RUN.bn:80-92,95-97`

```boon
page: LATEST {
    NotStarted
    opened |> WHEN { Opened => Wellen/hierarchy_page(artifact: opened.artifact, offset: 0, limit: 256), __ => SKIP }
}
-- plan as written: legal and persisted (D12), although `LIST {} |> HOLD rows {...}` is rejected (D9).
```

**Proposed plan change.** Change D9's implementing rule in §4.3 to: 'No stored value may be or contain a LIST, SET or MAP: HOLD state, a LATEST current value, or an inferred last value that D12 stores. Collections live only in collection authorities (D17).' Add 'LATEST/last-value collections' to the S4 census. Alternatively, ask the owner whether LATEST may hold collections; D17 would then be optional.

**Refuter correction (low tier, high confidence).** None needed to severity; medium is appropriate since this is a persistence/architecture-consistency gap, not a cycle-safety hole (D9 already blocks LATEST from being a cycle-closing device regardless of collections, per line 417-419's 'Nothing else closes a cycle: not LATEST'). The owner_question and proposed D9-rule rewording are well-targeted.

**Owner question.** Does the 'no LIST/MAP in HOLD' rule also cover LATEST current values and persisted last values?
- (a) yes: every stored value; collections only in authorities
- (b) no: only HOLD; LATEST may hold collections
Recommendation: (a), because that matches the owner's stated intent ('big things' out of state) and makes D17 the one way to hold effect-returned lists.

---

### EVI-009 [medium] R4's element identity uses an 'instance path', but Phase A builds instances only for stateful definitions, so repeated calls of a stateless view helper collide

*Decisions: D13, D20; Plan: §4.5 Phase A, R4; §4.6 Identity; §8 S3; Sources: E345; verdict: confirmed*

R4 and §4.6 define element identity as construction site key + instance path + row key, and give hand-built elements 'the identity of their literal site'. Phase A builds 'an instance tree only for stateful definitions, one instance per static call path' (plan:534-535). Most view helpers are stateless (TodoMVC dividers, labels, theme-styled wrappers). With only stateful instances, `LIST { divider(), title(), divider() }` gives both divider elements the same site key and instance path, and so the same identity. That breaks retained-state routing (focus, caret, scroll) and reconcile-by-identity, the same way A2's 'one value placed twice' case does, but here the programs are ordinary and contain no copies. The adopted lowering note defined ids as 'unique per (site, static call path)' over all inlined calls. The lowering.md review warned that inlined copies under the same parent collide. The plan does not carry either rule over.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:534-535,609,632-633,639`
- [file, verified] `docs/plans/compiler_rewrite_notes/lowering_alternative.md:23,67` — ids from stable site keys and call-path keys; unique per (site, static call path)
- [file, verified] `docs/plans/compiler_rewrite_notes/lowering.md:410` — review: inline policy makes node ids collide (divider() twice under one parent)

**Proposed plan change.** In §4.6 Identity, write: 'The instance path is the full static call path of element-construction calls, stateless FUNCTIONs included, and is independent of Phase A's stateful instance tree. Two calls of one function under one parent get distinct identities (call-site route plus ordinal among siblings with the same site).' Add to the S3 exit: 'repeated stateless element helpers under one parent get distinct identities, stable under an edit to a sibling definition'.

**Refuter correction (low tier, medium confidence).** Keep as an actionable finding but reframe as a wording/cross-reference gap rather than a proven runtime bug: the plan should state explicitly that 4.6's 'instance path' is Phase B's static-call-path key (lowering_alternative.md's 'unique per site, static call path'), not Phase A's stateful-only instance tree, to remove the ambiguity for implementers. Confidence is medium rather than high because the adopted lowering note's own rule, if carried over as intended, likely already prevents the concrete collision in the cited snippet.

---

### EVI-010 [medium] R-RENDER covers glow, shadows and family, but D22's rule also covers the Scene lights and geometry that the themes build and no renderer reads

*Decisions: D22, D13; Plan: §1 D22; §8 R-RENDER, S4; Sources: E345; verdict: confirmed*

D22 says keys the themes set that no renderer draws get renderer support, and ends its list with '…'. R-RENDER (1-2 weeks) implements only glow, material shadows and font family. TodoMVC also passes `lights: Theme/lights()` and `geometry: Theme/geometry()` to Scene/new, and every theme builds Light/directional, Light/ambient and Light/spot values. `lights` has no reader in boon_document, boon_native_gpu, boon_plan_executor, boon_runtime or boon_web_host. Supporting scene lighting in the native GPU and web renderers is a much larger item than glow, and the closed contracts (D13) must include Scene/new's lights and geometry either way. The examples.md review raised the inconsistency: the draft deletes some unread keys and keeps lights, geometry, spring_range_* and move_*. The plan kept the D22 rule without the list. Note that boon_document runtime.rs:5358-5360 already lowers 'shadows', 'glow' and 'spring_range' as style keys, so 'no renderer draws' needs to be checked per key.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:91,996-999`
- [file, verified] `examples/todo_mvc_physical/RUN.bn:325-332`
- [file, verified] `examples/todo_mvc_physical/Theme/Theme.bn:40-46; Theme/Professional.bn:373-387` — Light/directional, ambient, spot
- [measurement, verified] `rg -l -w lights crates/boon_document crates/boon_native_gpu crates/boon_plan_executor crates/boon_runtime crates/boon_web_host -> no files`
- [file, verified] `crates/boon_document/src/runtime.rs:5358-5360` — shadows/glow/spring_range lowered as style keys
- [file, verified] `docs/plans/compiler_rewrite_notes/examples.md:483`

**Proposed plan change.** Change the R-RENDER scope to: 'The exhaustive list of keys the themes set that no renderer consumes, produced by the S4 closed-contract census (incl. Scene lights/geometry, spring_range_*, move_*), each marked implement, contract-only or owner-declined'. Size R-RENDER after S4, and put Scene lighting to the owner as a separate question.

**Refuter correction (low tier, high confidence).** None; medium severity and the proposed owner_question are appropriate. This is a genuine scope/estimate gap: D22's literal wording ('...') would require Scene lights/geometry too, but R-RENDER's estimate and exit criteria don't budget for it.

**Owner question.** Does D22 require renderer support for Scene lights (directional/ambient/spot) and geometry, or only for the style keys glow, shadows and family?
- (a) all unread theme data, lighting included (size L)
- (b) style keys only; lights/geometry stay in the contract, unrendered, and are flagged
- (c) lights/geometry removed from the themes
Recommendation: Decide after S4 lists the keys. Default to (b), so R-RENDER stays 1-2 weeks, and record lighting as later work.

---

### EVI-011 [medium] The preamble overstates review coverage: the lowering design the plan adopts and the change-model note were never adversarially reviewed

*Decisions: D2; Plan: Preamble; §8 P0; Sources: E345; verdict: confirmed*

The preamble claims 'a 12-agent design round, each design adversarially reviewed'. Of the 10 design notes, only 6 carry an adversarial review (spec, checker, lowering, frontend_session, examples, process). Four do not: lowering_alternative.md, which the notes README says 'the plan follows', change_and_effects.md, the basis of D30-D36 and the source of the defaults the plan carried (microsteps, cancellation, Log/* anywhere), process_alternative.md and original_change_model.md. The reviewed lowering design (lowering.md, the V12 delta) is the one the plan did not adopt. Its review findings, such as node-id collisions under inlining, runtime call-path cost and the never-consed list, were never re-run against the design actually chosen. Separately, '16-agent audit … plus external research' double-counts: audit/ has 12 subsystem audits plus 4 research reports.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:11-15`
- [measurement, verified] `grep -c -E '^# Adversarial review|major_issues' docs/plans/compiler_rewrite_notes/*.md -> 2 for spec/checker/lowering/frontend_session/examples/process; 0 for lowering_alternative, change_and_effects, process_alternative, original_change_model`
- [file, verified] `docs/plans/compiler_rewrite_notes/README.md:14-15` — lowering_alternative: 'the plan follows this one'
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/README.md` — 12 audit files plus dod/incremental/inference/verification research

**Proposed plan change.** Correct the preamble to: 'a 12-file subsystem audit plus 4 external research reports; 10 adversarial claim checks; a design round of 8 designs, 6 of them adversarially reviewed (the adopted lowering variant and the change-model note were not); three critiques of an earlier draft'. Add a P0 item: an adversarial review of lowering_alternative.md as amended by R4/R8, and of the change_and_effects.md defaults the plan carries, before the spike reports are signed off.

**Refuter correction (low tier, high confidence).** Minor: the exact audit-file breakdown ('12 subsystem audits plus 4 research reports') is approximately but not precisely verified against the 19 files in audit/ (16 non-README .md files, of which claim_verification.md, plan_draft_critique.md and owner_decisions_d1_d13.md are not subsystem audits, leaving 13, not 12, plus 4 research reports: dod_research, incremental_research, inference_research, verification_research). This is a small counting slip that doesn't affect the core, well-supported claim -- the proposed preamble correction's exact numbers should be re-tallied before adoption.

---

### EVI-012 [low] Model C leftovers: L2's 'Log/* allowed anywhere' is a third effect class that D31 does not have, and several effectful builtins are unclassified

*Decisions: D31, D32; Plan: §1 D31; §11 L2; §8 P2b; Sources: E345; verdict: confirmed*

The L2 default adopts change_and_effects' Model C recommendation 'Log/* allowed anywhere and logs every update of its argument'. D31 defines exactly two classes: a query is safe to re-run, and a command acts on the outside world and is allowed only in copy contexts. Logging acts on the outside world and is not a query, so as written it is a command that is 'allowed anywhere', which D31 forbids. The notes' defaults 'THEN over a HOLD fed by command results is a warning' and 'Superseded query runs are cancelled (today's behaviour)' were dropped or carried over silently. Timer/interval, Stream/pulses and Build/succeed|fail have no class. BUILD.bn uses Log/info and Build/* in WHEN arms and File/write_text as a live pipeline sink, so these classes decide whether it migrates mechanically.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:100,1203`
- [file, verified] `docs/plans/compiler_rewrite_notes/change_and_effects.md:396,410,412`
- [file, verified] `examples/novywave/BUILD.bn:15-50`

**Proposed plan change.** In D31's implementing text (§4.3/P2b), add a third catalog class 'observer (Log/*)': allowed in live and copy contexts, runs on every update, never on start/restore, never persisted. Alternatively, classify Log as a command and state the BUILD.bn fix. List Timer/interval, Stream/*, Build/* with their classes in the P2b catalog deliverable.

**Refuter correction (low tier, medium confidence).** Low severity is reasonable since this affects a small builtin surface and is fixable with a short catalog addendum, as the finding itself proposes. Confidence is medium rather than high because it's plausible (but not evidenced in the plan text I read) that the owner's actual D31 answer implicitly carried the Log/* exception from change_and_effects.md's Model C recommendation without it being written back into D31's prose -- the plan doesn't attribute the Log/* default to an owner decision, only to a note default, which the finding correctly flags via its own refutation_hints check.

---

### EVI-013 [low] The '340 ms after deleting incidental steps' figure is the kernel prepare+solve timer, not a subtraction; the conclusion holds with a 290-355 ms range

*Plan: 3.1 last bullet; 3.4 Typecheck orchestration row; Sources: E12; verdict: confirmed*

The literal source of 340 is kernel_solver.md:73: 'prepare 60 ms + solve 280 ms = ~340 ms'. That counts prepare_kernel_project_projection as essential, although 3.4 and representations.md call it orchestration that is 'perhaps half incidental'. Checked against profile.md for the diagnostics lane: incidental work is only 9.2% (narrow union) to 14.0% (broad union) of the TodoMVC diagnostics window. 390 x (1-0.140) = 335 ms and 390 x (1-0.092) = 354 ms. Removing all non-solver typecheck work as well (solver frames are 84.3% of typecheck, typecheck is 88.7% of the window) leaves 390 x 0.887 x 0.843 = about 292 ms. The conclusion stands: at least 3.9x over the 75 ms budget. The 3.4 range '30-70% outside the solver' does not hold in the lane that has the budget: it is 15.7% for TodoMVC diagnostics, 28.9% for TodoMVC verified, 56.6% for NovyWave verified and 85.5% for counter verified.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/kernel_solver.md:73`
- [source, from notes] `docs/plans/compiler_rewrite_notes/review/measurements/profile.md §5 table (incidental union narrow 9.2 / broad 14.0, TodoMVC diagnostics) and §7 row 'Typecheck orchestration'` — measurements taken from the confirmed profile, not re-run; the arithmetic is mine
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/representations.md incidental_work 'Syntax-to-PreparedOwner projection' (17-19 ms TodoMVC, 'perhaps half incidental')`

**Proposed plan change.** 3.1: 'In the diagnostics lane incidental work is only 9-14% (profile.md). Deleting all of it, plus every non-solver step of typecheck, still leaves 290-355 ms of kernel solving on TodoMVC, against a 75 ms budget.' 3.4 row: 'Outside the solver: 16% (TodoMVC diagnostics) to 57% (NovyWave verified) and 85% (counter verified).'

**Refuter correction (low tier, medium confidence).** None to severity (low is right, since the finding's own conclusion is that the plan's headline conclusion -- '3.9x over the 75 ms budget' -- still holds). Flag for the owner/measurement agent that the profile.md incidental-work percentages (9.2%/14.0%) should be independently re-measured before the corrected range is adopted verbatim, since this reviewer could not re-run them under the tool restrictions in force.

---

### EVI-014 [low] 'About 62 program representations' mixes representations with proofs, indexes, caches and exports; about 40 are whole-program representations, and the interner and type-encoding lists double-count

*Plan: 3.3; Sources: E12; verdict: confirmed*

The census entries R01-R62 in representations.md count structures of mixed kinds. They include: caches (R12); proof or digest layers (R08, R17, R28, R32, R43, R50-R53, R60, 10 entries); indexes, catalogs and stores (R06, R07, R13, R14, R16, R18, R37); solver working state (R25); a whole-plan clone operation (R58); a CLI export (R62); a report DTO (R36); the source bundle (R01); and the runtime artifact (R61). Meanwhile R14, R15, R31 ('in 6 wrappers'), R48 (two graphs) and R09 (three structs) each bundle several structures. With the rule 'a structure holding per-entity rows for the whole program, built by converting another one and consumed later', I count 38-40 whole-program representations, plus 10 proof layers, 7 indexes/catalogs/stores and 7 others, which sums to 62. I spot-checked 13 entries and all are at their cited lines: AstStatement syntax:1558, AstExpr :1596, ProjectSyntaxSnapshot parser:1526, TypeTermArena term.rs:601, ComponentProgram program.rs:625, KernelCheckedLinkLayout link.rs:492, CheckedProgramFields boon_checked:2622, OutNet out_net.rs:279, CanonicalProgramCoreV2 program_core.rs:10, ErasedProgram boon_ir:31, PlanRowExpressionArena boon_plan:8866, DocumentPlan document.rs:53, MachinePlan boon_plan:5468. Two sub-counts are wrong. FrozenTypeStore is `pub struct FrozenTypeStore(TypeTermArena)` (term.rs:639), a newtype rather than a second encoding. The type-encoding list also omits FlowType (boon_checked:624), PlanValueType (boon_plan:5853) and, arguably, boon_effect_schema::ValueType (imported at kernel text.rs:10). 'Kernel ProjectTextSnapshot' and 'boon_contract text catalog' are the same interner: kernel text.rs:8 imports boon_contract::{PackedTextCatalogBuilder, ProjectTextSnapshot}, defined at boon_contract/src/text_catalog.rs:302/461. That leaves 3 interners: SharedParserSymbols (boon_syntax:1419), the boon_contract catalog, and DocumentPlan names (document.rs:56). The point of 3.3 stands; the numbers should say what they count.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/representations.md:61, 91-152`
- [file, verified] `crates/boon_compiler_kernel/src/term.rs:601, 639; crates/boon_compiler_kernel/src/text.rs:8-10; crates/boon_contract/src/text_catalog.rs:108, 302, 461; crates/boon_syntax/src/lib.rs:1419; crates/boon_plan/src/document.rs:56`

**Proposed plan change.** 3.3: 'About 40 whole-program representations (62 catalogued structures, including 10 proof layers and 14 indexes, catalogs, caches and exports), at least 27 conversions, five or six type encodings (rich Type/FlowType, kernel TypeTerm, ArtifactTypeTermV1, DataTypePlan, PlanValueType), three string interners, three call-expansion passes and at least six ID spaces.'

**Refuter correction (low tier, high confidence).** None; low severity is appropriate for what is essentially a bookkeeping/precision correction to an already-accepted headline number.

---
