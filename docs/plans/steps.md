# Boon Implementation Order

`GOAL_PROMPT.md` is the complete execution contract. This file fixes sequencing
only; linked plans remain authoritative for their own semantics and acceptance.

Before implementation, retire the paused goal created from the older
`GOAL_PROMPT.md`. Preserve commits `44c011a`, `a12c9e1`, `4d9863e`, and
`18ad761`, then start a fresh goal from the rewritten prompt.

1. Establish the verified compiler spine and OUT ownership with
   `BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md` and
   `TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md`. Pull forward only the
   `boon_semantic`, `boon_verify`, `ContractVerifiedProgram`, and opaque
   `ErasedProgram` infrastructure required from formal phases 0–1. This does
   not complete those formal phases or the OUT Clear End Condition.

2. Land the language-foundation and structural-inference implementation on the
   final exact value algebra. Keep formal-dependent acceptance open until
   step 4.

3. Land `TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md` on the final value
   algebra and verified artifact spine. Keep its formal-dependent Clear End
   Condition open until step 4.

4. Complete `BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md` phases 0–5, then
   rerun and close every acceptance/Clear End Condition from steps 1–3.

5. Complete the universal packed runtime:
   `BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md`, integrating formal Phase 6
   with packed `KernelIR`, then passing the packed flag-day deletion gate.

6. Mature the web stack on the final compiler/runtime:
   Client/Session/Server, persistence, content/streaming, formal Phase 7,
   NovyWave, Cells, and every FjordPulse product/deployment gate.

7. Run fresh pre-hardware web-app/native/Wasm/browser milestone evidence from
   one unchanged revision. No pre-packed or pre-foundation report is valid.

8. Complete `BOON_FIRST_RISCV_PROCESSOR_PLAN.md`, including generated RTL,
   architectural and formal proof, FPGA-board evidence, and Boon Orchard
   projection. Self-hosting and a public `MEMORY` keyword are not goals.

9. Complete `BOON_EXAMPLE_PORTFOLIO_PLAN.md`. Selected examples are regression
   fixtures during earlier steps; the full portfolio follows the first
   RISC-V/Orchard milestone.

10. From one final unchanged revision, rerun every applicable compiler,
    formal, packed, persistence, product, native/Wasm/browser, processor,
    FPGA, and portfolio gate. Hardware and portfolio edits make earlier
    milestone reports stale wherever they share source or artifacts.
