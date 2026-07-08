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
