# Native GPU Pipeline

Status: active architecture and verification contract.

This document is intentionally short. Historical native plans and report-v1
commands are not implementation contracts.

## Product Contract

The native playground opens two independent Wayland windows:

- preview: the production-shaped Boon application surface;
- dev: example selection, source editing, Run, Reset, TEST, status, and cached
  performance counters.

The desktop role only loads the catalog, starts both child processes, forwards
bounded binary messages, and supervises their lifetime. It has no window,
runtime, document, layout, renderer, or proof state.

Preview receives source units, never an example name. No compiler, runtime,
document, renderer, native host, or verifier code may branch on a fixture name.

## Ownership

```text
boon_parser              syntax
boon_typecheck           types and render contracts
boon_ir                  typed semantic graph
boon_plan                MachinePlan v2 and typed DocumentPlan
boon_plan_executor       Session, values, lists, indexes, currentness, deltas
boon_runtime             compile cache, scenarios, document evaluation
boon_document_model      DocumentFrame and DocumentPatch values
boon_document            retained indexes, layout, hit testing, render scene
boon_host                portable host events and semantic input
boon_native_gpu          retained WGPU renderer and product render graph
boon_native_app_window   app_window event and WGPU surface lifecycle
boon_native_playground   role composition and verifier producer
xtask                    report-v2 validation and six-gate aggregate
```

Hard rules:

- `MachinePlan` is the only executable artifact.
- `boon_plan_executor::MachineInstance` is the only mutable execution owner;
  instances share one verified `MachineTemplate` per compiled role plan.
- Runtime and product IPC contain no JSON.
- `boon_document` consumes typed patches; it does not inspect parser AST.
- `boon_native_gpu` does not depend on runtime, examples, or window events.
- `boon_native_app_window` does not depend on documents or renderer state.
- Final JSON serialization is limited to CLI and verifier/report tooling.

## Execution And Documents

Compilation produces one `MachinePlan` containing:

- storage slots and source routes;
- typed operations and demand policy;
- stable `FieldId`, `ListId`, `StateId`, and `SourceId` values;
- one typed `DocumentPlan` with stable templates, expressions, bindings, and
  visible-range materialization points.

`Duration[...] |> Timer/interval()` lowers to an ordinary source route with a
positive static `interval_ms`. The native preview waits on the earliest
scheduled source or caret deadline, dispatches through the same typed source
route as any other event, and coalesces missed timer deadlines instead of
creating a catch-up burst. Timer routing must never depend on an example id.

`Session` owns:

- scalar and row values;
- hidden row key and generation identity;
- list-field indexes;
- source routing;
- currentness barriers;
- direct and reverse dependencies;
- cycle detection;
- dirty propagation and typed deltas.

Reads used by a document or formula must be current before exposure. Cells
address lookup uses the generic list-field index. Derived cell values are
demand-current and dependency-driven; startup must not evaluate all logical
cells.

`boon_runtime` evaluates the typed `DocumentPlan` against `Session` and emits
ordered `DocumentPatch` batches. Mount emits a complete document. A turn emits
only changes caused by its typed deltas. Stable document node identity is
derived from plan template identity plus row key/generation, never labels,
geometry, or example knowledge.

Retained scalar dependencies may be value guarded. For a direct equality or
inequality against a stable captured value, the runtime indexes bindings by
dependency target and comparison value. A target change reevaluates only
unconditional bindings plus guards matching the previous or next value. All
unsupported expression shapes remain conservatively invalidated. This is a
generic document/runtime rule and must not branch on an example, node label, or
style name.

List materialization is demand based:

```text
logical rows/cells       runtime list cardinality
materialized rows/cells  current viewport plus bounded overscan and focus
rendered rows/cells      current clipped render scene
evaluated formulas       demanded current values plus dependent fanout
```

`List/chunk` preserves logical length while materializing only demanded chunks.
Scroll demand may change materialization without rebuilding the runtime graph.

## Native Event Path

The pinned `BoonLang/app_window` fork exposes one ordered asynchronous receiver
per surface. It covers pointer motion/buttons, wheel, physical/logical keys,
text and IME, focus, resize, scale, close, and accessibility actions.

The app-window queue contract is:

- one queue and one `AtomicWaker`;
- timestamp at the platform callback before the queue lock;
- coalesce only adjacent pointer motion and wheel events;
- motion keeps the newest timestamp;
- summed wheel input keeps the newest callback timestamp because the accepted
  envelope represents the completed coalesced callback burst;
- preserve every discrete event in order;
- overflow is fatal because input order has been lost;
- no polling timer, event history, resampling, or public verifier injection.

`NativeSurfaceHost` moves that receiver onto one dedicated, sleeping input pump.
The pump normalizes raw events into typed `HostEvent` values, assigns their
sequence, and closes callback-to-host timing before placing them in a second
bounded queue. It never evaluates Boon, mutates a document, lays out, renders,
or performs proof work. This keeps an in-progress surface frame from delaying
input acceptance. Both queues preserve order and fail closed on overflow.
Resize is normalized on the pump but applied later by the surface-owning render
thread before its `HostEventEnvelope` is exposed.

OS input and TEST/operator input enter the same public `HostEvent` routing
function. Operator events are explicitly marked and never call a private
runtime dispatch API.

## Role Protocol

Desktop, preview, and dev communicate through a custom length-prefixed binary
protocol over a private local socket. Frames have a magic value, version, typed
tag, explicit little-endian fields, and a cumulative byte limit.

The protocol carries only:

- role hello/ready and shutdown;
- catalog labels for dev;
- source-unit bundles and monotonic source revisions;
- immutable content-addressed asset bundles declared by the selected manifest;
- Run, Reset, and TEST requests;
- bounded preview status and scalar performance snapshots.

Source replacement is latest-wins with depth one. New work supersedes stale
pending work before expensive compilation when possible. Stale compile results
cannot replace a newer active revision.

Preview has no direct connection to dev. Dev cannot block preview presentation.
The dev performance row reads cached scalar snapshots; it performs no runtime,
IPC, JSON, or proof query from a render hook.

## Frame Transaction

Preview and dev use the same transaction shape:

1. await an accepted native event or role message while idle;
2. drain already accepted events from the bounded host queue;
3. record the visible input sequence/time selected for this transaction;
4. apply runtime changes or local viewport state;
5. patch the retained document/layout/render scene;
6. acquire the surface, encode, submit, and present;
7. publish cheap counters;
8. secure the exact proof snapshot and enqueue optional proof only after product
   presentation; product latency closes before this proof-only work.

Normal interaction performs no readback, report serialization, filesystem I/O,
process query, or full-state summary.

Surface acquisition is attempted once per transaction. Timeout, occlusion, or
reconfiguration leaves the retained scene current for a later wake; the input
path must not spin through repeated WGPU acquisition timeouts.

Frame pacing has two product states inside demand-driven mode:

- idle: no unsolicited frames;
- requested-animation burst: a bounded sequence for cursor/caret/scroll/test
  animation, ending after quiet frames or a hard cap.

Continuous probe is verifier-only and cannot satisfy product latency gates.

## Retained Rendering And Render Graph

The active `RenderScene` remains presentable while a newer source/document
snapshot is built. At most one pending snapshot exists. Newer revisions replace
older pending work. Commit requires matching source, layout, render, and surface
epochs.

The renderer owns a small explicit render graph. Resources have stable typed
identities and passes declare reads/writes. Product frames include only passes
needed for the visible scene, normally:

```text
retained scene update -> dirty upload -> opaque/alpha/text -> surface present
```

The graph must make skipped work visible in counters. It must not rediscover
identity from labels or geometry. Dirty chunks, transforms, clips, glyph runs,
assets, buffers, and pipelines remain cached across frames. Surface loss or
format/scale change increments the surface epoch and invalidates only resources
tied to that epoch.

`boon_document` is the single owner of retained scene items and text runs.
Product encoding borrows those arrays; it does not clone them into a second
renderer scene. Renderer-owned quad caches use document-provided content-addressed
chunk identities and shared immutable vertex payloads. Callers that do not
provide a trusted scene identity use uncached conversion so an arbitrary external
scene cannot reuse stale geometry.

The product renderer executes the same typed render graph with diagnostics
disabled. Cheap scalar counters remain enabled and count against the UX budget.
Full retained-chunk inventories, graph resource signatures, schedule decisions,
and report objects are built only by an explicit diagnostic/proof renderer; they
must not be constructed and discarded on normal preview frames.

Every visible surface receives a viewport background primitive. An empty area
must never expose an unpainted compositor-sized hole.

## Portable Assets, Responsive Layout, And Media

Versioned examples may declare asset files or directories in `examples/manifest.toml`.
Desktop loads and hashes those files once per example selection, assigns stable
`asset://<example>/<path>` URLs, and sends one bounded `PreviewAssets` message
before the source revision. Preview verifies every SHA-256 digest and installs
the same latest-wins immutable bundle into the product and proof renderers.
Decode, rasterization, and GPU upload are cached by content hash and requested
size. Normal render hooks never read the filesystem, fetch the network, or send
assets over IPC. A missing, mismatched, unsupported, or oversized asset fails
closed instead of silently drawing unrelated bytes.

Responsive layout is generic document behavior:

- `text_wrap` measures and shapes text within the assigned content width;
- wrapped rows use stable tracks and bounded minimum widths;
- `visible_min_width` and `visible_max_width` exclude hidden subtrees from
  layout, hit testing, semantics, and rendering, leaving no phantom gaps;
- `aspect_ratio` remains authoritative when an image has an empty sizing child;
- auto controls reserve intrinsic label width but cannot exceed a wrapped track.

These rules are portable inputs to native and future browser/Wasm renderers.
They must not branch on an example id, route label, asset filename, or provider.

`Element/embedded_media` and `Scene/Element/embedded_media` lower to the generic
`EmbeddedMedia` document and semantic role. A media descriptor carries a media
kind, provider, stable content id, title, poster asset, lazy-loading policy,
user-activation policy, embed URL, external fallback URL, sandbox policy,
referrer policy, feature permissions, and fullscreen policy. WGPU renders the
poster and ordinary Boon overlay children; it contains no YouTube-specific code.

The current native host opens the fallback URL through the platform's standard
URL launcher. A browser/Wasm host may map the same descriptor to a lazy,
sandboxed iframe after user activation. For YouTube it must use the
privacy-enhanced `youtube-nocookie.com` embed URL, set the page origin when API
control is enabled, and keep postMessage/player lifecycle in the host adapter,
not in Boon runtime or WGPU. See the official [iframe API](https://developers.google.com/youtube/iframe_api_reference),
[player parameters](https://developers.google.com/youtube/player_parameters),
and [privacy-enhanced embed guidance](https://support.google.com/youtube/answer/171780).
A platform that later supports an inline native web surface may add that as a
host capability, but the poster/external fallback remains mandatory and fully
functional on every native platform.

## Interaction State

Hit testing uses the active retained layout snapshot. Pointer hover, focus,
selection, text caret, and TEST cursor are explicit overlay/pseudo state, not
rewritten Boon expressions.

Paint/text patches keep the existing hit table. Binding or row-identity patches
update metadata only for their changed nodes. Scroll/geometry changes may
rebuild or rebucket hit bounds. A scalar selection patch must not rescan every
hit region.

Controls have generic default hover and keyboard-focus feedback when Boon style
does not override it. Cells may provide selected-state styling in regular Boon,
but the engine must not require a Cells-specific style workaround.

Passive wheel scroll updates local scroll/clip/transform state. It dispatches a
Boon source event only when the document has an explicit bound scroll source.

## Proof And Observability

Cheap counters are included in the product budget. Trace and readback proof are
explicit modes.

Verifier mode opens a separate bounded binary observer socket. Preview/dev may
publish:

- role and adapter/surface identity;
- callback-to-host samples;
- accepted input and presented-frame samples;
- source-switch acknowledgement/final samples;
- proof completion and drop counters.

No product frame waits for the observer.

Preview-to-desktop status writes use one serialized background writer. Scalar
HUD statistics are nonblocking and refreshed at no more than 10 Hz; a full
queue may discard a stats snapshot. Control messages remain ordered. The
preview event/render thread never performs a synchronous stats socket flush
after publishing a presented interaction frame.

Readback proof uses a background-priority, depth-one latest-wins worker with a
separate WGPU device and queue. Its renderer and pipelines stay alive across
requests. Preview secures the exact retained scene after the linked product
frame has presented and before publishing that frame to the verifier, so the
driver cannot race its next callback against synchronous snapshot creation.
Reports account for snapshot preparation and worker/readback time separately;
neither is part of product UX latency. Every record carries:

```text
frame_id, input_id, content_id, layout_id, render_id,
surface_id, surface_epoch, present_id, proof_id
```

Proof completion may lag. Reports name `proof_lag_frames`. UX latency excludes
readback and PNG persistence, but proof identity must match the measured frame.
Desktop screenshots, compositor scraping, Xvfb, browser screenshots, hash-only
claims, and human observation are not native proof.

## Timing Definitions

- callback-to-host: platform callback timestamp to completion of typed
  `HostEvent` normalization on the dedicated input pump; envelope metadata and
  surface-owned resize application happen after this boundary;
- input-to-present: accepted visible-changing host input to return from
  `present()` for the frame containing that input;
- render: CPU time inside retained scene encoding;
- present path: surface acquisition through submit and `present()` return;
- proof: post-present request through app-owned readback and PNG persistence.

Preview computes its own monotonic durations. Processes do not compare serialized
`Instant` values. Nearest-rank percentiles retain outliers and state warmup and
sample counts. Normal interaction lanes collect 120 samples and discard ten
warmup samples, leaving 110 measured values so p99 is distinct from max and one
bounded outlier can be represented honestly.

Product budgets:

- callback-to-host p99 <= 1 ms and max <= 2 ms;
- warm visible interaction and scroll p95 <= 16.7 ms, max <= 33.4 ms;
- warm switch acknowledgement p95 <= 16.7 ms;
- final switched preview p95 <= 250 ms, max <= 500 ms;
- settled preview plus dev CPU < 1% of one core with zero unsolicited frames.

The Cells visible-interaction sample set must include a real cell button click
through the app-window callback and the presented frame containing both the new
selection and current formula-bar text. Hover-only samples cannot satisfy the
Cells interaction gate. Scroll sampling starts only after that click frame is
presented.

Cells also has a separate repeated-selection metric: at least 24 alternating
real cell clicks, four warmup samples, and 20 measured samples with p95 at most
16.7 ms and max at most 33.4 ms. These samples remain a subset of visible
interaction evidence but are reported and gated independently.

## Verification

`docs/architecture/native_gpu_handoff_manifest.json` is the only gate list.
Public xtask commands are limited to:

```text
shaders
verify-architecture
verify-counter-dev
verify-todomvc-physical
verify-cells
verify-novywave
verify-negative
verify-all
```

The product gates launch the ordinary preview and dev windows through
`cosmic-background-launch` in the named `boon-circuit` workspace. The launcher
returns an opaque launch ID and a unique standard `wl_seat` name. Before input
begins, its launch-scoped reconcile operation gathers every mapped descendant
surface into that workspace without matching titles, app IDs, geometry, roles,
or example names. The public COSMIC workspace extension asserts tiling once
while the workspace remains inactive. Reconcile clears inherited
maximized state before mapping launch descendants into the retained tiling
layout. A bounded guard verifies that the workspace never becomes active.

The verifier creates a kernel uinput mouse and keyboard. Mouse motion, buttons,
wheel axes, key presses, and chords pass through the kernel, udev/libinput,
COSMIC's ordinary libinput backend, a launch-scoped compositor seat, Wayland,
and the normal app_window callbacks. Device names encode the returned seat;
the compositor ignores a named device when that seat does not exist. The
preview/dev processes select the seat by its standard `wl_seat.name`. Before
the first event, compositor-owned status must prove that exactly the pointer
and keyboard belong to that seat, that its target workspace is inactive, and
that every expected role window is tiled with zero floating or maximized
windows. There is no fallback to the physical seat and no input-before-layout
fallback.

The isolated seat hit-tests the retained surfaces in its inactive launch
workspace. It does not move or draw the user's compositor cursor, activate a
workspace, invoke global keyboard shortcuts, update accessibility cursor state,
or wake idle outputs. If the workspace becomes active, the compositor drops
isolated input until it is inactive again. Scenarios still use the same
executable roles and public host-event route as the product. There is no nested
compositor, private input protocol, compositor toplevel selection, desktop
scraping, or private runtime dispatch. `/dev/uinput` must be writable by the
test user. Hardware adapter, product timings, exact frame identity, callbacks,
and pixels are observed from the app and app-owned WGPU readback, never inferred
from the compositor. A TEST cursor is application-owned content and remains
visible in that readback; the isolated compositor cursor is intentionally not
rendered.

Inactive-workspace hit testing clips each toplevel to its retained tile
geometry even while the client still has an older, larger buffer attached.
Verifier discovery derives every candidate from the compositor-reported
logical output size; fixed desktop dimensions are forbidden.

Run fresh reports:

```bash
cargo xtask verify-all --report target/reports/report-v2/verify-all.json
```

Validate existing current reports without rerunning producers:

```bash
cargo xtask verify-all --check-existing \
  --report target/reports/report-v2/verify-all.json
```

The aggregate rejects stale source/tool identities, malformed fail reports,
oversized inline JSON, invalid artifacts, mismatched frame keys, software/CPU
adapter claims for passing hardware gates, proof-dependent visible updates, and
private input/runtime dispatch.

## Manual Launch

After automated gates pass, build release and launch through the workspace
qualified COSMIC helper:

```bash
cargo build --release -p boon_native_playground
cosmic-background-launch --workspace boon-circuit --frame-pacing demand -- \
  ./target/release/boon_native_playground --role desktop --example counter
```

Manual observation is a separate final confirmation for dev hover/click/wheel/
keyboard, TEST cursor/click behavior, Counter, and Cells. It cannot replace an
automated gate.
