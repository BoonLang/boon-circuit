//! Canonical target-neutral Boon data values.

#![forbid(unsafe_code)]

pub use bytes::Bytes;
mod bits;
pub use bits::{
    Bits, BitsArithmeticFailure, BitsByteOrder, BitsDirection, BitsError, BitsInterpretation,
    MAX_BITS_WIDTH,
};
mod number;
pub use number::{
    ExactNumber, ExactNumberError, ExactNumberParseError, ExactNumberParseReason,
    ExactNumberSemanticProfileV1, ExactNumberSign, ExactRoundingRule,
    MAX_NUMBER_ARITHMETIC_BIT_WORK, MAX_NUMBER_COMPONENT_BITS, MAX_NUMBER_FORMATTED_DIGITS,
    MAX_NUMBER_PARSED_DIGITS,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Bounded formatting options for Boon's `Number/to_text()` builtin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NumberTextFormat {
    pub radix: u32,
    pub min_width: usize,
    pub signed_width: Option<u32>,
    pub group_size: Option<usize>,
    pub prefix: bool,
}

impl Default for NumberTextFormat {
    fn default() -> Self {
        Self {
            radix: 10,
            min_width: 0,
            signed_width: None,
            group_size: None,
            prefix: false,
        }
    }
}

pub const MAX_NUMBER_TEXT_DIGITS: usize = 4096;

/// Formats one exact Number without implicit rounding or unbounded allocation.
pub fn format_number_text(
    value: &ExactNumber,
    format: NumberTextFormat,
) -> Result<String, ExactNumberError> {
    if !(2..=36).contains(&format.radix) {
        return Err(ExactNumberError::new(
            "Number/to_text radix must be between 2 and 36",
        ));
    }
    if format.min_width > MAX_NUMBER_TEXT_DIGITS {
        return Err(ExactNumberError::new(format!(
            "Number/to_text min_width must not exceed {MAX_NUMBER_TEXT_DIGITS}"
        )));
    }
    if format
        .group_size
        .is_some_and(|size| size == 0 || size > MAX_NUMBER_TEXT_DIGITS)
    {
        return Err(ExactNumberError::new(format!(
            "Number/to_text group_size must be between 1 and {MAX_NUMBER_TEXT_DIGITS}"
        )));
    }
    if format
        .signed_width
        .is_some_and(|width| !(1..=63).contains(&width))
    {
        return Err(ExactNumberError::new(
            "Number/to_text signed_width must be between 1 and 63",
        ));
    }
    let prefix = if format.prefix {
        match format.radix {
            2 => "0b",
            8 => "0o",
            16 => "0x",
            _ => {
                return Err(ExactNumberError::new(
                    "Number/to_text prefix is supported only for radix 2, 8, or 16",
                ));
            }
        }
    } else {
        ""
    };

    let integer_format = format.radix != 10
        || format.min_width != 0
        || format.signed_width.is_some()
        || format.group_size.is_some()
        || format.prefix;
    if !integer_format {
        return value.to_canonical_text();
    }

    let mut integer = value.clone();
    if let Some(width) = format.signed_width {
        if integer.is_negative() {
            return Err(ExactNumberError::new(
                "Number/to_text signed_width requires a non-negative bit pattern",
            ));
        }
        let raw = integer.to_u64_exact()?;
        let modulus = 1_u64 << width;
        if raw >= modulus {
            return Err(ExactNumberError::new(format!(
                "Number/to_text value {integer} does not fit signed_width {width}"
            )));
        }
        let sign_bit = 1_u64 << (width - 1);
        if raw & sign_bit != 0 {
            let signed = i128::from(raw) - i128::from(modulus);
            integer = ExactNumber::from_i64(i64::try_from(signed).map_err(|_| {
                ExactNumberError::new("Number/to_text signed conversion overflowed")
            })?);
        }
    }

    let (negative, digits) = integer.integer_to_str_radix(format.radix)?;
    let mut digits = digits.into_bytes();
    while digits.len() < format.min_width {
        digits.insert(0, b'0');
    }

    let separator_count = format
        .group_size
        .map(|size| digits.len().saturating_sub(1) / size)
        .unwrap_or(0);
    let mut output = String::with_capacity(
        usize::from(negative) + prefix.len() + digits.len() + separator_count,
    );
    if negative {
        output.push('-');
    }
    output.push_str(prefix);
    if let Some(group_size) = format.group_size {
        let first_group = digits.len() % group_size;
        for (index, digit) in digits.into_iter().enumerate() {
            if index > 0
                && (index == first_group || (index - first_group).is_multiple_of(group_size))
            {
                output.push(' ');
            }
            output.push(char::from(digit));
        }
    } else {
        output.extend(digits.into_iter().map(char::from));
    }
    Ok(output)
}

/// Returns the number of significant bits in the absolute whole-number value.
/// Fractional values are rejected instead of being silently truncated.
pub fn number_bit_width(value: &ExactNumber) -> Result<ExactNumber, ExactNumberError> {
    let magnitude = value.abs().to_u64_exact()?;
    Ok(ExactNumber::from_u64(u64::from(
        u64::BITS - magnitude.leading_zeros(),
    )))
}

/// Formats a whole-number bit pattern as bounded ASCII waveform text.
/// Invalid values use `?`; widths below one byte use `-`.
pub fn format_number_ascii_text(value: &ExactNumber, width: Option<&ExactNumber>) -> String {
    let Ok(value) = value.to_u64_exact() else {
        return "?".to_owned();
    };

    let inferred_width = (u64::BITS as usize - value.leading_zeros() as usize)
        .max(1)
        .div_ceil(8)
        * 8;
    let width = match width {
        Some(width) => match width.to_usize_exact() {
            Ok(width) => width,
            Err(_) => return "?".to_owned(),
        },
        None => inferred_width,
    };
    if width < 8 {
        return "-".to_owned();
    }
    if width > 64 {
        return "?".to_owned();
    }

    let mut bytes = Vec::with_capacity(width / 8);
    for group in 0..width / 8 {
        let shift = width - (group + 1) * 8;
        let byte = ((value >> shift) & 0xff) as u8;
        bytes.push(
            if byte == 0
                || (byte.is_ascii() && (byte.is_ascii_graphic() || byte.is_ascii_whitespace()))
            {
                byte
            } else {
                b'?'
            },
        );
    }
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    if bytes.is_empty() {
        return "?".to_owned();
    }
    for byte in &mut bytes {
        if *byte == 0 {
            *byte = b'?';
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|_| "?".to_owned())
}

/// Canonical recursive value for target-neutral structural Boon data.
///
/// Runtime row identity and persistence-specific list authority are represented
/// by their owning crates rather than embedded in this enum.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Value {
    Number(ExactNumber),
    Text(String),
    Bytes(Bytes),
    List(Vec<Value>),
    Object(BTreeMap<String, Value>),
    Tag {
        tag: String,
        fields: BTreeMap<String, Value>,
    },
    Map(BTreeMap<Value, Value>),
    Set(BTreeSet<Value>),
    /// Canonical fixed-width raw bit sequence. Appended so existing binary
    /// discriminants remain stable while the Phase 4 schema version advances.
    Bits(Bits),
}

impl Value {
    /// Constructs a number only when the integer has an exact Boon `Number`
    /// representation.
    pub fn integer(value: i64) -> Result<Self, ExactNumberError> {
        Ok(Self::Number(ExactNumber::from_i64(value)))
    }

    pub fn tag(tag: impl Into<String>) -> Self {
        Self::Tag {
            tag: tag.into(),
            fields: BTreeMap::new(),
        }
    }

    pub fn truth(value: bool) -> Self {
        Self::tag(if value { "True" } else { "False" })
    }

    pub fn as_truth(&self) -> Option<bool> {
        match self {
            Self::Tag { tag, fields } if fields.is_empty() && tag == "True" => Some(true),
            Self::Tag { tag, fields } if fields.is_empty() && tag == "False" => Some(false),
            _ => None,
        }
    }

    /// Whether this value has the closed structural representation accepted
    /// for canonical MAP keys and SET items.
    pub fn is_key_safe(&self) -> bool {
        match self {
            Self::Number(_) | Self::Text(_) | Self::Bytes(_) | Self::Bits(_) => true,
            Self::Object(fields) => fields.values().all(Self::is_key_safe),
            Self::Tag { fields, .. } => fields.values().all(Self::is_key_safe),
            Self::List(_) | Self::Map(_) | Self::Set(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_values_clone_shared_immutable_storage() {
        let bytes = Bytes::from_static(b"shared-byte-value");
        let value = Value::Bytes(bytes.clone());
        let cloned = value.clone();
        let Value::Bytes(cloned_bytes) = cloned else {
            panic!("cloned byte value changed kind");
        };

        assert_eq!(cloned_bytes, bytes);
        assert_eq!(cloned_bytes.as_ptr(), bytes.as_ptr());
    }

    #[test]
    fn number_text_format_is_bounded_and_waveform_complete() {
        let value = ExactNumber::from_i64(42);
        assert_eq!(
            format_number_text(
                &value,
                NumberTextFormat {
                    radix: 2,
                    min_width: 8,
                    group_size: Some(4),
                    ..NumberTextFormat::default()
                }
            )
            .unwrap(),
            "0010 1010"
        );
        assert_eq!(
            format_number_text(
                &value,
                NumberTextFormat {
                    radix: 16,
                    prefix: true,
                    ..NumberTextFormat::default()
                }
            )
            .unwrap(),
            "0x2a"
        );
        assert_eq!(
            format_number_text(
                &ExactNumber::from_i64(255),
                NumberTextFormat {
                    signed_width: Some(8),
                    ..NumberTextFormat::default()
                }
            )
            .unwrap(),
            "-1"
        );
        assert!(
            format_number_text(
                &value,
                NumberTextFormat {
                    min_width: MAX_NUMBER_TEXT_DIGITS + 1,
                    ..NumberTextFormat::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn waveform_number_helpers_are_exact_and_bounded() {
        assert_eq!(
            number_bit_width(&ExactNumber::zero()).unwrap(),
            ExactNumber::zero()
        );
        assert_eq!(
            number_bit_width(&ExactNumber::from_i64(255)).unwrap(),
            ExactNumber::from_i64(8)
        );
        assert!(number_bit_width(&"1.5".parse().unwrap()).is_err());

        let ascii = |value: i64, width: Option<i64>| {
            let value = ExactNumber::from_i64(value);
            let width = width.map(ExactNumber::from_i64);
            format_number_ascii_text(&value, width.as_ref())
        };
        assert_eq!(ascii(0x48, Some(8)), "H");
        assert_eq!(ascii(0x4845, Some(16)), "HE");
        assert_eq!(ascii(0, Some(7)), "-");
        assert_eq!(ascii(0, Some(8)), "?");
        assert_eq!(ascii(1, Some(8)), "?");
        assert_eq!(ascii(0x48, Some(65)), "?");
    }

    #[test]
    fn structural_value_contains_only_recursive_language_data() {
        let value = Value::Object(BTreeMap::from([
            ("bytes".to_owned(), Value::Bytes(vec![1, 2, 3].into())),
            (
                "list".to_owned(),
                Value::List(vec![
                    Value::tag("Null"),
                    Value::truth(true),
                    Value::Text("ready".to_owned()),
                ]),
            ),
            (
                "result".to_owned(),
                Value::Tag {
                    tag: "Ready".to_owned(),
                    fields: BTreeMap::from([("count".to_owned(), Value::integer(3).unwrap())]),
                },
            ),
            (
                "failure".to_owned(),
                Value::Tag {
                    tag: "NotReady".to_owned(),
                    fields: BTreeMap::new(),
                },
            ),
        ]));

        let Value::Object(fields) = value else {
            panic!("expected object");
        };
        assert_eq!(fields.len(), 4);
        assert_eq!(
            fields["result"],
            Value::Tag {
                tag: "Ready".to_owned(),
                fields: BTreeMap::from([("count".to_owned(), Value::integer(3).unwrap())]),
            }
        );
        assert_eq!(Value::truth(true).as_truth(), Some(true));
        assert_eq!(Value::truth(false).as_truth(), Some(false));
        assert_eq!(Value::tag("Null").as_truth(), None);
    }

    #[test]
    fn map_and_set_values_are_canonical_and_key_safe() {
        let composite_key = Value::Tag {
            tag: "Cell".to_owned(),
            fields: BTreeMap::from([
                ("column".to_owned(), Value::integer(2).unwrap()),
                ("row".to_owned(), Value::integer(1).unwrap()),
            ]),
        };
        assert!(composite_key.is_key_safe());

        let map = Value::Map(BTreeMap::from([
            (Value::Text("z".to_owned()), Value::integer(2).unwrap()),
            (Value::Text("a".to_owned()), Value::integer(1).unwrap()),
            (composite_key.clone(), Value::truth(true)),
        ]));
        let set = Value::Set(BTreeSet::from([
            Value::Text("z".to_owned()),
            Value::Text("a".to_owned()),
            composite_key,
        ]));

        let Value::Map(map) = map else {
            unreachable!();
        };
        assert_eq!(map.len(), 3);
        let Value::Set(set) = set else {
            unreachable!();
        };
        assert_eq!(set.len(), 3);
        assert!(!Value::List(Vec::new()).is_key_safe());
        assert!(!Value::Map(BTreeMap::new()).is_key_safe());
        assert!(!Value::Set(BTreeSet::new()).is_key_safe());
    }
}
