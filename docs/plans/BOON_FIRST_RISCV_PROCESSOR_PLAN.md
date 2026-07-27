# First Boon-Designed RISC-V Processor Plan

Status: proposed implementation contract. Canonical implementation depends on
complete language foundations, formal verification, universal packed-runtime
completion, and the mature web-application gates described below.

Working core name: **Boon RV32I**.

## Outcome

Build a real RV32I processor whose architectural logic is authored in Boon,
then run the same source through:

```text
ParsedProgram
  -> CheckedProgram
  -> SemanticProgram
  -> ContractVerifiedProgram
  -> ErasedProgram
  -> MachinePlan
  -> CoreHardwareIR
  -> native and browser/Wasm cycle simulators
  -> target elaboration
  -> TargetHardwareIR
  -> generated SystemVerilog
  -> RTL simulation and formal checks
  -> FPGA synthesis and board execution
```

This project has three equal purposes:

1. stabilize a general Boon-to-hardware/FPGA API;
2. prove that Boon can describe, verify, simulate, and synthesize a substantial
   stateful digital system;
3. provide the first major construction goal for
   [`../game/BOON_ORCHARD.md`](../game/BOON_ORCHARD.md).

The core is not a decorative CPU-shaped demo. It must execute externally built
RV32I programs and pass machine-readable architectural, differential, formal,
RTL, and board gates.

## What "Boon-Designed" Means

The following must be Boon source:

- instruction decode;
- immediate extraction;
- ALU and comparisons;
- branch and jump decisions;
- program-counter updates;
- register-file read/write control;
- load/store address and byte-mask logic;
- fetch/decode/execute/memory/writeback state machine;
- request/response bus control;
- trap/exit classification;
- retirement trace construction.

The following may be host or generated infrastructure:

- ELF/flat-image preparation;
- test orchestration;
- the RISC-V reference model;
- cycle stepping and trace collection around `CoreHardwareIR` and
  `TargetHardwareIR`;
- SystemVerilog emission;
- Yosys/nextpnr/formal invocation;
- board pin, clock/PLL, reset, BRAM primitive, and UART/USB adapters;
- a small board loader/monitor.

Board adapters may adapt interfaces. They may not reimplement decode, ALU,
register, control, or memory semantics outside Boon.

Generated SystemVerilog is an artifact. It is never the manually edited source
of truth.

## Why RISC-V First

A general Boon machine will need a scalar path for irregular serial work even
if the eventual custom architecture is primarily reactive, vector, or spatial.
RISC-V is a useful first scalar target because it supplies:

- a small ratified base ISA;
- independent reference models and architectural tests;
- open formal-verification infrastructure;
- existing compilers and firmware tools;
- a clear compatibility path on future FPGA boards and heterogeneous
  processors.

This does **not** decide that the ultimate Boon processor must expose RISC-V.
The long-term machine may use:

- a custom Boon scalar lane;
- a Boon Turn Engine;
- typed local memories;
- row/vector lanes;
- spatial pipelines;
- an optional RISC-V compatibility island.

Boon RV32I is the shortest credible route to proving the language and hardware
toolchain against a widely understood processor contract.

## Self-Hosting Is Not A Goal

This plan does not require:

- the Boon compiler to run on RISC-V;
- the Boon runtime to run as RV32I firmware;
- Boon source to compile into RISC-V machine code;
- an operating system written in Boon;
- a CPU designed in Boon to compile itself;
- any other circular bootstrap.

In the Wasm milestone, the **Boon-authored processor model** runs in a
browser/Wasm simulator. That does not mean the simulated RV32I executes the
Boon compiler or runtime.

Self-hosting is outside this roadmap. It is neither a prerequisite, a milestone,
nor hidden acceptance work.

## Current Repository Baseline

The repository is not yet a hardware compiler.

Useful foundations exist:

- checked and erased compiler layers;
- `MachinePlan` with typed IDs, storage, regions, commit, deltas, and profiles;
- a dense row-expression arena;
- deterministic native runtime semantics;
- Wasm-capable compiler/runtime components;
- VCD/FST/GHW support through `boon_wellen_host`;
- a documented FPGA lowering model for TodoMVC;
- `BITS[N]`, bounded target, and collection-lowering decisions in the
  foundations plan.

Important gaps remain:

- there is no `CoreHardwareIR`/`TargetHardwareIR`;
- there is no cycle simulator;
- there is no RTL backend;
- there is no hardware/formal report pipeline;
- `BITS[N]` and the final public `MAP`/`SET` model are planned but not
  implemented end to end;
- the current `fpga_todomvc` profile has only a narrow concrete implementation;
- the current CLI exposes `run`, `check`, `dump-plan`, and `dump-ir`, while
  `FPGA_TODOMVC_LOWERING.md` still describes an `explain-hardware` command that
  is not implemented.

Phase 0 must correct these gaps honestly. Existing planning language is not
hardware-readiness evidence.

## Dependency On The Completed Universal Stack

Before the canonical processor project begins, all of these prerequisites must
be complete:

- [`BOON_LANGUAGE_FOUNDATIONS_PLAN.md`](BOON_LANGUAGE_FOUNDATIONS_PLAN.md),
  including the final value algebra, `BITS[N]`, bounded `MAP`, and its flag-day
  deletions;
- the verified `ParsedProgram` -> `CheckedProgram` -> `SemanticProgram` ->
  `ContractVerifiedProgram` -> `ErasedProgram` -> `MachinePlan` compiler
  spine;
- every phase of
  [`BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md`](BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md);
- every phase and acceptance criterion of
  [`BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md`](BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md),
  including formal Phase 6 integration, native/Wasm parity, product-scale
  reports, and flag-day deletion;
- the final Client/Session/Server, persistence, content/streaming, NovyWave,
  Cells, FjordPulse, native, Wasm, browser, and deployment gates required by
  the active combined [`GOAL_PROMPT.md`](GOAL_PROMPT.md).

The following narrower technical gate remains useful as a diagnostic milestone
inside the foundation and packed work:

- `BITS[N]` parses, typechecks, lowers, executes, serializes, and compares
  identically on native and Wasm;
- bounded `MAP` exists as the public keyed authority;
- dense bit keys can select a proved direct-address layout;
- executable object fields are shape/offset based;
- hardware-bound execution uses packed cells and typed columns;
- cycle-hot work contains no recursive `Value`, string field lookup,
  `BTreeMap`, `BTreeSet`, `HashMap`, or `HashSet`;
- target profiles express concrete widths, capacities, ports, latency, and
  overflow/fault behavior;
- the `CheckedProgram`/`SemanticProgram`/`ContractVerifiedProgram`/
  `ErasedProgram`/`MachinePlan` and physical/hardware artifact versions used by
  the processor are stable and separately hashed;
- every compiler/runtime/layout path used by processor cycles has completed its
  flag-day deletion, with no legacy/reference production switch or
  compatibility materializer.

Passing this milestone does not authorize canonical processor implementation.
Before every prerequisite above passes, a pre-Stage-0 activity may perform only
read-only specification, board, toolchain, test-suite, and repository
inventory. Do not write canonical processor source, hardware IR implementation,
generated RTL, processor-specific compiler paths, or acceptance fixtures early.

Architectural CPU state uses `BITS`, bounded `MAP`, Tags, records, sources, and
ordinary Boon state/flow. It need not depend on general exact `NUMBER`
arithmetic.

## Public Language Decision

There is no `MEMORY` keyword.

The public surface remains ordinary Boon:

```text
BITS[N]
records and closed Tags
HOLD and turn commit
bounded MAP
SOURCE and output/effect ports
```

A profile plus checked access pattern may lower `MAP` to:

- a register bank;
- direct-address distributed storage;
- ROM;
- single- or multi-port RAM;
- FPGA BRAM;
- a CAM or indexed implementation.

`MAP` remains a semantic keyed authority. The selected physical storage,
primitive technology, and placement are private layout facts. Required port
counts and any latency visible across Boon turns are explicit checked/profile
facts; a backend cannot change them privately.

## Boon Hardware Contract

The CPU is deliberately the forcing function for this contract. The contract
must remain useful for unrelated counters, protocol controllers, DSP kernels,
state machines, and later Boon Turn Engine work.

### Clock And Reset

Clock and reset are target/profile bindings, not ordinary serializable Boon
values and not new public value kinds.

One hardware activation observes the current registered state and external
inputs, computes candidates, and commits all accepted register writes together
at the active clock edge.

Requirements:

- one explicit clock domain in the first profile;
- one reset policy and polarity in the profile;
- deterministic reset values for every register and authority;
- no inferred clock gating in V1;
- clock-domain crossings rejected until an explicit adapter exists;
- no observable mid-cycle state commit.

This directly reuses Boon's current/pending/commit discipline.

### SOURCE Presence And Arbitration

Hardware `SOURCE` ports preserve Boon presence and event order:

- a present signal/event plus payload is sampled once at the active edge;
- absence is distinct from a present payload whose bits are zero;
- a held decoupled input is consumed only on its declared handshake;
- repeated events are not collapsed into one dirty bit;
- coalescing is allowed only for an explicitly classified current-value signal;
- any sequence used by `LATEST` is a typed comparable input/metadata field and
  has the same meaning as the software runtime.

V1 hardware accepts multiple candidates for one state cell only when they are
statically mutually exclusive, or when `LATEST` can compare their real event
sequences. Equal-sequence ambiguous writes are errors. Source-text order,
combinational arrival time, routing, and synthesis order never become an
implicit priority.

Reset dominates ordinary source sampling and candidate commit. An asserted
reset produces no retirement record, effect, or ordinary semantic delta.

### Combinational And Registered Logic

Pure bounded Boon expressions may become combinational logic.

`HOLD` and bounded authority state may become registers or state memories.

The backend must detect and reject:

- zero-delay combinational cycles;
- unresolved multiple drivers;
- unbounded work;
- dynamic allocation;
- unsupported value widths;
- hidden host calls/effects;
- target-ineligible exact arithmetic;
- latency assumptions not satisfied by the chosen storage.

### MAP Ports And Latency

The compiler derives required access ports from checked uses.

For the register file:

```text
key                 BITS[5]
value               BITS[32]
capacity            32
read ports          2 combinational
write ports         1 committed on edge
x0 policy           reads zero, writes discarded
```

The V1 execution environment initializes all 32 keys at reset. `x1` through
`x31` start at zero as an explicit EEI/profile choice, not an RV32I ISA
guarantee. `x0` reads zero and discards writes. Register lookup is therefore a
proved total wrapper over public sparse `MAP` semantics and cannot produce
`NotFound`.

The compiler may lower this bounded total use to a 32-entry direct-address
bank. Canonical `MAP` enumeration does not require a tree or a scan in each CPU
cycle.

The backend must not silently replace a combinational read with a synchronous
one. If a physical RAM has registered read latency, the program uses an
explicit ordinary request/response adapter and the CPU state machine waits for
the response.

### External Ports

The core exposes one unified, wait-state-capable request/response memory port:

```text
req_valid
req_ready
req_kind: Read | Write
req_address: BITS[32]
req_write_data: BITS[32]
req_byte_enable: BITS[4]

rsp_valid
rsp_ready
rsp_read_word: BITS[32]
rsp_fault
```

Instruction fetch and data access share this port in V1. The multicycle core
therefore has at most one memory transaction in flight.

Protocol rules:

- a request is accepted exactly on `req_valid && req_ready`;
- request fields remain stable while `req_valid && !req_ready`;
- the responder emits exactly one response for every accepted request and no
  spontaneous response;
- response fields remain stable while `rsp_valid && !rsp_ready`;
- a response is consumed exactly on `rsp_valid && rsp_ready`;
- the core issues no second request until the response is consumed;
- a faulting store response certifies that no memory or MMIO side effect
  occurred;
- an accepted successful store is one atomic external effect of that
  instruction;
- reset accepts no request; the first profile uses drain-before-reset, so the
  environment waits for an accepted transaction's response before asserting
  reset to the core;
- an architectural trap never abandons an accepted transaction; it waits for
  the response and then retires or traps exactly once.

The verification profile declares `MAX_MEMORY_WAIT`. Under the assumption that
each accepted request receives its response within that many clocks, every
instruction class has a derived `MAX_INSTRUCTION_CYCLES` bound to retire or
trap. Without a response the core may stall indefinitely, but it keeps the
outstanding protocol state stable and performs no architectural register,
memory, retirement, or effect commit. Unbounded fairness/liveness is not
claimed by a bounded model check.

The address is a byte address. The adapter accesses the aligned word at
`req_address & 0xffff_fffc`. Byte-enable bit `i` names little-endian lane
`aligned_address + i`; store data is shifted into the selected lanes.
`rsp_read_word` is the raw aligned 32-bit word. The core selects the addressed
byte/halfword and performs architectural sign or zero extension. For reads,
`req_byte_enable` states the requested lanes and is checked even when the
physical memory fetches a full word.

Retirement distinguishes the raw `memory_bus_read_word` from the final
`memory_load_value`.

The profile defines whether the surrounding memory is:

- a simulator array;
- generated on-chip RAM, or ROM only for a compile-time immutable/static `MAP`
  with no mutation route;
- a board BRAM adapter;
- a later SDRAM controller.

The core source and architectural digest stay unchanged.

### Retirement And Debug Port

The core emits two distinct event kinds:

```text
RetiredInstruction
  order
  pc
  instruction
  outcome: Continues[next_pc] | Terminal[cause, bad_address?]
  register_write
  memory_access
  memory_bus_read_word
  memory_load_value

ExecutionEnvironmentFault
  cause
  faulting_fetch_pc
  bad_address?
```

An illegal instruction, misaligned control transfer/load/store, `ECALL`, or
`EBREAK` is tied to a decoded instruction. It consumes retirement order, emits
the terminal outcome/RVFI trap where representable, and performs no register,
memory, or effect commit.

A fetch access fault occurs before an instruction exists. It emits
`ExecutionEnvironmentFault`, not a synthetic retired instruction and not an
RVFI order entry. Terminal outcomes have no ordinary `next_pc`.

This port is verification infrastructure and the seed for:

- native/Wasm traces;
- RTL equivalence;
- RVFI adaptation;
- board UART signatures;
- Boon Orchard trace visualization.

Retirement/debug logic never feeds back into architectural state. Proof builds
keep it enabled. A stripped build is a separately elaborated artifact that
requires sequential equivalence; its resource and Fmax results may differ and
are reported separately.

## CoreHardwareIR And TargetHardwareIR

The mandatory verified compiler spine produces the authoritative
`ErasedProgram`, which produces a semantic `MachinePlan`. `MachinePlan` is the
hardware semantic input and the sibling software-oracle artifact used for
provenance/differential traces. Generic proof facts may establish boundedness,
purity, widths, totality, and hardware eligibility only after they pass through
`ContractVerifiedProgram`; hardware lowering may not recover or trust an
unverified source-level claim.

`CoreHardwareIR` is derived from:

```text
semantic MachinePlan
+ core hardware profile
```

The core profile fixes logical timing facts required by the design:

- bit widths;
- one clock/reset contract;
- logical storage port counts and read latency;
- source/handshake semantics;
- bounded capacities and maximum response wait;
- assertions and retirement interface.

`CoreHardwareIR` is invariant across environments that implement that same
logical core profile.

`TargetHardwareIR` is derived from:

```text
CoreHardwareIR
+ target/board capability profile
+ primitive/legalization and clock constraints
```

It may differ by target in primitive mapping, physical memories, adapters,
placement constraints, instrumentation, and realized clock. Every
`TargetHardwareIR` records its parent core digest. A target that cannot preserve
the core's logical ports/latency/protocol is rejected or uses an explicit
adapter already modeled in the core contract.

The IR layers contain, as appropriate:

- bit-vector and closed-Tag types;
- external typed ports;
- clocks and reset domains;
- combinational operations;
- registers and reset values;
- bounded `MAP` storage and access ports;
- finite-state-machine states and transitions;
- candidate writes and commit groups;
- assertions and assumptions;
- source/provenance mappings;
- declared latency and capacity;
- target eligibility and rejection reasons.

It does not contain:

- parser AST fallbacks;
- runtime string paths;
- generic recursive values;
- host pointers or Rust container layout;
- board pin numbers;
- example or processor-name special cases.

Native and Wasm cycle simulators consume normalized `CoreHardwareIR`.
SystemVerilog, formal wrappers, and board artifacts consume a normalized
`TargetHardwareIR` while retaining the parent core digest. Board pin numbers
remain in the separately digested shell, not the core IR.

## First CPU Contract

### ISA

Implement the complete little-endian RV32I base integer ISA, not a private
subset marketed as RV32I.

Architectural state:

- 32 registers of 32 bits;
- `x0` hardwired to zero;
- 32-bit program counter;
- byte-addressed 32-bit address semantics;
- a physically bounded memory supplied by the profile.

Instruction groups:

- `LUI`, `AUIPC`;
- `JAL`, `JALR`;
- conditional branches;
- immediate integer operations;
- register-register integer operations;
- `LB`, `LBU`, `LH`, `LHU`, `LW`;
- `SB`, `SH`, `SW`;
- `FENCE`;
- `ECALL`, `EBREAK`.

V1 policy:

- naturally aligned instructions and data accesses;
- misaligned instruction/load/store access produces a precise terminal trap;
- inaccessible memory produces a precise terminal trap;
- illegal instructions produce a precise terminal trap;
- `ECALL` and `EBREAK` produce distinct machine-readable terminal reasons;
- RV32I HINT encodings advance the PC with no other architectural effect;
- `FENCE` ignores `rd` and `rs1` as required by the base contract, treats
  reserved `fm`/predecessor/successor configurations conservatively as a
  fence, and may implement `FENCE.TSO` as the stronger ordinary fence;
- `FENCE` is a documented no-op only because the V1 port is strongly ordered
  and one-outstanding and the execution environment is single-hart with no
  cache or DMA.

Control-transfer fault attribution is explicit:

- a taken branch, `JAL`, or `JALR` to a non-four-byte-aligned target traps on
  that control-transfer instruction and performs no link-register write;
- a non-taken branch does not trap merely because its encoded target would be
  misaligned;
- `JALR` clears target bit zero before the four-byte alignment check;
- an aligned jump to an inaccessible address retires the jump; the subsequent
  fetch emits the separate execution-environment fault.

The plan follows the ratified
[RV32I base specification](https://docs.riscv.org/reference/isa/v20260120/unpriv/rv32.html).
Any implementation-time version is pinned in the proof manifest.

### Microarchitecture

Use a single-issue multicycle core:

```text
Reset
  -> FetchRequest
  -> FetchWait
  -> Decode
  -> Execute
  -> optional MemoryRequest
  -> optional MemoryWait
  -> Writeback
  -> Retire
  -> FetchRequest
```

Some states may safely merge after measurement, but the first implementation
prioritizes inspectability and proof over cycle count.

There is:

- one instruction in flight;
- one outstanding memory request;
- no pipeline hazards;
- no speculation;
- no cache coherence;
- no branch prediction;
- no reorder buffer.

This is large enough to exercise real hardware APIs and small enough to debug
from exact cycle traces.

### Decode

Decode is a closed checked result:

```text
Decoded[...]
IllegalInstruction[...]
```

Instruction classes carry already extracted/extended fields. Later states do
not repeat string- or opcode-pattern matching.

Required proofs:

- legal decode arms are mutually exclusive;
- every 32-bit instruction maps to exactly one legal class or illegal;
- sign extension and immediate bit placement match the ISA;
- encodings that the pinned spec/profile defines as illegal cannot execute a
  neighboring instruction; FENCE-reserved and HINT encodings follow their
  explicit policies above;
- `JALR` clears the required low target bit;
- shift amounts use only the legal RV32I width.

### Register File

The Boon source describes the register file as a bounded `MAP` from `BITS[5]`
to `BITS[32]`.

Compiler proof selects a direct-address layout with two reads and one write.
`x0` behavior is enforced at the architectural boundary and formally checked
on every cycle.

There is no handwritten RTL register file that bypasses Boon. A board-specific
primitive replacement is allowed only after sequential equivalence to the
generated generic bank is proved.

### Memory

The first simulator and FPGA SoC use a small bounded ROM/RAM image. This keeps
SDRAM-controller behavior out of initial CPU correctness.

Memory-mapped test facilities provide:

- pass;
- fail code;
- optional character/word trace;
- halt.

The board milestone adds a UART/USB loader or monitor only after the fixed-image
core passes. External SDRAM, caches, and networking come later.

## Artifact Boundaries

One build produces:

```text
checked program digest
erased program digest
semantic MachinePlan digest
core hardware profile digest
CoreHardwareIR digest
target/board capability profile digest
TargetHardwareIR digest
generated RTL digest
firmware image digest
board-shell digest, when applicable
proof/report manifest
```

The CPU's Boon source, semantic `MachinePlan`, core profile, and
`CoreHardwareIR` digest are identical across:

- native simulation;
- browser/Wasm simulation;
- each board profile.

Each RTL/formal/board artifact records that parent core digest. Its
`TargetHardwareIR`, instrumentation, primitive mapping, constraints, and shell
digests may differ and are compared only where their manifests declare
equivalence.

## Implementation Stages

### Stage 0: Freeze Contracts And Inventory Gaps

- Begin only after every prerequisite in the "Dependency On The Completed
  Universal Stack" section passes; reconcile the earlier read-only inventory
  against that unchanged revision.
- Reconcile the stale FPGA CLI/documentation claims.
- Inventory missing `BITS`, `MAP`, profile, IR, simulator, report, and toolchain
  pieces.
- Pin the RV32I spec, architectural tests, reference model, formal framework,
  synthesis tools, and board definitions.
- Inventory the exact owned board revisions and FPGA parts.
- Freeze report schemas before implementation.

Exit: no plan or command claims hardware capability that the repository cannot
execute.

### Stage 1: Generic Hardware API Fixtures

Implement unrelated fixtures first:

1. register with reset and increment;
2. combinational ALU over `BITS`;
3. bounded two-read/one-write `MAP` register bank;
4. wait-state request/response RAM adapter;
5. small protocol FSM with assertions.

For each fixture:

- native and Wasm cycle traces agree;
- normalized `CoreHardwareIR` and generic-simulation `TargetHardwareIR` are
  deterministic and parent-linked;
- generated RTL simulates equivalently;
- synthesis reports widths, storage, resources, and timing;
- invalid/unbounded variants fail with useful diagnostics.

Exit: the processor will consume a generic API rather than create special
compiler branches.

### Stage 2: Architectural RV32I Step Model In Boon

Write a pure architectural instruction-step model in Boon:

```text
(architectural state, instruction, memory response)
  -> (next architectural state, memory request, retirement result)
```

Use it to validate:

- decode;
- immediate extraction;
- ALU;
- branches/jumps;
- byte masks and sign extension;
- load/store merge behavior;
- traps.

This model is a test oracle and specification aid. It is not generated RTL and
not a production fallback.

Exit: every RV32I instruction family has direct and randomized differential
coverage against a pinned external oracle.

### Stage 3: Multicycle CPU In Boon

- Add the cycle FSM and unified memory port.
- Integrate the bounded `MAP` register file.
- Add fetch, wait states, writeback, traps, and retirement.
- Add deterministic reset, `MAX_MEMORY_WAIT`, per-instruction cycle bounds, and
  bounded progress assertions.
- Run flat binaries and self-checking firmware in the native simulator.

Exit: the Boon-authored core executes mixed instruction programs, not only
isolated operations.

### Stage 4: Browser/Wasm Simulation

- Build the same `CoreHardwareIR` cycle simulator for Wasm.
- Load the same firmware image and input schedule.
- Produce the same retirement bytes and final state digest.
- Expose a machine-readable step/run/reset/state/trace API. Presentation,
  breakpoints, and waveform UI belong to Stage 8.

Exit: native and browser/Wasm proof artifacts are byte-identical where the
manifest declares them canonical.

### Stage 5: Generated RTL

- Elaborate `CoreHardwareIR` into a parent-linked `TargetHardwareIR`.
- Prove that target elaboration preserves core-visible ports, candidates,
  commit, and retirement under the same response schedule.
- Emit one canonical SystemVerilog form from `TargetHardwareIR`.
- Lint and simulate it.
- Compare external ports and retirement events cycle by cycle against the
  `TargetHardwareIR` simulator.
- Synthesize with Yosys and place/route with the selected target flow.
- Emit resource, inferred-memory, clock, and timing reports.

Exit: there is no manual functional RTL patch between Boon and the passing RTL.

### Stage 6: Architectural And Formal Verification

- Run pinned RV32I architectural certification tests or their suitable
  machine-mode-free profile.
- Run focused instruction/signature tests.
- Differentially execute randomized bounded programs against the pinned Sail
  model; use Spike only as secondary evidence.
- Generate an RVFI-compatible verification adapter from the retirement port.
- Run the pinned `riscv-formal`/SymbiYosys subset supported by the core profile.
- Add core-specific safety and bounded-progress properties.

Exit: a self-written smoke program is not the only evidence.

Generic `WHERE` proofs and `ContractVerifiedProgram` may justify hardware
eligibility and translation steps, but they do not prove RV32I compliance.
Architectural tests, Sail differential execution, RVFI, `riscv-formal`, and
core-specific properties remain independent mandatory evidence.

### Stage 7: First FPGA Board

The working default is the owned iCESugar-Pro because it provides an ECP5
LFE5U-25F, open Yosys/nextpnr support, on-chip memory, SDRAM, programming, and a
USB serial path. The exact board revision and available resources are verified
from the physical board before freezing the profile; the
[official board repository](https://github.com/wuxx/icesugar-pro) is the
starting reference.

First board image:

```text
Boon RV32I core
+ small on-chip firmware ROM
+ small on-chip data RAM
+ machine-readable pass/fail/signature port
+ minimal one-way result UART
+ optional LED heartbeat, never sole proof
```

The board gate:

- programs the exact reported bitstream;
- runs an exact firmware digest;
- returns the final memory signature;
- for a bounded trace fixture, returns either the verbatim slow-step/FIFO trace
  or a trace digest produced by a separately generated and proved monitor;
- matches simulation;
- records utilization, achieved clock, tool versions, and board identity.

Only after this passes:

- add a firmware loader;
- use external SDRAM;
- add richer peripherals;
- compare another board.

### Stage 8: Boon Orchard Projection

Expose existing proof data to the game:

- instruction flow;
- current FSM state;
- register-bank activity;
- bus waits;
- retirement;
- failed assertions/tests;
- resource and timing budgets;
- native/Wasm/RTL/board artifact lineage.

Build presentation here on top of the Stage 4 machine-readable API: pause,
breakpoint-by-PC, register/memory inspection, waveform navigation, and
source/proof selection.

The game does not define CPU correctness. It visualizes and motivates the same
evidence produced by the compiler and verification pipeline.

Exit: completing the in-game processor corresponds to passing the real
processor proof manifest.

## Verification Contract

### Instruction Fixtures

Every instruction family covers:

- normal operands;
- zero and all-one values;
- signed boundaries;
- immediate boundaries;
- destination `x0`;
- source `x0`;
- taken and not-taken branches;
- positive and negative offsets;
- aligned lowest/highest in-profile addresses;
- illegal and misaligned cases;
- memory response delay and fault;
- taken branch/`JAL`/`JALR` misaligned-target traps with no link write;
- the corresponding non-taken branch without a trap;
- `JALR` bit-zero clearing before alignment validation;
- aligned control transfer followed by a distinct inaccessible-target fetch
  fault;
- HINT and reserved-FENCE behavior;
- read/write handshake backpressure and stable fields;
- faulting stores with no external side effect.

### Differential Execution

For one firmware image, initial state, memory profile, and external response
schedule, compare:

```text
architectural step model
native CoreHardwareIR simulator
Wasm CoreHardwareIR simulator
TargetHardwareIR simulator
generated RTL simulator
pinned external ISA oracle at retirement boundaries
```

Canonical comparison includes:

- retirement sequence;
- PC and instruction;
- register writes;
- memory operations and masks;
- traps/exits;
- final registers;
- final observable memory;
- pass/fail signature.

Microarchitectural intermediate cycles are compared between
`TargetHardwareIR` and RTL. Core-to-target elaboration is compared at the
core-visible protocol and retirement boundary. The external ISA oracle is
compared at retirement boundaries.

### Architectural Tests

Use the current
[RISC-V Architectural Certification Tests](https://github.com/riscv/riscv-arch-test)
with a pinned configuration and the Sail reference results where compatible
with the V1 execution environment.

The pinned Sail unprivileged model is the normative external ISA/trap oracle.
Spike may be a secondary differential tool, but its usual platform model cannot
silently supply privileged trap behavior absent from this V1 execution
environment.

Legacy
[`riscv-tests`](https://github.com/riscv-software-src/riscv-tests) may provide
additional bring-up coverage but is not the sole compliance claim.

An ACT excluded behavior cannot disappear from the "complete RV32I" claim. Each
exclusion is covered by a dedicated Sail differential fixture, explicit
terminal-trap fixture, or formal property, and the manifest links that
replacement evidence.

The manifest records:

- upstream repository and commit;
- selected ISA/profile;
- excluded tests and exact reasons;
- firmware compiler and flags;
- generated ELF/flat-image digests;
- expected and actual signatures.

### Formal Properties

At minimum:

- `x0` always reads zero;
- writes to `x0` have no effect;
- one-hot legal decode or illegal;
- at most one register write per retirement;
- no register or memory write after a faulting instruction;
- byte enables match access width and address;
- aligned loads reconstruct correct values and sign extension;
- aligned stores affect only selected bytes;
- PC changes match sequential/branch/jump/trap semantics;
- one instruction retires at most once;
- retirement order equals fetch order;
- request fields remain stable until request acceptance;
- response fields remain stable until response acceptance;
- exactly one response follows each accepted request;
- at most one memory request is outstanding;
- a faulting store has no external side effect;
- reset emits no retirement or ordinary source/delta event;
- reset reaches the first fetch state;
- under the profile assumption that an accepted request receives a response
  within `MAX_MEMORY_WAIT`, each instruction reaches retire or trap within its
  derived `MAX_INSTRUCTION_CYCLES`;
- without that response, the core may remain in the wait state but keeps
  protocol fields stable and performs no architectural commit.

The generated retirement port should adapt to
[`riscv-formal`'s RVFI](https://github.com/YosysHQ/riscv-formal). The adapter is
generated or declarative; it does not duplicate core behavior.

### RTL And Synthesis

Use the open Yosys/nextpnr flow where supported. The report includes:

- tool versions and digests;
- top module and board shell;
- generated RTL digest;
- inferred registers/RAMs/multipliers;
- LUT/FF/RAM usage;
- unconstrained paths;
- requested and achieved clock;
- warnings promoted or explicitly classified;
- post-synthesis and post-route status.

A successful synthesis with an unconstrained clock is not timing evidence.

### Board Evidence

No human-recognized LED pattern is sufficient.

Required:

- exact process/programmer exit status;
- board and FPGA identity;
- firmware and bitstream digests;
- UART/USB machine-readable pass/fail;
- final memory signature for every test;
- for a declared bounded trace test, either a verbatim slow-step/FIFO trace or
  a separately generated/proved trace-monitor digest matching simulation;
- timeout/failure behavior;
- fresh report from the final source.

## Board Decision Strategy

Do not select Zeus or another board by aspiration.

Before choosing a second target, record for each candidate:

- exact available silicon and board revision;
- logic, register, BRAM, DSP, and clock resources;
- open versus licensed toolchain requirements;
- simulation and CI reproducibility;
- programming/debug path;
- UART/USB/JTAG support;
- external RAM and its controller maturity;
- networking/peripheral needs;
- measured synthesis/place/route results for the same generated core.

Run the same core through the best two candidate profiles. Choose from evidence.

Likely hierarchy:

```text
first proof
  -> on-hand, open-toolchain FPGA

next system prototype
  -> board with stronger memory/peripheral integration

future acceleration research
  -> RVV/heterogeneous platforms, including Zeus-like systems when real
     hardware and tools justify the work

long-term
  -> custom Boon-native scalar/vector/spatial machine
```

The board shell changes; the Boon core does not.

## Stable Boon-FPGA API Criteria

The project succeeds as an API stabilizer only if:

1. counter, ALU, register bank, RAM adapter, protocol FSM, and CPU use the same
   generic hardware primitives;
2. no compiler branch recognizes `riscv`, an instruction name, or the game;
3. clock/reset, widths, bounds, ports, latency, and overflow/fault rules are
   explicit;
4. combinational versus registered `MAP` reads cannot be confused;
5. both IR layers are normalized, versioned, deterministic, source-mapped, and
   linked by the parent `CoreHardwareIR` digest;
6. native and Wasm consume the same `CoreHardwareIR`; RTL, formal, and board
   flows consume parent-linked `TargetHardwareIR`;
7. target-ineligible constructs fail with specific diagnostics;
8. generated RTL contains no host/runtime container dependency;
9. board adapters cannot bypass or patch core semantics;
10. proof reports are machine-readable and freshness-bound to all source,
    profile, firmware, tool, and board-shell inputs.

When Boon exposes a real compiler/typechecker/runtime limitation during this
work, fix the generic owner. Do not encode a RISC-V-specific Boon workaround as
the final design.

## Explicit Non-Goals

- Boon self-hosting.
- Running the Boon compiler or runtime on the first CPU.
- Linux or another operating system.
- privileged ISA modes.
- CSRs, interrupts, timers, debug module, or PMP in V1.
- `M`, `A`, `C`, `F`, `D`, `V`, bit-manipulation, or custom ISA extensions.
- caches, MMU, branch prediction, pipelining, multicore, or coherence.
- networking.
- a server workload.
- choosing the later useful real-world application.
- FPGA SDRAM in the first correctness milestone.
- vendor-specific hard-IP generation as the only implementation.
- winning an Fmax or CoreMark competition.
- broad board support before one board is completely proved.
- a public `MEMORY` keyword.
- handwritten Rust or RTL CPU logic as acceptance evidence.
- deciding that RISC-V is the ultimate Boon-native architecture.

## Acceptance Criteria

The plan is complete only when:

1. Every language, compiler, formal, packed-runtime, mature-web-stack, and
   fresh cross-target prerequisite named by this plan passed before Stage 0.
2. The architectural CPU logic is Boon source.
3. The implemented ISA contract is complete RV32I for the declared execution
   environment.
4. `BITS[N]` and bounded `MAP` are general language/compiler features.
5. The register file is a compiler-selected bounded `MAP` layout.
6. No `MEMORY` keyword exists.
7. The cycle hot path contains no recursive runtime value, string field lookup,
   or standard map/set container.
8. Generic hardware fixtures pass before the CPU depends on them.
9. One normalized `CoreHardwareIR` drives native/Wasm and every target
   elaboration; each `TargetHardwareIR` records that parent digest.
10. Native and Wasm retirement traces and final digests agree.
11. Generated RTL matches `TargetHardwareIR` ports and retirement cycle by
    cycle, and target elaboration preserves the core-visible contract.
12. Architectural signatures match a pinned independent reference.
13. Required formal properties pass.
14. Yosys synthesis and target place/route produce bounded fresh reports.
15. The physical board returns a machine-readable signature matching
    simulation.
16. The core source, semantic plan, core profile, and `CoreHardwareIR` digest
    are unchanged across board profiles; target IR and shell digests are
    separately recorded.
17. The Boon Orchard milestone consumes the real proof bundle.
18. Self-hosting, Linux, networking, and a useful server workload were not
    smuggled into the critical path.
19. The memory port proves one acceptance/one response, stable backpressure,
    little-endian lane behavior, one outstanding transaction, and no side
    effect on a faulting store.
20. Hardware `SOURCE` presence and `LATEST` sequence arbitration match software
    semantics; synthesis/source order never chooses a winner.
21. The register `MAP` is explicitly total after the declared reset and cannot
    yield `NotFound`.
22. Instruction traps and pre-instruction fetch faults have distinct,
    unambiguous trace/RVFI treatment and no forbidden side effects.
23. `MAX_MEMORY_WAIT` and per-instruction cycle bounds are proved under the
    declared response assumption; an unbounded stall remains safe.
24. Every architectural-test exclusion has replacement Sail, dedicated
    fixture, or formal evidence.

## Risks And Mitigations

### The CPU Becomes A One-Off Hardware DSL

Risk: compiler features are added only because one instruction needs them.

Mitigation: every primitive first passes unrelated fixtures; no RISC-V-aware
backend branches.

### MAP Semantics Are Distorted Into RAM Syntax

Risk: hardware latency or ports leak into public collection semantics.

Mitigation: `MAP` remains semantic; a target profile selects layout. Latency is
represented by explicit request/response adapters, never a silent semantic
change.

### The First Core Scope Expands Toward Linux

Risk: CSRs, interrupts, MMU, caches, SDRAM, and drivers delay the actual
Boon-hardware proof.

Mitigation: freeze RV32I multicycle plus bounded ROM/RAM and machine-readable
test port. Everything else requires a new plan.

### Simulation Passes But Generated RTL Drifts

Risk: RTL generation or vendor memories change timing/behavior.

Mitigation: cycle-exact `TargetHardwareIR`/RTL port and retirement comparison,
core-to-target contract equivalence, and sequential equivalence for any
primitive substitution.

### A Board Demo Hides Weak Verification

Risk: an LED blink is treated as CPU completion.

Mitigation: independent architectural tests, differential traces, formal
properties, and signature-bearing board evidence are mandatory.

### Board Or Vendor Availability Changes

Risk: a planned Zeus or other board is delayed, unavailable, or poorly
supported.

Mitigation: source, semantic plan, `CoreHardwareIR`, simulators, and formal
contract are board-independent. Target IR and shells are derived and
parent-linked. The first board is selected from owned/measured hardware, and
simulation remains a required product.

## End State

At completion, a player or engineer can open one Boon RV32I design and follow
it all the way down:

```text
readable Boon processor source
  -> checked hardware meaning
  -> inspectable cycle simulation
  -> browser/Wasm execution
  -> generated RTL
  -> formal and architectural proof
  -> a real FPGA running the same core
```

That is the first credible BoonComputer milestone: not self-hosting and not yet
the ultimate Boon-native processor, but a real scalar CPU designed in Boon that
forces the language, compiler, runtime, simulator, hardware IR, and FPGA API to
work together.
