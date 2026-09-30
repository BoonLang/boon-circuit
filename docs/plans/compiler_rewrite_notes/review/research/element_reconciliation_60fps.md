Research note, 2026-09-29, for review/REVIEW.md; not authority.

Topic: reconciling structural element data at 60 FPS. Plan targets: §4.6
(R-DOC), R4, spike S3, D13/D20/D25/D32/D35/D17, Q18, and the product budgets
in `docs/architecture/NATIVE_GPU_PIPELINE.md` ("Timing Definitions").
Systems: Xilem/Masonry, Flutter, React (Fiber, memo, Fast Refresh), SwiftUI,
Jetpack Compose, egui, GPUI, Iced, Dioxus, Makepad. Freya was not researched.

Method: docs pages fetched with WebFetch; source files fetched with curl into
`scratchpad/research/reconciliation/` and quoted verbatim. WebFetch returns
text through a summarizer, so quotes marked (WF) are the tool's quotation of
the page, not independently re-checked. One local microbenchmark (§8).
Every claim carries [verified: ...] or [not verified: ...].

## Summary

1. All systems split short-lived descriptions (views, widgets, elements) from a retained, identity-keyed store; R4 is that split.
2. Identity is structural (position/type) with sibling-local keys; Compose and GPUI `use_state` use call sites. No system lets a *value*
   keep identity when placed twice; Flutter asserts, SwiftUI forbids, egui warns, GPUI panics. §4.6 "copied or moved keep identity" is new.
3. Branch switches reset state in SwiftUI, Compose and Xilem `OneOf`; React's shared ternary state is a documented pitfall. §4.6 matches the former.
4. Sublinearity always comes from reference-identity skips or dirty marking (Flutter identical widget, Xilem `Arc::ptr_eq`, GPUI cached views),
   and Xilem diffs per rebuild, not per frame. The §4.6 claim holds only if the producer returns the same Arc for unchanged subtrees.
5. D32 ("every write fires") defeats that fast path for derived elements unless R-DOC keeps a cutoff like today's `scalar_guarded_dependents`.
6. Measured (§8): structural diff of 5,101 nodes 62 µs, ptr_eq-shared diff 0.4 µs, but *building* the tree 690 µs: the risk is production, not diffing.
7. Hot reload: Compose replaced counter keys with stable keys and resets only groups whose code hash changed; Makepad (2026) shipped a
   retained-vs-derived render state divergence. Retain little, key caches by content.
8. React and SwiftUI keep rendered output opaque; D13 is more open, so invalidation must be field-level to stay sublinear.

---

## 1. Xilem and Masonry (Rust)

Views are short-lived values; `View::rebuild(&self, prev: &Self, view_state,
ctx, element: Mut<Self::Element>, app_state)` diffs against the previous view
and mutates a retained Masonry widget (WF). [verified:
https://docs.rs/xilem_core/latest/xilem_core/trait.View.html]
- Arc fast path, verbatim: `if core::mem::take(&mut view_state.dirty) ||
  !Arc::ptr_eq(self, prev) {` then rebuild. `memoize` reruns only if
  `prev.data != self.data`, and its doc says "The story of Memoization in
  Xilem is still being worked out". [verified: `xilem_core/src/views/impl_rc.rs`
  l.90-120; `views/memoize.rs` l.10-13, 126]
- `Vec<impl ViewSequence>` is **positional**: common prefix rebuilt pairwise,
  tail torn down, a per-index generation bumped "on the falling edge" so stale
  async messages drop. No keys. [verified: `view_sequences/impl_vec.rs`
  l.12-31, 106-171]
- Branch switch: `OneOf` rebuilds in place for the same variant, otherwise
  "Teardown the old version" and builds fresh; `AnyView` bumps its generation
  "because the underlying widget has been swapped out". [verified:
  `views/one_of.rs` l.308-393; `views/any_view.rs` l.104-126]
- Levien: the view tree "is retained only long enough to assist in event
  dispatching and then be diffed against the next version"; identity is "based
  on structure ... or an explicit key"; Arc memoization uses "pointer equality
  on the Arc rather than a deep traversal", broken along the `make_mut` path
  (WF). [verified: https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html]
- Masonry passes do no work unless invalidation flags are set; scrolling
  requests compose, not layout; "stashed" widgets (hidden tabs) keep state
  without events or paint. [verified: `masonry_core/src/doc/pass_system.md`
  l.75, 190-201; `masonry_concepts.md` l.131-139]
- No published numbers: issue #362 (open since 2022-12-24) still asks for FPS
  and latency infrastructure. [verified: https://github.com/linebender/xilem/issues/362]

## 2. Flutter (Widget → Element → RenderObject)

- `canUpdate` is `oldWidget.runtimeType == newWidget.runtimeType &&
  oldWidget.key == newWidget.key`. [verified:
  https://api.flutter.dev/flutter/widgets/Widget/canUpdate.html]
- "The framework keeps a list of dirty elements and jumps directly to them
  during the build phase". An element "can return immediately from build,
  cutting off the walk, if the parent rebuilds the element with an identical
  widget", which is an object-identity comparison. Child lists: "Flutter does
  not employ a tree-diffing algorithm"; it matches start and end by
  type+key, then uses a hash table by key for the middle, O(N) (WF).
  [verified: https://docs.flutter.dev/resources/inside-flutter, sections
  "Sublinear widget building", "Linear reconciliation", "Tree surgery"]
- The Element tree holds State and RenderObjects, because widgets are
  immutable. [verified: same page]
- GlobalKey moves: a keyed widget moved "in the same animation frame" keeps
  its subtree. Reparenting "is relatively expensive": it calls
  `State.deactivate` on the whole subtree and forces InheritedWidget
  dependents to rebuild. "You cannot simultaneously include two widgets in
  the tree with the same global key. Attempting to do so will assert" (WF).
  [verified: https://api.flutter.dev/flutter/widgets/GlobalKey-class.html]
- `const` constructors "allow Flutter to short-circuit most of the rebuild
  work". The walk stops "when the same instance of the child widget as the
  previous frame is re-encountered". Lazy builders build "only the visible
  portion". Budget: "16ms for building, and 16ms for rendering on a 60Hz
  display" (WF). [verified: https://docs.flutter.dev/perf/best-practices]
- Hot reload "re-builds the widget tree, preserving the app state; it doesn't
  rerun main() or initState()". It triggers a rebuild of **all** existing
  widgets and warns that state can differ from a fresh start (WF). [verified:
  https://docs.flutter.dev/tools/hot-reload]

## 3. React (Fiber, keys, memo, Fast Refresh)

- An O(n) heuristic replaces O(n³) tree diff. Its assumptions: different
  types produce different trees (full remount), and keys identify children.
  Index keys mix state on reorder; unstable keys recreate nodes (WF).
  [verified: https://legacy.reactjs.org/docs/reconciliation.html]
- "React preserves a component's state for as long as it's being rendered at
  its position in the UI tree". The same component at the same position in
  both arms of a ternary **keeps** state, which the docs flag as a pitfall.
  Fixes: render in different positions, or add keys. "Keys are not globally
  unique. They only specify the position within the parent" (WF).
  [verified: https://react.dev/learn/preserving-and-resetting-state]
- `memo` compares props shallowly with `Object.is`. It "is completely useless
  if the props ... are always different". Passing JSX as `children` lets React
  know the children need no re-render. React Compiler auto-memoizes (WF).
  [verified: https://react.dev/reference/react/memo]
- Fiber's headline goal is splitting render work "into chunks and spread it
  out over multiple frames", because full re-render is too slow (WF).
  [verified: https://github.com/acdlite/react-fiber-architecture]
- `Children`: "uncommon and can lead to fragile code". Its data "does not
  include rendered output" of child components. The docs recommend data props
  or render props instead (WF). [verified: https://react.dev/reference/react/Children]
- Fast Refresh keeps `useState`/`useRef` values when hook order and arguments
  are unchanged. It resets class components and edited hook orders, and it
  always re-runs `useEffect`/`useMemo` (WF). [verified:
  https://reactnative.dev/docs/fast-refresh]
- Note: React elements carry **no** identity. Rendering one element object
  twice creates two fibers. Identity lives in the position.

## 4. SwiftUI (structural + explicit identity)

From the WWDC21 "Demystify SwiftUI" transcript (WF) [verified:
https://developer.apple.com/videos/play/wwdc2021/10022/]:
- Structural identity comes from the view's generic type and position. An
  `if` becomes `_ConditionalContent`, so each branch has its own identity.
  "whenever the identity changes, the state is replaced". Apple recommends
  inert modifiers (opacity) over branches "to preserve identity".
- Identifiers must be stable ("a new identifier represents a new item with a
  new lifetime") and unique ("Multiple views can't share an identifier").
  Index-based `ForEach` identity "could cause a bad bug".
- `AnyView` erases structure. It hides diagnostics and "can result in worse
  performance".
- Invalidation goes through a dependency graph: only views that depend on a
  changed value get a new body. "A view's value is short-lived ... the view
  itself has a longer lifetime."

## 5. Jetpack Compose (call-site identity, the closest to R4)

- "The instance of a composable in Composition is identified by its call
  site ... Calling composables from multiple call sites will create multiple
  instances". Without `key(movie.id)`, inserting at the top recomposes every
  shifted item and restarts its effects. Keys need only be unique "amongst
  the invocations of composables at the call site". Skipping requires stable
  inputs compared with `equals` (WF). [verified:
  https://developer.android.com/develop/ui/compose/lifecycle]
- Strong skipping (default since Kotlin 2.0.20) compares unstable parameters
  by instance `===` and stable ones by `equals` (WF). [verified:
  https://developer.android.com/develop/ui/compose/performance/stability/strongskipping]
- Moving state to a new position is opt-in: `movableContentOf` "moves the
  remembered state and nodes created in a previous call to the new location".
  The source avoids "using the identity of a lambda instance as it can be
  merged into a singleton or merged by later rewritings". [verified: AOSP
  `compose/runtime/.../MovableContent.kt` lines 21-41, 266-270]
- Hot reload: groups carry integer keys (`startRestartGroup(key)`). Counter
  and lambda-ordinal naming caused cascading renames and slot-table
  mismatches, so compiler 2.1.20+ uses stable group keys. A group is
  invalidated when a hash of its code (ignoring line numbers), compounded
  over its transitive dependencies, changes. "the state created by any
  invalidated groups will be reset" (WF). [verified:
  https://blog.jetbrains.com/kotlin/2026/01/the-journey-to-compose-hot-reload-1-0-0/]

## 6. Immediate mode: egui, and GPUI (Zed)

**egui.**
- Ids are hashes: `Id::new(source)`, and `with()` salts the parent. They must
  be unique (WF). [verified: https://docs.rs/egui/latest/egui/struct.Id.html]
- Automatic ids are **counter-based, not call-site-based**. `next_auto_id` is
  "based on the unique_id of this Ui and the number of widgets added so far.
  It is therefore NOT stable ... Do not use it for widgets that store state".
  State needs `make_persistent_id(id_salt)`, which is stable "as long as
  id_salt is unique within the current id scope". `check_for_id_clash`
  prints an on-screen error for an id reused at a different position in the
  same pass. [verified: egui `crates/egui/src/ui.rs` lines 909-926;
  `context.rs` lines 1415-1427]
- The README: in "a very large UI in a scroll area ... the content needs to
  be laid out each frame". "For most cases you can expect egui to take up
  1-2 ms per frame". Window titles are the default ids, so two same-named
  windows need an explicit id. [verified: egui README lines 232-242]

**GPUI.**
- The element tree is rebuilt every frame: "Before the start of the next
  frame, the entire element tree and any callbacks ... are dropped and the
  process repeats". [verified: zed `crates/gpui/src/element.rs` lines 10-14]
- Retained state is keyed by `(GlobalElementId, TypeId)`. `GlobalElementId`
  is `Arc<[ElementId]>`, the id stack from the root, so it is a *placement
  path*. `Element::id` "must be unique among children of the first
  containing element with an id". State is looked up in `next_frame` then
  `rendered_frame`, so it survives only while the id is rendered in
  consecutive frames. A duplicate access panics: "reentrant call to
  with_element_state for the same state type and element id". [verified:
  `element.rs` lines 62-67, 215; `window.rs` lines 4203-4246, 7132-7154]
- `use_state` keys by the caller's location
  (`ElementId::CodeLocation(*Location::caller())`, `#[track_caller]`). It
  warns: "If this is not sufficient to identify your state (e.g. you're
  rendering a list item), you can provide a custom ElementID". [verified:
  `window.rs` lines 4180-4195]
- Sublinearity comes from `AnyView::cached`: "The rendered subtree is recycled
  from the previous frame unless Context::notify was called". It requires a
  definite size (not measured from contents). [verified: `view.rs` lines
  39-45, 233-244, 488-568]
- Zed reports frame times "under 4ms", against the 8.33 ms 120 FPS budget
  (WF). [verified: https://zed.dev/blog/120fps; https://zed.dev/blog/videogame
  gives the 8.33 ms budget but says nothing about tree rebuilds]

## 7. Iced, Dioxus, Makepad

- **Iced.** `Tree::diff` recreates the subtree when the `Tag` (the
  `TypeId` of the widget's state) differs. `diff_children` is positional
  (truncate/extend, then pairwise). `keyed_column` uses a search that splices
  one contiguous changed range; it does not handle general reorder. `lazy`
  rebuilds when an FxHash u64 of the dependency changes, so equality rests on
  a hash. [verified: iced `core/src/widget/tree.rs` lines 59-75, 102-216;
  `widget/src/lazy.rs` lines 63-75, 222-225; `widget/src/keyed/column.rs` ~204-216]
- **Dioxus.** `rsx!` static structure compiles to `Template`s, and only
  dynamic nodes are diffed: "instead of 11 comparisons ... we have one".
  "Diffing this template takes 90% less time than before". Templates also
  enable hot reload without recompiling (WF). [verified:
  https://dioxuslabs.com/blog/templates-diffing/]
- **Makepad.** Issue #1061 (2026-04-22): after a live-edit reapply, CheckBox
  animator state survived but shader uniforms reset to DSL defaults, so the
  widget drew "off" while logically "on". "Nothing reconciles the two" (WF).
  [verified: https://github.com/makepad/makepad/issues/1061]
- **Freya.** [not verified: not researched in this budget]

## 8. Local measurement (throwaway microbenchmark)

`scratchpad/research/reconciliation/bench` (Rust, release, i7-9700K, 200-2,000
samples each). Nodes are `Arc` records with tag, site, key, text, a style
Arc and an `Arc<[Arc<Node>]>` of children, in a root → rows → cells grid. The
diff does ptr_eq first, then field compare, then pairwise children.

| grid (nodes) | build fresh tree | full structural diff | path copy (2 cells) | diff with ptr_eq |
| --- | ---: | ---: | ---: | ---: |
| 100×10 (1,101) | 139 µs | 14 µs | 1.3 µs | 0.3 µs (121 visited) |
| 100×50 (5,101) | 691 µs | 62 µs | 1.9 µs | 0.4 µs (201 visited) |
| 200×50 (10,201) | 1,733 µs | 121 µs | 2.8 µs | 0.6 µs (301 visited) |

p50 values. Build includes formatting the cell text. [verified: local run,
2026-09-30] Caveats: native Rust, not the Boon executor, which evaluates
records through the plan and is likely much slower per record [not
verified: executor per-record cost not measured]. No layout, text shaping or
GPU work is included.

---

## Implications for the plan

1. **Duplicate placement rule (§4.6 Identity, R4). Contradicts prior art;
   change.** No surveyed system lets a value that can be placed twice carry
   identity: Flutter asserts on duplicate GlobalKeys, SwiftUI forbids shared
   ids, egui warns, GPUI panics, and React sidesteps it with positional
   identity. In Boon it is reachable without HOLD: a WHEN/THEN copy of an
   element (D30) shown beside the live original, or one row element in two
   panes. Proposed: node identity = (value identity, occurrence); the first
   occurrence in document pre-order owns retained state; later ones get
   `value identity + placement slot path` plus a dev-window warning; never
   panic. S3 gets one fixture per case.
2. **Cross-parent moves (§4.6, R-DOC).** Flutter GlobalKey moves deactivate
   the subtree and must land in the same frame; Compose makes moves opt-in
   (`movableContentOf`); GPUI's id is a placement path, so moves lose state.
   Keeping "moved keeps identity" requires a global identity → retained-node
   map (not per-parent matching) and dropping constraint-keyed layout caches
   on reparent. S3 measures the map cost per visited node.
3. **Branch switches (§4.3 Change rules, D25, D30). Supports; write it
   down.** Literal-site identity gives per-arm identity like SwiftUI
   `_ConditionalContent`, Compose groups and Xilem `OneOf`, so two arms each
   building `Element/text_input` reset focus/caret on switch (React's shared
   state is its documented pitfall). Add a hover hint on such WHENs ("build
   once and vary fields to keep focus"), mirroring Apple's inert-modifier
   advice. D25 keeps every arm's *data* state; decide that an unselected
   arm's retained *host* state (scroll, caret) is dropped, as in GPUI,
   SwiftUI, React and today's `retain_view_state`, rather than "stashed"
   (Masonry). Dropping is consistent with D35.
4. **Element kind in the retained key (R4).** GPUI keys state by
   `(GlobalElementId, TypeId)`, Flutter requires `runtimeType` equality, iced
   uses `Tag(TypeId)`. After a hot-reload edit one site can change from
   `Button[...]` to `TextInput[...]` (D20). Retained key = (identity, tag); a
   tag change resets retained state.
5. **Row keys for `List/replace_all` (D17, R5).** Keys are the only thing
   that preserves list state in React, SwiftUI, Compose and Flutter;
   positional identity (Xilem `Vec`, iced, React index keys) mixes state on
   reorder. If replace_all mints fresh hidden keys per result, every Wellen
   page refresh remounts all rows and loses scroll/focus. Proposed:
   `List/replace_all(with: rows, key: .id)` with an optional key field;
   without it, keys are positional and hover says so. S3 adds a NovyWave
   refresh fixture counting remounted rows.
6. **"Nothing is diffed per frame" (§4.6 Recomputation). Supports, with a
   condition.** It is Xilem's model: diff per rebuild, skip on `Arc::ptr_eq`
   or memo equality; Flutter skips only on an identical instance. R4 must
   state the producer contract: the executor returns the *same* Arc for a
   subtree whose inputs did not fire and path-copies parents of changed
   subtrees. The reconciler tests ptr_eq, then structural equality, then
   diffs, and counts each outcome.
7. **D32 vs the fast path (D32, R8, R4). Suggests a change.** Under D32 a
   Cells selection move fires all 5,000 per-cell style records as fresh Arcs.
   Diffing them is cheap (62 µs, §8); producing them is not (0.7 ms natively,
   more in the executor). Today's runtime avoids this with value-guarded
   bindings (`crates/boon_document/src/runtime.rs:1160-1185`; NATIVE_GPU_PIPELINE
   "Retained scalar dependencies may be value guarded"). R-DOC must keep an
   equivalent that no Boon code can observe: the value-guard index for
   `x == captured` selectors, or interning an equal element record to the
   previous Arc inside the renderer. S3's Cells fixture must show O(changed
   cells) record evaluations per selection move.
8. **Make the S3 gate asymptotic and tie it to product budgets (S3, §4.6
   "Performance gate", NATIVE_GPU_PIPELINE "Timing Definitions").** "No worse
   than today" can pass while scaling linearly. Per interaction, count
   records evaluated, reconciler nodes visited (ptr_eq / equal / diffed),
   frame nodes patched and hit-table entries touched. Require constant counts
   for a one-row toggle in synthetic TodoMVC at N = 100, 1,000, 5,000; Cells
   repeated selection (20 measured clicks) evaluating O(1) cells with p95
   ≤ 16.7 ms and max ≤ 33.4 ms end to end; and, as a worst-case guard, a full
   5,000-node structural re-diff under ~0.5 ms (§8: 62 µs native).
9. **Keep demand-driven materialization (§4.6; NATIVE_GPU_PIPELINE "List
   materialization is demand based", "startup must not evaluate all logical
   cells").** Flutter lazy builders build "only the visible portion"; egui
   names large scroll areas as its cost cliff; Masonry and Xilem ship virtual
   scroll [verified: file names `masonry/src/widgets/virtual_scroll.rs`,
   `xilem/examples/virtual_cats.rs`; contents not read]. If "elements are
   ordinary values" makes `List/map` produce one record per logical row
   eagerly, Cells and NovyWave startup regress. R4 should state that a mapped
   list of elements is a lazy, row-keyed value materialized by viewport,
   overscan and focus, while the checker still sees an ordinary LIST.
10. **Reconcile lists from typed deltas (R4; NATIVE_GPU_PIPELINE `Session`
    "typed deltas").** Flutter and React scan children to match keys; the Boon
    list authority already knows which row keys changed. The reconciler takes
    (new list value, changed-row set) and falls back to a ptr_eq scan only
    without a delta.
11. **Stable site keys (§4.6 Hot reload, Q18, lowering.md LP3).** Compose's
    hot reload broke on counter and lambda-ordinal keys and moved to stable
    keys. Site key = hash(definition path, structural route of the literal),
    the scheme §4.5 uses for persistence identities. Adopt Compose's rule:
    retained state resets only for sites whose definition code hash (or a
    transitive dependency's) changed. That replaces lowering.md's "best
    effort" wording.
12. **Retain little; key render caches by content (§4.6, D35, R-RENDER).**
    Makepad's 2026 bug was retained state and derived render state diverging
    after a live edit. GPUI and today's playground (`runtime_view.rs:1433-1445`)
    retain per-id interaction state and prune absent ids. The retained set
    should be focus, caret/selection, IME composition, scroll offset and
    pointer capture. Text layout, glyph and style caches are keyed by content
    (text, resolved style, width), never by identity, so a style edit
    invalidates them automatically. Test: a style edit beside a focused input
    keeps focus and repaints.
13. **Field-level dependencies for readable elements (D13, §4.3 Contracts,
    S2).** React keeps rendered output opaque ("fragile"), SwiftUI's `AnyView`
    costs performance, and Compose has no view values. Boon lets code read
    `button.label` or `children`. If dependencies are per element record, any
    style update in a subtree re-fires every reader. The checker records
    static field paths; lowering creates field-path dependencies; reading
    `children` depends on the list authority, not each child. S2/S3 count
    reader re-fires on a NovyWave-shaped theme.
14. **Never cons element sites; compare values, not hashes (§4.5 Phase B).
    Supports.** Compose avoids lambda identity because rewritings "merge"
    instances; the plan already excludes construction sites via
    `owns_identity`. iced's `lazy` trusts a u64 FxHash for equality; the
    cutoff in item 7 must compare values, matching the AGENTS rule that reuse
    keys are exact inputs.
15. **Derive a "template" effect (R4 "replaces DocumentPlan templates").**
    Dioxus measured 90% less diffing by splitting static template parts from
    dynamic slots. Deleting DocumentPlan templates is fine if each
    construction site's constant fields are one shared constant-pool Arc, so
    ptr_eq skips them and only dynamic slots are compared.

## Sources

All fetched 2026-09-29/30 unless noted; "curl" = raw source file, quoted verbatim.
1. https://docs.rs/xilem_core/latest/xilem_core/trait.View.html (fetched)
2. https://github.com/linebender/xilem (main): `xilem_core/src/views/{impl_rc,one_of,any_view,memoize}.rs`, `xilem_core/src/view_sequences/impl_vec.rs`, `masonry_core/src/doc/{pass_system,masonry_concepts}.md` (curl)
3. https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html (fetched)
4. https://github.com/linebender/xilem/issues/362 (fetched)
5. https://docs.flutter.dev/resources/inside-flutter; https://docs.flutter.dev/perf/best-practices; https://docs.flutter.dev/tools/hot-reload (fetched)
6. https://api.flutter.dev/flutter/widgets/Widget/canUpdate.html; https://api.flutter.dev/flutter/widgets/GlobalKey-class.html (fetched)
7. https://legacy.reactjs.org/docs/reconciliation.html; https://react.dev/learn/preserving-and-resetting-state; https://react.dev/reference/react/memo; https://react.dev/reference/react/Children (fetched)
8. https://github.com/acdlite/react-fiber-architecture; https://reactnative.dev/docs/fast-refresh (fetched)
9. https://developer.apple.com/videos/play/wwdc2021/10022/ (fetched, transcript)
10. https://developer.android.com/develop/ui/compose/lifecycle; https://developer.android.com/develop/ui/compose/performance/stability/strongskipping (fetched)
11. https://android.googlesource.com/platform/frameworks/support/+/refs/heads/androidx-main/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/MovableContent.kt (curl, base64 TEXT)
12. https://blog.jetbrains.com/kotlin/2026/01/the-journey-to-compose-hot-reload-1-0-0/ (fetched)
13. https://docs.rs/egui/latest/egui/struct.Id.html (fetched); https://github.com/emilk/egui (main): `README.md`, `crates/egui/src/{ui,context}.rs` (curl)
14. https://github.com/zed-industries/zed (main): `crates/gpui/src/{element,window,view}.rs` (curl)
15. https://zed.dev/blog/120fps; https://zed.dev/blog/videogame (fetched; the latter has no tree-rebuild statement)
16. https://github.com/iced-rs/iced (master): `core/src/widget/tree.rs`, `widget/src/lazy.rs`, `widget/src/keyed/column.rs` (curl)
17. https://dioxuslabs.com/blog/templates-diffing/ (fetched)
18. https://github.com/makepad/makepad/issues/1061 (fetched)
19. https://github.com/krausest/js-framework-benchmark README (curl; operation definitions only; browser DOM results not used because they do not transfer to native)
20. Freya: not accessed.
21. Repo: `crates/boon_document/src/runtime.rs:1160-1185`, `crates/boon_native_playground/src/runtime_view.rs:1433-1445`, `docs/plans/compiler_rewrite_notes/lowering.md` (read)
22. Microbenchmark: `/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/research/reconciliation/bench/src/main.rs` (run 2026-09-30)
