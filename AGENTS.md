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

For visible manual playground launches on this COSMIC desktop, use the
workspace-qualified background launcher directly around the native window
creator:

```bash
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example todomvc
cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example cells
```

Background launch is not evidence for full OS input. Full headed verification
must run as a controlled focused session:

```bash
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-todomvc-headed-ply
BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply
```

Before claiming handoff readiness, run:

```bash
cargo xtask verify-report-schema
cargo xtask audit-goal-readiness --report target/reports/debug/goal-readiness.json
```

If `audit-goal-readiness` reports only missing human/all reports, report that
honestly as the remaining blocker instead of weakening schemas, reports, or
manual checks.
