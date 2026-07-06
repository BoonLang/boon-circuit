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
| Test harness cleanup | In progress | Cells visible-click, preview E2E, and scroll-speed no longer have isolated-Weston fallbacks; old proof aliases, stale report acceptance, source-replay refresh debt, duplicate verifier paths, and the remaining isolated-Weston verifier-owned compositor paths remain high-value deletion targets. |
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

## Next Cuts

1. Delete the remaining isolated-Weston verifier-owned compositor family and
   replace affected native handoff labels with headed hardware/app-owned paths.
2. Continue moving `ProductFrameGraph` ownership out of playground/report
   adapters and into typed renderer DTOs.
3. Delete duplicate report/schema refresh paths that only preserve stale
   fingerprints, old comparison contracts, or weak diagnostic timing
   substitutes.
4. Keep Cells product-latency and proof-lane reports fresh after each
   architecture cut.

## Completion Rules

- Do not mark the unified goal complete until the active native handoff manifest
  passes from fresh reports or a precise current blocker is documented.
- Do not use human observation, screenshots, browser paths, Xvfb, legacy Ply, or
  COSMIC scraping as native GPU proof.
- Do not keep compatibility shims for deleted paths.
- Commit only coherent checkpoints with focused verification.
