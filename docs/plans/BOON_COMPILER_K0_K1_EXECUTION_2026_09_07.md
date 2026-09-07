# K0 + K1 execution evidence — 2026-09-07

This is an execution log, not a replacement goal or acceptance contract.
The bounded [K0 + K1 goal](BOON_COMPILER_TENS_OF_MILLISECONDS_GOAL_PROMPT.md)
remains active. Starting HEAD: `9901854231ffc248e5bd3db79b582e86bc57b71c`;
starting compiler implementation: `37a576b6`. No pushes are authorized.

## Producer identity prerequisite

An ordinary release build initially finished in 0.77 seconds without rebuilding
the producer. Its embedded HEAD remained `25a354d4`, dirty, although the live
checkout was clean `99018542`. That binary is not a current timing baseline.
The build script included HEAD and staging state in its v1 identity but did
not watch the Git inputs that change them.

The repair retains the existing strict identity and validation rules. It adds
Git-resolved watches for HEAD, the current branch ref, existing index and
packed-refs; a packed branch watches its existing refs parent until its loose
ref exists. Linked worktrees resolve their private HEAD/index correctly.
Missing optional files and the whole `.git` directory are not watched.

The frozen release profile label now says `lto=false`, matching the unchanged
Cargo.toml setting, instead of `lto=off`. No optimization flags, allocator,
budgets, oracle hashes, or freshness rejection rules were relaxed. Debug
observations no longer attest release-only profile settings; unknown settings
are explicitly labeled unknown. This remains a fixed-lane identity, not a
general effective-profile resolver for arbitrary overrides.

Focused checks:

- `cargo test --locked --jobs 2 -p boon_cli --test build_git`: four tests pass
  for branch commits, detached HEAD/staging, packed refs and linked worktrees.
- `cargo build --locked --release --jobs 2 -p boon_cli --bin boon_cli --bin boon_cli_evidence`:
  refreshed the producer to the actual HEAD/dirty input identity.
- An unchanged repeat build finished in 0.13 seconds without recompiling.
- A direct Counter observation emitted the corrected profile label and
  current dirty source identity. Its 7.142 ms internal time is a plumbing
  smoke check, not fresh-process collector latency or performance acceptance.

## Initial source attribution

The existing `BOON_KERNEL_ORACLE_TRACE_DENSE_OWNER` diagnostic maps the dated
TodoMVC top-five residual variants to these definitions. The trace used the
pre-repair binary only for source attribution: its compiler implementation
matches the starting implementation, but its provenance is stale for scoring.

| Dense owner | Definition under `examples/todo_mvc_physical/Theme/` | Local pattern reads |
| --- | --- | --- |
| 91 | `Glassmorphism.bn::material` | `InteractiveRecessed.focus` |
| 115 | `Neumorphism.bn::material` | `InteractiveRecessed.focus` |
| 127 | `Professional.bn::material` | `Interactive.hovered`, `InteractiveRecessed.focus` |
| 103 | `Neobrutalism.bn::material` | `InteractiveRecessed.focus` |
| 80 | `Classic.bn::font` | `ButtonIcon.checked`, `SmallLink.hovered` |

These local definitions contain no HOLD. This does not prove their complete
transitive reuse conditions. `direct_result_summary_supported` explicitly
rejects PatternRead, and `DirectSummaryPlanCompiler::compile_expression` has no
pattern transfer. Exact first-result-path rejection, actual variable/mode
tuples, semantic tuple equivalence and provider epochs still need tracing.

The relevant correctness boundary is concrete: `project_pattern` performs
tag-sensitive payload narrowing and requirement scaffolding for open providers;
authoritative providers remain directional and may replace disappearing
projections. Pattern bindings also retain their fixed arm-local flow mode.
Simply treating this as an ordinary field projection is unsound. K1 must
preserve those equations and per-call state/capture ownership, including calls
through nested summaries.

## Next action and status

Finish K0 attribution and freeze the comparison cohort before changing transfer
semantics. Preserve a clean baseline source/binary pair, then implement and
test the generic pattern transfer if its hypothesis survives review. Count
summary work as well as residual work; use the required alternating comparison
and holdouts before claiming a speedup. K0 is not complete, K1 is not
implemented, and inherited performance failures remain open.
