# Multi-File Cells And Physical TodoMVC Migration Plan

## Summary

- Turn Cells into a real multi-file project in the runtime, editor, preview protocol, reports, and tests. The current disk split is kept, but the fake concatenated-buffer model is removed from public behavior.
- Migrate original physical TodoMVC from `/home/martinkavik/repos/boon/playground/frontend/src/examples/todo_mvc_physical` as `examples/todo_mvc_physical/`, preserving source except mechanical `LINK` to `SOURCE`.
- Implement the generic language/project features that physical TodoMVC truly needs now: modules, project payloads, `BUILD.bn` asset generation, `SOURCE` piping, record spread, `scene:` root, and physical scene styling.
- Defer only full physically accurate 3D/PBR rendering. V1 scene rendering is a real scene model with fast measurable 2.5D physical UI effects.

## Public Interfaces And Data Model

- Extend `examples/manifest.toml` project entries:
  - `source` remains the executable entry file.
  - `source_files` are ordered runtime project units.
  - Add `build_files` for build scripts such as `BUILD.bn`.
  - Add `asset_files` for non-Boon assets such as SVG icons.
- Replace public combined-source handling with a structured project payload:
  - `entry_file`
  - `units: [{ path, text, role }]`
  - `active_file`
  - `project_hash`
  - optional `generated_units`
  - optional `asset_files`
- Keep single-file examples as a one-unit compatibility path.
- Preserve per-file diagnostics: parser/runtime/typecheck errors report source file, line, and column.
- `scene: Scene/new(...)` becomes a supported public root beside `document: Document/new(...)`.
- Add a generic `SceneFrame` or equivalent renderer-neutral scene output. Native GPU consumes that generic scene output; it must not special-case physical TodoMVC.

## Key Implementation Changes

### 1. Planning And Source Preservation

- Copy physical TodoMVC files from the original repo into `examples/todo_mvc_physical/`:
  - `RUN.bn`
  - `BUILD.bn`
  - `Generated/Assets.bn`
  - `Theme/*.bn`
  - `assets/icons/*.svg`
- Apply only lexical `LINK` to `SOURCE` to migrated Boon source.
- Add a preservation test that compares migrated files against the original checkout after the same `LINK` to `SOURCE` normalization.
- Classic `examples/todomvc.bn` remains separate and is not rewritten into physical TodoMVC.

### 2. Real Multi-File Project Support

- Make Cells the first proof target for real project units.
- Current Cells files stay as the canonical split:
  - `examples/cells/defaults.bn`
  - `examples/cells/formula.bn`
  - `examples/cells/cell.bn`
  - `examples/cells/model.bn`
  - `examples/cells/columns.bn`
  - `examples/cells/store.bn`
  - `examples/cells/view.bn`
  - `examples/cells.bn`
- Stop exposing concatenated source with `-- file:` markers to the editor, preview protocol, source hashes, and reports.
- Dev editor shows one tab/buffer per project unit.
- Preview receives every unit through the structured payload.
- Editing any unit updates the project hash and refreshes preview with the whole project.

### 3. Module And Symbol Resolution

- Add file-path-derived module names for project units.
- Physical TodoMVC resolution examples:
  - `Theme/Theme.bn` provides `Theme/*`.
  - `Theme/Professional.bn` provides `Professional/*`.
  - `Theme/Glassmorphism.bn` provides `Glassmorphism/*`.
  - `Theme/Neobrutalism.bn` provides `Neobrutalism/*`.
  - `Theme/Neumorphism.bn` provides `Neumorphism/*`.
  - `Generated/Assets.bn` provides `Assets/*`.
- Preserve existing single-file/global behavior for current examples.
- Detect duplicate exported names inside the same module.
- Keep module semantics generic; do not add TodoMVC-specific imports or aliases.

### 4. Build And Asset Pipeline

- Implement enough real `BUILD.bn` execution for physical TodoMVC instead of treating generated assets as magic.
- Supported build capabilities for this migration:
  - `Directory/entries`
  - `File/read_text`
  - `File/write_text`
  - `Url/encode`
  - `Text/join_lines`
  - `List/retain`
  - `List/sort_by`
  - `List/map`
  - `Log/info`
  - `Log/error`
  - `Build/succeed`
  - `Build/fail`
  - `FLUSH` / `FLUSHED` result escape semantics
- Run build files in a sandbox rooted at the example/project directory.
- Verify `BUILD.bn` can reproduce `Generated/Assets.bn` from `assets/icons/*.svg`.
- Add a renderer asset registry that can resolve generated `data:image/svg+xml;utf8,...` values and rasterize/cache SVGs for native GPU rendering.
- Do not build a full general asset compiler yet; implement the real generic asset path needed by this project.

### 5. Language Features Needed By Physical TodoMVC

- Implement record spread syntax inside record literals:
  - `...expr` is valid inside `[ ... ]`.
  - Spreads merge left-to-right.
  - Later fields override earlier spread fields.
  - `UNPLUGGED` spread is a no-op.
  - Non-record spreads fail typecheck.
  - Duplicate explicit fields remain an error.
- Implement `|> SOURCE { target }` as the generic replacement for old `|> LINK { target }`.
- Support `SOURCE` in nested event/source records such as `event: [press: SOURCE]` and `element: [hovered: SOURCE]`.
- Add missing generic operations used by physical TodoMVC, including:
  - `Router/route`
  - `Router/go_to`
  - `Text/is_not_empty`
  - `Bool/toggle`
  - `List/every`
  - `Ulid/generate`

### 6. Scene Root And Physical Styling

- Accept `scene: Scene/new(root: ..., lights: ...)` as a root contract.
- Support the physical element family used by the source:
  - `Scene/Element/stripe`
  - `Scene/Element/block`
  - `Scene/Element/text`
  - `Scene/Element/text_input`
  - `Scene/Element/checkbox`
  - `Scene/Element/label`
  - `Scene/Element/button`
  - `Scene/Element/paragraph`
  - `Scene/Element/link`
- Preserve physical style data in the lowered scene model:
  - `depth`
  - `relief`
  - `gloss`
  - `metal`
  - `glow`
  - `material`
  - `move`
  - `spring_range`
  - `rounded_corners`
  - `borders`
  - `background.url`
  - scene lights
  - sizing and spacing
- Render V1 as performant 2.5D physical UI:
  - elevation shadows
  - bevel/highlight
  - material tint
  - glow
  - rounded surfaces
  - SVG-backed checkboxes
  - theme-visible style changes
- Do not implement full physically accurate PBR geometry in this milestone:
  - no per-element 3D mesh generation
  - no true perspective layout
  - no dynamic physical lighting simulation
  - no occlusion/normal-map material system

### 7. Physical TodoMVC Example

- Add manifest entry `id = "todo_mvc_physical"`.
- Runtime entry is `examples/todo_mvc_physical/RUN.bn`.
- `BUILD.bn` is listed as a build file and editor-visible project unit.
- `Generated/Assets.bn` is listed as a generated/runtime source unit.
- SVG files are listed as assets and included in project hash/report evidence.
- Add a physical TodoMVC scenario covering:
  - initial render
  - add todo
  - reject empty todo
  - toggle one todo
  - toggle all
  - filter all/active/completed
  - edit todo
  - cancel edit
  - clear completed
  - switch each theme
  - toggle light/dark mode
- Keep classic TodoMVC tests and example behavior intact.

## Test Plan

- Unit tests:
  - Cells project loads as eight real units.
  - Editor-visible source has no synthetic `-- file:` markers.
  - Project hash changes when any Cells helper file changes.
  - Per-file parser/type diagnostics point to the correct unit.
  - Module lookup resolves `Theme/*`, theme variant modules, and `Assets/*`.
  - Record spread follows left-to-right override semantics.
  - Invalid record spread fails typecheck.
  - `SOURCE` declarations and `|> SOURCE { ... }` bindings lower correctly.
  - `BUILD.bn` reproduces `Generated/Assets.bn`.
  - Asset registry resolves generated SVG data URLs.
- Integration tests:
  - Cells preview receives structured project payload and still passes existing scenario behavior.
  - Editing a non-entry Cells file updates preview behavior.
  - Physical TodoMVC source preservation passes against normalized original files.
  - Physical TodoMVC parses, typechecks, builds generated assets, and lowers to a scene frame.
  - Physical TodoMVC scenario interactions update state and route/filter correctly.
- Native GPU/readback tests:
  - `verify-native-gpu-preview-e2e --example cells`
  - `verify-native-gpu-preview-e2e --example todo_mvc_physical`
  - Physical TodoMVC readback proves nonblank frame, visible title, visible todo rows, visible checkbox SVGs, visible footer, and visible theme switcher.
  - Readback proves physical effects with measurable luminance/shadow/highlight differences.
  - Theme switching changes rendered material/color/elevation output.
- Required repo gates:
  - targeted Rust tests for parser/runtime/playground/native GPU changes
  - `cargo check -p boon_runtime -p boon_native_gpu -p boon_native_playground -p xtask`
  - native GPU gate sequence from `AGENTS.md`
  - `cargo xtask verify-native-gpu-all --check-existing --report target/reports/native-gpu-all.json`

## Assumptions

- `todo_mvc_physical` is added as a new example and does not replace classic `todomvc`.
- Full PBR geometry is intentionally deferred for performance, but modules, build execution, generated assets, and asset loading are not deferred.
- The preview window continues receiving source/project payloads only, never example-specific render shortcuts.
- Human visual testing remains a follow-up after app-owned native GPU reports pass.
