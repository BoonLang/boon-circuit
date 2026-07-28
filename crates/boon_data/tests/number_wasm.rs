#![cfg(target_arch = "wasm32")]

use boon_data::{ExactNumber, ExactRoundingRule};
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

#[wasm_bindgen_test]
fn exact_rounding_rules_match_the_native_contract() {
    let number = |value: &str| value.parse::<ExactNumber>().unwrap();
    for (value, quantum, rule, expected) in [
        ("5/2", "1", ExactRoundingRule::NearestEven, "2"),
        ("-5/2", "1", ExactRoundingRule::NearestAwayFromZero, "-3"),
        ("-7/3", "1", ExactRoundingRule::TowardZero, "-2"),
        ("-7/3", "1", ExactRoundingRule::TowardPositive, "-2"),
        ("7/3", "1", ExactRoundingRule::TowardNegative, "2"),
        ("7/3", "1", ExactRoundingRule::AwayFromZero, "3"),
        ("10/3", "0.01", ExactRoundingRule::NearestEven, "3.33"),
    ] {
        assert_eq!(
            number(value).round_to(&number(quantum), rule).unwrap(),
            number(expected)
        );
    }
    assert!(
        number("1")
            .round_to(&ExactNumber::zero(), ExactRoundingRule::NearestEven)
            .is_err()
    );
}
