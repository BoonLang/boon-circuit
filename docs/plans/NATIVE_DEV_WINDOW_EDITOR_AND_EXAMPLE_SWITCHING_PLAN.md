# Native Dev Window Editor And Example Switching Plan

Status: historical background, not the active native GPU contract

Recovery note, 2026-05-28: do not use this file as the implementation or
verification contract for native GPU handoff, dev-editor scroll recovery, or
example-switch recovery. The active native window contract is
`docs/architecture/NATIVE_GPU_PIPELINE.md` plus the repo `AGENTS.md` native GPU
gate list. The corrective plan for the recent scroll/example-switch regression
is `docs/plans/NATIVE_GPU_SCROLL_SWITCH_REGRESSION_RECOVERY.md`.

Created: 2026-05-24

This plan defines the missing dev/debug window work for the native playground.
The current visible dev window is not acceptable as a production/debugging
surface: it shows only a plain clipped source view, lacks browser-like example
tabs, lacks Run/Format controls, and does not yet use the richer Boon
playground editor experience.

The goal is a Rust-native dev/debug window rendered with the Boon native
document/GPU stack, not a webview, not CodeMirror, and not a Rust shortcut that
only works for TodoMVC or Cells.

Rust libraries may be added to implement this properly, including libraries for
text editing, rope storage, parsing/tokenization, text shaping, layout,
formatting, syntax highlighting, undo/redo, diffing, or diagnostics. The
requirement is not to write everything from scratch. The requirement is that the
chosen libraries fit the architecture and compile for the long-term targets.

Any new library must:

- work on native Linux now;
- be compatible with browser/WASM later, or be isolated behind a small backend
  trait with a WASM-capable replacement already identified;
- avoid OS-only assumptions in core editor/language logic;
- avoid global state that prevents multiple windows/tabs/editors;
- avoid coupling the editor to TodoMVC, Cells, or any concrete example;
- be usable from Rust without embedding a webview or JavaScript editor runtime;
- have acceptable license and maintenance status;
- keep rendering output in our native/browser rendering stack.

If a dependency is native-only but useful for a prototype, it must not enter the
core editor model. It may only live behind a replaceable adapter, and the plan
must name the WASM path before the implementation can be considered complete.

## Scope

This plan applies to:

- `examples/todomvc.bn`
- `examples/cells.bn`
- all future examples discovered from an example manifest
- the native dev/debug window of `boon_native_playground`

It complements `docs/plans/STRICT_EXAMPLE_VISIBLE_TESTING_RULES.md`. The strict
visible testing contract must verify the results of this plan.

## Architecture Boundaries

Keep each subsystem replaceable. Do not let editor, example catalog, runtime,
IPC, renderer, or verifier responsibilities bleed together.

Required components:

- `ExampleCatalog`: discovers built-in examples, user-added examples, and
  future manifest entries. Owns labels, source paths, ordering, categories, and
  persistence metadata. It does not parse or render Boon.
- `ExampleWorkspace`: owns open buffers, current file, dirty state, custom
  examples, and selected tab. It does not know how to render pixels or execute
  runtime events.
- `BoonLanguageService`: parser, formatter, syntax tokens, diagnostics, source
  maps, and validation rules such as rejecting `EXAMPLE` and `#` comments. It
  has no TodoMVC/Cells branches.
- `CodeEditorModel`: platform-neutral text buffer, selection, caret, undo/redo,
  scrolling state, and edit commands. It should be replaceable without changing
  example catalog, runtime, preview, or renderer APIs.
- `CodeEditorView`: native document/GPU representation of the editor model. It
  maps model state to generic visual primitives and hit regions.
- `DevWindowShell`: tabs, toolbar, diagnostics panels, status, and command
  wiring. It coordinates other components but should not implement parser,
  formatter, editor-buffer, runtime, or renderer logic itself.
- `PreviewTransport`: `ReplaceCode`, diagnostics, bounded telemetry, and
  preview status. It must move source/project payloads, not example-name
  rendering shortcuts.
- `PreviewRuntime`: parses/lowers/executes the received Boon source. It should
  not know whether the source came from a built-in example, custom example, or
  edited buffer.
- `NativeRenderer`: renders generic document/editor primitives. It must not
  branch on example names or editor command names.
- `Verifier`: launches exact windows, drives declared input scenarios, captures
  app-owned pixels, and checks reports. It must not mutate runtime state through
  private shortcuts when proving UI interaction.

Boundary rules:

- Rewriting only the code editor must not require changes to runtime, preview
  execution, example discovery, or TodoMVC/Cells source.
- Rewriting only the renderer must not require changes to parser, formatter,
  editor model, or example catalog.
- Adding a new example must not require native renderer branches.
- Adding a new editor feature must not require TodoMVC/Cells-specific code.
- Example metadata belongs outside Boon source.
- Runtime identity and source dispatch must remain independent of example
  display labels and tab labels.
- UI tests must fail if a component reaches across these boundaries to pass a
  scenario.

## Source Syntax Cleanup

### Comments

Boon example source must use Boon comments:

```boon
-- comment
```

The old `#` comments must be removed from examples and rejected or reported by
the formatter/linter unless a compatibility mode is explicitly requested.

Required work:

- convert `examples/todomvc.bn` and `examples/cells.bn` to `--` comments;
- update parser diagnostics to point users from `#` to `--`;
- update syntax highlighter token rules for `--` comments;
- update formatter to preserve and normalize `--` comments;
- add tests proving `#` comments do not silently pass in executable examples.

### No `EXAMPLE` Keyword In Boon Source

`EXAMPLE TodoMVC` / `EXAMPLE Cells` must not be supported as Boon syntax and
must not appear in executable example source. Example identity belongs in the
example manifest and dev-window metadata, not in the Boon language.

The visible editor must show only valid Boon code. If a line is metadata for
the playground, it belongs outside the editor buffer.

Required work:

- remove `EXAMPLE ...` lines from `examples/todomvc.bn`, `examples/cells.bn`,
  and any future executable `.bn` example;
- make parser/source validation reject `EXAMPLE` as an unsupported keyword in
  executable Boon files;
- make diagnostics say that example names belong in the manifest/dev metadata;
- make formatter refuse or preserve-as-error unsupported `EXAMPLE` input
  instead of normalizing it into accepted syntax;
- make syntax highlighting mark `EXAMPLE` as an invalid/reserved token when it
  appears in source;
- make all example loaders get display names from the manifest, file metadata,
  or dev-window state instead of from a Boon keyword.

The dev editor must never show syntax that the parser does not really support.

## Example Tabs

The dev window must provide browser-like tabs for examples.

Initial required tabs:

- `Cells`
- `TodoMVC`

Future examples must appear from the example manifest, not from hardcoded
renderer branches.

Required behavior:

- tabs are visible by default at the top of the dev window;
- clicking a tab switches the editor buffer and sends `ReplaceCode` to preview;
- the selected tab is visually distinct;
- dirty/modified examples show a dirty marker;
- switching away from a dirty tab must not silently discard edits;
- adding a future example requires manifest data, not Rust UI rewiring;
- tab switching must not restart the preview unless the runtime contract
  requires it;
- tab switching must update diagnostics, source hash, and preview status.

The preview window must still receive code/source only. It must not receive an
example name as a renderer shortcut.

## Add Examples On The Fly

The native dev window must support adding examples without recompiling or
editing Rust UI code.

Use the original `~/repos/boon` playground as a behavior reference:

- it has a project file map (`filename -> content`);
- it tracks the current file separately from the code editor contents;
- it supports built-in examples selected by name;
- it supports custom examples stored outside executable source;
- it can load examples from URL/query state before falling back to persisted
  editor state;
- it lets tooling inject code and optionally a filename into the editor;
- it keeps source changes synchronized back to the project file map.

Native implementation requirements:

- provide an `examples/manifest.toml` or equivalent catalog file for built-in
  examples;
- support a user/custom example store outside the `.bn` source text;
- support adding a new example from a file path, pasted source, or future
  tooling command;
- assign each custom example a stable ID independent of display name;
- store display name, source path or inline source, category, entry file, and
  verification requirements outside Boon code;
- show custom examples as tabs alongside built-in examples;
- allow renaming/removing custom examples without changing Boon source;
- preserve dirty buffers per example;
- support single-file examples first, but keep the data model compatible with
  multi-file examples like the original playground;
- make `Run` operate on the selected buffer/project, not on a hardcoded example;
- make `Format` operate on the selected buffer/project, not on a hardcoded
  example;
- make tab switching send `ReplaceCode`/project payloads to preview without
  giving preview an example-name rendering shortcut.

Tooling requirements:

- add a command/API to select an example by catalog ID or label;
- add a command/API to inject source with an optional filename;
- add a command/API to create/update a custom example;
- add a command/API to list available examples and custom examples;
- make all commands work with native now and browser/WASM later.

The catalog must be generic. It may have categories such as `main`, `7gui`,
`debug`, and `custom`, but these categories are metadata only. They must not
change renderer/runtime behavior.

## Dev Window Controls

The dev/debug window must expose expected editor controls.

Required controls:

- `Run`
- `Format`
- `Reset`
- current example tab selector
- diagnostics/status indicator
- preview connection/status indicator

Recommended controls:

- `Verify`
- `Reload From Disk`
- `Save`
- `Undo`
- `Redo`
- source hash / dirty status

### Run

`Run` must:

- parse the current editor buffer;
- validate/lower it through the normal parser/IR/runtime path;
- send the exact editor buffer to preview via `ReplaceCode`;
- update the preview from that code;
- show parser/runtime diagnostics in the dev window;
- never bypass the parser by directly mutating runtime state.

### Format

`Format` must work.

Required behavior:

- format the current editor buffer using a real Boon formatter;
- reject unsupported `EXAMPLE` declarations with a useful diagnostic;
- preserve `--` comments;
- normalize indentation;
- report formatter diagnostics without destroying the buffer;
- update dirty state;
- after formatting, `Run` must execute the formatted buffer successfully.

The formatter must be implemented as Boon language tooling, not as a
TodoMVC/Cells string rewrite.

## Native Code Editor

The dev window needs a real code editor, implemented in Rust and rendered by
the native Boon stack.

Forbidden shortcuts:

- webview editor;
- embedded browser;
- CodeMirror runtime dependency in the native app;
- plain static text pretending to be an editor;
- TodoMVC/Cells-specific highlighting rules;
- renderer branches based on example names.

Required editor features:

- full buffer storage, not only the visible fragment;
- vertical scrolling through the entire source;
- fast wheel scrolling;
- line numbers or a stable gutter;
- caret;
- selection;
- keyboard text input;
- deletion/backspace;
- Enter/newline indentation;
- Tab/Shift+Tab indentation behavior;
- Home/End/PageUp/PageDown;
- clipboard copy/paste where the native stack supports it;
- undo/redo model;
- visible focus state;
- line/column status;
- diagnostics underlines or markers;
- no clipping at the bottom of the window;
- no large blank middle region.

The editor must be rendered by generic native document/GPU components. If new
generic primitives are needed, add them generically.

Suggested library categories to evaluate instead of hand-rolling weak
subsystems:

- rope/buffer model;
- incremental text edits and undo/redo;
- syntax token storage;
- Unicode-aware cursor movement;
- text shaping/layout compatible with native and WASM;
- parser-backed diagnostics and source maps.

The editor model should be platform-neutral. Platform-specific code should be
limited to input adapters, clipboard adapters, font loading, and renderer
backends.

## Font And Syntax Highlighting

The native editor should match the editor look used by `~/repos/boon`.

Known reference from the Boon repo:

- `~/repos/boon/README.md` says Boon Playground uses JetBrains Mono with
  ligatures.
- `~/repos/boon/playground/fonts/src/main.rs` patches JetBrains Mono with
  longer dash/equals ligatures.
- `~/repos/boon/playground/frontend/typescript/code_editor/` contains the
  CodeMirror-based Boon highlighting setup that should be used as behavior
  reference, not as a native runtime dependency.

Required work:

- vendor or reference the same JetBrains Mono patched font as a repo asset;
- load it through the native text renderer;
- enable ligature behavior where supported by the native text stack;
- implement a Rust Boon syntax highlighter;
- derive tokens from the real parser/lexer where possible;
- highlight comments, keywords, declarations, source bindings, strings,
  numbers, operators, braces/brackets, diagnostics, and invalid tokens;
- keep highlighting incremental enough for large examples like Cells;
- ensure syntax highlighting does not slow preview rendering.

The highlighter must understand:

- invalid/reserved tokens such as `EXAMPLE`
- `--` comments
- `SOURCE`
- `HOLD`
- `LATEST`
- `THEN`
- `WHEN`
- `WHILE`
- lists/records/paths
- document/style declarations
- string interpolation/templates

## Dev Window Layout

The dev window must be useful by default.

Required visible layout:

- top tab strip;
- toolbar with Run/Format/Reset and status controls;
- large code editor filling the main area;
- diagnostics/status panel visible by default;
- scrollbars or other clear scroll affordance;
- current file/example title;
- source hash or dirty marker;
- preview connection status.

The source may be longer than the viewport, but the editor must contain the
full file and make it clear that the file is scrollable. The file must not be
truncated in memory or in the editor model.

## Future Example Support

Future examples must be added through metadata, not one-off Rust UI changes.

The example manifest should include:

- example label;
- stable example ID;
- source path;
- category/order;
- default tab order;
- initial scenario path;
- required visible testing rules;
- optional formatter/highlighter fixtures;
- whether the example should be shown by default.

The dev window should build its tabs from this manifest plus the custom example
store. It must not use Rust `match example` rendering paths.

## Honest Speed Measurement

The dev window and preview must be measured honestly. The earlier failure mode
was that Cells scrolling felt slow to a human while automated checks still
passed. This plan must close that gap.

Required speed measurement boundaries:

- measure the exact visible native launch path, not only a synthetic runtime
  loop;
- record whether each metric came from `real-window`, `host-synthetic`, or
  lower-tier runtime evidence;
- measure release builds for performance claims;
- include debug-build measurements only as secondary diagnostics;
- bind metrics to preview/dev PIDs, window roles, source hashes, and artifact
  hashes;
- report p50/p95/p99/max frame time, dropped frames, and longest visible stall;
- report the tested source length, row count, column count, and visible item
  count;
- fail when a test passes only because the example was reduced, cropped, or
  rendered as a static slice.

Cells must be a first-class performance gate:

- scrolling must target 60 FPS-class behavior with many cells;
- vertical wheel, horizontal wheel, and Shift-wheel paths must be measured;
- row and column headers must remain aligned during measured scrolling;
- formula bar focus/edit/commit must remain responsive while the grid is large;
- syntax highlighting and dev-editor updates must not slow preview scrolling;
- performance fixes must preserve the required Cells size and interaction
  surface.

Dev editor performance must also be measured:

- scroll through the full source buffer;
- measure syntax highlighting update cost;
- measure diagnostics update cost;
- measure tab switching and `Run`/`Format` command latency;
- fail if only a truncated source fragment is present in the editor model.

## Verification Gates

Add or update checked commands:

```sh
cargo xtask verify-boon-source-syntax \
  --report target/reports/boon-source-syntax.json

cargo xtask verify-native-dev-window-editor \
  --example todomvc \
  --report target/reports/native-gpu/dev-editor-todomvc.json

cargo xtask verify-native-dev-window-editor \
  --example cells \
  --report target/reports/native-gpu/dev-editor-cells.json

cargo xtask verify-native-example-tabs \
  --report target/reports/native-gpu/example-tabs.json

cargo xtask verify-native-editor-format \
  --report target/reports/native-gpu/editor-format.json

cargo xtask verify-native-example-speed \
  --example cells \
  --report target/reports/native-gpu/speed-cells.json

cargo xtask verify-native-dev-editor-speed \
  --report target/reports/native-gpu/dev-editor-speed.json
```

These gates must prove:

- executable examples do not contain `EXAMPLE`;
- parser/source validation rejects `EXAMPLE` with a useful diagnostic;
- examples use `--` comments;
- `#` comments are rejected or diagnosed in executable examples;
- tabs are visible and switch examples;
- Run sends the editor buffer through parser/IR/runtime/ReplaceCode;
- Format changes or confirms formatting and preserves semantics;
- editor buffer is complete and scrollable;
- editor uses the required native font asset;
- syntax highlighting is produced by Rust/native code;
- diagnostics appear in the dev window;
- preview updates after tab switch and Run;
- Cells visible scrolling is measured honestly and meets the configured
  60 FPS-class frame budget without shrinking the grid;
- dev editor scrolling and syntax highlighting are measured honestly on full
  source buffers;
- no renderer example-name shortcuts are introduced.

## Acceptance Criteria

The implementation is complete only when:

- TodoMVC and Cells source display correctly in the dev editor without
  unsupported `EXAMPLE` lines;
- `EXAMPLE` is rejected by parser/source validation and highlighted as invalid
  if a user types it;
- examples use `--` comments;
- tabs switch between Cells, TodoMVC, and manifest-defined future examples;
- Run works from the editor buffer;
- Format works and is tested;
- syntax highlighting and font match the Boon playground reference as closely
  as the native renderer permits;
- Cells scrolling and dev editor scrolling meet honest visible performance
  gates;
- the editor is implemented in Rust with the native rendering stack;
- strict visible testing proves the dev window state and interactions;
- all new reports pass schema validation.

## Non-Goals

Do not solve this with a browser/webview editor. Do not import CodeMirror into
the native app. Do not add TodoMVC/Cells-only Rust branches. Do not make
formatting or highlighting a regex-only shortcut if parser/token infrastructure
can provide structured tokens.
