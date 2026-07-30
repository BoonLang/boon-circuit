# BoonConsole Architecture

Status: canonical product and system architecture. No BoonConsole production
implementation exists yet.

This document owns the stable meaning of **BoonConsole**: one virtual and
physical console, one Boon-designed RV32I system, one replaceable standalone
`app.wasm`, and one bounded terminal bridge. The executable implementation
sequence, experiments, reports, and deletion gates are owned by
[`../plans/BOON_CONSOLE_IMPLEMENTATION_PLAN.md`](../plans/BOON_CONSOLE_IMPLEMENTATION_PLAN.md).
The generic processor remains owned by
[`../plans/BOON_FIRST_RISCV_PROCESSOR_PLAN.md`](../plans/BOON_FIRST_RISCV_PROCESSOR_PLAN.md).

The repository does not currently implement the console port, standalone Boon
Wasm emitter, hardware IRs, cycle simulator, RTL generator, RV32I core, SoC,
onboard interpreter, board shell, console bridge, or BoonConsole handoff
reports. This document is a contract for building them, not evidence that they
exist.

## Decision Summary

BoonConsole V1 is:

- a required virtual console and a semantically equivalent physical console;
- an iCESugar Pro module using its Lattice ECP5 LFE5U-25F FPGA;
- one Pmod BTN with four buttons;
- one Pmod SWT with four switches;
- one Pmod 8LD with eight individually addressable indicators;
- one Pmod SSD with two seven-segment digits;
- one Pmod CLS with a 16-by-2 character display;
- all five external modules connected and usable at the same time;
- no dependency on the iCESugar Pro onboard RGB LED;
- a Boon-designed RV32I core plus a small fixed kernel;
- an interpreter-first application platform executing the exact uploaded
  standalone `app.wasm`;
- a local terminal bridge over the iCELink USB CDC path;
- a deterministic simulator and hardware-in-the-loop verification target.

The first useful result is not a game level, a web product, an AOT workaround,
or a decorative FPGA demo. It is a replaceable Boon application whose exact
Wasm bytes run in the PC reference environment, the simulated SoC, and the
physical RV32I system, with equivalent logical input and output traces.

## Scope

V1 must prove all of these together:

1. Boon source reaches the mandatory verified compiler spine.
2. A bounded console application profile emits a standalone `app.wasm`.
3. The same `app.wasm` passes a PC reference interpreter and the simulated SoC.
4. A Boon-authored RV32I core reaches `CoreHardwareIR`, cycle simulation,
   `TargetHardwareIR`, generated RTL, formal/architectural proof, synthesis,
   and the physical FPGA.
5. A fixed kernel on that processor validates and interprets the original
   `app.wasm`.
6. Physical and virtual controls use the same logical `ConsolePort`.
7. Every physical module is driven by the same committed output frame.
8. A bounded binary bridge can inspect, upload, start, stop, reset, and recover
   the console without becoming an application runtime.
9. App, state, kernel, bitstream, board profile, simulator, and report identities
   remain distinguishable and traceable.
10. The final hardware-in-the-loop gate proves behavior; human observation is
    a separate optional follow-up.

## Non-Goals

V1 does not require:

- Boon self-hosting;
- the Boon compiler running on RV32I;
- Boon source compiling to RV32I machine code;
- host-side Wasm-to-RV32I AOT;
- Linux, an RTOS, WASI, a filesystem, sockets, or a network stack on the FPGA;
- a browser, phone app, GUI bridge, public server, cloud service, or paid
  deployment;
- live migration between machines;
- multiple apps executing concurrently;
- a public `MEMORY` keyword;
- a custom public integer type that changes Boon's exact `Number` semantics;
- a second unchecked compiler or runtime path;
- use of the onboard RGB LED for status or proof;
- a game implementation or Boon Orchard campaign gate;
- broad support for boards other than the measured iCESugar Pro assembly;
- handwritten functional CPU RTL;
- manually edited generated RTL;
- an LED-only or visual-only hardware acceptance test.

## Physical Assembly

The physical product target is one exact, inventoried assembly:

| Part | V1 role | Required |
| --- | --- | --- |
| iCESugar Pro, ECP5 LFE5U-25F | FPGA, SDRAM, SPI flash, iCELink programming and USB CDC | yes |
| Pmod BTN | four debounced buttons | yes |
| Pmod SWT | four sampled switches | yes |
| Pmod 8LD | eight positional indicators | yes |
| Pmod SSD | two seven-segment glyphs | yes |
| Pmod CLS | 16-by-2 text display | yes |

The [official iCESugar Pro repository](https://github.com/wuxx/icesugar-pro)
identifies an LFE5U-25F-6BG256C, 56 18-Kib sysMEM blocks, 32 MiB SDRAM,
32 MiB SPI flash, a 25 MHz crystal, iCELink programming, and a USB CDC path.
Those are upstream reference facts, not a substitute for inspecting the exact
owned module and extension board. The V1 board profile is not frozen until the
physical revision, schematic, connector orientation, FPGA package, oscillator,
SDRAM/flash parts, iCELink firmware, shared JTAG/GPIO lines, and every external
module revision have been recorded and checked.

All external modules must operate simultaneously. A profile or demo that
borrows the same physical pin for two active roles does not conform.

### Power And Electrical Rules

All logic is 3.3 V. Before connecting all modules:

- calculate the worst-case current from the exact module revisions and
  schematics;
- measure the extension-board 3.3 V rail at idle and worst-case output;
- record inrush, steady-state, and margin;
- verify every FPGA bank voltage and every module's logic-level requirement;
- verify connector polarity and ground continuity;
- keep the board unpowered while rewiring.

The Digilent Pmod interface specification does not justify assuming more than
approximately 100 mA from an arbitrary host. If the measured assembly cannot be
powered safely by the extension-board rail, use one regulated external 3.3 V
rail sized from the measured load, join grounds, and disconnect the board-side
3.3 V feed. Never connect two enabled regulators in parallel.

### Pmod CLS

V1 prefers the CLS UART mode because console output is write-only and a serial
adapter costs fewer FPGA pins than a parallel display contract. The exact module
revision and jumper state must be recorded: Digilent documents both old and new
Pmod UART pin conventions. A physical loopback/known-text test must establish
the actual pin convention and baud rate before the board profile is accepted.

The display driver owns bounded initialization, clear, cursor placement, and
fixed 32-character frame writes. Reading from the display is not part of V1
unless the electrical bring-up experiment shows that a write-only UART cannot
provide deterministic recovery.

## Logical ConsolePort

`ConsolePort` is a target-neutral typed capability. Board names, connector
numbers, voltage, pin names, UART baud rates, and display-driver timing never
appear in Boon source or semantic application identity.

The V1 logical surface is:

```text
ConsoleInput
  Boot
  ButtonChanged(index: 1..4, pressed: Tag, switches: BITS[4])
  SwitchesChanged(switches: BITS[4])
  TimerFired(slot: 1..8, generation: BITS[32])
  BridgeConnected
  BridgeDisconnected
  BridgeMessage(channel: BITS[8], payload: bounded BYTES)

ConsoleFrame
  indicators: BITS[8]
  seven_segment_left: SevenSegmentGlyph
  seven_segment_right: SevenSegmentGlyph
  character_lines: fixed 2 x 16 display cells
  timer_commands: bounded commands for slots 1..8
  bridge_messages: bounded binary messages
```

`pressed` uses closed Tags such as `Pressed` and `Released`; it is not a
privileged Boolean. Display glyphs are a closed, target-checked set. Text sent
to the Pmod CLS is a fixed-size display cell array, not an unbounded runtime
string.

Every accepted event carries:

- a boot-scoped monotonically increasing event sequence;
- an ingress cycle or virtual equivalent;
- its event kind and bounded payload;
- the sampled four-switch snapshot;
- the current app-instance generation.

The app cannot access ambient wall time, random data, filesystem, network,
board pins, USB, or display drivers.

### Port Ownership Before CoreHardwareIR

The existing repository has typed HTTP and WebSocket host ports. It has no
console port. BoonConsole extends the same verified ownership pattern; it does
not reinterpret an HTTP/WebSocket port or recover a port from source spelling
in a backend.

Ownership is:

```text
Boon SOURCE/output declarations
  -> CheckedProgram: typed references and target eligibility inputs
  -> SemanticProgram: logical ConsolePort bindings, stable semantic IDs,
     effects, ownership, bounds, and proof obligations
  -> ContractVerifiedProgram: accepted bounds and contracts
  -> ErasedProgram: verified target-neutral port declarations
  -> MachinePlan: semantic event/output routes and required capacities
  -> CoreHardwareIR: target-independent clocked signal/handshake realization
  -> TargetHardwareIR: primitive and timing realization
  -> board profile/shell: physical pins, connectors, drivers, and voltage
```

`boon_semantic` owns the logical port identity before `CoreHardwareIR`.
`MachinePlan` is the sole semantic backend input. Board profiles map a logical
port only after verification and cannot add application behavior.

## Two Verified Paths

### Permanent Hardware Path

The processor and fixed shell follow:

```text
Boon hardware source
  -> ParsedProgram
  -> CheckedProgram
  -> SemanticProgram
  -> ContractVerifiedProgram
  -> ErasedProgram
  -> MachinePlan
  -> CoreHardwareIR
  -> native cycle simulator
  -> TargetHardwareIR
  -> generated SystemVerilog
  -> RTL and formal verification
  -> Yosys synthesis
  -> nextpnr-ecp5 place and route
  -> ECP5 bitstream
  -> iCESugar Pro
```

The CPU, bus arbitration, commit rules, and hardware state machines are Boon
semantics. A target shell may instantiate clocks, resets, ECP5 memories, SDRAM,
SPI flash, UART, synchronizers, and pin adapters. It may not reimplement CPU or
application decisions.

### Replaceable Application Path

Applications follow:

```text
Boon app source
  -> the same checked and verified spine
  -> bounded BoonConsole app profile
  -> standalone app.wasm
  -> PC reference interpreter
  -> simulated BoonConsole SoC
  -> exact same bytes uploaded to the physical console
  -> fixed kernel and interpreter on Boon RV32I
```

The portable artifact is always the original standalone Wasm module. A native
host executable, browser-host Wasm bundle, `MachinePlan` JSON, derived RV32I
firmware, or source file cannot be labelled `app.wasm`.

Interpreter-first is a V1 invariant. AOT may be researched later as a separately
reported optimization, but it cannot be a prerequisite, compatibility path, or
substitute for the final exact-byte interpreter gate.

## Standalone App Profile

The profile is a compiler target over ordinary Boon, not a second language.
V1 admits only constructs whose bounds and representation are proved before
emission.

The initial eligibility envelope is:

- `BITS[N]` with statically known positive width;
- closed Tags and fixed records;
- fixed-count state cells and deterministic turn commit;
- bounded whole-valued exact `Number` only where verification proves a fixed
  signed or unsigned machine range and every operation remains exact;
- fixed display-cell arrays;
- bounded BYTES payloads at the bridge boundary;
- bounded timers;
- statically bounded control flow;
- no recursion;
- no floating point;
- no unbounded `Text`, `LIST`, `MAP`, `SET`, allocation, or collection
  traversal;
- no host, file, content, HTTP, WebSocket, persistence, or distributed effects;
- no WASI imports;
- one initial linear memory with a profile-owned maximum;
- deterministic integer-only WebAssembly MVP instructions selected by the
  emitter.

This is an eligibility restriction, not a change to public semantics. In
particular, a bounded whole `Number` remains exact `Number` at the semantic
boundary. The compiler rejects an application when it cannot prove the chosen
fixed representation; it does not round, wrap, or silently reinterpret it.

The first implementation phase freezes a versioned module ABI after comparing
interpreter candidates. The semantic ABI is already fixed:

1. the kernel supplies exactly one encoded event;
2. the app evaluates one atomic Boon turn;
3. the app returns one bounded candidate transaction;
4. the kernel validates the entire transaction;
5. state, output frame, timers, and bridge sends commit together;
6. a trap, fuel exhaustion, invalid pointer, invalid encoding, or capacity
   failure commits nothing.

The raw Wasm export names, memory offsets, and allocator strategy are not frozen
by this architecture document because they depend on the interpreter experiment.
They must be frozen in one versioned ABI fixture before the first emitter lands.

## Deterministic Event Ordering

Physical inputs are synchronized and debounced by the fixed shell. One sampling
cycle produces at most one input batch.

Within a batch:

1. a changed switch mask emits `SwitchesChanged`;
2. button changes emit by button index from 1 through 4;
3. each button event carries the same post-sample switch snapshot.

The global ingress arbiter orders events by accepted hardware cycle. Ties use:

1. physical input batch;
2. expired timer slot, ascending slot number;
3. accepted bridge frame order.

Every emitted event receives the next global sequence. The virtual harness
implements the same acceptance cycles and tie order. Source order, hash-map
order, OS scheduling, UART packet boundaries, and display timing never choose
semantic order.

One event is processed per application turn. No later event observes candidate
state or output from a failed turn.

## Capacity And Backpressure

The logical V1 queues are deliberately small and explicit:

| Resource | V1 capacity | Overflow policy |
| --- | ---: | --- |
| accepted input events | 64 events | enter recoverable `InputOverflow` safe fault; never drop an edge silently |
| timer slots | 8 | setting an occupied slot replaces it with a new generation |
| bridge RX app messages | 16 frames | reject the new frame with `Busy`; do not enqueue |
| bridge TX app messages | 16 frames | fail the app candidate transaction; do not partially commit |
| bridge app payload | 256 bytes | reject before enqueue |
| app candidate transaction | 1,024 bytes | reject the whole turn |
| in-flight application turns | 1 | no reentrancy |

Protocol control frames, app upload chunks, and recovery replies have separate
fixed kernel queues so an app cannot make recovery unavailable. Their exact
capacities are board-profile fields frozen with the kernel ABI.

The event fuel count, maximum Wasm pages, kernel stack, interpreter heap,
uploaded app size, persisted state size, and upload chunk size are mandatory
numeric board-profile fields. Their values remain unresolved until the Phase 0
BRAM/SDRAM and interpreter experiment. A profile with an unset value is invalid
and cannot produce a readiness report.

## Output Commit And Drivers

An application turn produces a complete desired `ConsoleFrame`. The kernel
validates it, advances the state generation, and publishes it atomically. The
fixed output drivers then converge each physical device toward that committed
frame:

- Pmod 8LD and Pmod SSD update from registered values;
- Pmod CLS transmits only the bytes needed for the committed 32-cell frame, or
  a complete bounded rewrite after reset;
- bridge messages become visible only after the same commit;
- virtual output changes at the same semantic commit sequence.

Physical driver latency is evidence, not semantic event ordering. A new frame
may supersede an older pending CLS frame only at a documented command boundary;
the driver must eventually show the latest complete frame and report its
presented generation.

## Virtual Console

The virtual console is mandatory verification infrastructure. It is not a
browser product and does not require a GUI.

It exposes:

- exact logical buttons, switches, indicators, seven-segment glyphs, and
  display cells;
- deterministic input schedules;
- cycle/turn stepping;
- app upload and reset;
- committed and presented output generations;
- raw protocol capture;
- machine-readable state, trace, and digest reports.

The PC reference interpreter and simulated SoC must consume the same app ABI
fixtures. The simulated SoC additionally executes the RV32I kernel and selected
interpreter under the cycle model. Direct PC interpretation is an independent
semantic oracle, not the physical acceptance substitute.

## Fixed Kernel And Minimum SoC

The fixed kernel owns only:

- boot and hardware self-test;
- app slot validation;
- interpreter initialization and fuel;
- event queue arbitration;
- atomic candidate validation and commit;
- timer slots;
- console-frame publication;
- bounded bridge protocol;
- reset and recovery;
- optional persistent app/state journal;
- machine-readable diagnostics.

The V1 SoC contains:

- the proved Boon RV32I core;
- boot ROM;
- on-chip scratch/data RAM;
- one bounded system bus;
- timer/cycle counter exposed only to the fixed kernel;
- interrupt-free or explicitly polled console ingress;
- GPIO/synchronizer/debounce blocks for BTN and SWT;
- registered 8LD and SSD outputs;
- a bounded CLS UART transmitter;
- iCELink CDC UART RX/TX;
- SPI flash controller when persistence lands;
- SDRAM controller only if the measured interpreter/app memory budget requires
  it;
- trace/signature support required by verification.

The app receives no raw memory-mapped I/O. The kernel is the only app capability
host.

## Bridge Protocol

The bridge is a local terminal tool named `boon-console`. It has no browser,
HTTP listener, WebSocket listener, discovery server, remote administration, or
application UI.

Its stable command families are:

- `probe`;
- `status`;
- `install --volatile <app.wasm>`;
- later `install <app.wasm>`;
- `start`;
- `stop`;
- `reset-app`;
- `reset-soc`;
- `events`;
- `send`;
- `recover`;
- `verify`.

The wire protocol is a dedicated bounded binary protocol carried over iCELink
USB CDC. It uses versioned positional fields, explicit lengths, boot/session
identity, monotonic sequences, integrity checking, and fail-closed decoding.
JSON and the general recursive Boon `Value` codec are forbidden on the device
hot path. `boon_wire` may own the codec, but existing distributed session
frames are not reused as a console protocol.

The exact framing transform and checksum are frozen by hostile-input and
resynchronization fixtures before device code lands. Frames may be split or
coalesced arbitrarily by USB/UART without changing their meaning.

The bridge never interprets application logic. It may render diagnostics,
compare digests, validate a module with the same profile, and run the PC oracle.

## Identity And Protocol Handshake

Every connection begins with a device-owned hello containing:

- protocol and app-ABI versions;
- boot nonce and boot counter;
- board-profile digest;
- bitstream digest;
- `TargetHardwareIR` digest;
- kernel digest;
- interpreter identity and build digest;
- installed app digest or explicit absence;
- installed state schema/digest or explicit absence;
- reset reason;
- capability and numeric limit table;
- current app generation and event sequence;
- safe/fault state.

The host replies with the expected protocol range and a fresh host nonce.
Mutating commands bind both nonces and a request sequence. A reply echoes the
request identity. Replayed, stale-boot, out-of-window, malformed, oversized, or
wrong-version frames fail without changing state.

The full lineage is:

```text
source bundle digest
semantic program digest
verification manifest digest
ErasedProgram digest
MachinePlan digest
app-profile digest
Wasm-emitter identity
app.wasm SHA-256

hardware source bundle digest
semantic/verification/erased/MachinePlan digests
core-profile digest
CoreHardwareIR digest
target-profile digest
TargetHardwareIR digest
generated RTL digest
constraint/shell digest
toolchain identity
bitstream digest
kernel/interpreter digest
board identity

scenario digest
input-schedule digest
protocol-capture digest
committed-frame trace digest
presented-output evidence digest
final state digest
report digest
```

A report must never collapse app, kernel, firmware, bitstream, or board identity
into one ambiguous "build hash."

## Reset, Disconnect, And Persistence

The console has explicit reset classes:

| Action | App bytes | Committed app state | Event queue | Physical frame | Bridge |
| --- | --- | --- | --- | --- | --- |
| `reset-app` | retained | reset to app initial state, unless `--restore` is explicitly requested later | cleared, then `Boot` | safe frame until first commit | remains available |
| `reset-soc` | retained only if installed persistently | retained only if journaled and compatible | cleared | safe frame | reconnects with new boot nonce |
| FPGA reconfiguration | retained only in SPI flash | retained only in SPI flash | lost | outputs reset/safe | reconnects |
| power loss | retained only in SPI flash | last complete journal commit only | lost | unpowered, then reset/safe | reconnects |
| USB disconnect | unchanged | unchanged | app continues; bridge messages receive bounded backpressure | unchanged | unavailable until reconnect |
| app trap/fuel fault | unchanged | last committed state | later app events held for recovery | safe frame | recovery remains available |

The earliest end-to-end milestone is intentionally volatile: `app.wasm` is
uploaded into RAM and is lost on SoC reset, FPGA reconfiguration, or power loss.
No volatile run may be presented as persistent installation.

Persistent installation later uses content-addressed, dual-slot metadata and an
atomic journal in SPI flash. A new app is activated only after complete write,
readback, digest validation, profile validation, and an atomic active-slot
record. State restoration requires exact app identity or a verified migration;
otherwise the kernel starts initial state and preserves the incompatible bytes
for diagnostic recovery.

USB disconnect does not stop local application behavior. It cannot grant or
revoke application capabilities because V1 has no external network authority.

## Safe State

The fixed shell owns a safe frame independent of application code:

```text
Pmod 8LD: 10000001 (the two outer indicators steady)
Pmod SSD: --
Pmod CLS line 1: "BOON SAFE       "
Pmod CLS line 2: "CODE 0000       "
```

The four hexadecimal code cells are replaced with a stable fault code. Leading
and trailing spaces are part of each 16-cell line. The bridge reports the same
code and structured reason.

The safe frame is entered after reset until a valid first app commit, and after
app trap, fuel exhaustion, invalid transaction, input overflow, failed app
validation, or persistence corruption. A driver-specific electrical failure
deasserts the affected outputs where possible and remains observable through
the bridge. The onboard RGB LED has no safety meaning.

## Accessibility

No essential state is color-only:

- Pmod 8LD meaning is positional and duplicated in terminal output;
- Pmod SSD uses glyphs and terminal text;
- Pmod CLS provides the primary local text status;
- buttons and switches are numbered consistently in source, enclosure,
  terminal, virtual harness, and reports;
- pressed/released and on/off states have textual forms;
- every scenario can be executed and asserted without sight of the hardware;
- safe/fault states include stable text and numeric codes;
- indicator animation cannot be the sole pass/fail evidence.

## Board Profile And Trusted Primitives

One versioned TOML board profile owns:

- exact module and extension-board revision;
- FPGA part/package/speed grade;
- oscillator and reset facts;
- connector-to-FPGA pin map;
- I/O standards, drive, slew, pull, and synchronization;
- iCELink shared-line/JTAG selection;
- UART pin direction and baud;
- SDRAM and SPI-flash parts/timing;
- all queue, memory, fuel, and app-size limits;
- clock targets;
- trusted primitive mappings;
- tool versions and constraints;
- electrical/power evidence identifiers.

The profile digest is separate from `CoreHardwareIR`. The shell and
`TargetHardwareIR` bind it.

Trusted target-specific primitives are limited to clock/reset generation,
ECP5 memory cells, PLLs where used, I/O buffers, synchronizers, SDRAM/flash
electrical adapters, and iCELink/UART pin adapters. Every primitive has a
behavioral model, target contract, synthesis mapping, and focused equivalence
or protocol test. Vendor availability never permits application or CPU logic
inside an opaque primitive.

## Readiness Contract

BoonConsole is ready for downstream use only when one unchanged revision
passes:

1. final language, type, verified-artifact, and target-eligibility gates;
2. generic `CoreHardwareIR`/`TargetHardwareIR`, cycle, RTL, synthesis, and
   formal fixtures;
3. complete RV32I architectural, differential, formal, RTL, and board proof;
4. all-peripherals electrical and protocol bring-up;
5. standalone Wasm emitter and hostile validator;
6. PC reference and simulated-SoC app equivalence;
7. exact-byte physical interpreter execution;
8. volatile upload/recovery;
9. persistent install, reset, corruption, and rollback behavior;
10. bounded bridge and negative protocol tests;
11. virtual/physical scenario trace and final-digest equivalence;
12. safe-state and accessibility checks;
13. manifest-backed fresh hardware-in-the-loop aggregate.

Game implementation is outside this architecture and the current goal. A
separately specified future game goal may consume the real console, CPU, Wasm,
and proof artifacts after this gate passes; it cannot redefine or waive them.

## Authority And Historical Documents

- This file owns console semantics, physical scope, interpreter-first parity,
  bridge behavior, reset/persistence meaning, safe state, and readiness.
- The implementation plan owns phases, experiments, planned files, reports,
  budgets, and rethink triggers.
- The RV32I plan owns the reusable processor and generic hardware contract.
- [`FPGA_TODOMVC_LOWERING.md`](./FPGA_TODOMVC_LOWERING.md) is historical and
  must be deleted after its useful generic constraints have executable
  replacements.
- [`../game/MISSION_WASM_AND_LANTERN_CONSOLES.md`](../game/MISSION_WASM_AND_LANTERN_CONSOLES.md)
  is a superseded game-direction checkpoint.
- [`../game/BOON_ORCHARD.md`](../game/BOON_ORCHARD.md) owns fiction and game
  vision only.
- The imported root `BOON_CONSOLE_REPOSITORY_UPDATE_GOAL.md` is first-invocation
  input, not a permanent competing authority.
