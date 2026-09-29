> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

# Process, measurement and gates

## area
Process, measurement and gates

## summary
The old process pinned the old compiler in place. It used a byte oracle that goes stale: the SHA-256 of a 20 MB pretty-JSON plan, already stale on TodoMVC and NovyWave. It required 3+30 A/B/A runs in 2 lanes and 2 cold modes, about 25-30 minutes per candidate. It froze the build profile, single-threading and caches, treated the profiler as optional, banned technique classes, and gated structure through about ten syn/string checks of the proof spine inside handoff gate 0. I propose a replacement AGENTS.md compiler section (full text below) and one contract of at most 300 lines, docs/architecture/BOON_COMPILER.md. The 12 BOON_COMPILER_* plans (13,143 lines) and the 44 evidence JSON files (652 KB) are archived. budgets/compiler.toml becomes format 4 (written to the scratchpad as design/compiler.v4.toml). It keeps the owner's time budgets, tightens counter's budget and measures RSS as a delta. It adds engine-neutral phase throughput floors and threads in the product lane with a CPU-time gate of 1.25x the wall budget. It adds a single-thread throughput lane, a small-program lane that predicts the native persons-pro 4/8 ms starter-compile gate, and time-based scaling gates. A warm edit corpus (design/compiler_edits.toml, about 900 revisions over 16 classes, every anchor checked against the current examples) runs through the product compile service in settled and typing modes. The format drops every byte-hash oracle and replaces it with a coded diagnostics corpus, example scenarios, warm-vs-cold checks, same-build determinism and an old-engine differential with classified divergences. The harness becomes three things: a lean compile-bench producer, which removes the per-process self-hash and pretty-JSON export (about 110-220 ms per process today); cargo xtask compiler-ab (interleaved ABBA, stops when the 99% CI clears 0 or falls inside +-1%); and cargo xtask compiler-checkpoint with a --nightly variant. For attribution I tested this round that perf_event_open with user-only sampling and callchains works at perf_event_paranoid=2. Kernel-mode events are denied, and inherited per-task events need one buffer per CPU (cpu=-1 with inherit fails mmap with EINVAL; cpu=0 with inherit works). samply 0.13.1 refuses to start at paranoid=2. So I recommend an in-repo perf_event_open sampler in xtask over porting the gdb scripts: gdbprof.py gets about 175 samples per run, sees only the current thread and needs gdb, and pmp3.py hard-codes a return address specific to one binary. I checked every gate, test, doc and budget entry that pins the old pipeline and classified each one. The obsolete compiler-performance harness (about 7,000 xtask lines) is deleted at migration start; this also clears the xtask line cap, which is already red (25,023 against 25,000, recounted today). Structural checks of old files stay until an atomic, owner-approved cutover, and the example-specific-branch guard is extended to the new crates on day one. The native GPU handoff manifest is met by the old engine during migration. The new engine runs shadow product gates behind an engine selector, and cutover is one commit that re-runs all 7 gates plus verify-all.

## design
## 0. New facts established this round (inputs to the design)

- **The xtask line cap is already red.** I re-implemented the count from `rust_file_line_counts` (architecture.rs:2859-2882) over the git-listed `crates/xtask` files: **25,023 production lines against `XTASK_RUST_CAP = 25_000`** (architecture.rs:9, checked at :137-142). The last `target/reports/report-v2/architecture.json` (2026-08-23) passed at 24,999. Commit 840c1c5c (2026-09-10) added 47 lines to compiler_work_sample.rs. Largest compiler-owned files: compiler_performance 2,732, compiler_interactions 2,612, packed_site_inventory 2,001, dependency_classifier 1,287, compiler_allocator 919, compiler_work_sample 355, compiler_producer 330.
- **User-only perf sampling works at paranoid=2.** `/proc/sys/kernel/perf_event_paranoid` = 2. A ctypes probe of `perf_event_open` gave:
  - SW task-clock and HW cycles with `exclude_kernel=1`, both counting and sampling IP+CALLCHAIN: **allowed**.
  - The same events with kernel included: EPERM.
  - `cpu=-1, inherit=1`: opens, but mmap fails with EINVAL.
  - `cpu=0, inherit=1`: opens and mmaps.
  - Other limits: `perf_event_max_sample_rate` = 15000, `perf_event_max_stack` = 127, `ptrace_scope` = 1.
  - `samply 0.13.1 record` exits at once: "needs to be set to 1 or lower".
- **Ad-hoc samplers:** agents in this audit wrote at least 8 separate `pmp*.py`/`gdbprof.py` copies (scratchpad crosscut/, kernel-solver/, backhalf/, frontend/, semantic/, profile/). A repo-owned profiler is overdue.
- **Native gates already carry compiler latency.** `examples/persons_pro.budget.toml` sets bounded_starter_source_compile p95 4.0 / max 8.0 ms, valid_edit_to_preview_visible p95 16.7 ms and keystroke_to_editor_visible p95 16.7 ms. These are measured in boon_native_playground/src/verify.rs:4868-4920. The compiled program is the one-line program typed in steps `valid-edit-preview` and `corrected-edit-preview` of examples/persons_pro.scn:60-85.
- **Diagnostics have no codes.** `CompilerDiagnostic { path, line, column, start, end, message }` (boon_compiler/src/lib.rs:189-196). A diagnostics oracle that does not compare message text needs stable codes from the new compiler.
- **Machine and repo state:**
  - scaling_governor = powersave, turbo on, i7-9700K without SHA-NI.
  - There is no CI (`.github/` is absent), so "nightly" needs an owner decision.
  - Only `architecture.json` exists under target/reports/report-v2, so verify-all --check-existing cannot pass today regardless of the compiler.
  - The `boon_compiler` facade has 9 dependent crates: behavior_harness, phase0_baseline, cli, host_runtime, plan_executor, program_runtime, web_host, native_playground, runtime.
- **Harness overhead per process:**
  - compiler_sample.rs:907-908 hashes its own 39 MB binary: about 110 ms per process.
  - compiler_sample.rs:1644-1648 and :1863-1864 hash a pretty-JSON plan: 20.4 MB and about 105 ms on TodoMVC.
  - `validate_producer_runtime` refuses to run unless mimalloc is exactly 3.5.0.
  - build.rs:143-146 re-runs provenance on every commit, docs-only commits included.

---

## 1. Proposed replacement for the compiler part of AGENTS.md

This replaces lines 30-44 (the paragraph "For compiler-performance work …" through "… do not reopen linked transfer code or summary memoization."). Every other paragraph stays as it is, including the engine-fix rule at lines 16-19, the console paragraph, the launch rules and the handoff paragraph.

```markdown
Compiler work follows `docs/architecture/BOON_COMPILER.md`: the owner's
semantic decisions, the budgets, the correctness oracles and the measurement
rules. The living plan it names is the only compiler plan; archived
`BOON_COMPILER_*` documents are history, not instructions.

- Greenfield. The new compiler is built in its own crates. Do not optimize,
  extend or refactor the old pipeline (`boon_typecheck`, `boon_compiler_kernel`,
  `boon_checked`, `boon_semantic`, `boon_verify`, `boon_ir`,
  `boon_compilation_db` and the old internals of `boon_compiler`). It stays the
  playground's default engine until cutover, serves as a test-only differential
  oracle, and is deleted at cutover. Touch it only when a native GPU handoff
  gate needs a fix before cutover. Switching the default engine is the cutover
  and needs the owner's go-ahead.
- Semantics. Boon syntax does not change. The decisions recorded in the
  contract (no static narrowing, sound unions, strict diagnostics, order-free
  record type identity) are settled. Any other change to what a program means,
  or to which programs are accepted, is a question for the owner, not an
  implementation choice.
- No self-proofs. The compiler does not build receipts, digests, seals,
  manifests or construction images, and does not re-verify its own output on
  the compile path. Verification runs where input is untrusted (decoding or
  loading a plan or artifact, hardware deploy) and in debug and test builds.
- Behavioural oracles. Correctness means: the diagnostics corpus matches its
  `.expect` files, every example compiles and passes its scenario, warm results
  equal cold results, the same build is deterministic, and (until cutover)
  every divergence from the old engine is classified in
  `tests/compiler/divergences.toml`. No byte hash is a correctness oracle.
  Update `.expect` files and divergence entries in the same commit as the
  change that explains them; making a negative case compile, or an example
  fail, needs the owner.
- Profile first. Before choosing an optimization target, record a sampled
  profile of the release binary on the affected fixture
  (`cargo xtask profile -- ...`) and cite its top frames. Phase timers and
  counters corroborate a profile; they do not replace it. Do not change kernel
  or security settings to profile.
- Measure lightly while developing: `cargo xtask compiler-ab` (product lane,
  fresh processes, interleaved, stops as soon as the result is clear). Run
  `cargo xtask compiler-checkpoint` when a milestone closes, after any
  build-profile change and before cutover; `--nightly` adds the slow lanes.
  Build-profile changes (LTO, codegen units, PGO, target CPU) are ordinary A/B
  candidates.
- Budgets. `budgets/compiler.toml` and `budgets/compiler_edits.toml` may be
  tightened or extended freely. Loosening a time, CPU, throughput or RSS limit,
  raising a scaling ratio, or removing a fixture, edit class, scenario or
  workload needs the owner's explicit approval.
- Keep the tree honest. Delete code a change supersedes in the same change.
  Revert an experiment that measures net-negative within its own goal. No
  fixture- or example-specific code paths, and no cache hits in cold
  measurements. Plans cite report paths and commits, not free-standing
  numbers; evidence stays in `target/reports`, not in tracked docs.
- Lessons, not bans. The contract's dated lessons record what was measured,
  on which tree, and when to reopen the question. No technique (memoization,
  function summaries, a second engine during migration, parallelism, caches)
  is forbidden by name.
```

Rationale for each old rule:

| Old rule | New handling |
| --- | --- |
| 3+30 A/B/A in both lanes and cold modes | Development uses compiler-ab; the full matrix runs only at checkpoint/nightly. |
| "Never edit budgets/compiler.toml" | Tighten freely; loosening needs the owner. |
| "Do not grow the summary evaluator…", "reuse keys … never final result-type equality", "do not reopen linked transfer code or summary memoization" | Become dated lessons L2/L4. |
| "measure verify-compiler-interactions first" | Replaced by the warm corpus lane. |
| Profile-first | Kept, with a tool that works without sudo. |

---

## 2. The compiler contract: `docs/architecture/BOON_COMPILER.md` (at most 300 lines)

Outline with a line budget per section:

1. **Status and authority (≈10).** Replaces the 12 plans. Names the one living plan (the greenfield plan this panel produces). The owner approves changes to §3 and §5 limits; the implementer updates status lines. `AGENTS.md` points here, and BOON_CONSOLE.md defers compiler stage names to this document.
2. **Product surface (≈35).**
   - Inputs: source units in memory, then edit deltas.
   - Outputs: coded, positioned diagnostics; the editor language snapshot; a runnable MachinePlan.
   - The compile-service contract that the playground and the bench share:
     - `submit(rev, delta)`.
     - The diagnostics lane publishes before any lowering.
     - The preview lane is latest-wins; a newer revision stops in-flight work within 2 ms; the last good plan stays live.
   - Clients of the same service: the child-program path (persons_pro), migration stages (predecessor stages cached by content) and distributed packages.
3. **Semantics (≈45).**
   - Authority: `LANGUAGE_SEMANTICS.md` plus this section.
   - The 2026-09-29 decisions in short form: no narrowing (one type per function; a WHEN has the union of its arms; requirements from all arms); sound unions (no lenient widening, no open empty object, reading a field that is not guaranteed is an error); the strictness list (TEXT |> Bool/not(), 5 |> THEN {6}, TEXT - 1, LATEST over NUMBER vs TEXT, missing PASSED fields, recursion, instantaneous cycles, field self-reference, OUT producer rules); field-order display (written order; merged types in first-appearance order).
   - What counts as a semantic change: anything that changes accepted programs or runtime behaviour beyond these decisions.
   - Diagnostic codes are stable identifiers; message text is free to improve.
4. **Pipeline shape (≈30).**
   - Engine-neutral phases: front, check, lower, seal.
   - One interner, dense ids, and retained per-definition state for warm.
   - The rule against self-proofs.
   - Verification at trust boundaries only: plan/artifact decode (`decode_program_artifact`, program_core.rs:453), artifact store/load, hardware deploy, debug/test builds.
   - A plan digest is computed lazily, and only where a consumer exists (program artifacts, app packages).
5. **Metrics and budgets (≈40).**
   - Pointer to budgets/compiler.toml.
   - Definitions: timing window; t_diag and t_plan; the product and single-thread lanes; the CPU gate; throughput; warm settled and typing modes; freshness; supersede-stop; RSS delta.
   - The change policy.
6. **Oracles (≈35).**
   - `.expect` format.
   - examples + scenarios + migrations.
   - warm==cold.
   - Determinism.
   - Differential and the divergence classes.
   - Debug verifier.
   - Explicitly not oracles: byte hashes across versions, the old engine's messages, id numbering.
7. **Measurement (≈35).**
   - compile-bench, compiler-ab, compiler-checkpoint and profile: when each runs.
   - The statistics rule.
   - Noise controls: record governor, turbo and loadavg; never change them.
   - Report format.
   - Summarizer requirements: attribute addresses to the nearest preceding symbol when no sized symbol matches, and cross-check sample phase shares against timers.
8. **Migration and cutover (≈30).** Engine selector, shadow native runs, and a checklist pointer (§7 of this design).
9. **Dated lessons (≈40).** Each entry gives date, tree, measurement, what it means for the new engine, and a reopen condition:
   - **L1 (2026-09-28).** The first sampled profile found `direct_result_summary_supported` at 36% inclusive and `schedule_variable` at about 16-19% self, both missed by two months of timer-based attribution. The summarizer dropped zero-size asm symbols and hid SHA-256 (about 15% of samples).
   - **L2 (2026-09-11).** K1 linked transfer on the per-call-path kernel regressed diagnostics by 29% (TodoMVC) and 82% (NovyWave). The three measurements contradict each other and are confounded by defects that were never profiled. In the new engine, per-definition schemes instantiated per call site are the default design, not a banned idea.
   - **L3.** The pretty-JSON plan SHA oracle froze representation, and 2 of its 3 values went stale without anyone noticing.
   - **L4.** Warm retention was declared five times (Phase 4, K3, Goal B, M4, T5) and never built. The benchmark measured a different request sequence than the product and was hard-coded to fail.
   - **L5.** About 150 packing commits cut allocations by 45% but time by only 2-10%.
   - **L6.** The 3+30 matrix cost 25-30 minutes per candidate. The two cold modes differ by about 1%. T0's 45% effect was clear at n=8.
   - **L7.** SHA-256+CBOR self-proofs took 25% (TodoMVC) and 31% (NovyWave) of verified time, and nothing on the preview path consumed them.
   - **L8.** The rejected K1 machinery stayed in the tree under scope rules.
   - **L9.** The frozen single-thread, cache-free, LTO-off protocol made parallelism, retention and LTO/PGO impossible to measure.

**Archive (delete from the tree; the contract names commit 286aa974 as the last one containing them):**
- ARCHITECTURE_REFACTOR_PLAN 5,315 lines
- DEFINITION_ARTIFACT_RESEARCH 456
- K0_K1_EXECUTION_2026_09_07 1,810
- K1_TRANSFER_COST_GOAL_PROMPT 159
- MACRO_ARCHITECTURE_RESEARCH 747
- PERFORMANCE_PLAN_INTERNAL 589
- PERFORMANCE_PLAN 2,567
- PROFILE_DRIVEN_PLAN_2026_09_28 308 (untracked)
- REASSESSMENT_2026_09_06 261
- REQUIREMENT_AGGREGATION_GOAL_PROMPT 215
- TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN 547
- TENS_OF_MILLISECONDS_GOAL_PROMPT 169
- docs/plans/evidence/compiler-*.json: 44 files, 652 KB, plus the untracked t0 JSON
- target/reports/compiler-performance: 121 files, 1.5 GB, untracked

**Links to repoint in the same change:**
- README.md:40-41
- docs/plans/GOAL_PROMPT.md:7, 79, 88-89, 102-103
- docs/plans/steps.md:5-6, 30, 46, 228, 498, 537
- TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:4-6, 20: its "mandatory verified compiler spine" is superseded by decision 3
- BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md:27-28, 79, 3365
- The BOON_COMPILER_ mentions in BOON_LANGUAGE_FOUNDATIONS, OUT_PARAMETERS, CIRCUIT_SIMPLIFICATION, PACKED_DATA and PERSISTENCE plans
- budgets/compiler.toml `owner_plan`

---

## 3. budgets/compiler.toml format 4 and budgets/compiler_edits.toml

Full proposals are in the scratchpad: `design/compiler.v4.toml` and `design/compiler_edits.toml`. Key decisions:

**Timing window and lanes**
- A cold sample is **one fresh process doing the product order**: units already in memory, then `t_diag` (diagnostics and language snapshot available), then `t_plan` (runnable plan). This halves the process count and matches the playground. Format 3 used one intent per process and measured a path the playground never takes (C5).
- The compiler never reads examples/manifest.toml to find its files. That lookup is 41% of counter's diagnostics time today.
- **product lane:** threads=auto, no persistent caches. **single_thread lane:** threads=1, taskset-pinned.
- CPU gate: `*_cpu_p95_ms` = 1.25 × the wall budget on the same runs. Parallelism may cut wall time but cannot hide duplicated work.

**Cold budgets**
- Owner budgets kept: TodoMVC 75/300 ms and NovyWave 250/1000 ms (diagnostics/verified).
- counter tightened from 10/50 to 5/10 ms.
- RSS becomes a delta over the pre-window high-water mark: 8 / 64 / 192 MiB. Today's absolute figures (40 / 154 / 314 MiB) are dominated by the 39 MB binary.
- `target_*` values (TodoMVC 25/60, NovyWave 75/150) are reported and ratcheted, not gated.

**Throughput (single_thread lane, minimum over TodoMVC and NovyWave)**

| Phase | Floor (lines/s) | Target (lines/s) | Today |
| --- | ---: | ---: | --- |
| front | 400k | 1M | ~190k |
| check | 100k | 300k | 9.7k / 26k |
| lower | 60k | 200k | |
| verified end to end | 30k | | 4.5-6.4k |

These floors are stricter than the cold wall budgets (NovyWave at 30k lines/s is 397 ms), so the cold gate cannot be met by adding cores alone.

**Small lane** (in-process, 50 compiles)
- Sources: counter, hello_world, minimal, and the two persons_pro child programs, read from the scenario steps (no copied fixture).
- Budgets: diagnostics p95 1 ms, verified p95 2 ms, max 4 ms. This leaves half of the native persons-pro 4/8 ms gate for runtime and IPC.

**Warm**
- Driver: the product compile service.
- Corpus: `budgets/compiler_edits.toml`, about 904 revisions over 16 required classes on TodoMVC, NovyWave, persons_pro (migration stage v3) and the persons_pro child program. Every step is typed one character per revision.
  - Classes: literal-text, literal-number, whitespace, comment, local-rename (multi-step, ends valid), add-field, remove-field, add-function, paste-block, undo-to-previous, type-error-and-fix, incomplete-syntax, cross-unit, cross-unit-type-change, migration-stage, child-program.
  - Every anchor was validated today as present and unique.
  - TodoMVC cross-unit edits wait for the owner-requested Theme refactor.
- **settled** mode gates:
  - submit → diagnostics: p95 16.7 ms, p99 25, max 33.4.
  - preview: p95 100 ms, max 200.
  - noop (whitespace/comment): 1 ms.
  - supersede-stop: p95 2 ms.
  - CPU per keystroke: p95 20 ms.
  - RSS growth: ≤ 8 MiB per session.
- **typing** mode (60 ms cadence) adds `typing_freshness_p95_ms = 25`: the time from keystroke k to the first diagnostics for any revision ≥ k. This directly gates today's "no diagnostics while typing" failure.
- The final revision of each edit is compiled cold and must match the warm result.
- The overall p95 gates; per-class numbers are reported only.
- `[warm.switch]` predicts counter-dev's example-switch measurements.

**Scaling**
- Engine-neutral generators, median single-thread CPU ms of 5 runs minus the size-0 baseline.
- Sizes 800/1600/3200, smaller for depth and unit count.
- Ratio ≤ 2.3 per doubling. n log n is about 2.21 at these sizes; format 3's 2.2 applied to work counters, and on time it would reject n log n.
- `static-branch-count` becomes `union-width`. New workloads: when-arm-count, record-width, pass-depth, list-pipeline-length.

**Oracles (no byte hashes)**
- `tests/compiler/diagnostics/*.bn` with `.expect` files, one `unit:line:col CODE` per line. Message text is not compared.
- Examples and scenarios through boon_behavior_harness, plus both migration sequences.
- Determinism: 2 runs, threads 1 vs auto, identical canonical plan identity and ordered diagnostics.
- Old-engine differential with `tests/compiler/divergences.toml` until cutover.
- Debug-build verifier on every produced plan.
- Canonical plan identity = a fast hash (xxh3-128) of the plan's compact encoding after test-only id canonicalization. It is computed only in oracle runs and never in the timing window.

**No build-profile pin.**
- Reports record profile, lto, codegen-units, panic, target-cpu, rustflags, rustc, allocator and git state.
- Proposed Cargo profiles:
  - `[profile.profiling] inherits = "release", debug = "line-tables-only"`, built in `target/profiling` with `-Cforce-frame-pointers=yes`.
  - Candidates `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, then PGO. Each is an A/B plus a checkpoint, with build time reported alongside (a one-crate kernel edit takes 143 s to rebuild today).

**Mapping from format 3:**
- `machine_plan_sha256` (:40, :50, :60): deleted (the TodoMVC and NovyWave values are stale).
- `[protocol]` fields (threads=1, caches disabled, profile_options, cold_modes, evidence lane): deleted.
- `owning_work_counter` (:98-140): replaced by the time-based metric.
- Single-literal `[warm]` edit: replaced by the corpus.

---

## 4. Measurement harness

**4.1 `boon_cli compile-bench`**
- Replaces compiler_sample.rs (2,015 lines) with about 600 lines.
- Modes: `cold` (one fixture per process, product order); `small --repeat N`; `warm --corpus … --fixture … --mode settled|typing` (drives the compile service); `scaling --generator id --size n` (the program is generated before the window); `oracle` (emits normalized diagnostics and the canonical plan identity, untimed).
- Flags: `--engine old|new` until cutover; `--threads 1|auto`.
- Output: one compact JSON line per sample (`serde_json::to_writer`) with {engine, lane, fixture, t_diag_ms, t_plan_ms, cpu_ms, rss_delta_kib, phases{front,check,lower,seal}, threads, counters?}.
- Phase marks sit in an always-on flat ring of (id, start, end). Engine counters are an optional free-form map, not a schema.
- Removed:
  - The per-process binary self-hash (compiler_sample.rs:908). build.rs embeds `BOON_BUILD_ID`, a content hash of Cargo.lock, the toolchain file and crate sources (not git HEAD, so docs commits stop relinking; build.rs:143-146). xtask hashes each binary once per session.
  - The pretty-JSON export (:1644-1648, :1863-1864).
  - The exact-mimalloc-version refusal. The version is recorded instead.
  - The `boon_cli_evidence` thread-local allocator lane. A thread-safe counting allocator becomes a `count-alloc` feature used nightly.

**4.2 `cargo xtask compiler-ab`** (about 450 lines)
- Takes `--a/--b <binary|git rev>`. A rev is built in `target/ab/<rev>` through a git worktree, cached between runs.
- Schedule: ABBA blocks of fresh processes, product lane, cells TodoMVC and NovyWave (optionally small or warm-quick).
- Per block, compute r = ln(B/A). Stop after at least 5 blocks when the 99% t-interval of the mean excludes 0 (an effect) or lies inside ±1% (no effect above 1%). At most 30 blocks. 99% compensates for optional stopping.
- Reports the effect %, the CI, a cpu_ms delta (flagged above 5%) and A/B output equality (informative).
- Cost at old-engine speeds is about 1-6 minutes; at target speeds, under a minute. Today's protocol costs 25-30 minutes.

**4.3 `cargo xtask compiler-checkpoint [--nightly]`** (about 900 lines)
- Lanes: cold (fixtures × 2 lanes × 3+20), small, warm settled and typing, switch, scaling, throughput, RSS, determinism, oracles.
- Prints one table and writes target/reports/compiler/checkpoint.json. Exits nonzero on any gate failure.
- Records governor, turbo and loadavg, and discards samples taken above loadavg 1.5. It never changes system settings.
- `--nightly` adds:
  - The old-engine differential over examples, the corpus, the scaling generators and all ~900 intermediate states of the warm corpus (a free corpus of invalid programs).
  - Allocation counts.
  - A/A noise calibration.
  - A debug-build verifier pass.
- The oracle runners themselves are cargo tests in the new compiler crate (target: whole corpus under 5 s), so they run on every change and do not count against the xtask line cap.

**4.4 `cargo xtask profile`**: the repo-owned sampler (about 800 lines; needs no sudo and no gdb)
- Builds the profiling binary unless `--binary` is given.
- Spawns the child behind a pre-exec pipe barrier and opens one event per online CPU with these attributes (the combination verified above):
  - pid=child, cpu=i, inherit=1, enable_on_exec=1
  - exclude_kernel=1, exclude_hv=1
  - SW task-clock, freq mode at 4 kHz
  - sample_type IP|TID|TIME|CALLCHAIN, mmap=1, comm=1, task=1
- Drains the ring buffers until the child exits, and maps addresses through the MMAP records.
- Symbolizes with the `object` crate and `rustc-demangle`, attributing unsized or asm addresses to the nearest preceding symbol as `[asm] name+off`. This fixes the blind spot that hid SHA-256 from the earlier summarizer.
- Outputs folded stacks (flamegraph/speedscope), top-40 self and inclusive tables, a per-thread split, and a **phase cross-check**: sample timestamps are bucketed by compile-bench phase marks, and sample share is compared with timer share, with a warning above 3 pp. The audit's gdb sampler matched timers within 1 pp, and this keeps that check.
- `--unwind dwarf` uses regs+stack user capture of 16 KiB at 1 kHz and framehop to profile the exact shipped binary without frame pointers.
- **Why not port the gdb scripts:** gdbprof.py gets about 175 samples per run at about 2.5x wall inflation (profile/log_a.txt), sees only the signalled thread through MI `-stack-list-frames`, and needs gdb installed. pmp3.py hard-codes `RET=0x555555554000+0x1ad0efa`, a return address specific to one binary and to gdb's ASLR-off base, so it breaks on every rebuild.
- samply stays an optional interactive tool if the owner sets paranoid=1.

---

## 5. Gates that pin the old pipeline: keep, replace or retire

"Migration start" means the first change that lands the new harness.

**Handoff gate 0 (`verify-architecture`, native_gpu_handoff_manifest.json order 0)**

| Check | Location | Action |
| --- | --- | --- |
| verified-semantic-compiler-spine | architecture.rs:62-65; 1294-1529 plus helpers 1531-1855 (dependency allowlist 1555-1593, opaque artifacts 1595-1730, direct fields 1732-1855); core-ownership boundary 1856-2334 (receipts.publish_ ==11 at :2015); exact 12-caller inventory 2336-2724 | **Keep until cutover.** Do not touch the inspected old files, and put the engine switch in a new module so the inventoried call sites do not change. **Retire at cutover**, replaced by a ~60-line `compiler-crate-graph` check: allowlisted dependency edges for the new crates; runtime and host reach the compiler only through the facade; compiler crates never depend on runtime or playground crates. |
| shared-document-plan-code | :67-69; 162-216; pins document_executable_backend.rs and PLAN_MAJOR_VERSION=11 | Keep until cutover, then retire. Replace with a checkpoint metric: document expressions / IR expressions ≤ 2 (64,482 / 4,536 today). |
| legacy-owner-solver-test-only | :72-74; 1194-1292 | Keep until cutover; retire with boon_typecheck. |
| normative-single-thread-compiler | :82-84; 218-279; env_remove counts in compiler_performance.rs (==1) and compiler_interactions.rs (==3) | **Retire at migration start**, together with those files. It encodes the frozen protocol. |
| construction-owned-checked-image-publication | :87-89; 282-652 | Keep until cutover, then retire (proof machinery). |
| dependency-classifier-schema-v1 | :92-94; dependency_classifier.rs; docs/architecture/phase1/dependency_classifier_schema_v1.toml (15,805 lines); digest boon_semantic lib.rs:51, used at :983 and :3321; xtask test dependency_classifier.rs:1644 | Keep until cutover (the old sources are frozen, so there is no drift). Retire at cutover with the TOML, the digest and the test. |
| canonical-checked-parameter-semantics | :97-99; read_dir of boon_ir, boon_semantic, boon_typecheck (dependency_classifier.rs:46-67) | Retire at cutover; it fails once those directories are deleted. |
| no-example-specific-engine-branches | :102-104; 2725-2822 | **Keep forever.** Extend the prefix list (2743-2767) to the new compiler crates on the day they are created, or invert it to all crates except cli, xtask and the playground's non-verifier roles. Without this, the anti-gaming guard does not cover the new engine. |
| xtask-rust-loc-cap | :137-142; red today at 25,023 | Keep. Fix by deleting about 7k obsolete harness lines at migration start, never by raising the cap or moving the harness into another crate. |
| no-product-serde-json, single-machine-plan-executor-path, toolchain, native input, app_window, playground and runtime caps | | Keep. They are not compiler-shaped, and the serde_json rule covers new crates automatically. |
| Synthetic architecture tests | architecture.rs:2945-3086 | Delete the tests of retired checks with those checks. |

**Standalone xtask commands (not in the manifest)**

| Command / file | What it pins | Action |
| --- | --- | --- |
| verify-compiler-performance (compiler_performance.rs 2,732) | Plan-SHA pass at :1534-1540 and :1879; RAYON=1 at :973 | Retire at migration start, after one final old-engine reference run is saved. |
| verify-compiler-interactions (2,612) | Hard-coded fail :623-629; SCC trace parser :1818-1826 reading dependency_manifest.rs:5594; counter names :1987-1993 | Retire at migration start; replaced by the warm lane. |
| verify-compiler-allocator (919); compiler_work_sample.rs (355: WorkSample deny_unknown_fields :300-311, has_complete_frontend_work :314-345); compiler_producer.rs (330) | Old sample schema and producer identity | Retire at migration start. |
| boon_cli compiler_sample.rs, allocator.rs, bin/boon_cli_evidence.rs | Old producer | Replace with compile-bench. |
| verify-phase0 | versions.toml:18-37, 163-201 probes of boon_semantic, boon_verify, ErasedProgram, ParsedProgram; version axes :306-319; baselines.toml:14-31; deletion_ledger compiler roots | Keep until cutover, then delete the compiler probes and roots. Retiring the whole command is an owner question. |
| packed-site-inventory | Roots packed_site_inventory.rs:29-53 (boon_compiler, boon_ir, boon_typecheck, semantic core_lowering and program_core); ledger phase0/packed_site_occurrences.tsv; cargo test :2061 | Keep until cutover, then drop the compiler roots and regenerate the ledger. Do not add the new crates. |
| verify-packed-baseline plus boon_phase0_baseline | fixtures.rs:9-50 uses `boon_compiler::compile_machine_plan` | Keep. It keeps working if the facade keeps its API. |
| verify-language-surface | examples/language_feature_coverage.toml, registry owned by boon_syntax (language_surface.rs:7-11) | Keep as a behavioural parser gate. Move the registry if the front end replaces boon_syntax. |

**Cargo tests**
- boon_compiler_kernel/src/lib.rs:50-74 (bans 7 crate dependencies): retire with the crate. The crate-graph rule moves into gate 0, which verify-all actually runs.
- The roughly 940 `#[test]`s in the old crates (typecheck 360, kernel 198, semantic 192, compiler 130, parser 82, plan 59, verify 33, checked 20, ir 11): before deletion, harvest their Boon programs into `tests/compiler/diagnostics` as positive cases or negative cases with a code and position.

**Budgets and docs**
- budgets/compiler.toml format 3: replace now with format 4. The old engine is measured as reference.
- BOON_CONSOLE.md:196-206, 218-230 (stage chain) and 501 (lineage): rewrite at cutover to Boon source → typed program (this contract) → MachinePlan → CoreHardwareIR…, with lineage source-bundle digest → MachinePlan digest at the deploy boundary. This needs the owner, because it is the console contract.
- LANGUAGE_SEMANTICS.md:51-52, 226-237 and PASS_PASSED_AND_TODOMVC_UI_MODEL.md:17-18: update stage names at cutover.
- NATIVE_GPU_PIPELINE.md:25-38 (ownership table naming boon_parser, boon_typecheck, boon_ir): update at cutover.
- NATIVE_GPU_PIPELINE.md:383-395 ("public xtask commands are limited to…"): add one line now saying the compiler measurement commands are owned by BOON_COMPILER.md and are not handoff gates.
- examples/*.budget.toml: keep. They are product gates the new engine must meet.

**xtask line budget**

| Step | Lines |
| --- | ---: |
| Today | 25,023 |
| Migration start: delete compiler_performance, interactions, allocator, work_sample, producer and the single-thread check | −7,018 → about 18,000 |
| Add compiler-ab, compiler-checkpoint and profile | +2,150 → about 20,200 |
| Cutover: delete dependency_classifier and about 1,900 architecture-check lines; add the crate-graph check | about 17,000 |

---

## 6. Native GPU handoff gates during and after migration

- **During migration the old engine is the playground default** and must keep all 7 manifest gates satisfiable:
  - Gate 0 needs three things: the LOC-cap fix at migration start; the example-branch guard extended to the new crates; no edits to files the old structural checks inspect.
  - Example refactors required by the new semantics (such as splitting the Theme dispatchers) must also compile on the old engine. A refactor the old engine cannot compile lands in the cutover change.
- **The new engine gets an engine selector in the compile request** (for example a `BOON_COMPILER_ENGINE=new` environment variable passed to the preview process). It is never an example name, and the preview still receives only source.
  - Shadow runs of counter-dev, todomvc-physical, cells, novywave, persons-pro and negative write to `target/reports/report-v2-new/` through the gates' `--report` override. They are informative and are not handoff evidence.
  - The compiler-level lanes predict the compile-bound native metrics, so agents do not need native runs in the inner loop: `[small]` predicts persons-pro bounded-starter-source-compile 4/8 ms and valid-edit-to-preview 16.7 ms; `[warm.switch]` predicts counter-dev example-switch ack/present.
- **At cutover:** one atomic, owner-approved change does four things: switches the default, deletes the old engine and flag, updates gate 0 (retire and replace per §5), and updates the docs. verify-all --check-existing binds reports to exact source identity (report_v2.rs:3334-3364), so a partial change leaves the aggregate red. Then run all 7 gates and `cargo xtask verify-all --check-existing --report target/reports/report-v2/verify-all.json`, and restart the release playground per the AGENTS.md launch rules. Human testing remains a separate follow-up.
- **After cutover:** the manifest gate list stays at 7 gates. `compiler-checkpoint` is not added to verify-all: it is too slow and too sensitive to machine noise, and compile latency that matters to the product is already gated through persons-pro and counter-dev.

---

## 7. Cutover checklist

1. The diagnostics corpus matches 100% of its `.expect` files on the new engine. The corpus contains the harvested old tests plus one repro per strictness rule in decisions.md §6.
2. Every example in examples/manifest.toml compiles clean, or yields its expected errors for negative fixtures, and passes its `.scn` scenario. Both migration sequences pass. Distributed packages (fjordpulse client/server/session) and the persons_pro child-program path compile and run.
3. `divergences.toml` contains only entries whose reason is a decisions.md item or an old-engine bug, each with a corpus repro. It contains zero new-engine-bug entries.
4. Warm==cold holds on all ~904 corpus revisions. Determinism holds (threads 1 vs auto). The debug-build verifier passes on all examples.
5. `compiler-checkpoint` passes on the reference machine, including warm settled and typing, small, throughput, scaling, RSS, and TodoMVC/NovyWave cold. Any miss needs the owner's written sign-off per metric.
6. Shadow runs of all 6 native product gates pass with the new engine.
7. The facade API (`boon_compiler::compile_machine_plan` and related functions) serves all 9 dependents, or they are ported: distributed_compiler.rs (legacy boon_typecheck at :1011, :1261, :1444-1449), the program_core child-program path, boon_editor language projection, the playground compile worker (moved onto the shared compile service), boon_cli check/dump-plan/dump-ir, and boon_phase0_baseline.
8. A MachinePlan encoding or `plan_digest` change is a declared artifact-format bump for program artifacts and app packages. plan_digest is stored at program_core.rs:355, checked at :453, and exposed at persistent.rs:3984. Old artifacts are rejected with a clear error.
9. The atomic change:
   - Delete boon_typecheck, boon_compiler_kernel, boon_checked, boon_semantic, boon_verify, boon_ir, boon_compilation_db, the old boon_compiler internals (kernel_oracle.rs, machine_plan_backend.rs and so on) and the 4 empty crate dirs.
   - Remove the gate-0 checks and add compiler-crate-graph.
   - Delete dependency_classifier.rs, the phase1 TOML and the digest.
   - Drop the compiler roots from packed-site and phase0, regenerate the ledger, and remove the differential oracle from budgets.
   - Update the docs listed in §5 and the AGENTS.md "old engine" sentences.
10. Run the 7 gates and verify-all --check-existing, run a checkpoint, and restart the release playground.

## 8. Code sketch: sampler event setup (the load-bearing detail)

```rust
// one per online CPU; cpu=-1 with inherit cannot be mmapped (EINVAL, verified)
let attr = perf_event_attr {
    type_: PERF_TYPE_SOFTWARE, config: PERF_COUNT_SW_TASK_CLOCK,
    sample_freq: 4000, flags: DISABLED | INHERIT | EXCLUDE_KERNEL | EXCLUDE_HV
        | MMAP | COMM | TASK | FREQ | ENABLE_ON_EXEC | SAMPLE_ID_ALL,
    sample_type: IP | TID | TIME | CALLCHAIN, ..zeroed() };
for cpu in online_cpus() { fds.push(perf_event_open(&attr, child_pid, cpu, -1, 0)?); mmap_ring(fd, 1 + 64)?; }
release_barrier(child); drain_until_exit(&fds); // MMAP2 records -> (file, offset) -> symbol
```

## owner_questions
- **question**: Profiling: should the repo ship its own perf_event_open sampler (cargo xtask profile), or will you set kernel.perf_event_paranoid=1 persistently so samply works? | **options**: (a) in-repo sampler, no system change; (b) paranoid=1 via sysctl.d and samply; (c) both | **recommendation**: (a). User-only perf_event_open sampling with callchains works at paranoid=2 on this machine (verified this round), so no security setting has to change. samply stays optional for interactive browsing if you want it. | **why**: Profile-first attribution is mandatory. Agents must not change security settings, and samply 0.13.1 refuses to start at paranoid=2.
- **question**: Where should the nightly full run happen? The repo has no CI. | **options**: (a) a local systemd user timer running `cargo xtask compiler-checkpoint --nightly` at night; (b) checkpoints only at milestones; (c) a CI runner | **recommendation**: (a) if you are willing to install the timer. Otherwise (b), with a checkpoint required before each milestone closes. | **why**: Light development A/B is only safe if a slower full run catches small drifts.
- **question**: Should the 12 BOON_COMPILER_* plans and 44 evidence JSON files be deleted, or moved to docs/archive? | **options**: delete (git keeps history; the contract names the last commit) / docs/archive/compiler-2026 with a header | **recommendation**: Delete. Archived copies still show up in agents' greps as competing instructions. | **why**: Four of these documents currently claim active authority.
- **question**: At cutover, may BOON_CONSOLE.md's compiler stage chain (lines 196-206, 218-230) and digest lineage (line 501) collapse to Boon source → typed program → MachinePlan, with a MachinePlan digest computed at the deploy boundary? | **recommendation**: Yes. Decision 3 removes the intermediate artifacts, and hardware deploy is a trust boundary where the plan digest stays. | **why**: AGENTS.md names BOON_CONSOLE.md as the console contract, so its text should not change without you.
- **question**: The scaling gate moves from work counters with a 2.2 ratio to single-thread CPU time with a 2.3 ratio per doubling. Do you accept that this is not a loosening? | **recommendation**: Accept. n log n is about 2.21 at these sizes, so a time metric capped at 2.2 would reject correct n log n code, and the old counters are specific to the old engine. | **why**: The policy says raising a scaling ratio needs your approval.
- **question**: What is the bar for cutover? | **options**: (a) every budget passes; (b) all oracles, all 7 handoff gates, the warm budgets and the TodoMVC/NovyWave cold budgets pass, and any other miss needs your sign-off per metric | **recommendation**: (b). Stretch targets are not required for cutover. | **why**: Keeping two engines costs build time and every gate change. The cutover bar should be explicit.
- **question**: Should compiler-checkpoint become an eighth gate in the native GPU handoff manifest? | **recommendation**: No. It takes about 15 minutes and is sensitive to machine noise. The compile latency the product feels is already gated by persons-pro (starter-source compile 4/8 ms, valid edit to preview 16.7 ms) and counter-dev (example switch). | **why**: The manifest is the single source of truth for handoff, and every addition slows verify-all.
- **question**: At cutover, should verify-phase0 and packed-site-inventory (about 6,900 xtask lines of phase-0 migration tooling) be retired entirely, or only lose their compiler probes and scan roots? | **recommendation**: At minimum remove the compiler probes and roots. Retiring them entirely is your call, because they also cover runtime crates. | **why**: Both reference old compiler crates that will be deleted.
- **question**: During migration, must example refactors driven by the new semantics (such as splitting the TodoMVC and NovyWave Theme dispatchers) still compile on the old engine? | **recommendation**: Yes, so the handoff gates stay green on the default engine. A refactor the old engine cannot compile lands in the cutover change. | **why**: Otherwise the product gates go red before the new engine is the default.
- **question**: PGO experiments need `rustup component add llvm-tools`. May agents install it when the first PGO A/B runs? | **recommendation**: Yes. It is a toolchain component, not a system setting. | **why**: Build-profile changes are now ordinary experiments, and PGO is the largest of them.

## risks
- **risk**: Optional stopping in the sequential A/B inflates false positives, so small regressions slip through. | **mitigation**: Stop on a 99% CI with at least 5 blocks, check CPU time as a second signal, and let the checkpoint/nightly full matrix with A/A calibration confirm effects before a milestone closes.
- **risk**: Time-based scaling gates are noisier than work counters. | **mitigation**: Use the median of 5 single-thread CPU-time runs, sizes large enough to take at least 5 ms, and a size-0 baseline subtraction. A failure is rerun once before it is reported.
- **risk**: The differential oracle drowns in expected divergences, because the new engine is stricter by design, and a real new-engine bug gets filed as a decisions.md divergence. | **mitigation**: Every divergence entry must name the decisions.md item or old-engine bug and have a minimal repro in the diagnostics corpus. Zero new-engine-bug entries is a cutover condition.
- **risk**: Edit-corpus anchors break when examples are refactored (the Theme refactor especially). | **mitigation**: Anchors are validated before the corpus runs, with a cheap fail-fast check, and the policy requires updating them in the same commit. TodoMVC cross-unit edits are deliberately deferred until after the Theme refactor.
- **risk**: The xtask line cap blocks the new harness, or tempts someone to raise it or move the harness into another crate. | **mitigation**: Delete the roughly 7,000 obsolete compiler-performance lines at migration start, before adding anything. The contract states that moving gate code out of xtask to dodge the cap is gaming.
- **risk**: verify-all --check-existing binds reports to exact source identity, so a partial gate retirement leaves the handoff aggregate red. | **mitigation**: Retire the old-pipeline checks only in the single atomic cutover change, with the owner's approval. Checks that pin files the old engine never touches stay untouched until then.
- **risk**: The frame-pointer profiling build differs slightly from the shipped release binary, and mimalloc's C code may break frame-pointer chains near allocations. | **mitigation**: Leaf attribution stays correct, and the phase cross-check flags gross bias. The --unwind dwarf mode profiles the exact release binary when needed.
- **risk**: Noise on the reference desktop (powersave governor, turbo, COSMIC load) produces flaky checkpoint results. | **mitigation**: Record the governor, turbo and loadavg without changing them, discard samples taken above loadavg 1.5, interleave ABBA, pin the single-thread lane with taskset, and gate on p95 over 20 scored samples.
- **risk**: Two engines in one binary lengthen release builds during migration (a one-crate kernel edit takes 143 s today). | **mitigation**: The new crates depend on nothing from the old pipeline. boon_cli carries the old engine behind a default feature, so the inner development loop can build without it. compiler-ab caches builds per revision.
- **risk**: Changing the MachinePlan encoding breaks stored program artifacts and app packages that carry plan_digest. | **mitigation**: Declare it as an artifact-format bump in the cutover change and reject old artifacts with a clear error. Nobody uses the compiler in production (decision 2).
- **risk**: The warm driver measures a service the playground does not actually use, repeating today's mistake. | **mitigation**: The contract requires the playground's compile worker to be a thin client of the same compile-service object that the bench drives. The native persons-pro and counter-dev gates cross-check the result end to end.

## work_items
- **id**: P1 | **title**: Replace the AGENTS.md compiler section, write BOON_COMPILER.md, archive the plans | **description**: Apply the AGENTS.md text in design section 1. Write the contract of at most 300 lines (section 2 outline, including dated lessons L1-L9). Delete the 12 BOON_COMPILER_* docs, the 44 evidence JSON files, the untracked PROFILE_DRIVEN plan and its evidence. Repoint the links listed in design section 2, and add the NATIVE_GPU_PIPELINE.md:383-395 note that compiler measurement commands are not handoff gates. | **size**: S | **acceptance**: `grep -r BOON_COMPILER_ docs README.md AGENTS.md` finds only the contract's history pointer. The contract is at most 300 lines. The owner has approved the text. | **depends_on**: owner answers on archive vs delete
- **id**: P2 | **title**: Land budgets format 4 and the edit corpus | **description**: Install design/compiler.v4.toml as budgets/compiler.toml and design/compiler_edits.toml as budgets/compiler_edits.toml. Add a cheap anchor-validation check to the corpus loader. | **size**: S | **acceptance**: Both files parse. Every anchor is found exactly once in the current examples. No machine_plan_sha256 and no build-flag pin remain. | **depends_on**: P1
- **id**: P3 | **title**: Retire the obsolete compiler-performance harness and fix the xtask line cap | **description**: Record one final old-engine reference run with the old tools and keep it under target/reports as reference. Then delete compiler_performance.rs, compiler_interactions.rs, compiler_allocator.rs, compiler_work_sample.rs, compiler_producer.rs and their main.rs commands, along with the normative-single-thread-compiler check (architecture.rs:82-84, 218-279). | **size**: S | **acceptance**: xtask production lines are at most 18,100 by the architecture.rs counter, and verify-architecture passes its line-cap check with the other checks unchanged. | **depends_on**: P1
- **id**: P4 | **title**: compile-bench producer in boon_cli | **description**: Replace compiler_sample.rs with compile-bench, providing cold, small, warm, scaling and oracle modes, `--engine old|new` and `--threads`. Output is compact JSON lines with t_diag and t_plan, cpu_ms, rss_delta and engine-neutral phases. build.rs embeds a content-hash BOON_BUILD_ID. Remove the per-process self-hash, the pretty-JSON export, the exact-mimalloc refusal and boon_cli_evidence. Allocation counting becomes the thread-safe `count-alloc` feature. | **size**: M | **acceptance**: The harness adds at most 5 ms outside the window per process (about 110-220 ms today). The old engine runs through it and reproduces format-3 phase timings within 3%. There are no git HEAD reruns on docs-only commits. | **depends_on**: P2
- **id**: P5 | **title**: cargo xtask compiler-ab | **description**: Sequential interleaved ABBA A/B between binaries or git revisions (built in a cached worktree target). Stops when the 99% CI of the mean log ratio excludes 0 or lies inside ±1%, with 5 to 30 blocks. Reports the CPU delta and A/B output equality. | **size**: M | **acceptance**: An A/A run reports no effect within 30 blocks. A deliberate 5% slowdown (a sleep build) is detected within 10 blocks. Wall time is at most 6 minutes on the old engine. | **depends_on**: P4
- **id**: P6 | **title**: cargo xtask profile (perf_event_open sampler) | **description**: Per-CPU inherited user-only task-clock sampling at 4 kHz with callchains, a pre-exec barrier and enable_on_exec. Uses a frame-pointer `profiling` Cargo profile and symbolizes with object and rustc-demangle, attributing to the nearest preceding symbol for unsized asm. Outputs folded stacks, self and inclusive top tables and a per-thread split, plus the phase-share cross-check against compile-bench marks. `--unwind dwarf` is optional. | **size**: M | **acceptance**: At paranoid=2 it collects at least 1,500 samples per TodoMVC verified run of the old engine. Phase shares match the timers within 3 pp. sha256 compress frames appear, the case the earlier summarizer missed. No gdb and no sudo are needed. | **depends_on**: P4
- **id**: P7 | **title**: Diagnostics corpus, codes and divergence tooling | **description**: Define the `.expect` format (unit:line:col CODE). Harvest Boon programs from the roughly 940 old compiler tests into tests/compiler/diagnostics, add one repro per decisions.md strictness rule, and write a cargo-test corpus runner. Add divergences.toml with reason classes and a differential runner (old vs new) over the examples, the corpus, the scaling generators and the warm-corpus intermediate states. | **size**: L | **acceptance**: The corpus runs in under 5 s as a cargo test. Every harvested case records the old engine's outcome. The differential lists and classifies every mismatch. | **depends_on**: P4, the new front end/checker emitting codes
- **id**: P8 | **title**: Warm corpus driver on the product compile service | **description**: Drive the shared compile service, the same object the playground's worker wraps, in settled and typing modes. Measure submit-to-diagnostics, preview, freshness, supersede-stop, CPU per keystroke and RSS growth, and run the warm-vs-cold consistency check. Include persons_pro migration stage v3 and the child program read from persons_pro.scn. | **size**: M | **acceptance**: Runs the full corpus of about 904 revisions on both engines (old engine nightly only). Reports overall and per-class p95. from_scratch_consistency is enforced. | **depends_on**: P4, the new compile service
- **id**: P9 | **title**: cargo xtask compiler-checkpoint [--nightly] | **description**: Orchestrate the cold (2 lanes), small, warm, switch, scaling, throughput, RSS, determinism and oracle lanes. Record machine state without changing it, discard samples taken under high load, print one table and write JSON. Nightly adds the differential, allocation counts, A/A calibration and the debug verifier. | **size**: M | **acceptance**: The run is at most 15 minutes at target speeds. It exits nonzero on any failed gate. Its report is at most 256 KiB with no sidecars. | **depends_on**: P5, P7, P8, P10
- **id**: P10 | **title**: Engine-neutral scaling generators | **description**: Generators for the 10 workloads in format 4 (call-depth, call-site-count, contextual-call-site-count, union-width, when-arm-count, record-width, pass-depth, list-pipeline-length, source-unit-count, dependency-cone-size), producing valid Boon under the new strict rules. | **size**: S | **acceptance**: All generated programs compile clean on the new engine. The old engine reproduces its known superlinear cases (union-width about n^2.5, call-depth about n^1.9) as failures. | **depends_on**: P4
- **id**: P11 | **title**: Architecture gate: cover the new crates | **description**: Extend the no-example-specific-engine-branches prefixes, or invert them to cover all crates, so the new compiler crates are scanned. Add a compiler-crate-graph check at cutover. | **size**: S | **acceptance**: A seeded `== "todo_mvc_physical"` in a new compiler crate fails verify-architecture. | **depends_on**: creation of the new crates
- **id**: P12 | **title**: Engine selector and shadow native gate runs | **description**: The preview compile request carries the engine choice, taken from an environment variable passed through desktop, never from an example name. Document shadow runs of the 6 product gates into target/reports/report-v2-new through `--report`. | **size**: S | **acceptance**: All 6 product gates run with the new engine without touching the handoff reports. The default engine is unchanged. | **depends_on**: new engine produces runnable plans
- **id**: P13 | **title**: Atomic cutover | **description**: Execute checklist items 1-10 in design section 7: delete the old engine, apply gate-0 retirements and replacements, drop the classifier, remove the compiler probes from phase0 and packed-site, update the docs and AGENTS.md, bump the artifact format, and run the handoff. | **size**: L | **acceptance**: All 7 manifest gates pass, and `cargo xtask verify-all --check-existing` passes on the cutover tree. The checkpoint passes or has owner sign-offs. The release playground is restarted. | **depends_on**: P3-P12, owner go-ahead
- **id**: P14 | **title**: Nightly runner | **description**: If the owner chooses it, provide a systemd user timer unit that runs `cargo xtask compiler-checkpoint --nightly` and keeps a trend file under target/reports/compiler/nightly/. | **size**: S | **acceptance**: The owner installs it. One week of nightly reports shows an A/A noise floor under 1.5%. | **depends_on**: P9, owner answer

## deletions
- **what**: The 12 BOON_COMPILER_* plan documents, including the untracked PROFILE_DRIVEN plan | **size**: 13,143 lines
- **what**: docs/plans/evidence/compiler-*.json and the untracked t0 evidence | **size**: 45 files, about 652 KB
- **what**: target/reports/compiler-performance (untracked; mostly preserved producer binaries) | **size**: 121 files, 1.5 GB
- **what**: xtask compiler_performance.rs, compiler_interactions.rs, compiler_allocator.rs, compiler_work_sample.rs, compiler_producer.rs and the normative-single-thread-compiler check (at migration start) | **size**: about 7,018 production lines
- **what**: boon_cli compiler_sample.rs, allocator.rs, bin/boon_cli_evidence.rs and the git-HEAD provenance part of build.rs (replaced by compile-bench, about 600 lines) | **size**: about 2,450 lines
- **what**: budgets/compiler.toml format-3 content: machine_plan_sha256 values, the frozen [protocol], owning_work_counter scaling, the single-literal warm edit | **size**: whole file replaced
- **what**: At cutover: architecture.rs compiler checks (spine, core-ownership, boundary-caller inventory, checked-image publication, legacy-owner, shared-document pin, canonical-parameter) and their helpers and tests | **size**: about 1,900 lines
- **what**: At cutover: dependency_classifier.rs, docs/architecture/phase1/dependency_classifier_schema_v1.toml and DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1 with its runtime uses | **size**: 3,177 plus 15,805 lines
- **what**: At cutover: compiler probes and roots in verify-phase0 (versions.toml 18-37, 163-201, 306-319; baselines and deletion_ledger compiler roots) and in packed-site-inventory (roots :29-53, regenerated ledger) | **size**: hundreds of registry lines
- **what**: At cutover: boon_compiler_kernel/src/lib.rs:50-74 architecture test (goes with the crate) and the empty crate dirs boon_bridge, boon_driver, boon_ply_playground, boon_transport_json | **size**: 25 lines plus 4 empty dirs

## evidence
- AGENTS.md:30-44: the current compiler paragraph (3+30 A/B/A, never edit budgets, K1/summary/linked-code bans, profile-driven plan pointer)
- budgets/compiler.toml:5-28: frozen protocol (lto=false, cgu=16, compiler_threads=1, caches disabled, two cold modes); :40, :50, :60 machine_plan_sha256; :98-140 owning_work_counter scaling
- crates/xtask/src/architecture.rs:9 XTASK_RUST_CAP=25_000; recounted with the rust_file_line_counts logic: 25,023 production lines (the 2026-08-23 architecture.json recorded 24,999); 840c1c5c (2026-09-10) added 47 lines to compiler_work_sample.rs
- crates/xtask/src/architecture.rs:13-160 check list; 162-216, 218-279, 282-652, 1194-1292, 1294-2724, 2725-2822 (example-branch prefixes at 2743-2767)
- crates/xtask/src/compiler_performance.rs:1534-1540 plan-SHA pass; compiler_interactions.rs:623-629 hard-coded fail, :1818-1826 SCC trace parser; RAYON_NUM_THREADS=1 at compiler_interactions.rs:682, 1237, 1271 and compiler_performance.rs:973
- crates/boon_cli/src/compiler_sample.rs:907-908 per-process SHA-256 of the 39 MB binary; :1644-1648 and :1863-1864 pretty-JSON plan hashing; crates/boon_cli/build.rs:143-146 provenance refresh on docs-only commits
- Probe this round: perf_event_open with exclude_kernel for SW task-clock and HW cycles (count and IP+CALLCHAIN sampling) succeeds at paranoid=2; with kernel included it is EPERM; cpu=-1+inherit mmap is EINVAL; cpu=0+inherit mmap works; perf_event_max_sample_rate=15000; samply 0.13.1 refuses to start at paranoid=2
- scratchpad profile/gdbprof.py: about 175 samples per run and 1.0 s wall per TodoMVC diagnostics run (profile/log_a.txt), MI -stack-list-frames on the current thread only; crosscut/pmp3.py hard-codes RET=0x555555554000+0x1ad0efa
- examples/persons_pro.budget.toml:1-6 and crates/boon_native_playground/src/verify.rs:4868-4920 (bounded-starter-source-compile p95 4 ms / max 8 ms, valid-edit-to-preview p95 16.7 ms); examples/persons_pro.scn:60-85 child-program steps
- docs/architecture/native_gpu_handoff_manifest.json: 7 gates, architecture at order 0; target/reports/report-v2 holds only architecture.json
- crates/boon_compiler/src/lib.rs:189-196 CompilerDiagnostic carries no code; boon_compiler has 9 dependent crates (grep of crates/*/Cargo.toml)
- verify_all.md C6 (gate inventory, classifier, phase0, packed-site, BOON_CONSOLE.md:196-206/218-230/501), C5 (playground path, warm benchmark mismatch, persons_pro recompiling 3 stages), C4 (plan_digest consumers at program_core.rs:355/453, persistent.rs:3984)
- docs/architecture/NATIVE_GPU_PIPELINE.md:25-38 ownership table, :383-395 public xtask command list
- All anchors in design/compiler_edits.toml checked today: each found exactly once; about 904 revisions including restore and undo

## perf_targets
- **metric**: Cold TodoMVC diagnostics / verified, product lane p95 | **target**: ≤ 75 / 300 ms gated; targets 25 / 60 ms | **basis**: Owner budgets kept from format 3; today about 390 / 790 ms
- **metric**: Cold NovyWave diagnostics / verified, product lane p95 | **target**: ≤ 250 / 1000 ms gated; targets 75 / 150 ms | **basis**: Owner budgets kept; today about 525 / 1870 ms
- **metric**: Cold counter diagnostics / verified | **target**: ≤ 5 / 10 ms (tightened from 10 / 50) | **basis**: The small-program path predicts the native persons-pro starter-compile gate
- **metric**: CPU time per cold compile (all threads) | **target**: ≤ 1.25 × the wall budget | **basis**: Threads allowed in the product lane without hiding duplicated work
- **metric**: Single-thread throughput front / check / lower / verified end to end | **target**: ≥ 400k / 100k / 60k / 30k lines/s | **basis**: Today about 190k / 9.7-26k / – / 4.5-6.4k; references: oxc 6.5M, Sorbet ~100k per core, Elm ~149k
- **metric**: Small-program compile (in-process) | **target**: diagnostics p95 1 ms, verified p95 2 ms, max 4 ms | **basis**: Half of the native persons-pro budget (4 ms p95 / 8 ms max end to end)
- **metric**: Warm submit to diagnostics, settled, all corpus keystrokes | **target**: p95 16.7 ms, p99 25, max 33.4; target 5 ms | **basis**: Owner warm budget; today 372-397 ms
- **metric**: Warm submit to preview plan | **target**: p95 100 ms, max 200; target 40 ms | **basis**: Owner budget; today about 1,150-1,200 ms
- **metric**: Typing freshness (keystroke to diagnostics for any newer revision) | **target**: p95 25 ms | **basis**: Today no diagnostics are shown under continuous typing (C5)
- **metric**: Supersede stop latency | **target**: p95 2 ms | **basis**: Today a new edit waits for the in-flight 450 ms solve
- **metric**: RSS delta per compile | **target**: counter ≤ 8, TodoMVC ≤ 64, NovyWave ≤ 192 MiB; warm growth ≤ 8 MiB per session | **basis**: Absolute RSS today is dominated by the 39 MB binary (40 / 154 / 314 MiB)
- **metric**: Harness overhead outside the timing window | **target**: ≤ 5 ms per sample process | **basis**: Today about 110 ms self-hash plus 70-106 ms pretty-JSON export
- **metric**: Development A/B cost per candidate | **target**: ≤ 6 min on the old engine, ≤ 1 min at target speeds | **basis**: Today's 3+30 × 2 lanes × 2 modes protocol takes about 25-30 min
- **metric**: Checkpoint wall time | **target**: ≤ 15 min at target speeds | **basis**: Sum of the cold, warm, scaling and oracle lanes at target per-sample costs
- **metric**: Profiler sample yield | **target**: ≥ 1,500 samples per TodoMVC verified run | **basis**: gdbprof got about 175 per run; perf_event_open at 4 kHz over about 0.4-0.8 s of CPU

---

# Adversarial review

## area
Process, measurement and gates (AGENTS.md compiler section, BOON_COMPILER.md contract, budgets v4, edit corpus, compile-bench / compiler-ab / checkpoint / profile, gate inventory, migration and cutover)

## verdict
needs_changes

## major_issues
- **issue**: The "product lane" does not measure the product's allocator. compile-bench runs inside boon_cli, which uses a static mimalloc global allocator. The playground preview process, which is what the user feels and what the native gates time, has no #[global_allocator] and so uses glibc malloc. | **evidence**: crates/boon_cli/src/main.rs:1-11 sets MimallocAllocator as #[global_allocator]. grep finds no global_allocator in crates/boon_native_playground. a_engineering.md:154 measured the system-allocator build (boon_cli_system) 17% slower on NovyWave verified (2184 vs 1865 ms). The v4 [lanes.product] comment says "what the user feels" but only records the allocator. | **fix**: Make the allocator part of the product definition: either switch the playground to the same allocator as the bench, or run the product lane with the playground's allocator (the boon_cli_system variant). Record the allocator in the native gate reports as well. Add an owner question, because this decides which number the budgets gate.
- **issue**: The RSS redesign rests on a false premise. The design says today's absolute RSS (40/154/314 MiB) is "dominated by the 39 MB binary". Binary text pages are loaded on demand, and the per-process self-hash runs after the cold peak is read. Also, a ru_maxrss delta hides growth that stays under an earlier high-water mark. | **evidence**: Measured this round: `boon_cli --help` peaks at 5,804 KiB, while `boon_cli check examples/counter.bn` peaks at 40,140 KiB and compiler-sample counter verified reports peak_rss_kib 40,276. So about 34 MiB is real compiler memory. compiler_sample.rs:846 calls producer_metadata (whose fs::read of the binary is at :908) only after the samples loop at :826-840 has read peak_rss. | **fix**: Keep absolute peak RSS as a lean producer measures it: about 6 MiB of baseline, no delta. If a window-scoped peak is wanted, write 5 to /proc/self/clear_refs before the window and read VmHWM after it. Keep the tightened limits. Remove the "binary dominates" rationale and lesson from the contract.
- **issue**: The engine selector does not reach every compile path, does not survive the verifier's launch path, and is not recorded anywhere. A shadow run could silently mix engines, and a stray environment variable could produce handoff reports from the new engine. | **evidence**: The design scopes the selector to "the preview compile request". C5 (verify_all.md:66) lists separate compile triggers: child-program compile on 'boon-program-compile' (program_core.rs:482-540), migration Preview/Activate (preview.rs:3332, 3356), migration stages (compile.rs:757-811) and distributed packages. The native verifier launches through `cosmic-background-launch ... -- env NAME=VALUE` with an explicit list (verify.rs:6702-6714), so the shell environment is not inherited. The desktop then passes its own environment to its children (desktop.rs:645-660). | **fix**: Make it one process-global engine choice, read once in the boon_compiler facade, so every entry point sees it. Have the verifier add it explicitly to the launch environment list. Write the engine into each report's producer identity, and have validate_child_report reject handoff reports built on a non-default engine. Keep `--engine` out of the product CLI commands and put it on compile-bench only. The repo has already quarantined one dual-engine CLI surface (commits 16350741, 96d46b51).
- **issue**: The design ignores a product requirement: wasm32 and musl builds. The compiler is linked into the browser web host and the FjordPulse deploy. Two design elements break there: threads=auto, and phase marks that always read Instant. | **evidence**: crates/boon_runtime/Cargo.toml:9 has boon_compiler as a normal dependency, and boon_runtime/src/lib.rs:1-5 calls compile_sealed_machine_plan. boon_web_host is a cdylib (Cargo.toml:9) that depends on boon_runtime (:19). deploy/fjordpulse/Dockerfile:17 builds boon_web_host for wasm32-unknown-unknown, and :16 builds the server for musl. The old compiler already switches to web_time::Instant under cfg(wasm32) (boon_compiler/src/lib.rs:20-21, session.rs:32-33). The web_host tests compile programs (tests/startup.rs:42, :160). | **fix**: Add `cargo check --target wasm32-unknown-unknown -p boon_web_host` and the musl server build to the checkpoint and to cutover checklist item 2. In the contract, require the new crates to build on wasm32 with a single-thread fallback and a clock abstraction for phase marks. Ask the owner whether in-browser compile latency is a product metric.
- **issue**: The counter-dev prediction models the wrong switch, and the oracles leave out one of the three migration sequences, the one on the counter-dev handoff path. | **evidence**: counter-dev alternates dev.next and dev.previous (verify.rs:1818-1836). Catalog order is manifest order (catalog.rs:51-59; dev.rs:1654-1666), so counter at manifest.toml:580 toggles with counter_migration at :617, which has migration_sequence at :627. v4 [warm.switch] models TodoMVC → counter instead. [oracles].migrations lists only todo and persons_pro. examples/migrations/counter/ exists. | **fix**: Model the switch lane as counter ↔ counter_migration, including the migration-stage compile path. Add examples/migrations/counter/sequence.toml to the oracles and to cutover item 2, and change "both migration sequences" to "all three".
- **issue**: Nothing gates the new engine during migration. v4 says gates apply only to the default engine and the old engine is "reference (never gated)", yet P9 requires the checkpoint to exit nonzero on any failure, and a checkpoint must run when each milestone closes. Either the checkpoint checks nothing, or it stays red on the incomplete new engine for months. compiler-ab's cells, TodoMVC and NovyWave, cannot compile on the new engine until the Theme refactor and the strictness fixes land. | **evidence**: design/compiler.v4.toml header: "Gates apply to the engine the playground uses by default. Until cutover the old engine is ... reference (never gated)". [protocol.dev].cells = todo-mvc-physical and novywave. The TodoMVC cross-unit edits are deferred until after the Theme refactor (compiler_edits.toml:119-120). | **fix**: Define a milestone gate profile. The oracle gates always apply to the new engine. Performance gates apply per fixture once the new engine accepts that fixture, with an explicit expected-unsupported list in the checkpoint report. Before TodoMVC compiles, compiler-ab uses what the engine does support: the small corpus, counter, language_surface/current and the scaling generators.
- **issue**: t_plan stops at "runnable MachinePlan available to the runtime", but the runtime only accepts a SealedMachinePlan. Sealing runs verify_plan, which includes a whole-plan SHA-256. That runtime-side contract is outside the compiler crates and is missing from the inventory. The runtime and playground line caps have almost no headroom for the API change it needs. | **evidence**: boon_plan/src/lib.rs:10032-10055: seal_machine_plan → verify_plan → plan_hash. SealedMachinePlan is used by boon_plan_executor/src/machine.rs, boon_runtime, program_core.rs (sealing at :446, digest check at :453) and boon_compiler lib.rs:535. Plan validation takes about 90 ms of TodoMVC verified (verify_all C5). Recount this round with the architecture.rs logic: runtime+executor 41,105 of 42,000 lines, playground 31,082 of 32,000. | **fix**: Define t_plan as "plan accepted by the runtime", or add a t_ready, so the compiler lanes predict the native gates. Add the SealedMachinePlan / verify_plan / plan_hash contract to the cutover inventory: in-process trusted plans skip the hash, and untrusted loads verify. Budget the line headroom, or plan compensating deletions, in boon_runtime, boon_plan_executor and the playground.
- **issue**: No oracle covers the editor's language snapshot, even though decisions 5 and 7 are rules about how types are displayed. One contract doc that contradicts decision 4 is also missing from the update list. | **evidence**: LanguageProjectSnapshot and InspectorHint (boon_editor/src/language.rs:21-31, 102-110) carry compact_label, detail_label, display_tree, semantic items and messages. The .expect format compares only `unit:line:col CODE`, and warm==cold compares only diagnostics and the plan. BOON_TYPE_NOTATION_AND_INSPECTOR.md:216-217 documents value-dependent narrowing (`Element/stripe` is narrowed to Row or Stack when direction is known), which decision 4 removes. That file is not in §5's list of docs to update. | **fix**: Add hint lines to .expect, for example `hint unit:line:col <compact_label>`, covering field-order and union display. Include the full language snapshot in warm==cold and in determinism. Add BOON_TYPE_NOTATION_AND_INSPECTOR.md to the docs rewritten at cutover.
- **issue**: Gate 0 is changed before cutover without an owner question, which contradicts the design's own atomic-cutover rule and AGENTS.md's "do not weaken" rule. There is also an ordering bug: P2 installs format 4 before P3's "final old-engine reference run with the old tools", and those tools need format 3. | **evidence**: normative_single_thread_compiler (architecture.rs:218-279) reads compiler_performance.rs and compiler_interactions.rs (env_remove counts 1 and 3). Deleting those files at migration start forces an edit to gate 0. compiler_performance.rs:1534-1540 needs the format-3 machine_plan_sha256. P2 depends only on P1. | **fix**: At migration start, remove only the two env_remove-count assertions, with explicit owner approval, and keep the kernel-string half until cutover. Add this as an owner question. Order the work as: P3 reference run, then P2 and the P3 deletions together.
- **issue**: Format 4 quietly drops two format-3 limits, which breaks its own rule that removing a limit needs the owner. | **evidence**: budgets/compiler.toml:81 cancellation_max_ms = 8.0 becomes only supersede_stop_p95_ms = 2.0, with no max. budgets/compiler.toml:77 loaded_bundle_lookup_max_ms = 1.0 has no v4 equivalent. | **fix**: Add supersede_stop_max_ms = 8.0. Either keep a bundle-lookup (switch) limit or list its removal as an owner question.
- **issue**: The noise-control rule cannot work on this machine. Samples taken above a 1-minute loadavg of 1.5 are discarded, but this desktop idles around 1.3-1.4, the single-thread lane adds about 1.0, and the threads=auto lane raises its own loadavg. Parallel agents (which AGENTS.md encourages) compiling during an A/B are the real noise source, and nothing guards against them. | **evidence**: `uptime` this round: load average 1.44, 1.34, 1.38 with no benchmark running. /proc/loadavg read 1.05-1.42 during my 12 sequential samples. Those samples (TodoMVC diagnostics, fresh process) had a mean of 400.2 ms and SD of 4.9 ms (CV 1.23%), so interleaving already absorbs slow drift. | **fix**: Replace the loadavg rule with a machine-wide flock measurement lock. Refuse to start while cargo or rustc processes are running. Measure foreign CPU time from /proc/stat minus the process's own time around each block, and discard a block only when that is high. Keep ABBA interleaving.
- **issue**: The profiling tool is overbuilt for this machine's real situation. samply is installed and produced the L1 profile on 2026-09-28 at paranoid=1. The owner-side fix is one line of sysctl.d. An 800-line in-repo sampler with DWARF unwinding, symbolization, folded output, per-thread splits and phase cross-checks is an underestimate and a long-term maintenance cost. | **evidence**: ~/.cargo/bin/samply 0.13.1 is present. The user's memory note compiler-profiling-setup.md records the samply workflow and that paranoid resets on reboot. `perf` is not installed. samply_err.txt shows the refusal at paranoid=2 only. | **fix**: Recommend option (b): the owner persists kernel.perf_event_paranoid=1, and the repo ships only a small summarizer over samply's JSON (nearest-preceding-symbol attribution for asm, plus the phase cross-check against compile-bench marks on the same clock). Build an in-repo sampler only if the owner refuses, and then frame-pointer only, without --unwind dwarf.
- **issue**: The process assumes commits the agents are not allowed to make, and it leaves the dirty tree unresolved. | **evidence**: AGENTS.md:14 says "Do not commit or push unless the user explicitly asks". The design depends on "same commit" rules, `compiler-ab --a/--b <git rev>` and a one-commit cutover. git status shows uncommitted old-kernel speedups (owner.rs, solver.rs, requirements.rs, term.rs: +123/-30) plus an AGENTS.md diff. The final old-engine reference run and the differential oracle depend on whether those stay. | **fix**: Change "same commit" to "same change set". Make compiler-ab default to two binaries, built from HEAD and from the working tree, using one shared target directory with the binaries copied out. Add a migration-start owner decision: commit or revert the uncommitted kernel changes before the reference run.

## missing
- A baseline handoff run before migration. Only target/reports/report-v2/architecture.json exists, and gate 0 is red on the line cap. persons-pro's bounded-starter-source-compile budget (4 ms p95) is probably red on the old engine today: the one-line child program measured 7.2-7.7 ms per fresh process, and its phase sum alone is about 4.2 ms. "The old engine keeps all 7 gates satisfiable" is unverified. Add P0: run the 7 gates once on the current tree and record which are red, so the migration does not inherit an impossible precondition.
- A wasm32-unknown-unknown and x86_64-unknown-linux-musl build gate, plus a single-thread and clock fallback for the new crates (boon_runtime depends on boon_compiler; Dockerfile:16-17).
- examples/migrations/counter/sequence.toml in the oracles, and counter ↔ counter_migration as the switch workload.
- An editor language-snapshot oracle (inspector hints and display trees, semantic items), included in warm==cold and in determinism.
- The runtime plan-acceptance contract (SealedMachinePlan, verify_plan, plan_hash in boon_plan, plan_executor and program_core) in the cutover inventory, with t_plan or t_ready including it.
- An inventory of which compiler-produced identities survive decision 3. source_bundle_digest_v1 is consumed by boon_runtime/src/lib.rs:240, boon_app_package/src/bundle.rs:53 and :498, boon_host_runtime/src/persistent.rs:3974, editor currentness in boon_editor/src/language.rs:105 and :119, and the boon_contract fixture at lib.rs:496. It is a content identity, not a self-proof, and must not be deleted by accident.
- The status of boon_parser and boon_syntax. Neither is on the AGENTS.md list of the old pipeline, yet the new front end does the parsing. The editor, the formatter (NATIVE_GPU_PIPELINE.md:26 says boon_parser owns formatting), boon_effect_schema and the playground depend on them.
- Existing Boon corpora for the diagnostics and differential oracles: testdata/phase0 (19 fail-closed fixtures used by boon_phase0_baseline), testdata/*.bn (6), crate-local .bn files (10), examples/language_surface/current, and the 43 of 56 examples/*.bn outside the manifest. Those include bytes_typecheck_negative_fixture.bn and bytes_negative_templates/, plus 49 .scn files, most not run by any test. The kernel's 198 tests and checked's 20 contain no Boon source to harvest.
- Distributed and app-packaging checks at cutover: boon_app_package build.rs:162 (compile_trusted_package_distributed_program_bundle) through boon_app_cli, boon_server_runtime/tests/loopback.rs, and the FjordPulse Docker build.
- Headroom on the playground and runtime line caps (918 and 895 lines left) for the engine selector, the compile-service client and the runtime seal API change.
- A mechanical enforcement of decision 3 for the new crates from day one: a dependency allowlist (no sha2, ciborium or blake3 outside a cfg(test/debug) verify module) and a line cap. Today it is enforced only by AGENTS.md prose.
- Evidence retention. target/reports is untracked and wiped by cargo clean, yet the rule says "plans cite report paths". Commit a one-line checkpoint summary (commit, engine, key p95s) per milestone.
- Build-profile candidates are workspace-wide. panic="abort" would break boon_host_runtime's worker panic isolation (catch_unwind at lib.rs:1472), and LTO or PGO changes the playground and FjordPulse deploy binaries. Adopting any of them must also re-run the native gates, not only a compiler checkpoint.
- Edit-corpus maintenance rule. When an example refactor removes an anchor, re-anchoring or replacing an edit within the same class should not need the owner. As written, "removing an edit needs the owner" creates friction on every Theme change.

## unrealistic
- compiler-ab at "1-6 min" leaves out builds. Each git revision gets its own target/ab/<rev>: a cold release build (the memory note records about 3.5 min for the profiling build with 6 jobs) and about 3.1 GB of disk (target/release is 3.1G) per revision.
- An 800-line xtask sampler that includes DWARF unwinding with framehop, MMAP2 address mapping, object and rustc-demangle symbolization, folded and top tables, per-thread splits and a phase cross-check.
- P4 acceptance: "the old engine reproduces format-3 phase timings within 3%" while the measured path changes. The product order uses EditorRich, where format 3 used RuntimePacked one intent per process (449 vs 434 ms, C5). The manifest lookup is removed, and it is about 41% of counter diagnostics.
- The differential over all ~900 intermediate states of the warm corpus, with every divergence classified and given a corpus repro. The old engine has no diagnostic codes and its error recovery differs, so this becomes an unbounded manual triage queue.
- Harvesting Boon programs from the ~940 old #[test]s. The kernel's 198 tests and checked's 20 contain no Boon source at all.
- A warm diagnostics_max_ms of 33.4 applied to every revision (about 1,800 samples across fixtures and modes) on a desktop whose idle load is about 1.4. That gates the OS scheduler's tail, not the compiler.
- Time-ratio scaling gates at ≤2.3 per doubling, from 5 fresh-process CPU medians with the baseline subtracted. At target speed, (t1600 - t0)/(t800 - t0) is a ratio of millisecond-scale differences, and cache-level crossings push linear code toward 2.2-2.5.
- The acceptance "an A/A run reports no effect within 30 blocks" judged on a single run, when the stopping rule peeks up to 26 times at 99%. Use a false-positive rate over N A/A runs, or an always-valid sequential test.

## simplifications
- Use user-only instructions:u counts (perf_event_open in counting mode; the same PERF_TYPE_HARDWARE class as the cycles event the design showed is allowed at paranoid=2) as the fast A/B signal and as the scaling metric, as rustc-perf does. Wall time stays the gate. Most A/B runs then resolve in about 5 blocks, and scaling stops depending on noisy time ratios.
- Drop the in-repo sampler. The owner persists paranoid=1, and the repo keeps samply plus a ~200-line summarizer with nearest-symbol attribution and the phase cross-check.
- Merge compiler-ab and compiler-checkpoint into one xtask command with modes, so they share sample collection and statistics. The default A/B inputs are two binaries from one shared target directory.
- Drop the new BOON_BUILD_ID. build.rs already computes a workspace content hash (BOON_BUILD_SOURCE_WORKSPACE_SHA256). Remove only the HEAD and dirty watch paths, or record identity only in xtask, once per session.
- Gate only end-to-end single-thread throughput (diagnostics and verified). Report per-phase lines/s without gating it: per-phase floors assume a front/check/lower structure that an incremental or demand-driven engine may not have.
- Gate absolute peak RSS. The empty-process baseline is 5.8 MiB, so the delta machinery adds nothing.
- Freeze the MachinePlan format (boon_plan) until cutover. The new engine targets the existing plan, which keeps runtime, persistence, app packages and console lowering out of the migration. It also allows a canonicalized plan-structure differential for valid programs, far stronger than scenarios alone. Plan-format work comes after cutover.
- Narrow the differential. For invalid inputs compare accept/reject and the first error line. For valid inputs compare canonical plans (if frozen) or scenario behaviour. Group divergences by (old message class, new code) rather than one entry per state.
- Use one process-global engine switch in the facade instead of threading the choice through each compile request.
- Add the engine-neutral compiler-crate-graph check, the no-sha2/ciborium rule and a line cap for the new crates on the day they are created, not at cutover. Only the retirements of old-crate checks need to wait.
- Drop the `negative` gate from the shadow runs. It checks only oversized observer frames (verify.rs:589-615) and never touches the compiler.

## extra_owner_questions
- Allocator: should the playground adopt the bench's mimalloc, or should the product lane measure with the system allocator the playground uses today? The difference is about 17% on NovyWave verified.
- Should the uncommitted old-kernel speedups (owner.rs, solver.rs, requirements.rs, term.rs) be committed or reverted before migration starts? They change the old-engine reference and the differential oracle.
- Decision 1 calls the old compiler a "test-only differential oracle". May the playground switch its default to the new engine per milestone, as soon as the handoff gates pass on it, rather than only in one final atomic cutover?
- Is the MachinePlan format frozen until cutover, with plan-format redesign deferred to after it?
- Must the new compiler support wasm32 (web host) at feature parity from cutover, and is in-browser compile latency a product metric?
- Are boon_parser and boon_syntax replaced by the new front end, or kept for the editor, formatter and effect schema? This decides whether they are on the "do not touch" list.
- At migration start, may the two env_remove-count assertions of normative-single-thread-compiler be removed (a gate-0 change) when the old harness is deleted?
- Format 4 drops cancellation_max_ms = 8.0 and loaded_bundle_lookup_max_ms = 1.0. Do you approve dropping them, or should they be kept?
- Which identities survive decision 3 as runtime contracts? In particular source_bundle_digest_v1 (runtime, app packages, persistence, editor currentness) and the program-artifact plan_digest.
