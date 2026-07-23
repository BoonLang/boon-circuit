//! Canonical target-neutral Boon data values.

#![forbid(unsafe_code)]

pub use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

/// Error returned when a value cannot satisfy Boon's finite `Number` contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FiniteRealError {
    message: String,
}

impl FiniteRealError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for FiniteRealError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for FiniteRealError {}

/// Canonical finite IEEE-754 value used for Boon decimal `Number` values.
///
/// NaN and infinities are rejected, and negative zero is normalized so value
/// equality, ordering, hashing, plan identity, and persistence agree.
#[derive(Clone, Copy, Debug)]
pub struct FiniteReal(f64);

impl FiniteReal {
    pub const ZERO: Self = Self(0.0);
    pub const ONE: Self = Self(1.0);

    pub fn new(value: f64) -> Result<Self, FiniteRealError> {
        if !value.is_finite() {
            return Err(FiniteRealError::new("real number must be finite"));
        }
        Ok(Self(if value == 0.0 { 0.0 } else { value }))
    }

    pub fn get(self) -> f64 {
        self.0
    }

    pub fn from_i64_exact(value: i64) -> Result<Self, FiniteRealError> {
        let real = value as f64;
        if real as i128 != i128::from(value) {
            return Err(FiniteRealError::new(format!(
                "integer `{value}` cannot be represented exactly as a Boon Number"
            )));
        }
        Self::new(real)
    }

    pub fn to_i64_exact(self) -> Result<i64, FiniteRealError> {
        if self.0.fract() != 0.0
            || self.0 < i64::MIN as f64
            || self.0 >= 9_223_372_036_854_775_808.0
        {
            return Err(FiniteRealError::new(format!(
                "number `{}` is not a representable whole i64",
                self.0
            )));
        }
        let value = self.0 as i64;
        if value as f64 != self.0 {
            return Err(FiniteRealError::new(format!(
                "number `{}` is not an exact whole i64",
                self.0
            )));
        }
        Ok(value)
    }

    pub fn to_usize_exact(self) -> Result<usize, FiniteRealError> {
        let value = self.to_i64_exact()?;
        usize::try_from(value).map_err(|_| {
            FiniteRealError::new(format!(
                "number `{}` is not a non-negative platform index",
                self.0
            ))
        })
    }
}

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

/// Formats one finite Number without implicit rounding or unbounded allocation.
pub fn format_number_text(
    value: FiniteReal,
    format: NumberTextFormat,
) -> Result<String, FiniteRealError> {
    if !(2..=36).contains(&format.radix) {
        return Err(FiniteRealError::new(
            "Number/to_text radix must be between 2 and 36",
        ));
    }
    if format.min_width > MAX_NUMBER_TEXT_DIGITS {
        return Err(FiniteRealError::new(format!(
            "Number/to_text min_width must not exceed {MAX_NUMBER_TEXT_DIGITS}"
        )));
    }
    if format
        .group_size
        .is_some_and(|size| size == 0 || size > MAX_NUMBER_TEXT_DIGITS)
    {
        return Err(FiniteRealError::new(format!(
            "Number/to_text group_size must be between 1 and {MAX_NUMBER_TEXT_DIGITS}"
        )));
    }
    if format
        .signed_width
        .is_some_and(|width| !(1..=63).contains(&width))
    {
        return Err(FiniteRealError::new(
            "Number/to_text signed_width must be between 1 and 63",
        ));
    }
    let prefix = if format.prefix {
        match format.radix {
            2 => "0b",
            8 => "0o",
            16 => "0x",
            _ => {
                return Err(FiniteRealError::new(
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
        return Ok(value.to_string());
    }

    let mut integer = value.to_i64_exact()?;
    if let Some(width) = format.signed_width {
        if integer < 0 {
            return Err(FiniteRealError::new(
                "Number/to_text signed_width requires a non-negative bit pattern",
            ));
        }
        let raw = i128::from(integer);
        let modulus = 1_i128 << width;
        if raw >= modulus {
            return Err(FiniteRealError::new(format!(
                "Number/to_text value {integer} does not fit signed_width {width}"
            )));
        }
        let sign_bit = 1_i128 << (width - 1);
        if raw & sign_bit != 0 {
            integer = i64::try_from(raw - modulus)
                .map_err(|_| FiniteRealError::new("Number/to_text signed conversion overflowed"))?;
        }
    }

    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let negative = integer < 0;
    let mut magnitude = integer.unsigned_abs();
    let mut digits = Vec::new();
    loop {
        digits.push(DIGITS[(magnitude % u64::from(format.radix)) as usize]);
        magnitude /= u64::from(format.radix);
        if magnitude == 0 {
            break;
        }
    }
    while digits.len() < format.min_width {
        digits.push(b'0');
    }
    digits.reverse();

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
pub fn number_bit_width(value: FiniteReal) -> Result<FiniteReal, FiniteRealError> {
    let magnitude = value.to_i64_exact()?.unsigned_abs();
    FiniteReal::from_i64_exact(i64::from(u64::BITS - magnitude.leading_zeros()))
}

/// Formats a whole-number bit pattern as bounded ASCII waveform text.
/// Invalid values use `?`; widths below one byte use `-`.
pub fn format_number_ascii_text(value: FiniteReal, width: Option<FiniteReal>) -> String {
    let Ok(value) = value.to_i64_exact().and_then(|value| {
        u64::try_from(value)
            .map_err(|_| FiniteRealError::new("ASCII values must be non-negative whole Numbers"))
    }) else {
        return "?".to_owned();
    };
    if value > (1_u64 << 53) - 1 {
        return "?".to_owned();
    }

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

impl FromStr for FiniteReal {
    type Err = FiniteRealError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !value.contains(['.', 'e', 'E']) {
            let integer = value.parse::<i64>().map_err(|_| {
                FiniteRealError::new(format!("`{value}` is not a representable Number"))
            })?;
            return Self::from_i64_exact(integer);
        }
        let parsed = value
            .parse::<f64>()
            .map_err(|_| FiniteRealError::new(format!("`{value}` is not a real number")))?;
        Self::new(parsed)
    }
}

impl fmt::Display for FiniteReal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl PartialEq for FiniteReal {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for FiniteReal {}

impl PartialOrd for FiniteReal {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FiniteReal {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Hash for FiniteReal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl Serialize for FiniteReal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> Deserialize<'de> for FiniteReal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Canonical recursive value for target-neutral structural Boon data.
///
/// Runtime row identity and persistence-specific list authority are represented
/// by their owning crates rather than embedded in this enum.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Value {
    Null,
    Bool(bool),
    Number(FiniteReal),
    Text(String),
    Bytes(Bytes),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    Variant {
        tag: String,
        fields: BTreeMap<String, Value>,
    },
    Error {
        code: String,
        fields: BTreeMap<String, Value>,
    },
}

impl Value {
    /// Constructs a number only when the integer has an exact Boon `Number`
    /// representation.
    pub fn integer(value: i64) -> Result<Self, FiniteRealError> {
        FiniteReal::from_i64_exact(value).map(Self::Number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;

    #[test]
    fn finite_number_is_canonical_and_whole_conversions_are_checked() {
        let positive_zero = FiniteReal::new(0.0).unwrap();
        let negative_zero = FiniteReal::new(-0.0).unwrap();
        assert_eq!(positive_zero, negative_zero);
        assert_eq!(positive_zero.get().to_bits(), 0.0f64.to_bits());
        assert_eq!(
            FiniteReal::from_i64_exact(1).unwrap(),
            "1.0".parse().unwrap()
        );
        assert_eq!(FiniteReal::new(42.0).unwrap().to_i64_exact().unwrap(), 42);
        assert!(FiniteReal::new(42.5).unwrap().to_i64_exact().is_err());
        assert!(FiniteReal::new(-1.0).unwrap().to_usize_exact().is_err());
        assert!(FiniteReal::from_i64_exact(9_007_199_254_740_993).is_err());
        assert!(FiniteReal::new(f64::NAN).is_err());
        assert!(FiniteReal::new(f64::INFINITY).is_err());
    }

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
    fn canonical_number_equality_order_and_hash_use_normalized_bits() {
        let positive_zero = FiniteReal::new(0.0).unwrap();
        let negative_zero = FiniteReal::new(-0.0).unwrap();
        let hash = |value: FiniteReal| {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        };

        assert_eq!(positive_zero.cmp(&negative_zero), std::cmp::Ordering::Equal);
        assert_eq!(hash(positive_zero), hash(negative_zero));
        assert!(FiniteReal::new(1.5).unwrap() > FiniteReal::ONE);
    }

    #[test]
    fn number_text_format_is_bounded_and_waveform_complete() {
        let value = FiniteReal::from_i64_exact(42).unwrap();
        assert_eq!(
            format_number_text(
                value,
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
                value,
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
                FiniteReal::from_i64_exact(255).unwrap(),
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
                value,
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
            number_bit_width(FiniteReal::ZERO).unwrap(),
            FiniteReal::ZERO
        );
        assert_eq!(
            number_bit_width(FiniteReal::new(255.0).unwrap()).unwrap(),
            FiniteReal::new(8.0).unwrap()
        );
        assert!(number_bit_width(FiniteReal::new(1.5).unwrap()).is_err());

        let ascii = |value: f64, width: Option<f64>| {
            format_number_ascii_text(
                FiniteReal::new(value).unwrap(),
                width.map(|width| FiniteReal::new(width).unwrap()),
            )
        };
        assert_eq!(ascii(0x48 as f64, Some(8.0)), "H");
        assert_eq!(ascii(0x4845 as f64, Some(16.0)), "HE");
        assert_eq!(ascii(0.0, Some(7.0)), "-");
        assert_eq!(ascii(0.0, Some(8.0)), "?");
        assert_eq!(ascii(1.0, Some(8.0)), "?");
        assert_eq!(ascii(0x48 as f64, Some(65.0)), "?");
    }

    #[test]
    fn structural_value_contains_only_recursive_language_data() {
        let value = Value::Record(BTreeMap::from([
            ("bytes".to_owned(), Value::Bytes(vec![1, 2, 3].into())),
            (
                "list".to_owned(),
                Value::List(vec![
                    Value::Null,
                    Value::Bool(true),
                    Value::Text("ready".to_owned()),
                ]),
            ),
            (
                "result".to_owned(),
                Value::Variant {
                    tag: "Ready".to_owned(),
                    fields: BTreeMap::from([("count".to_owned(), Value::integer(3).unwrap())]),
                },
            ),
            (
                "failure".to_owned(),
                Value::Error {
                    code: "not_ready".to_owned(),
                    fields: BTreeMap::new(),
                },
            ),
        ]));

        let Value::Record(fields) = value else {
            panic!("expected record");
        };
        assert_eq!(fields.len(), 4);
        assert_eq!(
            fields["result"],
            Value::Variant {
                tag: "Ready".to_owned(),
                fields: BTreeMap::from([("count".to_owned(), Value::integer(3).unwrap())]),
            }
        );
    }
}
