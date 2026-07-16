#![cfg(target_arch = "wasm32")]

use boon_transport_json::{FiniteReal, Limits, Value, decode, encode};
use std::collections::BTreeMap;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn wasm_uses_the_same_canonical_codec() {
    let input = br#"{"a":[null,true,51.5074,"\uD834\uDD1E"],"z":{"nested":false}}"#;
    let value = decode(input, &Limits::default()).unwrap();
    assert!(matches!(value, Value::Record(ref fields) if fields.contains_key("a")));
    assert_eq!(
        encode(&value, &Limits::default()).unwrap(),
        "{\"a\":[null,true,51.5074,\"\u{1d11e}\"],\"z\":{\"nested\":false}}".as_bytes()
    );

    let canonical = Value::Record(BTreeMap::from([
        ("a".to_owned(), Value::Bool(true)),
        ("z".to_owned(), Value::Null),
    ]));
    assert_eq!(
        encode(&canonical, &Limits::default()).unwrap(),
        br#"{"a":true,"z":null}"#
    );
}

#[wasm_bindgen_test]
fn wasm_generated_values_round_trip_deterministically() {
    for index in 0..512i64 {
        let value = Value::Record(BTreeMap::from([
            (
                "items".to_owned(),
                Value::List(vec![
                    Value::Null,
                    Value::Bool(index % 2 == 0),
                    Value::Number(FiniteReal::new(index as f64 / 10.0).unwrap()),
                ]),
            ),
            (
                "text".to_owned(),
                Value::Text(format!("row {index}: quote\" line\n \u{20ac} \u{1f6a2}")),
            ),
        ]));
        let encoded = encode(&value, &Limits::default()).unwrap();
        assert_eq!(decode(&encoded, &Limits::default()).unwrap(), value);
        assert_eq!(
            encode(
                &decode(&encoded, &Limits::default()).unwrap(),
                &Limits::default()
            )
            .unwrap(),
            encoded
        );
    }
}
