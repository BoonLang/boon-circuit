//! Canonical target-neutral Boon data values.

#![forbid(unsafe_code)]

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
    Bytes(Vec<u8>),
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
    fn structural_value_contains_only_recursive_language_data() {
        let value = Value::Record(BTreeMap::from([
            ("bytes".to_owned(), Value::Bytes(vec![1, 2, 3])),
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
