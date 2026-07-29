#[test]
fn checked_number_literals_and_patterns_are_canonical_exact_values() {
    let parsed = boon_parser::parse_source(
        "checked-exact-numbers.bn",
        r#"
whole: 1
equivalent: 1.00
fraction: 0.125
selected:
    whole
    |> WHEN {
        1.0 => Same
        __ => Different
    }
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("valid exact Number program");
    let numbers = checked
        .expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            CheckedExpressionKind::Number { value } => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(numbers.iter().filter(|value| *value == &ExactNumber::one()).count() >= 2);
    assert!(numbers.contains(&"1/8".parse::<ExactNumber>().unwrap()));

    let pattern = checked
        .expressions
        .iter()
        .find_map(|expression| match &expression.kind {
            CheckedExpressionKind::MatchArm {
                pattern: CheckedMatchPattern::Number { value },
                ..
            } => Some(value),
            _ => None,
        })
        .expect("checked exact Number pattern");
    assert_eq!(pattern, &ExactNumber::one());
}

#[test]
fn checked_number_literals_fail_closed_at_the_parse_budget() {
    let source = format!(
        "value: {}\n",
        "9".repeat(boon_data::MAX_NUMBER_PARSED_DIGITS + 1)
    );
    let parsed = boon_parser::parse_source("number-budget.bn", &source).unwrap();
    let output = check_program(&parsed);
    assert!(output.program.is_none());
    assert!(output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("invalid exact Number literal")
            && diagnostic.message.contains("digit budget")
    }));
}

#[test]
fn duration_metadata_requires_exact_whole_milliseconds() {
    assert_eq!(
        exact_duration_milliseconds(&"0.001".parse().unwrap(), 1_000),
        Some(1)
    );
    assert_eq!(
        exact_duration_milliseconds(&"0.0005".parse().unwrap(), 1_000),
        None
    );
    assert_eq!(
        exact_duration_milliseconds(&"-1".parse().unwrap(), 1),
        None
    );
}
