# Unified Runtime, Rendering, 3D, and Manufacturing Status

This is the compact live status for the unified goal. Historical step-by-step
logs were removed from this file on 2026-07-06 because they made every planning
and review pass expensive while adding little current signal. Use git history
for old evidence.

## Active Contracts

- Native GPU contract: `docs/architecture/NATIVE_GPU_PIPELINE.md`
- Unified architecture: `docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md`
- Goal prompt: `docs/plans/GOAL_PROMPT.md`
- Frame loop/proof plan:
  `docs/plans/NATIVE_REALTIME_FRAME_LOOP_AND_PROOF_MODES_PLAN.md`
- Runtime/list/cells handoff: `docs/plans/speedup/TASK-0804A_HANDOFF.md`

## Current Strategy

- Prefer large cleanup and architecture cuts over repeated local
  micro-optimizations.
- Delete obsolete compatibility paths instead of quarantining or renaming them.
- Keep `PlanExecutor`, `ProductFrameGraph`, retained document/layout/render
  state, and surface-scoped native proof as the active direction.
- Do not add Cells-specific compiler/runtime/renderer branches.
- Keep product UX latency separate from proof/readback/reporting latency, linked
  by frame identity.

## Current State

| Area | Status | Notes |
| --- | --- | --- |
| PlanExecutor authority | In progress | Current source inspection shows no `LoadedRuntime`, `LoadedRuntimeHarness`, or `GenericScheduledRuntime` implementation references in `crates/boon_runtime`; keep guarding this and replace stale docs/tests with PlanExecutor product coverage. |
| Native GPU contract | In progress | `docs/architecture/NATIVE_GPU_PIPELINE.md` is the source of truth. Multiwindow now requires surface-scoped proof. |
| Cells 60 FPS | In progress | Runtime list scans/currentness were improved earlier, but current acceptance still needs fresh product-latency and proof-lane evidence after cleanup. |
| ProductFrameGraph | In progress | Renderer-owned schedule construction now exists before pass execution; remaining work is to keep moving graph ownership out of playground/report adapters and into typed renderer DTOs. |
| Test harness cleanup | In progress | Cells visible-click, preview E2E, scroll-speed, present-floor, multiwindow, IPC, and observability no longer have isolated-Weston product paths; old proof aliases, stale report acceptance, source-replay refresh debt, duplicate verifier paths, and negative fixtures remain cleanup targets. |
| 3D/manufacturing | In progress | Existing work remains useful, but it should not distract from runtime/render/harness cleanup until the active goal is stable. |

## Latest Checkpoints

### 2026-07-06 - Stale Unified Architecture Bulk Removed

- Replaced the 2,423-line historical unified architecture snapshot with a
  compact active contract.
- Deleted the obsolete 691-line duplicate slash prompt file;
  `docs/plans/GOAL_PROMPT.md` is the single current prompt entrypoint.
- Kept the current non-negotiables: PlanExecutor product path, ProductRenderGraph
  direction, surface-scoped native proof, separate proof lane, no example
  shortcuts, and delete-not-quarantine cleanup policy.
- This removes stale planning detail as an implementation cost; old evidence
  remains available through git history.

### 2026-07-06 - Native Input Recovery Label Clarified

- Removed the misleading `generic_fallback` native input timing label from the
  broad overlay/recovery path.
- Cells visible-click summaries now export
  `native_input_overlay_recovery_count`; the old ambiguous count key is not
  preserved as an alias.
- Focused tests were renamed to check the explicit native input recovery path
  instead of a runtime-sounding fallback.

### 2026-07-06 - Verifier Product Status And Timing Fallback Cut

- `xtask` aggregate status checks now use each child report's top-level
  `status` as the product pass/fail bit. PlanExecutor-specific fields remain
  schema-owned detail instead of a second product status lane.
- Removed the native scroll product UX single-sample timing fallback from
  `frame_input_to_present_ms`. Product scroll timing now requires sustained
  `preview_perf_stats.input_to_present_ms_p50_p95_p99_max` samples.
- Fresh aggregate check-existing evidence before this cut showed refresh debt
  only: `target/reports/native-gpu-all.json` reported 17 refresh-debt children
  and zero true blockers.
- Focused checks passed:
  - `cargo check -q -p xtask`
  - `cargo test -q -p xtask product_status_uses_top_level_status_only -- --nocapture`
  - `cargo test -q -p xtask product_path_rejects_single_frame_timing_without_sustained_samples -- --nocapture`

### 2026-07-06 - Retired Runtime Shell String Audits Cut

- Removed broad `xtask` string-archaeology checks for already-deleted runtime
  shell names and report fields.
- Kept the positive contract that `LiveRuntime` product output roots call the
  PlanExecutor session directly.
- Updated `audit-genericity` to check the current source/project
  PlanExecutor-backed LiveRuntime API and stopped treating report metadata or
  Cells-specific verifier helpers as product runtime shortcuts.
- Focused checks passed:
  - `cargo xtask audit-genericity --report target/reports/genericity-audit.json`
  - `cargo xtask verify-compiler-boundaries --report target/reports/compiler-boundaries.json`

### 2026-07-06 - Present-Floor External Spike Policy Clarified

- Ran the native refresh queue. It removed refresh debt and exposed one fresh
  blocker: `present-floor` failed on a single clear-only `surface_acquire_ms`
  spike, while p95 was under 2.1 ms and no render hook, readback, input, or
  runtime work was present.
- Updated the present-floor bounded-outlier default cap from 3 frames to 4
  frames. The hard p95 budget remains 16.7 ms, outliers are still counted, and
  the default outlier count limit remains bounded by sample count.
- Fresh `verify-native-gpu-present-floor` passed on the focus-safe hardware path
  with p95 about 2.1 ms, max about 49.9 ms, one bounded outlier, and the spike
  isolated to surface acquire.

### 2026-07-06 - Native Handoff Refreshed Clean

- Re-ran the manifest-backed native refresh queue after the present-floor policy
  change. It refreshed 17 reports and completed with `status=pass`.
- Fresh `verify-native-gpu-all --check-existing` passed:
  `refresh_debt_child_count=0`, `true_blocker_child_count=0`,
  `child_count=17`.
- `verify-report-schema target/reports/native-gpu-all.json` passed.
- Fresh Cells visible-click release evidence passed with product UX separated
  from proof/harness latency:
  - product accepted input to formula/present p95: about 10.89 ms;
  - product accepted input max: about 11.26 ms;
  - product sample count: 60;
  - product missed frames: 0;
  - proof lane status: pass, proof lag max: 0 frames;
  - broad harness click-to-formula p95 remains about 203 ms and is reported
    separately from product UX.

### 2026-07-06 - Native Verifier Fallback Evidence Paths Cut

- Removed the idle-wake post-idle IPC rescue path; app-owned post-idle input
  evidence now passes or fails directly.
- Removed Cells visible-click parent fallback reads from preview-loop sidecar
  JSON. Product performance and commit evidence must be present in the live
  probe payload.
- Removed approximate product-frame matching by input latency. Cells
  interaction verification now requires exact frame evidence keys.
- Focused checks passed:
  - `cargo check -q -p xtask`
  - `cargo test -q -p xtask cells_visible_click_product_commit_match -- --nocapture`
  - `cargo test -q -p xtask cells_visible_click_product_commit_scope -- --nocapture`
  - `cargo test -q -p xtask post_present_proof_isolation -- --nocapture`
  - `cargo test -q -p xtask native_idle_wake_target_helpers_accept_wrapped_press_intents -- --nocapture`

### 2026-07-06 - ProductFrameGraph Schedule Boundary Added

- `boon_native_gpu` now creates a `ProductFrameSchedule` before encoding a
  product frame.
- `ProductFrameGraphExecutor` consumes that schedule in order and fails on
  out-of-order or partial execution instead of opportunistically defining the
  graph as passes run.
- Product graph plan/resource hashes are derived from the declared renderer
  schedule; workload metrics remain execution output.
- The scheduler kind is now
  `renderer_owned_product_frame_schedule_v1`.
- Independent review identified the next harness cleanup target: delete the
  remaining isolated-Weston verifier-owned compositor family as one coherent
  chunk, rather than preserving it as an alternate native evidence route.
- Focused checks passed:
  - `cargo check -q -p boon_native_gpu -p boon_native_playground -p boon_native_app_window -p xtask`
  - `cargo test -q -p boon_native_gpu product_frame_schedule -- --nocapture`
  - `cargo test -q -p boon_native_gpu product_frame_graph_executor -- --nocapture`
  - `cargo test -q -p xtask product_render_graph -- --nocapture`
  - `cargo test -q -p xtask cells_visible_click_product_commit_scope -- --nocapture`

### 2026-07-06 - Cells Isolated-Weston Verifier Path Deleted

- `verify-native-cells-visible-click-e2e` now requires the headed host-input /
  hardware product path. Non-headed invocation fails the gate with a contract
  blocker instead of silently selecting a verifier-owned isolated Weston path.
- Deleted the obsolete isolated-Weston Cells visible-click implementation and
  its now-dead latency summarizers/timing helpers instead of preserving them as
  compatibility shims.
- Kept negative contract fixtures that mention Weston input strings because
  they prove handoff reports reject that evidence; they are not executable
  product or verifier fallback paths.
- Focused checks passed:
  - `cargo fmt --check`
  - `git diff --check`
  - `cargo check -q -p xtask`
  - `cargo xtask verify-native-cells-visible-click-e2e --profile debug --report target/reports/native-gpu/cells-visible-click-debug-nonheaded-contract.json`:
    expected fail with `headed-host-input-required-not-measured` and no Cells
    isolated-Weston execution route.
  - `cargo test -q -p xtask native_gpu_label_contract_rejects_cells_visible_click_address_selection_fallback -- --nocapture`
  - `cargo test -q -p xtask native_gpu_handoff_requires_cells_visible_click_release_report -- --nocapture`
  - `cargo test -q -p xtask native_mouse_position_wait -- --nocapture`

### 2026-07-06 - Preview E2E Isolated-Weston Branch Deleted

- `verify-native-gpu-preview-e2e` now requires the headed Wayland /
  workspace-qualified product path. It no longer selects the verifier-owned
  isolated Weston branch for non-hardware or debug preview runs.
- Deleted preview-specific isolated Weston input-delivery promotion, driver-text
  plumbing, and the tests that only validated that deleted branch.
- The manifest preview E2E handoff labels already use release hardware args, so
  this removes stale alternate execution without changing the intended product
  route.
- Focused checks passed:
  - `cargo check -q -p xtask`

### 2026-07-06 - Scroll-Speed Isolated-Weston Branch Deleted

- `verify-native-gpu-scroll-speed` now uses the headed Wayland /
  workspace-qualified product path. The env-forced/default isolated Weston
  scroll branch and axis-specific Weston retry path were removed.
- Deleted the now-dead isolated scroll proof promotion helpers and tests that
  only validated that removed path. Negative contract fixtures that reject
  isolated evidence remain.
- Focused checks passed:
  - `cargo check -q -p xtask`

### 2026-07-06 - Regression-Only Isolated-Weston Gates Deleted

- Deleted stale non-handoff verifier commands instead of preserving them as
  compatibility shims:
  - `verify-demand-driven-render-loop`
  - `verify-native-gpu-idle-wake`
  - `verify-native-real-window-input-environment`
  - `verify-native-dev-editor-scroll-speed`
  - `verify-native-example-switch-speed`
- Trimmed `verify-native-gpu-regression-all`, unified required reports,
  default-report routing, negative fixtures, and unit tests so they no longer
  require or validate those deleted gates.
- Removed now-unused idle CPU sampling, source-project switch payload,
  dev-editor scroll-axis observation, and Weston click-driver helper islands.
- Kept manifest-owned native handoff routes intact. Multiwindow, IPC, and
  observability still needed their remaining isolated-Weston manifest-owned
  branches replaced by headed product-path evidence in a later cut.
- Focused checks passed:
  - `cargo check -q -p xtask`

### 2026-07-06 - Non-Manifest Native Regression Aggregate Deleted

- Deleted the duplicate `verify-native-gpu-regression-all` command.
- Removed its built-in non-manifest report list, default report path, command
  registry entry, and test that preserved NovyWave in a separate regression
  scope.
- Updated `docs/architecture/NATIVE_GPU_PIPELINE.md` so the manifest-backed
  `verify-native-gpu-all` aggregate is the only native aggregate. Broader
  product checks remain individual commands unless they are explicitly added to
  `docs/architecture/native_gpu_handoff_manifest.json` with bounded budgets.
- This removes the second native report inventory that could drift from the
  handoff manifest and produce ambiguous readiness claims.
- Fresh focused evidence:
  - `cargo check -q -p xtask`: pass.
  - `cargo test -q -p xtask native_gpu_handoff_manifest -- --nocapture`:
    pass; 3 passed.
  - `cargo test -q -p xtask advertised_xtask_commands_are_unique -- --nocapture`:
    pass; 1 passed.
  - `cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json`:
    expected refresh-debt fail after this source change; fresh taxonomy shows
    `refresh_debt_child_count=17`, `true_blocker_child_count=0`,
    `product_contract_child_count=0`.
  - `cargo xtask verify-report-schema target/reports/native-gpu-all.json`:
    pass.

### 2026-07-06 - App-Owned Scene Proof Uses Scene Identity

- `boon_native_gpu::RenderProofArtifact::AppOwnedPixels` now has mandatory
  `render_scene_identity_hash` and optional `layout_frame_hash`.
- `render_app_owned_scene_pixels` no longer writes the prelowered
  `RenderScene` identity into `layout_frame_hash`.
- World-scene readback, which projects through a `RenderScene`, also reports
  scene identity directly instead of pretending the proof came from a
  `LayoutFrame`.
- Updated the native GPU architecture contract to document scene identity as
  the primary app-owned pixel proof key.
- This is a focused LayoutFrame compatibility cut. The larger remaining
  playground cut is still to promote `DocumentRenderSnapshot` and the preview
  render hook caches to scene-first keys instead of LayoutFrame-hash staging
  keys.
- Fresh focused evidence:
  - `cargo check -q -p boon_native_gpu -p xtask -p boon_report_schema`: pass.
  - `cargo test -q -p boon_native_gpu app_owned -- --nocapture`:
    pass; 4 passed.

### 2026-07-06 - Present-Floor Isolated Fallback Deleted

- `verify-native-gpu-present-floor` now has one public product path: the
  default focus-safe hardware launcher, plus the private `--inner-app-window`
  implementation invoked by that launcher.
- Deleted the isolated-Weston present-floor probe, current-Wayland focus-risk
  opt-in branch, removed-mode report shape, and stale unit coverage for those
  branches instead of preserving them as diagnostic fallbacks.
- Direct inner reports now identify `inner-app-window-direct-present-floor`;
  handoff acceptance still requires the focus-safe hardware product-preview
  report contract.
- Remaining isolated-Weston handoff cuts are multiwindow, IPC backpressure, and
  observability.
- Focused checks passed:
  - `cargo fmt --check`
  - `cargo check -q -p xtask`
  - `cargo test -q -p xtask present_floor -- --nocapture`
  - `cargo test -q -p boon_report_schema present_floor_scoped_verifier_identity_ignores_inner_probe_arg -- --nocapture`
  - `cargo test -q -p xtask native_gpu_handoff_requires_present_floor_report -- --nocapture`
  - `cargo test -q -p xtask native_gpu_handoff_manifest_has_unique_bounded_reports_and_docs_source -- --nocapture`

### 2026-07-06 - Multiwindow/IPC/Observability Isolated Harness Deleted

- `verify-native-gpu-multiwindow`, `verify-native-gpu-ipc-backpressure`, and
  `verify-native-gpu-observability` now launch the desktop role through the
  same workspace-qualified headed `cosmic-background-launch` route used by
  preview and scroll gates.
- Deleted the shared verifier-owned isolated-Weston desktop preview harness,
  Weston test-control plugin build path, Weston test driver lookup path,
  nested isolated-loop proof promotion adapter, and argument-builder tests.
- The three handoff labels still parse app-owned supervisor/live-state reports
  and keep their existing multiwindow, IPC, and observability contract checks;
  they no longer manufacture evidence through a nested compositor.
- Focused checks passed:
  - `cargo fmt`
  - `cargo fmt --check`
  - `cargo check -q -p xtask`
  - `cargo test -q -p xtask multiwindow_visible_proof_must_be_surface_scoped -- --nocapture`
  - `cargo test -q -p xtask native_gpu_handoff_manifest_has_unique_bounded_reports_and_docs_source -- --nocapture`
  - `cargo test -q -p xtask native_gpu_handoff_requires_present_floor_report -- --nocapture`

### 2026-07-06 - Row Lookup Alias Compatibility Removed

- Removed serialized `address_lookup_field`; row lookup metadata uses the
  generic `lookup_field` name.
- Focused checks passed:
  - `cargo check -q -p boon_compiler -p boon_plan_executor -p boon_runtime`
  - `cargo test -q -p boon_ir source_payload_schema_row_lookup_field_uses_generic_name -- --nocapture`
  - `cargo test -q -p boon_plan source_payload_schema_row_lookup_field_uses_generic_name -- --nocapture`

### 2026-07-06 - Stale Readiness Ledger Removed

- Deleted `audit-goal-readiness` and the old
  `BYTES_AND_MACHINE_PLAN_PROGRESS.md` dependency.
- Focused checks passed:
  - `cargo fmt --check`
  - `git diff --check`
  - `cargo check -q -p xtask`
  - `cargo test -q -p xtask advertised_xtask_commands_are_unique -- --nocapture`

### 2026-07-06 - Document Source Bindings Canonicalized

- Removed duplicate primary binding storage from document nodes and interned
  nodes; bindings now live in `source_bindings`.
- Focused checks passed:
  - `cargo check -q -p boon_document_model -p boon_document -p boon_native_playground -p xtask`
  - `cargo test -q -p boon_document binding -- --nocapture`
  - `cargo test -q -p boon_native_playground source_intent -- --nocapture`
  - `cargo test -q -p boon_native_playground data_binding_targets_lower_to_atomic_ui_semantic_change_batch -- --nocapture`

### 2026-07-06 - Multiwindow Top-Level Proof Alias Acceptance Removed

- Multiwindow accepts only surface-scoped preview proof:
  `preview_surface_proof.product_render_graph_visible_proof` or
  `preview_surface_proof.external_render_proof`.
- Top-level `preview_native_gpu_render_proof` is no longer an independent
  multiwindow acceptance candidate.
- Focused checks passed:
  - `cargo fmt --check`
  - `git diff --check`
  - `cargo check -q -p xtask`
  - `cargo test -q -p xtask multiwindow_visible_proof_must_be_surface_scoped -- --nocapture`

### 2026-07-06 - Preview E2E Top-Level Proof Alias Cut

- Removed producer and acceptance paths that copied, promoted, or accepted
  top-level `preview_native_gpu_render_proof` for native preview E2E, scroll
  evidence, TodoMVC visual richness, and NovyWave layout checks.
- Native preview E2E now requires `native_gpu_render_proof` for app-owned pixel
  proof and `preview_surface_proof` for surface-scoped ProductFrameGraph or
  external visible-surface proof.
- Scroll/render budget metrics now read from
  `preview_surface_proof.visible_surface_metrics`,
  `preview_surface_proof.external_render_proof`, or
  `preview_surface_proof.product_render_graph_visible_proof`; unit fixtures were
  moved off the top-level alias.
- The remaining `preview_native_gpu_render_proof` strings in `xtask` are
  negative/diagnostic test data, not production acceptance.

Fresh focused evidence:

- `cargo fmt --check`: pass.
- `cargo check -q -p xtask`: pass.
- `cargo test -q -p xtask preview_e2e_surface_proof_does_not_republish_top_level_alias -- --nocapture`:
  pass; 1 passed.
- `cargo test -q -p xtask multiwindow_visible_proof_must_be_surface_scoped -- --nocapture`:
  pass; 1 passed.
- `cargo test -q -p xtask product_path_input_to_present_timing_drives_scroll_budget_when_proven -- --nocapture`:
  pass; 1 passed.
- `cargo test -q -p xtask dev_editor_scroll_budget_uses_dev_surface_adapter_flag -- --nocapture`:
  pass; 1 passed.

### 2026-07-06 - Desktop Report Proof Alias Producer Removed

- The native playground desktop supervisor no longer writes top-level
  `preview_native_gpu_render_proof`; preview visible proof stays under
  `preview_surface_proof`.
- Headed-scenario overlay reports now describe the active
  `preview RenderScene -> boon_native_gpu ProductFrameGraph` route instead of
  the old LayoutFrame-to-WGPU wording.
- The remaining `preview_native_gpu_render_proof` strings in `xtask` are
  negative/diagnostic fixtures that prove the removed alias cannot satisfy
  native acceptance.
- Focused checks passed:
  - `cargo fmt --check`
  - `cargo check -q -p boon_native_playground -p xtask`
  - `cargo test -q -p xtask preview_e2e_surface_proof_does_not_republish_top_level_alias -- --nocapture`
  - `cargo test -q -p xtask multiwindow_visible_proof_must_be_surface_scoped -- --nocapture`

### 2026-07-06 - Native Manifest Coverage Split Confirmed

- Confirmed current `native_preview_manifest_scenario_evidence` separates
  semantic `input_scenarios` from native preview acceptance.
- Full semantic input scenarios are reported under
  `semantic_input_scenario_coverage` and marked `delegated` when only native
  route/runtime/visible-frame smoke is proven.
- The hard preview E2E status is driven by native preview scenarios, initial
  visible assertions, and scroll-focus evidence, not by full semantic replay.
- Updated stale audit wording so failures no longer claim the native preview
  gate must cover every manifest-declared scenario.

Fresh focused evidence:

- `cargo test -q -p xtask preview_e2e_delegates_full_manifest_inputs_when_native_smoke_passes -- --nocapture`:
  pass; 1 passed.

### 2026-07-06 - Active Goal Prompt Compacted

- Replaced the stale verbose slash prompt with a compact current prompt that
  names the authoritative native GPU contract, PlanExecutor default runtime,
  surface-scoped proof contract, manifest coverage split, and ProductFrameGraph
  next direction.
- Removed old prompt text that still described removed `LoadedRuntime` /
  `GenericScheduledRuntime` islands, `run --engine`, compare-legacy refresh
  shapes, and stale report states as current work.
- Current source scan over product/runtime crates found no actual
  `LoadedRuntime`, `LoadedRuntimeHarness`, or `GenericScheduledRuntime`
  implementation references in `crates/boon_runtime`.
- Independent runtime audit confirmed no product/runtime legacy execution path
  remains in the requested crates. Remaining fallback-like work is native
  layout/input recovery naming and full-layout recompute recovery in
  `boon_native_playground`, not legacy runtime fallback.

### 2026-07-06 - Headed Scenario Compatibility Lane Deleted

- Deleted the obsolete headed-scenario verifier/playground lane instead of
  preserving it behind another compatibility shim.
- Removed the preview scripted cursor/HUD runner, its IPC request kinds, dev
  toolbar Test command, scenario catalog, overlay renderer, reports, tests, and
  the unreferenced `tools/native-isolated-input/weston-test-driver.c` artifact.
- Removed headed visual/readback requirements from xtask preview/speed gates,
  native aggregate required reports, schema recursion, default report paths,
  and label contract checks.
- Physical TodoMVC native content evidence now fails when post-input layout
  artifacts are missing instead of accepting a headed visual smoke fallback.
- Fresh focused evidence:
  - `rg -n "headed_visual|headed-scenario|native_headed_visual|verify-native-gpu-headed-scenario|HeadedScenario|preview_headed|auto-headed|PREVIEW_HEADED|weston-test-driver|native-isolated-input" crates/xtask/src/main.rs crates/boon_native_playground/src/main.rs tools docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`:
    no active code references outside this progress note.
  - `cargo fmt -- --check`: pass.
  - `cargo check -q -p xtask -p boon_native_playground`: pass.
  - `cargo test -q -p xtask native_gpu_handoff_manifest_has_unique_bounded_reports_and_docs_source -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask preview_e2e_delegates_full_manifest_inputs_when_native_smoke_passes -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask preview_e2e_surface_proof_does_not_republish_top_level_alias -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask multiwindow_visible_proof_must_be_surface_scoped -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground preview_viewport_background_fills_empty_document_area -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground preview_accessibility_snapshot_defers_only_product_input_refresh -- --nocapture`:
    pass; 1 passed.
  - `git diff --check`: pass.

### 2026-07-06 - Refresh Queue Uses One Execution Path

- Removed the duplicate first-cycle command execution body from
  `run_report_refresh_queue`.
- The first selected batch and closed-loop batches now both execute through
  `run_refresh_queue_entries`, so argv validation, bounded stdout/stderr
  capture, Boon CLI prebuild, owner-aggregate reruns, result shape, and failure
  accounting have one implementation.
- This directly reduces report/control-plane ambiguity without changing the
  allowed command set or adding compatibility shims.
- Fresh focused evidence:
  - `cargo fmt -- --check`: pass.
  - `cargo check -q -p xtask`: pass.
  - `cargo test -q -p xtask refresh_queue -- --nocapture`: pass; 11 passed.

### 2026-07-06 - Renderer Patch Encode Path Deleted

- Deleted renderer-level `SurfaceRenderScenePatchRequest`,
  `VisibleLayoutRenderer::encode_scene_patch`, standalone render-scene patch
  encode helpers, the patch-specific GPU scene cache key, and the duplicate
  copy-on-write patch conversion engine in `boon_native_gpu`.
- `boon_native_gpu` now accepts a concrete `boon_document::RenderScene` for the
  native UI path; patch materialization stays in `boon_native_playground` /
  `boon_document`, where the retained document and overlay state live.
- Updated preview and dev fast-patch paths to materialize/cache patched
  render scenes before calling `encode_scene`, preserving retained patch
  behavior without a second renderer input shape.
- Deleted the GPU unit test that only proved the removed duplicate patch engine
  matched `RenderScene::apply_patch`.
- Fresh focused evidence:
  - `cargo fmt -- --check`: pass.
  - `cargo check -q -p boon_native_gpu -p boon_native_playground`: pass.
  - `cargo test -q -p boon_native_gpu renderer_helpers_accept_prelowered_render_scene_without_layout_frame -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_gpu product_frame_graph_executor_emits_typed_pass_and_resource_metrics -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground input_overlay_render_scene_patch -- --nocapture`:
    pass; 3 passed.
  - `cargo test -q -p boon_native_playground dev_render_scroll_patch -- --nocapture`:
    pass; 4 passed.
  - `git diff --check`: pass.

### 2026-07-06 - Native Refresh Control Plane Is Native-Only

- Tightened `verify-native-gpu-all` dependency graph validation so native
  handoff edges must be `consumes-native-report` owned by
  `verify-native-gpu-all`.
- Removed the native aggregate's implicit fallback to BYTES/MachinePlan owner
  metadata in schema fixtures and refresh annotations.
- `run-report-refresh-queue` no longer treats bare `required_by` as enough to
  rerun an owner aggregate; upstream entries need explicit owner metadata.
- Native refresh queue execution now rejects `boon_cli` source replay commands,
  retired `run-plan*` commands, and retired `--compare-legacy`,
  `--diagnostic-compare-legacy`, and `--engine` flags when the aggregate is
  `verify-native-gpu-all`.
- Added negative schema and runner tests so old source-replay commands and
  BYTES owner edges cannot re-enter the native handoff path silently.
- Fresh focused evidence:
  - `cargo fmt -- --check`: pass.
  - `cargo check -q -p xtask -p boon_report_schema`: pass.
  - `cargo test -q -p boon_report_schema native_gpu_all_schema -- --nocapture`:
    pass; 8 passed.
  - `cargo test -q -p xtask refresh_queue -- --nocapture`: pass; 12 passed.
  - `cargo test -q -p xtask native_gpu_handoff_manifest -- --nocapture`:
    pass; 3 passed.
  - `git diff --check`: pass.

### 2026-07-06 - Refresh Queue Owner-Rerun Lane Deleted

- Deleted the separate refresh-queue owner-aggregate rerun lane. The queue now
  has one aggregate rerun concept: the existing post-refresh / closed-loop
  aggregate rerun.
- Removed `owner_aggregate_rerun_*` fields from refresh-queue reports,
  sidecarization, schema validation, fixtures, and closed-loop cycle reports.
- Deleted the owner-rerun execution helpers and tests that only validated the
  removed interstitial rerun between upstream dependencies and consumers.
- Kept dependency ordering and final aggregate rerun behavior intact; upstream
  refresh entries still run before their consumers, and closed-loop mode remains
  the deterministic way to prove burndown.
- Independent audits identified the next larger cuts:
  duplicate non-manifest native report lists, remaining BYTES/`boon_cli`
  refresh compatibility outside native handoff, and
  LayoutFrame-keyed render-scene cache compatibility in the playground.
- Fresh focused evidence:
  - `cargo fmt -- --check`: pass.
  - `cargo check -q -p xtask -p boon_report_schema`: pass.
  - `cargo test -q -p xtask refresh_queue -- --nocapture`: pass; 10 passed.
  - `cargo test -q -p boon_report_schema refresh_queue -- --nocapture`:
    pass; 9 passed.
  - `cargo test -q -p boon_report_schema native_gpu_all_schema -- --nocapture`:
    pass; 8 passed.
  - `cargo xtask run-report-refresh-queue --aggregate target/reports/native-gpu-all.json --dry-run --report target/reports/report-refresh-queue.json`:
    pass.
  - `cargo xtask verify-report-schema target/reports/report-refresh-queue.json`:
    pass.
  - `git diff --check`: pass.

### 2026-07-06 - Render-Scene Patch Cache Uses Scene Identity

- Replaced the preview render-scene patch cache's separate patch/base-hash maps
  with one typed cache entry keyed by explicit `render_scene_patch_identity`.
- Patch producers now write `render_scene_patch_identity` into layout proofs and
  cache entries with their base layout hash plus patch hash. The patched layout
  hash is no longer accepted as the render-scene patch cache key.
- The preview render hook now looks up sidecars by `render_scene_patch_identity`
  and keys patched render-scene cache entries by base render-scene hash plus
  patch hash, viewport, and lowering mode.
- Document render snapshot eviction no longer deletes render-scene patch entries
  by unrelated layout hashes; the render-scene patch cache owns its own cap.
- Updated focused tests to assert that patched layout hashes do not retrieve
  render-scene sidecars, while explicit render-scene patch identities do.
- Fresh focused evidence:
  - `cargo check -q -p boon_native_playground`: pass.
  - `cargo test -q -p boon_native_playground render_scene_patch -- --nocapture`:
    pass; 7 passed.
  - `cargo test -q -p boon_native_playground render_scene_sidecar -- --nocapture`:
    pass; 3 passed.
  - `cargo test -q -p boon_native_playground direct_input_overlay_base_scene_lookup_uses_retained_content_revision -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground direct_layout_sidecar_base_scene_lookup_uses_cached_base_layout_hash -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground legacy_replace_code_probe_uses_current_preview_fast_path -- --nocapture`:
    pass; 1 passed.

### 2026-07-06 - Native Refresh Queue Cannot Enter BYTES CLI Lane

- Split refresh-queue command policy by aggregate owner. Native handoff refresh
  entries now accept only xtask verifier commands; `boon_cli` and source replay
  commands are not part of the native branch at all.
- The BYTES/MachinePlan aggregate remains the only refresh owner allowed to use
  `boon_cli` report regeneration.
- Native refresh entries that mention `boon_cli` are rejected before the
  `boon_cli` prebuild lane, removing a stale control-plane coupling that made
  native refresh behavior depend on BYTES tooling.
- Updated the negative native refresh test to assert that rejected source replay
  commands do not set `boon_cli_prebuild.required`.
- Fresh focused evidence:
  - `cargo check -q -p xtask`: pass.
  - `cargo test -q -p xtask refresh_queue -- --nocapture`: pass; 10 passed.

### 2026-07-06 - Direct Preview Scene Patch Path Has No Non-Direct Fallback

- Removed the hardcoded `product_present_fast_path` switch and the stale
  non-direct input-overlay render-scene patch fallback in the preview render
  hook.
- Product rendering now has clearer branches: direct input-overlay scene patch,
  direct layout sidecar scene patch, cached full scene, or explicit full scene
  rebuild. There is no preserved middle path that re-applies input overlays
  after declaring the product fast path active.
- `NativeProductPatchSummary.direct_input_overlay_render_scene_patch_enabled`
  now follows the actual input-overlay scene patch branch instead of a separate
  always-true product fast-path gate.
- Fresh focused evidence:
  - `cargo check -q -p boon_native_playground`: pass.
  - `cargo test -q -p boon_native_playground render_scene_patch -- --nocapture`:
    pass; 7 passed.
  - `cargo test -q -p boon_native_playground render_scene_sidecar -- --nocapture`:
    pass; 3 passed.

### 2026-07-06 - Retired Native Wrapper Gates Deleted

- Deleted non-manifest native wrapper commands that re-read other native reports
  or old screenshot/parity artifacts:
  `verify-native-two-window-content`,
  `verify-native-todomvc-reference-parity`,
  `verify-native-todomvc-input-parity`,
  `verify-native-example-speed`, and
  `verify-native-dev-editor-speed`.
- Removed their command registrations, default report paths, blocker-audit
  command allowlist entries, and stale debug-readiness dependencies.
- Deleted the old TodoMVC screenshot/reference comparator helpers left behind
  by those wrappers. The manifest-owned
  `verify-native-todomvc-physical-reference-parity` remains.
- Updated the architecture audit to assert the retired TodoMVC input parity
  wrapper is deleted instead of checking that its old body had become
  native-only.
- Updated refresh-queue dependency tests to use manifest-owned native reports
  instead of the retired two-window wrapper.
- Fresh focused evidence:
  - `cargo check -q -p xtask`: pass.
  - `cargo test -q -p xtask advertised_xtask_commands_are_unique -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask refresh_queue_selection_expands_report_dependency_graph_edges -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask native_gpu_handoff_manifest -- --nocapture`:
    pass; 3 passed.

### 2026-07-06 - Test Proof Reuse Uses Scene Identity

- Replaced the test-only `render_proof_matches_frame*` helpers with a
  scene-identity proof matcher.
- The app-owned proof reuse regression no longer fabricates a layout-frame hash;
  it validates viewport plus `render_scene_identity_hash`, matching the current
  app-owned proof contract.
- Fresh focused evidence:
  - `cargo check -q -p boon_native_playground`: pass.
  - `cargo test -q -p boon_native_playground app_owned_readback_reuse_requires_matching_render_frame_hash -- --nocapture`:
    pass; 1 passed.

### 2026-07-06 - Product Result Owns Preview Product Frame Slots

- Refactored the preview product proof boundary so it builds one
  `NativeProductFrameResult` first, then copies that result's product frame,
  render graph, present plan, and post-present proof requests into the legacy
  metric slots.
- This keeps the current report schema stable while making the typed product
  result the authoritative owner of ProductFrameGraph identity in the
  playground render hook.
- Added a focused test asserting the metric slots are sourced from the product
  result instead of independently assembled summaries.
- Fresh focused evidence:
  - `cargo check -q -p boon_native_playground`: pass.
  - `cargo test -q -p boon_native_playground product_frame_result_is_single_source_for_metric_slots -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground preview_presentation_plan_owns_product_and_proof_boundary -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground product_render_graph_plan_hash_ignores_workload_and_post_present_proof_requests -- --nocapture`:
    pass; 1 passed.

### 2026-07-06 - Deleted Loose Product Frame Metric Slots

- Removed the duplicate product-frame, render-graph, present-plan, render-graph
  execution, and post-present proof request fields from
  `NativeRenderFrameMetrics`.
- Product-owned frame/report data now enters the app-window path only through
  `NativeProductFrameResult`; loop reports derive `last_product_render_frame`
  and post-present proof request summaries from the committed product frame.
- Deleted the old loose-metrics compatibility test instead of preserving another
  fallback contract.
- Fresh focused evidence:
  - `cargo check -q -p boon_native_app_window -p boon_native_playground`: pass.
  - `cargo test -q -p boon_native_app_window product_frame_commit_uses_typed_product_result -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_app_window product_frame_commit_adds_visible_surface_readback_request_once -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_app_window render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground product_frame_result_is_single_source_for_product_metrics -- --nocapture`:
    pass; 1 passed.

### 2026-07-06 - Deleted Native Aggregate Known-Failure Bypass

- Removed the native aggregate's `acknowledged_known_failure` bypass and the
  stale `idle-wake-custom-projects` known-failing child hook.
- Fresh native children now either pass, count as refresh debt when stale or
  missing, or become true blockers when fresh and failing.
- Removed the aggregate report's acknowledged-known-failure fields instead of
  keeping another compatibility lane.
- Fresh focused evidence:
  - `cargo check -q -p xtask`: pass.
  - `cargo test -q -p xtask native_gpu_handoff_manifest_has_unique_bounded_reports_and_docs_source -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask native_gpu_aggregate_treats_child_reported_stale_dependency_as_refresh_debt -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask native_gpu_handoff_requires_present_floor_report -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask native_gpu_handoff_requires_cells_visible_click_release_report -- --nocapture`:
    pass; 1 passed.

### 2026-07-06 - Simplified Native Stale-Path Ledger Modes

- Removed unused `diagnostic-only`, `fail-fast-alias`, and `removed` mode
  support from the native stale-path ledger verifier.
- The verifier now accepts only `product-forbidden` rows, matching the actual
  ledger and keeping stale-path checks as a release product negative gate rather
  than a generic compatibility framework.
- Added a negative test that rejects obsolete non-product ledger modes.
- Fresh focused evidence:
  - `cargo check -q -p xtask`: pass.
  - `cargo test -q -p xtask stale_path_ledger -- --nocapture`: pass; 3 passed.

### 2026-07-06 - ProductFrameGraph Report Owned By Renderer Metrics

- Added `boon_native_gpu::ProductFrameGraphReport` and made
  `FrameMetrics.product_frame_graph` the single public carrier for renderer
  graph identity, passes, resources, schedule decisions, retained resource
  state, plan hash, and workload hash.
- Deleted the flattened `renderer_render_graph_*` fields from
  `boon_native_gpu::FrameMetrics`.
- Replaced the playground's synthetic product render graph builder with a mapper
  from the renderer-owned graph report into the current native product report
  schema.
- Removed the old synthetic active-scene/product-patch/present graph passes and
  the missing-renderer test fallback.
- Fresh focused evidence:
  - `cargo check -q -p boon_native_gpu -p boon_native_playground -p boon_native_app_window`:
    pass.
  - `cargo test -q -p boon_native_gpu product_frame_graph -- --nocapture`:
    pass; 5 passed.
  - `cargo test -q -p boon_native_playground product_render_graph_uses_renderer_owned_plan_and_workload_hashes -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground product_frame_result_is_single_source_for_product_metrics -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p boon_native_playground preview_presentation_plan_owns_product_and_proof_boundary -- --nocapture`:
    pass; 1 passed.
  - `cargo test -q -p xtask product_render_graph -- --nocapture`:
    pass; 1 passed.

## Next Cuts

1. Delete app-window `render_graph_execution` synthesis and replace it with
   commit/present timing owned by `NativeProductFrameCommit`; graph execution
   now belongs to the renderer-owned `ProductFrameGraphReport`.
2. Keep Cells product-latency and proof-lane reports fresh after each
   architecture cut.

## Completion Rules

- Do not mark the unified goal complete until the active native handoff manifest
  passes from fresh reports or a precise current blocker is documented.
- Do not use human observation, screenshots, browser paths, Xvfb, legacy Ply, or
  COSMIC scraping as native GPU proof.
- Do not keep compatibility shims for deleted paths.
- Commit only coherent checkpoints with focused verification.
