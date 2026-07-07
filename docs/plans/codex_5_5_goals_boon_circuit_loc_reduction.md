# Codex 5.5 `/goal` prompts for Boon Circuit mainline LOC reduction — baseline-relative v3

Use these prompts in the repository root on the current `main` checkout.

Recommended effort:

- Use **GPT-5.5 Codex xhigh** for cross-crate deletion/consolidation goals.
- Use **GPT-5.5 Codex high** for measurement, plan-file cleanup, and final verification summaries.
- For this repo, prefer **xhigh** whenever a goal edits `xtask`, `boon_runtime`, `boon_plan_executor`, `boon_report_schema`, native GPU crates, or workspace membership.

Core execution rules:

- Work on `main`. Do not create branches.
- Do not commit or push unless the user explicitly asks.
- Do not use Python in the repository.
- Delete legacy code; do not move it into `legacy`, `archive`, `quarantine`, or disabled stubs.
- Do not leave removed-command stubs in final code.
- Do not weaken verification, native GPU schemas, report schemas, negative checks, budgets, or runtime finality checks.
- Do not replace strict validators with permissive skeletons.
- Do not minify code or avoid formatting merely to fake a lower physical line count.
- Track both physical lines and bytes. A real cleanup should reduce both.

Native GPU aggregate verification rule:

- Never hardcode the aggregate native GPU command or aggregate report path in a goal execution.
- Read `aggregate_command` and `aggregate_report_path` from `docs/architecture/native_gpu_handoff_manifest.json` in the current checkout immediately before running the aggregate gate.
- Do not copy native GPU handoff labels, report paths, command lists, required arguments, inline JSON byte budgets, or JSON sidecar byte budgets from `AGENTS.md` or from this prompt. The manifest is the source of truth.

Numerical target policy:

- Do **not** use a public-GitHub, web-view, or old prompt baseline.
- Always measure the local checkout first.
- Let `B_RUST_LINES` be the final `total` from:

```bash
git ls-files '*.rs' | sort | xargs wc -l | tail -1
```

- Let `B_RUST_BYTES` be the final `total` from:

```bash
git ls-files '*.rs' | sort | xargs wc -c | tail -1
```

Full-plan Rust physical-line targets, computed from the local baseline:

| Target level | Removed Rust physical lines | Final Rust physical lines |
|---|---:|---:|
| Minimum useful full pass | `max(25_000, 10% of B_RUST_LINES)` | `B_RUST_LINES - removed` |
| Good full pass | `25% to 35% of B_RUST_LINES` | `65% to 75% of B_RUST_LINES` |
| Aggressive full pass | `35% to 45% of B_RUST_LINES` | `55% to 65% of B_RUST_LINES` |

For a local baseline of `428,824` Rust physical lines, those formulas mean:

| Target level | Removed Rust physical lines | Final Rust physical lines |
|---|---:|---:|
| Minimum useful full pass | at least `42,882` | at most `385,942` |
| Good full pass | `107,206` to `150,088` | `278,736` to `321,618` |
| Aggressive full pass | `150,088` to `192,970` | `235,854` to `278,736` |

Recommended target for the known `428,824`-line checkout:

- Ask for **at least 100,000 removed Rust physical lines** before calling the cleanup successful.
- Aim for **120,000 to 160,000 removed Rust physical lines** if dependency proof supports deleting unused surfaces and consolidating duplicate owners.
- Expected final Rust physical lines after a good execution: **about 268,824 to 308,824**.
- A smaller result is acceptable only if Codex gives concrete proof that protected runtime/native/report gates prevent further safe deletion.

---

## /goal 0 — high — verify active plan files and remove weaker copies if they return

```text
/goal You are working in the Boon Circuit repository on main. Do not create branches. Do not commit or push. Do not use Python. Do not move removed code into legacy/archive/quarantine directories.

Task: make the LOC-reduction plan files unambiguous before any code cleanup.

Steps:
1. Read AGENTS.md, README.md, docs/architecture/NATIVE_GPU_PIPELINE.md, docs/architecture/native_gpu_handoff_manifest.json, Cargo.toml, and docs/plans/boon_circuit_loc_reduction_mainline_v3.md if it exists.
2. Inspect docs/plans for these files:
   - boon_circuit_loc_reduction_playbook.md
   - boon_circuit_loc_reduction_playbook_strict_v2.md
   - boon_circuit_loc_reduction_mainline_v3.md
   - codex_5_5_goals_boon_circuit_loc_reduction.md
3. Keep docs/plans/boon_circuit_loc_reduction_mainline_v3.md as the active LOC-reduction playbook.
4. Keep exactly one Codex goal prompt file, preferably docs/plans/codex_5_5_goals_boon_circuit_loc_reduction.md with baseline-relative targets.
5. Remove v1/v2 playbook files if they are present in docs/plans and are stale cleanup noise.
6. Do not touch native GPU handoff labels or report paths except by reading docs/architecture/native_gpu_handoff_manifest.json.
7. Run:
   - git status --short
   - git diff --stat
   - git diff --check

Deliverable: a short summary of which plan files remain, whether anything was removed, and the current git status.
```

---

## /goal 1 — high — exact local LOC baseline, byte baseline, and candidate map

```text
/goal You are working in the Boon Circuit repository on main. Do not create branches. Do not commit or push. Do not use Python. Do not edit source in this goal except for harmless target/ scratch files.

Task: measure the exact local baseline and prepare a safe deletion/consolidation candidate map.

Steps:
1. Read AGENTS.md, README.md, docs/architecture/NATIVE_GPU_PIPELINE.md, docs/architecture/native_gpu_handoff_manifest.json, Cargo.toml, docs/plans/boon_circuit_loc_reduction_mainline_v3.md, and docs/plans/codex_5_5_goals_boon_circuit_loc_reduction.md if it exists.
2. Create target/loc-reduction/.
3. Measure physical Rust lines:
   git ls-files '*.rs' | sort | xargs wc -l | sort -nr > target/loc-reduction/rust-physical-lines-before.txt
4. Measure Rust bytes:
   git ls-files '*.rs' | sort | xargs wc -c | sort -nr > target/loc-reduction/rust-bytes-before.txt
5. Measure all tracked source-ish files, excluding target and Cargo.lock:
   git ls-files | grep -E '\.(rs|toml|md|json|bn|scn|wgsl)$' | grep -v '^Cargo.lock$' | sort | xargs wc -l | sort -nr > target/loc-reduction/source-physical-lines-before.txt
   git ls-files | grep -E '\.(rs|toml|md|json|bn|scn|wgsl)$' | grep -v '^Cargo.lock$' | sort | xargs wc -c | sort -nr > target/loc-reduction/source-bytes-before.txt
6. Compute local targets from the measured Rust line baseline and write them to target/loc-reduction/local-targets.md:
   B=$(tail -1 target/loc-reduction/rust-physical-lines-before.txt | awk '{print $1}')
   MIN=$(( B / 10 ))
   if [ "$MIN" -lt 25000 ]; then MIN=25000; fi
   GOOD_LOW=$(( B * 25 / 100 ))
   GOOD_HIGH=$(( B * 35 / 100 ))
   AGGR_LOW=$(( B * 35 / 100 ))
   AGGR_HIGH=$(( B * 45 / 100 ))
   cat > target/loc-reduction/local-targets.md <<EOF_TARGETS
   # Local LOC reduction targets

   Baseline Rust physical lines: $B

   | Target level | Removed Rust physical lines | Final Rust physical lines |
   |---|---:|---:|
   | Minimum useful full pass | $MIN | $(( B - MIN )) |
   | Good full pass low | $GOOD_LOW | $(( B - GOOD_LOW )) |
   | Good full pass high | $GOOD_HIGH | $(( B - GOOD_HIGH )) |
   | Aggressive full pass low | $AGGR_LOW | $(( B - AGGR_LOW )) |
   | Aggressive full pass high | $AGGR_HIGH | $(( B - AGGR_HIGH )) |
   EOF_TARGETS
7. Build a candidate map in target/loc-reduction/candidate-map.md with these categories:
   - KEEP_CORE: parser, typecheck, IR, plan, PlanExecutor/runtime, compiler, report schema, CLI, TodoMVC, Cells, todo_mvc_physical support, native GPU two-window support.
   - DELETE_ONLY_IF_PROVEN_UNUSED: boon_scene_model, boon_solid_model, boon_manufacturing, boon_3mf, boon_mesh_export, stale docs/plans, obsolete Ply/browser/Xvfb/COSMIC proof references, obsolete native playground paths not used by the manifest-backed native flow.
   - CONSOLIDATE_ONLY_IF_GATES_STAY_STRICT: boon_plan_executor vs boon_runtime, boon_report_schema helpers, duplicate report writers, duplicated native setup/report paths, repeated xtask verification runners.
8. For every DELETE_ONLY_IF_PROVEN_UNUSED crate, run:
   cargo tree --workspace -i <crate-name> > target/loc-reduction/<crate-name>-reverse-tree.txt 2>&1 || true
   git grep -n '<crate-name without crates/ prefix>' -- . ':!target' > target/loc-reduction/<crate-name>-references.txt || true
9. Run focused no-edit checks:
   cargo metadata --no-deps > target/loc-reduction/metadata-before.json
   cargo check --workspace

Deliverable: summarize the exact Rust line baseline, exact Rust byte baseline, exact source-ish line baseline, top 20 Rust line/byte files, the computed local targets from target/loc-reduction/local-targets.md, and the safest first deletion/consolidation candidates. Do not perform deletions in this goal.
```

---

## /goal 2 — xhigh — delete proven unused non-first-pass surfaces

```text
/goal You are working in the Boon Circuit repository on main. Do not create branches. Do not commit or push. Do not use Python. Delete code; do not quarantine or archive it. Do not leave removed-command stubs. Do not weaken verification.

Task: remove only non-first-pass surfaces that are proven unused by current targets.

Context and constraints:
- The active direction is PlanExecutor-backed runtime plus native GPU two-window playground.
- TodoMVC, Cells, and todo_mvc_physical support are protected.
- Native GPU handoff truth is docs/architecture/native_gpu_handoff_manifest.json.
- Do not delete boon_scene_model, boon_solid_model, boon_manufacturing, boon_3mf, or boon_mesh_export unless cargo tree and git grep prove the deletion is safe or the same responsibility is moved into a kept crate in the same patch.

Steps:
1. Read target/loc-reduction/candidate-map.md and target/loc-reduction/local-targets.md if they exist. If missing, run the measurement/candidate commands from goal 1 first.
2. For each candidate crate or feature, prove whether it is unused:
   - cargo tree --workspace -i <crate>
   - git grep -n '<crate-or-feature-token>' -- . ':!target'
3. Delete only candidates whose references are removable without touching protected TodoMVC/Cells/todo_mvc_physical/native GPU paths.
4. For each deletion, remove all of the following together:
   - workspace member entry,
   - workspace dependency entry,
   - crate directory,
   - direct dependency entries in other Cargo.toml files,
   - command registration/help text that only targets the deleted feature,
   - stale docs that point to the deleted feature.
5. If a file requires line-range deletion, collect ranges first and apply them bottom-up. Do not run formatting between collecting and applying ranges.
6. Do not touch report-schema strictness or native GPU budgets.
7. Run focused checks after the deletion chunk:
   - cargo metadata --no-deps
   - cargo check --workspace
   - cargo test --workspace if cargo check passes quickly enough; otherwise run focused tests for touched crates and explain why full tests were deferred.
8. Measure after:
   git ls-files '*.rs' | sort | xargs wc -l | sort -nr > target/loc-reduction/rust-physical-lines-after-unused-delete.txt
   git ls-files '*.rs' | sort | xargs wc -c | sort -nr > target/loc-reduction/rust-bytes-after-unused-delete.txt

Target: remove as much proven-unused Rust as possible. For a full LOC pass, goal 2 should normally contribute a meaningful part of the minimum local target from target/loc-reduction/local-targets.md. If it removes little or nothing, the deliverable must explain which protected dependencies kept each candidate alive.

Deliverable: exact files/crates deleted, proof that protected targets still build/check, Rust physical lines before/after, Rust bytes before/after, and any candidates intentionally kept because they were still referenced.
```

---

## /goal 3 — xhigh — consolidate runtime / PlanExecutor / report-schema / xtask duplication without weakening gates

```text
/goal You are working in the Boon Circuit repository on main. Do not create branches. Do not commit or push. Do not use Python. Do not weaken verification. Do not replace strict validators with permissive skeletons.

Task: reduce handwritten Rust LOC by consolidating duplicated runtime/PlanExecutor/report-schema/xtask ownership while keeping behavior and report strictness intact.

Protected rules:
- The runtime must remain PlanExecutor-backed.
- TodoMVC and Cells must still dump IR and run scenarios.
- Report-schema validation must remain strict; helper extraction/macros are allowed, permissive schemas are not.
- Native GPU handoff labels/paths/budgets must be read from docs/architecture/native_gpu_handoff_manifest.json, not duplicated in a new checklist.

Steps:
1. Read AGENTS.md, README.md, native GPU manifest, docs/plans/boon_circuit_loc_reduction_mainline_v3.md, and target/loc-reduction/local-targets.md if it exists.
2. Build target/loc-reduction/runtime-owner-map.md with current and target owners for:
   - source dispatch,
   - dirty keysets,
   - list memory updates,
   - delta emission,
   - report evidence,
   - scenario replay,
   - xtask report runners,
   - native GPU report aggregation.
3. Inspect crates/boon_runtime, crates/boon_plan_executor, crates/boon_report_schema, crates/xtask, crates/boon_native_playground, crates/boon_native_app_window, and crates/boon_native_gpu for repeated structs, repeated report writers, repeated validation branches, repeated example-specific TodoMVC/Cells shells, duplicated constants, and duplicated command runners.
4. Prefer helper extraction, tables, macros, and one owner per responsibility.
5. Remove duplicate paths only after the kept path handles TodoMVC, Cells, and todo_mvc_physical generically where applicable.
6. Do not delete required evidence fields. Do not remove negative checks. Do not change report byte budgets unless the manifest/schema already owns that change.
7. Run focused checks after each chunk:
   - cargo check -p boon_runtime -p boon_plan_executor -p boon_report_schema -p boon_cli -p xtask
   - cargo run -p boon_cli -- dump-ir examples/todomvc.bn > target/loc-reduction/runtime-todomvc.ir.txt
   - cargo run -p boon_cli -- dump-ir examples/cells.bn > target/loc-reduction/runtime-cells.ir.txt
   - cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn > target/loc-reduction/runtime-todomvc.run.txt
   - cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn > target/loc-reduction/runtime-cells.run.txt
   - cargo xtask verify-report-schema
8. Measure after:
   git ls-files '*.rs' | sort | xargs wc -l | sort -nr > target/loc-reduction/rust-physical-lines-after-runtime-report.txt
   git ls-files '*.rs' | sort | xargs wc -c | sort -nr > target/loc-reduction/rust-bytes-after-runtime-report.txt

Target: after goal 2 and goal 3 together, the cleanup should be on track for the local minimum useful full-pass target in target/loc-reduction/local-targets.md. For a 428,824-line baseline, that means at least 42,882 removed Rust physical lines as the minimum and preferably 107,206+ removed by the end of the full plan. If the local baseline differs, use the computed local targets instead.

Deliverable: exact consolidation performed, behavior/gate commands run, line/byte before-after, and any duplication intentionally left because removing it would weaken proof.
```

---

## /goal 4 — high or xhigh — final cleanup, stale docs, and manifest-backed gates

```text
/goal You are working in the Boon Circuit repository on main. Do not create branches. Do not commit or push. Do not use Python. Do not weaken verification. Do not fabricate human reports.

Task: finish the LOC-reduction cleanup and produce an honest final line-count report.

Steps:
1. Search stale references:
   git grep -n -E 'boon_3mf|boon_mesh_export|boon_manufacturing|legacy|quarantine|old Ply|browser proof|Xvfb screenshot|whole-desktop|COSMIC scraping|one phase per branch|feature branch|Python tooling|\.py|second native.*handoff|native GPU checklist' -- . ':!target' || true
2. Delete stale docs only when they point to removed code or obsolete proof paths. Do not delete active native GPU docs.
3. Ensure only v3 LOC plan remains in docs/plans if any LOC plan is committed.
4. Run formatting/checking appropriate to the repo. If cargo fmt would explode intentionally compressed physical line counts, report that and ask whether formatting is desired; do not use minification as a LOC-reduction tactic.
5. Run final focused gates:
   - cargo check --workspace
   - cargo run -p boon_cli -- dump-ir examples/todomvc.bn > target/loc-reduction/final-todomvc.ir.txt
   - cargo run -p boon_cli -- dump-ir examples/cells.bn > target/loc-reduction/final-cells.ir.txt
   - cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn > target/loc-reduction/final-todomvc.run.txt
   - cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn > target/loc-reduction/final-cells.run.txt
   - cargo xtask verify-report-schema
   - cargo xtask verify-runtime-finality
   - Read aggregate_command and aggregate_report_path from docs/architecture/native_gpu_handoff_manifest.json in the current checkout, then run exactly:
     cargo xtask <aggregate_command_from_manifest> --check-existing --report <aggregate_report_path_from_manifest>
     Do not copy native GPU aggregate command names, report paths, labels, command lists, argument lists, or byte budgets from AGENTS.md or from this prompt. The manifest is the source of truth.
   - cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
6. If some final gates fail for pre-existing environment/report freshness reasons, do not hide the failure. Include exact command, exit status, and the relevant failure lines.
7. Measure final lines and bytes:
   git ls-files '*.rs' | sort | xargs wc -l | sort -nr > target/loc-reduction/rust-physical-lines-final.txt
   git ls-files '*.rs' | sort | xargs wc -c | sort -nr > target/loc-reduction/rust-bytes-final.txt
   git ls-files | grep -E '\.(rs|toml|md|json|bn|scn|wgsl)$' | grep -v '^Cargo.lock$' | sort | xargs wc -l | sort -nr > target/loc-reduction/source-physical-lines-final.txt
   git ls-files | grep -E '\.(rs|toml|md|json|bn|scn|wgsl)$' | grep -v '^Cargo.lock$' | sort | xargs wc -c | sort -nr > target/loc-reduction/source-bytes-final.txt
8. Compare the final result against target/loc-reduction/local-targets.md. Do not claim success if the final result misses the local minimum target unless the summary gives exact dependency or verification reasons.

Deliverable: final summary with:
- Rust physical lines before and after,
- Rust bytes before and after,
- source-ish physical lines before and after,
- source-ish bytes before and after,
- net removed lines,
- net removed bytes,
- whether the result met the local minimum/good/aggressive target,
- commands passed,
- commands failed or deferred with exact reasons,
- git status --short,
- git diff --stat.
```
