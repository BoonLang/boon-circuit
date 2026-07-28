#![cfg(target_arch = "wasm32")]

use boon_data::ExactNumber;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn canonical_exact_number_equality_order_and_hash_match_native_contract() {
    let positive_zero = ExactNumber::zero();
    let negative_zero = "-0.0".parse::<ExactNumber>().unwrap();
    let decimal = "59.91".parse::<ExactNumber>().unwrap();

    assert_eq!(positive_zero, negative_zero);
    assert_eq!(decimal.to_string(), "59.91");
    assert!("NaN".parse::<ExactNumber>().is_err());
    assert!("Infinity".parse::<ExactNumber>().is_err());

    let hash = |value: &ExactNumber| {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(hash(&positive_zero), hash(&negative_zero));
    assert_eq!(ExactNumber::one(), "1.0".parse().unwrap());
    assert_eq!(
        "9007199254740993".parse::<ExactNumber>().unwrap(),
        ExactNumber::from_i64(9_007_199_254_740_993)
    );
    assert!(
        "0.1"
            .parse::<ExactNumber>()
            .unwrap()
            .checked_add(&"0.2".parse().unwrap())
            .unwrap()
            == "0.3".parse().unwrap()
    );
}
