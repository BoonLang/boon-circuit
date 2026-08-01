#[test]
fn deferred_style_constraints_validate_reachable_call_specializations() {
    let parsed = boon_parser::parse_source(
        "deferred-style-specialization.bn",
        r#"
FUNCTION sized_box(width) {
    Element/container(
        element: []
        style: [width: width]
        child: Element/label(
            element: []
            style: []
            label: TEXT { child }
        )
    )
}

document: Document/new(root: sized_box(width: TEXT { invalid }))
"#,
    )
    .expect("style specialization fixture parses");
    let output = check_program(&parsed);

    assert!(
        output.report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("style field `width` must be a number")
        }),
        "deferred style diagnostics: {:#?}",
        output.report.diagnostics
    );
}

#[test]
fn deferred_style_constraints_accept_reachable_number_specializations() {
    let parsed = boon_parser::parse_source(
        "deferred-style-number-specialization.bn",
        r#"
FUNCTION sized_box(width) {
    Element/container(
        element: []
        style: [width: width]
        child: Element/label(
            element: []
            style: []
            label: TEXT { child }
        )
    )
}

document: Document/new(root: sized_box(width: 42))
"#,
    )
    .expect("style specialization fixture parses");
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "valid deferred style diagnostics: {:#?}",
        output.report.diagnostics
    );
}
