> **Probe sources, 2026-09-29. Not authority, not built.** Scenario probes run
> against the old compiler/runtime while analysing L2/L3; the findings are in
> `../change_and_effects.md` (probe names p1-p9, probe2-5). Plan dumps were not
> kept.

Boon files here and in `../drafts/` carry the extension `.bn.txt`, so the
workspace parser corpus check (`crates/xtask/src/language_surface.rs`, which
parses every `.bn` file in the workspace) never picks up documentation. The
notes refer to them by their original `.bn` names. To run one, copy it to a
`.bn` file outside `docs/` and point the matching `.scn` at it.
