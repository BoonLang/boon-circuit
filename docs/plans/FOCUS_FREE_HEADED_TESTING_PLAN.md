# Focus-Free Headed Testing Plan

## Goal

Run reliable native Ply playground E2E tests while the app is not foreground and
while the human user can keep working on the same desktop.

The test must still prove the real native renderer and real Boon SOURCE bindings.
It must not depend on the OS focus owner and must not send real keyboard or
pointer events to the desktop.

## Problem

The current OS-input probes use tools such as `wtype`, `ydotool`, or XTest.
Those tools target the compositor's focused surface or global pointer route. If
another window is focused, test text can be typed into the wrong app. That
happened once with `Test todo`, so this style of test is not acceptable as a
normal unattended gate.

COSMIC background workspace launching helps with window placement, but it does
not make real desktop input safe. A background window may be visible and render,
but global keyboard input still belongs to the focused application.

## Definitions

### Headed Focus-Free Test

A test that:

- launches the real native `boon_ply_playground` binary;
- creates a real Wayland/Ply/macroquad window;
- renders the real generic Ply UI;
- reads real element bounds from Ply;
- captures the app framebuffer after each step;
- drives interactions inside the verifier process from generic render metadata;
- never calls `wtype`, `ydotool`, XTest, or any desktop input injector.

This is the default headed gate.

### OS Input Probe

A separate, opt-in diagnostic that sends real OS keyboard or pointer events.
This is useful only for checking compositor input routing. It is not safe while
the user is working and must never be part of normal TodoMVC or Cells gates.

## Architecture

Add a new verifier mode:

```sh
./target/debug/boon_ply_playground \
  --verify-headed-focusless \
  --example todomvc \
  --report target/reports/todomvc-headed-focusless.json
```

Add a matching xtask wrapper:

```sh
cargo xtask verify-todomvc-headed-focusless \
  --workspace boon-circuit \
  --report target/reports/todomvc-headed-focusless.json
```

The xtask wrapper should:

1. build `boon_ply_playground`;
2. remove any stale report at the target path;
3. launch the verifier through:

   ```sh
   cosmic-background-launch --workspace boon-circuit -- \
     ./target/debug/boon_ply_playground \
       --verify-headed-focusless \
       --example todomvc \
       --report target/reports/todomvc-headed-focusless.json
   ```

4. wait for a fresh report file;
5. validate report schema and all pass/fail gates;
6. fail if the report says any OS input tool was used.

Current repo commands:

```sh
cargo xtask verify-todomvc-headed-focusless \
  --report target/reports/todomvc-headed-focusless.json
cargo xtask verify-cells-headed-focusless \
  --report target/reports/cells-headed-focusless.json
```

Installed local tools used by this gate on this machine:

```text
weston 13.0.0
wayland-utils 1.2.0
jq 1.7.1
ImageMagick 6.9.12
cosmic-background-launch
cosmic-screenshot
hyperfine 1.18.0
inotify-tools 3.22.6.0
sysstat 12.6.1 (`pidstat`)
gdb 15.1
cargo-nextest 0.9.136
watchexec 2.5.1
cage 0.1.5
```

The focus-free wrapper always sets `BOON_FORBID_OS_INPUT=1`. The verifier also
sets an internal focus-free flag from `--verify-headed-focusless`, so the safe
path does not depend on COSMIC preserving custom environment variables.

## Generic Input Driver

The focus-free driver must not contain TodoMVC-specific behavior. It should
execute scenario actions against generic rendered element IDs.

The renderer already has enough data to build this map:

- `Input`: element id, value, change source, submit source, blur source,
  escape source, address, target text.
- `Button`: element id, press source, address, target text.
- `Checkbox`: element id, press source, target text.
- `ForEach`: stable indexed native element IDs derived from hidden host list
  keys, not visible Boon identity.

Add a generic `RenderControlMetadata` collector next to the existing input
metadata collection. The collector must traverse `RenderNode` and return:

```text
element_id
element_label
visible
bounds
kind = input | button | checkbox | text
sources = change | submit | press | blur | escape
address
target_text
```

Then implement generic actions:

```text
type_text(element, text)
submit_text(element, text)
press(element)
escape(element)
blur(element)
hover(element)
```

Each action must:

1. render a frame and assert the target has visible non-zero bounds;
2. synthesize the same `LiveSourceEvent` that the generic UI callback would
   have emitted;
3. apply it through `state.apply_live_source_event`;
4. render at least one more frame;
5. capture a framebuffer screenshot;
6. assert semantic deltas, render patches, final state, and screenshot hash.

This still tests the Boon `VIEW` binding. The driver gets the source path from
the rendered element metadata, not from hardcoded TodoMVC branches.

## Scenario Format

Move headed action lists out of Rust and into scenario data. Rust may know how
to execute a generic action, but it must not know TodoMVC's row IDs or Cells'
cell IDs as code branches.

Example shape:

```text
STEP add-todo
  ACTION submit_text element=todo_new_input text="Focus-free todo"
  EXPECT source=store.sources.new_todo_input.key_down key=Enter
  EXPECT state contains "Focus-free todo"
  EXPECT render_patch kind=list_insert
```

For repeated rows, element labels can remain visible test addresses such as
`todo_row_checkbox[3]`; the hidden renderer key remains internal and must not
be Boon-visible identity.

## Screenshot Contract

For focus-free headed tests, the primary screenshot source should be the
app-owned framebuffer via `get_screen_data()`.

Do not use full-desktop screenshots as a passing proof for focus-free tests,
because a background workspace may not be visible on the current monitor.

If the macroquad framebuffer is blank:

- write a debug artifact using `cosmic-screenshot` if available;
- mark the focus-free headed gate failed;
- report `framebuffer_capture_backend = macroquad-framebuffer`;
- report `debug_desktop_capture_backend = cosmic-screenshot` separately.

This prevents a hidden focus/display issue from being hidden by desktop capture
fallbacks.

Current implementation note: macroquad framebuffer capture is blank on the
current Wayland/COSMIC path, so the focus-free verifier currently captures one
desktop checkpoint with `cosmic-screenshot` and reuses that image for per-step
artifact paths while still proving real visible Ply bounds, Boon VIEW SOURCE
metadata, semantic deltas, and render patches. This is acceptable only as an
intermediate comfort/reliability gate. The final screenshot gate still needs a
private compositor or app-owned framebuffer path that captures the actual app
pixels without depending on the active desktop workspace.

Private compositor probes on this machine:

- `weston --backend=headless-backend.so --renderer=pixman` starts, but
  `miniquad-ply` fails during Wayland/EGL initialization (`get_data_device`
  receives a null seat and EGL context creation returns `InitializeFailed`).
- `WLR_BACKENDS=headless cage -d -- ...` starts through wlroots, but the
  compositor lacks cursor-shape support for `miniquad-ply` and aborts before a
  smoke report is written.
- Keep `cosmic-background-launch --workspace boon-circuit` as the reliable
  headed path until the renderer can capture app-owned pixels or the native
  backend tolerates private headless compositors.

## Required Report Fields

Every focus-free headed report must include:

```text
status
input_backend = ply-synthetic-focus-free
os_focus_required = false
os_keyboard_or_pointer_used = false
os_input_tools_used = []
window_backend
window_pid
display_server
native_display_contract
framebuffer_nonblank = true
checkpoint_screenshot_or_video_paths
per_step_pass_fail
per_step.target_element_id
per_step.target_bounds
per_step.source_event
per_step.semantic_delta_count
per_step.render_patch_count
per_step.latency_ms
```

The report must fail if any per-step target bounds are missing, if a screenshot
is blank, if no SOURCE event was generated for an interactive step, or if any
OS input tool is invoked.

## Safety Gates

Add a hard guard:

```text
BOON_FORBID_OS_INPUT=1
```

When set, any call path that would execute `wtype`, `ydotool`, XTest, or another
desktop input injector must fail immediately with a clear error.

The focus-free xtask should always set this environment variable.

The old OS input probe must require:

```text
BOON_ALLOW_OS_INPUT_PROBE=1
```

and should print/report that it is unsafe while the user is using the machine.

## Optional Private Compositor Mode

The strongest long-term mode is a private nested Wayland compositor. In that
mode the app still creates a real Wayland window, but it is not on the user's
desktop focus path.

Preferred command shape when a compositor such as Weston or Cage is available:

```sh
dbus-run-session -- \
  weston --backend=headless-backend.so --socket boon-test-wayland --idle-time=0 &

WAYLAND_DISPLAY=boon-test-wayland \
  ./target/debug/boon_ply_playground \
    --verify-headed-focusless \
    --example todomvc \
    --report target/reports/todomvc-headed-focusless.json
```

This machine currently has `dbus-run-session`, `cosmic-background-launch`,
`wtype`, and `ydotool`, but not `weston`, `cage`, or `gamescope`. Therefore the
repo-local default should be COSMIC background launch plus in-process
focus-free input. Private compositor support can be added as an optional stronger
gate once a compositor package is installed.

## Verification Commands

After implementation, normal background-safe verification should be:

```sh
cargo fmt --check
cargo test -p boon_parser -p boon_runtime -p boon_ply_playground
cargo xtask verify-todomvc-headed-focusless --workspace boon-circuit --report target/reports/todomvc-headed-focusless.json
cargo xtask verify-cells-headed-focusless --workspace boon-circuit --report target/reports/cells-headed-focusless.json
cargo xtask verify-todomvc-reference-parity --report target/reports/todomvc-reference-parity.json
cargo xtask verify-playground-genericity --report target/reports/playground-genericity.json
```

The old full OS-input probe should be run only manually, explicitly, and while
the machine is unattended:

```sh
BOON_ALLOW_OS_INPUT_PROBE=1 cargo xtask verify-os-input-probe --report target/reports/os-input-probe.json
```

## Definition Of Done

The focus-free headed system is done when:

1. TodoMVC and Cells headed reports pass while the playground is launched into
   `boon-circuit` background workspace.
2. The user's focused app does not receive any test text or pointer movement.
3. Reports prove `os_keyboard_or_pointer_used = false`.
4. Every interactive scenario step has visible target bounds, SOURCE event,
   semantic delta, render patch, and nonblank screenshot.
5. The deterministic TodoMVC visual comparator still passes after the same run.
6. OS-input probes are quarantined behind explicit opt-in env vars.
