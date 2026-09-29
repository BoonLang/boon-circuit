> **Design-panel input, 2026-09-29. Not authority.** Written by the design round
> that fed `docs/plans/BOON_COMPILER_REWRITE_PLAN.md`, followed by its adversarial
> review. Where this note disagrees with the plan's decision table (D1-D13) or
> defaults, the plan wins. Delete this folder once the P0 spec and contract exist.
> File:line references point at the tree as of 2026-09-29.

# Example migration and Theme refactor (TodoMVC physical Theme/*.bn, NovyWave NovyTheme.bn, corpus-wide breakage under the new rules, verification of example behaviour)

## area
Example migration and Theme refactor (TodoMVC physical Theme/*.bn, NovyWave NovyTheme.bn, corpus-wide breakage under the new rules, verification of example behaviour)

## summary
I surveyed all 21 manifest examples, the 5 native product gates (counter-dev, todomvc-physical, cells, novywave, persons-pro; the "negative" gate uses no example) and scanned the whole corpus for code the new rules will reject. The biggest item depends on an owner call: 110 record fields are written `[x: x]` (for example `element: [events: events]`, `Material[of: of]`, `padding: [top: top]`, `PASS: [store: store]`), and the documented rule (LANGUAGE_SEMANTICS.md:216-219) makes every one of them a self-reference. Only one self-reference is really wrong: TodoMVC's `title: LATEST { title  title_to_update }` (RUN.bn:132-135). Three places depend on LATEST keeping its own state (counter_latest, interval_latest, NovyWave value_format). Other breakage: 68 places read payload fields through the WHEN subject inside a bare-tag arm (55 in NovyWave RUN.bn), which only works with narrowing; 18 THENs on a continuous HOLD or LATEST value (persons_pro 14, NovyWave 4); 5 persons_pro field reads on a variant union; and the TodoMVC appended row that lacks `completed` (RUN.bn:60-63). The TodoMVC Theme breaks almost everywhere without narrowing: `get(request)` mixes NUMBER, record, LIST and the `Fully` tag, so all 94 Theme calls are affected, including 13 typed `gap:` arguments and 8 field reads. The refactor makes themes plain data: each theme exports one `tokens(mode)` returning the same record type, Theme.bn picks one by theme name, and a root `theme` value is read with plain field paths (`theme.material.panel`, `theme.font.body.color`). That replaces about 20 shapes with 8 per-role types, and every token has one type in every theme. I read the runtime's style handling (boon_document runtime.rs:5322-5616, render_scene.rs) to keep rendering identical. A missing field is not always the same as a neutral value: missing colour means a per-element-kind default fill, missing border width means 2 px, `material:` keys overwrite style keys in alphabetical order, and a `depth` key on text changes layout. So neutral values are chosen per role to equal the runtime defaults, and the two places where materials were layered are pre-merged. The analysis also found that shadows and glow inside `material:`, and `family` in font records spread straight into a style, never render today; the refactor drops them without changing any pixels. I drafted Base.bn, Theme.bn, Professional.bn and Classic.bn and checked single-file versions with today's release compiler; they type-check and produce a MachinePlan (`Light/*` has to stay out of the root record because the old backend cannot lower it there). The old pipeline cannot serve as a behaviour baseline for most examples: `boon_cli run` passes only 5 of 21 manifest scenarios, todo_mvc_physical and persons_pro fail before the first scenario step, and 8 of 22 manifest entry sources fail `boon_cli check` today (cells included). The verification plan therefore puts scenario triage first, then a CPU-only render diff built on boon_behavior_harness, then an A/B comparison of app-owned readback frames, then verify-all. I estimate 2.5 to 4 engineer-weeks of example work, depending mostly on the self-reference decision and on scenario triage.

## design
# Example migration and Theme refactor

## 0. Headline findings

1. **The `[x: x]` self-reference rule is the largest migration item, and its size depends on an owner decision.** A corpus scan finds 110 fields of the form `[x: x]` (for example `element: [events: events]`, `Material[of: of]`, `ButtonIcon[checked: checked]`, `padding: [top: top]`, `Oklch[... hue: hue]`, `Numeric[value: value]`, `PASS: [store: store]`). It also finds 1 continuous self-loop (TodoMVC `title: LATEST { title ... }`, RUN.bn:132-135) and 3 self-references inside a stateful LATEST: `examples/counter_latest.bn:7-11`, `examples/interval_latest.bn:5-9` and `examples/novywave/RUN.bn:3019-3022`. The counter_latest comment says "An initial arm makes this field stateful, so this counter does not need an explicit HOLD block". Under the documented rule (LANGUAGE_SEMANTICS.md:216-219) all 110 fields are errors. Under the alternative "a field's initializer does not see its own name" they are all fine, but the LATEST self-state idiom stops working. See the owner questions.
2. **No narrowing means the TodoMVC Theme API cannot work as written.** Each theme's `get(request)` (Professional.bn:5-19) returns NUMBER (depth, elevation, sizing, spacing), `NUMBER | Fully` (corners, Professional.bn:358-369), a LIST (lights), or records (material, font, text, geometry, spring range). Without narrowing, all 94 `Theme/*` calls in RUN.bn get that union:
   - 13 typed `gap:` arguments break (`number("gap")`, boon_typecheck lib.rs:32013), for example `gap: Theme/spacing(of: None)` 8 times.
   - 8 field reads break: `Theme/material(of: Primary).color` ×3, `Danger).color`, `font(of: Input).size/.weight`, `font(...).color` ×4.
   - The rest go into open `style:` records (open_object_type, lib.rs:31960/41655) as a union the backend cannot represent. The C10 probe x04 shows the lowering failure.
3. **The in-arm narrowing that decision 4 removes is used 68 times:** `real_waveform_open_result |> WHEN { WaveformOpened => real_waveform_open_result.format ... }` (NovyWave RUN.bn:507-519, 68-77 and 53 more; host_service_effects.bn 12; server_effect_chain.bn 1). The fix is mechanical: `WaveformOpened[format, signal_count, scope_count] => ...`.
4. **The render contract depends on whether a field is present,** so "uniform records with explicit values" are identical only with the neutral values chosen per role (section 3). The same analysis shows that authored shadows and glow inside `material:`, and `family` in font records spread straight into a style, **never render today**.
5. **The old toolchain is not a usable behaviour oracle for the product examples.** `boon_cli check` fails on 8 of 22 manifest entry sources, including cells, because the compact ABI slice lacks `Dependency/catch_cycle`, `List/range`, `Text/find`, `Timer/interval`, `List/take` and `Text/to_bytes`. `boon_cli run --scenario` passes 5 of 21: todo_mvc_physical and persons_pro fail before the first scenario step with "document evaluation failed ... row ... has no field", and novywave fails at `load-default-file`. counter.scn is stale: it names `store.sources.increment_button.press`, but the source is `...increment_button.events.press` (counter.scn:12).

## 1. Survey: what the product and the gates use

Handoff gates (docs/architecture/native_gpu_handoff_manifest.json): architecture (no example), counter-dev → `counter`, todomvc-physical → `todo_mvc_physical`, cells → `cells`, novywave → `novywave`, persons-pro → `persons_pro`, negative (observer-frame check, verify.rs:589-615; no example).

| example | gate | old `check` | old `run` scenario | new-rule breakage (count) |
|---|---|---|---|---|
| counter | counter-dev | pass | stale .scn source path | 1 `[events: events]` |
| todo_mvc_physical (3,561 lines) | todomvc-physical | pass | fails before the first step | Theme dispatch (94 calls); title self-loop; appended row lacks `completed`; 34 `[x: x]` (20 go away with the refactor); ~20 heterogeneous record shapes (owner request) |
| cells | cells | **fail** (ABI slice) | fail | 6 `[x: x]` (payload forwarding `Numeric[value: value]`, formula.bn:7,56,65,93,329); needs the builtins above |
| novywave (12k lines) | novywave | pass | fails at load-default-file | 55 in-arm narrowing reads; 4 THEN on continuous (RUN.bn:109,166,247,353); 3 `[x: x]` (RUN.bn:4949 PASS, NovyView.bn:5731-5732); value_format LATEST self-state; NovyTheme shapes; dead trace_* functions with union `.color` reads (NovyTheme.bn:359-470) |
| persons_pro | persons-pro | pass | fails before the first step | 14 THEN on HOLD state (RUN.bn:141,147,160,256,...); 5 reads of `registration_succeeded.*` on a variant union (effect_schema lib.rs:1011-1038); 3 `[x: x]` |
| todomvc | none | pass | fails at edit-test-todo | `PASS: [store: store]` (todomvc.bn:199) |
| todo/counter migrations v1-v7 / v1-v3 | none | pass | – | 10 `[events: events]` |
| kavik_cz, layers | none | old compiler crashes | – | 6 and 4 `[x: x]` (`padding: [top: top]`, `hue: hue`) |
| fjordpulse | none | **parse error** | – | stale `Server.value.field` syntax (FjordPulseView.bn:1957); 28 `[x: x]` |
| counter_latest, interval_latest | none | pass / fail (Timer ABI) | pass / fail | LATEST self-state (valid only if owner Q2 = yes) |
| fibonacci, interval_hold, flush_error_propagation | none | fail (old backend or ABI) | fail | compiler dependencies only |
| minimal, hello_world, flow_operators, pages | none | pass | pass | none found |

Recursion: none. The call graphs of TodoMVC and NovyWave are acyclic (C9). My regex scan's hits were false positives from top-level bindings between FUNCTION blocks and from same-named functions in other modules.

PASSED presence: no violations. The only PASS sites are the scene roots, plus Theme.bn:74 and 89-93, which the refactor removes.

Unused parameters: about 12 candidates (for example NovyView.bn:1597 `row`, persons_pro Workspace.bn:1 `revision`). Decision 6 does not list them. I recommend a warning.

## 2. Predicted breakage by rule

**2.1 Self-reference.** The scanner is `design/selfref2.py`.
- 110 plain copies. By field: events 25, of 10, hovered 9, width 8, store 6, selected 6, checked 6, value 4.
- 1 real continuous self-loop: TodoMVC title.
- 3 LATEST self-state references (above).
- The 7 "direct" hits in NovyBridge.bn are false positives; that file already uses the documented BLOCK-alias idiom.

Under the documented rule, forwarding a pattern binder into a field of the same name needs a BLOCK alias. Payload patterns cannot rename their binders (LANGUAGE_SEMANTICS.md, WHEN section), and without in-arm narrowing `parsed => Numeric[value: parsed.value]` is also an error. So `Parsed[value] => Numeric[value: value]` becomes `Parsed[value] => BLOCK { parsed_value: value  Numeric[value: parsed_value] }`.

**2.2 Instantaneous cycles.** Only SOURCE, HOLD, publication and async effects break cycles (LANGUAGE_SEMANTICS.md:219-222), and a THEN body "is evaluated against the current tick snapshot" (:348-352).
- TodoMVC new_todo has the cycle title → title_to_update → (THEN) edited_title → (THEN inside edited_title's stateful LATEST) title (RUN.bn:125-158). Taken literally, the rule rejects it.
- counter_latest-style self-state is also rejected by that literal rule.
- Recommended rule (owner Q2): a read inside an event-gated arm (a THEN body or an event-WHEN arm) of a LATEST that has a continuous arm sees the committed value, so that edge is not instantaneous. The runtime already behaves this way; counter_latest passes today.

**2.3 No call-result narrowing.** See headline finding 2. Other themes are fine:
- PersonsTheme.bn already has one function per role, each returning one shape.
- FjordPulseTheme material(of) and KavikTheme accent(of) return one kind; their differing record shapes are never read.
- NovyTheme material(mode, of) returns records only. Its only `.color` reads are in dead functions (NovyTheme.bn:447, 468).

**2.4 No in-arm narrowing.** 68 sites; the scanner is `design/migration/arm_narrow.py`. The rewrite binds payload fields, e.g. `Finished[retained] => retained |> WHEN { Retained[content] => Wellen/open(content: content)  __ => SKIP }`.

**2.5 Sound unions and field presence.**
- TodoMVC `List/append(item: title_to_save |> THEN { [title: title_to_save] })` appends rows without `completed`. The following `item.completed |> WHEN { True => True, False => False, __ => False }` (RUN.bn:60-63) reads a field that may be missing. Fix: append `[title: title_to_save, completed: False]` and pass `item.completed`.
- persons_pro `passkey.registration_succeeded` is `RegistrationNotRequested | RegistrationSucceeded[...] | RegistrationCancelled | RegistrationFailed[...] | DuplicateCredential[...]` (RUN.bn:49, effect_schema:1011-1038). Reading `.workspace_grant_bound`, `.account_id`, `.credential_id` or `.label` is an error. Fix: `RegistrationSucceeded[workspace_grant_bound, account_id] => ...`.
- NovyWave's 4 appended lists (RUN.bn:2785, 2971, 2974) keep one shape.

**2.6 THEN on a continuous value.** Decision 6 lists `5 |> THEN {6}` as an error. The current code uses THEN on a HOLD or a fallback LATEST as an "on change" trigger: persons_pro 14 (`passkey.registration_succeeded |> THEN {...}` on a HOLD) and NovyWave 4 (`real_hierarchy_page_result |> THEN { 0 }` on a LATEST with a NotStarted fallback). Fix: trigger from the underlying event, or pattern-match the state (`WHEN { RegistrationSucceeded[...] => ...  __ => SKIP }`). These need a semantic look, not a codemod.

**2.7 LATEST and HOLD type compatibility** (TYPE_INFERENCE plan:185, 482-484). The IR census (`design/mig_unions.py`, `design/migration/hold_kinds.py`) finds only TodoMVC `store.todos.title` / `edited_title` (Text vs Union, caused by the title self-reference) and variant-only mixes in NovyWave. Nothing else breaks if compatibility means "one kind; variant sets merge".

**2.8 Heterogeneous LIST literals.** Almost all are element lists (for example `LIST { Element/label, Element/stripe }`: NovyWave 94, TodoMVC 16), plus lights and shadows.
- This needs one opaque `Element` type for builtin constructors, which is a type-system dependency; otherwise every list is a record union.
- `LIST { Light/directional(...), Light/ambient(...), Light/spot(...) }` is a real sum type and stays as it is.
- Shadow lists become uniform in the refactor.

**2.9 Mixed-kind WHEN joins.** Allowed by decision 5 and needing no change. `color: mode |> WHEN { Light => Oklch[...], Dark => TEXT {...} }` appears 19× in TodoMVC and flows only into open style records. Neobrutalism's `weight: 800 / 900` next to tag weights (Neobrutalism.bn:210-230) is a real `NUMBER | tag` union. I keep it on purpose: native measurement keys a numeric weight as "normal" (boon_native_gpu lib.rs:434), so replacing it with a tag could change text widths.

## 3. The render contract: what a missing field means

Style records are flattened into one `StyleMap` per node (boon_document runtime.rs:5322-5400). The iteration is **BTreeMap order, i.e. alphabetical**, and a later key overwrites an earlier one.

- `material: M` (lower_material, :5529-5544): only **scalar** fields of M are kept. `color` → keys `background` and `material_color`; every other scalar field keeps its own name. LIST fields and untagged records (`shadows`, `glow`) are **dropped**. A tag becomes its name as text, so `None` is stored as "None" and `Oklch[...]` as "Oklch[...]".
- Fields spread straight into a style: field-specific branches handle `border`, `outline`, `shadows` (:5546-5570), `glow` (:5572-5589), `font` (lower_font :5412-5440: family→font, style→font_style, size, color, weight, line; `align` is ignored), and `padding`/`move`. Everything else is copied by name. That includes `family`, which nothing reads under that name, so NovyWave's 57 `...NovyTheme/font(...)` spreads lose the family.
- "material" sorts after "background", "border" and "border_width", so a material overwrites them. "outline" sorts after "material", so an outline overwrites a material's border.
- Defaults when a key is missing (render_scene.rs):
  - fill: `bg`, then `background`, then `default_fill_for_kind` (:1950-1953). The default is opaque for Stack and Row nodes and transparent for Button, Checkbox and Text (:3555-3575).
  - border width: 2.0 for a full border, and the side border width falls back to `border_width` and then 1.0 (:2066-2100).
  - gloss, metal, refraction, transparency, frosted_blur, glass_highlight: 0; frosted_saturate: 1; glass_highlight_color: white (:2521-2590, :3577-3616).
  - checkbox defaults: :2365-2480. Shadow x, y, blur, spread: 0.
- Behaviour that depends only on a key being present: a `depth` or `relief` key on a Text node adds 8 to its measured width (boon_document lib.rs:5653-5658); the `scroll*` keys change retained structure (runtime.rs:3880-3893); `width`+`height` presence (lib.rs:4268).
- Keys that nothing reads: `material_color`, `glow`, `move_*`, `spring_range_*`. Scene `lights` and `geometry` are also unread (grep finds no reader).

**Consequences for the refactor:**
- `None` is equivalent to a missing colour or border, because a colour that fails to parse falls back.
- Numeric neutral values must equal the defaults above.
- A material must not carry a border where the element's own border has to show through (NovyWave, 61 of 65 themed styles also set `background:`/`border:`; `design/migration/style_collide.py`).
- The two places where materials were layered (RUN.bn:648-649 editing input; RUN.bn:464-465 panel + PanelFrame) are pre-merged.

## 4. TodoMVC Theme refactor

### 4.1 Principles
1. **Themes are data.** Each theme module exports `tokens(mode)`; Theme.bn only picks one by name. A root value `theme: Theme/tokens(name: theme_options.name, mode: theme_options.mode)` is read with field paths. There is no dispatch on static tags anywhere, so narrowing is never needed, and the compiler checks 5 functions once each instead of fanning out over call paths.
2. **Every token has exactly one type, the same in all 5 themes.** The WHEN over theme names then joins 5 identical record types, and a theme that forgets a token is a positioned "field missing in one arm" error. The type system checks theme completeness for free.
3. **Genuinely different roles get their own type:**
   - Material (9 scalar fields)
   - BorderedMaterial (Material + border, border_width)
   - Frame (7 edge and shadow fields, spread into a style)
   - Shadow (x, y, blur, spread, color)
   - Font (family, size, color, weight, style)
   - Checkbox (15 fields)
   - numeric groups (depth, elevation, corners, sizing, spacing), spring ranges, geometry
   - lights (a LIST of builtin light values, kept as a function because the old backend cannot lower `Light/*` inside a root record; my probe hit "call `Light/directional` has no typed PlanExecutor row operation")
4. **Uniformity comes from spreading a neutral record:** `[...Base/material(), color: X, gloss: 0.04]` is a closed record with exactly Base's fields.
5. **Only effective values are kept.** Values that Theme.bn's wrappers overrode, unused arms and fields the runtime drops are all deleted.

### 4.2 New files (drafted in `design/migration/todo_theme_new/`; checked with today's compiler as single-file probes, `two_theme_probe.bn` → "pass: MachinePlan")

```boon
-- Theme/Base.bn
FUNCTION material() {
    [
        color: None
        gloss: 0
        metal: 0
        refraction: 0
        transparency: 0
        frosted_blur: 0
        frosted_saturate: 1
        glass_highlight: 0
        glass_highlight_color: TEXT { #ffffff }
    ]
}

FUNCTION frame() {
    [
        border: None
        border_width: 0
        border_top: None
        border_top_width: 0
        border_bottom: None
        border_bottom_width: 0
        shadows: LIST {}
    ]
}
```

```boon
-- Theme/Theme.bn (replaces 169 lines)
FUNCTION tokens(name, mode) {
    name |> WHEN {
        Classic => Classic/tokens(mode: mode)
        Professional => Professional/tokens(mode: mode)
        Glassmorphism => Glassmorphism/tokens(mode: mode)
        Neobrutalism => Neobrutalism/tokens(mode: mode)
        Neumorphism => Neumorphism/tokens(mode: mode)
    }
}

FUNCTION lights(name, mode) {
    name |> WHEN {
        Classic => Classic/lights(mode: mode)
        Professional => Professional/lights(mode: mode)
        Glassmorphism => Glassmorphism/lights(mode: mode)
        Neobrutalism => Neobrutalism/lights(mode: mode)
        Neumorphism => Neumorphism/lights(mode: mode)
    }
}
```

**Before** (Professional.bn:5-19 and 21-196; a `get` dispatcher, 11 request kinds, about 20 shapes):
```boon
FUNCTION get(request) {
    request |> WHEN {
        Material[of] => material(material: of)
        Font[of] => font(font: of)
        ...
        Corners[of] => corners(of: of)
        Lights => lights()
        Sizing[of] => sizing(of: of)
    }
}
...
Interactive[hovered] => [
    ...surface_variant_base
    gloss: hovered |> WHEN { True => 0.04  False => 0 }
    metal: 0.03
]
InteractiveRecessed[focus] => [ ...surface_base  gloss: ...  glow: focus |> WHEN {...} ]
FilterSelected => [ ...primary_subtle_base  shadows: PASSED.mode |> WHEN {...} ]
```

**After** (Professional.bn, 203 lines instead of 444):
```boon
FUNCTION tokens(mode) {
    [
        material: materials(mode: mode)
        panel_shadows: mode |> WHEN {
            Light => LIST {
                [x: 0, y: 24, blur: 48, spread: 0, color: TEXT { #222e4424 }]
                [x: 0, y: 2, blur: 7, spread: 0, color: TEXT { #222e4414 }]
            }
            Dark => LIST {
                [x: 0, y: 26, blur: 52, spread: 0, color: TEXT { #0000006b }]
                [x: 0, y: 1, blur: 0, spread: 0, color: TEXT { #ffffff08 }]
            }
        }
        frame: frames(mode: mode)
        checkbox: checkbox(mode: mode)
        font: fonts(mode: mode)
        primary_color: mode |> WHEN { Light => TEXT { #bd454d }, Dark => TEXT { #dc6463 } }
        depth: [container: 8, element: 6, detail: 2, hero: 10]
        elevation: [card: 50, editing_focus: 24, selection: 4, todo_item: 4]
        corners: [touch: 8, comfort: 8, filter: mode |> WHEN { Light => 999, Dark => 6 }, pill: Fully]
        sizing: [toggle_control: 60]
        spacing: [tight: 5, standard: 10, new_todo_input_inset: 6]
        spring: [
            button: [extend: 6, compress: 4]
            button_destructive: [extend: 4, compress: 6]
            checkbox: [extend: 4, compress: 8]
        ]
        geometry: [edge_radius: 2, bevel_angle: 45]
    ]
}

FUNCTION materials(mode) {
    BLOCK {
        surface_base: [
            ...Base/material()
            color: mode |> WHEN { Light => TEXT { #ffffff }, Dark => TEXT { #1b1f24 } }
        ]
        primary_subtle_base: [
            ...Base/material()
            color: mode |> WHEN { Light => TEXT { #fbf1f2 }, Dark => TEXT { #1d2127 } }
            border: mode |> WHEN { Light => TEXT { #f0d0d3 }, Dark => TEXT { #d65d5e } }
            border_width: 1
        ]
        [
            background: [...Base/material(), color: mode |> WHEN { Light => TEXT { #f8fafc }, Dark => TEXT { #16191e } }]
            surface: surface_base
            surface_variant: surface_base
            surface_elevated: [...Base/material(), color: mode |> WHEN { Light => TEXT { #ffffff }, Dark => TEXT { #1d2127 } }]
            panel: [
                ...surface_base
                border: mode |> WHEN { Light => TEXT { #d7dde6 }, Dark => TEXT { #3a414b } }
                border_width: 1
            ]
            interactive: [...surface_base, metal: 0.03]
            interactive_hovered: [...surface_base, gloss: 0.04, metal: 0.03]
            editing: [...primary_subtle_base, gloss: 0.02]
            filter_selected: primary_subtle_base
            hero_title: [...Base/material(), color: TEXT { #00000000 }]
        ]
    }
}

FUNCTION frames(mode) {
    BLOCK {
        divider: mode |> WHEN { Light => TEXT { #e5e8ee }, Dark => TEXT { #323840 } }
        [
            new_todo_row: [...Base/frame(), border_bottom: divider, border_bottom_width: 1]
            todo_row: [...Base/frame(), border_bottom: divider, border_bottom_width: 1]
            footer: [...Base/frame(), border_top: divider, border_top_width: 1]
        ]
    }
}
```
Fonts use `body_base: [family: family, size: 25, color: text, weight: Light, style: Normal]`. `body_disabled` is `[...body_base, color: ...]` and `placeholder` is `[...body_base, color: ..., style: Italic]`. `ButtonIcon[checked]` becomes the two fonts `icon` and `icon_checked`. `SmallLink` is built at the call site as `[...theme.font.small, line: [underline: hovered]]`. The checkbox is a 15-field record with `checkbox_cast_x: 0` added where it was missing.

Classic (210 lines) shows the unusual cases:
- Mixed colours stay as they are: `surface_color: mode |> WHEN { Light => Oklch[lightness: 1], Dark => TEXT { #202020 } }`.
- `filter_selected: [...Base/material(), border: primary, border_width: 1]`: no fill, colour None, same as today's missing colour.
- `panel: [...surface_base, border: None, border_width: 1]`: Classic's PanelFrame had shadows only.
- `editing: [...surface_base, border: primary, border_width: 1]`.
- Footer frame: `[...Base/frame(), border_top: divider, border_top_width: 0.8, shadows: LIST { five explicit Shadow records }]`.

Neobrutalism's `filter_selected` is `[...Base/material(), color: primary, border: None, border_width: 2]`. Today it has no border fields and relies on the hover outline defaulting to 2 px.

### 4.3 RUN.bn call sites
- Root: `theme: Theme/tokens(name: theme_options.name, mode: theme_options.mode)`.
- `main_scene` uses `lights: Theme/lights(name: PASSED.theme_options.name, mode: PASSED.theme_options.mode)` and `geometry: theme.geometry`.
- The scene PASS needs an alias only under the documented rule: `scene: BLOCK { app_store: store  app_theme_options: theme_options  main_scene(PASS: [store: app_store, theme_options: app_theme_options]) }`.

| before | after |
|---|---|
| `gap: Theme/spacing(of: None)` ×8 / `Section` / `Small` | `gap: 0` / `10` / `9` (the same in every theme once Theme.bn's overrides are applied, Theme.bn:58-66) |
| `Theme/spacing(of: Tight/Standard/NewTodoInputInset)` | `theme.spacing.tight/.standard/.new_todo_input_inset` |
| `theme_gap(of: SwitcherBottomPush/TitlePanelGap/Small)` | `theme_gap(gap_height: 0/0/9)` (parameter renamed: a field `height: height` would be a self-reference) |
| `Theme/sizing(of: TouchTarget)` ×3 / `ToggleControl` | `40` / `theme.sizing.toggle_control` |
| `Theme/depth(of: X)`, `elevation`, `corners`, `spring_range` | `theme.depth.detail`, `theme.elevation.card`, `theme.corners.pill`, `theme.spring.button` |
| `material: Theme/material(of: Surface)` + `...Theme/material(of: PanelFrame)` | `material: theme.material.panel` + `shadows: theme.panel_shadows` |
| `material: [...Theme/material(of: InteractiveRecessed[focus: True]) ...Theme/material(of: PrimarySubtle)]` | `material: theme.material.editing` |
| `Theme/material(of: Interactive[hovered: element.hovered])` ×4 | `interactive_material(hovered: element.hovered)` (a 4-line helper that picks `interactive` or `interactive_hovered`) |
| `...Theme/material(of: NewTodoRowFrame/TodoRowFrame/PanelFooterFrame)` | `...theme.frame.new_todo_row/.todo_row/.footer` (the heights and paddings were overridden and are dropped) |
| `...Theme/material(of: TodoListFrame)` | `scroll: True, scrollbars: False` (the same in every theme) |
| `...Theme/checkbox_material()` | `...theme.checkbox` |
| `Theme/material(of: Primary).color` ×3 | `theme.primary_color` |
| `delete_button_material(hovered)` | `theme.material.surface_elevated` (its glow was dropped by lower_material and never rendered) |
| `hero_title_material()` (theme-name logic inside RUN.bn, :309-318) | `theme.material.hero_title` (Glassmorphism: `[...surface_elevated, glass_highlight: 0.95, frosted_blur: 12, frosted_saturate: 1.35]`) |
| `filter_button_material(selected)` | `selected |> WHEN { True => theme.material.filter_selected, False => [] }` |
| `...Theme/text(of: Hero)` | `font: theme.font.hero, depth: 0, move: [closer: 0]` |
| `...Theme/text(of: ButtonIcon[checked: checked])` | `font: checked |> WHEN { True => theme.font.icon_checked, False => theme.font.icon }` |
| `Theme/font(of: X)` / `.color/.size/.weight` | `theme.font.body` / `theme.font.input.size` etc. |
| `small_flat_text(of: SmallLink[hovered: h])` | `small_flat_text(text_font: [...theme.font.small, line: [underline: h]])` |

new_todo:
- Append `[title: title_to_save, completed: False]`, and use `initial_completed: item.completed`.
- Under the documented rule, rename the parameter: `FUNCTION new_todo(initial_title, initial_completed)` with `title: LATEST { initial_title  title_to_update }`, mirroring `completed`. This also makes `edited_title` start from the current title, which is a small behaviour fix the render diff will show.

### 4.4 Why rendering stays identical
- **Neutral material fields** equal the renderer defaults (section 3).
  - `color: None` → background "None", which fails to parse → the per-kind default fill, the same as a missing colour.
  - `border: None` draws nothing.
  - The only style keys a material can overwrite are background, border and border_width. The panel was the only overwrite that mattered (Surface material + PanelFrame border) and is pre-merged into `material.panel`.
  - Filter and theme buttons put `outline` after `material`, so the outline still wins. Where the material had no border width, the width stays at the 2 px default: the unselected filter button's `[]` adds no key, and Neobrutalism uses `border_width: 2`.
- **Frames:** every themed side border sets its own width, so the frame's `border_width: 0` is never used as a fallback. `shadows: LIST {}` writes no keys.
- **Fonts:** `style: Normal` renders as Normal (render_scene.rs:3316-3328). It only changes the measure-cache key ("Normal" vs "normal"), which the readback A/B covers.
- **Pre-merged layers.** Each theme's `editing` material is the old merge written out explicitly: IR(focus True) then PrimarySubtle.
  - Professional: PrimarySubtle + gloss 0.02.
  - Glassmorphism: surface glass fields + PrimarySubtle colour, refraction and border.
  - Classic, Neobrutalism and Neumorphism: PrimarySubtle + gloss 0.
- **Dead fields dropped** (no StyleMap change except the unread `glow="None"`): glow and shadows inside materials (Professional/Glassmorphism/Neobrutalism/Neumorphism InteractiveRecessed and FilterSelected, Neumorphism surface_elevated and primary_subtle, the delete-button glow), BodyDanger's `align`, the HeroTitle and Danger arms, unused scale arms (Soft, Edge, Inset, Dialog, Lift, Button, Base, IconContainer, EditingInputWidth), and the overridden frame heights and paddings.
- **Unifications that might show,** both to be confirmed by the render diff:
  - Classic's ButtonIcon text carried `depth: 0` and the other themes did not. The chevron text has width and height Fill, so the +8 measurement bump should not affect layout.
  - Classic's Hero text used `move: [further: 0]` and the other themes `[closer: 0]`. These move keys are never read.

Result: the TodoMVC theme goes from 2,443 to about 1,110 lines, from about 20 record shapes to 8 per-role types, and has 3 intentional union types (colour `Oklch | TEXT | None`, `corners.pill: NUMBER | Fully`, font `weight: NUMBER | tags`).

## 5. NovyTheme refactor

There is only one theme here (mode only), so the tag API stays and each function returns one shape.
- `material(mode, of)` covers 10 borderless surfaces: AppBackground, DialogBackdrop, TimelineGlass, WaveformGlass, TraceHigh, TraceLow, TraceUnknown, TraceEdge, AnalogTrace, Warning. Each is `[...plain_material(), ...]`, with `plain_material` defined in NovyTheme.bn itself.
- The new `panel_material(mode, of)` covers the 7 bordered surfaces: PanelSurface, PanelInset, Dialog, SelectedRow, HoverControl, PressedControl, FocusedControl. They carry `border` and `border_width`. They must stay separate because the 8+ elements that use borderless materials keep a live style-level `border:` that a neutral `border: None` would overwrite (e.g. NovyView.bn:4243-4291 trace segments, :2010 WaveformGlass).
- 61 call sites rename to `NovyTheme/panel_material`; the two dynamic ones (1903-1909, 3275) pick between two bordered tags.

```boon
TraceHigh => [
    ...plain_material()
    color: mode |> WHEN { Dark => TEXT { #3b82f6 }, Light => TEXT { #2563eb } }
]
-- before: [ color: ...  glow: mode |> WHEN { Dark => [color: TEXT { #3b82f633 }, intensity: 0.04] ... } ]
-- the glow was inside material: and never rendered (runtime.rs:5529-5544)
```

- `font(mode, of)` keeps its 5 arms `[family, size, weight, color]`. The `__` arm is dropped because callers only pass Body, Mono, Muted, Heading and Title (NovyView.bn:5730 callers).
- Dead code deleted: `trace_material`, `trace_cell_material`, `trace_fill_color`, `trace_border_color` (NovyTheme.bn:359-470, no callers), the CursorGlow, MarkerChip and Error arms, and the shadows and glow fields.
- Optional cleanup, identical under the diff: remove NovyView's style-level `background:` next to themed materials (the material always overwrites it) and the `border:` next to `panel_material`.
- `text_line(label_text, font, width, height)` needs its parameters renamed under the documented rule.
- Result: 487 lines become about 330, and about 22 shapes become 3.

## 6. Verification plan

- **V0, static checks.** The new compiler compiles every manifest entry source and every test fixture with 0 errors. Each migrated pattern is kept as a negative fixture with its expected positioned diagnostic: `[x: x]` (per the owner's decision), the title self-loop, THEN on a HOLD, reading a possibly-missing field, reading a field on a variant union, the in-arm narrowing read, a union passed to `gap:`, and a theme that is missing a token.
- **V1, scenario specs.** Every manifest `.scn` runs through `boon_behavior_harness` (BehaviorHarness::new, lib.rs:587-707), which dispatches authored actions through retained hit-testing. Triage the 16 failing scenarios first, starting with the stale counter.scn source path, so each scenario is a true spec before it gates the new compiler.
- **V2, CPU render-identity diff (new tool).**
  - Run two plans through the harness on the same scenario and compare at every step: the layout display-list bounds, `render_visual_primitives(...)` (render_scene.rs:1873, ignoring `style_identity`, `dependency_set` and retained ids), and text. Report the first difference with its node path, reusing `first_behavior_difference` from crates/boon_behavior_harness/tests/artifact_oracle.rs.
  - StyleMap differences are reported as information, with an allowlist of unread keys.
  - Add `examples/todo_mvc_physical_theme_matrix.scn`: 5 themes × 2 modes × {initial, todo-row hover (`pointer_hover`), editing open, Active filter selected, completed row, theme-switcher hover}, about 60 checkpoints. Also add a NovyWave light/dark matrix.
  - This runs without a GPU and should take well under a minute.
- **V3, native GPU A/B.** Run the product gate profiles (todomvc-physical, novywave) for the baseline and the candidate. Compare the app-owned render-target readback PNGs at the same checkpoints pixel-exactly, using a small xtask compare over the two artifact dirs. This catches real text-measurement effects that V2's approximate measurer misses. Only AGENTS.md-sanctioned evidence is used: kernel-uinput on an isolated seat and app-owned readback.
- **V4, handoff.** Run the manifest gates, then `cargo xtask verify-all --check-existing`.
- **Baseline order** (the old pipeline cannot mount todo_mvc_physical or persons_pro):
  1. Apply the minimal semantic fixes on the old toolchain (append `completed`, title), then re-run `boon_cli run`.
  2. If it mounts, capture the V2 and V3 baselines, apply the Theme refactor, and diff. The old compiler accepts the refactored code (probes above), so the whole migration can land **before** the new compiler exists and gives it a fixed target corpus.
  3. If it still fails, use a theme-only probe program (Theme functions called over (theme, mode, role) into a root record) with V2 over that program.
  4. As a last resort, compare lowered StyleMaps computed from the evaluated records.

## 7. Size of the example work
- Tooling (V2 harness, readback A/B, matrix scenarios): M.
- TodoMVC semantic fixes: S.
- TodoMVC Theme refactor: M-L (about 3 days including the 10-combination verification).
- NovyTheme: S-M.
- Self-reference: S under rule C, M under the documented rule (110 sites; compiler fix-its make it mechanical).
- Payload patterns (68): M.
- THEN on continuous and variant reads (persons_pro, NovyWave): M.
- Scenario triage (16 of 21 failing): L, the largest uncertainty.
- Negative fixtures: S.
- Stale fjordpulse syntax and parameter renames: S.

Total about 2.5-4 engineer-weeks, independent of the compiler rewrite except that the builtins cells needs are a compiler dependency.

## owner_questions
- **question**: Inside a record or BLOCK field's initializer, what does the field's own name refer to? Keep the documented rule that `[x: x]` is a self-reference (LANGUAGE_SEMANTICS.md:216-219), or change it so a field's initializer sees its siblings but not itself, which makes `[x: x]` a copy of the outer x? | **recommendation**: Rule C (the initializer does not see its own name). The corpus was written assuming C in 110 places and relying on self-reference in only 3. C costs 0 edits and keeps the idioms readable. The 3 LATEST self-state examples switch to HOLD, the documented state primitive, and TodoMVC's `title: LATEST { title ... }` becomes correct as written. If you keep the documented rule, the migration is mechanical with compiler fix-its but permanent boilerplate: every `[x: x]` needs a BLOCK alias or a renamed parameter, and payload forwarding needs an alias because binders cannot be renamed. | **why**: This is the largest source of edits and decides the fix for TodoMVC title. Under the documented rule, idioms such as `element: [events: events]`, `padding: [top: top]`, `Oklch[... hue: hue]`, `Numeric[value: value]` (inside `Parsed[value] =>`) and `PASS: [store: store]` all become errors (110 sites). The alternative makes all of them copies. Its cost is that the LATEST self-state idiom stops working, because `count: LATEST { 0, increment |> THEN { count + 1 } }` would then look for an outer `count`. That affects counter_latest, interval_latest (the 'without HOLD' demos) and NovyWave value_format. | **options**: A: documented rule (self-reference; 110 edits; LATEST self-state keeps working if Q2 = yes). B / Rule C: own name hidden in its own initializer (0 edits; counter_latest, interval_latest and value_format rewritten with HOLD, and the two demo examples renamed or dropped).
- **question**: Does a LATEST that has a continuous (fallback) arm count as a state boundary for cycle checking, so that a read inside one of its event-gated arms (THEN body or event-WHEN arm) sees the committed value? | **recommendation**: Yes. Define an edge as not instantaneous when the read sits inside an event-gated arm of such a LATEST, or inside a HOLD body. The runtime already behaves this way; the counter_latest scenario passes today. | **why**: The spec lists only SOURCE, HOLD, publication and async effects as cycle breakers (LANGUAGE_SEMANTICS.md:219-222), and a THEN body reads the current tick (:348-352). Taken literally, the TodoMVC new_todo cycle (title → title_to_update → edited_title → title, RUN.bn:125-158) and counter_latest-style self-state are instantaneous cycles, while example comments (counter_latest.bn:5-6) say the initial arm makes the field stateful. | **options**: Yes (formal rule above) / No (TodoMVC new_todo must switch title and edited_title to HOLD like examples/todomvc.bn, and counter_latest must use HOLD).
- **question**: Confirm that decision 4 also removes refinement of the WHEN subject inside an arm, i.e. `x |> WHEN { WaveformOpened => x.format }` becomes an error and code must bind the payload with `WaveformOpened[format] => format`. | **recommendation**: Confirm it: one type per name in a function, as in Rust and Elm, and the language already has `Tag[field]` patterns. Migrate the 68 sites mechanically. | **why**: 68 sites depend on it (NovyWave RUN.bn 55, host_service_effects.bn 12, server_effect_chain.bn 1). You also cited TypeScript, which does narrow here. Keeping it would be cheap and local (refine an immutable path inside the arm), so it is worth a yes or no. | **options**: Drop, and migrate to payload patterns / keep path refinement inside arms (TypeScript-style, compositional, no per-call-site cost).
- **question**: Shadows and glow written inside `material:` records, and `family` inside font records spread straight into a style, never render today, because the runtime drops them (runtime.rs:5529-5544, 5412-5440). Should the refactor delete them, keeping pixels identical, or move them to where they would render, which is a visible change? | **recommendation**: Delete them in the refactor so rendering stays exactly identical. If you want them, file a separate visual change (for example Neumorphism's soft surface shadows, the delete-button hover glow, NovyWave trace glows, NovyWave font families). | **why**: You asked to keep rendering identical, but some of the authored design intent is invisible today, notably Neumorphism's signature surface shadows and NovyWave's Inter and JetBrains Mono families on 57 text styles. | **options**: Delete (identical) / move to style level (visible change, needs your review of screenshots).
- **question**: THEN on a continuous value (a HOLD or a LATEST with a fallback) is used as an 'on change' trigger in persons_pro (14 sites) and NovyWave (4 sites). Decision 6 makes `5 |> THEN {6}` an error. Confirm that no 'changes of' semantics is wanted, so these sites get restructured. | **recommendation**: Keep it an error. Restructure to trigger from the underlying event, or pattern-match the state (`WHEN { RegistrationSucceeded[...] => ...  __ => SKIP }`). | **why**: Giving THEN an 'on change' meaning would be a semantic change that contradicts decision 6. The restructuring needs a human look at the effect flow, not a codemod. | **options**: Error and migrate / define THEN on a continuous value as 'fires when the value changes' (semantic change).
- **question**: The migrated TodoMVC is about 40% smaller (2,443 → about 1,110 theme lines) and has no call-path fan-out, so the compiler budgets (TodoMVC 75/300 ms, NovyWave 250/1000 ms) get easier because the fixtures changed. How should budgets and fixtures be handled? | **recommendation**: Re-baseline against the migrated examples, and add a synthetic stress fixture (for example a generated NovyWave-sized program with deep helper nesting) so the speed goal is not met just by simplifying examples. | **why**: Otherwise the speed goal could be met partly by simplifying the example rather than by making the compiler faster. | **options**: Re-baseline and add a stress fixture / keep the old line counts as the size target / budget per 1k lines.
- **question**: Do LIST literal elements join like WHEN arms (union element type), with builtin element constructors returning one opaque Element type? The TYPE_INFERENCE plan lists 'heterogeneous collection shape' as a negative fixture (plan:477). | **recommendation**: Yes: LIST elements join like WHEN, `Element/*` and `Scene/Element/*` return one opaque Element type, and the heterogeneous-collection fixture applies only to MAP/SET keys. | **why**: NovyWave has 94 and TodoMVC 16 LIST literals mixing element kinds, and the lights lists mix three `Light/*` builtins. Rejecting these would be absurd, and typing them as record unions would be pointless. | **options**: Union join plus opaque Element / reject heterogeneous LIST literals.
- **question**: Should unused function parameters be an error or a warning? | **recommendation**: A warning. | **why**: About 12 example sites have unused parameters. Decision 6 does not list this rule explicitly. | **options**: error / warning / off

## risks
- **risk**: There is no working rendering baseline: the old pipeline fails before the first scenario step on todo_mvc_physical and persons_pro, and fails mid-scenario on NovyWave, so the Theme refactor cannot be diffed directly against today's output. | **mitigation**: 1) Apply the two semantic fixes (append `completed`, title) on the old toolchain and re-test the mount. 2) If it still fails, build a theme-only probe program that evaluates old and new theme values over (theme, mode, role), and run V2 on it. 3) Last resort: compare lowered StyleMaps computed from the evaluated records using the runtime's lowering functions.
- **risk**: 'Neutral' values are not neutral in every context, because the runtime depends on key presence: default fill per element kind, a 2 px default border, material keys overwriting style keys in alphabetical order, `depth` on Text adding 8 to the measured width, and `scroll*` keys changing retained structure. | **mitigation**: Use neutral records only in the role positions analysed in design §3–4. Keep bordered and borderless materials separate (NovyWave). Pre-merge the layered materials. Comment each neutral value with the renderer default it matches. Gate every change with the V2 primitive diff over the 60-checkpoint theme matrix.
- **risk**: Changes in text measurement strings (`style: Normal` vs a missing style, numeric weights keyed as 'normal' in boon_native_gpu lib.rs:434) could shift layout with the real glyph measurer while V2's approximate measurer shows no difference. | **mitigation**: Run the V3 readback A/B pixel-exactly for the TodoMVC theme matrix and NovyWave. Keep Neobrutalism's numeric weights as they are.
- **risk**: The old backend cannot lower `Light/*` calls inside a root record (probe error: `Light/directional` has no typed PlanExecutor row operation), so a 'tokens includes lights' design would not compile on the toolchain used for the early migration. | **mitigation**: Keep lights as a per-theme `lights(mode)` function plus a Theme.bn dispatcher called from main_scene. It can be folded into tokens once the new backend handles it.
- **risk**: Scenario drift: 16 of 21 manifest scenarios fail today and some are stale (the counter.scn source path), so a failing scenario may be a spec bug, a runtime bug or a compiler bug. | **mitigation**: Triage every scenario before it gates the new compiler (work item EX9). Record known runtime failures separately from compiler acceptance.
- **risk**: The owner's answers change the volume of work by an order of magnitude (self-reference rule: 0 vs 110 edits; in-arm narrowing: 0 vs 68 edits). | **mitigation**: Get the self-reference, LATEST-boundary and in-arm narrowing answers before starting. Implement fixes as compiler fix-its where possible so either answer is mechanical.
- **risk**: The migration simplifies the benchmark fixtures, so the compiler speed goal could be met by simplifying examples rather than by a faster compiler. | **mitigation**: Re-baseline explicitly and add a synthetic stress fixture (owner question on budgets).
- **risk**: The pre-merged editing material or the `interactive_hovered` split could get one theme's values wrong in transcription, because the old cascade semantics are easy to misread. | **mitigation**: Derive each merged value mechanically: the later spread wins, and fields missing in both come from Base. Check with V2 on the 'editing open' and 'hover' checkpoints for all 10 theme and mode combinations.

## work_items
- **id**: EX1 | **title**: Render-identity diff tool and readback A/B compare | **description**: Build a CPU tool on boon_behavior_harness. It runs two MachinePlans on the same scenario and compares, at every step, the layout display-list bounds, render_visual_primitives (ignoring style_identity, dependency_set and retained ids) and text, and reports the first difference with its node path, reusing first_behavior_difference from artifact_oracle.rs. StyleMap differences are reported against an allowlist of unread keys (material_color, glow, move_*, spring_range_*, background=None). Also add an xtask that compares two native-gate artifact dirs of app-owned readback PNGs pixel-exactly. | **size**: M | **acceptance**: Two identical plans report 0 differences. A deliberately changed colour or gloss in one theme is reported with its node path. The TodoMVC matrix runs in under 60 s on CPU. The readback compare flags a single-pixel change.
- **id**: EX2 | **title**: Theme and mode matrix scenarios | **description**: Add examples/todo_mvc_physical_theme_matrix.scn: 5 themes × 2 modes × {initial, todo-row pointer_hover, editing open, Active filter selected, completed row, theme-switcher hover}. Add a NovyWave light/dark matrix scenario. Use only authored actions the harness supports (click, pointer_hover, double_click, key_down). | **size**: S | **acceptance**: Both scenarios pass semantic assertions on the baseline pipeline, or on the theme-only probe, and give about 60 and about 10 checkpoints.
- **id**: EX3 | **title**: TodoMVC semantic fixes and baseline capture | **description**: Append `[title: title_to_save, completed: False]` and pass `item.completed` (RUN.bn:60-63). Resolve title per owner questions 1 and 2. Add scene PASS aliases if the documented self-reference rule is kept. Re-run boon_cli run on todo_mvc_physical. If it mounts, capture the V2 and V3 baselines over EX2. Otherwise build the theme-only probe program as the baseline. | **size**: S | **depends_on**: EX1, EX2, owner questions 1 and 2 | **acceptance**: A baseline artifact exists (primitives per checkpoint and readback PNGs) for all 10 theme and mode combinations, or for the probe program. The todo_mvc_physical.scn result is recorded.
- **id**: EX4 | **title**: TodoMVC Theme refactor (themes as data) | **description**: Replace Theme.bn with tokens(name, mode) and lights(name, mode). Add Base.bn with the neutral material and frame records. Rewrite the 5 themes as tokens(mode) plus lights(mode), using the drafts in design/migration/todo_theme_new. Add root `theme`. Rewrite the 94 call sites per the mapping table, delete the get/shared_material/frame_material wrappers and the dead arms and fields, and pre-merge `editing` and `panel`. | **size**: L | **depends_on**: EX3 | **acceptance**: V2 shows 0 primitive differences over the theme matrix (any intentional difference must be listed in the change's description and accepted). V3 readback A/B shows 0 differing pixels at every checkpoint. The theme totals about 1,100 lines and at most 8 record types. The old compiler and the new compiler both accept it.
- **id**: EX5 | **title**: NovyTheme refactor | **description**: Split into material(mode, of) (10 borderless surfaces) and panel_material(mode, of) (7 bordered), both built as `[...plain_material(), ...]`. Keep font with 5 arms and no `__`. Delete trace_material, trace_cell_material, trace_fill_color and trace_border_color, the unused arms, and the glow and shadows fields. Rename 61 call sites. Rename text_line's width/height parameters if the documented rule is kept. Optionally remove dead style-level background and border next to themed materials. | **size**: M | **depends_on**: EX1 | **acceptance**: V2 shows 0 primitive differences on NovyWave's initial and light/dark checkpoints (plus any passing scenario steps). V3 readback shows 0 pixel differences. NovyTheme is about 330 lines.
- **id**: EX6 | **title**: Self-reference migration | **description**: Apply owner question 1 across the corpus. Under Rule C: rewrite counter_latest, interval_latest and NovyWave value_format with HOLD (renaming or retiring the 'without HOLD' demos). Under the documented rule: add BLOCK aliases or rename parameters at 110 sites (events 25, of 10, hovered 9, ...), preferably via compiler fix-its. | **size**: M | **depends_on**: Owner question 1 (and the new compiler's diagnostics if fix-its are used) | **acceptance**: The new compiler reports 0 self-reference and cycle diagnostics on the corpus, and the affected scenarios still pass.
- **id**: EX7 | **title**: Payload-pattern migration for in-arm narrowing | **description**: Rewrite the 68 bare-tag arms that read payload fields through the WHEN subject into `Tag[field, ...]` patterns: NovyWave RUN.bn 55, host_service_effects.bn 12, server_effect_chain.bn 1. Use design/migration/arm_narrow.py to list them. | **size**: M | **depends_on**: Owner question 3 | **acceptance**: arm_narrow.py reports 0 sites. NovyWave scenario steps that passed before still pass, and V2 is unchanged.
- **id**: EX8 | **title**: Replace THEN on continuous values and variant field reads | **description**: persons_pro: 14 THENs on HOLD passkey states and 5 `registration_succeeded.*` reads (RUN.bn:139-160, 252-275) become pattern matches or event-driven triggers. NovyWave: RUN.bn:109, 166, 247, 353 get the same treatment. | **size**: M | **depends_on**: Owner question 5 | **acceptance**: The new compiler reports no THEN-on-continuous or field-on-union errors, and the persons_pro and NovyWave scenarios behave as before (or as the triaged spec requires).
- **id**: EX9 | **title**: Scenario triage | **description**: Make every manifest .scn an accurate behaviour spec: fix stale source paths (counter.scn:12 and others), and record which failures are runtime bugs and which are compiler bugs. Today only 5 of 21 manifest scenarios pass with boon_cli run. | **size**: L | **acceptance**: Every manifest scenario is marked spec-correct. Failures in the triage log are attributed to runtime, compiler or example, and none are unexplained.
- **id**: EX10 | **title**: Negative fixture corpus from the migration | **description**: Before editing, keep one minimal fixture per migrated pattern with its expected positioned diagnostic: self-reference (per question 1), the continuous title self-loop, THEN on a HOLD, reading a field that may be missing, reading a field on a variant union, the in-arm narrowing read, a union passed to `gap:`, and a theme missing a token. | **size**: S | **acceptance**: About 10 fixtures, each producing exactly its expected diagnostic, with the right span, from the new compiler.
- **id**: EX11 | **title**: Small mechanical example fixes | **description**: fjordpulse `Server.value.field` → `Server/value.field` (FjordPulseView.bn:1957). Parameter renames where a field would copy a parameter of the same name (theme_gap, small_flat_text, text_line, layers layer_card). Unused parameters per owner question 8. | **size**: S | **acceptance**: Every manifest entry parses, and no self-reference or unused-parameter diagnostic remains unless it is intentional.
- **id**: EX12 | **title**: Corpus gate for the new compiler | **description**: CI job: the new compiler checks every manifest entry and test example (0 errors); the behaviour harness runs every manifest scenario; the native product gates (counter-dev, todomvc-physical, cells, novywave, persons-pro) and verify-all run per the handoff manifest. | **size**: M | **depends_on**: EX4-EX11 and compiler builtin coverage (Dependency/catch_cycle, List/range, Text/find, Timer/interval, List/take, Text/to_bytes) | **acceptance**: All green. verify-all --check-existing passes.

## deletions
- **what**: TodoMVC Theme.bn: get, shared_material, frame_material, theme_sizing, theme_spacing and the wrapper functions (replaced by tokens(name, mode) and lights(name, mode)) | **size**: ~155 lines
- **what**: The get(request) dispatcher, text(), HeroTitle/Danger arms, dead glow and shadows fields, unused scale arms (Soft, Edge, Inset, Dialog, Lift, Button, Base, IconContainer, EditingInputWidth) and overridden values in the 5 TodoMVC theme files | **size**: 2,275 → ~1,050 lines across the 5 files
- **what**: RUN.bn helpers delete_button_material, filter_button_material, hero_title_material (theme-name logic moved into theme data); theme_switcher_button_material and clear_button_font simplified | **size**: ~40 lines
- **what**: NovyTheme trace_material, trace_cell_material, trace_fill_color, trace_border_color (no callers); CursorGlow, MarkerChip, Error and `__` arms; glow and shadows fields inside materials | **size**: ~150 lines
- **what**: Optional: dead style-level background next to NovyWave themed materials, and border next to panel_material (the material always overwrites them) | **size**: ~100 lines in NovyView.bn

## evidence
- examples/todo_mvc_physical/Theme/Professional.bn:5-19 (get dispatcher), 358-369 (corners returns NUMBER or Fully), 371-398 (lights LIST); Theme.bn:48-66 (sizing/spacing overrides that made per-theme values dead), 87-95 (get over 5 themes), 97-169 (shared frames)
- examples/todo_mvc_physical/RUN.bn:60-63 (appended row lacks completed; `__ => False` on a missing field), 132-135 (title self-loop), 125-158 (sibling cycle through THEN arms), 309-318 (theme-name logic in RUN), 325 (PASS [store: store, theme_options: theme_options]), 464-465 (Surface material + PanelFrame spread), 648-649 (material cascade), 827-831 (outline + material)
- RUN.bn Theme call counts: material 24, spacing 16 (None ×8), font 16, depth 8, spring_range 7, corners 7, elevation 6, sizing 4, text 3, lights/geometry/checkbox 1 each = 94; gap typed NUMBER at boon_typecheck lib.rs:32013; style typed as open object at lib.rs:31960/41655
- crates/boon_document/src/runtime.rs:5322-5400 lower_style_record iterates a BTreeMap (alphabetical, last writer wins); 5529-5544 lower_material keeps only scalar fields; 5412-5440 lower_font (family→font, align ignored); 5546-5570 lower_shadows; 5572-5589 lower_glow; 5602-5616 scalar_style_value (tags become their name as text); 3880-3893 scroll keys change structure by presence
- crates/boon_document/src/render_scene.rs:1950-1953 and 3555-3575 fill fallback to per-kind default; 2066-2100 border width default 2.0 and side fallback; 2521-2590 and 3577-3616 material defaults (gloss/metal/refraction/transparency/frosted_blur 0, frosted_saturate 1, highlight colour white); 2365-2480 checkbox defaults; 3316-3338 font style and weight parsing
- crates/boon_document/src/lib.rs:5653-5658 a depth key on a Text node adds 8 to its measured width; no reader exists anywhere for material_color, glow, move_*, spring_range_*, or Scene lights/geometry (grep)
- examples/novywave/Theme/NovyTheme.bn:3-303 material (~22 arms, glow/shadows fields dead inside material:), 305-357 font (`__` arm lacks family/weight), 359-470 trace_* functions with no callers; style_collide.py: 61 of 65 NovyTheme-material styles also set background/border
- examples/novywave/RUN.bn:68-77, 507-519, 2257-2317 in-arm narrowing; 106-109, 164-170, 247, 353 THEN on continuous; 3019-3022 value_format LATEST self-state; design/migration/arm_narrow.py: 68 sites (novywave 55, host_service_effects 12, server_effect_chain 1)
- examples/persons_pro/RUN.bn:48-60 (HOLD over DevelopmentPasskey/register), 139-147, 252-268 (THEN on HOLD plus field reads); crates/boon_effect_schema/src/lib.rs:1000-1039 (the result is a 4-variant union)
- examples/counter_latest.bn:5-11 and interval_latest.bn:4-9 (LATEST self-state idiom documented in comments); LANGUAGE_SEMANTICS.md:216-222 (shadowing and cycle rule), 338-352 (THEN reads the current tick); TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:185, 465-490
- design/selfref2.py over examples/: 110 copies, 18 'direct' (only TodoMVC title real; NovyBridge and novywave:4407 false positives), 3 gated; top fields events 25, of 10, hovered 9, width 8, store 6
- boon_cli check over manifest sources: 8 of 22 fail today (cells, fibonacci, fjordpulse ×3, flush_error_propagation, interval_hold, interval_latest, kavik_cz, layers); cells fails on the missing compact ABI slice for Dependency/catch_cycle, List/range, Text/find
- boon_cli run --scenario over 21 manifest examples: 5 pass (minimal, hello_world, counter_latest, flow_operators, pages); todo_mvc_physical and persons_pro fail before the first scenario step; counter.scn:12 names store.sources.increment_button.press but the source is ...events.press
- Drafted refactor: /tmp/claude-1000/-home-martinkavik-repos-boon-circuit/76a4caad-9c38-4c5a-9114-58de96a02de9/scratchpad/design/migration/todo_theme_new/{Base,Theme,Professional,Classic}.bn; single-file probes two_theme_probe.bn and prof_probe.bn → 'pass: MachinePlan 11.0'; including Light/* inside the root tokens record failed lowering in the old backend
- IR census (design/mig_unions.py, migration/hold_kinds.py, mig_flow.py): TodoMVC when:mixed-kind 50 (Tag|Text 19, Object|Tag 18, List|Number|Object|Union 5, Number|Tag 4), latest:mixed-kind only store.todos.title/edited_title; persons_pro THEN on continuous 14; NovyWave 4 named plus 3 likely false positives
- crates/boon_behavior_harness/src/lib.rs:587-707 (BehaviorHarness mounts a MachinePlan and drives retained hits); crates/boon_behavior_harness/tests/artifact_oracle.rs (BehaviorFrame capture and first_behavior_difference, reusable for the V2 diff)

## perf_targets
- **metric**: TodoMVC theme source size and number of record types | **target**: ≤ 1,150 lines (from 2,443) and ≤ 8 per-role record types (from ~20+); 0 WHEN dispatches on static request tags | **basis**: Drafts: Professional 203 and Classic 210 lines; Theme.bn 25; Base.bn 31
- **metric**: Theme type-checking work on TodoMVC (new compiler) | **target**: The 5 tokens functions and 5 lights functions are checked once each; the 94 former Theme calls become field reads with no call instances | **basis**: Today the Theme dispatch drives 10,537 compiled call sites for 350 checked calls (C10); themes as data remove the dispatch
- **metric**: Render identity of the refactor (CPU) | **target**: 0 differing render primitives or layout rects across 5 themes × 2 modes × 6 states (~60 checkpoints) | **basis**: Owner requirement to keep rendering identical; V2 tool
- **metric**: Render identity of the refactor (GPU readback) | **target**: 0 differing pixels at every A/B checkpoint (TodoMVC matrix, NovyWave light/dark) | **basis**: Covers real glyph measurement; AGENTS.md app-owned readback evidence
- **metric**: Corpus acceptance | **target**: 0 errors on every manifest entry and test example; 21/21 manifest scenarios spec-correct and passing (from 5/21) | **basis**: boon_cli run baseline today
- **metric**: V2 diff runtime | **target**: < 60 s for the TodoMVC theme matrix on CPU, cheap enough to run on every example change | **basis**: The harness is CPU-only with an approximate text measurer

---

# Adversarial review

## area
examples (example migration, TodoMVC/NovyWave Theme refactor, example verification)

## verdict
needs_changes

## major_issues
- **issue**: The owner-question 1 recommendation (Rule C: a field's initializer cannot see its own name) goes against binding decision 6, which lists 'field self-reference' as an enforced positioned diagnostic. Rule C makes that diagnostic impossible. It also goes against an earlier owner-approved plan that spells out this exact rule and how to migrate it. The design also misreads what today's compiler does: it implements a hybrid, 'use the outer binding if one exists, otherwise the field itself', so old-compiler acceptance never shows that code follows either rule. | **evidence**: decisions.md item 6. BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md:470-491 (`[item: item]` is self-referential; use an outer alias) and :1127-1133 (parser-aware Rust codemod for 'explicit outer aliases', covering examples, tests, embedded Boon source and persistence migrations; 'Do not use regex rewriting or Python'). Probes run with target/release/boon_cli from the scratchpad: p5 `[other: n, n: 100]` gives other=100, so sibling fields are visible. p6 `FUNCTION pick(n) { [n: LATEST { n, press |> THEN { n + 1 } }] }` gives 5, 6, 6, so own-name resolves to the parameter. counter_latest (no outer `count`) resolves to itself. | **fix**: Plan and estimate under the documented rule (A). Keep Q1 as an optional semantic-change question, and state that it conflicts with decision 6. Drive the migration with compiler fix-its or the parser-aware codemod the OUT plan prescribes, not selfref2.py. Prefer passing the whole value through a call entry over BLOCK aliases. Call entries are not record fields, so `counter_button(element: [events: ...])` followed by `Element/button(element: element)` removes most of the 25 `[events: events]` sites (counter.bn:136, migrations, todo) without alias boilerplate. Add the hybrid resolution rule to the differential-oracle caveats.
- **issue**: Stateful LATEST is an undocumented semantic the examples rely on well beyond the 3 'self-state' sites. The docs say a LATEST with a continuous fallback is continuous, not stateful. Under the documented rules, `title: LATEST { initial_title, title_to_update }` falls back to the initial title on the next tick after an edit. The same goes for `completed`, `draft_title`, NovyWave value_format and real_hierarchy_page_result. Q2 frames this only as a cycle-breaking question. | **evidence**: TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:185 ('continuous if it has a continuous fallback'). LANGUAGE_SEMANTICS.md:386-405 (the sole present branch wins, and constants have no event sequence). counter_latest.bn:5-6 comment. `boon_cli run examples/counter_latest.bn` reports '1 state value(s)'. The corpus has 341 `LATEST {` blocks, 103 of them in novywave/RUN.bn. todo_mvc_physical/RUN.bn:132-158. | **fix**: Add an explicit owner question: 'Is a LATEST with a non-event arm a state cell (an implicit HOLD whose non-event arms only initialise it)?' Then derive the cycle rule (Q2) from the answer. If the answer is no, add a work item that converts every such LATEST to HOLD (TodoMVC new_todo, NovyWave, persons_pro, fjordpulse), and re-size it.
- **issue**: The baseline conclusions come from a binary of unknown provenance and from a runner that lacks product host services, and the proposed cure cannot work. target/release/boon_cli was built at 20:10:22, after the uncommitted kernel edits at 20:07 (owner.rs, solver.rs, term.rs). persons_pro fails in boon_cli because the child-program compile lane is missing, not because the example or compiler is broken. TodoMVC fails at mount even with an empty scenario, before any row is appended, so appending `completed` cannot fix it. The title rename is behaviour-neutral on the old compiler, which already binds `title` to the parameter (p6). | **evidence**: `ls --time-style=full-iso` on the binary and the modified kernel files. persons_pro error: 'value target Field(FieldId(25)) (store.draft_compile_line) is not current'. The field depends on `elements.draft_program.compiled` (persons_pro/RUN.bn:174-206). An empty .scn for todo_mvc_physical still fails with 'row 0:1:1 has no field 34'. boon_runtime LiveRuntime::from_project is what boon_cli run uses (boon_cli/src/lib.rs:96-116). | **fix**: Pin a baseline oracle: a clean build of a known-good commit in a separate worktree. Decision 1 allows the old compiler as a test-only oracle. Bisect for the last commit where todo_mvc_physical mounts. Run baselines through PersistentRuntime/native test playback with host services, not boon_cli. Make the theme-only probe the primary identity check instead of the fallback.
- **issue**: The design overstates what the behaviour harness can do, so V1, V2 and EX2 cannot run as written. BehaviorHarness::dispatch_scenario_step accepts only authored `click` actions. It rejects any frame with an embedded Program node, which rules out persons_pro. It has no migration-lifecycle support. The manifest scenarios use type_text 18×, key_down 23×, button_press 15×, double_click 5×, focused_chord/focused_key 6× and pointer_hover once. EX2 lists pointer_hover, double_click and key_down as 'supported'. | **evidence**: crates/boon_behavior_harness/src/lib.rs:704 (`!= Some("click")` returns an error), :651 and :1201-1212 (reject_embedded_programs). Migration scenarios run through boon_host_runtime/src/migration_scenario.rs, not the harness. | **fix**: Size harness extension as its own item (hover, double click, key/text input, focus, embedded programs), L rather than M inside EX1. Or avoid interaction for theme identity: render one static probe document containing every (theme, mode, role, hovered/selected/focus) combination, with theme functions called with explicit state, and diff CPU primitives. Keep interaction A/B only as a smoke test. Add the migration-scenario runner to V1 for todo_migration, counter_migration and the persons_pro migration.
- **issue**: The V3 native A/B assumes checkpoints the gates do not have and ignores existing limits. The gate profiles only sample hover or click. A roughly 60-checkpoint matrix exceeds the native verifier's 32-checkpoint cap and probably the 64-step playback limit. Pixel-exact comparison needs quiescent frames, and hover checkpoints may land mid-transition. | **evidence**: native_gpu_handoff_manifest.json: todomvc-physical and novywave use --visible-mode hover only. verify.rs:364 ('required checkpoint count exceeds 32'). preview.rs:61 TEST_STEP_LIMIT = 64. verify.rs:366-400 already supports kernel-uinput-isolated-seat workflows with app-owned-render-target-readback per scenario step. | **fix**: Build V3 on the existing native-workflow and `--required-checkpoint` readback path instead of a new tool. Split the matrix into one run per theme (at most 32 checkpoints and 64 steps each). Require a settled or idle frame before each capture, and record GPU/driver identity with both runs.
- **issue**: The design misses how the persons-pro gate couples to the scenario, and its suggested THEN-on-HOLD fix changes behaviour. The gate replays all of examples/persons_pro.scn with semantic_assertions:true. It reads steps valid-edit-preview and corrected-edit-preview as type_text into store.elements.source_editor, and it budgets child-program compiles. Rewriting `passkey.registration_succeeded |> THEN {...}` as `WHEN { RegistrationSucceeded[...] => ...  __ => SKIP }` makes it continuous: inside workspace_grant_state's HOLD it would re-assert PendingRevocation every tick and override `authentication_succeeded |> THEN { AccountOwned }`, and `List/append(item: registered_credential)` would get a continuous value. The steps first-passkey-protects, second-passkey-same-account and duplicate-credential-is-rejected exercise exactly these sites. | **evidence**: Manifest persons-pro proof_requirements: scenario examples/persons_pro.scn with semantic_assertions true, and budget metrics bounded-starter-source-compile-p95/max and valid-edit-to-preview-visible-p95/p99. verify.rs:2005-2040. persons_pro/RUN.bn:48-60, 139-147, 252-268. persons_pro.scn:139-172. | **fix**: In EX8, move each effect completion into its own event field (for example `registration_result: register_passkey |> THEN { DevelopmentPasskey/register(...) }`) that both the HOLD and the dependants consume. Apply the same to NovyWave RUN.bn:109, 166, 247 and 353. Freeze the step ids and source paths the gate consumes. Add persons_pro's embedded sources (Templates/Profile.bn starter_source, ProfilePage.bn library_source), which the new compiler compiles at runtime, and the diagnostic path/line/column contract (RUN.bn:182-206) to the corpus.
- **issue**: The design misses that the counter-dev gate depends on counter_migration. With --switch-samples 23 the gate alternately clicks dev.next and dev.previous from counter. Manifest order puts counter_migration next, so every switch compiles and mounts the counter migration stages v1-v3. The survey table says the migrations have no gate. The persons-pro gate also compiles persons_pro migration stages v1/v2 and Support.bn on launch, which the survey does not cover. | **evidence**: verify.rs:1820-1866 (dev.next/dev.previous loop); dev.rs:641-646 and 1654-1667 (adjacent_id over manifest order); examples/manifest.toml:580 (counter) and :617 (counter_migration); examples/migrations/persons_pro/sequence.toml (launch_stage v3, stages v1/v2 with Support.bn). | **fix**: Mark counter_migration and the persons_pro migration stages as gate-relevant in the survey and in EX12. Apply `[events: events]` fixes identically across stages so persisted field paths and the migration.scn carry-over stay stable.
- **issue**: The fjordpulse 'stale syntax' finding is wrong. FjordPulseView.bn:1957 column 291 falls inside a TEXT literal (`... owned by the Boon Server. }`). The old parser is scanning TEXT contents for role qualification. Editing the example would work around an engine bug, which AGENTS.md forbids. | **evidence**: `sed -n 1957p examples/fjordpulse/View/FjordPulseView.bn | cut -c270-300` prints 're owned by the Boon Server. })'. boon_cli check reports 'qualified role values use `Server/value.field` ... at line 1957, column 291'. | **fix**: Remove this item from EX11. Add a negative-free frontend fixture: TEXT content is never tokenized as code. The new parser must accept this file unchanged.
- **issue**: The breakage census is a lower bound and partly unmeasured. The IR-based categories (THEN on continuous values, LATEST/HOLD kind mixes, mixed-kind joins) exist only for examples the old compiler can lower. There is no IR for cells (a product gate), the 3 fjordpulse roles (distributed), kavik_cz, layers, fibonacci, interval_* or flush_error_propagation. cells' 'only 6 [x: x]' is therefore not a full list. arm_narrow.py only matches a bare-path subject read textually as `P.field`. It misses aliases, call-result subjects and field reads outside WHEN. EX6 and EX7 acceptance is a Python scanner reporting 0. | **evidence**: scratchpad design/mig_ir contains only .log (no .json) for cells, fjordpulse_*, kavik_cz, layers, fibonacci, interval_* and flush_error_propagation. design/migration/arm_narrow.py:10-30. | **fix**: Treat the regex counts as estimates. The authoritative census and every acceptance is 'the new compiler reports 0 diagnostics on every manifest source, stage and embedded source'. Put a 'census with the new checker' milestone before sizing EX6-EX8.
- **issue**: Rule categories are missing, and a cross-area schema dependency is unstated. (a) 'Collection inside HOLD' is a documented negative fixture, and NovyWave keeps list-carrying effect results in a stateful LATEST/HOLD (real_hierarchy_page_result holds HierarchyPage[rows: LIST, signal_ids: LIST]). (b) The plan types style ('Wrong style field type'), yet the design assumes style stays an open record that accepts `color: None`, `outline: [side, color] | NoOutline`, `weight: NUMBER | tag`, `material: Material | []`, spread checkbox_* keys and so on. The neutral values and the 'unions flow into open style' argument both depend on this. | **evidence**: TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md:475 and :479. novywave/RUN.bn:80-91. boon_effect_schema/src/lib.rs:490-520. todo_mvc_physical/RUN.bn:827-833. | **fix**: Add both as owner or typecheck-area questions. Write down the exact style/material schema the migrated examples need: which fields accept None or tags, and which unions are allowed. Assess collection-in-HOLD sites across the corpus before sizing the work.
- **issue**: The 'delete unread data' rule is applied inconsistently, and it includes an old-backend workaround. The design deletes glow, material shadows and font `family` because they never render. It keeps Scene `lights` and `geometry`, which have no reader anywhere in boon_document or boon_native_gpu, plus spring_range_*, move_* and the elevation tokens, which only feed move. It keeps lights as a separate per-theme function only because the old backend cannot lower `Light/*` inside a record, although the old compiler is deleted at cutover (decision 1) and AGENTS.md forbids Boon-level workarounds for engine limits. EX4 also requires the old compiler to accept the final corpus. | **evidence**: grep finds no `lights`, `geometry`, `move_closer` or `spring_range_` reader in boon_document, boon_native_gpu, boon_plan_executor or boon_runtime. document_executable_backend.rs:3886-3888 only builds the Light builtins. EX4 acceptance: 'The old compiler and the new compiler both accept it'. | **fix**: Choose one rule and ask the owner: keep all authored data the Scene API defines (the physical-renderer intent of a 'physical' showcase), or drop all unread data. Put lights in `tokens` and require the new backend to lower it. Drop old-compiler acceptance for the final corpus and use the old compiler only to capture the baseline.
- **issue**: The design contains factual errors that would skew the V2 expectations. (1) 'Renaming makes edited_title start from the current title, a small behaviour fix': inside edited_title's THEN, `title` already resolves to the sibling field on the old compiler (sibling capture, p5), so the change is behaviour-neutral. (2) 'The type system checks theme completeness for free / 5 identical record types': under decision 5 a missing token produces a union and errors only where it is read in RUN.bn (unread tokens are never flagged). Field types also differ per theme: Classic uses `Oklch | TEXT` with differing Oklch payload shapes (`Oklch[lightness]` versus `Oklch[lightness, chroma, hue, alpha]`, Classic.bn:22-60), while Base uses `None`. | **evidence**: todo_mvc_physical/RUN.bn:144-156, Classic.bn:22-60, scratchpad probes p5/p6. | **fix**: Correct both statements. If completeness should be checked, add an explicit check, such as a type annotation or a shared Theme record contract that each theme must satisfy. Say whether record joins merge field by field or produce unions of records, because that decides whether '8 per-role types' holds.

## missing
- Migration-scenario coverage in V1: todo_migration, counter_migration and persons_pro/migration.scn run through MigrationScenario (boon_host_runtime/src/migration_scenario.rs), not the behaviour harness
- Embedded Boon sources compiled at runtime by the new compiler (persons_pro Templates/Profile.bn starter_source, ProfilePage.bn library_source, migration v1/v2 strings), plus the child-program compile contract: compiled.bootstrap, artifact_id, and diagnostic path/line/column read at persons_pro/RUN.bn:174-230
- Non-manifest runtime-contract fixtures are not surveyed: server_http_echo, server_websocket_echo, server_persistent_counter, server_outputs, outbound_http_effect and persistence_fjordpulse_fixture (used by boon_server_runtime, boon_http_runtime and boon_host_runtime tests); about 32 bytes_*_plan_ops fixtures with .scn; examples/language_surface; testdata/phase0. Each needs a keep, migrate or delete decision
- Distributed example (fjordpulse Client/Session/Server) semantic breakage is unassessed. It cannot be lowered today, and role-path, OUT-producer and distributed-cycle rules were not checked
- Persistence identity: LATEST-to-HOLD rewrites and field or parameter renames inside migration stages must keep persisted field paths identical across stages, and the design needs a check for that
- Freezing .scn step ids and source paths that gates consume (persons_pro.scn valid-edit-preview and corrected-edit-preview; source store.elements.source_editor) during scenario triage (EX9)
- Assessment of OUT producer rules, unused pattern binders (for example cells formula.bn `InvalidNumber[reason, position]`) and effect calls made from continuous WHEN arms (novywave/RUN.bn:80-91 calls Wellen/hierarchy_page from a continuous selection)
- Baseline provenance: which commit or binary is the oracle, recorded with the artifacts
- Record-join semantics under decision 5 (field-wise merge or union of records), which decides whether the theme 'one type per token' claim holds
- A statement that the handoff gates only exercise Classic (the default theme) through hover and click, so the theme matrix is extra verification and not gate coverage

## simplifications
- Split TodoMVC into two steps. Step 1 is the minimal fix that makes the no-narrowing compiler accept it with zero render risk: delete `get` and make Theme.bn's existing per-kind wrappers (depth, spacing, font, material and the rest) dispatch directly to per-theme per-kind functions, plus add `primary_color()` for the 3 `.color` reads. Step 2 is the owner-requested shape refactor as a separate, separately verified change. This keeps compiler bring-up independent of a risky visual refactor.
- Verify theme identity with a static probe document that renders every (theme, mode, role, hovered/selected/focus) combination side by side, with state passed explicitly, and diff CPU primitives. This needs no interaction support in the harness, finishes in seconds, and needs no mountable TodoMVC baseline.
- Only tokens that are field-read (primary_color, font sizes and weights, gaps) need identical types. Tokens that are only passed to `material:` or spread into style could keep per-theme shapes, because decision 5 allows sound unions. That avoids the neutral-value to renderer-default coupling (color None, border_width 2 px, alphabetical overwrite). Offer this as an option, weighed against the owner's wish for uniform shapes.
- Remove `[events: events]` and similar sites by passing the whole record through a call entry (`element: element`) instead of adding BLOCK aliases. Rename parameters where one field copies one parameter (theme_gap, text_line).
- Build V3 on the existing native-workflow readback (`--required-checkpoint`, kernel-uinput isolated seat) split into one run per theme, instead of a new xtask compare tool.
- Use one pinned old-toolchain build as the baseline oracle, and drop 'old compiler must accept the final corpus' from EX4 and EX5. Then lights can live in `tokens`, and the design need not work around Light/* in the old backend.
- Use the same themes-as-data `tokens(mode)` pattern for NovyTheme as for TodoMVC, so both examples teach one idiom. Deciding bordered versus borderless per call site (style-level `border:`) may remove the need for the material/panel_material split.

## unrealistic
- 2.5-4 engineer-weeks: this omits harness extension (hover, double click, keys, text, embedded programs), restructuring persons_pro effect events under a semantic-assertion gate, the unmeasured breakage in cells, fjordpulse and kavik_cz, the stateful-LATEST and collection-in-HOLD questions, and the embedded sources and migration stages. EX1 alone is L, not M.
- V2 under 60 s with about 60 interaction checkpoints, when the harness can only click today
- Landing the whole migration on the old toolchain before the new compiler exists. The old runtime cannot mount todo_mvc_physical or persons_pro (with this binary). cells fails to check. Multi-file copies outside the repo fail with 'dense kernel checked construction does not cover the complete project' (C10 caveat). The old compiler also does not enforce the new rules (hybrid self-reference, narrowing, lenient joins).
- 0 differing pixels, pixel-exact across two native runs, without a settle/quiescence rule and with more checkpoints than the verifier's 32 cap
- '21/21 manifest scenarios spec-correct and passing' as an example-work target. Many failures are runtime or host-capability gaps (for example persons_pro's child-compile currentness error) outside the example scope.
- EX10 acceptance 'about 10 fixtures, each with the exact expected diagnostic and span', written before the new compiler's diagnostic format exists

## extra_owner_questions
- Is a LATEST with a non-event (constant or continuous) arm a state cell, that is an implicit HOLD whose non-event arm only initialises it? The docs say it is continuous, while the runtime and many examples (TodoMVC title and completed, NovyWave, counter_latest) treat it as state. If not, every such LATEST becomes HOLD.
- Does a HOLD commit act as an event for THEN, which persons_pro and NovyWave rely on? Or is `hold |> THEN` a type error as TYPE_INFERENCE plan:183 says, so that effect completions must be split into explicit event fields?
- Physical-scene data that no renderer reads today (Scene lights, geometry, spring_range, move/elevation, glow and shadows inside materials, font family in spreads): keep it all as Scene-API intent, or delete it all? The design currently deletes some and keeps others.
- Is 'collection inside HOLD' (a documented negative fixture) enforced for stateful LATEST/HOLD cells that hold effect results with LIST payloads, as in NovyWave's real_hierarchy_page_result?
- Is element `style` a typed schema, as the plan's 'Wrong style field type' fixture implies, or an open record? Which style and material fields accept None, tags such as NoOutline, and unions such as `NUMBER | Fully`?
- May an effect call (for example Wellen/hierarchy_page) be triggered from a continuous WHEN arm, or must effects be event-gated?
- Should unused pattern binders (for example `InvalidNumber[reason, position]`) follow the same rule as unused parameters?
