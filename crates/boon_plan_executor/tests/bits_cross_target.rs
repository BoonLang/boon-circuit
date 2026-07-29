#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

use boon_compiler::compile_source_text_to_machine_plan_for_role;
use boon_plan::{ProgramRole, TargetProfile};
use boon_plan_executor::{MachineInstance, SessionOptions, Value};
use std::collections::BTreeMap;

fn bits(width: u32, digits: &str) -> Value {
    Value::Bits(boon_data::Bits::parse_encoded(width, 16, digits).unwrap())
}

fn tagged_bits(tag: &str, width: u32, digits: &str) -> Value {
    Value::tagged(
        tag,
        BTreeMap::from([(
            "value".to_owned(),
            Value::Bits(boon_data::Bits::parse_encoded(width, 16, digits).unwrap()),
        )]),
    )
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fixed_width_bits_machine_plan_has_one_native_and_wasm_trace() {
    let compiled = compile_source_text_to_machine_plan_for_role(
        "bits-cross-target.bn",
        r#"
left: BITS[8] { 16ua3 }
right: BITS[8] { 16u05 }
word: BITS[16] { 16ua305 }

xor_value: left |> Bits/xor(with: right)
slice: left |> Bits/slice(from: 2, count: 3)
concat: left |> Bits/concat(with: right)
shift_right_arithmetic: left |> Bits/shift_right_arithmetic(by: 2)
rotate_left: left |> Bits/rotate_left(by: 2)
add_wrap: left |> Bits/add_or_wrap(with: right)
add_widening:
    left
    |> Bits/add_widening(with: right, interpretation: Unsigned)
checked_overflow:
    BITS[8] { 16uff }
    |> Bits/try_add(with: BITS[8] { 16u01 }, interpretation: Unsigned)
number_to_bits:
    255
    |> Number/to_bits(width: 8, interpretation: Unsigned)
to_signed_number: left |> Bits/to_number(interpretation: TwosComplement)
to_bytes: word |> Bits/to_bytes(byte_order: LittleEndian)
from_bytes:
    BYTES[2] { 16u05, 16ua3 }
    |> Bytes/to_bits(width: 16, byte_order: LittleEndian)
"#,
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .unwrap();
    let mut machine = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    for (path, expected) in [
        ("xor_value", bits(8, "a6")),
        ("slice", bits(3, "2")),
        ("concat", bits(16, "a305")),
        ("shift_right_arithmetic", bits(8, "e8")),
        ("rotate_left", bits(8, "8e")),
        ("add_wrap", bits(8, "a8")),
        ("add_widening", bits(9, "0a8")),
        ("checked_overflow", Value::tag("Overflow")),
        ("number_to_bits", tagged_bits("Converted", 8, "ff")),
        ("to_signed_number", Value::integer(-93).unwrap()),
        ("to_bytes", Value::Bytes(vec![0x05, 0xa3].into())),
        ("from_bytes", bits(16, "a305")),
    ] {
        assert_eq!(
            machine.root_value_current(path).unwrap(),
            expected,
            "cross-target BITS trace changed at `{path}`"
        );
    }
}
