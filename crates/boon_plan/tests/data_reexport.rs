#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn exact_number_public_path_is_the_canonical_data_type() {
    let public = "59.91".parse::<boon_plan::ExactNumber>().unwrap();
    let canonical: &boon_data::ExactNumber = &public;

    assert_eq!(canonical, &public);
}
