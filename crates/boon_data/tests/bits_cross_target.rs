#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

use boon_data::{
    Bits, BitsArithmeticFailure, BitsByteOrder, BitsDirection, BitsInterpretation, ExactNumber,
};
use std::cmp::Ordering;

fn bits(width: u32, digits: &str) -> Bits {
    Bits::parse_encoded(width, 16, digits).unwrap()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fixed_width_bits_have_one_native_and_wasm_operation_contract() {
    let left = bits(8, "a3");
    let right = bits(8, "05");

    assert!(left.bit(1, BitsDirection::Left).unwrap());
    assert!(left.bit(1, BitsDirection::Right).unwrap());
    assert_eq!(
        left.with_bit(2, BitsDirection::Left, true).unwrap(),
        bits(8, "e3")
    );
    assert_eq!(left.slice(2, 3).unwrap(), bits(3, "2"));
    assert_eq!(left.with_slice(2, &bits(3, "7")).unwrap(), bits(8, "f3"));
    assert_eq!(left.concat(&right).unwrap(), bits(16, "a305"));
    assert_eq!(left.bit_and(&right).unwrap(), bits(8, "01"));
    assert_eq!(left.bit_or(&right).unwrap(), bits(8, "a7"));
    assert_eq!(left.bit_xor(&right).unwrap(), bits(8, "a6"));
    assert_eq!(left.bit_not().unwrap(), bits(8, "5c"));
    assert_eq!(left.logical_shift_left(2).unwrap(), bits(8, "8c"));
    assert_eq!(left.logical_shift_right(2).unwrap(), bits(8, "28"));
    assert_eq!(left.arithmetic_shift_right(2).unwrap(), bits(8, "e8"));
    assert_eq!(left.rotate_left(2).unwrap(), bits(8, "8e"));
    assert_eq!(left.rotate_right(2).unwrap(), bits(8, "e8"));
    assert_eq!(left.zero_extend(12).unwrap(), bits(12, "0a3"));
    assert_eq!(left.sign_extend(12).unwrap(), bits(12, "fa3"));
    assert_eq!(left.truncate(4).unwrap(), bits(4, "3"));
    assert_eq!(
        left.compare(&right, BitsInterpretation::Unsigned).unwrap(),
        Ordering::Greater
    );
    assert_eq!(
        left.compare(&right, BitsInterpretation::TwosComplement)
            .unwrap(),
        Ordering::Less
    );
    assert_eq!(left.add_or_wrap(&right).unwrap(), bits(8, "a8"));
    assert_eq!(left.subtract_or_wrap(&right).unwrap(), bits(8, "9e"));
    assert_eq!(
        left.add_widening(&right, BitsInterpretation::Unsigned)
            .unwrap(),
        bits(9, "0a8")
    );
    assert_eq!(
        left.add_widening(&right, BitsInterpretation::TwosComplement)
            .unwrap(),
        bits(9, "1a8")
    );
    assert_eq!(
        bits(8, "ff")
            .try_add(&bits(8, "01"), BitsInterpretation::Unsigned)
            .unwrap(),
        Err(BitsArithmeticFailure::Overflow)
    );
    assert_eq!(
        right
            .try_subtract(&left, BitsInterpretation::Unsigned)
            .unwrap(),
        Err(BitsArithmeticFailure::Underflow)
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fixed_width_bits_have_one_native_and_wasm_boundary_encoding() {
    let word = bits(16, "a305");
    assert_eq!(
        word.to_bytes(BitsByteOrder::BigEndian).unwrap().as_ref(),
        [0xa3, 0x05]
    );
    assert_eq!(
        word.to_bytes(BitsByteOrder::LittleEndian).unwrap().as_ref(),
        [0x05, 0xa3]
    );
    assert_eq!(
        Bits::from_bytes(16, [0xa3, 0x05].as_slice(), BitsByteOrder::BigEndian).unwrap(),
        word
    );
    assert_eq!(
        Bits::from_bytes(16, [0x05, 0xa3].as_slice(), BitsByteOrder::LittleEndian).unwrap(),
        word
    );
    assert_eq!(
        word.to_number(BitsInterpretation::Unsigned).unwrap(),
        ExactNumber::from_i64(41_733)
    );
    assert_eq!(
        bits(8, "a3")
            .to_number(BitsInterpretation::TwosComplement)
            .unwrap(),
        ExactNumber::from_i64(-93)
    );
    assert_eq!(
        Bits::from_number(
            8,
            &ExactNumber::from_i64(-1),
            BitsInterpretation::TwosComplement
        )
        .unwrap(),
        bits(8, "ff")
    );

    let json = serde_json::to_vec(&word).unwrap();
    assert_eq!(serde_json::from_slice::<Bits>(&json).unwrap(), word);
    assert!(Bits::from_canonical_bytes(9, [0x81, 0x01].as_slice()).is_err());
}
