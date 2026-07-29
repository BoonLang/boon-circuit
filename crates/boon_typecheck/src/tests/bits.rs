#[test]
fn bits_literals_are_canonical_width_aware_checked_values() {
    let parsed = boon_parser::parse_source(
        "bits-width.bn",
        "opcode: BITS[7] { 2u0110011 }\n",
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );

    let checked = output.program.expect("valid BITS program");
    let opcode = checked
        .expressions
        .iter()
        .find(|expression| matches!(expression.kind, CheckedExpressionKind::Bits { .. }))
        .expect("checked BITS literal");
    assert_eq!(opcode.flow_type.ty, Type::Bits { width: 7 });
    let CheckedExpressionKind::Bits { value } = &opcode.kind else {
        unreachable!();
    };
    assert_eq!(value.width(), 7);
    assert_eq!(value.to_string(), "BITS[7] { 2u0110011 }");
}

#[test]
fn bits_literals_reject_overflow_and_exact_patterns_reject_other_widths() {
    let overflow =
        boon_parser::parse_source("bits-overflow.bn", "value: BITS[3] { 2u1000 }\n").unwrap();
    let overflow = check_program(&overflow);
    assert!(overflow.program.is_none());
    assert!(overflow
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("does not fit BITS[3]")));

    let mismatch = boon_parser::parse_source(
        "bits-pattern-width.bn",
        r#"
opcode: BITS[7] { 2u0110011 }
decoded:
    opcode
    |> WHEN {
        BITS[8] { 2u00110011 } => Register
        __ => Unknown
    }
"#,
    )
    .unwrap();
    let mismatch = check_program(&mismatch);
    assert!(mismatch.program.is_none());
    assert!(mismatch.report.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("exact bit pattern")
            && diagnostic.message.contains("BITS[7]")
    }));
}

#[test]
fn bits_types_are_exact_and_key_safe() {
    assert!(type_is_map_key_safe(&Type::Bits { width: 257 }));
    assert!(type_is_assignable_to(
        &Type::Bits { width: 7 },
        &Type::Bits { width: 7 }
    ));
    assert!(!type_is_assignable_to(
        &Type::Bits { width: 7 },
        &Type::Bits { width: 8 }
    ));
    assert!(!type_is_assignable_to(
        &Type::Number,
        &Type::Bits { width: 7 }
    ));
}
