#!/usr/bin/env python3
"""Summarize one or more raw `boon_cli compiler-sample warm-session` JSON batches.

Usage: warm_session_extract.py <batch.json> [<batch.json> ...] [--edits]

For each batch prints producer identity, scored edit -> diagnostics and
edit -> verified-preview distributions (min / median / p95 / p99 / max, the
p95 uses the same nearest-rank-on-index method as the xtask summarize_ms),
the request-only splits, the phase medians, the switch numbers, the
cancellation and latest-generation evidence. Read-only.
"""
import json
import statistics
import sys


def q(xs, f):
    xs = sorted(xs)
    if not xs:
        return float("nan")
    return xs[min(len(xs) - 1, int(round(f * (len(xs) - 1))))]


def dist(label, xs):
    if not xs:
        print(f"  {label}: no samples")
        return
    print(
        f"  {label}: n={len(xs)} min={min(xs):.1f} median={statistics.median(xs):.1f} "
        f"p95={q(xs, 0.95):.1f} p99={q(xs, 0.99):.1f} max={max(xs):.1f}"
    )


def main():
    paths = [a for a in sys.argv[1:] if not a.startswith("--")]
    show_edits = "--edits" in sys.argv
    for path in paths:
        with open(path) as f:
            b = json.load(f)
        md = b["producer"]
        print(f"batch: {path}")
        print(f"  producer={md['binary_path']} sha256={md['binary_sha256'][:16]}… head={md['build_source_head'][:12]} "
              f"dirty={md['build_source_dirty']} kind={md['product_kind']} instr={md['allocation_instrumentation']}")
        print(f"  workload={b['workload']} pid={b['producer_pid']} setup/scored={b['setup_samples']}/{b['scored_samples']} "
              f"threads={b['compiler_threads']} caches={b['compiler_caches']} requests={b['compiler_request_count']}")
        print(f"  rss KiB initial/final/peak = {b['initial_resident_rss_kib']}/{b['final_resident_rss_kib']}/{b['peak_rss_kib']} "
              f"(peak {b['peak_rss_kib']/1024:.1f} MiB)")
        edits = b["edits"]
        scored = [e for e in edits if e["scored"]]
        setup = [e for e in edits if not e["scored"]]
        dist("scored edit->diagnostics ms", [e["edit_to_diagnostics_ms"] for e in scored])
        dist("scored edit->verified preview ms", [e["edit_to_verified_preview_ms"] for e in scored])
        dist("scored preview request only ms", [e["verified_preview_request_ms"] for e in scored])
        dist("scored update ack ms", [e["update_ack_ms"] for e in scored])
        if setup:
            dist("setup edit->diagnostics ms", [e["edit_to_diagnostics_ms"] for e in setup])
            dist("setup edit->verified preview ms", [e["edit_to_verified_preview_ms"] for e in setup])
        fwd = [e for e in scored if e["direction"] == "forward"]
        rev = [e for e in scored if e["direction"] == "reverse"]
        dist("scored forward edit->diagnostics ms", [e["edit_to_diagnostics_ms"] for e in fwd])
        dist("scored reverse edit->diagnostics ms", [e["edit_to_diagnostics_ms"] for e in rev])
        if scored:
            keys = list(scored[0]["diagnostics_phase"].keys())
            print("  diagnostics phase medians ms: " + ", ".join(
                f"{k}={statistics.median(e['diagnostics_phase'][k] for e in scored):.1f}" for k in keys
                if statistics.median(e['diagnostics_phase'][k] for e in scored) > 0))
            print("  preview phase medians ms:     " + ", ".join(
                f"{k}={statistics.median(e['preview_phase'][k] for e in scored):.1f}" for k in keys
                if statistics.median(e['preview_phase'][k] for e in scored) > 0))
            dw = scored[0]["diagnostics_work"]["parse"]
            pw = scored[0]["preview_work"]["parse"]
            print(f"  diagnostics parse work (1st scored): attempted={dw['source_units_attempted']} parsed={dw['source_units_parsed']} reused={dw['source_units_reused']}")
            print(f"  preview parse work (1st scored):     attempted={pw['source_units_attempted']} parsed={pw['source_units_parsed']} reused={pw['source_units_reused']}")
            plans = {e["plan_sha256"] for e in scored if e["direction"] == "forward"}
            print(f"  distinct forward plan hashes: {len(plans)}; initial plan {b['initial_plan_sha256'][:16]}…")
        sw = [s for s in b["switches"] if s["scored"]]
        dist("scored switch ack ms", [s["acknowledgement_ms"] for s in sw])
        dist("scored loaded bundle lookup ms", [s["loaded_bundle_lookup_ms"] for s in sw])
        print(f"  switches: no-compile={all(s['compiler_requests_before'] == s['compiler_requests_after'] for s in sw)} "
              f"alloc_calls_total={sum(s['allocation_calls'] for s in sw)} alloc_bytes_total={sum(s['allocated_bytes'] for s in sw)}")
        c = b["cancellation"]
        print(f"  cancellation: scope={c['scope']} canceled_before={c['token_canceled_before_request']} rejected={c['request_rejected']} "
              f"stop_latency_ms={c['stop_latency_ms']:.4f} publication_unchanged={c['publication_unchanged']} "
              f"in_flight_supersession_supported={c['in_flight_supersession_supported']}")
        lg = b["latest_generation"]
        print(f"  latest_generation: stale_rejected={lg['stale_request_rejected']} no_stale_publication={lg['no_stale_publication']} "
              f"publish_latest_ms={lg['publish_latest_ms']:.1f} published={lg['published_revision']} latest={lg['latest_revision']}")
        if show_edits:
            for e in edits:
                print(f"    seq={e['sequence']:2d} scored={e['scored']!s:5} dir={e['direction']:7} rev={e['revision']:2d} "
                      f"diag={e['edit_to_diagnostics_ms']:7.1f} prev_req={e['verified_preview_request_ms']:7.1f} "
                      f"edit->prev={e['edit_to_verified_preview_ms']:7.1f} rss={e['resident_rss_kib']}")
        print()


if __name__ == "__main__":
    main()
