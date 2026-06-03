# Game-Like Boon Authoring: Idea 1

Status: concept note, not an implementation contract.

This document captures the idea discussed for writing Boon programs by playing a
game. The working names are `Boon Foundry`, `Circuitorio`, or `Boon Circuit
Studio`.

The important point is that this is not just a node editor with cute graphics.
The game should be a playable projection of a real Boon program: typed source,
static dependency graph, runtime state, live values, deterministic traces, and
debuggable migrations.

## Core Idea

Build a game-like IDE where the player constructs Boon code by placing,
connecting, inspecting, and debugging machines.

The player sees a factory, circuit board, city, or nested machine world. Under
that world, the editor maintains:

```text
Boon source
typed design IR
static dependency graph
visual layout metadata
runtime/debug metadata
```

Every meaningful game action must map to real Boon structure:

```text
place source input       -> SOURCE
place memory tank        -> HOLD
wire event pulse         -> THEN / WHEN event path
wire continuous value    -> pure dependency
merge candidates         -> LATEST / PRIORITY / EXCLUSIVE
create row factory       -> LIST |> List/map(...)
rename stateful value    -> DRAIN + DRAINING migration
fail fast                -> FLUSH
publish component        -> FUNCTION / module
```

The best mental model is:

```text
Factorio spatial causality
+ Zachtronics step/replay/debug loops
+ spreadsheet dependency recalculation
+ LabVIEW-style probes
+ Boon's static circuit runtime
```

## Non-Negotiable Boon Semantics

The game can look freeform, but the compiler must stay strict.

- The graph is static. Runtime list rows are hidden keys into memories, not
  graph nodes created at runtime.
- Dynamic list items must appear as selected row instances or summaries, not as
  thousands of permanent visual nodes.
- `SOURCE` is a named input port, not a vague global event.
- `THEN` is presence-gated. No event means `SKIP`, not `False`, not empty text,
  and not an empty list.
- `HOLD` is the only ordinary state cell. It stores the last committed value and
  updates at commit time.
- `LATEST` resolves multiple candidate writes by event sequence. Same-sequence
  ambiguity is an error unless an explicit policy exists.
- `WHEN` and `WHILE` must stay distinct: `WHEN` copies on selected event/value;
  `WHILE` is continuous selection.
- `FLUSH` is fail-fast flow control, not state migration.
- `DRAIN` / `DRAINING` is explicit state migration, not normal dependency flow.
- Hidden runtime keys, list generations, scope paths, render ids, and source
  binding ids are debug metadata. Boon code must not read or compare them.
- Layout metadata must never affect runtime behavior.

## Gameplay Objects

### SOURCE

Visual metaphor: input socket, sensor, button, keyboard receiver, event station.

`SOURCE` declares where host input can enter the Boon graph.

Display:

```text
increment_button.events.press
type: Event<Press>
last event: seq 184
bound UI: increment button
```

Gameplay:

- click a rendered UI button to reveal its source port;
- drag the source pulse into a `THEN` gate;
- show stale or unbound sources as diagnostics;
- in list rows, show the hidden row binding only in debug mode.

### THEN

Visual metaphor: pulse gate.

`THEN` runs the body only when the input is present in the current tick.

Display:

```text
press |> THEN { count + 1 }
input: present / SKIP
output: candidate value or SKIP
```

Gameplay:

- absent event makes the gate transparent/idle;
- present event sends a glowing pulse;
- the inspector must make it clear that absence is not a boolean value.

### HOLD

Visual metaphor: memory tank, register, storage drum.

`HOLD` stores the previous committed value and accepts candidate next values.

Display:

```text
count HOLD
current: 42
previous visible name: count
pending candidate: 43
last writer: increment_button.events.press
dirty this tick: yes
```

Gameplay:

- state tanks are the main things players care about preserving;
- feedback loops are legal only when they pass through `HOLD`;
- pure cycles without `HOLD` should be visibly rejected.

### LATEST

Visual metaphor: merge station with event sequence arbitration.

`LATEST` merges candidate values, ignoring `SKIP`.

Display:

```text
LATEST
branches:
  increment press -> 43 seq 184
  reset press     -> SKIP
winner: increment press
```

Gameplay:

- multiple active candidates animate into the merge;
- equal-sequence conflicts become a visible red error;
- a future `PRIORITY` or `EXCLUSIVE` policy can be represented as a different
  merge-machine mode.

### WHEN And WHILE

Visual metaphors:

- `WHEN`: snapshot switch, event decoder, paper stamp.
- `WHILE`: continuous selector, live switch, multiplexer.

Display the key distinction:

```text
WHEN: copies value when selected
WHILE: continues following dependencies while selected
```

Gameplay:

- use `WHEN` for form submit, message send, commit-on-enter;
- use `WHILE` for live language/theme/filter changes;
- wrong choice should produce good scenario failures and trace explanations.

### LIST And Row Templates

Visual metaphor: row factory, template machine, production line blueprint.

`LIST |> List/map(...)` creates a static row-template operator evaluated over
hidden active keys. The semantic graph does not clone per row.

Display:

```text
todos LIST
rows: 1,248
changed this tick: 3
inserted: 1
removed: 0
template: new_todo(todo, store)
```

Instance lens:

```text
row: /todos:42
title = "Buy groceries"
completed = False
editing = False
last source = todo_checkbox.events.click
dirty fields = completed, visible_todos, active_count
```

The hidden key and generation should be available in debug mode, but ordinary
source and ordinary visual authoring must not depend on them.

### DRAIN And DRAINING

Visual metaphor: migration pipe between state tanks.

The explicit future migration design uses paired markers:

```boon
counter: 0 |> HOLD state { ... } |> DRAINING
click_count: DRAIN { counter } |> HOLD state { ... }
```

Interpretation:

- `DRAINING` marks the old state as being migrated away.
- `DRAIN { counter }` transfers state into the destination.
- Storage identity becomes the destination, so after finalization the state
  stays under `click_count`.
- The old source stops accepting upstream triggers.
- Pending work is flushed before transfer, which matters for cross-domain moves.

Visual state:

```text
counter
  status: DRAINING
  normal inputs: disabled
  migration output: click_count

click_count
  status: RECEIVING_DRAIN / ACTIVE
  init: DRAIN { counter }
  value: copied from counter
```

Editor actions:

```text
Rename state with preservation
Split state into fields
Move state to module
Convert state type with transform
Finalize migration
```

Compile-time rules the game should enforce visually:

- every `DRAINING` source must have exactly one `DRAIN`;
- every `DRAIN` source must be marked `DRAINING`;
- no double drain;
- no conditional drain;
- no drain cycles;
- no ordinary references to a `DRAINING` value;
- remove both markers only after the drain is complete.

Field-level drain:

```boon
app_state: [count: 0, mode: Dark] |> HOLD state { ... } |> DRAINING

count: DRAIN { app_state.count } |> HOLD state { ... }
mode: DRAIN { app_state.mode } |> HOLD state { ... }
```

Lists generally should not use `DRAIN` for per-row identity. They have hidden
runtime item keys and generations.

Status note:

- Current `~/repos/boon` documentation still includes an older migration story
  where old state is wired into new state with `LATEST`.
- `~/repos/boon_experiments/docs/new_boon/3.6_STATE_EVOLUTION.md` documents the
  stricter `DRAIN + DRAINING` design.
- `~/repos/boon-zig` reserves `DRAIN` as a spec-gap keyword.
- For this game concept, `DRAIN + DRAINING` is the preferred visual model for
  stateful refactors.

### FLUSH

Visual metaphor: emergency bypass rail, reject chute, circuit breaker.

`FLUSH { error_value }` creates a hidden `FLUSHED[value]` wrapper. Downstream
function calls detect flushed arguments, skip ordinary work, and propagate the
flushed value until a boundary unwraps it.

Example:

```boon
validated_title:
    title |> WHEN {
        Empty => FLUSH { EmptyTitleError }
        value => value
    }
```

Visual display:

```text
parse input -> validate -> transform -> save -> render
                  |
                  v
              FLUSH { error }
                  |
                  v
        red bypass directly to boundary/error output
```

Inspector:

```text
status: FLUSHED
payload: EmptyTitleError
skipped downstream: transform, save
boundary: title_to_add
```

Game rules:

- normal wires show normal values;
- flushed values travel on red bypass wires;
- skipped machines dim out;
- boundaries show where `FLUSHED` unwraps;
- `FLUSH` is not a `HOLD`, not a migration, and not `SKIP`.

## Modules As Factories Inside Factories

Modules/functions/components should be displayed like nested factories.

High-level map:

```text
Project
  store factory
  UI factory
  todo row factory
  theme factory
  validation factory
```

Collapsed module:

```text
+ counter_panel() ----------------+
| inputs: store                   |
| outputs: Element                |
| live instances: 3               |
| dirty this tick: no             |
| errors: 0                       |
+---------------------------------+
```

Zoomed interior:

```text
app > document > todo_app() > todo_panel() > todo_row()
```

The UI needs two related views:

- **definition view**: the function/template blueprint;
- **instance view**: one live call site or row scope with current values.

Example:

```text
FUNCTION new_todo(todo, store)
definition: row template factory

selected instance:
  scope: /todos:42
  title = "Buy groceries"
  completed = False
  editing = False
```

Do not render every dynamic row as a separate permanent module. Render the row
template once, then let the user inspect selected runtime instances.

## Ownership Tree Vs Dependency Graph

Boon needs both views.

### Ownership Tree

Ownership answers:

```text
Where does this state live?
What dies when this object/list/module is removed?
Who owns this value?
```

Visual convention:

- solid thick edges;
- tree or forest layout;
- owner contains owned children;
- deleting an owner previews what disappears.

Example:

```text
document
  root element
    counter panel
      decrement button
      reset button
      increment button

store
  sources
  count HOLD
```

The old Boon README describes the principle: every piece of state has one owner;
references are dashed arrows, not ownership.

### Reference And Dependency Graph

References answer:

```text
Why did this value change?
Which sources can affect it?
Which values read it?
Which render output depends on it?
```

Visual convention:

- dashed edges for references/reads/source bindings;
- glowing pulse edges for current event flow;
- red bypass edges for `FLUSH`;
- blue migration edges for `DRAIN`;
- gray debug-only edges for hidden keys/generations/source binding ids.

Example:

```text
increment_button.press --dashed/source ref--> count HOLD
count -------------------------------dashed--> counter label
```

Default view should show ownership plus live values. Debug mode should add
dependency edges, dirty paths, flush paths, drain migrations, and hidden runtime
identity.

## Static Vs Dynamic Parts

The editor must constantly distinguish:

```text
static graph shape
dynamic runtime state
dynamic row instances
rendered host objects
hidden runtime identities
```

Static graph:

```text
SourceRead
Then
Latest
Hold
ListMap
RenderForEach
FunctionCall
```

Dynamic state:

```text
current HOLD values
pending candidates
dirty nodes
dirty keys
list order
valid bits
generations
source bindings
recent deltas
```

Display strategy:

- the static graph is the factory blueprint;
- dynamic state is live gauges, counters, pulses, selected-row details, and
  recent traces;
- hidden keys/generations appear only in the inspector or debug overlay;
- renderer visibility must not decide semantic recomputation.

For a large list:

```text
todos LIST
  semantic rows: 100000
  visible rows: 40
  dirty keys: 3
  recomputed row template instances: 3
  rendered patches: 6
```

This keeps the player from confusing UI windowing with Boon semantics.

## Live Values

Live values should be a first-class part of the visual game, not a side panel
only.

Useful displays:

- current value next to every selected wire;
- state gauge on every `HOLD`;
- dirty glow for values changed this tick;
- last-source tag showing what caused the current value;
- sparkline for recent numeric/bool values;
- list summary with length, dirty key count, inserts, removes;
- flush status and skipped downstream nodes;
- drain status and migration progress;
- type hints on ports and blocks.

Example state block:

```text
count HOLD
value: 42
previous: 41
pending: none
last writer: increment.press
dirty: no
owner: store
refs out: counter_value.label
```

Example list block:

```text
todos
rows: 1248
visible rows: 31
dirty rows this tick: 3
inserted: 1
removed: 0
stale source drops: 0
```

Example trace hover:

```text
source: todo_checkbox.events.click
seq: 184
changed:
  todo.completed[/todos:42]
  visible_todos
  active_count
  footer.render
```

## Code Round-Trip And Visual Metadata

Manual code edits must reflect back into the visual editor without treating
source line numbers as identity.

Recommended files:

```text
app.bn
app.layout.json
```

Layout metadata stores editor graph positions, not runtime behavior.

Example:

```json
{
  "formatter": "boon-visual-layout-v1",
  "pins": {
    "store.count": true
  },
  "positions": {
    "store.sources.increment.events.press": { "x": 80, "y": 120 },
    "store.count": { "x": 420, "y": 180 },
    "document.root": { "x": 780, "y": 180 }
  }
}
```

Do not key layout only by source span. Use layered identity:

```text
primary: stable persistence id, if present
secondary: semantic path
tertiary: structural fingerprint
fallback: source span and declaration order
```

Manual edit behavior:

- same expression changed: keep block position;
- pure rename with stable identity: keep position, update label;
- new node: auto-place near dependencies;
- deleted node: remove, optionally keep tombstone for undo;
- extract function: old block becomes call, new function becomes nested factory;
- add `DRAIN+DRAINING`: place old and new state blocks side by side;
- finalize drain: remove old block, keep destination position.

## Visual Formatter

The visual editor should have a formatter like code formatting.

Command ideas:

```text
format whole graph
format current module
format selection
pin selected blocks
unpin and reflow
reset layout metadata
```

The visual formatter must be deterministic:

```text
same source + same formatter version = same layout
```

Default layout:

```text
SOURCE
  -> THEN / WHEN
  -> LATEST / transforms
  -> HOLD
  -> derived values
  -> document / scene / output
```

Special handling:

- feedback loops route visibly through `HOLD`;
- `DRAINING` sources and `DRAIN` destinations are adjacent;
- `FLUSH` bypass paths route above or below the normal pipeline in red;
- list templates stay compact with row summaries;
- ownership groups become nested regions;
- function calls collapse into module buildings.

Scoring goals:

- stable diffs;
- low edge crossings;
- readable ownership groups;
- short trace paths;
- visible live values without overlap;
- static and dynamic layers not confused.

## Product Modes

### Map Mode

Architecture view. Modules as buildings. Good for ownership, boundaries, and
large-scale dependency direction.

### Interior Mode

Zoom into one module/function. Shows real machinery: `SOURCE`, gates, merges,
state, derived values, output construction.

### Trace Mode

Shows one source event moving through the program. Best for debugging:

```text
input event -> source -> then -> latest -> hold -> derived -> render delta
```

### Live Dashboard Mode

Same graph, optimized for current values, dirty paths, warnings, timing,
backpressure, flushes, and migration status.

### Code Mode

Serious source editor with synchronized selection. Expert users can type
directly and still use the visual/game views for debugging.

## MVP

The first vertical slice should be small:

```text
Counter
TodoMVC
Cells
```

This is enough to prove:

- source events;
- state cells;
- candidate merges;
- lists and row templates;
- derived values;
- render bindings;
- live values;
- deterministic replay;
- visual layout;
- code round-trip;
- migration visualization;
- flush visualization.

First screen:

```text
Play Surface     actual app preview
Circuit Surface  visual Boon graph/game world
Code Surface     canonical Boon source
Inspector        selected value/type/trace
Timeline         deterministic ticks
```

Progression:

1. Repair broken apps.
2. Complete missing state/events/bindings.
3. Refactor for cleaner code and graph shape.
4. Extend app behavior.
5. Ship/publish a real app.

## Verification And Scoring

Every generated or visually edited program should produce a proof bundle:

```text
parse succeeds
typecheck succeeds
canonical emit is stable
parse-emit-parse is equivalent
cycle checks pass
DRAIN/DRAINING checks pass
FLUSH paths typecheck
deterministic scenarios replay
source maps explain every generated line
visual layout is deterministic
```

Quality scores should not only be speed:

- readable source;
- fewer dirty nodes/keys;
- fewer ambiguous writes;
- fewer hidden layout overrides;
- shorter trace paths;
- clearer ownership;
- fewer unnecessary state cells;
- stable generated diffs;
- scenario coverage.

## Traps To Avoid

- Do not make canvas position semantic.
- Do not generate new graph nodes for each dynamic row.
- Do not hide state mutations behind object menus.
- Do not emit unreadable names like `node_482`.
- Do not make visual graphs impossible to search, refactor, or review.
- Do not let event broadcasts become untyped global soup.
- Do not leak runtime keys/generations into Boon code.
- Do not let the game use example-specific shortcuts instead of generic Boon
  source and runtime paths.
- Do not confuse `SKIP`, `UNPLUGGED`, `FLUSHED`, and `DRAINING`; they are
  different concepts and need different visuals.

## Recommended Visual Legend

```text
solid thick edge     ownership
dashed edge          reference/read/source binding
glowing edge         current tick pulse
red edge             FLUSH / FLUSHED bypass
blue edge            DRAIN migration
gray edge            hidden runtime metadata
green block          active normal value
yellow block         dirty this tick
red block            error or flushed boundary
blue block           draining or receiving drain
striped block        debug-only hidden identity
```

## Summary

The game should let people write Boon by building and debugging a living
machine.

The machine is fun because it is spatial, live, replayable, and inspectable.
The code is serious because the editor preserves Boon's real model: static typed
graph, explicit state, deterministic ticks, hidden dynamic keys, source-driven
migrations, and readable generated source.

The target experience:

```text
I can play with the machine.
I can see why it works.
I can see why it broke.
I can migrate state without losing it.
I can open the source and keep programming normally.
```
