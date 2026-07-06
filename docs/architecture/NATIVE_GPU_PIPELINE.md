# Native GPU Pipeline Architecture

This document describes the native-only GPU architecture for the Boon Circuit
playground and production preview. It is not a copy of the `boon-rust`
implementation. That repo is a useful reference for `app_window`, `wgpu`, WESL,
`wgsl_bindgen`, and `glyphon`, but this architecture must close the gaps that
reference still leaves open: no manually loaded shader shortcuts, no
example-specific renderers, no browser-backed native window, no headless-only
proof, no native API leaking into portable layers, and no dev UI that can slow
down the production preview.

Browser and terminal backends are out of implementation scope for this document.
The core contracts still need to stay host-neutral so a later browser wasm or
terminal backend can plug in without imitating native window names, Wayland
lifecycle, or GPU proof fields.

## Goals

1. Open a real native Wayland preview window and a real native dev/debug window.
2. Keep the preview path production-shaped: it can run without the dev window.
3. Keep runtime, document/layout, host input, renderer, windowing, and
   verification boundaries explicit.
4. Render only generic Boon `document` output and generic styles/components.
5. Make Cells fast at full 7GUIs size through virtualization and GPU batching,
   not by making the example smaller.
6. Make verification prove real app-owned pixels, real app_window surfaces,
   real window lifecycle, real input routing, and release-mode speed.
7. Keep the renderer replaceable behind a narrow render backend contract.

## Native Process Model

Use a role-based native executable:

```text
boon_native_playground --role preview --code-file examples/cells.bn
boon_native_playground --role dev --connect <preview-socket>
boon_native_playground --role desktop --example cells
```

`--role preview` is the production-shaped app. It owns the runtime, the loaded
Boon code, the preview window, the preview frame loop, and the preview GPU
device/queue. It renders whatever Boon code is currently loaded as the
user-facing app surface. It must not receive or branch on example names, and it
must not depend on any dev/debug widgets being loaded.

`--role dev` opens the dev/debug window. It connects to the preview role through
the role protocol. It shows the example selector, visible code editor, run/reset
/step controls, logs, inspectors, and diagnostics. Selecting an example in this
window only loads that example's Boon code into the editor and sends
`ReplaceCode` to the preview role. The preview window must render the replacement
code without knowing whether it came from a bundled example, edited source, or a
custom file. The dev role can also subscribe to bounded telemetry summaries,
coalesced debug deltas, timings, diagnostics, and explicit paged debug query
results. It must not subscribe to raw runtime snapshots or mirrored app state.

`--role desktop` is only a launcher/supervisor. The hard native path is:

1. launch the preview child process;
2. wait for the preview ready socket and first preview frame proof;
3. launch the dev child process;
4. supervise both children and record both PIDs, argv, role IDs, socket path,
   window IDs, surface IDs, and frame proofs;
5. keep preview alive when the dev window closes;
6. disconnect or terminate dev cleanly when preview closes.

`--role desktop --example <name>` is launcher convenience only. The desktop role
resolves the example to Boon code plus source hash before launching preview or
sending commands. Preview argv and preview reports must prove that the preview
role received `--code-file` or `ReplaceCode`, not `--example`.

For the hard gate, preview and dev are two child processes. Each child owns its
own `app_window::application::main`, native window, `app_window::Surface`,
`wgpu::Surface`, GPU device/queue, PID, and report identity. A same-process
multi-window implementation may be added later for experiments, but it is not
the acceptance path until it passes the same role, lifecycle, and performance
proofs.

Preview/dev roles communicate through a transport-neutral role protocol. The
native implementation uses separate processes plus bounded local IPC. A later
browser backend may use worker/postMessage or same-page channels, and a later
terminal backend may use one host surface with panes, without changing
`RuntimeTurn`, `DocumentPatch`, `LayoutFrame`, or preview telemetry schemas.
Those host-neutral schemas are internal contracts. They are not role-protocol
payloads unless explicitly wrapped in bounded, paged, coalesced debug messages.

## Crate Boundaries

The architecture should be split along ownership boundaries, not convenience.

```text
boon_parser
boon_ir
boon_runtime
boon_document_model
boon_document
boon_text
boon_host
boon_render_core
boon_native_gpu
boon_native_app_window
boon_native_playground
xtask
```

Initial implementation must add the fewest enforceable crates:
`boon_document_model`, `boon_document`, `boon_native_gpu`,
`boon_native_app_window`, and `boon_native_playground`.

`boon_text`, `boon_host`, and `boon_render_core` may start as modules inside an
owning crate only when their boundary rules remain mechanically verifiable. If a
boundary starts as a module instead of a crate, `verify-native-gpu-architecture`
must map source paths to logical boundary names and fail on forbidden `use`
paths, type names, feature gates, backend-specific proof fields, and
string/example branches inside those paths. Cargo dependency checks alone are
insufficient until the boundary is a separate crate.

### Allowed Dependency Graph

```text
boon_parser -> no native/window/GPU deps
boon_ir -> boon_parser
boon_document_model -> serde-compatible document data only
boon_runtime -> boon_ir, boon_document_model
boon_text -> renderer-neutral font metrics and shaping cache contracts
boon_document -> boon_document_model, boon_text, boon_host types
boon_host -> host-neutral event, viewport, role, and proof schemas
boon_render_core -> boon_document, boon_host
boon_native_gpu -> boon_render_core, boon_text, wgpu, glyphon
boon_native_app_window -> boon_host, app_window, wgpu surface/configuration types
boon_native_playground -> all native and core layers
xtask -> verification and launch orchestration
```

Hard dependency rules:

- `boon_runtime` may depend on document model patch types, not layout, `wgpu`,
  `app_window`, `glyphon`, native windowing, DOM, or terminal backends.
- `boon_document` may not depend on runtime, GPU, app_window, example crates, or
  OS windowing.
- `boon_native_gpu` may not depend on runtime, parser, examples, or app_window
  event APIs. It only receives layout/display-list data and a narrow render
  target.
- `boon_native_app_window` may own `wgpu::Surface` and surface
  configuration/lifecycle state. It may not depend on runtime, examples,
  document layout, renderer pipelines, shader modules, bind groups, GPU buffers,
  textures, glyphon, or renderer caches.
- `boon_native_playground` is the composition layer. It is the only crate that
  may connect runtime, document, host, native windowing, and native GPU.

### ID Ownership

Keep ID namespaces separate:

```text
Runtime: SourceId, NodeId, StateId, ListId, FieldId
Document: DocumentNodeId, SourceBindingId, ScrollRootId
Layout: HitRegionId, ScrollRegionId, RenderItemId
Renderer: GpuBufferId, GpuPipelineId, GpuInstanceId
Host: SurfaceId, WindowId, RoleId
Process: ProcessId, RoleConnectionId
```

No GPU/render IDs may appear in `RuntimeTurn`. No renderer may fabricate runtime
source IDs. No reverse mapping from GPU instances to Boon values may be used as
application state. Debug views may display cross-reference tables, but those
tables are diagnostic artifacts owned by the dev role, not Boon data.

## Runtime Boundary

### `boon_runtime`

Owns Boon execution only.

Responsibilities:

- parse/lower/execute Boon programs through the existing static graph path;
- accept typed `SourceBatch` input through public runtime APIs only;
- emit deterministic turns containing document patches, local replay/test
  snapshots, diagnostics, and metrics;
- expose cause/explanation data for the dev window.

Forbidden:

- `wgpu`, `app_window`, `glyphon`, WESL, DOM, terminal, or native windowing
  dependencies;
- example-specific TodoMVC or Cells branches;
- renderer, host, or window IDs leaking into Boon values;
- UI layout decisions that depend on screen size, DPI, GPU state, or window
  backend.

API shape:

```rust
pub struct RuntimeTurn {
    pub document_patches: Vec<DocumentPatch>,
    pub source_inventory: SourceInventory,
    pub diagnostics: Vec<Diagnostic>,
    pub metrics: RuntimeMetrics,
}

pub trait BoonProgram {
    fn mount(&mut self) -> RuntimeTurn;
    fn dispatch(&mut self, batch: SourceBatch) -> RuntimeTurn;
    fn snapshot(&self) -> RuntimeSnapshot;
}
```

`RuntimeSnapshot` is a local replay/test artifact. It must never be sent over
preview/dev IPC. Debugger views use bounded observability summaries and explicit
queries instead.

### Runtime Observability

The dev/debug window may show a real-time graph of app values, dependencies,
dirty propagation, timings, and selected data. That graph is an observability
view over the preview/runtime process. It is not a second copy of the runtime
heap and it is not a remote renderer for the preview app.

The preview/runtime role owns:

- the live Boon value graph;
- list/table/record storage;
- dependency edges and dirty sets;
- current document, layout, and render inputs;
- frame-loop state and GPU buffers.

The dev role may receive:

- bounded summaries, counters, hashes, timings, and dirty-set sizes;
- dependency graph metadata needed to draw the visible graph region;
- sampled values, histograms, ranges, and selected-node details;
- explicit paged query results for visible or selected debug panes.

The dev role must not receive a continuous full copy of:

- the runtime heap;
- every current app value;
- every list/table row;
- the full document tree every frame;
- the full display list or GPU instance stream.

If a future debugger needs bulk value history, it must be implemented as a
separate bounded trace subsystem with explicit byte caps, retention policy,
sampling/level-of-detail rules, and an off switch. It must not be the live IPC
path between preview and dev.

## Document And Layout Boundary

### `boon_document_model`

Owns serializable document data shared by runtime, layout, replay, and tests.

Responsibilities:

- define `DocumentNode`, `DocumentPatch`, `StyleValue`, `TextValue`,
  `SourceBinding`, `ScrollState`, and stable node IDs;
- keep every type serializable/replayable;
- avoid callbacks, GPU handles, OS window IDs, DOM nodes, terminal escape
  sequences, `app_window` types, `glyphon` types, and Rust closures.

### `boon_document`

Owns the renderer-neutral UI contract produced from Boon `document`.

Responsibilities:

- consume only `boon_document_model` values such as `DocumentFrame`,
  `DocumentPatch`, styles, source bindings, and scroll state;
- apply `DocumentPatch` streams into a `DocumentFrame`;
- compute deterministic CPU layout from document, viewport, scale, text metrics,
  and renderer capabilities;
- produce display lists, hit regions, scroll regions, accessibility/control
  semantics, and layout demands.

Parser/IR-to-document-model lowering belongs in the compiler/runtime path before
patches reach `boon_document`. Existing conversions that live inside the current
playground must move behind that boundary before the native GPU path is
accepted. `boon_document` does not inspect parser AST or runtime internals.

Forbidden:

- app-specific rendering branches;
- GPU objects or window objects;
- direct runtime source dispatch;
- hidden fallback views when Boon did not produce the required structure;
- terminal, DOM, Wayland, app_window, or glyphon concrete types.

API shape:

```rust
pub struct DocumentFrame {
    pub root: DocumentNodeId,
    pub nodes: DocumentNodes,
    pub focus: Option<DocumentNodeId>,
    pub scroll_roots: ScrollRoots,
}

pub enum DocumentPatch {
    UpsertNode(DocumentNode),
    RemoveNode(DocumentNodeId),
    SetText(DocumentNodeId, TextValue),
    SetStyle(DocumentNodeId, StylePatch),
    SetBinding(DocumentNodeId, SourceBinding),
    SetScroll(DocumentNodeId, ScrollState),
    SetListMaterialization(DocumentNodeId, MaterializedRange),
}

pub struct LayoutInput<'a> {
    pub document: &'a DocumentFrame,
    pub viewport: Viewport,
    pub text: &'a mut dyn TextMeasurer,
    pub capabilities: RenderCapabilities,
}

pub struct LayoutFrame {
    pub display_list: DisplayList,
    pub hit_regions: Vec<HitRegion>,
    pub scroll_regions: Vec<ScrollRegion>,
    pub accessibility: AccessibilityTree,
    pub demands: Vec<LayoutDemand>,
    pub metrics: LayoutMetrics,
}

pub enum LayoutDemand {
    MaterializeRange {
        node: DocumentNodeId,
        axis: Axis,
        visible: Range<u64>,
        overscan: Range<u64>,
    },
}
```

Cells must be expressed as a grid/list viewport in this layer. The runtime owns
the 26x100 logical cells. Layout computes visible range demand, and
runtime/document materializes stable keyed items. Renderers never pull app data
directly and never own list-window truth. A verifier must distinguish the
logical/reachable 2600 cells from the smaller materialized render range, so
virtualization is not mistaken for a shrunken grid.

### `boon_text`

Owns renderer-neutral text measurement contracts.

Responsibilities:

- define `TextMeasurer`, font keys, font metrics, shaped-run cache keys, and
  invalidation rules;
- provide deterministic line breaking and text bounds used by layout;
- allow native GPU to back the implementation with `glyphon` while preserving
  layout determinism.

The renderer must not change item geometry, line breaks, caret positions, or hit
regions after `LayoutFrame` is produced. If glyph shaping discovers missing
glyphs or atlas pressure, it reports renderer diagnostics; it does not mutate
layout.

## Host Boundary

### `boon_host`

Owns host-neutral events, surfaces, roles, and proof schemas.

Native `app_window` events are adapted into `HostEvent`/`HostInputEvent`. No
crate above `boon_native_app_window` may name `app_window`, Wayland, X11,
`xdotool`, native window handles, or `NativeInputEvent`.

API shape:

```rust
pub struct Viewport {
    pub logical_size: LogicalSize,
    pub scale: f64,
    pub physical_size: PhysicalSize,
}

pub enum HostEvent {
    Input(HostInputEvent),
    Resize(SurfaceResizeEvent),
    FocusChanged(FocusEvent),
    CloseRequested(CloseRequest),
    SurfaceLost(SurfaceId),
}

pub enum HostInputEvent {
    Key(KeyEvent),
    Text(TextInputEvent),
    Pointer(PointerEvent),
    Wheel(WheelEvent),
}

pub struct SurfaceResizeEvent {
    pub surface: SurfaceId,
    pub logical_size: LogicalSize,
    pub scale: f64,
    pub physical_size: PhysicalSize,
    pub epoch: u64,
}
```

Host input resolves through document hit testing before it becomes runtime
input:

```rust
pub struct HitResolution {
    pub surface: SurfaceId,
    pub target: Option<HitRegionId>,
    pub focused: Option<DocumentNodeId>,
    pub viewport_intent: Option<ViewportIntent>,
    pub source_intents: Vec<SourceIntent>,
}
```

`boon_host` and `boon_document` produce `SourceIntent` values only. They must not
construct or name `SourceBatch`. Only the playground composition layer may
translate `SourceIntent` plus `SourceInventory` into `boon_runtime::SourceBatch`
and call `BoonProgram::dispatch`. Native E2E tests must drive host input and hit
regions; they must not call private runtime mutation APIs or inject source
events behind the document/host route.

## Render Core And Replaceability

### `boon_render_core`

Owns renderer-neutral backend traits and proof schemas.

API shape:

```rust
pub trait RenderBackend {
    type Target;

    fn capabilities(&self) -> RenderCapabilities;
    fn render(
        &mut self,
        target: &mut Self::Target,
        frame: &LayoutFrame,
        mode: RenderMode,
    ) -> Result<RenderProof>;
}

pub enum RenderProofArtifact {
    AppOwnedPixels {
        artifact_path: Utf8PathBuf,
        artifact_sha256: String,
        capture_method: CaptureMethod,
        role_id: RoleId,
        window_id: WindowId,
        surface_id: SurfaceId,
        surface_epoch: u64,
        frame_seq: u64,
        layout_frame_hash: String,
        hash: String,
        width: u32,
        height: u32,
        nonblank_samples: NonBlankSampleReport,
    },
    CopyToPresent {
        source_texture_hash: String,
        target_surface_id: SurfaceId,
        target_surface_epoch: u64,
        target_format: SurfaceFormat,
        width: u32,
        height: u32,
        acquired_surface_texture: bool,
        command_submission_id: String,
        present_result: PresentResult,
    },
    TextCells {
        artifact_path: Utf8PathBuf,
        artifact_sha256: String,
        capture_method: CaptureMethod,
        role_id: RoleId,
        frame_seq: u64,
        hash: String,
        cols: u16,
        rows: u16,
    },
}
```

The native GPU renderer is one `RenderBackend` implementation. A future browser
or terminal renderer must be able to consume the same `LayoutFrame` and emit
backend-appropriate `RenderProofArtifact`s without changing runtime or document
schemas.

`RenderCapabilities` is a renderer-neutral, serializable contract. It may
describe portable limits and feature classes, but it must not expose backend type
names, device IDs, shader/pipeline IDs, glyph atlas state, app_window state, or
proof-only fields. `verify-native-gpu-layout-contract` must run layout against
at least one non-native/fake capability set and prove no native-only capability
is required.

Layout diffing is a private renderer optimization in v1. A public `LayoutDiff`
type may be added only after the producer, consumer, identity keys, and verifier
assertions are specified.

App-owned GPU readback is the primary screenshot evidence. The renderer may read
back the visible WGPU surface when `COPY_SRC` is supported, or read back an
explicit app-owned render target that is copied to the visible surface in the
same frame. Compositor screenshots are diagnostic only and must not be the
portable pass/fail mechanism. Hash-only render proof is not accepted; reports
must link every proof artifact by path and sha256.

## Native GPU Renderer

### `boon_native_gpu`

Owns the native GPU renderer. It consumes `LayoutFrame`, not Boon runtime state.

Responsibilities:

- own `wgpu::Device`, `wgpu::Queue`, render pipelines, buffers, textures,
  readback textures, and `glyphon` text cache;
- render generic rectangles, borders, clips, text, grids, carets, selections,
  scrollbars, and debug overlays;
- apply incremental GPU uploads as a private renderer optimization;
- expose frame timing, upload size, draw counts, text cache stats, and readback
  proof;
- render into an app-owned frame texture first, then copy/present to the host
  surface target.

Forbidden:

- Boon parser/runtime dependencies;
- TodoMVC, Cells, or example name branches;
- manual shader module creation from `.wgsl` strings outside generated binding
  wrappers;
- duplicate manual bind group, pipeline layout, or entry-point definitions that
  should come from generated `wgsl_bindgen` APIs;
- hidden CPU screenshot fallback as pass/fail evidence.

Shader pipeline:

```text
shaders/*.wesl
  -> cargo xtask shaders
  -> generated WGSL
  -> wgsl_bindgen generated Rust module
  -> boon_native_gpu uses generated Rust APIs
```

The renderer must use the generated `wgsl_bindgen` Rust API for shader modules,
pipeline layouts, bind group layouts, entry points, and pipeline constants. A
static gate must fail if the renderer bypasses this by loading generated WGSL
files directly and constructing duplicate layouts by hand.

`boon_native_gpu` must receive a narrow surface target, not an app_window object:

```rust
pub trait PresentSurface {
    fn id(&self) -> SurfaceId;
    fn viewport(&self) -> Viewport;
    fn format(&self) -> SurfaceFormat;
    fn epoch(&self) -> u64;
    fn acquire(&mut self) -> Result<SurfaceFrame>;
    fn present(&mut self, frame: SurfaceFrame) -> Result<SurfacePresentProof>;
}

pub struct NativeGpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: PipelineSet,
    glyphon: GlyphonState,
}

impl RenderBackend for NativeGpuRenderer {
    type Target = dyn PresentSurface;
}
```

`PresentSurface` exposes size, format, acquire/present, lifecycle epoch, and
proof IDs only. It must not expose window events, titles, focus state, source
bindings, example names, or app semantics.

## Native app_window Host

### `boon_native_app_window`

Owns native windows, app_window surfaces, and native event adaptation.

Responsibilities:

- initialize `app_window::application::main`;
- create one native Wayland window for each native role process;
- create exactly one `app_window::Surface` per window;
- follow `app_window::WGPU_STRATEGY` and `WGPU_SURFACE_STRATEGY`;
- adapt keyboard, text, pointer, wheel, resize, focus, close, and surface-loss
  information into host-neutral `HostEvent`s;
- keep stable `WindowId`, `SurfaceId`, `WindowRole`, raw surface identity, and
  process identity for verification.

Forbidden:

- Boon example semantics;
- renderer pipelines;
- layout decisions;
- X11-only input assumptions;
- live-desktop focus helpers as the default verification route;
- `xdotool` as required Wayland proof.

### Native Wayland Threading Contract

On Linux/Wayland, `app_window` reports `WGPU_STRATEGY = NotMainThread` and
`WGPU_SURFACE_STRATEGY = NotMainThread`. Therefore:

- `app_window::application::main` owns the process main thread;
- WGPU instance, surface creation, surface configuration, rendering, readback,
  and present run on a dedicated non-main render thread;
- app_window callbacks and input polling send bounded messages to the
  runtime/render loop;
- reports record main thread ID, render thread ID, `WGPU_STRATEGY`,
  `WGPU_SURFACE_STRATEGY`, and whether any WGPU call ran on the wrong thread.

If `app_window` cannot expose a needed per-window event, the implementation must
add a small wrapper or local app_window improvement before claiming the native
GPU path complete. Do not fake logical text input, focus, close, or per-window
wheel routing with global/coalesced state when the proof needs per-window
identity.

Required app_window wrapper/API capabilities:

- per-window input event stream or equivalent event provenance;
- logical text/IME input separate from physical key state;
- focus enter/leave;
- compositor close request;
- scale-change reporting;
- stable public window/surface identity for proof reports;
- title/app-id/proof metadata;
- resize event with logical size, scale, physical size, and epoch.

### Portable Verifier Harness

Native GPU E2E and scroll-speed gates must not depend on stealing focus from the
developer's live COSMIC desktop, on a Weston-specific test extension, or on
Linux-only global input tools. The default verifier path is host-portable:

- launch the real native two-process app through the same `app_window`/`wgpu`
  window path used by production;
- capture screenshots from the app-owned GPU output by reading back the visible
  WGPU surface or an explicit render target that is copied to the visible
  surface in the same frame;
- synthesize verifier input at the public host-event boundary, after native
  `app_window` events would normally be normalized and before document
  hit/focus/scroll routing;
- route every verifier event through `HostEvent`/`HostInputEvent`,
  document hit testing, `SourceIntent`/`ViewportIntent`, public runtime
  dispatch when a source binding exists, layout, GPU upload, render, and
  readback;
- record the event source as `operator_host_input`, not `real_os_input`, unless
  the event was actually delivered by the operating system/window backend.

The harmonizing layer is the single boundary shared by real OS input and
verifier input:

```text
OS/window event or verifier event
  -> HostEvent / HostInputEvent
  -> Document hit/focus/scroll resolution
  -> SourceIntent and/or ViewportIntent
  -> public runtime dispatch only when application source input is bound
  -> DocumentPatch / layout / render / GPU readback
```

The verifier input source must be a public API in `boon_host` or the native
composition layer. It must not call private runtime mutation APIs, mutate scroll
offsets directly, dispatch `SourceBatch` before hit/source-intent routing, send
example-specific preview commands, or bypass the renderer by fabricating
display-list or runtime state.

Reports must separate evidence tiers:

- `real_os_input`: true only for events delivered by the OS/window backend and
  observed by app_window or the platform input adapter;
- `operator_host_input`: true for verifier-synthesized host events injected at
  the harmonizing layer;
- `input_injection_method`: names the exact source, for example
  `operator_host_event_harness` or `os_pointer_keyboard_to_visible_window`;
- `visual_capture_method`: names app-owned WGPU readback, copied render target
  readback, or another explicit output-frame capture path;
- `private_runtime_dispatch_used`: always false for passing reports.

Platform-specific OS-input smoke tests may exist later for Linux, macOS, and
Windows, but they are not the default correctness or speed gate. They must reuse
the same harmonizing layer and may only upgrade `real_os_input` evidence; they
must not introduce a second testing path.

What we want from the native verifier:

- a real preview window and a real dev/debug window, with the preview rendering
  the current document selected or replaced from the dev side;
- visual proof captured from the preview renderer's own WGPU output, never from
  a whole-desktop screenshot or compositor-owned capture path;
- interaction proof driven through the same host-event API that OS events use
  after normalization, so tests cover hit testing, focus, source intents,
  scroll intents, runtime updates, layout, upload, and present;
- speed proof based on app-owned frame timing/readback and bounded renderer
  metrics, not on whether a Linux compositor accepted global input;
- report language that distinguishes operator host input from real OS input so
  portable CI/operator proof cannot accidentally masquerade as human or
  platform-input proof.

Remove the now-dead compositor-specific experiment from the implementation:

- delete nested-Weston/verifier-owned-compositor launch helpers from `xtask`;
- delete COSMIC toplevel activation and direct Wayland/ydotool/evemu fallback
  code from the native GPU verifier path;
- delete report fields that require nested-compositor provenance as mandatory
  evidence;
- keep `cosmic-background-launch --workspace boon-circuit` only for visible
  manual/operator app launches that should stay out of the user's workspace;
- keep app-owned WGPU readback and frame timing as the screenshot/performance
  evidence source.

### Surface Lifecycle

`SurfaceSlot` is owned by `boon_native_app_window` and implements
`PresentSurface`.

```rust
pub struct SurfaceSlot {
    pub window_id: WindowId,
    pub surface_id: SurfaceId,
    pub role: WindowRole,
    pub viewport: Viewport,
    pub epoch: u64,
    pub binding: SurfaceDeviceBinding,
    pub lifecycle: SurfaceLifecycle,
    // private: Window, app_window::Surface, wgpu::Surface
}
```

Each `SurfaceSlot` is bound to exactly one
`SurfaceDeviceBinding { adapter_id, device_id, queue_id, surface_id, format,
present_mode, alpha_mode, usage, epoch }`. `acquire`, `present`, and
reconfigure must reject stale epochs or frames from a different device binding.
Reports must include adapter info, chosen surface config, present mode, and
whether the adapter is software.

Required drop order:

1. stop the render loop and reject new presents;
2. submit or discard the final frame explicitly;
3. release `wgpu::Surface`;
4. release `app_window::Surface`;
5. drop `Window`;
6. report close/shutdown proof.

Resize handling must reject stale frames. A `SurfaceResizeEvent` increments the
surface epoch, invalidates layout for that viewport, reconfigures the WGPU
surface on the render thread, handles zero-size surfaces without presenting, and
records the final live size that matches the presented frame proof.

## Native Playground Orchestration

### `boon_native_playground`

Owns orchestration only.

Preview role responsibilities:

- load source and scenarios;
- run `boon_runtime`;
- maintain `DocumentFrame`;
- layout the preview document for the preview viewport;
- route host input through document hit regions into source batches;
- render preview frames on a fixed frame budget;
- publish bounded summaries, debug deltas, diagnostics, and metrics to the dev
  role.

Dev role responsibilities:

- render editor, logs, inspectors, timeline, and controls in its own native
  window;
- use the same `boon_document` -> `LayoutFrame` -> `boon_native_gpu` path for
  dev UI, with host-generated generic document data;
- send source edits and control commands to the preview role;
- consume preview observability events asynchronously;
- never block preview rendering.

Forbidden:

- rendering preview content through dev widgets;
- importing dev-only debug renderers into the preview role;
- injecting source events directly for headed/native E2E tests;
- making preview startup wait for the dev role.

## Input Contract

Native input and verifier input flow through one route:

```text
app_window, wrapper, or operator verifier event
  -> HostEvent / HostInputEvent
  -> Document hit/focus/scroll resolution
  -> SourceIntent and/or ViewportIntent
  -> SourceBatch only for application-bound source input
  -> boon_runtime dispatch
  -> DocumentPatch
  -> LayoutFrame / private renderer diff
  -> GPU upload/render
```

There must be separate event types for:

- physical key state for held keys and games;
- logical text input for editing;
- pointer press/release/click/drag;
- wheel scroll, including horizontal scroll;
- focus/blur;
- close request;
- resize/scale changes.

Passive wheel/pointer scrolling for document scroll roots is handled by
`boon_document` layout state and coalesced once per preview frame. It must not
create a `SourceBatch` or call `BoonProgram::dispatch` unless the document
explicitly binds scroll position as application source data.

Cells editing must support focus, formula bar text, raw formula/value display,
caret movement, commit/cancel, row/column scrolling, and dependent recalculation
through this same route.

The operator verifier must exercise the same route by constructing
`HostInputEvent`s at the harmonizing boundary. This is synthetic input, but it is
not synthetic runtime state and it is not synthetic scroll position. The pass
condition is the rendered app result captured from the GPU output after normal
host/document/runtime/render processing.

## Role Protocol And Backpressure

Preview-to-dev telemetry uses bounded nonblocking queues. Preview frame
rendering must never wait for dev telemetry consumption, debug rendering, IPC
writes, or telemetry serialization.

API shape:

```rust
pub enum PreviewCommand {
    ReplaceCode { code: BoundedSourceText, expected_hash: String },
    Run,
    Reset,
    Step,
    SubscribeDebug(DebugSubscription),
    QueryDebug(DebugQuery),
    SelectDiagnostic(DiagnosticId),
    Shutdown,
}

pub enum PreviewEvent {
    Ready(PreviewReady),
    Telemetry(TelemetryEnvelope),
    DebugUpdate(DebugEnvelope),
    DebugQueryResult(DebugQueryResult),
    Metrics(FrameMetrics),
    Diagnostics(BoundedDiagnostics),
    Disconnected(DisconnectReason),
}

pub struct TelemetryEnvelope {
    pub seq: u64,
    pub turn_id: u64,
    pub byte_len: usize,
    pub dropped_before: u64,
    pub payload_hash: String,
}

pub enum DebugSubscription {
    RuntimeSummary,
    DependencyGraphViewport { viewport: GraphViewport, max_nodes: usize },
    DirtyPropagationSummary,
    SelectedValues {
        ids: BoundedVec<RuntimeValueId>,
        max_values: usize,
        max_bytes: usize,
    },
}

pub enum DebugQuery {
    ValueSlice { id: RuntimeValueId, range: ValueRange, max_bytes: usize },
    DependencyNeighborhood { id: RuntimeValueId, depth: u8, max_nodes: usize },
    DocumentSlice { root: DocumentNodeId, range: ChildRange, max_bytes: usize },
}
```

Telemetry is coalesced by sequence number. Old telemetry messages may be
dropped. Commands are acknowledged separately. Source replacement commands,
debug subscriptions, and debug queries have explicit max payload sizes. Debug
updates are latest-value/coalesced by subscription. Large debug views must use
paged queries or sampled summaries instead of full-state mirroring.

Telemetry serialization must run from precomputed bounded summaries outside
runtime, document, layout, and render locks. If an IPC queue has no capacity, the
preview role must drop or coalesce before serialization; it must not clone or
serialize full runtime/document/layout/display-list state and then discover the
queue is full.

Reports must include:

- `preview_blocked_on_ipc_count`;
- `ipc_queue_depth_p50_p95_max`;
- `telemetry_serialize_ms_p50_p95_max`;
- `dropped_telemetry_count`;
- `dropped_frame_metrics_count`;
- `dropped_debug_update_count`;
- `debug_query_bytes_p50_p95_max`;
- `debug_subscription_bytes_p50_p95_max`;
- `dev_command_apply_ms_p50_p95_max`;
- `preview_heartbeat_gap_ms_max`.

`preview_blocked_on_ipc_count = 0` is a hard gate for scroll-speed,
multi-window, and split-window acceptance.

## Performance Rules

Preview performance is a product requirement, not only a renderer detail.

- The preview role has its own event loop, runtime state, document state, GPU
  resources, and surface.
- Dev telemetry is best-effort and bounded. If the dev role falls behind, the
  preview drops old telemetry instead of waiting.
- Cells grid rendering uses visible ranges, overscan, instance buffers, and text
  cache reuse.
- Wheel scrolling must update scroll offsets and visible windows without runtime
  graph rebuilds or passive-scroll runtime dispatch.
- Code editor scrolling in the dev role must use the same virtualized list/text
  infrastructure, not a giant per-line widget tree.
- Release-mode frame reports must include p50/p95/p99/max frame time, missed
  frame count, sample count, upload bytes, draw calls, visible nodes, text runs
  shaped, and dropped debug telemetry.

### Scroll Hot Path

Cells body/header scrolling and dev code-editor scrolling must report:

- `runtime_dispatch_count_for_passive_scroll`;
- `graph_rebuild_count`;
- `wheel_events_coalesced`;
- `input_queue_depth_max`;
- `layout_rebuild_scope`;
- `newly_materialized_range_count`;
- `scroll_frame_ms_p50_p95_p99_max`;
- `missed_frame_count`;
- `sample_frame_count`;
- `sustained_scroll_duration_ms`;
- `scroll_distance_px_rows_cols`;
- `materialized_range_before_after`;
- `visible_address_samples_before_after`;
- `wheel_to_visible_ms_p95_per_axis`.

For passive scroll, `runtime_dispatch_count_for_passive_scroll = 0` and
`graph_rebuild_count = 0` are hard gates.

### GPU Batching Budgets

After warmup, Cells scrolling must update scroll uniforms plus only newly
exposed row/column instance/text ranges. It must not allocate a widget, bind
group, pipeline, or GPU buffer per logical cell.

Initial 26x100 upload may be O(logical grid size). Steady scrolling must be
O(visible range + overscan delta). Release reports must include:

- `draw_calls_p50_p95_max`;
- `queue_write_count_p50_p95_max`;
- `upload_bytes_p50_p95_max`;
- `instance_count_visible`;
- `instance_count_uploaded`;
- `pipeline_switch_count_p95`.

Starting hard gates for 26x100 Cells after warmup:

- `draw_calls_p95 <= 16`;
- `queue_write_count_p95 <= 8`;
- `upload_bytes_p95 <= 262144`.

### Text Shaping And Cache

Text shaping is cached by font face, font size, scale factor, style, and text
content. Scroll must reuse shaped runs and glyph atlas entries; it may shape
only newly exposed rows/lines or changed editor text after warmup.

Reports must include:

- `text_runs_visible`;
- `text_runs_shaped`;
- `text_shape_cache_hits`;
- `text_shape_cache_misses`;
- `text_shape_cache_evictions`;
- `glyph_atlas_upload_bytes`;
- `glyph_atlas_evictions`.

Cells and code-editor scroll gates fail if a steady scroll frame reshapes every
visible text run.

## Verification Gates

The architecture is not accepted until the handoff reports listed in
`docs/architecture/native_gpu_handoff_manifest.json` exist and pass in release
mode on this Wayland machine, followed by the manifest-backed aggregate:

```text
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
```

Every native GPU gate must overwrite or remove any prior passing report before
execution and write a schema-valid pass/fail report. `verify-native-gpu-all`
must link every required native GPU report by path and sha256, and it is the
native GPU acceptance aggregate for this architecture. Its required report list
comes from the manifest, which is the single source of truth for handoff report
labels, paths, commands, required arguments, inline JSON byte budgets, and JSON
sidecar byte budgets. It also owns native handoff upstream report dependencies
through `upstream_dependencies` in
`docs/architecture/native_gpu_handoff_manifest.json` and the aggregate
`report_dependency_graph`. Native handoff dependencies must be native reports.
PlanExecutor source replay is semantic/runtime evidence owned by the
BYTES/MachinePlan verifier, not native GPU proof, and must not be used as a
preview E2E upstream or as a substitute for app-owned host-input, retained
runtime/output, product-render-graph, or WGPU/readback evidence.

Some native gates consume other native side reports. Those dependencies must be
explicit in the aggregate's report dependency graph, must name
`verify-native-gpu-all` as their owning aggregate, and must contribute bounded
`refresh_commands[].argv` entries when stale. A native product failure must not
depend on an untracked side report; refresh upstream native report dependencies
first, rerun the aggregate, and only then debug fresh product-contract
blockers.

The aggregate control plane must classify stale identity before data-plane
validation. If a child report was generated for a stale git commit, stale
worktree fingerprint, stale scoped verifier identity, or stale legacy binary
hash, the aggregate records refresh debt with the manifest-canonical command and
skips schema, native-contract, semantic, and artifact validation for that child
until it is regenerated. Fresh child reports still receive full validation.
Stale children must not manufacture product-contract blockers. The aggregate
must expose top-level `true_blocker_child_count` and `true_blocker_children`
alongside refresh-debt fields so tooling can distinguish "rerun reports first"
from "fix product or verifier code now" without inspecting large child reports.
If a fresh child report fails only because its own `blockers[]` describe stale
consumed evidence, such as a stale preview E2E report or stale framebuffer
artifact, the aggregate must classify that child as refresh debt rather than a
true product blocker. Fresh true blockers are reserved for failures that remain
after the consumed evidence is current.

Native reports carry both the legacy full `worktree_fingerprint` and scoped
fingerprints in `worktree_fingerprints`. The handoff aggregate may use the
`native-gpu-handoff` scoped fingerprint for native child freshness. The scoped
fingerprint includes product/verifier inputs such as `crates/`, `examples/`,
`budgets/native-gpu.toml`, `AGENTS.md`, Cargo metadata,
`NATIVE_GPU_PIPELINE.md`, and the handoff manifest. It intentionally excludes
progress ledgers and goal-prompt prose so plan-doc churn does not stale
otherwise current product reports. Scoped fingerprint material must include the
scoped committed `HEAD` tree/blob identity plus scoped dirty status and diff;
status/diff-only scoped fingerprints are not sufficient freshness evidence
after commits. Older reports without the scoped field are refresh debt for the
handoff aggregate; the full worktree fingerprint is preserved only as legacy
diagnostic context and must not make a missing scoped fingerprint fresh.

Native `xtask` reports also carry `verifier_identity`, a scoped verifier
contract identity containing the identity kind, scheme version, command,
measurement mode, contract version, canonical verifier arguments, and identity
hash. When present, `verifier_identity` is authoritative for verifier freshness:
a matching scoped identity may supersede a stale legacy `binary_hash`, while a
mismatched scoped identity is refresh debt even if the legacy binary hash
matches. Reports without `verifier_identity` fall back to the legacy
`binary_hash` check. This keeps harmless aggregate/schema binary churn from
forcing product verifier reruns without weakening stale-report detection for
reports that have no scoped identity.

PlanExecutor source replay reports must distinguish execution from comparison in
the BYTES/MachinePlan verifier. `plan_executor_status` and
`accepted_for_product_status` describe whether the PlanExecutor replay is usable
as semantic product evidence; `comparison_status` describes optional legacy
oracle parity. Native preview E2E does not consume those reports.

PlanExecutor source replay reports carry source-replay freshness evidence
separate from native verifier identity. `run-plan-scenario-events` reports keep
the legacy full `worktree_fingerprint`, but also include
`worktree_fingerprint_scope=plan-executor-source-replay`, scoped entries in
`worktree_fingerprints`, and `source_replay_identity`. The scope is limited to
Cargo metadata plus the compiler/runtime/PlanExecutor/CLI crates needed to
produce source replay behavior; source and scenario contents remain checked by
their own hashes and MachinePlan recomputation. `source_replay_identity` binds
the command, measurement mode, canonical arguments with `--report` removed,
source hash, scenario hash, target profile, plan hash/version, selected step
surface, and PlanExecutor coverage surface. The BYTES/MachinePlan aggregate may
use the `plan-executor-source-replay` scoped fingerprint and fresh
`source_replay_identity` instead of exact full-worktree/git matching; missing or
stale source-replay identity remains BYTES/MachinePlan refresh debt, not a
native GPU product blocker.

The preferred unattended refresh controller is `xtask
run-report-refresh-queue ... --until-clean --max-runs N`. It executes selected
aggregate-owned refresh commands, reruns the owning aggregate after each cycle,
and stops only when the aggregate is clean, the selected labels are burned down,
or a bounded stop reason such as `max-runs`, `refresh-command-failed`, or
`post-aggregate-unavailable` is reported. `--closed-loop` is an alias for
`--until-clean`; `--rerun-aggregate` remains a one-cycle aggregate rerun. Queue
reports must include
`closed_loop_requested`, `closed_loop_max_runs`, `closed_loop_stop_reason`,
final refresh debt counts, selected-label counts, and per-cycle summaries.
Dry-runs must be schema-valid and explicitly report `closed_loop_stop_reason =
dry-run` plus a skipped post-refresh aggregate rerun. Bulky refresh controller
arrays such as `results`, `closed_loop_cycles`, owner-rerun lists, and
remaining-refresh-command lists may be emitted as JSON sidecars with SHA-256 and
byte-length metadata; inline scalar counts remain canonical for quick
classification.

Refresh queues are dependency-aware. When a native refresh command depends on an
upstream report declared by the handoff manifest, the queue must select and run
the upstream refresh before the native consumer, mark the execution phase in
`refresh_execution_plan`, and report whether an entry was selected directly by
label or expanded from dependency edges. Upstream BYTES-owned replay refreshes
must be followed by their owner aggregate rerun before native preview consumers
are treated as meaningful product evidence. This keeps stale prerequisite
reports from masquerading as Cells/native rendering failures.

When selected refresh entries execute through `boon_cli`, the queue must run a
single `cargo build -p boon_cli` preflight before executing non-dry replay
commands and must report the result in `boon_cli_prebuild`. Dry-runs report the
same preflight as `skipped-dry-run`. A failed prebuild fails only the selected
`boon_cli` refresh entries instead of running a stale binary.

The broader product/regression gates remain required before claiming the
dev-editor/example-switch recovery complete, but they are not part of the
handoff aggregate unless this document and `AGENTS.md` are updated together:

```text
cargo xtask verify-native-counter-interaction-speed --report target/reports/native-gpu/counter-interaction-speed.json
cargo xtask verify-native-cells-interaction-speed --profile debug --report target/reports/native-gpu/cells-interaction-speed-debug.json
cargo xtask verify-native-cells-interaction-speed --profile release --report target/reports/native-gpu/cells-interaction-speed-release.json
cargo xtask verify-native-gpu-idle-wake --example counter --report target/reports/native-gpu/idle-wake-counter.json
cargo xtask verify-native-gpu-idle-wake --example todomvc --report target/reports/native-gpu/idle-wake-todomvc.json
cargo xtask verify-native-gpu-idle-wake --example cells --report target/reports/native-gpu/idle-wake-cells.json
cargo xtask verify-native-gpu-idle-wake --custom-project-fixture target/fixtures/native-gpu/custom-projects.json --report target/reports/native-gpu/idle-wake-custom-projects.json
cargo xtask verify-native-real-window-input-environment --report target/reports/native-gpu/real-window-input-environment.json
cargo xtask verify-native-visible-launch --example todomvc --report target/reports/native-gpu/todomvc-visible-launch.json
cargo xtask verify-native-visible-launch --example cells --report target/reports/native-gpu/cells-visible-launch.json
cargo xtask verify-native-examples --all --report target/reports/native-gpu/native-examples.json
cargo xtask verify-native-dev-window-editor --example todomvc --report target/reports/native-gpu/dev-editor-todomvc.json
cargo xtask verify-native-dev-window-editor --example cells --report target/reports/native-gpu/dev-editor-cells.json
cargo xtask verify-native-example-tabs --report target/reports/native-gpu/example-tabs.json
cargo xtask verify-native-editor-format --report target/reports/native-gpu/editor-format.json
cargo xtask verify-native-dev-editor-scroll-speed --profile debug --report target/reports/native-gpu/dev-editor-scroll-speed-debug.json
cargo xtask verify-native-dev-editor-scroll-speed --profile release --report target/reports/native-gpu/dev-editor-scroll-speed-release.json
cargo xtask verify-native-example-switch-speed --profile debug --report target/reports/native-gpu/example-switch-speed-debug.json
cargo xtask verify-native-example-switch-speed --profile release --report target/reports/native-gpu/example-switch-speed-release.json
cargo xtask verify-native-example-speed --example cells --report target/reports/native-gpu/speed-cells.json
cargo xtask verify-native-dev-editor-speed --report target/reports/native-gpu/dev-editor-speed.json
cargo xtask verify-native-gpu-regression-all --check-existing --report target/reports/native-gpu-regression-all.json
```

Do not run the legacy readiness audits as part of this architecture gate. The
old `verify-report-schema`, `audit-machine-readiness`, `audit-goal-readiness`,
Ply headed/headless, COSMIC split-launch, Xvfb, and browser/playground-smoke
paths belong to the historical verification system and must not be used to prove
the native two-window GPU playground. If broader repo readiness is rebuilt later,
it must consume `target/reports/native-gpu-all.json` without launching or
requiring legacy Ply/COSMIC/browser surfaces.

`verify-platform-contract` must fail if core crates expose `app_window`, `wgpu`,
`glyphon`, Wayland/X11, native process/window IDs, DOM types, terminal escape
sequences, or backend-specific proof fields outside the native/backend crates
that own them.

`verify-native-gpu-dependency-graph` must use `cargo metadata` or `cargo tree`
to reject forbidden edges, including:

- `boon_runtime` -> layout/GPU/window/native/browser/terminal crates;
- `boon_document` -> runtime/GPU/window/native/browser/terminal crates;
- `boon_native_gpu` -> runtime/parser/examples/app_window event APIs;
- `boon_native_app_window` -> runtime/document layout/examples/GPU pipelines.

`verify-native-gpu-architecture` must fail on:

- `wgpu`, `app_window`, or `glyphon` dependencies in runtime/parser/IR crates;
- `boon_runtime` or example crate dependencies in `boon_native_gpu`;
- `todomvc`, `todo_mvc`, `cells`, `pong`, or `arkanoid` branches in preview,
  document/layout, render core, renderer, or app_window code. Example-name
  handling is allowed only in the desktop/dev example resolver, and it must
  produce Boon source plus source hash before sending `ReplaceCode`;
- private runtime dispatch shortcuts in native E2E code;
- manual generated-WGSL loading in the renderer instead of generated
  `wgsl_bindgen` APIs;
- macroquad/miniquad/Ply dependencies in the new native GPU path;
- fallback screenshots used as pass/fail evidence.

`verify-native-gpu-layout-contract` must feed generic document fixtures plus
TodoMVC/Cells runtime outputs into `boon_document`, then assert deterministic
`LayoutFrame`, stable hit regions, accessibility/control semantics,
virtualization bounds, and no full 26x100 Cells widget expansion. It must also
run layout against at least one fake/non-native `RenderCapabilities` set and
prove layout does not require native-only capabilities.

`verify-native-gpu-shaders --check` must prove generated WESL/WGSL/bindgen
outputs are fresh, no `include_str!` or direct generated-WGSL loading is used in
the renderer, and no duplicate manual bind group or pipeline layouts exist
outside generated APIs.

`verify-native-gpu-multiwindow` must launch the desktop role and prove:

- two real native Wayland `app_window` child processes exist: preview and dev;
- each child has its own `application::main`, PID, argv, window, surface, GPU
  device/queue, and report identity;
- each window has an independent app_window surface and WGPU surface;
- reports include app_window backend, `WAYLAND_DISPLAY`, `WGPU_STRATEGY`,
  `WGPU_SURFACE_STRATEGY`, main thread ID, render thread ID, role IDs, window
  IDs, surface IDs, surface epochs, logical/physical sizes, scale, app-owned
  texture hash, copy-to-present proof, and current git commit;
- the preview surface renders a nonblank frame before the dev window finishes
  rendering its first full debug frame;
- closing the dev window does not stop preview rendering;
- closing the preview window shuts down the preview role cleanly and causes the
  dev role to show disconnected state or exit cleanly;
- Xvfb/X11/headless/native-browser substitutes are rejected for this native
  Wayland gate.

On COSMIC, the multiwindow gate may use `cosmic-background-launch` only as the
workspace-qualified process launcher. The gate must record `requested_workspace
= "boon-circuit"`, preview/dev child PIDs, PID cmdlines, native role reports,
and launcher report/artifact hashes. It must not use COSMIC toplevel scraping,
compositor activation, or whole-desktop screenshots as pass/fail evidence. If
the launcher cannot provide machine-readable process proof, the gate must fail
with a blocker naming the missing launcher capability.

`verify-native-gpu-ipc-backpressure` must stall or kill the dev role and prove
preview frame p95, memory cap, dropped telemetry count, IPC heartbeat behavior,
and continued preview rendering satisfy explicit budgets. It must read
`budgets/native-gpu.toml`, record `budget_hash`, and fail on missing budget
fields. The report must include explicit thresholds and observed values for
preview frame p95, heartbeat gap max, RSS/memory cap, dropped counts, queue
depth, debug bytes, and `preview_blocked_on_ipc_count = 0`.

`verify-native-gpu-observability` must enable a large runtime value graph and a
busy dev graph view, then prove debug subscriptions and queries are bounded,
paged, or sampled. It must fail if the dev role receives a continuous full copy
of the runtime heap, every app value, every list/table row, the full document
tree each frame, or full display-list/GPU instance streams. It must also prove
debug-update drops/coalescing are reported and preview frame timing remains
within explicit budget thresholds while the dev graph is overloaded. It must use
the same `budgets/native-gpu.toml` and report `budget_hash`, thresholds, and
observed values.

`verify-native-gpu-preview-e2e` must drive visible native controls and assert
runtime state, source inventory, frame hashes, hit targets, source intent
routing, and timing. It must not pass by calling private source dispatch
functions directly. It must write non-human operator reports under
`target/reports/native-gpu/preview-e2e-<example>.json`, bound to fresh
operator-host-input evidence by default. Reports must include scenario labels,
source/scenario hashes, window PID/cmdline, owned-surface proof, per-step
host-input route, hit target, source intent, runtime assertions,
screenshot/readback artifacts, and must not claim human observation or real OS
input unless real OS/window events were actually observed.

`verify-native-gpu-scroll-speed --example cells` must fail unless it is a
release-mode native GPU report with operator host wheel input or stronger,
current git/source hashes,
vertical and horizontal scroll evidence, `scroll_frame_ms_p95 <= 16.7`,
`wheel_to_visible_ms_p95 <= 50`, `preview_blocked_on_ipc_count = 0`,
`runtime_dispatch_count_for_passive_scroll = 0`, `graph_rebuild_count = 0`, and
no Xvfb/X11, headless-only, compositor-specific, private runtime dispatch, or
synthetic scroll-position evidence. It must perform sustained vertical and
horizontal wheel scroll over the full 26x100 logical grid through
`HostInputEvent::Wheel`, record visible address samples before and after outside
`A0:D0`, require `missed_frame_count = 0`, require `wheel_to_visible_ms_p95 <=
50` per axis, keep `materialized_cell_count_max` within the
visible-plus-overscan budget, capture the rendered result through GPU readback,
and list every frame over 16.7 ms as an outlier.

`verify-native-gpu-idle-wake` must prove the preview and dev child processes use
the demand-driven render loop in an idle desktop launch. Reports must use child
PID procfs tick deltas for CPU, include skipped idle polls, dirty/presented
revisions, last scheduler/role dirty reasons, app-owned readback hashes before
and after post-idle input and source replacement, and reject copied first-frame
hashes, stale PIDs, fake CPU samples, COSMIC/Ply/desktop screenshots, human
observation, and any wake branch selected by example name, visible label,
filesystem path, scenario name, or custom-example origin. The same verifier must
cover Counter, TodoMVC, Cells, and a table-driven custom-project fixture.

`verify-native-dev-editor-scroll-speed` supersedes the old dev-code-editor
surface scroll report for the manual user-facing editor path. The old
`verify-native-gpu-scroll-speed --surface dev-code-editor` compatibility alias
has been removed and must fail closed; use
`verify-native-dev-editor-scroll-speed --profile debug|release` instead. The
dedicated dev-editor gate must run in both debug and release profiles, use a
passive scroll-only probe, cover vertical and horizontal `scroll_column`
updates, include a selected custom-example buffer, and fail if `command_probe`,
source replacement, preview runtime summaries, graph rebuilds, full-file
materialization, full-file reshaping, or footer telemetry polling occur in the
scroll hot path.
Current speed targets are `wheel_to_visible_ms_p95 <= 35 ms` in debug and
`<= 16.7 ms` in release, with corresponding max-frame budgets in
`budgets/native-gpu.toml`.

`verify-native-example-switch-speed` must prove source switching uses the
generic async source/project payload path. Reports must cover Counter, TodoMVC,
Cells, two single-file custom examples, one multi-file custom project, rapid
A-B-A switching, duplicate/renamed labels, changed logical paths, and an invalid
custom source that preserves the last good preview frame. The synchronous ACK is
limited to command/source revision, hashes, queue status, byte counts, and
timing; it must not contain full source text, layout proof, runtime state,
runtime summary, parse/lower output, or debug summaries. Dev tab visuals must
update before preview parse/lower/runtime/layout work or preview ACK completion.

`verify-native-gpu-negative` must mutate or fabricate reports for stale
git/source/binary hashes, missing artifacts, future timestamps, Xvfb/headless
substitution, fake real-OS-input claims, synthetic scroll-position evidence,
private runtime dispatch, copied pixel hashes, stale surface epochs,
wrong-thread WGPU calls, single-process multiwindow masquerade, full-state IPC
mirroring, nested-compositor-only evidence in the portable gate, and stale
shader outputs. Each fabricated case must be rejected.

## Migration Plan

Implementation may be staged, but final acceptance for this architecture is the
two-process preview/dev desktop path with all gates passing. A preview-only probe
is an intermediate milestone, not handoff readiness.

1. Keep the native GPU path isolated from legacy browser, Ply, Xvfb, and
   compositor-probing verifiers.
2. Keep human observation as follow-up evidence only; do not weaken native GPU
   report requirements to make a manual path look complete.
3. Implement `boon_document_model`, `boon_document`, `boon_text`, `boon_host`,
   and `boon_render_core` as pure portable contracts first. Keep any
   module-based boundaries mechanically checked until they become crates.
4. Add `cargo xtask shaders` for this repo and wire WESL -> WGSL ->
   `wgsl_bindgen` into `boon_native_gpu`.
5. Build a one-process preview probe that renders generic document data through
   `app_window`/`wgpu`, with app-owned texture readback plus copy-to-present
   proof.
6. Build the desktop role with preview and dev as separate native child
   processes.
7. Connect preview role to the real Boon runtime and public source dispatch
   path.
8. Add virtualized Cells grid rendering and code editor scrolling.
9. Add the platform, dependency, architecture, layout, shader, multi-window,
   IPC, observability, E2E, negative-fixture, and scroll-speed gates.
10. Keep removed legacy verifier paths out of `xtask`; old command names may
    only fail fast with a native-GPU replacement message.

At no point should implementation make an example smaller, hardcode an example
renderer, accept a browser-backed native window, accept headless proof as native
Wayland proof, or weaken existing human/manual report schemas to make the new
path look complete.
