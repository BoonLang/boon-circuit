# Generic Boon VIEW Playground Plan

## Decision

The native Ply preview must be example-agnostic. Rust must not choose a
TodoMVC renderer or a Cells renderer. Boon source owns the visible surface
through a `VIEW` block, and Ply is only a backend that walks the compiled view
tree.

This keeps the playground aligned with the language goal: editor changes alter
both the circuit and the rendered surface, and future examples do not require
new Rust preview widgets.

## Implemented First Step

- `examples/todomvc.bn` and `examples/cells.bn` now declare `VIEW` blocks.
- The parser strips `VIEW` blocks before semantic circuit lowering, so render
  syntax does not become state equations.
- The Ply playground parses the editor's current `VIEW` block into generic
  nodes: `Column`, `Row`, `ForEach`, `Text`, `Input`, `Button`, and `Checkbox`.
- The preview walks those nodes against the latest Boon runtime
  `state_summary`.
- SOURCE bindings are declared in the `VIEW` nodes and converted to
  `LiveRuntime` source events by the generic Ply walker.
- Editor text changes trigger a rerun from the current Boon source and refresh
  the parsed view tree.

## Non-Negotiable Regression Rules

1. No new example may add a Rust `if example == ...` preview renderer.
2. The default app view and the embedded preview must use the same generic
   Boon VIEW walker.
3. `VIEW` syntax must not affect semantic schedule lowering except through the
   explicit SOURCE paths it references.
4. TodoMVC filters, Enter-to-add, row controls, and Cells edit/commit/cancel
   must be tested through visible Ply controls.
5. Example switching must not replay whole scenarios to hide slowness.
6. Wayland evidence must be reported by headed smoke/verification commands.

## Next Hardening Work

- Move the ad hoc VIEW parser out of the playground into a small render-IR
  module with unit tests.
- Make the render IR part of the parser output while keeping semantic lowering
  isolated from it.
- Replace report hash fields so semantic hashes and source-with-view hashes are
  both visible.
- Extend the generic view walker with layout/style attributes instead of
  example-specific visual choices in Rust.
- Update headed verification to discover controls from the parsed VIEW tree
  instead of hardcoded TodoMVC/Cells element ids.
- Add a regression that fails if `boon_ply_playground` contains a
  preview-level branch on `todomvc` or `cells`.

## Manual Test Contract

Run the native app under Wayland:

```sh
cosmic-background-launch --workspace boon-circuit -- \
  cargo run -p boon_ply_playground -- --example todomvc --mode app
```

Then verify as a human user:

- TodoMVC opens at readable scale.
- Typing in the input changes Boon state.
- Enter adds a row.
- Filter buttons change `selected_filter` and the visible row list.
- Switching to Cells is quick and does not freeze.
- Cells edits commit through Enter and formula updates remain deterministic.
- The Source tab remains available and editing the `VIEW` block changes the
  preview after the editor rerun.
