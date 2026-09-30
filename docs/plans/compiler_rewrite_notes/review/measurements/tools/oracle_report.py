#!/usr/bin/env python3
"""Render raw/oracle/summary.json (from oracle_run.py) into oracle.md."""
import json
import re
from pathlib import Path

REPO = Path("/home/martinkavik/repos/boon-circuit")
MEAS = REPO / "docs/plans/compiler_rewrite_notes/review/measurements"
RAW = MEAS / "raw/oracle"
OUT = MEAS / "oracle.md"

# Failure classes for `run --scenario`, matched in order against the error text.
CLASSES = [
    ("timeout", re.compile(r"__TIMEOUT__")),
    ("harness: scenario-format drift", re.compile(r"unknown field|unknown variant|missing field|scenario.*(parse|deserial)|invalid scenario", re.I)),
    ("harness: host service missing", re.compile(r"host service|host capability|no host|unsupported (host|effect|capability)|not (available|supported) in (the )?(cli|live runtime)|requires (a )?host", re.I)),
    ("compile error", re.compile(r"dense kernel|checked (construction|input)|typecheck|type error|parse error|syntax|lower(ing)?|kernel|compile|does not cover|cannot build|unknown (function|identifier|variable)|ABI|compact", re.I)),
    ("runtime assertion (expectation mismatch)", re.compile(r"expect|assert|mismatch|did not match|but (was|got|found)", re.I)),
    ("runtime evaluation error", re.compile(r"evaluation failed|has no field|panic|runtime|evaluat|failed to (apply|run) step|step .* failed|no such (source|route|state)|unknown (source|route|state|port)", re.I)),
]

# Hand-verified classes from reading raw/oracle/<id>.run-head.txt (see oracle.md "Notes per failure").
# The regex CLASSES above are only the fallback for ids not listed here.
EXPLICIT_CLASSES = {
    "cells": "compile error (compact ABI slice lacks Dependency/catch_cycle, List/range, Text/find)",
    "todomvc": "runtime assertion (expectation mismatch at step edit-test-todo)",
    "todo_migration": "harness: scenario-format drift (migration-runner `action` steps; boon_cli's parse_scenario rejects the file before any step)",
    "todo_mvc_physical": "runtime evaluation error before the first step (row has no field 34)",
    "novywave": "runtime assertion (expectation mismatch at step load-default-file; real_bridge_format got `none`, so the file bridge/host service is most likely absent in the CLI live runtime)",
    "kavik_cz": "compile error (dense kernel checked input: authored inputs differ from canonical inputs)",
    "persons_pro": "runtime evaluation error before the first step (store.draft_compile_line is not current: child-compile currentness)",
    "fjordpulse": "compile error (parse: qualified role value syntax at FjordPulseView.bn:1957:291)",
    "counter": "harness: scenario drift (counter.scn:12 names route store.sources.increment_button.press; counter.bn:29/125 expose ...increment_button.events.press)",
    "counter_migration": "harness: scenario-format drift (migration-runner `action` steps; boon_cli's parse_scenario rejects the file before any step)",
    "fibonacci": "compile error (FLUSH boundary cannot be lowered as retained document data)",
    "interval_latest": "compile error (compact ABI slice lacks Timer/interval)",
    "interval_hold": "compile error (compact ABI slice lacks Timer/interval)",
    "layers": "compile error (dense kernel checked input: ElementState declaration has incompatible CallContext origin)",
    "flush_error_propagation": "compile error (invalid executable local bindings during lowering)",
}

STEP_RE = re.compile(r"step[ \t]*[`'\"]?([A-Za-z0-9_./:-]+)[`'\"]?", re.I)
COUNT_RE = re.compile(r"(\d+)\s+(error|diagnostic)\(?s?\)?", re.I)


def first_line(text: str) -> str:
    for line in text.splitlines():
        line = line.strip()
        if line:
            return line
    return ""


def error_text(rec: dict) -> str:
    if rec.get("timed_out"):
        return "__TIMEOUT__ " + first_line(rec.get("stderr", "")) + first_line(rec.get("stdout", ""))
    return rec.get("stderr", "") + "\n" + rec.get("stdout", "")


def classify(rec: dict, example_id: str) -> str:
    if rec.get("timed_out"):
        return "timeout"
    if example_id in EXPLICIT_CLASSES:
        return EXPLICIT_CLASSES[example_id]
    text = error_text(rec)
    for name, rx in CLASSES:
        if rx.search(text):
            return name + " (regex fallback)"
    return "unclassified"


def coarse(cls: str) -> str:
    """Bucket a detailed class into the four classes the task asked for."""
    if cls.startswith("timeout"):
        return "timeout"
    if cls.startswith("compile error"):
        return "compile error"
    if cls.startswith("harness"):
        return "harness / scenario drift (no host service or wrong scenario format/route)"
    if cls.startswith("runtime assertion"):
        return "runtime assertion (expectation mismatch)"
    if cls.startswith("runtime evaluation"):
        return "runtime evaluation error before the first step"
    return "unclassified"


def excerpt(rec: dict, width: int = 150) -> str:
    if rec.get("timed_out"):
        return f"(killed after {rec['wall_s']} s)"
    line = first_line(rec.get("stderr", "")) or first_line(rec.get("stdout", ""))
    line = line.replace("boon_cli: ", "", 1)
    line = line.replace("|", "\\|")
    if len(line) > width:
        line = line[: width - 1] + "…"
    return line


def diag_count(rec: dict) -> str:
    """boon_cli check returns a single Err; only MachinePlan verification reports an error count."""
    text = rec.get("stderr", "")
    m = COUNT_RE.search(text)
    lines = [l for l in text.splitlines() if l.strip()]
    if m:
        return f"{m.group(1)} verification error(s), {len(lines)} stderr lines"
    return f"1 error, {len(lines)} stderr lines"


def step_id(rec: dict) -> str:
    text = error_text(rec)
    m = STEP_RE.search(text)
    return m.group(1) if m else ""


def check_cell(rec: dict | None) -> str:
    if rec is None:
        return "n/a"
    if rec.get("timed_out"):
        return f"TIMEOUT ({rec['wall_s']} s)"
    if rec["exit_code"] == 0:
        return f"pass ({rec['wall_s']} s)"
    return f"FAIL rc={rec['exit_code']} ({rec['wall_s']} s; {diag_count(rec)})"


def main() -> None:
    summary = json.loads((RAW / "summary.json").read_text())
    rows = []
    totals = {
        "entries": 0,
        "examples": 0,
        "sources_checked": 0,
        "check_head_pass": 0,
        "check_old_pass": 0,
        "check_differs": 0,
        "scenarios": 0,
        "scenario_pass": 0,
        "classes": {},
        "check_head_fail_ids": [],
        "check_old_fail_ids": [],
        "scenario_pass_ids": [],
        "scenario_fail_ids": [],
    }
    for e in summary["entries"]:
        totals["entries"] += 1
        if e["kind"] == "example":
            totals["examples"] += 1
        ch = e.get("check_head")
        co = e.get("check_old")
        rh = e.get("run_head")
        if e.get("skip"):
            rows.append(
                f"| {e['id']} | `{e['source']}` | (same source as `{e['id'].split('/')[0]}`) | (same) | n/a | | |"
            )
            continue
        totals["sources_checked"] += 1
        head_ok = ch["exit_code"] == 0 and not ch["timed_out"]
        old_ok = co["exit_code"] == 0 and not co["timed_out"]
        totals["check_head_pass"] += head_ok
        totals["check_old_pass"] += old_ok
        if not head_ok:
            totals["check_head_fail_ids"].append(e["id"])
        if not old_ok:
            totals["check_old_fail_ids"].append(e["id"])
        head_first = first_line(ch["stderr"]) if not head_ok else first_line(ch["stdout"])
        old_first = first_line(co["stderr"]) if not old_ok else first_line(co["stdout"])
        differs = (head_ok != old_ok) or (head_first != old_first)
        totals["check_differs"] += differs
        check_head = check_cell(ch)
        check_old = check_cell(co) + (" **DIFFERS**" if differs else " (same verdict+first line)")
        if rh is None:
            scen = "no scenario"
            cls = ""
            exc = excerpt(ch) if not head_ok else ""
        else:
            totals["scenarios"] += 1
            if rh["exit_code"] == 0 and not rh["timed_out"]:
                totals["scenario_pass"] += 1
                totals["scenario_pass_ids"].append(e["id"])
                scen = f"PASS ({rh['wall_s']} s): {first_line(rh['stdout'])}"
                cls = ""
                exc = ""
            else:
                totals["scenario_fail_ids"].append(e["id"])
                cls = classify(rh, e["id"])
                bucket = coarse(cls)
                totals["classes"][bucket] = totals["classes"].get(bucket, 0) + 1
                totals.setdefault("class_members", {}).setdefault(bucket, []).append(e["id"])
                sid = step_id(rh)
                scen = f"FAIL rc={rh['exit_code']} ({rh['wall_s']} s)" + (f", step `{sid}`" if sid else ", before/without a step id")
                exc = excerpt(rh)
        rows.append(
            f"| {e['id']} | `{e['source']}` | {check_head} | {check_old} | {scen} | {cls} | {exc} |"
        )

    lines = []
    lines.append("# Old compiler as an oracle: re-measurement of BOON_COMPILER_REWRITE_PLAN.md §2\n")
    lines.append(f"Measured {summary['entries'][0]['check_head']['started_at'][:10]} on branch `compiler-rewrite-plan`, git HEAD `{summary['git_head']}`, repo root as cwd.\n")
    lines.append("## Claim under test\n")
    lines.append("> **The old compiler is a weak oracle.** `boon_cli check` fails on 8 of the 22 manifest entry sources, and only 5 of the 21 manifest scenarios pass under `boon_cli run --scenario`. (plan §2, lines 129-131)\n")
    lines.append("## Binaries\n")
    lines.append(f"- HEAD: `{summary['head_cli']}` ({summary['head_cli_stat']['size']} bytes, mtime {summary['head_cli_stat']['mtime']})")
    lines.append(f"- old:  `{summary['old_cli']}` ({summary['old_cli_stat']['size']} bytes, mtime {summary['old_cli_stat']['mtime']})")
    lines.append("- The old binary's mtime is the time it was copied into the scratchpad, not its build time (the task states it was built 2026-09-28 20:10, before e36b2c24 was committed). The two files differ in size by 136 bytes, so they are distinct builds; whether the sep28 file already contained the uncommitted kernel edits cannot be told from the file itself.\n")
    lines.append("## Protocol\n")
    lines.append(f"- For every `[[example]]` in `examples/manifest.toml` (parsed with `tomllib`), and for every `[[example.programs]]` source that differs from its example's source: `<cli> check <source>`, run once with each binary, then `<HEAD cli> run <source> --scenario <scenario>` when the entry has a scenario. Each command has a {summary['timeout_s']} s timeout (`subprocess.run(timeout=...)`), runs serially from the repo root, and its full stdout/stderr/exit code/wall time is in `raw/oracle/<id>.<check-head|check-old|run-head>.txt`.")
    lines.append("- Runner: `tools/oracle_run.py`; this file is rendered by `tools/oracle_report.py` from `raw/oracle/summary.json`.")
    lines.append("- Wall times are single-shot on a noisy desktop and are context only, not measurements.")
    lines.append("- `boon_cli check` always compiles with `ProgramRole::Client` (crates/boon_cli/src/lib.rs `check_source`), so the fjordpulse session/server sources are checked as clients.\n")
    lines.append("## Load\n")
    lines.append(f"- uptime before: `{summary['uptime_before']}`")
    lines.append(f"- uptime after:  `{summary['uptime_after']}`\n")
    lines.append("## Results\n")
    lines.append("| example | source | check HEAD | check old (sep28) | scenario (HEAD) | failure class | error excerpt |")
    lines.append("| --- | --- | --- | --- | --- | --- | --- |")
    lines.extend(rows)
    lines.append("")
    lines.append("## Totals\n")
    lines.append(f"- `[[example]]` entries in the manifest: {totals['examples']}; `[[example.programs]]` sub-tables: {totals['entries'] - totals['examples']} (all under fjordpulse); distinct entry sources checked: {totals['sources_checked']}.")
    lines.append(f"- `check` HEAD: {totals['check_head_pass']} pass / {totals['sources_checked'] - totals['check_head_pass']} fail. Failing: {', '.join(totals['check_head_fail_ids']) or 'none'}.")
    lines.append(f"- `check` old (sep28): {totals['check_old_pass']} pass / {totals['sources_checked'] - totals['check_old_pass']} fail. Failing: {', '.join(totals['check_old_fail_ids']) or 'none'}.")
    lines.append(f"- Sources where the two builds differ (verdict or first output line): {totals['check_differs']}.")
    lines.append(f"- Scenarios run: {totals['scenarios']}; pass: {totals['scenario_pass']} ({', '.join(totals['scenario_pass_ids']) or 'none'}); fail: {totals['scenarios'] - totals['scenario_pass']} ({', '.join(totals['scenario_fail_ids']) or 'none'}).")
    if totals["classes"]:
        lines.append("- Scenario failure classes:")
        for cls, n in sorted(totals["classes"].items(), key=lambda kv: -kv[1]):
            members = ", ".join(totals["class_members"].get(cls, []))
            lines.append(f"  - {cls}: {n} ({members})")
    lines.append("- Timeouts: 0 of 62 commands; the longest command was `check examples/novywave/RUN.bn` at about 2.1 s, far below the 120 s limit.")
    lines.append("")
    lines.append("## Findings against the plan's claim\n")
    lines.append("1. **Entry count.** The manifest has **20** `[[example]]` tables, every one with a `scenario`, plus **3** `[[example.programs]]` tables under `fjordpulse` (client, session, server). That gives 23 tables, 22 distinct sources (the client program repeats the fjordpulse example source) and **20** scenarios. The plan's \"22 manifest entry sources\" is the distinct-source count; its \"21 manifest scenarios\" matches nothing in the manifest (the notes' `plan_draft_critique.md` item 5 already flagged the mixed denominators).")
    lines.append(f"2. **`check` failures.** With the 20-example denominator the plan's \"8\" reproduces exactly: {', '.join(i for i in totals['check_head_fail_ids'] if '/' not in i)}. With the 22-source denominator the plan itself chose, the count is **{len(totals['check_head_fail_ids'])} of 22**, because `fjordpulse/server` also fails (compact ABI slice lacks `List/take` and `Text/to_bytes`) and `fjordpulse/session` fails with \"client programs must expose one retained document or scene root\", which is an artifact of `boon_cli check` always compiling as `ProgramRole::Client` rather than a compiler bug. So: 8/20, 9/22 (excluding the role artifact) or 10/22 (as measured). The plan's \"8 of the 22\" mixes the numerator of one denominator with the other.")
    lines.append(f"3. **Scenario passes.** **{totals['scenario_pass']} of {totals['scenarios']}** pass ({', '.join(totals['scenario_pass_ids'])}), the same five the earlier notes named. The plan's \"5 of 21\" has the right numerator and a denominator that does not exist; 5/20 is the reproducible figure.")
    lines.append("4. **Failure mix.** Of the 15 failing scenarios, 8 are the same compile errors that fail `check` (the runtime never starts), 3 are harness/scenario-file problems that say nothing about the compiler (the two migration `.scn` files are written for `boon_host_runtime::MigrationScenarioRunner`'s `action` steps, which `boon_runtime::parse_scenario` rejects at TOML level; `counter.scn:12` names a source route that `counter.bn` does not expose), 2 fail on document evaluation before the first step (todo_mvc_physical, persons_pro) and 2 fail an expectation at their first interactive step (todomvc at `edit-test-todo`, novywave at `load-default-file`, where every `real_*` root is `none`, consistent with no file bridge in the CLI runtime). Only the last four exercise the old compiler's output at all, and only todomvc's failure is unambiguously about runtime behaviour rather than a missing host service. The qualitative claim \"the old compiler is a weak oracle\" therefore holds, but the reason is mostly compile-time rejection and harness gaps, not divergent behaviour.")
    lines.append("5. **HEAD vs sep28 build.** All 22 `check` outputs are byte-identical between the two binaries once the command/timing header is stripped (`cmp` on `sed '1,4d'` of each pair: same=22 diff=0). The kernel memoization commit e36b2c24 did not change any accept/reject verdict or message on the manifest sources.")
    lines.append("6. **Speed of the oracle run.** Both binaries check every manifest source in well under 3 s each; the whole 62-command series took 10 s of wall clock. Timeouts are not a factor in the oracle's weakness.")
    lines.append("")
    lines.append("## Notes per failure (from the raw files)\n")
    lines.append("| example | where it fails | evidence (raw file) |")
    lines.append("| --- | --- | --- |")
    lines.append("| cells | compile | `raw/oracle/cells.check-head.txt`: \"dense kernel checked construction does not cover the complete project\"; leaf reasons: `Dependency/catch_cycle`, `List/range` (x2), `Text/find` (x3) \"is not in the current compact ABI slice\" |")
    lines.append("| kavik_cz | compile | `cannot build dense kernel checked input: kernel definition 49 call expression 15 authored inputs {11,12,14} differ from canonical inputs {11,12}` |")
    lines.append("| fjordpulse (client) | parse | `examples/fjordpulse/View/FjordPulseView.bn: qualified role values use `Server/value.field`, not `Server.value.field` at line 1957, column 291` |")
    lines.append("| fjordpulse/session | CLI role | `client programs must expose one retained document or scene root` (check_source hard-codes ProgramRole::Client) |")
    lines.append("| fjordpulse/server | compile | dense kernel: `List/take`, `Text/to_bytes` not in the compact ABI slice |")
    lines.append("| fibonacci | lowering | `ordinary document callable `fibonacci_result` body failed during shared function lowering: FLUSH boundary at executable expression 139 cannot be lowered as retained document data` |")
    lines.append("| interval_latest, interval_hold | compile | dense kernel: `Timer/interval` not in the compact ABI slice |")
    lines.append("| layers | compile | `cannot build dense kernel checked input: kernel declaration 17 kind ElementState has incompatible origin CallContext { call: KernelExpressionId(36), ordinal: 0 }` |")
    lines.append("| flush_error_propagation | lowering | `invalid executable local bindings: semantic field statement 0 from checked statement 4 value expression 7 expands checked expression 7 with type Object(...)` |")
    lines.append("| counter | scenario step `press-increment` | `MachinePlan has no source route `store.sources.increment_button.press``; `examples/counter.bn:29` and `:125` use `sources.increment_button.events.press` |")
    lines.append("| todo_migration, counter_migration | scenario file parse | `TOML parse error at line 5, column 1 ... unknown field `action``; the file's step schema belongs to `crates/boon_host_runtime/src/migration_scenario.rs` (MigrationScenarioRunner), not `boon_runtime::parse_scenario` |")
    lines.append("| todomvc | scenario step `edit-test-todo` | `expectation mismatches: document produced no retained patches after deltas [SetValue { target: RowField { row: RowId { list: ListId(0), key: 5, generation: 1 }, field: FieldId(19) }, value: Tag { tag: \"True\", fields: {} } }]` |")
    lines.append("| novywave | scenario step `load-default-file` | `root `active_signal` expected `simple_tb.s.A`, got `clk`; root `real_bridge_format` expected `VCD`, got `none`; root `real_first_signal_id` expected `simple_tb.s.A`, got `none`` |")
    lines.append("| todo_mvc_physical | document evaluation before step 1 | `document evaluation failed: ... derived op 25 output Some(List(ListId(1))): row 0:1:1 has no field 34; available fields are [20, 21, 22, 24, 25, 26, 28, 30, 33]` |")
    lines.append("| persons_pro | document evaluation before step 1 | `document evaluation failed: ... value target Field(FieldId(25)) (store.draft_compile_line, draft_compile_line) is not current` |")
    lines.append("")
    lines.append("## Reproduce\n")
    lines.append("```bash")
    lines.append("cd /home/martinkavik/repos/boon-circuit")
    lines.append("python3 docs/plans/compiler_rewrite_notes/review/measurements/tools/oracle_run.py     # writes raw/oracle/*.txt + summary.json + uptime.txt")
    lines.append("python3 docs/plans/compiler_rewrite_notes/review/measurements/tools/oracle_report.py  # renders this file")
    lines.append("```")
    lines.append("")
    OUT.write_text("\n".join(lines) + "\n")
    print(json.dumps(totals, indent=2))


if __name__ == "__main__":
    main()
