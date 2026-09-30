#!/usr/bin/env python3
"""Summarize a samply profile of `boon_cli compiler-sample` with
nearest-preceding-symbol attribution and time-window scoping.

Symbol resolution. The asm `sha256_compress` in the release binary has size 0
in the ELF symbol table, so a size-based lookup (the one samply uses for its
`.syms.json` sidecar) drops every sample inside it. `boon_cli` frames are
therefore resolved against the full `nm -n -C --defined-only` text-symbol
table by "largest symbol start <= address", ignoring sizes. Other libraries
(libc, ld.so) use the sidecar's tables the same way.

Scoping. samply's unwinder cannot unwind through `sha256_compress` (every
SHA-256 sample is a depth-2 stack: the leaf plus one garbage return address)
and it also truncates deep stacks under large frames (kernel solver,
DocumentCompiler). Stack-based scoping would silently drop those samples, so
the compile region is located by TIME instead: compiler-sample prints
`observation_started_unix_us` / `compiler_artifact_ready_unix_us` per process
(one JSON per line on stdout), the profile's `samples.time` is CLOCK_MONOTONIC
ms and `meta.startTime` is the unix-ms epoch of profile zero; the monotonic
value of profile zero is self-calibrated per profile as the constant that
maximizes containment of the full-stack compile samples in their windows.
Phases are placed inside the window by the cumulative phase timers (the phases
run in a fixed order and the timers sum to elapsed_ms within ~0.3%).
Stack-based phase attribution of the samples that do carry a phase frame is
compared with the time-based one as the independent cross-check.

Profile format (Firefox processed profile, samply 0.13.1): profile.libs[],
profile.threads[] one per process with samples.{stack,time,weight},
stackTable.{frame,prefix} (prefix = caller), frameTable.{address,func,
category} (address is lib-relative), funcTable.{name,resource},
resourceTable.lib -> libs index, per-thread stringArray.

Usage:
  samply_summary.py PROFILE.json.gz --nm boon_cli.nm.txt --stdout RUNS.stdout \
      [--syms PROFILE.json.syms.json] [--top 40] [--label NAME]
"""
from __future__ import annotations

import argparse
import bisect
import gzip
import json
import re
import statistics
import sys
from collections import Counter, defaultdict

SHA_RE = re.compile(r"sha2::|sha256|Sha256|sha256_compress|digest::|<D as digest")
CBOR_RE = re.compile(r"ciborium|serde::|serde_core::|canonical_serde|serde_json")
HASH_INFRA_RE = re.compile(r"sha2::|sha256|Sha256|sha256_compress|digest::|<D as digest|ciborium|serde::|serde_core::"
                           r"|serde_json|canonical_serde|core::fmt|alloc::|hashbrown|core::hash|std::io|<.* as core::hash"
                           r"|^\[|mi_|_mi_|libc|memcpy|memmove|memset|memcmp|core::ptr|core::slice|core::iter")

# Inclusive time for a group = share of samples whose (possibly truncated)
# stack contains a matching frame. A LOWER BOUND where stacks are truncated.
GROUPS = [
    ("SHA-256 (sha2|sha256|Sha256|digest)", SHA_RE.pattern),
    ("CBOR/serde (ciborium|serde|canonical_serde)", CBOR_RE.pattern),
    ("SHA-256 or CBOR/serde (union)", SHA_RE.pattern + "|" + CBOR_RE.pattern),
    ("verify_plan", r"boon_plan::verify_plan|plan_sha256"),
    ("receipt/seal/fingerprint/manifest/digest names",
     r"receipt|Receipt|::seal|Seal|fingerprint|Fingerprint|manifest|Manifest|digest|Digest"),
    ("validate/verify/freeze names (self-validation)",
     r"::validate|::verify|Validate|Verif|freeze_against"),
    ("example_manifest / toml (examples/manifest.toml)", r"boon_example_manifest|example_manifest|toml"),
    ("Debug fmt (as core::fmt::Debug>::fmt)", r"as core::fmt::Debug>::fmt"),
    ("boon_parser", r"boon_parser::"),
    ("boon_syntax", r"boon_syntax::"),
    ("boon_typecheck", r"boon_typecheck::"),
    ("boon_compiler_kernel", r"boon_compiler_kernel::"),
    ("boon_compiler_kernel::solver", r"boon_compiler_kernel::solver"),
    ("boon_compiler_kernel::term (TypeTermArena etc.)", r"boon_compiler_kernel::term"),
    ("boon_checked", r"boon_checked::"),
    ("boon_semantic", r"boon_semantic::"),
    ("boon_ir", r"boon_ir::"),
    ("boon_verify", r"boon_verify::"),
    ("boon_plan", r"boon_plan::"),
    ("boon_compiler (crate; includes kernel_oracle, backends)", r"boon_compiler::"),
    ("boon_compiler::kernel_oracle", r"kernel_oracle::"),
    ("boon_compiler::machine_plan_backend", r"machine_plan_backend::"),
    ("boon_compiler::document_plan_backend", r"document_plan_backend::"),
    ("boon_contract", r"boon_contract::"),
    ("boon_document", r"boon_document::"),
    ("alloc/free (malloc|mi_|alloc::alloc|__rust_alloc|realloc|free)",
     r"\bmalloc\b|\bfree\b|\brealloc\b|\bcalloc\b|\bmi_|_mi_|mimalloc|alloc::alloc::|__rust_alloc|__rust_dealloc|__rdl_|RawVec.*grow|finish_grow"),
    ("BTreeMap (alloc::collections::btree)", r"alloc::collections::btree"),
    ("hashbrown", r"hashbrown::"),
    ("memcpy/memmove/memset/memcmp", r"memcpy|memmove|memset|memcmp|bcmp"),
    ("libc/ld.so/kernel frames at leaf", r"^(libc\.so|ld-linux|\[kernel\])"),
]

INCIDENTAL_NARROW = (SHA_RE.pattern + "|ciborium|canonical_serde"
                     r"|boon_plan::verify_plan|plan_sha256"
                     r"|receipt|Receipt|fingerprint|Fingerprint|::seal|Seal|manifest|Manifest|digest|Digest")
INCIDENTAL_BROAD = (INCIDENTAL_NARROW
                    + r"|::validate|::verify|Validate|Verif|freeze_against"
                    + r"|boon_example_manifest|example_manifest|toml"
                    + r"|variant_set_with_order|as core::fmt::Debug>::fmt")

COMPILE_STACK_RE = re.compile(
    r"boon_compiler::(check_runtime_source|finish_checked_sealed_machine_plan|compile_diagnostics_source"
    r"|finish_checked_machine_plan_with_cancellation|compile_machine_plan|finish_checked_program_to_machine_plan)"
    r"|kernel_oracle::|boon_parser::|boon_typecheck::|boon_compiler_kernel::|boon_semantic::|boon_ir::|boon_verify::"
    r"|machine_plan_backend::|document_plan_backend::|boon_plan::verify_plan|boon_plan::seal|boon_example_manifest")
HARNESS_RE = re.compile(r"compiler_sample::(producer_metadata|compiled_sample|run|hex_digest)|Sha256Writer|std::fs::read"
                        r"|serde_json::ser|boon_cli::main")

# Phase order as executed; cumulative timer offsets place the boundaries.
PHASE_ORDER = ["parse_ms", "typecheck_ms", "semantic_ms", "contract_verify_ms", "ir_lower_ms",
               "ir_validation_ms", "backend_ms", "plan_validation_ms"]
# Stack evidence for a phase (used only for the cross-check against time).
PHASE_STACK_RE = [
    ("parse_ms", re.compile(r"boon_compiler::parse_kernel_compile_source|boon_compiler::source_files_for_path"
                            r"|boon_parser::|boon_syntax::|boon_example_manifest|toml")),
    ("typecheck_ms", re.compile(r"kernel_oracle::|boon_typecheck::|boon_compiler_kernel::|boon_checked::"
                                r"|KernelSemanticInput[A-Za-z0-9]*>::seal")),
    ("semantic_ms", re.compile(r"boon_semantic::")),
    ("contract_verify_ms", re.compile(r"boon_verify::")),
    ("ir_lower_ms", re.compile(r"boon_ir::erase_and_lower|boon_ir::lower|boon_ir::erase")),
    ("ir_validation_ms", re.compile(r"boon_ir::verify_")),
    ("backend_ms", re.compile(r"machine_plan_backend::|document_plan_backend::")),
    ("plan_validation_ms", re.compile(r"CompiledMachinePlanFromSource>::seal|boon_plan::seal_machine_plan|boon_plan::verify_plan|plan_sha256")),
]

SOLVER_RE = re.compile(r"boon_compiler_kernel::solver|KernelSession>::check|solve_interfaces|schedule_variable|ComponentSolver")
KERNEL_RE = re.compile(r"boon_compiler_kernel::")
FIELD_PREDICATE_RE = re.compile(
    r"ScalarFieldCatalog|materialized_output_fields|row_field_copies|state_dependent_materialized_row_fields"
    r"|ListRowFieldCatalog|value_list_authorit|authority_source_list|MaterializedRowFieldPlan|root_computation_fields"
    r"|list_activation|activation_mode|field_role|FieldRole")
INTERN_RE = re.compile(r"intern|Intern")
CONSUMER_RE = re.compile(r"receipt|Receipt|::seal|Seal|fingerprint|Fingerprint|manifest|Manifest|digest|Digest"
                         r"|verify_plan|plan_sha256|canonical|Canonical|hash|Hash|image|Image|handoff|Handoff|proof|Proof|route|Route")
HASH_SUFFIX_RE = re.compile(r"::h[0-9a-f]{16}$")


def load_nm(path):
    entries = []
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            parts = line.rstrip("\n").split(" ", 2)
            if len(parts) != 3 or parts[1] not in ("t", "T", "w", "W", "i"):
                continue
            try:
                entries.append((int(parts[0], 16), HASH_SUFFIX_RE.sub("", parts[2])))
            except ValueError:
                continue
    entries.sort()
    return [a for a, _ in entries], [n for _, n in entries]


def load_syms(path):
    if not path:
        return {}
    with open(path, encoding="utf-8") as fh:
        raw = json.load(fh)
    strings = raw["string_table"]
    out = {}
    for lib in raw["data"]:
        entries = sorted((e["rva"], strings[e["symbol"]]) for e in (lib.get("symbol_table") or []))
        out[lib["debug_name"]] = ([r for r, _ in entries], [n for _, n in entries])
    return out


def nearest_preceding(addrs, names, addr):
    i = bisect.bisect_right(addrs, addr) - 1
    return names[i] if i >= 0 else None


class ThreadResolver:
    def __init__(self, profile, thread, nm_table, syms, main_lib):
        self.t = thread
        self.libs = profile["libs"]
        self.nm_addrs, self.nm_names = nm_table
        self.syms = syms
        self.main_lib = main_lib
        self.categories = [c["name"] for c in profile["meta"]["categories"]]
        self.frame_cache = {}
        self.stack_cache = {}
        self.unresolved = Counter()

    def frame_name(self, frame):
        cached = self.frame_cache.get(frame)
        if cached is not None:
            return cached
        ft, fn, rt = self.t["frameTable"], self.t["funcTable"], self.t["resourceTable"]
        func = ft["func"][frame]
        addr = ft["address"][frame]
        cat = ft["category"][frame]
        cat_name = self.categories[cat] if cat is not None and cat < len(self.categories) else "?"
        resource = fn["resource"][func]
        if addr is None or addr < 0 or resource is None or resource < 0:
            name = "[kernel]" if cat_name == "Kernel" else "[garbage-return-address]"
            self.unresolved[name] += 1
        else:
            lib = self.libs[rt["lib"][resource]]
            lib_name = lib.get("name") or lib.get("debugName")
            if lib_name == self.main_lib:
                name = nearest_preceding(self.nm_addrs, self.nm_names, addr)
                if name is None:
                    name = f"{lib_name}!0x{addr:x}"
                    self.unresolved["below-first-symbol"] += 1
            else:
                table = self.syms.get(lib_name) or self.syms.get(lib.get("debugName", ""))
                sym = nearest_preceding(table[0], table[1], addr) if table else None
                if sym is None:
                    name = f"{lib_name}!0x{addr:x}"
                    self.unresolved[f"no-symbols:{lib_name}"] += 1
                else:
                    name = f"{lib_name}!{sym}"
        self.frame_cache[frame] = name
        return name

    def stack_names(self, stack):
        cached = self.stack_cache.get(stack)
        if cached is not None:
            return cached
        st = self.t["stackTable"]
        chain = []
        s = stack
        while s is not None:
            chain.append(self.frame_name(st["frame"][s]))
            s = st["prefix"][s]
        result = tuple(chain)
        self.stack_cache[stack] = result
        return result


def pct(n, d):
    return 0.0 if not d else 100.0 * n / d


def short(name, n=118):
    return name if len(name) <= n else name[: n - 3] + "..."


def has(names, rx):
    return any(rx.search(n) for n in names)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("profile")
    ap.add_argument("--nm", required=True)
    ap.add_argument("--syms")
    ap.add_argument("--stdout", required=True, help="captured stdout of the profiled runs (one JSON per line)")
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument("--label", default="")
    ap.add_argument("--main-lib", default="boon_cli")
    args = ap.parse_args()

    profile = json.load(gzip.open(args.profile))
    nm_table = load_nm(args.nm)
    syms = load_syms(args.syms)
    interval_ms = float(profile["meta"].get("interval", 1.0))
    epoch_ms = float(profile["meta"]["startTime"])

    runs = []
    with open(args.stdout, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line.startswith("{"):
                runs.append(json.loads(line))
    json_by_pid = {str(s["producer_pid"]): s for r in runs for s in r["samples"]}
    intent = runs[0]["intent"] if runs else "?"

    print(f"# samply summary {args.label}".rstrip())
    print(f"profile: {args.profile}")
    print(f"sampling interval: {interval_ms} ms ({1000.0 / interval_ms:.0f} Hz); nm text symbols: {len(nm_table[0])}; "
          f"sidecar libs: {sorted(syms)}; intent: {intent}; compiler-sample JSON records: {len(json_by_pid)}")

    # --- gather ---------------------------------------------------------------
    procs = {}  # pid -> dict(samples=[(t, names, w)], json=..., life=(start,end))
    unresolved = Counter()
    for th in profile["threads"]:
        if th.get("processName") != args.main_lib:
            continue
        pid = str(th["pid"]).split(".")[0]
        js = json_by_pid.get(pid)
        res = ThreadResolver(profile, th, nm_table, syms, args.main_lib)
        samples = th["samples"]
        weights = samples.get("weight") or [1] * samples["length"]
        recs = []
        for i in range(samples["length"]):
            stack = samples["stack"][i]
            if stack is None:
                continue
            recs.append((samples["time"][i], res.stack_names(stack), weights[i] or 1))
        recs.sort(key=lambda r: r[0])
        procs.setdefault(pid, {"samples": [], "json": js, "life": (th.get("processStartupTime"), th.get("processShutdownTime"))})
        procs[pid]["samples"].extend(recs)
        unresolved.update(res.unresolved)

    # --- anchor calibration -----------------------------------------------------
    # rel window (ms since epoch_ms) from JSON; monotonic = rel + X.
    estimates = []
    for pid, pr in procs.items():
        js = pr["json"]
        if not js:
            continue
        rel0 = js["observation_started_unix_us"] / 1000.0 - epoch_ms
        comp = [t for t, names, _ in pr["samples"] if has(names, COMPILE_STACK_RE)]
        if comp:
            estimates.append(min(comp) - rel0)
    if not estimates:
        print("no compile-stack samples found; cannot calibrate the time anchor", file=sys.stderr)
        return 1
    x0 = statistics.median(estimates)

    def containment(x):
        n = 0
        for pr in procs.values():
            js = pr["json"]
            if not js:
                continue
            a = js["observation_started_unix_us"] / 1000.0 - epoch_ms + x
            b = js["compiler_artifact_ready_unix_us"] / 1000.0 - epoch_ms + x
            n += sum(w for t, names, w in pr["samples"] if a <= t <= b and has(names, COMPILE_STACK_RE))
        return n

    grid = [x0 + d / 20.0 for d in range(-200, 201)]  # +-10 ms in 0.05 ms steps
    scores = [(containment(x), x) for x in grid]
    best_n = max(s for s, _ in scores)
    plateau = [x for s, x in scores if s == best_n]
    anchor = (min(plateau) + max(plateau)) / 2.0
    total_comp = sum(sum(w for _, names, w in pr["samples"] if has(names, COMPILE_STACK_RE)) for pr in procs.values())
    print(f"\n## time anchor")
    print(f"  monotonic ms of meta.startTime, calibrated: {anchor:.2f} (first-sample estimates spread "
          f"{min(estimates):.2f}..{max(estimates):.2f}); plateau width {max(plateau) - min(plateau):.2f} ms; "
          f"compile-stack samples inside their windows: {best_n} / {total_comp} ({pct(best_n, total_comp):.1f}%)")

    # --- per-process windows ------------------------------------------------------
    window = []          # (pid, t, names, w, phase_time)
    pre = post = 0
    print(f"\n## processes captured ({len(procs)} boon_cli processes; samply's own thread excluded)")
    for pid, pr in procs.items():
        js = pr["json"]
        tot = sum(w for _, _, w in pr["samples"])
        if not js:
            print(f"  pid {pid}: {tot} samples, no compiler-sample JSON (skipped)")
            continue
        a = js["observation_started_unix_us"] / 1000.0 - epoch_ms + anchor
        b = js["compiler_artifact_ready_unix_us"] / 1000.0 - epoch_ms + anchor
        bounds = []
        acc = a
        for ph in PHASE_ORDER:
            acc += js["phase"][ph]
            bounds.append((ph, acc))
        inwin = 0
        for t, names, w in pr["samples"]:
            if t < a:
                pre += w
            elif t > b:
                post += w
            else:
                inwin += w
                phase = None
                for ph, edge in bounds:
                    if t < edge:
                        phase = ph
                        break
                window.append((pid, t, names, w, phase or PHASE_ORDER[-1]))
        life = pr["life"]
        life_s = f", lifetime {life[1] - life[0]:.0f} ms" if life[0] is not None and life[1] is not None else ""
        print(f"  pid {pid}: {tot} samples{life_s}; elapsed_ms {js['elapsed_ms']:.1f}; samples in compile window {inwin} "
              f"({inwin * interval_ms:.0f} ms sampled, ratio {inwin * interval_ms / js['elapsed_ms']:.2f}); cpu_ms {js['compiler_cpu_ms']:.1f}")
    if unresolved:
        print(f"  frames without a symbol (labelled, kept): {dict(unresolved)}")

    W = sum(w for _, _, _, w, _ in window)
    all_total = sum(sum(w for _, _, w in pr["samples"]) for pr in procs.values())
    elapsed_all = [pr["json"]["elapsed_ms"] for pr in procs.values() if pr["json"]]
    print(f"\n## scopes (by time)")
    print(f"  whole-process samples: {all_total} ({all_total * interval_ms:.0f} ms)")
    print(f"  compile window (observation_started..compiler_artifact_ready): {W} samples ({W * interval_ms:.0f} ms) "
          f"vs sum elapsed_ms {sum(elapsed_all):.0f} ms (ratio {W * interval_ms / sum(elapsed_all):.2f}); "
          f"{pct(W, all_total):.1f}% of process time")
    print(f"  before window (startup, arg parsing, manifest read): {pre} ({pct(pre, all_total):.1f}%); "
          f"after window (export, binary self-hash, JSON output, exit): {post} ({pct(post, all_total):.1f}%)")
    print(f"  elapsed_ms per process: min {min(elapsed_all):.1f}, median {statistics.median(elapsed_all):.1f}, max {max(elapsed_all):.1f}")

    # stack completeness
    reach = sum(w for _, _, names, w, _ in window if has(names, COMPILE_STACK_RE))
    sha_leaf_n = sum(w for _, _, names, w, _ in window if SHA_RE.search(names[0]))
    garbage = sum(w for _, _, names, w, _ in window if len(names) <= 2 and names[-1].startswith("[garbage"))
    print(f"  stack completeness inside window: {pct(reach, W):.1f}% of samples carry a compile-phase frame; "
          f"{pct(sha_leaf_n, W):.1f}% are SHA-256 leaves (unwinder stops there); {pct(garbage, W):.1f}% are depth<=2 garbage stacks")

    # --- self time -------------------------------------------------------------------
    def self_table(recs, title, top):
        tot = sum(w for _, w in recs)
        c = Counter()
        for names, w in recs:
            c[names[0]] += w
        print(f"\n## {title}: top {top} self-time functions ({tot} samples)")
        print(f"{'rank':>4} | {'self%':>6} | {'n':>6} | function (nearest preceding symbol)")
        cum = 0
        for i, (name, n) in enumerate(c.most_common(top), 1):
            cum += n
            print(f"{i:>4} | {pct(n, tot):6.1f} | {n:>6} | {short(name)}")
        print(f"  (top {top} cover {pct(cum, tot):.1f}% of self time)")
        return c

    win_recs = [(names, w) for _, _, names, w, _ in window]
    self_table(win_recs, "compile window", args.top)
    all_recs = [(names, w) for pr in procs.values() for _, names, w in pr["samples"]]
    self_table(all_recs, "whole process", 12)

    # --- inclusive groups ------------------------------------------------------------
    def inclusive(recs, pattern):
        rx = re.compile(pattern)
        return sum(w for names, w in recs if has(names, rx))

    print(f"\n## inclusive shares by symbol group (stack contains a matching frame; LOWER BOUND where stacks are truncated)")
    print(f"{'group':<66} | {'window%':>14} | {'whole-proc%':>14}")
    for title, pattern in GROUPS:
        a = inclusive(win_recs, pattern)
        b = inclusive(all_recs, pattern)
        print(f"{title:<66} | {pct(a, W):6.1f} ({a:>5}) | {pct(b, all_total):6.1f} ({b:>5})")
    inc_n = inclusive(win_recs, INCIDENTAL_NARROW)
    inc_b = inclusive(win_recs, INCIDENTAL_BROAD)
    print(f"{'incidental union, narrow (SHA, CBOR, verify_plan, receipt/seal/fingerprint/manifest/digest)':<66} | {pct(inc_n, W):6.1f} ({inc_n:>5}) |")
    print(f"{'incidental union, broad (+validate/verify/freeze, manifest.toml, variant_set_with_order, Debug fmt)':<66} | {pct(inc_b, W):6.1f} ({inc_b:>5}) |")

    # --- SHA detail -------------------------------------------------------------------
    sha_incl = inclusive(win_recs, SHA_RE.pattern)
    cbor_incl = inclusive(win_recs, CBOR_RE.pattern)
    union = inclusive(win_recs, SHA_RE.pattern + "|" + CBOR_RE.pattern)
    print(f"\n## SHA-256 / CBOR detail (compile window)")
    print(f"  SHA-256 leaf self: {pct(sha_leaf_n, W):.1f}% ({sha_leaf_n}); SHA-256 inclusive: {pct(sha_incl, W):.1f}% ({sha_incl}); "
          f"CBOR/serde inclusive: {pct(cbor_incl, W):.1f}% ({cbor_incl}); union: {pct(union, W):.1f}% ({union})")
    by_phase = Counter()
    for _, _, names, w, ph in window:
        if SHA_RE.search(names[0]):
            by_phase[ph] += w
    print("  SHA-256 leaf samples by time-phase: " + ", ".join(f"{ph}={n} ({pct(n, sha_leaf_n):.0f}%)" for ph, n in by_phase.most_common()))
    # consumers via nearest full-stack neighbour in time within the same process
    consumers = Counter()
    by_pid_full = defaultdict(list)
    for pid, t, names, w, ph in window:
        if not SHA_RE.search(names[0]) and not names[-1].startswith("[garbage") and has(names, COMPILE_STACK_RE):
            by_pid_full[pid].append((t, names))
    for pid in by_pid_full:
        by_pid_full[pid].sort()
    for pid, t, names, w, ph in window:
        if not SHA_RE.search(names[0]):
            continue
        full = by_pid_full.get(pid)
        if not full:
            consumers["<no neighbour>"] += w
            continue
        times = [x for x, _ in full]
        i = bisect.bisect_left(times, t)
        cands = [full[j] for j in (i - 1, i) if 0 <= j < len(full)]
        nt, nn = min(cands, key=lambda c: abs(c[0] - t))
        # consumer = deepest-from-root frame matching CONSUMER_RE, else outermost boon_* frame under the wrapper
        pick = None
        for fr in nn:  # leaf -> root
            if CONSUMER_RE.search(fr) and not HASH_INFRA_RE.search(fr):
                pick = fr
                break
        if pick is None:
            pick = next((fr for fr in reversed(nn) if fr.startswith("boon_") or fr.startswith("<boon_")), nn[0])
        consumers[f"[{ph}] " + pick] += w
    print(f"  SHA-256 consumers, inferred from the nearest full-stack sample in time (indicative only; SHA stacks are truncated):")
    for name, n in consumers.most_common(20):
        print(f"    {pct(n, W):5.1f}% ({n:5d})  {short(name, 130)}")
    intern_sha = sum(w for names, w in win_recs if has(names, SHA_RE) and has(names, INTERN_RE))
    print(f"  samples with both a SHA-256 frame and an intern* frame in the same (truncated) stack: {intern_sha}")

    # --- phases: time vs stack ---------------------------------------------------------
    timers = defaultdict(float)
    for pr in procs.values():
        if pr["json"]:
            for k, v in pr["json"]["phase"].items():
                timers[k] += v
    timers_total = sum(timers.values())
    phase_time = Counter()
    phase_stack = Counter()
    agree = Counter()
    for _, _, names, w, ph in window:
        phase_time[ph] += w
        sp = None
        for pname, rx in PHASE_STACK_RE:
            if has(names, rx):
                sp = pname
                break
        if sp:
            phase_stack[sp] += w
            agree[(sp, ph)] += w
    n_both = sum(agree.values())
    n_agree = sum(n for (sp, tp), n in agree.items() if sp == tp)
    print(f"\n## phase attribution (compile window): time-based vs compiler-sample timers, and stack-based cross-check")
    print(f"  sum of phase timers {timers_total:.1f} ms vs sum elapsed_ms {sum(elapsed_all):.1f} ms")
    print(f"{'phase':<20} | {'time-phase%':>11} | {'n':>6} | {'timer%':>7} | {'stack-phase%':>12} | {'n':>6}")
    for ph in PHASE_ORDER:
        print(f"{ph:<20} | {pct(phase_time[ph], W):11.1f} | {phase_time[ph]:>6} | {pct(timers[ph], timers_total):7.1f} | "
              f"{pct(phase_stack[ph], n_both):12.1f} | {phase_stack[ph]:>6}")
    print(f"  samples with stack phase evidence: {n_both} ({pct(n_both, W):.1f}% of window); stack phase == time phase for {n_agree} ({pct(n_agree, n_both):.1f}%)")
    dis = [(k, n) for k, n in agree.items() if k[0] != k[1]]
    dis.sort(key=lambda kv: -kv[1])
    if dis:
        print("  disagreements (stack -> time): " + ", ".join(f"{sp}->{tp}={n}" for (sp, tp), n in dis[:8]))

    # --- claim-specific checks ----------------------------------------------------------
    def phase_recs(ph):
        return [(names, w) for _, _, names, w, p in window if p == ph]

    def share(recs, rx):
        tot = sum(w for _, w in recs)
        return sum(w for names, w in recs if has(names, rx)), tot

    def sub_stages(recs, rx_crate, exclude, top=12, label=""):
        tot = sum(w for _, w in recs)
        c = Counter()
        for names, w in recs:
            stage = next((n for n in reversed(names) if rx_crate.search(n) and not exclude.search(n)), None)
            if stage is None:
                stage = "<SHA-256 leaf, stack truncated>" if SHA_RE.search(names[0]) else "<no crate frame in truncated stack: leaf " + short(names[0], 60) + ">"
            c[stage] += w
        print(f"    {label} (outermost matching frame, inclusive; SHA/truncated stacks listed separately):")
        for name, n in c.most_common(top):
            print(f"      {pct(n, tot):5.1f}%  {short(name, 125)}")

    print(f"\n## claim-specific checks (compile window, time-phases)")

    pv = phase_recs("plan_validation_ms")
    pv_tot = sum(w for _, w in pv)
    pv_sha = sum(w for names, w in pv if SHA_RE.search(names[0]))
    pv_vp, _ = share(pv, re.compile(r"boon_plan::verify_plan|plan_sha256"))
    pv_enc, _ = share(pv, re.compile(r"encode|Encode|serialize|Serialize|ciborium|to_vec|write_all|Vec<u8>"))
    print(f"  plan_validation (seal -> verify_plan): {pv_tot} samples = {pct(pv_tot, W):.1f}% of window; SHA-256 leaf inside it {pct(pv_sha, pv_tot):.1f}%; "
          f"verify_plan/plan_sha256 frame present {pct(pv_vp, pv_tot):.1f}%; encode/serialize frames {pct(pv_enc, pv_tot):.1f}%")
    if pv:
        c = Counter()
        for names, w in pv:
            c[names[0]] += w
        print("    top self-time inside plan_validation:")
        for name, n in c.most_common(8):
            print(f"      {pct(n, pv_tot):5.1f}%  {short(name, 120)}")

    sem = phase_recs("semantic_ms")
    sem_tot = sum(w for _, w in sem)
    sem_sha = sum(w for names, w in sem if SHA_RE.search(names[0]))
    sem_n, _ = share(sem, re.compile(INCIDENTAL_NARROW))
    sem_b, _ = share(sem, re.compile(INCIDENTAL_BROAD))
    print(f"  semantic: {sem_tot} samples = {pct(sem_tot, W):.1f}% of window; SHA-256 leaf {pct(sem_sha, sem_tot):.1f}%; "
          f"receipt/seal/fingerprint/manifest/digest/hash frames (narrow, incl. SHA leaves) {pct(sem_n, sem_tot):.1f}%; +validate/verify (broad) {pct(sem_b, sem_tot):.1f}%")
    if sem:
        sub_stages(sem, re.compile(r"boon_semantic::"), re.compile(r"elaborate_kernel|elaborate_with_representation"), 14, "semantic sub-stages")

    tc = phase_recs("typecheck_ms")
    tc_tot = sum(w for _, w in tc)
    tc_solver, _ = share(tc, SOLVER_RE)
    tc_kernel, _ = share(tc, KERNEL_RE)
    tc_sha = sum(w for names, w in tc if SHA_RE.search(names[0]))
    tc_tcheck, _ = share(tc, re.compile(r"boon_typecheck::|boon_checked::"))
    print(f"  typecheck: {tc_tot} samples = {pct(tc_tot, W):.1f}% of window; solver frames (solver::|ComponentSolver|KernelSession::check|schedule_variable) {pct(tc_solver, tc_tot):.1f}% "
          f"-> outside solver {pct(tc_tot - tc_solver, tc_tot):.1f}%; any boon_compiler_kernel frame {pct(tc_kernel, tc_tot):.1f}% -> none {pct(tc_tot - tc_kernel, tc_tot):.1f}%; "
          f"boon_typecheck/boon_checked frames {pct(tc_tcheck, tc_tot):.1f}%; SHA-256 leaf {pct(tc_sha, tc_tot):.1f}%")
    if tc:
        sub_stages(tc, re.compile(r"boon_typecheck::|boon_compiler_kernel::|kernel_oracle::|boon_checked::"),
                   re.compile(r"compiler_checked_from_kernel|checked_construction_from_kernel$"), 16, "typecheck sub-stages")

    be = phase_recs("backend_ms")
    be_tot = sum(w for _, w in be)
    be_fp, _ = share(be, FIELD_PREDICATE_RE)
    be_sha = sum(w for names, w in be if SHA_RE.search(names[0]))
    be_intern, _ = share(be, INTERN_RE)
    be_doc, _ = share(be, re.compile(r"document_plan_backend|DocumentCompiler"))
    print(f"  backend: {be_tot} samples = {pct(be_tot, W):.1f}% of window; field-catalog/materialized-field/activation predicates {pct(be_fp, be_tot):.1f}%; "
          f"document backend frames {pct(be_doc, be_tot):.1f}%; SHA-256 leaf {pct(be_sha, be_tot):.1f}%; intern* frames {pct(be_intern, be_tot):.1f}%")
    if be:
        sub_stages(be, re.compile(r"machine_plan_backend::|document_plan_backend::"),
                   re.compile(r"compile_erased_program|compile_erased_program_with_distributed_context$"), 14, "backend sub-stages")
        c = Counter()
        for names, w in be:
            c[names[0]] += w
        print("    backend top self-time:")
        for name, n in c.most_common(10):
            print(f"      {pct(n, be_tot):5.1f}%  {short(name, 120)}")

    pa = phase_recs("parse_ms")
    pa_tot = sum(w for _, w in pa)
    pa_manifest, _ = share(pa, re.compile(r"boon_example_manifest|example_manifest|toml|readlink|realpath|canonicalize|statx"))
    pa_sha = sum(w for names, w in pa if SHA_RE.search(names[0]))
    pa_lex, _ = share(pa, re.compile(r"boon_syntax::|boon_parser::(lex|tokenize|parse_ast|ast_)"))
    print(f"  parse: {pa_tot} samples = {pct(pa_tot, W):.1f}% of window; manifest.toml lookup/validation/path canonicalization {pct(pa_manifest, pa_tot):.1f}%; "
          f"lexing/AST frames {pct(pa_lex, pa_tot):.1f}%; SHA-256 leaf {pct(pa_sha, pa_tot):.1f}%")
    if pa:
        sub_stages(pa, re.compile(r"boon_parser::|boon_syntax::|boon_example_manifest|boon_compiler::source_files_for_path|toml"),
                   re.compile(r"parse_kernel_compile_source$"), 10, "parse sub-stages")

    # whole-process harness split by time
    print(f"\n## whole-process split by time (fresh-process harness overhead)")
    print(f"  before compile window {pct(pre, all_total):.1f}%, compile window {pct(W, all_total):.1f}%, after window {pct(post, all_total):.1f}% of process samples; "
          f"whole-process SHA-256 leaf share {pct(sum(w for names, w in all_recs if SHA_RE.search(names[0])), all_total):.1f}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
