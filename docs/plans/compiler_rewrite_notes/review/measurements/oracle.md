# Old compiler as an oracle: re-measurement of BOON_COMPILER_REWRITE_PLAN.md §2

Measured 2026-09-29 on branch `compiler-rewrite-plan`, git HEAD `a60a11d6865fcc63bb86847e1d17df79e3e4f12d`, repo root as cwd.

## Claim under test

> **The old compiler is a weak oracle.** `boon_cli check` fails on 8 of the 22 manifest entry sources, and only 5 of the 21 manifest scenarios pass under `boon_cli run --scenario`. (plan §2, lines 129-131)

## Binaries

- HEAD: `/home/martinkavik/repos/boon-circuit/target/release/boon_cli` (39068512 bytes, mtime 2026-09-29 19:10:34)
- old:  `/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/bin/boon_cli.sep28` (39068648 bytes, mtime 2026-09-29 19:10:11)
- The old binary's mtime is the time it was copied into the scratchpad, not its build time (the task states it was built 2026-09-28 20:10, before e36b2c24 was committed). The two files differ in size by 136 bytes, so they are distinct builds; whether the sep28 file already contained the uncommitted kernel edits cannot be told from the file itself.

## Protocol

- For every `[[example]]` in `examples/manifest.toml` (parsed with `tomllib`), and for every `[[example.programs]]` source that differs from its example's source: `<cli> check <source>`, run once with each binary, then `<HEAD cli> run <source> --scenario <scenario>` when the entry has a scenario. Each command has a 120 s timeout (`subprocess.run(timeout=...)`), runs serially from the repo root, and its full stdout/stderr/exit code/wall time is in `raw/oracle/<id>.<check-head|check-old|run-head>.txt`.
- Runner: `tools/oracle_run.py`; this file is rendered by `tools/oracle_report.py` from `raw/oracle/summary.json`.
- Wall times are single-shot on a noisy desktop and are context only, not measurements.
- `boon_cli check` always compiles with `ProgramRole::Client` (crates/boon_cli/src/lib.rs `check_source`), so the fjordpulse session/server sources are checked as clients.

## Load

- uptime before: `19:22:56 up 1 day,  6:38,  1 user,  load average: 3.31, 3.62, 3.27`
- uptime after:  `19:23:06 up 1 day,  6:38,  1 user,  load average: 3.10, 3.56, 3.26`

## Results

| example | source | check HEAD | check old (sep28) | scenario (HEAD) | failure class | error excerpt |
| --- | --- | --- | --- | --- | --- | --- |
| cells | `examples/cells.bn` | FAIL rc=1 (0.018 s; 1 error, 546 stderr lines) | FAIL rc=1 (0.017 s; 1 error, 546 stderr lines) (same verdict+first line) | FAIL rc=1 (0.016 s), before/without a step id | compile error (compact ABI slice lacks Dependency/catch_cycle, List/range, Text/find) | dense kernel checked construction does not cover the complete project: { |
| todomvc | `examples/todomvc.bn` | pass (0.066 s) | pass (0.065 s) (same verdict+first line) | FAIL rc=1 (0.068 s), step `edit-test-todo` | runtime assertion (expectation mismatch at step edit-test-todo) | scenario step `edit-test-todo` expectation mismatches: document produced no retained patches after deltas [SetValue { target: RowField { row: RowId {… |
| todo_migration | `examples/migrations/todo/v1.bn` | pass (0.024 s) | pass (0.023 s) (same verdict+first line) | FAIL rc=1 (0.025 s), before/without a step id | harness: scenario-format drift (migration-runner `action` steps; boon_cli's parse_scenario rejects the file before any step) | TOML parse error at line 5, column 1 |
| todo_mvc_physical | `examples/todo_mvc_physical/RUN.bn` | pass (0.828 s) | pass (0.822 s) (same verdict+first line) | FAIL rc=1 (0.848 s), before/without a step id | runtime evaluation error before the first step (row has no field 34) | document evaluation failed: expression 64481: expression 64253: expression 64252: expression 64251: expression 64250: expression 36136: expression 36… |
| novywave | `examples/novywave/RUN.bn` | pass (2.081 s) | pass (1.943 s) (same verdict+first line) | FAIL rc=1 (1.979 s), step `load-default-file` | runtime assertion (expectation mismatch at step load-default-file; real_bridge_format got `none`, so the file bridge/host service is most likely absent in the CLI live runtime) | scenario step `load-default-file` expectation mismatches: root `active_signal` expected `simple_tb.s.A`, got `clk`; root `real_bridge_format` expecte… |
| kavik_cz | `examples/kavik_cz/RUN.bn` | FAIL rc=1 (0.029 s; 1 error, 1 stderr lines) | FAIL rc=1 (0.028 s; 1 error, 1 stderr lines) (same verdict+first line) | FAIL rc=1 (0.029 s), before/without a step id | compile error (dense kernel checked input: authored inputs differ from canonical inputs) | cannot build dense kernel checked input: kernel definition 49 call expression 15 authored inputs {KernelExpressionId(11): 1, KernelExpressionId(12): … |
| persons_pro | `examples/persons_pro/RUN.bn` | pass (0.206 s) | pass (0.214 s) (same verdict+first line) | FAIL rc=1 (0.207 s), before/without a step id | runtime evaluation error before the first step (store.draft_compile_line is not current: child-compile currentness) | document evaluation failed: expression 3669: expression 3655: expression 3654: expression 1972: expression 1971: expression 1642: expression 1303: ex… |
| fjordpulse | `examples/fjordpulse/Client/RUN.bn` | FAIL rc=1 (0.028 s; 1 error, 1 stderr lines) | FAIL rc=1 (0.026 s; 1 error, 1 stderr lines) (same verdict+first line) | FAIL rc=1 (0.031 s), before/without a step id | compile error (parse: qualified role value syntax at FjordPulseView.bn:1957:291) | examples/fjordpulse/View/FjordPulseView.bn: qualified role values use `Server/value.field`, not `Server.value.field` at line 1957, column 291 |
| fjordpulse/client | `examples/fjordpulse/Client/RUN.bn` | (same source as `fjordpulse`) | (same) | n/a | | |
| fjordpulse/session | `examples/fjordpulse/Session/RUN.bn` | FAIL rc=1 (0.01 s; 1 error, 1 stderr lines) | FAIL rc=1 (0.01 s; 1 error, 1 stderr lines) (same verdict+first line) | no scenario |  | client programs must expose one retained document or scene root |
| fjordpulse/server | `examples/fjordpulse/Server/RUN.bn` | FAIL rc=1 (0.017 s; 1 error, 796 stderr lines) | FAIL rc=1 (0.017 s; 1 error, 796 stderr lines) (same verdict+first line) | no scenario |  | dense kernel checked construction does not cover the complete project: { |
| counter | `examples/counter.bn` | pass (0.019 s) | pass (0.018 s) (same verdict+first line) | FAIL rc=1 (0.019 s), step `press-increment` | harness: scenario drift (counter.scn:12 names route store.sources.increment_button.press; counter.bn:29/125 expose ...increment_button.events.press) | scenario step `press-increment` target: MachinePlan has no source route `store.sources.increment_button.press` |
| counter_migration | `examples/migrations/counter/v1.bn` | pass (0.017 s) | pass (0.017 s) (same verdict+first line) | FAIL rc=1 (0.018 s), before/without a step id | harness: scenario-format drift (migration-runner `action` steps; boon_cli's parse_scenario rejects the file before any step) | TOML parse error at line 5, column 1 |
| minimal | `examples/minimal.bn` | pass (0.01 s) | pass (0.01 s) (same verdict+first line) | PASS (0.01 s): pass: 1 turn(s), 0 state value(s), 0 derived field value(s), 0 list(s) |  |  |
| hello_world | `examples/hello_world.bn` | pass (0.012 s) | pass (0.011 s) (same verdict+first line) | PASS (0.012 s): pass: 1 turn(s), 0 state value(s), 0 derived field value(s), 0 list(s) |  |  |
| counter_latest | `examples/counter_latest.bn` | pass (0.016 s) | pass (0.016 s) (same verdict+first line) | PASS (0.017 s): pass: 4 turn(s), 1 state value(s), 0 derived field value(s), 0 list(s) |  |  |
| fibonacci | `examples/fibonacci.bn` | FAIL rc=1 (0.019 s; 1 error, 1 stderr lines) | FAIL rc=1 (0.022 s; 1 error, 1 stderr lines) (same verdict+first line) | FAIL rc=1 (0.019 s), before/without a step id | compile error (FLUSH boundary cannot be lowered as retained document data) | ordinary document callable `fibonacci_result` body failed during shared function lowering: FLUSH boundary at executable expression 139 cannot be lowe… |
| interval_latest | `examples/interval_latest.bn` | FAIL rc=1 (0.007 s; 1 error, 88 stderr lines) | FAIL rc=1 (0.007 s; 1 error, 88 stderr lines) (same verdict+first line) | FAIL rc=1 (0.008 s), before/without a step id | compile error (compact ABI slice lacks Timer/interval) | dense kernel checked construction does not cover the complete project: { |
| interval_hold | `examples/interval_hold.bn` | FAIL rc=1 (0.008 s; 1 error, 120 stderr lines) | FAIL rc=1 (0.008 s; 1 error, 120 stderr lines) (same verdict+first line) | FAIL rc=1 (0.008 s), before/without a step id | compile error (compact ABI slice lacks Timer/interval) | dense kernel checked construction does not cover the complete project: { |
| flow_operators | `examples/flow_operators.bn` | pass (0.019 s) | pass (0.02 s) (same verdict+first line) | PASS (0.021 s): pass: 4 turn(s), 2 state value(s), 4 derived field value(s), 0 list(s) |  |  |
| layers | `examples/layers.bn` | FAIL rc=1 (0.007 s; 1 error, 1 stderr lines) | FAIL rc=1 (0.007 s; 1 error, 1 stderr lines) (same verdict+first line) | FAIL rc=1 (0.007 s), before/without a step id | compile error (dense kernel checked input: ElementState declaration has incompatible CallContext origin) | cannot build dense kernel checked input: kernel declaration 17 kind ElementState has incompatible origin CallContext { call: KernelExpressionId(36), … |
| pages | `examples/pages.bn` | pass (0.018 s) | pass (0.017 s) (same verdict+first line) | PASS (0.019 s): pass: 4 turn(s), 1 state value(s), 2 derived field value(s), 0 list(s) |  |  |
| flush_error_propagation | `examples/flush_error_propagation.bn` | FAIL rc=1 (0.015 s; 1 error, 1 stderr lines) | FAIL rc=1 (0.015 s; 1 error, 1 stderr lines) (same verdict+first line) | FAIL rc=1 (0.015 s), before/without a step id | compile error (invalid executable local bindings during lowering) | invalid executable local bindings: semantic field statement 0 from checked statement 4 value expression 7 expands checked expression 7 with type Obje… |

## Totals

- `[[example]]` entries in the manifest: 20; `[[example.programs]]` sub-tables: 3 (all under fjordpulse); distinct entry sources checked: 22.
- `check` HEAD: 12 pass / 10 fail. Failing: cells, kavik_cz, fjordpulse, fjordpulse/session, fjordpulse/server, fibonacci, interval_latest, interval_hold, layers, flush_error_propagation.
- `check` old (sep28): 12 pass / 10 fail. Failing: cells, kavik_cz, fjordpulse, fjordpulse/session, fjordpulse/server, fibonacci, interval_latest, interval_hold, layers, flush_error_propagation.
- Sources where the two builds differ (verdict or first output line): 0.
- Scenarios run: 20; pass: 5 (minimal, hello_world, counter_latest, flow_operators, pages); fail: 15 (cells, todomvc, todo_migration, todo_mvc_physical, novywave, kavik_cz, persons_pro, fjordpulse, counter, counter_migration, fibonacci, interval_latest, interval_hold, layers, flush_error_propagation).
- Scenario failure classes:
  - compile error: 8 (cells, kavik_cz, fjordpulse, fibonacci, interval_latest, interval_hold, layers, flush_error_propagation)
  - harness / scenario drift (no host service or wrong scenario format/route): 3 (todo_migration, counter, counter_migration)
  - runtime assertion (expectation mismatch): 2 (todomvc, novywave)
  - runtime evaluation error before the first step: 2 (todo_mvc_physical, persons_pro)
- Timeouts: 0 of 62 commands; the longest command was `check examples/novywave/RUN.bn` at about 2.1 s, far below the 120 s limit.

## Findings against the plan's claim

1. **Entry count.** The manifest has **20** `[[example]]` tables, every one with a `scenario`, plus **3** `[[example.programs]]` tables under `fjordpulse` (client, session, server). That gives 23 tables, 22 distinct sources (the client program repeats the fjordpulse example source) and **20** scenarios. The plan's "22 manifest entry sources" is the distinct-source count; its "21 manifest scenarios" matches nothing in the manifest (the notes' `plan_draft_critique.md` item 5 already flagged the mixed denominators).
2. **`check` failures.** With the 20-example denominator the plan's "8" reproduces exactly: cells, kavik_cz, fjordpulse, fibonacci, interval_latest, interval_hold, layers, flush_error_propagation. With the 22-source denominator the plan itself chose, the count is **10 of 22**, because `fjordpulse/server` also fails (compact ABI slice lacks `List/take` and `Text/to_bytes`) and `fjordpulse/session` fails with "client programs must expose one retained document or scene root", which is an artifact of `boon_cli check` always compiling as `ProgramRole::Client` rather than a compiler bug. So: 8/20, 9/22 (excluding the role artifact) or 10/22 (as measured). The plan's "8 of the 22" mixes the numerator of one denominator with the other.
3. **Scenario passes.** **5 of 20** pass (minimal, hello_world, counter_latest, flow_operators, pages), the same five the earlier notes named. The plan's "5 of 21" has the right numerator and a denominator that does not exist; 5/20 is the reproducible figure.
4. **Failure mix.** Of the 15 failing scenarios, 8 are the same compile errors that fail `check` (the runtime never starts), 3 are harness/scenario-file problems that say nothing about the compiler (the two migration `.scn` files are written for `boon_host_runtime::MigrationScenarioRunner`'s `action` steps, which `boon_runtime::parse_scenario` rejects at TOML level; `counter.scn:12` names a source route that `counter.bn` does not expose), 2 fail on document evaluation before the first step (todo_mvc_physical, persons_pro) and 2 fail an expectation at their first interactive step (todomvc at `edit-test-todo`, novywave at `load-default-file`, where every `real_*` root is `none`, consistent with no file bridge in the CLI runtime). Only the last four exercise the old compiler's output at all, and only todomvc's failure is unambiguously about runtime behaviour rather than a missing host service. The qualitative claim "the old compiler is a weak oracle" therefore holds, but the reason is mostly compile-time rejection and harness gaps, not divergent behaviour.
5. **HEAD vs sep28 build.** All 22 `check` outputs are byte-identical between the two binaries once the command/timing header is stripped (`cmp` on `sed '1,4d'` of each pair: same=22 diff=0). The kernel memoization commit e36b2c24 did not change any accept/reject verdict or message on the manifest sources.
6. **Speed of the oracle run.** Both binaries check every manifest source in well under 3 s each; the whole 62-command series took 10 s of wall clock. Timeouts are not a factor in the oracle's weakness.

## Notes per failure (from the raw files)

| example | where it fails | evidence (raw file) |
| --- | --- | --- |
| cells | compile | `raw/oracle/cells.check-head.txt`: "dense kernel checked construction does not cover the complete project"; leaf reasons: `Dependency/catch_cycle`, `List/range` (x2), `Text/find` (x3) "is not in the current compact ABI slice" |
| kavik_cz | compile | `cannot build dense kernel checked input: kernel definition 49 call expression 15 authored inputs {11,12,14} differ from canonical inputs {11,12}` |
| fjordpulse (client) | parse | `examples/fjordpulse/View/FjordPulseView.bn: qualified role values use `Server/value.field`, not `Server.value.field` at line 1957, column 291` |
| fjordpulse/session | CLI role | `client programs must expose one retained document or scene root` (check_source hard-codes ProgramRole::Client) |
| fjordpulse/server | compile | dense kernel: `List/take`, `Text/to_bytes` not in the compact ABI slice |
| fibonacci | lowering | `ordinary document callable `fibonacci_result` body failed during shared function lowering: FLUSH boundary at executable expression 139 cannot be lowered as retained document data` |
| interval_latest, interval_hold | compile | dense kernel: `Timer/interval` not in the compact ABI slice |
| layers | compile | `cannot build dense kernel checked input: kernel declaration 17 kind ElementState has incompatible origin CallContext { call: KernelExpressionId(36), ordinal: 0 }` |
| flush_error_propagation | lowering | `invalid executable local bindings: semantic field statement 0 from checked statement 4 value expression 7 expands checked expression 7 with type Object(...)` |
| counter | scenario step `press-increment` | `MachinePlan has no source route `store.sources.increment_button.press``; `examples/counter.bn:29` and `:125` use `sources.increment_button.events.press` |
| todo_migration, counter_migration | scenario file parse | `TOML parse error at line 5, column 1 ... unknown field `action``; the file's step schema belongs to `crates/boon_host_runtime/src/migration_scenario.rs` (MigrationScenarioRunner), not `boon_runtime::parse_scenario` |
| todomvc | scenario step `edit-test-todo` | `expectation mismatches: document produced no retained patches after deltas [SetValue { target: RowField { row: RowId { list: ListId(0), key: 5, generation: 1 }, field: FieldId(19) }, value: Tag { tag: "True", fields: {} } }]` |
| novywave | scenario step `load-default-file` | `root `active_signal` expected `simple_tb.s.A`, got `clk`; root `real_bridge_format` expected `VCD`, got `none`; root `real_first_signal_id` expected `simple_tb.s.A`, got `none`` |
| todo_mvc_physical | document evaluation before step 1 | `document evaluation failed: ... derived op 25 output Some(List(ListId(1))): row 0:1:1 has no field 34; available fields are [20, 21, 22, 24, 25, 26, 28, 30, 33]` |
| persons_pro | document evaluation before step 1 | `document evaluation failed: ... value target Field(FieldId(25)) (store.draft_compile_line, draft_compile_line) is not current` |

## Reproduce

```bash
cd /home/martinkavik/repos/boon-circuit
python3 docs/plans/compiler_rewrite_notes/review/measurements/tools/oracle_run.py     # writes raw/oracle/*.txt + summary.json + uptime.txt
python3 docs/plans/compiler_rewrite_notes/review/measurements/tools/oracle_report.py  # renders this file
```

