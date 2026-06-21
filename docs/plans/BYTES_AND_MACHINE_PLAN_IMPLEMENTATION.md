Implement the BYTES language/runtime feature and the typed MachinePlan architecture in /home/martinkavik/repos/boon-circuit. Follow docs/plans/BYTES_AND_MACHINE_PLAN_IMPLEMENTATION.md as the primary contract. AGENTS.md, current architecture decisions, runtime/language/LIST/delta docs, native GPU contract, report-schema rules, and all existing verification requirements remain binding. Do not commit or push unless explicitly asked.

Work from current HEAD and preserve unrelated user changes. This is an incremental migration, not a rewrite. First inspect the actual current code and update symbol/path assumptions in the plan if they have moved, but do not silently reduce scope or weaken acceptance criteria.

Primary outcomes:
1. Add a real Boon BYTES type with dynamic, inferred-fixed, and explicit-fixed forms:
   - BYTES {}
   - BYTES[__] { 16uFF, ... }
   - BYTES[N] { ... }
   - explicit-base byte literals in range 0..255
   - nested composition/flattening
   - exact fixed-size checking and zero-filled BYTES[N] {}
   - explicit TEXT/BYTES encoding boundaries
   - explicit endian for multi-byte numeric reads/writes
2. Introduce crates/boon_plan and a deterministic verified MachinePlan between semantic IR and execution.
3. Replace string-path/AST-based accepted execution with typed IDs, typed storage, source routes, dirty plans, typed operation regions, deterministic commit rules, and semantic delta plans.
4. Add a CPU PlanExecutor in dual-path mode. Keep legacy execution until concrete TodoMVC/Cells state, delta, source-binding, conflict, and error parity is proven.
5. Refactor examples semantically:
   - physical TodoMVC asset ingestion uses File/read_bytes and explicit encoding/decoding;
   - Cells keeps user formulas as TEXT but converts once to ASCII BYTES for byte-oriented grammar scanning;
   - ordinary UI labels, todo titles, placeholders, URLs, and diagnostics remain TEXT.
6. Add honest report-schema/xtask verification, negative fixtures, release benchmarks, allocation/copy counters, and independent subagent audits.

Non-negotiable architectural rules:
- No central reducer, runtime graph cloning, per-row graph construction, actor/channel-per-value engine, or Differential Dataflow core.
- No app-specific Rust behavior for TodoMVC, Cells, or BYTES examples.
- SOURCE remains the structural external-input abstraction.
- Hidden keys/generations remain below Boon source and cannot become Boon values.
- Renderers continue to consume semantic deltas; renderer-specific patches do not enter semantic IR or MachinePlan.
- Current native GPU rendering/surface work is separate from future GPU compute. Do not derail or falsely complete native GPU gates.
- Human-readable strings and AST spans may exist in DebugMap/reports, but executable MachinePlan operands must be typed IDs/refs.
- Accepted plan runs must prove runtime_ast_eval_count = 0, executable_string_path_count = 0, unknown_plan_op_count = 0, graph_rebuild_count = 0, and graph_clones_per_item = 0.
- Do not delete the legacy path in the same milestone that introduces PlanExecutor.
- Do not weaken existing budgets or report gates just to obtain green output.
- Do not fabricate visible Wayland, manual, GPU, hardware, or operator evidence. Mark unavailable checks blocked with exact reasons.

Implement phase by phase and keep a progress ledger in docs/plans/BYTES_AND_MACHINE_PLAN_PROGRESS.md. For every phase record:
- base/current commit
- files changed
- decisions/ADRs
- commands run and exact status
- report paths
- open findings/blockers
- whether the phase exit gate is truly satisfied

Required execution sequence:

PHASE 0 — BASELINE
- Read AGENTS.md and all relevant current docs.
- Record git status, HEAD, toolchain, existing failures, current TodoMVC/Cells semantic outputs, reports, and release benchmarks.
- Save a machine-readable baseline under target/reports/bytes-plan/.
- Do not misclassify pre-existing or environment-dependent failures.

PHASE 1 — PLAN BOUNDARY
- Add crates/boon_plan to the workspace.
- Define PlanVersion, MachinePlan, TargetProfile, typed plan IDs, constants, source routes, storage layout, regions/ops, dirty plan, commit plan, delta plan, capability summary, DebugMap, deterministic serialization/hash, and verifier.
- Add a compile-to-plan entry point and dump-plan developer command.
- Keep existing runtime default.

PHASE 2 — BYTES PARSER
- Add ByteLiteral and BytesLiteral AST forms with exact spans.
- Parse BYTES {}, BYTES[__] {}, BYTES[N] {}, explicit-base byte literals, nesting, and multiline/comment forms.
- Reject malformed bases/digits, >255 values, unsupported v1 size expressions, and unterminated forms with targeted diagnostics.
- BYTES must be included in expression coverage and may not become Unknown.

PHASE 3 — BYTES TYPE SYSTEM
- Add Type::Bytes with fixed/dynamic length semantics.
- Resolve inferred lengths before MachinePlan lowering.
- Enforce fixed/dynamic composition and size matching.
- Add required builtin signatures:
  Bytes/length, Bytes/is_empty, Bytes/get, Bytes/set, Bytes/slice, Bytes/take, Bytes/drop, Bytes/concat, Bytes/equal, Bytes/find, Bytes/starts_with, Bytes/ends_with, Bytes/zeros, Text/to_bytes, Bytes/to_text, Bytes/from_hex, Bytes/to_hex, Bytes/from_base64, Bytes/to_base64, Bytes/read_unsigned, Bytes/read_signed, Bytes/write_unsigned, Bytes/write_signed.
- Multi-byte operations require Little or Big endian; byte_count is limited to 1,2,4,8 in v1.
- Do not implicitly accept TEXT inside BYTES constructors. Emit an explicit Text/to_bytes suggestion.
- Do not implement BITS interop in this pass.
- Reuse one coherent existing bounds/conversion failure convention; document the decision in docs/architecture/BYTES_SEMANTICS.md and test it.

PHASE 4 — SEMANTIC IR
- Add byte constants, initial values, typed byte operations, source-payload typing, source spans, and constant folding.
- Keep large byte constants deduplicated and report them by IDs/hashes/lengths, not repeated blobs.
- Keep old human-readable debug tables temporarily, but do not use them as MachinePlan executable operands.

PHASE 5 — REAL MACHINE PLAN LOWERING
- Lower current SOURCE/HOLD/THEN/WHEN/LATEST/LIST/aggregate/delta semantics into typed MachinePlan structures.
- Replace string executable references with typed ValueRef/IDs.
- Create target-specific storage layout, scalar/indexed dirty routes, commit groups, and delta schemas.
- Add plan verification and normalized plan reports.
- TodoMVC, Cells, and BYTES fixtures must lower with executable_string_path_count=0, runtime_ast_dependency_count=0, and unknown_plan_op_count=0 for their supported surface.

PHASE 6 — CPU PLAN EXECUTOR AND PARITY
- Add typed storage and a PlanExecutor that runs source routing, dirty scalar/indexed regions, candidate resolution, tick-boundary commit, and semantic deltas.
- Add --engine legacy|plan|compare or an equivalent API.
- Compare concrete normalized state, list validity/generation/order, source bindings, semantic deltas, conflicts, errors, and relevant cause data.
- Avoid duplicating uncontrolled external side effects in compare mode.
- Implement all existing non-BYTES operations needed by TodoMVC and Cells without app-specific branches.

PHASE 7 — BYTES RUNTIME/STORAGE
- Intern constants.
- Preallocate fixed root/indexed byte banks according to StorageLayout.
- Use a measured arena/bytes::Bytes/Arc representation for dynamic bytes without leaking Rust types into semantics.
- Implement v1 operations safely and deterministically.
- Add generic File/read_bytes and File/write_bytes at the host boundary where appropriate.
- Add byte source payload and semantic delta support.
- Prove no Rust panic/OOB/uninitialized access/host-endian dependence.
- Bounded fixed-byte normal ticks must allocate zero byte buffers after warm-up.

PHASE 8 — EXAMPLES
- Refactor examples/todo_mvc_physical/BUILD.bn to ingest bytes and encode/decode explicitly at the boundary.
- Refactor examples/cells/formula.bn to scan ASCII BYTES after one explicit conversion while preserving visible behavior.
- Add focused BYTES examples and positive/negative scenarios.
- Do not convert human-readable text merely to exercise BYTES.

PHASE 9 — VERIFICATION AND PERFORMANCE
- Add repository-native xtask/report-schema gates for BYTES language/runtime, negative fixtures, MachinePlan architecture, TodoMVC/Cells parity, no-fallback counters, storage/profile evidence, example refactors, release performance, and stale/tampered evidence rejection.
- Suggested command names are in the implementation plan; adapt only to current xtask conventions and document exact final commands.
- Reports must bind commit, source, target profile, plan hash/version, build profile, and artifacts.
- Run release measurements against the Phase 0 baseline. Report p50/p95/p99/max, sample count, warm-up, allocations, copies, rows/regions touched, compile/lower time, and runtime tick time.
- Claim speedups only when reproduced. Report regressions honestly.

PHASE 10 — DEFAULT SWITCH
- Switch default execution to PlanExecutor only after all required parity, negative, no-fallback, performance/regression, report-schema, readiness/finality, and existing example gates pass.
- Keep explicit legacy mode temporarily and prove plan reports did not call it.
- Remove legacy AST/string execution only in a later cleanup after no accepted path depends on it.

Subagent requirements:
- Use independent subagents throughout, not only at the end.
- Implementers should own disjoint areas/worktrees where practical.
- No implementer may be the sole reviewer of its own work.
- Required independent roles:
  1. architecture auditor
  2. BYTES language auditor
  3. plan/runtime parity auditor
  4. performance/storage auditor
  5. adversarial verification auditor
  6. example/docs auditor
- Every reviewer must inspect actual code/diffs and run commands independently. Do not accept another agent's summary as evidence.
- Save reviewer reports under target/reports/bytes-plan/subagents/ using the schema in the implementation plan.
- Resolve every critical/high finding. Resolve or explicitly defer medium findings with rationale, owner, and milestone.
- The adversarial reviewer must prove aggregate gates fail for stale hashes, edited success flags, missing reports, unknown/fallback execution, AST/string execution, unbounded bytes reported as hardware-safe, and fake benchmark profiles.

Required baseline/final checks include the current repository's real commands, at minimum:
- cargo fmt --all -- --check
- cargo check --workspace --all-targets
- cargo test --workspace --no-fail-fast
- cargo run -p boon_cli -- dump-ir examples/todomvc.bn
- cargo run -p boon_cli -- dump-ir examples/cells.bn
- cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn
- cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn
- cargo xtask verify-report-schema
- cargo xtask audit-goal-readiness

Also implement and run the new BYTES/MachinePlan gates described in docs/plans/BYTES_AND_MACHINE_PLAN_IMPLEMENTATION.md, including a fresh aggregate --check-existing gate. Test advertised xtask command registration and negative/tamper behavior.

Work style:
- Begin with the two vertical slices defined in the plan: a BYTES[4] literal through PlanExecutor, and one existing TodoMVC source route through typed MachinePlan parity.
- Integrate in small reviewable phases; do not land a giant speculative rewrite.
- Prefer fixes in parser/typechecker/IR/plan/runtime over Boon example workarounds.
- Keep debug/explainability while removing debug strings from execution.
- When current code differs from this prompt, preserve the architectural invariant and document the exact adaptation.
- If a phase is incomplete, say so. Do not mark it complete because scaffolding or a free-form report exists.

Final response requirements:
1. State exactly which phases are complete, partial, blocked, or deferred.
2. List changed files and architecture decisions.
3. List every command run with pass/fail/blocked status.
4. Link/report all generated evidence and subagent findings.
5. Give concrete parity and performance numbers; do not use unsupported adjectives.
6. Identify unresolved findings and environment/manual checks honestly.
7. Do not claim custom hardware, GPU compute, native visible behavior, or human approval unless actually verified by the required evidence.