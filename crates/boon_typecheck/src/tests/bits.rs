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

#[test]
fn bits_builtin_registry_preserves_and_transforms_static_widths() {
    let parsed = boon_parser::parse_source(
        "bits-builtins.bn",
        r#"
left: BITS[8] { 2u1010_0011 }
right: BITS[8] { 2u0000_0101 }
word: BITS[16] { 16u1234 }

bit: left |> Bits/get(position: 1)
set: left |> Bits/set(position: 2, to: True)
slice: left |> Bits/slice(from: 2, count: 3)
concat: left |> Bits/concat(with: right)
masked: left |> Bits/and(with: right)
shifted: left |> Bits/shift_left(by: 2)
extended: left |> Bits/zero_extend(width: 12)
truncated: left |> Bits/truncate(width: 4)
compared: left |> Bits/compare(with: right, interpretation: Unsigned)
widened: left |> Bits/add_widening(with: right, interpretation: Unsigned)
checked_add: left |> Bits/try_add(with: right, interpretation: Unsigned)
checked_subtract: left |> Bits/try_subtract(with: right, interpretation: Unsigned)
converted: 255 |> Number/to_bits(width: 8, interpretation: Unsigned)
as_number: left |> Bits/to_number(interpretation: TwosComplement)
as_bytes: word |> Bits/to_bytes(byte_order: BigEndian)
from_bytes: BYTES[2] { 16u12, 16u34 } |> Bytes/to_bits(width: 16, byte_order: BigEndian)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("valid BITS builtin program");
    let result = |function: &str| {
        let call = checked
            .calls
            .iter()
            .find(|call| call.function == function)
            .unwrap_or_else(|| panic!("missing checked call `{function}`"));
        let expression = checked
            .expressions
            .iter()
            .find(|expression| expression.id == call.expression)
            .unwrap_or_else(|| panic!("missing checked expression for `{function}`"));
        assert_eq!(
            expression.flow_type, call.result,
            "`{function}` expression and call result types diverged"
        );
        call.result.ty.clone()
    };

    assert_eq!(result("Bits/get"), true_false_type());
    assert_eq!(result("Bits/set"), Type::Bits { width: 8 });
    assert_eq!(result("Bits/slice"), Type::Bits { width: 3 });
    assert_eq!(result("Bits/concat"), Type::Bits { width: 16 });
    assert_eq!(result("Bits/and"), Type::Bits { width: 8 });
    assert_eq!(result("Bits/shift_left"), Type::Bits { width: 8 });
    assert_eq!(result("Bits/zero_extend"), Type::Bits { width: 12 });
    assert_eq!(result("Bits/truncate"), Type::Bits { width: 4 });
    assert_eq!(result("Bits/compare"), bits_comparison_type());
    assert_eq!(result("Bits/add_widening"), Type::Bits { width: 9 });
    assert_eq!(
        result("Bits/try_add"),
        bits_added_type(Type::Bits { width: 8 })
    );
    assert_eq!(
        result("Bits/try_subtract"),
        bits_subtracted_type(Type::Bits { width: 8 })
    );
    assert_eq!(
        result("Number/to_bits"),
        bits_converted_type(Type::Bits { width: 8 })
    );
    assert_eq!(result("Bits/to_number"), Type::Number);
    assert_eq!(
        result("Bits/to_bytes"),
        Type::Bytes(BytesType::Fixed(2))
    );
    assert_eq!(result("Bytes/to_bits"), Type::Bits { width: 16 });
}

#[test]
fn bits_builtin_static_domain_errors_fail_before_lowering() {
    for (source, expected) in [
        (
            "value: BITS[8] { 2u1 } |> Bits/and(with: BITS[7] { 2u1 })\n",
            "requires equal-width operands",
        ),
        (
            "value: BITS[8] { 2u1 } |> Bits/get(position: 9)\n",
            "position 9 exceeds BITS[8]",
        ),
        (
            "count: 2\nvalue: BITS[8] { 2u1 } |> Bits/slice(from: 1, count: count)\n",
            "positive compile-time whole Number",
        ),
        (
            "value: BITS[8] { 2u1 } |> Bits/slice(from: 7, count: 3)\n",
            "range from 7 with count 3 exceeds BITS[8]",
        ),
        (
            "value: BITS[8] { 2u1 } |> Bits/shift_left(by: 1 / 2)\n",
            "non-negative whole Number",
        ),
        (
            "value: BITS[8] { 2u1 } |> Bits/zero_extend(width: 7)\n",
            "cannot transform BITS[8] to BITS[7]",
        ),
        (
            "value: BITS[8] { 2u1 } |> Bits/truncate(width: 9)\n",
            "cannot transform BITS[8] to BITS[9]",
        ),
        (
            "value: BITS[7] { 2u1 } |> Bits/to_bytes(byte_order: BigEndian)\n",
            "requires a byte-aligned BITS width",
        ),
        (
            "value: BYTES[1] { 16u12 } |> Bytes/to_bits(width: 16, byte_order: BigEndian)\n",
            "requires exactly 2 byte(s), found 1",
        ),
        (
            "value: BITS[8] { 2u1 } |> Bits/to_number(interpretation: Left)\n",
            "expected: TwosComplement | Unsigned",
        ),
    ] {
        let parsed = boon_parser::parse_source("bits-domain-error.bn", source).unwrap();
        let output = check_program(&parsed);
        assert!(
            output.program.is_none(),
            "invalid BITS source unexpectedly checked: {source}"
        );
        assert!(
            output
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "missing `{expected}` for `{source}` in {:#?}",
            output.report.diagnostics
        );
    }
}

#[test]
fn nested_bits_results_remain_exact_for_later_sibling_expressions() {
    let parsed = boon_parser::parse_source(
        "nested-bits-bindings.bn",
        r#"
store: [
    left: BITS[8] { 16ua3 }
    right: BITS[8] { 16u05 }
    concatenated: left |> Bits/concat(with: right)
    encoded:
        concatenated
        |> Bits/to_bytes(byte_order: BigEndian)
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let named = |path: &str| {
        output
            .report
            .named_value_type_table
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.flow_type.ty.clone())
            .unwrap_or_else(|| panic!("missing named type `{path}`"))
    };
    assert_eq!(named("store.concatenated"), Type::Bits { width: 16 });
    assert_eq!(
        named("store.encoded"),
        Type::Bytes(BytesType::Fixed(2))
    );
}
