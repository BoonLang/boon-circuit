// Included by `../tests.rs`; kept in the parent test module for private typechecker helper access.

#[test]
fn record_spread_fields_are_typed_and_later_fields_override() {
    let parsed = boon_parser::parse_source(
        "record-spread-type.bn",
        r#"
SOURCE
HOLD
LATEST
LIST {}
base: [a: 1, b: TEXT { old }]
merged: [...base, b: 2, c: TEXT { ok }]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
    let merged_expr_id = parsed
        .expressions
        .iter()
        .find_map(|expr| match &expr.kind {
            AstExprKind::Object(fields) | AstExprKind::Record(fields)
                if fields.iter().any(|field| field.spread)
                    && fields.iter().any(|field| field.name == "c") =>
            {
                Some(expr.id)
            }
            _ => None,
        })
        .expect("fixture should contain merged spread object");
    let merged_type = report
        .expr_type_table
        .entries
        .iter()
        .find(|entry| entry.expr_id == merged_expr_id)
        .expect("merged object should be typed");
    let Type::Object(shape) = &merged_type.flow_type.ty else {
        panic!(
            "merged object should infer an object shape: {:?}",
            merged_type
        );
    };
    assert_eq!(shape.fields.get("a"), Some(&Type::Number));
    assert_eq!(shape.fields.get("b"), Some(&Type::Number));
    assert_eq!(shape.fields.get("c"), Some(&Type::Text));
    assert_eq!(
        shape.field_order,
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
    );
}

#[test]
fn record_spread_rejects_non_record_values() {
    let parsed = boon_parser::parse_source(
        "bad-record-spread.bn",
        "SOURCE\nHOLD\nLATEST\nLIST {}\nmerged: [...1]\n",
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("record spread expects a record value")
    }));
}

#[test]
fn duplicate_explicit_record_fields_are_reported() {
    let parsed = boon_parser::parse_source(
        "duplicate-record-field.bn",
        "SOURCE\nHOLD\nLATEST\nLIST {}\nmerged: [a: 1, a: 2]\n",
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("duplicate explicit record field `a`")
    }));
}

#[test]
fn drain_and_draining_forward_source_types_without_ir_validation() {
    let parsed = boon_parser::parse_source(
        "drain-types.bn",
        r#"
settings: [theme: TEXT { dark }, count: 1]
old_count: 1 |> DRAINING
new_count: DRAIN { old_count }
old_theme:
    TEXT { old }
    |> HOLD theme { LATEST {} }
    |> DRAINING
new_theme:
    DRAIN {
        settings.theme
    }
    |> Text/to_uppercase()
FUNCTION expose(PASS) {
    PASSED.settings.theme
}
context: expose(PASS: [settings: settings])
passed_theme: DRAIN { PASSED.settings.theme }
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );

    let type_for = |expr_id| {
        report
            .expr_type_table
            .entries
            .iter()
            .find(|entry| entry.expr_id == expr_id)
            .map(|entry| entry.flow_type.ty.clone())
            .expect("expression should have an inferred type")
    };

    let mut saw_binding_drain = false;
    let mut saw_field_drain = false;
    let mut saw_passed_drain = false;
    let mut draining_types = Vec::new();
    for expr in &parsed.expressions {
        match &expr.kind {
            AstExprKind::Drain {
                path: AstDrainPath::Binding { name },
            } if name == "old_count" => {
                saw_binding_drain = true;
                assert_eq!(type_for(expr.id), Type::Number);
            }
            AstExprKind::Drain {
                path: AstDrainPath::Field { binding, fields },
            } if binding == "settings" && fields == &["theme".to_owned()] => {
                saw_field_drain = true;
                assert_eq!(type_for(expr.id), Type::Text);
            }
            AstExprKind::Drain {
                path: AstDrainPath::Passed { fields },
            } if fields == &["settings".to_owned(), "theme".to_owned()] => {
                saw_passed_drain = true;
                assert_eq!(type_for(expr.id), Type::Text);
            }
            AstExprKind::Draining { .. } => {
                draining_types.push(type_for(expr.id));
            }
            _ => {}
        }
    }
    assert!(saw_binding_drain && saw_field_drain && saw_passed_drain);
    assert_eq!(draining_types.len(), 2);
    assert!(draining_types.contains(&Type::Number));
    assert!(draining_types.contains(&Type::Text));
}

#[test]
fn row_field_drain_conversion_can_initialize_a_multiline_hold() {
    let parsed = boon_parser::parse_source(
        "todo-migration-v6.bn",
        include_str!("../../../../examples/migrations/todo/v6.bn"),
    )
    .unwrap();
    let list_shape = list_item_shape(&parsed, "tasks").expect("tasks list item shape");
    assert_eq!(list_shape.fields.get("completed"), Some(&true_false_type()));
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
    let task_completed = parsed
        .expressions
        .iter()
        .find(|expr| {
            matches!(&expr.kind, AstExprKind::Path(parts) if parts == &["task".to_owned(), "completed".to_owned()])
        })
        .expect("fixture has task.completed");
    let task_completed_type = &report
        .expr_type_table
        .entries
        .iter()
        .find(|entry| entry.expr_id == task_completed.id)
        .expect("task.completed is typed")
        .flow_type
        .ty;
    assert_eq!(task_completed_type, &true_false_type());
    let drain = parsed
        .expressions
        .iter()
        .find(|expr| {
            matches!(
                &expr.kind,
                AstExprKind::Drain {
                    path: AstDrainPath::Binding { name }
                } if name == "completed"
            )
        })
        .expect("fixture has completed drain");
    let drain_type = &report
        .expr_type_table
        .entries
        .iter()
        .find(|entry| entry.expr_id == drain.id)
        .expect("drain is typed")
        .flow_type
        .ty;
    assert_eq!(drain_type, &true_false_type());
}

#[test]
fn nonvisual_output_registry_infers_closed_record_and_list_types() {
    let parsed = boon_parser::parse_source(
        "server-outputs.bn",
        include_str!("../../../../examples/server_outputs.bn"),
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
    assert_eq!(
        report
            .output_root_types
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["api_response", "pending_priorities"]
    );
    assert!(matches!(
        &report.output_root_types[0].ty,
        Type::Object(shape) if !shape.open
    ));
    assert!(matches!(
        &report.output_root_types[1].ty,
        Type::List(item) if item.as_ref() == &Type::Number
    ));
}

#[test]
fn output_registry_rejects_authority_declarations() {
    let parsed = boon_parser::parse_source(
        "output-authority.bn",
        r#"
outputs: [
    bad: 0 |> HOLD bad { LATEST {} }
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.has_errors());
    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("outputs must be reconstructed from existing current values")
        }),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
}
