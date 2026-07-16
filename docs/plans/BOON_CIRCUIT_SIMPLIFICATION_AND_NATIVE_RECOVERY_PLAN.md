# Boon Circuit Simplification And Native Recovery

Status: active implementation contract for the destructive cleanup.

## Objective

Reduce the current roughly 384,000 tracked Rust lines to at most 240,000 while
restoring a responsive native playground. The final repository has one
execution engine, one typed document/render update path, one native input path,
compact verification tooling, and no executable 3D/manufacturing island.

The checkpoint at `6935352` is intentionally not a completed native-input fix:
the automated Counter TEST route passed while physical COSMIC dev-window input
remained unresponsive.

## Current Implementation State

Mandatory slices 1 through 5 are implemented. At commit `9b4ed71`, the fresh
architecture report passes every check with 183,763 tracked Rust lines, 31,703
test Rust lines, 31,361 playground production lines, 5,117 xtask production
lines, and 20,544 runtime-plus-executor production lines. The counter now
partitions trailing inline test modules from production instead of hiding them
or double-counting them. Duplicate private playground behavior oracles were
deleted, reducing its focused suite from 118 tests taking roughly 34 seconds to
62 tests taking roughly 1.5 seconds. `app_window` is published at
`BoonLang/app_window`, pinned to immutable revision
`6aec9831f281df355736df28a4c3aacdef7cf8a1`, and measures 1,192 net code lines
over v0.3.3 against the 1,200-line cap.

The recovered execution path now preserves generic scoped row sources from the
typed compiler plan through `Session`, document bindings, retained hit targets,
and scenario dispatch. Source-event transforms evaluate against the event row,
helper parameter names no longer define source ownership, and unscoped visual
target text is not mistaken for a row target. Document evaluation treats equal
text and enum values semantically, preserves hidden/semantic labels, and checks
that patch-applied retained frames equal the authoritative runtime frame after
every test dispatch. NovyWave now uses canonical row-owned scope and signal
events instead of parallel scenario-only controls.

Fresh semantic runs pass through `MachinePlan`, `Session`, and typed document
patches. `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`,
and `cargo test --workspace --all-targets --quiet` pass. The architecture and
negative manifest reports pass from current HEAD; product-native reports await
the compositor restart described below.

The nested-compositor diagnostic path has been deleted. The replacement uses
ordinary COSMIC preview/dev windows, kernel uinput mouse and keyboard devices,
the normal app_window callback route, and app-owned exact-frame WGPU readback.
The compositor fork now has a generic launch-scoped reconciliation operation so
all descendant windows of one background launch are gathered and tiled without
fixture, role, title, app-ID, or geometry matching. The matching compositor and
launcher release binaries are installed, but the running compositor predates
that operation. No refreshed native report is accepted until the COSMIC session
loads the installed binary and the real Counter, Cells, wheel, keyboard, TEST,
proof, and aggregate gates pass.

The remaining completion work is explicit:

1. restart the COSMIC session so the installed compositor exposes launch-scoped
   window reconciliation, then refresh all six manifest reports and the
   aggregate;
2. launch the release playground with demand pacing and obtain the required
   physical human confirmation.

## Non-Negotiable Rules

- Delete obsolete code. Do not rename, quarantine, alias, or preserve it behind
  compatibility switches.
- Make changes in large ownership slices. A slice may be temporarily broken in
  the worktree, but every slice commit must compile.
- Run targeted checks only at slice boundaries. Regenerate expensive native
  reports only after the architecture has stabilized.
- Keep Cells and all runtime/compiler/renderer behavior generic.
- Keep readback out of normal frames. Explicit proof requests use asynchronous
  app-owned WGPU readback tied to exact frame identity.
- Add no Python and no Boon-specific behavior to windowing, runtime, compiler,
  document, renderer, or verifier infrastructure.

## Required Architecture

### Execution And Documents

- `MachinePlan` is the only executable artifact. Its format may break; no old
  decoder remains.
- `boon_plan_executor::Session` exclusively owns values, lists, indexes, source
  routing, currentness, formula dependencies, cycles, dirty keys, and deltas.
- `boon_runtime` is a thin compile/cache/scenario facade returning typed
  `RuntimeTurn` values.
- `boon_document` alone turns typed `DocumentPatch` values into retained layout
  and render changes. The playground does not interpret parser AST or rebuild
  bindings from JSON.
- Product crates do not depend on `serde_json`. JSON is limited to final CLI and
  verifier report serialization.

### Native Windowing And Frames

- Generic window-event improvements live in `BoonLang/app_window`, not in a
  copied workspace dependency. Boon Circuit pins an immutable fork revision.
- `Surface::take_events()` returns one ordered asynchronous receiver covering
  pointer, button, wheel, physical/logical key, text/IME, focus, resize, scale,
  close, and accessibility actions.
- The event queue uses one `AtomicWaker`, coalesces only adjacent motion/wheel
  events, preserves discrete order, and reports overflow as fatal. It contains
  no Boon names, event histories, public test injection, polling timer, or
  second platform dispatcher.
- Desktop only supervises preview and dev. Preview and dev use the same native
  role runner and the same typed event-to-frame transaction.
- Every transaction drains input, applies runtime changes, patches retained
  document/render state, submits/presents if dirty, then schedules optional
  proof work. Proof never blocks product presentation.
- Hidden COSMIC workspaces use explicit demand pacing: requested callbacks are
  coalesced to output refresh cadence and stop when clients stop requesting
  frames. Standard inactive-workspace throttling is not valid performance
  evidence.
- Source replacement uses one typed depth-one latest-wins mailbox. Product IPC
  is binary and contains no JSON.

### Verification

- Delete `boon_report_schema`; report-v2 types and validation are tooling-only.
- Reduce xtask to at most eight public commands and a six-gate native manifest:
  architecture, Counter/dev, TodoMVC/physical, Cells, NovyWave, and negative.
- Every proof names its frame, input, content, layout, render, surface epoch, and
  present revisions. PNG proof is an asynchronous sidecar.
- A private Wayland server drives the actual app-window callback path. TEST is
  clicked through that path; its scenario input enters at the public HostEvent
  boundary and displays a retained cursor overlay.
- Public behavior tests are integration tests. Private unit tests remain only
  for genuine algorithms; wrapper-parity, report-field, duplicate-oracle, and
  private implementation tests are deleted.

## Mandatory Slices

1. Delete the four 3D/manufacturing crates, examples, fixtures, runtime outputs,
   native branches, commands, schemas, tests, and plans.
2. Break the plan format and migrate all execution to PlanExecutor; delete the
   duplicate runtime representation and execution oracles.
3. Move document bindings into typed plan/runtime output and delete playground
   AST/JSON lowering and state-summary synchronization.
4. Create the external app_window fork event API, delete `vendor/app_window`,
   and rewrite native host/playground roles around it.
5. Replace report/verifier v1 and duplicated tests with compact report v2 and
   the manifest-owned gate inventory.
6. Run final structural, semantic, native, visual, performance, idle, and manual
   verification once.

## Completion Gates

- Tracked Rust lines: at most 240,000; test Rust: at most 32,000.
- Playground: at most 32,000; xtask: at most 25,000; runtime plus executor: at
  most 42,000; app_window fork additions: at most 1,200 net lines.
- No vendored app_window, report-schema crate, executable 3D/manufacturing code,
  duplicate executable artifact, product JSON path, input resampling/history,
  verifier test injection, or compatibility fallback remains.
- Counter, TodoMVC, physical TodoMVC, Cells, and NovyWave scenarios pass through
  the same runtime and document APIs.
- Window callback to HostEvent p99 is at most 1 ms. Warm visible interaction and
  scroll p95 are at most 16.7 ms and max at most 33.4 ms. Warm example-switch
  acknowledgement p95 is at most 16.7 ms; final preview p95 is at most 250 ms
  and max at most 500 ms.
- Settled release preview plus dev CPU is below 1% of one core with zero
  unsolicited frames.
- Formatting, workspace check/test, scenarios, all fresh manifest gates, report
  validation, and the aggregate pass.
- The release playground is launched in the COSMIC background workspace. The
  goal is complete only after the automated gates pass and the user confirms
  physical dev hover/click/wheel/keyboard, TEST, Counter, and Cells behavior.

Rust/Zig code generation is deliberately after this recovery. It must not grow
the repository before these gates pass.
