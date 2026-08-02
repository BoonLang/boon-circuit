# BoonConsole Implementation Plan

Status: proposed executable implementation contract, reconciled against
`5febfea02772b9b82921af225de0356c6b68196c` on 2026-07-30. This first
invocation changed documentation and planning only.

The canonical system meaning is
[`../architecture/BOON_CONSOLE.md`](../architecture/BOON_CONSOLE.md). This plan
owns implementation order, current-state evidence, experiments, planned code
owners, gates, reports, budgets, supersession, and the Clear End Condition.

No production compiler, hardware, RTL, RV32I, Wasm interpreter, board, bridge,
or game implementation is authorized by the documentation-reconciliation
invocation that created this plan. Implementation begins only after a separate
user instruction.

## Outcome

Produce one traceable system:

```text
Boon hardware source
  -> verified MachinePlan
  -> CoreHardwareIR
  -> cycle simulation
  -> TargetHardwareIR
  -> generated RTL
  -> formal/RTL/synthesis evidence
  -> iCESugar Pro bitstream
  -> Boon RV32I + fixed console kernel

Boon application source
  -> the same verified compiler spine
  -> bounded standalone app.wasm
  -> PC reference interpreter
  -> simulated SoC
  -> the exact same app.wasm on physical RV32I

one deterministic scenario
  -> virtual input/output trace
  -> simulated-SoC trace
  -> physical USB CDC trace
  -> equal logical commits and final digests
```

The console uses Pmod BTN, SWT, 8LD, SSD, and CLS concurrently. The local
terminal bridge can upload and recover an app, but does not execute its logic.

## Current-State Audit

The following table records code, not planning aspirations.

| Area | Current evidence | Reconciliation |
| --- | --- | --- |
| Syntax and parser surface | `crates/boon_syntax/src/lib.rs` owns the canonical feature registry and AST DTOs; `crates/boon_parser/src/lib.rs` owns parsing, validation, formatting, and opaque `ParsedProgram` issuance; exact `Number`, `BITS`, `MAP`, `SET`, structured OUT/PASS, and `FLUSH` are accepted; `WHERE` is still planned and rejected | reuse the final surface; do not create console syntax as a parser shortcut |
| Data algebra | `crates/boon_data/src/lib.rs` has only `Number`, `Text`, `Bytes`, `List`, `Object`, `Tag`, `Map`, `Set`, and `Bits`; `number.rs` owns exact rational `ExactNumber` | app eligibility may prove fixed whole-number representation but cannot invent alternate Number semantics |
| Checked artifact | `crates/boon_typecheck/src/lib.rs` owns opaque `CheckedProgram`, typed call/context tables, host-port metadata, BITS/MAP/SET types, and diagnostics | extend typed target/console eligibility in this owner |
| Semantic artifact | `crates/boon_semantic/src/lib.rs` owns opaque `SemanticProgram`; `lowering_contract.rs` binds typed HTTP/WebSocket host ports | add target-neutral `ConsolePort` here; do not overload HTTP/WebSocket |
| Verification | `crates/boon_verify/src/lib.rs` requires `ContractVerifiedProgram`, but authored `WHERE` is absent and the explicit-contract manifest is currently correctly empty | complete the formal contract phases needed for bounds and hardware eligibility before canonical hardware lowering |
| Erased IR | `crates/boon_ir/src/lib.rs` has opaque `ErasedProgram`, source routes, effects, outputs, storage, and HTTP/WebSocket host-port declarations | add verified console declarations only through the semantic/verification boundary |
| MachinePlan | `crates/boon_plan/src/lib.rs` owns the sole executable `MachinePlan`; `TargetProfile` is the closed enum `SoftwareDefault`, `SoftwareBounded`, `FpgaTodomvc` | replace the one-off profile enum with extensible, versioned target/profile data before adding BoonConsole |
| Compiler | `crates/boon_compiler` compiles the verified/erased program to `MachinePlan`; no standalone Wasm or hardware backend exists | add generic backends after the verified spine, never parallel front ends |
| CLI | `crates/boon_cli/src/main.rs` implements only `run`, `check`, `dump-plan`, and `dump-ir` | every hardware/Wasm command in this plan is future work |
| Runtime scenarios | `crates/boon_runtime/src/lib.rs` parses deterministic TOML scenarios with exact source targets and semantic assertions | reuse the scenario concepts, but give console events an app-owned schema rather than TodoMVC/Cells field-name heuristics |
| Wasm today | several Rust runtime/compiler/persistence crates build for `wasm32`; `boon_app_package` packages the browser host Wasm | this is host-runtime portability, not a standalone Boon application emitter or onboard app runtime |
| Hardware today | no tracked `CoreHardwareIR`, `TargetHardwareIR`, cycle simulator, RTL backend, RV32I source, board profile, or hardware report path exists | implementation starts with generic fixtures |
| Historical FPGA profile | `TargetProfile::FpgaTodomvc` exists, while `docs/architecture/FPGA_TODOMVC_LOWERING.md` names a nonexistent `explain-hardware` command and explicitly says it is stale | preserve useful constraints, replace the executable profile generically, then delete the historical file |
| Wire | `crates/boon_wire` has canonical bounded Value encoding and positional bounded Client/Session frames | reuse bounded-codec discipline, not distributed frame schemas or recursive Value on the device |
| Persistence | `crates/boon_persistence` has canonical images, stable identities, generations, migrations, atomic candidate work, and content digests | reuse concepts and digest owners; write a device-sized flash journal rather than embedding native/redb/web storage |
| Report infrastructure | `crates/xtask/src/report_v2.rs` validates bounded, manifest-backed native GPU reports and sidecars | reuse validation patterns; create a separate console manifest because the native GPU manifest remains native-window authority |
| Local FPGA tools | `yosys`, `nextpnr-ecp5`, `ecppack`, `openFPGALoader`, `icesprog`, `verilator`, and `sby` are currently installed under the local OSS CAD suite | pin versions in reports; installed tools are not passing hardware evidence |
| Local RISC-V tools | neither `riscv32-unknown-elf-gcc` nor `riscv64-unknown-elf-gcc` is currently found | Phase 0 must install/pin a reproducible base-RV32I firmware toolchain before firmware gates |
| Connected hardware | the audit found no `/dev/ttyACM*` or `/dev/ttyUSB*`; current USB enumeration did not establish an iCESugar Pro | physical revision, CDC node, and programmer behavior remain measured Phase 0 work |

Empty local directories named `crates/boon_bridge` and `crates/boon_driver` are
not tracked Cargo packages and have no source. They are not implementation to
preserve and do not decide a future crate name.

## Dependencies And Sequencing

The console does not wait for NovyWave, FjordPulse, public deployment, broad web
product completion, or Boon Orchard. It does require:

1. the active simplification/native-recovery plan to finish its current
   architecture and native handoff closure before new production surfaces are
   added;
2. the final public data algebra and structured typechecking required by the app
   and hardware profiles;
3. the mandatory verified artifact spine with real target-bound obligations;
4. the formal phases needed to prove bounds, totality, widths, and legal
   translation facts;
5. packed/hardware-relevant representations such that `CoreHardwareIR`,
   `TargetHardwareIR`, the cycle hot path, and device artifacts contain no
   recursive `Value`, runtime string lookup, or tree collection;
6. the generic hardware pipeline and fixtures before RV32I;
7. the verified RV32I proof bundle before the interpreter SoC claims hardware
   execution.

The universal packed-runtime plan remains authoritative for software execution.
BoonConsole cannot create a second compatibility runtime or claim that a narrow
hardware layout completes the universal packed plan. Conversely, broad web
product acceptance is not a prerequisite for hardware IR or the console.

## Decisions Required By This Plan

### 1. Who Owns A Port Before CoreHardwareIR?

`boon_semantic::SemanticProgram` owns stable logical port meaning and identity.
The typechecker supplies typed references; verification accepts bounds and
contracts; `ErasedProgram` and `MachinePlan` carry the verified target-neutral
declaration. `CoreHardwareIR` owns clocked realization. The board profile owns
pins. Backends may not infer a port from a source name.

### 2. How Does Target/Profile Machinery Become Extensible?

Replace the closed `TargetProfile` enum with:

- a small stable software-profile selector for built-in defaults;
- a versioned `TargetProfileDocument` loaded from canonical TOML;
- a typed, validated, normalized profile stored or digest-bound in
  `MachinePlan`;
- separate app, core-hardware, and board-target profile schemas;
- explicit capabilities, limits, rejection reasons, and parent digests.

No stringly typed profile lookup reaches an executable backend. The
`FpgaTodomvc` variant is deleted only after its real bounded checks are
represented by the generic schema and equivalent negative tests pass.

### 3. What Does Existing Wasm Support Actually Provide?

It provides Rust host/runtime portability to browser Wasm and a browser package
builder. It does not emit a standalone Boon guest, define a guest ABI, validate
a console subset, or execute `app.wasm` on RV32I.

### 4. What Is The Smallest Honest App Subset?

Start with closed Tags, `BITS[N]`, fixed records, fixed state, bounded events,
fixed display cells, and exact Numbers only when proved whole and within a fixed
machine range. Reject recursion, unbounded collections/text/allocation,
floating point, WASI, distributed roles, and ambient effects. Add a bounded
collection only after an unrelated fixture proves its static storage and fuel
cost.

### 5. Which Interpreter Alternatives Are Compared?

Run the same emitted modules through:

1. [Wasmi](https://github.com/wasmi-labs/wasmi), a maintained Rust interpreter
   advertising `no_std`, deterministic execution, and fuel metering;
2. [WAMR](https://github.com/wasm-micro-runtime/wasm-micro-runtime), a
   maintained embedded C runtime with classic/fast interpreters and RISC-V
   support;
3. a purpose-built interpreter for only the frozen BoonConsole Wasm profile.

Wasmi is the initial independent PC oracle unless its profile configuration
cannot reject unsupported features precisely. WAMR and the purpose-built
interpreter are the primary physical candidates. None is selected by README
claims.

For each candidate, build base RV32I without `M`, `A`, `C`, an OS, or WASI and
measure:

- pinned source/license/security status;
- linked ROM bytes;
- static RAM, stack, peak heap, and maximum Wasm pages;
- module validation time;
- initialization and per-event cycles;
- fuel/trap/stack-overflow behavior;
- malformed-module rejection;
- unsupported-feature rejection;
- host ABI size and auditability;
- portability burden;
- results against official Wasm tests relevant to the frozen subset;
- identical console scenarios and final digests.

Selection requires passing semantics and hostile inputs first, then fitting
resource and latency budgets with margin. If a maintained runtime fits, it wins
unless the purpose-built implementation is materially smaller and its complete
subset is realistically auditable. A custom interpreter cannot quietly expand
into an incomplete general Wasm engine.

### 6. What Is The Minimum SoC?

One multicycle RV32I core, boot ROM, bounded RAM, one system bus, kernel timer,
console GPIO/debounce, 8LD/SSD registers, CLS UART TX, iCELink CDC UART RX/TX,
trace/signature support, and optional flash/SDRAM controllers added only at
their phases. No interrupts are required if a proved polling schedule meets
input and UART bounds.

### 7. Can The Interpreter Fit In BRAM?

This is unresolved until synthesis. The upstream board reports 1,008 Kib
(126 KiB) of embedded memory and 32 MiB SDRAM. That BRAM must also hold boot
code, kernel data, stacks, queues, app bytes, app memory, and CPU/SoC buffers.

The Phase 4 experiment links each candidate with the minimum kernel, synthesizes
actual initialized memories, runs the worst-case app fixture, and reserves at
least 25% BRAM headroom. If code, stack, queues, app, and maximum live memory do
not fit together with that margin, SDRAM becomes mandatory before Phase 5. No
spreadsheet estimate can waive the synthesized and runtime peak.

### 8. What Is The Earliest Replaceable App Milestone?

Volatile upload:

```text
boon-console install --volatile target/console/app.wasm
boon-console start
```

The kernel validates into a staging buffer, compares the full SHA-256, starts a
new app generation, and discards the app on SoC reset, FPGA reconfiguration, or
power loss. This precedes flash persistence and is the first physical
exact-byte parity gate.

### 9. Which Programmer Is Used?

Use the board project's `icesprog` flow as the initial programming owner because
the official board documentation identifies it for the onboard iCELink and it
is installed locally. Before freezing commands:

- run non-mutating probe/help/version checks with the exact connected board;
- identify whether SRAM and flash programming need `icesprog`, `dapprog`, or a
  pinned `openFPGALoader` route;
- program a disposable bring-up bitstream;
- power-cycle/read back identity where supported;
- record exact tool, arguments, output, and programmed bitstream digest.

The plan will freeze one primary command and one independently tested recovery
path. Drag-and-drop programming is convenient manual recovery, not automated
acceptance evidence.

### 10. What Is The USB CDC Path?

The official board documentation says iCELink exposes a CDC serial connection
to FPGA logic. The exact Linux device identity is unresolved because no serial
node was present during this audit.

Phase 0 must capture:

- USB VID/PID, serial number, interface number, and physical path;
- stable udev properties and `/dev/serial/by-id` link;
- FPGA RX/TX pins and direction from the exact schematic/revision;
- baud, framing, reset behavior, and buffer limits;
- a loopback/PRBS/error-injection report;
- coexistence with programming/JTAG and any shared-line mode.

The bridge opens only the matched device identity supplied by the user or
profile; it does not guess the first `ttyACM` device.

### 11. What Is The Board Profile Format?

Canonical TOML under `hardware/targets/`, parsed into a deny-unknown-fields DTO,
normalized, validated, and SHA-256 bound. It contains no application behavior.
The planned first file is:

```text
hardware/targets/icesugar_pro_boon_console_v1.toml
```

Pin constraints are generated or checked against separately digested LPF files.
Every field listed in the architecture's board-profile section is required.

### 12. How Is Power Proven?

Record exact module revisions, datasheet maxima, calculated sum, measured idle
and worst-case current, 3.3 V minimum/maximum at the farthest connector, source
rail identity, and thermal observation. Exercise all indicators, both SSD
digits, continuous CLS writes, buttons, switches, CDC, SDRAM if used, and flash
access together. If an external supply is needed, use common ground and one
enabled 3.3 V source; prove the board regulator is disconnected before power.

### 13. Which Scenario And Report Infrastructure Is Reused?

Reuse:

- exact source/turn concepts from `boon_runtime` scenarios;
- canonical source-bundle digests from `boon_contract`;
- bounded binary/golden-vector discipline from `boon_wire`;
- manifest, report, sidecar, negative-check, tool-version, and freshness
  validation patterns from `xtask/report_v2`.

Do not add console gates to
`docs/architecture/native_gpu_handoff_manifest.json`. Create
`docs/architecture/boon_console_handoff_manifest.json` only when the first
corresponding executable gate lands. It becomes the sole list and byte-budget
authority for console handoff.

### 14. Where Does The Bridge Live?

Add one coherent `crates/boon_console` package after recovery closes:

- a `no_std`-capable library for ABI DTOs, target limits, digests, and framing;
- optional `std` modules for serial transport, PC oracle, and scenario harness;
- one `boon-console` binary.

This is justified by a stable device/host protocol boundary. Do not create
separate crates for uploader, protocol, simulator, reports, or every peripheral.
The on-device kernel remains under `hardware/boon_console/kernel/` because its
freestanding build and selected interpreter may not be a Cargo workspace
package.

### 15. What Persists Across Each Reset?

Use the reset matrix in the architecture. Phase 5 volatile bytes never survive
SoC reset. Phase 6 persistent install uses SPI flash dual slots and an atomic
active record; state uses a separate journal. USB disconnect never resets the
app. App reset retains bytes but resets state. Trap retains bytes and last
committed state while entering the safe frame.

### 16. What Is The Exact Safe Frame?

`8LD=10000001`, SSD=`--`, CLS lines `"BOON SAFE       "` and
`"CODE 0000       "`, with the code replaced by a stable hexadecimal fault
code. The terminal reports the same code. This is fixed-shell behavior and is
tested even with no valid app installed.

### 17. How Is Full Digest Lineage Preserved?

Every compiler/hardware/app/protocol/report boundary carries the parent digests
listed in the architecture. Reports validate the chain, not just presence of
hash-shaped strings. The physical hello supplies bitstream, target, kernel,
interpreter, app, state, and board identities. The verifier rejects stale or
mixed lineage before input.

### 18. What Are The Final Commands?

These command names are planned and do not exist yet:

```bash
cargo test -p boon_hardware
cargo test -p boon_console
cargo run -p boon_cli -- emit-console-wasm \
  examples/boon_console/app.bn \
  --profile hardware/profiles/boon_console_app_v1.toml \
  --out target/boon-console/app.wasm
cargo xtask verify-boon-console-all \
  --report target/reports/boon-console-v1/verify-all.json
cargo xtask verify-boon-console-all \
  --check-existing \
  --report target/reports/boon-console-v1/verify-all.json
```

The manifest may rename a command before implementation. Once the manifest
lands, its commands and arguments are the only handoff commands. Documentation
must not retain an alternate list.

## Planned Code Ownership

Add no new crate until active recovery is complete. Then use this minimal
layout:

| Owner | Planned responsibility |
| --- | --- |
| `crates/boon_typecheck/src/` | target/profile eligibility inputs, typed ConsolePort references, fixed bounds |
| `crates/boon_semantic/src/` | semantic ConsolePort binding, port IDs, bounds, effects, obligations |
| `crates/boon_verify/src/` | target eligibility, width/totality/bound proof facts |
| `crates/boon_ir/src/` | verified erased console declarations, no board data |
| `crates/boon_plan/src/` | extensible normalized profiles, MachinePlan console routes/capacities |
| `crates/boon_compiler/src/wasm_app_backend.rs` | standalone integer-only Wasm emission from verified artifacts |
| `crates/boon_compiler/src/hardware_backend.rs` | MachinePlan to generic hardware-lowering entrypoint |
| `crates/boon_hardware/` | CoreHardwareIR, TargetHardwareIR, normalization, cycle simulator, RTL emission, target validation |
| `crates/boon_console/` | app ABI/profile, binary protocol, PC oracle adapter, virtual harness, serial CLI |
| `crates/xtask/src/` | console report producers, validators, aggregate, tool invocation |
| `hardware/profiles/` | versioned app and core logical profiles |
| `hardware/targets/` | measured board profile and constraints |
| `hardware/boon_rv32i/` | Boon CPU source, architectural fixtures, firmware fixtures |
| `hardware/boon_console/` | SoC/shell Boon source, kernel, interpreter integration, peripheral fixtures |
| `fixtures/contracts/boon_console/` | ABI, Wasm, protocol, IR, trace, report golden vectors |
| `examples/boon_console/` | ordinary Boon reference app and scenario, with no engine workaround |

`boon_hardware` is one new generic compiler/backend boundary, not an RV32I
crate. It must pass unrelated fixtures before CPU code exists.

## Phase 0: Authority, Inventory, And Frozen Experiments

The first-invocation documentation reconciliation completes only the authority
portion of this phase. Before production edits:

1. finish the active simplification/native-recovery exit;
2. re-audit the new unchanged revision;
3. inventory the physical board and all module revisions;
4. photograph/record labels and connector orientation without using images as
   functional proof;
5. pin official board schematic/constraints and every external module manual;
6. pin the RISC-V ISA, Sail/reference model, architectural tests,
   `riscv-formal`, Yosys, nextpnr, SymbiYosys, Verilator, programmer, and
   firmware toolchain;
7. run CDC identification and loopback;
8. complete the power calculation and safe supply decision;
9. spike Wasmi, WAMR, and the purpose-built subset on host and base RV32I;
10. synthesize BRAM-only memory envelopes and decide the SDRAM trigger;
11. freeze app ABI, profile schemas, protocol framing, digest domains, report
    schemas, and numeric device limits;
12. create the console manifest only with executable commands.

Exit:

- no unresolved board fact is represented as a frozen profile value;
- interpreter and memory decisions have measured reports;
- every planned command either exists or remains explicitly marked future;
- no production hardware/compiler code has begun before recovery closure.

## Phase 1: Language, Verification, And App Target

### 1A. Close Required Foundations

- complete exact target-relevant typechecking and structural inference;
- implement authored `WHERE` and real proof obligations required by target
  bounds;
- preserve the mandatory `ContractVerifiedProgram` ownership gate;
- prove fixed widths, whole-Number range, bounded control, storage size, fuel
  model inputs, effect absence, and closed capability surface;
- remove parser/backend rediscovery and every compatibility bypass.

### 1B. Replace TargetProfile

- introduce versioned profile DTOs and canonical normalization;
- convert software profiles without behavior drift;
- express the real bounded checks behind `FpgaTodomvc`;
- add invalid/unknown-field/capacity/identity tests;
- remove the enum variant and string aliases in one flag day after parity.

### 1C. Standalone Wasm Emitter

- emit one deterministic standalone module from `ContractVerifiedProgram` /
  `ErasedProgram` and `MachinePlan`;
- freeze imports/exports/memory ABI with golden fixtures;
- use no WASI;
- emit only the frozen integer subset;
- validate with an independent parser and hostile fixtures;
- bind source, verified manifest, profile, emitter, and module digest;
- prove deterministic byte-for-byte rebuilds;
- run app scenarios in Wasmi and at least one independent interpreter.

Exit:

- an invalid/unbounded app fails before emission with source-mapped diagnostics;
- the same valid source emits identical `app.wasm` bytes;
- two independent PC interpreters produce identical logical commits;
- no browser host bundle is mislabelled as the guest artifact.

## Phase 2: Generic Hardware Pipeline

### 2A. CoreHardwareIR

- lower only verified `MachinePlan`;
- represent bits, closed Tags, ports, clocks/reset, combinational nodes,
  registers, bounded storage, FSMs, candidate writes, commit groups,
  assumptions/assertions, and source maps;
- normalize and hash deterministically;
- reject recursive values, strings, dynamic allocation, unbounded storage,
  unknown latency, multiple clocks, or unsupported effects.

### 2B. Cycle Simulator

- execute normalized CoreHardwareIR directly;
- produce bounded cycle, port, commit, assertion, and state-digest traces;
- run native first;
- add host Wasm parity only as a cross-target proof, not as `app.wasm`;
- prove reset, backpressure, simultaneous source arbitration, and bounded
  progress.

### 2C. TargetHardwareIR And RTL

- elaborate explicit target memories, adapters, clocks, and instrumentation;
- retain the parent core digest;
- emit deterministic SystemVerilog;
- lint and simulate with Verilator;
- compare TargetHardwareIR and RTL cycle by cycle;
- run formal assertions/assumptions;
- synthesize and place/route with fresh utilization/timing reports.

### 2D. Unrelated Fixtures

Pass, in order:

1. resettable counter;
2. combinational BITS ALU;
3. two-read/one-write bounded MAP register bank;
4. wait-state RAM request/response adapter;
5. protocol FSM with simultaneous inputs and assertions;
6. candidate/commit rollback fixture;
7. generic four-input/eight-output port fixture.

Exit:

- no CPU, console, app, or example name exists in generic lowering branches;
- every fixture agrees across CoreHardwareIR, TargetHardwareIR, RTL, and formal
  checks;
- tool, source, profiles, IRs, RTL, and report lineage is complete.

## Phase 3: Peripheral Shell And Physical Bring-Up

Bring up one peripheral at a time with fixed diagnostic logic, then all
together:

1. clock/reset and machine-readable UART signature;
2. Pmod 8LD walking and simultaneous patterns;
3. Pmod SSD complete glyph table and multiplex timing;
4. Pmod BTN synchronization, debounce, press/release, and simultaneous edges;
5. Pmod SWT all 16 values and simultaneous button snapshots;
6. Pmod CLS reset, UART convention, baud, commands, full 32-cell frames, and
   recovery;
7. iCELink CDC RX/TX framing under fragmentation, corruption, and sustained
   load;
8. all modules plus CDC active concurrently;
9. power worst case;
10. safe frame from fixed shell.

The diagnostic logic uses the generic hardware pipeline. It is deleted from the
production shell after equivalent self-test/report paths exist.

Exit:

- one measured target profile and LPF map the exact physical assembly;
- no pin conflict or shared-line ambiguity remains;
- the all-peripherals trace and WGPU-independent app-owned evidence pass;
- no onboard RGB observation is needed.

## Phase 4: Verified RV32I And Console SoC

Complete the processor plan through its reusable proof bundle:

- pure architectural step model;
- complete declared RV32I execution environment;
- multicycle core and bounded memory bus;
- native CoreHardwareIR execution;
- architectural tests and randomized Sail differential execution;
- RVFI adapter and selected `riscv-formal` checks;
- TargetHardwareIR/RTL cycle equivalence;
- synthesis and timing;
- board firmware signatures and bounded trace digest.

Then build the minimum console SoC around the unchanged core:

- boot ROM/RAM and fixed kernel skeleton;
- console ingress and frame registers;
- CDC transport;
- selected interpreter memory envelope;
- SDRAM controller only if the Phase 0 decision requires it;
- SPI flash read path reserved but persistence not yet active.

Exit:

- the processor proof is reusable and contains no console special case;
- fixed firmware executes on simulated and physical RV32I;
- board signature and trace match simulation;
- interpreter memory placement has at least the required headroom.

## Phase 5: Interpreter-First Volatile App Platform

- integrate the selected interpreter in the fixed kernel;
- validate and upload into a staging slot;
- implement the frozen app ABI;
- enforce maximum pages, stack, fuel, transaction bytes, timers, queues, and
  payloads;
- make a successful turn atomic;
- enter the safe frame on traps or validation failures;
- keep recovery protocol available independently of app queues;
- run the same emitted `app.wasm` in the PC oracle, simulated SoC, RTL where
  practical, and physical board;
- prove virtual/physical event ordering and all output fields;
- prove upload/restart/replacement without reprogramming the FPGA.

Required scenarios include:

- every button press/release with every switch snapshot;
- simultaneous switch/button changes;
- all indicator bits;
- SSD glyph boundaries;
- full CLS rows and rapid superseding frames;
- timers and stale generations;
- bridge message echo and backpressure;
- input queue overflow;
- app trap, fuel exhaustion, invalid pointer, oversized transaction;
- malformed/unsupported Wasm;
- disconnect/reconnect while the app continues;
- app replacement and stale event rejection.

Exit:

- exact app bytes and digest match all environments;
- committed logical traces and final state digests match;
- physical presented-output generations reach every expected frame;
- volatile loss on reset is explicit and tested.

## Phase 6: Persistent Apps, State, Bridge, And Recovery

### 6A. Flash Layout

- two content-addressed app slots;
- complete metadata with app/profile/ABI digest;
- staged write, readback, validation, then atomic active record;
- boot rollback after corrupt/incomplete activation;
- wear-aware state journal with complete commit markers;
- separate app and state identities;
- preserved incompatible bytes for recovery.

### 6B. Terminal Bridge

- implement the architecture command surface;
- bind commands to device identity and boot nonce;
- support explicit device selection;
- stream bounded progress and diagnostics;
- never expose a network listener;
- compare PC oracle and device result;
- refuse mixed bitstream/kernel/profile/app lineage.

### 6C. Reset And Fault Matrix

Exercise every architecture row:

- app reset;
- app reset with explicit restore;
- SoC reset;
- FPGA reconfiguration;
- power loss during app write;
- power loss during state commit;
- USB disconnect;
- trap/fuel/input-overflow safe state;
- corrupt active record;
- incompatible state schema;
- recovery to previous valid app.

Exit:

- installation and state are atomic across injected interruption points;
- the device always boots to a valid app or fixed recovery/safe state;
- local app operation is independent of bridge presence;
- no JSON, browser, server, or host-executed app decision exists.

## Phase 7: Final Hardware-In-The-Loop Gate

Create the final manifest entries only for executable producers. At minimum they
cover:

1. authority/profile inventory;
2. standalone app Wasm determinism and hostile validation;
3. generic hardware IR/RTL/formal equivalence;
4. RV32I architectural/differential/formal proof;
5. board identity, toolchain, programming, and timing;
6. all-peripherals electrical/protocol bring-up;
7. PC-reference versus simulated-SoC scenario parity;
8. volatile physical exact-byte parity;
9. persistence/reset/corruption recovery;
10. terminal bridge negative protocol checks;
11. safe-state/accessibility checks;
12. aggregate lineage/freshness.

The final verifier:

- proves the expected board and inactive host-side test context before input;
- reads the device hello and rejects identity drift;
- uploads or selects the exact app;
- drives scenario inputs through the app-owned protocol;
- captures device events, commits, presented generations, and final digest;
- independently checks visible electrical outputs through fixture-owned
  feedback where practical;
- uses no whole-desktop screenshot, browser, Ply, Xvfb, COSMIC toplevel
  scraping, `xdotool`, or fabricated human observation;
- reports every unavailable physical feedback channel honestly.

Human observation, enclosure work, and play testing follow only after the
automated gate passes.

## Acceptance Budgets

These are V1 product targets. The manifest freezes the final numeric values; a
change requires measured rationale and an architecture/plan update.

### Timing

| Budget | Target |
| --- | ---: |
| core clock | meet 25 MHz with non-negative worst-case slack |
| debounced input acceptance to committed frame, p99 | at most 50 ms |
| debounced input acceptance to committed frame, maximum | at most 100 ms |
| committed frame to registered 8LD/SSD output | at most 2 ms |
| committed frame to complete CLS presentation | at most 100 ms |
| bridge `status` after opening an available device | at most 500 ms |
| recovery hello after SoC reset | at most 2 s |

Debounce delay is reported separately and included in physical
input-to-presentation totals. Proof capture overhead is reported separately and
cannot make product timing look faster.

### Resources

- at least 20% LUT headroom after place/route;
- at least 25% sysMEM headroom for the selected BRAM-only design;
- if SDRAM is used, at least 20% sysMEM headroom and measured worst-case SDRAM
  latency within the interpreter fuel/timing model;
- no inferred latch;
- no unbounded queue or allocator;
- maximum stack, heap, app bytes, Wasm pages, state bytes, and fuel are concrete
  profile numbers with measured peaks;
- the full app/kernel/interpreter/SoC image fits the selected persistent and
  volatile memories with 20% byte headroom.

### Reports

Until the manifest freezes stricter per-gate values:

- inline JSON maximum: 64 KiB per gate report;
- one sidecar maximum: 64 MiB;
- aggregate sidecars maximum: 256 MiB;
- logs are summarized and content-addressed, never embedded without bounds;
- every trace declares event/cycle count and truncation status;
- a passing report cannot contain a truncated required trace.

### Behavioral

- zero silently dropped physical edges;
- zero partially committed app turns;
- zero accepted malformed/unsupported modules;
- zero stale-boot or replayed mutating commands accepted;
- zero app decisions executed by the host bridge;
- zero board/example/RV32I name checks in generic compiler code;
- exact final digest equality across every declared equivalent environment.

## Reproducibility And Freshness

Every report records:

- clean/dirty worktree fingerprint;
- Git commit and tracked diff digest;
- source bundle and compiler artifact chain;
- tool executable digests and versions;
- external dependency revisions;
- board/profile/module identity;
- generated source, RTL, constraints, firmware, kernel, interpreter, app,
  bitstream, and scenario digests;
- command and bounded environment;
- start/end timestamps and duration;
- producer exit status;
- report and sidecar digests.

Any tracked change affecting a parent artifact makes descendants stale.
Reprogramming with different bytes, changing a Pmod jumper, swapping a board
revision, or changing interpreter configuration also makes physical evidence
stale even if Git does not change.

## Deletion And Supersession Ledger

| Item | Action and gate |
| --- | --- |
| root `BOON_CONSOLE_REPOSITORY_UPDATE_GOAL.md` | imported invocation input only; remove from the eventual tracked change unless the user explicitly wants it archived |
| `TargetProfile::FpgaTodomvc` | replace with generic profile documents, prove equivalent bounded rejection, delete enum variant and aliases |
| `FPGA_TODOMVC_LOWERING.md` | retain only until generic hardware constraints and negative tests exist, then delete |
| nonexistent `boon_cli explain-hardware` claims | delete now from active authority; never add a compatibility command solely to satisfy stale docs |
| `MISSION_WASM_AND_LANTERN_CONSOLES.md` | retain as clearly labelled historical game checkpoint; it owns no implementation decision |
| AOT-first physical mission path | superseded; may return only as optional later research after exact-byte interpreter readiness |
| Boon Orchard projection in processor acceptance | remove; game implementation is outside this goal and requires a separate future specification |
| browser-host Wasm called app/mission Wasm | reject terminology; preserve browser host implementation under its real role |
| handwritten diagnostic peripheral logic | delete from production shell after generated equivalents and self-test paths pass |
| reference/legacy hardware executor switches | diagnostics only during proof; delete before phase exit |
| duplicate console gate lists | delete after the manifest exists; the manifest becomes sole handoff list |

## Risks And Rethink Triggers

### The Interpreter Does Not Fit

Trigger: two measured candidates exceed BRAM/resource margins or miss the
100 ms maximum repeatedly.

Response: stop local opcode tuning. Reassess SDRAM, app ABI memory ownership,
kernel/interpreter split, purpose-built subset size, and the RV32I
microarchitecture. Do not switch to required AOT.

### RV32I Is Too Slow For The App Contract

Trigger: the same event classes exceed budget after interpreter and memory
profiling.

Response: use traces to distinguish core cycles, interpreter dispatch, ABI
copying, display serialization, and SDRAM stalls. Consider a faster but still
generic RV32I implementation or verified interpreter acceleration. Do not move
app decisions into fixed RTL.

### The Board Cannot Power All Modules

Trigger: calculated or measured rail margin fails, voltage droops, or thermal
limits are approached.

Response: stop powering the assembly, design the single external regulated
3.3 V path with common ground and disconnected board feed, then repeat the full
power gate.

### CDC And Programming Interfere

Trigger: programming/JTAG mode changes corrupt CDC, reset the app unexpectedly,
or share unprofiled pins.

Response: freeze an explicit mode/reset protocol and recovery flow, or use the
official alternate programming route. Never guess around shared lines.

### Generic Hardware Work Becomes CPU-Specific

Trigger: a compiler branch contains an instruction, CPU, console, board, or game
name.

Response: stop the CPU slice, construct an unrelated fixture, and fix the
generic owner.

### Verification Is Only Visual

Trigger: a pass depends on someone seeing LEDs/LCD or a desktop screenshot.

Response: add app-owned signatures, loopback/fixture feedback, presented
generations, and machine-readable traces. Record any inherently unobserved
channel as an open hardware-verification gap.

### Repeated Failures Have The Same Shape

Trigger: two fresh attempts fail for the same architectural class:
unbounded queue, recursive representation, interpreter memory, output
serialization, report readback, or stale lineage.

Response: stop micro-fixes and reassess the owning architecture, profile, ABI,
memory system, scheduling, or verification design.

## Clear End Condition

This plan is complete only when:

1. the final language/type/verification prerequisites pass with no bypass;
2. generic hardware fixtures precede and remain independent of RV32I;
3. the complete declared RV32I core passes architectural, differential, formal,
   RTL, synthesis, and physical signature gates;
4. all five Pmods work concurrently on the exact measured assembly;
5. the onboard RGB LED has no product or proof dependency;
6. one deterministic standalone `app.wasm` is emitted from verified Boon and
   is byte-identical across deployment;
7. the PC oracle, simulated SoC, and physical interpreter produce equal logical
   commit traces and final digests;
8. no required AOT or host-executed app decision exists;
9. queue, memory, fuel, timing, power, and report budgets pass;
10. volatile upload, persistent install, reset, power-loss, corruption,
    rollback, disconnect, trap, and safe-state scenarios pass;
11. the terminal-only binary bridge passes bounded hostile protocol tests;
12. full digest lineage is fresh and validated from source through physical
    evidence;
13. the manifest-backed aggregate passes on one unchanged revision;
14. the supersession/deletion ledger is complete;
15. an independent review finds no example, board, CPU, or game workaround in
    a generic owner.

Only then is BoonConsole technically available to a separately specified
downstream goal. This plan does not begin or schedule Boon Orchard. Game
progress, human observation, an LED blink, a stale report, or a simulator-only
result cannot close this plan.
