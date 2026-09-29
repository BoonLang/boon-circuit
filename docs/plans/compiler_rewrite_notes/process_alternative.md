> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

> Earlier, unreviewed variant from the first run of the same design prompt.

# Process, measurement and gates (AGENTS.md compiler rules, compiler contract doc, budgets/compiler.toml v4, measurement harness and attribution, gate inventory, cutover checklist, native handoff during migration)

## area
Process, measurement and gates (AGENTS.md compiler rules, compiler contract doc, budgets/compiler.toml v4, measurement harness and attribution, gate inventory, cutover checklist, native handoff during migration)

## summary
The current rules, budgets and gates are built around the old pipeline and cannot tell a faster, stricter new compiler apart from a broken one. They pin plan bytes (machine_plan_sha256, stale for 2 of 3 fixtures), force one thread with caches disabled, demand a 25-30 minute A/B/A 3+30 protocol, and check through a syn-based architecture gate that the receipt/proof spine exists. I propose one short AGENTS.md compiler block, one contract document of at most 300 lines that replaces all 12 BOON_COMPILER_* plans (13,143 lines) and the 44 evidence files, and a budgets/compiler.toml v4. The v4 file keeps the existing time budgets, adds a CPU-time gate so the latency lane can use threads, and adds single-thread throughput floors per phase in lines/s and a fixed-cost small-program lane (needed for the native persons-pro 4 ms compile budget). It also adds a 12-class warm edit corpus that types each edit one keystroke at a time through the playground's own compile service, and scaling gates keyed on CPU time. Behavioural oracles replace every byte oracle: an authored diagnostics corpus, every example scenario, same-build determinism, and an old-vs-new differential with a divergence list that must cite a decision. Measurement becomes a sequential interleaved A/B for development (5-30 pairs, about 1-4 minutes) plus full checkpoint and nightly runs. Attribution uses a gdb ITIMER_PROF sampler driven from xtask, with samply once the owner sets perf_event_paranoid=1, and both are validated against phase spans. The harness binary self-hash (23% of CPU samples in my TodoMVC diagnostics run) and the pretty-JSON plan export come out of every sample. Four facts I measured today change the plan. First, xtask is at 25,023 production lines against its 25,000 cap, so the architecture gate is already red. Second, the current release binary passes `boon_cli check` on only 12 of 20 manifest examples (cells among the failures) and `boon_cli run --scenario` on only 5 of 20, so the old compiler is a weak oracle and the scenario suite must be triaged before it can be trusted. Third, compiler-sample refuses to take more than one sample per process. Fourth, the playground's "proof" worker is WGPU readback, not compiler proof, so removing compiler self-proofs does not touch the native gates. Every old-pipeline gate is listed below with a disposition: replace in P1 (the compiler-performance, interactions and allocator verifiers, the work-sample schema, budgets v3), keep until cutover (the spine, classifier, phase0 probes, packed inventory, the kernel layering test), or keep permanently (no-example-branches extended to the new crates, single MachinePlan, the LOC caps). Cutover is a single atomic, owner-approved change that must pass a fresh verify-all on the playground's default engine.

## design

# Process, measurement and gates for the greenfield Boon compiler

## 0. New facts measured today that shape this design

| # | Fact | How measured |
|---|---|---|
| F1 | xtask production Rust is **25,023 lines against the 25,000 cap** (`XTASK_RUST_CAP`, crates/xtask/src/architecture.rs:9, checked at :137-142). xtask has no uncommitted changes, so HEAD is over the cap as well. Most likely `verify-architecture`, which is handoff gate 0, has been red since the last xtask commit. | I re-implemented `rust_file_line_counts` / `is_test_path` / `workspace_files` (architecture.rs:2823-2905) in Python over `git ls-files -co --exclude-standard crates/xtask`. The largest files are verify_phase0.rs (4,914), report_v2.rs (3,595), architecture.rs (2,944), compiler_performance.rs (2,732) and compiler_interactions.rs (2,612). |
| F2 | The old compiler is a weak oracle. `boon_cli check` fails 8 of the 20 manifest examples: cells, kavik_cz, fjordpulse, fibonacci, interval_latest, interval_hold, layers and flush_error_propagation. Errors seen include "dense kernel checked construction does not cover the complete project", "cannot build dense kernel checked input", a FLUSH lowering failure and a qualified-role syntax error. `boon_cli run <src> --scenario <scn>` passes only minimal, hello_world, counter_latest, flow_operators and pages (5 of 20). The other 15 are compile failures, runtime evaluation errors (todo_mvc_physical: "row 0:1:1 has no field 34"), expectation mismatches (novywave) or scenario-format drift (migrations: "unknown field `action`"). | target/release/boon_cli (built 2026-09-28 20:10 from the dirty tree), run over every `[[example]]` in examples/manifest.toml. Caveat: the playground uses the EditorRich path (C5), which may accept examples that the CLI's RuntimePacked path rejects. |
| F3 | Harness overhead shows up in the samples. In one gdb ITIMER_PROF run of TodoMVC diagnostics, **91 of 395 CPU samples (23%)** were in `producer_metadata`, which reads and SHA-256s the 39 MB binary on every process (crates/boon_cli/src/compiler_sample.rs:904-910). | The verif/prof.py sampler: 395 samples in 1.1 s wall, 2 ms CPU interval. |
| F4 | The current harness cannot measure repeated in-process compiles. It rejects them with "compiler cold observations require exactly one sample per producer process" (compiler_sample.rs:820). The native product already gates small-program compile latency: examples/persons_pro.budget.toml sets `bounded_starter_source_compile_p95 = 4.0`, `max = 8.0` and `valid_edit_to_preview_visible_p95 = 16.7`, enforced at crates/boon_native_playground/src/verify.rs:4883-4907. Today a counter-sized fresh compile is about 16-23 ms (C5 and the baseline), so this gate is very likely red. That is an estimate; I did not run the native gate. | Code reading plus a compiler-sample attempt. |
| F5 | Handoff reports are tied to the exact tree. The identity is HEAD, the full diff and the untracked files, plus a hash of the xtask executable and the manifest (crates/xtask/src/report_v2.rs:3334-3364). Any edit invalidates every existing handoff report. | Code reading. |
| F6 | The playground's "proof" lane is WGPU presented-texture readback (crates/boon_native_playground/src/proof.rs:1-64, 206-260), not compiler proof. Decision 3 (no self-proofs) therefore does not affect the native gates. The stored program-artifact digest check at crates/boon_program_runtime/src/program_core.rs:440-460 sits on a real trust boundary (decoding a stored artifact) and stays. | Code reading. |

## 1. AGENTS.md: proposed replacement for the compiler paragraphs

Replace **AGENTS.md lines 30-44** (the paragraph from "For compiler-performance work, measure the warm interaction path" through "...do not reopen linked transfer code or summary memoization.", including the uncommitted edit) with the block below. Everything else stays as it is. That includes the general engine-fix rule at lines 16-19, which stays correct: fixing an example that strictness rejects is not a workaround.

```markdown
Treat `docs/architecture/BOON_COMPILER.md` as the compiler contract (goals,
oracles, measurement, gates, migration, dated lessons) and
`docs/plans/BOON_COMPILER_REWRITE_PLAN.md` as the only compiler execution
plan. Older compiler plans are history, not authority.

Compiler work:

- The compiler is being rewritten from scratch in new crates behind the
  `boon_compiler` facade. Do not optimize or extend the old pipeline
  (`boon_typecheck`, `boon_compiler_kernel`, `boon_checked`, `boon_semantic`,
  `boon_verify`, `boon_ir` and the kernel path in `boon_compiler`). Change it
  only to keep it building, to feed the differential oracle, or to delete it
  at cutover.
- Boon syntax is frozen. The semantic decisions listed in the contract are
  settled; any other change to what a program means goes to the owner first.
  When a strict rule rejects an example, fix the example. When an example
  exposes a compiler limitation, fix the compiler.
- Correctness is behavioural: the diagnostics corpus in `tests/compiler/`,
  every example scenario, same-build determinism, and until cutover the
  old-vs-new differential with its reviewed divergence list. A stored byte
  hash of a plan or of diagnostics is never an oracle. Update expectation
  files in the same commit as the intended change.
- No self-proofs: no receipts, manifests, seals, digests or re-verification
  of what the same process just built, and no cryptographic hashing inside a
  compile. Verify plans at trust boundaries (decoding stored or received
  artifacts) and in debug and test builds only.
- Profile before choosing an optimization: `cargo xtask profile-compiler`
  (or samply once perf_event_paranoid <= 1) on the release binary, ranked by
  self and inclusive time. Timers and counters corroborate; they do not
  choose. Never change perf_event_paranoid or other system settings.
- Develop with `cargo xtask compiler-ab` (interleaved, product lane, stops
  when the effect is significant or under 1%). Run
  `cargo xtask compiler-checkpoint` before a milestone closes and after any
  build-profile change. Warm numbers count only when they come from the
  playground's own compile service driven over the edit corpus.
- `budgets/compiler.toml` holds the gates. Tightening is free. Loosening a
  time, CPU, throughput or RSS limit, or dropping a fixture, edit class,
  scenario or scaling workload, needs the owner's explicit approval. No code
  path may depend on an example's name or identity.
- Threads are allowed in latency lanes provided the CPU-time gate and the
  single-thread throughput floor pass. LTO, codegen-units, PGO and
  target-cpu changes are ordinary A/B candidates.
- Delete superseded code in the same change. Revert a rejected experiment
  before its goal ends; keep a mechanism measured as net-negative only when
  correctness needs it.
- Dated lessons live in the contract with their measurement and the
  condition for reopening them. They guide design; none bans a technique
  permanently.
- Cite numbers with a command and a commit or report path; anything else is
  an estimate and must say so.
- The playground's default compiler changes only in a commit that passes a
  fresh `cargo xtask verify-all`. Reports produced with a non-default engine
  are never handoff evidence.
```

Why each line is there: the process audit found that bans based on measurements taken before any profile (K1/K1'), the mandatory 3+30 protocol, the byte oracle, the frozen single thread, "optional" profiling and scope rules that kept regressing code in the tree are what produced two months of misattribution (a_process_history blockers). Each bullet removes one of those causes or keeps one real safeguard: loosening needs the owner, no example-specific code, the engine-fix rule, and verify-all for anything the product runs.

## 2. The compiler contract: `docs/architecture/BOON_COMPILER.md` (<= 300 lines)

It sits next to NATIVE_GPU_PIPELINE.md and BOON_CONSOLE.md. It contains no execution history. Line budget per section:

1. **Status and authority (10).** This file, `budgets/compiler.toml`, `budgets/compiler_edits.toml` and `tests/compiler/` together form the compiler contract. The single living plan is `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, with dated milestone status. Language meaning is owned by LANGUAGE_SEMANTICS.md plus the type-rule catalogue that the language panel produces; this file never defines semantics. History is indexed in `docs/archive/COMPILER_HISTORY.md`.
2. **Settled decisions (2026-09-29) (20).** No static tag narrowing: one type per function, and a WHEN takes the union of its arms. Unions are sound and fully typed, with no lenient field widening and no open-empty fallback. Strict enforcement of the documented rules, each as a positioned diagnostic. Record type identity ignores field order, and display uses written order, then first-appearance order for merged types. Syntax is frozen. Open semantic questions are listed in the plan and go to the owner.
3. **Product surface and metrics (35).** Consumers:
   - the playground compile service (apply_update, then EditorDiagnostics, then diagnostics published, then VerifiedPreview; latest-wins; supersession);
   - runtime child-program compile (persons-pro starter source);
   - distributed/role compile;
   - migration stages;
   - CLI check/dump/run.

   A metrics table gives, for cold, small, warm, throughput, CPU, RSS and scaling, the gate and the target; the values themselves live only in budgets. Also defined here: the timing window, "lines", and the four pipeline-neutral phases (front/check/lower/seal).
4. **Pipeline obligations that gates depend on (30).** The facade API stays stable, and during migration an engine selector exists with one default. One MachinePlan type (`boon_plan`). No cryptographic hash inside a compile; one fast plan identity, computed lazily at export or cache time. Cooperative cancellation with at most 1 ms stop latency. Diagnostics are published before the verified back half. Same-build determinism, independent of thread count. Phase spans and work counters are always on and returned in the result, never parsed from stderr. Debug and test builds run the full plan verifier on every compile.
5. **Correctness oracles (40).** O1 through O6 as in section 3.5 below. Section 5 also states what is not an oracle: stored byte hashes, old-compiler acceptance of a program, and message text. It defines the expectation-update rule: bless in the same commit, and the reviewer sees the diff.
6. **Trust boundaries (15).** Stored or received program artifacts (program_core.rs:440-460), plans loaded from disk or network, and console/FPGA deploy all require a full decode, verify_plan and a digest check. Nothing on the in-process preview path is re-verified.
7. **Measurement (45).** Attribution procedure and the sampler validation rule. The dev A/B protocol. Checkpoint and nightly contents. Noise controls. Report location and citation rules. Harness hygiene rules: no per-sample binary hashing, no pretty-JSON export, compact reports, no sidecars.
8. **Gates and when they must be green (25).** A table of gate, command and when required: A/B for any performance claim, checkpoint at milestone close, nightly for regression watch, verify-all at the engine switch, at cutover and for handoff claims.
9. **Migration and cutover (35).** Phases P0-P7, freeze rules for the old engine, the engine selector and preflight lane, a short form of the cutover checklist, and a rollback tag.
10. **Lessons (30).** The dated list in section 8 of this design.
11. **Change policy (10).** Who may change what: budgets (tighten freely; loosen only with the owner), expectation files, the divergence list (each entry cites a decision), lessons (add freely; retire only with a newer measurement), and the contract itself (owner review).

Content to salvage from the old plans before archiving: the rationale for the budgets (16.7 ms and 100 ms), the product-surface list, the trust-boundary list, the synthetic scaling generators (kept in the new bench), and the lessons. Deliberately dropped: the proof/receipt spine requirements, digest-migration ceremonies, the K/M/T plans, single-thread and cache-disabled normativity, and "sampling profiler optional" (BOON_COMPILER_PERFORMANCE_PLAN.md:1706-1709).

**Archive** (remove from the tree; `docs/archive/COMPILER_HISTORY.md`, at most 60 lines, lists each file with the last commit that contained it and one line of summary). Sizes are from `wc -l`:
- BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md (5,315)
- BOON_COMPILER_K0_K1_EXECUTION_2026_09_07.md (1,810)
- BOON_COMPILER_PERFORMANCE_PLAN.md (2,567)
- BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md (747)
- BOON_COMPILER_PERFORMANCE_PLAN_INTERNAL.md (589; has uncommitted edits)
- BOON_COMPILER_TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN.md (547)
- BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md (456)
- BOON_COMPILER_PROFILE_DRIVEN_PLAN_2026_09_28.md (308; untracked)
- BOON_COMPILER_REASSESSMENT_2026_09_06.md (261)
- BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md (215)
- BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md (169)
- BOON_COMPILER_K1_TRANSFER_COST_GOAL_PROMPT.md (159)
- all 44 files `docs/plans/evidence/compiler-*.json` (648 KB, including the untracked t0 file)

Before archiving, the owner decides whether the two uncommitted or untracked files get one commit for the record.

**Rewrite or re-point:**
- README.md:40-41 (links to the TENS plan and prompt) → contract and plan.
- GOAL_PROMPT.md → cut the compiler section to a 5-line pointer.
- steps.md header (lines 1-12) → pointer.
- Plans that call BOON_COMPILER_PERFORMANCE_PLAN.md the owner of compiler latency → link the contract instead:
  - BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md:50, 177, 873, 909
  - BOON_LANGUAGE_FOUNDATIONS_PLAN.md:71, 2467
  - BOON_PERSISTENCE_ARCHITECTURE_PLAN.md:23, 801
  - BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md:27-28, 79, 3365
  - BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md:21
  - BOON_CIRCUIT_SIMPLIFICATION_AND_NATIVE_RECOVERY_PLAN.md:5, 705, 800
  - TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:20
- budgets/compiler.toml:2 `owner_plan`.

No Rust source references the plan docs; the `BOON_COMPILER_*_TRACE` env vars are not doc references.

**Flag for the language panel:** TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md calls itself "authoritative". It still carries the exact-value (static selector) model and the "mandatory verified compiler spine", which decisions 3 and 4 overturn. Its rule catalogue (flow modes, etc.) is the source for the decision-6 strictness list. Extract the catalogue into the type-rule document, then archive the plan.

## 3. budgets/compiler.toml v4

Full drafts: `scratchpad/design/compiler.v4.toml` (208 lines, parses with tomllib) and `scratchpad/design/compiler_edits.toml` (all 7 sample anchors checked to occur exactly once in the current examples).

### 3.1 What changes

| v3 (today) | v4 | Why |
|---|---|---|
| `machine_plan_sha256` per fixture (:43, :53, :63), checked for exact equality (compiler_performance.rs:1534-1540) | removed; `[oracles]` section plus a `scenario` per fixture | It freezes bytes, not meaning. 2 of 3 values are stale, so the gates have been unable to pass since at least 09-10. |
| `profile_options = "...lto=false;codegen-units=16..."` (:17), `target_cpu = "generic"`, hardcoded in boon_cli build.rs | not pinned; every report records the flags | Build-profile changes are ordinary A/B experiments. |
| `compiler_threads = 1`, `compiler_caches = "disabled"` (:22-23) | `lanes.product` (threads=auto, no persistent caches) and `lanes.single_thread` | Wall-clock parallelism is allowed. A CPU-time gate and the single-thread floor stop parallelism from hiding extra work. |
| two cold modes × two lanes × 3+30 | `protocol.dev` (sequential ABBA, 5-30 pairs), `protocol.checkpoint` (3+20, both lanes), `protocol.nightly` (3+30 plus differential, empty-session, allocation counts, A/A calibration) | The two cold modes differ by about 1% (391 vs 396 ms). A 3% effect needs about 7 pairs at a CV near 2%. |
| evidence lane with a thread-local allocator | dropped from the gates (nightly `allocation-counts` only, informational) | Allocation counts are not a time proxy (lesson L4), and the lane forces a single-threaded design. |
| `peak_rss_mib_max` (32/128/512), measured as absolute process high-water; failing for weeks | `rss_delta_mib_max` = ru_maxrss after the window minus before (8/64/192), plus `warm.rss_growth_mib_max` | The 39 MB binary dominated the counter figure. Leak growth over a session was never gated. |
| warm: one literal edit through `CompileIntent::Diagnostics`, then VerifiedPreview (two solves; not the product path) | `[warm]` drives the product compile service over a 12-class keystroke corpus, in settled and typing modes, with supersession stop latency, no-op edits, CPU per keystroke and warm==cold consistency | The old warm gate measured a path the product does not use and was easy to game. |
| scaling scored by `owning_work_counter` names (:97-137, pinned in compiler_performance.rs:2602-2611 and compiler_interactions.rs:1987-1993) | CPU time of the compile window minus the size-0 baseline, sizes 800/1600/3200, maximum doubling ratio 2.3, 10 workloads | Pipeline-neutral. The profile found n^2.5 / n^1.9 / n^1.8 hazards at exactly these sizes. |
| (none) | `[throughput]` single-thread lines/s floors per phase | Keeps the per-phase speed of the batch compiler honest, independent of fixture size. |
| (none) | `[small]` in-process repeated compile of tiny programs | Predicts the native persons-pro compile budget (F4). |

### 3.2 Gates and targets (product lane wall p95; CPU p95 is 1.25 × wall)

| Fixture | diag gate / target | verified gate / target | RSS delta |
|---|---|---|---|
| counter (140 lines) | 5 / 2 ms (tightened from 10) | 10 / 4 ms (tightened from 50) | 8 MiB |
| todo-mvc-physical (3,576 input lines) | 75 / 25 ms | 300 / 60 ms | 64 MiB |
| novywave (11,926 input lines) | 250 / 75 ms | 1000 / 150 ms | 192 MiB |

The targets are the speed-ceiling estimates from a_profile and a_process_history; they are not measurements. They are reported but not gated, and the owner ratchets the gates toward them after cutover.

Throughput floors (single-thread lane, median): front 400k lines/s, check 100k, lower 60k, whole verified compile 30k. Targets: 1M / 300k / 200k. The floors are consistent with the budgets: NovyWave diagnostics = 30 ms + 119 ms = 149 ms, under 250; verified is about 398 ms, under 1000. Today the old engine checks at 9.9k lines/s (TodoMVC) and 26k (NovyWave), and parses at about 190k lines/s. These floors apply to the new engine; the old engine is reported as "reference".

Small lane: counter, hello_world and minimal compiled in one process after one warm-up (process-static tables such as the builtin ABI may be built once). Diagnostics p95 at most 1 ms, verified p95 at most 2 ms, verified max at most 4 ms.

Warm: edit to diagnostics published p95 16.7, p99 25, max 33.4 ms across all keystrokes of all classes. Verified preview p95 100 and max 200 ms. No-op edits p95 1 ms. Supersession stop p95 2 ms. CPU per keystroke p95 20 ms. Session RSS growth at most 8 MiB. Targets: diagnostics 5 ms, preview 40 ms.

### 3.3 Edit corpus (`budgets/compiler_edits.toml`)

Each edit finds a unique text anchor, which fails loudly when missing or ambiguous, then types `insert` one character per revision (or deletes one character per revision). The intermediate states are the real invalid states a user produces. `restore = true` measures the inverse edit too. The 12 required classes are: literal-text, literal-number, whitespace, comment, local-rename, add-field, remove-field, add-function, type-error-and-fix, incomplete-syntax, cross-unit, undo-to-previous. Each class exists for both TodoMVC and NovyWave, about 300 revisions in total.

Modes:
- **settled**: wait for each revision to complete; gives the latency distributions.
- **typing**: fixed 60 ms cadence; exercises supersession and measures stop latency and wasted CPU.

At the end of every sequence the session compares the warm result with a cold compile of the same final text: normalized diagnostics plus canonical plan bytes. This from-scratch consistency check guards against unsound reuse and against a whole-revision short-circuit gaming the corpus.

Anchors move with the Theme refactors (decision 4) and are updated in the same commit; only removing a class needs the owner.

### 3.4 Scaling workloads

call-depth, call-site-count, contextual-call-site-count, union-width (was static-branch-count), when-arm-count, record-width, pass-depth, list-pipeline-length, source-unit-count, dependency-cone-size. The existing synthetic-scaling generators in compiler_sample.rs are the starting point.

### 3.5 Oracles (`[oracles]`)

- **O1 diagnostics corpus**, `tests/compiler/diagnostics/<area>/<name>.bn` with a `.expect` file containing one line per diagnostic: `severity key line:col`, where the span is the primary span start. An optional `.txt` message snapshot exists for review only and never gates. Files with no `.expect` must compile clean. Seeds:
  - the scratchpad lang-req probes (q1_not_text, q2_trim_num, q3_num_minus_text, q4_field_missing, r2-r8, s1_passed_missing, t1, ...);
  - the decision-6 list;
  - the inline negative tests in the boon_typecheck and boon_compiler tests;
  - examples/bytes_negative_templates;
  - every `stage = "current"` fixture in examples/language_feature_coverage.toml as a positive.

  Expectations are written from the spec and decisions, not generated from old-compiler output: the old compiler accepts the programs that decision 6 rejects.
- **O2 examples and scenarios.** Every manifest example compiles with zero errors, and its scenario passes through `boon_behavior_harness`, comparing document frames, effect transcripts and persisted state (crates/boon_behavior_harness/src/lib.rs:42-64, 567). Given F2, this suite needs triage first (work item P5).
- **O3 determinism.** The same build compiles each fixture twice, with threads=1 and threads=auto, and must produce identical canonical plan bytes and diagnostics. The harness hashes the bytes; the compiler never does.
- **O4 differential (until cutover).** Old and new engines run on all examples and the corpus. Differences must appear in `tests/compiler/divergences.toml` as {subject, kind: new-rejects | new-accepts | diagnostic-differs | behaviour-differs, decision, note}. An entry without a decision reference fails the check. The differential only counts where the old engine passes its own scenario.
- **O5 warm==cold consistency**, as described in 3.3.
- **O6 debug verifier.** Debug and test builds run the full plan verifier on every compile. Release builds verify only at trust boundaries.

A dev-only, non-gating equality check: `compiler-ab --compare-outputs` reports whether A and B produce identical canonical plans and diagnostics. For a pure performance refactor, identical outputs settle the correctness question; differing outputs send the change to O1-O4. This compares A against B, never against a stored hash.

## 4. Measurement harness

### 4.1 Who does what

- **Sampling lives in `boon_cli`.** boon_cli is on the serde_json allowlist (architecture.rs:1002), so compiler crates stay JSON-free. `boon_cli bench-sample --engine old|next --fixture <src> --intent diagnostics|verified --lane product|single_thread [--repeat N] --out <file>` replaces compiler_sample.rs (2,015 lines, target about 400):
  - it reads units before the window and calls the facade;
  - it measures wall and CPU with getrusage(RUSAGE_SELF), plus the ru_maxrss delta;
  - it writes one compact JSON record per sample.

  Sample schema v4: `{v, engine, fixture, intent, lane, threads, lines, wall_ms, cpu_ms, rss_delta_kib, phases_ms{front,check,lower,seal}, errors, warnings, ok, counters{free-form}}`. Counters are informational and never gate.

  The old engine is supported through a small mapping of its existing phase timings, so both engines are measured by the same code with the same window. `--repeat` gives the in-process small lane.
- `boon_cli bench-warm --engine ... --corpus budgets/compiler_edits.toml --mode settled|typing` drives the product compile service in process and writes per-keystroke records. **Dependency (integration panel):** the compile service (today crates/boon_native_playground/src/compile.rs:125-165 for the worker and :535-606 for EditorDiagnostics then VerifiedPreview) has to move behind the facade so the playground and the benchmark share one object, and it has to publish diagnostics before VerifiedPreview (C5: today the snapshot is held until the preview finishes).
- **Orchestration lives in xtask**, about 900 new lines, paid for by deletions (4.8):
  - `cargo xtask compiler-ab`: A/B scheduling and statistics.
  - `cargo xtask compiler-checkpoint [--nightly]`: runs the full budgets file, the oracles and the warm corpus, and writes one compact report under `target/reports/compiler/<utc>-<shortrev>/` plus a `latest.json`. There is no byte-limit or sidecar ceremony. Reports are never committed; plans cite the rev and the command.
  - `cargo xtask profile-compiler`.
- Tracing env vars (`BOON_*_TRACE`) stay as human tools. No gate may parse stderr again; the old SCC trace parser at compiler_interactions.rs:1821-1822 read a line emitted at boon_semantic/src/dependency_manifest.rs:5594.

### 4.2 Development A/B (`compiler-ab`)

- Inputs are two binaries or two revs; for revs, xtask builds each into its own target dir.
- Four cells: {todo-mvc-physical, novywave} × {diagnostics, verified}, product lane, fresh process. Both binaries are pinned to the same core with `taskset -c 2`. Pairing, CPU pinning and nice are user-level and allowed; no governor or sysctl changes.
- Schedule ABBA. For each pair, r = ln(B/A). After at least 5 pairs, stop when the 95% t-interval of the mean r excludes 0 (report the effect with its CI), or lies inside ±1% (equivalent), or 30 pairs are reached (inconclusive).
- Accept when no cell's upper CI bound is above +1% and the targeted cell improves. Flag any cell whose cpu_ms rises by more than 5%.
- Cost: about 7 s per pair across the 4 cells with the old engine, so roughly 35 s to 3.5 min, against the estimated 25-30 min of 3+30 A/B/A. With the new engine it drops to seconds.

### 4.3 Checkpoint and nightly

The checkpoint is required before a milestone closes and after any build-profile change. It covers every section of the budgets file. The nightly run adds the differential, the empty-session lane, allocation counts and an A/A calibration that measures the A/B tool's false-positive rate. The repo has no CI (no `.github/workflows`), so "nightly" means either an owner-approved user systemd timer or scheduled task, or a manual run at checkpoints (owner question). Samples taken when the 1-minute loadavg is above 1.5 are discarded and retried; the turbo state and loadavg are recorded.

### 4.4 Attribution

**Tier A (preferred): samply** on the release binary once the owner makes `kernel.perf_event_paranoid=1` persistent (`/etc/sysctl.d`). It currently reads 2. The memory note says passwordless sudo can change it, but agents must not change security settings. samply is multi-threaded, low-overhead and needs no rebuild.

**Tier B (always available): a gdb CPU-time sampler run by xtask.** The three existing scripts:

| Script | Clock | Assessment |
|---|---|---|
| profile/gdbprof.py | wall clock, SIGINT at random 0.3-1.7× the mean interval, external gdb/MI driver | Works and matched the phase timers within about 1 pp. It parses MI text, samples IO/wait time as well, and is the most complex to drive. |
| crosscut/pmp3.py | uniform over inferior run time, gdb Python, async | Its return-address trick recovers callers of sha256_compress (asm without CFI), but `RET=0x555555554000+0x1ad0efa` is hard-coded, so it breaks on every rebuild. |
| verif/prof.py | process CPU time: `setitimer(ITIMER_PROF)` injected at main, SIGPROF stops | 45 lines, no hard-coded addresses, CPU-time uniform, unwinds through .eh_frame. My trial: 395 samples in 1.1 s wall on TodoMVC diagnostics. It stretches wall time by about 60% because each stop costs time, but CPU attribution stays unbiased: stopped time is not CPU time. |

**Recommendation:** base the collector on verif/prof.py, saved as `tools/profile/gdb_cpu_sampler.py` (about 60 lines; Python, so not under the xtask cap). Changes:
- randomize the interval by ±50% to avoid aliasing;
- sample all threads with `thread apply all bt` when threads > 1;
- use no shell: `bench-sample --out` writes to a file, because startup-with-shell off breaks stdout redirection, which my first trial hit.

The Rust side is `cargo xtask profile-compiler --fixture <src> --intent <i> [--engine next] [--runs 5] [--sampler gdb|samply] [--root <symbol>]`, about 250 lines of orchestration and aggregation. It:
- runs N processes;
- keeps only samples whose stack contains the facade compile entry (`--root`), which drops harness and process-start samples such as the 23% self-hash;
- writes `folded.txt` (flamegraph format) and `top.txt` (top 40 self and inclusive frames plus phase shares);
- **validates** by failing the run if any phase share differs by more than 5 pp from the result's phase spans.

The samply path records with `--save-only --unstable-presymbolicate` and applies the same summary. Symbolizers must extend zero-size symbols to the next symbol (lesson L3).

Moving the whole Python sampler into Rust inside xtask is not worth it: gdb Python needs no rebuild and no dependency. An in-binary SIGPROF sampler (pprof-rs, or about 200 lines of timer_create per thread plus a frame-pointer walk in a `profiling` build) is the fallback if multi-threaded profiling at low overhead becomes necessary before paranoid=1.

Timers and counters are always on (4 phase spans plus sub-spans, and a flat counter struct returned in the result). They corroborate; they never choose a target.

### 4.5 Remove harness overhead (P1)

1. `producer_metadata` SHA-256 of the 39 MB binary on every process (compiler_sample.rs:904-910): about 100-110 ms per process, 23% of samples (F3). Replace with identity embedded at build time or computed by xtask once per batch (keyed by path, inode, mtime and size).
2. Pretty-JSON MachinePlan export plus SHA per verified sample (compiler_sample.rs:1645-1646 and 1863-1864): 70-106 ms, with a 20.4 MB export for TodoMVC. Delete it. Determinism (O3) runs once per checkpoint on compact canonical bytes.
3. The refusal to run unless mimalloc is exactly 3.5.0 (validate_producer_runtime, around compiler_sample.rs:880-900) becomes report-only.
4. crates/boon_cli/build.rs:140-150 forces a rebuild and relink after docs-only commits, and emits about 431 per-file rerun-if-changed lines. Record provenance in xtask at report time instead.
5. Reports are compact JSON, one per run, with no sidecars.

### 4.6 Build profiles

- `[profile.release]` keeps today's values as the starting point. The Cargo.toml comment that says M6 attests the lane is removed.
- Add `[profile.profiling]` (inherits release, `debug = "line-tables-only"`).
- Candidates run through compiler-ab like any other change: `lto = "thin"` or `"fat"`, `codegen-units = 1`, `target-cpu`, and PGO trained on the examples plus the edit corpus. PGO needs the rustup `llvm-tools` component, which changes the pinned rust-toolchain.toml (`profile = "minimal"`); that is an owner question.
- Build time (clean build and one-crate-edit rebuild; measured today at 143 s for a kernel edit) is reported next to runtime and not gated. The owner arbitrates if a candidate pushes the one-crate rebuild above 3 minutes.
- Keep the new engine's crates out of the runtime closure so release rebuilds stay small.

### 4.7 Noise controls

Interleaving, core pinning, paired statistics, loadavg gating, an A/A calibration nightly, and machine identity recorded in reports. Reports from other machines are informational.

### 4.8 xtask line budget

At P0, delete compiler_allocator.rs (919 production lines) together with its main.rs dispatch, the `boon_cli_evidence` producer and crates/boon_cli/src/allocator.rs (278). That takes xtask to about 24,100 and makes gate 0 green again without raising the cap; raising the cap would weaken a check. At P1, delete compiler_performance.rs (2,732), compiler_interactions.rs (2,612), compiler_work_sample.rs (355 production) and compiler_producer.rs (330), and add about 900 lines of new orchestration. Net headroom is about 7k lines. At cutover, dependency_classifier.rs (1,287 production) and about 1,800 lines of architecture checks go too.

## 5. Gate inventory and dispositions

Legend: **P0/P1** = act in that migration phase; **C** = at cutover (one atomic change); **Keep** = permanent.

| Gate / artifact | Where | What it pins | Disposition |
|---|---|---|---|
| verified-semantic-compiler-spine | architecture.rs:62-65, 1294-1529; allowlist 1555-1593; opaque artifacts 1595-1730; required fields 1732-1855; core ownership 1856-2334 (receipts `== 11` at :2015); 12-entry boundary callers 2336-2546 | stage crates and artifacts, receipts, manifests, the exact caller inventory | Keep until cutover. The new crates are additive and must not touch these files. Retire at C. |
| shared-document-plan-code | :67-69, 162-216 | document_executable_backend.rs, `PLAN_MAJOR_VERSION = 11` | Keep until C. At C, replace with a behavioural plan-format test (encode, decode and verify every example plan), or retire if the file goes. |
| legacy-owner-solver-test-only | :72-74, 1194-1292 | 9 cfg-gated boon_typecheck owner modules | Retire at C. |
| normative-single-thread-compiler | :82-84, 218-279 | kernel parallel strings; `env_remove` counts in compiler_performance.rs (==1) and compiler_interactions.rs (==3) (:223-224, 254-274) | **P1:** drop the xtask sub-checks in the same change that deletes those verifiers; otherwise `read_text` fails. Retire the kernel part at C. Replaced by the CPU gate plus the single-thread lane. |
| construction-owned-checked-image-publication | :87-89, 282-652 | checked-image publication shapes and exact reference counts | Retire at C. |
| dependency-classifier-schema-v1 | :92-94; dependency_classifier.rs (3,177 lines); docs/architecture/phase1/dependency_classifier_schema_v1.toml (15,805 lines); `DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1` boon_semantic/src/lib.rs:51, used in production at :983 and :3321 | public types and fields of 19 tracked sources, including boon_document_model | Keep until C. If the new backend has to change boon_document_model earlier, regenerate with the ignored helpers (dependency_classifier.rs:2176, :2262) in the same commit. Retire at C with the TOML and the constant. |
| canonical-checked-parameter-semantics | :97-99; dependency_classifier.rs:46-67 (read_dir of boon_ir, boon_semantic and boon_typecheck src) | directory existence | Retire at C. It fails once those directories are deleted. |
| no-example-specific-engine-branches | :102-104, 2725-2822 | example ids and labels in engine sources | **Keep.** Add every new compiler crate to the prefix list in the commit that creates it. |
| single-machine-plan-executor-path | :1063-1181 | exactly one `MachinePlan` / `MachineInstance` / `MachineTemplate` struct | **Keep.** The new engine emits `boon_plan::MachinePlan` and defines no second struct. |
| no-product-serde-json | :985-1042 (allowlist :1002) | serde_json only in boon_cli, boon_phase0_baseline, xtask | **Keep.** New engine crates stay JSON-free; bench JSON lives in boon_cli and xtask. |
| xtask-rust-loc-cap | :9, 137-142 | 25,000 (currently 25,023) | **Keep the cap.** Fix at P0 by deleting compiler_allocator.rs (4.8). |
| other architecture checks (app-window fork, toolchain pin, isolated input, playground and runtime caps, report-schema) | architecture.rs | not compiler | Unaffected. |
| verify-compiler-performance | compiler_performance.rs (2,732); budgets v3; plan-hash equality :1534-1540 | old protocol, SHA oracles | **P1:** replace with `compiler-checkpoint`; delete. |
| verify-compiler-allocator | compiler_allocator.rs (919); `boon_cli_evidence`; boon_cli allocator.rs (278) | allocation evidence lane | **P0:** retire. |
| verify-compiler-interactions | compiler_interactions.rs (2,612); fails by construction (`missing_acceptance_evidence` literal :622-629, aggregate :2361-2375); SCC trace parser :1821-1822; cancellation check that times an already-cancelled token | a non-product warm path | **P1:** replace with the warm lane; delete. |
| WorkSample schema | compiler_work_sample.rs:298-311 (`deny_unknown_fields`), `has_complete_frontend_work` :314-345 (requires typecheck and kernel counters) | old phase and counter shapes | **P1:** replace with sample schema v4. |
| compiler_producer.rs | 330 lines | producer-pair provenance | **P1:** replace with compiler-ab binary pairs. |
| boon_cli compiler-sample | compiler_sample.rs (2,015): self-hash :904-910, export :1645-1646 and :1863-1864, one sample per process :820 | harness overhead, no small lane | **P1:** replace with bench-sample and bench-warm. |
| boon_cli build.rs provenance | :140-150 plus per-file rerun-if-changed | rebuild after docs commits | **P1:** trim. |
| verify-phase0 | verify_phase0.rs; docs/architecture/phase0/versions.toml:18-37, 163-201 (probes for `pub struct ParsedProgram`, `SemanticProgram`, `ContractVerifiedProgram`, `ErasedProgram`, crates/boon_semantic, crates/boon_verify); version_axes 306-319 | stage type names | Keep until C, then drop the compiler-stage probes. Whether verify-phase0 survives as a whole is outside this area. |
| packed-site-inventory | packed_site_inventory.rs:29-53 SCAN_ROOTS (boon_compiler, boon_ir, boon_typecheck, boon_semantic core_lowering.rs and program_core.rs); ledger docs/architecture/phase0/packed_site_occurrences.tsv; cargo test `checked_in_inventory_is_current` :2061 | old compiler files | Keep. At C, remove the deleted roots and regenerate the ledger. Do not add the new compiler crates; the profiler judges their data layout. |
| kernel layering test | crates/boon_compiler_kernel/src/lib.rs:50-74 (bans 7 crates; cargo test only; no CI) | kernel isolation | Deleted with the crate at C. Add an equivalent layering test for the new crates at P3. |
| dependency_classifier xtask test | dependency_classifier.rs:1644 (reads boon_checked lib.rs) | old crate | Retire at C. |
| behavior-harness flat oracle | crates/boon_behavior_harness/tests/artifact_oracle.rs (`test-flat-oracle` → boon_compiler) | old flat oracle | P2: add the engine-neutral scenario differential. Retire the flat oracle at C. |
| boon_compiler integration tests | crates/boon_compiler/tests/{artifact_oracle,kernel_transfer,map_set,nested_boolean_match,pulses,staged_compilation}.rs | old internals | Port their behaviour assertions into O1/O2 during P2-P4; delete at C. |
| budgets/compiler.toml v3 | :17, :22-23, :43/:53/:63, :97-137 | old protocol | **P1:** replace with v4. |
| examples/*.budget.toml | native product budgets, including persons_pro compile and edit-preview latency | product behaviour | **Keep** unchanged. |
| BOON_CONSOLE.md:196-206, :218-230, :501 | stage chain CheckedProgram → SemanticProgram → ContractVerifiedProgram → ErasedProgram → MachinePlan; ErasedProgram digest | prose contract (no code checks it) | P0: add a one-line migration note. C: rewrite to the new chain (source → syntax → typed HIR → MachinePlan), with console-owner sign-off. |
| LANGUAGE_SEMANTICS.md:51-52, :226-237; PASS_PASSED_AND_TODOMVC_UI_MODEL.md:17-18 | stage names | prose | Update at C. |
| AGENTS.md:30-44, the 12 plans, 44 evidence files, README links | old rules | authority | **P0** (sections 1-2). |
| handoff product gates: counter-dev, todomvc-physical, cells, novywave, persons-pro, negative | native_gpu_handoff_manifest.json orders 1-6 | behaviour plus native latency, including the persons-pro compile budgets | **Keep unchanged.** They must pass on the engine-switch commit and on the cutover commit (section 6). |

## 6. Native handoff gates during and after the migration

- **During (old engine is the default).** The manifest stays unchanged, and there is no second gate list. The architecture gate stays green if the new crates remain additive: extend the no-example-branches prefixes, keep serde_json out, emit `boon_plan::MachinePlan` only, and keep xtask under the cap. The P0 deletion is needed first because the gate is already red (F1).
- **Engine selector.** The facade reads one selector (env `BOON_COMPILER_ENGINE=old|next`, default `old`) and stamps the engine into every compile result. The preview includes it in its status snapshot, and the native verifier copies it into the report producer block. `verify-all --check-existing` rejects any report whose engine differs from the compiled default. This small verifier change strengthens a check, so it is allowed; it is a native-area item. Preflight native runs with `next` are useful signal but are never handoff evidence.
- **Preflight targets.** persons-pro's `bounded-starter-source-compile` p95 of 4 ms and max of 8 ms end to end, and its `valid-edit-to-preview-visible` p95 of 16.7 ms, are compiler-bound. The compiler checkpoint's `[small]` and `[warm]` lanes are set to predict them (1-2 ms small, 16.7 ms warm).
- **Switch commit.** It flips the default to `next`. The full manifest runs fresh (report identity is bound to the tree, F5), plus the compiler checkpoint. Only then may handoff readiness be claimed.
- **After cutover.** The manifest keeps the same 7 gate ids and commands. The architecture gate loses about 8 compiler-shaped checks and keeps the permanent ones. The compiler checkpoint remains a separate required gate for compiler work. Whether it should join the handoff manifest is an owner question; I recommend keeping it separate.
- **Observation to resolve first.** cells is a handoff example, yet `boon_cli check examples/cells.bn` fails on the current binary (F2). Confirm whether the playground's EditorRich path compiles it before treating the old engine as the baseline for cells.

## 7. Migration phases and cutover checklist

- **P0 process reset** (S): AGENTS.md block, contract, archive and index, re-pointed links; delete compiler_allocator.rs and the evidence lane (xtask back under the cap); run verify-architecture to confirm it is green.
- **P1 harness v4** (M): bench-sample, bench-warm skeleton, compiler-ab, compiler-checkpoint, profile-compiler, budgets v4; delete the old compiler verifiers and trim normative-single-thread-compiler's xtask sub-checks; baseline the old engine.
- **P2 oracles** (M-L): diagnostics corpus, divergence file, determinism, differential runner.
- **P3 new-engine crates** (other panels): architecture prefixes and layering test land with the crates; checkpoint runs on `next`.
- **P4 example refactors** for strictness and the Theme split, which must work on `next` and on `old` where possible.
- **P5 scenario triage** (can start at P0): classify each of the 15 failing `run` scenarios as harness drift, runtime bug, old-engine bug or scenario drift; fix harness, runtime and scenarios so all 20 pass on `next`.
- **P6 switch** (default = next) plus fresh verify-all.
- **P7 cutover deletion.**

**Cutover checklist** (all items on one tree; the owner approves the deletion commit):
1. O1: every corpus file matches its `.expect` on `next`, and every positive and every manifest example compiles with zero errors.
2. O2: all 20 manifest example scenarios pass through boon_behavior_harness on `next`, including the migration sequences (persons_pro v1-v3, todo/counter migrations) and persisted-state checks.
3. O4: the differential has no unexplained divergences, and every divergence cites a decision.
4. O3 and O5: determinism across thread counts, and warm==cold for every corpus sequence.
5. The compiler checkpoint is green: cold, CPU, throughput, small, warm (both modes), scaling and RSS.
6. Every compile entry point uses `next`: playground built-in and migration stages (compile.rs:757-811, 848), runtime child programs (program_core.rs:482-540), distributed/role compile (distributed_compiler.rs, currently the legacy checker), boon_cli check/dump/run, and boon_plan_executor, boon_runtime, boon_host_runtime, boon_program_runtime and boon_web_host (all depend on boon_compiler per their Cargo.toml). A grep for the old entry points (elaborate, verify_explicit_contracts, erase_and_lower, KernelSession, derive_checked_order_chains) finds no production caller outside the old crates.
7. The switch commit (P6) passed a fresh `cargo xtask verify-all` with engine = next as the compiled default.
8. Tag `compiler-v1-final` on the last tree that contains the old engine, as the rollback point.
9. The atomic deletion commit contains:
   - the old crates (boon_typecheck, boon_compiler_kernel, boon_checked, boon_semantic, boon_verify, boon_ir, and the kernel/oracle parts of boon_compiler), plus the features test-flat-oracle, legacy-owner-oracle and test-kernel-oracle;
   - the architecture checks listed in section 5 (about 1,800 lines);
   - dependency_classifier.rs, the phase1 TOML and the digest constant;
   - the phase0 compiler probes;
   - packed-site-inventory roots, with the ledger regenerated;
   - the kernel layering test;
   - the behavior-harness flat oracle and the old boon_compiler tests;
   - the divergence file and the `old` engine selector branch;
   - the BOON_CONSOLE.md, LANGUAGE_SEMANTICS.md and PASS_PASSED doc updates.
10. After deletion: `cargo test --workspace`, a fresh verify-all, the compiler checkpoint, the xtask LOC count, and a restart of the release playground as AGENTS.md requires.
11. Ratchet: the owner sets new gates at, for example, 2× the measured p95, never looser than today's.

## 8. Lessons (dated; seed for contract section 10)

- **L1 2026-09-11 (K1).** Linked residual specialization removed 71% of the targeted residual work, yet end-to-end time got 23-130% worse because shared summary programs were re-walked on every invocation. Confound: no sampling profile existed; an unmemoized predicate walk at 36% inclusive went unseen. Design consequence: instantiate a per-definition scheme by substitution, not by re-interpretation per call path. Reopen per-path evaluation only with a sampled profile showing it wins.
- **L2 2026-09-28 (T0/T1a).** The first sampling profile found a 36%-inclusive walk and a 15.8%-self hotspot that two months of timer attribution had missed; TodoMVC diagnostics dropped about 55% in one day. Profile first.
- **L3 2026-09-29.** profsum.py dropped zero-size asm symbols, so sha256_compress (14.8-15.1% of samples) was invisible. With correct attribution, SHA plus CBOR is 25-31% of verified time. Symbolizers extend zero-size symbols, and samplers must match the phase spans within 5 pp.
- **L4 2026-08 (packing).** About 150 commits cut allocations by about 45% but time by only 2-10%. Allocation counts do not stand in for time.
- **L5 2026-09-11..14.** Rejected K1 machinery stayed in the tree and took about 13 slices to recover from. Revert rejected experiments within the goal.
- **L6 2026-09-29.** The warm benchmark ran Diagnostics plus VerifiedPreview (two solves), while the playground runs EditorDiagnostics plus a shared solve and holds diagnostics until the preview finishes. Benchmark the product object.
- **L7 2026-09-29.** Byte oracles were stale for 2 of 3 fixtures since at least 09-10, RSS budgets had failed for weeks, and the interactions report fails by construction. A gate must be able to pass and must actually be run.
- **L8 2026-09-27/28 (M3/M4).** Result-type equality is not a sound reuse key. The literal edit left 155/155 owners unchanged; a type-changing edit left only 38/155. Memo keys must be exact inputs plus dependency revisions; output equality may be used only for early cutoff of dependents, and the warm corpus must include type-changing edits.
- **L9 2026-09-29.** Only 12/20 manifest examples pass `boon_cli check` and 5/20 pass `run --scenario` on the current binary. Oracles come from spec-derived expectations; the old engine contributes differential evidence only where it passes its own scenarios.


## owner_questions
- **question**: Will you make kernel.perf_event_paranoid=1 persistent (for example /etc/sysctl.d/60-perf.conf) so samply works on this machine? | **options**: (a) yes, set it persistently; (b) no, agents use only the gdb ITIMER_PROF sampler run by xtask; (c) set it only during profiling sessions | **recommendation**: (a), and keep the gdb sampler as a zero-setup fallback that is validated against phase spans | **why**: Every optimization choice depends on sampled attribution. Agents must not change security settings, and the gdb sampler is single-thread and stretches wall time by about 60% (CPU attribution remains unbiased).
- **question**: How should the 12 BOON_COMPILER_* plans (13,143 lines) and the 44 compiler evidence JSON files be archived? Two of them are uncommitted or untracked (PERFORMANCE_PLAN_INTERNAL edits and PROFILE_DRIVEN plus its t0 evidence). | **options**: (a) delete them from the tree, keeping a 60-line docs/archive/COMPILER_HISTORY.md index with the last commit hashes; (b) move them to docs/archive/compiler/; (c) keep them in place, marked historical | **recommendation**: (a), after committing the two uncommitted or untracked files once so history keeps them | **why**: Several of these documents still describe themselves as active, and agents followed them as authority. Git keeps the history, and anything in the tree keeps getting read as current.
- **question**: Should the cold budgets stay at today's values (TodoMVC 75/300, NovyWave 250/1000) with stretch targets reported, or be tightened now? | **options**: (a) keep them as gates, report targets (TodoMVC 25/60, NovyWave 75/150), ratchet after cutover; (b) tighten to the targets now | **recommendation**: (a). Counter is tightened to 5/10 ms right away because the new design makes it trivial, and the small-program lane covers the native persons-pro 4 ms budget. | **why**: The targets are estimates from the speed-ceiling analysis, not measurements. Gating on them before the new engine exists only produces red noise.
- **question**: Should the compiler checkpoint join the native GPU handoff manifest after cutover? | **options**: (a) keep it separate, required for compiler milestones and the engine switch; (b) add a manifest entry that runs `xtask compiler-checkpoint --check-existing` | **recommendation**: (a) | **why**: The manifest is the native product gate list. The product-visible compile latency is already gated there through the persons-pro compile and edit-to-preview budgets.
- **question**: May the facade read a BOON_COMPILER_ENGINE=old|next selector during the migration, with the engine stamped into native verifier reports and verify-all rejecting reports from a non-default engine? | **options**: (a) env selector plus a report stamp; (b) a cargo feature build with a separate report directory; (c) no preflight; switch blind | **recommendation**: (a) | **why**: It allows native preflight on the new engine without a second gate list, and it keeps non-default-engine reports out of handoff evidence.
- **question**: Should nightly runs be automated on this machine? | **options**: (a) a user-level systemd timer running `cargo xtask compiler-checkpoint --nightly` when the machine is idle; (b) a Claude scheduled task; (c) manual checkpoints only | **recommendation**: (c) now, with checkpoints mandatory at milestones, then (a) once the new engine lands | **why**: The repo has no CI, and gates that nobody runs went stale for weeks (lesson L7).
- **question**: May PGO experiments add the rustup llvm-tools component to the pinned toolchain (rust-toolchain.toml currently uses profile=minimal)? | **options**: yes / no / later | **recommendation**: Later, once the new engine is stable. LTO and codegen-units=1 experiments need no toolchain change and can run any time. | **why**: A toolchain change affects every build. PGO on the old engine would be wasted effort.
- **question**: Is byte-identical plan output across compiler versions required anywhere? | **options**: (a) no, determinism only within one build; (b) yes, for some persistence or deploy consumer | **recommendation**: (a). Stored program artifacts carry compiler_id and recompute and compare their digest at load (program_core.rs:342-360, 440-460), which is a trust-boundary check that works with any compiler version. | **why**: It decides whether any byte oracle survives. The design assumes none does.
- **question**: Who signs off the BOON_CONSOLE.md stage-chain rewrite (lines 196-206, 218-230 and 501 name ParsedProgram, CheckedProgram, SemanticProgram, ContractVerifiedProgram and ErasedProgram) at cutover? | **options**: owner / console workstream | **recommendation**: Add a one-line migration note now; the owner approves the rewritten chain in the cutover change | **why**: AGENTS.md names BOON_CONSOLE.md as the active console contract, and the cutover deletes every stage it names.
- **question**: Must all 20 manifest example scenarios pass through the behaviour harness before cutover, given that only 5 of 20 pass `boon_cli run --scenario` today? | **options**: (a) all 20, fixing harness, runtime and scenario drift outside the compiler; (b) only the native handoff examples plus the ones that pass today | **recommendation**: (a) | **why**: The scenario suite is the only engine-independent behavioural oracle, and the old engine cannot fill that role (it fails `check` on 8 of 20).

## risks
- **risk**: The differential oracle gives false confidence where the old engine is lenient or wrong. It accepts programs that decision 6 rejects and fails 8 of 20 examples on `check`. | **mitigation**: Author diagnostics expectations from the spec and decisions, never from old output. The differential counts only where the old engine passes its own scenario. Every divergence must cite a decision.
- **risk**: The scenario suite is mostly red today (5 of 20 via `boon_cli run`), so O2 cannot gate until it is triaged. | **mitigation**: Scenario triage (P5) starts at P0 and runs in parallel. Classify each failure as harness, runtime, scenario-format drift or old-engine bug, and fix it outside the compiler. The cutover requires 20 of 20 on the new engine.
- **risk**: A partial deletion of old-pipeline gates turns verify-all red, because reports are bound to the exact tree plus the xtask binary (report_v2.rs:3334-3364) and the syn-based checks read files that would be gone. | **mitigation**: Retire gates only in two batches. P1 covers the xtask verifiers plus the matching sub-checks of normative-single-thread-compiler. The cutover is one atomic, owner-approved commit followed by a fresh verify-all.
- **risk**: The architecture gate is already red on the xtask LOC cap (25,023 against 25,000), and any new orchestration code makes it worse. | **mitigation**: P0 deletes compiler_allocator.rs (919 lines) and the evidence lane. New sampling logic goes in boon_cli, and new xtask code is paid for by deleting the old compiler verifiers (about 6k lines). The cap stays where it is.
- **risk**: The warm corpus can be gamed by whole-revision memoization, or can pass on reuse that is unsound. | **mitigation**: Edits are typed one keystroke per revision, so every keystroke is a new revision. The corpus includes type-changing classes (add-field, remove-field, type-error-and-fix, cross-unit), measures CPU per keystroke, and checks warm against cold on every sequence's final revision.
- **risk**: The gdb sampler perturbs timing and samples only one thread per stop. | **mitigation**: Use CPU-time (ITIMER_PROF) sampling, which stays unbiased despite the stops. Fail a profile if phase shares differ by more than 5 pp from the spans. Sample all threads with `thread apply all bt` when threads > 1. Move to samply once paranoid=1.
- **risk**: Noise on a desktop machine (COSMIC session, no governor control) produces spurious A/B results. | **mitigation**: Interleaved ABBA, pairing on a pinned core, loadavg gating, a sequential CI stopping rule with a ±1% equivalence band, and a nightly A/A calibration of the false-positive rate.
- **risk**: LTO fat, codegen-units=1 and PGO slow the development loop (a one-crate edit already takes 143 s). | **mitigation**: Treat them as A/B candidates with build time reported. Keep the new engine out of the runtime closure. Use the release profile for measurement; the owner arbitrates if a rebuild goes above 3 minutes.
- **risk**: The warm lane depends on moving the compile service out of the playground binary crate. Until then there is no product-faithful warm measurement. | **mitigation**: Make it an explicit dependency on the integration panel. Until it lands, bench-warm drives the facade session with the same request sequence and is marked 'provisional', and it is not counted for a gate.
- **risk**: Expectation files turn into rubber-stamped 'bless' churn. | **mitigation**: Expectations hold only code, key and primary span, with message text kept for review only. They are blessed in the same commit as the intended change, and the reviewer sees the .expect diff. Divergence entries require a decision reference.

## work_items
- **id**: PM0 | **title**: Process reset: AGENTS.md block, contract doc, archive, links | **description**: Replace AGENTS.md:30-44 with the block in section 1. Write docs/architecture/BOON_COMPILER.md (at most 300 lines, sections as outlined) and docs/plans/BOON_COMPILER_REWRITE_PLAN.md (the living plan; its content comes from the panel). Archive the 12 BOON_COMPILER_* docs and 44 evidence JSONs with a docs/archive/COMPILER_HISTORY.md index. Re-point README.md:40-41, GOAL_PROMPT.md, steps.md and the plan cross-links listed in section 2. Add a migration note to BOON_CONSOLE.md. | **size**: S | **acceptance**: `rg -l 'BOON_COMPILER_(PERFORMANCE|TENS|K0|K1|REQUIREMENT|PROFILE|MACRO|DEFINITION|REASSESSMENT|ARCHITECTURE_REFACTOR)' docs README.md AGENTS.md budgets` returns only the history index. The contract has at most 300 lines.
- **id**: PM1 | **title**: Bring xtask back under its LOC cap; retire the allocation-evidence lane | **description**: Delete crates/xtask/src/compiler_allocator.rs (919 lines), its dispatch in main.rs, the boon_cli_evidence producer and crates/boon_cli/src/allocator.rs. Then run `cargo xtask verify-architecture`. | **size**: S | **depends_on**: PM0 | **acceptance**: xtask production lines are at most about 24,150 by the architecture.rs counter, and verify-architecture reports pass for xtask-rust-loc-cap (confirm whether any other check is red).
- **id**: PM2 | **title**: Sample schema v4 and boon_cli bench-sample (both engines) | **description**: Replace compiler_sample.rs with a lean bench-sample. The timing window starts with source units in memory. It measures wall, CPU (getrusage) and the ru_maxrss delta, reports four neutral phases, supports --repeat for the small lane, writes compact JSON via --out, and does no binary self-hash and no plan export. Map the old engine's phase timings to front/check/lower/seal. Trim boon_cli/build.rs provenance. | **size**: M | **depends_on**: PM1 | **acceptance**: Per-process overhead outside the window is at most 5 ms (today about 110 ms self-hash plus 70-106 ms export). A sampled run shows 0 samples in producer_metadata. The old engine's numbers agree with compiler-sample's elapsed_ms within 3%.
- **id**: PM3 | **title**: cargo xtask compiler-ab (sequential interleaved A/B) | **description**: Two binaries or revs, four cells, ABBA on a pinned core, paired log-ratio with a sequential 95% CI stopping rule (5-30 pairs, ±1% equivalence band), cpu_ms flagging, and optional A-vs-B canonical output equality. | **size**: M | **depends_on**: PM2 | **acceptance**: An A/A run reports 'equivalent' in at least 19 of 20 repetitions. A synthetic 3% slowdown (sleep injection) is detected within 15 pairs. A typical run takes at most 5 min with the old engine.
- **id**: PM4 | **title**: budgets/compiler.toml v4 plus compiler-checkpoint; retire the old compiler verifiers | **description**: Land the v4 budgets file (draft at scratchpad/design/compiler.v4.toml) and `cargo xtask compiler-checkpoint [--nightly]`. Delete compiler_performance.rs, compiler_interactions.rs, compiler_work_sample.rs and compiler_producer.rs. In the same change, remove the xtask env_remove sub-checks from normative_single_thread_compiler (architecture.rs:223-224, 254-274). Baseline the old engine. | **size**: M | **depends_on**: PM2, PM3 | **acceptance**: The checkpoint runs end to end on the old engine and produces one compact report. verify-architecture passes. No budget entry references a byte hash or an owning counter.
- **id**: PM5 | **title**: cargo xtask profile-compiler with the gdb CPU-time collector and the samply path | **description**: Add tools/profile/gdb_cpu_sampler.py, derived from scratchpad verif/prof.py, with a randomized interval, optional all-threads sampling and no shell. xtask orchestrates N runs, filters by root symbol, writes folded.txt and top.txt, and validates phase shares against the spans. The samply path works when paranoid is at most 1. The symbolizer extends zero-size symbols. | **size**: S | **depends_on**: PM2 | **acceptance**: On TodoMVC verified: at least 1,000 in-window samples in at most 2 min, and phase shares within 5 pp of the spans. The run fails loudly when the validation fails.
- **id**: PM6 | **title**: Diagnostics corpus and oracle runner | **description**: tests/compiler/diagnostics with .bn/.expect (severity, key, line:col), seeded from the lang-req probes, the decision-6 list, inline negative tests, bytes_negative_templates, and the language_feature_coverage current fixtures as positives. Add a runner with --bless. Add determinism (O3) and a differential runner with tests/compiler/divergences.toml (each entry cites a decision). | **size**: L | **depends_on**: PM0 | **acceptance**: The corpus holds at least 150 files covering every decision-6 rule. The runner reports per-engine pass/fail. An undocumented divergence fails. Determinism holds on the old engine for all passing examples.
- **id**: PM7 | **title**: Scenario suite triage (behavioural oracle) | **description**: Classify and fix the 15 manifest examples failing `boon_cli run --scenario` (harness drift, scenario-format drift such as migration `action`, runtime bugs, old-engine bugs) using boon_behavior_harness traces. Old-engine bugs are only recorded, not fixed. | **size**: M | **depends_on**: PM0 | **acceptance**: Every failure has a recorded class. All harness, scenario and runtime-class failures are fixed. The suite runs in the checkpoint.
- **id**: PM8 | **title**: Warm edit corpus and bench-warm on the product compile service | **description**: Complete budgets/compiler_edits.toml: 12 classes × {TodoMVC, NovyWave} (draft at scratchpad/design/compiler_edits.toml). bench-warm has settled and typing modes, supersession stop latency, CPU per keystroke, RSS growth and warm==cold checks. It depends on the integration panel moving CompileWorker behind the facade and publishing diagnostics before VerifiedPreview. | **size**: M | **depends_on**: PM2; integration: compile service in facade | **acceptance**: Every class is present for both fixtures with anchors unique. Per-class p50/p95/max are reported. The warm==cold check covers every sequence. Results are provisional until the product service is driven.
- **id**: PM9 | **title**: Scaling workloads v4 | **description**: Port the synthetic generators; add union-width, when-arm-count, record-width, pass-depth and list-pipeline-length. Score the CPU-time doubling ratio at sizes 800/1600/3200 minus the size-0 baseline. | **size**: S | **depends_on**: PM2 | **acceptance**: On the old engine it reproduces the known hazards (static-branch about n^2.5, call-depth about n^1.9, contextual-call-site-count about n^1.8) as failures above 2.3.
- **id**: PM10 | **title**: Architecture gate hygiene for the new crates | **description**: Add each new compiler crate to the no-example-specific-engine-branches prefixes in the commit that creates it. Add a layering cargo test for the new crates. Confirm the new engine emits boon_plan::MachinePlan only and has no serde_json dependency. | **size**: S | **depends_on**: new engine crates exist | **acceptance**: verify-architecture passes with the new crates present, and a seeded example-name branch in a new crate fails the gate.
- **id**: PM11 | **title**: Engine selector plus engine stamp in native reports | **description**: Add a facade selector BOON_COMPILER_ENGINE=old|next (default old). Stamp the engine into compile results, the preview status and the native verifier producer block. verify-all --check-existing rejects reports whose engine differs from the default. | **size**: S | **depends_on**: new engine usable through facade | **acceptance**: A native preflight with next produces reports that verify-all refuses to count. Default runs are unchanged.
- **id**: PM12 | **title**: Build-profile experiments | **description**: Add [profile.profiling]. A/B lto=thin/fat, codegen-units=1 and target-cpu on the new engine through compiler-ab with build times reported; PGO after the owner answers the llvm-tools question. | **size**: S | **depends_on**: PM3, new engine | **acceptance**: Each candidate has a recorded effect with its CI and build time. Winners land in Cargo.toml with a dated lesson or plan entry.
- **id**: PM13 | **title**: Engine switch commit | **description**: Flip the facade default to next. Run the full native handoff manifest fresh plus the compiler checkpoint. | **size**: S | **depends_on**: PM4-PM11, new engine meets checkpoint | **acceptance**: verify-all is green with engine=next on the switch tree, including the persons-pro compile and edit-to-preview budgets, and the compiler checkpoint is green.
- **id**: PM14 | **title**: Cutover: atomic deletion of the old pipeline and its gates | **description**: Execute checklist items 1-11 from section 7: delete the old crates, features and tests; remove the architecture checks, the dependency classifier with its TOML and digest, the phase0 probes, the packed-inventory roots and the kernel layering test; update the BOON_CONSOLE, LANGUAGE_SEMANTICS and PASS_PASSED docs; tag compiler-v1-final beforehand. | **size**: L | **depends_on**: PM13, owner approval | **acceptance**: cargo test --workspace, a fresh verify-all and the compiler checkpoint are green on the cutover tree. A grep for old entry points finds no production caller. xtask stays under the cap with more than 5k lines of headroom.

## perf_targets
- **metric**: TodoMVC cold diagnostics p95 (product lane, wall) | **target**: gate 75 ms; target 25 ms | **basis**: budgets/compiler.toml fixture budget; target from the a_profile speed-ceiling estimate (15-25 ms)
- **metric**: TodoMVC cold verified p95 | **target**: gate 300 ms; target 60 ms | **basis**: budget; estimate 30-60 ms
- **metric**: NovyWave cold diagnostics p95 | **target**: gate 250 ms; target 75 ms | **basis**: budget; estimate 50-75 ms
- **metric**: NovyWave cold verified p95 | **target**: gate 1000 ms; target 150 ms | **basis**: budget; estimate 80-150 ms
- **metric**: counter cold diagnostics / verified p95 | **target**: gate 5 / 10 ms (tightened from 10/50); target 2 / 4 ms | **basis**: Today 7/16 ms, of which 41% is manifest loading (a_profile)
- **metric**: CPU time per cold compile (all threads) | **target**: at most 1.25 × the wall budget for every fixture and intent | **basis**: Allows threads without hiding duplicated work
- **metric**: Single-thread throughput floors | **target**: front at least 400k lines/s, check at least 100k, lower at least 60k, whole verified at least 30k (targets 1M / 300k / 200k) | **basis**: Consistent with the NovyWave budgets (149 ms diagnostics, 398 ms verified); old engine checks at 9.9k-26k lines/s
- **metric**: Small-program in-process compile (counter, hello_world, minimal) | **target**: diagnostics p95 at most 1 ms, verified p95 at most 2 ms, verified max at most 4 ms | **basis**: Native persons-pro budget: bounded-starter-source-compile p95 4 ms, max 8 ms end to end (examples/persons_pro.budget.toml; verify.rs:4883-4907)
- **metric**: Warm edit to diagnostics published (12-class corpus, all keystrokes) | **target**: p95 16.7 ms, p99 25, max 33.4; target p95 5 ms | **basis**: Existing warm budget; today 377-396 ms
- **metric**: Warm edit to verified preview | **target**: p95 100 ms, max 200; target p95 40 ms | **basis**: Existing warm budget; today about 780 ms (benchmark) / about 1160 ms edit-to-preview
- **metric**: Supersession stop latency | **target**: p95 at most 2 ms | **basis**: Today up to the full 365-800 ms phase (no polls in kernel or semantic)
- **metric**: No-op edit (whitespace/comment) | **target**: p95 at most 1 ms | **basis**: Measures retention correctness and cost floor
- **metric**: Scaling doubling ratio (CPU, 1600 to 3200, minus baseline) | **target**: at most 2.3 for all 10 workloads | **basis**: n log n is about 2.19 at these sizes; the old engine shows n^1.8-n^2.5 hazards
- **metric**: RSS delta over the compile window | **target**: counter 8 MiB, TodoMVC 64 MiB, NovyWave 192 MiB; warm session growth at most 8 MiB | **basis**: Replaces absolute peak budgets dominated by the 39 MB binary; warm grows 166 to 194 MB over 10 edits today
- **metric**: Harness per-sample overhead outside the compile window | **target**: at most 5 ms | **basis**: Today about 110 ms binary self-hash (23% of samples) plus 70-106 ms pretty-JSON export per verified sample
- **metric**: Development A/B turnaround | **target**: at most 5 min for 4 cells detecting a 3% effect | **basis**: Today's 3+30 A/B/A protocol is estimated at 25-30 min per candidate
- **metric**: Profile turnaround | **target**: at least 1,000 in-window samples on TodoMVC verified in at most 2 min, phase shares within 5 pp of spans | **basis**: Trial: 395 samples per TodoMVC diagnostics run in 1.1 s wall with the ITIMER_PROF gdb sampler

## deletions
- **what**: 12 BOON_COMPILER_* plan documents (archived out of tree) | **size**: 13,143 lines
- **what**: docs/plans/evidence/compiler-*.json | **size**: 44 files, 648 KB
- **what**: xtask compiler_allocator.rs + boon_cli_evidence target + boon_cli allocator.rs (P0) | **size**: 919 + 278 lines
- **what**: xtask compiler_performance.rs, compiler_interactions.rs, compiler_work_sample.rs, compiler_producer.rs (P1) | **size**: 2,732 + 2,612 + 435 + 330 = 6,109 lines
- **what**: boon_cli compiler_sample.rs (replaced by a lean bench-sample of about 400 lines) and most of build.rs provenance | **size**: 2,015 lines to about 400; build.rs 271 to about 40
- **what**: budgets/compiler.toml v3 protocol pins, machine_plan_sha256 oracles, owning_work_counter scaling | **size**: whole file rewritten
- **what**: Architecture-gate compiler checks at cutover (architecture.rs 162-652, 1194-1529, 1555-2546 and dispatch at 62-99) | **size**: about 1,800 lines
- **what**: dependency_classifier.rs + docs/architecture/phase1/dependency_classifier_schema_v1.toml + boon_semantic digest constant (cutover) | **size**: 3,177 + 15,805 lines
- **what**: phase0 compiler-stage probes (versions.toml:18-37, 163-201, 306-319), packed-site-inventory old roots, kernel layering test (lib.rs:50-74), behavior-harness flat oracle, 6 boon_compiler integration tests (cutover) | **size**: hundreds of lines plus regenerated ledger
- **what**: target/reports/compiler-performance (local, untracked; owner may clean) | **size**: 121 files, about 1.5 GB

## evidence
- xtask production LOC re-counted at 25,023 against XTASK_RUST_CAP 25,000 (crates/xtask/src/architecture.rs:9, 137-142, counter at 2823-2905), by reimplementing rust_file_line_counts over `git ls-files -co --exclude-standard crates/xtask`
- boon_cli check over all 20 manifest examples: 12 pass; fail are cells, kavik_cz, fjordpulse, fibonacci, interval_latest, interval_hold, layers, flush_error_propagation (target/release/boon_cli built 2026-09-28 20:10)
- boon_cli run --scenario over all 20 manifest examples: 5 pass (minimal, hello_world, counter_latest, flow_operators, pages); todo_mvc_physical fails with 'row 0:1:1 has no field 34', migrations with 'unknown field `action`'
- gdb ITIMER_PROF sampler (scratchpad verif/prof.py) on TodoMVC diagnostics: 395 samples, 1.1 s wall; 91 of 395 in boon_cli producer_metadata (compiler_sample.rs:904-910, SHA-256 of the 39 MB binary); 104 in sha256_compress
- compiler_sample.rs:820 refuses more than one sample per process; pretty-JSON plan export plus SHA at compiler_sample.rs:1645-1646 and 1863-1864
- budgets/compiler.toml: profile_options :17, compiler_threads/caches :22-23, machine_plan_sha256 :43/:53/:63, owning_work_counter :97-137; plan-hash equality at crates/xtask/src/compiler_performance.rs:1534-1540
- crates/xtask/src/compiler_interactions.rs:622-629 hard-coded missing_acceptance_evidence (fails by construction); :1821-1822 parses 'boon_semantic dependency_manifest_v7 projection_graph:counts'
- crates/xtask/src/compiler_work_sample.rs:298-345 deny_unknown_fields WorkSample and has_complete_frontend_work requiring typecheck and kernel counters
- architecture.rs:218-279 normative_single_thread_compiler reads compiler_performance.rs / compiler_interactions.rs and requires env_remove counts 1 and 3
- architecture.rs:985-1042 no-product-serde-json allowlist (boon_cli, boon_phase0_baseline, xtask at :1002); :1063 single MachinePlan/MachineInstance/MachineTemplate definition; :2725-2822 example-name branch ban with prefix list
- crates/xtask/src/report_v2.rs:3334-3364: handoff report identity = HEAD + diff + untracked + xtask executable + manifest
- examples/persons_pro.budget.toml: bounded_starter_source_compile_p95 = 4.0, max 8.0; valid_edit_to_preview_visible_p95 = 16.7; enforced at crates/boon_native_playground/src/verify.rs:4883-4907
- crates/boon_native_playground/src/proof.rs: the playground proof worker is WGPU presented-texture readback, not compiler proof
- crates/boon_program_runtime/src/program_core.rs:342-360, 440-460: stored program artifacts carry compiler_id and plan digest, recomputed and compared at load (trust boundary)
- docs/architecture/native_gpu_handoff_manifest.json gates: 0 architecture, 1 counter-dev, 2 todomvc-physical, 3 cells, 4 novywave, 5 persons-pro (profile-benchmark-steps valid-edit-preview, corrected-edit-preview), 6 negative
- crates/boon_compiler_kernel/src/lib.rs:50-74 kernel layering test bans 7 crates; crates/xtask/src/packed_site_inventory.rs:29-53 SCAN_ROOTS include boon_compiler, boon_ir, boon_typecheck and boon_semantic files
- boon_compiler dependents (Cargo.toml): boon_cli, boon_host_runtime, boon_program_runtime, boon_native_playground, boon_plan_executor, boon_runtime, boon_web_host, optional boon_phase0_baseline and boon_behavior_harness
- /proc/sys/kernel/perf_event_paranoid = 2; samply installed at ~/.cargo/bin/samply; gdb at /usr/bin/gdb
- Draft files: drafts/compiler.v4.toml and compiler_edits.toml (anchors checked to occur exactly once)
