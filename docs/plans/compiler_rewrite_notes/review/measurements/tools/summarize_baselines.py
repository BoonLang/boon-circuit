#!/usr/bin/env python3
"""Aggregate raw compiler-sample / time -v / evidence outputs into summary.json and markdown tables."""
import glob, json, os, re, statistics, sys

RAW = "/home/martinkavik/repos/boon-circuit/docs/plans/compiler_rewrite_notes/review/measurements/raw/baselines"
LINES = {"counter": 140, "todomvc": 3576, "novywave": 11926}  # budgets/compiler.toml compiler_input_source_lines
PLAN = {  # BOON_COMPILER_REWRITE_PLAN.md section 2 table (ms)
    ("counter", "diagnostics"): {"total": 7, "parse": 3.5, "typecheck": 3.4},
    ("counter", "verified"): {"total": 16, "parse": 3.7, "typecheck": 6.6, "semantic": 4.8, "ir": 0.1, "backend": 0.4, "plan_validation": 0.4},
    ("todomvc", "diagnostics"): {"total": 390, "parse": 18, "typecheck": 370},
    ("todomvc", "verified"): {"total": 790, "parse": 18, "typecheck": 435, "semantic": 158, "ir": 3, "backend": 85, "plan_validation": 90},
    ("novywave", "diagnostics"): {"total": 526, "parse": 64, "typecheck": 457},
    ("novywave", "verified"): {"total": 1870, "parse": 64, "typecheck": 713, "semantic": 780, "ir": 25, "backend": 214, "plan_validation": 66},
}
PLAN_RSS_MIB = {"todomvc": 154, "novywave": 314}
PLAN_ALLOC = {"novywave": (7.46e6, 1.33e9)}
ORACLE = {"counter": "5158476e5e9f095ef8c8eb3d9041af4e103d428d5ffe0c691d2c8daa687e065d", "todomvc": "8e7120c33db1d5a5c03a1d5aaeb9cc112e102fafed8c0e518c557903309eb949", "novywave": "e2d673e3116f03622731f695cf2fb598d3313077845013f776930d38cc2acbea"}  # budgets/compiler.toml machine_plan_sha256

def pct(sorted_vals, q):
    if len(sorted_vals) == 1:
        return sorted_vals[0]
    pos = q * (len(sorted_vals) - 1)
    lo, hi = int(pos // 1), min(int(pos // 1) + 1, len(sorted_vals) - 1)
    return sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * (pos - lo)

def stats(vals):
    s = sorted(vals)
    return {"n": len(s), "min": s[0], "median": statistics.median(s), "p95": pct(s, 0.95), "max": s[-1], "first": vals[0]}

def verdict(plan, measured):
    if plan is None or measured is None:
        return "n/a"
    if abs(measured - plan) <= 0.10 * plan:
        return "confirmed"
    return "higher" if measured > plan else "lower"

def load_series():
    series = {}
    for path in sorted(glob.glob(f"{RAW}/*_*_*_*_[0-9]*.json")):
        name = os.path.basename(path)[:-5]
        m = re.match(r"(head|old|evidence)_(counter|todomvc|novywave)_(diagnostics|verified)_(fresh-process|empty-session)_(\d+)$", name)
        if not m:
            continue
        lane, fixture, intent, mode, idx = m.groups()
        d = json.load(open(path))
        s = d["samples"][0]
        rec = {
            "idx": int(idx), "elapsed_ms": s["elapsed_ms"], "cpu_ms": s["compiler_cpu_ms"],
            "peak_rss_kib": s["peak_rss_kib"], "minor_faults": s["compiler_minor_page_faults"],
            "phase": s["phase"], "plan_sha256": s.get("plan_sha256"), "diag_fp": s.get("diagnostics_fingerprint_v1"),
            "alloc_calls": s["allocations"]["allocation_calls"], "alloc_bytes": s["allocations"]["allocated_bytes"],
            "checked_calls": s["work"]["checked_calls"], "compiled_call_sites": s["work"]["kernel_compile"]["compiled_call_sites"],
            "producer_head": d["producer"]["build_source_head"], "producer_dirty": d["producer"]["build_source_dirty"],
            "binary_sha256": d["producer"]["binary_sha256"], "instrumentation": d["producer"]["allocation_instrumentation"],
        }
        series.setdefault((lane, fixture, intent, mode), []).append(rec)
    for k in series:
        series[k].sort(key=lambda r: r["idx"])
    return series

PHASES = ["parse_ms", "typecheck_ms", "semantic_ms", "contract_verify_ms", "ir_lower_ms", "ir_validation_ms", "backend_ms", "plan_validation_ms"]

def summarize(series):
    out = {}
    for key, recs in series.items():
        lane, fixture, intent, mode = key
        entry = {
            "lane": lane, "fixture": fixture, "intent": intent, "mode": mode, "n": len(recs),
            "producer_head": recs[0]["producer_head"], "producer_dirty": recs[0]["producer_dirty"], "binary_sha256": recs[0]["binary_sha256"],
            "elapsed_ms": stats([r["elapsed_ms"] for r in recs]),
            "cpu_ms": stats([r["cpu_ms"] for r in recs]),
            "peak_rss_kib": stats([r["peak_rss_kib"] for r in recs]),
            "phase": {p: stats([r["phase"][p] for r in recs]) for p in PHASES},
            "ir_sum_ms": stats([r["phase"]["ir_lower_ms"] + r["phase"]["ir_validation_ms"] for r in recs]),
            "phase_sum_ms": stats([sum(r["phase"][p] for p in PHASES) for r in recs]),
            "plan_sha256": sorted({r["plan_sha256"] for r in recs if r["plan_sha256"]}),
            "diag_fingerprints": sorted({r["diag_fp"] for r in recs if r["diag_fp"]}),
            "checked_calls": recs[0]["checked_calls"], "compiled_call_sites": recs[0]["compiled_call_sites"],
            "samples_elapsed_ms": [round(r["elapsed_ms"], 2) for r in recs],
        }
        if lane == "evidence":
            entry["alloc_calls"] = stats([r["alloc_calls"] for r in recs])
            entry["alloc_bytes"] = stats([r["alloc_bytes"] for r in recs])
        med = entry["elapsed_ms"]["median"]
        entry["throughput_lines_per_s"] = LINES[fixture] / (med / 1000.0)
        entry["parse_throughput_lines_per_s"] = LINES[fixture] / (entry["phase"]["parse_ms"]["median"] / 1000.0)
        out["|".join(key)] = entry
    return out

def load_time_v():
    out = {}
    for path in sorted(glob.glob(f"{RAW}/time_*_*_[0-9].txt")):
        m = re.match(r"time_(check|dump-plan)_(counter|todomvc|novywave)_(\d)\.txt$", os.path.basename(path))
        if not m:
            continue
        cmd, fixture, idx = m.groups()
        txt = open(path).read()
        rss = int(re.search(r"Maximum resident set size \(kbytes\): (\d+)", txt).group(1))
        wall = re.search(r"Elapsed \(wall clock\) time .*: (\S+)", txt).group(1)
        user = float(re.search(r"User time \(seconds\): (\S+)", txt).group(1))
        mm, ss = wall.split(":")[-2:]
        wall_s = int(mm) * 60 + float(ss)
        out.setdefault((cmd, fixture), []).append({"idx": int(idx), "max_rss_kib": rss, "wall_s": wall_s, "user_s": user})
    summary = {}
    for (cmd, fixture), recs in out.items():
        summary[f"{cmd}|{fixture}"] = {"cmd": cmd, "fixture": fixture, "n": len(recs),
            "max_rss_kib": stats([r["max_rss_kib"] for r in recs]), "wall_s": stats([r["wall_s"] for r in recs]), "user_s": stats([r["user_s"] for r in recs])}
    return summary

def fmt(v, nd=1):
    return f"{v:,.{nd}f}"

def main():
    series = load_series()
    summary = summarize(series)
    timev = load_time_v()
    json.dump({"series": summary, "time_v": timev}, open(f"{RAW}/summary.json", "w"), indent=1)
    md = []
    md.append("### Cold totals (fresh-process, one process per observation, ms)\n")
    md.append("| fixture | intent | plan | HEAD min / median / p95 (max) | HEAD verdict | sep28 min / median / p95 (max) | sep28 verdict |")
    md.append("| --- | --- | ---: | ---: | --- | ---: | --- |")
    for fixture in ["counter", "todomvc", "novywave"]:
        for intent in ["diagnostics", "verified"]:
            h = summary.get(f"head|{fixture}|{intent}|fresh-process"); o = summary.get(f"old|{fixture}|{intent}|fresh-process")
            plan = PLAN[(fixture, intent)]["total"]
            def cell(e):
                s = e["elapsed_ms"]; return f"{fmt(s['min'])} / {fmt(s['median'])} / {fmt(s['p95'])} ({fmt(s['max'])})"
            md.append(f"| {fixture} | {intent} | {plan} | {cell(h)} | {verdict(plan, h['elapsed_ms']['median'])} | {cell(o)} | {verdict(plan, o['elapsed_ms']['median'])} |")
    md.append("\n### Per-phase medians (fresh-process, ms; plan value / HEAD median / sep28 median, verdict on HEAD)\n")
    md.append("| fixture | intent | parse | typecheck | semantic | contract_verify | IR (lower+validation) | backend | plan validation | phase sum vs total |")
    md.append("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for fixture in ["counter", "todomvc", "novywave"]:
        for intent in ["diagnostics", "verified"]:
            h = summary[f"head|{fixture}|{intent}|fresh-process"]; o = summary[f"old|{fixture}|{intent}|fresh-process"]; p = PLAN[(fixture, intent)]
            def ph(name, key):
                pv = p.get(name)
                hv = h["ir_sum_ms"]["median"] if key == "ir" else h["phase"][key]["median"]
                ov = o["ir_sum_ms"]["median"] if key == "ir" else o["phase"][key]["median"]
                if pv is None:
                    return f"— / {fmt(hv, 2)} / {fmt(ov, 2)}"
                return f"{pv} / {fmt(hv, 2)} / {fmt(ov, 2)} ({verdict(pv, hv)})"
            cells = [ph("parse", "parse_ms"), ph("typecheck", "typecheck_ms"), ph("semantic", "semantic_ms"), ph(None, "contract_verify_ms"),
                     ph("ir", "ir"), ph("backend", "backend_ms"), ph("plan_validation", "plan_validation_ms")]
            md.append(f"| {fixture} | {intent} | " + " | ".join(cells) + f" | {fmt(h['phase_sum_ms']['median'])} vs {fmt(h['elapsed_ms']['median'])} |")
    md.append("\n### Empty-session (in-process CompilerSession) vs fresh-process, medians (ms)\n")
    md.append("| fixture | intent | HEAD fresh | HEAD session (min/med/p95) | sep28 fresh | sep28 session (min/med/p95) |")
    md.append("| --- | --- | ---: | ---: | ---: | ---: |")
    for fixture in ["todomvc", "novywave"]:
        for intent in ["diagnostics", "verified"]:
            hf = summary[f"head|{fixture}|{intent}|fresh-process"]["elapsed_ms"]; hs = summary[f"head|{fixture}|{intent}|empty-session"]["elapsed_ms"]
            of = summary[f"old|{fixture}|{intent}|fresh-process"]["elapsed_ms"]; os_ = summary[f"old|{fixture}|{intent}|empty-session"]["elapsed_ms"]
            md.append(f"| {fixture} | {intent} | {fmt(hf['median'])} | {fmt(hs['min'])} / {fmt(hs['median'])} / {fmt(hs['p95'])} | {fmt(of['median'])} | {fmt(os_['min'])} / {fmt(os_['median'])} / {fmt(os_['p95'])} |")
    md.append("\n### Peak RSS\n")
    md.append("| fixture | plan (MiB, verified) | compiler-sample verified HEAD VmHWM min/med/max (MiB) | compiler-sample diagnostics HEAD (MiB) | time -v check max RSS min/med/max (MiB) | time -v dump-plan --out (MiB) | verdict (plan vs medians) |")
    md.append("| --- | ---: | ---: | ---: | ---: | ---: | --- |")
    for fixture in ["counter", "todomvc", "novywave"]:
        v = summary[f"head|{fixture}|verified|fresh-process"]["peak_rss_kib"]; d = summary[f"head|{fixture}|diagnostics|fresh-process"]["peak_rss_kib"]
        c = timev[f"check|{fixture}"]["max_rss_kib"]; dp = timev[f"dump-plan|{fixture}"]["max_rss_kib"]
        plan = PLAN_RSS_MIB.get(fixture)
        k = lambda s: f"{s['min']/1024:.1f} / {s['median']/1024:.1f} / {s['max']/1024:.1f}"
        md.append(f"| {fixture} | {plan if plan else '—'} | {k(v)} | {k(d)} | {k(c)} | {k(dp)} | {('sample ' + verdict(plan, v['median']/1024) + ', dump-plan ' + verdict(plan, dp['median']/1024) + ', check ' + verdict(plan, c['median']/1024)) if plan else 'n/a'} |")
    md.append("\n### Allocation evidence (target/release/boon_cli_evidence, fresh-process, 3 runs)\n")
    md.append("| fixture | intent | plan | allocation calls min/med/max | allocated bytes min/med/max | elapsed median ms | verdict |")
    md.append("| --- | --- | ---: | ---: | ---: | ---: | --- |")
    for fixture in ["counter", "todomvc", "novywave"]:
        for intent in ["diagnostics", "verified"]:
            e = summary.get(f"evidence|{fixture}|{intent}|fresh-process")
            if not e:
                continue
            a = e["alloc_calls"]; b = e["alloc_bytes"]
            plan = PLAN_ALLOC.get(fixture) if intent == "verified" else None
            pv = f"{plan[0]/1e6:.2f} M / {plan[1]/1e9:.2f} GB" if plan else "—"
            vd = f"calls {verdict(plan[0], a['median'])}, bytes {verdict(plan[1], b['median'])}" if plan else "n/a"
            md.append(f"| {fixture} | {intent} | {pv} | {a['min']/1e6:.3f} / {a['median']/1e6:.3f} / {a['max']/1e6:.3f} M | {b['min']/1e9:.3f} / {b['median']/1e9:.3f} / {b['max']/1e9:.3f} GB | {fmt(e['elapsed_ms']['median'])} | {vd} |")
    md.append("\n### Throughput (compiler_input_source_lines / HEAD median, fresh-process)\n")
    md.append("| fixture | lines | diagnostics k lines/s (HEAD / sep28) | verified k lines/s (HEAD / sep28) | parse k lines/s from verified parse_ms (HEAD / sep28) |")
    md.append("| --- | ---: | ---: | ---: | ---: |")
    for fixture in ["counter", "todomvc", "novywave"]:
        hd = summary[f"head|{fixture}|diagnostics|fresh-process"]; hv = summary[f"head|{fixture}|verified|fresh-process"]
        od = summary[f"old|{fixture}|diagnostics|fresh-process"]; ov = summary[f"old|{fixture}|verified|fresh-process"]
        md.append(f"| {fixture} | {LINES[fixture]:,} | {hd['throughput_lines_per_s']/1000:.1f} / {od['throughput_lines_per_s']/1000:.1f} | {hv['throughput_lines_per_s']/1000:.1f} / {ov['throughput_lines_per_s']/1000:.1f} | {hv['parse_throughput_lines_per_s']/1000:.0f} / {ov['parse_throughput_lines_per_s']/1000:.0f} |")
    md.append("\n### Work counters (HEAD verified, fresh-process)\n")
    md.append("| fixture | checked_calls | compiled_call_sites | HEAD plan_sha256 (stable across samples?) | sep28 plan_sha256 | budgets/compiler.toml oracle | HEAD == sep28 | HEAD == oracle |")
    md.append("| --- | ---: | ---: | --- | --- | --- | --- | --- |")
    for fixture in ["counter", "todomvc", "novywave"]:
        e = summary[f"head|{fixture}|verified|fresh-process"]; o = summary[f"old|{fixture}|verified|fresh-process"]
        md.append(f"| {fixture} | {e['checked_calls']:,} | {e['compiled_call_sites']:,} | {e['plan_sha256'][0][:16]}… ({'stable' if len(e['plan_sha256'])==1 else 'UNSTABLE: '+str(len(e['plan_sha256']))}) | {o['plan_sha256'][0][:16]}… | {ORACLE[fixture][:16]}… | {'yes' if e['plan_sha256']==o['plan_sha256'] else 'NO'} | {'yes' if e['plan_sha256']==[ORACLE[fixture]] else 'NO'} |")
    md.append("\n### Diagnostics fingerprints (HEAD vs sep28, fresh-process)\n")
    for fixture in ["counter", "todomvc", "novywave"]:
        e = summary[f"head|{fixture}|diagnostics|fresh-process"]; o = summary[f"old|{fixture}|diagnostics|fresh-process"]
        md.append(f"- {fixture}: HEAD {e['diag_fingerprints'][0][:16]}… ({len(e['diag_fingerprints'])} distinct), sep28 {o['diag_fingerprints'][0][:16]}… ({len(o['diag_fingerprints'])} distinct), equal: {'yes' if e['diag_fingerprints']==o['diag_fingerprints'] else 'NO'}")
    md.append("\n### Per-sample elapsed (ms), fresh-process, in launch order (sample 00 is the first launch of that binary for that fixture/intent)\n")
    for fixture in ["counter", "todomvc", "novywave"]:
        for intent in ["diagnostics", "verified"]:
            for lane in ["head", "old"]:
                e = summary[f"{lane}|{fixture}|{intent}|fresh-process"]
                md.append(f"- {lane} {fixture} {intent}: {e['samples_elapsed_ms']}")
    print("\n".join(md))

if __name__ == "__main__":
    main()
