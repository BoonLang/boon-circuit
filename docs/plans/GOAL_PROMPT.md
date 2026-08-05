# `/goal` Prompt

```text
/goal Complete the unified Boon language, verified compiler, universal packed
runtime, BoonConsole with the first Boon-designed RV32I processor and exact
interpreted app.wasm parity, mature web-application stack, and example portfolio
objective from the current HEAD.

The first and only active implementation objective at goal start is the full
`BOON_COMPILER_PERFORMANCE_PLAN.md` Clear End Condition. Execute that plan's
internal phases before the unified phases later in this prompt. Do not begin or
resume simplification/native recovery, language foundations, formal, packed,
console, product, portfolio, or game implementation while any required
compiler-performance report is missing or red.

Read AGENTS.md and these contracts completely before editing:

- docs/plans/steps.md
- docs/plans/BOON_COMPILER_PERFORMANCE_PLAN.md
- docs/plans/BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md
- docs/plans/BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md
- docs/plans/BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md
- docs/plans/BOON_CIRCUIT_SIMPLIFICATION_AND_NATIVE_RECOVERY_PLAN.md
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

- Resolve and record the actual `git rev-parse HEAD` when this new goal starts.
  Paste this fenced prompt as the new objective; do not replace it with a
  bootstrap sentence pinned to a historical commit hash. If the product stores
  a long pasted objective in an attachment, that attachment must contain this
  complete current prompt rather than an older hash reference. Later authorized
  checkpoint commits advance the same live goal and do not require another
  `/goal resume`.
- This prompt replaces the pre-foundations unified product goal. Do not resume
  an agent goal that captured the older prompt. Preserve its compatible commits,
  retire that paused goal, and start a fresh goal from this file.
- Git history is the archive for the replaced prompt. Do not restore it as a
  second active goal, compatibility plan, or alternate authority.
- `steps.md` fixes execution order. Individual plans remain authoritative for
  their own semantics, invariants, reports, budgets, and acceptance criteria.
- The active simplification/native-recovery plan owns its exit. Its
  repository-wide tracked-Rust and test-Rust totals are inventory telemetry,
  not completion gates; retain its focused subsystem caps and require a proven
  duplicate or superseded owner before deleting implementation or tests.

Authority and conflict rules:

- `BOON_COMPILER_PERFORMANCE_PLAN.md` owns compiler execution architecture and
  performance: source-unit snapshots, the owned compiler database and
  `CompilerSession`, dependency-indexed invalidation, cancellation, immutable
  compiler-artifact reuse, phase profiles, scaling fixtures, and cold, warm,
  cancellation, and RSS budgets. It may replace internal compiler
  representations, but it does not own public language semantics, proof or
  evidence soundness, runtime persistence identity, native presentation
  semantics, or the mandatory verified artifact spine.
- `BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md` is subordinate to those budgets
  and owns the current post-`d177af9` multiplier decision, detailed by
  `BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md`: immutable source and body-
  insensitive item snapshots; parser-owned stable definition/occurrence routes;
  separate canonical-snapshot, session-lineage, semantic/persistence, and dense
  identity planes; one database with typed evaluation/currentness edges distinct
  from proof/link relocations; interface SCC and checked-definition shards;
  demanded definition executable artifacts; construction-owned domain
  artifacts; thin summary/relocation linking; one consuming
  `SealedRunnableMachine` builder; persistent currentness; and only measured
  dependency inversions. Its intermediate directional exits never replace the
  performance plan's complete scored Clear End Condition.
- `BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md` refines that implementation
  order after structural occurrence identity. Unit-native production checking
  is landed; next enforce distinct syntax/checked/stable/linked identity types,
  replace AST-rewriting project link with an immutable overlay, and make the
  database a real typed evaluator. Then make compiler intents request roots;
  publish normalized semantic fact sections once; use definition plan-code
  modules, thin linking, compositional phase seals, and one consuming runnable
  builder; and split model/builder/link crates only after measured one-way seams
  exist. It does not replace the mandatory artifact spine or any performance/
  evidence budget.
- Compiler scaling evidence uses actual parser inspections and
  typechecker/semantic/proof/backend work, never final AST/call/graph sizes as
  substitutes. Build one current two-job release producer explicitly and invoke
  it directly for repeated samples; the performance verifiers must reject a
  missing or stale producer and must not launch Cargo themselves.
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

  `boon_checked` owns the opaque `CheckedProgram` representation and
  `boon_typecheck` is its sole audited issuer; `boon_semantic` owns contextual
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
- commits 8e065e4, db5e11c, and 5a6993b establish the compiler-performance
  contract and harness foundations, independent source-unit parsing, the
  `boon_syntax` flag-day boundary, exact parser/typechecker work counters, and
  prebuilt-producer report propagation. Preserve these checkpoints; do not
  repeat their documentation or compatibility work;
- the current direct debug diagnostics sample is about 14.9 ms for Counter and
  744.1 ms for NovyWave, with NovyWave split between about 332.4 ms parsing and
  411.7 ms typechecking at 76,412 KiB peak RSS. Parser tracing attributes about
  126.5 ms to canonical validation and observes 1,060,559 validation visits for
  73,571 tokens. This is directional evidence, not release acceptance, and it
  makes a bounded current-release confirmation followed by indexed validation/
  assembly plus deterministic non-recursive checked-diagnostic projection the
  first implementation tranche;
- checkpoint `677d09d` completed the exact
  three-setup/30-scored direct protocol in both cache-disabled modes: Counter
  is 5.34/5.27 ms p95 fresh/empty, physical TodoMVC is 61.24/61.30 ms, and
  NovyWave is 234.48/248.86 ms, with maximum RSS 11,140/27,132/70,868 KiB and
  one stable checked-result digest per fixture. NovyWave fresh allocations fell
  from 1,997,515 calls / 225,753,409 bytes at the checked-boundary baseline to
  1,933,602 / 219,951,108. The 248.86 ms empty result has narrow headroom and is
  not a Phase 1 exit. The subsequent ownership slice makes `ParsedProgram` one
  immutable shared snapshot, removes `Checker`/`CheckedProgramBuilder`
  lifetimes, replaces per-function owned paths with one flat path arena, passes
  all 64 parser and 80 typechecker tests including both product-scale oracles,
  and retains the exact checked digest. Its directional NovyWave release
  samples are 230.83/229.30 ms fresh/empty and 1,933,612 fresh allocations /
  219,965,836 bytes. The next worklist slice removes redundant expression
  propagation lanes and recycles dense pending buffers; the exact checked
  digest and both product-scale oracles remain unchanged, while a current
  fresh release sample uses 1,932,386 allocations / 219,267,245 bytes and the
  traced checked builder falls from 134.02 to 130.61 ms. Exact call-input
  sensitivity then seeds concrete fixed-result builtins once while retaining
  every projection/cache dependency: NovyWave call visits fall from 5,060 to
  3,848, no-op visits from 1,964 to 752, and directional release samples are
  229.97/228.51 ms fresh/empty with 1,929,058 fresh allocations / 219,061,685
  bytes. The complete 80-test suite, full-sweep audit, and exact digest pass.
  Recursive flow inference then retains one immutable parsed-program owner per
  root instead of cloning/dropping it on every cache miss. Work and allocation
  counts remain exact; a three-sample directional release batch has
  227.33/227.73 ms fresh/empty medians. Dense FLUSH propagation then uses the
  authoritative reverse dependency graph instead of whole-program fixed-point
  rescans, and inline AST-child buffers remove per-node temporary vectors. A
  six-sample directional release batch has 224.50/225.92 ms fresh/empty medians
  and 1,827,343 fresh allocations / 217,055,736 bytes; the full FLUSH oracle,
  suite, and digest pass. Checked-read plans now borrow immutable AST segments,
  retain canonical text only for unresolved paths, and share declaration-reader
  storage with invalidation; contextual setup consumes its indexes in place and
  builds the final reverse flow graph without a transient edge tuple buffer. A
  six-sample directional release batch has 220.22/221.15 ms fresh/empty medians,
  165.15/165.51 ms typecheck medians, and 1,791,466 fresh allocations /
  213,748,071 bytes; the complete suite, exact work, audit, and digest pass.
  Signature-to-declaration synchronization now publishes through the owned
  dense declaration index without two full ordered snapshots or an update
  vector. The exact digest and suite pass; fresh allocation work falls to
  1,789,024 calls / 212,744,182 bytes. Its six-sample batch is timing-neutral/
  noisy at 222.34/219.94 ms fresh/empty, so it is not a latency claim. The
  changed-signature/PASSED-cone attempt was decomposed after changing the
  digest: generic declaration writes now journal the exact callable signatures
  they disturb and the unchanged synchronization boundaries publish only that
  journal plus explicitly changed signatures. The pre-change NovyWave tuple
  oracle, exact digest, and complete suite pass. Fresh/empty allocation work is
  1,788,315/1,788,321 calls and 212,679,422/212,744,019 bytes, 709 calls / 64,760
  bytes lower in each mode. A six-pair batch is still bimodal at 225.75/231.58
  ms fresh/empty and 169.80/173.69 ms typecheck, so this is not a latency claim.
  The next ownership slice gives signatures an explicit callable-declaration
  publication API and makes structural/solver/final value lanes reject callable
  IDs. The independently retried PASSED worklist then follows only the reverse
  lexical-PASSED callee-to-caller cone. NovyWave context visits fall from 504 to
  369 with all 369 changes, the tuple oracle, exact digest, and full suite
  unchanged. Fresh/empty allocation work falls to 1,787,633/1,787,639 calls and
  212,614,838/212,679,435 bytes. A six-pair batch has 219.70/219.39 ms total and
  164.18/164.53 ms typecheck medians, with one slow outlier per mode; this
  remains directional. The context phase is still about 10.52 ms because it
  builds complete ordered call maps before pruning, and the 35 inference rounds
  remain. The compact-owner cutover now projects immutable expression owners
  once into dense signature ordinals and per-signature expression/root slices,
  borrows PASSED paths, and uses dense leaf/requirement/recursion/worklist state.
  The legacy owner/root oracle, fixed tuple oracle, exact digest, and full suite
  pass. Fresh/empty allocation work falls to 1,758,855/1,758,861 calls and
  211,969,403/212,034,000 bytes. Six-pair medians are 216.62/217.17 ms total and
  161.95/162.37 ms typecheck. The traced context phase falls to 8.22 ms and the
  checked builder is directionally 117.37 ms. Parameter schemes remain about
  9.08 ms, structural schemes 5.17 ms, and checked inference 43.23 ms/35 rounds.
  The next fixed-point slice now cold-seeds the 618 generic
  input-insensitive/fixed-product call plans before the first expression wave
  while preserving the ordinary order for all 1,202 input-sensitive calls.
  Inference falls to 34 rounds, 34,653 expression visits, 690 callable visits,
  and 3,484 call visits; the tuple oracle, clean audit, exact digest, and all 80
  tests pass. Fresh/empty allocation work falls to 1,732,729/1,732,735 calls and
  210,213,894/210,278,491 bytes. Six-pair release medians are 214.35/212.82 ms
  total and 158.87/158.18 ms typecheck. This remains directional, not Phase 1
  acceptance. The subsequent owned-database slice fuses checked construction,
  ordered diagnostic projection, and report assembly into one
  `CheckedProgramDatabase`; it deletes `Checker`, `CheckedProgramBuilder`, both
  transfer bundles, the duplicate named-value owner, the post-seal external
  environment clone, and compiler-proven dead recursive inference/report
  helpers. The source diff is net 1,153 lines smaller. The fixed tuple oracle,
  exact digest, clean audit, all 78 ordinary tests, and both product-scale gates
  pass with unchanged work. Fresh/empty allocation work is
  1,732,728/1,732,734 calls and 210,213,846/210,278,443 bytes; six-pair release
  medians are 213.89/212.96 ms total and 158.70/157.43 ms typecheck. The release
  rebuild remains 1m35s, so the later measured crate/downstream-relink boundary
  remains open. Checked reverse dependencies now replace 154,585 fragmented
  possible-row headers and 26,425 populated-row allocations with immutable
  packed offsets/edges for 40,880 base and 29,812 derived flow edges; the
  construction-only pattern column is not retained. The exact digest and every
  work counter remain unchanged, all 79 ordinary tests and both product gates
  pass, and fresh/empty allocations fall to 1,687,722/1,687,728 calls and
  210,064,354/210,128,951 bytes. Six-pair medians fall to 208.89/209.55 ms total
  and 152.86/153.76 ms typecheck, with 65,768/66,400 KiB maximum RSS. Continue
  with the dominant checked-inference worklist, remaining contextual/structural
  work, and measured name/type interning. The next retained slice preserves
  unchanged widened list/object nodes and coalesces an input-only call retry
  only after two consecutive no-op/evidence-only visits. Every coalesced call
  is refreshed before the contextual-wrapper quiescence hook and any visible
  result/output returns fail-closed to the ordinary solver. The exact digest,
  all 82 ordinary tests, and both product gates pass. Expression/declaration/
  call visits fall to 34,502/1,876/3,388, input enqueues to 1,127, and no-op
  visits to 893; 18 coalesced calls refresh exactly. The release worklist falls
  from about 39.6 to 36.3 ms despite 36 stable repair rounds versus 34. Fresh/
  empty allocations fall to 1,666,870/1,666,876 calls and
  208,381,392/208,445,989 bytes. Six-pair medians are 206.28/206.55 ms total
  and 150.21/150.89 ms typecheck, with 65,932/66,352 KiB maximum observed RSS.
  The following compact-type checkpoint moves the one canonical structural
  operator into `boon_checked`: substitution is one copy-on-write traversal
  with an inline cycle stack, widening preserves unchanged shared object/list/
  Tag nodes and normalized field order, and canonical Tag sorting no longer
  formats allocation-backed keys. No eager shape hash is retained. The exact
  digest, seven checked-graph tests, all 85 ordinary typechecker tests, and both
  product gates pass. Fresh/empty allocations fall again to
  1,621,578/1,621,584 calls and 206,521,350/206,585,947 bytes. An 18-pair
  directional release batch has 204.74/205.60 ms total and 149.59/150.10 ms
  typecheck medians, with 223.03/223.09 ms maxima and 65,348/65,924 KiB maximum
  RSS. The next ownership slice reuses the solver's packed call graph throughout
  contextual inference, packs lexical PASSED reads without cloned paths, and
  stores structural statement children/values inline or densely. All 85
  ordinary tests and both product gates pass with the exact digest and
  unchanged solver work. Fresh/empty allocations fall again to
  1,613,479/1,613,485 calls and 206,406,282/206,470,879 bytes, exactly 8,099
  calls and 115,068 bytes below the compact-type checkpoint in each mode. The
  contextual graph lane falls from about 0.28 to 0.03 ms; an 18-pair batch is
  noisy at 218.24/220.90 ms total and 159.53/161.72 ms typecheck medians, so
  this is an ownership/allocation result rather than a total-latency claim.
  Continue with the measured contextual parameter/worklist and checked-
  inference owners, remaining construction/diagnostic work, and name/type
  interning, then finish scaling/parity proof and fresh adversarial review and
  regenerate the full protocol.
  Do not mistake the diagnostics result for verified closure: current direct
  NovyWave verified samples remain about 7.72/7.56 seconds and 515 MiB,
  dominated by about 6.9 seconds of semantic construction, so the verified
  time/RSS gates remain mandatory later blockers;
- a fresh high-level trace at checkpoint `c77dabc` confirms that the verified
  blocker is architectural multiplication: 17,716 checked expressions and
  1,820 calls become about 45,000 semantic expressions and 5,146 OUT call
  instances, then 247,537 dependency records and a 248,201-node/1,060,194-edge
  proof graph. The compile takes about 7.79 seconds, allocates about 2.99 GB
  cumulatively, and peaks at 515,172 KiB. Only 61 of 426 initially pure
  ordinary candidates are retained; 357 close over an unretained body and 71
  more are rejected at open boundary types. Stop polishing typechecker
  containers while this is the dominant owner. Implement retained callable
  definitions plus dense invocation overlays for parameter, PASSED, type,
  owner, effect, and resource bindings; begin with the generic type-polymorphic
  pure closure and measure retained-body, overlay, avoided-specialization, OUT,
  dependency-record, proof-node, and proof-edge counts. A flattened expansion
  may be a test oracle only. Then derive dependency facts once from definitions
  plus overlays, seal a compact `SemanticProgram`, and proceed to direct
  lowering/streaming hashes. Resume remaining Phase 1 interning/scaling/parity
  work after this cardinality cut;
- the 2026-08-03 retained-definition/invocation-overlay candidate validates the
  larger direction: NovyWave fresh/empty verified samples fall to
  4,415.27/4,455.01 ms and 317,844/318,428 KiB, semantic construction falls to
  3,758.10/3,777.86 ms, and the semantic graph falls to 16,521 nodes with about
  10.92 million allocations/1.552 GB. Open typed boundaries and pure render
  constructors now share definitions; compact checked-call occurrences carry
  constructor contexts and dependency proof classifies those overlays. The RSS
  gate is directionally green but the 1,000 ms gate is red. Both modes emit
  deterministic `f293e8a8...`, but the historical budget hash `4d3c284a...` is
  not a valid semantic oracle: its persistence type for
  `store.selected_value_column_width_key` omits the source-reachable `Widest`
  state. The `c77dabc`-like and retained artifacts include all four states and
  have byte-identical persistence sections. Do not restore the unsound artifact
  and do not accept `f293e8a8...` merely because it is faster. First implement
  the performance plan's test-only flat/specialized differential oracle,
  exact stable-contract section checks, plan verification, migration/restart
  checks, negative cases, and recorded V3 oracle migration. Keep budget V2 red
  until that proof passes. A trace assigns 2,368.00 ms to the dependency
  manifest, which still creates 159,612 records and a
  160,276-node/512,204-edge proof graph from only 16,417 execution expressions;
  after oracle repair, emit proof receipts during semantic construction, fold
  owner-local and exact projection roots, and close dependencies on their
  compact summary graph rather than merely repacking or coarsening the entity
  inventory. Then add demand-driven retained plan instances, share sealed
  semantic row fingerprints across consumers, and split the compiler-loading
  facade out of `boon_runtime` only at a measured rebuild seam. Follow the
  complete ranked architecture tranches in the performance plan and do not
  relabel this checkpoint as Phase 1 closure. The first non-default
  `test-flat-oracle` slice now parses/checks once and lowers both
  representations without adding a production fallback. Counter and a focused
  four-state width fixture pass stable-contract and multi-turn document/
  snapshot differential tests. An explicit optimized NovyWave preflight passes
  structural stable-contract, plan-verifier, and exact
  `Compact | Normal | Wide | Widest` persistence comparison in 14.82 seconds.
  Raw internal expression offsets and duplicated dirty/commit work counts are
  normalized or treated as performance telemetry, while zero-unresolved
  invariants remain exact. Budget V2 stays red until the V3 provenance report,
  real-host NovyWave behavior scenario, migration/restart matrix, and negative
  cases are complete;
- continue the performance goal from local checkpoint `9540262`, which
  preserves `174eb4b`'s checked/execution ownership boundary,
  `c870358`'s compilation database, compact-proof/sealed-plan
  checkpoint `38e6541`, and activation/effect checkpoint `32bcf40`, and follow
  `BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md`. The canonical list-dataflow,
  sparse durable-overlay lifecycle, product-faithful `boon_behavior_harness`,
  real local host, retained document hit testing, exact activation-turn
  handoff, atomic reset/activation persistence, deterministic effect transcript,
  and pre-commit pruning of unleased producer work are landed. The headless
  harness has a 28-crate normal forward closure versus native playground's 38.
  Preserve this route and complete the real-host NovyWave migration/restart/
  provenance/negative matrix before phase acceptance; it does not delay the
  active compiler cut. Preserve the generic engine contract and do not add
  row-handle durability, projection-specific activation, example shortcuts, a
  second mount/replay authority, or a production flat fallback;
- prioritize one owner/projection compilation database over further micro-
  optimization. A current directional release trace still spends 2,350.35 ms
  building 159,652 rich dependency records, 208,982 coverage rows, and a
  160,316-node/512,314-edge proof graph for only 663 callable owners plus the
  program root. Implement a minimal `CompilationDb` vertical slice in which a
  stable owner/projection or definition/invocation-overlay request owns dense
  semantic row receipts, an exact dependency span, input/result fingerprints,
  `changed_at`, `verified_at`, and work counters. Use that same request graph
  for cold construction, owner/projection proof roots, and later warm
  currentness/backdating; do not build a separate proof index and incremental
  graph. Keep exhaustive V3 derivation test-only until adversarial mutation,
  omission, cycle, cone-precision, and retained/flat parity authorizes a
  flag-day V4 proof; production must then allocate no exhaustive dependency or
  coverage inventory;
- preserve the first production V4 projection-proof result, but do not mistake
  it for the database or performance exit. A directional optimized NovyWave
  sample improves from the preceding sealed-plan sample's 4,581.206 ms and
  317,316 KiB to 3,977.806 ms and 247,092 KiB; manifest time improves from
  2,321.269 to 1,807.287 ms, and the exact production graph falls from
  159,617 nodes/506,915 edges to 14,518 nodes/43,714 edges. Exhaustive V3 is
  test-only with independent V4 materializer parity. The remaining manifest
  still rescans completed checked, execution, and lowering graphs, so `/goal`
  must next move row/projection receipt emission into those builders, share the
  receipts with proof and revision currentness, and delete the corresponding
  inventory walks. Reprofile and reduce unnecessary semantic demand after that
  ownership cut. Do not stop at this improvement and do not return to receipt-
  hashing or container micro-tuning while the 1,000 ms total and 350 ms
  semantic/proof gates remain red;
- apply the post-`c870358` architecture audit, not a receipts-only reading. The
  4,052.379 ms directional sample would still take about 2.24 seconds if the
  entire manifest vanished. A single `OwnerBodyUnit` is also too coarse: split
  stable interface and authored definition shards from occurrence-owned
  invocation shards and ephemeral program/bundle link fixed points. Use typed
  `Pending -> Finalized` rows with distinct local, linked-target, and dense-
  image digests; split broad program-root dependencies. Make one sealed
  semantic image primary storage and delete rich graph owners as borrowed
  views/test materializers pass. Do not accept a database facade under the
  eager nine-graph pipeline;
- preserve the first functional `boon_compilation_db` integration but continue
  through it. The shared kernel now owns revision/backdating metadata, compact
  forward/reverse request edges, deterministic SCC sealing, and implementation
  roots; semantic V4 has deleted its duplicate SCC code and owner-by-projection
  scan. Four kernel tests and all 19 focused manifest tests pass. A fresh
  directional NovyWave sample is still 4,011.485 ms at 250,416 KiB, with
  3,265.269 ms semantics and 1,771.603 ms manifest work, so the roughly
  40--50 ms improvement is not an exit. The next `/goal` work must establish
  stable definition/invocation/link identities, make finalized rows produce
  receipts, and delete checked/execution inventories, not stop at the crate
  boundary;
- preserve the first callable-interface firewall after `c870358` without
  mistaking it for a time exit. Registered dense projection IDs and leaf
  public-shape nodes reduce the largest NovyWave SCC from 4,296 nodes to 85,
  while a directional run remains 3,961.669 ms/250,596 KiB. Checked, execution,
  and lowering inventories still cost about 378/477/272 ms and receipt folding
  502 ms. Continue with finalized shard rows and delete those passes; do not
  spend the next tranche tuning the now-small SCC kernel;
- apply the current-HEAD owner-deletion audit after that firewall. A direct
  release trace is 4,044.712 ms/250,736 KiB: about 1.321 s builds eager semantic
  graphs, 1.820 s reconstructs/folds proof, 362 ms expands the backend, and
  another 111 ms hashes the canonical semantic core. The next checkpoint must
  move the complete checked and execution domains into image-owned columns,
  finalize execution only after resource synthesis/binding/lineage, remove
  `SemanticProgram`'s checked/execution owners and their production inventories,
  and keep the old artifacts only as independent test oracles. A receipt
  sidecar or callback beneath the same rich owners is rejected;
- preserve the resulting checked/execution ownership checkpoint `174eb4b`
  without mistaking its first representation for a speed result. The typechecker now
  owns one opaque checked seal, resource owns the only pending-execution
  mutation window, production `SemanticProgram` drops both prior owners, and
  Manifest V5 imports finalized receipts with source-owner parity oracles.
  Focused tests and architecture checks pass, but one direct optimized
  NovyWave sample regresses to 5,665.819 ms/507,428 KiB because full stable
  projection keys and invocation-path vectors are cloned into row routes and
  119,441 edges; execution-image finalization alone costs 1,142.939 ms. After
  checkpointing this required seam, do not respond to the regression with
  isolated map, hash, allocator, or serializer tweaks;
- apply the completed post-`174eb4b` whole-pipeline audit. Three independent
  read-only reviews of projection/image ownership, semantic demand expansion,
  and the complete artifact spine select one flag-day replacement: stable
  parser-derived occurrence identities; one collision-checked dense owner/path/
  projection registry; typed row columns and CSR relocations; direct Manifest
  V6 sealing without V1/V5 import; verified-intent demand before OUT/contextual
  occurrence expansion; one sealed semantic authority; compact distributed
  link summaries; one plan-code linker across document/row/migration; and one
  consuming `SealedRunnableMachine` with runtime indexes built once. Follow the
  architecture plan's exact staging, deletion ledger, fingerprint domains,
  counters, independent oracles, and anti-facade rejection tests. Each vertical
  batch must delete an existing scan, rich graph, recursive lowerer, or metadata
  reconstruction owner. Do not accept an interner-only patch, V2 wrapper over
  V1/V5, production compatibility path, or crate re-export as progress;
- preserve the first dense V2/V6 working-tree cut only as a checkpoint, not a
  performance or architecture exit. Checked stable keys are owned once behind
  dense routes/CSR relocations; execution projections reference checked IDs and
  parent-pointer invocation paths; Manifest V6 imports fixed digests and dense
  edges; and snapshot-local call identity uses a reverse duplicate ordinal, so
  unrelated or identical earlier calls do not renumber later duplicates. It is
  not yet parser-owned structural identity: raw source text/path and rich DTO
  payloads with dense IDs/spans still contaminate snapshot receipts. A final
  two-job release rebuild takes 3m00s; its direct optimized NovyWave sample is
  3,549.342 ms/274,896 KiB with the unchanged plan hash, 375.894 ms execution-
  image finalization, 727.061 ms manifest work, and 1,805,377,118 allocated
  bytes. The V2 builders still scan finished rich
  checked/execution columns, demand still follows 5,147 eager OUT instances,
  49,283 execution rows and 78,336 legacy proof rows remain, and the other rich
  semantic graphs/canonical core/backend lowerers are still live. Continue by
  moving verified intent before OUT/contextual expansion and delete the
  superseded scanner/owner with parser structural occurrence routes, typed
  normalized row payloads, the first demanded definition, and compact
  occurrence frames; do not tune the dense containers or claim Phase 1;
- follow the post-`9540262` multiplier audit before further compiler edits.
  Separate deterministic snapshot routes, session-local syntax lineage,
  language-owned semantic/persistence identity, and revision-local dense IDs;
  never force one digest to serve all four. The selected next flag-day slice
  combines verified-intent roots, one demanded definition-specialization
  worklist, compact invocation frames, demanded OUT/contextual topology, and
  typed normalized image rows. Delete a recursive OUT/contextual owner and its
  post-hoc scanner in that slice and prove fewer NovyWave call instances and
  execution rows. Direct proof sealing and one plan-code linker follow before
  persistent currentness, compact bundle linking, bounded parallelism, or
  crate extraction. Do not parallelize or split the current eager ownership;
- preserve the first demanded-definition working-tree cut without mistaking it
  for tranche completion. It computes 312 retainable definitions once and
  keeps sparse concrete ancestry for the 190 definitions reaching call-local
  render contexts. NovyWave has 3,494 rather than 5,147 OUT calls, 47,296
  rather than 49,283 execution rows, and 82,364 rather than 119,671 projection
  edges; two direct runs are about 3.45 seconds/271 MiB with deterministic plan
  hash `db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`,
  and the independent flat artifact oracle passes the stable contract and
  persistence comparison. Preserve `VerifiedSemanticIntentV1`, which now
  publishes and validates all planned checked root categories before OUT,
  supplies OUT's exact program schedule, and shares retained definitions with
  contextual expansion. Continue by making those roots drive construction-time
  normalized rows/relocations. Delete the post-hoc image scanner and Manifest
  re-import owner, then land the shared plan-code linker; do not return to
  reachability/map micro-tuning or claim a performance exit;
- preserve Manifest V7's first construction-owned domain as a transition, not
  the target proof shape. Production lowering replay is deleted; 36,979 rows
  are emitted by lowering and V7 leaves 36,183 legacy rows. The focused debug
  oracle and architecture gate pass, and reuse of sealed aggregate digests
  removes about 434 ms of duplicate debug serialization. The remaining roughly
  737 ms exposed a checked-type-table round trip. The next flag-day cut is now
  landed as `SemanticLoweringContractV2`/`CanonicalProgramCoreV2`: full
  expression/function lowering inventories and all three full checked tables
  in the runnable core are deleted. The remaining lowering named-value metadata
  projects only a narrow transitional interface; distributed values use exact
  executable identities, and remote function contracts come from exact sealed
  producer materializations. Fresh debug NovyWave evidence has 1,885 lowering
  rows, 120.7 ms metadata generation, a 10,640-node/80,698-edge graph, and a
  12.67 s focused semantic run. This is progress, not acceptance. Continue with
  optional diagnostic source maps, deletion of the transitional named metadata
  into storage-owned interfaces, and direct
  construction/sealing of resource/reactive/storage/view/memory table and CSR
  spans; delete each replay scanner and rich duplicate owner without a
  compatibility path;
- preserve the first reactive owner deletion: read construction now publishes
  exact trigger routes instead of making trigger planning rediscover lexical,
  owner, and call ancestry. Its build-local exact `(root, terminal)` trigger-
  plan index reduces NovyWave state-arm work from about 962.0 to
  295.4--309.5 ms and the reactive phase from about 1,172.9 to
  496.9--513.2 ms while the exact oracle
  passes. It is revision-local and cycle-rejecting, not a persistent whole-
  project cache. Do not micro-tune its residual cost. Delete the roughly
  1,824.5--1,848.1 ms execution-image finalization scan and roughly
  1,695.0--1,714.8 ms Manifest
  re-import owner next; if trigger planning later dominates, replace its
  recursive expansion with one normalized dependency/SCC plan and shared arm
  spans;
- apply the post-`ac2b234` phase-ownership audit before another micro-
  optimization. Resource construction currently mutates execution to
  synthesize inline list-authority expressions/statements and backpatches
  materialization row bindings, then copies those bindings into its own table.
  Move authority normalization into `ExecutionBuilding`, seal execution once,
  make the resource table the sole source/target/lineage owner, and migrate all
  consumers. Emit final typed rows, entity routes, component receipts, and CSR
  relocations from their construction owners; delete `execution_for_resource`,
  the post-hoc execution handoff, repeated whole-execution validations, and the
  execution/resource Manifest inventories as one coherent flag-day cut. Then
  extend the same publication rule through reactive/storage/view/memory and
  remove the duplicate canonical-core mapping/hash. Deleting one validation,
  sharing a hash buffer, packing the existing collector, or cosmetically
  splitting crates is not the exit;
- preserve the completed execution/resource ownership checkpoint: execution
  now seals before resource construction, the resource table alone owns
  materialization rows and lineage, and resource construction publishes 735
  typed proof rows directly. The exact ignored NovyWave oracle and architecture
  verifier pass, while a focused debug trace still reports 1,829.237 ms for
  execution-image finalization, 604.621 ms for resource derivation, and
  1,701.115 ms for Manifest with 35,448 replay rows remaining. Treat this as
  proof that the next work is the larger definition-receipt/compact-invocation
  execution architecture and direct row/CSR publication, not resource-row or
  serialization-buffer micro-tuning;
- preserve the completed V3 executable-receipt cut. Stable checked-definition
  routes, dense parent-linked invocation overlays, construction-bound entity
  routes, final executable-row receipts, and one CSR relocation arena now seal
  `SealedSemanticImageV3`. Production publishes 30,771 rows, 4,158 projections,
  and 9,037 relocations instead of the V2 47,296/6,483/16,834; the cumulative
  path arena, rich V2 mirror, duplicate expression-origin receipts, and legacy
  owner rediscovery are `cfg(test)` parity oracles only. Manifest follows direct
  definition, authored-call-site, and parent-overlay edges. The exact ignored
  NovyWave V2/V3 owner oracle and architecture gate pass, and the direct debug
  verified sample keeps plan hash
  `db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`;
- follow the post-`addb056` whole-system architecture, not a sequence of local
  loop or hashing tweaks. Cold and warm compilation use one persistent
  definition-to-runnable request graph: immutable per-unit syntax snapshots,
  stable interfaces and definition receipts, demanded executable definition
  shards, compact invocation overlays, construction-owned domain summaries,
  one relocation/SCC link, and a consuming runnable builder. The V2 mirror is
  deleted from production; next remove Manifest's compact receipt re-import by
  making the shared summary/link graph authoritative, then connect parser unit
  artifacts and request backdating to `CompilerSession`, make verified intent
  the sole demand queue, delete repeated ordinary-body lowerers, and replace
  repeated distributed-role elaboration with delta linking. A whole-program
  cache beside the old passes is rejected;
- apply the post-`96b1611` seventh current-tree audit when choosing the next
  tranche. One directional debug sample is still 4,029.882 ms/257,892 KiB, with
  91.054 ms parse, 691.933 ms typecheck, 2,284.212 ms semantic, 724.634 ms
  backend, 104.649 ms plan validation, and 553.520 ms serialization. The next
  cut must consume the V3/remaining-domain projection registry directly into
  one retained revision-zero request graph, derive the compact Manifest proof
  summary from it, and delete Manifest's second owner/projection/edge import.
  Then retain structurally shared parser units and checker interface/definition
  result cells in `CompilerSession`; do not add a final-artifact cache or query
  shell around whole reparse/recheck. Follow with the one ordinary definition-
  code linker, consuming runnable builder, and distributed delta linker. Only
  split crates at those proven one-way seams and require measured Rust
  invalidation improvement separately from Boon latency;
- preserve the first seventh-audit request-graph owner deletion. Manifest V7
  now publishes finalized projection receipts and checked/execution/remaining-
  domain plus owner edges into one graph and seals that graph as a retained
  revision-zero request snapshot. Root/callable proof digests and session
  currentness share its exact identity/CSR/SCC/memo authority; the second
  Manifest registration/edge-import graph is deleted, and the normal sealed
  runtime artifact does not carry compiler currentness state. The current
  directional evidence is 8,315 nodes/29,131 edges, 415.329 ms Manifest work,
  and 4,112.475 ms/260,660 KiB overall with the unchanged plan hash. This is not
  a latency/RSS gate or warm-reuse result. Locally checkpoint it, re-audit the
  whole pipeline for larger refactors, then retain parser-unit and checker
  interface/definition results with exact dependencies, backdating, and zero-
  unrelated-work evidence before proceeding to demand/link/runnable cuts;
- follow the completed post-`d177af9` definition-artifact/thin-link research,
  not another local container optimization. The retained graph remains the
  revision-zero semantic proof snapshot, but compiler evaluation/currentness
  dependencies are a separate typed edge plane from cyclic proof/link
  relocations in the same database and identity registry. First add parser-
  owned structural item/occurrence routes, a body-insensitive unit item index,
  typed request revision/backdating, interface SCCs, and immutable checked-
  definition results. Emit checked receipts during construction and delete the
  approximately 392 ms production checked-image rescan. Then carry each
  demanded definition through one semantic/plan-code artifact, delete its old
  OUT/contextual and three backend recursive-body owners, thin-link domain
  summaries/relocations, and consume the linked image into one runnable
  machine. Normal preview does not serialize pretty JSON. Distributed roles
  publish deltas instead of full confirmation elaborations. The exact flag-day
  sequence and rejection rules are in
  `BOON_COMPILER_DEFINITION_ARTIFACT_RESEARCH.md`;
- preserve the first post-research syntax/session tranche. `ParsedSourceUnit`
  owns a body-insensitive item index and stable definition keys; a session
  retains unit syntax by `SourceUnitId`, reparses only changed identities, and
  applies upsert/remove/rename atomically. Producer format V4 exposes attempted,
  parsed, and reused units, and the warm verifier requires exactly one parsed
  plus `N - 1` reused for a one-unit edit. Direct and cached project assembly,
  stable-key, topology, diagnostics, and verified-session focused tests pass.
  Do not call this warm completion: project assembly and typechecking remain
  whole-program. The following checkpoint completes structural occurrence
  routes; continue with typed evaluation request slots, interface SCC/
  definition results, and deletion of the production checked-image scan;
- preserve the structural occurrence identity tranche. Parser-owned unit/item/
  statement/expression routes now identify checked calls and pipes without raw
  source substrings, offsets, lines, or dense ids; the checker deletes its
  identical-source counting pass. Exact stability tests and the canonical
  NovyWave plan hash pass. The initial post-parse route traversal adds roughly
  16--24 ms to directional debug parsing, so do not call it a latency win or
  tune its containers next. Implement typed request currentness plus interface/
  definition artifacts, emit checked receipts during construction, and delete
  the approximately 392 ms checked-image rescan before revisiting compact route
  storage;
- preserve checkpoint `42c1aa9`'s typed, generation-safe shared evaluator and
  exact syntax request spine: parse unit, body-insensitive unit link summary,
  project namespace plan, per-module index, per-unit link overlay, and linked
  unit. Automatic `require` edges, full indirect-cycle paths, reverse-edge
  replacement, backdating, tombstones, cancellation/supersession accounting,
  and fail-closed raw reads are now the only production evaluator path. The
  module-interface test must keep relinking only the affected module and a
  body-only edit must keep unrelated linked units pointer-shared. This is not
  M2 or a warm-gate pass: `LinkUnit` still rewrites a cloned AST and
  `ProjectState.checked` still forces whole-project checking. Build the complete
  authored-owner plus unit-root input/source-map/constraint/interface-SCC/body
  spine next, then delete the whole-project checked owner and production
  `checked_image_handoff`. Do not substitute request-container micro-tuning or
  Polars-style unsafe for these owner deletions; reconsider narrow unsafe
  shard/CSR kernels only after a profile shows safe mechanics dominate the new
  architecture and require safe API encapsulation plus parity and Miri/fuzz
  evidence;
- preserve the post-`2a84c47` safe owner-representation checkpoint. It reuses
  variable-free shared types, seals body/shard content through compact
  construction receipts, removes duplicate checked-row receipt storage, and
  makes shard/source-map/assembly proof payloads externally immutable with
  exact ABI/role input checks. A memory-capped debug NovyWave empty-session
  observation preserves
  `9b5abdb1d09d2658ce75fbfa86916a06054080fc9550cbc753fc484e0dab540f`
  while moving from 6,948.653 to 6,118.881 ms, 27,146,292 to 25,198,810
  allocations, and 347,876 to 335,496 KiB. TodoMVC moves from 2,615.818 to
  2,388.892 ms with its exact hash. This is not a gate pass. Independent audits
  require complete invalid-diagnostic parity, a `DiagnosticsAggregate` demand
  root that constructs zero checked/dense/executable rows, authoritative
  resource/order/project facts, and deletion of compatibility semantic
  recomputation, `ProjectState.checked`, old production checker entrypoints,
  and `checked_image_handoff`. Do that architecture work next. Consider narrow
  unsafe `TypeUnifier`/CSR/packed-row kernels only afterward if a fresh profile
  proves safe mechanics dominate and a safe A/B oracle, invariants, parity,
  Miri/fuzz evidence, and material whole-run improvement all pass;
- preserve the 2026-08-04 staged `OwnerDiagnosticsAggregate` correction. The
  internal request globalizes owner-local spans through project source layouts,
  seals exact owner/body/source-map coverage, and proves zero construction ABI,
  checked-shard, compatibility, or executable requests. Independent reviews
  rejected the initial public cutover because project-level forward-OUT/order/
  output/render/host diagnostics and editor presentation facts were absent, and
  because speculative record/pattern/context scope scans were not semantically
  sound. Public `CompileIntent::Diagnostics` therefore still uses complete
  checked assembly; the transient approximately 3.72-second lean NovyWave probe
  is not an accepted score. Build one authoritative per-owner lexical/import
  plan with exact projection and shadowing before project/ABI resolution,
  memoize compact SCC/result-transfer dependency slices once, then publish
  construction-independent owner/project diagnostic facts and an honest
  optional editor-presentation sidecar. Cut over and rescore only after the
  complete multi-unit invalid oracle and editor projection pass. Consider
  narrow profiled unsafe kernels only after those architectural deletions;
- follow the post-`e510726` macro-architecture audit, reconciled through
  `a48f488`, in
  `BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md`. The historical trace exposed
  global syntax assembly and checked-report ownership; the current post-M1
  trace reports zero rebased nodes but still spends about 763 ms in verified
  typecheck, 2,276 ms in semantic construction, 832 ms in backend work, 102 ms
  in plan validation, and 531 ms in explicit export serialization. It allocates
  about 1.61 GB cumulatively. Do not tune the structural-route containers.
  Next delete the packed-syntax/dense-checked fallback identity plane and make
  project linking an immutable overlay; then install a real typed evaluator,
  interface SCC/definition requests, and direct receipts that delete the
  checked handoff. Continue through normalized domain facts, shared plan-code,
  thin link, compositional phase seals, and consuming runnable publication;
- preserve the landed diagnostics/runtime capability split. Diagnostics owns a
  completed `CheckedProgramConstruction` and performs no checked-image handoff;
  verified preview consumes and seals those exact checked fields later without
  another parse or type solve. Focused digest rejection and exact sealed-
  program/session parity pass. Fresh debug NovyWave diagnostics is
  422.445 ms/92,432 KiB (120.842 ms parse, 292.423 ms typecheck), and traced
  `assemble_report` is 2.083/2.133 ms instead of 408.603 ms. The verified plan
  hash remains `db18f345676378b8633829c0bbd7870c0a1dc5a2459649c9bbfdd6b8969374ab`.
  At that checkpoint this was not M1/M2 or latency closure: checking and syntax
  were whole-project, verified publication still ran the 63,657-row scanner,
  and the warm gates remained red. M1 is now checkpointed at `a48f488`; retain
  its exact diagnostics/artifact parity and zero-rebase result. Do not build M2
  on the remaining ambiguous ID APIs. Establish typed unit syntax, checked
  local, stable owner, and linked-image identities; make project linking an
  overlay; then make `CompilationDb` execute and retain typed requests rather
  than merely storing a post-build graph. Definition checking must emit
  receipts that delete the deferred scan rather than rechecking after
  diagnostics;
- treat the 16.7 ms diagnostics and 100 ms replacement-preview gates as warm
  incremental contracts. A faster full rebuild cannot satisfy them. Record
  executed/reused/backdated requests and require zero unrelated parse/check/
  semantic/proof/plan work for a constant-cone edit;
- move verified-intent demand collection before occurrence expansion and
  backend lowering. Split top-level authority/interface summaries from the
  final program-link sink, and version the callable interface schema before
  adding context-scheme facts. Land the eventual consuming plan builder and
  `SealedRunnableMachine` as one owner-deletion tranche; a seal wrapper that
  preserves completed-plan rewrites or per-consumer executor metadata is not a
  checkpoint;
- do not preserve V3's 208k subject count in the replacement proof. The
  test-only oracle maps every historical subject to one canonical finalized
  shard row and classifier field/domain and independently proves coverage,
  projection commitments, mutation detection, and exact cones. Production
  fingerprints each actual database row once, binding all fields plus one
  typed dependency span. Reject one-receipt-per-historical-child-field designs;
- expand that vertical cut through the rich semantic domains, deleting each
  superseded production owner. Link ordinary executable definitions once
  across document, row/scalar, and migration domains with resolved compact
  invocation frames instead of recompiling exact calls. Runtime never consumes
  a semantic AST or flat fallback. Replace full-plan clone/rewrite/compact/hash
  finalization and per-consumer executor metadata reconstruction with one
  consuming `SealedRunnableMachine` builder whose dense indexes are built once.
  Normal in-memory publication does not retain complete IR/semantic products or
  pretty JSON; explicit debug/serialized intents own extra products and
  untrusted deserialization verifies/builds indexes once. Retain the same
  source/shard/link/proof/plan/runnable requests across revisions with atomic
  upsert/remove/rename, exact reverse cones, backdating, worklist cancellation,
  latest-generation publication, and clean-full parity. Move
  cross-layer IDs and invert compiler/runtime, migration-harness,
  effect-adapter, and content-store
  dependencies only when before/after closure and rebuild measurement proves
  the cut. No compatibility re-export, cosmetic file split, or smaller Rust
  rebuild may be counted as a Boon latency win;
- the current compiler-throughput checkpoint keeps Counter, physical TodoMVC,
  and NovyWave `MachinePlan` output deterministic while replacing copied OUT
  type environments with active-path overlays, retaining canonical-root-reading
  ordinary callables, replacing repeated whole-program type inference with a
  dependency-indexed dirty worklist plus a fail-closed full-sweep audit, and
  replacing manifest V2's per-owner closure enumeration with an exact
  content-addressed SCC graph proof; both inference worklists now exhaust with
  clean no-change audits, including exact actual-to-formal parameter and
  selector-dependent arm-scope invalidation; the dense nearest-pattern-arm
  index replaces a quadratic forwarding query, reducing contextual owner
  propagation from about 1.34 seconds to 18 ms and the traced typecheck from
  5.79 to 4.59 seconds. Dense reverse semantic reachability now replaces one
  DFS per runtime-root/effect and state-arm/effect pair, reducing NovyWave host-
  effect scheduling from about 1.51 seconds to 5.7 ms and the complete reactive
  phase from about 1.87 seconds to 0.35 seconds. Ordinary-callable eligibility
  analyzes each body once and propagates rejection through a reverse dependency
  worklist instead of repeating whole-body fixed-point scans. A post-reboot
  debug measurement completes Counter in 0.09 seconds, physical TodoMVC in 2.02
  seconds, and NovyWave in 20.68 seconds at 1,000,416 KiB peak RSS; the
  historical artifacts emitted SHA-256 values
  `dc1fe51b659d1746a0b0b4ae2dcba21d50a9426499eb2bde28dbed988e6cfb08`,
  `c9a12cd0a1bcf748a20e3a072afa09d0f923c2c9dbd664f2343d343494404f96`,
  and `4d3c284a9240cdc68c70aff7f30c570367e285cc1e8f823585900829bafd8ff7`.
  These identify that measurement only; the invalid NovyWave artifact is not a
  current semantic oracle;
- after the current diagnostics tranche passes, the full-plan blockers remain
  the historically measured 2.84 seconds of contextual materialization/
  execution expansion and 3.81-second whole-program dependency proof, plus
  backend/hash/memory closure. Retain more shared semantic callable definitions,
  invalidate only affected contextual instances, and keep optional flattened
  proof/debug sidecars outside the interactive path. Cold improvements do not
  replace the required persistent compiler session or its warm-edit/switch/
  cancellation gates; do not return to tactical type-cache invalidation or
  another manifest-only tweak after the same owner fails twice;
- FjordPulse currently has no basis for weakening the 108-story/340-scenario
  acceptance inventory. Only its two explicitly deferred backup/restore
  automation scenarios may retain that final status.

Execution strategy:

- Work in the exact order in `steps.md` and the phases below.
- Execute all internal phases of `BOON_COMPILER_PERFORMANCE_PLAN.md` before
  entering unified Phase 0 below. The similar phase numbers belong to different
  plans; unified Phase 0 is not an alternate documentation task that can bypass
  the blocking performance prerequisite. Within that prerequisite, use
  `BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md` as the current refactor order.
- Work in large coherent ownership slices. Temporary compile breakage is
  acceptable inside a flag-day slice; do not preserve two execution worlds to
  keep an intermediate tree green.
- Preserve correct current implementation. Audit and adapt it instead of
  rebuilding a subsystem merely because a newer plan names the final contract.
- Run targeted parser/typecheck/semantic/verify/IR/plan/executor/runtime tests at
  slice boundaries. Run broad workspace, product, and report gates only at major
  milestones. Generate final reports only after the final tracked edit for the
  relevant milestone.
- Keep compiler development feedback practical. Use focused debug-profile
  checks while fixing semantics and correctness; reserve release builds and
  end-to-end performance reports for completed milestone candidates. Run only
  one Cargo invocation at a time, normally with two build jobs on the reference
  machine; never overlap independent Cargo builds or test suites. Split crates
  only at the performance plan's stable ownership and dependency-invalidation
  boundaries, with an atomic no-compatibility cutover and unchanged artifact
  proof boundaries.
- Preserve the current dev/test profile intent. Do not add LTO, one-codegen-unit
  builds, `target-cpu=native`, global `RUSTFLAGS`, extra compiler threads, or a
  custom acceptance profile without a same-source A/B report covering Rust
  build wall/RSS and Boon latency/RSS/work/artifact identity. A crate split must
  measurably shrink an affected rebuild set or establish a required ownership
  and invalidation boundary; a file-only move is not an optimization.
- After one required Rust build, invoke the built `boon_cli` and focused test
  binaries directly for repeated Boon fixtures. Do not pay a Cargo graph scan
  and test-harness relink for every example or every unchanged focused check.
- Follow the performance plan's measurement loop: select one dominant owner
  from phase time plus real work counters, state the invariant and expected
  counter reduction, implement one coherent change, run focused correctness
  oracles, and directly remeasure Counter and NovyWave before choosing the next
  owner. Use small explicitly non-acceptance preflight samples before the full
  three-setup/30-scored protocol. If the expected work does not fall, reassess
  the architecture rather than accumulating local patches.
- Treat compiler timeouts and graph explosions as architecture failures, not as
  requests for larger timeouts. The former 120-second rule and debug fixture
  ceilings are historical emergency bounds, not acceptance targets.
  `BOON_COMPILER_PERFORMANCE_PLAN.md` is the sole numeric authority: both its
  fresh-process and empty-session cache-disabled cold gates must pass before
  persistent state or caches may satisfy any warm gate. Budgets may only
  tighten unless a changed represented workload is documented with exact
  before/after evidence.
- Parsing, typechecking, semantic invalidation, exact callable-dependency
  sealing, and verification required for an executable preview are on the
  interactive path defined by the compiler-performance plan. Flattened proof
  and debug graphs, handoff reports, and large serialization are not. Never use
  fast UI scheduling, an incomplete diagnostic profile, or an artifact cache to
  conceal unchanged cold compiler graph explosion.
- After the same blocker class appears twice, stop tactical patching and change
  the owning parser, compiler, proof, runtime, currentness, document, renderer,
  host, persistence, physical-layout, or verifier architecture.
- Use subagents for disjoint compiler, proof, packed runtime, distributed,
  streaming, product, hardware, and adversarial-review boundaries.
- Before advancing past any unified phase or numbered `steps.md` exit, assign at
  least one fresh-context read-only adversarial subagent to map every applicable
  plan item to live implementation and current evidence and to seek omissions,
  compatibility paths, stale reports, or weakened acceptance. Any finding
  reopens the owning work; rerun affected evidence after fixes.
- Before every compiler-performance phase-exit claim, use a fresh-context
  read-only subagent to try to disprove it. Before leaving the complete
  performance prerequisite, run the performance plan's three disjoint final
  reviewers for implementation completeness, measurement integrity, and
  semantic/architectural soundness. Subagents may inspect concurrently but may
  not launch Cargo, producers, collectors, or other heavy commands; the primary
  agent serializes those commands and shares their exact evidence.
- Delete superseded syntax, aliases, codecs, plans, tests, runtime paths,
  representations, and compatibility fallbacks once replacements compile. Do
  not rename, quarantine, feature-gate, or retain them as a second path.
- Do not start or continue a deletion slice merely to reduce repository-wide
  source or test line telemetry. Name the duplicate or superseded owner, the
  surviving owner, and the behavioral evidence first.
- Never hide an engine limitation in example Boon source. Reduce it to an
  unrelated fixture, fix the generic owner, then remove the diagnostic
  workaround.
- Keep command/report output bounded. Use focused filters and jq summaries; do
  not dump large report bodies into the conversation.
- Add no Python source, scripts, invocations, or generated Python artifacts.
- Do not commit or push unless the user explicitly requests it.
- If checkpoint commits are authorized for the goal invocation, a successful
  commit is persistence only: immediately continue with the next red or missing
  gate. Do not end, pause, or report the compiler-performance prerequisite as
  complete because documentation, instrumentation, a crate boundary, a focused
  test, or a directional benchmark landed.

Blocking compiler-performance prerequisite:

- Complete `BOON_COMPILER_PERFORMANCE_PLAN.md` before resuming the remaining
  simplification/native-recovery closure or any later production phase.
- Begin from checkpoint `d177af9`, which preserves `d113544`'s whole-program
  ownership audit, `96b1611`'s compact execution receipts, `c870358`'s
  compilation database, `38e6541`'s compact-proof/sealed-plan work, and
  `32bcf40`'s activation/effect boundary. Follow the post-checkpoint definition-
  artifact sequence: immutable unit syntax and body-insensitive item indexes;
  parser-owned stable definition/occurrence routes; separate canonical
  snapshot, session lineage, semantic/persistence, and dense identities; typed
  evaluation/currentness edges distinct from proof/link relocations; interface
  SCC and checked-definition shards; demanded definition executable artifacts
  with compact invocation frames; construction-owned domain artifacts and thin
  linking; and one consuming `SealedRunnableMachine` builder with dense
  executor indexes built once.
  Explicit diagnostics, verified-preview, serialized-artifact, debug-IR/debug-
  plan, and distributed-link intents own their products. The ordinary trusted
  preview must not retain complete IR or rebuild plan verification/runtime
  metadata at each handoff; deserialized/untrusted plans verify/build indexes
  once. Preserve and complete the real-host migration/restart/provenance oracle
  before phase acceptance, but do not delay the active compiler cut for it. The
  package-local dev profile choices are already present; do not repeat a stale
  parser-profile task. Use focused correctness and bounded direct release
  preflight while changing each owner, then regenerate the full current two-job
  release protocol at milestone candidates. Do not substitute a crate split,
  a packed version of the exhaustive entity graph, or a fast subphase for the
  complete diagnostics or verified-plan gate.
- Pass the fresh-process and empty-`CompilerSession` no-cache gates first, then
  the warm edit, loaded switch, cancellation, invalidation-locality, scaling,
  deterministic-artifact, and RSS gates. Persistent compiler state is the
  second layer, never a substitute for the cold result.
- Every request that emits an executable artifact still seals exact semantic
  dependencies and crosses `boon_verify`. A diagnostics-only request may stop
  after a complete `CheckedProgram`, publishes no runnable artifact, and is
  generation-labeled until the corresponding verified result exists.
- Compiler changes make earlier native reports stale. Refresh native handoff
  evidence only after this prerequisite and its native timing cutover pass.
- Missing or failing performance evidence is executable work for the active
  goal, not a reason to yield a completion response. Continue through measured
  optimization slices and authorized checkpoints until every performance-plan
  Clear End Condition is green or a genuine external blocker satisfies the
  goal system's blocked-state rules.
- Passing numeric reports is necessary but not sufficient. The three final
  adversarial performance reviewers must also confirm that every applicable
  planned optimization/deletion/boundary is implemented and used, no shortcut
  or duplicate hot path remains, report provenance is current, and all cold,
  warm, scaling, cancellation, determinism, RSS, and native timing requirements
  pass. Any finding reopens its owning phase; fixes invalidate affected reports,
  which must be regenerated before all three reviews repeat. Do not begin step
  2 until the manifest-backed compiler-performance closure validates both
  performance reports and all three current review sidecars.

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
- Preserve ordinary callable identity instead of recursively cloning every user
  function body into each call site. Expand only contextual functions and
  transparent wrappers whose semantics require specialization; prune statically
  unselected structural branches before OutNet, dependency-manifest, and proof
  graph construction. Add scaling fixtures that vary call depth, call count,
  and static branch count and assert bounded artifact growth as well as result
  correctness.
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
`ErasedProgram` boundary, the OUT structural/compiler slice passes, ordinary
calls retain callable boundaries, static dead branches do not enter downstream
semantic/proof graphs, and representative debug compiles complete without the
120-second recovery timeout. The full OUT Clear End Condition remains open
until Phase 4.

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
