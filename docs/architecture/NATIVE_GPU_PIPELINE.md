# Native GPU Pipeline Architecture

This document describes the native-only GPU architecture for the Boon Circuit
playground and production preview. It is not a copy of the `boon-rust`
implementation. That repo is a useful reference for `app_window`, `wgpu`, WESL,
`wgsl_bindgen`, and `glyphon`, but this architecture must close the gaps that
reference still leaves open: no manually loaded shader shortcuts, no
example-specific renderers, no browser-backed native window, no headless-only
proof, and no dev UI that can slow down the production preview.

## Goals

1. Open a real native Wayland preview window and a real native dev/debug window.
2. Keep the preview path production-shaped: it can run without the dev window.
3. Keep renderer, runtime, input, windowing, and verification boundaries clear.
4. Render only generic Boon `document` output and generic styles/components.
5. Make Cells fast at full 7GUIs size through virtualization and GPU batching,
   not by making the example smaller.
6. Make verification prove real app-owned pixels, real app_window surfaces,
   real window lifecycle, and real input routing.

Browser support is out of scope for this document.

## Native Process Model

Use a role-based native executable:

```text
boon_native_playground --role preview --example cells
boon_native_playground --role dev --connect <preview-socket>
boon_native_playground --role desktop --example cells
```

`--role preview` is the production-shaped app. It owns the runtime, the preview
window, the preview frame loop, and the preview GPU device/queue. It must not
depend on any dev/debug widgets being loaded.

`--role dev` opens the dev/debug window. It connects to the preview role through
a bounded local IPC channel. It can request source replacement, run/reset/step,
and subscribe to snapshots, deltas, timings, and diagnostics.

`--role desktop` is only a launcher. It starts the preview role, waits for its
ready socket, then starts the dev role. The hard native gate is still two real
native windows because both roles create real `app_window` windows. The process
boundary is intentional: if the dev window is slow, the preview can keep
rendering and may drop debug snapshots.

The implementation may later add a same-process multi-window mode, but it must
not replace the production-shaped preview role or weaken the two-real-window
verification gate.

## Crate Boundaries

The architecture should be split along ownership boundaries, not convenience.

```text
boon_parser
boon_ir
boon_runtime
boon_document
boon_native_gpu
boon_native_app_window
boon_native_playground
xtask
```

### `boon_runtime`

Owns Boon execution only.

Responsibilities:

- parse/lower/execute Boon programs through the existing static graph path;
- accept typed `SourceBatch` input;
- emit deterministic turns containing document patches, state snapshots,
  diagnostics, and metrics;
- expose cause/explanation data for the dev window.

Forbidden:

- `wgpu`, `app_window`, `glyphon`, WESL, or native windowing dependencies;
- example-specific TodoMVC or Cells branches;
- renderer element IDs leaking into Boon values;
- UI layout decisions that depend on screen size, DPI, or GPU state.

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

### `boon_document`

Owns the renderer-neutral UI contract produced from Boon `document`.

Responsibilities:

- define stable document nodes, style values, source bindings, focus state,
  scroll state, clips, text runs, and list/grid virtualization metadata;
- apply `DocumentPatch` streams into a `DocumentFrame`;
- compute deterministic CPU layout from `DocumentFrame`, viewport size, and
  scale factor;
- produce hit regions and a render display list.

Forbidden:

- app-specific rendering branches;
- GPU objects or window objects;
- direct source dispatch;
- hidden fallback views when Boon did not produce the required structure.

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
    SetListWindow(DocumentNodeId, ListWindow),
    SetScroll(DocumentNodeId, ScrollState),
}

pub struct LayoutFrame {
    pub draw_items: Vec<DrawItem>,
    pub hit_regions: Vec<HitRegion>,
    pub scroll_regions: Vec<ScrollRegion>,
    pub metrics: LayoutMetrics,
}
```

Cells must be expressed as a grid/list viewport in this layer. The runtime may
own the 26x100 logical cells, but layout and rendering must work on visible
ranges plus overscan. Scrolling updates visible windows and GPU buffers; it must
not rebuild or redraw every logical cell as a separate heavyweight widget.

### `boon_native_gpu`

Owns the GPU renderer. It consumes `LayoutFrame`, not Boon runtime state.

Responsibilities:

- own the `wgpu::Device`, `wgpu::Queue`, render pipelines, buffers, textures,
  readback textures, and `glyphon` text cache;
- render generic rectangles, borders, clips, text, grids, carets, selections,
  scrollbars, and debug overlays;
- apply incremental GPU uploads from layout diffs;
- expose frame timing, upload size, draw counts, text cache stats, and readback
  proof;
- render into an app-owned frame texture first, then copy/present to the
  `app_window` surface.

Forbidden:

- Boon parser/runtime dependencies;
- TodoMVC, Cells, or example name branches;
- manual shader module creation from `.wgsl` strings outside generated binding
  wrappers;
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

API shape:

```rust
pub struct NativeGpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: PipelineSet,
    glyphon: GlyphonState,
}

impl NativeGpuRenderer {
    pub fn new(adapter_policy: AdapterPolicy) -> Result<Self>;
    pub fn configure_surface(&mut self, surface: &mut SurfaceSlot) -> Result<()>;
    pub fn upload_layout(&mut self, diff: LayoutDiff) -> Result<UploadStats>;
    pub fn render_preview(
        &mut self,
        target: &mut SurfaceSlot,
        frame: &LayoutFrame,
        mode: RenderMode,
    ) -> Result<FrameProof>;
}
```

### `boon_native_app_window`

Owns native windows, surfaces, and input collection through `app_window`.

Responsibilities:

- initialize `app_window::application::main`;
- create one or more native Wayland windows;
- create exactly one `app_window` surface per window;
- follow `app_window::WGPU_STRATEGY` and `WGPU_SURFACE_STRATEGY`;
- translate app_window keyboard, text, pointer, wheel, resize, focus, and close
  events into `NativeInputEvent`;
- keep a stable `WindowId` and `WindowRole` for each native window.

Forbidden:

- Boon example semantics;
- renderer pipelines;
- layout decisions;
- X11-only input assumptions;
- `xdotool` as required Wayland proof.

API shape:

```rust
pub enum WindowRole {
    Preview,
    Dev,
}

pub struct NativeWindowSpec {
    pub role: WindowRole,
    pub title: String,
    pub logical_size: LogicalSize,
}

pub struct NativeWindowHost {
    windows: SlotMap<WindowId, NativeWindow>,
}

impl NativeWindowHost {
    pub async fn create_window(&mut self, spec: NativeWindowSpec) -> Result<WindowId>;
    pub async fn next_event(&mut self) -> Result<NativeWindowEvent>;
    pub fn surface_slot(&mut self, id: WindowId) -> Option<&mut SurfaceSlot>;
}
```

### `boon_native_playground`

Owns orchestration only.

Preview role responsibilities:

- load source and scenarios;
- run `boon_runtime`;
- maintain `DocumentFrame`;
- layout the preview document for the preview viewport;
- route native input through hit regions into source batches;
- render preview frames on a fixed frame budget;
- publish bounded snapshots and metrics to the dev role.

Dev role responsibilities:

- render editor, logs, inspectors, timeline, and controls in its own native
  window;
- send source edits and control commands to the preview role;
- consume preview snapshots asynchronously;
- never block preview rendering.

Forbidden:

- rendering preview content through dev widgets;
- injecting source events directly for headed/native E2E tests;
- making preview startup wait for the dev role.

## Input Contract

Native input flows through one route:

```text
app_window input event
  -> NativeInputEvent
  -> Document hit region / focused node
  -> SourceBatch
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

Cells editing must support focus, formula bar text, raw formula/value display,
caret movement, commit/cancel, row/column scrolling, and dependent recalculation
through this same route.

## Performance Rules

Preview performance is a product requirement, not only a renderer detail.

- The preview role has its own event loop, runtime state, document state, GPU
  resources, and surface.
- Dev snapshots are best-effort and bounded. If the dev role falls behind, the
  preview drops old snapshots instead of waiting.
- Cells grid rendering uses visible ranges, overscan, instance buffers, and text
  cache reuse.
- Wheel scrolling must update scroll offsets and visible windows without a full
  runtime graph rebuild.
- Code editor scrolling in the dev role must use the same virtualized list/text
  infrastructure, not a giant per-line widget tree.
- Release-mode frame reports must include p50/p95 frame time, upload bytes,
  draw calls, visible nodes, text runs shaped, and dropped debug snapshots.

## Verification Gates

The architecture is not accepted until these gates exist and pass in release
mode on this Wayland machine.

```text
cargo xtask verify-native-gpu-architecture
cargo xtask verify-native-gpu-shaders
cargo xtask verify-native-gpu-multiwindow
cargo xtask verify-native-gpu-preview-e2e --example todomvc
cargo xtask verify-native-gpu-preview-e2e --example cells
cargo xtask verify-native-gpu-scroll-speed --example cells
cargo xtask audit-goal-readiness --report target/reports/goal-readiness.json
```

`verify-native-gpu-architecture` must fail on:

- `wgpu`, `app_window`, or `glyphon` dependencies in runtime/parser/IR crates;
- `boon_runtime` or example crate dependencies in `boon_native_gpu`;
- `todomvc`, `todo_mvc`, `cells`, `pong`, or `arkanoid` branches in renderer or
  app_window crates;
- manual generated-WGSL loading in the renderer instead of generated
  `wgsl_bindgen` APIs;
- macroquad/miniquad/Ply dependencies in the new native GPU path;
- fallback screenshots used as pass/fail evidence.

`verify-native-gpu-multiwindow` must launch the desktop role and prove:

- two real native `app_window` windows exist: preview and dev;
- each window has an independent app_window surface;
- the preview surface renders a nonblank frame before the dev window finishes
  rendering its first full debug frame;
- closing the dev window does not stop preview rendering;
- closing the preview window shuts down the preview role cleanly and causes the
  dev role to show disconnected state or exit cleanly;
- both windows record fresh frame hashes and surface-size proofs for the current
  checkout.

`verify-native-gpu-preview-e2e` must drive visible native controls and assert
runtime state, source inventory, frame hashes, hit targets, and timing. It must
not pass by calling private source dispatch functions directly.

`verify-native-gpu-scroll-speed --example cells` must prove vertical and
horizontal wheel scrolling with current full Cells size. It must record the input
backend, focused window proof, scroll deltas, visible row/column changes, frame
timings, and whether any debug snapshots were dropped.

## Migration Plan

1. Add the new crates behind feature-free workspace members, without touching
   the current Ply playground behavior.
2. Implement `boon_document` as a pure data/layout layer over current runtime
   document output.
3. Add `cargo xtask shaders` for this repo and wire WESL -> WGSL ->
   `wgsl_bindgen` into `boon_native_gpu`.
4. Build a static `app_window`/`wgpu` one-window probe that renders generic
   document data and proves app-owned texture readback plus surface readback.
5. Build the desktop role with preview and dev windows as separate native roles.
6. Connect preview role to the real Boon runtime and source dispatch path.
7. Add virtualized Cells grid rendering and code editor scrolling.
8. Add the architecture, shader, multi-window, E2E, and scroll-speed gates.
9. Only after the new native GPU path passes, retire or demote the current
   macroquad/Ply path.

At no point should implementation make an example smaller, hardcode an example
renderer, accept a browser-backed native window, or weaken existing human/manual
report schemas to make the new path look complete.
