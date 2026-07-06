# Boon Unified Runtime, Retained Rendering, 3D, and Manufacturing Architecture Plan

**Status:** Proposed implementation contract  
**Repository:** `BoonLang/boon-circuit`  
**Planning baseline inspected:** `95f86d265de7585ee1bc6d04cddf356d6cc16ae3` (`main`, 2026-06-22)  
**Intended repository path:** `docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md`  
**Companion execution prompt:** `docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md`

> The commit above is a planning snapshot, not a pin. Every implementation pass must inspect the actual current `HEAD`, preserve user work, and update path/symbol assumptions without reducing the contract.

## Implementation Checkpoints

- 2026-07-06: Removed the old `verify-native-gpu-scroll-speed --surface dev-code-editor`
  compatibility alias and its report-conversion helpers. The generic native GPU
  scroll-speed gate is now an example preview gate only; dev-editor scrolling is
  owned by the dedicated `verify-native-dev-editor-scroll-speed --profile
  debug|release` verifier. The old native GPU dev-code-editor scroll report
  label stays out of the native handoff manifest and must fail closed if invoked.

---

## 1. Purpose

This document unifies the unfinished BYTES/MachinePlan migration, the runtime-finality work, the document/UI migration, the native GPU work, the demand-driven render-loop plan, and the new 3D/manufacturing direction into one architecture.

The central goal is simple:

> A Boon program is compiled once into a typed static plan. Runtime changes retain stable identity and stay incremental through document state, layout, text, rendering, GPU memory, accessibility, 3D world state, and manufacturing outputs.

The target system has one semantic source of truth and several purpose-built compiled products:

```text
Boon source
    │
    ▼
Parser AST → typed IR → MachinePlan
    │
    ▼
PlanExecutor / runtime memories
    │
    ├── UI semantic changes ────────────┐
    ├── world/assembly changes ──────┐  │
    ├── accessibility changes ────┐  │  │
    └── manufacturing parameters  │  │  │
                                   │  │  │
                 ┌─────────────────┘  │  └──────────────────────┐
                 ▼                    ▼                         ▼
          SemanticScene          WorldScene              Retained UiTree
                 │                    │                         │
      ┌──────────┴─────────┐          │                         ▼
      ▼                    ▼          │                 Retained LayoutTree
native AccessKit     web semantic     │                         │
                    bridge only       │                         ▼
                                      └───────────────→ Retained RenderScene
                                                                │
                                                                ▼
                                                        shared boon_wgpu
                                                  native surface / web canvas

Authoritative SolidGraph / AssemblyGraph
    ├── visual surface compiler → retained mesh or FDG-D-like cache → WGPU
    └── manufacturing compiler → certified 2D material regions → 3MF/toolpaths
```

This is a hybrid **pipeline**, not a hybrid source model. There must not be two editable geometry truths that can drift apart.

---

## 2. Relationship to existing plans

This plan extends rather than replaces the useful parts of:

- `docs/plans/BYTES_AND_MACHINE_PLAN_IMPLEMENTATION.md`
- `docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md`
- `docs/plans/RUNTIME_FINALITY_HONESTY_PLAN.md`
- `docs/plans/REMOVE_VIEW_DOCUMENT_UI_GOAL.md`
- `docs/plans/NATIVE_DEMAND_DRIVEN_RENDER_LOOP_PLAN.md`
- `docs/plans/GOAL_PROMPT.md`
- `docs/architecture/RUNTIME_MODEL.md`
- `docs/architecture/LIST_MODEL.md`
- `docs/architecture/DELTA_PROTOCOL.md`
- `docs/architecture/NATIVE_GPU_PIPELINE.md`

When documents conflict, use this priority order:

1. `AGENTS.md` and explicit current user instructions.
2. Honesty, provenance, and non-fabrication requirements in existing audits and report schemas.
3. This unified architecture for runtime-to-rendering, browser/native parity, 3D, and manufacturing.
4. Existing specialized plans for details not contradicted here.
5. Current implementation behavior only as a migration constraint, not as the desired architecture.

The active BYTES/MachinePlan progress ledger remains authoritative for that migration. Do not restart it, erase its history, or create a competing ledger for the same tasks. The unified progress ledger should reference its phase/task IDs.

---

## 3. Non-negotiable decisions

### 3.1 One runtime architecture

- The normal path is parser AST → typed IR → `MachinePlan` → `PlanExecutor`.
- The legacy path may remain temporarily for differential verification, never as a hidden fallback in the final default path.
- No TodoMVC-, Cells-, file-name-, path-, source-text-, or example-specific runtime behavior.
- Strings remain labels/debug metadata, not hot-path identity or dispatch keys.
- LIST items remain keyed memories with generations, not cloned runtime graphs.

### 3.2 One visual renderer for native and browser

- Use a custom, platform-neutral WGPU renderer.
- Use the same retained render model, shader sources, buffer layouts, material model, clipping, text system, picking, readback logic, and frame graph on native and browser.
- Native and browser hosts differ only where operating-system/browser integration requires it.
- Do not adopt Bevy or another application/game engine as the renderer architecture.
- Do not create a classic HTML/CSS visual implementation for the web version.

### 3.3 Accessibility is semantic, not inferred from pixels

- Boon owns a platform-neutral `SemanticScene` with stable IDs, roles, labels, values, states, relationships, bounds, and actions.
- Native adapters lower it to AccessKit/platform accessibility APIs.
- The browser uses the smallest practical DOM/ARIA/text-input bridge. That DOM is not the visual renderer and must not mirror GPU primitives.
- HTML-in-canvas may later be an optional browser enhancement, never a required correctness dependency.

### 3.4 Renderer-owned framebuffer and readback

- The renderer normally draws into app-owned color, depth, and pick-ID textures.
- Presentation is a final copy/composite to the native surface or browser canvas.
- Screenshots, regression captures, object/feature picking, thumbnails, selection outlines, and color sampling remain under Boon’s control.

### 3.5 One authoritative 3D semantic model

- A typed constructive `SolidGraph`/`AssemblyGraph` is authoritative for printable solids.
- It is a bounded functional/region representation; nodes need not preserve true signed-distance magnitudes.
- Triangle meshes, FDG-D-like directed dual grids, voxels, SDFs, UDFs, or B-Reps may be import/export/cache/specialized-node representations, never the universal source of truth.
- GPU geometry is disposable and rebuildable.
- Printing never silently consumes the visual mesh.

### 3.6 Rendering and printing are separate compilers

```text
SolidGraph
    ├── visual compiler: fast, adaptive, screen-error driven
    └── manufacturing compiler: deterministic, tolerance driven, material aware
```

Visual accuracy and manufacturing accuracy must never be conflated.

### 3.7 Honest completion

- Existing report-schema and evidence provenance rules remain binding.
- Headless output is not headed Wayland proof.
- Synthetic input is not human/OS input proof unless a gate explicitly permits it.
- A visual mesh that looks closed is not proof of a valid printable material region.
- “Exact printing” means deterministic, closed, tolerance-bounded manufacturing output—not exact real-number representation of arbitrary transcendental geometry.

---

## 4. Current architecture and primary bottlenecks

The current runtime direction is good: static plans, keyed memories, semantic deltas, typed routes, and no runtime graph cloning. The rendering side does not yet preserve those benefits end to end.

2026-07-03 update: the headed Cells visible-click release gate now passes on
hardware (`8117094`, `target/reports/native-gpu/cells-visible-click-e2e-release.json`;
product p95 `9.903322ms`, max `10.352470ms`, zero missed frames, exact
app-owned WGPU proof). This proves the current retained product/proof split can
hit 60 FPS-class click latency for Cells. It does not make the renderer final:
the next native WGPU architecture slice must still implement and measure a
generic `ProductRenderGraph` / `PresentPlan` path, and keep it only if it
improves performance or removes/quarantines legacy hot-path coupling without
budget regression.

2026-07-06 checkpoint: the manifest-backed native handoff aggregate is fresh
and passing after the `PlanExecutorListStore` cleanup and a verifier contract
fix (`target/reports/native-gpu-all.json`, `status=pass`,
`refresh_debt_child_count=0`, `true_blocker_child_count=0`). The verifier cut
removed a false geometry contract from visible-readback proof: window usability
is now checked against the app-reported surface `physical_size`, while WGPU
readback remains the app-owned nonblank/frame-identity proof. This avoids
treating COSMIC/tiled readback rectangles as the UI layout size, without adding
example-specific renderer/runtime behavior.

Current Cells evidence is also fresh:
`target/reports/native-gpu/cells-visible-click-e2e-release.json` passes with
product-path input-to-present p95 `9.435423ms`, `64` product samples, and zero
product missed frames. The older broad `click_to_formula_visible_ms_p95`
remains reported separately because it includes external driver/app-wake/proof
timing; it is not the product UX budget. The authoritative fast path for this
checkpoint is `MachinePlan`/`PlanExecutor`, `PlanExecutorListStore`, retained
native GPU rendering, and separated proof/readback reporting. Remaining work is
the larger architecture cleanup: continue deleting legacy runtime/harness
ambiguity, make `ProductRenderGraph` / `PresentPlan` the normal product
boundary, and avoid reintroducing fallback proof or example-specific hot paths.

### 4.1 Snapshot collapse after semantic deltas

The current effective path is approximately:

```text
semantic deltas
    ↓
DocumentPatch
    ↓
mutable DocumentFrame
    ↓
full integrity validation
    ↓
full LayoutFrame snapshot
    ↓
render-scene reconstruction/lowering
    ↓
native-GPU-specific retained comparison
    ↓
GPU writes
```

A small interaction can therefore trigger broad tree walking, allocation, cloning, style copying, layout reconstruction, render lowering, hashing, metrics construction, and upload work.

### 4.2 Specific current hot-path issues

At the planning baseline:

- `DocumentNodeId`, `SourceBindingId`, and `ScrollRootId` are string-backed.
- `StyleMap` is `BTreeMap<String, StyleValue>`.
- `DocumentNode` owns a full style map and only one optional source binding.
- `DisplayItem` clones text and a complete style map.
- `DocumentState::apply_patch` performs full integrity validation before and after each patch.
- Layout validates the document again.
- `SetText` uses broad invalidation rather than measured dirty facts.
- `SetListMaterialization` appends ranges, allowing historical ranges to accumulate.
- Typed rich-text/editor style variants serialize through strings in a way that cannot round-trip as the same variant.
- Document rendering and native GPU define overlapping render-scene representations and several request entry points.
- Retained GPU ranges are managed through behavior that can invalidate broad caches on ring growth/wrap.
- Text shape keys mix shaping, wrapping, placement, and paint concerns.
- `boon_native_gpu`, `boon_document`, and native host files have become large enough that ownership boundaries are difficult to see.
- The native-only architecture document no longer expresses the desired shared native/browser renderer.

### 4.3 Active runtime half-migration

The BYTES/MachinePlan work is materially advanced but incomplete. The current ledger reports completed Plan Boundary and parser phases, partial downstream phases, a missing default switch, unresolved release benchmark/speed-budget work, and remaining typed BYTES operations. The unified architecture depends on completing that migration rather than building rendering around legacy runtime assumptions.

---

## 5. Target layers and crate responsibilities

Prefer stabilizing module boundaries before proliferating public crates. Extract crates when contracts are exercised and no longer churn rapidly.

### 5.1 Compiler and runtime

```text
boon_parser
    real AST, spans, stable ExprId

boon_typecheck / boon_ir
    typed nodes, scopes, source schemas, lists, functions, UI/world/solid constructors

boon_plan
    immutable MachinePlan tables and dense route indexes

boon_runtime
    PlanExecutor, typed columnar memories, deterministic ticks, change emission
```

The runtime owns Boon execution. It does not own windows, GPU objects, report enrichment, screenshots, or platform input proof.

### 5.2 Semantic application model

Initially these may be modules; later they may become crates:

```text
boon_document_model
    portable serialized UI/document contract

boon_document
    hot retained UI tree, patch application, incremental layout, hit testing

boon_scene_model
    WorldScene, WorldPatch, cameras, lights, instances, appearance resources

boon_semantics
    SemanticScene, SemanticPatch, focus/actions, adapter-neutral roles

boon_geometry_ir
    profiles, curves, SolidGraph, AssemblyGraph, units, feature/material identity
```

### 5.3 Rendering

```text
boon_render
    canonical retained RenderScene, RenderPatch, primitives, materials, clips, text refs

boon_wgpu
    platform-neutral WGPU implementation, targets, pipelines, arenas, picking, readback

boon_host_native
    window/surface lifecycle, native events, IME, clipboard, AccessKit adapter

boon_host_web
    canvas/surface lifecycle, browser events, IME bridge, semantic DOM/ARIA adapter
```

`boon_native_gpu` can migrate toward `boon_wgpu`; a rename is not required before its API is platform-neutral.

### 5.4 3D and manufacturing

```text
boon_geometry_compile
    bounds, dependency tracking, adaptive visual surface chunks, optional FDG-D cache

boon_manufacturing
    validation, direct sections, material regions, offsets, toolpath-ready layers

boon_3mf
    3MF object/material/slice serialization and import/export diagnostics
```

---

## 6. Identity and revision model

Stable identity is the backbone of the architecture. Content hashes are useful cache keys but are not user/runtime identity.

### 6.1 Required identities

```rust
ProgramRevision
ExprId
ScopeId
SourceId
StateId
ListId
ListKey
ListGeneration

UiNodeId
LayoutNodeId
RenderNodeId
SemanticId
SourceBindingId

GeometryLogicalId
GeometryRevision
FeatureId
PartId
InstanceId
AppearanceMaterialId
PhysicalMaterialId
RegionId
SurfaceChunkId
PickId
```

### 6.2 Stable identity versus revision

- **Logical identity** survives value and parameter changes.
- **Revision** changes when dependent data changes.
- **Generation** disambiguates removal and reinsertion of a keyed/list item.
- **Cache key** may include content hashes, tolerances, device feature profile, LOD, and compiler version.

Example:

```text
wheel geometry logical ID: WheelPrototype
wheel geometry revision: 41 → 42 after radius changes
wheel instances: FrontLeft, FrontRight, RearLeft, RearRight remain stable
```

Changing the wheel radius must not destroy selection/focus identity for each wheel instance.

### 6.3 Derivation

A UI node identity should be derivable from stable compiler/runtime structure:

```text
UiNodeId = ProgramRevision-compatible stable expression identity
         + ScopeId
         + optional ListKey/ListGeneration
         + output/child slot
```

A surface chunk identity should be:

```text
SurfaceChunkId = GeometryLogicalId + spatial key + LOD/tolerance class
```

Its `GeometryRevision` is separate.

### 6.4 Prohibited identity sources

Do not derive logical identity solely from:

- current text;
- current color/material;
- array/list position;
- GPU buffer offset;
- tessellated vertex contents;
- full parameter hashes;
- source path or example name.

---

## 7. Complete the MachinePlan/runtime migration first

The rendering work must consume the new runtime contracts rather than deepen legacy coupling.

### 7.1 Required completion of active BYTES/MachinePlan work

Continue the existing ledger and complete all partial/not-started phases, including:

- remaining typed BYTES row/function operations;
- fixed/dynamic BYTES behavior and diagnostics across parser/typecheck/IR/plan/runtime;
- full MachinePlan lowering coverage;
- typed columnar runtime storage and dense routes;
- PlanExecutor parity across TodoMVC, full Cells, BYTES cases, and negative cases;
- release-mode Cells benchmark and the unresolved speed-budget task(s), including the current `TASK-0804A` family;
- default `boon_cli run` switch only after parity and performance gates pass;
- explicit legacy-path selection only for differential/debug use;
- retirement of legacy runtime code only after default-path soak and no hidden fallback.

### 7.2 Runtime output contract

The executor should emit typed change sets, not complete application snapshots:

```rust
pub struct RuntimeChangeSet {
    pub tick: TickId,
    pub program_revision: ProgramRevision,
    pub ui: Vec<UiSemanticChange>,
    pub world: Vec<WorldSemanticChange>,
    pub semantics: Vec<SemanticChange>,
    pub geometry: Vec<GeometryParameterChange>,
    pub diagnostics: Vec<RuntimeDiagnostic>,
}
```

A route table maps a changed runtime slot to affected lowered properties/nodes. Unaffected scenes receive no work.

### 7.3 Typed application outputs; no special `VIEW` path

The compiler must lower normal typed top-level values into explicit MachinePlan output ports. Rendering must not depend on a special `VIEW` syntax, source-text search, function-name convention, or example path.

Supported application outputs should be represented as typed ports such as:

```rust
pub struct ApplicationOutputs {
    pub document: Option<PlanValueId>,
    pub app_scene: Option<PlanValueId>,
    pub world: Option<PlanValueId>,
    pub semantics: Option<PlanValueId>,
    pub manufacturing: Option<PlanValueId>,
}
```

Source-level names may initially be conventional (`document`, `scene`, `app`) for compatibility, but the parser/typechecker/IR must resolve them structurally and type them. The final lowering must not scan raw expression text. Existing `VIEW` compatibility may be handled by an explicit parser/IR migration adapter with tests and a removal task.

The preferred integrated output is:

```text
app: App/new(
    ui: ...
    world: ...
    manufacturing: ...
)
```

A UI-only program can continue to expose a regular `document`/scene value without constructing a 3D world.

### 7.4 Tick and commit ordering

Preserve deterministic phases:

```text
1. accept source/input events
2. compute dirty runtime routes
3. evaluate scheduled plan nodes
4. commit state/list memories
5. emit canonical semantic changes
6. lower semantic changes into subsystem patches
7. enqueue one render/layout wake if needed
```

No subsystem observes half-committed state.

---

## 8. Typed change sets from runtime to application models

### 8.1 Transactional batches

Every subsystem change batch carries an epoch/tick and is applied transactionally.

```rust
pub struct ChangeBatch<T> {
    pub epoch: u64,
    pub changes: Vec<T>,
}
```

Required properties:

- deterministic order;
- no duplicate contradictory writes without explicit last-write semantics;
- stable node IDs;
- validation at changed boundaries;
- complete rollback or no partial publication on failure;
- changed-root and dirty-fact reporting.

### 8.2 UI changes

Use structural and property-specific changes rather than broad upserts:

```rust
pub enum UiSemanticChange {
    InsertNode { id: UiNodeId, parent: UiNodeId, index: u32, node: UiNodeInit },
    RemoveSubtree { id: UiNodeId },
    MoveNode { id: UiNodeId, parent: UiNodeId, index: u32 },
    SetText { id: UiNodeId, text: TextContentId },
    SetLayoutStyle { id: UiNodeId, style: LayoutStyleId },
    SetPaintStyle { id: UiNodeId, style: PaintStyleId },
    SetTextStyle { id: UiNodeId, style: TextStyleId },
    SetMaterial { id: UiNodeId, material: AppearanceMaterialId },
    SetBindings { id: UiNodeId, bindings: BindingSetId },
    SetVisibility { id: UiNodeId, visible: bool },
    SetScroll { id: UiNodeId, offset: Vec2 },
    SetListWindow { id: UiNodeId, window: MaterializedWindow },
}
```

### 8.3 World changes

```rust
pub enum WorldPatch {
    CreateInstance(ModelInstance),
    RemoveInstance(InstanceId),
    SetTransform { instance: InstanceId, transform: Transform3D },
    SetGeometryRevision { geometry: GeometryLogicalId, revision: GeometryRevision },
    SetAppearance { instance: InstanceId, appearance: AppearanceBinding },
    SetVisibility { instance: InstanceId, visible: bool },
    SetCamera { camera: CameraId, value: Camera },
    SetLight { light: LightId, value: Light },
}
```

### 8.4 Semantic/accessibility changes

```rust
pub enum SemanticPatch {
    InsertNode { id: SemanticId, parent: SemanticId, index: u32, node: SemanticNode },
    RemoveSubtree(SemanticId),
    SetName { id: SemanticId, name: TextContentId },
    SetValue { id: SemanticId, value: SemanticValue },
    SetState { id: SemanticId, state: SemanticState },
    SetBounds { id: SemanticId, bounds: Option<Rect> },
    SetFocus { id: Option<SemanticId> },
    Announce { politeness: LivePoliteness, message: TextContentId },
}
```

---

## 9. Document model: serialized form and hot retained form

The readable/serializable document contract and the performance-oriented retained model should not be the same physical data structure.

### 9.1 Portable serialized form

Keep a stable protocol suitable for reports, replay, fixtures, IPC where appropriate, and debugging. It may use human-readable IDs and tagged values.

Fix typed style serialization:

```rust
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum StyleValue {
    Text(String),
    Number(f64),
    Bool(bool),
    RichTextSpans(Vec<StyleRichTextSpan>),
    EditorTypeHints(Vec<StyleEditorTypeHint>),
}
```

Provide explicit legacy decoding only at the boundary.

### 9.2 Hot retained form

```rust
pub struct HotDocument {
    nodes: SlotMap<UiNodeKey, HotUiNode>,
    styles: StyleStores,
    texts: TextStore,
    bindings: BindingStore,
    dirty_roots: DirtyRootSet,
}

pub struct HotUiNode {
    parent: Option<UiNodeKey>,
    children: SmallVec<[UiNodeKey; 4]>,

    layout_style: LayoutStyleId,
    paint_style: PaintStyleId,
    text_style: TextStyleId,
    material: AppearanceMaterialId,
    interaction_style: InteractionStyleId,
    semantic_style: SemanticStyleId,

    text: Option<TextContentId>,
    bindings: SmallVec<[InteractionBinding; 4]>,
    flags: UiNodeFlags,
}
```

### 9.3 Style partitioning

Compile dynamic source properties into typed style records:

```rust
pub struct ComputedStyle {
    pub layout: LayoutStyleId,
    pub paint: PaintStyleId,
    pub text: TextStyleId,
    pub material: AppearanceMaterialId,
    pub interaction: InteractionStyleId,
    pub semantics: SemanticStyleId,
}
```

Unknown/experimental properties may remain in a cold extension map. They must not force hot-path string lookup for known properties.

### 9.4 Multiple interactions per node

Replace one optional binding with a small typed set:

```rust
pub struct InteractionBinding {
    pub intent: InteractionIntent,
    pub source_id: SourceId,
    pub scope: ScopeId,
    pub row_key: Option<ListKey>,
    pub row_generation: Option<ListGeneration>,
    pub bind_epoch: u64,
}
```

### 9.5 Batch application and validation

Replace repeated single-patch full-tree validation with:

```rust
pub fn apply_batch(
    &mut self,
    batch: &ChangeBatch<UiSemanticChange>,
) -> Result<DocumentChangeSet, PatchApplyError>;
```

Normal validation checks only:

- changed node existence/generation;
- affected parent/child consistency;
- insertion/move cycle safety;
- changed binding/source validity;
- changed materialization window validity;
- changed resource references.

Retain full validation behind tests, debug assertions, proof builds, or an explicit expensive-invariants feature.

---

## 10. Retained incremental layout

### 10.1 Layout state

```rust
pub struct LayoutNode {
    pub parent: Option<LayoutNodeId>,
    pub children: SmallVec<[LayoutNodeId; 4]>,
    pub constraints: Constraints,
    pub intrinsic_size: Size,
    pub bounds: Rect,
    pub clip: Option<Rect>,
    pub scroll_transform: Vec2,
    pub dirty: LayoutDirtyFlags,
    pub revision: u64,
}
```

### 10.2 Dirty facts, not broad invalidation names

Examples:

```text
color/material change:
    paint dirty only

text content change:
    shape dirty
    line layout dirty
    intrinsic size dirty only if measured size changed
    ancestor layout propagation stops when output size is unchanged

padding change:
    local layout dirty
    propagate only while bounds change

scroll offset:
    transform/clip/visibility dirty
    no runtime graph rebuild
    no text reshaping

hover:
    paint/material/possibly depth transform only
```

### 10.3 Layout boundaries

Introduce explicit layout containment/boundary flags so reflow can stop at stable containers. A changed child should not make unrelated roots dirty.

### 10.4 Virtualized lists

`MaterializedWindow` is current state, not append-only history:

```rust
pub struct MaterializedWindow {
    pub axis: Axis,
    pub first_key: Option<ListKey>,
    pub last_key: Option<ListKey>,
    pub overscan_before: u32,
    pub overscan_after: u32,
    pub revision: u64,
}
```

Long-running scroll must reach a memory plateau. Removed/offscreen rows retain semantic identity in runtime memories but need not retain full layout/render allocations.

### 10.5 Layout patches

The layout engine updates a retained render-facing state:

```rust
pub enum LayoutPatch {
    InsertFragment { id: LayoutNodeId, parent: LayoutNodeId, index: u32, fragment: LayoutFragment },
    RemoveSubtree(LayoutNodeId),
    SetBounds { id: LayoutNodeId, bounds: Rect },
    SetClip { id: LayoutNodeId, clip: Option<Rect> },
    SetTransform { id: LayoutNodeId, transform: Transform2D },
    SetVisibility { id: LayoutNodeId, visible: bool },
}
```

Full `LayoutFrame` snapshots remain available for initial build, recovery, reports, and debugging, not as mandatory per-interaction transport.

---

## 11. Shared retained text system

Layout and rendering must share one text service so measurement and rendered glyph selection cannot diverge.

### 11.1 Split cache layers

```text
TextContentId
    ↓
ShapeKey → ShapeId
    ↓
LineLayoutKey(width/wrap) → LineLayoutId
    ↓
Placement(position/clip/transform)
    ↓
Paint(color/material/effects)
```

A shaping key contains only properties that affect glyph selection/shaping:

```rust
pub struct ShapeKey {
    pub text: TextContentId,
    pub font_face: FontFaceId,
    pub font_size: OrderedF32,
    pub features: FontFeatureSetId,
    pub language: LanguageId,
    pub direction: TextDirection,
}
```

Width belongs to line layout. Position, rotation, and clip belong to placement. Color belongs to paint.

### 11.2 Retained glyph/atlas behavior

- Keep glyph atlas resources stable across frames.
- Upload only newly rasterized glyphs.
- Track atlas generation independently from text-run identity.
- Coalesce text buffer writes.
- A color-only text change must not reshape or rerasterize glyphs.
- A scroll-only change must update placement/clip, not text content/layout.

### 11.3 Browser/native parity

Use the same shaping/layout/rendering path on native and browser. A hidden/semantic browser input may receive IME and selection events, but it does not become the visual text renderer.

---

## 12. One canonical retained render model

Eliminate overlapping document and native-GPU render-scene representations.

### 12.1 Portable primitives

```rust
pub enum Primitive {
    UiBox(UiBoxPrimitive),
    Text(TextPrimitive),
    Image(ImagePrimitive),
    Path(PathPrimitive),
    Mesh(MeshPrimitive),
}
```

Every primitive references stable resources:

```rust
pub struct RenderItem {
    pub id: RenderNodeId,
    pub primitive: Primitive,
    pub transform: TransformId,
    pub clip: ClipId,
    pub material: AppearanceMaterialId,
    pub pick_id: PickId,
    pub z_order: ZOrder,
    pub visibility: Visibility,
}
```

### 12.2 Render patches

```rust
pub enum RenderPatch {
    Insert { id: RenderNodeId, parent: RenderNodeId, index: u32, item: RenderItem },
    RemoveSubtree(RenderNodeId),
    SetTransform { id: RenderNodeId, transform: TransformId },
    SetClip { id: RenderNodeId, clip: ClipId },
    SetMaterial { id: RenderNodeId, material: AppearanceMaterialId },
    SetGeometry { id: RenderNodeId, geometry: GeometryHandle },
    SetTextRun { id: RenderNodeId, run: TextRunId },
    SetVisibility { id: RenderNodeId, visible: bool },
    SetPickId { id: RenderNodeId, pick_id: PickId },
}
```

### 12.3 Physical UI specialization

`todomvc_physical` should use compact instanced primitives, not arbitrary generated meshes or FDG-D.

```rust
#[repr(C)]
pub struct UiBoxInstance {
    pub rect: [f32; 4],
    pub z: f32,
    pub depth: f32,
    pub corner_radii: [f32; 4],
    pub border_width: f32,
    pub material_id: u32,
    pub clip_id: u32,
    pub pick_id: u32,
    pub flags: u32,
}
```

A shared unit box/rounded-box pipeline handles panels, rows, buttons, checkboxes, and depth. Text and icons remain separate retained runs/paths.

### 12.4 Example delta behavior

```text
Todo title edit:
    Runtime SetText
    → reshape one text run
    → remeasure one line
    → reflow bounded ancestors only if width/height changed
    → update one text instance/range

Hover delete button:
    SetMaterial/flags only
    → no layout
    → no text shaping
    → one small GPU write

Move one keyed todo row:
    MoveNode / transform/order patches
    → unchanged rows keep IDs and GPU allocations
```

---

## 13. Platform-neutral WGPU renderer

### 13.1 Renderer ownership

`boon_wgpu` owns:

- adapter/device/queue configuration abstraction;
- pipelines and shader modules;
- material, transform, clip, text, instance, vertex, and index resources;
- app-owned frame targets;
- picking and readback requests;
- GPU resource lifetime and device-loss recovery;
- retained scene cache;
- render metrics and optional proof capture.

Hosts own:

- window/canvas creation;
- WGPU surface lifecycle;
- OS/browser events;
- IME and clipboard bridge;
- platform accessibility adapter;
- wake integration.

### 13.2 Frame targets

```rust
pub struct FrameTargets {
    pub scene_color: wgpu::Texture,
    pub depth: wgpu::Texture,
    pub pick_ids: wgpu::Texture,
    pub optional_normals: Option<wgpu::Texture>,
    pub optional_feature_ids: Option<wgpu::Texture>,
}
```

Render flow:

```text
prepare dirty resources
    ↓
scene depth/opaque pass
    ↓
transparent/world effects
    ↓
UI/text pass
    ↓
selection/focus/outline composite
    ↓
final tone/composite
    ↓
present to surface/canvas
```

### 13.3 Persistent GPU arenas

Use persistent arenas for retained data:

```text
persistent:
    UI instances
    mesh vertices/indices
    transforms
    materials
    clips
    text-run metadata

transient ring:
    per-frame uniforms
    short-lived staging/compute scratch
```

```rust
pub struct GpuAllocation {
    pub slot: u32,
    pub generation: u32,
    pub offset: u64,
    pub size: u64,
}
```

Requirements:

- ring wrap never invalidates retained geometry;
- changed ranges are coalesced into a small number of writes/copies;
- arena growth preserves logical allocations or relocates them through an explicit patch/update table;
- compaction is threshold driven and measurable;
- no-op frames upload no retained geometry;
- device loss discards GPU caches and replays current retained scenes.

### 13.4 Picking and readback

- Use a compact integer pick attachment.
- Read one pixel/small region only on click or deliberate hover pause.
- Map `PickId` through a CPU table to UI/world/part/feature/source identity.
- Screenshot/readback is asynchronous and requested, never a mandatory per-frame copy.
- Tests may render headlessly to owned textures, but headless evidence is not substituted for headed-platform proof.

### 13.5 Shader organization

- Keep one WGSL/WESL source pipeline for native and web.
- Continue generated binding verification.
- Shader contracts must be proven from actual compilation/reflection, not marker strings.
- Keep target-specific feature profiles small; do not enable native backends in WASM.
- Track compressed WASM size, startup time, shader/pipeline creation time, and first useful frame.

---

## 14. Demand-driven frame scheduling

The render loop must not continuously acquire/present at ~60 Hz while idle.

### 14.1 Dirty reasons

```rust
bitflags! {
    pub struct FrameDirty: u32 {
        const SCENE_PATCH      = 1 << 0;
        const LAYOUT_PATCH     = 1 << 1;
        const TEXT_ATLAS       = 1 << 2;
        const CAMERA           = 1 << 3;
        const ANIMATION        = 1 << 4;
        const POINTER_FEEDBACK = 1 << 5;
        const READBACK         = 1 << 6;
        const SURFACE_RESIZE   = 1 << 7;
        const EXPOSE           = 1 << 8;
    }
}
```

### 14.2 Scheduler behavior

```text
poll input/runtime/IPC
    ↓
apply batches
    ↓
if no dirty reason and no deadline:
    block/sleep through platform event loop
else:
    prepare only dirty resources
    acquire surface only when a frame will be presented
    render/present
```

Animations register the next deadline. Input, runtime, IPC, and worker completions can wake the host through a narrow `WakeHandle`.

### 14.3 Required proof

- idle window does not continuously acquire/present;
- input remains responsive while idle;
- telemetry/dev overload does not block preview rendering;
- readback requests trigger exactly the necessary frame/work;
- scroll does not dispatch through the Boon runtime unless the program semantically observes scroll.

---

## 15. Native and browser hosts

### 15.1 Shared rendering contract

Both hosts consume the same `RenderPatch` stream and call the same renderer methods:

```rust
pub trait PresentSurface {
    fn size(&self) -> PhysicalSize;
    fn format(&self) -> wgpu::TextureFormat;
    fn acquire(&mut self) -> Result<SurfaceFrame, SurfaceError>;
}
```

The renderer API must not accept a native window type.

### 15.2 Native host

Owns:

- `app_window`/Wayland window and surface lifecycle;
- real OS input adaptation;
- IME, clipboard, cursor, file/drop integration;
- AccessKit adapter;
- headed evidence capture integration where permitted by the existing proof contract.

### 15.3 Web host

Owns:

- `<canvas>` and WebGPU surface setup;
- browser pointer/keyboard/touch events;
- IME/text-input bridge;
- clipboard and URL integration;
- minimal semantic DOM/ARIA adapter;
- optional SEO/public-page snapshot integration outside the visual renderer.

### 15.4 No visual DOM renderer

The web host must not reproduce every visual primitive as HTML/CSS. The minimal semantic tree may include:

- buttons, links, text fields, lists/list items, headings, status/live regions;
- focused/selected assembly parts and property controls;
- a hidden/native text-input endpoint.

It must not include:

- rounded rectangles, shadows, glyph quads, clips;
- generated mesh chunks or triangles;
- one node per rendered pixel/surface cell.

### 15.5 HTML-in-canvas

Treat emerging HTML-in-canvas support as optional progressive enhancement for browser-native islands such as rich text input. Do not make it a correctness dependency, and do not allow it to create a different primary visual implementation.

### 15.6 Native desktop supervisor and bounded IPC

Preserve the existing native preview/dev separation while moving rendering behind the shared WGPU contract:

```text
boon_native_playground supervisor
    ├── preview child
    │     ├── owns an independent native window/surface
    │     ├── receives Boon source through `--code-file` or `ReplaceCode`
    │     ├── compiles/runs/renders through generic paths
    │     └── emits bounded telemetry and paged/query responses
    │
    └── dev/debug child
          ├── owns an independent native window/surface
          ├── resolves examples to source and edits source
          ├── sends `ReplaceCode`/input/control requests
          └── consumes bounded telemetry/query results
```

Requirements:

- The preview child never receives an example name as rendering semantics. It receives source and ordinary configuration only.
- The dev child may know example catalogs, but no example identity crosses into generic parser/runtime/document/render behavior.
- Never mirror the complete runtime, document, layout, display list, render scene, or GPU state over IPC.
- IPC queues have explicit byte/count bounds, backpressure/drop policy, heartbeat behavior, serialization timing, and observability.
- Preview input/rendering must remain responsive if dev/debug telemetry consumption stalls.
- Each child owns an independent surface and failure/restart boundary.
- The shared renderer remains usable without this supervisor (tests, browser host, single-window embedding, headless owned-target capture).

### 15.7 SEO

A pure WGPU editor route is not expected to provide searchable page content by pixels. Public/indexable Boon apps or model pages should compile a server/build-time semantic HTML snapshot from the same Boon semantic data:

```text
Boon semantic source
    ├── runtime SemanticScene
    └── public HTML metadata/content snapshot
```

Editor internals remain canvas-first.

---

## 16. SemanticScene and accessibility

### 16.1 Canonical semantic model

```rust
pub struct SemanticNode {
    pub id: SemanticId,
    pub role: SemanticRole,
    pub name: TextContentId,
    pub description: Option<TextContentId>,
    pub value: Option<SemanticValue>,
    pub state: SemanticState,
    pub actions: SemanticActions,
    pub relations: SemanticRelations,
    pub bounds: Option<Rect>,
    pub language: Option<LanguageId>,
    pub heading_level: Option<u8>,
    pub href: Option<UrlId>,
}
```

### 16.2 Focus and selection synchronization

Use the same stable identity mapping:

```text
SemanticId
    ├── AccessKit node
    ├── web semantic element data-boon-id
    ├── RenderItem PickId
    └── Boon interaction/source binding
```

```text
screen reader focuses “Front-left wheel”
    → Boon focus/selection event
    → WGPU highlight

pointer picks front-left wheel
    → Boon selection change
    → semantic focus/state update
```

### 16.3 Semantic granularity

TodoMVC exposes controls, rows, text, and status—not drawing primitives.

A car editor exposes the assembly, parts, selected feature, dimensions, warnings, and actions—not surface triangles.

### 16.4 AccessKit boundary

AccessKit is a native accessibility adapter, not a renderer and not the canonical schema. Keep Boon’s semantic model rich enough for native accessibility, browser semantics, and SEO/document concepts.

---

## 17. 3D semantic architecture

### 17.1 Keep UI and world models separate

Do not stretch `DocumentNode` into a universal 3D node. Use:

```rust
pub struct AppScene {
    pub ui: UiScene,
    pub world: WorldScene,
    pub semantics: SemanticScene,
}
```

They can share resource IDs and interaction identity while retaining domain-specific contracts.

### 17.2 WorldScene

```rust
pub struct WorldScene {
    pub cameras: SlotMap<CameraId, Camera>,
    pub lights: SlotMap<LightId, Light>,
    pub instances: SlotMap<InstanceId, ModelInstance>,
    pub appearances: ResourceTable<AppearanceMaterialId, AppearanceMaterial>,
}

pub struct ModelInstance {
    pub id: InstanceId,
    pub geometry: GeometryLogicalId,
    pub geometry_revision: GeometryRevision,
    pub transform: Transform3D,
    pub appearance: AppearanceBinding,
    pub part_id: PartId,
    pub pick_id: PickId,
    pub visibility: Visibility,
}
```

### 17.3 Appearance versus physical material

Keep these separate:

```text
AppearanceMaterialId:
    base color, roughness, metallic, transmission, emissive, textures

PhysicalMaterialId:
    PLA, PETG, resin A, soluble support, aluminium stock, etc.

PartId / FeatureId:
    body, wheel well, door, hole, rib, axle seat
```

Continuous appearance attributes may interpolate. Discrete part/feature/physical-material IDs never interpolate.

### 17.4 Authoritative SolidGraph

Represent occupied material as a typed region graph:

```rust
pub struct SolidGraph {
    pub nodes: Vec<SolidNode>,
    pub root: SolidNodeId,
    pub units: Units,
}

pub struct SolidNode {
    pub logical_id: GeometryLogicalId,
    pub op: SolidOp,
    pub bounds: Aabb64,
    pub feature_id: FeatureId,
    pub physical_region: RegionId,
}

pub enum SolidOp {
    Box { size: Vec3d },
    Sphere { radius: f64 },
    Cylinder { radius: f64, height: f64 },
    Cone { radius0: f64, radius1: f64, height: f64 },
    Torus { major_radius: f64, minor_radius: f64 },

    Extrude { profile: ProfileId, height: f64 },
    Revolve { profile: ProfileId, axis: Axis3d },
    Sweep { profile: ProfileId, path: CurveId },
    Loft { profiles: Vec<ProfileId> },

    Union { children: Vec<SolidNodeId> },
    Intersection { children: Vec<SolidNodeId> },
    Difference { base: SolidNodeId, tools: Vec<SolidNodeId> },

    Transform { child: SolidNodeId, transform: Mat4d },
    Offset { child: SolidNodeId, distance: f64 },
    Shell { child: SolidNodeId, thickness: f64 },
    SmoothUnion { a: SolidNodeId, b: SolidNodeId, radius: f64 },

    Functional { evaluator: EvaluatorId },
    ImportedSolid { source: ImportedSolidId },
}
```

### 17.5 Region evaluator contract

A node is not required to return an exact signed distance. It must provide enough conservative information for rendering/manufacturing:

```rust
pub trait RegionEvaluator {
    fn bounds(&self) -> Aabb64;
    fn contains(&self, point: Vec3d) -> bool;
    fn interval(&self, region: Aabb64) -> OccupancyInterval;
    fn gradient_or_normal(&self, point: Vec3d) -> Option<Vec3d>;
    fn variation_bound(&self, region: Aabb64) -> Option<f64>;
    fn section(&self, plane: Plane, tolerance: f64) -> Option<Region2D>;
}
```

Common primitives and extrusions should have analytic/specialized sections. Generic functional nodes use interval-controlled adaptive subdivision.

### 17.6 AssemblyGraph

```rust
pub struct AssemblyGraph {
    pub parts: ResourceTable<PartId, PartDefinition>,
    pub instances: Vec<PartInstance>,
    pub constraints: Vec<AssemblyConstraint>,
}

pub struct PartDefinition {
    pub geometry: GeometryLogicalId,
    pub appearance: AppearanceBinding,
    pub physical_material: Option<PhysicalMaterialId>,
    pub manufacturing_role: ManufacturingRole,
}

pub enum ManufacturingRole {
    PrintableSolid,
    VisualOnly,
    VoidModifier,
    SupportModifier,
    InfillModifier,
    Reference,
}
```

Repeated instances share geometry compilation. Four wheels must not create four independent wheel meshes or solid graphs.

---

## 18. Visual 3D compilation

### 18.1 Visual compiler contract

```rust
pub struct VisualCompileRequest {
    pub geometry: GeometryLogicalId,
    pub revision: GeometryRevision,
    pub camera_error: ScreenError,
    pub world_error_cap: f64,
    pub feature_profile: RenderFeatureProfile,
}
```

Output retained chunks:

```rust
pub struct SurfaceChunk {
    pub id: SurfaceChunkId,
    pub bounds: Aabb,
    pub lod: u8,
    pub error_bound: f64,
    pub geometry_revision: GeometryRevision,
    pub source_features: SmallVec<[FeatureId; 4]>,
    pub representation: SurfaceRepresentation,
}

pub enum SurfaceRepresentation {
    IndexedMesh(IndexedMeshChunk),
    DirectedDualGrid(DirectedDualGridChunk),
    ProceduralPrimitive(ProceduralPrimitiveChunk),
}
```

### 18.2 FDG-D-like cache

A directed dual-grid representation may be useful for adaptive freeform surface caching and local recompilation:

```rust
pub struct DirectedDualCell {
    pub position: [f32; 3],
    pub normal: PackedNormal,
    pub directed_crossings: u32,
    pub appearance_material: AppearanceMaterialId,
    pub part_id: PartId,
    pub feature_id: FeatureId,
}
```

It is not the authoritative solid. For V1, decode changed cells to indexed triangles on CPU. Consider GPU decode only after profiling proves it worthwhile.

### 18.3 Rendering behavior

- Primitive nodes may render procedurally or via shared meshes.
- Freeform nodes compile adaptively by screen/world error.
- Mesh chunks retain stable allocations until their chunk revision changes.
- Transform/visibility/material changes never force geometry recompilation.
- Camera movement may request a different LOD without mutating semantic geometry.
- Selection/picking IDs remain flat/discrete across material interpolation.

---

## 19. Manufacturing compiler

### 19.1 Never print the visual cache by default

```text
Incorrect normal path:
    Boon → visual mesh → external slicer

Correct normal path:
    Boon SolidGraph/AssemblyGraph
        → manufacturing validation
        → direct layer material regions
        → 3MF slices/toolpaths
```

A tolerance-controlled manufacturing mesh remains an explicit interoperability/export product.

### 19.2 Manufacturing request

```rust
pub struct PrintCompileRequest {
    pub assembly: AssemblyId,
    pub layer_height: f64,
    pub xy_error: f64,
    pub z_error: f64,
    pub minimum_feature: f64,
    pub integer_grid: f64,
    pub build_volume: Aabb64,
    pub profile: PrinterProfileId,
}
```

### 19.3 Validation

Before slicing, prove or diagnose:

- units are explicit and finite;
- printable parts define occupied material;
- visual-only/open surfaces are not silently materialized;
- materials/regions do not overlap ambiguously;
- wall thickness and clearances are evaluated against requested thresholds;
- build-volume placement is valid;
- touching parts are deliberately fused or separated;
- unsupported/unknown operations report a blocker rather than falling back silently;
- approximation bounds meet the request.

### 19.4 Direct sections

For each layer plane:

```text
specialized analytic sections where available
    +
adaptive interval/quadtree fallback for generic nodes
    ↓
closed oriented material regions
    ↓
deterministic integer-grid regularization
    ↓
perimeters / infill / supports or 3MF slice polygons
```

```rust
pub struct Layer {
    pub z: f64,
    pub regions: Vec<MaterialRegion2D>,
    pub achieved_error: f64,
    pub diagnostics: Vec<ManufacturingDiagnostic>,
}

pub struct MaterialRegion2D {
    pub material: PhysicalMaterialId,
    pub polygons: Vec<PolygonWithHoles>,
}
```

### 19.5 Meaning of “exact”

Boon may promise:

1. unambiguous occupied-material semantics;
2. deterministic regularized Boolean behavior;
3. closed, non-self-intersecting layer regions;
4. explicit units and physical material IDs;
5. achieved error at or below a declared tolerance;
6. diagnostics for unresolved sub-tolerance/singular features.

It must not promise finite exact representation of every possible real-valued surface or printer motion.

### 19.6 3MF

Prefer 3MF over STL for normal export because it can preserve units, objects/components, materials, and slice data. STL remains a compatibility export generated from a separately requested manufacturing tessellation.

---

## 20. Proposed working 3D Boon examples

The following examples define the **target API and behavior**. The exact module names may be adjusted during implementation, but the final checked-in examples must compile and run generically through parser → IR → MachinePlan → runtime → world/solid lowering. They must not be recognized by path or source-text markers.

### 20.1 Example A: `examples/hello_3d/RUN.bn`

Purpose:

- prove generic world construction;
- shared UI and 3D rendering;
- runtime-to-world transform/material deltas;
- picking and semantic selection;
- native/browser visual parity.

```boon
store: [
    controls: [
        rotate_left: SOURCE
        rotate_right: SOURCE
        toggle_color: SOURCE
    ]

    angle: 0 |> HOLD state {
        controls.rotate_left.event.press  |> THEN { state - 15 }
        controls.rotate_right.event.press |> THEN { state + 15 }
    }

    blue: False
        |> Bool/toggle(when: controls.toggle_color.event.press)
]

cube_geometry:
    Solid/box(
        id: CubeGeometry
        size: Vec3[x: 40, y: 40, z: 40]
    )

world:
    World/new(
        camera: Camera/perspective(
            id: MainCamera
            position: Vec3[x: 95, y: 75, z: 95]
            look_at: Vec3[x: 0, y: 0, z: 0]
            vertical_fov: 45
        )

        lights: LIST {
            Light/directional(
                id: KeyLight
                direction: Vec3[x: -1, y: -2, z: -1]
                illuminance: 12000
            )
        }

        objects: LIST {
            World/instance(
                id: Cube
                geometry: cube_geometry
                transform: Transform/rotate_y(degrees: store.angle)
                appearance: Appearance/pbr(
                    base_color: store.blue |> WHEN {
                        True  => Oklch[lightness: 0.68, chroma: 0.16, hue: 250]
                        False => Oklch[lightness: 0.72, chroma: 0.17, hue: 40]
                    }
                    roughness: 0.34
                    metallic: 0.05
                )
                semantic: [role: Model, label: TEXT { Rotating cube }]
            )
        }
    )

ui:
    Scene/Element/stripe(
        direction: Row
        gap: 8
        style: [paint: False]
        items: LIST {
            Scene/Element/button(
                element: [event: [press: SOURCE]]
                label: TEXT { Rotate left }
            ) |> SOURCE { store.controls.rotate_left }

            Scene/Element/button(
                element: [event: [press: SOURCE]]
                label: TEXT { Rotate right }
            ) |> SOURCE { store.controls.rotate_right }

            Scene/Element/button(
                element: [event: [press: SOURCE]]
                label: TEXT { Toggle color }
            ) |> SOURCE { store.controls.toggle_color }
        }
    )

app: App/new(ui: ui, world: world)
```

Required behavior:

- rotation changes emit one instance-transform patch and no geometry rebuild;
- color changes emit one material/resource update and no layout/geometry rebuild;
- cube picking resolves to `Cube`, `CubeGeometry`, and semantic label;
- screenshot and pick readback work from renderer-owned targets;
- no example-specific branch exists in Rust.

### 20.2 Example B: `examples/printable_bracket_3d/RUN.bn`

Purpose:

- prove constructive solids and regularized difference;
- prove physical material and manufacturing-role separation;
- prove direct slicing and 3MF output;
- prove UI parameter changes invalidate only affected geometry/sections.

```boon
parameters: [
    width: 70
    depth: 34
    base_thickness: 6
    upright_height: 42
    upright_thickness: 7
    hole_diameter: 6.4
    edge_radius: 2
]

base:
    Solid/rounded_box(
        id: Base
        size: Vec3[
            x: parameters.width
            y: parameters.depth
            z: parameters.base_thickness
        ]
        radius: parameters.edge_radius
    )

upright:
    Solid/rounded_box(
        id: Upright
        size: Vec3[
            x: parameters.width
            y: parameters.upright_thickness
            z: parameters.upright_height
        ]
        radius: parameters.edge_radius
    )
    |> Solid/translate(
        by: Vec3[
            x: 0
            y: -(parameters.depth - parameters.upright_thickness) / 2
            z: (parameters.upright_height - parameters.base_thickness) / 2
        ]
    )

hole_positions: LIST {
    -parameters.width / 2 + 12
    parameters.width / 2 - 12
}

mounting_holes:
    hole_positions
    |> List/map(x, new:
        Solid/cylinder(
            id: Feature/hole(x: x)
            radius: parameters.hole_diameter / 2
            height: parameters.base_thickness + 2
        )
        |> Solid/translate(by: Vec3[x: x, y: 0, z: 0])
    )
    |> Solid/union()

bracket_solid:
    Solid/union(items: LIST { base, upright })
    |> Solid/difference(tools: LIST { mounting_holes })

bracket_part:
    Part/new(
        id: Bracket
        geometry: bracket_solid
        appearance: Appearance/pbr(
            base_color: Oklch[lightness: 0.66, chroma: 0.11, hue: 220]
            roughness: 0.48
            metallic: 0
        )
        physical_material: PrintMaterial/PLA
        manufacturing_role: PrintableSolid
    )

assembly:
    Assembly/new(
        id: BracketAssembly
        parts: LIST { bracket_part }
        instances: LIST {
            Part/instance(id: BracketInstance, part: Bracket)
        }
    )

world:
    World/from_assembly(
        assembly: assembly
        camera: Camera/isometric(fit: BracketAssembly)
        lights: Light/studio_set()
    )

print_job:
    Print/job(
        assembly: assembly
        profile: Print/profile(
            layer_height: 0.20
            xy_error: 0.03
            z_error: 0.10
            minimum_feature: 0.40
            integer_grid: 0.001
        )
    )

app: App/new(
    ui: Print/inspector(job: print_job)
    world: world
    manufacturing: print_job
)
```

Required behavior:

- validation reports a printable closed material region;
- two holes remain holes in every intersecting layer;
- direct slices are deterministic across runs;
- requested/achieved error is recorded;
- 3MF contains millimetre units, one object/component, and physical material metadata;
- the visual mesh is not reused as manufacturing proof;
- a negative fixture with `hole_diameter` below `minimum_feature` reports a diagnostic instead of silently erasing it.

### 20.3 Example C: `examples/parametric_car_3d/RUN.bn`

Purpose:

- prove assemblies, shared geometry prototypes, freeform solids, visual-only surfaces, material separation, incremental visual recompilation, semantic selection, and manufacturing preparation.

```boon
store: [
    controls: [
        longer: SOURCE
        shorter: SOURCE
        larger_wheels: SOURCE
        paint_red: SOURCE
        paint_blue: SOURCE
    ]

    length: 168
        |> HOLD state {
            controls.longer.event.press  |> THEN { state + 4 }
            controls.shorter.event.press |> THEN { state - 4 }
        }

    wheel_radius: 28
        |> HOLD state {
            controls.larger_wheels.event.press |> THEN { state + 1 }
        }

    paint: LATEST {
        Red
        controls.paint_red.event.press  |> THEN { Red }
        controls.paint_blue.event.press |> THEN { Blue }
    }
]

spec: [
    length: store.length
    width: 76
    height: 52
    wheelbase: store.length * 0.62
    track: 62
    wheel_radius: store.wheel_radius
    tire_width: 15
    body_wall: 2.2
]

body_profiles:
    LIST {
        Profile/rounded_rectangle(id: Nose,   width: spec.width * 0.72, height: 24)
        Profile/rounded_rectangle(id: Cabin,  width: spec.width,        height: spec.height)
        Profile/rounded_rectangle(id: Tail,   width: spec.width * 0.82, height: 30)
    }

body_outer:
    Solid/loft(
        id: BodyOuter
        axis: X
        stations: LIST {
            Loft/station(x: -spec.length / 2, profile: body_profiles[Nose])
            Loft/station(x: 0,                profile: body_profiles[Cabin])
            Loft/station(x: spec.length / 2,  profile: body_profiles[Tail])
        }
    )

wheel_centers:
    LIST {
        Vec3[x: -spec.wheelbase / 2, y: -spec.track / 2, z: -8]
        Vec3[x: -spec.wheelbase / 2, y:  spec.track / 2, z: -8]
        Vec3[x:  spec.wheelbase / 2, y: -spec.track / 2, z: -8]
        Vec3[x:  spec.wheelbase / 2, y:  spec.track / 2, z: -8]
    }

wheel_wells:
    wheel_centers
    |> List/map(center, new:
        Solid/cylinder(
            id: Feature/wheel_well(center: center)
            radius: spec.wheel_radius + 3
            height: spec.tire_width + 10
            axis: Y
        )
        |> Solid/translate(by: center)
    )
    |> Solid/union()

body_solid:
    body_outer
    |> Solid/difference(tools: LIST { wheel_wells })
    |> Solid/shell(thickness: spec.body_wall)

wheel_geometry:
    Solid/revolve(
        id: WheelPrototypeGeometry
        axis: Y
        profile: Profile/tire_and_rim(
            radius: spec.wheel_radius
            width: spec.tire_width
        )
    )

body_part:
    Part/new(
        id: Body
        geometry: body_solid
        appearance: store.paint |> WHEN {
            Red  => Appearance/car_paint(color: TEXT { #b91c1c })
            Blue => Appearance/car_paint(color: TEXT { #1d4ed8 })
        }
        physical_material: PrintMaterial/PETG
        manufacturing_role: PrintableSolid
    )

wheel_part:
    Part/new(
        id: WheelPrototype
        geometry: wheel_geometry
        appearance: Appearance/tire_and_rim()
        physical_material: PrintMaterial/TPU
        manufacturing_role: PrintableSolid
    )

window_surfaces:
    Surface/from_body_regions(
        id: Windows
        body: body_outer
        regions: LIST { Windshield, RearWindow, LeftWindows, RightWindows }
    )

windows_part:
    Part/new(
        id: Windows
        geometry: window_surfaces
        appearance: Appearance/glass(tint: 0.25)
        manufacturing_role: VisualOnly
    )

assembly:
    Assembly/new(
        id: Car
        parts: LIST { body_part, wheel_part, windows_part }
        instances: LIST {
            Part/instance(id: BodyInstance, part: Body)
            Part/instance(id: WindowsInstance, part: Windows)

            wheel_centers
            |> List/enumerate()
            |> List/map(entry, new:
                Part/instance(
                    id: WheelInstance[index: entry.index]
                    part: WheelPrototype
                    transform: Transform/translate(by: entry.value)
                )
            )
        }
    )

world:
    World/from_assembly(
        assembly: assembly
        camera: Camera/orbit(target: Car, distance: 280)
        lights: Light/studio_set()
        ground: World/grid(unit: 10)
    )

print_job:
    Print/job(
        assembly: assembly
        include: LIST { Body, WheelPrototype }
        exclude: LIST { Windows }
        split: Print/split_to_build_volume(
            volume: Vec3[x: 220, y: 220, z: 250]
            connectors: Print/dowel_connectors(diameter: 4, clearance: 0.25)
        )
        profile: Print/profile(
            layer_height: 0.16
            xy_error: 0.035
            minimum_feature: 0.45
        )
    )

app: App/new(
    ui: Car/editor_controls(store: store, assembly: assembly, print_job: print_job)
    world: world
    manufacturing: print_job
)
```

Required incremental behavior:

| User change | Required work |
|---|---|
| Paint red → blue | Material update only; zero body/wheel geometry compilation |
| Orbit camera | Camera uniform/visibility/LOD work only |
| Move one wheel | One instance transform update |
| Increase wheel radius | Recompile wheel prototype once; four instances retain identity and reuse it; wheel-well/body dependency recompiles only affected body chunks |
| Increase body length | Recompile affected body/solid chunks and manufacturing sections; wheel prototype remains cached |
| Select a wheel | Pick/semantic/outline state only |
| Hide windows | Visibility patch only |
| Export print job | Compile from `SolidGraph`/assembly and exclude visual-only windows |

Required semantic tree:

```text
Car editor
├── 3D viewport
├── Car assembly
│   ├── Body
│   ├── Windows (visual only)
│   ├── Front-left wheel
│   ├── Front-right wheel
│   ├── Rear-left wheel
│   └── Rear-right wheel
├── Parameters
│   ├── Body length
│   ├── Wheel radius
│   └── Paint
└── Manufacturing
    ├── Validation status
    └── Export 3MF
```

Required manufacturing behavior:

- visual-only windows are not silently printed;
- body/wheels are separate material/part regions;
- split/connectors are explicit derived manufacturing operations;
- print validation reports shell thickness, minimum features, clearances, and build-volume fit;
- a visual FDG-D/mesh cache is not accepted as manufacturing source evidence.

---

## 21. Migration phases

Every phase must preserve existing working behavior and honest gates. Use adapters and differential tests; do not perform an all-at-once rewrite.

### Phase 0 — Reconcile current state and establish baselines

Deliverables:

- inspect actual `HEAD`, worktree, `AGENTS.md`, active plans, ledgers, and current reports;
- map current code paths to this plan;
- record release/debug baselines for TodoMVC, full Cells, idle rendering, scroll, text edit, and GPU uploads;
- create `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md` with task IDs, evidence links, blockers, and status definitions;
- cross-link—not duplicate—the BYTES/MachinePlan ledger.

Exit gate: baseline reports are reproducible and no current user changes were reset.

### Phase 1 — Finish BYTES/MachinePlan and runtime default-path migration

Deliverables:

- complete existing partial phases and missing typed BYTES operations;
- close speed-budget/release benchmark tasks;
- prove legacy/new parity and performance;
- switch CLI default only after gates;
- retain explicit differential legacy mode temporarily;
- ensure runtime emits typed semantic change batches.

Exit gate: existing MachinePlan goal-readiness audit has no implementation blockers attributable to incomplete migration; any human/OS evidence remains honestly separated.

### Phase 2 — Transactional UI/document changes and hot model

Deliverables:

- `apply_batch` transaction;
- numeric generational hot IDs and debug-name table;
- style/text/binding interning;
- typed style partitioning;
- multi-binding controls;
- tagged style serialization with legacy boundary decoder;
- changed-edge validation plus optional full validator;
- replace append-only materialization ranges.

Exit gate: single-property changes touch only expected hot records; full validator parity passes in tests.

### Phase 3 — Retained incremental layout and shared text

Deliverables:

- retained layout tree and dirty propagation;
- layout boundaries;
- virtualized list window replacement;
- shared shaping/line-layout/placement/paint caches;
- layout patches;
- proof that hover/scroll do not reshape or broadly reflow.

Exit gate: TodoMVC/Cells visual and hit-test parity; long scroll memory plateau; bounded dirty-node reports.

### Phase 4 — Canonical retained render model

Deliverables:

- one `boon_render` scene/patch contract;
- snapshot-to-patch adapter during migration;
- eliminate duplicate document/native GPU scenes and request variants;
- instanced physical UI primitives;
- stable material/clip/transform/text resources.

Exit gate: current examples render through the canonical scene; unchanged render nodes keep identities across interactions.

### Phase 5 — WGPU retained resources, framebuffer, and demand-driven scheduler

Deliverables:

- persistent GPU arenas;
- coalesced dirty writes;
- mandatory `ProductRenderGraph` / `PresentPlan` implementation trial where
  `ActivePreviewScene + ProductPatch` compiles into explicit product passes;
- before/after Cells and TodoMVC reports that prove whether the render graph
  improves latency or removes legacy product hot-path coupling without
  regression;
- product graph counters for pass count, plan hash, dirty chunks, upload bytes,
  encode time, cache hits, full-rebuild fallback count, proof-on-product-frame
  count, and stale epoch rejection;
- app-owned color/depth/pick targets;
- asynchronous screenshot/pick readback;
- demand-driven frame scheduler and wake handle;
- device-loss replay;
- production/proof metrics separation.

Exit gate: no-op/hover/text/scroll upload and frame-acquisition gates pass;
render-graph trial evidence is recorded; kept render-graph code either improves
performance or removes/quarantines legacy product hot-path coupling without
budget regression; existing native GPU proof requirements are not weakened.

### Phase 6 — Shared native/browser renderer and semantics

Deliverables:

- platform-neutral renderer API;
- thin native host and web host;
- identical shader/render-scene path;
- `SemanticScene` and incremental patches;
- native AccessKit adapter;
- minimal web semantic/IME bridge;
- browser artifact size/startup reports;
- render parity captures within explicitly documented tolerance.

Exit gate: same input render log produces matching native/web owned-target captures; no visual DOM implementation exists.

### Phase 7 — WorldScene and basic 3D WGPU path

Deliverables:

- cameras, lights, appearance materials, instances, meshes/primitives, picking;
- `WorldPatch` retained updates;
- shared geometry instances;
- basic orbit camera/grid/editor interactions;
- `hello_3d` example.

Exit gate: rotation/material changes prove no geometry rebuild; native/web 3D parity and semantic picking pass.

### Phase 8 — SolidGraph, assembly, and adaptive visual compilation

Deliverables:

- profiles/curves/primitives/CSG/transforms/extrude/revolve/initial loft/shell operations;
- bounds, interval/region evaluation, feature/material provenance;
- AssemblyGraph and repeated instances;
- retained adaptive indexed mesh chunks;
- optional FDG-D-like cache behind one representation enum;
- dependency-aware local recompilation.

Exit gate: printable bracket and car visual model compile generically; wheel prototype reuse and targeted invalidation reports pass.

### Phase 9 — Manufacturing compiler and 3MF

Deliverables:

- manufacturing roles/material regions;
- validation and diagnostics;
- specialized analytic sections;
- adaptive interval/quadtree fallback;
- deterministic integer-grid polygon regularization;
- 3MF object/material/slice output;
- explicit manufacturing mesh/STL fallback.

Exit gate: bracket direct slices/3MF pass tolerance and determinism tests; negative fixtures fail honestly.

### Phase 10 — Parametric car and production hardening

Deliverables:

- full car assembly example;
- visual-only/open surface policy;
- multi-material/part print preparation;
- build-volume split/connectors as explicit derived operations;
- performance, memory, browser size, accessibility, readback, and manufacturing reports;
- documentation and default-path cleanup.

Exit gate: all automated aggregate gates pass; remaining human/platform evidence is listed precisely and not fabricated.

### Phase 11 — Legacy retirement

Only after soak/parity/default-path proof:

- remove obsolete runtime path;
- remove duplicate render models and snapshot-only entry points;
- remove transitional adapters;
- update docs to describe only the real architecture;
- retain replay/report compatibility migrations where required.

---

## 22. Performance and behavior gates

Do not choose favorable microexamples or shrink Cells. Preserve existing benchmark fixtures and add scenario-specific delta proofs.

### 22.1 Required work proportionality

| Scenario | Required result |
|---|---|
| No-op/idle | 0 runtime semantic changes, 0 layout nodes, 0 text shaping, 0 retained geometry uploads, no continuous present |
| Hover a button | 0 layout, 0 shaping, one/few material or instance fields changed |
| Change Todo title without size change | one shape/line update, no unrelated subtree reflow |
| Scroll Cells | no graph rebuild; work proportional to newly visible rows/cells; memory plateaus |
| Reorder one keyed row | unchanged rows keep IDs/GPU allocations |
| Change car paint | material-only update |
| Move one wheel | instance-transform-only update |
| Change wheel radius | one prototype compile plus declared dependent body/well chunks; four instances reused |
| Change body length | affected body chunks/sections only; wheel prototype retained |
| Read one pick pixel | no full-frame CPU readback |
| GPU arena growth | no global retained-cache invalidation |
| Device loss | rebuild GPU cache from current retained semantic/render/world state |

### 22.2 Metrics

Capture fixed-cost production counters by default:

```text
runtime dirty routes/nodes
UI patches by kind
validated nodes/edges
layout dirty/visited/changed nodes
text shapes/line layouts/glyph uploads
render patches by kind
GPU bytes written/copied
GPU arena allocations/moves/fragmentation
surface acquisitions/presents
pick/readback bytes and latency
visual geometry chunks compiled/reused
manufacturing cells/sections refined
requested and achieved manufacturing error
WASM compressed size and first useful frame
```

Verbose vectors/strings/traces belong behind debug/proof features or sampled capture, not unconditional hot paths.

### 22.3 Baseline-relative budgets

Existing hard budgets remain binding. For new paths:

- establish a reproducible release baseline before optimization claims;
- require no regression in semantic correctness and evidence provenance;
- make budgets hardware/environment keyed where needed;
- reject a “faster” result obtained by smaller examples, lower fidelity, weaker validation, or disabled semantics;
- require explicit approval for material browser artifact growth, with measured cause and alternatives.

---

## 23. Verification and report strategy

### 23.1 Existing gates remain

Continue running existing parser/typecheck/runtime, MachinePlan/BYTES, report-schema, platform contract, native GPU architecture/shader/multiwindow/IPC/observability/E2E/scroll/negative/aggregate, and readiness audits.

### 23.2 New gates to implement

Suggested `xtask` contracts:

```text
verify-runtime-change-sets
verify-document-batch-patches
verify-retained-layout-deltas
verify-text-cache-layers
verify-render-patch-contract
verify-wgpu-retained-arenas
verify-wgpu-readback
verify-demand-driven-render-loop
verify-native-web-render-parity
verify-semantic-scene
verify-accessibility-adapters
verify-browser-artifact-budget
verify-3d-hello-cube
verify-solid-graph
verify-3d-printable-bracket
verify-manufacturing-slices
verify-3mf-export
verify-3d-parametric-car
verify-unified-architecture-all
```

These names are proposed. Add them to the advertised-command uniqueness/support gate before treating them as available.

### 23.3 Report provenance

Every report should include where applicable:

- repository commit and dirty-worktree state;
- compiler/tool versions;
- target triple/browser/GPU adapter/backend;
- command and scenario fixture hash;
- artifact hashes;
- headed/headless and synthetic/OS/human evidence classification;
- requested/achieved tolerances;
- failure/blocker classification;
- dependencies on other reports.

### 23.4 Negative gates

At minimum reject:

- example/path/source-text special cases;
- hidden legacy runtime fallback;
- full-state IPC mirroring;
- full document/layout/render reconstruction for declared local patches;
- global GPU-cache invalidation on ring wrap/growth;
- browser visual DOM fallback counted as renderer parity;
- headless proof mislabeled as headed;
- stale report/artifact hashes;
- printing from visual mesh without explicit manufacturing tessellation request;
- visual-only surfaces silently treated as solids;
- unresolved manufacturing features silently deleted;
- material/part/feature IDs interpolated as continuous values.

---

## 24. File/module reorganization

Start with internal modules, then extract stable public crates.

```text
crates/boon_document/src/
├── state.rs
├── patch.rs
├── validation.rs
├── hot_model.rs
├── style/
├── layout/
│   ├── mod.rs
│   ├── flow.rs
│   ├── intrinsic.rs
│   ├── dirty.rs
│   └── virtualization.rs
├── text/
├── hit_test.rs
└── semantics.rs

crates/boon_native_gpu/src/        # rename later if useful
├── renderer.rs
├── frame_graph.rs
├── render_scene_cache.rs
├── arenas/
├── pipelines/
├── text/
├── assets/
├── picking.rs
├── readback.rs
├── scheduler.rs
└── metrics.rs

crates/boon_native_app_window/src/
├── application.rs
├── window.rs
├── surface.rs
├── input.rs
├── ime.rs
├── accessibility.rs
└── event_loop.rs

crates/boon_geometry_ir/src/
├── ids.rs
├── units.rs
├── profiles.rs
├── curves.rs
├── solid.rs
├── assembly.rs
├── materials.rs
└── validation.rs

crates/boon_geometry_compile/src/
├── dependencies.rs
├── bounds.rs
├── adaptive.rs
├── mesh.rs
├── directed_dual_grid.rs
└── cache.rs

crates/boon_manufacturing/src/
├── request.rs
├── validate.rs
├── section.rs
├── quadtree.rs
├── regions.rs
├── regularize.rs
├── toolpath.rs
└── reports.rs
```

Move PNG encoding, SHA/proof packaging, and verbose capture into optional modules/crates so verification remains strong without contaminating the production hot path.

Split oversized playground/scenario files into app shell, fixtures, scenarios, generated data, and proof modules.

---

## 25. API compatibility and migration rules

- Add adapters before replacing public entry points.
- Every adapter has a removal issue/task and a parity test.
- Do not expose WGPU types through compiler/runtime/document/geometry semantic APIs.
- Do not expose platform window types through the renderer.
- Keep serialized report/protocol compatibility through explicit versioned conversion.
- New numeric hot IDs may retain human-readable debug names in reports.
- Legacy runtime and snapshot rendering modes must be explicit flags during migration, never implicit fallback.
- Do not rename crates merely for aesthetics before contracts are stable.

---

## 26. Definition of done

This architecture is implemented only when all of the following are true:

1. The normal CLI/runtime path uses the completed MachinePlan/PlanExecutor architecture with no hidden legacy fallback.
2. Semantic changes remain incremental through retained UI state, layout, text, render scene, and persistent GPU memory.
3. TodoMVC and full Cells retain correctness, keyed identity, virtualization, input behavior, and required speed/evidence gates.
4. Idle rendering is demand driven.
5. Native and browser use one WGPU visual implementation and one shader/render-model contract.
6. Boon owns color/depth/pick targets and working asynchronous readback.
7. Accessibility is generated from one SemanticScene, with native AccessKit and minimal web semantics adapters.
8. `hello_3d`, `printable_bracket_3d`, and `parametric_car_3d` compile through generic Boon language/runtime paths.
9. 3D transform/material edits prove targeted patches and geometry reuse.
10. Printable solids are authoritative `SolidGraph`/assembly regions, not visual meshes.
11. The bracket and car print paths produce deterministic, material-aware, tolerance-reported slices/3MF or honest diagnostics.
12. Existing and new aggregate gates pass with fresh, correctly classified evidence.
13. Documentation describes the actual default implementation rather than aspirational shortcuts.
14. Any remaining human/platform validation is listed clearly; it is not fabricated or renamed as automation.

---

## 27. Immediate next steps

1. Add this document and the companion `/goal` prompt to the repository.
2. Create the unified progress ledger and link the existing BYTES/MachinePlan task ledger.
3. Run the actual current readiness/audit commands and record blockers before code changes.
4. Finish the active MachinePlan/default-path migration.
5. Implement transactional document changes and hot typed storage.
6. Introduce retained layout/text and the canonical render-patch boundary.
7. Replace transient retained GPU behavior with persistent arenas and owned targets.
8. Add shared native/browser hosts and SemanticScene.
9. Add `hello_3d`, then SolidGraph/bracket, then the car/manufacturing pipeline.

The governing implementation principle is:

> Preserve stable identity and precise changes from Boon equations all the way to pixels and printer regions. Recompute only when a dependency, tolerance, or platform resource genuinely requires it.

---

## 28. Progress checkpoints

### 2026-07-06 — ProductRenderGraph is mandatory

- Removed the native playground `BOON_NATIVE_PRODUCT_RENDER_GRAPH` runtime escape hatch.
- Removed xtask graph-disabled/baseline ProductRenderGraph comparison logic.
- `ProductRenderGraph` / `PresentPlan` emission is now the only product interaction contract; missing graph, present plan, or execution evidence is a verifier failure.
- Removed graph-disabled before-report requirements from `verify-native-product-render-graph`; the focused gate now checks current active reports only.
- Kept negative proof counters such as full-rebuild fallback and proof/readback-in-product counts because they reject slow paths rather than preserve legacy behavior.
- Focused verification for this checkpoint: `cargo fmt -- --check`, `cargo build -q -p xtask`, `cargo test -q -p xtask product_render_graph -- --nocapture`, `cargo test -q -p xtask cells_visible_click_product -- --nocapture`, `cargo test -q -p boon_native_gpu product_frame_graph -- --nocapture`, `cargo test -q -p boon_native_playground product_render_graph -- --nocapture`, and `git diff --check`.

### 2026-07-06 — Native GPU product encode requires retained state

- Removed private optional retained-state fallbacks from the native GPU visible encode path.
- `encode_render_scene_to_surface_with_pipeline`, `encode_render_scene_patch_to_surface_with_pipeline`, and `encode_internal_scene_to_surface` now require retained text state, render-scene cache, quad buffers, upload ring, prepared-quad cache, previous chunk IDs, and ProductFrameGraph state.
- Removed the fresh per-call `QuadUploadRing` and `ProductFrameGraphState` fallbacks from product encoding.
- Render-scene patch encoding now requires a `patch_identity`, so patch caching cannot silently collapse to an uncached one-off path.
- Focused verification for this checkpoint: `cargo fmt -- --check`, `cargo build -q -p xtask`, `cargo test -q -p boon_native_gpu product_frame_graph -- --nocapture`, `cargo test -q -p boon_native_playground product_render_graph -- --nocapture`, `cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json`, `target/debug/xtask verify-report-schema target/reports/native-gpu/architecture.json`, and `git diff --check`.
