# Warm path re-measurement of BOON_COMPILER_REWRITE_PLAN.md (§0, §2, §3.5)

Measured 2026-09-29 23:52 - 2026-09-30 00:00 CEST on the i7-9700K host (8 cores, 48 GB,
Linux 6.18.7, `perf_event_paranoid=1`, untouched). Measurement agent of the independent plan
review. Nothing outside this directory and the scratchpad was written; `budgets/compiler.toml`
was not touched; no commit. The only build was `cargo build -p xtask`, which recompiled nothing
(0.26 s wall, "Finished ... in 0.22s"); the release producers `target/release/boon_cli` and
`target/release/boon_cli_evidence` (both mtime 2026-09-29 19:10:34, HEAD `a60a11d6`, clean)
were used as they were. Machine load: 1.17 before, 1.99 after; peak 2.26 during the cold
reference. All commands ran from the repository root.

## Verdict in one line

The plan's warm numbers reproduce: edit → diagnostics on TodoMVC is 380 ms in the single
benchmark sample and 386.8 ms median over 30 samples (cold, same binary, same session mode:
385.8 ms), edit → verified preview is 1,158 ms / 1,192.5 ms median; the warm status *is*
hard-coded to fail (two evaluation flags are literal `false`, and the aggregate additionally
requires an always-non-empty "missing evidence" list to be empty); the playground publishes
diagnostics in the same message as the verified preview, after both compiler requests. The one
claim that only partially holds is "a request sequence the product never makes": the product
does make a two-request sequence of the same shape on every edit, only with the larger
`EditorDiagnostics` intent instead of the benchmark's `Diagnostics`.

## Plan claims checked

| plan text (ref) | measured / found | verdict |
| --- | --- | --- |
| warm edit → diagnostics (TodoMVC) 372-397 ms (§0 table, §2 "Warm path") | xtask 0/1: 380.1 ms (product lane), 379.4 ms (evidence lane). Direct 3+30 HEAD: min 382.0 / median 386.8 / p95 408.5 / max 455.2. Direct 3+30 sep28: 376.9 / 389.5 / 405.0 / 410.1 | confirmed |
| warm edit → preview (TodoMVC) 1,151-1,207 ms (§0), "about 1.15 s" (§2) | xtask 0/1: 1,157.8 ms (product), 1,171.6 ms (evidence). 3+30 HEAD: 1,173.2 / 1,192.5 / 1,249.2 / 1,393.3. 3+30 sep28: 1,162.0 / 1,193.1 / 1,231.0 / 1,277.4 | confirmed |
| "Warm is not faster than cold" (§2) | warm edit → diagnostics median 386.8 ms vs cold empty-session diagnostics median 385.8 ms (same binary, same series): ratio 1.00. Warm verified-preview request alone median 801.8 ms vs cold verified 782.7 ms: ratio 1.02. Parse is the only reused phase (17.1 → 6.7 ms) and typecheck grows by about the same (363.7 → 377.2 ms) | confirmed |
| warm benchmark "status is hard-coded to fail" (§3.5) | `compiler_interactions.rs:1012` `full_cancellation_gate_pass: false`, `:1014` `native_present_gate_pass: false`; `warm_status` requires both (`:1034-1035`). `collect` always fills `missing_acceptance_evidence` with two strings and `aggregate_status` requires it empty. Observed: `warm fail ... aggregate fail (missing native-presentation and in-flight-supersession evidence)` with every measurable gate except the latency budgets passing | confirmed |
| warm benchmark "measures a request sequence the product never makes" (§3.5) | Benchmark: `apply_update` → `request(Diagnostics)` → `request(VerifiedPreview)` on one revision. Product (`compile.rs:537-541`, `594-600`): `request(EditorDiagnostics)` → `request(VerifiedPreview)` on one revision. Same two-request shape; the product never issues `CompileIntent::Diagnostics` (0 hits in the playground) and the sampler cannot issue `EditorDiagnostics` (0 hits in `boon_cli`), so the product's exact sequence is not measurable with today's tools | partially confirmed |
| "The playground shows diagnostics only after the whole verified preview" (§2, §3.5) | Code reading, not a runtime measurement: one `CompileOutcome { language, result }` is sent after both requests (`compile.rs:373`, `384-389`); the dev window receives the language snapshot only from that outcome (`preview.rs:1493-1508` → `desktop.rs:289` → `dev.rs:460-461` → `ui.rs:787-790`) | confirmed (by code) |

The plan's ranges sit in the centre of the measured 30-sample bands; the plan's lower bounds
(372 / 1,151) are 5-10 ms below my observed minima (376.9 / 1,162.0), inside run-to-run noise.

## What `cargo xtask verify-compiler-interactions` does

Source: `crates/xtask/src/compiler_interactions.rs` (2,612 lines), `crates/xtask/src/main.rs`,
`budgets/compiler.toml` `[protocol]` and `[warm]`, `crates/boon_cli/src/compiler_sample.rs`
`warm_session_sample` (lines 967-1391).

- **Command form** (`main.rs:346`):
  `verify-compiler-interactions [--check-existing] [--report <path>] [--setup-samples N] [--scored-samples N]`.
  There is no environment variable; the only knobs are the two flags. `--check-existing`
  re-validates an existing report without collecting.
- **Sample counts.** Defaults come from `[protocol] setup_samples = 3`, `scored_samples = 30`.
  `validate_effective_samples` (`:2419-2429`) rejects only `scored == 0` and
  `setup + scored > 128` (`MAX_SAMPLE_COUNT`, `:25`), so the smallest run is
  `--setup-samples 0 --scored-samples 1`. Any non-default pair is stamped
  `run_classification = development-non-acceptance` (`classification`, `:2405-2416`).
- **Producers.** It runs the prebuilt `target/release/boon_cli` (product lane) and
  `target/release/boon_cli_evidence` (allocation-counting evidence lane); it never builds them.
  `require_current_prebuilt_producer` (`compiler_work_sample.rs:8-49`) fails if any file under
  `crates/` (except `xtask`), `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo` or
  `third_party/mimalloc-3.5.0` is newer than the binary. No such file existed.
- **Warm workload** (`run_warm_batch`, `:674-704`): one process per lane, env
  `RAYON_NUM_THREADS=1`, `BOON_KERNEL_EXPERIMENTAL_PARALLEL` removed, running
  `boon_cli compiler-sample warm-session examples/todo_mvc_physical/RUN.bn --switch-source examples/counter.bn --edit-unit examples/todo_mvc_physical/RUN.bn --edit-from 'TEXT { Walk the dog }' --edit-to 'TEXT { Walk the dogs }' --setup-samples N --scored-samples N`.
  Inside (`compiler_sample.rs:1074-1195`, the edit loop): the session opens TodoMVC and counter, compiles both
  to `VerifiedPreview`, then for each of the `setup + scored` edits it toggles the string
  forward/reverse and measures, on the same synchronous `CompilerSession` and the same revision:
  `apply_update` (update_ack), `request(CompileIntent::Diagnostics)` (`:1094-1099`, →
  `edit_to_diagnostics_ms`), then `request(CompileIntent::VerifiedPreview)` (`:1133-1138`, →
  `edit_to_verified_preview_ms`, measured from the edit start, so it *includes* the diagnostics
  request). Then `setup + scored` loaded switches between the two projects (`last_verified`
  lookup only), one pre-canceled request (`token.cancel()` before `request`), and one
  stale-revision request followed by a latest-revision `VerifiedPreview`.
- **Scaling workloads.** After warm, six synthetic scaling dimensions, each at sizes
  0/32/64, each with `setup + scored` product/evidence process pairs (fresh process per
  observation), plus one trace run for `dependency-cone-size`. At 0/1 that is 39 processes.
- **Status.** `warm_evaluation` (`:952-1016`) computes 15 measured flags and then hard-codes:

  ```rust
  // crates/xtask/src/compiler_interactions.rs:1010-1014
          // A pre-canceled synchronous call is useful preflight evidence but is
          // not the planned "generation superseded while working" gate.
          full_cancellation_gate_pass: false,
          native_present_evidence: "not-measured-compiler-only".to_owned(),
          native_present_gate_pass: false,
  ```

  `warm_status` (`:1018-1040`) ANDs all flags including these two (`:1034-1035`
  `&& evaluation.full_cancellation_gate_pass && evaluation.native_present_gate_pass;`), so the
  warm status cannot be `pass`. `in_flight_supersession_supported` is also always `false`,
  set by the producer (`compiler_sample.rs:1277-1280`: "CompilerSession is synchronous and
  exposes no worker queue or generation handle"). Independently, `collect` always pushes two
  strings into `missing_acceptance_evidence` right after the before/after identity check
  (`:623-628`: "native-presented-frame: compiler-only harness cannot replace app-owned
  WGPU readback", "in-flight-supersession: synchronous CompilerSession exposes no worker
  generation handle") and `aggregate_status` (`:2361-2375`) requires that list to be empty.
  The tool then prints `verification wrote a valid fail report` and exits 1 (`main.rs:53`).
- **Run time.** 0/1: 13.75 s and 13.98 s wall (two runs). The direct warm session alone is
  46.5 s at 3+30, so the default 3/30 protocol would be about 93 s of warm plus roughly
  1,191 scaling processes at ~0.16 s each, i.e. an estimated 4-5 minutes (not run).
- **Identity gotcha (my error, not the tool's).** The report path must not be under an
  untracked directory: `current_source_identity` (`report_v2.rs`) hashes the *contents* of
  every untracked file, and `run` re-validates the identity after writing the report
  (`validate_existing`, `:510`). My first run wrote the report into this (untracked) review
  directory and the tool reported `compiler interaction report source/tool identity is stale`
  after a complete, valid collection (report intact, 830,029 bytes, kept as
  `raw/warm/compiler-interactions-setup0-scored1-run1-stale-revalidation.json`). The second
  run wrote to the scratchpad and finished normally.

## Results

### 1. `cargo xtask verify-compiler-interactions --setup-samples 0 --scored-samples 1`

Command (from `tools/run_warm.sh 0 1`, log `raw/warm/run-setup0-scored1.log`):

```
timeout 1500 cargo xtask verify-compiler-interactions --setup-samples 0 --scored-samples 1 --report /tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/measure/warm/compiler-interactions-setup0-scored1.json
```

23:56:25 → 23:56:39 CEST, wall 13.98 s (user 13.45 s), uptime load 1.09 before / 1.43 after.
Tool output: `wrote compiler interactions ...: warm fail, 6 scaling dimensions, aggregate fail
(missing native-presentation and in-flight-supersession evidence)`, then `xtask: verification
wrote a valid fail report`, exit 1. Report copied to
`raw/warm/compiler-interactions-setup0-scored1.json` (830,157 bytes; extract with
`tools/warm_extract.py <report> --edits`).

| metric (n = 1, so p50 = p95 = max) | product lane (`boon_cli`) | evidence lane (`boon_cli_evidence`) | budget `[warm]` |
| --- | ---: | ---: | ---: |
| edit → diagnostics ms | 380.08 | 379.4 | p95 16.7 / p99 25 / max 33.4 |
| edit → verified preview ms | 1,157.82 | 1,171.6 | p95 100 / max 200 |
| verified-preview request alone ms | 777.7 | 792.3 | |
| update ack ms | 0.0019 | 0.0024 | |
| loaded switch ack ms | 0.0003 | 0.0003 | p95 16.7 |
| loaded bundle lookup ms | 0.0001 | 0.0002 | max 1.0 |
| pre-canceled request stop latency ms | 0.0011 | 0.0007 | max 8.0 |
| latest-generation publish ms | 782.6 | 790.9 | |
| session peak RSS | 201,392 KiB (196.7 MiB) | 201,368 KiB | 512 MiB |
| compiler requests in session | 7 | 7 | |

Phase split of the single scored edit (product lane): diagnostics request parse 6.3 ms +
typecheck 371.3 ms; preview request parse 0.9, typecheck 433.4, semantic 161.0, ir_lower 2.8,
ir_validation 0.6, backend 85.5, plan_validation 90.7 ms.

Evaluation flags: `evidence_parity_pass`, `loaded_bundle_lookup_pass`, `switch_ack_pass`,
`switch_no_compile_pass`, `switch_no_allocation_pass`, `session_peak_rss_pass`,
`session_tail_growth_pass`, `pre_canceled_request_pass`, `latest_generation_pass` = true;
`diagnostics_p95/p99/max_pass`, `verified_preview_p95/max_pass` = false (measured, 23× and
11.6× over budget); `in_flight_supersession_supported`, `full_cancellation_gate_pass`,
`native_present_gate_pass` = false (structural). All six scaling dimensions: `pass`
(owning-work doubling ratio 2.0 each; base/doubled p50 ms: call-depth 3.2/4.4,
call-site-count 2.5/2.8, contextual-call-site-count 16.0/28.4, static-branch-count 11.0/16.7,
source-unit-count 3.2/4.6, dependency-cone-size 11.2/18.0).

First (identity-stale) run, 23:53:43 → 23:53:57, wall 13.75 s, same collection: product
379.5 / 1,156.3 ms (preview request 776.8), evidence 375.8 / 1,157.3; cancellation 0.0006 ms;
latest-generation publish 787.5 ms.

### 2. Direct warm session, 3 setup + 30 scored, HEAD binary

Command (from `tools/run_warm_series.sh`, log `raw/warm/warm-session-head-3-30.log`):

```
env -u BOON_KERNEL_EXPERIMENTAL_PARALLEL RAYON_NUM_THREADS=1 target/release/boon_cli compiler-sample warm-session examples/todo_mvc_physical/RUN.bn --switch-source examples/counter.bn --edit-unit examples/todo_mvc_physical/RUN.bn --edit-from 'TEXT { Walk the dog }' --edit-to 'TEXT { Walk the dogs }' --setup-samples 3 --scored-samples 30 > raw/warm/warm-session-head-3-30.json
```

23:56:39 → 23:57:26, wall 46.67 s, load 1.43 → 1.48. Extract with
`tools/warm_session_extract.py raw/warm/warm-session-head-3-30.json --edits`.

| scored metric (n = 30) | min | median | p95 | p99 | max |
| --- | ---: | ---: | ---: | ---: | ---: |
| edit → diagnostics ms | 382.0 | 386.8 | 408.5 | 455.2 | 455.2 |
| edit → verified preview ms | 1,173.2 | 1,192.5 | 1,249.2 | 1,393.3 | 1,393.3 |
| verified-preview request alone ms | 789.5 | 801.8 | 851.6 | 938.1 | 938.1 |
| forward edits, edit → diagnostics (n = 15) | 383.6 | 388.4 | 405.7 | | 408.5 |
| reverse edits, edit → diagnostics (n = 15) | 382.0 | 386.3 | 402.6 | | 455.2 |
| loaded switch ack ms | 0.0 | 0.0 | 0.0 | | 0.0 |

Setup samples: 380.2 / 380.4 / 383.4 ms (diagnostics); 1,164.4 / 1,171.6 / 1,183.7 ms (preview).
Phase medians: diagnostics parse 6.7 + typecheck 377.2; preview parse 1.0, typecheck 446.3,
semantic 166.1, ir_lower 3.1, ir_validation 0.7, backend 89.4, plan_validation 90.5. Work
counters: the diagnostics request re-parses 1 of 8 units and reuses 7; the preview request
reuses all 8 but re-runs the whole typecheck (`has_fully_reused_preview_frontend_work`,
`compiler_interactions.rs`, requires `typecheck.owner_* > 0`). One distinct forward plan hash
across 15 forward edits. 71 compiler requests; RSS 166,516 → 168,248 KiB, peak 203,384 KiB
(198.6 MiB). Cancellation stop 0.0008 ms; latest-generation publish 806.4 ms; switches: no
compile requests, 0 allocation calls, 0 bytes.

### 3. Direct warm session, 3 + 30, sep28 binary (the build the plan probably used)

Same command with `/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/bin/boon_cli.sep28`
(sha256 `64aee717f253…`, `build_source_head 286aa974`, dirty). 23:57:26 → 23:58:13, wall 46.47 s,
load 1.48 → 1.53. Output `raw/warm/warm-session-sep28-3-30.json`.

| scored metric (n = 30) | min | median | p95 | p99 | max |
| --- | ---: | ---: | ---: | ---: | ---: |
| edit → diagnostics ms | 376.9 | 389.5 | 405.0 | 410.1 | 410.1 |
| edit → verified preview ms | 1,162.0 | 1,193.1 | 1,231.0 | 1,277.4 | 1,277.4 |
| verified-preview request alone ms | 784.2 | 802.7 | 829.0 | 867.4 | 867.4 |

Phase medians: diagnostics 6.7 + 379.9; preview typecheck 447.3, semantic 166.1, backend 89.4,
plan_validation 90.8. Cancellation 0.0013 ms; latest-generation publish 790.7 ms. HEAD and
sep28 are the same compiler on this workload (medians within 0.7%).

### 4. Cold reference in the same series, HEAD binary

Command (5 one-sample processes per intent; the sampler refuses `--samples 5` in a cold mode
with "compiler cold observations require exactly one sample per producer process"), log
`raw/warm/cold-ref.log`, 23:59:21 → 23:59:29, load 2.20 → 2.26:

```
env -u BOON_KERNEL_EXPERIMENTAL_PARALLEL RAYON_NUM_THREADS=1 target/release/boon_cli compiler-sample examples/todo_mvc_physical/RUN.bn --intent diagnostics --mode empty-session --samples 1 > raw/warm/cold-ref-empty-session-diagnostics-<i>.json
env -u BOON_KERNEL_EXPERIMENTAL_PARALLEL RAYON_NUM_THREADS=1 target/release/boon_cli compiler-sample examples/todo_mvc_physical/RUN.bn --intent verified --mode empty-session --samples 1 > raw/warm/cold-ref-empty-session-verified-<i>.json
```

| cold empty-session (n = 5) | values ms | min | median | max | phase medians |
| --- | --- | ---: | ---: | ---: | --- |
| diagnostics | 393.2, 389.6, 384.7, 383.4, 385.8 | 383.4 | 385.8 | 393.2 | parse 17.1, typecheck 363.7 |
| verified | 779.7, 782.7, 779.4, 788.3, 794.0 | 779.4 | 782.7 | 794.0 | parse 16.8, typecheck 428.1, semantic 156.2, ir_lower 2.7, ir_validation 0.6, backend 82.3, plan_validation 90.7 |

The sibling `baselines.md` (10 fresh-process samples, 23:17-23:20) agrees: TodoMVC diagnostics
p50 407.8 ms fresh / ~399 ms empty-session, verified 810.1 / ~797 ms.

### Warm vs cold

| | cold (empty-session, median) | warm (3+30 HEAD, median) | ratio |
| --- | ---: | ---: | ---: |
| diagnostics | 385.8 ms | edit → diagnostics 386.8 ms | 1.00 |
| verified | 782.7 ms | verified-preview request alone 801.8 ms | 1.02 |
| verified | 782.7 ms | edit → preview (diagnostics + preview) 1,192.5 ms | 1.52 |

Warm reuse saves about 10 ms of parse per request (17 → 7 ms diagnostics, 17 → 1 ms preview)
and nothing else; typecheck, semantic, backend and plan validation are all re-run per request,
and the typecheck is 13-18 ms slower warm than cold. "Warm is not faster than cold" is exact.

## Where the playground publishes diagnostics

Read, not executed (no playground was launched). Chain for a built-in single-role preview:

1. `crates/boon_native_playground/src/compile.rs:537-541` - `compile_built_in` issues
   `self.session.request(project, revision, CompileIntent::EditorDiagnostics, cancellation)`
   and projects the result into the `language` out-parameter (`:585-586`).
2. `compile.rs:594-600` - the same function then issues
   `self.session.request(project, revision, CompileIntent::VerifiedPreview, cancellation)` and
   returns the plan; nothing is sent between the two requests.
3. `compile.rs:373` - the worker loop: `let CompileAttempt { language, result } = compiler.compile(request, &cancellation);`
4. `compile.rs:384-389` - one message after both requests:
   `.unbounded_send(CompileOutcome { job_id, revision, language, result })`.
5. `preview.rs:1493-1508` - `Wake::Compiled(outcome)`: only here
   `output.send(Message::PreviewLanguageSnapshot { snapshot })?;` (`:1508`) is sent to the
   desktop process, i.e. once per completed verified compile.
6. `desktop.rs:289` - `(Role::Preview, message @ Message::PreviewLanguageSnapshot { .. })` is
   forwarded to the dev window; `dev.rs:460-461` - `Message::PreviewLanguageSnapshot { snapshot } => { if model.accept_language_project(snapshot) {`;
   `ui.rs:787-790` renders `language.diagnostics_text()` into the "Diagnostics" inspector field.

So diagnostics reach the editor after `EditorDiagnostics` + `VerifiedPreview` have both
finished, i.e. after the whole verified preview, exactly as the plan says. The only exception is
a parse error: `compile.rs:544-562` projects the parser diagnostic into `language` and returns
`Err` before the preview request, but it is still published through the same single outcome.

`CompileIntent` (`crates/boon_compiler/src/session.rs:42-49`) has `Diagnostics`,
`EditorDiagnostics` ("Complete checked rows and editor-facing type/presentation tables without
sealing an executable artifact. Callers must opt into this larger root."), `VerifiedCheck`,
`VerifiedPreview`, `Handoff`. The benchmark uses `Diagnostics`; the product uses
`EditorDiagnostics`; `rg EditorDiagnostics crates/boon_cli/src` has no hits, so the product's
request cannot be sampled with today's CLI. The product's edit → diagnostics latency is therefore
at least the benchmark's 380 ms and, since `EditorDiagnostics` is a larger root and the
snapshot is only published after the preview, in practice the ~1.16-1.19 s edit → preview
figure.

## Files

- `raw/warm/compiler-interactions-setup0-scored1.json` (830,157 B) and `run-setup0-scored1.log` - clean xtask run.
- `raw/warm/compiler-interactions-setup0-scored1-run1-stale-revalidation.json` (830,029 B) and `run-setup0-scored1-run1-stale-revalidation.log` - first run, valid collection, exit 1 on post-write identity re-validation.
- `raw/warm/warm-session-head-3-30.json` / `.log`, `raw/warm/warm-session-sep28-3-30.json` / `.log` - direct 3+30 warm sessions.
- `raw/warm/cold-ref-empty-session-{diagnostics,verified}-{1..5}.json`, `raw/warm/cold-ref.log` - cold reference.
- `raw/warm/series.log`, `raw/warm/uptime_00_pre_build.txt` (load 1.84 at 23:52:16), `raw/warm/uptime_99_post_series.txt` (load 1.99 at 00:00:20), `raw/warm/xtask_build.log`.
- `tools/run_warm.sh`, `tools/run_warm_series.sh`, `tools/warm_extract.py`, `tools/warm_session_extract.py`.

## Caveats

- The xtask run is a single scored sample by design (the smallest count the tool accepts); the
  30-sample distributions come from the same producer command the xtask spawns, run directly,
  not from the xtask, so they carry no xtask validation or scaling data.
- The machine was shared (load 1.1-2.3); the 455 ms and 1,393 ms maxima in the HEAD 3+30 run
  are one outlier edit (seq 13) and are not reproduced by the sep28 run (max 410 / 1,277).
- The playground publication order is established by reading the code; no playground process
  was launched and no runtime timestamps were taken.
- `--report` must point outside untracked directories; otherwise the tool's post-write
  re-validation fails after a complete collection (see "Identity gotcha").
- The default 3/30 xtask protocol was not run; its 4-5 minute duration is an estimate from the
  0/1 run and the direct 3+30 warm sessions.
