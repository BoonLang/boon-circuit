# Native Editor Type Hints

This document covers the native editor behavior. The Boon-facing notation,
runtime object-shape rules, and inspector examples are defined in
`docs/architecture/BOON_TYPE_NOTATION_AND_INSPECTOR.md` and must remain the
source of truth.

## Inline Hints

Inline hints are sparse. They appear only where the type adds useful context:

- function signatures and return values;
- pipeline call results such as `|> List/count() : NUMBER`;
- tag-union fields such as `selected_filter: Active | All | Completed`;
- compact render constructor results such as `[kind: Button]`.

Large object shapes are not drawn inline. They belong in the sidebar.

## Sidebar Inspector

The sidebar is the canonical detailed type display for the active hover or
caret token. It must preserve newlines and 4-space indentation, clip its own
content, and scroll independently from the code editor.

Sidebar type/value rows are rich text. The renderer uses a small inspector
notation highlighter so field names, native types, tags, values, operators, and
punctuation are colored without changing the underlying copied text.

The inspector highlighter should reuse the same style categories as source
highlighting:

| Inspector token | Source-editor style category |
| --- | --- |
| field names before `:` | `definition` |
| `TEXT`, `NUMBER`, `BOOL`, `LIST`, `VALUE`, `ABSENT` | `type` |
| tags and runtime kind names | `tag` |
| strings | `string` |
| numbers | `number` |
| operators | `operator` |
| brackets, commas, colons, parens | `punctuation` |
| collapsed marker `...` | `chain-alt` |

Every span must keep `source_text` identical to the displayed text, so coloring
does not affect scrolling, selection, copy/paste, or measurement.

Examples:

```boon
count: NUMBER
completed: BOOL = False
```

```boon
[
    active_count: NUMBER
    selected_filter: Active | All | Completed
    todos: LIST<[
        title: TEXT
        completed: BOOL
    ]>
]
```

## Runtime Values

Runtime values, when available, should appear as a bounded section in the same
sidebar. The dev window must not mirror the whole app state eagerly. Current v1
uses path-bounded IPC from the active token, with bounded object fields and list
samples; arbitrary tree expansion and list ranges are future UI on the same
bounded route.

Scalar and summary values are shown inline with the type:

```boon
active_count: NUMBER = 3
completed: BOOL = False
todos: LIST = 4 items
```

## Native Metadata

The editor attaches filtered inline hints as `editor_type_hints_json` metadata on
line text nodes. The rendered source text and syntax spans must keep their
original source ranges so scrolling, selection, and copy/paste do not depend on
hint text.
