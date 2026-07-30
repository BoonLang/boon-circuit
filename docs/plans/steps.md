# Boon Implementation Order

`GOAL_PROMPT.md` is the complete execution contract. This file fixes sequencing
only; linked plans remain authoritative for their own semantics and acceptance.

1. Finish the active
   `BOON_CIRCUIT_SIMPLIFICATION_AND_NATIVE_RECOVERY_PLAN.md` exit before adding
   production compiler targets, hardware crates, RTL, a console bridge, or game
   work. Preserve the current verified-artifact and typed-list checkpoints.
   Documentation, board inventory, and measured tool/interpreter experiments
   may proceed, but do not create a production bypass around unfinished
   recovery.

2. Establish the final verified compiler spine and OUT ownership with
   `BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md` and
   `TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md`. Pull forward only the
   `boon_semantic`, `boon_verify`, `ContractVerifiedProgram`, and opaque
   `ErasedProgram` infrastructure required from formal phases 0–1. This does
   not complete those formal phases or the OUT Clear End Condition.

3. Land the language-foundation and structural-inference implementation on the
   final exact value algebra. Keep formal-dependent acceptance open until
   step 5.

4. Land `TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md` on the final value
   algebra and verified artifact spine. Keep its formal-dependent Clear End
   Condition open until step 5.

5. Complete `BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md` phases 0–5, then
   rerun and close every acceptance/Clear End Condition from steps 2–4.

6. Complete only the packed hardware prerequisites needed to cross from
   verified `MachinePlan` into normalized hardware artifacts: fixed widths,
   shape/offset access, bounded storage, target eligibility, dense IDs, and no
   recursive `Value`, runtime string lookup, or tree collection in hardware IR
   or cycle execution. `BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md` still
   owns the universal software runtime; this step does not claim its phases,
   flag-day deletion, product-scale reports, or Clear End Condition.

7. Complete `BOON_CONSOLE_IMPLEMENTATION_PLAN.md`: generic hardware fixtures,
   `CoreHardwareIR`, cycle simulation, `TargetHardwareIR`, generated RTL,
   verified Boon RV32I, the all-peripherals iCESugar Pro shell, standalone
   `app.wasm`, interpreter-first virtual/physical parity, terminal bridge,
   persistence/recovery, and the final hardware-in-the-loop gate. The reusable
   CPU work follows `BOON_FIRST_RISCV_PROCESSOR_PLAN.md`. It does not wait for
   NovyWave, FjordPulse, public deployment, or Boon Orchard.

8. Complete the universal packed runtime:
   `BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md`, integrating formal Phase 6
   with packed `KernelIR`, then passing its native/Wasm, product-scale, and
   flag-day deletion gates. Reuse proved hardware-relevant facts, but do not
   turn `CoreHardwareIR` into a second software executor.

9. Mature the web stack on the final compiler/runtime:
   Client/Session/Server, persistence, content/streaming, formal Phase 7,
   NovyWave, Cells, and every FjordPulse product/deployment gate. Console
   device persistence is a separate bounded flash owner; it does not replace
   the universal application-persistence contract.

10. Run fresh native/Wasm/browser/product evidence from one unchanged revision.
   No pre-foundation, pre-packed, pre-console, or otherwise stale report is
   valid.

11. Complete `BOON_EXAMPLE_PORTFOLIO_PLAN.md`. Selected examples remain
    regression fixtures during earlier steps; the full portfolio follows the
    first proved RV32I/BoonConsole milestone.

12. Stop this goal without beginning Boon Orchard production. The game is not
    specified enough to be part of this execution plan. If it is pursued later,
    create a separate user-approved game goal and implementation contract after
    BoonConsole hardware-in-the-loop readiness; it may consume real CPU,
    console, app-Wasm, simulator, and report artifacts, but never own or weaken
    those contracts.

13. From one final unchanged revision, rerun every applicable compiler,
    formal, packed, persistence, console, product, native/Wasm/browser,
    processor, FPGA, and portfolio gate. Hardware and portfolio edits make
    earlier milestone reports stale wherever they share source or artifacts.
