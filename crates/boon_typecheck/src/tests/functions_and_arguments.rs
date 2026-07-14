// Included by `../tests.rs`; kept in the parent test module for private typechecker helper access.

#[test]
fn cross_module_text_function_is_accepted_as_a_style_color() {
    let parsed = boon_parser::parse_project(
        "RUN.bn",
        [
            (
                "Theme.bn".to_owned(),
                r#"
FUNCTION accent() {
    TEXT { #2f6c4f }
}
"#
                .to_owned(),
            ),
            (
                "View.bn".to_owned(),
                r#"
FUNCTION root() {
    Scene/Element/text(
        element: []
        style: [color: Theme/accent()]
        text: TEXT { Semantic color }
    )
}
"#
                .to_owned(),
            ),
            (
                "RUN.bn".to_owned(),
                "document: Document/new(root: View/root())\n".to_owned(),
            ),
        ],
    )
    .unwrap();

    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn todomvc_completed_hints_use_widened_true_false_shape() {
    let source = include_str!("../../../../examples/todomvc.bn");
    let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );

    let clear_completed_line = source
        .lines()
        .position(|line| line.contains("THEN { todo.completed }"))
        .map(|index| index + 1)
        .expect("TodoMVC should use todo.completed in clear-completed removal");
    let completed_path_hint = report
        .type_hint_table
        .entries
        .iter()
        .find(|entry| {
            entry.line == clear_completed_line
                && entry.category == "path"
                && entry.detail_label == "BOOL"
        })
        .expect("todo.completed should have a hover hint");
    assert!(
        completed_path_hint.detail_label == "BOOL",
        "todo.completed should be widened from list rows, got {}",
        completed_path_hint.detail_label
    );

    let mut in_new_todo = false;
    let completed_field_line = source
        .lines()
        .position(|line| {
            if line.trim_start().starts_with("FUNCTION new_todo") {
                in_new_todo = true;
            }
            in_new_todo && line.trim() == "completed:"
        })
        .map(|index| index + 1)
        .expect("new_todo should define a completed field");
    let completed_field_hint = report
        .type_hint_table
        .entries
        .iter()
        .find(|entry| entry.line == completed_field_line && entry.category == "definition")
        .expect("completed field should have a definition hint");
    assert!(
        completed_field_hint.detail_label == "BOOL",
        "completed HOLD field should be widened from LATEST branches, got {}",
        completed_field_hint.detail_label
    );

    let all_completed_line = source
        .lines()
        .position(|line| line.contains("Bool/and(completed_count > 0)"))
        .map(|index| index + 1)
        .expect("TodoMVC should combine active and completed counts");
    for count_name in ["active_count", "completed_count"] {
        let count_hint = report
            .type_hint_table
            .entries
            .iter()
            .find(|entry| {
                entry.line == all_completed_line
                    && source
                        .get(entry.start..entry.end)
                        .is_some_and(|text| text.trim() == count_name)
            })
            .expect("count path should have a hover hint");
        assert_eq!(
            count_hint.detail_label, "NUMBER",
            "{count_name} should keep its List/count result type on later references"
        );
    }
}
