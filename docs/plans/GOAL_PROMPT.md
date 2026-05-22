# `/goal` Prompt

Use this prompt for the next unattended native GPU implementation pass.

```text
Implement and verify the native GPU playground architecture in /home/martinkavik/repos/boon-circuit. Treat docs/architecture/NATIVE_GPU_PIPELINE.md as the primary contract, with AGENTS.md and the existing docs/plans/*.md verification rules still binding. Do not commit or push unless explicitly asked.

Start from current HEAD and do not restart the repo. The current native GPU path has useful scaffolding, generated shader bindings, report-schema extraction, and contract gates, but it is not complete. The known hard blockers from the last review are:
- `verify-native-gpu-multiwindow`, `verify-native-gpu-ipc-backpressure`, `verify-native-gpu-observability`, `verify-native-gpu-preview-e2e`, and `verify-native-gpu-scroll-speed` are still blocked stubs.
- `boon_native_playground` must become a real desktop supervisor that spawns two child processes: preview and dev/debug.
- The preview child must receive only Boon source through `--code-file` or `ReplaceCode`, never an example name.
- The dev child may resolve examples to source, edit/replace source, and observe preview through bounded telemetry/query IPC only.
- `boon_native_app_window` must own real Wayland app_window windows/surfaces and adapt native events into generic host events.
- `boon_native_gpu` must present real app-owned pixels through wgpu and generated shader APIs, not scaffold proof fields.
- Report-schema/native-gate checks must reject fabricated reports, stale reports, missing artifact hashes, headless/X11 proof, private runtime dispatch, copied-pixel-only proof, full-state IPC mirroring, and same-commit stale reports.
- Runtime/report ownership must not regress. Keep runtime focused on Boon execution. Move any remaining report/window enrichment out of runtime into callers or `boon_report_schema` when practical.
- Shader verification must prove the real WESL/WGSL/wgsl_bindgen pipeline. Do not let marker strings or copy-only WESL-to-WGSL output count as completion.

Implementation requirements:
1. Native desktop role starts two native Wayland child processes, one preview and one dev/debug, with independent app_window/wgpu surfaces. Browser tabs are out of scope for this pass.
2. Preview renders the selected Boon source with generic document/layout/render components only. No example-specific preview branches, no Rust shortcuts, no hardcoded TodoMVC or Cells behavior.
3. Dev/debug window shows the code editor and controls by default, sends `ReplaceCode`, and consumes bounded telemetry or paged queries. It must never mirror the full runtime/document/layout/display-list state over IPC.
4. IPC must be bounded and measurable: queue depth, dropped telemetry, byte caps, serialization timings, heartbeat gaps, and proof that preview rendering does not block on dev/debug overload.
5. Cells and dev editor scrolling must be fast under real Wayland OS wheel input, including vertical and horizontal scroll. Do not make Cells smaller to pass. Prove no passive-scroll runtime dispatch and no graph rebuild.
6. Host/document/render boundaries must stay typed and replaceable enough that native GPU can later be swapped, and browser/terminal frontends can be added later without leaking app_window/wgpu into core crates.

Verification requirements:
- cargo fmt --check
- cargo check -p boon_report_schema -p boon_runtime -p xtask -p boon_native_gpu -p boon_native_app_window -p boon_native_playground
- cargo test -p boon_report_schema -p boon_runtime --lib --no-fail-fast
- cargo test -p xtask advertised_xtask_commands_are_unique_and_supported
- cargo xtask verify-platform-contract --report target/reports/native-gpu/platform-contract.json
- cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json
- cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json
- cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json
- cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json
- cargo xtask verify-native-gpu-multiwindow --report target/reports/native-gpu/multiwindow.json
- cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json
- cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json
- cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json
- cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json
- cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json
- cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json
- cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json
- cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
- cargo xtask verify-report-schema

Use real visible Wayland/app_window evidence for native E2E and speed gates. Do not use headless/Xvfb as native proof. If COSMIC/Wayland tooling or app_window lacks an API needed for reliable proof, document the blocker and improve the local wrapper/tooling instead of weakening gates. When all non-human native GPU gates pass, stop and tell the user what remains for human visible testing.
```

Short slash command:

```text
/goal follow docs/plans/GOAL_PROMPT.md and complete the native GPU playground implementation and verification contract.
```
