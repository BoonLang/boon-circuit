use boon_transport_json::{
    BoundedJsonInput, DiagnosticCode, FiniteReal, Limits, MAX_DIAGNOSTIC_BYTES,
    MAX_EXACT_NUMBER_BOUND, MAX_SUPPORTED_DEPTH, Value, decode, encode,
};
use std::collections::BTreeMap;

#[test]
fn validates_policy_bounds() {
    let invalid_depth = Limits {
        max_depth: MAX_SUPPORTED_DEPTH + 1,
        ..Limits::default()
    };
    assert_eq!(
        decode(b"null", &invalid_depth).unwrap_err().code,
        DiagnosticCode::InvalidLimits
    );

    if let Some(too_large) = MAX_EXACT_NUMBER_BOUND.checked_add(1) {
        let invalid_input = Limits {
            max_input_bytes: too_large,
            ..Limits::default()
        };
        assert_eq!(
            BoundedJsonInput::new(invalid_input).unwrap_err().code,
            DiagnosticCode::InvalidLimits
        );
    }
}

#[test]
fn input_bytes_are_bounded_before_decode_and_during_stream_assembly() {
    let limits = Limits {
        max_input_bytes: 4,
        ..Limits::default()
    };
    assert_eq!(decode(b"null", &limits).unwrap(), Value::Null);
    let diagnostic = decode(b"false", &limits).unwrap_err();
    assert_eq!(diagnostic.code, DiagnosticCode::InputTooLarge);
    assert_eq!(diagnostic.offset, 4);

    let mut input = BoundedJsonInput::new(limits).unwrap();
    input.push(b"nu").unwrap();
    input.push(b"ll").unwrap();
    let diagnostic = input.push(b"x").unwrap_err();
    assert_eq!(diagnostic.code, DiagnosticCode::InputTooLarge);
    assert_eq!(input.as_bytes(), b"null");
    assert_eq!(input.finish().unwrap(), Value::Null);
}

#[test]
fn depth_and_node_limits_are_independent() {
    let no_children = Limits {
        max_depth: 0,
        ..Limits::default()
    };
    assert_eq!(decode(b"[]", &no_children).unwrap(), Value::List(vec![]));
    assert_eq!(
        decode(b"[null]", &no_children).unwrap_err().code,
        DiagnosticCode::DepthLimit
    );

    let one_node = Limits {
        max_nodes: 1,
        ..Limits::default()
    };
    assert_eq!(
        decode(b"{}", &one_node).unwrap(),
        Value::Record(BTreeMap::new())
    );
    assert_eq!(
        decode(br#"{"a":null}"#, &one_node).unwrap_err().code,
        DiagnosticCode::NodeLimit
    );
}

#[test]
fn decoded_string_byte_limit_applies_to_values_and_keys() {
    let three_bytes = Limits {
        max_string_bytes: 3,
        ..Limits::default()
    };
    assert_eq!(
        decode(br#""\u20ac""#, &three_bytes).unwrap(),
        Value::Text("\u{20ac}".to_owned())
    );

    let two_bytes = Limits {
        max_string_bytes: 2,
        ..Limits::default()
    };
    assert_eq!(
        decode(br#""\u20ac""#, &two_bytes).unwrap_err().code,
        DiagnosticCode::StringLimit
    );
    assert_eq!(
        decode(br#"{"long":0}"#, &two_bytes).unwrap_err().code,
        DiagnosticCode::StringLimit
    );
}

#[test]
fn array_and_object_limits_are_independent() {
    let arrays = Limits {
        max_array_items: 2,
        max_object_fields: 10,
        ..Limits::default()
    };
    assert!(decode(b"[0,1]", &arrays).is_ok());
    assert_eq!(
        decode(b"[0,1,2]", &arrays).unwrap_err().code,
        DiagnosticCode::ArrayItemsLimit
    );

    let objects = Limits {
        max_array_items: 10,
        max_object_fields: 2,
        ..Limits::default()
    };
    assert!(decode(br#"{"a":0,"b":1}"#, &objects).is_ok());
    assert_eq!(
        decode(br#"{"a":0,"b":1,"c":2}"#, &objects)
            .unwrap_err()
            .code,
        DiagnosticCode::ObjectFieldsLimit
    );
}

#[test]
fn encoder_enforces_output_and_structural_limits_while_writing() {
    let exactly_four = Limits {
        max_output_bytes: 4,
        ..Limits::default()
    };
    assert_eq!(encode(&Value::Null, &exactly_four).unwrap(), b"null");
    assert_eq!(
        encode(&Value::Bool(false), &exactly_four).unwrap_err().code,
        DiagnosticCode::OutputTooLarge
    );

    let value = Value::List(vec![Value::List(vec![Value::Bool(true)])]);
    let shallow = Limits {
        max_depth: 1,
        ..Limits::default()
    };
    assert_eq!(
        encode(&value, &shallow).unwrap_err().code,
        DiagnosticCode::DepthLimit
    );

    let one_node = Limits {
        max_nodes: 1,
        ..Limits::default()
    };
    assert_eq!(
        encode(&Value::List(vec![Value::Null]), &one_node)
            .unwrap_err()
            .code,
        DiagnosticCode::NodeLimit
    );

    let short_strings = Limits {
        max_string_bytes: 2,
        ..Limits::default()
    };
    assert_eq!(
        encode(&Value::Text("abc".to_owned()), &short_strings)
            .unwrap_err()
            .code,
        DiagnosticCode::StringLimit
    );

    let short_arrays = Limits {
        max_array_items: 1,
        ..Limits::default()
    };
    assert_eq!(
        encode(&Value::List(vec![Value::Null, Value::Null]), &short_arrays)
            .unwrap_err()
            .code,
        DiagnosticCode::ArrayItemsLimit
    );

    let short_objects = Limits {
        max_object_fields: 1,
        ..Limits::default()
    };
    assert_eq!(
        encode(
            &Value::Record(BTreeMap::from([
                ("a".to_owned(), Value::Null),
                ("b".to_owned(), Value::Null),
            ])),
            &short_objects,
        )
        .unwrap_err()
        .code,
        DiagnosticCode::ObjectFieldsLimit
    );
}

#[test]
fn diagnostic_text_and_offsets_remain_bounded() {
    let limits = Limits {
        max_diagnostic_bytes: 17,
        ..Limits::default()
    };
    let key = "x".repeat(500);
    let input = format!("{{\"{key}\":0,\"{key}\":1}}");
    let diagnostic = decode(input.as_bytes(), &limits).unwrap_err();
    assert_eq!(diagnostic.code, DiagnosticCode::DuplicateKey);
    assert!(diagnostic.offset <= input.len());
    assert!(diagnostic.message.len() <= 17);
    assert!(
        diagnostic
            .message
            .is_char_boundary(diagnostic.message.len())
    );

    let hard_capped = Limits {
        max_diagnostic_bytes: usize::MAX,
        max_depth: MAX_SUPPORTED_DEPTH + 1,
        ..Limits::default()
    };
    let diagnostic = decode(b"null", &hard_capped).unwrap_err();
    assert!(diagnostic.message.len() <= MAX_DIAGNOSTIC_BYTES);

    let multiline = b"{\n  \"a\": 1,\n  \"a\": 2\n}";
    let diagnostic = decode(multiline, &Limits::default()).unwrap_err();
    assert_eq!(diagnostic.code, DiagnosticCode::DuplicateKey);
    assert!(diagnostic.offset >= 10);
    assert!(diagnostic.offset <= multiline.len());
}

#[test]
fn finite_real_prevents_non_finite_values_before_encoding() {
    assert!(FiniteReal::new(f64::NAN).is_err());
    assert!(FiniteReal::new(f64::INFINITY).is_err());
    assert!(FiniteReal::new(f64::NEG_INFINITY).is_err());
}
