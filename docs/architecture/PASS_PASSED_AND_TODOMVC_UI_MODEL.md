# PASS/PASSED And TodoMVC UI Model

This document is the implementation contract for the classic TodoMVC cleanup.
It separates language context passing, classic document UI APIs, physical scene
APIs, and native renderer internals.

## Why PASS/PASSED Exists

`PASS/PASSED` is explicit context wiring. It is not global state.

The call that crosses an application/component boundary provides the context:

```boon
document: Document/new(root: todo_app(PASS: [store: store]))
```

Every nested function call made while rendering `todo_app` inherits that context
automatically. A nested component can read:

```boon
PASSED.store.selected_filter
```

without the caller adding `store` to every function signature in the call graph.
This keeps component dependencies visible in the graph while avoiding manual
plumbing through unrelated intermediate functions. The compiler still typechecks
the full required context: if a direct or nested call needs `PASSED.store`, then
the enclosing root call must have passed a context with `store`.

`PASS:` can still be written explicitly at another call boundary to introduce,
replace, narrow, or extend the current context. The runtime must not maintain a
dynamic global `PASSED` stack; this is compile-time context propagation.

## No LINK

`LINK` is not coming back.

The old examples used `LINK` to create element references and source bindings.
In this repo, source ports remain ordinary `SOURCE` fields and elements bind to
them through typed event groups. The user-facing replacement is `events`, not
`LINK`.

## Event Groups

Current source ports are explicit data:

```boon
store: [
    sources: [
        new_todo_input: [
            events: [
                change: SOURCE
                key_down: SOURCE
                focus: SOURCE
                blur: SOURCE
            ]
        ]
    ]
]
```

Elements bind a whole event group:

```boon
Element/text_input(
    element: [events: PASSED.store.sources.new_todo_input.events]
)
```

The compiler/lowerer expands that group to concrete source paths. Boon source
uses the public `events` object:

```boon
PASSED.store.sources.new_todo_input.events.change.text
```

The current runtime source IDs remain canonicalized without the grouping segment,
for example `store.sources.new_todo_input.change`, so older scenarios and row
source binding internals stay stable while the public Boon shape moves to
`events`. The element API should validate event names against the element kind.
Unknown events and non-`SOURCE` event fields are errors.

## Classic TodoMVC vs Physical TodoMVC

Classic TodoMVC is the current product target. Its reference source is:

```text
/home/martinkavik/repos/boon/playground/frontend/src/examples/todo_mvc/todo_mvc.bn
```

Physical TodoMVC is a separate experiment. Its reference source is:

```text
/home/martinkavik/repos/boon/playground/frontend/src/examples/todo_mvc_physical/RUN.bn
```

Classic TodoMVC uses:

```boon
document: Document/new(...)
Element/*
```

Physical TodoMVC uses:

```boon
scene: Scene/new(...)
Scene/Element/*
```

The two roots should not be collapsed into renderer-private graphics fields.
Shared language concepts such as `PASS/PASSED`, `NoElement`, `Hidden`,
`Reference`, and `element.hovered` may exist in both roots, but scene-only ideas
like `material`, `depth`, `move`, `relief`, `spring_range`, `lights`, and
`geometry` stay scene-root concepts.

Classic TodoMVC should not expose physical IDs or a global selected-row
identity. Row identity for retention, source binding, stale-event rejection, and
debugging is hidden runtime state, not Boon data.

## Public Document Element API

Classic `document:` UI code should use public document APIs from the old classic
TodoMVC source:

- `Element/container`, `Element/stripe`, `Element/label`, `Element/paragraph`,
  `Element/link`, `Element/button`, `Element/checkbox`, and
  `Element/text_input`.
- `element: [events: ...]` for source binding.
- `element: [hovered: True]` plus `element.hovered` for hover-dependent styles
  and conditional children.
- `NoElement` for absent render children.
- `Hidden[text: ...]` for invisible accessible labels.
- `Reference[element: ...]` for label references.
- Public style objects such as `outline`, `NoOutline`, `borders`,
  `shadows: LIST {...}`, `font.line.strikethrough`, `font.line.underline`,
  `rounded_corners`, `transform.rotate`, `background.url`, `text_shadow`,
  `line_height`, and `font_smoothing`.

TodoMVC titles should be `Element/label` with a double-click event, not buttons
styled to look like labels.

## Deleted Public Fields

The following renderer-style fields are bad public API. They should not appear
in Boon examples, typechecking allowlists, or verifier expectations:

- `shadow1_*`
- `border_top`
- `selected_border`
- `strike_if`
- `color_if`
- `focus_border`
- `focus_border_width`
- `hover_visible`
- `hover_color`
- `hover_border`
- `hover_underline_if`
- `hover_scope`

Renderer internals may keep private implementation keys, but those names must
not leak into Boon source.

## Idle CPU

Demand-driven native windows must not poll every 20ms while visually idle. If an
explicit timer is pending, the loop waits until that timer or an external wake.
While `app_window` only exposes coalesced input state, not a per-input wake
callback, the loop may keep a slow passive input poll so real mouse/keyboard
input remains responsive. That fallback must be visible in reports and must be
slow enough to avoid the old resize/idle CPU burn. Explicit timers remain valid
for caret blink, active inspector refresh, preview summary refresh, and verifier
frames.
