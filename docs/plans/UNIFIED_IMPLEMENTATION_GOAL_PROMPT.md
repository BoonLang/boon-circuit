# `/goal` Prompt — Complete the Unified Boon Runtime, Rendering, 3D, and Manufacturing Architecture

Place the companion architecture file at:

```text
docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md
```

Place this file at:

```text
docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md
```

Then run the full prompt below.

```text
Implement and verify the unified Boon runtime, retained rendering, shared native/browser WGPU, accessibility, 3D, and manufacturing architecture in:

/home/martinkavik/repos/boon-circuit

Treat these as binding contracts, in this order:

1. AGENTS.md and explicit user instructions.
2. Honesty, evidence provenance, and non-fabrication requirements already enforced by report schemas and readiness audits.
3. docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md
4. The active BYTES/MachinePlan implementation plan and progress ledger.
5. Existing runtime, LIST, delta, document/UI, native GPU, and demand-driven-render-loop plans where they do not conflict with the unified architecture.

Do not commit or push unless explicitly asked.

This is a continuation of an active half-migration, not a rewrite and not a fresh project. Work from the actual current HEAD and preserve unrelated user changes. Never reset, checkout away, clean, overwrite, or discard worktree changes. Detect the real repository state before editing. The planning snapshot was main commit 95f86d265de7585ee1bc6d04cddf356d6cc16ae3 on 2026-06-22, but do not assume the repository is still exactly there.

Do not stop after writing another plan. Implement code, tests, examples, reports, and documentation. Continue through the ordered phases as far as the current environment and real blockers permit. Do not claim completion while readiness gates still report implementation blockers. If a platform/human prerequisite prevents honest proof, complete all non-blocked work, preserve the strict gate, record the precise blocker, and stop with an evidence-based handoff rather than weakening or fabricating the result.

This is intended to be one continuous `/goal`, not a sequence of unrelated
short plans. Preserve forward progress: when one task blocks repeatedly after
bounded diagnosis, record the blocker in the progress ledger, keep the relevant
readiness/default-switch gate failing, and continue into the next non-blocked
architectural phase if that phase can produce useful implementation evidence or
may remove the blocker by changing the runtime/document/layout/render pipeline.
Do not spin indefinitely on one benchmark or slow path.

──────────────────────────────────────────────────────────────────────────────
A. FIRST: INSPECT, RECONCILE, AND PRESERVE THE ACTIVE MIGRATION
──────────────────────────────────────────────────────────────────────────────

Before changing code:

1. Read:
   - AGENTS.md
   - docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md
   - docs/plans/BYTES_AND_MACHINE_PLAN_IMPLEMENTATION.md
   - docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md
   - docs/plans/RUNTIME_FINALITY_HONESTY_PLAN.md
   - docs/plans/REMOVE_VIEW_DOCUMENT_UI_GOAL.md
   - docs/plans/NATIVE_DEMAND_DRIVEN_RENDER_LOOP_PLAN.md
   - docs/plans/GOAL_PROMPT.md
   - docs/architecture/RUNTIME_MODEL.md
   - docs/architecture/LIST_MODEL.md
   - docs/architecture/DELTA_PROTOCOL.md
   - docs/architecture/NATIVE_GPU_PIPELINE.md
   - relevant report-schema and xtask source.

2. Inspect:
   - git status, current branch, current HEAD, recent commits, and untracked files;
   - current cargo workspace/crate graph;
   - the actual default CLI execution path;
   - MachinePlan/PlanExecutor and legacy-executor call sites;
   - current document patch, layout, render-scene, WGPU, native host, report, and example paths;
   - current reports and readiness-audit output.

3. Create or update:

   docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md

   It must include:
   - actual starting HEAD and dirty-worktree summary;
   - links to the existing BYTES/MachinePlan ledger rather than copied task history;
   - phase/task IDs for the unified plan;
   - status: not-started / in-progress / blocked / implemented / verified;
   - changed files and symbols;
   - exact commands and report paths;
   - blockers and evidence classification;
   - next executable task.

4. Run the existing readiness/audit commands before implementation and record their real result. A component aggregate passing while audit-goal-readiness still fails is not completion.

5. Update stale path/symbol assumptions in the plan/ledger when the code moved, but never silently reduce scope, remove negative gates, or reinterpret a blocker as success.

──────────────────────────────────────────────────────────────────────────────
B. NON-NEGOTIABLE ARCHITECTURAL RULES
──────────────────────────────────────────────────────────────────────────────

1. One normal runtime:
   parser AST → typed IR → MachinePlan → PlanExecutor.

2. The legacy executor may exist temporarily only as an explicit differential/debug path. No hidden fallback in the final default execution path.

3. No TodoMVC-, Cells-, file-path-, example-name-, source-text-, substring-, comment-, or marker-based behavior in parser, compiler, runtime, document lowering, rendering, 3D, or manufacturing.

4. Preserve stable keyed identity and generation through runtime, UI, layout, rendering, picking, accessibility, world instances, parts, and geometry revisions.

5. Preserve semantic deltas end to end. Do not collapse every interaction into full DocumentFrame/LayoutFrame/RenderScene reconstruction and then rediscover differences.

6. Use one custom WGPU visual renderer for native and browser. Do not add Bevy or another application/game engine. Do not implement a separate HTML/CSS visual renderer.

7. Browser DOM use is limited to semantic accessibility, text/IME, links/forms where required, and optional public SEO snapshots. It must not mirror visual primitives or mesh cells.

8. Boon owns app-created scene color, depth, pick-ID, and optional feature/normal targets. Keep screenshot, picking, and asynchronous readback under Boon control.

9. The authoritative printable 3D model is a typed SolidGraph/AssemblyGraph. Meshes, SDFs, UDFs, B-Reps, voxels, and FDG-D-like structures may be import/export/cache/specialized representations, not the universal editable source.

10. Visual geometry is disposable. Never use the visual mesh as the normal manufacturing authority.

11. Rendering and printing are separate compilers from the same semantic source:
    - screen-error-driven visual compiler;
    - tolerance/material-driven manufacturing compiler.

12. Keep AppearanceMaterialId, PhysicalMaterialId, PartId, FeatureId, RegionId, and PickId distinct. Never interpolate discrete semantic/manufacturing IDs.

13. Preserve all existing report provenance, stale-report rejection, artifact hashing, headed/headless classification, synthetic/OS/human evidence classification, and negative-gate requirements.

──────────────────────────────────────────────────────────────────────────────
C. IMPLEMENTATION ORDER
──────────────────────────────────────────────────────────────────────────────

Execute these phases in order. Small preparatory refactors are allowed, but do not skip an earlier architectural dependency by building a parallel shortcut.

PHASE 0 — BASELINES AND TASK GRAPH

- Reproduce current tests/reports.
- Establish release/debug baselines for:
  - TodoMVC;
  - full Cells 26x100 or the current full fixture, without shrinking it;
  - BYTES/MachinePlan cases;
  - idle native rendering;
  - hover, title edit, list reorder, vertical/horizontal scroll;
  - current GPU bytes/writes/cache behavior;
  - current browser/WASM artifact if one exists.
- Add the unified progress ledger and a dependency graph between active tasks.
- Do not optimize before recording a comparable baseline.

PHASE 1 — COMPLETE THE EXISTING BYTES/MACHINEPLAN HALF-MIGRATION

Continue the current ledger from its real state. Do not restart or duplicate it.

Required outcomes:

- complete all remaining typed BYTES parser/typecheck/IR/plan/runtime operations and diagnostics;
- preserve dynamic, inferred-fixed, and explicit-fixed BYTES semantics;
- complete MachinePlan lowering coverage;
- finish typed/columnar hot runtime storage and dense route indexes;
- close remaining row/function BYTES operation gaps;
- diagnose and attempt to close the current release Cells benchmark/speed-budget work, including the actual successor of TASK-0804A if IDs changed;
- prove parity across TodoMVC, full Cells, BYTES, keyed LIST behavior, negative cases, and reports;
- ensure runtime output is typed semantic change batches;
- switch normal boon_cli run to PlanExecutor only after parity, performance, and readiness gates permit it;
- retain an explicit temporary legacy/differential flag if useful;
- remove legacy code only after default-path soak and no-hidden-fallback verification.

Do not describe the runtime as final while audit-goal-readiness reports runtime implementation blockers.

TASK-0804A and related Cells benchmark blockers are not permission to loop
forever. Run a bounded investigation, preserve the best measurements and
root-cause notes, and make one of these decisions:

- fixed: update the BYTES/MachinePlan progress ledger, rerun aggregate and
  readiness gates, and continue toward the default switch;
- still blocked by the current runtime architecture: keep the default switch and
  old readiness gate blocked, create or update a reviewed ADR/progress entry
  that says which later runtime/document/layout/render phases are expected to
  remove the cost, then continue into those phases;
- invalid/stale task: supersede it with replacement evidence, an explicit ADR,
  and a new tracked task/report.

Do not relabel TASK-0804A as solved merely because later phases are planned.
Also do not prevent later runtime, retained document/layout/render, or WGPU
work from starting solely because TASK-0804A remains open; those changes may be
the real fix. The unified goal is only complete when the final readiness and
performance gates prove the blocker is fixed, superseded, or no longer relevant
to the default path.

PHASE 2 — TRANSACTIONAL DOCUMENT CHANGES AND HOT RETAINED MODEL

Implement a batch change boundary, approximately:

DocumentState::apply_batch(&ChangeBatch<UiSemanticChange>)
    -> Result<DocumentChangeSet, PatchApplyError>

Required outcomes:

- apply a runtime tick’s UI changes transactionally;
- validate changed nodes/edges/references rather than the full tree before and after every individual patch;
- retain an exhaustive full validator behind tests/debug/proof feature and prove parity;
- introduce numeric generational hot IDs with human-readable debug-name tables;
- keep the serialized/readable protocol separate from hot storage;
- intern text, layout styles, paint styles, text styles, materials, clips, and bindings;
- replace string-keyed hot style lookup for known properties with typed records;
- support multiple typed interaction bindings per node;
- fix RichTextSpans/EditorTypeHints typed serialization round-trip, with explicit legacy decoding at the protocol boundary;
- replace broad SetText invalidation with dirty facts;
- replace append-only materialized ranges with current materialized windows;
- add structural InsertChild/RemoveChild/MoveChild or equivalent precise changes rather than relying on broad UpsertNode.

Keep public compatibility through adapters/versioned conversion while migrating.

PHASE 3 — RETAINED INCREMENTAL LAYOUT AND SHARED TEXT

Required outcomes:

- retained LayoutTree keyed by stable IDs;
- dirty flags for constraints, intrinsic size, bounds, clip, transform, visibility, and scroll;
- layout containment/boundaries so propagation stops when outputs are unchanged;
- virtualized list work proportional to newly visible items and memory that reaches a plateau;
- LayoutPatch output instead of mandatory full LayoutFrame snapshots per interaction;
- full snapshots retained for initial construction, replay, recovery, reports, and debug only;
- one shared text service for layout and rendering;
- separate shape, line-layout, placement, and paint caches;
- color-only text changes do not reshape;
- scroll-only changes do not reshape or rerasterize;
- glyph atlas uploads only new glyphs and remains retained across frames.

Prove:

- hover: zero layout and zero shaping;
- passive scroll: no runtime graph rebuild and no unrelated text shaping;
- title edit: one/bounded text run and layout subtree;
- keyed row move: unchanged rows keep identity.

PHASE 4 — ONE CANONICAL RETAINED RENDER MODEL

Create a platform-neutral canonical render contract, initially as modules and later a crate if stable.

Required primitives include:

- UiBox;
- Text;
- Image;
- Path;
- Mesh.

Required retained resources include:

- transforms;
- clips;
- appearance materials;
- text runs;
- geometry handles;
- pick IDs.

Required outcomes:

- one RenderScene/RenderPatch contract consumed by WGPU;
- temporary snapshot-to-patch adapter for migration;
- remove/retire duplicate document/native-GPU render scenes and multiple overlapping request APIs after parity;
- stable RenderNodeId independent from content and GPU offsets;
- todomvc_physical rendered with compact instanced rounded boxes/physical UI primitives plus retained text, not generated arbitrary meshes or FDG-D;
- source events/picking map through stable IDs and list generations.

PHASE 5 — WGPU RETAINED RESOURCES, OWNED TARGETS, AND DEMAND-DRIVEN SCHEDULING

Refactor boon_native_gpu toward a platform-neutral boon_wgpu contract without doing a cosmetic rename first.

Required outcomes:

- persistent arenas for retained UI instances, mesh vertices/indices, transforms, materials, clips, and text metadata;
- transient ring only for per-frame/scratch data;
- ring wrap/growth never invalidates all retained geometry;
- coalesced dirty GPU writes/copies;
- app-owned scene color, depth, and integer pick-ID targets;
- asynchronous screenshot and small-region/pixel pick readback;
- frame graph that renders world, UI/text, selection/focus, and final composite;
- demand-driven scheduler with explicit dirty reasons and wake handle;
- no surface acquisition/present while truly idle;
- animation deadlines and input/IPC/worker wake integration;
- device-loss recovery by recreating GPU resources and replaying current retained scenes;
- fixed lightweight production counters; verbose strings/vectors/traces only in proof/debug capture;
- preserve and complete the native desktop supervisor contract: `boon_native_playground` starts independent preview and dev/debug child processes with independent native surfaces; preview receives Boon source only through `--code-file`/`ReplaceCode`, never example identity; dev may resolve examples to source; telemetry/query IPC is bounded and must not mirror full runtime/document/layout/render state; preview rendering remains responsive under dev/debug overload.

Keep actual WESL/WGSL/generated-binding verification. Do not allow marker strings or copied shader text to count as proof.

PHASE 6 — SHARED NATIVE/WEB VISUAL PATH AND SEMANTIC ACCESSIBILITY

Required outcomes:

- one platform-neutral renderer API with no native-window types;
- thin native surface/input/IME/clipboard host;
- thin web canvas/input/IME/clipboard host;
- same RenderPatch log, shader sources, buffer formats, material model, text rendering, picking, and readback logic on both platforms;
- target-specific WGPU feature selection so WASM does not pull native backends or engine-scale dependencies;
- compressed WASM size, startup, pipeline creation, and first-useful-frame reports;
- one retained SemanticScene/SemanticPatch model;
- typed MachinePlan application output ports for regular `document`/`app`/`world`/`manufacturing` values; remove semantic dependence on special `VIEW`, source-text search, or example paths while retaining an explicit tested compatibility adapter only during migration;
- native AccessKit adapter;
- minimal web DOM/ARIA semantic and text-input bridge only;
- bidirectional focus/selection synchronization between semantics and GPU picking;
- optional public/indexable semantic HTML snapshot generation, separate from the editor visual implementation;
- no visual DOM fallback counted as native/web parity.

HTML-in-canvas may be explored only as optional progressive enhancement after the baseline web adapter works. It must not become a correctness dependency or a separate primary renderer.

PHASE 7 — WORLD SCENE AND BASIC 3D

Add a sibling WorldScene rather than turning DocumentNode into a universal 3D type.

Required outcomes:

- Camera, Light, ModelInstance, AppearanceMaterial, Transform3D, GeometryLogicalId, GeometryRevision, PartId, FeatureId, PickId;
- retained WorldPatch updates;
- indexed meshes and procedural/shared primitive geometry;
- instancing: repeated objects share one geometry resource;
- orbit/perspective/isometric camera support needed by examples;
- depth, opaque, transparent, selection/outline, UI/text passes;
- world picking and SemanticScene integration;
- native/web parity for the same owned render target.

Implement and check in:

examples/hello_3d/RUN.bn

Use the target behavior from the unified architecture plan. It must compile through generic parser/IR/MachinePlan/runtime/world lowering. Required proofs:

- rotation → one transform patch, no geometry rebuild;
- color → one material update, no geometry rebuild;
- picking → stable object/geometry/semantic IDs;
- renderer-owned screenshot and pick readback;
- no example-specific Rust branch.

PHASE 8 — SOLIDGRAPH, ASSEMBLYGRAPH, AND VISUAL COMPILATION

Implement typed geometry semantics with explicit units.

Initial required operations:

- box, sphere, cylinder, cone, torus;
- 2D profiles and rounded rectangles;
- transform/translate/rotate/scale;
- union, intersection, regularized difference;
- extrude and revolve;
- initial sweep/loft sufficient for checked examples;
- offset/shell with explicit diagnostics where unsupported/ambiguous;
- functional/imported nodes through a typed evaluator boundary.

Each solid node must provide or conservatively derive:

- bounds;
- occupied-region membership;
- interval/range classification over a region;
- normal/gradient when available;
- variation/error bound when available;
- specialized section when available.

Implement:

- AssemblyGraph with PartDefinition and repeated PartInstance;
- AppearanceMaterialId separate from PhysicalMaterialId;
- ManufacturingRole: PrintableSolid, VisualOnly, VoidModifier, SupportModifier, InfillModifier, Reference;
- feature/part/region provenance;
- dependency-aware GeometryRevision and chunk invalidation;
- adaptive retained visual surface chunks;
- IndexedMesh as the initial universal render output;
- optional DirectedDualGrid/FDG-D-like cache behind a representation enum only after indexed-mesh correctness works;
- CPU decode first; GPU decode only after profiling proves a need.

A visual cache is never authoritative solid state.

PHASE 9 — DIRECT MANUFACTURING COMPILER AND 3MF

Implement manufacturing from SolidGraph/AssemblyGraph, not from the visual mesh.

Required outcomes:

- PrintCompileRequest with explicit units, layer height, XY/Z error, minimum feature, integer grid, build volume, and printer/profile IDs;
- validation of printable roles, closed occupied material, region/material conflicts, finite units, build volume, wall thickness, clearances, and unsupported operations;
- analytic/specialized sections for common primitives/extrusions/revolutions;
- interval-controlled adaptive 2D quadtree fallback for generic nodes;
- closed oriented material regions with holes;
- deterministic integer-grid polygon regularization;
- requested and achieved error in reports;
- diagnostics instead of silent deletion/filling of unresolved features;
- physical material IDs and part/component identity;
- 3MF object/component/material/slice export;
- separately requested tolerance-controlled manufacturing mesh/STL compatibility export.

Implement and check in:

examples/printable_bracket_3d/RUN.bn

Use the target behavior in the architecture plan. Prove:

- closed printable material;
- holes remain holes through relevant layers;
- deterministic slices and artifact hash for the same build/toolchain/profile;
- achieved tolerance is reported;
- 3MF units/components/material metadata are present;
- negative sub-minimum-feature fixture diagnoses/fails honestly;
- visual mesh/cache is not accepted as manufacturing evidence.

PHASE 10 — PARAMETRIC CAR ASSEMBLY

Implement and check in:

examples/parametric_car_3d/RUN.bn

The example must be a generic Boon assembly, not a hardcoded Rust model.

Required behavior:

- body solid, wheel prototype, four wheel instances, and visual-only windows;
- wheel instances share one wheel geometry;
- car paint changes update appearance only;
- moving one wheel updates one transform only;
- wheel-radius changes compile the wheel prototype once and recompile only declared dependent wheel-well/body chunks;
- body-length changes preserve wheel prototype cache when its inputs do not change;
- windows are selectable/renderable but excluded from printing unless explicitly solidified;
- semantic assembly tree exposes body, windows, four wheels, parameters, validation, and export action;
- print preparation uses explicit physical materials/parts and build-volume splitting/connectors as derived manufacturing operations;
- no visual FDG-D/mesh data is accepted as manufacturing source truth.

PHASE 11 — CLEANUP, DEFAULTS, AND DOCUMENTATION

Only after parity/soak/gates:

- remove obsolete legacy runtime code and hidden fallback paths;
- remove duplicate render scene/request models;
- remove snapshot-to-patch adapters no longer needed;
- split oversized modules while preserving clear public contracts;
- extract stable crates only when useful;
- update old architecture docs to link to or accurately reflect the real implementation;
- preserve versioned report/protocol compatibility where required;
- ensure public docs do not overstate finality.

──────────────────────────────────────────────────────────────────────────────
D. REQUIRED FILE/MODULE ORGANIZATION DIRECTION
──────────────────────────────────────────────────────────────────────────────

Do not do a mass crate rename before behavior is stable. Split internal modules first. Move toward:

- boon_document: state, patch, validation, hot_model, style, layout, text, hit_test, semantics;
- boon_render: canonical scene, patches, primitives, materials, clips, text refs;
- boon_native_gpu/boon_wgpu: renderer, frame_graph, scene_cache, arenas, pipelines, text, picking, readback, scheduler, metrics;
- native host: window, surface, input, IME, accessibility, event loop;
- boon_scene_model: world resources and patches;
- boon_geometry_ir: IDs, units, profiles, curves, solids, assemblies, materials, validation;
- boon_geometry_compile: bounds, dependencies, adaptive chunks, indexed mesh, optional directed dual grid, cache;
- boon_manufacturing: request, validation, sections, quadtree, regions, regularization, toolpath-ready layers, reports;
- boon_3mf: 3MF serialization and validation.

Move PNG/proof packaging, hashes, and verbose capture out of production hot data structures into optional proof/capture modules.

──────────────────────────────────────────────────────────────────────────────
E. PERFORMANCE AND WORK-PROPORTIONALITY CONTRACT
──────────────────────────────────────────────────────────────────────────────

Add tests/reports that reject regressions in work proportionality. At minimum:

1. Idle/no-op:
   - zero semantic changes;
   - zero layout nodes visited because of application changes;
   - zero text shaping;
   - zero retained geometry uploads;
   - no continuous surface acquisition/present.

2. Hover/material-only UI change:
   - zero layout;
   - zero shaping;
   - one/few material or instance fields uploaded.

3. Text edit:
   - one/bounded text run shaped;
   - layout limited to affected boundary;
   - no unrelated display/render reconstruction.

4. Cells scroll:
   - no graph rebuild;
   - no passive-scroll runtime dispatch unless semantically observed;
   - work proportional to newly visible items;
   - CPU/GPU/materialization memory reaches a plateau;
   - full fixture is unchanged.

5. One keyed row move:
   - unchanged rows keep IDs and GPU allocations.

6. GPU arena growth/wrap:
   - no global retained-cache invalidation.

7. Car paint:
   - material-only update.

8. One wheel transform:
   - instance-only update.

9. Wheel-radius edit:
   - one shared prototype compile;
   - four instances reused;
   - only explicit dependencies invalidated.

10. Pick readback:
    - one pixel/small region, not full-frame CPU copy.

11. Manufacturing:
    - requested and achieved error reported;
    - no visual-resolution shortcut;
    - deterministic output for fixed inputs/environment;
    - unresolved feature produces a diagnostic.

Use existing hard budgets where present. For new metrics, record a reproducible release baseline before making performance claims. Never pass by shrinking examples, lowering fidelity, disabling validation/semantics, or relabeling evidence.

──────────────────────────────────────────────────────────────────────────────
F. TESTS, GATES, AND REPORTS
──────────────────────────────────────────────────────────────────────────────

Run focused checks continuously and full gates at phase boundaries. At minimum preserve and run the applicable existing commands, including:

- cargo fmt --check
- cargo check --workspace or the narrower documented package set while iterating
- cargo test --workspace --no-fail-fast where feasible, plus all affected package tests
- cargo test -p xtask advertised_xtask_commands_are_unique_and_supported
- cargo xtask verify-build-bytes-boundary
- cargo xtask verify-bytes-byte-bank-layout
- cargo xtask verify-bytes-file-read-plan
- cargo xtask verify-bytes-file-write-plan
- cargo xtask verify-bytes-storage-profile
- cargo xtask verify-bytes-negative
- cargo xtask verify-bytes-machine-plan-adversarial
- cargo xtask verify-bytes-machine-plan-all
- cargo xtask verify-report-schema
- cargo xtask audit-machine-readiness, if still advertised under that name
- cargo xtask audit-goal-readiness
- cargo xtask verify-platform-contract --report target/reports/native-gpu/platform-contract.json
- cargo xtask verify-native-gpu-dependency-graph --report target/reports/native-gpu/dependency-graph.json
- cargo xtask verify-native-gpu-architecture --report target/reports/native-gpu/architecture.json
- cargo xtask verify-native-gpu-layout-contract --report target/reports/native-gpu/layout-contract.json
- cargo xtask verify-native-gpu-shaders --check --report target/reports/native-gpu/shaders.json
- cargo xtask verify-native-gpu-multiwindow --report target/reports/native-gpu/multiwindow.json
- cargo xtask verify-native-gpu-ipc-backpressure --report target/reports/native-gpu/ipc-backpressure.json
- cargo xtask verify-native-gpu-observability --report target/reports/native-gpu/observability.json
- cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc.json
- cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells.json
- cargo xtask verify-native-gpu-scroll-speed --example cells --report target/reports/native-gpu/scroll-speed-cells.json
- cargo xtask verify-native-gpu-scroll-speed --surface dev-code-editor --report target/reports/native-gpu/scroll-speed-dev-code-editor.json
- cargo xtask verify-native-gpu-negative --report target/reports/native-gpu/negative.json
- cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json

Resolve command names against current xtask help. Do not silently omit an existing advertised gate because a path/name changed.

Add, advertise, test, implement, and then run equivalent new gates for:

- verify-runtime-change-sets
- verify-document-batch-patches
- verify-retained-layout-deltas
- verify-text-cache-layers
- verify-render-patch-contract
- verify-wgpu-retained-arenas
- verify-wgpu-readback
- verify-demand-driven-render-loop
- verify-native-web-render-parity
- verify-semantic-scene
- verify-accessibility-adapters
- verify-browser-artifact-budget
- verify-3d-hello-cube
- verify-solid-graph
- verify-3d-printable-bracket
- verify-manufacturing-slices
- verify-3mf-export
- verify-3d-parametric-car
- verify-unified-architecture-all

These new names are contracts to add; do not report them as run before they exist and pass the advertised-command support test.

Reports must include, as applicable:

- current commit and dirty state;
- toolchain and target/browser/GPU adapter/backend;
- command/scenario/fixture hash;
- artifact hashes;
- headed/headless status;
- synthetic/OS/human input status;
- requested and achieved manufacturing tolerances;
- dependent report paths/hashes;
- measured work counters;
- blockers/failure classifications.

Negative gates must reject at least:

- example/path/source-text special cases;
- hidden legacy fallback;
- full-state IPC mirroring;
- full snapshot reconstruction for local declared changes;
- global retained GPU-cache invalidation on arena/ring events;
- visual DOM counted as shared renderer;
- stale or mismatched reports/artifacts;
- headless/synthetic proof mislabeled as headed/OS/human;
- printing from visual mesh as the normal path;
- visual-only/open surfaces silently made printable;
- material/feature/part IDs interpolated;
- unresolved manufacturing features silently erased;
- weakened examples or tolerances used to pass performance.

Use real visible Wayland/app_window evidence for native headed E2E/speed gates where the existing contract requires it. Do not use headless/Xvfb as native proof. If tooling lacks an API required for honest proof, improve the wrapper/tooling or report the blocker; do not weaken the gate.

──────────────────────────────────────────────────────────────────────────────
G. IMPLEMENTATION PRACTICES
──────────────────────────────────────────────────────────────────────────────

- Make small coherent changes with focused tests.
- Preserve compatibility through explicit adapters and versioned conversion.
- Give every transitional adapter a removal task and parity test.
- Do not expose wgpu/platform window types through compiler/runtime/document/geometry semantic APIs.
- Do not derive stable identity from content, list position, GPU offset, or parameter hash.
- Do not add unconditional verbose allocations to hot metrics paths.
- Keep failures typed and diagnostic; do not silently degrade fidelity.
- Add comments for non-obvious invariants, not line-by-line narration.
- Keep generated artifacts out of source unless the repository contract explicitly tracks them.
- Format and run focused tests before moving to the next phase.
- Update the unified progress ledger after every meaningful task cluster, including failures and blockers.
- Preserve and update the BYTES/MachinePlan progress ledger for its own tasks.

When an operation cannot yet meet its contract, return a typed unsupported/tolerance diagnostic and add a negative test. Never silently fall back to a lower-fidelity mesh, visual cache, or generic full rebuild while reporting success.

──────────────────────────────────────────────────────────────────────────────
H. COMPLETION CRITERIA
──────────────────────────────────────────────────────────────────────────────

Do not declare the unified goal complete until all are true:

1. MachinePlan/PlanExecutor is the normal execution path and readiness audits have no implementation blockers from the half-migration.
2. Runtime semantic changes remain incremental through retained UI, layout, text, canonical render scene, and persistent GPU resources.
3. TodoMVC and full Cells correctness, keyed identity, virtualization, input, performance, and evidence gates pass.
4. Idle rendering is demand driven.
5. Native and browser use the same WGPU visual implementation and shader/render contract.
6. Boon owns framebuffer targets and working pick/screenshot readback.
7. One SemanticScene drives native AccessKit and minimal web semantics/IME adapters.
8. hello_3d, printable_bracket_3d, and parametric_car_3d compile and run through generic Boon paths.
9. World transform/material changes and shared geometry invalidation prove targeted work.
10. SolidGraph/AssemblyGraph is the manufacturing authority.
11. Bracket and car manufacturing produce deterministic, material-aware, tolerance-reported slices/3MF or honest diagnostics.
12. Existing and new aggregate gates pass with fresh correctly classified evidence.
13. Old duplicate/legacy paths are removed only after parity/soak.
14. Documentation matches the real default implementation.
15. Any remaining human/platform validation is listed precisely and not fabricated.

At the end of the implementation pass, provide a concise evidence-based handoff containing:

- actual starting and ending HEAD/dirty status;
- implemented phases/tasks;
- changed files grouped by subsystem;
- exact commands run and pass/fail result;
- report/artifact paths and hashes where generated;
- performance/work-proportionality results;
- unresolved blockers with evidence classification;
- the next concrete task;
- an explicit statement that no commit/push occurred unless the user asked for it.

Do not substitute a persuasive narrative for failed or missing gates.
```

## Short slash command

```text
/goal follow docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md and continue the existing BYTES/MachinePlan half-migration through the unified retained UI/WGPU, native-web, accessibility, 3D SolidGraph, and manufacturing architecture. Preserve all existing honesty and verification gates; do not commit or push unless explicitly asked.
```
