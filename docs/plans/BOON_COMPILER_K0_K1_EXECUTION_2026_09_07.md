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

The identity repair is checkpoint `ab670feca7e715728f95d95b66593652313b3575`.
An ordinary post-commit build recompiled the producer and emitted that clean
HEAD, demonstrating the repaired trigger on the real workspace.

Fresh identity-valid development preflight (one setup + five scored, both
product/evidence lanes and both cold modes):
`target/reports/compiler-performance/k0-identity-preflight.json`.

| Fixture / mode | Diagnostics p50 / p95 ms | Verified p50 / p95 ms |
| --- | --- | --- |
| Counter / fresh-process | 8.213 / 8.297 | 17.150 / 17.482 |
| Counter / empty-session | 7.140 / 7.348 | 16.051 / 16.868 |
| TodoMVC / fresh-process | 908.076 / 932.523 | 1276.965 / 1352.684 |
| TodoMVC / empty-session | 915.345 / 931.499 | 1303.899 / 1343.032 |
| NovyWave / fresh-process | 502.072 / 504.200 | 1881.583 / 1886.286 |
| NovyWave / empty-session | 508.744 / 509.970 | 1878.022 / 1945.904 |

All source/diagnostic/plan hashes, evidence parity and cache-disabled checks
pass. Overall status is **fail**: TodoMVC and NovyWave exceed time budgets;
Counter verified (~39.4 MiB) and TodoMVC exceed RSS budgets. Counter timing and
NovyWave RSS pass. These are baseline observations, not a regression verdict
against the old producer and not a K1 acceptance run. NovyWave fresh-process
bootstrap WHERE verification is 0.053 ms p50 / 0.057 ms p95; it is not authored
WHERE verification, which remains unimplemented and fail-closed.

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
transitive reuse conditions. Debug-only tracing now confirms PatternRead is the
first rejected result-path node in all five definitions: expressions 137, 161,
114, 171 and 104 for owners 80, 91, 103, 115 and 127 respectively.
`DirectSummaryPlanCompiler::compile_expression` also lacks that transfer.

[Machine-readable attribution](evidence/compiler-k0-attribution-2026-09-07.json)
records the exact variable/mode identities, representative root/dependency
identities, all quiescent type/mode tuple classes and sampled root epochs.
The dominant variant is `[None, None]` static arguments with
`initial_state_surface=true` in each definition.

| Definition | Distinct dominant-variant calls | Quiescent input type/mode classes | Calls with a nonauthoritative provider |
| --- | ---: | ---: | ---: |
| Classic.font | 175 | 21 | 155 |
| Glassmorphism.material | 179 | 26 | 154 |
| Neobrutalism.material | 179 | 26 | 154 |
| Neumorphism.material | 179 | 26 | 154 |
| Professional.material | 179 | 26 | 154 |

The five variants own 140,791 of 282,393 logical operations (49.9%). Their
891 call frames converge to 125 definition-qualified tuple classes, but final
type equality is **not** a valid reuse key. Many distinct open context roots
resolve to the same `{name, mode}` shape with different live dependency cells.
Twenty representative variable IDs generated 597 root-mutation observations;
some roots went through five to seven distinct bindings before convergence.
This is sampled root history, not a complete dependency-event oracle.

All 1,871 requested actual variables produced quiescent records. Trace runs
retained 620,555 activations and the same diagnostic fingerprint. IDs are
qualified by their tracing run; they are not stable cross-revision identities.
The observers are debug-only and their timing is not acceptance evidence.
An additional untraced debug NovyWave diagnostics smoke completed in
2,451.511 ms with zero diagnostics; Rust build time is excluded.

The relevant correctness boundary is concrete: `project_pattern` performs
tag-sensitive payload narrowing and requirement scaffolding for open providers;
authoritative providers remain directional and may replace disappearing
projections. Pattern bindings also retain their fixed arm-local flow mode.
Simply treating this as an ordinary field projection is unsound. K1 must
preserve those equations and per-call state/capture ownership, including calls
through nested summaries.

Independent read-only review identified two mandatory counterexample gates
before conversion. First, a wrapper calling `choose(First, open_value)` must
not inherit `Wrapped[item]` requirements from an untaken `Second` branch.
Residual specialization prunes that branch, but unconditional summary-input
requirement emission would not. Second, a later demanded requirement can
invalidate a selector already read in the same summary activation. The
existing scratch memoization and acyclic self-replay suppression must not
publish that stale result. Include nested Invoke variants of both tests.

The proposed ownership is a typed formal-root projection path (whole, field,
pattern), with private occurrence cells and lazy requirement backflow in the
existing summary evaluator. Dependency/read-write scheduling must make those
mutations converge in the existing queue. Pattern-local mode identity stays
separate from interned type-path identity. Computed pattern providers that
cannot be expressed by this path remain a precise generic residual case; do
not add a detached partial pattern evaluator or result-type-equality cache.

### Reproduced baseline correctness failure

`crates/boon_compiler/tests/kernel_transfer.rs` adds the high-level wrapper
cases. The pattern case passes on the existing residual path. The ordinary
field case **already fails before K1**: `choose(First, value)` exports an open
`item: VALUE` object requirement into `wrapper(value)`, so calling the wrapper
with text is incorrectly rejected. This is the eager requirement emission
identified by review, not a newly introduced pattern-transfer failure.

The failing case is explicitly ignored only to retain an honest clean K0
baseline checkpoint. Reproduce it with
`cargo test --locked --jobs 2 -p boon_compiler --test kernel_transfer -- --ignored`.
K1 must remove that ignore and make the test pass; an ignored reproduction
cannot satisfy K1 acceptance. No previously passing gate was disabled.

The 172 existing kernel unit tests, including the dependency firewall, pass
after the attribution-only change. The 14 summary-focused tests also pass.
The next scheduling change must distinguish a summary's actual-input read
subscriptions from its output-observation subscription in the existing reverse
consumer store. Input mutations may requeue the active summary; publishing its
own fresh contextual-hole result must not cause endless self-replay. Duplicate
roles on one variable must retain the read role, including after aliasing.
Do not add a second queue or globally persistent contextual-hole cache.

Frozen comparison cohort: Counter, TodoMVC physical and NovyWave, diagnostics
and verified intents, fresh-process and empty-session, unchanged release flags
and mimalloc lane. The independent wrapper requirements, stateful and invalid
source holdouts remain correctness gates. Target the five mapped residual
variants (140,791 linked operations); also report total activations (620,555),
summary-node evaluations (32,365), summary-call activations (6,373), term
interning, allocation calls/bytes and RSS so moved work remains visible. Counts
are not a universal cost model: the cut also needs an end-to-end product gain
outside noise. The 25% work-reduction hypothesis and 3+30 comparison protocol
are unchanged.

The first source probe also exposed an independent inline-function limitation:
`FUNCTION wrapper(value) { choose(which: First, value: value) }` receives no
AST body child (`ast_statement` assigns no expression to a function, while
`ast_statement_block` only collects following indented children). The checked
adapter then reports no direct or structural result. The canonical multiline
probe above isolates transfer semantics; it is not a fix or product workaround
for inline function support. Preserve this missing-feature report for a parser
tranche; do not claim that feature was implemented by K1.

## Next action and status

Attribution and the generic regression probes have run. Freeze a clean K0
source/binary pair before changing transfer semantics, then implement and
test lazy requirement and pattern transfer. Count
summary work as well as residual work; use the required alternating comparison
and holdouts before claiming a speedup. Baseline preservation is still pending,
K1 is not implemented, and inherited performance failures remain open.
