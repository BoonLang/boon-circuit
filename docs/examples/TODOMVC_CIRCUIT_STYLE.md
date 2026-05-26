# TodoMVC Circuit-Style Target

This is the intended shape of TodoMVC for the new engine. It is not final Boon
syntax; it is a design target. The important property is that each field locally
declares what can change it.

The original plain TodoMVC in `~/repos/boon` does not put an `id` field on todo
records. That is the right default. Runtime retention needs hidden row keys, but
those keys are not Boon values. Boon code sees and compares data only.

## Store Shape

```boon
store: [
    sources: [
        new_todo_input: [
            change: SOURCE
            key_down: SOURCE
        ]
        toggle_all_checkbox: [click: SOURCE]
        clear_completed_button: [press: SOURCE]
        filter_all: [press: SOURCE]
        filter_active: [press: SOURCE]
        filter_completed: [press: SOURCE]
    ]

    new_todo_text:
        Text/empty |> HOLD text {
            LATEST {
                sources.new_todo_input.change.text

                title_to_add |> THEN {
                    Text/empty
                }
            }
        }

    title_to_add:
        sources.new_todo_input.key_down |> WHEN {
            [key: Enter, text: text] =>
                text |> Text/trim() |> WHEN {
                    "" => SKIP
                    title => title
                }

            __ => SKIP
        }

    selected_filter:
        All |> HOLD filter {
            LATEST {
                sources.filter_all.press |> THEN { All }
                sources.filter_active.press |> THEN { Active }
                sources.filter_completed.press |> THEN { Completed }
            }
        }

    todos:
        LIST {
            [title: TEXT { Buy groceries }]
            [title: TEXT { Clean room }]
        }
        |> List/append(item: title_to_add |> THEN {
            [title: title_to_add]
        })
        |> List/remove(todo, when:
            LATEST {
                todo.sources.remove_todo_button.press |> THEN { True }
                sources.clear_completed_button.press |> THEN {
                    todo.completed
                }
            }
        )
        |> List/map(todo, new: new_todo(todo: todo, store: store))
]
```

## Todo Row Shape

```boon
FUNCTION new_todo(todo) {
    sources: [
        remove_todo_button: [press: SOURCE]
        editing_todo_title_element: [
            change: SOURCE
            key_down: SOURCE
            blur: SOURCE
        ]
        todo_title_element: [double_click: SOURCE]
        todo_checkbox: [click: SOURCE]
    ]

    [
        title:
            todo.title |> HOLD title {
                LATEST {
                    sources.editing_todo_title_element.key_down |> WHEN {
                        [key: Enter, text: text] =>
                            text |> Text/trim() |> WHEN {
                                "" => title
                                trimmed => trimmed
                            }

                        __ => SKIP
                    }

                    sources.editing_todo_title_element.blur |> THEN {
                        edit_text |> Text/trim() |> WHEN {
                            "" => title
                            trimmed => trimmed
                        }
                    }
                }
            }

        edit_text:
            todo.title |> HOLD draft {
                LATEST {
                    sources.todo_title_element.double_click |> THEN {
                        title
                    }

                    sources.editing_todo_title_element.change.text |> WHEN {
                        changed =>
                            changed |> Text/trim() |> WHEN {
                                "" => draft
                                trimmed => trimmed
                            }
                    }

                    sources.editing_todo_title_element.key_down.key |> WHEN {
                        Escape => title
                        __ => SKIP
                    }
                }
            }

        completed:
            False |> HOLD completed {
                LATEST {
                    sources.todo_checkbox.click |> THEN {
                        completed |> Bool/not()
                    }

                    store.sources.toggle_all_checkbox.click |> THEN {
                        store.all_completed |> Bool/not()
                    }
                }
            }

        editing:
            False |> HOLD editing {
                LATEST {
                    sources.todo_title_element.double_click |> THEN {
                        True
                    }

                    sources.editing_todo_title_element.key_down.key |> WHEN {
                        Enter => False
                        Escape => False
                        __ => SKIP
                    }

                    sources.editing_todo_title_element.blur |> THEN {
                        False
                    }
                }
            }
    ]
}
```

The runtime still creates a hidden row address for every todo:

```text
(list_id=todos, parent_scope=/, item_key=<key>, generation=<generation>)
```

That key is used for source binding, deltas, storage, stale-event rejection, and
debugging. It is not exposed to Boon code and cannot be compared by the Boon
developer.

## Derived Values

```boon
store.visible_todos:
    store.todos
    |> List/retain(todo, if:
        store.selected_filter |> WHEN {
            All => True
            Active => todo.completed |> Bool/not()
            Completed => todo.completed
        }
    )

store.active_count:
    store.todos
    |> List/retain(todo, if: todo.completed |> Bool/not())
    |> List/count

store.completed_count:
    store.todos
    |> List/retain(todo, if: todo.completed)
    |> List/count

store.all_completed:
    store.active_count == 0 |> Bool/and(store.completed_count > 0)
```

## Runtime Lowering

The program above lowers to a static set of operators:

```text
new_todo_text_next
title_to_add_event
selected_filter_next
todos_append
todos_remove[key]
todo_title_next[key]
todo_edit_text_next[key]
todo_completed_next[key]
todo_editing_next[key]
visible_todos_view
counts
render_todo_row_template[key]
```

For 2 todos or 100000 todos, the operator graph is the same. Only memory size
and changed-key sets grow.

## Why The Old Physical Experiment Had Id

The separate physical TodoMVC experiment in `~/repos/boon` added:

```boon
id: TodoId[id: Ulid/generate()]
```

because that example also had a global `selected_todo_id` and compared it in the
row renderer to decide which row was being edited.

That should be treated as a workaround from the old experiment, not the target
language model. In the circuit model, editing should be row-local state driven by
row-local sources, as in the original plain TodoMVC.

If a program's input data contains a field named `id`, Boon treats it as ordinary
data. Comparing it compares data. It is not a reference and not runtime identity.

## Required Debug View

The debugger should be able to show:

```text
todo.completed[42]
  current: true
  last changed by: /todos:42/todo_checkbox.click
  possible causes:
    /todos:42/todo_checkbox.click
    /toggle_all_checkbox.click
```

This is the property the reducer version loses.
