#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn stored_value_public_path_is_the_canonical_structural_type() {
    let public = boon_persistence::StoredValue::integer(7).unwrap();
    let canonical: boon_data::Value = public.clone();
    let round_trip: boon_persistence::StoredValue = canonical;

    assert_eq!(round_trip, public);
}
