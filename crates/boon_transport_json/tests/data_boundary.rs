#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn codec_accepts_and_returns_canonical_data_values_directly() {
    let value = boon_data::Value::List(vec![
        boon_data::Value::Null,
        boon_data::Value::integer(7).unwrap(),
    ]);

    let encoded = boon_transport_json::encode(&value, &boon_transport_json::Limits::default())
        .expect("canonical data value should encode");
    let decoded: boon_data::Value =
        boon_transport_json::decode(&encoded, &boon_transport_json::Limits::default())
            .expect("canonical JSON should decode");

    assert_eq!(decoded, value);
}
