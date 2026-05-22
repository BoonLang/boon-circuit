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
custom file. The dev role can also subscribe to snapshots, deltas, timings, and
diagnostics.

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

`boon_document_model`, `boon_text`, `boon_host`, and `boon_render_core` may be
separate crates or modules inside a larger crate at first, but the dependency
rules below must be enforceable from day one.

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
boon_native_app_window -> boon_host, app_window
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
- `boon_native_app_window` may not depend on runtime, examples, document layout,
  or GPU pipelines.
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
- emit deterministic turns containing document patches, state snapshots,
  diagnostics, and metrics;
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

- lower parser/runtime document data into `DocumentFrame`;
- apply `DocumentPatch` streams into a `DocumentFrame`;
- compute deterministic CPU layout from document, viewport, scale, text metrics,
  and renderer capabilities;
- produce display lists, hit regions, scroll regions, accessibility/control
  semantics, and layout demands.

Parser AST -> `DocumentFrame` lowering belongs here or in a compiler-facing
document module, not in native host or renderer crates. Existing conversions
that live inside the current playground must move behind this boundary before
the native GPU path is accepted.

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

impl SourceBatch {
    pub fn from_document_intents(intents: &[SourceIntent]) -> Result<Self>;
}
```

Only the playground composition layer may call `BoonProgram::dispatch`. Native
E2E tests must drive host input and hit regions; they must not call private
runtime mutation APIs or inject source events behind the document/host route.

## Render Core And Replaceability

### `boon_render_core`

Owns renderer-neutral backend traits and proof schemas.

API shape:

```rust
pub trait RenderBackend {
    type Target;

    fn capabilities(&self) -> RenderCapabilities;
    fn upload_layout(&mut self, diff: &LayoutDiff) -> Result<UploadStats>;
    fn render(
        &mut self,
        target: &mut Self::Target,
        frame: &LayoutFrame,
        mode: RenderMode,
    ) -> Result<RenderProof>;
}

pub enum RenderProofArtifact {
    AppOwnedPixels {
        hash: String,
        width: u32,
        height: u32,
    },
    HostSurface {
        hash: String,
        width: u32,
        height: u32,
    },
    TextCells {
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

## Native GPU Renderer

### `boon_native_gpu`

Owns the native GPU renderer. It consumes `LayoutFrame`, not Boon runtime state.

Responsibilities:

- own `wgpu::Device`, `wgpu::Queue`, render pipelines, buffers, textures,
  readback textures, and `glyphon` text cache;
- render generic rectangles, borders, clips, text, grids, carets, selections,
  scrollbars, and debug overlays;
- apply incremental GPU uploads from layout diffs;
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
    pub lifecycle: SurfaceLifecycle,
    // private: Window, app_window::Surface, wgpu::Surface
}
```

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
- publish bounded snapshots and metrics to the dev role.

Dev role responsibilities:

- render editor, logs, inspectors, timeline, and controls in its own native
  window;
- use the same `boon_document` -> `LayoutFrame` -> `boon_native_gpu` path for
  dev UI, with host-generated generic document data;
- send source edits and control commands to the preview role;
- consume preview snapshots asynchronously;
- never block preview rendering.

Forbidden:

- rendering preview content through dev widgets;
- importing dev-only debug renderers into the preview role;
- injecting source events directly for headed/native E2E tests;
- making preview startup wait for the dev role.

## Input Contract

Native input flows through one route:

```text
app_window or wrapper event
  -> HostEvent / HostInputEvent
  -> Document hit/focus/scroll resolution
  -> SourceIntent and/or ViewportIntent
  -> SourceBatch only for application-bound source input
  -> boon_runtime dispatch
  -> DocumentPatch
  -> layout diff
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

## Role Protocol And Backpressure

Preview-to-dev telemetry uses bounded nonblocking queues. Preview frame
rendering must never wait for dev snapshot consumption, debug rendering, IPC
writes, or snapshot serialization.

API shape:

```rust
pub enum PreviewCommand {
    ReplaceCode { code: String, expected_hash: String },
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
    Snapshot(SnapshotEnvelope),
    DebugUpdate(DebugEnvelope),
    DebugQueryResult(DebugQueryResult),
    Metrics(FrameMetrics),
    Diagnostics(Vec<Diagnostic>),
    Disconnected(DisconnectReason),
}

pub struct SnapshotEnvelope {
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
    SelectedValues { ids: Vec<RuntimeValueId> },
}

pub enum DebugQuery {
    ValueSlice { id: RuntimeValueId, range: ValueRange, max_bytes: usize },
    DependencyNeighborhood { id: RuntimeValueId, depth: u8, max_nodes: usize },
    DocumentSlice { root: DocumentNodeId, range: ChildRange, max_bytes: usize },
}
```

Snapshot telemetry is coalesced by sequence number. Old snapshots may be
dropped. Commands are acknowledged separately. Source replacement commands,
debug subscriptions, and debug queries have explicit max payload sizes. Debug
updates are latest-value/coalesced by subscription. Large debug views must use
paged queries or sampled summaries instead of full-state mirroring. Reports must
include:

- `preview_blocked_on_ipc_count`;
- `ipc_queue_depth_p50_p95_max`;
- `snapshot_serialize_ms_p50_p95_max`;
- `dropped_snapshot_count`;
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
- Dev snapshots are best-effort and bounded. If the dev role falls behind, the
  preview drops old snapshots instead of waiting.
- Cells grid rendering uses visible ranges, overscan, instance buffers, and text
  cache reuse.
- Wheel scrolling must update scroll offsets and visible windows without runtime
  graph rebuilds or passive-scroll runtime dispatch.
- Code editor scrolling in the dev role must use the same virtualized list/text
  infrastructure, not a giant per-line widget tree.
- Release-mode frame reports must include p50/p95 frame time, upload bytes,
  draw calls, visible nodes, text runs shaped, and dropped debug snapshots.

### Scroll Hot Path

Cells body/header scrolling and dev code-editor scrolling must report:

- `runtime_dispatch_count_for_passive_scroll`;
- `graph_rebuild_count`;
- `wheel_events_coalesced`;
- `input_queue_depth_max`;
- `layout_rebuild_scope`;
- `newly_materialized_range_count`.

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

The architecture is not accepted until these gates exist and pass in release
mode on this Wayland machine.

```text
cargo xtask verify-platform-contract
cargo xtask verify-native-gpu-dependency-graph
cargo xtask verify-native-gpu-architecture
cargo xtask verify-native-gpu-layout-contract
cargo xtask verify-native-gpu-shaders --check
cargo xtask verify-native-gpu-multiwindow
cargo xtask verify-native-gpu-ipc-backpressure
cargo xtask verify-native-gpu-observability
cargo xtask verify-native-gpu-preview-e2e --example todomvc
cargo xtask verify-native-gpu-preview-e2e --example cells
cargo xtask verify-native-gpu-scroll-speed --example cells
cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor
cargo xtask verify-report-schema
cargo xtask audit-machine-readiness --report target/reports/debug/machine-readiness.json
cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
```

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
- `todomvc`, `todo_mvc`, `cells`, `pong`, or `arkanoid` branches in renderer or
  app_window crates;
- private runtime dispatch shortcuts in native E2E code;
- manual generated-WGSL loading in the renderer instead of generated
  `wgsl_bindgen` APIs;
- macroquad/miniquad/Ply dependencies in the new native GPU path;
- fallback screenshots used as pass/fail evidence.

`verify-native-gpu-layout-contract` must feed generic document fixtures plus
TodoMVC/Cells runtime outputs into `boon_document`, then assert deterministic
`LayoutFrame`, stable hit regions, accessibility/control semantics,
virtualization bounds, and no full 26x100 Cells widget expansion.

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
  texture hash, presented surface hash, and current git commit;
- the preview surface renders a nonblank frame before the dev window finishes
  rendering its first full debug frame;
- closing the dev window does not stop preview rendering;
- closing the preview window shuts down the preview role cleanly and causes the
  dev role to show disconnected state or exit cleanly;
- Xvfb/X11/headless/native-browser substitutes are rejected for this native
  Wayland gate.

`verify-native-gpu-ipc-backpressure` must stall or kill the dev role and prove
preview frame p95, memory cap, dropped snapshot count, IPC heartbeat behavior,
and continued preview rendering stay within budget.

`verify-native-gpu-observability` must enable a large runtime value graph and a
busy dev graph view, then prove debug subscriptions and queries are bounded,
paged, or sampled. It must fail if the dev role receives a continuous full copy
of the runtime heap, every app value, every list/table row, the full document
tree each frame, or full display-list/GPU instance streams. It must also prove
debug-update drops/coalescing are reported and preview frame timing remains
inside budget while the dev graph is overloaded.

`verify-native-gpu-preview-e2e` must drive visible native controls and assert
runtime state, source inventory, frame hashes, hit targets, source intent
routing, and timing. It must not pass by calling private source dispatch
functions directly.

`verify-native-gpu-scroll-speed --example cells` must fail unless it is a
release-mode Wayland report with real wheel input, current git/source hashes,
vertical and horizontal scroll evidence, `scroll_frame_ms_p95 <= 16.7`,
`wheel_to_visible_ms_p95 <= 50`, `preview_blocked_on_ipc_count = 0`,
`runtime_dispatch_count_for_passive_scroll = 0`, `graph_rebuild_count = 0`, and
no Xvfb/X11 or synthetic scroll-position evidence.

`verify-native-gpu-scroll-speed --surface dev-code-editor` must use a long
source file, real wheel input, release mode, line virtualization, text cache
metrics, and the same frame/latency thresholds as the Cells scroll gate.

## Migration Plan

1. Add the new core/native crates behind feature-free workspace members,
   without touching the current Ply playground behavior.
2. Keep current operator/human report schemas intact while the native GPU path
   is added; do not weaken existing manual-report requirements.
3. Implement `boon_document_model`, `boon_document`, `boon_text`, `boon_host`,
   and `boon_render_core` as pure portable contracts first.
4. Add `cargo xtask shaders` for this repo and wire WESL -> WGSL ->
   `wgsl_bindgen` into `boon_native_gpu`.
5. Build a one-process preview probe that renders generic document data through
   `app_window`/`wgpu`, with app-owned texture readback plus surface readback.
6. Build the desktop role with preview and dev as separate native child
   processes.
7. Connect preview role to the real Boon runtime and public source dispatch
   path.
8. Add virtualized Cells grid rendering and code editor scrolling.
9. Add the platform, dependency, architecture, layout, shader, multi-window,
   IPC, E2E, and scroll-speed gates.
10. Only after the new native GPU path passes, retire or demote the current
    macroquad/Ply path.

At no point should implementation make an example smaller, hardcode an example
renderer, accept a browser-backed native window, accept headless proof as native
Wayland proof, or weaken existing human/manual report schemas to make the new
path look complete.
