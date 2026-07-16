#![cfg(target_arch = "wasm32")]

use boon_data::FiniteReal;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn canonical_number_bits_equality_order_and_hash_match_native_contract() {
    let positive_zero = FiniteReal::new(0.0).unwrap();
    let negative_zero = FiniteReal::new(-0.0).unwrap();
    let decimal = FiniteReal::new(59.91).unwrap();

    assert_eq!(positive_zero, negative_zero);
    assert_eq!(positive_zero.get().to_bits(), 0);
    assert_eq!(decimal.get().to_bits(), 0x404d_f47a_e147_ae14);
    assert!(FiniteReal::new(f64::NAN).is_err());
    assert!(FiniteReal::new(f64::INFINITY).is_err());

    let hash = |value: FiniteReal| {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(hash(positive_zero), hash(negative_zero));
    assert_eq!(
        FiniteReal::from_i64_exact(1).unwrap(),
        "1.0".parse().unwrap()
    );
    assert!(FiniteReal::from_i64_exact(9_007_199_254_740_993).is_err());
}
