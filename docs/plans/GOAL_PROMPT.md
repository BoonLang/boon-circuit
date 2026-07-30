# `/goal` Prompt

```text
/goal Complete the unified Boon language, verified compiler, universal packed
runtime, BoonConsole with the first Boon-designed RV32I processor and exact
interpreted app.wasm parity, mature web-application stack, and example portfolio
objective from the current HEAD.

Read AGENTS.md and these contracts completely before editing:

- docs/plans/steps.md
- docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md
- docs/plans/BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md
- docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md
- docs/plans/TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md
- docs/plans/BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md
- docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md
- docs/plans/BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md
- docs/plans/BOON_CONSOLE_IMPLEMENTATION_PLAN.md
- docs/plans/NOVYWAVE_BOON_REWRITE_PLAN.md
- docs/plans/FJORDPULSE_FULL_STACK_BOON_REWRITE_PLAN.md
- docs/plans/BOON_FIRST_RISCV_PROCESSOR_PLAN.md
- docs/plans/BOON_EXAMPLE_PORTFOLIO_PLAN.md
- docs/architecture/LANGUAGE_SEMANTICS.md
- docs/architecture/BYTES_SEMANTICS.md
- docs/architecture/RUNTIME_MODEL.md
- docs/architecture/LIST_MODEL.md
- docs/architecture/DELTA_PROTOCOL.md
- docs/architecture/BOON_CONSOLE.md
- docs/architecture/NATIVE_GPU_PIPELINE.md
- docs/architecture/native_gpu_handoff_manifest.json

Goal replacement rule:

- This prompt replaces the pre-foundations unified product goal. Do not resume
  an agent goal that captured the older prompt. Preserve its compatible commits,
  retire that paused goal, and start a fresh goal from this file.
- Git history is the archive for the replaced prompt. Do not restore it as a
  second active goal, compatibility plan, or alternate authority.
- `steps.md` fixes execution order. Individual plans remain authoritative for
  their own semantics, invariants, reports, budgets, and acceptance criteria.

Authority and conflict rules:

- `BOON_LANGUAGE_FOUNDATIONS_PLAN.md` owns the target public value algebra,
  Tags-only truth, private absence/fault channels, exact `NUMBER`, one-based
  positions, `BITS[N]`, `LIST`/`SET`/`MAP` authorities, matching, `FLUSH`,
  bounded repetition, target eligibility, and its flag-day deletion rules.
- `TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md` owns annotation-free structural
  inference, `CheckedProgram` type/render metadata, full-AST checking, flow
  typing, diagnostics, and removal of parser-owned semantic side channels.
- The current architecture documents describe executable behavior until the
  corresponding foundations phase lands. A plan is not permission for a parser,
  runtime, persistence layer, or target to partially accept future semantics.
  Once a phase lands, update the architecture documents in the same flag-day
  change and delete the replaced behavior.
- `BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md` owns calls, OUT,
  final-position PASS, order-independent lexical bindings, contextual
  functions, ownership, migration, and its Clear End Condition.
- The final compiler artifact order is:

    ParsedProgram
    -> CheckedProgram
    -> SemanticProgram
    -> ContractVerifiedProgram
    -> ErasedProgram
    -> MachinePlan
    -> PhysicalPlan or CoreHardwareIR

  `boon_typecheck` owns `CheckedProgram`; `boon_semantic` owns contextual
  expansion, OutNet validation, semantic ownership, typed views, dependency
  manifests, and proof obligations; `boon_verify` produces the mandatory
  `ContractVerifiedProgram`; `boon_ir` erases WHERE/OUT/PASS/transparent
  wrappers into `ErasedProgram`. No executable backend may bypass verification
  or consume parser/checked-only semantics.
- `TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md` owns filtering, stable
  ordering, order-chain provenance, take/page semantics, view-instance cursors,
  physical access planning, hot native/Wasm indexes, removal of `List/query`
  and `List/query_prefix`, deletion of the old `boon_query`/
  `boon_query_redb` worlds, and its Clear End Condition.
- `BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md` owns the two public WHERE forms,
  proof obligations, assurance, proof reports, persisted-invariant activation,
  and the required `ContractVerifiedProgram` gate. Its proof model consumes the
  foundations plan's exact Number, Tags, BITS, collection, and FLUSH semantics.
- `BOON_PERSISTENCE_ARCHITECTURE_PLAN.md` owns stable semantic identity, atomic
  turns, migration, canonical durable DTOs, restore, and durability evidence.
  Physical packed layouts never become persisted semantic identity.
- `BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md` owns executable physical
  layouts, dense IDs/tables, packed runtime cells, arenas, currentness,
  dependency storage, collection kernels, KernelIR, boundary materialization,
  native/Wasm parity, and full flag-day deletion of the old execution world.
  It is the universal runtime architecture, not a RISC-V-only prelude. Phase 5
  may land compatible fixed-width/dense hardware prerequisites without claiming
  the packed plan's software phases or Clear End Condition; Phase 7 completes
  that universal owner.
- The Client/Session/Server contract in this prompt supersedes conflicting
  older Client/Server pairing, ProgramRole::Document, internal HTTP/JSON,
  SessionInfo, and transport details.
- The NovyWave plan remains authoritative for product behavior and acceptance,
  except language/compiler details superseded above.
- The FjordPulse plan remains authoritative for pinned revision
  dd6e750c2ca9dec3041f66ceda31d30379d4027a, 108 stories, 340 scenarios,
  product behavior, budgets, security, persistence, Live mode, deployment, and
  its Clear End Condition. Reconcile its package, data, Number, truth, proof,
  and physical-access language with the authorities above without weakening
  product requirements.
- `NATIVE_GPU_PIPELINE.md` and its manifest remain authoritative for native
  input, WGPU proof, performance evidence, and native handoff reports.
- `BOON_CONSOLE.md` owns console scope, all-peripheral physical composition,
  target-neutral ConsolePort meaning, interpreter-first exact-app-Wasm parity,
  event/output ordering, bridge behavior, reset/persistence, safe state,
  accessibility, board-profile boundaries, and readiness.
- `BOON_CONSOLE_IMPLEMENTATION_PLAN.md` owns the executable console sequence,
  board/interpreter experiments, planned implementation owners, reports,
  budgets, deletion gates, and hardware-in-the-loop Clear End Condition.
- `BOON_FIRST_RISCV_PROCESSOR_PLAN.md` owns reusable
  CoreHardwareIR/TargetHardwareIR, RV32I, cycle simulation, generated RTL,
  ISA/formal verification, and processor FPGA evidence. It does not own the
  console, standalone app Wasm, bridge, or a game projection.
- Boon Orchard owns fiction and game vision only. Game implementation is not
  part of this goal or its completion conditions. A separately specified,
  user-approved future goal may consume the real proof bundle after BoonConsole
  readiness.
- `BOON_EXAMPLE_PORTFOLIO_PLAN.md` owns the full post-processor portfolio. Its
  selected examples are also regression fixtures during earlier phases.
- There is no public `MEMORY` keyword. Self-hosting and BoonInBoon are not goals,
  prerequisites, hidden acceptance conditions, or reasons to distort the
  language/compiler/runtime architecture.

Current checkpoints to preserve and audit rather than redo:

- commit 44c011a added substantial Client/Session/Server compiler/runtime/host
  infrastructure, positional CBOR framing, SessionInfo intrinsics, immutable
  bytes::Bytes values, bounded file/content effects, Wellen integration, real
  VCD/FST/GHW fixtures, and real NovyWave effect/data paths;
- commit a12c9e1 added a large partial CheckedProgram/OUT/typed-collection
  lowering checkpoint;
- commit 4d9863e deleted the reflective query collection,
  QueryCollectionState, `boon_query`, and `boon_query_redb`, and introduced the
  compiler-derived typed access kernel in `boon_list_access`;
- commit 18ad761 advanced ErasedProgram-only consumers, expression arenas,
  complete owner ancestry, retained windows, explicit deep evaluation, and
  generic currentness;
- these commits are implementation evidence, not proof that any current plan's
  Clear End Condition passes;
- preserve the one-world query cutover and useful generic implementation, but
  reconcile it with the final exact value algebra and verified semantic spine;
- native reports predating later semantic, packed, source, or product changes
  are stale and cannot prove the final revision;
- Cells previously met interaction budgets, but every semantic/compiler/runtime
  cut must preserve and freshly prove them;
- FjordPulse currently has no basis for weakening the 108-story/340-scenario
  acceptance inventory. Only its two explicitly deferred backup/restore
  automation scenarios may retain that final status.

Execution strategy:

- Work in the exact order in `steps.md` and the phases below.
- Work in large coherent ownership slices. Temporary compile breakage is
  acceptable inside a flag-day slice; do not preserve two execution worlds to
  keep an intermediate tree green.
- Preserve correct current implementation. Audit and adapt it instead of
  rebuilding a subsystem merely because a newer plan names the final contract.
- Run targeted parser/typecheck/semantic/verify/IR/plan/executor/runtime tests at
  slice boundaries. Run broad workspace, product, and report gates only at major
  milestones. Generate final reports only after the final tracked edit for the
  relevant milestone.
- After the same blocker class appears twice, stop tactical patching and change
  the owning parser, compiler, proof, runtime, currentness, document, renderer,
  host, persistence, physical-layout, or verifier architecture.
- Use subagents for disjoint compiler, proof, packed runtime, distributed,
  streaming, product, hardware, and adversarial-review boundaries.
- Delete superseded syntax, aliases, codecs, plans, tests, runtime paths,
  representations, and compatibility fallbacks once replacements compile. Do
  not rename, quarantine, feature-gate, or retain them as a second path.
- Never hide an engine limitation in example Boon source. Reduce it to an
  unrelated fixture, fix the generic owner, then remove the diagnostic
  workaround.
- Keep command/report output bounded. Use focused filters and jq summaries; do
  not dump large report bodies into the conversation.
- Add no Python source, scripts, invocations, or generated Python artifacts.
- Do not commit or push unless the user explicitly requests it.

Phase 0: reconcile contracts, inventory current state, and freeze evidence

- Reconcile every active plan to one target value algebra, one compiler artifact
  spine, one typed-list architecture, one persistence identity contract, one
  Client/Session/Server topology, one physical-layout boundary, and one
  processor pipeline.
- Keep current architecture documents honest about current behavior. Prepare
  their flag-day replacement in the implementation slice that changes behavior;
  do not claim planned syntax or representation already works.
- Add machine-checked inventories and deletion ledgers for legacy Bool/Null/
  Error runtime values, FiniteReal/binary64 semantics, zero-based APIs, old
  patterns, recursive executable values, string field lookup, tree containers,
  query-world artifacts, unverified compiler entrypoints, physical-layout leaks,
  and stale reports.
- Freeze semantic, verification, physical-layout, target-profile, product,
  dataset, artifact, and report versioning before changing implementations.
- Freeze baseline correctness, allocations, memory, lookup/currentness work,
  native/Wasm behavior, product latency, persistence, and report provenance.
- Add unrelated executable fixtures for direct/wrapped OUT, exact arithmetic,
  Tags/presence/fault, BITS, MAP/SET, typed views, proof erasure, nested
  ownership, effect cancellation, stale routing, visible windows, packed
  scalar/row storage, and bounded hardware eligibility.

Exit: all target contracts agree, all current behavior is labeled honestly, the
old paused goal is retired, and every replacement has a baseline and deletion
ledger.

Phase 1: establish OUT and the verified semantic compiler spine

- Implement the OUT plan's language, structural ownership, semantic, runtime,
  tooling, migration, and deletion work needed to establish the final spine.
  Do not claim its full Clear End Condition in this phase.
- Finish structured parameters, exact named arguments, canonical fresh/forwarded
  OUT, final-only PASS, order-independent declarations, cycle diagnostics,
  contextual expansion, and complete owner ancestry.
- Complete the `CheckedProgram` ownership and structural-inference work needed
  for this cut from `TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md`.
- Pull forward the artifact-boundary subset of formal phases 0–1:
  `boon_semantic`, `boon_verify`, the complete required-obligation manifest,
  mandatory `ContractVerifiedProgram` construction, and opaque
  verification-derived `ErasedProgram`. This dependency slice does not make
  formal Phase 0 or 1 complete; both are re-audited on the final value model in
  Phase 4.
- Introduce the final ParsedProgram -> CheckedProgram -> SemanticProgram ->
  ContractVerifiedProgram -> ErasedProgram ownership boundaries.
- `SemanticProgram` preserves callable boundaries, OutNet, typed logical views,
  semantic ownership, complete dependency/resource manifests, WHERE obligations,
  and source provenance without carrying parser ambiguity.
- Initially contract-free programs still traverse a completeness-checked
  verification gate. There is no unchecked or direct CheckedProgram-to-backend
  path.
- Erase WHERE, OUT, PASS, and transparent wrappers only after a verified
  semantic artifact exists. Preserve an explicit bounded work stack and compact
  expression arena through MachinePlan.
- Make machine, document, distributed, persistence, native host, Wasm, verifier,
  and later hardware consumers use only the appropriate final artifact.
- Complete diagnostics/editor data, ownership, persistence/distribution, visible
  2,600-row window, wrapper-equivalence, default-stack, and deletion evidence.

Exit: every executable path has one verified semantic source and one opaque
`ErasedProgram` boundary, and the OUT structural/compiler slice passes. The
full OUT Clear End Condition remains open until Phase 4.

Phase 2: implement universal language foundations

- Implement every semantic, parser, type, runtime, persistence/wire, target,
  migration, differential, and deletion item in
  `BOON_LANGUAGE_FOUNDATIONS_PLAN.md`. Keep only its formal-dependent Phase 7
  acceptance open until Phase 4 below.
- Finish every compatible inference, full-AST, render-contract, diagnostic, and
  deletion item in `TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md` against that final
  algebra. Keep formal-dependent final acceptance open until Phase 4.
- Replace public/runtime privileged Bool, Null, and Error value variants with
  ordinary Tags/objects plus private nonserializable presence and fault channels.
- Land fail-closed matching, exact rational NUMBER, one-based positions, BITS,
  MAP/SET authorities, collection-in-HOLD rejection, FLUSH, generic transient
  lowering, bounded pulse fusion, and target eligibility.
- Perform each topic as an atomic flag-day across source, parser, typechecker,
  semantic/verification/erased IR, plan, executor, persistence, wire, effects,
  native, Wasm, products, fixtures, docs, and reports.
- Exact Number resource exhaustion is a terminal resource error; it never
  truncates or silently becomes binary floating point.
- No self-hosting compiler, bootstrap stage, OS work, or public MEMORY syntax is
  introduced.

Exit: the foundations and structural-inference implementation is complete
except for explicitly formal-dependent acceptance. No self-hosting condition,
executable legacy value profile, dynamic type fallback, or parser-owned
semantic side channel remains. Neither plan is declared fully complete before
Phase 4.

Phase 3: finish typed-list access on the final algebra

- Implement every compatible semantic/compiler/runtime/persistence item in the
  typed-list plan against exact Number, Tags-only truth, BITS, final ownership,
  and the verified semantic spine. Keep its formal-dependent Clear End
  Condition open until Phase 4.
- Preserve the already-deleted query crates/world. Reuse correct generic
  indexing/currentness code; delete any remaining reflective or duplicate path.
- Build typed filter/order/take/page views and evaluated capture identities in
  `SemanticProgram`. After verification, lower their observable requirements
  into target-independent `ListAccessIntent` in `ErasedProgram`; preserve that
  intent in `MachinePlan`, and select native/Wasm index layouts only in
  `PhysicalPlan`.
- Finish stable order, exact/range/prefix/union/intersection access, cursor
  scope, persistence rebuild, corruption behavior, browser yielding, precise
  currentness, hard memory/fanout/candidate budgets, and zero request-time
  redb/IndexedDB access.
- Prove the 58,500- and 60,000-row fixtures, Cells zero-scan/currentness/frame
  behavior, native/Wasm order/cursor parity, deep pages, mutation fanout,
  allocation/index bytes, and all deletion scans.

Exit: the typed-list implementation and non-formal gates pass on the final
value/compiler model, not the superseded binary64/runtime model. Its full Clear
End Condition remains open until Phase 4.

Phase 4: complete formal verification phases 0 through 5

- Implement formal-plan phases 0 through 5 completely.
- Use exact rational theory, private solver Booleans for source `True | False`
  Tags, BITS bitvector theory, MAP/SET authority invariants, FLUSH commit
  semantics, typed-list algebra, and final persistence activation.
- Implement both WHERE forms, exhaustive dependency manifests, proof obligations,
  source-level counterexamples, evidence/contract hashes, imported contract
  policy, proof report budgets, proof erasure equivalence, and the mandatory
  ContractVerifiedProgram construction boundary.
- Complete pure functions, reactive state, lists/TodoMVC, migrations,
  persistence stamps/activation receipts, editor/CLI/AI diagnostics, and
  incremental proof cache/budget evidence.
- Unknown, timeout, unsupported, contradictory, stale, mismatched, or incomplete
  proof never emits a new runnable artifact.
- Re-run and close the full OUT, foundations, structural-inference, and
  typed-list acceptance/Clear End Conditions. Any report generated before the
  completed formal implementation is stale for those gates.

Exit: every formal acceptance condition through Phase 5 passes and no unchecked
semantic artifact reaches ErasedProgram; every deferred Clear End Condition
from Phases 1–3 is closed.

Phase 5: establish packed hardware prerequisites

- Preserve semantic MachinePlan identity while adding only the target-relevant
  fixed widths, bounds, dense IDs, shape/offset access, and typed storage facts
  needed by hardware eligibility.
- Require CoreHardwareIR, TargetHardwareIR, hardware lowering, and cycle
  execution to contain no recursive Value, runtime string field lookup,
  BTreeMap/BTreeSet/HashMap/HashSet, dynamic allocation, or compatibility
  materializer.
- Keep physical slots, columns, pointers, board pins, and primitive mappings out
  of public values, persistence identity, and semantic hashes.
- Obtain every proof fact through ContractVerifiedProgram and translation
  validation; a target profile cannot bless an unchecked bound.
- Do not claim the universal packed-runtime phases, PhysicalPlan/KernelIR
  migration, product-scale reports, flag-day deletion, formal Phase 6, or
  packed Clear End Condition from this narrower gate.

Exit: verified MachinePlan can enter normalized generic hardware artifacts with
fixed bounded representation and no software-runtime fallback. The universal
packed software runtime remains open until Phase 7.

Phase 6: build and prove BoonConsole and the first Boon-designed RV32I

- No production console/compiler/hardware implementation begins until the
  active simplification/native-recovery exit and Phases 1–5 pass. Read-only
  spec, toolchain, interpreter, power, board, and owned-hardware inventory may
  run earlier.
- Implement every phase and acceptance criterion in
  `BOON_CONSOLE_IMPLEMENTATION_PLAN.md` and the reusable processor work in
  `BOON_FIRST_RISCV_PROCESSOR_PLAN.md`.
- Build unrelated hardware API fixtures before the CPU. Use ordinary BITS,
  bounded MAP, Tags, records, sources, verified semantics, and generic target
  profiles; add no MEMORY keyword or RISC-V/console-aware language shortcut.
- Implement CoreHardwareIR, cycle simulation, TargetHardwareIR, generated
  SystemVerilog, target elaboration, architectural tests, Sail differential
  execution, RVFI/riscv-formal properties, synthesis, and physical
  signature/trace evidence.
- Bring up Pmod BTN, SWT, 8LD, SSD, and CLS concurrently on the exact measured
  iCESugar Pro assembly. The onboard RGB LED has no product or proof role.
- Emit a deterministic bounded standalone `app.wasm` from the verified spine.
  Execute the exact same bytes with an independent PC reference interpreter,
  the simulated SoC, and the physical RV32I kernel/interpreter. Required AOT is
  forbidden.
- Finish volatile upload, persistent install/state, terminal-only USB CDC
  bridge, reset/corruption recovery, safe state, accessibility, virtual/
  physical parity, bounded reports, and the manifest-backed hardware-in-the-
  loop aggregate.
- Boon Orchard remains a vision document only. Do not begin game implementation
  in this goal; it neither gates nor completes the processor/console proof.

Exit: one Boon-authored RV32I and one exact replaceable Boon app are traceable
from source through verified artifacts, hardware IR, generated RTL, physical
board execution, app-Wasm interpretation, all-peripheral logical traces, and
fresh HIL evidence. Documentation, simulator-only parity, host AOT, or visual
observation does not pass.

Phase 7: complete the universal packed runtime and formal optimization

- Implement packed-plan phases 0 through 6: inventories/budgets, dense semantic
  artifacts, packed cells/typed arenas, dense scalar runtime, columnar rows,
  currentness/dependencies/delta staging, and collection/index kernels.
- Preserve semantic MachinePlan identity and produce separately versioned
  PhysicalPlan layouts. No pointer, slot, layout, or physical column leaks into
  public values, persistence identity, or semantic hashes.
- Integrate formal Phase 6 with packed Phase 7 KernelIR. Proof facts may select
  a generic optimization only through ContractVerifiedProgram; every accepted
  transformation has translation validation and measured evidence.
- Complete packed phases 8 and 9: boundaries, native/Wasm parity, product-scale
  reports, and deletion of the old value/row/dependency/materialization
  execution world.
- Remove cycle-hot recursive Value, string lookup, BTreeMap/BTreeSet/HashMap/
  HashSet, full snapshot, and compatibility materialization paths. Retain an
  ordered container only in a classified cold boundary/tooling use.
- Reuse Phase 5 hardware facts where their identity and proof domains match,
  but do not make hardware IR a software executor.
- Complete every packed acceptance criterion and its Clear End Condition.

Exit: one universal packed execution world serves native, Wasm, server, and
products; formal Phase 6 and the packed Clear End Condition pass.

Phase 8: finish distributed/session, persistence, bytes, content, and streaming

- Audit and preserve correct existing implementation after the final semantic
  and packed migrations.
- Implement one browser Client, one resumable Session island per tab, and one
  global Server. Permit only Client <-> Session <-> Server; reject direct
  Client <-> Server references/calls.
- Derive cross-island edges from qualified values/calls. Boon source declares no
  internal routes, RPC, HTTP, JSON, synthetic result SOURCE values, or invented
  effects blocks.
- Finish Session template isolation, complete owner/generation routing, fair
  scheduling, scoped replies, shared demand, resumability/expiry, positional
  CBOR framing, schema mismatch, stale rejection, and secret absence.
- Finish canonical exact-Number/BITS/Tag/MAP/SET persistence, atomic candidate
  turns, FLUSH abort, restore/migration, semantic-vs-physical identity, and
  browser/native rebuild behavior.
- Finish immutable BYTES, bounded File/read_bytes, atomic File/write_bytes,
  Content/import/save, and multishot File/read_stream with 64 KiB chunks,
  four-chunk credit, strict sequencing, bounded queues, cancellation, stale
  owner rejection, RAII cleanup, and Busy behavior.
- Prove two tabs, cross-user isolation, fairness, reconnect/expiry, stale routes,
  bounded memory/backpressure, branch removal, disconnect, corruption, atomic
  writes, Busy, restart, and terminal cleanup.

Exit: distributed/session/security, persistence, and streaming lifecycle gates
pass on the final compiler and packed runtime. Device flash persistence remains
a separate bounded console adapter and does not replace this contract.

Phase 9: complete formal external contracts and mature products

- Complete formal Phase 7 over the stabilized distributed/provider boundaries.
  No contract disappears across a role or provider edge; assumptions are
  versioned and conditional assurance remains distinct from closed proof.
- Finish NovyWave with real VCD/FST/GHW data, hierarchy, rows, traces, cursor
  values, comparison, analog/physical display, paging, cancellation, bounded
  materialization, resource cleanup, and 60 FPS interaction.
- Preserve and freshly prove Cells typed find/chunk behavior, sparse
  currentness/errors, indexed lookup, dependency/range updates, cycles, retained
  layout/render state, generic virtual windows, editing, selection, scrolling,
  hover, focus, and formula-bar visibility.
- Require Cells product input-to-visible and scroll p95 <= 16.7 ms and
  max <= 33.4 ms, with proof/readback latency accounted separately and zero
  normal-path full-grid recompute, list scan, relower, layout/host rebuild, or
  scene rebuild.
- Complete every non-deferred FjordPulse phase 0 through 13 against pinned
  revision dd6e750c2ca9dec3041f66ceda31d30379d4027a: exact Numbers with explicit
  external rounding/encoding, Client/Session/Server, generic HTTP/WS, typed
  access, 58,500 stations, retained MapViewport, browser WebGPU, accessibility,
  deterministic and Live modes, Entur/raster providers, public/Admin workflows,
  security, canonical persistence, migration/restart/redeploy, and Coolify.
- Convert all 338 non-deferred FjordPulse scenarios to fresh passing evidence.
  Only the two explicitly deferred backup/restore automation scenarios may
  remain deferred.
- Product logic and Session policy remain Boon. Rust remains generic platform
  machinery with no product/example branch or fixture-response substitution.

Exit: formal Phase 7, every NovyWave acceptance scenario, all Cells functional/
performance gates, all 108 FjordPulse stories, and the exact 340-scenario final
classification pass.

Phase 10: prove the complete compiler, console, native, and web milestone

- Run independent adversarial reviews for compiler genericity, proof
  completeness, typed-list semantics/access/currentness, packed storage,
  standalone app-Wasm identity, hardware lowering, interpreter bounds, RV32I
  compliance, console HIL integrity, native/Wasm parity, persistence/restore,
  nested ownership/event safety, Session isolation/security, streaming cleanup,
  native proof integrity, NovyWave real-data ownership, Cells performance, and
  FjordPulse parity.
- Generate native GPU handoff reports using only
  `docs/architecture/native_gpu_handoff_manifest.json` and console reports
  using only the future
  `docs/architecture/boon_console_handoff_manifest.json`. Run each
  manifest-backed aggregate on the same unchanged revision.
- Prove zero-scan bounded 58,500-station first/deep pages, browser 60 FPS,
  persistence/restart/migration, Live Entur operation, HTTPS/WSS, and production
  deployment at https://fjordpulse-boon.kavik.cz.
- Bind source, semantic/verified/erased artifacts, schema/index, physical plan,
  app Wasm, hardware IR, RTL, bitstream, kernel/interpreter, board, dataset,
  adapter, binary, deployment, surface, input, presented frame, and proof
  identities as applicable.

Exit: the complete compiler/runtime, BoonConsole, and web-application story is
mature and freshly proved. Documentation, scaffolding, stale reports, partial
parity, or deployment without persistence/restart proof does not pass.

Phase 11: complete the example portfolio

- Use selected examples throughout earlier phases as unrelated regression and
  budget fixtures: Counter/TodoMVC, Cells, NovyWave, a FjordPulse-shaped indexed
  dataset, an exact/rounded compute kernel, and BITS/byte/hardware fixtures.
- After the RV32I/BoonConsole milestone, implement every remaining phase and
  completion condition in `BOON_EXAMPLE_PORTFOLIO_PLAN.md`.
- Canonical native/Wasm examples use exact Number. GPU execution uses proved
  exact/fixed-point/BITS lowering or explicit exact source rounding with
  equivalent observable results; `ApproxF32` is not an alternate Boon Number
  semantics.
- Integrate the already-proved RISC-V processor rather than creating a second
  generated-CPU design or backend.

Exit: the full portfolio passes correctness, cancellation, cross-target,
performance, provenance, and genericity acceptance without weakening the
processor or product evidence.

Final verification and absolute stop condition:

- Run independent final reviews for every phase and every referenced plan.
- Final scans must find no active Python; semantic BYTE; SOURCE { ... };
  invented effects blocks; internal JSON role transport; ProgramRole::Document
  or paired fallback; positional/renamed calls; non-final PASS; ListMapBinding;
  runtime OUT; unchecked semantic/compiler path; executable WHERE; privileged
  public Bool/Null/Error runtime value; FiniteReal/binary64 Number semantics;
  zero-based public collection/text/bytes/BITS positions; legacy pattern
  behavior; public MEMORY; BoonInBoon/self-hosting requirement; List/query;
  List/query_prefix; ListQuery/PlanQuery contracts; QueryCollectionState;
  boon_query/boon_query_redb; reflective field paths; duplicate authority;
  request-time redb/IndexedDB; parser/backend contextual rediscovery; exposed
  hidden identity; positional owner/event fallback; cycle-hot recursive Value or
  tree container; semantic/physical hash conflation; compatibility runtime;
  handwritten functional CPU RTL; example/RISC-V-specific shortcut; temporary
  codemod; or stale report.
- Removed terms may remain only in isolated compile-fail/invalid-schema tests or
  explicit deletion-audit documentation, never as accepted behavior.
- Every compatible Clear End Condition in the OUT, foundations,
  structural-inference, typed-list, formal, persistence, packed, NovyWave,
  Cells/native, FjordPulse, RISC-V, BoonConsole, and example-portfolio
  contracts must pass.
- Re-run every applicable final report after the final tracked edit. Evidence
  from a pre-foundations, pre-formal, pre-packed, pre-product, pre-processor, or
  otherwise different source/artifact revision is stale.
- Mark the goal complete only after the final independent review finds no
  unresolved issue. If unavailable credentials, production infrastructure, or
  owned hardware genuinely blocks an external gate after all independent work
  is complete, record exact evidence and mark the goal blocked, never complete.
```
