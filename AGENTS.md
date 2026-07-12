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

For Cells/native performance work, do not get trapped in a loop of tiny local
optimizations after the same gate keeps failing. Once fresh measurements show
the same class of blocker repeatedly, zoom out and reassess the architecture:
input scheduling, retained layout/render state, runtime currentness,
readback/verification design, and the Boon example structure are all valid
places to change. It is acceptable to simplify or restructure example Boon code
when that makes the intended app cleaner, but do not hide engine limitations in
example-specific hacks.

When the user asks for or explicitly permits parallel help, use subagents for
independent reads before continuing micro-fixes. Good splits include native
input/event-shape analysis, retained WGPU/render/readback architecture,
runtime/list/currentness design, and external architecture research on how
spreadsheet-like apps keep selection, editing, and scrolling at 60 FPS. Fold
their findings into a larger implementation plan instead of treating each
finding as another isolated patch.

Do not fabricate human-observation reports. Human testing is a separate
follow-up after the native GPU gates pass, and it must not be used as a shortcut
for native GPU verifier evidence.

For visible native playground launches on this COSMIC desktop, use the
workspace-qualified background launcher around the native GPU desktop entrypoint
only when a visible manual launch is explicitly needed:

```bash
cargo build -p boon_native_playground
cosmic-background-launch --workspace boon-circuit --frame-pacing demand -- ./target/debug/boon_native_playground --role desktop --example todomvc
cosmic-background-launch --workspace boon-circuit --frame-pacing demand -- ./target/debug/boon_native_playground --role desktop --example cells
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
cosmic-background-launch --workspace boon-circuit --frame-pacing demand -- ./target/release/boon_native_playground --role desktop --example todo_mvc_physical
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

Do not use Ply, Xvfb, whole-desktop screenshots, `xdotool`, `ydotool`,
direct COSMIC toplevel probing, or browser windows as evidence for the native
GPU path. Use app-owned WGPU readback screenshots, native GPU reports, and the
host-event verifier route described in `docs/architecture/NATIVE_GPU_PIPELINE.md`.
Automated product scenarios use the repository's kernel-uinput device role on
the ordinary COSMIC seat. Workspace setup may use only the opaque launch ID,
the standard workspace protocols, and the fork's launch-scoped reconcile
operation; it must not identify or activate toplevels by title, app ID, role,
geometry, or example name.

Before claiming native GPU handoff readiness, run only the native GPU handoff
reports listed in `docs/architecture/native_gpu_handoff_manifest.json`, then run
the manifest-backed aggregate:

```bash
cargo xtask verify-all --check-existing --report target/reports/report-v2/verify-all.json
```

The manifest is the single source of truth for handoff report labels, paths,
commands, required arguments, inline JSON byte budgets, and JSON sidecar byte
budgets. Do not maintain a second handoff list in AGENTS.md.

If human observation is still needed after the native gates pass, tell the user
to continue with human testing as a separate follow-up. Do not weaken native GPU
schemas, reports, budgets, or negative checks to pass old Ply/COSMIC readiness
audits.
