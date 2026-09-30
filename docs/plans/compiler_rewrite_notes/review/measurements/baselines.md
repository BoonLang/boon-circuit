# Baseline re-measurement of BOON_COMPILER_REWRITE_PLAN.md (§0 and §2)

Measured 2026-09-29 23:17-23:20 CEST on the i7-9700K host (8 cores, 48 GB, Linux 6.18.7,
`perf_event_paranoid=1`, untouched). Measurement agent of the independent plan review.
No samply run here. Nothing outside this directory and the scratchpad was written; no build was run.

## Verdict in one line

Every baseline number in §0 and the §2 table reproduces on the HEAD release binary within
10% (the largest deviation is NovyWave diagnostics, +5% on the median). The allocation figure
reproduces to the byte. The old sep28 build and the HEAD build are the same compiler for
these fixtures: identical plan hashes, identical diagnostics fingerprints, identical work
counters, and timings that differ by 0-2%, inside the noise band.

## What was run

### Binaries

| lane | path | sha256 | `build_source_head` | dirty | allocator / instrumentation |
| --- | --- | --- | --- | --- | --- |
| HEAD | `/home/martinkavik/repos/boon-circuit/target/release/boon_cli` (mtime 2026-09-29 19:10:34) | `f87b12074c83…` | `a60a11d6` | clean | mimalloc / none |
| sep28 | `/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/bin/boon_cli.sep28` | `64aee717f253…` | `286aa974` | **dirty** | mimalloc / none |
| evidence | `/home/martinkavik/repos/boon-circuit/target/release/boon_cli_evidence` (mtime 2026-09-29 19:10:34) | `400c35626fdb…` | `a60a11d6` | clean | mimalloc / thread-local Rust global allocator counters |

`286aa974` is 17 commits before `a60a11d6`; `e36b2c24` ("kernel: memoize summary support")
sits between them. The sep28 binary was built from a dirty tree, and its kernel work counters
(`kernel_solve.summary_node_evaluations`, `scheduled_work_items`, every field of `work`) are
bit-identical to HEAD's on TodoMVC verified, so it already contained the e36b2c24 edits
uncommitted. Treat the two lanes as the same compiler; the A/B/A interleave measures the
machine's noise, not a code change.

### How `compiler-sample` measures (from `crates/boon_cli/src/compiler_sample.rs`)

- `--samples` must be exactly 1 (line 818: "compiler cold observations require exactly one
  sample per producer process"); the protocol in `budgets/compiler.toml` is
  `sample_process_isolation = "one-process-per-observation"`. So "10 samples" here means 10
  process launches per lane, interleaved HEAD, sep28, HEAD, sep28, … per sample index.
- `--mode fresh-process` does **not** spawn a child. It calls `compile_diagnostics_source`
  (diagnostics) or `check_runtime_source` + `finish_checked_sealed_machine_plan` (verified)
  directly in the freshly launched process. `--mode empty-session` instead builds a
  `CompilerSession`, opens the project and issues `CompileIntent::Diagnostics` /
  `CompileIntent::VerifiedPreview`; the timer includes `compiler_source_project_for_path`
  and `open_project`.
- Output: one JSON object on stdout (`format_version` 10) with producer metadata, then a
  single sample: `elapsed_ms` (wall, `Instant`), `compiler_cpu_ms` (`getrusage` user+sys),
  minor/major page faults, `peak_rss_kib` (`VmHWM` from `/proc/self/status`, read right after
  the compiler artifact and before the plan is re-serialized for its sha), the
  `phase` block (`parse_ms`, `typecheck_ms`, `semantic_ms`, `contract_verify_ms`,
  `ir_lower_ms`, `ir_validation_ms`, `backend_ms`, `plan_validation_ms`, all from the
  compiler's own profile), `work` counters (parser, typecheck, kernel compile, kernel solve),
  and `allocations` (zeros on the product binary; real counters only on `boon_cli_evidence`).
  No percentile is computed by the tool; min/median/p95 below are computed by
  `tools/summarize_baselines.py` over the 10 (or 5, or 3) files.
- `boon_cli check` issues `CompileIntent::VerifiedCheck` through a `CompilerSession`, so it is
  a verified compile, not a diagnostics run. `boon_cli dump-plan --out` runs
  `compile_machine_plan` and then `serde_json::to_vec_pretty` on the plan. Neither is a
  diagnostics-lane RSS measurement; the diagnostics-lane RSS is `compiler-sample`'s
  `peak_rss_kib` in the diagnostics rows.

### Commands (exact lines are in `raw/baselines/commands_*.log`)

All from `/home/martinkavik/repos/boon-circuit`:

```
tools/run_baselines.sh fresh-process 10 counter todomvc novywave
  # per fixture/intent/index i, in this order: HEAD then sep28
  # <bin> compiler-sample <src> --intent <diagnostics|verified> --mode fresh-process --samples 1 > raw/baselines/<lane>_<fixture>_<intent>_fresh-process_<i>.json
tools/run_baselines.sh empty-session 5 todomvc novywave
  # same, --mode empty-session
tools/run_rss.sh
  # /usr/bin/time -v target/release/boon_cli check <src>            > raw/baselines/time_check_<fixture>_<i>.txt.stdout 2> raw/baselines/time_check_<fixture>_<i>.txt
  # /usr/bin/time -v target/release/boon_cli dump-plan <src> --out <scratchpad>/measure/plan.bin   (same naming, time_dump-plan_…)
tools/run_alloc.sh
  # target/release/boon_cli_evidence compiler-sample <src> --intent <intent> --mode fresh-process --samples 1 > raw/baselines/evidence_<fixture>_<intent>_fresh-process_<i>.json
python3 tools/summarize_baselines.py    # -> raw/baselines/summary.json and the tables below
```

Sources: `examples/counter.bn` (140 lines), `examples/todo_mvc_physical/RUN.bn`
(3,576 compiler-input lines per `budgets/compiler.toml`; `wc -l` of RUN.bn alone is 1,117),
`examples/novywave/RUN.bn` (11,926 compiler-input lines; RUN.bn alone 4,949). Throughput uses
the compiler-input line counts, as the plan does.

### Machine load (`uptime`, load average 1/5/15 min)

| series | before | after |
| --- | --- | --- |
| fresh-process, 10 × 2 lanes × 3 fixtures × 2 intents (120 processes, 94 s) | 23:17:18 — 1.27, 1.44, 1.55 | 23:18:52 — 1.90, 1.61, 1.60 |
| empty-session, 5 × 2 lanes × 2 fixtures × 2 intents (40 processes, 44 s) | 23:18:59 — 1.61, 1.56, 1.59 | 23:19:44 — 2.36, 1.81, 1.67 |
| `/usr/bin/time -v` check + dump-plan, 3 × 3 fixtures (18 processes, 17 s) | 23:19:52 — 2.17, 1.78, 1.66 | 23:20:08 — 2.97, 1.97, 1.73 |
| allocation evidence, 3 × 3 fixtures × 2 intents (18 processes, 14 s) | 23:20:08 — 2.97, 1.97, 1.73 | 23:20:22 — 2.84, 1.99, 1.74 |

The load average rises during each series mostly because of the series itself (one busy core).
For every series `compiler_cpu_ms` equals `elapsed_ms` to within 0.1 ms at the median and the
minimum, so the compiler was never descheduled; the min-to-p95 spread (2-8%) is frequency,
cache and memory-placement noise, not preemption. Statistics: min, median, p95 (linear
interpolation at rank 0.95·(n−1)), max over all launches including the first (no discarded
warm-up launches, unlike the protocol's 3 setup + 30 scored).

## Results

### Cold totals (fresh-process, one process per observation, ms)

| fixture | intent | plan | HEAD min / median / p95 (max) | HEAD verdict | sep28 min / median / p95 (max) | sep28 verdict |
| --- | --- | ---: | ---: | --- | ---: | --- |
| counter | diagnostics | 7 | 7.1 / 7.3 / 8.7 (9.3) | confirmed | 6.9 / 7.2 / 8.7 (9.1) | confirmed |
| counter | verified | 16 | 15.9 / 16.6 / 18.6 (19.5) | confirmed | 15.8 / 16.6 / 19.3 (20.8) | confirmed |
| todomvc | diagnostics | 390 | 386.2 / 407.8 / 419.8 (424.7) | confirmed | 386.5 / 404.2 / 437.7 (444.4) | confirmed |
| todomvc | verified | 790 | 788.9 / 810.1 / 845.8 (851.9) | confirmed | 790.3 / 806.3 / 819.9 (824.5) | confirmed |
| novywave | diagnostics | 526 | 537.4 / 553.2 / 572.7 (580.6) | confirmed | 527.4 / 543.5 / 572.2 (585.6) | confirmed |
| novywave | verified | 1870 | 1,883.2 / 1,912.8 / 2,033.6 (2,065.0) | confirmed | 1,859.3 / 1,887.5 / 2,157.4 (2,195.2) | confirmed |

### Per-phase medians (fresh-process, ms; plan value / HEAD median / sep28 median, verdict on HEAD)

| fixture | intent | parse | typecheck | semantic | contract_verify | IR (lower+validation) | backend | plan validation | phase sum vs total |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| counter | diagnostics | 3.5 / 3.67 / 3.62 (confirmed) | 3.4 / 3.53 / 3.52 (confirmed) | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | 7.2 vs 7.3 |
| counter | verified | 3.7 / 3.69 / 3.55 (confirmed) | 6.6 / 6.79 / 7.10 (confirmed) | 4.8 / 5.01 / 4.97 (confirmed) | — / 0.04 / 0.04 | 0.1 / 0.09 / 0.10 (lower) | 0.4 / 0.43 / 0.44 (confirmed) | 0.4 / 0.45 / 0.46 (higher) | 16.5 vs 16.6 |
| todomvc | diagnostics | 18 / 18.23 / 18.12 (confirmed) | 370 / 385.51 / 382.78 (confirmed) | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | 404.4 vs 407.8 |
| todomvc | verified | 18 / 17.95 / 17.68 (confirmed) | 435 / 442.75 / 442.46 (confirmed) | 158 / 163.29 / 162.22 (confirmed) | — / 0.04 / 0.05 | 3 / 3.43 / 3.50 (higher) | 85 / 86.44 / 87.48 (confirmed) | 90 / 92.05 / 91.45 (confirmed) | 808.3 vs 810.1 |
| novywave | diagnostics | 64 / 66.31 / 66.19 (confirmed) | 457 / 479.19 / 471.02 (confirmed) | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | — / 0.00 / 0.00 | 547.8 vs 553.2 |
| novywave | verified | 64 / 65.83 / 66.08 (confirmed) | 713 / 732.41 / 726.36 (confirmed) | 780 / 797.17 / 788.24 (confirmed) | — / 0.05 / 0.05 | 25 / 24.93 / 25.81 (confirmed) | 214 / 219.05 / 217.78 (confirmed) | 66 / 65.81 / 66.31 (confirmed) | 1,907.2 vs 1,912.8 |

### Empty-session (in-process CompilerSession) vs fresh-process, medians (ms)

| fixture | intent | HEAD fresh | HEAD session (min/med/p95) | sep28 fresh | sep28 session (min/med/p95) |
| --- | --- | ---: | ---: | ---: | ---: |
| todomvc | diagnostics | 407.8 | 402.8 / 411.0 / 428.4 | 404.2 | 396.2 / 398.9 / 421.5 |
| todomvc | verified | 810.1 | 801.7 / 805.9 / 836.2 | 806.3 | 793.2 / 797.4 / 842.2 |
| novywave | diagnostics | 553.2 | 539.0 / 562.7 / 577.6 | 543.5 | 533.6 / 555.6 / 589.9 |
| novywave | verified | 1,912.8 | 1,885.3 / 1,919.9 / 1,978.8 | 1,887.5 | 1,896.4 / 1,935.0 / 1,975.7 |

### Peak RSS

| fixture | plan (MiB, verified) | compiler-sample verified HEAD VmHWM min/med/max (MiB) | compiler-sample diagnostics HEAD (MiB) | time -v check max RSS min/med/max (MiB) | time -v dump-plan --out (MiB) | verdict (plan vs medians) |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| counter | — | 39.2 / 39.4 / 39.5 | 26.1 / 26.2 / 26.2 | 39.2 / 39.2 / 39.6 | 38.1 / 38.1 / 38.2 | n/a |
| todomvc | 154 | 161.7 / 161.9 / 161.9 | 132.2 / 132.3 / 132.4 | 158.1 / 158.1 / 158.2 | 152.5 / 152.5 / 152.7 | sample confirmed, dump-plan confirmed, check confirmed |
| novywave | 314 | 315.8 / 315.9 / 315.9 | 176.2 / 176.3 / 176.5 | 368.1 / 368.2 / 368.3 | 312.5 / 314.5 / 314.6 | sample confirmed, dump-plan confirmed, check higher |

### Allocation evidence (target/release/boon_cli_evidence, fresh-process, 3 runs)

| fixture | intent | plan | allocation calls min/med/max | allocated bytes min/med/max | elapsed median ms | verdict |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| counter | diagnostics | — | 0.037 / 0.037 / 0.037 M | 0.006 / 0.006 / 0.006 GB | 7.3 | n/a |
| counter | verified | — | 0.058 / 0.058 / 0.058 M | 0.012 / 0.012 / 0.012 GB | 16.4 | n/a |
| todomvc | diagnostics | — | 0.979 / 0.979 / 0.979 M | 0.246 / 0.246 / 0.246 GB | 389.8 | n/a |
| todomvc | verified | — | 2.704 / 2.704 / 2.704 M | 0.607 / 0.607 / 0.607 GB | 814.4 | n/a |
| novywave | diagnostics | — | 2.419 / 2.419 / 2.419 M | 0.468 / 0.468 / 0.468 GB | 595.5 | n/a |
| novywave | verified | 7.46 M / 1.33 GB | 7.458 / 7.458 / 7.458 M | 1.329 / 1.329 / 1.329 GB | 1,915.8 | calls confirmed, bytes confirmed |

### Throughput (compiler_input_source_lines / HEAD median, fresh-process)

| fixture | lines | diagnostics k lines/s (HEAD / sep28) | verified k lines/s (HEAD / sep28) | parse k lines/s from verified parse_ms (HEAD / sep28) |
| --- | ---: | ---: | ---: | ---: |
| counter | 140 | 19.2 / 19.4 | 8.5 / 8.4 | 38 / 39 |
| todomvc | 3,576 | 8.8 / 8.8 | 4.4 / 4.4 | 199 / 202 |
| novywave | 11,926 | 21.6 / 21.9 | 6.2 / 6.3 | 181 / 180 |

### Work counters (HEAD verified, fresh-process)

| fixture | checked_calls | compiled_call_sites | HEAD plan_sha256 (stable across samples?) | sep28 plan_sha256 | budgets/compiler.toml oracle | HEAD == sep28 | HEAD == oracle |
| --- | ---: | ---: | --- | --- | --- | --- | --- |
| counter | 18 | 11 | 5158476e5e9f095e… (stable) | 5158476e5e9f095e… | 5158476e5e9f095e… | yes | yes |
| todomvc | 352 | 10,537 | 0befc7f89cdeb6d6… (stable) | 0befc7f89cdeb6d6… | 8e7120c33db1d5a5… | yes | NO |
| novywave | 1,821 | 2,699 | 5926de7bed188b50… (stable) | 5926de7bed188b50… | e2d673e3116f0362… | yes | NO |

### Diagnostics fingerprints (HEAD vs sep28, fresh-process)

- counter: HEAD d5df263914da90f1… (1 distinct), sep28 d5df263914da90f1… (1 distinct), equal: yes
- todomvc: HEAD b0bf7b1700ec2233… (1 distinct), sep28 b0bf7b1700ec2233… (1 distinct), equal: yes
- novywave: HEAD 6f5d4bd58c810982… (1 distinct), sep28 6f5d4bd58c810982… (1 distinct), equal: yes

### Per-sample elapsed (ms), fresh-process, in launch order (sample 00 is the first launch of that binary for that fixture/intent)

- head counter diagnostics: [7.95, 9.3, 7.28, 7.27, 7.11, 7.1, 7.17, 7.18, 7.97, 7.66]
- old counter diagnostics: [7.67, 7.61, 7.27, 7.11, 6.96, 6.95, 7.12, 7.01, 8.31, 9.1]
- head counter verified: [17.55, 16.46, 16.99, 17.0, 19.5, 15.96, 16.16, 16.66, 16.46, 15.93]
- old counter verified: [20.78, 16.87, 16.73, 17.08, 16.36, 17.38, 16.43, 15.87, 15.83, 16.15]
- head todomvc diagnostics: [424.66, 409.04, 408.61, 411.62, 413.86, 395.08, 406.92, 393.33, 388.96, 386.18]
- old todomvc diagnostics: [444.4, 429.61, 421.21, 420.04, 402.06, 403.74, 404.6, 386.51, 391.69, 388.64]
- head todomvc verified: [811.3, 788.93, 792.28, 810.29, 809.85, 794.45, 801.46, 829.67, 838.43, 851.86]
- old todomvc verified: [814.19, 790.33, 810.5, 799.71, 807.81, 795.62, 799.84, 824.54, 804.8, 809.85]
- head novywave diagnostics: [552.98, 580.64, 553.52, 560.57, 537.44, 539.01, 561.12, 537.46, 562.98, 543.55]
- old novywave diagnostics: [585.63, 544.23, 542.71, 528.09, 527.41, 555.86, 542.37, 536.51, 546.62, 552.63]
- head novywave verified: [1909.37, 1995.28, 2065.0, 1960.58, 1951.55, 1883.17, 1887.05, 1907.57, 1916.24, 1891.5]
- old novywave verified: [2003.97, 2195.15, 2111.31, 1923.79, 1885.93, 1881.07, 1869.18, 1880.13, 1889.1, 1859.34]

## Verdict per plan number (HEAD median vs plan; "confirmed" = within ±10%)

| plan claim | plan | measured (HEAD, fresh-process, min / median / p95) | verdict |
| --- | ---: | ---: | --- |
| §0/§2 counter diagnostics | 7 ms | 7.1 / 7.3 / 8.7 ms | confirmed (+4%) |
| §0/§2 counter verified | 16 ms | 15.9 / 16.6 / 18.6 ms | confirmed (+4%) |
| §0/§2 TodoMVC diagnostics | 390 ms | 386.2 / 407.8 / 419.8 ms | confirmed (+5%) |
| §0/§2 TodoMVC verified | 790 ms | 788.9 / 810.1 / 845.8 ms | confirmed (+3%) |
| §0/§2 NovyWave diagnostics | 526 ms | 537.4 / 553.2 / 572.7 ms | confirmed (+5%) |
| §0/§2 NovyWave verified | 1,870 ms | 1,883.2 / 1,912.8 / 2,033.6 ms | confirmed (+2%) |
| §2 counter phases (diag: parse 3.5, typecheck 3.4) | | 3.67 / 3.53 | confirmed |
| §2 counter phases (verified: 3.7 / 6.6 / 4.8 / 0.1 / 0.4 / 0.4) | | 3.69 / 6.79 / 5.01 / 0.09 / 0.43 / 0.45 | confirmed; IR and plan-validation differ only in the second decimal of a one-significant-figure plan value |
| §2 TodoMVC phases (diag: 18 / 370) | | 18.23 / 385.51 | confirmed (typecheck +4%) |
| §2 TodoMVC phases (verified: 18 / 435 / 158 / 3 / 85 / 90) | | 17.95 / 442.75 / 163.29 / 3.43 / 86.44 / 92.05 | confirmed; IR 3.43 vs "3" is +14% as lower+validation (ir_lower 2.82 + ir_validation 0.61), and ir_lower alone rounds to 3; on NovyWave the plan's 25 equals lower+validation (17.58 + 7.37), so the plan's IR column is the sum and the TodoMVC cell is a one-significant-figure rounding |
| §2 NovyWave phases (diag: 64 / 457) | | 66.31 / 479.19 | confirmed (typecheck +5%) |
| §2 NovyWave phases (verified: 64 / 713 / 780 / 25 / 214 / 66) | | 65.83 / 732.41 / 797.17 / 24.93 / 219.05 / 65.81 | confirmed |
| §0 parse NovyWave 64 ms | 64 | 65.8 (verified) / 66.3 (diagnostics) | confirmed (+3%) |
| §2 peak RSS TodoMVC 154 MiB (budget 128) | 154 | compiler-sample VmHWM 161.9; `time -v check` 158.1; `time -v dump-plan` 152.5 | confirmed (+5% / +3% / −1%); over the 128 MiB budget as the plan says |
| §0/§2 peak RSS NovyWave verified 314 MiB | 314 | compiler-sample VmHWM 315.9; `dump-plan` 314.5; `check` 368.2 | confirmed on the protocol scope and dump-plan; `boon_cli check` (session + VerifiedCheck) is 17% higher |
| §2 NovyWave verified allocations 7.46 M / 1.33 GB | 7.46 M / 1.33 GB | 7,457,514 calls / 1,329,184,184 bytes, identical on all 3 runs | confirmed (exact) |
| §2 diagnostics throughput 9-23 k lines/s | 9-23 k | counter 19.2, TodoMVC 8.8, NovyWave 21.6 k lines/s | confirmed (both ends within 6%) |
| §2 verified throughput 4.5-6.4 k lines/s | 4.5-6.4 k | TodoMVC 4.4, NovyWave 6.2 k lines/s (counter 8.5 k, outside the quoted range, evidently excluded) | confirmed for the two large fixtures |
| §2 parse ~190 k lines/s | ~190 k | TodoMVC 199 k, NovyWave 181 k (counter 38 k: the plan's §3.4 says 76% of counter's parse is manifest validation) | confirmed |
| §0 "350 checked calls become 10,537 compiled call sites" (TodoMVC) | 350 → 10,537 | `checked_calls` 352 → `compiled_call_sites` 10,537 (30.0×) | confirmed |
| §0 warm edit → diagnostics 372-397 ms, → preview 1,151-1,207 ms | | not measured here (warm-session benchmark is out of this task's scope) | not measured |

Old sep28 lane, for the record: 7.2 / 16.6 / 404.2 / 806.3 / 543.5 / 1,887.5 ms medians for the
same six rows; every verdict is the same. sep28 is 0-1.7% faster at the median, which is
smaller than the min-to-median gap within either lane, so it is noise.

## In-process (`empty-session`) vs fresh-process

In-process compiles through `CompilerSession` are not faster: TodoMVC diagnostics 411.0 vs
407.8 ms, verified 805.9 vs 810.1 ms; NovyWave diagnostics 562.7 vs 553.2 ms, verified 1,919.9
vs 1,912.8 ms (HEAD medians, 5 vs 10 launches). Differences are within 2%, so process startup
is not part of the cold cost, and the §2 numbers are the compiler's own time.

## Things worth knowing that the plan does not say

1. **The budget oracles are stale for TodoMVC and NovyWave.** Both binaries produce
   `machine_plan_sha256` `0befc7f8…` for TodoMVC and `5926de7b…` for NovyWave, stable across
   all 10 launches, while `budgets/compiler.toml` pins `8e7120c3…` and `e2d673e3…`. Counter
   matches its oracle (`5158476e…`). Either the plan format or lowering changed after the
   oracles were pinned, or the oracles were pinned from another build. `cargo xtask` oracle
   checks for those two fixtures would fail today. Not touched, as instructed.
2. **`boon_cli check` is not a diagnostics run.** It is a `VerifiedCheck` through a session and
   costs the full verified time (1.84 s, 368 MiB on NovyWave). Anyone using `check` to estimate
   the diagnostics lane gets the verified number.
3. **The diagnostics lane RSS** is 132 MiB on TodoMVC and 176 MiB on NovyWave (compiler-sample
   `peak_rss_kib`), which the plan does not quote; a 26 MiB floor is visible on counter.
4. **Allocation counts are fully deterministic** (three runs identical to the byte), so the
   7.46 M / 1.33 GB figure is a property of the build, not of the run.
5. **Sample 00 of each lane is often the slowest** (e.g. HEAD TodoMVC diagnostics 424.7 ms
   first, 386-414 after; sep28 NovyWave verified 2,004 and 2,195 ms in the first two launches).
   The protocol's 3 setup launches would remove that; here they are included, which inflates
   p95 and max but not the median.

## Files

- Report: `/home/martinkavik/repos/boon-circuit/docs/plans/compiler_rewrite_notes/review/measurements/baselines.md`
- Raw JSON and `time -v` outputs, command logs with uptime: `/home/martinkavik/repos/boon-circuit/docs/plans/compiler_rewrite_notes/review/measurements/raw/baselines/` (`summary.json` has every statistic and per-sample list)
- Scripts: `/home/martinkavik/repos/boon-circuit/docs/plans/compiler_rewrite_notes/review/measurements/tools/run_baselines.sh`, `run_rss.sh`, `run_alloc.sh`, `summarize_baselines.py`
