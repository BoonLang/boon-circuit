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
| PlanExecutor authority | In progress | Legacy runtime ambiguity is reduced but not fully gone. Continue removing fallback paths instead of adding comparison-only harness code. |
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

## Next Cuts

1. Split or delete manifest coverage logic that conflates full semantic scenario
   coverage with the smaller native input proof subset.
2. Remove remaining legacy `LoadedRuntime`/`GenericScheduledRuntime` fallback
   routes where `PlanExecutor` is the intended authority.
3. Delete duplicate report/schema refresh paths that only preserve stale
   fingerprints or old comparison contracts.
4. Re-run focused native Cells product-latency and proof-lane reports after the
   harness is lean enough that reports are trustworthy.

## Completion Rules

- Do not mark the unified goal complete until the active native handoff manifest
  passes from fresh reports or a precise current blocker is documented.
- Do not use human observation, screenshots, browser paths, Xvfb, legacy Ply, or
  COSMIC scraping as native GPU proof.
- Do not keep compatibility shims for deleted paths.
- Commit only coherent checkpoints with focused verification.
