# BoonDriver Architecture And Implementation Plan

Status: architecture plan

Created: 2026-05-24

Related contracts:

- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/plans/STRICT_EXAMPLE_VISIBLE_TESTING_RULES.md`
- `docs/plans/NATIVE_DEV_WINDOW_EDITOR_AND_EXAMPLE_SWITCHING_PLAN.md`

## Motivation

Native app verification should be as reliable as browser automation without
depending on unsafe global desktop input. Browser tests work well because the
browser exposes an automation protocol. Playwright does not blindly type into
the user's desktop; it asks the browser to resolve elements, synthesize input
inside the controlled browser process, wait for frames, and capture screenshots
from the app surface.

Boon needs the same class of contract for native, browser/WASM, and later host
backends. Raw OS input tools such as `xdotool`, `wtype`, and `ydotool` are not a
portable foundation:

- they can target the wrong focused window;
- Wayland intentionally blocks cross-client input injection and pixel capture;
- `ydotool`/uinput is global and can type into the user's real workspace;
- macOS and Windows have different permissions and event models;
- OS input alone cannot prove which Boon source binding, runtime source ID, or
  render patch handled an event.

BoonDriver is the app-owned automation protocol. It must drive the same host
input boundary used by the app, resolve real document hit targets, produce
runtime and render evidence, and capture app-owned pixels. It is not a private
runtime shortcut.

## Goals

1. Provide deterministic app automation for native GPU tests.
2. Make interaction tests strict enough to catch blank windows, stale frames,
   wrong hit targets, missing source bindings, and non-interactive UI.
3. Keep the driver host-neutral so the same scenarios can run on native,
   browser/WASM, and future terminal backends.
4. Keep OS-specific automation behind replaceable adapters.
5. Make performance tests honest by binding metrics to source hashes, window
   identities, frame readbacks, and tested data sizes.
6. Preserve the existing safety rule: never send global live-desktop input
   unless the user explicitly opts in.

## Non-Goals

- BoonDriver must not replace real human testing.
- BoonDriver must not claim raw OS/human input unless an OS/backend adapter
  proves that tier.
- BoonDriver must not call `LiveRuntime::apply_source_event` directly when
  claiming UI interaction.
- BoonDriver must not contain TodoMVC, Cells, or future example-specific action
  branches.
- BoonDriver must not depend on browser DOM APIs, native window APIs, or a
  specific renderer in its core scenario model.

## Evidence Tiers

BoonDriver should refine the current `host-synthetic` tier into explicit tiers:

```text
runtime
  Direct runtime API calls. Useful for semantic tests only.

boon-driver
  Scenario actions go through BoonDriver, document hit testing, source bindings,
  host input routing, runtime dispatch, layout/render updates, and app-owned
  framebuffer/readback evidence.

real-window
  A platform adapter proves events were delivered by the OS/compositor to the
  exact preview/dev window and process.

human
  Manual human observation with fresh artifacts and explicit provenance.
```

`boon-driver` is allowed to be a strong automated interaction tier, but it must
not pretend to be `real-window`. A manifest may require either tier depending
on the gate. If a gate requires `real-window`, a passing `boon-driver` report is
supporting evidence, not final acceptance.

## Architecture

```text
Scenario file
  -> BoonDriver scenario parser
  -> target resolver
  -> platform input adapter
  -> host input boundary
  -> document hit/scroll/focus routing
  -> source binding dispatch
  -> runtime turn
  -> document/layout update
  -> renderer update
  -> app-owned framebuffer/readback
  -> assertions and report
```

### Core Crate Boundary

The long-term target is a small host-neutral crate:

```text
boon_driver
```

It owns:

- scenario action schemas;
- wait/assertion schemas;
- target selectors over document metadata;
- evidence/report schemas;
- driver session state;
- negative-check rules;
- adapter traits.

It must not depend on:

- `wgpu`;
- `app_window`;
- Wayland/X11/macOS/Windows APIs;
- browser DOM APIs;
- `boon_native_playground`;
- example source files.

Initial implementation may start as a module in `boon_native_playground` only
if `verify-native-gpu-architecture` enforces the same dependency and shortcut
rules. Promote it to a crate once the API stabilizes.

### Adapter Boundaries

```rust
pub trait DriverTargetResolver {
    fn resolve(&self, selector: DriverSelector) -> DriverTarget;
}

pub trait DriverInputAdapter {
    fn pointer_move(&mut self, target: &DriverTarget) -> DriverEventProof;
    fn pointer_button(&mut self, target: &DriverTarget, button: DriverButton)
        -> DriverEventProof;
    fn text_input(&mut self, target: &DriverTarget, text: &str)
        -> DriverEventProof;
    fn key(&mut self, target: &DriverTarget, key: DriverKey) -> DriverEventProof;
    fn wheel(&mut self, target: &DriverTarget, delta: DriverWheel)
        -> DriverEventProof;
}

pub trait DriverFrameAdapter {
    fn wait_for_frame(&mut self, cause: DriverFrameCause) -> DriverFrameProof;
    fn readback(&mut self) -> DriverReadbackProof;
}

pub trait DriverStateAdapter {
    fn runtime_summary(&self) -> DriverRuntimeSummary;
    fn document_summary(&self) -> DriverDocumentSummary;
    fn render_summary(&self) -> DriverRenderSummary;
}
```

Adapters may be implemented by:

- native app-owned host input;
- native compositor/real-window input;
- browser/WASM automation;
- terminal backend input;
- test-only replay harnesses.

The core driver does not know which adapter is in use. It only records the
evidence tier and verifies that the adapter's proof is strong enough for the
manifest requirement.

## Scenario Model

Scenarios should describe user intent, not runtime internals:

```text
STEP add-todo
  ACTION fill selector=source:todo.new_text text="BoonDriver todo"
  ACTION press selector=source:todo.new_submit
  EXPECT text visible "BoonDriver todo"
  EXPECT runtime list todo_count +1
  EXPECT framebuffer changed

STEP cells-scroll-horizontal
  ACTION wheel selector=role:cells-grid dx=240 dy=0 modifiers=shift
  EXPECT scroll x increased
  EXPECT text visible "Z"
  EXPECT frame_p95_ms <= 16.7
```

Allowed selectors:

- document node ID;
- source binding path;
- accessibility role/label;
- manifest scenario alias;
- stable data address exposed by the document model;
- row/column address for generic grid controls.

Forbidden selectors:

- Rust variable names;
- GPU instance IDs;
- example-specific enum variants;
- runtime private field offsets;
- visible text alone when there are multiple matching controls and no
  disambiguating role.

## Required Evidence Per Action

Every BoonDriver action must record:

- example ID and manifest path;
- source path and source hash;
- preview/dev role IDs and process/window/surface IDs when available;
- selected evidence tier;
- target selector;
- resolved document node;
- hit region or scroll region;
- focused element before and after;
- exact input event sequence;
- source binding ID and source path;
- runtime source/action IDs;
- runtime semantic delta;
- document patch summary;
- render patch summary;
- before/after frame hashes;
- readback paths and SHA-256 values;
- pass/fail assertions.

The action fails if any required layer is missing. It must never silently
downgrade from UI evidence to runtime-only evidence.

## App-Owned Screenshot Contract

BoonDriver screenshots come from app-owned framebuffer/readback paths:

- native GPU: WGPU visible surface readback or equivalent app-owned render
  target readback;
- browser/WASM: canvas or page screenshot through browser automation;
- terminal: terminal buffer capture from the backend.

Whole-desktop screenshots are debug artifacts only. They must not satisfy
driver gates because they can capture the wrong workspace or unrelated windows.

## Cross-Platform Strategy

### Native Linux

Default automation should use the app-owned BoonDriver adapter. Native
real-window evidence is owned by the native GPU contract, which proves input and
readback through app-owned host-event/WGPU paths rather than a separate
Linux-human-like harness. See `docs/architecture/NATIVE_GPU_PIPELINE.md`.

### Browser/WASM

The same scenario files should run through a browser adapter:

- selectors map to document/source-binding metadata exported by the WASM app;
- input is delivered through Playwright/WebDriver to the canvas/page;
- screenshots come from browser-owned page/canvas capture;
- frame waits use `requestAnimationFrame` and Boon frame counters;
- reports use the same BoonDriver schema.

The browser adapter may use Playwright for transport, but the scenario should
still assert Boon document/source/runtime/render evidence. DOM-only checks are
not enough for Boon.

### macOS

Use the app-owned BoonDriver adapter by default. Real-window evidence can be an
opt-in adapter using platform accessibility/event APIs only in an isolated test
session with explicit permissions. The core BoonDriver scenario and evidence
schema must not change.

### Windows

Use the app-owned BoonDriver adapter by default. Real-window evidence can be an
opt-in adapter using UI Automation/SendInput in a dedicated desktop/session.
The adapter must prove target window identity and must not rely on global focus
without verification.

### Terminal

A terminal backend can implement the same driver concepts:

- target resolver over terminal layout cells/controls;
- input adapter over backend key/mouse events;
- readback over terminal buffer snapshots;
- frame proof over render ticks.

## Implementation Plan

### Phase 1: Formalize BoonDriver Reports

- Add a `boon_driver` logical boundary or crate.
- Define `DriverScenario`, `DriverAction`, `DriverSelector`,
  `DriverActionProof`, `DriverFrameProof`, and `DriverReport`.
- Add schema validation to `boon_report_schema`.
- Add negative checks that reject:
  - runtime-only evidence mislabeled as driver evidence;
  - missing hit/source binding proof;
  - stale readbacks;
  - missing before/after frame diffs;
  - example-specific Rust branches in driver code.

### Phase 2: Wrap Existing Host-Synthetic Path

- Rename current `host-synthetic` reports to `boon-driver` where they satisfy
  the stricter path.
- Keep compatibility fields temporarily:
  - `evidence_tier: boon-driver`.
- Fail if the action bypasses the app input adapter or document hit testing.
- Update TodoMVC and Cells reports to include BoonDriver action proofs.

### Phase 3: Scenario Files

- Move interaction scenario definitions out of Rust into manifest-linked files.
- Support example-neutral actions: fill, press, key, wheel, focus, blur, wait,
  assert text, assert source, assert runtime delta, assert frame changed.
- Require every manifest scenario label to be exercised by a BoonDriver report.
- Add fixtures for future generic examples so adding a new example does not
  require Rust UI rewiring.

### Phase 4: Performance Driver

- Extend BoonDriver with frame timing and scroll measurement.
- Require release-build measurements for performance claims.
- Bind speed reports to:
  - source hash;
  - binary hash;
  - tested row/column/source length;
  - window/surface IDs;
  - frame readback hashes;
  - p50/p95/p99/max frame timings;
  - dropped frames;
  - longest stall.
- Fail if a performance test uses reduced example size, cropped source, or a
  static slice.

### Phase 5: Platform Real-Window Adapters

- Keep native real-window evidence in the native GPU verifier path.
- Add browser/WASM adapter through Playwright/WebDriver later.
- Add macOS/Windows adapters only behind opt-in platform features.
- Keep `boon-driver` as the default automated tier everywhere.

## Verification Commands

Proposed command set:

```sh
cargo xtask verify-boon-driver-schema \
  --report target/reports/boon-driver/schema.json

cargo xtask verify-boon-driver-e2e \
  --example todomvc \
  --report target/reports/boon-driver/todomvc.json

cargo xtask verify-boon-driver-e2e \
  --example cells \
  --report target/reports/boon-driver/cells.json

cargo xtask verify-boon-driver-speed \
  --example cells \
  --report target/reports/boon-driver/cells-speed.json

cargo xtask verify-boon-driver-all \
  --check-existing \
  --report target/reports/boon-driver/all.json
```

Native GPU all-example gates may depend on these reports, but they must still
fail when a manifest requires stronger `real-window` evidence and only
`boon-driver` evidence exists.

## Acceptance Criteria

BoonDriver is acceptable when:

- TodoMVC and Cells can run full scenario suites through the driver;
- all actions prove target resolution, hit testing, source binding, runtime
  dispatch, render patch, and framebuffer/readback changes;
- app-owned screenshots are nonblank and bound to the app surface;
- Cells scroll speed is measured at the required size;
- dev-editor scroll and command latency are measured on full source buffers;
- reports clearly distinguish `boon-driver`, `real-window`, and `human`;
- adding a new example needs manifest/scenario data, not Rust driver branches;
- browser/WASM can reuse the same scenario files with a different adapter;
- negative tests fail if runtime shortcuts, example shortcuts, stale artifacts,
  or wrong evidence tiers are introduced.
