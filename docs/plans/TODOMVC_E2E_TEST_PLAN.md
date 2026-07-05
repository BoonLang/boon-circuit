# Retired: TodoMVC Legacy E2E Plan

This plan is retired. TodoMVC verification no longer uses legacy
`headed-ply`, `ply-headless`, operator-e2e, manual JSON, browser, or Xvfb proof
paths.

Use the native GPU handoff manifest and `NATIVE_GPU_PIPELINE.md` for current
verification. If TodoMVC needs new coverage, add it to the manifest-backed
native GPU path or the PlanExecutor scenario path, not to the removed Ply
harness.
