# Boon Circuit Agent Notes

Treat `docs/architecture/NATIVE_GPU_PIPELINE.md` as the active implementation
and verification contract for native window work. The older plan files still
document historical Ply/runtime verification, but they are not the source of
truth for the native two-window GPU playground.

Do not commit or push unless the user explicitly asks.

When Boon source exposes a real compiler, typechecker, runtime, or engine
limitation, fix the engine instead of writing a Boon-level workaround. A
workaround is acceptable only as a temporary diagnostic step while proving the
engine bug, and it must not be left as the final implementation.

Do not fabricate human-observation reports. Human testing is a separate
follow-up after the native GPU gates pass, and it must not be used as a shortcut
for native GPU verifier evidence.

For visible native playground launches on this COSMIC desktop, use the
workspace-qualified background launcher around the native GPU desktop entrypoint
only when a visible manual launch is explicitly needed:

```bash
cargo build -p boon_native_playground
cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_native_playground --role desktop --example todomvc
cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_native_playground --role desktop --example cells
```

After finishing native playground work that the user should manually test,
restart the latest playground in release mode in the `boon-circuit` COSMIC
background workspace unless the user explicitly says not to. Do not leave
multiple old release playgrounds running for the same example: first inspect
existing matching processes, stop only the matching release playground process
tree for that same example, then build and launch the current binary:

```bash
pgrep -af 'boon_native_playground.*todo_mvc_physical'
# Stop only the matching old desktop/preview/dev PIDs for the same example.
cargo build --release -p boon_native_playground
cosmic-background-launch --workspace boon-circuit -- ./target/release/boon_native_playground --role desktop --example todo_mvc_physical
```

The native desktop launch must create two native windows: a production-style
preview window and a dev/debug window. The preview process/window receives Boon
source, not example names or example-specific render shortcuts.

After launch, prove the process exists with `pgrep -af boon_native_playground`.
If the user says the app is bothering their current workspace, stop immediately,
run `pgrep -af 'boon_native_playground'`, and kill only the matching
playground/test process you started.

`cosmic-background-launch` is local/custom COSMIC tooling from the sibling
`~/repos/cosmic*` checkouts, not an external immutable system command. It may be
used only as the workspace-qualified process launcher. Do not use COSMIC
toplevel scraping, compositor activation, or whole-desktop screenshots as
verification evidence; the native proof must come from app-owned reports,
process IDs, host events, and WGPU readback.

Do not use legacy Ply, Xvfb, whole-desktop screenshots, `xdotool`, `ydotool`,
direct COSMIC toplevel probing, or browser windows as evidence for the native
GPU path. Use app-owned WGPU readback screenshots, native GPU reports, and the
host-event verifier route described in `docs/architecture/NATIVE_GPU_PIPELINE.md`.

Before claiming native GPU handoff readiness, run only the native GPU gates:

```bash
cargo xtask verify-platform-contract --report target/reports/native-gpu/platform-contract.json
cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json
cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json
cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json
cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json
cargo xtask verify-native-gpu-multiwindow --report target/reports/native-gpu/multiwindow.json
cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json
cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json
cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json
cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json
cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json
cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
```

If human observation is still needed after the native gates pass, tell the user
to continue with human testing as a separate follow-up. Do not weaken native GPU
schemas, reports, budgets, or negative checks to pass old Ply/COSMIC readiness
audits.
