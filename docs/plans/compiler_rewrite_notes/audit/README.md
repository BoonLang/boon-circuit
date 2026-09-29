> **Audit input, 2026-09-28/29. Not authority.** These are the audits that the
> compiler rewrite plan (`docs/plans/BOON_COMPILER_REWRITE_PLAN.md`) was built
> from, kept as the record of what is wrong with today's compiler and why. Each
> file was written by one audit agent reading the tree read-only. Where a file
> disagrees with the plan, the plan wins. File:line references point at the
> tree as of 2026-09-29. Paths shown as `<session scratchpad, not kept>` refer
> to throwaway measurement files that were not preserved.

# Compiler audit (2026-09-29)

## Subsystem audits of today's compiler

| file | scope |
| --- | --- |
| frontend.md | source loading, parser, line merging, identity digests, session reparse |
| typecheck_orchestration.md | everything between parse and semantic: owner projection, kernel packing, sessions, materialization, sealing |
| kernel_compile.md | kernel compile side: checked definitions to the constraint program, per-call-path instantiation |
| kernel_solver.md | the retracting solver, requirements, receipts |
| semantic.md | elaboration to CanonicalProgramCoreV2, graphs, receipts, digests |
| backend.md | IR, verify handoff, plan sealing and hashing, MachinePlan and document backends |
| representations.md | census of the ~62 program representations and 27+ conversions |
| engineering.md | hashing, allocation, serialization, profiles, crate boundaries, env reads |
| integration_warm.md | what runs per keystroke in the playground; the missing warm path |
| profile.md | CPU attribution of the whole pipeline (gdb sampler, before samply worked) |
| language.md | the static semantics the compiler must implement, inherent vs incidental cost |
| process_history.md | AGENTS.md rules, budgets, oracles and plan history that shaped the architecture |

## External research

| file | topic |
| --- | --- |
| inference_research.md | fast inference for structural languages (Roc, Elm, OCaml, Simple-sub, tsgo, Sorbet, …) |
| incremental_research.md | incremental and warm compilation for sub-frame editor latency |
| dod_research.md | data-oriented compiler engineering and per-phase targets (Zig, Carbon, rustc, …) |
| verification_research.md | keeping self-verification, digests and determinism off the hot path |

## Cross-checks

| file | content |
| --- | --- |
| claim_verification.md | independent verification of every headline claim from the audits (confirmed / partially confirmed / refuted, with evidence) |
| plan_draft_critique.md | adversarial critique of the first plan draft; the plan was rewritten to fix what it found |
| owner_decisions_d1_d13.md | the owner's first-round answers, quoted, as recorded before they moved into the plan's §1 |

`tools/gdbprof.py` is the unprivileged gdb/ITIMER_PROF sampler used for
`profile.md` while `perf_event_paranoid` was 2; samply is the normal tool now.
