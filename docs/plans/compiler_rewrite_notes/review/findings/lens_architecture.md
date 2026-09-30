# Lens 3: architecture and performance: all findings

Generated from the verified finder output for `../REVIEW.md`. Severity is the severity after refutation; each finding keeps its refuter's corrections.

### ARC-001 [high] R7+R8 replace the executor's push scheduler; they are not a 'delta' and are unsized

*Decisions: D21, D32, D33; Plan: §4.5 R7, R8; §8 S5; Sources: A2; verdict: confirmed; severity critical -> high after refutation*

Today's executor commits every state write immediately and recursively routes dependents inside the same trigger frame (push scheduling), orders same-trigger update ops producer-before-consumer precisely so later ops read the new value, and gates every downstream transition, delta emission and persistence write on a value-equality check. D21 (snapshot reads), D32 (every write fires), D33 (piped-input reset) and 'microsteps each reading its own committed snapshot' invert all four mechanisms. That is the core dispatch of a 37,874-line file (12 route_state_transition call sites, 26 state-setter call sites, 19 `if changed` gates, 59 functions in the trigger region), not a listed delta. The plan sizes it as one row in a table and puts it under 'Runtime behaviour + format'.

Evidence:
- [file, verified] `crates/boon_plan_executor/src/machine.rs:17769-17877` — route_state_transition: recursive, synchronous routing of updates/mutations/pulses for a state inside the current trigger frame (push scheduler; no tick phases).
- [file, verified] `crates/boon_plan_executor/src/machine.rs:19681-19795` — execute_update commits via set_root_state/set_row_authority_field immediately and calls route_state_transition only `if changed`.
- [file, verified] `crates/boon_plan_executor/src/machine.rs:21769-21783` — set_root_state returns false (no dirty, no Delta::SetValue, no dependents) when the new value equals the current one: today's HOLD equality dedup.
- [file, verified] `crates/boon_plan_executor/src/machine.rs:21910-21924; 17529-17531` — set_row_field and set_transient_effect_result also early-return on equality (row HOLDs and effect results are deduplicated too, conflicting with D32 'an effect-result HOLD fires on every completion').
- [file, verified] `crates/boon_plan_executor/src/machine.rs:21575-21600` — sort_update_ops_by_dependencies orders same-trigger ops so consumers run after producers and read the just-committed value: this is the mechanism D21 removes.
- [file, verified] `docs/architecture/RUNTIME_MODEL.md:76-93` — The documented phased tick ('No stateful value should commit in the middle of evaluation') is not what the executor implements; D21 is a scheduler change, not a bug fix.
- [measurement, verified] `cd scratchpad/A2 && target/release/boon_cli run p1_then_over_hold.bn.txt --scenario p1_then_over_hold.scn ; same for p3_source_repeat → both 'pass'` — Step 'one-again-same-value': value stays 1, fires stays 1, presses 2; p3: last_fires and latest_fires stay 1 on an equal write. Confirms the equality dedup on HOLD and LATEST today; under D32 all four expectations flip.
- [measurement, verified] `rg -c 'route_state_transition\(' / 'set_root_state\(|set_row_field\(|...' / 'if changed' in machine.rs → 12 / 26 / 19` — Call-site counts for the scheduler surface.

```boon
store: [
    set_one: SOURCE
    value: 0 |> HOLD value { set_one |> THEN { 1 } }
    fires: 0 |> HOLD fires { value |> THEN { fires + 1 } }
]
-- today (measured): second press → value 1, fires 1 (equal write deduplicated)
-- plan D32: second press → value 1, fires 2 (every write fires)
-- developer expectation under D30/D32: fires 2
```

**Proposed plan change.** Rewrite the R7/R8 rows of the 4.5 table as one item 'Scheduler v2' with its own sizing and a P0 spike (fold into S5): (a) a per-tick phased commit (write buffer + microstep loop) replacing recursive route_state_transition; (b) a `fires` notion separate from value change (see the R-DOC finding); (c) removal of the equality early-returns in set_root_state, set_row_field, set_activation_*, set_transient_effect_result and the 19 `if changed` gates, with Delta/persistence emission decided separately; (d) copy-vs-live evaluation as a per-op flag emitted by lowering (WHEN/THEN = Copy, WHILE = Live), not a plan-level semantics version. State in the plan that this is a rewrite of the executor's dispatch core and add it to the effort table.

**Refuter correction (high tier, high confidence).** The 'if changed' count is 18, not 19. Lower the severity to high, not critical, because the plan already has a re-baseline-from-spikes mechanism and a risk row. The gap is that no spike covers R8. Better change: widen S5 from 'snapshot semantics' to 'Scheduler v2 (D21+D30+D32+D33)', with exit criteria covering a phased commit, fires-vs-changed, removal of the equality gates, and a size estimate. Also add a runtime-scheduler row to the effort table, instead of leaving R8/R9 inside P4's lowering estimate (line 1008).

---

### ARC-002 [medium] 'Old-engine plans keep today's behaviour through a plan semantics version until P8' means two schedulers in one executor that is 900 lines under its cap; R7 already breaks the policy

*Decisions: D21, D30, D32; Plan: §4.5 R7, R8; §12.1 'Gate churn'; Sources: A2; verdict: confirmed; severity critical -> medium after refutation*

Keeping today's change model for old-engine plans while the new engine's plans run D30-D33 requires both scheduling regimes to coexist in machine.rs from P0/P1 to P8. The executor crate is capped at 42,000 lines and the plan itself records it at 41,105. Worse, the policy is already inconsistent: R7 (snapshot reads) is 'landed early on the shared executor' unconditionally, so old-engine plans and the old-engine handoff gates run under a hybrid (new snapshot reads, old change model) that no document specifies, and every gate scenario is re-triaged twice (after R7, again at P7). 12.1 claims old gates change only in P0, P5 and P8.

Evidence:
- [file, verified] `crates/xtask/src/architecture.rs:10` — `RUNTIME_EXECUTOR_RUST_CAP: usize = 42_000`.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:899-900` — Runtime headroom recorded as 41,105/42,000 with compensating deletions planned only for R1-R7, not R8/R9.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:611-612 (R7) vs 613 (R8) vs 1249 (12.1)` — R7 unconditional on the shared executor with old-engine gates re-run; R8 versioned per plan; 12.1 says old gates change only in P0/P5/P8.
- [file, verified] `crates/boon_plan_executor/src/machine.rs (rg 'PLAN_MAJOR_VERSION|semantics' → no behaviour switch)` — No versioned-behaviour switch exists in the executor today; it would be new plumbing through the trigger region.
- [measurement, verified] `rg --files -g '*.scn' examples crates tests | wc -l → 55; manifest gates: architecture, counter-dev, todomvc-physical, cells, novywave, persons-pro, negative` — Scope of scenarios and gates that would be re-triaged.

**Proposed plan change.** Replace the sentence with an explicit policy and owner choice. Option A (recommended): one scheduler; the in-tree old engine's plans run on the new scheduler via a converter that marks every WHEN op Live and every HOLD as today's shape; behavioural divergences from today are frozen once in tests/compiler/divergences.toml; the pinned worktree build from 5.2 stays the behaviour baseline for today's semantics. Option B: keep two regimes but only until P4 exit, with a line-cap exemption recorded in P0 and R7 moved under the same version switch as R8. Option C: land R7-R9 only for engine=next plans and never on old plans (old-engine gates then measure old semantics exactly). In all cases correct 12.1's 'old gates change only in P0, P5, P8'.

**Refuter correction (high tier, medium confidence).** Retitle: 'R7/R8 policy contradicts 12.1 and P4; the semantics version is unspecified'. Drop the 'two schedulers' framing. Proposed text change: (1) in 12.1, add 'and after S5 (D21 re-triage)' to the gate-change list; (2) delete 'if scheduled' from P4 line 1008, or make R7's timing consistent; (3) spell out R8's version switch as 'new plans carry copy/live op kinds; the equality dedup and piped-input reset are gated by a plan semantics flag', with its line cost; (4) extend line 900's compensating deletions to R8/R9. Option A of the owner question is still a reasonable recommendation.

**Owner question.** Should the shared executor keep today's change/snapshot semantics for old-engine plans until P8, or run one scheduler and accept listed behaviour divergences in the old-engine differential?
- A: one scheduler + converter + divergences.toml (old engine stops being a behaviour oracle for the change model; the pinned worktree build is)
- B: two regimes behind a plan semantics version until P4 exit, line-cap exemption
- C: R7-R9 only for engine=next plans; old plans untouched until P8
Recommendation: A. The old engine is a compiler oracle, not a scheduler oracle; the behaviour baseline (5.2 item 1) already pins today's semantics.

---

### ARC-003 [medium] R-DOC contradicts D32: 'every write fires' vs 'the reconciler visits only subtrees whose values changed'; the plan needs a fire/changed split and a shared Value representation

*Decisions: D13, D32; Plan: §4.6 Representation/Recomputation; §4.5 R4, R8; Sources: A2; verdict: confirmed; severity high -> medium after refutation*

Under D13+D32 an element record is a derived value that 'fires whenever one of its inputs fires', equal or not. Either the executor emits the equal record to the renderer (then the reconciler diffs it every time, contradicting 'nothing is diffed per frame') or it does not (then a THEN over an element record does not fire, contradicting D32). Today the document runtime is driven solely by Delta::SetValue, which is emitted only when a value changed; there is no second channel. In addition, today's Value is deep-copied (Vec/BTreeMap, no Arc), so 'structural sharing (Arc)' and cheap pointer equality require a new Value representation in boon_data, which every crate uses; the plan lists neither.

Evidence:
- [file, verified] `crates/boon_document/src/runtime.rs:1044-1070` — turn_affects_structure decides full re-evaluate+diff_frames vs retained patches purely from Delta::SetValue/InsertRow/RemoveRow.
- [file, verified] `crates/boon_plan_executor/src/machine.rs:21769-21781` — Delta::SetValue is pushed only after the equality early-return; 'fires' and 'changed' are the same thing today.
- [file, verified] `crates/boon_data/src/lib.rs:224-234` — Value::List(Vec<Value>), Value::Object(BTreeMap<String, Value>), tagged fields BTreeMap: no Arc, deep clone and deep compare on every write.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:629-636` — 4.6 'Representation … structural sharing (Arc)' and 'the reconciler visits only subtrees whose values changed, so nothing is diffed per frame'.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:98 (D32)` — 'a derived value fires (at most once per step) whenever one of its inputs fires'.
- [file, verified] `crates/boon_plan_executor/Cargo.toml:8-27; crates/boon_plan_executor/tests/*cross_target.rs` — Executor already builds and tests on wasm32; the new reconciler and scheduler must keep that (web host depends on boon_document and boon_runtime).

```boon
row_view: FUNCTION(todo) { Element/label(label: todo.title, style: theme.row) }
-- D32: a write to `theme` (equal value) fires every row's element record
-- 4.6: reconciler must visit only changed subtrees
-- needed rule: rows re-fire (scheduling) but emit no change (no reconcile)
```

**Proposed plan change.** Add to 4.5/4.6: 'The runtime has two channels. *Fires* schedules copy ops (THEN/WHEN bodies) and is raised on every write (D32). *Changed* is raised when the committed value differs from the previous one (Arc pointer equality first, structural equality second) and is the only channel the renderer, persistence and host deltas subscribe to. Element records reach the reconciler through *changed* only.' Add to R4 sizing: 'Value gains Arc-shared records/lists/tagged objects (boon_data) so element subtrees share structure and compare by pointer; this touches every Value consumer'. Add 'builds for wasm32' to the R-DOC and scheduler exit criteria.

**Refuter correction (high tier, medium confidence).** Reword from 'contradicts D32' to 'underspecified: which runtime consumers follow fires vs changed after D32'. State that the changed channel is runtime-internal (renderer, persistence, host deltas) and not visible to Boon programs, so D32 and its 'library function over HOLD' clause stay intact. Point the Arc/representation note at machine.rs:141/6963 Value/EvalValue and the boon_document values rather than boon_data. Keep the wasm32 exit criterion.

---

### ARC-004 [medium] Identity rule 'values copied or moved keep their identity' makes two placements of one element collide, and the key derivation for hot reload is unspecified

*Decisions: D13; Plan: §4.6 Identity, Hot reload; §8 S3; Sources: A2; verdict: confirmed; severity high -> medium after refutation*

With elements as first-class values, `LIST { divider, divider }` (or a spread copy `[...divider, style: s]` used twice) yields two children with one hidden identity under 4.6's rule. Today node ids are unique by construction (template node + instance path + row fragment) and the playground keeps focus/hover/pressed/scroll/text-input state keyed by that id string, so duplicate identities either drop one child or mis-route retained state. The plan also does not say how the construction-site key is derived; today's ids come from dense executable/list ids (row ids embed the dense ListId), which is exactly what shifts under unrelated edits.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:631-640` — 'Values copied or moved keep their identity'; 'Hand-built elements get the identity of their literal site'.
- [file, verified] `crates/boon_native_playground/src/runtime_view.rs:1433-1466` — retain_view_state keeps hovered/pressed/focused/text_drag/scroll_offsets/text_inputs keyed by node-id string, filtered by `frame.nodes.contains_key`.
- [file, verified] `crates/boon_document/src/runtime.rs:96, 3234, 3570` — Row node fragments are `row-{ListId}-{key}-{generation}` (dense ListId); materialize fragments embed dense ids.
- [file, verified] `docs/plans/compiler_rewrite_notes/lowering.md:210-217 (notes, not authority)` — An earlier session proposed definition-relative stable anchors (DefKey + constructor ordinal, SiteKey); the plan dropped the derivation without replacing it.

```boon
divider: Element/stripe(style: theme.rule)
panel: Element/column(items: LIST { divider, divider })
-- plan 4.6: both children keep divider's identity → collision
-- today: elements are not values; each call site is its own template node
-- expected: two distinct retained nodes
```

**Proposed plan change.** Specify in 4.6: 'Identity = (construction-site key, instance path, row key, placement ordinal among siblings that share the first three). Construction-site key = hash(definition path, constructor ordinal within that definition); instance path = stable call-site keys (caller definition, callee, ordinal among calls to that callee), never dense ids; row key = the collection's semantic row key, never ListId/generation.' Add to S3: a test with one element value placed twice and with a spread copy placed twice; and a hot-reload test that inserts a definition and a list before the edited one and asserts focus/scroll survive.

**Refuter correction (high tier, medium confidence).** Downgrade to medium, since S3 owns this explicitly. Replace the LIST example: a positional row key may already separate LIST items. The stronger example is one element value in two named slots of one parent, or a spread copy placed twice. Recast the proposal as S3 exit criteria rather than final 4.6 text: (a) the identity includes the placement path or a sibling ordinal; (b) the construction-site key derives from definition path plus constructor ordinal, never dense ids; (c) the listed duplicate-placement and insert-before hot-reload tests.

---

### ARC-005 [medium] S3's and R-DOC's exit criterion ('per-frame work and retained-update counts no worse than today') is not measured by any existing gate

*Plan: §4.6 Performance gate; §8 S3; §12.1 row 1; Sources: A2; verdict: confirmed; severity high -> medium after refutation*

The native handoff gates measure input-to-present p95/max, callback-to-host, switch acknowledgement and settled CPU. Document work counters (full_evaluation_count, retained_patch_count, subtree_reflow_count, patched_node_count) exist in boon_document but are not surfaced in playground reports or xtask verifiers, and 'per-frame work' has no definition. As written, the R-DOC gate cannot fail, and the 60 FPS gates can pass while the reconciler does N× more work hidden under the 16.7 ms budget on this machine.

Evidence:
- [file, verified] `docs/architecture/NATIVE_GPU_PIPELINE.md:360-367, 346-352` — Product budgets: callback-to-host p99 ≤1 ms; warm interaction/scroll p95 ≤16.7 ms, max ≤33.4 ms; switch ack; settled CPU <1%. No work-count budget.
- [file, verified] `crates/boon_document/src/lib.rs:3285-3289` — retained_patch_count, subtree_reflow_count, subtree_reflow_fallback_count, subtree_reflowed_node_count exist in the retained document.
- [measurement, verified] `rg -c 'full_evaluation_count|retained_patch_count|subtree_reflow_count|patched_node_count' crates/boon_native_playground/src crates/xtask/src → no matches` — Counters never reach reports.
- [file, verified] `docs/architecture/native_gpu_handoff_manifest.json gates[]` — Gates: architecture, counter-dev, todomvc-physical, cells, novywave, persons-pro, negative; none has a work-count field.

**Proposed plan change.** Define the R-DOC gate in 4.6 as: (1) the todomvc-physical, cells and novywave native gates run with engine=next stamped; (2) a new cheap-counter lane in the observer protocol publishing per-interaction 'reconciled node count', 'full rebuild count' and 'retained attribute patch count' (today: patched_node_count, full_evaluation_count, retained_patch_count); (3) a baseline of those counters captured on engine=old before S3 starts and stored under target/reports; (4) pass = counters ≤ baseline ×1.1 per interaction sample AND the p95/max budgets. Add the counter route to S3's deliverables so the spike report can quote it.

**Refuter correction (high tier, medium confidence).** Correct the evidence: patch_count and full_lowered already flow through observer.rs:383-384 into verify.rs:4734-4735 (profile-stage-breakdown). Proposed change: define the R-DOC gate as 'patches p95/max and full_lowers from profile-stage-breakdown, plus a reconciled-node count added to the same sample, captured on engine=old for todomvc-physical, cells and novywave before S3; pass = at most baseline x1.1 and the latency budgets'. Extend the existing stage sample rather than adding a new lane.

---

### ARC-006 [medium] R-DOC worst case is fan-out from root state read by every row (Cells selection, TodoMVC filter/theme, NovyWave selected row), which today is absorbed by retained scalar bindings

*Decisions: D13, D32; Plan: §4.6 Recomputation, Performance gate; §8 S3; Sources: A2; verdict: confirmed*

Today a scalar change that only affects constructor arguments patches retained bindings in place (no template re-evaluation); a structural change triggers a full evaluate + diff_frames. Under R-DOC each row's element record is a derived value; a root state read by every row (selected cell, filter, theme) fires every row's record per write (D32), and with deep-copy Values each is rebuilt and compared. S3 lists 'many rows', Cells and a NovyWave list but not this fan-out shape, and 4.6 has no equivalent of the leaf-level attribute patch.

Evidence:
- [file, verified] `crates/boon_document/src/runtime.rs:1022-1037, 1044-1070` — rebuild_transactional = full evaluate + diff_frames only when turn_affects_structure; otherwise retained patches.
- [file, verified] `crates/boon_data/src/lib.rs:224-234` — No Arc sharing in Value; row records are cloned and compared deeply.
- [file, verified] `docs/architecture/NATIVE_GPU_PIPELINE.md:369-378` — Cells gate requires a real cell click frame with new selection plus formula-bar text and a repeated-selection p95 ≤16.7 ms over 20 samples: exactly the fan-out case.
- [measurement, verified] `rg -c HOLD examples/novywave/RUN.bn → 75; wc -l examples/novywave/RUN.bn → 4949` — NovyWave state count for the fan-out estimate.

**Proposed plan change.** Add to S3 scope: 'a root state read by every row (Cells selected cell; TodoMVC filter and theme; NovyWave selected row) with ≥1,000 rows; report reconciled-node counts and deep-clone counts per write'. Add to 4.6 Recomputation: 'element attributes that depend on a scalar are recomputed as leaf updates on the retained node (today's retained scalar bindings), not by rebuilding the enclosing record; the reconciler receives attribute-level changes where the dataflow can prove the record shape is unchanged'.

**Refuter correction (low tier, medium confidence).** Downgrade the framing from 'S3 doesn't cover this' (partially untrue given the Cells gate) to 'the R-DOC design section (§4.6) never states how the reconciler avoids O(rows) rebuild+compare when a root read fans out to every row's element record, even though an existing native gate (NATIVE_GPU_PIPELINE.md) already measures exactly this case for Cells.' Keep severity medium; the actionable ask (add a leaf-level attribute-patch design note to §4.6, and cross-reference the existing NATIVE_GPU_PIPELINE.md gate in S3's exit criteria) still stands.

---

### ARC-007 [medium] O4 'documents compared by rendered frames, structurally, modulo ids' compares two runtimes, not two compilers, and the id/ordering canonicalisation is unspecified

*Plan: §7 O4; §4.5 last paragraph; §5.2 item 3; Sources: A2; verdict: confirmed*

After R-DOC the old engine's frames come from the template runtime and the new engine's from the reconciler; a frame diff then tests R-DOC against boon_document v11, which is useful but is not a compiler differential and cannot attribute a divergence. 'Modulo ids' also needs a rule: node ids embed dense ListId/generation and text-input nodes carry retained buffer/caret state; children order and style-map defaults must be canonicalised.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:619-621; 835 (O4)` — 'Documents are compared by rendered frames, structurally, modulo ids.'
- [file, verified] `crates/boon_document/src/runtime.rs:96, 3570; crates/boon_native_playground/src/runtime_view.rs:1445-1466` — Row node ids embed ListId/key/generation; text-input state lives outside the frame keyed by id.
- [file, verified] `crates/boon_document/src/render_scene.rs:1071, 1114` — lower_layout_frame_to_render_scene(_with_retained_keys): a lower level at which ids can be stripped and both representations meet.

**Proposed plan change.** Split O4 into O4a (dataflow: same executor, converted v11 vs v12 plans, exact) and O4b (documents: compare after lowering both to RenderScene with retained keys stripped and children in document order; text-input content excluded). State that O4b is an R-DOC acceptance test owned by S3/R-DOC, and that the compiler-side check for elements is O2 scenarios plus the P3a render diff (5.2 item 3).

**Refuter correction (low tier, medium confidence).** No severity change needed. Note in the write-up that §5.2 item 3 is a second place a render-frame diff is described, which the owner should reconcile with O4 rather than treating the finding as solely about O4.

---

### ARC-008 [medium] R8 (every write fires, follow-up microsteps) has no runtime-cost spike although the native 60 FPS gates and R-DOC's 'no worse than today' depend on per-tick work

*Decisions: D32, D30, D16; Plan: §4.5 R8, §8 S5, §12.1; Sources: A1; verdict: confirmed*

§12.1 covers D30-D36 only as a semantics risk (runtime scenarios seeded from change_probes). R8 removes HOLD's equal-value deduplication and adds follow-up microsteps within a tick, each reading its own committed snapshot (§4.5 R8; D32 'a derived value fires once per step'). On Cells (spreadsheet recalculation in plain Boon, D16) and NovyWave lists this can multiply the number of op evaluations per tick relative to today's dedup, and the R-DOC gate is stated only as 'per-frame work and retained-update counts no worse than today at 60 FPS' for the document side. S5 already instruments the executor for same-tick reads, so the cost measurement is cheap to add there, but no spike or gate names it.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:613` — R8 description: every write fires, microsteps, HOLD stops deduplicating
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1246` — §12.1 row for D30-D36 lists semantics mitigations only
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:649-651` — R-DOC gate: per-frame work no worse than today at 60 FPS
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:913` — S5 instruments the executor

**Proposed plan change.** Extend S5 (or add S7, 3 days, same executor instrumentation): count op evaluations, HOLD writes and microsteps per tick on the Cells, TodoMVC many-rows and NovyWave scenarios under (a) today's dedup and (b) D32 'every write fires'; exit: ≤1.5x today's per-tick op evaluations and the existing 60 FPS native gates unchanged; if exceeded, the owner decides between a library-level dedup idiom in the migrated examples and a scheduler change before P4. Add a §12.1 row 'R8 per-tick cost' pointing at it.

**Refuter correction (low tier, medium confidence).** None; severity and proposed remedy (extend S5 or add a small S7 spike with an explicit ≤1.5x today's per-tick op-evaluation exit criterion) are well-scoped and actionable as written.

---

### ARC-009 [medium] Per-definition reuse cannot reach NovyWave's root group (about 36% of its code), and the warm corpus never edits it

*Decisions: D13, D23, D30; Plan: 4.1, 4.3 (Functions, Cycles), 4.4 Root group, 4.7 Cold first / Reuse, 6 NovyWave warm row, 8 P6, 11 Q20; Sources: A3; verdict: confirmed; severity high -> medium after refutation*

The plan says per-definition reuse will be added in P6 'if needed (likely for NovyWave)', and Q20 gates NovyWave warm once it lands. But the checker solves every root value as one monomorphic group. The checker notes say this group is deliberately not SCC-ordered, because PORT payloads flow back from consumers, and that it is always re-solved whole. So P6 reuse applies only to FUNCTION schemes. On NovyWave, `store` covers RUN.bn:3-4160: 35.6% of tokens and 34.2% of non-comment lines. On TodoMVC the root is only 4.6%. Splitting the root by root-field SCC would not help NovyWave. `scene` (RUN.bn:4949) passes all of `store` into NovyView/main_scene, NovyView.bn has 659 `PASSED.store` reads, and `store` declares 99 SOURCEs that view code binds, so {store.*, scene} is one SCC. Using the plan's own targets (check ≤20 ms NovyWave), one keystroke inside `store` costs about 7 ms of root re-solve, plus ≤4 ms Phase A, about 1.25 ms to reparse RUN.bn (208 KB at 6 ns/B) and about 2 ms of publication. That is roughly 14-15 ms before any cone, whatever P6 does. The graph at 'root-field-path granularity' in 4.3 serves cycles and update kinds, not the solver. The 'each group stores its exact input key' design in 4.7 was written for the notes' per-SCC session (frontend_session.md §2.4), which this checker does not implement. The draft warm corpus has 8 NovyWave edits, all in RUN.bn's FUNCTION region (lines ≥4161) or NovyModel.bn. None is inside `store`, and none is in NovyView.bn (5,741 lines, 379 FUNCTIONs).

Evidence:
- [measurement, verified] `python3 scratchpad/A3/defs.py and a token counter over examples/novywave/**/*.bn and examples/todo_mvc_physical/**/*.bn (BUILD.bn excluded)` — NovyWave: 65,287 tokens, 23,266 in root (35.6%); 3,753 of 10,983 non-comment lines. TodoMVC: 742 of 16,177 tokens (4.6%). NovyWave has 504 FUNCTIONs by `grep -c '^FUNCTION'`: RUN 45, NovyView 379, NovyModel 46, NovyTheme 12, NovyBridge 13, NovyReference 7, Assets 1, hold 1.
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:111, :172, :334` — 'The group is not ordered by SCC because PORT payloads come from consumers through invariance'; 'When it changes, the whole group is re-solved'; 'Only function schemes use early cutoff.'
- [file, verified] `examples/novywave/RUN.bn:3, :4160-4161, :4949` — `store: [` runs to line 4160, the FUNCTIONs start at 4161, and `scene: NovyView/main_scene(PASS: [store: store])` is at 4949. 99 SOURCE lines inside store; 659 `PASSED.store` in View/NovyView.bn.
- [measurement, verified] `python3 tabulation of docs/plans/compiler_rewrite_notes/drafts/compiler_edits.toml` — novywave has 8 edits: 6 in RUN.bn (anchors at lines 4183, 4472 and FUNCTION real_signal_bit_width/startup_file_record) and 2 in NovyModel.bn. None falls in lines 3-4160 or NovyView.bn. todo-mvc-physical has 12, all in RUN.bn and none in Theme/.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:368, :491-494, :655-661, :815`

**Proposed plan change.** Replace the first two bullets of 4.7 with: "**Cold first.** A full recheck per keystroke fits TodoMVC's budget. **Reuse granularity.** Reuse applies to FUNCTION schemes only. The root group (every root definition, and every role in a distributed program) is re-solved whenever any member's structural fingerprint changes or any scheme it instantiates changes its interface. The NovyWave floor for an edit inside `store` is therefore root group + Phase A + reparse of the edited unit + publication (estimate 14-15 ms at the §6 targets), independent of P6." In §6, split the NovyWave warm row into 'edit in a FUNCTION whose interface is unchanged' (gated once P6 lands) and 'edit in the root or reaching main_scene' (report-only, bounded by that floor). Add to S2's exit criteria: 'root-group share of NovyWave check time measured; ≤5 ms or the root group gets a per-root-definition ground-result memo (needs the session TypeStore, see the TypeStore finding)'. Add NovyWave corpus edits: incomplete-syntax and add-field typed inside `store` (e.g. after `default_waveform_asset:`), plus three NovyView.bn edits (a literal, adding a style key, adding a `PASSED.store.x` read).

**Refuter correction (high tier, medium confidence).** Reword the floor as an estimate range, 'about 8-15 ms (root re-solve 1-7 ms pending S2)', and drop the implication that it breaks a target. Keep the plan text change: 'reuse applies to FUNCTION schemes; root-group edits always re-solve the group'. Keep the corpus additions (edits inside `store` and in NovyView.bn) and the S2 exit item measuring the root-group share. The owner question can stay, with recommendation (a).

**Owner question.** On NovyWave, is an edit-to-diagnostics floor of about 15 ms for edits inside `store` acceptable, or should the root group get its own reuse?
- (a) Accept a whole-root re-solve and gate only FUNCTION-body edits
- (b) A per-root-definition memo of ground-evaluated types inside the root group (requires a session-lifetime TypeStore)
- (c) Split the root by SCC (does not help NovyWave: store and scene form one SCC)
Recommendation: (a) now, with (b) as a P6 option if S2 measures the root share above 5 ms.

---

### ARC-010 [medium] With D13 element types and PASSED rows, most view edits change interfaces all the way up to main_scene, so backdating rarely fires on NovyWave

*Decisions: D13, D20, D23; Plan: 4.4 Model, 4.7 Reuse/Errors, 8 S2 and P6; Sources: A3; verdict: confirmed*

4.4 says that with D13 every view function's result type carries its element subtree, and PASSED is a requirement row forwarded to callers (checker.md:110). So a view edit that adds or removes a style key, an element or a PASSED read changes the callee's interface fingerprint. That changes each caller in turn, up to main_scene (S2's generator has depth 22), and then the root group. Under the absorbing-error policy, every error keystroke flips an interface to Error and back, so the cone is rechecked twice per error episode. On NovyWave, per-definition reuse is therefore likely to pay off only for literal edits and non-view helpers. The warm path is in practice the cold path. The incremental research note says the same ('Do not rely on incrementality alone'), but the plan still presents P6 reuse as the NovyWave remedy.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:521-525` — 'every view function's result type carries its element subtree'
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:110` — root reads and PASSED are forwarded upward as outer entries
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md adversarial review 'missing' (interface flapping under typing mode)`
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/incremental_research.md recommendations ('Worst-case edits to hub owners ... degrade to cold')`
- [reasoning, from notes] `S2 generator: 373 constructors, depth 22 (plan:907)` — This is an inference from the type model; nothing measured it.

**Proposed plan change.** Add to S2's exit criteria: "Report the fraction of single-token edits in view functions (TodoMVC after the Theme refactor, plus the generator) that change the edited function's interface fingerprint, and the mean cone size." Add to 4.7: "The edited definition's own diagnostics are published first as a partial Checked (`complete: false`), before the cone and root group are rechecked. The complete revision replaces it; O5 applies to complete revisions only." State that the NovyWave warm target is met by cold speed, and that P6 reuse is sized only after the measured interface-change rate. Adopt expression-level Error (previous finding) so a broken expression that does not reach the result leaves the interface alone.

**Refuter correction (low tier, medium confidence).** Confidence is medium rather than high only because the severity of the practical impact (how often view edits actually change interfaces) is unmeasured on both sides of the argument — the finding is honest about this ('nothing measured it', marked verified_myself: false for the S2-depth-22 inference). The proposed remedy (add an interface-fingerprint-churn measurement to S2's exit criteria; publish the edited definition's own diagnostics before the cone recheck) is concrete and worth keeping at medium severity.

---

### ARC-011 [medium] 'Cold first'/full-recheck (10.7 ms by the plan's own sum) contradicts the plan's own fast-edit targets — the ≤8 ms warm goal, the ≤1 ms no-op gate, and §12.2's 5 ms parallel-check threshold — and the warm gate's overall-p95 aggregation plus a NovyWave gated/report-only inconsistency can hide failing classes

*Plan: §4.7 Cold first, §4.7 warm row, §6 warm rows, §8 P1b exit, §8 P5/P6, §9.3, §11 Q20, §12.2; Sources: A1, A3; verdict: confirmed*

§6 states the warm basis as 'full recheck ≈ 1.2 + 6 + 1.5 + ≤2 = 10.7 ms' and in the same row sets a '≤8 ms goal'. §4.7 says 'A full recheck per keystroke fits TodoMVC's budget. Per-definition reuse is added in P6 if the warm corpus requires it (likely for NovyWave)'. With the stated phase targets, 8 ms on TodoMVC is reachable only with per-definition reuse or with phases beating their own targets by ≥25%, so either the goal is a hidden requirement for reuse on TodoMVC (P6 sizing 'L on its own') or it should be dropped. Related: §12.2 'per-level parallel checking, only if the checker exceeds ~5 ms' sits below the §6 check target of ≤6 ms, so by the plan's own numbers the trigger already fires. Also P1b's exit 'parse ≥1 M lines/s' allows ≤12 ms on NovyWave, 3x looser than §6's ≤4 ms front-end target and 4.6x looser than the prototype.

Additional (A3): §6 also requires 'no-op edits ≤1 ms' and budgets v4 gates `noop_edit_p95_ms = 1.0` over whitespace/comment classes, which likewise fails against the same 10.7 ms full-recheck sum absent a short-circuit; §4.7 defers all fingerprint reuse to P6 'if needed'. The draft v4 lists novywave among gated fixtures and says 'only the overall numbers gate', contradicting Q20 (NovyWave report-only), and an overall p95 can let a rare class (root/hub edits) sit at 2x budget unseen. Separately, 'edit → preview' is compile-only in the benchmark: native present is marked 'not-measured-compiler-only', so no row budgets the plan-swap into the preview runtime that happens on every error-free keystroke.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:814` — warm row: gate 16.7, goal 8, basis sum 10.7
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:655-658` — cold first; reuse in P6 if needed, likely NovyWave
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1256` — parallel checking only if the checker exceeds ~5 ms
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:938` — P1b exit: parse ≥1 M lines/s; reparse largest unit ≤1.5 ms
- [reasoning, verified] `11,926 lines / 1 M lines/s = 11.9 ms vs §6 ≤4 ms` — arithmetic
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/compiler.v4.toml:168, :177, :181` — fixtures include novywave; noop_edit_p95_ms = 1.0; 'only the overall numbers gate'
- [file, verified] `crates/xtask/src/compiler_interactions.rs:1012-1014` — native_present_evidence = 'not-measured-compiler-only'; full_cancellation_gate_pass and native_present_gate_pass hard-coded false

**Proposed plan change.** Either (a) change the goal to '≤8 ms with per-definition reuse (P6, required for TodoMVC)' and size P6 accordingly, or (b) drop the 8 ms goal and keep 'p95 ≤16.7 gate; no-op edits ≤1 ms'. Change §12.2 to 'only if the checker exceeds its §6 target'. Change the P1b exit to '§6 front-end numbers under the frozen profile (NovyWave ≤4 ms, TodoMVC ≤1.2 ms, largest-unit reparse ≤1.5 ms)'. (A3) Additionally, move two sound short-circuits from P6 into P5 (sound because types never depend on values, D4): (1) if every structural and literal-bytes fingerprint is unchanged, return the previous Arc<Checked> with positions re-anchored from the fresh syntax; (2) if only literal-bytes fingerprints changed, reuse all check results and rerun only the literal validations. In 9.3/v4, gate p95 per class, not overall, and list novywave explicitly as report-only until Q20 flips. Add a report row 'edit → first presented preview frame (native verifier), plan swap included'.

**Refuter correction (low tier, high confidence).** None needed on severity; this is the most thoroughly verified finding in the batch and should probably rank as high rather than medium given how many independently-checkable internal inconsistencies it catches.

---

### ARC-012 [medium] Supersession stop latency is unmeasurable as specified and cannot meet its own target: today's probe times an already-cancelled request, per-unit/per-definition polling is too coarse for NovyWave (a root definition such as `scene` covers the whole view and Phase B partial-evaluation), three documents give three different limits, and today's warm-cancellation gates are hard-coded to fail

*Decisions: D13; Plan: §4.5 Phase A/B; §4.7 Cancellation; §6 supersession stop row; §9.3; Sources: A1, A3; verdict: confirmed; severity high -> medium after refutation*

(1) The only cancellation evidence today is `scope: "pre-canceled-request"`: the token is cancelled before `session.request` is called, so the 8 ms gate measures request rejection (~0.006 ms per integration_warm.md:83), not in-flight stop. §6 keeps 'supersession stop latency p95 ≤1 ms, max ≤8 ms | polls' and §9.3 keeps 'the old 8 ms max' without saying how it is measured; only the notes' SE4 acceptance says 'cancelling it at a random point' (frontend_session.md:520), which the plan did not carry over. (2) Granularity arithmetic: 4,096 solver steps at the checker model's ~80 ns/step ≈ 0.33 ms is fine, but §4.7 also polls 'per unit' in the front end and 'per definition in lowering'. NovyWave RUN.bn (208 KB) reparses in 1.24 ms even in the best-of-30 prototype (frontend_session.md:13), so a cancel arriving at the start of that unit waits >1 ms; Phase B is a partial evaluator that inlines every call from the roots, so a root definition can own most of the ≤20 ms NovyWave Phase B, and a per-definition poll there can exceed the 8 ms max. (3) Limits disagree: §6 p95 ≤1/max ≤8; frontend_session.md:581 p95 ≤1/max ≤2; drafts/compiler.v4.toml:178 `supersede_stop_p95_ms = 2.0`.

Additional confirmation (A3): NovyWave's `scene` (RUN.bn:4949) is one root definition whose instance tree (Phase A) and DAG emission (Phase B, up to 20 ms) cover the whole view because main_scene is instantiated there, so per-root-definition polling makes the worst-case stop latency roughly the time to lower `scene`, violating p95 ≤1 ms and max ≤8 ms; hash-consed joins of large D13 element types in the TypeStore are also not counted as 'solver steps'. Today cancellation is checked only between phases (session.rs:718-807), and today's warm benchmark already hard-codes full_cancellation_gate_pass and native_present_gate_pass to false, with the validator requiring status Fail.

Evidence:
- [file, verified] `crates/boon_cli/src/compiler_sample.rs:1250-1281` — token.cancel() before session.request; scope 'pre-canceled-request'
- [file, verified] `crates/xtask/src/compiler_performance.rs:2540-2556` — cancellation_max_ms validated only for finiteness/order
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:670-672` — 'Polled per unit, per group, every 4,096 solver steps and per definition in lowering. Stop latency ≤1 ms.'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:817` — §6 row: p95 ≤1 ms, max ≤8 ms | polls
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:321,520,581` — poll points; SE4 random-point cancellation acceptance; max ≤2 ms
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/compiler.v4.toml:178` — supersede_stop_p95_ms = 2.0
- [file, from notes] `docs/plans/compiler_rewrite_notes/audit/integration_warm.md:83` — pre-canceled probe measured 0.006 ms; gate measures nothing
- [file, verified] `docs/plans/compiler_rewrite_notes/lowering.md:293` — 'with cancellation polled per root definition'
- (3 more evidence entries in the finder output)

**Proposed plan change.** §4.7: 'Cancellation is polled by work count in every phase: every 4,096 tokens in the lexer/parser, every 4,096 solver steps, every 4,096 emitted DAG nodes in Phase B, plus per unit/group/definition boundaries.' §6/§9.3: 'Stop latency is measured by the in-flight probe: start a compile of the fixture, cancel at a uniformly random point in [0, cold time], time from cancel() to the worker's stop acknowledgement; ≥200 cancels per fixture; p95 ≤1 ms, max ≤4 ms on NovyWave.' Delete today's pre-canceled probe from the warm report or label it 'rejection latency'. Pick one number set and change frontend_session/v4 draft to match. (A3, alternate/compatible granularity wording) Replace the cancellation bullet in §4.7 with: 'Polled per unit, per group, every 4,096 solver steps, every 4,096 TypeStore node visits inside a single operation, and every 4,096 emitted instances or nodes in Phase A and Phase B.' In P1b harness v4, supersession is measured by cancelling at a uniformly random point of an in-flight compile over ≥1,000 trials per fixture, reporting p95 and max.

**Refuter correction (high tier, high confidence).** Remove 'hard-coded to fail' as a problem. Cite it as the existing harness admitting the gap ('A pre-canceled synchronous call ... is not the planned ... gate', compiler_interactions.rs:1010-1012). Add the line-671 vs line-817 inconsistency inside the plan itself. Lower to medium: this is text plus a harness definition, fixable in P1b/P5, and it does not change a decision. Keep the proposed wording: work-count polls in the parser and Phase A/B, and a random-point in-flight probe with a stated trial count and one agreed p95/max.

---

### ARC-013 [medium] Warm edit→preview ≤40 ms budgets only the compile (18.7 ms); the other ~21 ms of steps between keystroke and preview frame are neither listed nor measured

*Decisions: D13; Plan: §4.7, §6 warm rows, §8 S1/P5, §12.1; Sources: A1; verdict: confirmed; severity high -> medium after refutation*

Today's keystroke path (verified in code): dev editor → `DevSourceChanged` with every source unit over bincode IPC (dev.rs:354) → desktop `accept_source` and `PreviewApply` with the whole bundle to the preview process (desktop.rs:188-203, 367-396) → preview latest-wins compile thread runs EditorDiagnostics then VerifiedPreview (preview.rs:1300-1313; compile.rs:540, 598) → language snapshot back preview→desktop→dev → readiness thread (`RuntimeReadinessRequest`, preview.rs:1546) → activation/mount → `present_runtime` → `apply_runtime_update_measured`: take_patches, `view.apply_patches` (document build), `set_interaction_state`, `converge_document_demands` (preview.rs:5760-5800) → GPU present. That is 4 IPC hops of full text/snapshot plus readiness, mount, document build, demand convergence and present. The plan's ≤40 ms row cites 'full re-lowering' only (§6) and §4.7 budgets 'snapshot/IPC ≈ ≤2 ms' from an unmeasured '~0.1 ms per hop' estimate (frontend_session.md:606) while the same note estimates the NovyWave language snapshot at ~3 MB per keystroke (frontend_session.md:504). The playground already records `compile_us`, `post_compile_us`, `present_us`, `input_to_present_us` (observer.rs:208-382), but no report in the plan or notes gives their values for TodoMVC, and no spike measures them. Under D13/R-DOC the mount and document-build steps change (elements reconciled by identity at runtime), so today's values would not carry over anyway. For persons_pro the native gate already demands `valid_edit_to_preview_visible_p95 = 16.7 ms` end to end (persons_pro.budget.toml:3), so the non-compile share must fit ~12 ms there; for TodoMVC (many rows) it is unknown.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:814-816` — warm rows: diagnostics sum incl. 'snapshot/IPC ≈ ≤2'; preview 'full re-lowering'
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:673-674` — 'Full text per changed unit, about 0.1 ms per hop'
- [file, verified] `crates/boon_native_playground/src/desktop.rs:188-203` — DevSourceChanged → accept_source → send_preview(Replace)
- [file, verified] `crates/boon_native_playground/src/preview.rs:1300-1313` — preview process compiles via CompileWorker::replace
- [file, verified] `crates/boon_native_playground/src/preview.rs:5760-5800` — document build, interaction, demand convergence timers after compile
- [file, verified] `crates/boon_native_playground/src/observer.rs:208-382` — input_to_present_us, present_us, post_compile_us fields exist
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:504-506` — ~3 MB snapshot per keystroke estimate; SE5 deltas ≤4 KB gate
- [file, from notes] `docs/plans/compiler_rewrite_notes/audit/integration_warm.md:6` — step-by-step description of today's keystroke path (consistent with the code I read)
- (1 more evidence entries in the finder output)

**Proposed plan change.** Add to §6 a warm→preview breakdown row: compile (front+check+A+B ≤18.7 ms) + IPC source hops (2 × ≤0.5 ms, measured with 433 KB) + snapshot hops (≤2 ms, or ≤4 KB delta after SE5) + readiness/mount + document build/reconcile (R-DOC) + present, each with a number, summing to ≤40 ms. Extend S1's shadow counter-dev native run to report `compile_us`, `post_compile_us`, `present_us` and `input_to_present_us` from observer.rs for counter AND TodoMVC on the OLD engine now (a baseline that exists today), and make the P5 exit require the same breakdown on `next`. Add 'warm IPC/snapshot cost' to §12.1 with S1 as its spike.

**Refuter correction (high tier, medium confidence).** Cite the existing profile-stage-breakdown (verify.rs:4217/4643) and the persons_pro chain as the measurement route. Change the proposal to 'run the profile-stage breakdown on TodoMVC with engine=old in P0 as the baseline; P5 exit requires the same breakdown on next, with each stage budgeted to sum to ≤40 ms'. Keep the IPC/snapshot-size point: the NovyWave ~3 MB snapshot estimate (frontend_session.md:504) against the 0.1 ms/hop assumption.

---

### ARC-014 [medium] The message budget covers only the cheap direction: today's snapshot travels 2 hops with a full decode, validate and re-encode, carries a display tree per hint, and will grow under D13

*Decisions: D13, D20; Plan: 4.7 Messages; 6 TodoMVC row ('snapshot/IPC ≤2'); 8 P5; Sources: A3; verdict: confirmed*

The plan budgets 'full text per changed unit, about 0.1 ms per hop'. For source that is plausible: I measured 0.07 ms for TodoMVC's 105 KB and 0.18 ms for NovyWave's 435 KB over a unix socketpair. But today the dev window sends the whole bundle to the desktop router, which fully decodes, validates and re-encodes it for the preview. The language snapshot comes back the same way over 2 more hops. Every InspectorHint carries a complete TypeDisplayNode tree with Strings, and under D13/D20 a hint on a view expression carries its whole element subtree. The notes estimated about 3 MB per keystroke on NovyWave already, before D13. Neither the snapshot size nor its 2-hop cost is budgeted. Separately, 'full text per changed unit' combined with today's latest-wins mailbox, which replaces the pending job, drops unit A's change if a later job carries only unit B. That is safe today only because every message carries the full bundle.

Evidence:
- [file, verified] `crates/boon_native_playground/src/dev.rs:350-358` — clear_language() plus DevSourceChanged with all units on every keystroke
- [file, verified] `crates/boon_native_playground/src/desktop.rs:188-203, :287-296, :371-395` — desktop re-sends all working_units in PreviewApply and forwards PreviewLanguageSnapshot
- [file, verified] `crates/boon_native_playground/src/protocol.rs:1774-1830, :1311-1340` — bincode encode and decode per hop, with validation, including validate_language_snapshot
- [file, verified] `crates/boon_editor/src/language.rs:21-30, :94-112; crates/boon_checked/src/lib.rs:514-529` — InspectorHint.display_tree: TypeDisplayNode (recursive, String labels); the snapshot holds all semantics and all hints
- [measurement, verified] `python3 scratchpad/A3/sock.py (AF_UNIX socketpair, 200 iterations)` — median/p95: 105,017 B 0.072/0.104 ms; 435,298 B 0.183/0.222 ms; 3,000,000 B 0.443/0.689 ms. Raw transport only, no bincode.
- [file, verified] `crates/boon_native_playground/src/compile.rs:140-160` — replace() cancels and replaces the pending request (latest-wins)

**Proposed plan change.** Rewrite the 4.7 Messages bullet: "Source: full text per changed unit. The compile mailbox merges edits per unit (path → latest text) and supersedes only the compile, never the edits. Snapshot: compact per-definition blocks (diagnostics, compact hint labels, occurrences) with deltas keyed by (DefId, version). Full type display trees are served on demand by a hover request, never shipped for every node. Budget: snapshot ≤256 KB and ≤1 ms per hop on NovyWave, measured in the warm corpus." Add a P5 spike: measure the snapshot bytes on TodoMVC and NovyWave with D13 hints before the protocol is fixed.

**Refuter correction (low tier, high confidence).** Minor nuance: InspectorHint already has compact_label/detail_label fields today, so part of the proposed remedy ('compact hint labels') is less of a new design than an existing field that simply isn't relied on to avoid shipping display_tree. Keep severity medium; the owner_question format (in-preview snapshot deltas + hover requests vs. editor-local check-only session) is a fair, actionable framing.

**Owner question.** Where should the diagnostics lane run?
- (a) In the preview process, as today, with snapshot deltas and hover requests over IPC
- (b) In the dev process: an editor-local check-only Session gives diagnostics and hover with no IPC, while the preview keeps its own session for build (check runs twice on two cores, and memory doubles)
Recommendation: (a) by default. Switch to (b) if the P5 spike shows snapshots over 256 KB or more than 1 ms per hop. (b) still satisfies AGENTS.md: the preview still receives Boon source.

---

### ARC-015 [medium] Recommendation: a typed v12 dataflow core with a test-only v11→v12 converter, not v11 + delta

*Decisions: D2; Plan: §4.5 'Hidden v11 contracts', 'Format'; §12.2; Sources: A2; verdict: confirmed; severity high -> medium after refutation*

The notes' argument for v11 ('runtime code stays byte-identical, so both plans run on the same executor and the differential is meaningful') is voided by the plan's own R4, R7, R8 and R9, which change the executor's behaviour and delete the document section. What remains of v11 to 'reproduce' is small and mostly dead or debug-only: the executor ingests the plan through one function (Metadata::new) and 154 field accesses; debug_map string parsing has 16 non-test sites in 4 crates; DependencyEdge/SourceRoute ops are ignored; capability_summary is derivable; view_bindings/initial_patch_batch/delta_plan/dirty/commit counters have no runtime reader; DocumentPlan (1,395 lines of types) is replaced by R4 anyway. Bolting serde-default fields, a per-plan semantics version and 'old document v11 path' onto that is more work and more risk than a typed v12 core consumed by one executor, with the old compiler's v11 converted for the differential.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/lowering_alternative.md:18-24, 37-44` — The v11 verdict rests on 'runtime code for executing plans stays byte-identical' and API-only runtime changes R1-R4; the plan added R4-R9.
- [file, verified] `crates/boon_plan_executor/src/machine.rs:4662-5476` — Metadata::new is the single ingestion point (~815 lines); 154 `plan.` accesses in machine.rs (rg -c).
- [measurement, verified] `rg -c debug_map crates --glob '*.rs' (non-test): machine.rs 8, boon_runtime/lib.rs 4, boon_document/runtime.rs 3, boon_plan/lib.rs 1` — All parse `field:N`/`state:N`/`list:N` with strip_prefix/rsplit (machine.rs:5761-5784, boon_runtime lib.rs:1659-1671, 1748-1775, boon_document runtime.rs:5822-5905, boon_plan lib.rs:6645-6652). A typed names table replaces ~16 sites.
- [file, verified] `crates/boon_plan_executor/src/machine.rs:5064` — `PlanOpKind::SourceRoute | PlanOpKind::DependencyEdge => {}` — ignored by the executor.
- [file, verified] `crates/boon_plan/src/lib.rs:14572; 5468-5502; crates/boon_plan/src/document.rs:53-64` — derive_capability_summary exists; MachinePlan carries dirty_plan/commit_plan/delta_plan/capability_summary/document; DocumentPlan carries view_bindings and initial_patch_batch.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1253-1257` — 12.2 already plans to remove the dead fields and the debug-map string parsing after cutover, i.e. the plan pays for v11 twice.

**Proposed plan change.** Replace 'Format: MachinePlan v11 plus a listed delta' with: 'Format: MachinePlan v12, a typed dataflow core: ops (with `mode: Copy|Live` and identity flags), storage layout, persistence plan (identity_v1 unchanged), source routes, pulses, host ports, output roots, and a typed `names` table (root fields/states/lists by semantic path). No document section (D13). The executor consumes only v12. A test-only `boon_plan_compat::v11_to_v12` converter (deleted at P8) maps the old engine's plans (dataflow parts only; document dropped) so O4 runs both compilers' output on one executor. Persistence identities are computed by the frozen identity_v1 functions on both sides, so schema_hash equality remains checkable.' Pros: one scheduler, no serde-default hacks, no semantics version, 12.2 work done once, typed names for the inspector and scenario runner. Cons: converter (~1-2 k lines, throwaway); O4 loses document comparison (see the O4 finding); COMPILER_ID bump and artifact recompiles (already required by R2/R4).

**Refuter correction (high tier, medium confidence).** Reword the claim: 'v11's byte-identical-executor rationale is voided by R4/R7-R9 (plan:601-614); the remaining question is whether the two-format executor until cutover should be v11+semantics-version or v12 core + compat.' In the proposed change: (a) the v11→v12 converter is a production compat path used by BOON_COMPILER_ENGINE=old until P7 (plan:766-770), not test-only. It carries the old DocumentPlan to the retained old document runtime (deleted with it at P8, plan:1053/1184) and maps old WHEN ops to mode Live, which replaces R8's per-plan semantics version. (b) List the non-executor consumers to migrate (runtime_view.rs, host_runtime persistent.rs/migration_scenario.rs, web_persistent.rs, program_host.rs capability_summary/host_ports) and size them in S1. (c) Make it a question to the owner under D2, with a recommendation, rather than a flat replacement: 'decide v11+delta vs v12-core+compat at S1 exit, using S1's sized delta list'. Cons to add: the compat path must be kept correct for product use until P7, not only for O4.

---

### ARC-016 [medium] 4.5 text corrections: persistence-format acceptance is owned by boon_plan, not the executor; R3 must give every row field a semantic id; DependencyEdge removal proof should be a script over all sources

*Decisions: D12; Plan: §4.5 Persistence, R3, Phase B last bullet; Sources: A2; verdict: confirmed*

(1) 'The executor accepts versions 5 and 6 until cutover' is misattributed: PERSISTENCE_FORMAT_VERSION is checked in boon_plan (predecessor validation and verify_plan); stored images do not carry it, so dual acceptance is a two-line boon_plan change for migration predecessor plans, and D12 already permits one reset. (2) R3 is real and under-specified: page cursors require a semantic id for every field of a hashed row and bounded paging fails without a canonical memory identity, so non-persisted row fields need ids from the same structural-route scheme as D12. (3) 'DependencyEdge ops are dropped once a checkpoint has proven them redundant' names no proof; the executor ignores them, consumers are the old pipeline only, and the notes checked two examples.

Evidence:
- [file, verified] `crates/boon_plan/src/lib.rs:26, 4272, 11083` — PERSISTENCE_FORMAT_VERSION = 5; validate_for_application rejects a predecessor with a different format_version; verify_plan checks equality.
- [measurement, verified] `rg -n 'format_version|FORMAT_VERSION' crates/boon_persistence/src crates/boon_host_runtime/src crates/boon_plan_executor/src → no matches` — Neither stored images nor the executor read the persistence format version.
- [file, verified] `crates/boon_plan_executor/src/cursor.rs:392-420; machine.rs:30679, 30750, 30780` — hash_record requires semantic_row_id and semantic_row_field_id for every field (CursorError::Invalid otherwise); bounded page capture/continuation/result fail without a canonical memory identity.
- [file, verified] `crates/boon_plan_executor/src/machine.rs:5064; rg -c DependencyEdge: boon_plan 8, machine_plan_backend 7, boon_semantic 14, distributed_compiler 2, executor 1, boon_ir 1; boon_document 0` — Executor ignores DependencyEdge; all other consumers are deleted at P8.

**Proposed plan change.** Rewrite the three sentences: 'boon_plan accepts persistence format 5 and 6 for migration predecessor plans until P8 (lib.rs validate_for_application, verify_plan); stored data resets once (D12/D19).' 'R3: every list row field, durable or not, gets a semantic id from its structural route (the D12 identity scheme); cursors and bounded paging use it; persistence leaves are the durable subset.' 'DependencyEdge ops are dropped after a one-off script over dump-plan output of all 177 tracked sources (including fjordpulse and the distributed examples) shows every DependencyEdge's inputs are a subset of another op with the same output; the script and its report live under target/reports and are cited in the S1 report.'

**Refuter correction (low tier, high confidence).** None; this is a solid, narrowly-scoped plan-text-accuracy finding with concrete, low-cost fixes (rewrite three sentences), correctly rated medium severity since it doesn't change the plan's decisions, only its attribution accuracy.

---

### ARC-018 [medium] Field display order (D7) goes stale under backdating when a dependency reorders its fields

*Decisions: D7; Plan: 4.3 Display (D7); 4.7 Reuse; 7 O5; Sources: A3; verdict: confirmed*

Interface fingerprints ignore field order (D7), and reuse is keyed on them. Say `new_todo` (TodoMVC RUN.bn:98) reorders the fields of its result record. Its structural fingerprint changes and it is rechecked, but its order-free interface fingerprint does not, so the dependents are backdated. Their stored hints then keep the old order, while a cold compile shows the new one. O5 fails on the snapshot, or hints are silently stale. Joins across definitions also need a defined 'source order' across units for first appearance (spec.md:141). The notes put a display-order hash into interface_fp; the notes' review proposed rendering lazily instead. The plan specifies neither.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:76 (D7), :466-469, :658-661`
- [file, verified] `docs/plans/compiler_rewrite_notes/spec.md:141-143` — 'merging left to right in source order'
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md §2.4 (interface_fp includes display-order hash) and its review 'simplifications'`

**Proposed plan change.** Add to 4.3 Display: "Display order is never stored in reused check results. The printer computes it at publication from the current presentation tables of the producing definitions. Across definitions, first appearance is ordered by (Config unit order, byte offset)." Add a warm corpus class 'reorder-fields' (swap two fields of a record literal read by other definitions) that O5 compares.

**Refuter correction (low tier, high confidence).** None needed; this is a precise, well-evidenced gap where the notes disagree with each other and the plan is silent, exactly as claimed. Keep at medium severity (a one-sentence spec fix, as the finding itself says) but this is one of the more clean-cut, fully-confirmed findings in the batch.

---

### ARC-019 [medium] Error containment is unspecified: recovery sync points, the size of an error region, and which diagnostics the absorbing Error suppresses

*Decisions: D6, D18, D23, D24, D30; Plan: 4.2 Layout L4 and Recovery; 4.7 Errors; 4.4 Diagnostics (explain mode); Sources: A3; verdict: confirmed; severity high -> medium after refutation*

The plan says only that 'Every unit reports all of its errors. Recovery is a pure function of the text' and that dependents 'see a deterministic absorbing error type'. It does not say (1) where recovery resynchronizes, (2) whether a whole definition or only the broken expression becomes Error, or (3) which diagnostic families stay silent near Error. The parser prototype behind the 2.6 ms figure recovers to EOF after a top-level error, and an unclosed bracket also runs to EOF. The notes' indentation-guided implicit close did not reach the plan, and L4 ('Indentation ... never decides structure') reads as forbidding it. So typing `[`, `(`, `{` or `TEXT {` inside NovyWave's `store` would swallow the rest of RUN.bn (up to 4,949 lines, including 45 FUNCTIONs). The result would be hundreds of unknown-name errors in other units and a root group missing its fields. Raw TEXT (D18: newlines and `--` are text) makes an unclosed `TEXT {` the worst case. The examples' multi-line TEXT puts its content deeper than the opener line and aligns `}` with that line, so an indentation-based recovery rule is available. D24 (unused parameter or binder) will fire falsely whenever a parameter's only use sits inside an unparsed region. A PASSED-row conflict, such as a typo in `PASSED.store.x` inside a view function, is detected in the root group at the PASS site (RUN.bn:4949). The explain mode then re-solves 'the failing definition', which is the root group, not the function containing the typo. Today one error aborts everything (probe below), so none of this exists yet to reuse.

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/fe_proto.rs:219-228, :238, :281, :299` — recover() skips to a separator or closer, or to Eof. elements() reports 'unclosed bracket' only at Eof. Top-level errors call recover(T::Eof).
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md §1.4 (line ~186)` — 'An unclosed opener is closed implicitly when a line at indentation ≤ the opener line's indentation ... Column-0 items always resync.' This is absent from plan 4.2.
- [file, verified] `examples/novywave/Generated/Assets.bn:7-9; examples/todo_mvc_physical/BUILD.bn:25-28` — Multi-line TEXT content is indented deeper than the opener line and `}` aligns with it. BUILD.bn's TEXT contains Boon code with `[` and `--`.
- [measurement, verified] `target/release/boon_cli check scratchpad/A3/two_errors.bn` — A file with 4 independent syntax errors reports exactly one (`pipeline continuation has no preceding value ... line 9`) and ignores the earlier errors at lines 2-3.
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:280, :306-307, :486-489 (explain mode), :662-664`

```boon
store: [
    items: LIST { [title: TEXT { a
    count: items |> List/count()
]
-- plan: unspecified; prototype: everything to EOF joins `items`;
-- expected: one 'unclosed TEXT' error at line 2; `count` and later definitions intact
```

**Proposed plan change.** Add to 4.2 Recovery: "Recovery (error mode only) may use layout. Column-0 lines always resynchronize. An unclosed opener, including `TEXT {`, is closed implicitly before the first line whose indentation is ≤ the opener line's and that does not start with its closer. That is sound because multi-line TEXT content must be indented deeper than its opener line (a new L4 check). Each implicit close is one diagnostic." Add to 4.7 Errors: "Error is expression-level: the parser emits an Error node for each unparsed region, and the rest of the definition is checked normally. Error is both top and bottom for constraints, and a conflict involving Error is dropped. These are suppressed in a definition with an Error node: D24 unused parameter/binder, D23 exactness, exhaustiveness, homogeneity, 'never runs/never updates', and contract checks on Error-typed values. Requirement-row fields carry the consumer ExprRef (DefId + local node), so a PASSED conflict is reported at the read, not at the PASS site." Add a typing-mode corpus assertion: for syntax-only intermediate states, zero diagnostics outside the edited definition, and hints of other definitions unchanged.

**Refuter correction (high tier, medium confidence).** Fix the evidence wording: 'the prototype resyncs top-level errors at the next depth-0 newline, but an unclosed opener (including TEXT {) runs to EOF, and the notes' indentation-based implicit close and fenced reparse (frontend_session.md §1.4) are absent from plan §4.2'. Proposed change: (1) one sentence under L4: 'L4 governs valid programs; error-mode recovery may use indentation (implicit close, column-0 resync), with one diagnostic per implicit close'. Also adopt the notes' fenced reparse as the warm fallback. (2) In §4.7 Errors, list the diagnostic families suppressed in a definition containing an Error node, at least D24 and exhaustiveness/homogeneity. (3) Add an O1/edit-corpus assertion that typing-mode intermediate states produce zero diagnostics outside the edited definition. Treat the PASSED-conflict position as a separate P2a question for the owner, not part of this finding's core.

---

### ARC-020 [medium] Target provenance table: only the front end is measured; check, Phase A/B, warm, RSS and allocation targets are estimates or sums, and the check target sits at the optimistic end of three models that disagree by 12x

*Decisions: D13, D20; Plan: §0, §4.4, §6, §8 S2; Sources: A1; verdict: confirmed; severity high -> medium after refutation*

Provenance of every §6 row and §4 per-phase number (V = verified myself, N = taken from notes):

| target | class | stated basis | arithmetic | cheapest falsifier |
|---|---|---|---|---|
| front end NovyWave/TodoMVC/counter ≤4/≤1.2/≤0.05 ms | prototype-measured | fe_proto.rs 2.6/0.64/0.021 ms (N) | 435,298 B (V) / 2.6 ms = 167 MB/s, 4.6 M lines/s; 1.5x margin to target. Measured best-of-30 with `rustc -O -C target-cpu=native` (N, frontend_session.md:22), not the frozen protocol (p95 of 30, target_cpu=generic, lto=false, cg-units=16; budgets/compiler.toml:16-21 V) | P1a in-repo crate under compile-bench fresh-process p95 on the frozen profile; exit = §6 numbers, not P1b's "≥1 M lines/s" (= ≤12 ms NovyWave) |
| check TodoMVC/NovyWave ≤6/≤20 ms | estimated | "S2 re-derives; pre-D13 model 1.2/4.5" | 6 ms / 4,081 principal expressions (V) = 1.47 µs/expr; 20 ms / 15,650 (V) = 1.28 µs/expr ≈ 600 k lines/s check-only. Today: 387 ms / 4,079 = 95 µs/expr (V). Three internal models: checker.md:186-201 1.2/4.5 ms (60 ns ground, 250 ns var); audit/profile.md:37 4,081×10×150 ns = 6 ms, ×2-3 → 10-20 ms / 40-60 ms; frontend_session.md:446 c = 2 µs/node → 15/60 ms. Comparators (dod_research.md:16-30, N): Sorbet ~100 k lines/s/core (~10 µs/line), Carbon stated target 1 M lines/s (~1 µs/line) on M4, tsgo ~200 k lines/s multi-core | S2, but its exit is "extrapolated check ≤6/≤20" with no extrapolation method; see proposed_change |
| Phase A ≤1.5/≤4 ms | estimated per-node | lowering_alternative.md:176 (0.5-1.5 / 1.5-4 ms; 4.5k/16.3k HIR nodes × 2-3 passes) | 1.5 ms / (4.5k × 3) = 111 ns per node-visit | S1 is counter-scale; nothing validates TodoMVC scale before P4 |
| Phase B ≤8/≤20 ms | estimated per-node + a script over TODAY's plan JSON | lowering.md:295-304 (1.5-4 / 6-13 ms whole back half), lowering_alternative.md:179,243 (~65k inline visits × 50-100 ns; hashcons.py 64,482→11,049 nodes, drafts/hashcons.py V) | 8 ms / 65k visits = 123 ns/visit. Both models predate D13 (elements as data, R4), which replaces the document-emission stage they price | S1 measures "v11+delta emission" on counter only |
| cold diag/verified TodoMVC ≤10/≤20, NovyWave ≤30/≤55 | derived-by-sum | 1.2+6+1.5 = 8.7 (13% slack); 8.7+8 = 16.7 (16%); 4+20+4 = 28 (7%); 28+20 = 48 (13%) | — | inherits S2/S1 |
| small program ≤1/≤2 ms in-process | derived | persons_pro gate bounded_starter_source_compile_p95 = 4.0 ms (examples/persons_pro.budget.toml:5 V) | today counter 7/16 ms of which ~3 ms is manifest load inside the timed window (lib.rs:1876-1900 V; profile.md:6 N) | S1 |
| warm edit→diag TodoMVC p95 ≤16.7 gate, ≤8 goal | derived-by-sum | 1.2+6+1.5+≤2 = 10.7 ms | 10.7 > 8 goal (see finding 5) | P6 corpus |
| warm edit→preview ≤40 ms | derived | "full re-lowering" 10.7+8 = 18.7 ms | 21 ms unbudgeted (finding 3) | S1 shadow run |
| supersession p95 ≤1, max ≤8 ms | polls | 4,096 solver steps × ~80 ns (checker model: 250 ns var expr ≈ 3 steps) ≈ 0.33 ms; but per-unit and per-definition polls are coarser (finding 4) | today's probe measures nothing (compiler_sample.rs:1250-1281 V) | new in-flight probe |
| throughput ≥250 k diag / ≥150 k verified lines/s | derived | NovyWave ≤30 ms ⇒ 397 k; ≤55 ms ⇒ 217 k; floors are 1.6x looser than the fixture gates; notes' own v4 draft floors are 100 k check / 30 k verified with 300 k targets (drafts/compiler.v4.toml:134-140 V) | finding 10 | S2 ns/expr |
| peak RSS NovyWave verified ≤64 MiB | estimated | today 314 MiB | floor 5.8 MiB (`/usr/bin/time -v target/release/boon_cli --help`, V); counter check 39.5 MiB, NovyWave check 368 MiB (V); 58 MiB of room; notes' 2 KB/line ⇒ ~24 MB (dod_research.md:45 N) | P5 bench |
| allocations ≤10 k, NovyWave | estimated | checker.md:205 ≤2k pooled (N) | contradicts §4.2 per-definition boxed slices × 1,389 definitions and v11's String-typed plan (finding 8) | P2a counter |
| 1 instantiation/call, ≤4 work items/expr | counters | today 10,537/350 = 30x and 169,680/4,079 = 41.6 (V) | model implies ~2-3 | S2 reports both |

The plan's headline claim (§0: TodoMVC ≤10/≤20, NovyWave ≤30/≤55) therefore rests on one measured prototype plus estimates, and the check row was set at the optimistic end (6 ms is profile.md's *base* before its own 2-3x allowance, and 4x checker.md's model) without a stated reason.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:800-826` — §6 table and bases
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:525-528` — check model 1.2/4.5 pre-D13; S2 re-derives; targets 6/20
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:186-205` — unit costs and 1.2/4.5 ms model; its own target ≤4/≤12 ms
- [file, verified] `docs/plans/compiler_rewrite_notes/audit/profile.md:37` — 4,081 × 10 × 150 ns ≈ 6 ms; ×2-3 → 10-20 ms TodoMVC, 40-60 ms NovyWave
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:444-448` — c = 2 µs/node checker target → 15/60 ms cold
- [measurement, verified] `target/release/boon_cli compiler-sample examples/todo_mvc_physical/RUN.bn --intent diagnostics --mode fresh-process --samples 1` — principal_expressions 4081, checked_calls 350, compiled_call_sites 10537, owner_unification_steps 169680, parse 36 ms, typecheck 387 ms, VmHWM 130 MiB (single cold sample)
- [measurement, verified] `target/release/boon_cli compiler-sample examples/novywave/RUN.bn --intent diagnostics --mode fresh-process --samples 1` — principal 15650, checked_calls 1821, compiled_call_sites 2699, parse 76 ms, typecheck 466 ms, VmHWM 176 MiB
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:22` — prototype: rustc -O -C target-cpu=native, best of 30
- (2 more evidence entries in the finder output)

**Proposed plan change.** Add a 'method' column to the §6 table stating for each row: measured/prototype/estimate/sum, the producer and protocol, and the spike that validates it. Replace the check row basis with: 'S2 measured ns per principal expression on the frozen profile (p95 of 30 in-process repeats); pass ≤6/≤20 ms; 6-12/20-40 ms → owner re-baseline with a recorded reason; >12/>40 ms → element representation (D13/D20) revisited before P2a starts.' State explicitly that 6/20 ms is 4x checker.md's model and profile.md's base without margin, so S2 must deliver a measurement, not an extrapolation.

**Refuter correction (high tier, medium confidence).** Drop the long provenance table as a finding, since the plan's basis column and plan:802 already disclose that the rows are estimates. Keep the actionable core. (1) S2's exit must be a measurement of the prototype checker on the S2 inputs under the named compile-bench protocol (p95, sample count, profile), not an 'extrapolation'. If extrapolation remains, state the method (ns per principal expression × migrated-example expression count). (2) Record in §4.4/§6 that 6/20 ms is 5x/4.4x checker.md's model, beyond checker.md's own ≤4/≤12 ms target, and at or below profile.md's 6/25 ms base without its 2-3x allowance. (3) Give S2 a middle band that triggers an owner re-baseline. Merge with ARC-026's numeric-tripwire proposal to avoid two findings for one fix.

---

### ARC-021 [medium] Prototype-measured and spike numbers are not taken under the measurement protocol the gates use (best-of-30, target-cpu=native, standalone rustc vs p95-of-30, generic, mimalloc, fresh process); no spike defines its protocol

*Plan: §2, §4.2, §6 front-end row, §8 P0 spikes; Sources: A1; verdict: confirmed*

The only measured basis in §6 (front end 2.6/0.64/0.021 ms) comes from fe_proto.rs built with `rustc -O -C target-cpu=native`, single thread, best of 30 (frontend_session.md:22). The gates are p95 of 30 scored samples after 3 setup samples, one fresh process per observation, with target_cpu=generic, lto=false, codegen-units=16 and mimalloc (budgets/compiler.toml:16-24). Best-of vs p95 and native vs generic together routinely cost 1.2-1.6x, which is the whole 1.5x margin between 2.6 ms and the ≤4 ms target. The prototype also excludes resolution, recovery-with-all-errors, fingerprints of nested definitions and the session interner's DefKey table that the in-repo crate must do. None of S1-S6 states a protocol (producer, profile, samples, statistic); P0 runs before harness v4 exists (P1b). Today's fresh-process window (verified: the timer starts inside `compile_diagnostics_source`, compiler_sample.rs:1669-1685, so process spawn is excluded) still includes reading `examples/manifest.toml` and ~1,300 readlinks (lib.rs:1876-1900), which the plan drops, so the §2 'today' parse numbers are not like-for-like with the prototype either (my single cold samples: parse 36 ms TodoMVC / 76 ms NovyWave vs §2's 18/64).

Evidence:
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:22` — rustc -O -C target-cpu=native, best of 30
- [file, verified] `budgets/compiler.toml:16-28` — generic, lto=false, cg16, 3+30, fresh-process, VmHWM scope
- [file, verified] `crates/boon_cli/src/compiler_sample.rs:1669-1685` — timer window: starts before compile_diagnostics_source(source_path), inside the process
- [file, verified] `crates/boon_compiler/src/lib.rs:1876-1900` — source_files_for_path loads examples/manifest.toml inside the window
- [measurement, verified] `compiler-sample fresh-process --samples 1 on TodoMVC and NovyWave` — parse 36.0 / 76.0 ms single cold samples; §2 quotes 18 / 64

**Proposed plan change.** Add to §8 P0 a 'spike measurement protocol' paragraph: every spike number is reported as fresh-process p95 of ≥10 runs (plus in-process p50 of 30 repeats), on the frozen release profile (generic, lto=false, cg16, mimalloc), with the command line and commit in the report; prototype code lives in a `boonc_*` crate from S1 on, not in a standalone rustc file. Re-state the front-end row as '2.6 ms best-of-30 native; ≤4 ms p95 generic is the gate'. Note in §2 which 'today' numbers include the manifest load.

**Refuter correction (low tier, high confidence).** None on the core claim. One nuance: my reproduction was a single fresh-process sample (noisy by construction, per the harness's own setup_samples/scored_samples design), so part of the 35ms vs 18ms gap could be first-run variance rather than purely the manifest-load explanation; the finding should note this as a contributing-but-not-sole-cause hypothesis. Severity medium is appropriate; this is a legitimate 'the gate's own protocol wasn't used to produce the numbers the gate is judged against' finding.

---

### ARC-022 [medium] Phase A/B targets (≤1.5/≤4, ≤8/≤20 ms) rest on pre-D13 per-node models plus a script over today's plan JSON; S1 validates them only at counter scale, so nothing checks them before P4 (5-7 weeks)

*Decisions: D13, D2; Plan: §4.5, §6 Phase A/B rows, §8 S1; Sources: A1; verdict: confirmed*

§6 cites 'per-node model; S1 measures v11+delta emission' for Phase B. The models are lowering.md:295-304 (whole back half 1.5-4 / 6-13 ms) and lowering_alternative.md:171-180, 243-244 (~65k/32k inline document visits × 50-100 ns; 64,482→11,049 nodes via drafts/hashcons.py, which canonicalises TODAY's v11 plan JSON). Both were written before D13/R4 replaced DocumentPlan templates with element records in the dataflow DAG, i.e. the priced stage no longer exists in that form; the plan's Phase B number (≤20 ms NovyWave) is above both models' upper bounds, which is safe only if D13 does not add work. S1's scope is 'counter, counter_migration and one list-plus-persistence fixture', so its exit cannot say anything about 4.5k/16.3k-node fixtures, and S2 covers the checker only. The first Phase B measurement at TodoMVC scale is therefore inside P4.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:532-548` — Phase A/B targets and design
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:907` — S1 scope: counter, counter_migration, one list-plus-persistence fixture
- [file, verified] `docs/plans/compiler_rewrite_notes/lowering.md:295-304` — per-stage model; L4 emit priced on ~3k/~6k doc nodes
- [file, verified] `docs/plans/compiler_rewrite_notes/lowering_alternative.md:171-180,243-244` — ~65k inline visits × 50-100 ns; 11,049 / 7,745 consed nodes
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/hashcons.py:1-33` — conservative canonicalisation over today's plan JSON document expressions

**Proposed plan change.** Add S1b (end of P2a, 3 days): run Phase A+B of the S1 slice over the S2-migrated TodoMVC view functions with elements as D13 records; exit: Phase A ≤1.5 ms, Phase B ≤8 ms p95 in-process, DAG ≤12k nodes, and the emitted plan mounts on the runtime with the R-DOC prototype from S3. Change the §6 basis text for Phase A/B to 'pre-D13 per-node model; validated by S1b'.

**Refuter correction (low tier, medium confidence).** The refutation_hint itself ('Refuted if S1's list-plus-persistence fixture is actually TodoMVC-sized') cuts both ways: the plan's silence on scale is itself the gap being flagged, so absence of a stated size does not refute the finding, it is the finding. Keep as stated; the proposed S1b spike (validate Phase A/B on S2-migrated TodoMVC view functions with D13 element records before P4) is a reasonable, concretely-scoped addition.

---

### ARC-023 [medium] Throughput floors (≥250 k lines/s diagnostics, ≥150 k verified) are 2.5-5x the notes' own draft floors and 6x Sorbet-class single-core checking; plausible only if S2 confirms ~1.3 µs per expression with structural element types

*Decisions: D13; Plan: §6 throughput row, §8 S2; Sources: A1; verdict: confirmed*

Arithmetic: NovyWave 11,926 lines → 250 k lines/s = ≤47.7 ms diagnostics (the fixture gate ≤30 ms is 1.6x tighter, so the floor is consistent but redundant for NovyWave and really targets the unknown stress corpus); 150 k verified = ≤79.5 ms vs the ≤55 ms gate. Check-only at ≤20 ms is ~600 k lines/s = 1.28 µs per principal expression (1.31 expressions per line). Public single-core comparators from the notes: Sorbet ~100 k lines/s/core (local, forward-only inference), Carbon's stated 1 M lines/s check target on an M4, tsgo ~200 k lines/s across threads; oxc's 6.5 M lines/s is parse-only and irrelevant to the check floor. The notes' own proposal was ≥100 k lines/s check with 300 k stretch (dod_research.md:43) and the v4 draft floors are check 100 k / verified 30 k (compiler.v4.toml:135-137). Boon's grammar is small and Simple-sub style inference is linear-ish per definition, so ~1 µs/expr is not absurd for a DOD checker, but D13 makes every view function's result type carry its element subtree (NovyWave: 373 constructors, depth 22, 252 style records; §8 S2) and instantiation copies every node containing a variable; since every Theme function reads PASSED (checker.md:421 'unrealistic'), the ground fast path the model assumes (60% ground) may rarely apply. The floor is therefore an S2 outcome, not an input.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:818` — throughput row: ≥250 k / ≥150 k, today 23 k / 6.4 k
- [file, from notes] `docs/plans/compiler_rewrite_notes/audit/dod_research.md:16-17,28-30,43-44` — Sorbet 100k/core; Carbon 1M target; tsgo 200k; proposed Boon check ≥100k, stretch 300k
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/compiler.v4.toml:132-140` — draft floors and targets
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:419-422` — reviewer: ground-share assumptions unrealistic because Theme reads PASSED; scheme growth with element trees
- [measurement, verified] `compiler-sample NovyWave diagnostics --samples 1` — 15,650 principal expressions / 11,926 lines = 1.31 expr/line

**Proposed plan change.** Set the §6 floors from S2: 'diagnostics floor = 0.6 × (11,926 lines / S2-measured NovyWave diagnostics time), verified floor likewise, both rounded down; provisional 150 k / 75 k until S2 reports'. Require S2 to report ns per principal expression, the ground-slot share, the largest scheme node count and instantiation copy counts, on TodoMVC and on the NovyWave-shaped generator.

**Refuter correction (low tier, high confidence).** None needed to severity; consider tightening the proposed_change to explicitly cross-reference the existing S2 annotation pattern used on the neighboring 'check' row (line 808) rather than inventing new phrasing.

---

### ARC-024 [medium] 'Compiler-internal allocations ≤10 k per compile' has no defined boundary, contradicts §4.2's per-definition boxed slices, and is unmeasurable with today's evidence lane

*Plan: §4.2 Arena, §6 allocations row, §9.3; Sources: A1; verdict: confirmed*

§4.2 says each definition's arena level 'is copied into boxed slices' (frontend_session.md:184) and NovyWave has 1,389 definitions; at 2-4 slices each that is 2.8-5.6k allocations before checking starts. checker.md:205 budgets ≤2k for the checker with pooled arenas. Phase B assembles a MachinePlan v11, whose type definitions in boon_plan/src/lib.rs have 78 `String`/`Vec<String>`/`BTreeMap<String` fields and a `DebugMap` (lib.rs:5502, 9952) whose labels are `field:N`/`state:N` strings (§4.5 'hidden v11 contracts'); for NovyWave (~1,266 ops, 2,071 row nodes, ~6k element nodes) that is tens of thousands of allocations. The plan says 'plan construction reported', but the evidence lane counts every global-allocator event on the compiler thread (budgets/compiler.toml:13-15), so the split needs a phase-scoped counter that does not exist. In fresh-process mode no arena can be reused from a previous compile, so ≤10k must hold with fresh arenas; it is plausible only if per-definition storage is one arena per unit with offsets (or pooled scratch) and if the plan assembly is excluded.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:820` — ≤10 k; plan construction reported
- [file, verified] `docs/plans/compiler_rewrite_notes/frontend_session.md:184` — per-definition level copied into boxed slices
- [file, verified] `docs/plans/compiler_rewrite_notes/checker.md:205` — pooled arenas, ≤2k allocations
- [measurement, verified] `grep -c ': String\|Vec<String>\|BTreeMap<String' crates/boon_plan/src/lib.rs` — 78 matches
- [file, verified] `budgets/compiler.toml:12-15` — evidence lane: thread-local global-allocator events on the single compiler thread

**Proposed plan change.** Rewrite the row as: 'allocator events on the compile thread from first byte to `Checked` (diagnostics lane) ≤10 k on NovyWave, fresh process; Phase B including MachinePlan assembly reported separately with its own ≤N once S1b measures it; counters reset at the lane boundary by the session.' In §4.2 replace 'copied into boxed slices' with 'appended to one per-unit arena with definition-relative offsets' or state that the slices come from a pooled scratch that the session reuses across revisions.

**Refuter correction (low tier, high confidence).** The finding's '2-4 slices each' estimate is conservative (actual DefSyntax has up to 6 boxed-slice fields, though several — children/literals/diags — are often empty and may not allocate). More importantly, the proposed_change's first alternative ('one per-unit arena with offsets') would conflict with the plan's explicit incremental-editing rationale for per-definition arenas; the recommendation should instead ask for a pooled/bump allocator that amortizes allocations across many small per-definition slices while keeping per-definition granularity, or simply require the ≤10k figure to state a phase-scoped counter boundary (which is the finding's second, better alternative).

---

### ARC-029 [medium] Trust (D3): name the three untrusted boundaries and require the executor and document runtime to fail closed on unverified plans

*Decisions: D3; Plan: §4.5 Trust; §7 O6; Sources: A2; verdict: confirmed*

Today every activation seals (verify_plan + hash) via MachineTemplate::new_shared, and program_core decodes stored artifacts then calls MachineTemplate::new_sealed. With a trusted constructor, the untrusted boundaries are: stored program artifacts (program_core decode_program_artifact; persons_pro child artifacts are stored here), plans received by a distributed session (session.rs:88), and app bundles loaded by the web host (boon_app_package). Metadata::new re-checks only some invariants (e.g. unique producer call-site ids); dense-index reads elsewhere (`expressions[id.0]`, per the notes) panic rather than error on a malformed plan. The plan names 'artifacts, app bundles and received plans' but not the call sites nor a fail-closed test, and program_host's in-process verify (to be dropped) is the only defence for child plans today.

Evidence:
- [file, verified] `crates/boon_plan_executor/src/machine.rs:9509-9527` — new_shared → seal_shared_machine_plan (verify) → new_sealed; new_sealed documented as 'does not introduce a second unchecked runtime entrypoint'.
- [file, verified] `crates/boon_program_runtime/src/program_core.rs:315, 375, 455-461, 712` — decode_program_artifact + validate_plan + MachineTemplate::new_sealed; the compile path also goes through new_sealed.
- [file, verified] `crates/boon_distributed_runtime/src/session.rs:88` — MachineTemplate::new_shared(artifact.plan().clone()) for received artifacts.
- [file, verified] `crates/boon_program_runtime/src/program_host.rs:498; crates/boon_web_host/Cargo.toml:12-20` — In-process verify_plan in program_host; web host depends on boon_app_package, boon_document, boon_runtime (bundle load on wasm32).
- [file, verified] `crates/boon_plan_executor/src/machine.rs:4700-4702` — Metadata::new has its own InvalidPlan checks (e.g. producer call-site uniqueness), i.e. some verifier duplication already exists; the rest relies on verify_plan having run.

**Proposed plan change.** Expand R1: 'load_untrusted is called at exactly three sites: program_core::decode_program_artifact, boon_distributed_runtime session artifact receipt, and boon_app_package bundle load (native and wasm32). MachineTemplate::new_trusted is `pub(crate)`-visible only to boon_runtime/boon_program_runtime compile paths.' Add to O6: 'a negative corpus of plans that fail verify_plan; the executor, document runtime and reconciler must return Error, never panic, on every one of them (fuzz seed from the verifier's own failure fixtures)'. Keep persons_pro child *compiles* trusted (in-process compiler output) but their stored artifacts untrusted.

**Refuter correction (low tier, high confidence).** None substantive; the finding correctly acknowledges partial existing coverage (the boundary categories are named) and asks only for call-site precision and a fail-closed test, which is the right scope.

---

### ARC-030 [medium] The session API lacks the editor queries the plan itself promises: hover (update kind, what a WHEN copies, effects), fix-its, related locations, OUT inline hints

*Decisions: D11, D15, D24, D30, D31; Plan: 4.8 Public API; 4.3 Stale-copy hint; D31; L3c; 4.3 Cycles; Sources: A3; verdict: confirmed*

The facade exposes only diagnostics(), hints(file), occurrences() and has_errors(), plus syntax::highlight and format. The plan also promises: hover that 'always says what a WHEN copies'; hover saying 'no value until … first happens' (L3c); effects visible 'through hover, an inferred effect row on every FUNCTION scheme' (D31); 10 fix-its (D11 parentheses, D24 removal, one-input LATEST, D15 LATEST→HOLD, …); and cycle diagnostics that 'list the path'. None of these fits the listed API: a Diagnostic has no fixes and no related locations, and there is no position query. Today's editor also uses an inline OUT hint channel with no mapping in the plan. Today's feature set is go-to-definition, occurrence highlighting, hints, diagnostics, format and lexical highlighting. Symbol rename and completion do not exist today; DEV_RENAME renames examples and files, not symbols. So those are not regressions.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:681-688, :409-413, :100 (D31), :1207 (L3c), :422`
- [measurement, verified] `grep -o -i 'fix-it' docs/plans/BOON_COMPILER_REWRITE_PLAN.md | wc -l` — 10
- [file, verified] `crates/boon_editor/src/language.rs:84, :111, :144, :193` — inline_out_hints, lex_source highlighting, definition_at
- [file, verified] `crates/boon_native_playground/src/dev_state.rs:57, :344; dev.rs:711-730, :1045` — NavigateDefinition (F12/ctrl-click); rename is example/file rename; format_source_unit

**Proposed plan change.** Add to 4.8: `Checked::hover(file, byte) -> Option<Hover { type_display, update_kind, copies: [Name], follows: [Name], effects, no_value_until }>` (lazy, and the only source of full display trees). `Diagnostic { …, related: [(file, span, label)], fixes: [Fix { label, edits: [TextEdit] }] }`. `Checked::out_hints(file)`. `DistributedSession::checked(role)`, with a stated rule for files shared between roles. Optionally `Checked::completions(file, byte)`, since name resolution already has the scope stack. Add an O1 expectation format for fixes and related locations.

**Refuter correction (low tier, high confidence).** Acknowledge the BOON_COMPILER.md contract deliverable (P0, line 893) as a plausible venue where these APIs could still be specified later; the finding's own refutation_hints already anticipates and accepts this ('If P0's BOON_COMPILER.md contract is meant to spell out the API, add these to that deliverable'), so this doesn't refute the finding, only means the proposed_change should point at that document as an alternative target, which the finding already allows for.

---

### ARC-025 [low] Memory targets: the floor is 5.8 MiB and ≤64 MiB leaves 58 MiB of room (generous by the notes' own 2 KB/line estimate), but the plan does not say which producer/process or which statistic (absolute VmHWM vs delta) the two 64 MiB numbers use

*Plan: §4.7 Memory, §6 RSS row, §9.3; Sources: A1; verdict: confirmed*

Measured with the existing binary: `/usr/bin/time -v target/release/boon_cli --help` → 5,808 KiB max RSS (the 39 MB binary maps lazily); `check examples/counter.bn` → 40,424 KiB; `check examples/novywave/RUN.bn` → 377,088 KiB, 1.89 s wall; compiler-sample VmHWM: TodoMVC diagnostics 133,452 KiB, NovyWave diagnostics 180,536 KiB. So today's counter compile alone adds ~34 MiB over the floor. The plan's ≤64 MiB (peak, NovyWave verified) and ≤64 MiB (retained session state, NovyWave) are different quantities: budgets v3 measures the process high-water mark through the artifact (compiler.toml:27-28, VmHWM via /proc/self/status); the notes' v4 draft switches to `rss_delta = ru_maxrss after minus before` (compiler.v4.toml:39) and gives NovyWave `rss_delta_mib_max = 192`, while §9.3 says 'absolute peak RSS from a lean producer (baseline ~6 MiB)'. 'Retained session state' can only be measured in the compile-bench process (the preview process carries wgpu), after a defined edit corpus, and the 'today 383-414 MB' figure has no stated producer. With source 435 KB, 12 B/node arenas (~0.4 MB), a 1-2 MB TypeStore, ~0.4 MB RIR and a ~2 MB plan, ~30 MiB is the expected ceiling, so 64 MiB is safe but loose.

Evidence:
- [measurement, verified] `/usr/bin/time -v target/release/boon_cli --help; check examples/counter.bn; check examples/novywave/RUN.bn` — max RSS 5,808 / 40,424 / 377,088 KiB
- [measurement, verified] `compiler-sample fresh-process --samples 1 (TodoMVC, NovyWave diagnostics)` — peak_rss_kib 133,452 / 180,536
- [file, verified] `budgets/compiler.toml:27-28` — peak_rss_scope = process-high-water-through-compiler-artifact
- [file, verified] `docs/plans/compiler_rewrite_notes/drafts/compiler.v4.toml:39,127` — rss_delta definition; NovyWave rss_delta_mib_max = 192
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:676,819,1158` — retained ≤64 MiB; peak ≤64 MiB; 'absolute peak RSS from a lean producer'

**Proposed plan change.** §6: 'peak RSS = VmHWM of the compile-bench process on the frozen binary, fresh process, NovyWave verified, ≤48 MiB gate (floor 5.8 MiB measured 2026-09-29), 64 MiB hard max'. 'Retained session state = VmHWM delta of the compile-bench process between session open and the end of the 12-class NovyWave edit corpus, ≤64 MiB, growth ≤8 MiB across the corpus'. Name the producer for today's 383-414 MB.

**Refuter correction (low tier, high confidence).** None; severity as labeled (low) is appropriate — this is a definitional/measurement-methodology gap, not a design flaw.

---

### ARC-027 [low] Risks in §12.1 without a spike or measurement, and spikes whose exits cannot fail

*Plan: §8 spikes, §12.1; Sources: A1; verdict: confirmed*

Rows with no spike: 'Persistence schema drift' (golden vectors exist only once identity_v1 is implemented in P4; no P0 check that the D12 structural-route identities are stable under the 12 edit classes), 'Gate churn' and 'Plan sprawl' (process only), 'Noisy machine' (protocol only; the plan's own instruction-count signal needs perf_event access, which Q13 now provides). Risks not listed at all: warm IPC/snapshot cost (finding 3), R8 per-tick cost (finding 9), the build-profile changes in P6 (LTO/cg1/PGO) changing the frozen protocol that every earlier spike number was taken under, and wasm32 (Q17 makes it report-only, acceptable). Spike exits that cannot fail: S1 'every needed runtime/format change is listed and sized', S4 'counts per class', S6 'each runtime bug sized'. S5 is the only spike that lands code on the shared executor before P2 and re-runs old-engine gates; its blast radius on the native handoff gates is not time-boxed.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1238-1251` — §12.1 table
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:906-916` — spike exits
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1033-1036` — P6 build-profile A/B and native gate re-run

**Proposed plan change.** Add §12.1 rows: 'warm IPC/snapshot cost → S1 shadow run breakdown', 'R8 per-tick cost → S5/S7', 'protocol change in P6 → all §6 rows re-measured on the new profile before the checkpoint; spike numbers are labelled with their profile'. Give S1/S4/S6 a numeric exit each (S1: counter verified ≤2 ms in-process and the three scenarios pass; S4: the census script runs on all 177 .bn files with zero unclassified sites; S6: ≥N of 21 scenarios mountable through the host-service runner or a runtime-fix track is opened) and a time box (S5 ≤5 days including the old-engine gate re-run).

**Refuter correction (low tier, medium confidence).** Narrow the 'spikes whose exits cannot fail' framing: only S4's exit is purely qualitative by design (acceptable for a census spike); S1 and S6 already contain falsifiable/numeric components mixed with one soft clause each, so the finding should ask only for the specific soft clauses to be tightened (or acknowledge they're deliverables, not gates) rather than characterizing whole spikes as unfalsifiable. The missing-risk-rows half of the finding is solid and should be kept as the primary actionable ask.

---

### ARC-028 [low] 'No code path may depend on an example's name' is not checkable as written; today the compile path is keyed by source path through examples/manifest.toml, and Config.app can carry an example identity

*Decisions: Q18; Plan: §4.9, §7, §9.1; Sources: A1; verdict: confirmed*

§9.1 states the rule and §4.9 lists 'no example-specific branches' as checked from day one, but no mechanism is named. Today `source_files_for_path` (crates/boon_compiler/src/lib.rs:1876-1893) matches the source path against every manifest entry and program to pick file lists, i.e. the compile path is example-keyed by path. The plan removes the manifest from the compile path (§4.2 Discovery) but keeps `Config { entry, target, role, app, ... }` (§4.8), where `app`/ApplicationIdentity and `entry` are strings an implementation could branch on, and `debug_map` labels and source-route `path` strings (§4.5 hidden contracts) embed paths. A grep lint for example ids catches literals only.

Evidence:
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:1113-1114` — rule text in §9.1
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:711` — 'no example-specific branches' checked from the day each crate exists
- [file, verified] `crates/boon_compiler/src/lib.rs:1876-1900` — manifest-keyed file discovery inside the compile path today
- [file, verified] `docs/plans/BOON_COMPILER_REWRITE_PLAN.md:679-686` — Config carries entry and app

**Proposed plan change.** Add oracle O7 'rename': every fixture is compiled a second time with its units copied under random unit paths, a random entry stem and a fresh ApplicationIdentity; diagnostics (modulo path text), the language snapshot and the canonical plan (modulo path/app strings and the identities derived from them under Q18) must be identical. Add an xtask lint that fails if any `boonc_*` crate contains a string literal equal to a manifest example id or a path under examples/. Point §9.1's sentence at O7 and the lint.

**Refuter correction (low tier, high confidence).** None; severity as labeled (low) is reasonable — this is a process/checkability gap, and the proposed O7 oracle plus xtask lint is a reasonable, concrete fix.

---

### ARC-031 [low] wasm32 from day one has no product consumer after P7; single-threaded cancellation is undefined; the sha2 ban needs precise wording

*Decisions: D3; Plan: 4.8 Porting (boon_runtime loses its compiler dependency); 4.9; 8 P1a; 11 Q17; Sources: A3; verdict: confirmed*

The wasm32 requirement exists because boon_runtime links the compiler into the web host. The plan removes that dependency in P7. boon_web_host/src has no compile call; only its wasm32 dev-dependency test compiles programs. After P7, then, nothing in the browser compiles Boon, and 'in-browser compile latency is reported' has nothing real to measure. Also, a synchronous compile on single-threaded wasm32 can never see a Cancel(AtomicBool), because no other thread exists to set it. A web editor would need either a worker with deadline polling or a resumable, budgeted check API. On sha2 there is no contradiction: identity_v1 goes through boon_plan, which depends on sha2, and SHA-256 cost is negligible (a TodoMVC plan has 11 memory_id, 23 leaf_id and 36 type_fingerprint fields). But the 4.9 check must forbid only direct dependencies, or it trips on boon_plan. mimalloc is set only in binaries, so it does not affect wasm.

Evidence:
- [file, verified] `crates/boon_web_host/Cargo.toml (deps: boon_runtime; wasm32 dev-dep boon_compiler at :102); crates/boon_runtime/Cargo.toml:9; crates/boon_runtime/src/lib.rs:1-5`
- [measurement, verified] `grep -rn -i 'compile|from_source|from_project' crates/boon_web_host/src/*.rs` — no compile path; only tests/startup.rs:7-8, :42 compile
- [file, verified] `deploy/fjordpulse/Dockerfile:17` — builds boon_web_host for wasm32-unknown-unknown
- [file, verified] `crates/boon_plan/Cargo.toml (sha2), crates/boon_plan/src/lib.rs:6347`
- [measurement, verified] `target/release/boon_cli dump-plan examples/todo_mvc_physical/RUN.bn --out scratchpad/A3/todo_plan.json; grep -o counts` — 0.78 s; 20.4 MB JSON; memory_id 11, leaf_id 23, type_fingerprint 36, schema_hash 1
- [file, verified] `crates/boon_cli/src/bin/boon_cli_evidence.rs:8-13` — global_allocator is binary-level

**Proposed plan change.** Q17 default: "The new crates pass `cargo check --target wasm32-unknown-unknown` and the musl build (gate). No in-browser compile consumer exists after P7, so in-browser latency is not reported until a web editor is planned. That plan must choose a Web Worker with deadline-based Poll (the clock abstraction) or a resumable `check_step(budget)` API." 4.9: "no *direct* dependency on sha2, ciborium or serde_json (checked with `cargo tree -e normal --depth 1`)".

**Refuter correction (low tier, high confidence).** None; severity as labeled (low) is appropriate — these are portability/wording clarifications, not architectural risks.

---
