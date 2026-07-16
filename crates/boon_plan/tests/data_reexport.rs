#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn finite_real_public_path_is_the_canonical_data_type() {
    let public = boon_plan::FiniteReal::new(59.91).unwrap();
    let canonical: boon_data::FiniteReal = public;

    assert_eq!(canonical.get(), 59.91);
}
