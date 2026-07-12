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
- `boon_plan_executor::Session` is the only execution owner.
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

The queue contract is:

- one queue and one `AtomicWaker`;
- timestamp at the platform callback before the queue lock;
- coalesce only adjacent pointer motion and wheel events;
- motion keeps the newest timestamp;
- summed wheel input keeps the oldest timestamp;
- preserve every discrete event in order;
- overflow is fatal because input order has been lost;
- no polling timer, event history, resampling, or public verifier injection.

`NativeSurfaceHost` converts each event to one `HostEventEnvelope`, including a
bounded callback-to-host duration. OS input and TEST/operator input enter the
same public `HostEvent` routing function. Operator events are explicitly marked
and never call a private runtime dispatch API.

## Role Protocol

Desktop, preview, and dev communicate through a custom length-prefixed binary
protocol over a private local socket. Frames have a magic value, version, typed
tag, explicit little-endian fields, and a cumulative byte limit.

The protocol carries only:

- role hello/ready and shutdown;
- catalog labels for dev;
- source-unit bundles and monotonic source revisions;
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

1. await a native event or role message while idle;
2. drain already queued events;
3. accept visible input and record its sequence/time;
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

Every visible surface receives a viewport background primitive. An empty area
must never expose an unpainted compositor-sized hole.

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

- callback-to-host: platform callback timestamp to accepted `HostEventEnvelope`;
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
returns an opaque launch ID. Before input begins, its launch-scoped reconcile
operation gathers every mapped descendant surface into that workspace without
matching titles, app IDs, geometry, roles, or example names. The standard
ext-workspace protocol activates the workspace, the public COSMIC workspace
extension resets and enables tiling, and a bounded guard restores the previous
workspace on exit or parent death.

The verifier creates a kernel uinput mouse and keyboard. Mouse motion, buttons,
wheel axes, key presses, and chords pass through the kernel, udev/libinput,
COSMIC's real seat, Wayland, and the normal app_window callbacks. Scenarios use
the same executable roles and host-event route as the product. There is no
nested compositor, private input protocol, compositor toplevel selection,
desktop scraping, or private runtime dispatch. `/dev/uinput` must be writable
by the test user. Hardware adapter, product timings, exact frame identity, and
pixels are observed from the app and app-owned WGPU readback, never inferred
from the compositor.

The system cursor is visible while the bounded workspace is active, as it is
for human interaction. It is compositor-owned and therefore intentionally not
part of app-owned WGPU readback. Cursor movement is proven by the originating
uinput process plus the corresponding `RealOs` app_window callback; rendered
application state is proven separately by exact-frame readback.

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
