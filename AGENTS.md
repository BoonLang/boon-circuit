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
