# Mission Wasm And Lantern Consoles

Status: game and product-direction checkpoint, not an implementation contract.

This document is a companion to
[Boon Orchard](./BOON_ORCHARD.md). It records the portable-program,
optional-hardware, console, island, and local-cluster direction that emerged
after the original game vision.

The central player promise is:

> A player can program the meaning of one to four buttons in Boon, package that
> behavior as `mission.wasm`, run the complete experience on a local PC, and,
> optionally, plant the same mission in a Boon-designed RISC-V processor on an
> FPGA so physical controls can affect or communicate with islands, machine
> lanterns, and later real servers.

The board makes the machine physical. It must not make the game purchasable
only in pieces.

## Decisions Captured Here

The following direction is now intentional:

- the first processor heart is the Boon-designed RV32I processor;
- `mission.wasm` is the canonical portable application artifact;
- Boon is the primary in-game language for authoring missions;
- another language may be used outside the game if it produces a module that
  passes the same mission profile and capability checks;
- the mature physical target loads and executes the original `mission.wasm`
  through a runtime on the Boon-designed RV32I;
- the first physical milestone may instead host-AOT-compile that exact mission
  into RV32I firmware while the onboard runtime is still too large;
- the behavior of the buttons belongs to the mission, not to hard-coded game or
  FPGA logic;
- one button is enough for the first complete interaction, and up to four
  buttons form the initial useful console;
- switches, LEDs, the two-digit display, the character display, sensors, or
  network interfaces are optional capabilities added only when a mission needs
  them;
- the game, its islands, and a server-like lantern cluster can run entirely on
  one local PC;
- every physical console and FPGA function required by the campaign has a
  semantically equivalent virtual implementation;
- no FPGA, Pmod, Raspberry Pi, VPS, paid hosting account, or public server is
  required to finish the campaign;
- physical boards and real deployments extend the finished game into the real
  world rather than unlocking otherwise unavailable progress.

This chooses an execution and interaction substrate. It does not yet choose one
final useful application for every lantern.

## Terms

| Term | Meaning |
| --- | --- |
| `mission.wasm` | Canonical, hash-identified portable mission behavior |
| mission source | Boon source in the game, or compatible source compiled by an external tool |
| mission profile | Bounded Wasm subset and capability contract accepted by the game and board toolchain |
| `ConsolePort` | Logical buttons, switches, indicators, and optional display interface |
| `DatagramPort` | Bounded, declared island-to-island or lantern-to-gateway message interface |
| `VirtualWasm` | Direct execution of `mission.wasm` on the local PC or in the browser |
| Virtual Heart | RV32I firmware executing on the simulated Boon processor |
| `RV32Board` | RV32I firmware or a Wasm runtime executing on the physical Boon processor |
| Orchard Agent | Local gateway for USB, authenticated remote communication, deployment, and diagnostics |
| machine lantern | One island/server site containing a processor, mission, state, ports, and evidence |

The fiction may eventually call a portable mission a **machine seed** or
**mission seed**. The technical artifact should remain unambiguous:
`mission.wasm`.

## One Mission, Several Honest Execution Modes

```text
Boon source                     another supported language
    \                                      /
     -> compile, validate, and profile-check
                         |
                    mission.wasm
                    exact digest
                         |
       +-----------------+-------------------+
       |                 |                   |
       v                 v                   v
  VirtualWasm       host-side AOT       later board runtime
  direct on PC      to RV32I firmware   or interpreter
       |                 |                   |
  virtual lantern   +----+---------+    original bytecode
  and console       |              |    executes on RV32I
                    v              v
               Virtual Heart    RV32Board
               in simulator     on FPGA
```

These modes provide different embodiments of the same mission:

### Direct Local Preview

The local game or browser executes the exact `mission.wasm` bytes directly.
Virtual buttons, switches, LEDs, and displays implement the same `ConsolePort`
as physical modules.

This is the fastest authoring and debugging mode. It is also the complete
fallback for a player who owns no FPGA.

### Virtual Heart

The host compiles the same `mission.wasm` into RV32I firmware and runs it on the
simulated Boon processor.

This mode keeps the processor the player built relevant even when no board is
present. Direct Wasm preview may bypass the processor for speed; Virtual Heart
proves that the mission works through it.

This path is a required new implementation milestone, not a capability implied
by finishing the RV32I core. It needs a mission-profile validator, guest ABI,
Wasm-to-RV32I AOT path, support runtime, base-RV32I instruction audit, and
direct-Wasm-versus-RV32I equivalence fixtures.

### First Physical Heart

The first practical FPGA route may be:

```text
mission.wasm
  -> host validation and AOT compilation
  -> RV32I firmware
  -> Boon-designed RV32I on FPGA
```

The FPGA executes application decisions as RV32I instructions derived from the
canonical mission. Deployment records preserve both the exact Wasm digest and
the derived firmware digest.

In the first fixed-image SoC, that firmware may be embedded in the board image,
so changing the mission requires regeneration and reprogramming. A loader comes
later.

This is not an onboard Wasm interpreter. The game must describe it as
**RV32I firmware compiled from `mission.wasm`**, not as original Wasm bytecode
running on the board.

### Mature Physical Cartridge

The preferred later cartridge experience is:

```text
mission.wasm
  -> board loader
  -> Wasm runtime or interpreter executing on RV32I
```

This makes the exact original module loadable without regenerating RV32I
firmware. It is a later milestone, gated by measured code size, data memory,
cycle cost, persistent module loading, and possibly external SDRAM support.

The mature goal must not force an interpreter into the first processor proof
milestone before it fits.

### Truth Labels

The game and reports must distinguish at least:

- `direct-host-wasm`;
- `simulated-rv32i-aot`;
- `fpga-rv32i-aot`;
- `fpga-rv32i-original-wasm`;
- `tethered-physical-console`;
- `remote-server-lantern`.

Parity means the same mission digest, ABI, ordered logical events, state schema,
outputs, traps, and golden traces. It does not mean identical wall-clock
performance or identical implementation.

## Boon And Other Mission Languages

Boon should be the language taught by the campaign:

```text
Boon mission source
  -> checked bounded mission
  -> mission.wasm
```

The compiler path needed to emit a compact standalone mission module is a real
piece of future work. The existing use of Wasm as a host for Boon simulation
does not by itself provide this application artifact.

External tools may also produce a mission:

```text
Rust, C, Zig, AssemblyScript, or another language
  -> mission.wasm
  -> the same validator, ABI, bounds, and capability rules
```

The game should not privilege externally authored modules with hidden
capabilities. A module is accepted because it satisfies the mission contract,
not because of its source language.

This requires no self-hosting. Compilation, validation, AOT translation, and
bitstream generation may all remain on the PC.

The public Boon language still uses bounded `MAP` rather than gaining a
`MEMORY` keyword for this feature. Eligible bounded state can lower to Wasm
globals, fixed static storage, or the RV32I memory representation chosen by the
compiler.

## The Initial Mission Profile

The first profile should be deliberately small enough to validate, simulate,
compile, and fit:

- deterministic, bounded event execution;
- integer-first behavior, initially centered on `i32`;
- fixed imports and exports;
- no linear memory in the first physical button mission;
- an explicit measured target-memory budget before admitting later modules with
  linear memory, whose first standard Wasm page is already 64 KiB;
- no filesystem, unrestricted sockets, ambient clock, or arbitrary host calls;
- no WASI dependency in the first board profile;
- no threads, atomics, SIMD, garbage collection, exceptions, or dynamic code;
- no unbounded recursion, queue growth, or hidden allocation;
- explicit traps and deterministic handling of unsupported capabilities;
- rejection of unsupported Wasm before execution or deployment.

The profile may grow when a real mission needs more. It should not grow merely
to advertise general Wasm compatibility.

## The Minimal Programmable Console

The hardware shell supplies clean logical I/O. The mission supplies meaning.

### Required First Input

One button is sufficient for the first end-to-end proof:

```text
button edge
  -> synchronized ConsolePort event
  -> mission.wasm
  -> mission state transition
  -> visible local or world effect
```

The first useful console expands this to one through four buttons.

Every physical FPGA shell must:

- synchronize physical input levels;
- capture button events so a polling processor does not miss a short press;
- report capability presence;
- timestamp or sequence events with a deterministic logical counter.

Event capture must use either a bounded FIFO or bounded per-button counters.
Its acknowledgement, clearing, coalescing, and overflow behavior is part of the
profile and must match the virtual adapter. A single edge bit that silently
collapses repeated presses is not sufficient for trace parity.

It must not decide what a button means. `BUTTON_1` may be `CHECK` in one
mission, `SEND_BEACON` in another, and `ACCEPT_TRANSFER` in a third because the
loaded `mission.wasm` defines that policy.

### Optional Persistent Inputs

Up to four slide switches can expose persistent configuration or permission
bits. A button event should snapshot their state into one logical input event.

A switch changing should not silently become an unrestricted remote command.

### Optional Outputs

The modules already owned are a useful initial capability set:

- eight LEDs for state, progress, acknowledgement, or communication;
- a two-digit seven-segment display for an island ID, epoch, queue depth, or
  fault code;
- a character display for button labels and bounded status text.

The FPGA shell owns electrical timing such as display multiplexing and serial
transmission. The mission writes logical frames; it does not bit-bang a display
from Wasm.

Additional sensors, audio, storage, Wi-Fi, or Ethernet remain optional future
capabilities. A mission that requires one must:

- declare it;
- provide deterministic behavior when it is missing;
- have a virtual adapter where the campaign requires that mission;
- not make an unowned peripheral a hidden completion requirement.

## Console Port

A possible logical shape is:

```text
ConsoleInput
  button_levels      BITS[4]
  button_events      bounded ordered events
  button_overflow    BOOL
  switch_levels      BITS[4]
  logical_tick       U32
  capability_bits    U32
  input_sequence     U32

ConsoleOutput
  led_mask           BITS[8]
  left_glyph         U8
  right_glyph        U8
  glyph_blank_mask   BITS[2]
  status_code        U32
  optional_text      bounded bytes
```

This is illustrative rather than a frozen ABI. The invariant is that virtual
and physical adapters produce the same logical events and consume the same
logical output frames.

## Islands And Executable Lantern Instances

An island or server-like site is the persistent world placement. A machine
lantern is the computational inhabitant planted there. The island persists
while its lantern is held, upgraded, migrated, replaced, or absent.

The lantern's executable identity can be modeled as:

```text
LanternInstance
  island_or_site_identity
  mission_wasm_digest
  mission_profile_version
  state_schema_version
  bounded_versioned_state
  last_processed_event
  ConsolePort
  DatagramPort
  execution_backend
  deployment_evidence
```

Possible execution backends include:

- `VirtualWasm` in the local game;
- derived RV32I firmware in the Virtual Heart;
- derived RV32I firmware on a physical FPGA;
- original Wasm bytecode in a later board runtime;
- an authorized local process, Raspberry Pi, or remote server.

The lantern identity is its mission, state, capabilities, history, and bound
site—not the machine currently executing it.

## Planting A Physical Lantern

The first fixed-image FPGA milestone may embed one AOT mission and its initial
state in firmware or the bitstream. Changing that mission may require
regenerating and reprogramming the image.

Live planting and unplanting are a later capability. They require:

- a board loader or monitor;
- a bounded snapshot/restore ABI;
- a backend-neutral, versioned canonical state encoding;
- quiescence and single-owner transfer rules;
- evidence that the destination restored the same logical state.

A mission whose state is not declared checkpointable cannot migrate live.
Wasm globals, host objects, and RV32I memory layouts are not inherently
portable snapshots.

Once that machinery exists, planting a lantern on a board becomes a real state
migration:

```text
pause the virtual lantern at a deterministic event boundary
  -> verify mission and state-schema digests
  -> transfer bounded state and last event sequence
  -> start the physical owner
  -> wait for acknowledgement
  -> mark the lantern execution backend physical
```

The virtual and physical copies must never both own the same lantern state.
Unplanting performs the reverse operation at another safe boundary.

This gives the FPGA a stronger role than acting as a decorative keyboard. The
board can actually execute one lantern's decisions, retain its local state,
and drive its physical console while the island remains the world placement.

## Controlling And Communicating With The Game

The same mission can use button events in several ways:

- change the local state of the lantern running it;
- send a declared message to another island's lantern;
- accept or reject a proposed state transfer;
- request a check, checkpoint, hold, resume, retry, or route change;
- acknowledge an event;
- control a bounded game action exposed as a capability;
- operate a later real service through an authorized gateway.

A press should create a typed, sequenced intent rather than invoke arbitrary
host code:

```text
button press
  -> mission decision
  -> bounded intent
  -> queued or transmitted
  -> accepted, rejected, stale, or expired
  -> applied receipt
  -> LED/display/world response
```

The world should show the difference between:

- the button was pressed;
- the intent was created;
- the intent was sent;
- the remote lantern accepted it;
- the operation was applied;
- the outcome is unknown because communication was lost.

An LED that means success should represent an actual receipt, not merely local
optimism.

## A Lantern Steward Mission

One built-in `mission.wasm` can turn the same virtual or physical console into
an orchard controller.

It may:

- select a lantern or a small group;
- request status;
- stage a declared operation;
- hold or resume a machine at a safe boundary;
- confirm an already prepared operation;
- acknowledge an incident;
- send a small beacon or presence signal to another player's lantern.

The console may later control real local or hosted services, but only through
narrow declared capabilities. It is not a general remote shell.

Custom missions remain free to assign completely different meanings to the
one-to-four buttons.

## Local-First Orchard

The complete game should run without paid infrastructure:

```text
one local PC
  game world
  VirtualWasm mission instances
  virtual RV32I/FPGA simulation
  virtual consoles and peripherals
  local Orchard Agent
  local island/server cluster
  simulated links, delay, loss, and partitions
```

Several islands may be separate logical servers while still sharing one local
machine. The game can simulate topology, limited bandwidth, latency, dropped
messages, restarts, and partitions without pretending that a paid VPS exists.

The local cluster is not an inferior demo mode. It is the normal complete game.

Optional extensions include:

- moving one lantern's execution to the physical FPGA;
- moving one or more lanterns to local Raspberry Pis;
- authorizing a real service on the player's PC;
- deploying a lantern to hosting such as a VPS;
- connecting another player's local or physical lantern.

No purchase produces exclusive mission logic, rewards, or progress.

## Virtual And Physical Equivalence

Without hardware:

```text
virtual BTN/SWT
  -> mission.wasm on PC or Virtual Heart
  -> virtual LED/SSD/CLS
  -> local game islands and cluster
```

With a deployed physical node:

```text
physical BTN/SWT
  -> FPGA input shell
  -> mission on Boon RV32I
  -> physical LED/SSD/CLS
  -> USB world link
  -> local game islands and cluster
```

Both paths use the same mission contract and world-message semantics.

Physical provenance may be displayed as evidence, but it grants no additional
authority by itself. A virtual input cannot claim to be physical, and a
physical input is not automatically trusted to control a real server.

## Current Board Network Boundary

With the current iCESugar Pro and owned Pmods, USB is the available connection
to the local PC. The honest architecture is:

```text
deployed FPGA node
  -> USB
  -> local Orchard Agent
  -> authenticated local or remote transport
  -> another lantern or server
```

The Orchard Agent owns network credentials, TLS, durable audit history, and
remote API calls. In deployed-node mode, the board owns its local mission,
console state, bounded messages, and physical I/O. In tethered-console mode,
the host owns the mission.

Two modes must remain visibly distinct:

### Tethered Physical Console

The mission runs on the PC. USB carries physical button events and display
frames.

This is useful for bring-up and debugging, but it does not prove that the
Boon-designed RV32I executes the mission.

### Deployed Physical Node

The mission decisions run on RV32I. Buttons and displays are local FPGA
peripherals. USB carries diagnostics and world messages. After the later
loader and snapshot ABI exist, it may also carry deployment and snapshots; the
first fixed image is regenerated and reprogrammed instead.

If the USB data link is lost while the board remains independently powered, the
deployed mission may continue local computation and console interaction. It is
nevertheless partitioned from the orchard network. A literal unplug may also
remove power, in which case continued execution is impossible. Bounded outgoing
messages must queue, expire, or report overflow under a declared policy.

The current board is not an independently networked Internet server. A later
Wi-Fi or Ethernet shell can bind the same `DatagramPort` without changing the
mission contract.

## Campaign Shape

This direction supports a clear progression:

1. **Forge the Heart** — build and verify the RV32I processor in Boon.
2. **Give the Heart Hands** — connect one button, then up to four, and produce
   the first visible output.
3. **Write the First Mission** — write button behavior in Boon and compile it
   to `mission.wasm`.
4. **Awaken the First Island** — run the mission locally with a virtual
   console.
5. **Run on the Heart** — execute the derived firmware on the simulated RV32I.
6. **Speak Across the Dark** — send a message between two local islands and
   distinguish send from acknowledgement.
7. **Plant the Machine** — optionally move one lantern's execution onto a
   physical FPGA.
8. **Weather the Silence** — survive delayed, lost, or partitioned
   communication without duplicating effects.
9. **Light the Constellation** — coordinate a local or mixed cluster of
   virtual, physical, and optionally real lanterns.
10. **Cut the Tether** — when the chosen mission, persistent image, and
    independent power support it, prove that local physical behavior continues
    without a PC data connection.

A player without hardware completes the same conceptual arc through
VirtualWasm and the Virtual Heart. Physical hardware turns those abstractions
into a real object but does not replace the campaign.

## Real-World Continuation

After the authored campaign, the game can remain useful as a programmable
console and observable lantern network.

Possible declared capabilities include:

- run a health check;
- start a predefined local job;
- acknowledge an alert;
- enter or leave a safe maintenance state;
- stage, confirm, or roll back an update;
- send a bounded presence or status signal;
- control a player-authored local automation;
- inspect a local, Raspberry Pi, or hosted machine lantern.

These examples demonstrate the console shape without locking Boon Orchard to
one application.

The interesting continuity is:

> The same button behavior first tested against a virtual island can later run
> on the processor the player built and communicate with a real machine.

## Safety And Truth Rules

- A mission declares every game, host, device, and network capability it uses.
- No button may invoke an arbitrary shell command.
- Dangerous external actions require host authorization and explicit target
  identity independently of virtual or physical input.
- The development FPGA is not presented as a high-assurance security token.
- Retries use event identity and explicit idempotence rules.
- Stale operations are rejected rather than replayed blindly.
- Missing optional hardware is explicit and deterministic.
- A physical LED changing because the host sent bytes proves a physical
  console, not mission execution on RV32I.
- A physical lantern-execution claim requires RV32I to consume mission events,
  update canonical state, and produce outputs.
- A standalone claim requires the programmed and independently powered board to
  boot and execute the local mission without a PC data connection.
- Networked operation may still honestly require a USB gateway.
- Every local-only player can complete the game without buying hardware or
  renting a server.

## Verification Direction

Eventually, one mission fixture should run through:

```text
same mission.wasm digest
same initial state
same ordered logical input events
  -> direct host Wasm
  -> derived firmware on simulated RV32I
  -> derived firmware on FPGA RV32I
  -> later original Wasm on the board runtime
same ordered logical outputs and final state digest
```

A two-lantern communication fixture should additionally preserve:

```text
two endpoint and mission identities
declared DatagramPort schemas
one deterministic send/delivery schedule
message and acknowledgement identities
accepted and applied receipts
duplicate-suppression decisions
partition and reconnection epochs
final state and message-queue digests at both endpoints
```

The evidence should include:

- mission profile and ABI version;
- canonical Wasm digest;
- state-schema version and canonical snapshot encoding where migration is
  supported;
- AOT toolchain identity where used;
- derived RV32I firmware digest;
- a base-RV32I instruction audit proving that no unintended ISA extension,
  CSR, or hardware multiply/divide dependency entered the image;
- preservation of required Wasm integer and trap semantics in software
  lowerings;
- measured code, memory, and cycle budgets;
- unsupported-profile negative cases;
- logical trace equivalence;
- communication schedules, message identities, receipts, duplicate handling,
  overflow behavior, and post-partition reconciliation evidence;
- explicit physical or virtual provenance.

## Still Open

This checkpoint does not decide:

- the final useful mission or application;
- the final fictional name for `mission.wasm`;
- the exact Wasm ABI;
- whether the first compact board runtime is custom or based on an existing
  implementation;
- when external SDRAM or persistent module loading enters the processor plan;
- the first network peripheral;
- the persistence and reconciliation policy for richer island state;
- the final authorization model for real deployments;
- how many remote or multiplayer services the released game operates itself.

Those decisions should follow working local missions, measured RV32I artifacts,
and the first virtual/physical console prototype.
