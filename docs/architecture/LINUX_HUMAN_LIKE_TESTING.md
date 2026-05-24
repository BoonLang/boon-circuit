# Reliable Linux Human-Like Testing Plan

Status: architecture plan

Created: 2026-05-24

Related contracts:

- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/architecture/BOON_DRIVER.md`
- `docs/plans/STRICT_EXAMPLE_VISIBLE_TESTING_RULES.md`

## Purpose

This document describes how to get reliable Linux tests that are closer to real
human input than the app-owned BoonDriver tier, without typing into the user's
active desktop.

The current native verifier can prove strict app-owned interaction through the
host input boundary, but the hard `real-window` tier remains blocked on this
machine because the isolated Weston probe does not expose an automation-capable
seat or virtual input protocols, and live `ydotool`/uinput would target the
active desktop globally.

Reliable human-like testing must be isolated, repeatable, and safe by default.

## Definitions

### BoonDriver Tier

App-owned automation. It drives the same host input boundary the app uses,
resolves document hit targets and source bindings, and captures app-owned
framebuffer/readback artifacts. It does not prove compositor-delivered OS input.

### Human-Like Linux Tier

Automated Linux test where input is delivered through an isolated compositor or
test desktop to the exact native app window. It must prove:

- target compositor/display identity;
- target process/window/surface identity;
- pointer/keyboard/wheel delivery through a compositor seat or equivalent;
- app-owned or compositor-owned output capture;
- no possibility of typing into the user's active desktop.

### Live Desktop Tier

Input is injected into the user's current desktop session. This is not allowed
by default. It requires explicit user consent and must be treated as a manual or
dangerous diagnostic path, not normal CI.

## Why The Current Weston Probe Fails

The current report says:

```text
has_wl_seat=false
has_virtual_keyboard=false
has_virtual_pointer=false
```

That means the isolated Weston instance was not an automation-capable desktop
for our test. A Wayland display can exist while still not providing the input
objects or protocols needed by a verifier to deliver keyboard, pointer, and
wheel events to a client.

This is not a Boon runtime problem. It is a host/compositor capability problem.
Wayland intentionally prevents ordinary clients from injecting input into other
clients or reading other clients' pixels. A reliable test must either cooperate
with the compositor or own the compositor/test desktop.

## Safety Rules

- Never use `ydotool`, `wtype`, XTest, or similar global input tools against
  the live user desktop by default.
- Never treat whole-desktop screenshots as passing evidence.
- Never infer input success from process existence.
- Never mark a test as `real-window` unless event provenance is bound to the
  exact preview/dev PID, window ID, and surface ID.
- Always clean up test compositor and app processes.
- Always fail closed when compositor capabilities are missing.

## Preferred Architecture

```text
xtask verifier
  -> isolated Linux test session
  -> automation compositor or controlled desktop
  -> preview/dev native app processes
  -> compositor seat input injection
  -> app_window input adapter
  -> Boon host input boundary
  -> document/source/runtime/render
  -> app-owned readback and compositor output capture
  -> report
```

The Linux human-like adapter should be a platform adapter under the BoonDriver
architecture. BoonDriver remains the scenario engine; Linux human-like testing
only changes the input/capture backend and raises the evidence tier when it can
prove compositor delivery.

## Standard Options

### Option A: Boon Test Compositor

Build a small test compositor specifically for Boon verification.

Possible implementation bases:

- `smithay`;
- wlroots through a small helper process;
- a minimal custom compositor if the required protocol surface stays narrow.

Required features:

- isolated Wayland socket;
- `wl_compositor`;
- `xdg_wm_base`;
- real `wl_seat`;
- pointer, keyboard, and wheel event injection owned by the verifier;
- virtual keyboard/pointer equivalent or internal test control API;
- per-surface/window identity tracking;
- output/frame capture;
- deterministic frame clock controls or frame notifications;
- no connection to the user's live desktop.

Pros:

- strongest long-term safety and repeatability;
- no global desktop input;
- can expose exactly the evidence Boon needs;
- portable scenario semantics through BoonDriver;
- can be CI friendly.

Cons:

- most implementation work;
- must be maintained as part of the test infrastructure;
- must match enough Wayland behavior to exercise `app_window` honestly.

This is the recommended long-term Linux path.

### Option B: Configured Nested Compositor

Use an existing compositor such as Weston, Cage, gamescope, or another
wlroots-based wrapper, but only if capability probing proves it exposes the
required automation surface.

The verifier must check:

- isolated socket is used;
- `wl_seat` exists;
- keyboard/pointer/wheel injection is available;
- app windows can be targeted by identity, not focus guessing;
- output capture is available;
- teardown is reliable.

Pros:

- less code if an existing compositor already exposes the right protocols;
- useful for early CI.

Cons:

- capabilities vary by build, version, backend, and distro;
- nested compositors often lack the exact virtual input/capture protocols;
- failures can look like app bugs unless the probe is strict.

This can be an interim path, but it must not be assumed. The probe decides.

### Option C: Isolated VM Or Dedicated Test Desktop

Run the app in a full Linux VM or dedicated test user session, then use global
input tools inside that isolated environment.

Allowed tools inside isolation:

- `ydotool`/uinput;
- compositor-specific automation;
- accessibility tooling if available;
- screenshot tools scoped to the VM/test display.

Pros:

- closest to real human desktop behavior;
- can use standard desktop tools without risking the user's session;
- good for release candidates and nightly validation.

Cons:

- heavier than a compositor helper;
- slower setup;
- harder artifact transfer and debugging;
- still needs strict process/window binding.

This is the safest way to use global input tools.

### Option D: Live Desktop Opt-In

Use `ydotool`/uinput or compositor tools against the user's active desktop only
when the user explicitly opts in.

Required consent variables should remain intentionally loud:

```sh
BOON_ALLOW_LIVE_DESKTOP_INPUT=1
BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS=1
```

This path is for local diagnostics only. It must not be a default gate and must
not run while the user is working unless they explicitly request it.

## Human-Like Input Contract

A Linux human-like action must record:

- isolated display socket;
- compositor implementation and version;
- compositor capability report;
- target preview/dev PID;
- target window ID and surface ID;
- target role report path;
- target hit region;
- input device/seat proof;
- event sequence and timestamps;
- app input adapter provenance;
- source binding resolved;
- runtime semantic delta;
- app-owned before/after readback;
- optional compositor output capture;
- teardown proof.

The report fails if:

- input target is inferred only from focus;
- no seat or virtual input path exists;
- app-owned readback is missing;
- compositor output capture is stale or from the wrong surface;
- any event can escape to the user's live desktop;
- the scenario passes only through runtime shortcuts.

## Capture Contract

Human-like Linux tests should capture two kinds of pixels:

1. App-owned readback from the native GPU app.
2. Compositor/output capture from the isolated compositor when available.

App-owned readback proves the renderer drew content. Compositor capture proves
the isolated display received and presented it. The strongest report includes
both and compares them enough to detect blank, stale, wrong-window, or
single-color output.

Whole-desktop COSMIC screenshots are not acceptable passing evidence because
they can capture unrelated user windows and the wrong workspace.

## Implementation Plan

### Phase 1: Capability Probe

Add a command:

```sh
cargo xtask verify-linux-human-like-environment \
  --report target/reports/linux-human-like/environment.json
```

It should:

- start the candidate isolated display;
- run `wayland-info` or equivalent against that display;
- detect `wl_seat`;
- detect virtual keyboard/pointer or an equivalent test-control API;
- detect output capture support;
- launch a tiny app_window smoke client;
- inject a pointer, key, and wheel event;
- capture a frame;
- write a pass/fail report.

The command must fail with precise blockers instead of falling back to live
desktop input.

### Phase 2: BoonDriver Platform Adapter

Implement a Linux adapter for BoonDriver:

```text
boon_driver_linux
```

Responsibilities:

- launch isolated compositor/test session;
- launch preview/dev app roles under that display;
- resolve native window/surface IDs;
- deliver pointer/keyboard/wheel events through the compositor seat;
- collect app input adapter provenance;
- collect compositor capture when available;
- clean up processes.

The adapter must not know TodoMVC or Cells semantics. It executes BoonDriver
scenario actions.

### Phase 3: Human-Like Example Gates

Add commands:

```sh
cargo xtask verify-linux-human-like-e2e \
  --example todomvc \
  --report target/reports/linux-human-like/todomvc.json

cargo xtask verify-linux-human-like-e2e \
  --example cells \
  --report target/reports/linux-human-like/cells.json

cargo xtask verify-linux-human-like-speed \
  --example cells \
  --report target/reports/linux-human-like/cells-speed.json
```

These commands should:

- reuse manifest scenario labels;
- reuse BoonDriver scenario files;
- require isolated display capability pass;
- require release builds for speed claims;
- require full Cells size;
- write evidence tier `real-window` only when compositor delivery is proven.

### Phase 4: CI Profile

Define two CI profiles:

```text
boon-driver-ci
  Fast, app-owned, cross-platform scenario verification.

linux-human-like-ci
  Slower Linux-only isolated compositor or VM verification.
```

The human-like profile may run nightly or pre-release if it is too heavy for
every commit. It should still be deterministic and unattended.

### Phase 5: Live Desktop Diagnostic

Keep live desktop input as an explicit diagnostic:

```sh
BOON_ALLOW_LIVE_DESKTOP_INPUT=1 \
BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS=1 \
cargo xtask verify-linux-live-desktop-input \
  --example cells \
  --report target/reports/linux-human-like/live-desktop-cells.json
```

This must:

- print a warning before launch;
- require both consent variables;
- bind events to app windows when possible;
- never run as part of `verify-native-gpu-all`;
- never generate human-observation reports.

## Report Paths

Proposed report layout:

```text
target/reports/linux-human-like/environment.json
target/reports/linux-human-like/todomvc.json
target/reports/linux-human-like/cells.json
target/reports/linux-human-like/cells-speed.json
target/reports/linux-human-like/all.json
```

Debug artifacts:

```text
target/artifacts/linux-human-like/<run-id>/compositor.log
target/artifacts/linux-human-like/<run-id>/wayland-info.txt
target/artifacts/linux-human-like/<run-id>/preview-readback.png
target/artifacts/linux-human-like/<run-id>/dev-readback.png
target/artifacts/linux-human-like/<run-id>/output-capture.png
target/artifacts/linux-human-like/<run-id>/input-trace.json
```

## Acceptance Criteria

Reliable Linux human-like testing is acceptable when:

- it runs without touching the user's active desktop;
- the capability probe fails clearly when the compositor lacks required input;
- TodoMVC and Cells scenarios run through the same scenario data as BoonDriver;
- events are delivered through an isolated compositor seat or equivalent;
- reports prove exact preview/dev process/window/surface identity;
- app-owned readbacks and compositor/output captures are fresh and nonblank;
- Cells scroll speed is measured at the required size in release mode;
- failures identify whether the problem is compositor capability, app input,
  runtime state, rendering, screenshot capture, or performance;
- `verify-native-gpu-all` can depend on the human-like reports only when the
  target environment actually supports them;
- live desktop input remains opt-in and never runs as a normal unattended gate.

