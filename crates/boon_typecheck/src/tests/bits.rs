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
