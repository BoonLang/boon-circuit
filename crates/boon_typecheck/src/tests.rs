use super::*;

// Typecheck tests are grouped by language surface while staying in this module for private helper access.
include!("tests/flow_and_state.rs");
include!("tests/functions_and_arguments.rs");

#[test]
fn list_chunk_type_uses_declared_output_field_names() {
    let parsed = boon_parser::parse_source(
        "chunk-fields.bn",
        "events: SOURCE\nvalue: 0 |> HOLD value { LATEST { events |> THEN { value } } }\nvalues: LIST {}\nrows: List/chunk(values, size: 2, items: group, label: index)",
    )
    .unwrap();
    let expression = parsed
        .expressions
        .iter()
        .find(|expression| {
            matches!(&expression.kind, AstExprKind::Call { function, .. } if function == "List/chunk")
                || matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "List/chunk")
        })
        .unwrap();
    let Type::List(item) = simple_expr_type(expression, &parsed.expressions) else {
        panic!("List/chunk should infer a list");
    };
    let Type::Object(shape) = item.as_ref() else {
        panic!("List/chunk should infer object rows");
    };

    assert_eq!(
        shape
            .ordered_fields()
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["index", "group"]
    );
    assert!(!shape.fields.contains_key("row_number"));
    assert!(!shape.fields.contains_key("cells"));
}
