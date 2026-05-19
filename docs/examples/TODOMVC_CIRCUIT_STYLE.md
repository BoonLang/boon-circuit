# TodoMVC Circuit-Style Target

This is the intended shape of TodoMVC for the new engine. It is not final Boon
syntax; it is a design target. The important property is that each field locally
declares what can change it.

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
        Text/empty() |> HOLD text {
            LATEST {
                sources.new_todo_input.change.text

                title_to_add |> THEN {
                    Text/empty()
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

    next_todo_id:
        3 |> HOLD id {
            title_to_add |> THEN {
                id + 1
            }
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
            [id: 1, title: TEXT { Buy groceries }]
            [id: 2, title: TEXT { Clean room }]
        }
        |> List/append(title_to_add |> THEN {
            [id: next_todo_id, title: title_to_add]
        })
        |> List/map(new_todo)
]
```

## Todo Row Shape

```boon
FUNCTION new_todo(seed) {
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
        id: seed.id

        alive:
            True |> HOLD alive {
                LATEST {
                    sources.remove_todo_button.press |> THEN {
                        False
                    }

                    store.sources.clear_completed_button.press |> THEN {
                        completed |> WHEN {
                            True => False
                            False => alive
                        }
                    }
                }
            }

        title:
            seed.title |> HOLD title {
                LATEST {
                    sources.editing_todo_title_element.change.text |> WHEN {
                        changed =>
                            changed |> Text/trim() |> WHEN {
                                "" => title
                                trimmed => trimmed
                            }
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

## Derived Values

```boon
visible_todos:
    store.todos
    |> List/retain(todo,
        if:
            todo.alive
            && store.selected_filter |> WHEN {
                All => True
                Active => todo.completed |> Bool/not()
                Completed => todo.completed
            }
    )

active_count:
    store.todos
    |> List/count(todo, if: todo.alive && Bool/not(todo.completed))

completed_count:
    store.todos
    |> List/count(todo, if: todo.alive && todo.completed)

all_completed:
    active_count == 0 && completed_count > 0
```

## Runtime Lowering

The program above lowers to a static set of operators:

```text
new_todo_text_next
title_to_add_event
next_todo_id_next
selected_filter_next
todos_append
todo_alive_next[key]
todo_title_next[key]
todo_completed_next[key]
todo_editing_next[key]
visible_todos_view
counts
render_todo_row_template[key]
```

For 2 todos or 100000 todos, the operator graph is the same. Only memory size
and changed-key sets grow.

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
