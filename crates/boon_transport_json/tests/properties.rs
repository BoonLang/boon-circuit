use boon_transport_json::{DiagnosticCode, FiniteReal, Limits, Value, decode, encode};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn below(&mut self, upper: usize) -> usize {
        (self.next() as usize) % upper
    }
}

#[test]
fn generated_structural_values_round_trip_to_one_canonical_encoding() {
    for seed in 1..=2_048u64 {
        let mut rng = Rng(seed);
        let value = generated_value(&mut rng, 4);
        let encoded = encode(&value, &Limits::default()).unwrap();
        let decoded = decode(&encoded, &Limits::default()).unwrap();
        assert_eq!(decoded, value, "round trip failed for seed {seed}");
        assert_eq!(
            encode(&decoded, &Limits::default()).unwrap(),
            encoded,
            "canonical encoding changed for seed {seed}"
        );
    }
}

#[test]
fn arbitrary_bounded_byte_inputs_never_escape_limits_or_diagnostic_bounds() {
    let limits = Limits {
        max_input_bytes: 192,
        max_output_bytes: 192,
        max_depth: 12,
        max_nodes: 96,
        max_string_bytes: 64,
        max_array_items: 32,
        max_object_fields: 32,
        max_diagnostic_bytes: 31,
    };

    for seed in 1..=10_000u64 {
        let mut rng = Rng(seed ^ 0xa5a5_5a5a_d3c3_b4b4);
        let length = rng.below(193);
        let mut bytes = Vec::with_capacity(length);
        for _ in 0..length {
            bytes.push(rng.next() as u8);
        }
        match decode(&bytes, &limits) {
            Ok(value) => {
                let encoded = encode(&value, &limits);
                if let Ok(encoded) = encoded {
                    assert!(encoded.len() <= limits.max_output_bytes);
                    assert_eq!(decode(&encoded, &limits).unwrap(), value);
                }
            }
            Err(diagnostic) => {
                assert!(diagnostic.offset <= bytes.len());
                assert!(diagnostic.message.len() <= limits.max_diagnostic_bytes);
            }
        }
    }
}

#[test]
fn generated_duplicate_keys_are_always_rejected_in_strict_mode() {
    for seed in 1..=512u64 {
        let key = format!("key_{seed}_\u{20ac}");
        let input = format!("{{\"{key}\":0,\"{key}\":1}}");
        let diagnostic = decode(input.as_bytes(), &Limits::default()).unwrap_err();
        assert_eq!(diagnostic.code, DiagnosticCode::DuplicateKey);
    }
}

fn generated_value(rng: &mut Rng, depth: usize) -> Value {
    let choice = if depth == 0 {
        rng.below(4)
    } else {
        rng.below(7)
    };
    match choice {
        0 => Value::Null,
        1 => Value::Bool(rng.next() & 1 != 0),
        2 => {
            let numerator = (rng.next() % 2_000_001) as i64 - 1_000_000;
            let divisor = [1.0, 2.0, 4.0, 10.0, 100.0][rng.below(5)];
            Value::Number(FiniteReal::new(numerator as f64 / divisor).unwrap())
        }
        3 => Value::Text(generated_text(rng)),
        4 => {
            let length = rng.below(5);
            Value::List(
                (0..length)
                    .map(|_| generated_value(rng, depth.saturating_sub(1)))
                    .collect(),
            )
        }
        _ => {
            let length = rng.below(5);
            let mut fields = BTreeMap::new();
            for index in 0..length {
                fields.insert(
                    format!("field_{index}_{:x}", rng.next() & 0xffff),
                    generated_value(rng, depth.saturating_sub(1)),
                );
            }
            Value::Record(fields)
        }
    }
}

fn generated_text(rng: &mut Rng) -> String {
    const PIECES: &[&str] = &[
        "plain",
        "\"quote\"",
        "back\\slash",
        "line\nfeed",
        "\0control",
        "\u{20ac}",
        "\u{1f6a2}",
        "\u{5317}",
        "/slash",
    ];
    let length = rng.below(5);
    let mut text = String::new();
    for _ in 0..length {
        text.push_str(PIECES[rng.below(PIECES.len())]);
    }
    text
}
