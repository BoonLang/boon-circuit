# `/goal` Prompt

```text
/goal Implement docs/plans/BOON_CIRCUIT_SIMPLIFICATION_AND_NATIVE_RECOVERY_PLAN.md completely from the current HEAD.

Work in mandatory large ownership slices, not micro-optimization loops. Delete
obsolete implementations rather than renaming, quarantining, aliasing, or
preserving compatibility paths. Temporary compile breakage is acceptable inside
a slice; every slice commit must compile. Run targeted checks only at slice
boundaries and regenerate expensive native reports only after the architecture
stabilizes. Use compact report summaries and no Python.

Use subagents for independent ownership slices and final adversarial review, but
keep one authority for each subsystem. Do not add Cells/example-specific runtime,
compiler, renderer, windowing, or verifier behavior.

Required result:
- MachinePlan is the only executable artifact and PlanExecutor Session is the
  only execution owner;
- runtime emits typed RuntimeTurn/DocumentPatch data and product crates contain
  no JSON state/report path;
- the external BoonLang/app_window fork provides the generic asynchronous
  browser-compatible event stream and vendor/app_window is gone;
- desktop, preview, and dev use one native event-to-frame transaction;
- normal frames contain no readback/report work, while explicit asynchronous
  WGPU proof is linked to exact frame identity;
- report schema v1, duplicate verifiers/oracles, the entire executable
  3D/manufacturing island, and unnecessary tests are deleted;
- tracked Rust is <=240,000 lines, tests <=32,000, playground <=32,000, xtask
  <=25,000, and runtime plus executor <=42,000.

Do not mark the goal complete until all structural scans, workspace tests,
generic example scenarios, fresh manifest-backed native gates, report schemas,
performance/idle budgets, and an adversarial subagent review pass. Then launch
the release playground in the COSMIC background workspace. Physical native input
is not declared fixed until the user confirms dev hover/click/wheel/keyboard,
TEST, Counter, and Cells behavior. Commit phase checkpoints when complete; do not
push without an explicit request.
```
