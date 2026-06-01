# Boon Type Notation And Inspector

This document is the implementation contract for Boon-facing type notation,
runtime object shapes, editor type hints, and inspector UI. It is also the
template for future runtime roots such as `scene:`.

## Boon-Facing Type Notation

Use Boon syntax whenever a type is shown to a user.

```boon
title: TEXT
completed: BOOL
todos: LIST<[
    [
        title: TEXT
        completed: BOOL
    ]
]>
```

- Native types and built-in display aliases are uppercase: `TEXT`, `NUMBER`,
  `BOOL`, `LIST<T>`, `VALUE`, `ABSENT`.
- `BOOL` is the Boon-facing alias for the structural union `False | True`; it is
  not a nominal runtime primitive.
- Other tags display directly, such as `Active | All | Completed`, never
  `tag Active | All | Completed`.
- Untagged object shapes use bracket notation: `[title: TEXT]`.
- Exact empty objects display as `[]`.
- Collapsed known object shapes keep the normal opening bracket on the collapsed
  row, for example `▸ style: [` or `▸ color: Oklch[`. Do not display `[...]`,
  because it looks like an old placeholder and is easy to confuse with missing
  structure.
- Detailed object shapes use newlines and 4-space indentation:

```boon
[
    title: TEXT
    completed: BOOL
]
```

Tagged objects use the tag before the same bracket object shape only when the
value is actually tagged:

```boon
Oklch[
    l: NUMBER
    c: NUMBER
    h: NUMBER
]
```

`Element` is not a user-facing type keyword.

## Runtime Object Shapes

`Element/button(...)`, `Element/text(...)`, `Element/stripe(...)`, and related
names are ordinary functions. They return structural objects that the selected
runtime/root can process.

Do not invent syntax such as `Element/button[...]`.

The active root selects the render contract. The current `document:` root uses
document-renderable object shapes. Future roots such as `scene:` register their
own accepted object shapes without changing Boon expression syntax.

Initial document-runtime shape registry template:

| Root | Function | Returned shape | Slot contract |
| --- | --- | --- | --- |
| `document:` | `Document/new(...)` | `[kind: Document]` | `root:` accepts one document-renderable object |
| `document:` | `Element/container(...)` | `[kind: Stack]` | `child:` accepts a document-renderable object, with current `TEXT`/`NUMBER` child shorthand preserved |
| `document:` | `Element/stripe(...)` | `[kind: Row | Stack]` | `items:` accepts `LIST` of document-renderable objects |
| `document:` | `Element/text(...)` | `[kind: Text]` | display fields contribute `TEXT` constraints for function parameters |
| `document:` | `Element/label(...)` | `[kind: Text]` | display fields contribute `TEXT` constraints for function parameters |
| `document:` | `Element/paragraph(...)` | `[kind: Text]` | display fields contribute `TEXT` constraints for function parameters |
| `document:` | `Element/link(...)` | `[kind: Text]` | display fields contribute `TEXT` constraints for function parameters |
| `document:` | `Element/button(...)` | `[kind: Button]` | event/source payloads are ordinary Boon values; there is no user-visible `Event` type |
| `document:` | `Element/checkbox(...)` | `[kind: Button]` | checked/visible/focus state is `BOOL` when checked directly |
| `document:` | `Element/text_input(...)` | `[kind: TextInput]` | text/value fields contribute `TEXT` constraints for function parameters; current wrapper objects such as `[text: TEXT]` remain valid |

The typechecker may keep an internal render contract sentinel, but normal
labels, hints, and diagnostics must show Boon object notation.

Current strict checks are intentionally tied to contracts that are already
unambiguous in existing Boon code:

- `root:` accepts one document-renderable object.
- `items:` and `children:` accept `LIST` of document-renderable objects.
- `child:` accepts one document-renderable object or the existing scalar text
  shorthand.
- `checked`, `visible`, `selected`, and `focus` accept `BOOL` when a
  direct value is checked.
- Text-like display arguments are used to infer function parameter types, but
  direct render fields are not rejected merely because the runtime currently
  accepts display coercions or wrapper objects used by the bundled examples.

## Editor Hints

The sidebar is the canonical detailed type display. Header and footer text
should not duplicate full type details.

Inline hints are intentionally sparse. They should appear when the type is not
obvious from nearby source:

- function parameters;
- function return values;
- pipeline step results;
- fields with tag unions;
- compact render constructor return shapes such as `[kind: Button]`.

Avoid inline labels for large object structures. Show those in the sidebar.

## Sidebar Inspector

The sidebar must preserve newlines and 4-space indentation. It must scroll
independently from the editor and clip its own content.

Type and value rows should be syntax-colored with the same visual language as
the source editor: field names, native types, tags, strings, numbers, operators,
and punctuation receive distinct spans. The highlighter is for type/value
notation, not the full Boon source grammar.

Coloring should reuse the source editor's syntax style categories, so type and
value displays feel like Boon code:

| Inspector token | Source-editor style category |
| --- | --- |
| field names before `:` | `definition` |
| native types and aliases such as `TEXT`, `NUMBER`, `BOOL`, `LIST`, `VALUE`, `ABSENT` | `type` |
| tags such as `False`, `True`, `Active`, `Completed`, `Button` | `tag` |
| string values such as `"Done"` | `string` |
| numeric values such as `12` or `-3.5` | `number` |
| `|`, `=`, `+`, `-`, `*`, `/` | `operator` |
| `[`, `]`, `<`, `>`, `:`, `,`, `(`, `)` | `punctuation` |
| `...` collapsed-shape marker | `chain-alt` |
| ordinary lowercase value names | `variable` |

The rich spans must preserve exact `source_text`; coloring must not alter
selection, copy/paste, caret positioning, or layout.

The detail area should render the active hover token when available. Otherwise
it falls back to the caret token. When neither has an inferred type, it shows a
short empty state.

## Value Inspector

The same sidebar may include bounded runtime values for the active token path.
It must not mirror the entire app state eagerly.

When a current value is available, it should sit next to the type with normal
Boon-style expression flow:

```boon
active_count: NUMBER = 3
completed: BOOL = False
new_todo_text: TEXT = TEXT {}
selected_filter: Active | All | Completed = All
todos: LIST = 4 items
```

Implemented v1 behavior:

- scalars display directly;
- objects display field count and a bounded top-level field sample;
- lists display length and a bounded item sample;
- the dev window asks the preview process for explicit paths only;
- request bounds are enforced for depth, field count, item count, and path count.

For a large value such as `store`, the sidebar must show only a bounded summary,
never the whole state graph. Deeper fields and arbitrary list ranges are a
future UI expansion on top of the same path-bounded IPC route; they are not part
of the current v1 sidebar.

## Diagnostics

Diagnostics should use the same notation as hints. For render slots, prefer
root/slot wording:

```text
`items` expects a LIST of objects accepted by `document:`
expected: LIST<[...]>
found: LIST<[title: TEXT]>
```

Avoid internal terms in normal diagnostics: `RenderContract`, `TypeVar`,
`Record`, `Unknown`, `Event`, and `Bool`.

## Implementation Status

Implemented v1:

- Boon-facing notation uses uppercase native types, direct tags, bracket object
  shapes, `[]` for exact empty objects, and `[...]` for collapsed known shapes.
- `BOOL` displays the exact `False | True` tag union while preserving structural
  typing internally.
- The native editor renders sparse inline hints as virtual metadata, so source
  text, selection, copy/paste, and measurement stay tied to the original code.
- The sidebar is the detailed type/value surface and uses the inspector
  highlighter categories listed above.
- The preview value IPC route is path-bounded by requested path, depth, field
  count, list sample count, and path count.
- Typechecking has a formal root/constructor registry for `document:` and can
  register future roots at the type-contract layer.
- `Element/stripe(...)` returns `[kind: Row | Stack]`, narrowed to `Row` or
  `Stack` when the `direction` value is known.

Not implemented as a separate platform yet:

- Future roots such as `scene:` have a registry template and typecheck API, but
  no native/runtime lowering path is expected until a real non-document runtime
  root exists.
