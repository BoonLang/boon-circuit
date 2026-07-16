use boon_transport_json::{
    DiagnosticCode, FiniteReal, JSON_DECODE_FAILED_TAG, JSON_DECODED_TAG, JSON_DIAGNOSTIC_TAG,
    JSON_ENCODE_FAILED_TAG, JSON_ENCODED_TAG, Limits, Value, decode, decode_boon, encode,
    encode_boon,
};
use std::collections::BTreeMap;

fn number(text: &str) -> Value {
    Value::Number(text.parse::<FiniteReal>().unwrap())
}

#[test]
fn maps_every_json_kind_to_canonical_boon_values() {
    let input = br#"{
        "array": [null, true, false, -12, 51.5074, "fjord"],
        "object": {"nested": []}
    }"#;
    let decoded = decode(input, &Limits::default()).unwrap();
    assert_eq!(
        decoded,
        Value::Record(BTreeMap::from([
            (
                "array".to_owned(),
                Value::List(vec![
                    Value::Null,
                    Value::Bool(true),
                    Value::Bool(false),
                    number("-12"),
                    number("51.5074"),
                    Value::Text("fjord".to_owned()),
                ]),
            ),
            (
                "object".to_owned(),
                Value::Record(BTreeMap::from([(
                    "nested".to_owned(),
                    Value::List(Vec::new()),
                )])),
            ),
        ]))
    );
}

#[test]
fn utf8_escapes_and_surrogate_pairs_are_decoded_by_json_rules() {
    let input = br#""line\nquote\"slash\\solidus\/tab\t/\b\f\r/\u20ac/\uD834\uDD1E""#;
    assert_eq!(
        decode(input, &Limits::default()).unwrap(),
        Value::Text("line\nquote\"slash\\solidus/tab\t/\u{8}\u{c}\r/\u{20ac}/\u{1d11e}".to_owned())
    );

    let direct_utf8 = "\"Tromso - \u{5317} - \u{1f6a2}\"";
    assert_eq!(
        decode(direct_utf8.as_bytes(), &Limits::default()).unwrap(),
        Value::Text("Tromso - \u{5317} - \u{1f6a2}".to_owned())
    );
}

#[test]
fn invalid_utf8_and_invalid_surrogates_are_rejected() {
    let invalid_utf8 = [b'"', 0xf0, 0x28, 0x8c, 0x28, b'"'];
    let diagnostic = decode(&invalid_utf8, &Limits::default()).unwrap_err();
    assert_eq!(diagnostic.code, DiagnosticCode::InvalidUtf8);
    assert_eq!(diagnostic.offset, 1);

    for input in [
        br#""\uD834""#.as_slice(),
        br#""\uDD1E""#.as_slice(),
        br#""\uD834\u0041""#.as_slice(),
    ] {
        let diagnostic = decode(input, &Limits::default()).unwrap_err();
        assert_eq!(diagnostic.code, DiagnosticCode::InvalidSyntax);
        assert!(diagnostic.offset <= input.len());
    }
}

#[test]
fn exponents_are_finite_and_use_the_canonical_number_type() {
    let decoded = decode(b"[6.022e23,-1.25E-2,5e-324,-0]", &Limits::default()).unwrap();
    assert_eq!(
        decoded,
        Value::List(vec![
            number("6.022e23"),
            number("-1.25E-2"),
            number("5e-324"),
            Value::Number(FiniteReal::ZERO),
        ])
    );

    let overflow = decode(b"1e400", &Limits::default()).unwrap_err();
    assert_eq!(overflow.code, DiagnosticCode::NumberOutOfRange);

    let inexact_integer = decode(b"9007199254740993", &Limits::default()).unwrap_err();
    assert_eq!(inexact_integer.code, DiagnosticCode::NumberOutOfRange);
    assert_eq!(
        decode(b"9007199254740992", &Limits::default()).unwrap(),
        number("9007199254740992")
    );

    for malformed in [
        b"1e".as_slice(),
        b"1e+".as_slice(),
        b".1".as_slice(),
        b"01".as_slice(),
        b"NaN".as_slice(),
        b"Infinity".as_slice(),
    ] {
        assert_eq!(
            decode(malformed, &Limits::default()).unwrap_err().code,
            DiagnosticCode::InvalidSyntax
        );
    }
}

#[test]
fn unknown_and_missing_fields_remain_data_for_boon_validation() {
    let Value::Record(fields) = decode(
        br#"{"known":1,"unknown":{"nested":true}}"#,
        &Limits::default(),
    )
    .unwrap() else {
        panic!("JSON object did not decode to a record");
    };
    assert_eq!(fields["known"], number("1"));
    assert!(matches!(fields["unknown"], Value::Record(_)));
    assert!(!fields.contains_key("missing"));
}

#[test]
fn duplicate_keys_are_rejected_after_escape_decoding() {
    for input in [
        br#"{"a":1,"a":2}"#.as_slice(),
        br#"{"a":1,"\u0061":2}"#.as_slice(),
        br#"{"outer":{"same":1,"same":2}}"#.as_slice(),
    ] {
        let diagnostic = decode(input, &Limits::default()).unwrap_err();
        assert_eq!(diagnostic.code, DiagnosticCode::DuplicateKey);
        assert!(diagnostic.offset > 0);
        assert!(diagnostic.offset <= input.len());
    }
}

#[test]
fn canonical_encoding_orders_keys_and_uses_deterministic_escaping() {
    let value = Value::Record(BTreeMap::from([
        ("z".to_owned(), Value::Text("last".to_owned())),
        (
            "a".to_owned(),
            Value::Text("quote\" slash/ backslash\\ line\n nul\0".to_owned()),
        ),
        ("e".to_owned(), number("1.25")),
        ("\u{e9}".to_owned(), Value::Bool(true)),
    ]));
    let first = encode(&value, &Limits::default()).unwrap();
    let second = encode(&value, &Limits::default()).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        String::from_utf8(first).unwrap(),
        "{\"a\":\"quote\\\" slash/ backslash\\\\ line\\n nul\\u0000\",\"e\":1.25,\"z\":\"last\",\"é\":true}"
    );
}

#[test]
fn typed_boon_results_use_explicit_variants_and_bounded_diagnostics() {
    let Value::Variant { tag, fields } = decode_boon(b"true", &Limits::default()) else {
        panic!("decode result is not a variant");
    };
    assert_eq!(tag, JSON_DECODED_TAG);
    assert_eq!(fields["value"], Value::Bool(true));

    let Value::Variant { tag, fields } = decode_boon(br#"{"a":1,"a":2}"#, &Limits::default())
    else {
        panic!("decode failure is not a variant");
    };
    assert_eq!(tag, JSON_DECODE_FAILED_TAG);
    let Value::Variant {
        tag: diagnostic_tag,
        fields: diagnostic,
    } = &fields["diagnostic"]
    else {
        panic!("failure diagnostic is not a variant");
    };
    assert_eq!(diagnostic_tag, JSON_DIAGNOSTIC_TAG);
    assert_eq!(diagnostic["code"], Value::Text("duplicate_key".to_owned()));
    assert!(matches!(diagnostic["offset"], Value::Number(_)));

    let Value::Variant { tag, fields } = encode_boon(&Value::Bool(false), &Limits::default())
    else {
        panic!("encode result is not a variant");
    };
    assert_eq!(tag, JSON_ENCODED_TAG);
    assert_eq!(fields["text"], Value::Text("false".to_owned()));
}

#[test]
fn variants_require_explicit_boon_domain_to_wire_mapping() {
    let value = Value::Variant {
        tag: "Selected".to_owned(),
        fields: BTreeMap::from([("id".to_owned(), Value::Text("NSR:Stop:1".to_owned()))]),
    };
    let diagnostic = encode(&value, &Limits::default()).unwrap_err();
    assert_eq!(diagnostic.code, DiagnosticCode::UnsupportedValue);
    assert!(diagnostic.message.contains("no implicit JSON wire"));

    let Value::Variant { tag, fields } = encode_boon(&value, &Limits::default()) else {
        panic!("encode failure is not a variant");
    };
    assert_eq!(tag, JSON_ENCODE_FAILED_TAG);
    assert!(matches!(
        fields["diagnostic"],
        Value::Variant { ref tag, .. } if tag == JSON_DIAGNOSTIC_TAG
    ));
}
