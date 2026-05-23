# Runtime Production And Native TodoMVC Parity Plan

Status: planned

Created: 2026-05-23

This plan is the hard gate for two remaining classes of work:

1. Remove the remaining compiled-plan `leak_runtime_path(...)` layer and finish
   the related production runtime hardening called out by the latest AI review.
2. Make the native two-window TodoMVC playground visually and behaviorally match
   the original web TodoMVC reference, with proof from app-owned screenshots and
   synthesized input through the app input layer.

The goals are intentionally coupled. A production-shaped preview window is not
credible while the runtime still leaks compiled path strings, and runtime
hardening is not enough if the visible native app is blank, frozen, or only
validated by whole-desktop screenshots.

## Current Evidence

### Runtime Evidence

The current runtime has already moved beyond the earlier prototype shape, but
the compiled plan still contains many `&'static str` fields for runtime paths,
list names, source names, field names, and formula diagnostics. Those fields are
currently fed by:

```text
crates/boon_runtime/src/lib.rs:11667
fn leak_runtime_path(path: String) -> &'static str {
    Box::leak(path.into_boxed_str())
}
```

This must not be treated as complete. It is acceptable only as an interim
verification bridge. A long-running process that recompiles or reloads many
Boon programs cannot leak every compiled path forever.

The same area still contains related hardening gaps:

- compiled plan structs use `&'static str` as runtime identity;
- source routing still carries route-kind inference such as
  `GenericSourceRouteKind`;
- row source binding capacity can panic instead of rejecting the program or
  returning a typed error;
- list storage is improved but is not yet the final list-level columnar memory;
- some IDs are exact boxed names now, which avoids hash collisions but is still
  not the final compiler-assigned dense-ID shape.

### Native TodoMVC Evidence

The user-supplied screenshot from 2026-05-23 shows the current native two-window
TodoMVC state is not acceptable:

- the dev window has a small code strip at the top, a small debug strip at the
  bottom, and a large empty middle area;
- the preview window contains visible TodoMVC text, but it is not laid out like
  the web reference;
- the preview does not fill its client surface with a coherent app;
- the title appears as a small red `4` instead of the canonical large `todos`
  heading;
- the input row and list rows do not match the reference structure;
- interaction cannot be accepted unless synthesized input changes runtime state
  and app-owned pixels.

Known reference material:

- MoonZoon TodoMVC source:
  `/home/martinkavik/repos/MoonZoon/examples/todomvc/frontend/src/main.rs`
- MoonZoon TodoMVC store:
  `/home/martinkavik/repos/MoonZoon/examples/todomvc/frontend/src/store.rs`
- Raybox baked TodoMVC reference screenshot:
  `/home/martinkavik/repos/raybox/assets/todomvc/reference_screenshot.png`
- Raybox classic TodoMVC visual plan:
  `/home/martinkavik/repos/raybox/docs/plans/todomvc_classic/PLAN.md`
- Current Boon Circuit TodoMVC example:
  `examples/todomvc.bn`

## Goal 1: Production Runtime Hardening

Remove the compiled-plan string leak layer and finish the related runtime
production work. The result must be suitable for a long-running native preview
process that recompiles, reloads, and swaps Boon programs repeatedly.

### Required Runtime Shape

#### 1. Owned Program Symbols

Introduce an owned symbol/name table that is owned by the compiled program or
runtime plan:

```rust
pub struct RuntimeSymbols {
    paths: Vec<Box<str>>,
    // indexes/maps are allowed, but ownership stays here
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RuntimeSymbolId(u32);
```

Rules:

- no `Box::leak` in production runtime code;
- no global interner for compiled program paths;
- no hidden static cache to preserve old lifetimes;
- diagnostic/report labels may use `RuntimeSymbolId`, but hot execution must
  prefer typed compiler IDs;
- compiled plans must own all names they need for diagnostics, reports, and
  debugging.

`leak_runtime_path(...)` must be deleted, not wrapped.

#### 2. Dense Compiler IDs For Execution

Replace runtime identity fields that currently use names with compiler-owned
dense IDs wherever the IR already has those concepts:

- `SourceId` for input source dispatch;
- `ListId` for list memory and list operations;
- `FieldId` for record/list field storage;
- `StateId` for state slots;
- `NodeId` or `ExprId` for diagnostics and explainability.

Names remain labels only. They must not be hot-path execution identity.

Interim exact boxed names such as `FieldSlotId(Box<str>)` and
`ListSlotId(Box<str>)` are better than hash-derived slot IDs, but they are not
the final architecture. The acceptance gate must fail until hot storage and
compiled actions address fields/lists through dense IDs.

#### 3. Table-Driven Source Actions

Replace route-kind inference with a compiled action table:

```rust
pub struct SourceActionTable {
    by_source: Vec<SmallVec<[SourceAction; 2]>>,
}

pub enum SourceAction {
    SetRootText { target: StateId, value_field: PayloadFieldId },
    SetRootScalar { target: StateId, value_field: PayloadFieldId },
    AppendListRow { list: ListId, fields: Box<[FieldWritePlan]> },
    UpdateListField { list: ListId, field: FieldId, row: RowAddressPlan },
    RemoveListRow { list: ListId, row: RowAddressPlan },
    BulkSetListBool { list: ListId, field: FieldId, value_field: PayloadFieldId },
}
```

The exact enum can differ, but the contract is fixed:

- runtime event dispatch enters with `SourceId`;
- payload decoding is normalized once;
- route execution does not infer behavior from names, text/key presence,
  `primary_list`, `indexed_commit_field`, or route-kind heuristics;
- source binding metadata can map visible document controls back to hidden row
  addresses, but it must not become Boon-visible data.

The old `GenericSourceRouteKind` path may remain only as a temporary migration
adapter behind a failing production-hardening report. It must not be accepted by
the final gate.

#### 4. List-Level Columnar Storage

Move list row storage from row-owned `RuntimeRecord`/`ValueColumns` into
list-level columns:

```rust
pub struct ListMemory {
    keys: Vec<RowKey>,
    generations: Vec<RowGeneration>,
    order: Vec<RowSlot>,
    valid: bitvec::vec::BitVec,
    free_slots: Vec<RowSlot>,
    text_columns: Vec<TextColumn>,
    bool_columns: Vec<BoolColumn>,
    enum_columns: Vec<EnumColumn>,
}
```

Rules:

- row movement updates order slots, not field values;
- deleting a row marks validity and recycles the slot;
- dirty-key work addresses row slots and field IDs directly;
- snapshots/reports can materialize rows for debugging, but normal ticks must
  not rebuild row records or clone entire lists;
- append, delete, toggle, edit, filter, and clear-completed must be proportional
  to the semantic delta, not the full list.

#### 5. Capacity And Error Behavior

Remove panic-based user-program limits:

- `MAX_ROW_SOURCE_BINDINGS` overflow must become a compile-time validation
  error or typed runtime error;
- report the failing source/list/field by ID plus diagnostic label;
- add no-panic tests for too many row bindings, too many generated fields, and
  invalid row/source binding reuse.

#### 6. Parser, Diagnostics, And CI Hygiene

Finish the review items that are directly tied to production runtime use:

- structured parser diagnostics with stable spans through parser -> IR ->
  runtime;
- multi-error reporting where practical;
- `cargo clippy` gate for parser, IR, and runtime;
- property/fuzz tests for parser and runtime no-panic behavior;
- deterministic replay tests for event batches and snapshots;
- differential checks against a slow reference evaluator where the runtime has
  optimized storage/action paths.

### Forbidden Runtime Shortcuts

- Do not replace `Box::leak` with `lazy_static`, `OnceLock`, a global string
  cache, or process-lifetime `Arc<str>` interning.
- Do not keep names as execution identity and merely rename the fields.
- Do not hide panics behind verifier filters.
- Do not claim columnar storage while each logical row still owns all field
  vectors.
- Do not claim table-driven routing while the runtime still decides behavior by
  route-kind inference from source names or list labels.

### Runtime Acceptance Gates

The implementation must add or update a checked verifier:

```sh
cargo xtask verify-runtime-production-hardening \
  --report target/reports/runtime-production-hardening.json
```

That report must fail unless all of these are true:

- `leak_runtime_path` does not exist;
- production runtime code contains no `Box::leak`;
- compiled runtime plans do not use `&'static str` for path/list/source/field
  identity;
- hot source dispatch uses `SourceId -> [SourceAction]`;
- hot list memory uses list-level columns;
- field/list storage uses dense compiler IDs;
- capacity overflow returns structured errors instead of panicking;
- report evidence names any remaining migration adapters as blockers.

Required command set:

```sh
cargo fmt --check
cargo clippy -p boon_parser -p boon_ir -p boon_runtime --all-targets -- -D warnings
cargo test -p boon_parser -p boon_ir -p boon_runtime --lib
cargo xtask verify-runtime-production-hardening \
  --report target/reports/runtime-production-hardening.json
cargo xtask verify-runtime-finality \
  --report target/reports/runtime-finality.json
cargo xtask verify-report-schema
```

## Goal 2: Native Two-Window TodoMVC Parity

Make the native playground produce two useful windows:

1. a production-shaped preview window containing the app rendered from Boon
   code;
2. a dev/debug window containing the example/source editor and diagnostics.

Both windows must be filled with meaningful content by default. The preview
must look and behave like the original web JavaScript TodoMVC variant.

### Required Architecture

Follow `docs/architecture/NATIVE_GPU_PIPELINE.md`:

- preview and dev are separate native windows for the native path;
- preview can run without the dev window;
- dev sends `ReplaceCode` or loaded source to preview;
- preview must not receive an example name as a rendering shortcut;
- preview renders only generic Boon `document` output plus generic styles and
  generic components;
- dev/debug widgets must not slow preview rendering or input handling.

Browser tabs/windows can be planned later. This plan's implementation gate is
native only.

### Dev Window Requirements

The dev window must be useful on first launch:

- code editor visible by default;
- selected source path/title visible;
- editor fills the available middle area instead of leaving a blank middle;
- status/diagnostic panes are visible and scrollable without covering the
  editor;
- scroll performance is measured for the editor;
- no solid-color placeholder window can pass.

The screenshot in the user report shows a large blank middle area. That exact
failure must be covered by an automated content-fill check.

### Preview Window Requirements

TodoMVC must match the reference structure:

- centered app with canonical `todos` title;
- title typography/color/position comparable to the web reference;
- large input row with `What needs to be done?` placeholder;
- toggle-all affordance;
- list rows with circular toggle, title text, completed strikethrough/dimmed
  style, and delete affordance;
- footer with item count, All/Active/Completed filters, and Clear completed;
- informational footer matching the original copy;
- responsive placement that fills the window coherently without leaving
  accidental blank margins or cropped content.

The current small red `4` title failure must have a targeted regression check.

### Generic Renderer And Boon Source Rules

TodoMVC-specific structure and text must come from Boon source or generic test
fixtures, not Rust renderer branches.

Allowed Rust work:

- generic document nodes;
- generic layout primitives;
- generic style attributes;
- generic text shaping;
- generic input/button/checkbox behavior;
- generic hit testing;
- generic app screenshot/readback;
- generic synthesized input dispatch;
- generic visual comparison tools.

Forbidden Rust work:

- `if example == "todomvc"` rendering branches;
- `TodoMvcView`, `todo_row`, `selected_filter`, or similar renderer-only
  special cases;
- hardcoded TodoMVC strings in native renderer code;
- bypassing document/source bindings by directly mutating TodoMVC state from a
  test.

### App-Owned Screenshot Proof

Whole-desktop screenshots are not evidence for this gate. Verification must
capture pixels from the app-owned render path:

- preview framebuffer/readback for the preview window;
- dev framebuffer/readback for the dev window;
- window/surface IDs and role IDs in the report;
- screenshot dimensions, clear color ratio, content bounding boxes, and
  non-empty text/control region evidence;
- artifacts saved under `target/reports/native-gpu/`.

The verifier must fail if the only available screenshot is a COSMIC desktop
capture or an unrelated compositor image.

### Synthesized Input Proof

Input verification must drive the same harmonized input layer used by real OS
events:

```text
OS/window event -> host input event -> hit test -> document source binding ->
runtime SourceBatch -> render patch -> framebuffer change
```

The test harness may inject at the host-input boundary for deterministic
testing, but it must not skip hit testing, document source bindings, runtime
dispatch, or rendering.

Required TodoMVC scenarios:

- add a todo by focusing the input, typing text, and pressing Enter;
- reject an empty add;
- toggle a single row;
- toggle all rows;
- switch All, Active, and Completed filters;
- edit a row, commit with Enter, cancel with Escape, and commit on blur;
- clear completed rows;
- delete a row;
- verify every interaction changes both runtime state and preview pixels.

The report must include the synthesized event sequence, focused element/hit
region proof, runtime source IDs, semantic delta summary, render patch summary,
and before/after framebuffer hashes or crop diffs.

### Visual Reference Comparator

Add or update a deterministic visual comparator:

```sh
cargo xtask verify-native-todomvc-reference-parity \
  --report target/reports/native-gpu/todomvc-reference-parity.json
```

The comparator must use:

- `/home/martinkavik/repos/raybox/assets/todomvc/reference_screenshot.png`;
- `/home/martinkavik/repos/MoonZoon/examples/todomvc/frontend/src/main.rs`;
- `examples/todomvc.bn`;
- fresh preview framebuffer artifacts from the current binary.

It does not need exact pixel identity across platforms, but it must check:

- app bounding box location and size;
- title text region;
- input region;
- row heights;
- footer layout;
- visible controls;
- blank/clear-color ratio;
- connected mismatch regions;
- structural text/control inventory.

The report must write:

- normalized reference crop;
- normalized native crop;
- heatmap or structural diff;
- JSON metrics with pass/fail thresholds.

### Legacy Testing Cleanup

Do not keep dead testing paths that allow this class of bug to pass again.

Remove or quarantine legacy checks that use:

- Ply as the final native evidence path;
- COSMIC whole-desktop screenshots as app proof;
- direct Linux/Wayland/xdotool probing as the required correctness mechanism;
- browser windows for the native two-window gate;
- IPC-only state mutation as a substitute for visible input.

If any legacy command remains for historical compatibility, it must be named as
legacy, excluded from readiness, and documented as non-evidence for this plan.

### Performance Gates

The preview window must be measured independently from dev/debug work:

- preview first meaningful frame in release mode;
- TodoMVC interaction latency p50/p95/p99;
- frame time during repeated add/toggle/filter/edit operations;
- no full runtime snapshot sent to dev during normal preview interaction;
- no unbounded per-frame allocations after warmup;
- dev editor scroll latency measured separately from preview interaction.

The preview must not be slowed down by loading or rendering dev widgets. Reports
must identify preview and dev process IDs separately.

### Native TodoMVC Acceptance Gates

Add or update these checked commands:

```sh
cargo xtask verify-native-gpu-preview-e2e \
  --example todomvc \
  --report target/reports/native-gpu/preview-e2e-todomvc.json

cargo xtask verify-native-todomvc-reference-parity \
  --report target/reports/native-gpu/todomvc-reference-parity.json

cargo xtask verify-native-todomvc-input-parity \
  --report target/reports/native-gpu/todomvc-input-parity.json

cargo xtask verify-native-two-window-content \
  --example todomvc \
  --report target/reports/native-gpu/todomvc-two-window-content.json
```

The combined native gate passes only if:

- both native windows exist and have distinct role/window/surface IDs;
- preview is filled with TodoMVC content matching the reference structure;
- dev is filled with editor/debug content and has no large accidental blank
  middle region;
- synthesized input drives real document bindings and produces runtime plus
  framebuffer deltas;
- performance thresholds pass in release mode;
- static genericity checks find no TodoMVC-specific renderer branch.

## Combined Definition Of Done

The work is complete only when all commands below pass from the current
checkout:

```sh
cargo fmt --check
cargo clippy -p boon_parser -p boon_ir -p boon_runtime --all-targets -- -D warnings
cargo test -p boon_parser -p boon_ir -p boon_runtime --lib
cargo xtask verify-runtime-production-hardening \
  --report target/reports/runtime-production-hardening.json
cargo xtask verify-runtime-finality \
  --report target/reports/runtime-finality.json
cargo xtask verify-native-gpu-preview-e2e \
  --example todomvc \
  --report target/reports/native-gpu/preview-e2e-todomvc.json
cargo xtask verify-native-two-window-content \
  --example todomvc \
  --report target/reports/native-gpu/todomvc-two-window-content.json
cargo xtask verify-native-todomvc-reference-parity \
  --report target/reports/native-gpu/todomvc-reference-parity.json
cargo xtask verify-native-todomvc-input-parity \
  --report target/reports/native-gpu/todomvc-input-parity.json
cargo xtask verify-playground-genericity \
  --report target/reports/playground-genericity.json
cargo xtask verify-report-schema
cargo xtask audit-machine-readiness \
  --report target/reports/debug/machine-readiness.json
```

If any of those commands do not exist, implementing them is part of this plan.

## Implementation Order

1. Add the runtime production-hardening verifier in failing form.
2. Replace leaked compiled strings with owned symbols and dense IDs.
3. Convert source routing to compiled `SourceId -> [SourceAction]` execution.
4. Convert list storage to list-level columns.
5. Remove panic limits and add no-panic tests.
6. Make runtime hardening, clippy, tests, and finality pass.
7. Add app-owned preview/dev screenshot capture and synthesized input hooks.
8. Make the two native windows fill their surfaces with real content.
9. Extend generic document/style support until TodoMVC can match the reference
   from Boon code.
10. Add visual, input, content-fill, genericity, and performance verifiers.
11. Delete or quarantine obsolete Ply/COSMIC/direct-desktop evidence paths.
12. Run the combined definition of done and leave any remaining human testing as
    an explicit follow-up, not as an automated pass.

## Human Testing Handoff

After the automated gates pass, start the native playground for the user only as
manual confirmation:

```sh
cargo build -p boon_native_playground
cosmic-background-launch --workspace boon-circuit -- \
  ./target/debug/boon_native_playground --role desktop --example todomvc
```

Manual testing is follow-up evidence. It must not replace the automated
app-owned screenshot and synthesized-input gates above.
