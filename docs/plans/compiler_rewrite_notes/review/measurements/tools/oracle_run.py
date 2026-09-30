#!/usr/bin/env python3
"""Re-measure the "old compiler as an oracle" claim of BOON_COMPILER_REWRITE_PLAN.md §2.

For every `[[example]]` in examples/manifest.toml (plus fjordpulse's extra
`[[example.programs]]` sources) this runs:

  <HEAD boon_cli> check <source>
  <old  boon_cli> check <source>
  <HEAD boon_cli> run <source> --scenario <scenario>     (when the entry has one)

each with a 120 s timeout, from the repository root (package resolution is
cwd-relative), and stores every raw stdout/stderr/exit code/wall time under
raw/oracle/. A summary.json is written for oracle_report.py.

Usage: python3 oracle_run.py [--only ID[,ID...]]
"""
import json
import os
import subprocess
import sys
import time
import tomllib
from pathlib import Path

REPO = Path("/home/martinkavik/repos/boon-circuit")
HEAD_CLI = REPO / "target/release/boon_cli"
OLD_CLI = Path(
    "/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/"
    "19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/bin/boon_cli.sep28"
)
RAW = REPO / "docs/plans/compiler_rewrite_notes/review/measurements/raw/oracle"
TIMEOUT_S = 120


def uptime() -> str:
    return subprocess.run(["uptime"], capture_output=True, text=True).stdout.strip()


def run_one(label: str, argv: list[str]) -> dict:
    """Run argv from the repo root with a timeout; return a record and write the raw file."""
    started = time.perf_counter()
    started_at = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    rec = {
        "label": label,
        "command": " ".join(argv),
        "cwd": str(REPO),
        "started_at": started_at,
    }
    try:
        proc = subprocess.run(
            argv,
            cwd=REPO,
            capture_output=True,
            text=True,
            errors="replace",
            timeout=TIMEOUT_S,
        )
        rec["exit_code"] = proc.returncode
        rec["timed_out"] = False
        rec["stdout"] = proc.stdout
        rec["stderr"] = proc.stderr
    except subprocess.TimeoutExpired as exc:
        rec["exit_code"] = None
        rec["timed_out"] = True
        rec["stdout"] = (exc.stdout or b"").decode("utf-8", "replace") if isinstance(exc.stdout, bytes) else (exc.stdout or "")
        rec["stderr"] = (exc.stderr or b"").decode("utf-8", "replace") if isinstance(exc.stderr, bytes) else (exc.stderr or "")
    rec["wall_s"] = round(time.perf_counter() - started, 3)

    raw_path = RAW / f"{label}.txt"
    with raw_path.open("w") as fh:
        fh.write(f"# command (cwd={REPO}):\n{rec['command']}\n")
        fh.write(f"# started_at: {started_at}\n")
        fh.write(f"# exit_code: {rec['exit_code']}  timed_out: {rec['timed_out']}  wall_s: {rec['wall_s']}\n")
        fh.write("# ---- stdout ----\n")
        fh.write(rec["stdout"])
        if rec["stdout"] and not rec["stdout"].endswith("\n"):
            fh.write("\n")
        fh.write("# ---- stderr ----\n")
        fh.write(rec["stderr"])
        if rec["stderr"] and not rec["stderr"].endswith("\n"):
            fh.write("\n")
    rec["raw_file"] = str(raw_path.relative_to(REPO))
    return rec


def entries() -> list[dict]:
    manifest = tomllib.loads((REPO / "examples/manifest.toml").read_text())
    out = []
    for ex in manifest["example"]:
        out.append(
            {
                "id": ex["id"],
                "source": ex["source"],
                "scenario": ex.get("scenario"),
                "kind": "example",
            }
        )
        for prog in ex.get("programs", []):
            if prog["source"] == ex["source"]:
                out.append(
                    {
                        "id": f"{ex['id']}/{prog['role']}",
                        "source": prog["source"],
                        "scenario": None,
                        "kind": "program (same source as the example entry; not re-run)",
                        "skip": True,
                    }
                )
            else:
                out.append(
                    {
                        "id": f"{ex['id']}/{prog['role']}",
                        "source": prog["source"],
                        "scenario": None,
                        "kind": f"program role={prog['role']} (check only; boon_cli check always uses ProgramRole::Client)",
                    }
                )
    return out


def main() -> None:
    only = None
    if len(sys.argv) > 2 and sys.argv[1] == "--only":
        only = set(sys.argv[2].split(","))
    RAW.mkdir(parents=True, exist_ok=True)
    for cli in (HEAD_CLI, OLD_CLI):
        if not cli.is_file():
            sys.exit(f"missing binary: {cli}")

    summary = {
        "head_cli": str(HEAD_CLI),
        "old_cli": str(OLD_CLI),
        "head_cli_stat": {"size": HEAD_CLI.stat().st_size, "mtime": time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(HEAD_CLI.stat().st_mtime))},
        "old_cli_stat": {"size": OLD_CLI.stat().st_size, "mtime": time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(OLD_CLI.stat().st_mtime))},
        "git_head": subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True).stdout.strip(),
        "timeout_s": TIMEOUT_S,
        "uptime_before": uptime(),
        "entries": [],
    }
    print(f"uptime before: {summary['uptime_before']}", flush=True)

    for entry in entries():
        if only and entry["id"] not in only:
            continue
        safe = entry["id"].replace("/", "__")
        print(f"== {entry['id']}  {entry['source']}", flush=True)
        if entry.get("skip"):
            summary["entries"].append(entry)
            continue
        entry["check_head"] = run_one(f"{safe}.check-head", [str(HEAD_CLI), "check", entry["source"]])
        print(f"   check HEAD: rc={entry['check_head']['exit_code']} {entry['check_head']['wall_s']}s", flush=True)
        entry["check_old"] = run_one(f"{safe}.check-old", [str(OLD_CLI), "check", entry["source"]])
        print(f"   check old : rc={entry['check_old']['exit_code']} {entry['check_old']['wall_s']}s", flush=True)
        if entry["scenario"]:
            entry["run_head"] = run_one(
                f"{safe}.run-head",
                [str(HEAD_CLI), "run", entry["source"], "--scenario", entry["scenario"]],
            )
            r = entry["run_head"]
            print(f"   run  HEAD : rc={r['exit_code']} timed_out={r['timed_out']} {r['wall_s']}s", flush=True)
        summary["entries"].append(entry)

    summary["uptime_after"] = uptime()
    print(f"uptime after: {summary['uptime_after']}", flush=True)
    (RAW / "summary.json").write_text(json.dumps(summary, indent=2))
    (RAW / "uptime.txt").write_text(
        f"before: {summary['uptime_before']}\nafter:  {summary['uptime_after']}\n"
    )


if __name__ == "__main__":
    main()
