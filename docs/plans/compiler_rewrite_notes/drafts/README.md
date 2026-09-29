> **Design drafts and probes, 2026-09-29. Not authority, not built.** Scratch
> work from the design round of `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`,
> kept so its measurements and drafts can be reproduced. Nothing here is part
> of the workspace build. Several Boon files predate owner decisions D14-D36
> and do not follow them. Boon files carry `.bn.txt` (see
> `../change_probes/README.md`); the table and the notes use their `.bn` names.

# Rewrite design drafts

| path | what it is | referenced by |
| --- | --- | --- |
| `fe_proto.rs` | single-pass lexer/parser prototype; source of the "parse NovyWave 2.6 ms, prototype-measured" figure in the plan's §0 | `frontend_session.md` |
| `lexbench.rs` | lexer micro-benchmark used before the prototype | `frontend_session.md` |
| `probes/` | layout-rule, precedence and TEXT probes for D11/D18 | `frontend_session.md`, `spec.md` |
| `feprobes/` | front-end prototype inputs (layout rules L1-L3 and their rejections) | `frontend_session.md` |
| `tspec/`, `tspec2/` | typing-spec probes: one-arm LATEST, self LATEST, WHILE over events, cycles, THEN over state | `spec.md` |
| `checker/` | checker probes and the TodoMVC diagnostics dump (`todo_diag.json`) | `checker.md` |
| `lowering/` | HOLD and unused-code probes, document analysis script | `lowering.md` |
| `migration/todo_theme_new/` | the "themes as data" refactor draft for TodoMVC (D8) | `examples.md`, plan §5.1 |
| `migration/*.py`, `mig_*.py`, `selfref*.py`, `hashcons.py` | census scripts behind the breakage counts (unions, recursion, self-references, style collisions, unused parameters, hash-consing estimate) | `examples.md`, plan §5.1 |
| `migration/*.bn` | theme, profile and self-reference probes | `examples.md` |
| `compiler.v4.toml`, `compiler_edits.toml` | draft of budgets v4 and the edit classes for the warm benchmark | `process_alternative.md`, plan §9.3 |
| `selfref_hits.txt` | output of the self-reference census | `examples.md` |
