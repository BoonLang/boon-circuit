> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

# Compiler rewrite design notes

| file | area |
| --- | --- |
| spec.md | typing spec draft (static semantics v2) + review |
| checker.md | checker algorithm and data structures + review |
| lowering.md | typed program to MachinePlan (V12-delta variant) + review |
| lowering_alternative.md | earlier variant: MachinePlan v11 at cutover, full inline + hash-consing (the plan follows this one) |
| frontend_session.md | lexer/parser prototype, session, API, consumer port matrix + review |
| examples.md | example breakage census, Theme refactor drafts, verification + review |
| process.md | AGENTS.md/contract/budgets v4/measurement/gates/cutover + review |
| process_alternative.md | earlier variant of the process design |
| original_change_model.md | survey of the original `~/repos/boon` change and effect model (input to L2/L3) |
