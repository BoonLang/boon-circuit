# Native Wayland Test

This is a private Weston 13.0.0 input-control tool. The module creates a test
seat inside Weston and exposes the standard `weston_test` version 1 wire
interface. Pointer, button, axis, and key requests enter libweston through its
normal compositor-side input notification functions, so target clients receive
ordinary Wayland input events.

Never load the module in a production compositor. Any client connected to that
compositor can control the test seat.

## Build and smoke

From the repository root:

```sh
tools/native-wayland-test/build.sh
tools/native-wayland-test/smoke.sh
```

Both scripts require Weston and libweston development metadata at exactly
version 13.0.0. The build also requires a C compiler, `pkg-config`, and
`wayland-scanner`. Generated protocol files and both binaries are written to
`target/tools/native-wayland-test/`.

The smoke script rebuilds, starts a private headless Weston, confirms that the
driver can bind `weston_test` v1 and read its pointer position, then terminates
Weston and removes its temporary runtime directory. It does not launch Boon.

## Driver

Set `WAYLAND_DISPLAY` and, when needed, `XDG_RUNTIME_DIR` for the Weston being
controlled, then run:

```text
native-wayland-test-driver query
native-wayland-test-driver move X Y
native-wayland-test-driver button down|up [left|middle|right|CODE]
native-wayland-test-driver click [left|middle|right|CODE]
native-wayland-test-driver axis vertical|horizontal VALUE
native-wayland-test-driver wheel vertical|horizontal TICKS [DISTANCE]
native-wayland-test-driver key down|up|press KEY
native-wayland-test-driver deactivate
```

`KEY` accepts an evdev numeric code, a letter, a digit, or one of `enter`,
`escape`, `tab`, `space`, `backspace`, `delete`, `left`, `right`, `up`, and
`down`. A wheel tick defaults to 120 axis units.

## ABI boundary

The installed Weston headers do not declare the exported compositor input-seat
entry points used by Weston's own test module. `weston-test-module.c` carries
only those Weston 13 declarations. The build rejects any version other than
13.0.0, the module checks the runtime major version, and it links against
`libweston-13`.

The vendored XML preserves the request/event order of Weston's standard
`weston_test` v1 interface. The unrelated `weston_test_runner` interface is
omitted.

## Limits

Wayland object IDs are connection-local. A separate control client cannot name,
enumerate, query, or explicitly activate another client's `wl_surface` through
the standard protocol. Pointer clicks still follow Weston's normal shell
activation path, and `deactivate` uses the protocol's supported null-surface
activation request to clear keyboard focus. `move_surface`, touch, device
hotplug, and test breakpoints are rejected by this bounded module.
