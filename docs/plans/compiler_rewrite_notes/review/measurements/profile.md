# Sampling profile of the release compiler (re-verification of the plan's attribution claims)

Date: 2026-09-29, 23:27-23:41 local. Branch `compiler-rewrite-plan`, HEAD a60a11d6.
Binary: `/home/martinkavik/repos/boon-circuit/target/release/boon_cli` (built 2026-09-29 19:10, 39,068,512 bytes, unstripped).
Machine: i7-9700K (8 cores, no SHA-NI), 48 GB, Linux 6.18.7, samply 0.13.1, `perf_event_paranoid` = 1 (not changed).
Fixtures: `examples/counter.bn`, `examples/todo_mvc_physical/RUN.bn`, `examples/novywave/RUN.bn`.

Raw profiles (too big for the repo): `/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/measure/prof/*.json.gz` (+ `.syms.json` sidecars, `boon_cli.nm.txt`).
Summarizer text output, per-run compiler-sample JSON, samply stderr and uptime brackets: `raw/profile/`.
Summarizer: `tools/samply_summary.py`.

## 1. Headline

| Fixture (intent) | Plan claim: SHA-256 + canonical CBOR share of verified time | Measured (compile window) | Verdict |
| --- | --- | --- | --- |
| TodoMVC verified | 25% | SHA-256 leaf 21.0%, SHA inclusive 23.1%, CBOR/serde 4.0%, **union 25.3%** (1,075 / 4,257 samples) | confirmed |
| NovyWave verified | 31% | SHA-256 leaf 25.2%, SHA inclusive 26.0%, CBOR/serde 5.9%, **union 31.3%** (1,889 / 6,038) | confirmed |
| counter verified | 39% | 1 kHz: SHA leaf 25.7%, union **35.2%** (229 / 651); 5 kHz: SHA leaf 27.0%, union **38.7%** (1,124 / 2,904) | confirmed |
| TodoMVC diagnostics | (audit: 4.4%) | SHA leaf 5.7%, union 6.3% (136 / 2,153) | consistent |

The plan's SHA-256 + CBOR shares reproduce on today's HEAD binary to within about one percentage point on TodoMVC and NovyWave and within 0-4 points on counter. Everything below is about how the number was obtained and what the rest of the time is.

## 2. Commands

The task's literal command fails: `boon_cli compiler-sample ... --samples 5` is rejected by the CLI (`crates/boon_cli/src/compiler_sample.rs:818`: "compiler cold observations require exactly one sample per producer process", exit 1). Fresh-process mode does not spawn children; one process is one sample. The equivalent is samply's `--iteration-count N`, which runs the command N times under one recording, each iteration a fresh process. All recordings were made from the repo root.

```bash
cd /home/martinkavik/repos/boon-circuit
P=/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/measure/prof
# counter verified, 20 processes, 1 kHz (default rate)
~/.cargo/bin/samply record --save-only --unstable-presymbolicate --iteration-count 20 -o $P/counter_verified.json.gz -- \
  /home/martinkavik/repos/boon-circuit/target/release/boon_cli compiler-sample examples/counter.bn --intent verified --mode fresh-process --samples 1 > $P/counter_verified.stdout 2> $P/counter_verified.stderr
# TodoMVC verified, 5 processes
~/.cargo/bin/samply record --save-only --unstable-presymbolicate --iteration-count 5 -o $P/todo_verified.json.gz -- \
  /home/martinkavik/repos/boon-circuit/target/release/boon_cli compiler-sample examples/todo_mvc_physical/RUN.bn --intent verified --mode fresh-process --samples 1 > $P/todo_verified.stdout 2> $P/todo_verified.stderr
# NovyWave verified, 3 processes
~/.cargo/bin/samply record --save-only --unstable-presymbolicate --iteration-count 3 -o $P/novywave_verified.json.gz -- \
  /home/martinkavik/repos/boon-circuit/target/release/boon_cli compiler-sample examples/novywave/RUN.bn --intent verified --mode fresh-process --samples 1 > $P/novywave_verified.stdout 2> $P/novywave_verified.stderr
# TodoMVC diagnostics, 5 processes
~/.cargo/bin/samply record --save-only --unstable-presymbolicate --iteration-count 5 -o $P/todo_diagnostics.json.gz -- \
  /home/martinkavik/repos/boon-circuit/target/release/boon_cli compiler-sample examples/todo_mvc_physical/RUN.bn --intent diagnostics --mode fresh-process --samples 1 > $P/todo_diagnostics.stdout 2> $P/todo_diagnostics.stderr
# supplementary: counter verified at 5 kHz (the 1 kHz run has only 651 compile-window samples)
~/.cargo/bin/samply record --save-only --unstable-presymbolicate --rate 5000 --iteration-count 20 -o $P/counter_verified_5khz.json.gz -- \
  /home/martinkavik/repos/boon-circuit/target/release/boon_cli compiler-sample examples/counter.bn --intent verified --mode fresh-process --samples 1 > $P/counter_verified_5khz.stdout 2> $P/counter_verified_5khz.stderr
# symbols for nearest-preceding attribution
nm -n -C --defined-only target/release/boon_cli > $P/boon_cli.nm.txt
# summaries (one per profile)
python3 docs/plans/compiler_rewrite_notes/review/measurements/tools/samply_summary.py $P/<name>.json.gz \
  --nm $P/boon_cli.nm.txt --syms $P/<name>.json.syms.json --stdout $P/<name>.stdout --label <name> \
  > docs/plans/compiler_rewrite_notes/review/measurements/raw/profile/<name>_summary.txt
```

Did samply capture the fresh processes? Yes. Each profile contains one thread per `boon_cli` process plus samply's own (empty) thread: 20 / 5 / 3 / 5 / 20 processes with 150-190 (counter, 1 kHz), 1,063-1,132 (TodoMVC verified), 2,195-2,258 (NovyWave), 538-578 (TodoMVC diagnostics) and 745-952 (counter, 5 kHz) samples each. Every process's `producer_pid` in the compiler-sample JSON matched a profile thread. `--mode empty-session` was not needed.

## 3. Noise (load and repeatability)

`uptime` brackets (load average 1-min at start / end of each series):

| Series | Before | After |
| --- | --- | --- |
| counter verified 1 kHz | 23:27:51 load 1.78, 1.68, 1.66 | 23:27:57 load 1.88, 1.70, 1.67 |
| TodoMVC verified | 23:28:52 load 1.54, 1.64, 1.65 | 23:28:58 load 1.58, 1.65, 1.65 |
| NovyWave verified | 23:28:58 load 1.58, 1.65, 1.65 | 23:29:05 load 1.94, 1.73, 1.68 |
| TodoMVC diagnostics | 23:29:05 load 1.94, 1.73, 1.68 | 23:29:09 load 2.10, 1.77, 1.69 |
| counter verified 5 kHz | 23:41:08 load 2.23, 1.64, 1.57 | 23:41:14 load 2.05, 1.62, 1.56 |

`elapsed_ms` (compiler-sample's own timer, the compile region) per process:

| Series | n | min | median | p95 | max |
| --- | --- | --- | --- | --- | --- |
| counter verified 1 kHz | 20 | 16.3 | 26.8 | 45.4 | 46.5 |
| counter verified 5 kHz | 20 | 16.6 | 17.7 | 48.4 | 50.3 |
| TodoMVC verified | 5 | 810.8 | 848.6 | 885.3 | 885.3 |
| NovyWave verified | 3 | 2004.7 | 2008.3 | 2045.5 | 2045.5 |
| TodoMVC diagnostics | 5 | 402.4 | 431.5 | 435.1 | 435.1 |

Counter is bimodal in both series: 9-11 runs at 16-18 ms and 8-9 runs at 27-50 ms (raw values in `raw/profile/*_compiler_sample.jsonl`). The slow runs are not attributable to one phase from the timers alone; treat any counter share as +-3 points. The other fixtures spread 4-9% between min and max, which does not move the shares below by more than a point.

## 4. Attribution method and its two problems

**Nearest-preceding symbol.** Frames in `boon_cli` are resolved against the full `nm -n -C` text-symbol table (36,777 symbols) by "largest symbol start <= address", ignoring symbol sizes, because the asm `sha256_compress` (at 0x1ad138c) has size 0 and a size-based lookup drops every sample inside it. samply's own `.syms.json` sidecar has no size-0 entries at all, so it cannot name those samples. Cross-check: for 400/400 of the sidecar's known addresses in `boon_cli` the `nm` nearest-preceding name agrees with the sidecar's name. libc/ld.so frames use the sidecar tables the same way. Address-less frames are labelled `[kernel]` (category Kernel) or `[garbage-return-address]` and kept, never dropped.

**Problem 1: the unwinder stops inside `sha256_compress`.** Every SHA-256 sample is a depth-2 stack: `sha256_compress` plus one garbage return address (the asm routine has no CFI). SHA time therefore has no caller and cannot be scoped or attributed by stack. This is why a first, stack-scoped version of the summarizer showed SHA-256 at 1.3% of TodoMVC "compile" time: it had silently excluded almost all SHA samples.

**Problem 2: deep stacks are truncated.** Under large frames (kernel solver, `checked_construction_from_kernel`, `DocumentCompiler`) the recorded stack ends before reaching the phase wrapper or `main_entry` (samply's user-stack copy runs out). In TodoMVC verified only 486 of ~4,050 compile-milliseconds carried a `check_runtime_source`/`finish_checked_sealed_machine_plan` frame. Stack-based *inclusive* shares are therefore lower bounds; leaf (self) attribution is unaffected.

**Fix: scope by time, not by stack.** compiler-sample prints `observation_started_unix_us` and `compiler_artifact_ready_unix_us` per process; these bracket exactly `elapsed_ms`. The profile's `samples.time` is CLOCK_MONOTONIC ms and `meta.startTime` is the unix-ms epoch of profile zero. The monotonic value of profile zero is not stored, so the summarizer calibrates it per profile as the constant that maximizes containment of the full-stack compile samples inside their windows (0.05 ms grid). Results: plateau width 0.0-0.75 ms; 3,374/3,375 (TodoMVC verified), 4,568/4,568 (NovyWave), 470/470 (counter 1 kHz), 2,117/2,117 (counter 5 kHz), 2,039/2,044 (TodoMVC diagnostics) full-stack compile samples fall inside their windows; per-process samples-in-window / `elapsed_ms` = 0.99-1.05 (counter 1 kHz: 1.07-1.16, i.e. 1-6 extra samples per 16-46 ms window). Phases are placed inside the window by cumulative phase timers; they run in a fixed order and sum to `elapsed_ms` within 0.3%.

**Independent cross-check of the time placement (task item c).** For samples that do carry phase evidence in their stack, the stack-derived phase agrees with the time-derived phase for 76.9% (TodoMVC verified), 68.1% (NovyWave), 86.1% / 87.5% (counter 1 kHz / 5 kHz) and 97.0% (TodoMVC diagnostics) of samples. The disagreements are almost entirely `typecheck -> semantic` and `semantic -> backend`, i.e. samples inside the semantic phase whose truncated stacks show only `boon_checked`/`boon_compiler_kernel` type frames, and backend samples whose stacks show only `boon_semantic::program_core` types; they are regex ambiguity from shared data types, not clock error (a clock error would also break the 100% window containment above). Time-phase shares vs timers agree within 0.1-1.4 points on every fixture (table in section 6).

**Whole-process context.** The fresh-process harness dominates process time outside the window: after the window each process SHA-256-hashes its own 39 MB binary (`producer_metadata`) and exports/hashes a pretty-JSON plan. Counter: compile window 17.7% of process samples, 80.4% after the window, `sha256_compress` alone 80.4% of the whole process (5 kHz). TodoMVC verified: 77.8% / 21.8%. NovyWave: 90.6% / 9.2%. TodoMVC diagnostics: 76.1% / 22.9%. All shares below are of the compile window only.

## 5. What dominates (compile window)

Top self-time functions (full top-40 lists in `raw/profile/*_summary.txt`):

| TodoMVC verified (4,257 samples) | % | NovyWave verified (6,038) | % | counter verified 5 kHz (2,904) | % | TodoMVC diagnostics (2,153) | % |
| --- | --- | --- | --- | --- | --- | --- | --- |
| sha256_compress | 20.7 | sha256_compress | 24.5 | sha256_compress | 26.6 | ComponentSolver::schedule_variable | 17.8 |
| ComponentSolver::schedule_variable | 8.9 | machine_plan_backend::erased_field_is_runtime_row_storage | 5.6 | libc readlink | 10.8 | sha256_compress | 5.3 |
| libc memcpy | 3.4 | libc memcpy | 3.5 | mi_page_free_list_extend | 6.0 | ComponentSolver::refresh_requirements | 4.7 |
| ComponentSolver::refresh_requirements | 2.4 | libc memcmp | 1.9 | ciborium_ll Encoder::push | 2.8 | TypeTermArena::term | 4.4 |
| TypeTermArena::term | 2.3 | ciborium_ll Encoder::push | 1.8 | libc statx | 2.6 | ComponentSolver::invalidate_requirement_cone | 3.2 |
| libc memcmp | 2.3 | _mi_page_malloc_zero | 1.6 | libc memcpy | 2.6 | TypeTermArena::find_term | 2.6 |
| ComponentSolver::invalidate_requirement_cone | 1.8 | ComponentSolver::evaluate_summary_value | 1.4 | libc memcmp | 1.7 | libc memcpy | 2.2 |
| _mi_page_malloc_zero | 1.7 | SemanticExecutionImageColumnsV1::validate | 1.3 | ciborium_ll Title::from | 1.3 | ComponentSolver::variable_syntax_selected | 2.0 |
| TypeTermArena::find_term | 1.6 | TypeTermArena::term | 1.2 | _mi_page_malloc_zero | 1.3 | ComponentSolver::evaluate_summary_value | 1.8 |
| DocumentCompiler::compile_expression | 1.2 | TypeTermArena::find_term | 1.1 | mi_theap_malloc_aligned | 0.9 | ComponentProgramBuilder::finish | 1.5 |

Inclusive shares by group (stack contains a matching frame; lower bounds where stacks are truncated; SHA-256 leaves are exact):

| Group | TodoMVC verified | NovyWave verified | counter verified (5 kHz) | TodoMVC diagnostics |
| --- | --- | --- | --- | --- |
| SHA-256 leaf (self) | 21.0 | 25.2 | 27.0 | 5.7 |
| SHA-256 inclusive | 23.1 | 26.0 | 28.0 | 5.8 |
| CBOR/serde (ciborium, serde, canonical_serde) | 4.0 | 5.9 | 11.7 | 0.5 |
| SHA-256 or CBOR union | **25.3** | **31.3** | **38.7** | 6.3 |
| receipt/seal/fingerprint/manifest/digest names | 7.9 | 12.0 | 36.1 | 3.7 |
| validate/verify/freeze names | 5.3 | 6.4 | 24.6 | 3.3 |
| examples/manifest.toml (example_manifest, toml) | 1.2 | 0.5 | 25.6 | 2.7 |
| Debug fmt | 2.1 | 1.6 | 0.1 | 0.3 |
| incidental union, narrow (SHA, CBOR, verify_plan, receipt/seal/fingerprint/manifest/digest) | 28.7 | 36.6 | 65.7 | 9.2 |
| incidental union, broad (+ validate/verify/freeze, manifest.toml, variant_set_with_order, Debug fmt) | **34.4** | **43.1** | **68.1** | 14.0 |
| boon_compiler_kernel (any frame) | 47.7 | 25.8 | 14.6 | 82.6 |
| boon_compiler_kernel::solver | 33.8 | 11.2 | 0.7 | 65.5 |
| boon_compiler_kernel::term | 12.7 | 7.1 | 1.5 | 21.4 |
| boon_checked | 11.2 | 13.8 | 8.0 | 0.2 |
| boon_semantic | 23.0 | 39.5 | 14.8 | 0.0 |
| boon_compiler::kernel_oracle | 24.4 | 16.8 | 21.7 | 79.0 |
| boon_compiler::machine_plan_backend | 2.0 | 8.4 | 1.7 | 0.0 |
| boon_compiler::document_plan_backend | 9.7 | 3.1 | 0.7 | 0.0 |
| boon_plan | 11.2 | 9.3 | 2.5 | 0.0 |
| boon_parser | 3.0 | 3.7 | 6.0 | 6.5 |
| boon_typecheck | 1.1 | 2.6 | 7.3 | 0.9 |
| boon_ir / boon_verify | 0.5 / 0.0 | 1.3 / 0.0 | 0.3 / 0.2 | 0.0 / 0.0 |
| alloc/free (mimalloc, alloc::alloc, RawVec grow) | 9.1 | 8.1 | 14.2 | 7.2 |
| BTreeMap | 8.0 | 14.0 | 9.3 | 2.9 |
| memcpy/memmove/memset/memcmp | 5.7 | 5.4 | 4.3 | 3.7 |

Reading: after SHA-256, TodoMVC verified is the kernel solver (`schedule_variable` alone 8.9% self; solver frames 33.8% inclusive despite truncation), then semantic (19.5% by time) and the document backend (10.5%). NovyWave verified is semantic (41.3% by time, of which SHA leaves are 27.7 points) and one backend predicate (`erased_field_is_runtime_row_storage`, 5.6% of the whole window). Counter verified is `examples/manifest.toml` handling (parse phase 32.8% of the window; `validate_at` 63.7% of parse, 315 `readlink` + 76 `statx` leaf samples) plus SHA-256. TodoMVC diagnostics is the kernel solver (solver frames 65.5%, `schedule_variable` 17.8% self).

Where the SHA-256 leaves fall (by time-phase): TodoMVC verified plan_validation 42%, semantic 27%, typecheck 26%, parse 5%; NovyWave semantic 45%, typecheck 37%, plan_validation 10%, parse 5%, backend 3%; counter (5 kHz) typecheck 58%, semantic 35%; TodoMVC diagnostics typecheck 59%, parse 41%. Consumers inferred from the nearest full-stack neighbour in time (indicative only): `verify_plan`/`CompiledMachinePlanFromSource::seal` (TodoMVC 8.7 points), `DefinitionTermProofScratch::import` and `receipt::build_borrowed_snapshot_receipts` (kernel term proofs and receipts), `ExecutionImageHandoffBuilderV5::push_presealed`, `ExecutionReceiptPublisherV5::*`, `dependency_manifest::*_v7`, `checked_image_handoff_from_kernel_publication_parts`, `checked_structural_call_site_digest_v4`.

## 6. Phase shares: profile vs compiler-sample timers

Time-phase% is the share of window samples falling in each phase's timer-derived sub-window; timer% is the phase timer's share of the summed timers; stack-phase% is the stack-evidence-only attribution (its denominator is the samples with evidence).

| Phase | TodoMVC verified time / timer / stack | NovyWave verified | counter 5 kHz | TodoMVC diagnostics |
| --- | --- | --- | --- | --- |
| parse | 4.8 / 4.2 / 6.3 | 4.6 / 4.3 / 7.8 | 32.8 / 33.0 / 47.5 | 10.8 / 9.9 / 11.5 |
| typecheck | 53.8 / 54.2 / 72.1 | 37.8 / 38.0 / 52.1 | 39.6 / 39.5 / 35.6 | 88.7 / 90.1 / 88.5 |
| semantic | 19.5 / 19.6 / 16.8 | 41.3 / 41.5 / 35.5 | 23.1 / 23.2 / 14.5 | - |
| contract_verify | 0.0 / 0.0 / 0.0 | 0.0 / 0.0 / 0.0 | 0.1 / 0.2 / 0.2 | - |
| ir_lower | 0.4 / 0.4 / 0.1 | 0.9 / 0.9 / 0.2 | 0.3 / 0.3 / 0.0 | - |
| ir_validation | 0.0 / 0.1 / 0.0 | 0.4 / 0.4 / 0.1 | 0.1 / 0.1 / 0.0 | - |
| backend | 10.5 / 10.6 / 1.9 | 11.4 / 11.5 / 3.1 | 1.8 / 1.8 / 0.8 | - |
| plan_validation | 11.1 / 10.9 / 2.7 | 3.6 / 3.4 / 1.2 | 2.3 / 2.0 / 1.4 | 0.5 / 0.0 / 0.0 |

The stack column undercounts backend and plan_validation because those phases are where stacks are truncated (DocumentCompiler) or SHA-dominated (seal); the time column is the one to trust. The audit's gdb sampler reported the same phase shares within 1 point (TodoMVC verified typecheck 55.5%, semantic 19.7%, backend 11.5%, plan validation 11.1%); today's numbers are 53.8 / 19.5 / 10.5 / 11.1.

## 7. Claim-by-claim

| Claim (plan section) | Measured today | Verdict |
| --- | --- | --- |
| SHA-256 + canonical CBOR = 25% TodoMVC verified, 31% NovyWave verified, 39% counter verified (3.2) | 25.3% / 31.3% / 35.2% (1 kHz, 651 samples) and 38.7% (5 kHz, 2,904 samples). SHA leaf alone 21.0 / 25.2 / 27.0%. | confirmed |
| "all plainly incidental work together is 34-46%" (3.2, headline) | broad regex union 34.4% TodoMVC, 43.1% NovyWave (narrow: 28.7 / 36.6%); counter 68.1%. Lower bound because inclusive matching sees truncated stacks; class membership is regex-defined (hashing, CBOR, verify_plan, receipts/seals/fingerprints/manifests/digests, validate/verify/freeze, manifest.toml, variant_set_with_order, Debug fmt). | partially confirmed (34 exact; 43 vs 46) |
| Typecheck orchestration: 30-70% of the phase is outside the solver (3.4) | outside solver frames (solver::, ComponentSolver, KernelSession::check, schedule_variable): TodoMVC verified 28.9%, NovyWave verified 56.6%, counter verified 85.5%, TodoMVC diagnostics 15.7%. Outside the whole boon_compiler_kernel crate: 14.3 / 37.5 / 65.1 / 7.4%. Largest non-solver items: SHA leaves (10.3 / 24.9 / 39.4 / 3.8% of typecheck), prepare_kernel_project_projection (4.1 / 11.8 / 27.4%), KernelCheckedLinkLayout::materialize_rows (3.9 / 6.5 / 3.8%), KernelProjectInput::new_with_abi (1.0 / 3.3 / 6.2%). | partially confirmed: fixture-dependent; TodoMVC diagnostics is 16% outside the solver by this definition, 29% for TodoMVC verified |
| Semantic: half of NovyWave's 780 ms is receipts and self-validation (3.4) | NovyWave semantic phase = 41.3% of window (~830 ms/process); inside it SHA leaves 27.7%, receipt/seal/fingerprint/manifest/digest/hash frames incl. SHA leaves 45.7%, plus validate/verify 53.4%. TodoMVC: 44.7 / 54.0%. Sub-stages: derive_semantic_execution_graph 15.4%, build_canonical_program_core 10.7%, build_callable_dependency_manifest_v7 5.7%, finalize_execution 4.8%. | confirmed (46-53%) |
| Backend: one field predicate is 57% of NovyWave's backend (3.4) | `machine_plan_backend::erased_field_is_runtime_row_storage` is 48.6% of NovyWave backend samples, self and inclusive (336 / 692), called from Vec collection in `materialized_output_fields` (198), `derived_expression_for_value` (62), `state_dependent_materialized_row_fields` (33), `take_authority_mapped_row_fields` (18). 0 samples on TodoMVC (its backend is 92.8% DocumentCompiler). | partially confirmed (49% vs 57%; 95% CI +-3.7 points) |
| verify_plan runs at every seal and 80-92% of it is SHA-256 (3.2) | verify_plan cannot be isolated (SHA stacks are truncated); the enclosing plan_validation (seal) phase is used. TodoMVC: 11.1% of window, SHA leaf 79.4%, memcpy 7.0%, binary Encoder 4-5%. NovyWave: 3.6% of window, SHA leaf 68.6%. counter 5 kHz: 2.3%, SHA 41.2% (68 samples). | partially confirmed: TodoMVC at the lower edge, NovyWave 69% of the seal phase |
| SHA-256 is used as the interning hash (3.4, backend) | Static: `boon_plan::PlanRowExpressionArena::intern` (crates/boon_plan/src/lib.rs, impl at 8893) computes `let structural_key = canonical_sha256(&node)?` per intern. Profile: frames of `PlanRowExpressionArena::intern`/`canonical_sha256`/`boon_plan::binary::encode` appear in 4.3% of NovyWave backend samples (0.45% of the window; SHA leaves in the backend 6.1%), 0.2% of TodoMVC backend, 3.9% of counter backend. | confirmed as a fact; cost is small (<1% of verified time) |
| Parser: counter's 3.5 ms parse is 76% `examples/manifest.toml` validation (3.4) | counter 5 kHz parse phase: manifest.toml lookup/validation/path canonicalization 80.9% (`ExampleManifest::validate_at` 63.7%, `toml::de::from_str` 14.2%), lexing/AST frames 9.1%. | confirmed |
| Diagnostics are bound by the kernel solver (3.1) | TodoMVC diagnostics: typecheck 88.7% of window; solver frames 84.3% of typecheck; `schedule_variable` 17.8% self. | confirmed |

## 8. Caveats

- One sample per process is enforced by the CLI; `--iteration-count` reproduces the "N samples" intent, but each N is a fresh process with cold page cache effects included.
- Sample counts are modest: 4,257 (TodoMVC verified), 6,038 (NovyWave), 2,904 (counter 5 kHz; 651 at 1 kHz), 2,153 (TodoMVC diagnostics) compile-window samples. Binomial 95% half-widths at these sizes are 1.3 / 1.2 / 1.8 / 1.0 points for a 25% share.
- SHA-256 attribution to callers is by time neighbourhood, not by stack; the per-phase SHA split is exact, the per-function consumer list is indicative.
- Inclusive shares are lower bounds where stacks are truncated (kernel solver, DocumentCompiler, seal). Self-time and time-window numbers are not affected.
- The anchor calibration assumes a single monotonic offset per profile; it was validated by 99.8-100% containment and per-process window/elapsed ratios of 0.99-1.05 (counter 1 kHz 1.07-1.16, one to six samples on 16-46 ms windows).
- The machine is noisy (load 1.5-2.2 during the series) and counter is bimodal (16-18 ms vs 27-50 ms). Shares on counter carry +-3 points; the two counter series agree on SHA leaf (25.7 vs 27.0%) and union (35.2 vs 38.7%).
- The release binary has function symbols but no line tables; attribution is per function (nearest preceding text symbol), never per line.
- Only the HEAD binary was profiled. The 2026-09-28 binary (`scratchpad/bin/boon_cli.sep28`) was not, as the HEAD numbers already reproduce the plan's figures.
