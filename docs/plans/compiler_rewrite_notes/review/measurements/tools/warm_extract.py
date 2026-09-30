#!/usr/bin/env python3
"""Extract the warm numbers from a `cargo xtask verify-compiler-interactions` report.

Usage: warm_extract.py <compiler-interactions.json> [--edits]

Prints the scored edit -> diagnostics and edit -> verified-preview summaries
(min / median / p95 / max, plus the report's own p50/p95/p99/max), the
switch and cancellation evidence, and the evaluation flags. With --edits the
per-edit raw rows of the product lane are printed too. Read-only.
"""
import json
import statistics
import sys


def q(xs, f):
    xs = sorted(xs)
    if not xs:
        return float("nan")
    return xs[min(len(xs) - 1, int(round(f * (len(xs) - 1))))]


def summ(label, xs):
    if not xs:
        print(f"  {label}: no samples")
        return
    print(
        f"  {label}: n={len(xs)} min={min(xs):.1f} median={statistics.median(xs):.1f} "
        f"p95={q(xs, 0.95):.1f} max={max(xs):.1f}"
    )


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    show_edits = "--edits" in sys.argv
    path = args[0]
    with open(path) as f:
        r = json.load(f)
    print(f"report: {path}")
    print(f"  contract={r['contract']} format={r['format_version']} status={r['status']} "
          f"run_classification={r['run_classification']}")
    p = r["protocol"]
    print(f"  effective setup/scored = {p['effective_setup_samples']}/{p['effective_scored_samples']} "
          f"(budget default {p['default_setup_samples']}/{p['default_scored_samples']}) "
          f"threads={p['compiler_threads']} caches={p['compiler_caches']}")
    for lane in ("product", "evidence"):
        pr = r["producers"][lane]
        md = pr["metadata"]
        print(f"  {lane}: {pr['path']} sha256={pr['sha256'][:16]}… head={md.get('build_source_head', '?')[:12]} "
              f"dirty={md.get('build_source_dirty')} instr={md.get('allocation_instrumentation')}")
    w = r["warm"]
    print(f"warm: status={w['status']} peak_rss_kib={w['peak_rss_kib']} "
          f"cancellation_stop_ms={w['cancellation_stop_ms']:.4f} latest_generation_publish_ms={w['latest_generation_publish_ms']:.1f}")
    for k in ("diagnostics_edit_to_ready_ms", "verified_preview_edit_to_ready_ms", "update_ack_ms",
              "loaded_bundle_lookup_ms", "switch_ack_ms"):
        s = w[k]
        print(f"  report.{k}: n={s['sample_count']} p50={s['p50']:.4f} p95={s['p95']:.4f} p99={s['p99']:.4f} max={s['max']:.4f}")
    for lane_key in ("raw", "evidence_raw"):
        b = w[lane_key]
        edits = [e for e in b["edits"] if e["scored"]]
        all_edits = b["edits"]
        print(f"lane {lane_key}: pid={b['producer_pid']} setup/scored={b['setup_samples']}/{b['scored_samples']} "
              f"requests={b['compiler_request_count']} rss initial/final/peak KiB={b['initial_resident_rss_kib']}/{b['final_resident_rss_kib']}/{b['peak_rss_kib']}")
        summ("scored edit->diagnostics ms", [e["edit_to_diagnostics_ms"] for e in edits])
        summ("scored edit->verified preview ms", [e["edit_to_verified_preview_ms"] for e in edits])
        summ("scored diagnostics request only ms", [e["diagnostics_request_ms"] for e in edits])
        summ("scored preview request only ms", [e["verified_preview_request_ms"] for e in edits])
        summ("scored update ack ms", [e["update_ack_ms"] for e in edits])
        summ("ALL edits (incl setup) edit->diagnostics ms", [e["edit_to_diagnostics_ms"] for e in all_edits])
        summ("ALL edits (incl setup) edit->verified preview ms", [e["edit_to_verified_preview_ms"] for e in all_edits])
        sw = [s for s in b["switches"] if s["scored"]]
        summ("scored switch ack ms", [s["acknowledgement_ms"] for s in sw])
        summ("scored loaded bundle lookup ms", [s["loaded_bundle_lookup_ms"] for s in sw])
        print(f"  switches: no-compile={all(s['compiler_requests_before'] == s['compiler_requests_after'] for s in sw)} "
              f"alloc_calls={[s['allocation_calls'] for s in sw]} alloc_bytes={[s['allocated_bytes'] for s in sw]}")
        c = b["cancellation"]
        print(f"  cancellation: scope={c['scope']} token_canceled_before_request={c['token_canceled_before_request']} "
              f"request_rejected={c['request_rejected']} stop_latency_ms={c['stop_latency_ms']:.4f} "
              f"publication_unchanged={c['publication_unchanged']} in_flight_supersession_supported={c['in_flight_supersession_supported']}")
        lg = b["latest_generation"]
        print(f"  latest_generation: stale_rejected={lg['stale_request_rejected']} no_stale_publication={lg['no_stale_publication']} "
              f"publish_latest_ms={lg['publish_latest_ms']:.1f} published={lg['published_revision']} latest={lg['latest_revision']}")
        if edits:
            e = edits[0]
            dp, pp = e["diagnostics_phase"], e["preview_phase"]
            print("  first scored edit phases (diagnostics): " + ", ".join(f"{k}={v:.1f}" for k, v in dp.items() if v))
            print("  first scored edit phases (preview):     " + ", ".join(f"{k}={v:.1f}" for k, v in pp.items() if v))
            dw, pw = e["diagnostics_work"], e["preview_work"]
            print(f"  first scored edit diagnostics_work.parse: {dw.get('parse')}")
            print(f"  first scored edit preview_work.parse:     {pw.get('parse')}")
        if show_edits:
            for e in all_edits:
                print(f"    edit seq={e['sequence']} scored={e['scored']} dir={e['direction']} rev={e['revision']} "
                      f"ack={e['update_ack_ms']:.4f} diag_req={e['diagnostics_request_ms']:.1f} edit->diag={e['edit_to_diagnostics_ms']:.1f} "
                      f"prev_req={e['verified_preview_request_ms']:.1f} edit->prev={e['edit_to_verified_preview_ms']:.1f} rss={e['resident_rss_kib']}")
    ev = w["evaluation"]
    print("evaluation flags:")
    for k, v in ev.items():
        print(f"  {k} = {v}")
    print(f"missing_acceptance_evidence: {r['missing_acceptance_evidence']}")
    print("scaling:")
    for s in r["scaling"]:
        print(f"  {s['id']}: status={s['status']} base p50={s['base']['elapsed_ms']['p50']:.1f}ms doubled p50={s['doubled']['elapsed_ms']['p50']:.1f}ms "
              f"owning_ratio={s['owning_work_fixed_overhead_ratio']}")


if __name__ == "__main__":
    main()
