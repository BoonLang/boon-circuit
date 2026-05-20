# Manual Testing Runbook

This runbook is the final human gate for TodoMVC and Cells. It must not be
replaced by headed automation, source-event injection, or a filled template that
was not backed by a real visible session.

## Current Automated Baseline

Before manual testing, refresh the automated reports:

```bash
cargo xtask verify-foundation
cargo xtask verify-playground-launch
cargo xtask verify-playground-custom-source
cargo xtask verify-os-input-probe --report target/reports/os-input-probe.json
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-todomvc-headed-ply
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply
cargo xtask verify-todomvc-speed
cargo xtask verify-cells-speed
cargo xtask verify-todomvc-negative
cargo xtask verify-cells-negative
cargo bench -p boon_runtime --bench todomvc -- --report target/reports/todomvc-bench.json --speed-report target/reports/todomvc-bench-speed.json
cargo xtask bench-todomvc
cargo xtask bench-example cells
cargo xtask explain-todomvc-hardware --report target/reports/todomvc-hardware.json
cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn --report target/reports/todomvc-cli-run.json
cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/cells-cli-run.json
cargo xtask verify-report-schema
```

The headed reports must show:

```text
input_injection_method = os_pointer_keyboard_to_visible_window
os_input_coverage.missing_full_os_pointer_keyboard_steps = []
```

The aggregate commands reuse those full headed reports unless
`BOON_ALLOW_OS_POINTER_PROBE=1` is explicitly set again. This prevents a final
aggregate run from overwriting full OS pointer/keyboard evidence with a partial
headed report.

The current templates are generated from those headed reports:

```bash
cargo xtask verify-todomvc-human --write-template --report target/reports/manual-templates/todomvc-human.json
cargo xtask verify-cells-human --write-template --report target/reports/manual-templates/cells-human.json
cargo xtask write-manual-handoff --report target/reports/manual-handoff.json
```

They intentionally have `status = "needs_manual"`, a placeholder observer, empty
manual artifact lists, and all checklist labels set to `false`. They are not
valid human reports until a real tester has used the visible playground, filled
the session fields, and attached at least one fresh screenshot/video captured
during that manual session. Reusing only headed automation artifacts is rejected.

## Manual TodoMVC Pass

Launch the visible playground:

```bash
cargo run -p boon_ply_playground -- --example todomvc
```

On this COSMIC desktop, a background workspace launch is acceptable for opening
the surface without stealing unrelated focus:

```bash
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example todomvc
```

For a bounded background-launch smoke that exits by itself and writes evidence:

```bash
cargo xtask verify-playground-background-launch --report target/reports/playground-background-launch.json
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --smoke-launch --example todomvc --frames 3 --report target/reports/playground-background-launch-todomvc.json
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --smoke-launch --example cells --frames 3 --report target/reports/playground-background-launch-cells.json
```

Prefer the `cargo xtask verify-playground-background-launch` wrapper for
evidence. It invokes `cosmic-background-launch`, captures the printed child
PIDs/launch ids, waits for fresh TodoMVC and Cells smoke reports, validates
their schemas, and verifies the bounded child processes have exited.

Background launch only controls initial focus. Real keyboard and mouse
interaction still has to target the visible playground window, because OS input
is delivered to the active surface. That is acceptable when the machine is left
for testing; it is not a substitute for a human report unless a real person
performs the visible checklist and records the session artifacts.

Do not use background launch for the full automated headed verifier. A direct
test of `cosmic-background-launch --workspace boon-circuit -- cargo run
--release -p boon_ply_playground -- --verify-headed --example todomvc` left the
verifier process alive for 120 seconds without creating its report. That is a
failure, not evidence. The full headed verifier should be run as a directly
controlled foreground process on the dedicated testing workspace:

```bash
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-todomvc-headed-ply
```

The tester must interact with the visible TodoMVC surface and watch the scenario
checklist, semantic state, delta log, selected value inspector, and dependency
panel. Verify every label from `examples/todomvc.scn`, including:

```text
add, reject empty, toggle all, row toggle, filters, edit open/change/Enter,
edit Escape cancel, blur commit, clear completed, hover delete, delete row
```

Record the session timing and create at least one checkpoint artifact during
that interval:

```bash
mkdir -p target/reports/manual-artifacts
TODO_MANUAL_STARTED_AT=$(date +%s)
# interact with the visible TodoMVC window now
import -window root target/reports/manual-artifacts/todomvc-human-checkpoint-${TODO_MANUAL_STARTED_AT}.png
TODO_MANUAL_FINISHED_AT=$(date +%s)
TODO_MANUAL_DURATION=$((TODO_MANUAL_FINISHED_AT - TODO_MANUAL_STARTED_AT))
sha256sum target/reports/manual-artifacts/todomvc-human-checkpoint-${TODO_MANUAL_STARTED_AT}.png
pgrep -af 'boon_ply_playground|target/(debug|release)/boon_ply_playground'
```

If `import -window root` cannot capture the active COSMIC/Wayland desktop, use
the desktop screenshot tool instead, but keep the file under
`target/reports/manual-artifacts/` with `human` or `manual` in its filename and
record the real `sha256sum` output.

Create and check `target/reports/todomvc-human.json` from the TodoMVC template
only after the session. The helper computes artifact hashes and requires an
explicit `--pass-label` for every scenario label before it writes a passing
report. Do not set these labels until that checklist item has really been
verified in the visible session.

```bash
TODO_ARTIFACT=target/reports/manual-artifacts/todomvc-human-checkpoint-${TODO_MANUAL_STARTED_AT}.png
TODO_PASS_LABELS=(
  --pass-label initial
  --pass-label add-test-todo-type
  --pass-label add-test-todo-submit
  --pass-label reject-empty-todo
  --pass-label toggle-all-complete
  --pass-label toggle-all-active
  --pass-label toggle-buy-groceries
  --pass-label filter-active
  --pass-label toggle-dynamic-test-todo-under-active-filter
  --pass-label filter-completed
  --pass-label filter-all
  --pass-label edit-test-todo
  --pass-label edit-test-todo-change
  --pass-label edit-test-todo-commit
  --pass-label edit-test-todo-cancel-open
  --pass-label edit-test-todo-cancel-change
  --pass-label edit-test-todo-cancel-escape
  --pass-label edit-test-todo-blur-open
  --pass-label edit-test-todo-blur-change
  --pass-label edit-test-todo-blur-commit
  --pass-label clear-completed
  --pass-label hover-delete-clean-room
  --pass-label delete-clean-room
  --pass-label empty-state
  --pass-label add-after-clear-type
  --pass-label add-after-clear-submit
  --pass-label toggle-all-single-after-clear
  --pass-label clear-all-rows
)
cargo xtask prepare-todomvc-human-report \
  --observer "replace-with-real-tester-name" \
  --started "$TODO_MANUAL_STARTED_AT" \
  --finished "$TODO_MANUAL_FINISHED_AT" \
  --window-pid "replace-with-visible-playground-pid" \
  --focused-window-proof "replace-with-how-focus-was-confirmed-before-input" \
  --notes "replace-with-visual-quality-notes-and-deviations" \
  --capture-method "import -window root, or name the desktop screenshot/video tool actually used" \
  --artifact "$TODO_ARTIFACT" \
  "${TODO_PASS_LABELS[@]}" \
  --report target/reports/todomvc-human.json
```

Replace every `replace-with-*` value before running the helper; the checker
rejects those placeholders.

Manual screenshot/video filenames must include `human` or `manual`, and at least
one manual checkpoint must be distinct from the linked headed report artifacts.
Manual screenshot/video files must be captured during the recorded manual
session window; old files, copied headed artifacts, and screenshots created
outside the session are rejected. PNG checkpoints must have a valid PNG
signature, plausible dimensions, and nontrivial file size; MP4/WebM checkpoints
must have the expected container signature. Empty or mislabeled placeholder
files are rejected even if their hashes match the report.
The report also carries `headed_report_path` and `headed_report_sha256`; the
checker rejects the human report if the linked headed report changed, is stale,
does not pass schema validation, no longer proves full OS pointer/keyboard
input, is future-dated, or has future/inconsistent manual session timing.
It also carries the manual-session display socket, window pid/title, input
backend, capture backend, focus proof, and
`input_injection_method = human_visible_window`.
The helper writes `visual_checkpoint_pass_fail` entries for every supplied
manual artifact, and the checker rejects checkpoints that are hashed but not
listed as visually passed.
`--window-pid` is the visible manual playground process, not the earlier headed
verifier process copied into the template for binding context.
The helper also writes `manual_report_prepared_by`,
`manual_report_template_path`, and `manual_report_template_sha256`; checker mode
rejects hand-written reports that do not come through that prepared template.

Then check it:

```bash
cargo xtask verify-todomvc-human --check --report target/reports/todomvc-human.json
```

## Manual Cells Pass

Launch the visible playground:

```bash
cargo run -p boon_ply_playground -- --example cells
```

Or, on this COSMIC desktop:

```bash
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example cells
```

The tester must interact with the visible Cells surface and verify every label
from `examples/cells.scn`, including literal edits, Enter commit, Escape cancel,
formula dependencies, cycle errors, stale edge replacement, and fanout
recompute.

Record the session timing and checkpoint artifact the same way:

```bash
mkdir -p target/reports/manual-artifacts
CELLS_MANUAL_STARTED_AT=$(date +%s)
# interact with the visible Cells window now
import -window root target/reports/manual-artifacts/cells-human-checkpoint-${CELLS_MANUAL_STARTED_AT}.png
CELLS_MANUAL_FINISHED_AT=$(date +%s)
CELLS_MANUAL_DURATION=$((CELLS_MANUAL_FINISHED_AT - CELLS_MANUAL_STARTED_AT))
sha256sum target/reports/manual-artifacts/cells-human-checkpoint-${CELLS_MANUAL_STARTED_AT}.png
pgrep -af 'boon_ply_playground|target/(debug|release)/boon_ply_playground'
```

Create and check `target/reports/cells-human.json` from the Cells template only
after the session:

```bash
CELLS_ARTIFACT=target/reports/manual-artifacts/cells-human-checkpoint-${CELLS_MANUAL_STARTED_AT}.png
CELLS_PASS_LABELS=(
  --pass-label initial
  --pass-label edit-a1-literal
  --pass-label commit-a1-literal
  --pass-label edit-a1-cancel-draft
  --pass-label cancel-a1-draft
  --pass-label commit-b1-formula
  --pass-label change-a1-updates-b1
  --pass-label cycle-error
  --pass-label replace-b1-formula-removes-stale-cycle-edge
  --pass-label a1-recomputes-after-cycle-break
  --pass-label change-a1-after-edge-replacement-does-not-recompute-b1
  --pass-label commit-c1-fanout-formula
  --pass-label commit-d1-fanout-formula
  --pass-label change-a1-fanout-recomputes-dependents-only
  --pass-label d1-updated-by-fanout
)
cargo xtask prepare-cells-human-report \
  --observer "replace-with-real-tester-name" \
  --started "$CELLS_MANUAL_STARTED_AT" \
  --finished "$CELLS_MANUAL_FINISHED_AT" \
  --window-pid "replace-with-visible-playground-pid" \
  --focused-window-proof "replace-with-how-focus-was-confirmed-before-input" \
  --notes "replace-with-visual-quality-notes-and-deviations" \
  --capture-method "import -window root, or name the desktop screenshot/video tool actually used" \
  --artifact "$CELLS_ARTIFACT" \
  "${CELLS_PASS_LABELS[@]}" \
  --report target/reports/cells-human.json
```

The helper already runs this check; rerun it any time after editing the report
manually:

```bash
cargo xtask verify-cells-human --check --report target/reports/cells-human.json
```

## Final Aggregate Gate

Only after both human reports pass:

```bash
cargo xtask verify-todomvc-all --check-existing --report target/reports/todomvc-all.json
cargo xtask verify-cells-all --check-existing --report target/reports/cells-all.json
cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json
cargo xtask audit-manual-readiness --report target/reports/debug/manual-readiness.json
cargo xtask audit-goal-readiness --report target/reports/debug/goal-readiness.json
```

If a human report is missing or stale, the aggregate command writes a debug-only
blocked report under `target/reports/debug/*-all-blocked.json` and deliberately
does not create a passing top-level `*-all.json`.

`audit-manual-readiness` runs the same readiness contract but writes a
manual-specific report name. Before the real human pass exists, it should fail
only on the missing human reports and the missing final aggregate reports. After
both human reports pass, both readiness commands must pass.
