#!/usr/bin/env python3
"""Summarise raw/proto/bench_*.txt into min / median / p95 tables (markdown on stdout,
JSON in raw/proto/summary.json).

Two kinds of raw file:
  bench_orig_<set>.txt  unmodified prototype: each process prints ONE line
                        "files=.. tokens=.. nodes=.. lex_ms=X lex+parse+defs_ms=Y .." where
                        X and Y are the BEST (minimum) of its 30 internal iterations, and one
                        "warm: ... best_ms=Z" line (best of 30 warm re-parses of the largest file).
  bench_iter_<set>.txt  patched copy: additionally prints "iter lex_ms=X all_ms=Y" for EVERY
                        internal iteration (30 per process), so the distribution over all
                        RUNS*30 iterations can be reported.

Percentiles use linear interpolation on the sorted sample (numpy's default 'linear').
"""
import json
import os
import re
import statistics
import sys

REPO = "/home/martinkavik/repos/boon-circuit"
RAW = os.path.join(REPO, "docs/plans/compiler_rewrite_notes/review/measurements/raw/proto")
SETS = ["counter", "todo_run", "todo", "novywave_run", "novywave", "examples", "all_tracked"]
LABEL = {
    "counter": "counter.bn (1 file)",
    "todo_run": "todo_mvc_physical/RUN.bn alone (1 file)",
    "todo": "TodoMVC unit set (8 files)",
    "novywave_run": "novywave/RUN.bn alone (1 file)",
    "novywave": "NovyWave unit set (8 files)",
    "examples": "all tracked examples/**/*.bn (142 files)",
    "all_tracked": "all tracked *.bn (177 files)",
}


def pct(sorted_vals, p):
    n = len(sorted_vals)
    if n == 1:
        return sorted_vals[0]
    pos = p * (n - 1)
    lo = int(pos)
    hi = min(lo + 1, n - 1)
    return sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * (pos - lo)


def stats(vals):
    s = sorted(vals)
    return {"n": len(s), "min": s[0], "median": statistics.median(s), "p95": pct(s, 0.95), "max": s[-1]}


def lines_and_bytes(set_name):
    lines = 0
    nbytes = 0
    with open(os.path.join(RAW, f"files_{set_name}.txt")) as f:
        for path in f.read().split():
            with open(os.path.join(REPO, path), "rb") as src:
                data = src.read()
            lines += data.count(b"\n")
            nbytes += len(data)
    return lines, nbytes


BEST_RE = re.compile(
    r"files=(\d+) bytes=(\d+) tokens=(\d+) nodes=(\d+) defs=(\d+) diags=(\d+) "
    r"lex_ms=([\d.]+) lex\+parse\+defs_ms=([\d.]+)"
)
WARM_RE = re.compile(r"warm: reparse (\S+) \((\d+) bytes\) \+ fingerprint \+ diff (\d+) defs: best_ms=([\d.]+)")
ITER_RE = re.compile(r"^iter lex_ms=([\d.]+) all_ms=([\d.]+)$")


def parse(path):
    best_lex, best_all, warm, it_lex, it_all, runs, meta = [], [], [], [], [], 0, None
    if not os.path.exists(path):
        return None
    with open(path) as f:
        for line in f:
            if line.startswith("### run"):
                runs += 1
                continue
            m = BEST_RE.search(line)
            if m:
                meta = dict(files=int(m[1]), bytes=int(m[2]), tokens=int(m[3]), nodes=int(m[4]), defs=int(m[5]), diags=int(m[6]))
                best_lex.append(float(m[7]))
                best_all.append(float(m[8]))
                continue
            m = WARM_RE.search(line)
            if m:
                warm.append(float(m[4]))
                continue
            m = ITER_RE.match(line)
            if m:
                it_lex.append(float(m[1]))
                it_all.append(float(m[2]))
    return dict(runs=runs, meta=meta, best_lex=best_lex, best_all=best_all, warm=warm, it_lex=it_lex, it_all=it_all)


def fmt(x):
    return f"{x:.3f}"


def main():
    summary = {}
    for tag in ("orig", "iter"):
        for s in SETS:
            r = parse(os.path.join(RAW, f"bench_{tag}_{s}.txt"))
            if r is None or not r["best_all"]:
                continue
            lines, nbytes = lines_and_bytes(s)
            entry = dict(set=s, tag=tag, runs=r["runs"], lines=lines, bytes=nbytes, meta=r["meta"])
            entry["best_all"] = stats(r["best_all"])
            entry["best_lex"] = stats(r["best_lex"])
            if r["warm"]:
                entry["warm"] = stats(r["warm"])
            if r["it_all"]:
                entry["iter_all"] = stats(r["it_all"])
                entry["iter_lex"] = stats(r["it_lex"])
            summary[f"{tag}:{s}"] = entry

    with open(os.path.join(RAW, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)

    print("### Unmodified prototype, `bench` mode: per-process best-of-30 `lex+parse+defs_ms`, over 20 processes\n")
    print("| set | lines | bytes | tokens | nodes | defs | diags | min ms | median ms | p95 ms | max ms | lines/s (median) | MB/s (median) |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for s in SETS:
        e = summary.get(f"orig:{s}")
        if not e:
            continue
        m, a = e["meta"], e["best_all"]
        med_s = a["median"] / 1e3
        print(f"| {LABEL[s]} | {e['lines']:,} | {e['bytes']:,} | {m['tokens']:,} | {m['nodes']:,} | {m['defs']:,} | {m['diags']} | "
              f"{fmt(a['min'])} | {fmt(a['median'])} | {fmt(a['p95'])} | {fmt(a['max'])} | {e['lines']/med_s:,.0f} | {e['bytes']/med_s/1e6:.1f} |")

    print("\n### Unmodified prototype: per-process best-of-30 `lex_ms` (lexer only) and warm re-parse `best_ms`, over 20 processes\n")
    print("| set | lex min | lex median | lex p95 | warm min | warm median | warm p95 | warm target |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |")
    for s in SETS:
        e = summary.get(f"orig:{s}")
        if not e:
            continue
        l = e["best_lex"]
        w = e.get("warm")
        ws = f"{fmt(w['min'])} | {fmt(w['median'])} | {fmt(w['p95'])}" if w else "- | - | -"
        print(f"| {LABEL[s]} | {fmt(l['min'])} | {fmt(l['median'])} | {fmt(l['p95'])} | {ws} | largest file of the set |")

    print("\n### Patched copy: EVERY iteration (20 processes x 30 iterations = 600 samples) of `lex+parse+defs` per set\n")
    print("| set | n | min ms | median ms | p95 ms | max ms | lines/s (median) | lines/s (p95) |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for s in SETS:
        e = summary.get(f"iter:{s}")
        if not e or "iter_all" not in e:
            continue
        a = e["iter_all"]
        print(f"| {LABEL[s]} | {a['n']} | {fmt(a['min'])} | {fmt(a['median'])} | {fmt(a['p95'])} | {fmt(a['max'])} | "
              f"{e['lines']/(a['median']/1e3):,.0f} | {e['lines']/(a['p95']/1e3):,.0f} |")

    print("\n### Patched copy: every iteration, lexer only\n")
    print("| set | n | min ms | median ms | p95 ms | max ms |")
    print("| --- | ---: | ---: | ---: | ---: | ---: |")
    for s in SETS:
        e = summary.get(f"iter:{s}")
        if not e or "iter_lex" not in e:
            continue
        a = e["iter_lex"]
        print(f"| {LABEL[s]} | {a['n']} | {fmt(a['min'])} | {fmt(a['median'])} | {fmt(a['p95'])} | {fmt(a['max'])} |")


if __name__ == "__main__":
    main()
