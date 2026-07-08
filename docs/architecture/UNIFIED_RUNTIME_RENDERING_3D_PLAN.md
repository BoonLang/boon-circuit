# Unified Runtime, Rendering, 3D, and Manufacturing Plan

**Status:** Active architecture summary
**Updated:** 2026-07-06
**Native GPU contract:** `docs/architecture/NATIVE_GPU_PIPELINE.md`
**Execution ledger:** `docs/plans/UNIFIED_RUNTIME_RENDERING_3D_PROGRESS.md`
**Goal prompt:** `docs/plans/GOAL_PROMPT.md`

This file is intentionally compact. Older versions mixed current architecture,
historical checkpoints, implementation notes, and speculative migration detail
into thousands of lines. Keep the active contract here short enough that a new
agent can read it before making code changes.

## Product Goal

Boon source compiles once into a typed static plan. Runtime changes keep stable
identity and flow incrementally through document state, layout, rendering, GPU
memory, accessibility, 3D world state, and manufacturing outputs.

```text
Boon source
  -> parser AST
  -> typed IR
  -> MachinePlan
  -> PlanExecutor memories
  -> semantic/document/layout/render/solid deltas
  -> retained product surfaces
```

There must be one semantic execution path. Verification may compare or inspect
old concepts only as negative evidence that they are absent from the product
path.

## Current Authority

For native window and native proof work, `NATIVE_GPU_PIPELINE.md` is the source
of truth. This document only defines the broader system direction and cleanup
rules.

When documents conflict, use this order:

1. Current user instruction and `AGENTS.md`.
2. `docs/architecture/NATIVE_GPU_PIPELINE.md` for native GPU behavior.
3. This summary for unified runtime/rendering/manufacturing direction.
4. Focused progress ledgers and verifier schemas.
5. Historical docs only as context, never as implementation authority.

## Non-Negotiables

- Normal execution is `MachinePlan` plus `PlanExecutor`.
- Do not recreate pre-PlanExecutor runtime shells, engine selection flags,
- No TodoMVC-, Cells-, file-name-, example-name-, or source-text-specific
  runtime/compiler/renderer branches.
- LIST rows are keyed memories with generations, not cloned runtime graphs.
- Strings are debug labels or stable external identifiers, not hot dispatch
  structures when typed IDs are available.
- Native/browser rendering should share one retained WGPU renderer model.
- Browser DOM is an accessibility/input bridge, not a second visual renderer.
- App-owned WGPU readback is proof. Whole-desktop screenshots, Xvfb, browser
- Product latency excludes proof/readback/report writing, but proof artifacts
  must be linked to the presented frame by explicit frame evidence identity.
- Delete obsolete paths. Do not quarantine, alias, or preserve compatibility
  fields unless a current product contract explicitly requires them.

## Active Runtime Shape

The runtime path is:

```text
Typed source -> MachinePlan -> PlanExecutor -> keyed memories -> semantic deltas
```

Required properties:

- root and indexed fields use currentness barriers before values are exposed;
- list lookup uses typed/indexed routes where the plan provides them;
- demand-current fields do not run whole-grid startup work;
- formula/list dependencies are generic runtime features, not Cells hacks;
  comparison payloads.

negative guards only after the guard has served its purpose. Product code should
not contain fallback runtime execution.

## Active Rendering Shape

The product rendering path is:

```text
PlanExecutor deltas
  -> retained document state
  -> retained layout state
  -> ProductRenderGraph / PresentPlan
  -> WGPU resources
  -> presented surface
```

Required properties:

- select, hover, focus, text editing, caret, and scrolling patch retained state;
- no full document relower, full layout rebuild, or full render-scene rebuild on
  normal visible interactions;
- render graph nodes own stable inputs, outputs, resource lifetime, and timing;
- proof/readback/report work is a separate lane with bounded backpressure;
- dev-window HUD/stats consume cached scalar snapshots only.

The render graph is not optional. It is the desired product boundary because it
matches Boon's dataflow model and makes resource ownership, invalidation,
testing, and proof identity explicit.

## Native Performance Target

Cells is the current stress case because it combines sparse lists, formulas,
selection, text input, and scrolling. The acceptance target is still generic:

- accepted input to visible product update p95 <= 16.7 ms;
- scroll frame p95 <= 16.7 ms;
- bounded max outliers, reported with root cause;
- zero full-grid recompute, full relower, full layout rebuild, full render-scene
  rebuild, or product proof/readback blocking on normal interactions;
- proof/readback latency reported separately and linked to frame identity.

Recent passing Cells reports prove the current retained path can be fast, but
they do not finish the architecture. The remaining work is to make the render
graph and PlanExecutor-backed path the simple default, then delete old harness
and report paths that hide or confuse the product path.

## 3D and Manufacturing Direction

The 3D source of truth is a typed `SolidGraph` / `AssemblyGraph`, not a visual
mesh.

```text
SolidGraph / AssemblyGraph
  -> visual compiler: fast retained GPU geometry
  -> manufacturing compiler: deterministic material regions / tool output
```

Rules:

- visual meshes are disposable caches;
- printing does not consume screen meshes silently;
- manufacturing output must be deterministic, tolerance bounded, closed, and
  material aware;
- renderer speed work must not couple manufacturing semantics to WGPU details.

## Cut List

Prefer large simplifying cuts over endless local tuning:

   PlanExecutor coverage exists.
2. Replace fallback report fields with explicit product/recovery/proof names, or
   delete them if they only served migration.
3. Delete stale verifier conversions that translate old report shapes into new
   ones.
4. Remove old proof aliases instead of accepting both scoped and unscoped proof.
5. Collapse overgrown plan files into active contracts plus current ledgers.
6. Move repeated JSON/proof plumbing into typed structs when it reduces report
   ambiguity and token-heavy debugging.
7. Delete broad recovery paths when retained patches cover the same interaction
   deterministically.

## Verification Policy

Before claiming handoff readiness:

```bash
cargo fmt --check
git diff --check
cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json
```

For focused implementation slices, run the smallest deterministic checks that
prove the edited boundary, then update the progress ledger with:

- what was deleted;
- what product path remains;
- which checks ran;
- any fresh blocker with code-level root cause.

Do not spend tokens dumping full JSON reports. Summarize with focused queries and
store detailed evidence in repo-local reports.
