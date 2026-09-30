# Proposed edits to BOON_COMPILER_REWRITE_PLAN.md

Review output, not authority, not applied. Line numbers refer to the plan at a60a11d6. Each edit names the findings it comes from (ids from REVIEW.md, for example SEM-001) or a prior-art note (`research/<file>` item N), so every change traces back to its evidence.

**How to apply.**
1. Edits are ordered by plan section: §1 wording, then §3, §4.2-§4.9, §5, §5.2, §6, §7, §8, §9, §11, §12, and last the preamble, §0 and §2. Apply bottom-up if you paste by line number. "Replace" quotes the current text, shortened with "…"; "With" is the complete new text.
2. Unconditional edits fix wording, contradictions inside the plan, or specification that follows from a settled decision. They can be applied as one batch.
3. An edit marked **(conditional on QUESTIONS.md Qn)** carries the text for the review's recommended option. If the owner picks another option, drop the edit or use that option's text from QUESTIONS.md. Where only one clause of an edit is conditional, the mark sits on that clause.
4. No decision is changed here. The §1 edits either align a decision's wording with what it already says, or they are conditional.
5. Question ids are those of QUESTIONS.md: §A (A1-A10) must be answered before static semantics v2 is written, §B before the phase it names, §C by default unless the owner objects. The plan's own questions are written "plan Q17" and so on. A1 activation copy; A2 step rule; A3 D23 exactness; A4 stale-copy error; A5 cycle edges; A6 collections in stored values; A7 live-query restart; A8 what persists; A9 Cells iteration; A10 state below a copy. B1 copy-context supersession and echo; B2 interrupted commands; B3 FLUSH in HOLD updates; B4 effect classes; B5 trigger identity; B6 the wire; B7 multi-user memory; B8 sensitive inputs; B9 previous schemas; B10 stored copies and moved state; B11 hardware stores and commits; B12 BYTES capacity; B13 recursive data; B14 whole-variant binder; B15 theme data under closed contracts; B16 multi-line TEXT indentation; B17 diagnostics shape. C1 next-only sources before P7; C2 old-engine plans on the new scheduler; C3 change-rule oracle; C4 S2 band and committed reuse; C5 NovyWave root floor; C6 diagnostics lane; C7 budgets v4; C8 line caps; C9 effort and P0 staffing.
6. New names do not collide with the plan's names: O7-O9, R10-R11, S1b, S7 and the runtime track R-CHG. Three findings each proposed an "O7"; this file renumbers them (O7 change-rule traces, O8 rename, O9 hostile input).
7. The largest single edit is §4.3 "Change rules", which becomes the edge table, the activation rule, the step rule and the effect-class table. P0 should write those first.
8. Conditional edits, by question: A1 in D30, §4.3 Activation and Copy contexts, the §5.1 pure-arm row; A2 in D32, §4.3 Steps, LATEST and HOLD, R7-R8; A3 in §4.3 Contracts; A4 in §4.3 Stale copies; A5 in D9 (wording) and §4.3 Cycles; A6 in D9 (collections) and the §5.1 collections row; A7 in D31, the `Value/distinct` clauses of D32 and D34, P2b's `Value/distinct` and the effect-class hover; A8 in D12, §4.3 History nodes, §4.5 durable leaves, R5; A9 in the Cells row and S7; A10 in D25 and Copy contexts; C1 in §4.8 engine selection, the §5.1 rule, §9.1 and P7; C2 in R7-R8 and §4.5 Format; C4 and C5 in §4.7, §6 and P6; C9 in P0 and the effort table; B and C items at the clauses that name them.

## §1 Owner decisions (wording)

### §1 D4 (line 73): which values types never depend on
Findings: MIS-016 (as corrected: spec.md:24 already has this wording). Specification that follows from spec.md:24 and today's BITS[N] rule (LANGUAGE_SEMANTICS.md), not a new decision.
Replace: "Types never depend on values."
With:
> Types never depend on runtime values; BITS and BYTES widths come from compile-time constants.

### §1 D9 (line 78): name the edges that close a cycle (conditional on QUESTIONS.md A5)
Findings: SEM-001, SEM-002, SEM-049; research/copy_vs_live_synchronous_languages.md item 3.
Replace: "Nothing else closes a cycle: not LATEST, not derived fields, not THEN bodies."
With:
> A HOLD update is the value a HOLD's body writes into its state. Nothing else closes a cycle: not LATEST, not derived fields, not THEN bodies, and not a HOLD's piped input (a D33 reset is a FIRE edge, not a HOLD update edge). §4.3 "Edges" says which reads fire and which copy.

### §1 D9 (line 78): collections in any stored value (conditional on QUESTIONS.md A6)
Findings: EVI-008, SEM-024, SEM-049.
Replace: "LIST, SET and MAP values, and records or tagged objects containing them, may not be HOLD state."
With:
> LIST, SET and MAP values, and records or tagged objects containing them, may not be stored state: not HOLD state, not a LATEST's current value, and not any other value D12 stores. Collections live only in collection authorities (D17).

### §1 D12 (line 81): define the inferred clause and the restore of query answers
Findings: SEM-008, SEM-026, MIS-003, SEM-024, SEM-025, SEM-028; research/state_restore_persistence.md item 3.
Replace: "the last value of an app-computed value that code reads outside its own update; the compiler infers that set." and "Query results are re-run on restore, commands never (D31)."
With (the "history node" and query clauses are conditional on QUESTIONS.md A8):
> … and (because every value keeps its last value, D30) the last value of every history node that code reads outside its own update, as defined in §4.3 "Activation"; the compiler infers that set. Live queries re-run on restore; an answer that a HOLD, LATEST or collection authority already holds is restored like any other leaf. Commands never re-run (D31). DRAIN/DRAINING, the sequential migration catalog and `persistence_only` predecessor stages are kept.

### §1 D25 (line 94): WHILE arms, not WHEN arms (conditional on QUESTIONS.md A10)
Findings: SEM-014.
Replace: "Every WHEN arm's state exists, even when a compile-time constant rules the arm out."
With:
> Every WHILE arm's state exists, even when a compile-time constant rules the arm out; WHEN arms carry no state (L6).

### §1 D26 (line 95): the consequence sentence
Findings: MIS-003.
Replace: "HOLD and collection updates are then the only state primitives, which is what the cycle (D9) and persistence (D12) rules are built on."
With:
> HOLD and collection updates are then the only state the user declares, which is what the cycle (D9) and persistence (D12) rules are built on. LATEST, copy outputs and SOURCE payloads hold implicit state because every value keeps its last value (D30); §4.3 "State classes" lists them.

### §1 D28 (line 97): the view example must be WHILE
Findings: SEM-013 (an example fix; D28 itself is unchanged).
Replace: "A collection chosen through WHEN (`zs: WHEN { A => xs, B => ys }`) is a read-only view"
With:
> A collection chosen through WHILE (`zs: sel |> WHILE { A => xs, B => ys }`) is a read-only view whose element type is the union; the same arms under WHEN copy the collection when `sel` updates.

### §1 D30 (line 99): WHEN at activation (conditional on QUESTIONS.md A1)
Findings: SEM-007, MIG-005, SEM-008.
Replace: "**WHEN and THEN copy:** the body runs once each time the input updates and copies the current values of everything else it reads"
With:
> **WHEN and THEN copy:** the body runs once each time the input updates and copies the current values of everything else it reads. A WHEN whose input already has a value also copies once when its scope activates; that activation copy is not an update (§4.3 "Activation"). A THEN never runs at activation.

### §1 D31 (line 100): "update" instead of "change", commands never cancelled, never starts on restore
Findings: SEM-016, SEM-043, SEM-017, SEM-023, SEM-030, SEM-019, EVI-012; research/query_command_effect_systems.md items 7 and 10.
Replace: "restarts when its arguments change" / "and never runs on start, restore or hot reload" / "Effects become visible through hover, an inferred effect row on every FUNCTION scheme, …"
With (the restart clause is conditional on QUESTIONS.md A7, the class sentence on B4):
> … restarts on every update of an argument (D32) … and is never started by start, restore or hot reload (a durable command already staged before a crash is completed under the outbox contract). Commands are never cancelled or superseded. Every effectful catalog entry has one class from §4.3 "Effect classes": query, command, host source or observer (`Log/*`, the L2 default). Effects become visible through hover, an inferred effect row on every FUNCTION scheme (which is also the call's placement requirement, §4.3 "Placement"), diagnostics, and an effect log in the dev window and scenario runner.

### §1 D32 (line 101): once per tick, and the named filter
Findings: SEM-003, SEM-016; research/change_semantics_frp_ui_excel.md item 1, research/copy_vs_live_synchronous_languages.md item 1.
Replace: "a derived value fires (at most once per step)" and "A "only real changes" filter, if ever wanted, is a library function over HOLD, not engine support."
With ("per tick" is conditional on QUESTIONS.md A2, the `Value/distinct` clause on A7):
> … a derived value fires (at most once per tick, §4.3 "Steps") … The "only real changes" filter is a standard-library function over HOLD, not engine support: P0 adds `x |> Value/distinct(start: …)` to the catalog (the name is the owner's), and hover says "fires on every write" where it matters.

### §1 D34 (line 103): where request_fingerprint's two roles go
Findings: SEM-016, SEM-006.
Replace: "Wellen's hand-written `request_fingerprint` is unnecessary once restarts are driven by argument updates."
With (the echo clause is conditional on QUESTIONS.md B1, the `Value/distinct` alternative on A7):
> Wellen's hand-written `request_fingerprint` goes. Restarts are driven by argument updates; its dedupe role moves to SKIP in the argument's HOLD or to `Value/distinct` (D32), and its echo role moves to the query result, which carries the arguments it answers (a catalog convention).

### §1 D35 (line 104): hot reload, hardware stores, sensitive input
Findings: SEM-032, MIS-006, MIS-011; research/state_restore_persistence.md item 1.
Replace: "in-memory for one-shot CLIs, tests and FPGA (power-on resets every HOLD to its starting value); dev hot reload always keeps state."
With (the RTL clause is conditional on QUESTIONS.md B11, the last sentence on B8):
> … in-memory for one-shot CLIs, tests and Boon-generated RTL (power-on resets every HOLD to its starting value); a console board profile with a state journal is a deployment with a store. Dev hot reload keeps every leaf whose identity and type are unchanged; a leaf whose type changed is reset to its starting value in dev and listed in the dev window, and it is a migration error in a release build. Sensitive host inputs (password text and the like) stay in a host-owned buffer; Boon code sees a host reference, never plaintext, so they are never app memory (BOON_PERSISTENCE_ARCHITECTURE_PLAN.md "Sensitive Input And Credentials"). *(Owner item, SEM-032: the hot-reload clause replaces D35's "dev hot reload always keeps state"; confirm before applying.)*

### §1 D36 (line 105): HEAD/OPTIONS and transient sends
*(Specification addition, not pure wording: HEAD and OPTIONS are not named in D36. It follows from D36's own criterion, HTTP method semantics, and is marked here so the owner can strike it.)*
Findings: SEM-019, SEM-025.
Replace: "`Http/get` is a query (…); `Http/send` (POST, PUT, PATCH, DELETE) is a command (copy contexts only, exactly once per update)."
With (the "transient" clause is conditional on QUESTIONS.md B2):
> `Http/get` (and HEAD and OPTIONS) is a query (…); `Http/send` (POST, PUT, PATCH, DELETE) is a command (copy contexts only, exactly once per update), transient by default: a send in flight at a crash is not replayed and delivers its `Interrupted` outcome after restore.

## §3 Diagnosis

### §3.1 (lines 149-151): the cause of per-path solving, as the EVI-001 refuter worded it
Findings: EVI-001 (dropped as a finding, kept as a low diagnosis-accuracy edit; its probe `latest_12.bn` was re-run by the refuter: 12,261 frames, 508 ms).
Replace: "The root cause is semantic. Results were narrowed per call by singleton tags, joins were non-monotone, and field order was part of type identity. D4, D5 and D7 remove all three."
With:
> The mechanism is per-path copying without generalization: the kernel copies callee constraint graphs per call path, keyed by the caller's variable ids, and direct summaries re-evaluate callee summaries on every activation. A 66-line program with one LATEST at the leaf of a 12-level call tree, with no WHEN, joins or records, costs about 500 ms and 12,261 frames. The old semantics (per-call narrowing, order-sensitive requirement folds, retractable requirements) made generalization unsafe; D4, D5 and D7 remove that obstacle, and §4.4's scheme compaction with one instantiation per syntactic site removes the mechanism. The `latest_K`, `chain_K` and `view_K` generator families join the §6 stress corpus.

### §3.1 (lines 152-153): the 340 ms figure is a timer, not a subtraction
Findings: EVI-013 (the 9-14% shares are to be re-measured before adoption).
Replace: "Deleting every plainly incidental step would still leave TodoMVC diagnostics at about 340 ms, against a 75 ms budget."
With:
> In the diagnostics lane incidental work is only 9-14% of TodoMVC's time (profile.md). Deleting all of it, plus every non-solver step of typecheck, still leaves about 290-355 ms of kernel solving on TodoMVC, against a 75 ms budget.

### §3.3 (lines 174-177): count representations, not structures
Findings: EVI-014.
Replace: "About 62 program representations and at least 27 conversions." and "Five type encodings, four string interners, …"
With:
> About 40 whole-program representations (62 catalogued structures, including 10 proof layers and 14 indexes, catalogs, caches and exports), at least 27 conversions, five or six type encodings (rich Type/FlowType, kernel TypeTerm, ArtifactTypeTermV1, DataTypePlan, PlanValueType), three string interners, three call-expansion passes and at least six ID spaces.

### §3.4 table, row "Typecheck orchestration" (line 186)
Findings: EVI-013.
Replace: "30-70% of the phase is outside the solver:"
With:
> Outside the solver: 16% (TodoMVC diagnostics) to 57% (NovyWave verified) and 85% (counter verified):

## §4.2 Front end

### §4.2 Expressions (lines 293-297): what the left side of `|>` is
Findings: MIG-019.
Replace: "Expressions. D11 precedence; comparisons do not chain; a pipe form is the only thing allowed on the right of `|>`."
With:
> **Expressions.** D11 precedence; comparisons do not chain; a pipe form is the only thing allowed on the right of `|>`. On the left of `|>`, a whole binary expression is the pipe input: `a == 0 |> f()` is `(a == 0) |> f()`, which does not count as mixing operators under D11. The 182 example sites of that shape (NovyWave 160) are a named corpus in P1b's tree-equivalence test.

### §4.2 Definitions (after line 315): nothing that reaches output is ordered by session ids
Findings: SEM-047, ARC-018; research/incremental_compilation_subframe_latency.md item 4.
Replace: "Anything that ends up in a plan is hashed from name bytes and structural routes, never from session SymbolIds."
With:
> Anything that ends up in a plan is hashed from name bytes and structural routes, never from session SymbolIds. Nothing that reaches output is ordered by SymbolId either: canonical rows, tag sets and diagnostics sort by name bytes or source position, because session SymbolIds depend on edit history.

### §4.2 Recovery (lines 306-308): make recovery normative
Findings: MIS-012, ARC-019, MIS-020; research/incremental_compilation_subframe_latency.md items 6 and 13.
Replace: "**Recovery.** Every unit reports all of its errors. Recovery is a pure function of the text, so warm and cold results match. Today the first parse error aborts the whole project."
With:
> **Recovery.** Every unit reports all of its errors. Recovery is a pure function of the text, so warm and cold results match. L1-L4 govern valid programs; in error mode, recovery may also use indentation. Sync points:
> - a line at column 0 that starts with `name:` or `FUNCTION` closes every open construct, TEXT raw mode included, with one "unclosed X" diagnostic anchored at the opener;
> - an unclosed opener is closed implicitly before the first line indented no deeper than the opener's line that does not start with its closer, with one diagnostic per implicit close;
> - inside brackets, recovery skips to the next separator at the same depth;
> - a missing operand or call entry becomes an Error node and parsing continues.
>
> Nesting deeper than a fixed limit (at least 20 k levels, or an explicit stack) is a positioned "too deeply nested" diagnostic, never a stack overflow. The warm path keeps the notes' fenced reparse (frontend_session.md §1.4) as its fallback. Today the first parse error aborts the whole project.

(conditional on QUESTIONS.md B16) Add to L4: "(f) no line inside a multi-line TEXT starts at column 0 (all 5 current blocks comply); this is what makes column-0 resync sound."

### §4.2 Tooling (lines 333-334): rebuild `format`, do not port it
Findings: MIS-015.
Replace: "`format` ports `format_source_unit`."
With:
> `format` is rebuilt on the boonc_syntax token stream; today's line-based `format_source_unit` rewrites multi-line TEXT contents. Raw-mode TEXT spans are preserved byte for byte. `format` is idempotent, and parse(format(x)) equals parse(x), evaluated TEXT constants included, on every tracked .bn file (an O1 fixture). It applies D11 parenthesis fix-its only on request. `highlight` classes `--` inside TEXT as text.

## §4.3 Typing rules

### §4.3 Types (after line 351): collections, ports, finite types, Optional fields, widths, catalog tags
Findings: SEM-040, SEM-041, SEM-042, MIS-016, MIS-017, SEM-043.
Replace: (insert after the "Join" bullet)
With (the "Finite types" bullet is conditional on QUESTIONS.md B13, the BYTES capacity clause on B12):
> - **Collections and ports.** LIST<E>, SET<E> and MAP<K,V> join covariantly (E ⊔ E'); ports join as PORT(ρ⁺ ⊔ ρ'⁺, ρ⁻ ⊓ ρ'⁻). A join, a WHEN/WHILE/LATEST result, and a FUNCTION parameter or result give a read-only view (D28). Only a collection authority reached by a static root path accepts event-driven updates; a write through a view is an error ("writes must name the authority"). Element writes are checked against the authority's element type, which is the join of its literal items and all its writes. spec.md §1.5's unification of element variables is withdrawn.
> - **Finite types.** Types are finite trees. A state cell, collection element or effect payload whose type would contain itself is a "recursive shape" error that lists the path; trees are written as rows with parent keys or as nested literals of fixed depth.
> - **Optional fields.** An Optional field is consumed by a contract that lists it as optional, or by a defaulting spread `[f: default, ...r]`, which makes `f` Required with type default ⊔ r.f. There is no presence test. The missing-field diagnostic names the producer that lacked the field and, for HOLD or collection state, proposes `[...previous, f: …]`.
> - **Widths and BYTES.** The width and BYTES static rules are spec.md:24, 44-45, 186, 209-214, 543 and 639-645, with width equality as a residual scheme predicate (checker.md:75); P0 writes them into static semantics v2. A HOLD whose starting value is BYTES[N] has fixed capacity N, so a BYTES[M] update with M ≠ N is an error.
> - **Catalog tags.** Outcome tag names are consistent across the whole catalog under D18: a name is bare everywhere or carries a payload everywhere.

### §4.3 Functions (lines 363-366): activation row and update-kind terms
Findings: SEM-023, EVI-005, EVI-002; research/query_command_effect_systems.md item 10.
Replace: "Each is checked once into a scheme: parameter predicates, an implicit **PASSED** requirement row, OUT scope effects and collection-write effects."
With (the last sentence is conditional on QUESTIONS.md B5):
> Each is checked once into a scheme: parameter predicates, an implicit **PASSED** requirement row, OUT scope effects, collection-write effects, an **activation row** and **update-kind terms**. The activation row is the union over the body and its callees of: creates state (HOLD, SOURCE, stateful builtin, D26 library functions included), runs a live query, issues a command, each with its position (live under the caller's context, copy, or live). Update-kind terms give each parameter path a kind variable; "never runs" and "has no start value" become scheme predicates checked at instantiation and reported at the call site with a note into the callee, like polymorphic exhaustiveness. The body inherits its call site's context: at a live call site its plain reads fire, at a copy call site they copy. SOURCE declarations and effect sites inside a FUNCTION are generative trigger roots, fresh per instantiation, so two call instances are different triggers for the LATEST rule.

### §4.3 WHEN and WHILE (after line 376): residual binders, no kind elimination, §5.3 withdrawn
Findings: SEM-037, SEM-039, SEM-038.
Replace: (append to the list)
With (the last bullet is conditional on QUESTIONS.md B14; it is a syntax addition, so if syntax stays frozen the fix-it lists the full binder set instead):
> - A binder or `__` arm after tag arms has the subject type minus the variants those arms matched; literal arms remove nothing. The binder is a new name, so the subject is still not refined (D10). `color |> WHEN { Oklch[lightness] => …, other => TEXT { {other} } }` types `other` as TEXT. This is the original Boon rule (ERROR_HANDLING.md:333-344, spec.md:373).
> - Kinds cannot be eliminated: a NUMBER | TEXT value can be displayed, compared, forwarded and passed to a contract, but not computed with. The diagnostic's fix-it is "give each arm a tag".
> - spec.md §5.3 (stable-path refinement and per-variant selector requirements) is withdrawn; payload fields are reached only through `Tag[binders]`.
> - A tag pattern may bind the matched variant whole, `page: HierarchyPage => pass(page: page)` (proposed syntax), typed as that single variant.

### §4.3 Change rules (new, after line 379): the edge table
Findings: SEM-001, SEM-004, SEM-005, MIG-002; research/copy_vs_live_synchronous_languages.md item 3.
Replace: (insert as the first item of "Change rules")
With:
> **Edges.** Every read is a FIRE edge (an update of the source re-runs the reader) or a COPY edge (the reader samples the committed value when something else runs it). D9 edges are labels on edges, independent of FIRE and COPY.
>
> | construct | FIRE edges | COPY edges |
> | --- | --- | --- |
> | plain expression, record field, TEXT interpolation, LIST item, document slot | every operand | none |
> | THEN | its input | every read in its body |
> | WHEN | its selector | every read in its arms |
> | WHILE | its selector; every read in the selected arm | none |
> | HOLD | its piped input (a D33 reset); its body's result into the state (**HOLD update edge**, D9) | the binder read inside its body |
> | read of a HOLD or collection from outside it | the read | none |
> | LATEST | every arm | none |
> | collection update (List/append, List/replace_all, row update) | its item/with value; the update into the collection (**collection update edge**, D9) | none of its own |
> | collection element field `L[*].f`, membership `L[#]` | a list operation that reads `item.f` fires from `L[*].f` and `L[#]`; `L[#]` depends only on construction and event-driven updates | none |
> | stateful builtin (D26 residue) | its inputs | its state read (**stateful-builtin state edge**, D9) |
> | SOURCE payload, effect result | the read | none |
> | live query | its arguments | none |
> | command, copy-context query | none (it runs from its copy context) | its arguments |
> | FUNCTION call | as its call site's context (§4.3 "Functions") | as its call site's context |

### §4.3 Change rules, "Updates" (lines 380-383) and new "Steps" (conditional on QUESTIONS.md A2)
Findings: SEM-004, SEM-005, SEM-002, SEM-003, MIS-002, MIS-003; research/copy_vs_live_synchronous_languages.md items 2 and 9.
Replace: "A derived value fires at most once per step when any input fires."
With:
> - **Updates.** Every write fires, equal or not (D32). A derived value fires at most once per tick when any input fires. Start, restore, hot reload, entering a WHILE arm and creating a row are not updates. Every value keeps its last value.
> - **Steps.** A tick ingests exactly one external occurrence (a SOURCE occurrence, an effect completion or a timer tick); hosts queue the rest for later ticks. Within a tick each node evaluates at most once, in topological order of FIRE edges (rank = longest FIRE path from the tick's occurrence; HOLD and collection commits are ordinary ranked nodes), and only after every FIRE input has settled for the tick. Live (FIRE) reads see this tick's settled values; a copy body reads the pre-tick snapshot plus its input's new value (D21 as written), so commits become visible to copies at the next tick. No result depends on declaration order: the swap `x: 1 |> HOLD x { press |> THEN { y } }` with `y: 2 |> HOLD y { press |> THEN { x } }` gives x = 2, y = 1. No reader sees an intermediate state: with `c: 0 |> HOLD c { a |> THEN { a * 10 } }` and `x: a + c`, `x` fires once per press and never shows a new `a` with an old `c`. The document and commands see only the settled tick. Runtime-counted pulse sources (Stream/pulses) run one pulse per settle step under the target's runtime pulse ceiling; whether the steps of one activation complete within one tick (no intermediate frame) or across ticks is decided by spike S7 (QUESTIONS.md A9). On a Wasm turn, commits stay private to the turn. Whether a same-tick chain in RTL is one clock edge or one cycle per stage is deferred to the hardware plan, in writing, with one edge as the default (QUESTIONS.md B11).

### §4.3 Change rules, new "Activation" (conditional on QUESTIONS.md A1; the History nodes bullet on A8)
Findings: SEM-007, SEM-008, MIG-005, SEM-010, SEM-026; D12.
Replace: (insert after "Steps")
With:
> - **Activation.** Activation (start, restore, hot reload, row creation, entering a WHILE arm) is not an update. At activation every live expression is evaluated from the current values of its inputs, and every WHEN whose selector has a value evaluates its selected arm once, copying the current snapshot; nothing downstream fires and no command runs. A WHEN over a selector with a value from the start therefore has a value from the start. THEN bodies never run at activation, and occurrences are not replayed. Commands are allowed in THEN bodies and in WHEN arms whose selector gets its first value later (SOURCE payloads, effect results, THEN outputs); a command in a WHEN arm over a selector with a start value is an error with the fix-it "use THEN". A FUNCTION parameter bound to a constant at a call site has a value from the start for that instance.
> - **History nodes.** A history node is a value that activation cannot recompute: a LATEST, a THEN output, a WHEN output whose selector has no start value, an occurrence payload, a command result. D12's "last value … that code reads outside its own update" means exactly the history nodes with a reader other than their own copy consumer (a THEN or WHEN whose input is that node, a HOLD whose piped input is that node, an arm of a stored LATEST). Values computed live from stored leaves are recomputed at activation and never stored.

### §4.3 Change rules, "Copy contexts" and "Live contexts" (lines 384-389)
Findings: SEM-007, SEM-009, SEM-015, SEM-013, SEM-014, SEM-016, EVI-005.
Replace: "**Copy contexts: THEN bodies and WHEN arms.** The body runs once each time the input updates, … "never runs"). **Live contexts: plain expressions and WHILE arms.** They follow every update of what they read while selected."
With (the WHEN-at-activation clauses depend on QUESTIONS.md A1, the WHILE-in-copy sentence on A10):
> - **Copy contexts: THEN bodies and WHEN arms.** The body runs once each time the input updates (and, for WHEN, once at activation), reading the committed snapshot (D21) plus the input's new value; nothing else it reads re-runs it. THEN over something that never updates is an error ("never runs"). A WHEN over a selector that never updates is evaluated once at activation, and its unreachable arms get a hover note (D25; a warning only where a fix-it can remove the arm). Inside a copy context a nested WHEN selects once over copied values; a nested THEN is an error ("this THEN is inside a copy and can never run again; move it outside or use LATEST"); a nested WHILE is a live scope for one activation, from the copy run until the next update of the copy's input. A copy body that reads a value with no value yet produces no update, like SKIP: nothing downstream fires and no command is issued; the checker warns when the read value's fire roots are disjoint from the trigger's. A collection read in a copy context is an O(1) snapshot of its committed revision and never becomes an authority.
> - **Live contexts: plain expressions and WHILE arms.** They follow every update of what they read while selected. A WHILE arm stays selected while the selector's new value selects the same arm; re-selecting it is not an activation. State declared in a WHILE arm exists whether or not the arm is selected and is not reset on re-entry (D25, D32); live queries in the arm stop on deselection and restart on re-entry.

### §4.3 Change rules, "Placement" (lines 390-393): through calls
Findings: EVI-005, SEM-023, MIG-016; research/query_command_effect_systems.md items 5 and 10.
Replace: "Commands belong to copy contexts only (D31). This also answers L6."
With:
> Commands belong to copy contexts only (D31). The rule applies through calls via the activation row (§4.3 "Functions"): calling a FUNCTION that creates state, D26 library functions such as `Bool/toggle` included, from a copy context is the same error, and a FUNCTION that needs both a live and a copy position for one context is an error at its definition. The checker traces each command's trigger to its roots and warns when a root is a live-query completion ("runs again after every restart because `Wellen/open` re-runs"). This also answers L6.

### §4.3 Change rules, "LATEST" (lines 394-397): fire sets, ties, zero arms
Findings: SEM-005, SEM-006, SEM-002, SEM-048, MIS-003.
Replace: "Two arms that can update from the same trigger in the same step are an error. A one-input LATEST is an error: remove the wrapper."
With (the tie clause is conditional on QUESTIONS.md A2):
> Each arm has a fire set, the occurrences it can fire from, computed in the update-kind pass. Two arms whose fire sets share an occurrence are an error, at any rank, since the tick is the step (so persons_pro `publish_state` is restructured). An effect result is its own occurrence: it never updates in the tick that started its run. A one-input LATEST is an error: remove the wrapper. A zero-arm LATEST means "no writes" and is legal as a HOLD body (the HOLD keeps its piped value), notably in DRAINING declarations; the one-input fix-it never applies to it.

### §4.3 Change rules, "HOLD" (lines 398-400): same-tick reset against body
Findings: SEM-003, MIS-003.
Replace: "its own writes do not re-trigger it."
With (conditional on QUESTIONS.md A2):
> … its own writes do not re-trigger it. A piped input and a body candidate fired by one occurrence are an error, like a LATEST tie (fix-it: move the reset into the body's LATEST); "reset dominates" is the fallback if the owner declines.

### §4.3 Change rules, new "FLUSH" (after line 404; conditional on QUESTIONS.md B3)
Findings: EVI-004 (as corrected), SEM-020.
Replace: (insert after "No value yet")
With:
> - **FLUSH.** A FLUSH inside a HOLD update lands at the enclosing field boundary (D29). The HOLD's state is unchanged and not persisted, and the binder always reads the stored state; the field shows the error value and fires once as an update. FLUSH in a HOLD initializer or a LATEST starting arm is an error. If an argument or the selecting arm of an effect call FLUSHes, the call does not run and the effect log records `skipped(flush)`; a live query whose argument becomes flushed cancels its run in flight and restarts when the argument next has a value. DRAIN sources and the path to their destination contain no effect calls. `flush_error_propagation.scn` is the fixture.

### §4.3 "Fire edges never close a cycle" (lines 405-408) and "Cycles (D9)" (lines 417-424): one rule (conditional on QUESTIONS.md A5)
Findings: SEM-001 (with its correction), SEM-002, MIG-002, EVI-005; research/copy_vs_live_synchronous_languages.md items 3 and 11, research/incremental_compilation_subframe_latency.md item 3.
Replace: the "Fire edges never close a cycle" bullet and the four "Cycles (D9)" bullets.
With (delete the bullet at 405-408; the Cycles block becomes):
> **Cycles (D9).**
> - There is one labelled dependency graph with the edges of "Edges", including local nodes, state cells and collection element nodes `L[*].f` and `L[#]`.
> - Every dependency cycle must contain a D9 edge (a HOLD update edge, a collection update edge or a stateful-builtin state edge), and the graph of FIRE edges must be acyclic. So a HOLD's piped input never closes a cycle, and a cycle closed only by a THEN-body or WHEN-arm copy is an error (D9: not THEN bodies).
> - Any other cycle is a positioned diagnostic that lists the path. Programs that are acyclic at runtime but cyclic in structure are rejected on purpose.
> - Phase A's Tarjan pass runs on exactly these labelled edges, per instance, in the diagnostics lane; SCCs are recomputed from the current revision on every compile.
> - Fixtures: the counter `count: 0 |> HOLD count { press |> THEN { count + 1 } }` (legal); the swap of "Steps" (legal); `a: b |> HOLD a { press |> THEN { a + 1 } }` with `b: a + 1` (error, piped input); `a: 0 |> HOLD a { b |> THEN { a + 1 } }` with `b: a |> THEN { a }` (error); `cells_evaluator_reads_sheet_values.bn` and `cells_rows_read_other_rows_hold.bn` (accept); `cells_row_reads_other_row_derived_result.bn` (reject with the path).

### §4.3 "Stale-copy hint" (lines 409-412) (conditional on QUESTIONS.md A4)
Findings: SEM-012, SEM-011, MIG-007; research/change_semantics_frp_ui_excel.md item 7.
Replace: the whole bullet.
With:
> - **Stale copies.** `x |> WHEN {…}` where `x` has a value from the start and an arm reads a value whose fire roots are not all fire roots of `x` (reads through callees and PASSED count; effects in arms are exempt) is an error: "this WHEN copies `mode` when `name` updates; use WHILE to follow `mode`". A WHEN whose input gets its first value later may read anything; it decodes or copies on purpose. A deliberate snapshot on a state update is written `initial |> HOLD s { x |> THEN { … } }`. Hover lists what a WHEN copies ("copies `todos` when `selected_filter` updates") and what a WHILE follows. After D30, WHEN answers an update and WHILE follows a state.

### §4.3 update-kind sentence (lines 414-415)
Findings: SEM-005, SEM-006; research/copy_vs_live_synchronous_languages.md item 7.
Replace: "Update kinds are computed in one forward pass in dependency order, seeded at constants, HOLD, stateful builtins, SOURCE and effect results."
With:
> Update kinds are computed in one value-independent forward pass in dependency order, seeded at constants, HOLD, stateful builtins, SOURCE and effect results. The same pass computes each node's rank and fire set ("Steps"); a SOURCE occurrence and an effect result are their own roots.

### §4.3 new "Effect classes" (after the Change rules)
Findings: SEM-019, EVI-012, MIS-026, SEM-016, SEM-017, SEM-018, SEM-006, SEM-010.
Replace: (new paragraph; P0 catalog deliverable)
With (the table is conditional on QUESTIONS.md B4 for the observer and host-source classes, the supersession sentence on B1, the restart column and hover sentence on A7):
> **Effect classes (D31, D36).** Every effectful catalog entry has one placement class, an explicit column in host_effect_policy with the same explicit-match discipline.
>
> | class | contexts | runs | initial entries |
> | --- | --- | --- | --- |
> | host source | live only | starts when its scope activates; arguments never restart it; missed ticks are never replayed (D35) | Timer/interval, Router/route, host level ports, `restored` |
> | query | live: a resource; copy: once per update | live: restarts on every update of an argument (D32), cancels the run in flight, keeps its last answer | `File/read_*` (read_stream included), Directory/entries, Content/import, `Wellen/*`, Timer/deadline, Http/get (HEAD, OPTIONS), Secret/verify, `Crypto/hmac_*` |
> | command | copy only | exactly once per update with copied arguments; never cancelled or superseded; each completion is an update, in completion order | `File/write_*`, Content/save, Http/send, Router/go_to, Clock/wall, Random/bytes, `DevelopmentPasskey/*` |
> | observer | any | on every update of its argument, the first included; never on start or restore; never persisted | `Log/*` |
>
> One run may deliver several result updates (stream entries); each is an update, and a restart or cancel ends the stream. An arm or record that contains a command's result has no value until the command answers. A **scope** is the program (plain fields), a WHILE arm while selected, a collection row while it exists, or a FUNCTION call instance while its caller's scope is active. A live query starts when its scope activates, restarts once per tick when any argument updated in that tick, and cancels on scope exit. On hot reload a live query whose call-site identity and argument expressions are unchanged keeps its run and its last value; a new or changed site starts; a removed site cancels; commands are untouched. Copy-context queries do not supersede each other unless the entry is marked `superseding` (initially only Timer/deadline, which makes `text |> THEN { Timer/deadline(delay_ms: 300) }` a debounce). Hover on a live query says "restarts on every update of `offset`; keeps the answer for the previous arguments while restarting", and on a HOLD piped from a live query "reset by every answer, including after a restart" (A8).

### §4.3 new "State classes" (after "Effect classes")
Findings: MIS-003, SEM-026, EVI-008.
Replace: (new paragraph)
With:
> **State classes.** Persistence (§4.5), Phase A, identity_v1 and the backends refer to this table.
>
> | class | holds | declared |
> | --- | --- | --- |
> | HOLD | its state | by the user |
> | collection authority | its rows | by the user |
> | stateful builtin (D26 residue) | its state | by the catalog |
> | LATEST | its current value | implicitly |
> | THEN output; WHEN output over a selector without a start value | its last value, and whether it has one | implicitly |
> | SOURCE payload | its last payload, and whether it has one | implicitly (host) |
> | live query | its last answer while restarting; never stored | implicitly |
> | derived live value; WHEN over a start-valued selector | nothing (recomputed at activation) | none |
>
> P0 asks the owner to retire PRIORITY and EXCLUSIVE (RUNTIME_MODEL.md:86, LANGUAGE_SEMANTICS.md:407); no example uses them.

### §4.3 Names (lines 427-430): the HOLD binder and the element binder
Findings: SEM-015, MIG-012.
Replace: "Lookup goes through lexical scopes, innermost first: record-literal siblings, BLOCK locals, WHEN binders, OUT/call-context binders, FUNCTION parameters."
With:
> Lookup goes through lexical scopes, innermost first: record-literal siblings, HOLD binders (a binder may differ from its field's name), BLOCK locals, WHEN binders, the constructor `element` binder (the kind's element state, visible in every argument of an element constructor except `element:`), OUT/call-context binders, FUNCTION parameters.

### §4.3 Contracts (lines 445-450): the exactness rule (conditional on QUESTIONS.md A3)
Findings: SEM-035 (with its correction), MIG-011, EVI-003, SEM-036; research/structural_inference_exact_records.md items I1-I2.
Replace: "User FUNCTIONs: record parameters are **exact** (D23). … P0 specifies how exactness composes through spreads and forwarding."
With:
> - User FUNCTIONs: record parameters are **exact** (D23). The exact set of parameter `p` of `f` is the least fixpoint over the call DAG of: `p`'s direct reads; the exact sets of the callee parameters `p` is forwarded into whole; for a spread `[…, ...p]` into an exact position, that position's set minus the explicitly written fields; a contract's key set, for forwards into contracts; and the demand of every consumer the value reaches through `f`'s result, state or a collection.
> - Exactness is checked at **entry points**: where a value that is not a parameter (a literal, a root value, state, a row, an effect result, a spread result) enters an exact position, its field set must equal that position's exact set. The fix-it narrows the argument to a literal of the consumed fields, for example `file_tree_row_label(row: [scope_key: row.scope_key, expanded_label: row.expanded_label, …])`. Forwarding a parameter, or a spread of one, into a smaller exact set is an implicit projection, not an error. The implicit PASSED row stays open.
> - Because result flow makes the exact set consumer-dependent, exactness is a deferred predicate checked after solving (§4.4), never a closed row inside the solver.

### §4.3 Contracts (after line 450): slots, `None`, accessibility keys
Findings: SEM-045, MIG-010, MIS-024.
Replace: (append)
With (the `None` bullet is conditional on QUESTIONS.md B15):
> - A slot accepts a union of element kinds and NoElement. A contract diagnostic at a slot carries the construction span of the offending element.
> - Colour-, border- and glow-typed style keys accept the tag `None`, meaning unset; the renderer treats it as absent explicitly, and the generated contract documents it.
> - The catalog's completeness test covers accessibility keys (roles, labels, heading levels, lang, selection, sensitive flags).

### §4.3 Diagnostics (after line 463): data model, suppression, one root cause
Findings: MIS-013, MIS-014, ARC-019; research/incremental_compilation_subframe_latency.md item 6.
Replace: "- Code after Phase A keeps only debug assertions."
With:
> - Code after Phase A keeps only debug assertions.
> - Every diagnostic is `Diagnostic { code, severity, primary, related: [(span, label)], message (lazy), fixits: [TextEdit], meaning_preserving }`. A warning is emitted only with a meaning-preserving fix-it; otherwise it is an error.
> - Error is expression-level: the parser emits an Error node per unparsed region, and the rest of the definition is checked normally. Error is top and bottom for constraints, and any conflict with Error on either side, at any depth, is dropped. An Error node poisons every property derived from it (type, update kind, consumed-field set, binder use, OUT production); rules that read a poisoned property stay silent, which covers D24, D23 exactness, exhaustiveness, homogeneity, "never runs" and contract checks on Error-typed values. A name unresolved only because recovery swallowed its region stays silent. Every Error node traces back to exactly one diagnostic (a debug assertion).
> - One predicate violation reached through N call sites is one diagnostic at the definition, with a related span at the first call site in position order and "(and N-1 more)".
> - Requirement-row fields carry the consumer's expression, so a PASSED conflict is reported at the read (QUESTIONS.md B17).

### §4.3 Display (lines 468-470): display order is never reused
Findings: SEM-047, ARC-018.
Replace: "One printer serves hints, the inspector and diagnostics."
With:
> One printer serves hints, the inspector and diagnostics. Display order is never stored in reused check results: the printer computes it at publication from the current presentation tables of the producing definitions, and it is part of no reuse key. Across definitions, first appearance is ordered by (Config unit order, byte offset). The printer is a pure function of (TypeId, side table), so O5 compares rendered text; the warm-corpus class "reorder-fields" checks it.

## §4.4 Checker algorithm

### §4.4 Approach (lines 476-478): uses and verdicts
Findings: SEM-036, EVI-003.
Replace: "Types depend only on producers, so a use never changes a producer's type."
With:
> - A use never changes a value's lower bound; exactness and the other consumer-side checks read the final bounds.
> - The solver only adds bounds. Verdicts that can flip as bounds grow (D23 exactness, concreteness and exhaustiveness, D24 unused names, the LATEST start-value rule) run in a post-solve pass per definition, and the root group runs last. A root destination that a FUNCTION writes or forwards into enters its scheme as an outer monomorphic variable, like a root read; exactness against it is checked after the root group solves, so the scheme's reuse key includes those outer variables.

### §4.4 Per function (lines 487-489): the normal form and shared instantiations
Findings: SEM-046, SEM-044.
Replace: "3. Linear compaction into a canonical, hash-consed scheme." and "4. Instantiation exactly once per syntactic call site. It copies only nodes that contain variables."
With:
> 3. Normalization, then hash-consing: substitute variables that occur only positively by their lower bounds and only negatively by their upper bounds; merge variables that always co-occur with the same polarity; number entry variables by first occurrence in (parameters, PASSED, outer reads, result); sort residual predicates by (kind, entry indices). SchemeId equality is the early-cutoff key, and the confluence oracle compares normalized schemes.
> 4. Instantiation exactly once per syntactic call site. It copies only nodes that contain variables, and non-ground terms are hash-consed modulo variable renaming, so identical instantiations share nodes. A port bound at its own constructor is ground and does not generalize.

### §4.4 Representation and Diagnostics (lines 500-503): open rows, consumer-consumer conflicts
Findings: SEM-036; research/structural_inference_exact_records.md item I3.
Replace: "Ports are polarized (`PORT(ρ⁺, ρ⁻)`). The solver has no union-find." and "Conflicts are order-independent (consumer, producer) pairs."
With:
> Ports are polarized (`PORT(ρ⁺, ρ⁻)`). The solver has no union-find. Parameter and PASSED rows are open (width subtyping) in the solver; where closed rows are used at all, two closed rows with different field sets meet to a conflict (Required ⊓ Absent is empty, Optional ⊓ Absent is Absent), and the meet-is-glb property test covers that case. … Conflicts are order-independent (consumer, producer) pairs, plus (consumer, consumer) pairs when two requirements on one variable have an empty meet (two different exact field sets, TEXT against NUMBER); those are reported at the definition with both spans, and the call-site errors they cause are suppressed.

### §4.4 Distributed programs (after line 516): the wire (conditional on QUESTIONS.md B6)
Findings: MIS-001 (with its correction).
Replace: (append a paragraph)
With:
> **Wire.** Every tick's write of an export crosses the wire, in order (under §4.3 "Steps" a producer writes an export at most once per tick). As an unobservable optimization, an import none of whose transitive consumers reaches a fire position (THEN input, WHEN selector, HOLD piped input or update, LATEST arm) may be coalesced to its latest value. This replaces the Value/Event exports. A cross-role call is a D31 query: live in live contexts, once per update in copy contexts, superseded per call site, re-run on restore; Current/Invocation map to live/copy. An imported update is applied in the consumer's next tick. Stores per role follow D35 (Server durable; Session and Client by deployment). The server `outputs` root and `host_ports` get catalog contracts. Runtime item R10 (§4.5).

### §4.4 Oracles and Model (lines 518-528): verdicts after the fixpoint; S2 measures
Findings: EVI-003, ARC-020, ARC-009, ARC-010, SEM-044, ARC-023.
Replace: "Confluence: shuffling the order must not change the result." and "Spike S2 re-derives the model. Targets: ≤6 ms TodoMVC, ≤20 ms NovyWave."
With:
> Confluence: shuffling the order of producers and consumers must not change the result, and verdicts are asserted only after the fixpoint. … Spike S2 measures rather than extrapolates: the prototype checker on the S2 inputs under the compile-bench protocol (§6). The targets ≤6 ms TodoMVC and ≤20 ms NovyWave are 5x and 4.4x checker.md's model and beyond its own ≤4 / ≤12 ms target. Pass at ≤6 / ≤20 ms; at 6-12 / 20-40 ms the owner re-baselines with a recorded reason; above 12 / 40 ms the element representation (D13, D20) is revisited before P2a. S2 also reports ns per principal expression, the ground-slot share, the largest scheme, the instantiation copy count (the non-ground scheme size actually copied, summed over call sites, against the source element count), the root group's share of NovyWave check time, and the fraction of single-token view edits that change the edited function's interface fingerprint, with the mean cone size.

## §4.5 Lowering and the runtime delta

### §4.5 Phase A (lines 540-542): list access and target limits
Findings: MIS-018, MIS-019, MIS-002.
Replace: "- target-profile limits keyed on `Config.target`. These are the checks that are user-reachable only inside `verify_plan` today: typed-index capacity and declared capacity."
With:
> - list-access planning (typed-index selection and key multiplicity), a pure function of the typed program and `Config.target`, so capacity diagnostics stay in the diagnostics lane;
> - target-profile limits keyed on `Config.target`, split into static checks (typed-index capacity, declared capacity, the static tick bound from the FIRE graph) and runtime ceilings that stay target-owned (pulses per activation, index key and payload limits on restored values).

### §4.5 Phase B (line 557): prove DependencyEdge redundancy with a script
Findings: ARC-016.
Replace: "DependencyEdge ops are dropped, once a checkpoint has proven them redundant."
With:
> DependencyEdge ops are dropped after a one-off script over dump-plan output of all 177 tracked sources (fjordpulse and the distributed examples included) shows that every DependencyEdge's inputs are a subset of another op with the same output; the script and its report are cited in the S1 report.

### §4.5 Persistence (lines 568-589): rewrite from D12 and D35
Findings: SEM-026, SEM-008, MIS-003, SEM-024, SEM-025, SEM-028 (with its correction), SEM-031, ARC-016, MIS-010, MIS-006, SEM-030, SEM-029, MIS-009, MIS-007; research/state_restore_persistence.md items 2-4.
Replace: from "**Persistence (D12).**" to "Migration predecessor stages compile in a `persistence_only` mode. The cache key is (stage texts, schema version, predecessor key)."
With (clauses marked Qn depend on that question):
> **Persistence (D12, D35).**
> - **Durable leaves** are the stored classes of §4.3 "State classes": HOLD state (HOLDs inside D26 library functions included); stateful-builtin state; authoritative collection rows, with every row field that is itself a durable leaf; and every history node read outside its own update (§4.3 "Activation", A8).
> - **Never stored:** host SOURCE levels and payloads, wire inputs, live-query answers and everything recomputable at activation (D35). A HOLD, LATEST or collection authority that received a query answer is a leaf like any other; the live query re-runs at activation and its first answer is an ordinary update (A8). There is no reachability and no view/model rule; element-local hover, focus and pressed are host ports, not memory.
> - **Identity** is (named owner path, state binder name). The named owner path is the chain of named fields and row keys through which the leaf's value is bound, crossing FUNCTION calls and BLOCKs without recording them. A leaf inside a library FUNCTION is (call-site owner path, library function name, its declared state name); that state name is part of the library's API, and renaming it is a DRAIN. Arm indices, list item indices and pipe-stage ordinals are never part of an identity; a leaf with no named owner is a compile error with a fix-it to name it (BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:479-484). Moving or renaming a leaf changes its identity, and DRAIN/DRAINING carry it. Moving state into or out of a WHILE arm changes its identity, so it resets unless a DRAIN names it; the removal warning below offers that fix-it (B10).
> - An inferred leaf (a stored history node that is not HOLD or collection state) also fingerprints its producing expression; when that expression changes, the stored copy is dropped at activation and the node has no value until its next update (B10).
> - `identity_v1` covers MemoryId, leaf ids (LATEST and last-value leaf kinds included), type fingerprints (including Optional presence), `semantic_schema_hash`, DRAIN recipe and edge ids, EffectId, and deletions as tombstones. It keeps SHA-256 over small canonical inputs through `boon_plan` constructors, pinned by golden vectors.
> - Per-compile ids (output roots, distributed and wire ids) may switch to a 128-bit non-cryptographic hash. Nothing hashes a whole program or a whole plan during a compile.
> - `PERSISTENCE_FORMAT_VERSION` is bumped. `boon_plan` accepts formats 5 and 6 for migration predecessor plans until P8 (`validate_for_application`, `verify_plan`); stored data resets once (D12, D19).
> - Dataflow firing is not store I/O: the persistence layer may skip a durable write whose canonical bytes equal the stored bytes.
> - The persistence plan is emitted for every target; an in-memory deployment simply never opens it.
> - Migration predecessor stages compile in a `persistence_only` mode. The cache key is (stage texts, schema version, predecessor key).
>
> **Activation sequence.** 1. Install restored leaves; nothing fires. 2. Activation evaluation (§4.3): live expressions and WHEN over valued selectors; THEN bodies, commands and occurrences do not run; live queries start. 3. Host level ports are supplied with their current values as part of activation, not as updates; later host changes are updates. 4. Publish the first frame. 5. The first tick: the `restored` occurrence fires exactly once when a store image was restored (not on hot reload, not on a fresh start), before any host occurrence; durable command intents staged before a crash are then completed under the outbox contract (idempotent replay or reconciliation), and a transient run in flight at the crash delivers its catalog `Interrupted` outcome as an ordinary completion (B2).
>
> **Schema evolution** (B9 for the predecessor form). Phase A emits a durable-schema diff against the predecessor stage in the diagnostics lane: added, removed and retyped leaves with their source paths. A removed leaf is a positioned warning in dev ("stored data for `x` will be deleted: add DRAIN or keep a reader") and an error in `boon-app build` unless the package manifest acknowledges it. DRAIN extends to every D12 leaf kind. A removed or renamed effect maps through an alias table, so pending outbox items migrate instead of blocking activation. Predecessors are sources for dev stages and frozen lowered fragments (`apps/*/migrations/catalog.toml`) inside app packages. This warning is what keeps a reader-dependent durable set (A8) from silently deleting data when a reader is removed.
>
> **Roles and scopes** (B7). Values received over a wire are host inputs for persistence: never stored; a role that needs one after a restart copies it into a HOLD. Rows of a host-supplied collection (connections, sessions) are host-keyed: leaves nested in them are transient and end with the row. Per-user or per-tenant memory is a store namespace chosen by the deployment; the host API provides namespace deletion and export and a retention period for Session-role namespaces. Encryption at rest is a deployment store option.

### §4.5 Trust (lines 591-599): name the three untrusted sites; catalog version
Findings: ARC-029, MIS-008.
Replace: "- **Loaded from outside:** `load_untrusted(bytes)` means decode + `verify_plan`, used for artifacts, app bundles and received plans."
With:
> - **Loaded from outside:** `load_untrusted(bytes)` means decode + `verify_plan`. It is called at exactly three sites: `program_core::decode_program_artifact` (stored program artifacts, persons_pro child artifacts included), artifact receipt in `boon_distributed_runtime`, and bundle load in `boon_app_package` (native and wasm32). `MachineTemplate::new_trusted` is visible only to the in-process compile paths. The executor, document runtime and reconciler return an error, never panic, on a plan that fails `verify_plan` (O6).
> - **Catalog version.** A `CATALOG_VERSION` (major.minor) is stamped in the plan header and in COMPILER_ID; `load_untrusted` rejects a newer major than the host's. Removed catalog names stay as errors with a fix-it (`Http/request` becomes `Http/get` or `Http/send`).

### §4.5 Format (lines 601-602): decide v11+delta or v12 at S1 exit
Findings: ARC-015 (with its correction), ARC-002.
Replace: "**Format: MachinePlan v11 plus a listed delta.** D2 allows it. Each item is sized from spike S1:"
With:
> **Format.** D2 allows either MachinePlan v11 plus the delta below, or a typed v12 dataflow core (ops with `mode: Copy | Live` and identity flags, storage layout, persistence plan with identity_v1 unchanged, source routes, pulses, host ports, output roots, a typed `names` table, no document section) with a v11-to-v12 compat path that maps the old engine's WHEN ops to Live and carries its DocumentPlan to the retained v11 document runtime until P8. The choice is made at S1 exit from S1's sized delta list (QUESTIONS.md C2); either way the compat path stays correct for product use until the last example flips. Each item is sized from spike S1:

### §4.5 delta table (lines 604-614): R3, R4, R5, R7-R8 as scheduler v2, R9, new R10-R11
Findings: ARC-016, ARC-003, ARC-004, ARC-001, ARC-002, MIG-022, SEM-004, MIS-002, SEM-017, SEM-021, SEM-025, SEM-033, MIS-001, MIS-011, MIS-022; research/element_reconciliation_60fps.md items 5-6.
Replace: rows R3, R4, R5, R7, R8 and R9.
With (row R7-R8 is conditional on QUESTIONS.md A2 for the ranked core and C2 for old plans; R5's key on A8; R9's `Interrupted` on B2; R10 on B6; R11 on B8):
> | R3 | Every list row field, durable or not, gets a semantic id from its named owner path (the D12 identity scheme); cursors and bounded paging use it; persistence leaves are the durable subset | Runtime |
> | R4 | **Document model v2 (D13).** Elements are data values with a hidden stable identity (§4.6); the renderer reconciles record trees by identity into frame nodes, replacing DocumentPlan templates and constructors. Value gains Arc-shared records, lists and tagged objects so element subtrees share structure and compare by pointer (touches every Value consumer: machine.rs Value/EvalValue, boon_document values); the executor returns the same Arc for a subtree whose inputs did not fire; identity per §4.6 | Runtime + format |
> | R5 | A `List/replace_all` collection op (D17) with an optional key argument naming a row field; without it, keys are positional and hover says so | Runtime + catalog |
> | R7-R8 | **Scheduler v2 (D21, D30, D32, D33), track R-CHG.** A per-tick ranked commit replaces the recursive push dispatch (route_state_transition): each node evaluates once per tick in FIRE-rank order, live reads see the tick's settled values, and copy bodies read the pre-tick snapshot plus their input's new value (§4.3 "Steps"). A fires channel, raised on every write, is separate from a runtime-internal changed channel (§4.6). The equality early-returns in set_root_state, set_row_field, `set_activation_*` and set_transient_effect_result and the 18 `if changed` gates go. Copy vs live is a per-op flag from lowering; D33 resets; SKIP keeps the last value; activation is not an update. The target-owned pulse ceiling stays as a runtime guard, with a debug assertion that ticks stay within Phase A's static bound. Old-engine plans run on the same core with WHEN ops Live, equality dedup on and resets off until P7; snapshot reads apply to them only if S5 finds no same-tick cross-cell read in the gates and triaged scenarios. This rewrites the executor's dispatch core; S5 sizes it | Runtime behaviour + format |
> | R9 | **Effects (D31, D34-D36).** The §4.3 effect classes; commands never cancelled or superseded (the per-owner transient cancel, machine.rs:19184-19291, is removed for commands); no `Pending` injection; `Http/get` + `Http/send`; host level ports (per-element hovered, focused, pressed; pointer position and pointer-down on canvas and waveform elements; a Session `connection` level; viewport; server connection lists) and a `restored` occurrence; `Interrupted` outcomes after restore (B2); an effect log with the record {call site (definition, path, row key), catalog entry, class, context, run id, arguments with sensitive ones redacted, event ∈ started, queued, superseded(by), cancelled(scope exit, flush, restore), completed(outcome), interrupted, skipped(start, flush, no value), tick} | Runtime + catalog |
> | R10 | **Distributed wire (D30-D32, D35):** per-edge transport, cross-role calls as queries, per-role stores (§4.4). Size M, 1-2 weeks, proven by a Client+Session O2 scenario | Runtime + format |
> | R11 | **Sensitive inputs:** the host-owned buffer stays; element data, the effect log, the inspector, scenario artifacts and native reports carry a host reference or a redaction, never plaintext | Runtime + catalog |
>
> **Consumers of R-CHG and R9-R11**, each sized in S1: boon_plan_executor; boon_web_host with boon_web_effect_host; boon_server_host; boon_http_runtime and boon_http_client; boon_wellen_host; boon_host_services; boon_distributed_runtime; the boon_native_playground dev window.

### §4.5 differential (lines 616-619): a ledger, not an exact oracle
Findings: MIG-024 (with its correction), ARC-007.
Replace: "The old-vs-new differential is exact for dataflow parts that use none of R2-R9. Documents are compared by rendered frames, structurally, modulo ids."
With:
> The old-vs-new differential (O4a) is a divergence ledger: it is exact only for programs that use none of R2-R11, today two static pages, so no exit criterion depends on it; the change rules have their own oracle (O7). Comparing documents is an R-DOC acceptance test (O4b, §7), not a compiler differential.

## §4.6 Document runtime v2

### §4.6 Identity, Hot reload, Hand-built elements (lines 632-639)
Findings: ARC-004, EVI-009, SEM-045; research/element_reconciliation_60fps.md items 1, 4 and 11.
Replace: the Identity, Hot reload and Hand-built elements bullets.
With:
> - **Identity.** An element value's hidden identity is (construction-site key, instance path, row key). The site key is hash(definition path, structural route of the literal); the instance path is the full static call path of element-construction calls, stateless FUNCTIONs included, independent of Phase A's stateful instance tree and built from stable call-site keys, never dense ids; the row key is the collection's semantic row key, never ListId or generation. A document node is (value identity, placement): the first placement in document pre-order owns retained state; later placements of the same value get value identity plus placement slot path and a dev-window warning, never a panic. The retained key includes the element tag, so a hot-reload edit that turns `Button[...]` into `TextInput[...]` resets retained state. Hand-built elements get the identity of their literal site. S3 settles what `Reference[element:]` and label targets point to (spec.md Q6).
> - **Hot reload.** Retained state resets only for sites whose definition code hash, or a transitive dependency's, changed; so focus, caret and scroll survive edits outside the edited definition.

### §4.6 Recomputation (lines 634-636): two channels, leaf updates, lazy lists
Findings: ARC-003 (with its correction), ARC-006; research/element_reconciliation_60fps.md items 6, 7 and 9.
Replace: "**Recomputation.** The dataflow recomputes element subtrees incrementally, and the reconciler visits only subtrees whose values changed, so nothing is diffed per frame."
With:
> - **Recomputation.** The runtime has two channels. *Fires* is raised on every write (D32) and schedules copy ops. *Changed* is raised when the committed value differs (pointer equality first, structural second); it is runtime-internal and invisible to Boon code, and it is the only channel the renderer, persistence and host deltas subscribe to. Element records reach the reconciler through *changed* only; it tests pointer equality, then structural equality, then diffs, and counts each outcome. Where the dataflow proves a record's shape unchanged, attributes that depend on a scalar are updated on the retained node (today's value-guarded retained bindings), not by rebuilding the record, so a root value read by every row (Cells selection, TodoMVC filter and theme, NovyWave selected row) costs O(changed rows). A mapped list of elements is a lazy, row-keyed value materialized by viewport, overscan and focus (NATIVE_GPU_PIPELINE.md).

### §4.6 Performance gate (lines 650-651): measurable, asymptotic, accessibility
Findings: ARC-005 (with its correction), MIS-024, ARC-003; research/element_reconciliation_60fps.md item 8.
Replace: "**Performance gate.** Per-frame work and retained-update counts on TodoMVC (many rows), NovyWave and Cells must be no worse than today at 60 FPS."
With:
> **Performance gate.** Before S3 starts, capture on engine=old, for todomvc-physical, cells and novywave, the profile-stage-breakdown sample (patches p95/max, full_lowers) plus a reconciled-node count added to the same sample. Pass: every counter at most baseline × 1.1 per interaction, the native latency budgets hold, and counts stay constant for a one-row toggle in synthetic TodoMVC at 100, 1,000 and 5,000 rows. The semantic and accessibility tree (roles, labels, heading levels, lang, selection, focus, sensitive flags) equals v11's, modulo ids, on every manifest example, and the sensitive-redaction tests pass. R-DOC builds for wasm32.

## §4.7 Session and warm path

### §4.7 Cold first and Reuse (lines 655-661) (conditional on QUESTIONS.md C4 and C5)
Findings: ARC-009, ARC-010, ARC-011; research/incremental_compilation_subframe_latency.md items 1-3, 5 and 7.
Replace: the "Cold first" and "Reuse, when added" bullets.
With:
> - **Cold first, reuse committed.** A full recheck per keystroke fits TodoMVC's 16.7 ms gate and stays as the warm == cold oracle. Per-definition reuse of FUNCTION schemes is a committed P6 item: every production system with sub-frame edits gets there by reuse, and NovyWave's cold target is already over the frame. The root group (every root definition, and every role of a distributed program) is re-solved whenever a member's structural fingerprint or an instantiated scheme's interface changes, so an edit inside NovyWave's `store` has a floor of about 8-15 ms (root re-solve 1-7 ms pending S2), independent of P6; such edits stay report-only, and a per-root-definition memo (with a session-lifetime TypeStore) is added in P6 if S2 measures the root share above 5 ms. If TodoMVC's diagnostics lane exceeds 12 ms p95 at S2, reuse moves into P5.
> - **Reuse.** Check results are a pure function of (member structural fingerprints, dependency interface fingerprints, config). Each group stores its exact input key and is re-verified Salsa-style; early cutoff compares freshly computed interface fingerprints (backdating). The interface fingerprint is a by-product of scheme normalization, compositional over child hashes, and its cost is gated (§9.3). SCCs come from the current revision only; results of a cancelled revision are kept only per completed group.
> - **Short-circuits (P5).** Types never depend on values (D4), so: if every structural and literal-bytes fingerprint is unchanged, the previous `Checked` is returned with positions re-anchored; if only literal-bytes fingerprints changed, check results are reused and only literal validations rerun.
> - **Staged publication.** Front-end diagnostics are published first; the edited definition's own diagnostics follow as a partial `Checked` (`complete: false`) before the cone and root group are rechecked. O5 applies to complete revisions only.

### §4.7 Errors, Cancellation, Messages, Memory (lines 662-675)
Findings: ARC-019, ARC-012 (with its correction), ARC-031, ARC-014, ARC-025; research/incremental_compilation_subframe_latency.md item 5.
Replace: the Errors sentence "Dependents of a broken definition see a deterministic absorbing error type." and the Cancellation, Messages and Memory bullets.
With (the lane-placement fallback is conditional on QUESTIONS.md C6):
> - **Errors.** Error is expression-level and poisons the properties derived from it (§4.3 "Diagnostics"); dependents of a broken definition see a deterministic absorbing error type. There are no history-dependent "last good" interfaces, so warm and cold results match.
> - **Cancellation.** Polled by work count in every phase: every 4,096 tokens in the lexer and parser, every 4,096 solver steps, every 4,096 TypeStore node visits inside one operation, every 4,096 emitted instances or DAG nodes in Phase A and B, during snapshot serialization, and at unit, group and definition boundaries. A poll checks a flag and a deadline, so a single-threaded wasm32 compile can run in slices. Stop latency is measured in flight: cancel at a uniformly random point of a running compile, at least 1,000 trials per fixture, timed from cancel to the worker's acknowledgement; p95 ≤1 ms, max ≤8 ms (the same numbers in §6 and §9.3). Today's pre-cancelled probe is reported as rejection latency.
> - **Messages.** Source: full text per changed unit; the compile mailbox merges edits per unit (path to latest text) and supersedes only the compile, never the edits. Snapshot: compact per-definition blocks (diagnostics, compact hint labels, occurrences) with deltas keyed by (definition, version); full type display trees are served on demand by a hover request, never shipped per node. Budget: snapshot ≤256 KB and ≤1 ms per hop on NovyWave, measured in the warm corpus by a P5 spike before the protocol is fixed. If the spike fails, the diagnostics lane moves to an editor-local check-only session in the dev process. The dev window keeps and shifts the previous snapshot; snapshots are matched on (app, example, stage, revision).
> - **Memory.** Retained session state, measured as the VmHWM delta of the compile-bench process between session open and the end of the 12-class NovyWave edit corpus, ≤64 MiB, with growth ≤8 MiB across the corpus and a long-session check after about 10 k typing keystrokes. P0 names the producer of today's 383-414 MB figure.

## §4.8 Public API and engine selection

### §4.8 API (after line 687): hover, diagnostics, profiles, editor scope
Findings: ARC-030, MIS-013, MIS-021, MIS-020, MIS-019.
Replace: (append after the code block)
With:
> Additions: `Checked::hover(file, byte) -> Option<Hover { type_display, update_kind, copies, follows, effects, no_value_until }>`, lazy and the only source of full display trees; `Diagnostic` as in §4.3; `Checked::out_hints(file)`; `DistributedSession::checked(role)`, with a stated rule for files shared between roles. `Config.target` is a validated semantic target profile (software default, software bounded, console app, core hardware); board and shell profiles never reach the compiler, so the MachinePlan is identical across boards. `Config.capability_profile`: host effects and ports outside the profile are checker diagnostics.
>
> **Editor features at P7:** today's (highlighting, inspector, go-to-definition, next reference, format, diagnostics), plus hints, navigation and hover for every definition not affected by a syntax error (a syntax error in X keeps hints and navigation elsewhere), plus the hover payload above (P5, size M). Symbol rename, completion and an LSP are out of scope (§12.2). `boon_cli check --fix` applies meaning-preserving fix-its and re-checks; the P3a scripts use it.

### §4.8 Engine selection (lines 689-696)
Findings: EVI-006, MIG-020 (with its correction), EVI-007, ARC-028.
Replace: both bullets.
With (the second bullet is conditional on QUESTIONS.md C1, recommended option (d); option (a) follows it):
> - The engine is one process-global choice (`BOON_COMPILER_ENGINE=old|next`), read once by the `boon_compiler` facade and carried in `Config`. Every compile entry point honours it: the shared compile service (playground preview and dev lanes), `compile_program_artifact` (child programs through ProgramCompileWorker), `compile_migration_stage` (Preview, Activate, TEST), distributed packages and `boon_cli compile-bench`. Every report records the number of compiles per engine and fails if they are mixed.
> - Each manifest example may carry a `next_source` overlay for sources only the new engine accepts. engine=next runs (preflight, O2, P3b-P6) read the overlay; the old engine and its gates never do. The default engine stays old until P7, which swaps every overlay in. The native verifier passes the engine explicitly and stamps it into reports, and `verify-all` refuses reports from a non-default engine. The overlay is manifest data, so no code path depends on an example's name.
> - `COMPILER_ID` becomes compiler_id(engine), and the prepared-program cache key includes it, so the engines' stored child artifacts never mix. persons_pro's `published_compiler` differs by engine by design (an O4a divergence).
>
> Option (a) instead: each example declares `engine = "old" | "next"`, flips in its own change set once its native gate and O2 scenarios pass on next (promoting its overlay), `verify-all` checks each report against the declared engine, and a flipped example's performance gates are report-only until P6.

### §4.8 Porting other consumers (lines 698-701)
Findings: MIG-021 (with its correction), MIG-020.
Replace: "Everything else (editor, program_runtime, …) is ported in the P7 switch change set."
With:
> host_runtime (MigrationScenarioRunner), boon_runtime (for `boon_cli run`), the behaviour harness and phase0 get the engine switch by P4, and the plan_executor, server and http tests by P5, because O2 and the P4 and P5 exits run through them. The other consumers (editor, program_runtime, app_package, web host, xtask) are ported by P7; `boon_runtime` loses its compiler dependency at P7.

## §4.9 Engineering rules

### §4.9 (lines 710-723): precise bans, lint, wasm32, value arithmetic, hostile input
Findings: ARC-031, ARC-028, MIS-022, MIS-027, MIS-020; research/incremental_compilation_subframe_latency.md item 13.
Replace: "- no `sha2`, `ciborium` or `serde_json` in the compile crates; …", "- no example-specific branches;" and "- builds for `wasm32-unknown-unknown` (single-threaded, clock abstraction) and for musl."
With:
> - no direct dependency on `sha2`, `ciborium` or `serde_json` in the compile crates (checked with `cargo tree -e normal --depth 1`); identity hashing goes only through `boon_plan`'s frozen constructors;
> - no example-specific branches: an xtask lint fails if a `boonc_*` crate contains a string literal equal to a manifest example id or a path under examples/, and O8 checks behaviour under renamed paths;
> - `cargo check --target wasm32-unknown-unknown` (single-threaded, clock abstraction) and the musl build pass for the compile crates; boon_plan_executor, boon_document and boon_web_host build for wasm32 in every change set that touches R1-R11.
>
> Add to the code rules: all compile-time evaluation (checker constants, static checks, Phase B folding) goes through boon_data's Number, TEXT and BYTES operations, and no compile crate reimplements value arithmetic; hostile-input bounds cap nesting depth, type and scheme DAG size, instantiations, solver steps and diagnostics per compile, each as a positioned "limit exceeded" diagnostic, never a panic; printers and hover are DAG-aware and truncate.

## §5 Example migration

### §5 preamble (line 736): the Rust-embedded sources
Findings: MIG-014.
Replace: "- Boon inside about 12 Rust test files."
With:
> - Boon inside Rust: 191 snippets in 48 files. The 47 snippets in 15 files that belong to crates surviving P8 (plan_executor 16, server_runtime 16, wellen_host 5, and one or two each in document_model, editor, web_host, host_runtime, behavior_harness and cli) are migrated in P3b; snippets in old-compiler crates stay with the old engine and are deleted at P8.

### §5.1 table (lines 743-764): the measured census
Findings: MIG-007, MIG-008, MIG-009, MIG-011, MIG-012, MIG-013, MIG-014, MIG-015, MIG-016, MIG-018, MIG-019, MIG-001, MIG-004, SEM-002, SEM-003, SEM-022, SEM-024, SEM-033, SEM-048, EVI-005, EVI-008, EVI-010; numbers from census/RESULTS.md (scanner estimates; the new checker's census stays authoritative).
Replace: the whole table.
With (rows that depend on a question say so):
> | class | sites (.bn; +R in Rust strings) | fix |
> | --- | ---: | --- |
> | TodoMVC Theme `get(request)` mixes kinds (D4, D8) | 94 calls, all in RUN.bn; 6 theme files, 2,444 lines; 37 WHEN→WHILE candidates inside them | **Themes as data.** Each theme exports `tokens(mode)`; `Theme/tokens(name, mode)` picks one. A dispatch on `name` or `mode`, and every call-site selector whose arms read `theme.*`, is WHILE. Lights are the same in every theme: `Theme/lights(name, mode)` stays a WHILE dispatcher. The draft (examples.md §4) covers Classic and Professional only and drops glow and material shadows, which D22 keeps; re-estimate ~1,250-1,400 lines, ~9-10 role types (Glow added), 5-7 engineer-days with the 5×2 matrix. |
> | NovyWave `NovyTheme` (D8) | 84 material + 60 font calls, plus 6 + 6 `trace_*` | Same idiom, with `of \|> WHILE` or `tokens(mode)` records (the notes' "tag API stays" is superseded). Bordered and borderless materials are separate roles. Dead `trace_*` deleted. S-M. |
> | Self-referential LATEST (D15) | 5: counter_latest.bn:8, interval_latest.bn:6, novywave RUN.bn:3020 `value_format`, persons_pro RUN.bn:333 `publish_request_sequence` and :456 `mode` (persons-pro gate); TodoMVC RUN.bn:132 is a D14 outer copy | Fix-it `initial \|> HOLD s { LATEST { … } }`. |
> | LATEST with two starting arms, or a starting arm that can update | ≤15 upper bound (NovyWave 13; 1-2 real in a hand check of 6); persons_pro `publish_state` | Listed by the checker (needs update kinds); manual review. |
> | One-input LATEST | 91 (87 in `bytes_*`), +1 R | Remove the wrapper (fix-it). |
> | Zero-arm LATEST | 65 (14 in migration stages) | Legal as a HOLD body (§4.3); no change. |
> | HOLD piped inputs that update after start (D33) | 49 candidates | S4 splits them: inputs that never update after start or row creation (no change) and live inputs such as the NovyWave zoom chain (behaviour change, listed). |
> | Cycles (D9) | TodoMVC new_todo (title ↔ edited_title) after D15; completed ↔ all_completed closes through a THEN-body copy and the library `Bool/toggle`'s input | Re-derived against the P0 library `Bool/toggle` and the edge table: if its piped value (the LATEST) is the HOLD's piped input the cycle is an error and TodoMVC is rewritten; if it is a body candidate the cycle closes through a HOLD update edge and stays legal. Rewrite with HOLD where the checker flags it. |
> | Cells (D16) (conditional on QUESTIONS.md A9) | the formula engine (formula.bn, 366 lines, `Dependency/catch_cycle` ×1), plus `display_text` and the formula bar (WHILE, D30), `__selected_*` private keys (D13), 16 unused binders (D24), the cells.scn rework | Parse each `formula_text` into a tagged node (Lit, Ref[position], Binary, Sum, ParseError); sheet values come from counted iteration over the parsed list (`Stream/pulses` driving a collection-authority update, one pass per formula cell; `List/fold` only if pulses cannot settle within one activation); a cell still pending after the last pass is CycleError, and every loop member is marked. Collection element fields are graph nodes (§4.3). Every edit recalculates the whole sheet (Excel's volatile path), and cells.scn's `expect_recomputed` steps move to the performance gate. Sized after spike S7: L (~2 weeks) if S7 passes on the existing primitive. |
> | WHEN subject reads in arms (D10) | 82 (NovyWave 55, host_service_effects 12, testdata/typed_passkey_effects 14, server_effect_chain 1), +10 R | Binder patterns (fix-it). |
> | WHEN that must follow live updates (D30) | ~290-335 (view 142, state 132, theme 61; NovyWave ~190, FjordPulse 57, TodoMVC 48, persons_pro 23), +15 R; 27 more over effect results (hand review) | WHILE, driven by the checker's stale-copy rule and fix-it (A4); the scanner (hand-sample precision about 35 of 37) is the calibration target. WHENs that copy on purpose stay. |
> | WHEN over a value with pure arms | 736 | No change if WHEN copies at activation (A1); WHILE otherwise. |
> | THEN over HOLD state | 35 in the same file (persons_pro 14, typed_passkey_effects 14, host_service_effects 6, server_effect_chain 1), plus NovyWave 4 over a LATEST with a fallback | Stays legal (D30, D32); equal rewrites now fire (S5 counts affected sites). |
> | State inside copy contexts (placement) | 7: NovyWave RUN.bn:4540, fibonacci.bn:61, 3 in `testdata/compiler_*_pulses.bn`, 2 in boon_plan_executor testdata; plus calls through state-creating FUNCTIONs (NovyWave `new_selected_signal`, `new_selected_visible_item`) | NovyWave by the WHILE pass or by moving its SOURCEs to row creation; fibonacci per A10; fixtures rewritten. |
> | Effect-returned lists and effect placement (D9, D17, D31) | NovyWave hierarchy and signal pages; LATEST/HOLD fed by a query in a copy context (NovyWave, several); commands in live contexts: 2 (`File/write_text`, todo_mvc_physical/BUILD.bn:33, novywave/BUILD.bn:33) | `List/replace_all` authorities, in the example's next overlay (§5.1 rule). `code => TEXT {…} \|> File/write_text(path: output_file)` moves inside the arm; BUILD files are new-engine only for O4a. `request_fingerprint` goes (D34). |
> | Suffix fallback removed (D14) | 67: NovyWave 44 (41 `elements.*` in row constructors, RUN.bn:4490-4853), TodoMVC 2 (RUN.bn:552, :810), server examples 5, fixtures 10 (the per-file split sums to 61; re-run `census.py` for the rest, `census/RESULTS.md` closing note) | Qualify with `store.`, or pass `elements` in as a parameter or through PASSED. |
> | Sibling call-argument reads | 119, of which `element.hovered` 110 | Legal: §4.3 Names lists the constructor `element` binder. |
> | Optional-field reads (D5) | TodoMVC append without `completed`; persons_pro variant-union reads (5) | Append complete rows; use binder patterns. |
> | Collections in stored state (D9) | 0 in HOLD; NovyWave `real_hierarchy_page_result` keeps a LIST in a LATEST (A6) | The HOLD diagnostic stays; the LATEST moves to a `List/replace_all` authority. |
> | Closed contracts (D13) | `event: [` 142; `events: [` 147 (54 as `element: [events:`); `hovered:` 243 lines (96 `hovered: element.hovered`); private keys such as Cells `__selected_*` | Fix against the generated contracts; S4 records the rule behind each count. Unrendered keys go to R-RENDER from S4's list (D22). |
> | Exact record parameters (D23) | 140 calls pass a record variable (NovyWave 104; the finder's scanner also saw 53 in Rust strings, a figure census/RESULTS.md does not report); 93 never forward the whole parameter; 0 literals with extra fields | Entry-point narrowing fix-it (P2a) under A3; edits in P3b. |
> | Pony precedence and comparison chains (D11, D18) | 12 mixed, 0 chains; 182 `a == b \|> f()` sites keep their meaning (§4.2) | Parentheses (fix-it); the 182 sites are a named P1b corpus. |
> | Tag both bare and tagged in one type (D18.4) | 3: BUILD.bn `Ok` ×2, NovyWave `StringValue`, +3 R | Rename one form (fix-it); catalog outcome tags made consistent. |
> | Unused parameters and binders (D24) | 43: 7 parameters (plus 4 unused OUT parameters; P0 decides whether they count), 36 pattern binders (cells 16, NovyWave 21, TodoMVC 5), +26 R | Drop them: `Tag[a, b] => e` becomes `Tag => e` (fix-it). |
> | Lists of row updates `List/map(new: …) \|> List/latest()` | 28 (NovyWave 22, cells 5, plan_executor testdata 1), +1 R | Ordinary lists of last values (L5). |
> | Host values mirrored in HOLDs (D35) | 2 mirrors: examples/todomvc.bn:99 `new_todo_focused`, fjordpulse Client/RUN.bn:499; 6 persisted NovyWave hover, pointer, focus and pressed labels; the fjordpulse Session connection HOLD | Read host ports (R9's port list). S4 counts "HOLD written only from host level transitions". |
> | `Http/request` (D36) | 2: outbound_http_effect.bn:7, server_effect_chain.bn:11, +2 R | `Http/get` in live contexts, `Http/send` in THEN/WHEN. |
> | Old-compiler bugs that look like example bugs | fjordpulse, cells (unchanged) | Fixed in the new compiler; examples unchanged. |

### §5.1 Old-engine compatibility rule (lines 766-770) (conditional on QUESTIONS.md C1, option (d))
Findings: MIG-020, EVI-007, EVI-006, MIG-030, MIG-029.
Replace: the whole paragraph.
With:
> **Where migrated sources live before P7.** A migration that the old engine accepts and that keeps today's behaviour lands on main in the example's source, and the old engine must still pass that example's gate. A migration that only the new engine accepts (List/replace_all, Http/get and Http/send, host ports, the Cells redesign) lands on main in the example's `next_source` overlay (§4.8), which engine=next preflight, O2 and P3b-P6 use; it never becomes a Boon workaround, and the old gates never read it. A gate that is already red at P0 is recorded, not blocking, and that example is verified on next only. O4a runs on the pre-overlay revision of each example. P7 swaps every overlay in. (Under C1 option (a), examples flip one at a time instead, and P7 is the last flip.)

### §5.2 Verifying example behaviour (lines 780-796)
Findings: MIS-023, SEM-021, MIG-008, ARC-007, MIS-017, EVI-006; research/query_command_effect_systems.md item 12.
Replace: item 2's last sentence, item 3, and the "Gate coupling" paragraph's last sentence.
With:
> 2. … Freeze the step ids and source paths the gates consume. **Scenario format v2** (at most one page, documented with LANGUAGE_SEMANTICS v2): step kinds `restart`, `hot_reload`, `set_host_port`, `advance_time`, `complete_query` (with ordering) and `expect_effect_log` (the R9 record); render assertions compare R-DOC frame trees, not delta strings, and migrating the 113 DocumentPlan-delta assertions is P3b work. A stress mode restarts every live query mid-flight and simulates restore with commands in flight.
> 3. **CPU render diff.** Static theme-probe documents render every combination of theme, mode, role and state, with state passed explicitly. The matrix adds checkpoints in both orders (switch theme then toggle mode, and the reverse) that assert a themed style value or a readback crop, not only root text. After R-DOC the diff compares rendered frame trees; that is the same comparison as O4b, owned by R-DOC. The `bytes_*_plan_ops` fixtures get a keep, migrate or delete decision: kept ones get value assertions from BYTES_SEMANTICS.md under D21 and D30-D32, `bytes_indexed_duplicate_update_conflict` becomes a negative fixture, and `bytes_same_event_dependency` is rewritten for committed-snapshot reads (2-3 days, P2b or P3a).
>
> … and has a 4 ms p95 child-compile budget, measured on next at P5 exit. Its child compiles and migration-stage compiles honour the engine choice (§4.8).

## §6 Performance targets

### §6 table (lines 805-821): add a method column
Findings: ARC-020, ARC-021, ARC-022, ARC-023, ARC-024, ARC-025, ARC-011, ARC-012, ARC-013, ARC-009; research/incremental_compilation_subframe_latency.md item 2.
Replace: the table.
With (the check, throughput and warm rows depend on QUESTIONS.md C4 and C5; lowering a target is an owner item):
> | metric | target | basis | method |
> | --- | --- | --- | --- |
> | front end, NovyWave / TodoMVC / counter | ≤4 / ≤1.2 / ≤0.05 ms p95, frozen profile | prototype 2.6 / 0.64 / 0.021 ms, best of 30, target-cpu=native | prototype-measured; re-measured under the frozen protocol in P1a |
> | check, TodoMVC / NovyWave | ≤6 / ≤20 ms | 5x / 4.4x checker.md's model | estimate; S2 measures, with the §4.4 band |
> | Phase A, TodoMVC / NovyWave | ≤1.5 / ≤4 ms | pre-D13 per-node model | estimate; S1b validates |
> | Phase B, TodoMVC / NovyWave | ≤8 / ≤20 ms | pre-D13 per-node model and a script over today's v11 plan JSON | estimate; S1b validates |
> | cold diagnostics / verified, TodoMVC | ≤10 / ≤20 ms | sum | sum of estimates |
> | cold diagnostics / verified, NovyWave | ≤30 / ≤55 ms | sum | sum of estimates |
> | small program in-process (counter, starter source) | ≤1 / ≤2 ms p95 | persons-pro native gate: 4 ms end to end | estimate; S1 measures counter |
> | warm edit → diagnostics, TodoMVC | p95 ≤16.7 ms per edit class; no-op edits ≤1 ms; ≤8 ms goal only with P6 reuse | full recheck 1.2 + 6 + 1.5 + ≤2 = 10.7 ms | sum; P5 measures |
> | warm edit → diagnostics, NovyWave | FUNCTION-body edits gated once P6 reuse lands; edits inside the root group report-only (floor ~8-15 ms) | root-group re-solve | estimate; S2 measures the root share |
> | warm edit → preview, TodoMVC | p95 ≤40 ms (budget 100) as a sum: compile ≤18.7 + source hops 2 × ≤0.5 + snapshot hops ≤2 + readiness and mount + document build and reconcile + present | full re-lowering plus the listed steps | estimate; profile-stage breakdown on engine=old in P0, on next at P5 exit |
> | edit → first presented preview frame (native verifier, plan swap included) | report | none | report-only |
> | supersession stop latency | p95 ≤1 ms, max ≤8 ms | in-flight probe at a random point (§4.7) | measured from P1b |
> | single-thread throughput, fixtures ≥10 k lines | provisional ≥150 k lines/s diagnostics, ≥75 k verified; final floor = 0.6 × the S2-measured rate, rounded down | today 23 k / 6.4 k | estimate; set from S2 (owner item: lowers the plan's 250 k / 150 k) |
> | peak RSS, NovyWave verified | ≤48 MiB gate, 64 MiB hard max: VmHWM of compile-bench, fresh process (floor 5.8 MiB measured) | today 314 MiB | estimate |
> | allocations, NovyWave | allocator events on the compile thread from first byte to `Checked` ≤10 k, fresh process; Phase B with MachinePlan assembly reported separately, with its own limit once S1b measures it; per-definition slices come from a pooled scratch reused across revisions | today 7.46 M in total | estimate |
> | interface fingerprint cost | cold with fingerprints ≤1.05 × cold without | research | new gate |
> | amplification counters | 1 instantiation per syntactic call; ≤4 solver work items per expression | today 30× calls, ~41 steps per expression | counter |

### §6 closing paragraph (lines 823-826): the measurement protocol
Findings: ARC-021, ARC-027.
Replace: (append)
With:
> Every spike number is reported as fresh-process p95 of at least 10 runs plus in-process p50 of 30 repeats, on the frozen release profile (generic CPU, lto=false, codegen-units=16, mimalloc), with its command line and commit; from S1 on, prototype code lives in a `boonc_*` crate, not a standalone rustc file. If P6 changes the build profile, every row is re-measured on the new profile before the checkpoint.

## §7 Correctness strategy

### §7 O1 (line 836): authorship, rule ids, no bless mode
Findings: MIG-025, MIS-013, MIS-014, MIS-017, MIS-016, MIS-027, MIS-011, MIS-015.
Replace: "Expectations are written from the spec, never taken from old-compiler output. Seeds: …"
With:
> … plus hint lines for display rules, related spans and fix-its. Expectations are written from static semantics v2 (itself rewritten from D1-D36 and §4.3, not copied from notes/spec.md), never from old-compiler output, by an agent that does not write boonc_check. Every `.expect` line cites a rule id; the runner has no bless mode, and a changed `.expect` cites the rule or decision that changed. Every fix-it fixture is applied, re-checked (the code disappears) and re-run where a scenario exists. The owner reviews fixtures in batches of families before P2a exits. Seeds: every audit probe; a negative and a positive fixture per rule; one "one root cause, exactly one diagnostic" fixture per strict rule; multi-line TEXT layout fixtures; `format` idempotence on every tracked file; the 10 `bytes_negative_templates` instantiated as concrete fixtures; BITS/BYTES width fixtures; rational identities folded at compile time; a sensitive-input fixture; `language_surface/current`; `testdata/phase0`.

### §7 O2, O3, O4, O5, O6 (lines 837-841)
Findings: SEM-021, SEM-025, SEM-016, SEM-018, MIS-007, MIS-009, MIS-011, MIS-001, MIG-024, ARC-007, ARC-029; research/incremental_compilation_subframe_latency.md items 4 and 9.
Replace: the O2-O6 cells.
With:
> | O2 behaviour | Every manifest and migration scenario through the host-service runner after triage, effects asserted through the R9 effect log. Also: a restart scenario per persisted example (D12); a crash between a command's trigger and its completion; a sensitive input across a restart (store and effect log checked for plaintext); a live query argument rewritten faster than the query answers, with the expected behaviour stated; equal rewrites of a live-query argument; row removal cancelling a query; rename, retype and delete per new leaf kind; a namespace deletion; one Client+Session scenario; and a UI-state retention scenario (focus survives an unrelated edit). |
> | O3 determinism | Each fixture is compiled twice, with threads 1 and N; canonical plan bytes, diagnostics and the language snapshot must be identical, and the harness compares. A variant comparing a fresh process with a warm session after 500 scripted keystrokes (plan bytes, diagnostics, snapshot). |
> | O4a divergence ledger | Old engine vs new engine on one executor, only where the old engine passes its own scenario, on sources both accept. Exact only where R2-R11 are unused, so no exit criterion depends on it. Every divergence is listed in `tests/compiler/divergences.toml` with its decision. |
> | O4b documents (R-DOC acceptance) | Both engines' documents lowered to RenderScene with retained keys stripped, children in document order, text-input content excluded. Owned by S3 and R-DOC. |
> | O5 warm == cold | At every step of every edit-corpus sequence (a Zig-style `#update=` file with the expected diagnostics per step): identical diagnostics, snapshot and canonical plan. |
> | O6 debug verifier | Debug builds run `verify_plan`, behind a feature or env switch rather than on every keystroke, and so does every checkpoint. A negative corpus of plans that fail `verify_plan`: the executor, document runtime and reconciler return an error, never panic, on every one. |

### §7 new O7-O9 and the persistence row (after line 843)
Findings: MIG-024 (with its correction), SEM-004, ARC-028, MIS-020, SEM-008, SEM-028; research/state_restore_persistence.md item 4.
Replace: the "persistence" row.
With (O7 is conditional on QUESTIONS.md C3):
> | O7 change-rule traces | In P0 an agent that implements neither R-CHG nor P4 writes the expected per-step traces (root values, effect log, element trees modulo ids) for every change_probes scenario and every rule in §4.3 "Change rules", in D30 vocabulary, including the glitch probe (`x: a + c` fires once per press); the owner signs them, and they become O2 fixtures with a named owner and a size stated in P0. Optional stronger step, time-boxed as a spike: `boon_spec_interp`, a test-only AST interpreter that runs the same traces (its 2-4 k line estimate is optimistic once lists, elements and scripted effects are in). |
> | O8 rename | Every fixture is compiled a second time with its units under random unit paths, a random entry stem and a fresh ApplicationIdentity; diagnostics (modulo path text), the language snapshot and the canonical plan (modulo path and app strings and the ids derived from them, plan Q18) must be identical. |
> | O9 hostile input | cargo-fuzz over the parser and checker (corpus from the examples) and over `load_untrusted` on mutated plan bytes: no panic, bounded time and memory. |
> | persistence | `identity_v1` golden vectors, including: pipe stage inserted, arm reordered, HOLD moved into a FUNCTION, library body refactored, leaf deleted and re-created (tombstone), LATEST and last-value leaves. Per example, the checker emits a durable-set golden file that must match the D12 rule (§4.5). |

### §7 closing paragraph (lines 845-847): a local nightly run
Findings: MIG-030 (with its correction).
Replace: "The repo has no CI."
With:
> The repo has no CI. A local nightly job (a systemd user timer, set up by the owner) runs `cargo xtask compiler-checkpoint --nightly` and the native gates in the launch-scoped workspace while holding the measurement lock, and writes a one-line summary per night.

## §8 Execution phases

### §8 Sizing and critical path (lines 853-861)
Findings: MIG-021 (with its correction), MIG-023, MIG-029.
Replace: "**Sizing.** One strong engineer or agent per phase. …" and the critical-path block.
With:
> **Sizing.** One strong engineer or agent per phase. ∥ marks work that runs in parallel with the critical path. Parallel agents run only with the owner's permission (AGENTS.md) and share one machine: builds, tests and timing runs serialize on the measurement lock and the native gates' launch-scoped seat.
>
> **Critical path.**
> `P0a (decisions, spec, S4, S6, gate run) → P0b (S1, S1b, S2, S3, S5, S7) → P2a → P2b → P3b → P4 → R-DOC integration on P4 output (1-2 weeks) → P5 → P6 → P7 → P8`
> P1a overlaps P0. R-CHG runs from S5 and must finish by P4 exit. R-DOC runs from S3; its performance gate closes only on P4's element records. S6 comes before S5, because S5 counts over S6's scenarios.

### §8 P0 heading and Semantics (lines 863-872)
Findings: MIG-023, MIG-025, MIS-025, EVI-011, SEM-049, MIS-003; research/spreadsheet_recalculation_cycles.md item 2.
Replace: "### P0: Reset, decide, de-risk (2 weeks)" and the Semantics bullets.
With (the heading is conditional on QUESTIONS.md C9, recommended (a)):
> ### P0: Reset, decide, de-risk (P0a then P0b, 2-3 weeks each; 11-17 engineer-weeks, about 4-6 calendar weeks with the spikes run as parallel agents, owner permitting)
> **Semantics (owner).**
> - The language questions L1-L15 were answered on 2026-09-29 (D20-D36, §11). The review's questions (QUESTIONS.md) §A are answered before any spec text is written; §B before the phase each names; §C apply by default unless the owner objects.
> - Write "static semantics v2" into LANGUAGE_SEMANTICS.md from D1-D36 and §4.3, not from notes/spec.md §6 or change_and_effects.md; every rule gets an id; target ≤1,200 lines, about 1 engineer-week. The first deliverables are the edge table, the activation rule and the step rule. The review's 18-pair decision composition matrix is re-run on the result.
> - Produce the HOLD-alternative options note (D9), including a typed lookup whose result has a `Cycle` variant (the mechanism of spreadsheet engines, without the name `catch_cycle`).
> - Adversarially review lowering_alternative.md as amended by R4 and R-CHG, and the change_and_effects.md defaults the plan carries, before the spike reports are signed off.
> - Ask the owner to retire PRIORITY and EXCLUSIVE.

### §8 P0 Reconcile the active contracts (lines 874-890): missing documents
Findings: MIS-025, MIS-002, MIS-006, MIS-016, MIS-019, MIS-011, SEM-043.
Replace: (append to the list)
With:
> - RUNTIME_MODEL.md: tick phases, commit point and conflict step after R-CHG (:76-93, :188-200); one external occurrence per tick.
> - LANGUAGE_SEMANTICS.md: HOLD initialization (:374-377, D33) and the hardware analogy (:351-360, D30).
> - FPGA_TODOMVC_LOWERING.md: delete it; its generic constraints move to §4.3 "State classes".
> - BOON_PERSISTENCE_ARCHITECTURE_PLAN.md: also :30-35, :348-368 ("Sensitive Input And Credentials" is retained) and :1290-1317 (hardware stores under D35).
> - BOON_CONSOLE.md (:48, :150-170, :300-307, :535-556): the store each hardware deployment uses; console state restore keys on identity_v1 schema compatibility (semantic_schema_hash plus DRAIN edges), not exact app.wasm identity; commits within a tick stay private to the turn; the stage chain (source → syntax → typed program → MachinePlan) is rewritten now, not at P8.
> - BOON_FIRST_RISCV_PROCESSOR_PLAN.md: also the Boon Hardware Contract (:250-300), the BITS/MAP gate items (:190-195, :638) and :669-678, including how a tick maps to a clock edge.
> - BOON_CONSOLE_IMPLEMENTATION_PLAN.md: target eligibility analysis (value ranges, bounded collections, the static tick bound, no element values) is a named work item outside the rewrite and off its critical path.
> - TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:136-139 joins the superseded paragraphs.

### §8 P0 Process (lines 893-900)
Findings: MIG-027, MIG-030, MIG-026.
Replace: "- Archive the old plans (§9.2).", "Bring xtask back under its line cap by deleting `compiler_allocator.rs` and the evidence lane.", "- Run all 7 handoff gates once and record which are red." and "Plan compensating deletions for R1-R7."
With (the cap sentence is conditional on QUESTIONS.md C8):
> - The archive moves to P1b, with budgets v4 (§9.2).
> - Bring xtask back under its line cap by deleting `compiler_allocator.rs` and its main.rs dispatch; nothing else in xtask changes before budgets v4.
> - Run all 7 handoff gates once and record which are red; an example whose old-engine gate is red is verified on next only (§5.1).
> - … Measure the growth of R2-R5, R-CHG, R9-R11 and R-DOC in S1 and S3. The runtime and playground caps get a temporary allowance for dual-path code, recorded in architecture.rs with an expiry at P8.

### §8 P0 spikes (lines 906-913)
Findings: MIG-021, EVI-006, ARC-022, ARC-020, ARC-009, ARC-010, SEM-044, EVI-002, ARC-004, ARC-005, ARC-006, EVI-009, MIS-024, ARC-001, ARC-008, SEM-004, MIS-002, MIG-022, MIS-023, ARC-027, SEM-034, MIG-001, MIG-002, MIG-016, SEM-035, SEM-041, EVI-008, SEM-024, SEM-033, SEM-038, SEM-039, SEM-040, EVI-010; research/structural_inference_exact_records.md item I6, research/element_reconciliation_60fps.md items 1 and 5, research/query_command_effect_systems.md item 2.
Replace: rows S1-S6.
With (S7 is conditional on QUESTIONS.md A9; S5's rank core on A2):
> | **S1: vertical slice** | counter and counter_migration (its stages compile with next) and one list-plus-persistence fixture through a minimal front end, a ground-only checker and Phase A/B, run on the runtime; S1 may use the v11 document path and a throwaway engine switch | `verify_plan` passes; counter verified ≤2 ms in-process; the scenarios pass; every runtime and format change (R1-R11, and the v11-or-v12 choice) is listed and sized with its line growth; the code-complete to gates-green time is recorded. Restart under D12 moves to P4 exit; focus across an unrelated edit and the native shadow run move to P5 exit. |
> | **S1b: back half at scale** | end of P2a, 3 days: Phase A+B of the S1 slice over the S2-migrated TodoMVC view functions with D13 element records | Phase A ≤1.5 ms, Phase B ≤8 ms p95 in-process, DAG ≤12 k nodes, the plan mounts with the S3 prototype, Phase B allocations measured. |
> | **S2: checker core with structural elements** | TodoMVC view functions (theme refactored) plus a NovyWave-shaped generator (373 constructors, 252 style records, depth 22, theme and PASSED reads), plus records of 50-200 fields, the full element-kind union, WHEN arms joining element records with differing Optional fields, chains of four or more forwarded exact parameters, a BITS residual predicate, a same-trigger LATEST fixture | measured, not extrapolated, with the §4.4 band and counts. |
> | **S3: document model v2** | prototype element-record reconciliation on TodoMVC (many rows), Cells and a NovyWave list, plus: a root value read by every row with ≥1,000 rows (Cells selection, TodoMVC filter and theme, NovyWave selected row); one element value in two named slots; a spread copy placed twice; repeated stateless helpers under one parent; a hot-reload insert of a definition and a list before the edited one; a NovyWave `replace_all` refresh counting remounted rows | the §4.6 gate on the prototype; identity rules settled; O(changed cells) record evaluations per Cells selection move. |
> | **S4: census scanner** | closed contracts against the renderer and host tables, self-referential LATEST and LATEST with two starting arms, collections in state, commands outside copy contexts and state inside them, suffix-resolved names, Optional fields reaching storage, cycles under D9, plus: WHEN over a start-valued selector whose arms read independently updating values (calibrating the checker's list), LATEST whose starting arm can update, HOLD piped inputs that update after start, zero-arm LATEST, state inside copy contexts including through calls, tags both bare and tagged, record parameters forwarded or spread into results, whole records passed to a FUNCTION that reads a strict subset, recursive shapes, collections in LATEST or last values, LATEST/HOLD fed by a copy-context query, HOLDs written only from host level transitions, arms forwarding the matched value whole, subject field reads vs nested-path reads in tag arms, writes through views, commands whose trigger roots include a live-query completion, theme keys no renderer reads | runs on all 177 .bn files and the Rust snippets with zero unclassified sites; the rule behind every count is recorded; counts per class size P3. |
> | **S5: scheduler v2** (after S6) | instrument the executor to count same-tick cross-cell reads, op evaluations, HOLD writes and ticks per interaction over S6's scenarios and the native gates, under today's dedup and under every write fires; then build the ranked-commit core (R7-R8) on a branch; count live-query argument updates against distinct argument tuples on NovyWave and Cells | no behaviour change on main in P0; the glitch probe gives one `x` firing per press; per-tick op evaluations ≤1.5 × today with the 60 FPS gates unchanged, or the owner chooses between a library dedup idiom and a scheduler change before P4; scenarios whose results differ from today listed; R-CHG sized; instrumentation ≤5 days. |
> | **S6: scenario triage and baseline pin** | host-service runner, bisection, triage table, scenario format v2 (§5.2) | each runtime bug sized; a stated number of the 20 manifest scenarios mountable (the plan's "21" matches no manifest count, measurements/oracle.md), or a runtime-fix track opens that must finish before P4 exits. |
> | **S7: Cells in plain Boon** | a 10-cell sheet with add, sum and one circular pair, then the full sheet, under D9, D16 and the edge table | checks and passes cells.scn's cycle steps (its `expect_recomputed` steps become a performance check); which edges close each cycle is stated; the chosen iteration stays within cells.budget.toml (formula_edit_input_to_idle p95 3.0 ms) on 2,600 rows; every loop member is marked; any needed primitive is in the catalog before P3b is sized. |

### §8 P0 Exit (lines 915-916)
Findings: MIG-023.
Replace: "The owner signs off the spec and the spike reports, and the plan is re-baselined from them."
With:
> The owner signs off the spec and the spike reports, and the plan is re-baselined from them: every estimate is re-derived from S1's code-complete to gates-green ratio, not from line counts.

### §8 P1a and P1b (lines 918-938)
Findings: MIS-012, MIG-027, MIG-021, MIG-025, MIS-013, ARC-011, MIG-019.
Replace: P1a's body (append an exit) and P1b's exit "parse ≥1 M lines/s; reparsing the largest unit ≤1.5 ms."
With:
> P1a **Exit:** the char-by-char typing test over all examples; a probe corpus with N independent errors yields at least the N expected positioned diagnostics, with no cascades outside the affected definitions; an unclosed-opener probe per bracket kind and for TEXT.
>
> P1b adds: one change set that lands budgets v4 (§9.3), deletes the evidence lane and the old verifiers, updates architecture.rs:218-280, archives the 12 plans and 44 evidence files and re-points the 13 referencing files (until then BOON_COMPILER_PERFORMANCE_PLAN.md stays as the budgets owner_plan, marked history); a CLI and env engine-switch stub; the `Diagnostic` data model and `boon_cli check --fix` (1-1.5 k lines); the start of O1 authoring (1.5-2 engineer-weeks). **Exit:** §6 front-end numbers under the frozen profile (NovyWave ≤4 ms, TodoMVC ≤1.2 ms p95); reparsing the largest unit ≤1.5 ms; the 182 `a == b |> f()` sites parse as `(a == b) |> f()`.

### §8 P2a, P2b, P3a, P3b (lines 940-986)
Findings: MIG-017, MIG-011, SEM-035, EVI-005, EVI-012, MIS-017, MIG-020, EVI-007, SEM-011, MIS-013, MIG-014.
Replace: add to P2a and P2b; in P3a delete "- `List/replace_all` rewrites;" and replace "All of this follows the old-engine compatibility rule (§5.1)."; in P3b replace "(2-3 weeks, critical path)" and the exit.
With:
> P2a adds: fix-its for D10 binders, D24, one-input LATEST, parentheses, suffix qualification, WHEN→WHILE (the stale-copy rule) and D23 argument narrowing; the exactness rule (A3) is fixed before scheme work starts.
> P2b adds: the §4.3 effect class of every catalog entry (`Stream/*`, Timer/interval and `Build/*` included), `Value/distinct`, accessibility keys in the completeness test, and the BYTES fixture decisions (§5.2).
> P3a: "All of this follows §5.1's engine rule; next-only rewrites (List/replace_all, Http, host ports) go to the example's next overlay. The theme refactor starts only after the stale-copy list exists (the calibrated S4 scanner or the checker's rule). The measured counts per class are in §5.1."
> ### P3b: Census-driven migration (3-4 weeks, critical path)
> P3b adds: D23 narrowing (93-140 sites), the 47 Rust snippets in surviving crates, and the 113 DocumentPlan-delta scenario assertions. **Exit:** every tracked Boon source checks on the new checker with 0 errors, and warnings only where the census lists them, or has been deleted with a stated reason. The render diff shows no unintended changes.

### §8 new track R-CHG (insert before R-DOC, line 988)
Findings: ARC-001, ARC-002, ARC-008, MIG-022, MIG-021, MIS-022, MIS-001, MIS-011, MIG-024.
Replace: (new subsection)
With:
> ### ∥ R-CHG: Runtime change model (4-7 weeks, from S5; must finish by P4 exit)
> Scheduler v2 (§4.5 R7-R8), effects (R9), the distributed wire (R10) and sensitive inputs (R11), on the shared executor and in every consumer listed under §4.5, wasm32 builds included. Its author is not the O7 trace author.
> **Exit:** the O7 traces and change_probes scenarios pass on next; the glitch probe passes; per-tick cost within S5's bound; old-engine plans still pass their gates under the C2 policy.

### §8 R-DOC and R-RENDER (lines 988-1001)
Findings: MIG-021, EVI-010.
Replace: "(4-8 weeks, from S3; must finish before P5 exit)" and "Implement `glow`, shadows inside `material:`, and font `family` in the native renderer and the web host."
With (the lights sentence is conditional on QUESTIONS.md B15):
> (4-8 weeks, from S3; the performance gate closes only after P4 emits element records for the migrated sources, then 1-2 weeks of integration before P5 exit)
>
> Implement the keys the themes set that no renderer consumes, from the exhaustive list the S4 closed-contract census produces, each marked implement, contract-only or owner-declined; the known ones are `glow`, shadows inside `material:` and font `family`. Scene lights and geometry stay in the contract, unrendered and flagged, as later work. Sized after S4.

### §8 P4, P5, P6 (lines 1003-1041)
Findings: MIG-021, MIG-022, SEM-008, EVI-006, ARC-013, ARC-011, ARC-014, MIS-021, ARC-009, ARC-018, ARC-027; research/incremental_compilation_subframe_latency.md items 3 and 10.
Replace: "- Runtime delta R1-R3, R5-R6, R8 and R9, plus R7 if scheduled."; P5's exit; "- Per-definition reuse, if needed (size L on its own if NovyWave requires it)." and "Adopting any of these also requires re-running the native gates."
With (the P6 reuse line is conditional on QUESTIONS.md C4 and C5):
> P4: "- Runtime delta R1-R6 (R7-R11 are track R-CHG). - The engine switch in host_runtime (MigrationScenarioRunner), boon_runtime, the behaviour harness and phase0." P4 **Exit** adds: restart restores state under D12, a WHEN over a restored HOLD selector is re-evaluated at restore, and a THEN output read live is restored from the store.
> P5 adds the §4.7 short-circuits, the snapshot-size spike and the hover payload. **Exit:** native preflight runs with next, on the overlays, pass the product gates (R-DOC done); O3 and O5 green; focus survives an unrelated edit; the persons-pro child-compile p95 and the warm-to-preview breakdown (§6) are measured on next, each stage within its budget.
> P6: "- Per-definition reuse of FUNCTION schemes (size L); the root group is always re-solved (§4.7)." The warm corpus adds reorder-fields, create-then-break a HOLD cycle, insert lines above a definition with an error, and NovyWave edits inside `store` and in NovyView.bn (a literal, a style key, a `PASSED.store.x` read). "Adopting any of these also requires re-running the native gates and re-measuring every §6 row on the new profile."

### §8 P7 and P8 (lines 1043-1059)
Findings: MIG-020, MIG-023, MIS-025, MIG-026.
Replace: "### P7: Switch (days)", "- Port the remaining consumers (§4.8) in the same change set." and "- Update BOON_CONSOLE's stage chain (source → syntax → typed program → MachinePlan), README links and the remaining docs."
With:
> ### P7: Switch (1-3 weeks: swap every next overlay in and flip the default, C1)
> - Port the consumers not yet ported (§4.8).
>
> P8: "- Verify that the P0 console rewrite still holds; update README links and the remaining docs." and "- Ratchet the budgets; return the line caps to their pre-rewrite values or lower."

### §8 Effort (lines 1061-1070) (conditional on QUESTIONS.md C9, recommended (a))
Findings: MIG-023 (with its correction), MIG-017, MIG-024. REVIEW.md and QUESTIONS.md C9 quote 24-34 weeks with P3b at 2-3; this table takes MIG-017's 3-4 weeks for P3b, hence 25-35.
Replace: the table and the paragraph after it.
With:
> | | plan estimate | reviewer estimate |
> | --- | --- | --- |
> | Critical path, sequential, one engineer per phase | ~20-27 weeks | ~25-35 weeks: P0 4-6, P2a 3-4, P2b 2, P3b 3-4, P4 5-7, R-DOC integration 1-2, P5 3-4, P6 2, P7 1-3, P8 1 |
> | Sum of all phases and tracks | ~32-45 engineer-weeks | ~50-70: adds the P0 spikes and spec (+9-15), R-CHG (4-7), O1 authoring (1.5-2), P7 (1-3), O7 traces (sized in P0; the reference interpreter 1.5-2.5 if chosen) |
> | Calendar time with parallel agents | roughly 12-18 weeks | ~20-28 weeks: native gates and the measurement lock serialize on one machine |
>
> The reviewer's figures are estimates. After S1, every estimate is re-baselined from S1's code-complete to gates-green time, not from line counts. The largest uncertainties are R-CHG, R-DOC, the scenario triage and the closed-contract census. NovyWave holds more than half the migration edits, so its gate is the migration's critical path.

## §9 Process changes

### §9.1 intro (lines 1078-1079): the right AGENTS.md lines
Findings: MIG-029.
Replace: "This block replaces lines 30-44. Lines 10-14, the console/CPU authority paragraph, are updated to point to the reconciled contracts (P0)."
With:
> This block replaces lines 30-44. Lines 8-12, the console/CPU authority paragraph, are updated to point to the reconciled contracts (P0); line 14, the commit rule, stays.

### §9.1 AGENTS.md block (lines 1081-1127)
Findings: MIG-029 (with its correction), ARC-028, MIG-025, MIG-020.
Replace: the listed sentences inside the block.
With (the last bullet applies only under QUESTIONS.md C1 option (a); under the recommended option (d) that sentence stays as written):
> - "Boon syntax is frozen; …" becomes: "Boon syntax changes only through owner decisions (D11 and D18 are settled), never ad hoc; the settled semantic decisions are in the contract."
> - After "when an example exposes a compiler limitation, fix the compiler." add: "An old-engine limitation never justifies changing Boon source: record it in tests/compiler/divergences.toml; the example change goes to the example's next overlay, which P7 swaps in."
> - "Update expectations in the same change set as the intended change." becomes: "Update expectations in the same change set as the intended change; every `.expect` change cites a rule id, and there is no bless mode."
> - "Develop with `cargo xtask compiler-ab`;" becomes: "Develop with `cargo xtask compiler-ab` once harness v4 exists (P1b); until then make no performance claims;"
> - "No code path may depend on an example's name." becomes: "No code path may depend on an example's name (checked by the xtask lint and oracle O8); an example's engine is manifest data."
> - "Delete superseded code in the same change set;" becomes: "Delete superseded code in the same change set, except the old engine, the document v11 path and old-plan semantics, which go at P8;"
> - Add: "Parallel agents share one machine: no builds, tests or timing runs while the measurement lock is held." and "Boon files under docs/ use the `.bn.txt` extension; scratch probes live outside the repo."
> - "The playground's default compiler engine changes only in a change set that passes a fresh `cargo xtask verify-all` with that engine as the default." becomes: "An example's engine changes only in a change set that passes that example's native gate and a fresh `cargo xtask verify-all` with the declared engines."

### §9.2 Documents (lines 1134-1140): archive in P1b
Findings: MIG-027.
Replace: "- **Archive.** Delete from the tree, keeping a ≤60-line `docs/archive/COMPILER_HISTORY.md` index with last-commit hashes:"
With:
> - **Archive (P1b, with budgets v4).** budgets/compiler.toml names BOON_COMPILER_PERFORMANCE_PLAN.md as its owner_plan and compiler_performance.rs fails without it, so the archive lands in the budgets v4 change set. Delete from the tree, keeping a ≤60-line `docs/archive/COMPILER_HISTORY.md` index with last-commit hashes, and re-point the 13 files that `rg -l` finds:

### §9.3 budgets v4 (lines 1147-1169): the v3 to v4 mapping (QUESTIONS.md C7, default (a))
Findings: MIG-028, ARC-011, ARC-012, ARC-025; research/incremental_compilation_subframe_latency.md items 2 and 8.
Replace: (append before "**Protocol:**") and the Protocol paragraph's first clause.
With:
> **Mapping from v3.** This section carries a table from every v3 key to its v4 key, each marked kept, tightened, changed metric, removed or loosened. Loosenings are owner items: for example `max_doubling_ratio` 2.2 → 2.3, dropping `cancellation_max_ms` or `loaded_bundle_lookup_max_ms`, and absolute peak RSS 32/128/512 MiB replaced by deltas. `drafts/compiler.v4.toml` is not authoritative; where it disagrees with this section (supersession max, bundle lookup, RSS metric, scaling ratio), this section holds until the owner decides.
>
> **Warm gates** are p95 per edit class, not overall; the first edit after a cold open is gated separately from steady-state edits; each fixture has a warm/cold ratio ceiling; definitions rechecked per edit class are reported; NovyWave warm is report-only until plan Q20 flips. Interface fingerprints: cold with fingerprints ≤1.05 × cold without.
>
> **Protocol:** samples are taken only while the measurement lock is held and no cargo process runs (not a load-average threshold, which this machine exceeds whenever an agent works); ABBA A/B in development …

## §11 Open questions

### §11 intro (line 1197): the review's questions
Findings: REVIEW.md §1.
Replace: "**Language semantics.** These are not adopted until the owner answers them, and P0 cannot exit without the answers."
With:
> **Language semantics.** These are not adopted until the owner answers them, and P0 cannot exit without the answers. The 2026-09-30 review adds QUESTIONS.md: §A is answered before static semantics v2 is written, §B before the phase each question names, and §C applies by default unless the owner objects.

### §11 L2, L3, L6 (lines 1203-1210)
Findings: EVI-012, SEM-012, EVI-005.
Replace: "Defaults: `Log/*` allowed anywhere and logs every update of its argument; …", "**Answered 2026-09-29: model C (D30).**" and L6's answer.
With:
> L2: "… Defaults: `Log/*` is the observer class of §4.3 "Effect classes" (any context, every update of its argument, never on start or restore); Clock/wall and Random/bytes are commands."
> L3: "**Answered 2026-09-29: model C revised by D30 (WHEN copies); the notes' live-WHEN reading is superseded.**"
> L6: "… (D30). Placement applies through calls (§4.3); a WHILE nested inside a copy is a live scope until that copy runs again (QUESTIONS.md A10)."

### §11 plan Q17 and plan Q20 (lines 1229, 1232)
Findings: ARC-031, ARC-009.
Replace: "The new compiler builds for wasm32, single-threaded. In-browser compile latency is reported, not gated." and "Report-only at P7; gated once per-definition reuse lands."
With:
> Q17: "The new crates pass `cargo check --target wasm32-unknown-unknown` and the musl build (a gate). No in-browser compile consumer exists after P7, so in-browser latency is not reported until a web editor is planned; that plan chooses a Web Worker with deadline-based polling or a resumable `check_step(budget)` API."
> Q20: "FUNCTION-body edits are gated once per-definition reuse lands; edits inside the root group stay report-only (§4.7)."

## §12 Risks and later work

### §12.1 risk table (lines 1240-1251)
Findings: ARC-027, ARC-001, ARC-008, ARC-013, MIG-017, ARC-002, MIG-024, SEM-028.
Replace: the "Gate churn" row's mitigation and the "change and effect rules" row's mitigation; append rows.
With:
> - Gate churn mitigation: "… Old gates change only in P0 (line cap), after S5 if the D21 re-triage changes old-engine behaviour (C2), in P1b (budgets v4), in P5 (engine stamping), at the P7 overlay swap, and in the atomic P8."
> - Change and effect rules mitigation: "… plus the O7 signed traces and R-CHG's exit."
> - New row: "Scheduler v2 is larger than a delta or raises per-tick cost | S5 instruments and prototypes it (≤1.5 × per-tick op evaluations); its own track, R-CHG, with its own exit."
> - New row: "Warm IPC and snapshot cost exceed the 40 ms preview budget | profile-stage breakdown on engine=old in P0 and on next at P5 exit; the P5 snapshot-size spike."
> - New row: "The P6 build-profile change moves every number | every §6 row re-measured on the new profile before the checkpoint; spike numbers carry their profile."
> - New row: "NovyWave holds more than half the migration edits | its gate is the migration's critical path."
> - New row: "Persistence identities are unstable under refactors | golden vectors for the §7 persistence cases written in P0, not only in P4; the schema-diff warning (§4.5)."

### §12.2 and §12.3 (lines 1253-1265)
Findings: ARC-011, MIS-021, ARC-015, SEM-041, SEM-037, SEM-038.
Replace: "- Optional per-level parallel checking, only if the checker exceeds ~5 ms." and (append to §12.3)
With:
> - Optional per-level parallel checking, only if the checker exceeds its §6 target.
> - Symbol rename, completion and an LSP (§4.8).
> - If v11 plus delta is chosen at S1 exit, the typed v12 core (§4.5 Format).
>
> §12.3 adds: μ-types for trees and JSON-like data (QUESTIONS.md B13); kind patterns (`NUMBER => n`) if the census finds code that needs them; a whole-variant binder if B14 is declined.

## Preamble, §0 and §2

### Preamble (lines 11-13): the review coverage
Findings: EVI-011 (audit counts re-tallied against compiler_rewrite_notes/audit/: 12 subsystem audits and 4 research reports, besides claim_verification.md, plan_draft_critique.md and owner_decisions_d1_d13.md).
Replace: "- a 16-agent audit of every compiler subsystem, plus external research;" and "- a 12-agent design round, each design adversarially reviewed;"
With:
> - a subsystem audit (12 audit files) plus 4 external research reports;
> - a design round of 8 designs, 6 of them adversarially reviewed (the adopted lowering variant, lowering_alternative.md, and the change-model note, change_and_effects.md, were not; P0 reviews them);

### §0 summary table (lines 34 and 37)
Findings: ARC-011, ARC-025.
Replace: "≤16.7 ms gate, ≤8 ms goal" and "≤64 MiB"
With:
> "≤16.7 ms gate; ≤8 ms goal with per-definition reuse (P6)" and "≤48 MiB gate, 64 MiB max"

### §2 (after line 118): what the fixture numbers include
Findings: ARC-021.
Replace: (append below the table)
With:
> The counter numbers include about 2.7 ms of examples/manifest.toml validation (§3.4). The prototype numbers in §4.2 are best of 30 with target-cpu=native, not the frozen profile the §6 gates use.
