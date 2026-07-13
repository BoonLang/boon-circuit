// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

#[test]
fn novywave_project_lowers_source_wrapped_controls() {
    let parsed = boon_parser::parse_project(
        "examples/novywave/RUN.bn",
        [
            (
                "examples/novywave/Bridge/NovyBridge.bn".to_owned(),
                include_str!("../../../../examples/novywave/Bridge/NovyBridge.bn").to_owned(),
            ),
            (
                "examples/novywave/Generated/Assets.bn".to_owned(),
                include_str!("../../../../examples/novywave/Generated/Assets.bn").to_owned(),
            ),
            (
                "examples/novywave/Generated/NovyReference.bn".to_owned(),
                include_str!("../../../../examples/novywave/Generated/NovyReference.bn").to_owned(),
            ),
            (
                "examples/novywave/Model/NovyModel.bn".to_owned(),
                include_str!("../../../../examples/novywave/Model/NovyModel.bn").to_owned(),
            ),
            (
                "examples/novywave/Theme/NovyTheme.bn".to_owned(),
                include_str!("../../../../examples/novywave/Theme/NovyTheme.bn").to_owned(),
            ),
            (
                "examples/novywave/View/NovyView.bn".to_owned(),
                include_str!("../../../../examples/novywave/View/NovyView.bn").to_owned(),
            ),
            (
                "examples/novywave/RUN.bn".to_owned(),
                include_str!("../../../../examples/novywave/RUN.bn").to_owned(),
            ),
        ],
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    assert!(
        !ir.derived_values
            .iter()
            .any(|value| value.path == "store.file_tree_rows"),
        "inline initialized file_tree_rows must not be emitted as a derived list view: {:#?}",
        ir.derived_values
            .iter()
            .filter(|value| matches!(value.kind, DerivedValueKind::ListView))
            .map(|value| value.path.as_str())
            .collect::<Vec<_>>()
    );
    for expected_view in [
        "store.external_catalog_file_tree_rows",
        "store.external_fallback_file_tree_rows",
        "store.external_file_tree_rows",
    ] {
        assert!(
            ir.derived_values.iter().any(|value| {
                value.path == expected_view && value.kind == DerivedValueKind::ListView
            }),
            "{expected_view} must be emitted as a materialized derived list view: {:#?}",
            ir.derived_values
                .iter()
                .filter(|value| matches!(value.kind, DerivedValueKind::ListView))
                .map(|value| value.path.as_str())
                .collect::<Vec<_>>()
        );
    }
    assert!(
        ir.lists.iter().any(|list| {
            list.name == "external_catalog_file_tree_rows"
                && list
                    .row_scope_id
                    .and_then(|scope_id| ir.row_scopes.get(scope_id.as_usize()))
                    .is_some_and(|scope| scope.row_scope == "external_file_tree_row")
        }),
        "external catalog list must own external_file_tree_row scope, lists={:?}, row_scopes={:?}",
        ir.lists,
        ir.row_scopes
    );

    for expected_path in [
        "store.elements.signal_search_input",
        "store.elements.keyboard_capture",
        "store.elements.format_cycle",
    ] {
        assert!(
            ir.view_bindings.iter().any(|binding| {
                binding.kind == ViewBindingKind::Source
                    && binding.path == expected_path
                    && binding.source_id.is_some()
            }),
            "missing source view binding for {expected_path}"
        );
    }
    assert!(ir.state_cells.iter().any(|cell| {
        cell.path == "selected_signal.formatter"
            && cell.indexed
            && cell.initial_value
                == InitialValue::RowInitialField {
                    path: "formatter".to_owned(),
                }
    }));
    let external_file_tree_file = ir
        .state_cells
        .iter()
        .find(|cell| cell.path == "store.external_file_tree_file")
        .expect("NovyWave external loaded file should be a state cell");
    assert_eq!(
        external_file_tree_file.initial_value,
        InitialValue::Text {
            value: "none".to_owned()
        }
    );
    let external_fallback = ir
        .lists
        .iter()
        .find(|list| list.name == "external_fallback_file_tree_rows")
        .expect("NovyWave external fallback list should lower as a list memory");
    let ListInitializer::RecordLiteral { rows } = &external_fallback.initializer else {
        panic!(
            "external fallback rows should be a record literal: {:?}",
            external_fallback.initializer
        );
    };
    let first_row = rows
        .first()
        .expect("external fallback rows should include the fallback entry");
    for field_name in ["file", "selected_file"] {
        let field = first_row
            .fields
            .iter()
            .find(|field| field.name == field_name)
            .expect("external fallback row should expose root-backed file fields");
        assert_eq!(
            field.value,
            InitialValue::RootInitialField {
                path: "external_file_tree_file".to_owned()
            },
            "list literal field `{field_name}` should keep a generic root-state initializer reference"
        );
    }
    assert!(
        ir.state_cells
            .iter()
            .any(|cell| cell.path == "store.active_file" && !cell.indexed),
        "NovyWave active_file must remain a root state cell: {:#?}",
        ir.state_cells
            .iter()
            .filter(|cell| cell.path.contains("active_file"))
            .collect::<Vec<_>>()
    );
    assert!(
        !ir.derived_values.iter().any(|value| value.path == "store"),
        "container path `store` must not be emitted as a derived value"
    );
    assert!(
        ir.update_branches.iter().any(|branch| {
            branch.source == "external_file_tree_row.file_row_elements.select_file"
                && branch.target == "store.active_scope"
                && branch.expression
                    == UpdateExpression::Const {
                        value: "none".to_owned(),
                    }
        }),
        "external file row source must clear active_scope, branches={:?}",
        ir.update_branches
            .iter()
            .filter(|branch| branch.source.contains("file_row_elements.select_file"))
            .cloned()
            .collect::<Vec<_>>()
    );
    assert!(
        ir.update_branches.iter().any(|branch| {
            branch.source == "file_tree_row.file_row_elements.select_file"
                && branch.target == "store.active_scope"
                && matches!(
                    &branch.expression,
                    UpdateExpression::ListFindValue {
                        list,
                        field,
                        target,
                        ..
                    } if list == "store.startup_workspace_opened_files"
                        && field == "file"
                        && target == "selected_scope_key"
                )
        }),
        "default file-row scope selection must keep its own indexed lookup branch: {:?}",
        ir.update_branches
            .iter()
            .filter(|branch| {
                branch.source == "file_tree_row.file_row_elements.select_file"
                    && branch.target == "store.active_scope"
            })
            .collect::<Vec<_>>()
    );
    assert!(
        ir.update_branches.iter().any(|branch| {
            branch.source == "store.elements.panels_toggle_arrangement"
                && branch.target == "store.panel_arrangement"
                && matches!(
                    &branch.expression,
                    UpdateExpression::MatchValueConst { input, arms }
                        if input == "store.panel_arrangement"
                            && arms.iter().any(|arm| {
                                arm.pattern == "__"
                                    && arm.output == UpdateValueExpression::Const {
                                        value: "Docked".to_owned(),
                                    }
                            })
                )
        }),
        "multiline toggle WHEN must remain owned by its THEN branch"
    );
}


#[test]
fn todomvc_lowering_is_static_and_keyed() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    assert_eq!(ir.kind, ProgramKind::Generic);
    assert!(
        ir.nodes
            .iter()
            .filter(|node| node.expr_id.is_some())
            .count()
            > 10
    );
    assert_eq!(ir.lists[0].graph_clones_per_item, 0);
    assert_eq!(ir.lists[0].capacity, None);
    assert_eq!(
        ir.lists[0].initializer,
        ListInitializer::RecordLiteral {
            rows: vec![
                ListInitialRecord {
                    fields: vec![
                        ListRowInitialField {
                            name: "title".to_owned(),
                            value: InitialValue::Text {
                                value: "Read documentation".to_owned(),
                            },
                        },
                        ListRowInitialField {
                            name: "completed".to_owned(),
                            value: InitialValue::Bool { value: false },
                        },
                    ],
                },
                ListInitialRecord {
                    fields: vec![
                        ListRowInitialField {
                            name: "title".to_owned(),
                            value: InitialValue::Text {
                                value: "Finish TodoMVC renderer".to_owned(),
                            },
                        },
                        ListRowInitialField {
                            name: "completed".to_owned(),
                            value: InitialValue::Bool { value: true },
                        },
                    ],
                },
                ListInitialRecord {
                    fields: vec![
                        ListRowInitialField {
                            name: "title".to_owned(),
                            value: InitialValue::Text {
                                value: "Walk the dog".to_owned(),
                            },
                        },
                        ListRowInitialField {
                            name: "completed".to_owned(),
                            value: InitialValue::Bool { value: false },
                        },
                    ],
                },
                ListInitialRecord {
                    fields: vec![
                        ListRowInitialField {
                            name: "title".to_owned(),
                            value: InitialValue::Text {
                                value: "Buy groceries".to_owned(),
                            },
                        },
                        ListRowInitialField {
                            name: "completed".to_owned(),
                            value: InitialValue::Bool { value: false },
                        },
                    ],
                },
            ],
        }
    );
    assert!(
        ir.state_cells
            .iter()
            .any(|cell| cell.path == "todo.completed" && cell.indexed)
    );
    let todo_scope = ir
        .row_scopes
        .iter()
        .find(|scope| scope.list == "todos" && scope.row_scope == "todo")
        .expect("TodoMVC row scope must lower into typed IR");
    assert!(
        ir.lists
            .iter()
            .any(|list| list.name == "todos" && list.row_scope_id == Some(todo_scope.id))
    );
    assert!(ir.sources.iter().any(|source| {
        source.path == "todo.sources.todo_checkbox.click"
            && source.scoped
            && source.scope_id == Some(todo_scope.id)
    }));
    assert!(ir.sources.iter().any(|source| {
        source.path == "store.sources.new_todo_input.key_down"
            && source.payload_schema.fields == vec![SourcePayloadField::Key]
    }));
    assert!(ir.sources.iter().any(|source| {
        source.path == "store.sources.new_todo_input.change"
            && source.payload_schema.fields == vec![SourcePayloadField::Text]
    }));
    assert!(ir.sources.iter().any(|source| {
        source.path == "todo.sources.todo_checkbox.click" && source.payload_schema.fields.is_empty()
    }));
    assert!(ir.view_bindings.iter().any(|binding| {
        binding.node_kind == "Input"
            && binding.attr == "change"
            && binding.kind == ViewBindingKind::Source
            && binding.path == "store.sources.new_todo_input.change"
            && binding.source_id.is_some()
    }));
    assert!(ir.view_bindings.iter().any(|binding| {
        binding.node_kind == "Checkbox"
            && binding.attr == "checked"
            && binding.kind == ViewBindingKind::Data
            && binding.path == "todo.completed"
            && binding.scope_id == Some(todo_scope.id)
    }));
    assert!(ir.view_bindings.iter().any(|binding| {
        binding.node_kind == "Button"
            && binding.attr == "target"
            && binding.kind == ViewBindingKind::Target
            && binding.path == "todo.title"
            && binding.scope_id == Some(todo_scope.id)
    }));
    assert!(ir.state_cells.iter().any(|cell| {
        cell.path == "todo.completed" && cell.indexed && cell.scope_id == Some(todo_scope.id)
    }));
    assert!(ir.state_cells.iter().any(|cell| {
        cell.path == "todo.title"
            && cell.initial_value
                == InitialValue::RowInitialField {
                    path: "title".to_owned(),
                }
    }));
    assert!(ir.state_cells.iter().any(|cell| {
        cell.path == "store.new_todo_text"
            && cell.initial_value
                == InitialValue::Text {
                    value: String::new(),
                }
    }));
    assert!(ir.derived_values.iter().any(|value| {
        value.path == "store.title_to_add"
            && value.kind == DerivedValueKind::SourceEventTransform
            && value
                .sources
                .contains(&"store.sources.new_todo_input.key_down".to_owned())
    }));
    assert!(ir.possible_causes.iter().any(|entry| {
        entry.target == "todo.completed"
            && entry
                .sources
                .contains(&"todo.sources.todo_checkbox.click".to_owned())
    }));
    assert!(
        ir.nodes
            .iter()
            .any(|node| matches!(node.kind, IrNodeKind::ListRemove))
    );
    assert!(ir.list_operations.iter().any(|operation| {
        operation.list == "todos"
            && operation.kind
                == ListOperationKind::Append {
                    trigger: "store.title_to_add".to_owned(),
                    fields: vec![
                        ListAppendField {
                            name: "title".to_owned(),
                            value: ListAppendFieldValue::Source {
                                path: "store.title_to_add".to_owned(),
                            },
                        },
                        ListAppendField {
                            name: "completed".to_owned(),
                            value: ListAppendFieldValue::Const {
                                value: "False".to_owned(),
                            },
                        },
                    ],
                }
    }));
    assert!(ir.list_operations.iter().any(|operation| {
        operation.list == "todos"
            && operation.kind
                == ListOperationKind::Remove {
                    source: "todo.sources.remove_todo_button.press".to_owned(),
                    predicate: ListPredicate::AlwaysTrue,
                }
    }));
    assert!(ir.list_operations.iter().any(|operation| {
        operation.list == "todos"
            && operation.kind
                == ListOperationKind::Remove {
                    source: "store.sources.clear_completed_button.press".to_owned(),
                    predicate: ListPredicate::RowFieldBool {
                        path: "todo.completed".to_owned(),
                    },
                }
    }));
    assert!(ir.list_operations.iter().any(|operation| {
        operation.list == "todos"
            && operation.kind
                == ListOperationKind::Retain {
                    target: "store.visible_todos".to_owned(),
                    predicate: ListPredicate::SelectedFilterVisibility {
                        selector: "store.selected_filter".to_owned(),
                        row_field: "todo.completed".to_owned(),
                    },
                }
    }));
    assert!(ir.update_branches.iter().any(|branch| {
        branch.target == "store.selected_filter"
            && branch.source == "store.sources.filter_active.press"
            && branch.expression
                == UpdateExpression::Const {
                    value: "Active".to_owned(),
                }
    }));
    assert!(ir.update_branches.iter().any(|branch| {
        branch.target == "todo.completed"
            && branch.source == "todo.sources.todo_checkbox.click"
            && matches!(branch.expression, UpdateExpression::BoolNot { .. })
            && branch.indexed
    }));
    assert!(ir.update_branches.iter().any(|branch| {
        branch.target == "todo.editing"
            && branch.source == "todo.sources.editing_todo_title_element.key_down"
            && branch.expression
                == UpdateExpression::Const {
                    value: "False".to_owned(),
                }
            && branch.indexed
    }));
    assert!(ir.update_branches.iter().any(|branch| {
        branch.target == "todo.title"
            && branch.source == "todo.sources.editing_todo_title_element.key_down"
            && branch.expression
                == UpdateExpression::TextTrimOrPrevious {
                    path: "edit_text".to_owned(),
                    previous: "title".to_owned(),
                }
            && branch.indexed
    }));
    assert!(ir.update_branches.iter().any(|branch| {
        branch.target == "todo.title"
            && branch.source == "todo.sources.editing_todo_title_element.blur"
            && branch.expression
                == UpdateExpression::TextTrimOrPrevious {
                    path: "edit_text".to_owned(),
                    previous: "title".to_owned(),
                }
            && branch.indexed
    }));
    assert!(ir.update_branches.iter().any(|branch| {
        branch.target == "todo.edit_text"
            && branch.source == "todo.sources.editing_todo_title_element.change"
            && branch.expression
                == UpdateExpression::TextTrimOrPrevious {
                    path: "text".to_owned(),
                    previous: "edit_text".to_owned(),
                }
            && branch.indexed
    }));
    assert!(ir.update_branches.iter().any(|branch| {
        branch.target == "todo.edit_text"
            && branch.source == "todo.sources.editing_todo_title_element.key_down"
            && branch.expression
                == UpdateExpression::PreviousValue {
                    path: "title".to_owned(),
                }
            && branch.indexed
    }));
    assert!(ir.nodes.iter().any(|node| {
        matches!(node.kind, IrNodeKind::RenderLowering) && node.name == "render_todos_template"
    }));
    verify_hidden_identity(&ir).unwrap();
}


#[test]
fn hidden_identity_verifier_scans_boon_facing_ir_identifiers() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    assert!(
        ir.lists
            .iter()
            .any(|list| list.hidden_key_type.ends_with("Key")),
        "internal list key types should remain IR metadata"
    );
    verify_hidden_identity(&ir).unwrap();

    let mut with_bad_source = ir.clone();
    with_bad_source.sources[0].path = "todo.sources.source_id.press".to_owned();
    assert!(
        verify_hidden_identity(&with_bad_source)
            .unwrap_err()
            .contains("source_id")
    );

    let mut with_bad_state = ir.clone();
    with_bad_state.state_cells[0].path = "todo.hidden_generation".to_owned();
    assert!(
        verify_hidden_identity(&with_bad_state)
            .unwrap_err()
            .contains("hidden_generation")
    );

    let mut with_bad_branch = ir.clone();
    with_bad_branch.update_branches[0].expression = UpdateExpression::PreviousValue {
        path: "bind_epoch".to_owned(),
    };
    assert!(
        verify_hidden_identity(&with_bad_branch)
            .unwrap_err()
            .contains("bind_epoch")
    );

    let mut with_bad_row_key = ir.clone();
    with_bad_row_key.sources[0].path = "todo.sources.$boon.row_key.press".to_owned();
    let row_key_error = verify_hidden_identity(&with_bad_row_key).unwrap_err();
    assert!(
        row_key_error.contains("$boon") || row_key_error.contains("row_key"),
        "{row_key_error}"
    );

    let mut with_bad_target_key = ir.clone();
    with_bad_target_key.update_branches[0].target = "store.target_key".to_owned();
    assert!(
        verify_hidden_identity(&with_bad_target_key)
            .unwrap_err()
            .contains("target_key")
    );

    let mut with_bad_list_operation = ir.clone();
    with_bad_list_operation.list_operations[0].kind = ListOperationKind::Retain {
        target: "store.visible_todos".to_owned(),
        predicate: ListPredicate::RowFieldBool {
            path: "todo.hidden_key".to_owned(),
        },
    };
    assert!(
        verify_hidden_identity(&with_bad_list_operation)
            .unwrap_err()
            .contains("hidden_key")
    );

    let mut with_bad_chunk_projection = ir.clone();
    with_bad_chunk_projection
        .list_projections
        .push(ListProjection {
            target: "store.rows".to_owned(),
            list: "store.todos".to_owned(),
            kind: ListProjectionKind::Chunk {
                size: Some(4),
                item_field: "row_key".to_owned(),
                label_field: "row_number".to_owned(),
            },
        });
    assert!(
        verify_hidden_identity(&with_bad_chunk_projection)
            .unwrap_err()
            .contains("row_key")
    );
}


#[test]
fn cause_tables_derive_row_scope_from_list_map_function() {
    let source = include_str!("../../../../examples/todomvc.bn")
        .replace(
            "new_todo(todo: todo, store: store)",
            "make_item(todo: todo, store: store)",
        )
        .replace(
            "FUNCTION new_todo(todo, store)",
            "FUNCTION make_item(todo, store)",
        );
    let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
    let ir = lower(&parsed).unwrap();
    assert!(parsed.row_scope_functions.iter().any(|scope| {
        scope.function == "make_item" && scope.list == "todos" && scope.row_scope == "todo"
    }));
    assert!(
        ir.state_cells
            .iter()
            .any(|cell| cell.path == "todo.completed" && cell.indexed)
    );
    assert!(ir.possible_causes.iter().any(|entry| {
        entry.target == "todo.completed"
            && entry
                .sources
                .contains(&"todo.sources.todo_checkbox.click".to_owned())
    }));
}
