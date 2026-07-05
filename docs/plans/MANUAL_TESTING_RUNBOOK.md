# Manual Testing Runbook

This runbook covers human follow-up after the native GPU gates pass. It is not
part of the automated evidence path and must not replace app-owned native GPU
reports.

## Automated Baseline

Refresh native evidence with the manifest-backed handoff commands from
`docs/architecture/native_gpu_handoff_manifest.json`, then run:

```bash
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
```

The native GPU reports are the automated source of truth. They must use
app-owned host events, WGPU/readback evidence, frame identity, bounded report
payloads, and the product/proof latency split described in
`docs/architecture/NATIVE_GPU_PIPELINE.md`.

Do not use deleted or retired evidence paths:

- no legacy Ply playground;
- no Xvfb/browser/COSMIC screenshot proof;
- no `headed-ply`, `ply-headless`, focus-free headed, operator-e2e, or manual
  JSON report substitutes;
- no human observation as a shortcut for native report schema or performance
  gates.

## Visible Manual Launch

Only launch the visible playground when a human wants to inspect behavior after
the native gates have run. Use the native two-window playground:

```bash
cargo build --release -p boon_native_playground
cosmic-background-launch --workspace boon-circuit -- ./target/release/boon_native_playground --role desktop --example todomvc
cosmic-background-launch --workspace boon-circuit -- ./target/release/boon_native_playground --role desktop --example cells
```

Stop matching old playground processes for the same example before launching a
new one. The launch must create the production-style preview window and the
dev/debug window. The preview receives Boon source, not example-specific render
shortcuts.

## Human Follow-Up

Human testing can record observations such as visual polish, confusing
interaction, or compositor-specific behavior. Treat those notes as follow-up
work items, not verifier proof. If human testing contradicts native evidence,
refresh or strengthen the native report instead of weakening the schema.

For Cells, manual checks should focus on the user-visible spreadsheet contract:

- clicking a cell visibly moves selection;
- the formula/value input updates for the selected cell;
- editing a value or formula updates dependents;
- hover/focus styling is localized to the targeted cell;
- vertical and horizontal scrolling remain smooth.

Any claim that Cells is fixed still requires fresh native evidence for product
latency and proof latency as separate metrics.
