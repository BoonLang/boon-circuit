# Unified Runtime, Rendering, 3D, and Manufacturing Status

This is the compact live status for the unified goal. Historical step-by-step
logs were removed from this file on 2026-07-06 because they made every planning
and review pass expensive while adding little current signal. Use git history
for old evidence.

## Active Contracts

- Native GPU contract: `docs/architecture/NATIVE_GPU_PIPELINE.md`
- Unified architecture: `docs/architecture/UNIFIED_RUNTIME_RENDERING_3D_PLAN.md`
- Goal prompt: `docs/plans/UNIFIED_IMPLEMENTATION_GOAL_PROMPT.md`
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
| ProductFrameGraph | In progress | Present/proof split exists, but renderer ownership and retained resource scheduling still need larger cuts. |
| Test harness cleanup | In progress | Old proof aliases, stale report acceptance, source-replay refresh debt, and duplicate verifier paths remain high-value deletion targets. |
| 3D/manufacturing | In progress | Existing work remains useful, but it should not distract from runtime/render/harness cleanup until the active goal is stable. |

## Latest Checkpoints

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

1. Run compact native/BYTES aggregate summaries and use fresh taxonomy to choose
   refresh queue work versus true product blockers.
2. Audit native layout/input recovery paths that still report generic fallback
   labels; either prove them as explicit retained-layout recovery or rename/cut
   them so they cannot be confused with runtime fallback.
3. Delete duplicate report/schema refresh paths that only preserve stale
   fingerprints or old comparison contracts.
4. Move the current linear retained `ProductFrameGraph` toward a real
   renderer-owned dirty-resource scheduler.
5. Re-run focused native Cells product-latency and proof-lane reports after the
   harness is lean enough that reports are trustworthy.

## Completion Rules

- Do not mark the unified goal complete until the active native handoff manifest
  passes from fresh reports or a precise current blocker is documented.
- Do not use human observation, screenshots, browser paths, Xvfb, legacy Ply, or
  COSMIC scraping as native GPU proof.
- Do not keep compatibility shims for deleted paths.
- Commit only coherent checkpoints with focused verification.
