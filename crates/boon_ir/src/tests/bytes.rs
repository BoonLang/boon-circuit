// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

#[test]
fn bytes_set_lowers_a_fixed_single_byte_value() {
    let parsed = boon_parser::parse_source(
        "bytes-set-fixed-single-byte.bn",
        r#"
store: [
    payload: BYTES[2] { 16u01, 16u02 }
    patch: SOURCE
    patched:
        BYTES[2] {} |> HOLD patched {
            store.patch |> THEN {
                store.payload |> Bytes/set(index: 1, value: BYTES[1] { 16u5A })
            }
        }
]

document: Document/new(root: Element/label(element: [], label: TEXT { Bytes }))
"#,
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let branch = ir
        .update_branches
        .iter()
        .find(|branch| branch.target == "store.patched")
        .expect("missing Bytes/set update branch");

    assert_eq!(
        branch.expression,
        UpdateExpression::BytesSet {
            path: "store.payload".to_owned(),
            index: 1,
            value: 0x5A,
        }
    );
}

#[test]
fn bytes_encoding_update_expressions_lower_from_pipe_and_call_forms() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_encoding_plan_ops.bn",
        include_str!("../../../../examples/bytes_encoding_plan_ops.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let branch_for = |target: &str| {
        ir.update_branches
            .iter()
            .find(|branch| branch.target == target)
            .unwrap_or_else(|| panic!("missing update branch for {target}"))
    };
    assert_eq!(
        branch_for("store.hex").expression,
        UpdateExpression::BytesToHex {
            path: "store.joined".to_owned()
        }
    );
    assert_eq!(
        branch_for("store.base64").expression,
        UpdateExpression::BytesToBase64 {
            path: "store.joined".to_owned()
        }
    );
    assert_eq!(
        branch_for("store.zeros").expression,
        UpdateExpression::BytesZeros { byte_count: 4 }
    );
    assert_eq!(
        branch_for("store.decoded_hex").expression,
        UpdateExpression::BytesFromHex {
            path: "hex_input".to_owned()
        }
    );
    assert_eq!(
        branch_for("store.decoded_base64").expression,
        UpdateExpression::BytesFromBase64 {
            path: "base64_input".to_owned()
        }
    );
    assert_eq!(
        branch_for("store.decoded_base64_hex").expression,
        UpdateExpression::BytesToHex {
            path: "store.decoded_base64".to_owned()
        }
    );
    verify_hidden_identity(&ir).unwrap();
}


#[test]
fn bytes_set_conversion_bank_update_expressions_lower_from_fixed_bank_fixture() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_set_conversion_bank_plan_ops.bn",
        include_str!("../../../../examples/bytes_set_conversion_bank_plan_ops.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let branch_for = |target: &str| {
        ir.update_branches
            .iter()
            .find(|branch| branch.target == target)
            .unwrap_or_else(|| panic!("missing update branch for {target}"))
    };
    assert_eq!(
        branch_for("store.patched").expression,
        UpdateExpression::BytesSet {
            path: "left_payload".to_owned(),
            index: 1,
            value: 0x5A,
        }
    );
    assert_eq!(
        branch_for("store.text").expression,
        UpdateExpression::BytesToText {
            path: "store.patched".to_owned(),
            encoding: "Utf8".to_owned(),
        }
    );
    assert_eq!(
        branch_for("store.hex").expression,
        UpdateExpression::BytesToHex {
            path: "store.patched".to_owned(),
        }
    );
    assert_eq!(
        branch_for("store.base64").expression,
        UpdateExpression::BytesToBase64 {
            path: "store.patched".to_owned(),
        }
    );
    verify_hidden_identity(&ir).unwrap();
}


#[test]
fn bytes_text_conversion_update_expressions_require_explicit_encoding() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_text_conversion_plan_ops.bn",
        include_str!("../../../../examples/bytes_text_conversion_plan_ops.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let branch_for = |target: &str| {
        ir.update_branches
            .iter()
            .find(|branch| branch.target == target)
            .unwrap_or_else(|| panic!("missing update branch for {target}"))
    };
    assert_eq!(
        branch_for("store.encoded").expression,
        UpdateExpression::TextToBytes {
            path: "text_payload".to_owned(),
            encoding: "Utf8".to_owned(),
        }
    );
    assert_eq!(
        branch_for("store.decoded").expression,
        UpdateExpression::BytesToText {
            path: "store.encoded".to_owned(),
            encoding: "Utf8".to_owned(),
        }
    );

    let missing = boon_parser::parse_source(
        "missing-bytes-encoding-ir.bn",
        r#"
store: [
encode: SOURCE
text: TEXT { hi }
encoded:
    BYTES {} |> HOLD encoded {
        store.encode |> THEN { store.text |> Text/to_bytes() }
    }
]
document: Document/new(root: Element/label(element: [], label: TEXT { Missing encoding }))
"#,
    )
    .unwrap();
    let error = lower(&missing).expect_err("missing encoding must fail before IR lowering");
    assert!(
        error
            .to_string()
            .contains("requires explicit `encoding: Utf8|Ascii`"),
        "unexpected error: {error}"
    );
}


#[test]
fn bytes_numeric_update_expressions_lower_from_pipe_and_call_forms() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_numeric_plan_ops.bn",
        include_str!("../../../../examples/bytes_numeric_plan_ops.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let branch_for = |target: &str| {
        ir.update_branches
            .iter()
            .find(|branch| branch.target == target)
            .unwrap_or_else(|| panic!("missing update branch for {target}"))
    };
    assert_eq!(
        branch_for("store.read_u16_le").expression,
        UpdateExpression::BytesReadUnsigned {
            path: "payload".to_owned(),
            offset: 0,
            byte_count: 2,
            endian: "Little".to_owned(),
        }
    );
    assert_eq!(
        branch_for("store.read_u16_be").expression,
        UpdateExpression::BytesReadUnsigned {
            path: "payload".to_owned(),
            offset: 0,
            byte_count: 2,
            endian: "Big".to_owned(),
        }
    );
    assert_eq!(
        branch_for("store.read_i16_be").expression,
        UpdateExpression::BytesReadSigned {
            path: "payload".to_owned(),
            offset: 2,
            byte_count: 2,
            endian: "Big".to_owned(),
        }
    );
    assert_eq!(
        branch_for("store.read_i8").expression,
        UpdateExpression::BytesReadSigned {
            path: "payload".to_owned(),
            offset: 5,
            byte_count: 1,
            endian: "Little".to_owned(),
        }
    );
    assert_eq!(
        branch_for("store.written_unsigned").expression,
        UpdateExpression::BytesWriteUnsigned {
            path: "payload".to_owned(),
            offset: 6,
            byte_count: 2,
            endian: "Big".to_owned(),
            value: 4660,
        }
    );
    assert_eq!(
        branch_for("store.written_signed").expression,
        UpdateExpression::BytesWriteSigned {
            path: "payload".to_owned(),
            offset: 4,
            byte_count: 2,
            endian: "Little".to_owned(),
            value: -129,
        }
    );
    assert_eq!(
        branch_for("store.written_unsigned_read").expression,
        UpdateExpression::BytesReadUnsigned {
            path: "store.written_unsigned".to_owned(),
            offset: 6,
            byte_count: 2,
            endian: "Big".to_owned(),
        }
    );
    assert_eq!(
        branch_for("store.written_signed_read").expression,
        UpdateExpression::BytesReadSigned {
            path: "store.written_signed".to_owned(),
            offset: 4,
            byte_count: 2,
            endian: "Little".to_owned(),
        }
    );
    verify_hidden_identity(&ir).unwrap();
}


#[test]
fn row_local_bare_source_identifier_lowers_indexed_bytes_reads() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_indexed_source_payload_plan_ops.bn",
        include_str!("../../../../examples/bytes_indexed_source_payload_plan_ops.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();

    assert!(ir.update_branches.iter().any(|branch| {
        branch.indexed
            && branch.target == "store.rows.payload_len"
            && branch.source == "store.rows.inspect"
            && branch.expression
                == UpdateExpression::BytesLength {
                    path: "store.rows.payload".to_owned(),
                }
    }));
    assert!(ir.update_branches.iter().any(|branch| {
        branch.indexed
            && branch.target == "store.rows.payload_second"
            && branch.source == "store.rows.inspect"
            && branch.expression
                == UpdateExpression::BytesGet {
                    path: "store.rows.payload".to_owned(),
                    index: 1,
                }
    }));
    assert!(!ir.update_branches.iter().any(|branch| {
        branch.source == "store.rows.receive"
            && matches!(
                branch.target.as_str(),
                "store.rows.payload_len" | "store.rows.payload_second"
            )
    }));
}


#[test]
fn lower_rejects_duplicate_direct_latest_source_branches() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/bytes_indexed_duplicate_update_conflict_plan_ops.bn");
    let source = std::fs::read_to_string(&fixture).unwrap();
    let parsed = boon_parser::parse_source(fixture.display().to_string(), &source).unwrap();
    let error = lower(&parsed)
        .expect_err("semantic IR lowering must not silently drop duplicate LATEST branches");
    assert!(
        error.contains("duplicate direct `LATEST` branch"),
        "unexpected duplicate LATEST lowering error: {error}"
    );
    assert!(
        error.contains("receive.bytes"),
        "duplicate LATEST error should identify the source trigger: {error}"
    );
}
