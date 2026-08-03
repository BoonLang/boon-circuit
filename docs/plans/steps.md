# Boon Implementation Order

`GOAL_PROMPT.md` is the complete execution contract. This file fixes sequencing
only; linked plans remain authoritative for their own semantics and acceptance.

Before advancing past any numbered step, assign at least one fresh-context,
read-only adversarial subagent to map that step and every completed linked-plan
item to live implementation and current evidence, actively looking for omitted
work, compatibility paths, stale reports, weakened acceptance, or a false exit
claim. A finding reopens the owning work and any corrective edit invalidates
affected evidence. Review agents do not run Cargo or heavy verifiers
independently; the primary agent serializes those commands. Step 1 has the
stronger three-reviewer performance closure defined below.

1. Complete `BOON_COMPILER_PERFORMANCE_PLAN.md` before the remaining active
   recovery exit or any later production phase. Publish and reconcile its
   documentation first, then make both cache-disabled cold modes pass their
   diagnostics, verified-plan, memory, determinism, and scaling gates before
   relying on persistent sessions or immutable artifact reuse. Preserve the
   mandatory verified-artifact spine, complete diagnostics, proof soundness,
   persistence identities, and current language semantics. Use one Cargo build
   or test suite at a time, normally with two build jobs on the reference
   machine; invoke prebuilt binaries for repeated measurements. Use the
   performance plan's measured flag-day crate boundaries instead of overlapping
   Cargo producers or cosmetic file-only splits.
   Treat artifact sizes only as cardinality evidence: parser inspection,
   typechecker worklist/cache/replay, semantic/proof traversal, and backend work
   counters own scaling gates. Build the release producer explicitly once;
   performance verifiers must never start a nested Cargo build.
   Execute the current architectural tranches in
   `BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md`; that file is subordinate to
   the performance budgets and does not create an earlier exit.

   Preserve the landed independent-unit parser, `boon_syntax`/`boon_checked`
   cutovers, structured work counters, indexed parser validation/assembly, and
   deterministic non-recursive checked-diagnostic projection. The current
   `677d09d` release candidate passes the six direct cold diagnostics time/RSS
   combinations, but NovyWave empty-session has narrow p95 headroom and that
   evidence is stale after the next frontend edit. The parsed snapshot and both
   checked construction owners are now lifetime-free without a material
   allocation regression. Dense solver queues now retain their buffers and one
   complete dependency index owns expression propagation, reducing a current
   NovyWave sample by 1,226 allocations without changing its digest. Exact
   call-input sensitivity then removes concrete fixed-result builtins from
   redundant input-driven instantiation while retaining projection/cache
   dependencies: call visits fall from 5,060 to 3,848 and no-op visits from
   1,964 to 752 with the full audit and digest unchanged. Directional release
   samples are 229.97/228.51 ms fresh/empty and the fresh sample uses 1,929,058
   allocations / 219,061,685 bytes. Recursive checked-flow inference now keeps
   one immutable parsed-program owner per root rather than cloning/dropping it
   on every cache miss; solver/allocation work is unchanged, while a three-
   sample directional release batch has 227.33/227.73 ms fresh/empty medians.
   Dense FLUSH propagation then replaces full-program fixed-point rescans with
   the authoritative reverse dependency graph, while inline AST-child buffers
   remove per-node temporary vectors. A six-sample directional release batch
   has 224.50/225.92 ms fresh/empty medians and 1,827,343 fresh allocations /
   217,055,736 bytes; the complete suite, FLUSH oracle, and digest pass. Checked
   read plans now borrow AST path segments while indexing, retain a canonical
   path only for unresolved reads, and share the authoritative declaration-
   reader table with invalidation. Contextual setup also consumes indexed
   selector/domain/signature state in place, and the final flow adjacency is
   filled without a transient 29,812-edge tuple buffer. The complete 80-test
   suite and exact digest pass; a six-sample directional release batch has
   220.22/221.15 ms fresh/empty medians, 165.15/165.51 ms typecheck medians, and
   1,791,466 fresh allocations / 213,748,071 bytes. Signature-to-declaration
   publication now writes through the owned dense declaration index instead of
   constructing two ordered type snapshots and a third update vector at every
   registry synchronization. The exact digest and complete suite pass; fresh
   allocation work falls to 1,789,024 calls / 212,744,182 bytes. A six-sample
   confirmation batch is timing-neutral/noisy at 222.34/219.94 ms fresh/empty,
   while the traced signature-sync subphase falls from about 0.78 to 0.40 ms.
   The rejected changed-signature/PASSED-cone experiment is now decomposed:
   generic declaration writes journal the exact callable signatures they
   disturb, and the unchanged synchronization boundaries publish only that
   journal plus explicitly changed signatures. The pre-change NovyWave tuple
   oracle, exact digest, and all 80 tests pass. Fresh/empty allocation work falls
   to 1,788,315/1,788,321 calls and 212,679,422/212,744,019 bytes. A six-pair
   batch remains bimodal at 225.75/231.58 ms total and 169.80/173.69 ms
   typecheck, so this is not a latency claim. Signature-owned callable
   publication is now explicit, structural/solver/final value lanes reject
   callable IDs, and the independently retried PASSED worklist follows only the
   reverse lexical-PASSED caller cone. NovyWave context visits fall from 504 to
   369 while all 369 changes, the tuple oracle, exact digest, and 80 tests remain
   unchanged. Fresh/empty allocation work is 1,787,633/1,787,639 calls and
   212,614,838/212,679,435 bytes. A six-pair batch has 219.70/219.39 ms total and
   164.18/164.53 ms typecheck medians, with one slow outlier per mode. The
   context phase remains about 10.52 ms because it builds complete ordered call
   maps before pruning, and the 35 rounds remain. Move that cone into a compact
   indexed owner. That cutover now projects immutable expression owners once to
   dense signature ordinals and per-signature expression/root slices, borrows
   PASSED paths, and uses dense leaf/requirement/recursion/worklist state. The
   legacy owner/root oracle, fixed tuple oracle, exact digest, and full suite
   pass. Fresh/empty allocation work is 1,758,855/1,758,861 calls and
   211,969,403/212,034,000 bytes. Six-pair medians are 216.62/217.17 ms total and
   161.95/162.37 ms typecheck; traced context work falls to 8.22 ms and the
   checked builder is directionally 117.37 ms. The remaining measured owners are
   about 9.08 ms of parameter schemes, 5.17 ms of structural schemes, and 43.23
   ms/35 rounds of checked inference. The next solver slice cold-seeds the 618
   generic input-insensitive/fixed-product calls before the first expression
   wave and preserves the ordinary order for all input-sensitive calls.
   NovyWave falls to 34 rounds, 34,653 expression visits, 690 callable visits,
   and 3,484 call visits; the fixed tuple oracle, clean audit, exact digest, and
   all 80 tests pass. Fresh/empty allocation work falls to
   1,732,729/1,732,735 calls and 210,213,894/210,278,491 bytes; six-pair release
   medians are 214.35/212.82 ms total and 158.87/158.18 ms typecheck. This is
   directional rather than Phase 1 acceptance. Checked construction, ordered
   diagnostics, and report assembly are now fused into one
   `CheckedProgramDatabase`; the two prior owners and transfer bundles, the
   post-seal external-environment clone, and compiler-proven dead recursive
   inference/report helpers are deleted. The source diff is net 1,153 lines
   smaller. The fixed tuple oracle, exact digest, clean audit, all 78 ordinary
   tests, and both product gates pass with unchanged work. Fresh/empty
   allocation work is 1,732,728/1,732,734 calls and
   210,213,846/210,278,443 bytes; six-pair medians are 213.89/212.96 ms total
   and 158.70/157.43 ms typecheck. The unchanged 1m35s release rebuild keeps the
   measured crate/downstream-relink boundary open. Checked reverse dependencies
   now pack 154,585 possible base rows, 40,880 base edges, and 29,812 derived
   flow edges into immutable offsets/edge arrays instead of per-row vectors;
   the construction-only pattern column is not retained. The exact digest and
   every work counter remain unchanged, all 79 ordinary tests and both product
   gates pass, and fresh/empty allocations fall to 1,687,722/1,687,728 calls
   and 210,064,354/210,128,951 bytes. Six-pair medians are 208.89/209.55 ms total
   and 152.86/153.76 ms typecheck, with 65,768/66,400 KiB maximum RSS. The first
   worklist follow-up preserves unchanged widened list/object nodes and
   coalesces input-only retries only after two consecutive no-op/evidence-only
   visits, with mandatory refresh before the wrapper hook and fail-closed
   ordinary-solver repair. The exact digest, all 82 ordinary tests, and both
   product gates pass. NovyWave expression/declaration/call visits fall to
   34,502/1,876/3,388, input enqueues to 1,127, and no-op visits to 893; the
   release worklist falls from about 39.6 to 36.3 ms. Fresh/empty allocations
   are 1,666,870/1,666,876 calls and 208,381,392/208,445,989 bytes. Six-pair
   medians are 206.28/206.55 ms total and 150.21/150.89 ms typecheck, with
   65,932/66,352 KiB maximum observed RSS. The next checkpoint centralizes
   single-pass copy-on-write substitution and structural widening in
   `boon_checked`, preserves unchanged shared object/list/Tag nodes and field
   order, uses an inline substitution-cycle stack, and replaces formatted Tag
   sort keys with the exact allocation-free comparator. The exact digest,
   seven checked-graph tests, all 85 ordinary typechecker tests, and both
   product gates pass. Fresh/empty allocations fall to
   1,621,578/1,621,584 calls and 206,521,350/206,585,947 bytes. An 18-pair
   directional release batch has 204.74/205.60 ms total and 149.59/150.10 ms
   typecheck medians, with 223.03/223.09 ms maxima and 65,348/65,924 KiB maximum
   RSS. The first contextual ownership slice reuses the solver's packed call
   graph, packs PASSED reads without cloned projection paths, and stores
   structural statement children/values inline or densely. The exact digest,
   all 85 ordinary tests, both product gates, and solver work remain unchanged;
   fresh/empty allocations fall to 1,613,479/1,613,485 calls and
   206,406,282/206,470,879 bytes. The contextual graph lane falls from about
   0.28 to 0.03 ms, while an 18-pair system-noisy batch has 218.24/220.90 ms
   total and 159.53/161.72 ms typecheck medians, so this is not a whole-run
   latency claim. The first tranche still includes the contextual parameter/
   worklist and checked-inference owners, remaining construction/diagnostic
   work, measured name/type interning, scaling/parity evidence, and the fresh
   Phase 1 adversarial review. Reprofile after each owner-level slice and
   regenerate the complete cold protocol after the final edit. Direct verified
   NovyWave remains about 7.6--7.7 seconds and
   515 MiB, dominated by about 6.9 seconds of semantic work; keep that later
   verified time/RSS closure explicitly red. A fresh checkpoint-`c77dabc`
   architecture trace shows 17,716 checked expressions and 1,820 calls growing
   into about 45,000 semantic expressions, 5,146 OUT instances, 247,537
   dependency records, and a 248,201-node/1,060,194-edge proof graph, with about
   2.99 GB of cumulative allocation. Do not continue micro-optimizing the
   typechecker while this multiplier dominates. Pull forward the retained-
   definition/compact-invocation-overlay part of semantic sealing: retain pure
   type-polymorphic bodies once, carry parameter/PASSED/type/owner/effect and
   resource differences in dense overlays, count avoided specializations, and
   build proof facts from definitions plus overlays. Flattened expansion is
   test-oracle-only. Once cardinality falls, compact the sealed semantic
   artifact and close proof, backend, hashing, and memory until both verified-
   plan modes pass. Then return to remaining Phase 1 interning/scaling/parity
   closure; only afterward may persistent-session warm work satisfy its
   separate gates.

   The first 2026-08-03 retained-definition/invocation-overlay candidate cuts
   NovyWave fresh/empty verified time to 4,415.27/4,455.01 ms, semantic time to
   3,758.10/3,777.86 ms, peak RSS to 317,844/318,428 KiB, semantic nodes to
   16,521, and allocations to about 10.92 million calls/1.552 GB. This confirms
   the architecture and crosses the RSS gate directionally, but not the 1,000
   ms time gate. Its deterministic `f293e8a8...` plan hash differs from the
   historical `4d3c284a...`, but that old artifact is not a valid oracle: it
   omits reachable `Widest` from the persistence type of
   `store.selected_value_column_width_key`. The `c77dabc`-like and retained
   artifacts include the complete type and have byte-identical persistence
   sections. Keep budget V2 red; neither restore the unsound artifact nor bless
   the current hash for speed. Implement the performance plan's test-only flat
   differential oracle, stable-contract checks, plan/migration/restart and
   negative tests, and recorded V3 oracle migration first. Then lower
   definition-plus-overlay instances directly and replace the remaining
   post-hoc manifest path with construction-time row receipts, owner-local and
   projection Merkle roots, and a compact exact summary graph. The historical
   2,368.00 ms trace built 159,612 records and a
   160,276-node/512,204-edge graph; a directional existing-release trace taken
   after `968c56a` is still 2,350.35 ms for 159,652 records and a
   160,316-node/512,314-edge graph. Share the resulting sealed row
   fingerprints across later consumers and measure the runtime/compiler
   loading crate seam before any further small typechecker-container edits.

   The first feature-gated flat-oracle slice is implemented without a
   production mode switch. Counter and the focused four-state persistence
   fixture pass stable-contract and multi-turn runtime/document comparison; an
   explicit optimized NovyWave preflight passes structural contract, verifier,
   and exact `Compact | Normal | Wide | Widest` persistence comparison in
   14.82 seconds. Internal arena offsets and duplicated dirty/commit operation
   counts are not semantic identities; structural expression hashes and exact
   zero-unresolved invariants replace those raw comparisons. Keep the V2 budget
   red until V3 records source/old/new identities, the real-host NovyWave
   behavior trace, migration/restart evidence, and focused negative cases.

   Resume from clean local checkpoint `968c56a` and follow
   `BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md`. The canonical list-dataflow,
   sparse durable-overlay lifecycle, `boon_behavior_harness`, real local host,
   and retained document hit testing are landed. The headless harness has a
   28-crate normal forward closure versus native playground's 38, but its
   full-host restart path exposes two generic blockers: machine construction
   drops startup transient effects, and independently scheduled hosts produce
   different completion/revision order.

   First replace machine/runtime construction plus synthetic empty `mount`
   with one activation product carrying the exact initial turn. Then execute
   the real host once and replay the same logical effect-completion transcript
   into the second retained/flat candidate so exact turns, revisions,
   persistence, migration, and restart can be compared without normalizing
   causality. Finish the bounded oracle provenance and negative matrix on this
   route. Preserve the generic list contract and do not add row-handle
   serialization, projection-specific activation, example shortcuts, or a
   production flat fallback.

   The next compiler tranche is the architectural proof cut, not another
   packed container. During semantic construction, assign every executable row
   exactly once to a callable/program owner, compute its canonical row
   fingerprint, and record exact row/owner references. Fold owner-local Merkle
   roots plus stable narrow projection roots and run dependency SCC closure on
   that summary graph instead of automatically making every rich field record
   a node. The current 664 owners are an aggregation starting point, not
   permission to broaden exact dependency cones; compact typed row-level
   regions remain only where parity proves they are necessary. Keep the
   exhaustive V3 inventory test-only as an independent materializer until
   mutation, omission, cycle, cone-precision, stable-contract, and behavior
   evidence authorizes a flag-day V4 proof. Production then allocates no
   exhaustive dependency or coverage records.

   Continue by unifying the rich semantic graphs into one sealed database with
   shared row fingerprints and demand-collected plan instances, deleting each
   superseded production owner as parity lands. Replace full-`MachinePlan`
   clone/rewrite/compact/hash finalization with one builder seal. Only then
   promote independent parsed units and the existing owned checked database
   into the revisioned compiler service with stable owner/local identities,
   public/implementation fingerprints, exact cones, backdating,
   `changed_at`/`verified_at`, cancellation, and clean-full parity. Pull crate
   splits and runtime/compiler dependency inversion only when measured closure
   and rebuild evidence proves the ownership cut. Keep Rust rebuild speed and
   Boon compile latency as separate gates.

   Use the performance plan's edit-loop, milestone-preflight, and full-
   acceptance harness levels. Focused debug tests and direct one-sample producer
   runs are the normal edit loop; one current two-job release build feeds
   repeated direct samples; three-setup/30-scored reports run only for a
   candidate that passed preflight. Do not change LTO, codegen units, target CPU,
   compiler threads, timeouts, or profiles without before/after build-cost and
   Boon-runtime evidence. A crate split must reduce a measured rebuild set or
   establish a required ownership/invalidation boundary, preserve artifact and
   diagnostic parity, and immediately enable the next optimization.

   Documentation, instrumentation, a crate split, a focused test pass, or an
   authorized checkpoint commit is not this step's exit. After each checkpoint,
   continue with the next red or missing performance gate in the same goal run.
   Do not start step 2 until the performance plan's complete cold, warm,
   cancellation, invalidation, scaling, determinism, RSS, and native timing
   Clear End Condition passes from current evidence. Then run its three fresh-
   context adversarial subagent reviews for implementation completeness,
   measurement integrity, and semantic/architectural soundness. Review agents
   are read-only and the primary agent serializes every Cargo/producer command.
   Any finding reopens the owning performance phase; after fixes, regenerate
   stale reports and repeat all three reviews. The manifest-backed compiler-
   performance closure must validate both performance reports and all three
   current review sidecars before starting step 2.

2. Finish the active
   `BOON_CIRCUIT_SIMPLIFICATION_AND_NATIVE_RECOVERY_PLAN.md` exit before adding
   production compiler targets, hardware crates, RTL, a console bridge, or game
   work. Preserve the current verified-artifact and typed-list checkpoints.
   Judge that exit by its ownership, behavior, native evidence, and focused
   subsystem gates. Repository-wide tracked-Rust and test-Rust totals are
   telemetry and must not prolong recovery through deletion for its own sake.
   Documentation, board inventory, and measured tool/interpreter experiments
   may proceed, but do not create a production bypass around unfinished
   recovery.

3. Establish the final verified compiler spine and OUT ownership with
   `BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md` and
   `TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md`. Pull forward only the
   `boon_semantic`, `boon_verify`, `ContractVerifiedProgram`, and opaque
   `ErasedProgram` infrastructure required from formal phases 0–1. This does
   not complete those formal phases or the OUT Clear End Condition.

4. Land the language-foundation and structural-inference implementation on the
   final exact value algebra. Keep formal-dependent acceptance open until
   step 6.

5. Land `TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md` on the final value
   algebra and verified artifact spine. Keep its formal-dependent Clear End
   Condition open until step 6.

6. Complete `BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md` phases 0–5, then
   rerun and close every acceptance/Clear End Condition from steps 3–5.

7. Complete only the packed hardware prerequisites needed to cross from
   verified `MachinePlan` into normalized hardware artifacts: fixed widths,
   shape/offset access, bounded storage, target eligibility, dense IDs, and no
   recursive `Value`, runtime string lookup, or tree collection in hardware IR
   or cycle execution. `BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md` still
   owns the universal software runtime; this step does not claim its phases,
   flag-day deletion, product-scale reports, or Clear End Condition.

8. Complete `BOON_CONSOLE_IMPLEMENTATION_PLAN.md`: generic hardware fixtures,
   `CoreHardwareIR`, cycle simulation, `TargetHardwareIR`, generated RTL,
   verified Boon RV32I, the all-peripherals iCESugar Pro shell, standalone
   `app.wasm`, interpreter-first virtual/physical parity, terminal bridge,
   persistence/recovery, and the final hardware-in-the-loop gate. The reusable
   CPU work follows `BOON_FIRST_RISCV_PROCESSOR_PLAN.md`. It does not wait for
   NovyWave, FjordPulse, public deployment, or Boon Orchard.

9. Complete the universal packed runtime:
   `BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md`, integrating formal Phase 6
   with packed `KernelIR`, then passing its native/Wasm, product-scale, and
   flag-day deletion gates. Reuse proved hardware-relevant facts, but do not
   turn `CoreHardwareIR` into a second software executor.

10. Mature the web stack on the final compiler/runtime:
   Client/Session/Server, persistence, content/streaming, formal Phase 7,
   NovyWave, Cells, and every FjordPulse product/deployment gate. Console
   device persistence is a separate bounded flash owner; it does not replace
   the universal application-persistence contract.

11. Run fresh native/Wasm/browser/product evidence from one unchanged revision.
   No pre-foundation, pre-packed, pre-console, or otherwise stale report is
   valid.

12. Complete `BOON_EXAMPLE_PORTFOLIO_PLAN.md`. Selected examples remain
    regression fixtures during earlier steps; the full portfolio follows the
    first proved RV32I/BoonConsole milestone.

13. Stop this goal without beginning Boon Orchard production. The game is not
    specified enough to be part of this execution plan. If it is pursued later,
    create a separate user-approved game goal and implementation contract after
    BoonConsole hardware-in-the-loop readiness; it may consume real CPU,
    console, app-Wasm, simulator, and report artifacts, but never own or weaken
    those contracts.

14. From one final unchanged revision, rerun every applicable compiler,
    formal, packed, persistence, console, product, native/Wasm/browser,
    processor, FPGA, and portfolio gate. Hardware and portfolio edits make
    earlier milestone reports stale wherever they share source or artifacts.
