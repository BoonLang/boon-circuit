> **Review measurements, 2026-09-29/30. Not authority.** Independent re-measurement
> of the numbers in `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, taken for
> `../REVIEW.md` on the release binary built from a60a11d6 (HEAD of
> `compiler-rewrite-plan`) and, where noted, on the 2026-09-28 build the plan
> was probably measured with. Machine: i7-9700K, shared desktop (load 1-4).

| file | what it re-measures | verdict |
| --- | --- | --- |
| oracle.md | "check fails on 8 of 22 sources; 5 of 21 scenarios pass" | qualitatively confirmed; denominators wrong (20 examples / 22 sources / 20 scenarios); 8 of 15 scenario failures are compile rejections, 3 harness drift |
| parser_prototype.md | prototype parse 2.6 / 0.64 / 0.021 ms, 141 files accepted | confirmed within 6%; prototype scope is a lexer + arena parser only |
| baselines.md | §0/§2 timings, phase breakdown, RSS, allocations, throughput, 350 → 10,537 call sites | all confirmed within 10%; budgets plan-hash oracles stale |
| profile.md | SHA-256+CBOR 25/31/39%, incidental 34-46%, orchestration 30-70% outside solver, semantic receipts, backend predicate | shares confirmed; orchestration claim only partially (16-86% by fixture) |
| warm.md | warm 372-397 ms / 1.15 s, warm not faster than cold, status hard-coded to fail, diagnostics after preview | confirmed |

| effects_observed.md | today's effect re-run behaviour under a real host-service adapter (the notes had read it from code) | observed: THEN-body transient effects re-stage and cancel on every HOLD-argument update; arm-scoped effects never fire while the selector is open from the start |

Raw outputs of the timing series (about 8 MB: per-run JSON, samply summaries,
scenario logs) are kept outside the repository; every report lists the exact
commands, and `tools/` holds the runner and summarizer scripts that regenerate
them. `raw/` keeps only the small effect-harness logs, and
`tools/effects_harness/` the harness sources (with `.txt` suffixes so the
workspace does not build them).
