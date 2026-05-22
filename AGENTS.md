# Boon Circuit Agent Notes

Treat `docs/plans/IMPLEMENTATION_PLAN.md`,
`docs/plans/EXAMPLE_VERIFICATION_PLAN.md`,
`docs/plans/TODOMVC_E2E_TEST_PLAN.md`, and
`docs/plans/MANUAL_TESTING_RUNBOOK.md` as the implementation and verification
contract.

Do not commit or push unless the user explicitly asks.

Do not fabricate `target/reports/todomvc-human.json` or
`target/reports/cells-human.json`. Those reports are only valid after a real
visible manual session with a human observer, fresh manual screenshot/video
artifacts, the visible manual playground `--window-pid`, an explicit
`--focused-window-proof`, helper provenance fields such as
`manual_report_prepared_by`, and every scenario label explicitly passed.
For Codex/operator completion, use non-human
`target/reports/todomvc-operator-e2e.json` and
`target/reports/cells-operator-e2e.json` reports. Those reports must be bound to
fresh full headed OS-input reports and must not claim human observation.

For visible manual playground launches on this COSMIC desktop, use the
workspace-qualified background launcher directly around the native window
creator:

```bash
cargo build -p boon_ply_playground
cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_ply_playground --example todomvc --mode app
cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_ply_playground --example cells --mode app
```

After launch, prove the process exists with `pgrep -af boon_ply_playground`.
If the user says the app is bothering their current workspace, stop immediately,
run `pgrep -af 'boon_ply_playground|xvfb-run|Xvfb'`, and kill only the matching
playground/test process you started.

`cosmic-background-launch` is local/custom COSMIC tooling from the sibling
`~/repos/cosmic*` checkouts, not an external immutable system command. If it
does not provide enough proof for reliable app testing, suggest improving that
tool instead of working around it here. Useful improvements would include a
machine-readable report with launched child PID, requested and actual workspace,
window identity/title/app-id, mapped/focused state, and optional targeted
screenshot metadata so agents can prove that a visible app opened in the
`boon-circuit` workspace without disturbing the user's active workspace.

Do not assume native Wayland windows are visible to `xdotool`. On this COSMIC
Wayland desktop, `xdotool search --pid <playground-pid>` can fail even when the
window is real, so lack of an xdotool window id is not proof that launch failed.
For screenshot evidence, prefer repo-generated artifacts from the playground
or xtask reports, for example:

```bash
cargo xtask verify-cells-visible-reality --report target/reports/cells-visible-reality.json
cargo xtask verify-todomvc-headed-ply
cargo xtask verify-cells-headed-ply
```

Those commands create targeted screenshot/report artifacts such as
`target/reports/cells-visible-reality-smoke.png` and headed step screenshots.
Use the `image-preflight` skill before viewing or uploading these screenshots.
If a visible COSMIC screenshot is absolutely needed, use COSMIC screenshot
portal tooling and record that it is a visible manual artifact, not automated
OS-input proof. Do not use whole-desktop screenshots as launch evidence when a
repo report screenshot is available.

Background launch is not evidence for full OS input. Full headed verification
runs in an isolated Xvfb/X11 session by default, so OS pointer/keyboard events
cannot land in the user's active desktop windows. Do not set
`BOON_ALLOW_LIVE_DESKTOP_INPUT=1` or
`BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS=1` unless the user
explicitly asks for live desktop input. Both variables are required before an
xtask verifier may target the live desktop:

```bash
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-todomvc-headed-ply
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply
```

Launch-smoke verifiers also use isolated Xvfb/X11. Do not accept whole-desktop
COSMIC screenshots as launch evidence; they can capture unrelated user windows.

Before claiming handoff readiness, run:

```bash
cargo xtask verify-report-schema
cargo xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json
cargo xtask verify-todomvc-operator-e2e --report target/reports/todomvc-operator-e2e.json
cargo xtask verify-cells-operator-e2e --report target/reports/cells-operator-e2e.json
cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
```

If `audit-machine-readiness` passes but `audit-goal-readiness` reports only
missing operator/all reports, generate the operator E2E reports from fresh
headed evidence. If human reports are missing, tell the user to continue with
human testing as follow-up instead of weakening schemas, reports, or manual
checks.
