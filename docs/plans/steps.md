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

   Preserve the landed independent-unit parser, `boon_syntax`/`boon_checked`
   cutovers, structured work counters, indexed parser validation/assembly, and
   deterministic non-recursive checked-diagnostic projection. The current
   `677d09d` release candidate passes the six direct cold diagnostics time/RSS
   combinations, but NovyWave empty-session has narrow p95 headroom and that
   evidence is stale after the next frontend edit. The parsed snapshot and both
   checked construction owners are now lifetime-free without a material
   allocation regression. Dense solver queues now retain their buffers and one
   complete dependency index owns expression propagation, reducing a current
   NovyWave sample by 1,226 allocations without changing its digest, but the 35
   inference rounds and 5,060 call visits remain. The remaining first tranche
   is therefore fusing the duplicate checker/builder into one owned database,
   measured compact/name interning, larger contextual/call worklist reduction,
   scaling/parity evidence, and the fresh Phase 1 adversarial review. Reprofile
   after each owner-level slice and regenerate the complete cold protocol after
   the final edit. Then close semantic sealing, proof, backend, hashing, and
   memory until both verified-plan modes pass; only afterward may persistent-
   session warm work satisfy its separate gates.

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
