# Boon Circuit LOC Reduction: Mainline v3 Execution Plan

_Last checked against `BoonLang/boon-circuit` `main` on 2026-07-07._

This is the execution version of the LOC-reduction plan. It is written for an AI coding agent that should follow simple mechanical steps without inventing a new cleanup strategy.

Repository: <https://github.com/BoonLang/boon-circuit>

## 0. What this plan is, and what it is not

This plan reduces handwritten Rust and duplicated verification/report/runtime code.

It is **not** the Cells 60 FPS performance plan. It must not replace native performance work, render-graph work, currentness work, retained layout work, or WGPU readback/verification architecture work.

Use this cleanup plan only when it helps the active direction:

- PlanExecutor-backed runtime.
- Native GPU two-window playground.
- Generic TodoMVC, Cells, and `todo_mvc_physical` verification without example-specific hacks.
- Keeping runtime, document, render, report, and native GPU gates honest and fast.

If the active work is a Cells/native performance blocker, do not disappear into broad LOC cleanup. Only do cleanup that directly removes duplicated slow paths, stale currentness paths, duplicated render paths, or stale verification paths that are blocking the performance/currentness plan.

## 1. Absolute source-of-truth order

When instructions conflict, obey this order:

1. The user's latest instruction.
2. `AGENTS.md`.
3. `README.md`.
4. `docs/architecture/NATIVE_GPU_PIPELINE.md`.
5. `docs/architecture/native_gpu_handoff_manifest.json`.
6. Current performance/render/currentness plan files in `docs/plans/`, if present.
7. This file.

Before editing, read these files from the current checkout:

```bash
sed -n '1,260p' AGENTS.md
sed -n '1,320p' README.md
sed -n '1,320p' docs/architecture/NATIVE_GPU_PIPELINE.md
cat docs/architecture/native_gpu_handoff_manifest.json
sed -n '1,260p' Cargo.toml
find docs/plans -maxdepth 1 -type f | sort
```

Then search for any active performance/currentness plan:

```bash
git grep -n -E '60 FPS|60fps|currentness|render graph|retained|readback|Cells.*speed|scroll-speed|native.*performance' docs AGENTS.md README.md || true
```

If a current performance/currentness plan exists, write down which file is active in `target/loc-reduction/active-performance-plan.txt`. Do not create a second competing plan.

## 2. Main-branch-only workflow

The user wants all work on `main`.

Rules:

- Do **not** create feature branches.
- Do **not** tell another agent to create feature branches.
- Work in the current `main` checkout.
- Do not commit or push unless the user explicitly asks.
- If the user later asks for commits, commit on `main`, not on a side branch.
- Use `target/loc-reduction/` for temporary notes, measurements, and scratch reports.
- Do not commit `target/` files.
- Before a risky chunk, save a patch snapshot under `target/loc-reduction/`.

Snapshot commands:

```bash
mkdir -p target/loc-reduction
git status --short > target/loc-reduction/status-before-chunk.txt
git diff > target/loc-reduction/diff-before-chunk.patch
```

After a chunk:

```bash
git status --short
git diff --stat
git diff --check
```

If a chunk goes wrong, prefer a targeted reverse patch. Do not run broad destructive commands such as `git reset --hard` unless the user explicitly asks.

## 3. Hard rules

### 3.1 No Python in the repo

Do not add Python files, Python scripts, Python snippets, or Python-based repo tooling.

Allowed:

- Shell one-liners used locally.
- Temporary Rust helpers under `/tmp`.
- Committed Rust tooling inside `crates/xtask`.

Not allowed:

- `.py` files.
- Python in `docs/plans` as suggested tooling.
- Python-generated committed source unless the generator is not part of the repo and the generated source is already required. Prefer not to do this.

### 3.2 Delete legacy code; do not park it

Do not create these directories or equivalents:

```text
legacy/
old/
archive/
unused/
quarantine/
parking/
attic/
```

Do not move removed code into comments.

Do not leave final placeholder command stubs such as:

```rust
fn old_command_removed() -> Result<()> {
    anyhow::bail!("removed")
}
```

When deleting a feature or command, delete:

- command registration,
- help text,
- parser/match arms,
- implementation,
- tests that only check the deleted behavior,
- report schema entries only if the whole report is deleted,
- stale documentation that points to the deleted command.

### 3.3 Do not weaken verification

Never make a failing proof pass by weakening the checker.

Forbidden shortcuts:

- Turning errors into warnings.
- Removing negative checks while keeping the feature.
- Replacing strict report-schema validation with a permissive skeleton.
- Hand-writing passing JSON.
- Lowering byte budgets just by deleting required report fields.
- Hiding `remaining_example_specific_shells` without removing real example-specific shells.
- Using old Ply, browser, Xvfb screenshots, whole-desktop screenshots, COSMIC scraping, or human observation as native GPU proof.

### 3.4 Native GPU manifest is the source of truth

Do not maintain another native handoff checklist.

The native handoff truth is:

```text
docs/architecture/native_gpu_handoff_manifest.json
```

Use the manifest-backed aggregate. Do not duplicate its report labels, report paths, commands, required arguments, inline JSON budgets, or sidecar budgets in this plan or in new code.

### 3.5 Repeated tooling belongs in `xtask`

Temporary Rust helpers under `/tmp` are fine for one-off mechanical edits.

If a helper is needed repeatedly across the repo, either:

- make it a committed `xtask` subcommand, or
- stop and use existing `cargo`, `git grep`, `cargo tree`, or shell commands.

Do not leave copy-pasted helper programs scattered in docs or repo files.

### 3.6 Keep line numbers stable during deletion

When deleting by line number:

- Collect all target ranges first.
- Sort ranges from highest starting line to lowest starting line.
- Delete bottom-up.
- Do not run `cargo fmt`, rustfmt, IDE formatting, or broad find/replace between collecting ranges and applying them.
- After deleting a range, only line numbers **above** that range remain stable.
- If you must delete from the top, recollect line numbers before continuing.

## 4. Protected surfaces

Do not delete these examples:

```text
examples/todomvc.bn
examples/cells.bn
examples/todomvc.scn
examples/cells.scn
```

Do not delete support for these examples or report paths:

```text
todomvc
cells
todo_mvc_physical
```

Do not delete these crates unless the same change moves the same responsibility into another kept crate and all affected gates pass:

```text
crates/boon_parser
crates/boon_typecheck
crates/boon_ir
crates/boon_plan
crates/boon_plan_executor
crates/boon_compiler
crates/boon_runtime
crates/boon_report_schema
crates/boon_cli
crates/boon_document_model
crates/boon_editor
crates/boon_driver
crates/boon_host
crates/boon_document
crates/boon_native_gpu
crates/boon_native_app_window
crates/boon_native_playground
crates/xtask
```

Special rule:

- `boon_plan_executor` and `boon_runtime` may be consolidated, but the final state must have one real execution model. It must not be an adapter-backed fake pass.

Potential deletion candidates only after proof:

```text
crates/boon_3mf
crates/boon_mesh_export
crates/boon_manufacturing
crates/boon_solid_model
crates/boon_scene_model
```

Do not delete a candidate if it is used by TodoMVC, Cells, `todo_mvc_physical`, native GPU handoff, native/document paths, report-schema checks, runtime finality, goal readiness, or the active performance/currentness plan.

## 5. Baseline measurement

Do not add a committed LOC script for the first pass. Use temporary reports under `target/loc-reduction/`.

```bash
mkdir -p target/loc-reduction

git status --short > target/loc-reduction/status-before.txt
cargo metadata --no-deps > target/loc-reduction/metadata-before.json

git ls-files '*.rs' \
  | while IFS= read -r f; do printf '%10s  %s\n' "$(wc -l < "$f")" "$f"; done \
  | sort -nr \
  > target/loc-reduction/rust-physical-lines-before.txt

git ls-files \
  | grep -E '\.(rs|toml|bn|scn|json|wgsl|md)$' \
  | while IFS= read -r f; do printf '%10s  %s\n' "$(wc -c < "$f")" "$f"; done \
  | sort -nr \
  > target/loc-reduction/source-bytes-before.txt

sed -n '1,80p' target/loc-reduction/rust-physical-lines-before.txt
sed -n '1,80p' target/loc-reduction/source-bytes-before.txt
```

After each chunk:

```bash
git ls-files '*.rs' \
  | while IFS= read -r f; do printf '%10s  %s\n' "$(wc -l < "$f")" "$f"; done \
  | sort -nr \
  > target/loc-reduction/rust-physical-lines-after.txt

git ls-files \
  | grep -E '\.(rs|toml|bn|scn|json|wgsl|md)$' \
  | while IFS= read -r f; do printf '%10s  %s\n' "$(wc -c < "$f")" "$f"; done \
  | sort -nr \
  > target/loc-reduction/source-bytes-after.txt

diff -u target/loc-reduction/rust-physical-lines-before.txt target/loc-reduction/rust-physical-lines-after.txt || true
diff -u target/loc-reduction/source-bytes-before.txt target/loc-reduction/source-bytes-after.txt || true
```

Do not commit `target/loc-reduction/`.

## 6. Focused verification policy

Do not run the full world after every tiny edit. Use focused checks during local chunks, then full aggregate gates before claiming completion.

### 6.1 After every edit batch

Run:

```bash
cargo metadata --no-deps
cargo check --workspace
git diff --check
```

If `cargo check --workspace` is too broad for the current machine while still iterating, run focused package checks for every touched crate, then run workspace check before leaving the chunk:

```bash
cargo check -p <touched_crate>
cargo test -p <touched_crate>
cargo check --workspace
```

### 6.2 Parser / typecheck / IR / compiler / runtime touched

Run:

```bash
cargo run -p boon_cli -- dump-ir examples/todomvc.bn > target/loc-reduction/todomvc.ir.txt
cargo run -p boon_cli -- dump-ir examples/cells.bn > target/loc-reduction/cells.ir.txt
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn > target/loc-reduction/todomvc.run.txt
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn > target/loc-reduction/cells.run.txt
cargo xtask verify-runtime-finality
```

If performance/currentness code was touched, also run the relevant speed check from the current README or active performance plan. Common examples are:

```bash
cargo xtask bench-example cells
cargo xtask bench-todomvc
```

Use the exact active command if a current plan names a stricter one.

### 6.3 Report schema touched

Run:

```bash
cargo test -p boon_report_schema
cargo xtask verify-report-schema
cargo xtask verify-runtime-finality
```

If native report schema is touched, also run the manifest-backed native aggregate before claiming completion.

### 6.4 Native GPU / native app window / native playground touched

Read the manifest from the current checkout, then run the manifest-backed aggregate command and aggregate report path.

Use this shell extraction only to avoid duplicating the manifest in this plan:

```bash
native_aggregate="$({ sed -n 's/.*"aggregate_command"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' docs/architecture/native_gpu_handoff_manifest.json || true; } | head -1)"
native_report="$({ sed -n 's/.*"aggregate_report_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' docs/architecture/native_gpu_handoff_manifest.json || true; } | head -1)"

test -n "$native_aggregate"
test -n "$native_report"

cargo xtask "$native_aggregate" --check-existing --report "$native_report"
```

Before claiming native handoff readiness, run the individual reports listed by `docs/architecture/native_gpu_handoff_manifest.json`, then the aggregate above.

If a hardware/display requirement prevents a native command from running, record the exact command and reason. Do not substitute old proof paths.

### 6.5 Final completion gates

Before saying the cleanup is complete, run the final gates from the current README and manifest.

Minimum final set:

```bash
cargo test --workspace
cargo run -p boon_cli -- dump-ir examples/todomvc.bn > target/loc-reduction/final-todomvc.ir.txt
cargo run -p boon_cli -- dump-ir examples/cells.bn > target/loc-reduction/final-cells.ir.txt
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn > target/loc-reduction/final-todomvc.run.txt
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn > target/loc-reduction/final-cells.run.txt
cargo xtask verify-report-schema
cargo xtask verify-runtime-finality
cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
```

Then run the native manifest aggregate as described above.

If the current README or active performance plan names stricter final commands, run those too.

## 7. Stable deletion mechanics

### 7.1 Manual bottom-up deletion

For a file `crates/example/src/lib.rs`:

```bash
nl -ba crates/example/src/lib.rs | sed -n '1200,1500p'
```

Write target ranges in descending order:

```text
1420-1488  delete obsolete helper C
1330-1399  delete obsolete helper B
1210-1290  delete obsolete helper A
```

Delete `1420-1488` first, then `1330-1399`, then `1210-1290`.

Do not format until all ranges from that file are done.

After deleting all ranges from the file:

```bash
cargo fmt --check || true
cargo fmt
cargo check -p <crate_name>
```

### 7.2 Temporary Rust reverse-delete helper

Use this helper only for files with stable physical lines. It accepts one file and 1-based inclusive ranges. It sorts ranges bottom-up internally.

Create it under `/tmp`, not in the repo:

```bash
cat > /tmp/boon_reverse_delete.rs <<'RS'
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
struct Span {
    start: usize,
    end: usize,
}

fn parse_span(text: &str) -> Result<Span, String> {
    let Some((a, b)) = text.split_once('-') else {
        return Err(format!("range must be START-END, got {text:?}"));
    };
    let start: usize = a.parse().map_err(|_| format!("bad start in {text:?}"))?;
    let end: usize = b.parse().map_err(|_| format!("bad end in {text:?}"))?;
    if start == 0 || end == 0 || start > end {
        return Err(format!("invalid 1-based inclusive range {text:?}"));
    }
    Ok(Span { start, end })
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 {
        return Err("usage: boon_reverse_delete FILE START-END [START-END ...]".to_string());
    }

    let path = PathBuf::from(args.remove(0));
    let original = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut lines = original.lines().map(str::to_owned).collect::<Vec<_>>();
    let ended_with_newline = original.ends_with('\n');

    let mut spans = args.iter().map(|s| parse_span(s)).collect::<Result<Vec<_>, _>>()?;
    spans.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));

    let mut previous_start = usize::MAX;
    for span in spans {
        if span.end > lines.len() {
            return Err(format!(
                "range {}-{} exceeds file line count {}",
                span.start,
                span.end,
                lines.len()
            ));
        }
        if span.end >= previous_start {
            return Err(format!(
                "overlapping or unsorted-after-normalization range {}-{}",
                span.start,
                span.end
            ));
        }
        let start_index = span.start - 1;
        let end_exclusive = span.end;
        lines.drain(start_index..end_exclusive);
        previous_start = span.start;
    }

    let mut output = lines.join("\n");
    if ended_with_newline {
        output.push('\n');
    }
    fs::write(&path, output).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}
RS

rustc /tmp/boon_reverse_delete.rs -o /tmp/boon_reverse_delete
```

Usage:

```bash
/tmp/boon_reverse_delete crates/example/src/lib.rs 1420-1488 1330-1399 1210-1290
```

Then inspect:

```bash
git diff -- crates/example/src/lib.rs
cargo fmt
cargo check -p <crate_name>
```

### 7.3 One-line or giant-line files

Some generated-looking or compressed files may have very few physical lines but huge byte size. Do not line-delete those by range.

Use one of these instead:

- Replace the whole file with a smaller real implementation.
- Delete exact command blocks by unique text.
- Split only if splitting helps deletion or ownership. Splitting alone does not reduce LOC.

## 8. Mainline chunk order

Work in this order. Do not jump to later broad deletions while earlier proof is missing.

### Chunk 0: Align with active performance/currentness work

Goal: make sure LOC cleanup supports the current direction instead of replacing it.

Commands:

```bash
mkdir -p target/loc-reduction
git grep -n -E '60 FPS|60fps|currentness|render graph|retained|readback|Cells.*speed|scroll-speed|native.*performance' docs AGENTS.md README.md || true
```

Write a tiny local note:

```bash
cat > target/loc-reduction/current-priority.txt <<'TXT'
Active user priority:
- Main branch only.
- LOC cleanup is allowed only if it does not replace Cells/native performance/currentness work.
- Native GPU handoff truth remains docs/architecture/native_gpu_handoff_manifest.json.
TXT
```

Pass condition:

- You know whether there is an active performance/currentness plan.
- You do not create a second plan that competes with it.

### Chunk 1: Baseline and top offenders

Goal: know exactly where the handwritten lines are before deleting.

Run the baseline commands from section 5.

Then inspect the largest files:

```bash
sed -n '1,120p' target/loc-reduction/rust-physical-lines-before.txt
sed -n '1,120p' target/loc-reduction/source-bytes-before.txt
```

For each top file, classify it:

```text
KEEP_CORE       required for current target
COMPRESS        required but repetitive
DELETE_PROOF    maybe obsolete, needs dependency proof
DO_NOT_TOUCH     performance/currentness active path; avoid cleanup unless directly helping
```

Write classifications to:

```text
target/loc-reduction/top-file-classification.txt
```

Do not commit this file.

### Chunk 2: Remove competing old plan files

Goal: avoid multiple conflicting playbooks in `docs/plans`.

If v1/v2 cleanup plans were copied into the repo as untracked brainstorming files, keep only the newest mainline plan the user wants.

Example:

```bash
git status --short docs/plans
```

If these are untracked and not intentionally kept:

```text
docs/plans/boon_circuit_loc_reduction_playbook.md
docs/plans/boon_circuit_loc_reduction_playbook_strict_v2.md
```

then delete them after copying this v3 file to its final path.

Do not delete unrelated plans.

Pass condition:

- There is at most one active LOC-reduction playbook in `docs/plans`.
- It says main branch only.
- It does not contain Python tooling.
- It does not say quarantine.
- It does not contain a second native GPU handoff checklist.

Search:

```bash
git grep -n -E 'quarantine|legacy/|archive/|feature branch|one phase per branch|python|\.py|native GPU checklist|handoff checklist' docs/plans/boon_circuit_loc_reduction_mainline_v3.md || true
```

Allowed hits are only rule text forbidding those things.

### Chunk 3: Compress `xtask` by turning repeated commands into data

Goal: reduce command duplication without weakening gates.

Rules:

- Do not delete current final gates.
- Do not remove native handoff entries manually; the manifest owns them.
- Do not leave removed-command stubs.
- Repeated verification runners should be Rust data tables plus one generic runner.

Find duplication:

```bash
git grep -n -E 'verify-.*todomvc|verify-.*cells|verify-native|report_schema|runtime_finality|goal-readiness' crates/xtask/src crates/xtask || true
```

Refactor shape:

```rust
struct ExampleSpec {
    name: &'static str,
    source: &'static str,
    scenario: Option<&'static str>,
}

const EXAMPLES: &[ExampleSpec] = &[
    ExampleSpec { name: "todomvc", source: "examples/todomvc.bn", scenario: Some("examples/todomvc.scn") },
    ExampleSpec { name: "cells", source: "examples/cells.bn", scenario: Some("examples/cells.scn") },
];
```

Generic runner pattern:

```rust
fn run_example_scenario(spec: &ExampleSpec) -> anyhow::Result<()> {
    // one real implementation that preserves the previous report fields and errors
    Ok(())
}
```

Delete duplicated wrappers bottom-up after the generic runner passes tests.

Focused checks:

```bash
cargo check -p xtask
cargo test -p xtask
cargo xtask verify-report-schema
```

If runtime/report/native commands were touched, run their checks from section 6.

Pass condition:

- Repeated TodoMVC/Cells command bodies are data-driven.
- Deleted commands have no registration, help text, or dead stubs left.
- Existing current commands still exist.
- Report fields and failures did not become looser.

### Chunk 4: Compress `boon_report_schema` without weakening it

Goal: keep strict report validation while removing repeated handwritten validators.

Do **not** replace validators with a permissive skeleton.

Allowed reductions:

- Extract repeated field-presence checks into helpers.
- Extract repeated typed number/string/bool/object/array checks into helpers.
- Extract repeated artifact hash checks into helpers.
- Extract repeated path/report metadata checks into helpers.
- Use macros for repeated strict validators when the macro expands to the same checks.
- Use manifest-backed native report validation where appropriate instead of duplicated native lists.

Forbidden reductions:

- Accepting unknown missing required fields.
- Replacing a strict validator with `serde_json::Value` passthrough.
- Turning a required field into optional just to simplify code.
- Deleting negative tests while the report remains supported.
- Lowering native GPU budgets or schema requirements to pass a size check.

Mechanical method:

1. Pick one repeated pattern.
2. Add one helper with the same strict error behavior.
3. Convert the smallest group of call sites.
4. Run checks.
5. Delete old repeated code bottom-up.
6. Move to the next repeated pattern.

Useful searches:

```bash
git grep -n -E 'required|missing|expected|serde_json|as_object|as_array|as_str|as_bool|as_u64|report' crates/boon_report_schema/src || true
```

Focused checks:

```bash
cargo check -p boon_report_schema
cargo test -p boon_report_schema
cargo xtask verify-report-schema
cargo xtask verify-runtime-finality
```

If native report validation changed, run the native manifest aggregate from section 6.4.

Pass condition:

- Required fields are still required.
- Negative checks still fail when they should.
- The native manifest remains the only native handoff list.
- LOC is lower because repeated validator code moved into strict helpers/macros.

### Chunk 5: Consolidate runtime and PlanExecutor responsibilities

Goal: end with one real execution model and less duplicated runtime logic.

This chunk is high risk. Do it only after baseline checks are green.

Current desired shape:

```text
boon_runtime
  public host-facing API
  runtime reports
  scenario/run glue
  delegates execution to the real executor

boon_plan_executor
  compiled plan execution
  storage/currentness/dirty propagation
  source dispatch
  delta emission
```

Alternative acceptable final shape:

```text
boon_runtime
  owns the real executor and storage/currentness loop

boon_plan_executor
  deleted or reduced to a tiny compatibility layer with no duplicate execution engine
```

Unacceptable shape:

```text
boon_runtime
  fake adapter pass

boon_plan_executor
  separate real pass

plus duplicated TodoMVC/Cells special routes
```

Find duplication:

```bash
git grep -n -E 'PlanExecutor|adapter|todomvc|cells|SourceId|compiled_schedule|dirty|delta|current|route|remaining_example_specific_shells' crates/boon_runtime/src crates/boon_plan_executor/src crates/boon_ir/src || true
```

Mechanical steps:

1. Pick the owner for source dispatch, dirty propagation, currentness, and delta emission.
2. Move one responsibility at a time.
3. Keep report evidence derived from typed IR plus compiled program, not free-form booleans.
4. Replace example-specific routes with data from compiled plans.
5. Delete the duplicate implementation bottom-up.
6. Run runtime and report checks after each moved responsibility.

Focused checks:

```bash
cargo check -p boon_plan_executor
cargo check -p boon_runtime
cargo test -p boon_plan_executor
cargo test -p boon_runtime
cargo run -p boon_cli -- dump-ir examples/todomvc.bn > target/loc-reduction/runtime-todomvc.ir.txt
cargo run -p boon_cli -- dump-ir examples/cells.bn > target/loc-reduction/runtime-cells.ir.txt
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn > target/loc-reduction/runtime-todomvc.run.txt
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn > target/loc-reduction/runtime-cells.run.txt
cargo xtask verify-runtime-finality
cargo xtask verify-report-schema
```

If performance/currentness was touched:

```bash
cargo xtask bench-example cells
cargo xtask bench-todomvc
```

Use stricter commands from the active performance plan if present.

Pass condition:

- TodoMVC and Cells still run through generic compiled/runtime paths.
- Runtime reports still include honest generic runtime evidence.
- Currentness/dirty propagation remains real, not recomputed by full snapshots unless that is already the documented current behavior.
- LOC drops by removing duplicate engines, not by deleting proof.

### Chunk 6: Native playground/window cleanup tied to render/currentness plan

Goal: reduce native duplication without breaking the active native GPU contract.

Do not treat this as a generic UI cleanup. Native code is part of performance and proof.

Protected requirements:

- Native GPU pipeline contract remains active.
- Native desktop launch must create the required two-window shape if AGENTS/current docs require it.
- Preview path receives Boon source, not example-specific render shortcuts.
- App-owned WGPU readback and host-event verifier route remain the native proof path.
- The manifest remains the handoff source of truth.

Find duplication:

```bash
git grep -n -E 'boon_native_playground|boon_native_app_window|boon_native_gpu|preview|dev|desktop|todo_mvc_physical|todomvc|cells|readback|currentness|render' crates/boon_native_playground crates/boon_native_app_window crates/boon_native_gpu docs || true
```

Allowed reductions:

- Collapse duplicate native window setup into one helper.
- Collapse duplicate preview/dev event loops only if their required differences stay explicit.
- Replace example-name render shortcuts with generic Boon-source-driven paths.
- Delete stale Ply/browser/COSMIC evidence paths if they are no longer part of the current native GPU contract.
- Consolidate repeated report writing while preserving exact fields and budgets.

Forbidden reductions:

- Removing readback proof.
- Replacing app-owned reports with human observation.
- Removing two-window behavior while AGENTS/current docs require it.
- Hiding performance/currentness failures by skipping checks.
- Duplicating manifest report lists.

Focused checks:

```bash
cargo check -p boon_native_gpu
cargo check -p boon_native_app_window
cargo check -p boon_native_playground
```

Then run the native manifest aggregate from section 6.4.

If Cells speed/currentness changed, run the active performance command. If no stricter plan exists, run:

```bash
cargo xtask bench-example cells
```

Pass condition:

- Native code is smaller because setup/report/render duplication was removed.
- Native proof path remains app-owned and manifest-backed.
- Performance/currentness gates are not weakened.

### Chunk 7: Delete modeling/manufacturing crates only after dependency proof

Goal: remove non-first-pass surfaces only when they are truly unused by current targets.

Candidates:

```text
boon_3mf
boon_mesh_export
boon_manufacturing
boon_solid_model
boon_scene_model
```

Do not delete these just because they sound future-facing. They may be used by `todo_mvc_physical`, native/document paths, report checks, or performance work.

For each candidate, run proof commands.

Replace `<candidate>` with the package name, for example `boon_scene_model`:

```bash
candidate=<candidate>
mkdir -p target/loc-reduction/deletion-proof-$candidate

cargo tree --workspace -i "$candidate" > target/loc-reduction/deletion-proof-$candidate/cargo-tree-invert.txt 2>&1 || true

git grep -n -E "$candidate|${candidate#boon_}|todo_mvc_physical|native_gpu|native_playground|document|render|report|runtime_finality|goal-readiness" -- \
  > target/loc-reduction/deletion-proof-$candidate/references.txt || true

sed -n '1,200p' target/loc-reduction/deletion-proof-$candidate/cargo-tree-invert.txt
sed -n '1,240p' target/loc-reduction/deletion-proof-$candidate/references.txt
```

Interpretation:

- If `cargo tree -i` shows a current protected crate depends on the candidate, do not delete it.
- If `git grep` shows use from `todo_mvc_physical`, native GPU, native playground, document/render, report schema, runtime finality, or goal readiness, do not delete it.
- If use is only stale docs or deleted command paths, delete those references with the feature.
- If the candidate is used only by another deletion candidate, delete the whole unused island in one chunk.

Safe deletion method:

1. Remove candidate from workspace members in `Cargo.toml`.
2. Remove dependencies on candidate from other `Cargo.toml` files.
3. Delete command registrations that expose only that candidate.
4. Delete docs that describe only the deleted future surface.
5. Delete the candidate crate directory.
6. Run focused checks.

Focused checks:

```bash
cargo metadata --no-deps
cargo check --workspace
cargo test --workspace
cargo xtask verify-report-schema
cargo xtask verify-runtime-finality
```

If native/document paths were touched, run the native manifest aggregate.

Pass condition:

- The candidate has no remaining protected dependency.
- Workspace metadata is valid.
- Current examples and gates still pass.
- No stub crate remains.

### Chunk 8: Reduce IR/type/report boilerplate using macros or generated `OUT_DIR` code

Goal: remove repeated handwritten typed-ID, visitor, traversal, and report-boundary code without changing semantics.

Good targets:

- typed ID newtypes,
- repeated `raw()` / `new()` methods,
- repeated serde impls,
- repeated visitor/folder traversals,
- repeated typed report wrappers.

Preferred committed pattern:

```rust
macro_rules! id_types {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
            pub struct $name(u32);

            impl $name {
                pub const fn new(raw: u32) -> Self { Self(raw) }
                pub const fn raw(self) -> u32 { self.0 }
            }
        )*
    };
}
```

Allowed:

- Rust macros.
- Rust `build.rs` that writes generated Rust into `OUT_DIR`, if it clearly reduces committed handwritten code and does not make debugging worse.

Forbidden:

- Python generators.
- Committed generated blobs larger than the handwritten code they replace.
- Changing JSON numeric output for typed IDs.
- Removing parser/lowering coverage checks.

Focused checks:

```bash
cargo check -p boon_ir
cargo check -p boon_typecheck
cargo check -p boon_compiler
cargo test -p boon_ir
cargo run -p boon_cli -- dump-ir examples/todomvc.bn > target/loc-reduction/ir-todomvc.txt
cargo run -p boon_cli -- dump-ir examples/cells.bn > target/loc-reduction/ir-cells.txt
cargo xtask verify-runtime-finality
```

Pass condition:

- Public report/JSON shape is unchanged unless current docs explicitly require a change.
- Typed IDs remain transparent at report boundaries.
- Repeated handwritten ID/traversal code is gone.

### Chunk 9: Final cleanup of stale docs and command references

Goal: remove stale references after code deletion.

Search for removed features and commands:

```bash
git grep -n -E 'boon_3mf|boon_mesh_export|boon_manufacturing|legacy|quarantine|old Ply|browser proof|Xvfb screenshot|whole-desktop|COSMIC scraping' -- . ':!target' || true
```

Only delete stale docs that refer to removed code or obsolete proof paths.

Do not delete architecture docs that explain historical decisions unless the user asks.

Pass condition:

- Current docs do not tell agents to use deleted commands.
- Current docs do not list a second native handoff checklist.
- Historical docs remain clearly historical if kept.

## 9. Command deletion protocol

Use this protocol whenever deleting an `xtask` or CLI command.

1. Find all references:

```bash
git grep -n '<command-name>' -- . ':!target'
```

2. Delete in bottom-up order within each file:

- tests for the command,
- docs for the command,
- help text,
- command enum/parse entries,
- dispatch match arm,
- implementation function,
- helper functions used only by that command.

3. Search again:

```bash
git grep -n '<command-name>' -- . ':!target' || true
```

4. Allowed remaining hits:

- release notes or historical docs clearly marked historical,
- this plan saying the command was removed,
- generated target files, which are not committed.

5. Run focused checks.

Do not leave a command that exists only to print "removed".

## 10. Report-schema compression protocol

Use this protocol whenever touching validators.

Before refactor:

```bash
cargo xtask verify-report-schema
```

Pick one repeated validation shape, such as:

```text
required top-level string field
required top-level bool field
required object path
required array of objects
artifact hash block
runtime_execution mirror block
native report byte-budget block
```

Add a strict helper:

```rust
fn required_str<'a>(object: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing or invalid string field `{}`", key))
}
```

Convert a small number of call sites.

Run:

```bash
cargo test -p boon_report_schema
cargo xtask verify-report-schema
```

Then delete old duplicate code bottom-up.

After refactor:

```bash
cargo xtask verify-runtime-finality
```

If any error message is part of tests or report diagnostics, preserve it or update the test intentionally. Do not silently make errors less precise.

## 11. Runtime consolidation protocol

Use this protocol whenever removing duplicate runtime/executor paths.

For each responsibility, write down current owner and target owner in `target/loc-reduction/runtime-owner-map.txt`:

```text
source dispatch:        current=?, target=?
dirty keysets:          current=?, target=?
list memory updates:    current=?, target=?
delta emission:         current=?, target=?
report evidence:        current=?, target=?
scenario replay:        current=?, target=?
```

Move one line of the map at a time.

After moving one responsibility:

```bash
cargo check -p boon_plan_executor
cargo check -p boon_runtime
cargo test -p boon_plan_executor
cargo test -p boon_runtime
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn > target/loc-reduction/runtime-map-todomvc.txt
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn > target/loc-reduction/runtime-map-cells.txt
```

Then delete the old owner code bottom-up.

Never keep both owners with a comment saying one is old.

## 12. Native performance/currentness guardrails

This cleanup must not make Cells or native GPU feel worse.

Before touching native/render/currentness code, record the current active command from README or the performance plan.

Search:

```bash
git grep -n -E 'bench-example cells|scroll-speed|visible-click|present-floor|currentness|frame-loop|60 FPS|60fps' README.md docs AGENTS.md crates/xtask || true
```

If a check exists, run it before and after the native cleanup when feasible.

Do not remove a slow path by skipping work that must happen. Remove it by:

- retaining layout/render state,
- avoiding stale full snapshots,
- sharing render setup,
- using generic source/delta data,
- deleting duplicate proof paths,
- preserving app-owned readback evidence.

If a cleanup reveals that the architecture is wrong, stop doing tiny deletions and update the active implementation plan instead of masking the blocker.

## 13. Success metrics

A chunk is successful only if all are true:

- Handwritten Rust LOC decreased or stayed flat while code became more generic.
- No protected surface was deleted.
- No verifier was weakened.
- No Python was added.
- No legacy/quarantine/stub path was added.
- Native handoff truth stayed manifest-backed.
- Focused checks for touched crates passed.
- Final aggregate gates pass before claiming the whole cleanup is done.

Good LOC reduction:

```text
large duplicate function family -> one generic function + data table
repeated validators -> strict helper/macro
old command and feature -> deleted completely
runtime adapter duplication -> one real execution model
native setup duplication -> one shared setup path
```

Bad LOC reduction:

```text
strict checker -> permissive checker
real proof -> human observation
old feature -> stub saying removed
current path -> hidden example-specific shortcut
performance blocker -> skipped work
branch workflow -> created feature branches
```

## 14. Final report format for the user

When reporting completion, say exactly:

```text
Changed files:
- ...

Deleted/compacted areas:
- ...

Checks run:
- ... passed
- ... failed: <exact reason>

LOC/byte result:
- Rust physical lines before: ...
- Rust physical lines after: ...
- Source bytes before: ...
- Source bytes after: ...

Native GPU handoff:
- Manifest used: docs/architecture/native_gpu_handoff_manifest.json
- Aggregate command used: <read from manifest>
- Aggregate report path: <read from manifest>
- Result: passed / failed / not run with exact reason

Performance/currentness note:
- Active performance plan consulted: yes/no, file if known
- Cells/native speed checks run: ...
```

Do not claim success if final gates did not run. Say exactly what passed, what failed, and what was not run.

## 15. Copying this plan into the repo

Recommended repo path:

```text
docs/plans/boon_circuit_loc_reduction_mainline_v3.md
```

After copying it there, remove older untracked LOC playbooks unless the user explicitly wants to keep them as historical brainstorming files.

Search for conflicting instructions:

```bash
git grep -n -E 'one phase per branch|feature branch|quarantine|archive removed code|Python tooling|\.py|second native.*handoff|native GPU checklist' docs/plans -- ':!target' || true
```

Allowed hits are only text that forbids those patterns.
