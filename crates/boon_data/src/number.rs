use num_bigint::{BigInt, BigUint, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use serde::de::{Error as _, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

/// Default maximum size of either canonical rational component.
pub const MAX_NUMBER_COMPONENT_BITS: u64 = 1_048_576;
/// Default maximum number of source digits consumed by one Number parse.
pub const MAX_NUMBER_PARSED_DIGITS: usize = 4096;
/// Default maximum number of digits emitted by one Number format.
pub const MAX_NUMBER_FORMATTED_DIGITS: usize = 4096;
/// Default upper bound for the conservative arithmetic work estimate.
pub const MAX_NUMBER_ARITHMETIC_BIT_WORK: u64 = 16 * 1024 * 1024;

/// Versioned deterministic limits for Boon's exact Number evaluator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactNumberSemanticProfileV1 {
    pub max_numerator_bits: u64,
    pub max_denominator_bits: u64,
    pub max_parsed_digits: usize,
    pub max_formatted_digits: usize,
    pub max_arithmetic_bit_work: u64,
}

impl Default for ExactNumberSemanticProfileV1 {
    fn default() -> Self {
        Self {
            max_numerator_bits: MAX_NUMBER_COMPONENT_BITS,
            max_denominator_bits: MAX_NUMBER_COMPONENT_BITS,
            max_parsed_digits: MAX_NUMBER_PARSED_DIGITS,
            max_formatted_digits: MAX_NUMBER_FORMATTED_DIGITS,
            max_arithmetic_bit_work: MAX_NUMBER_ARITHMETIC_BIT_WORK,
        }
    }
}

/// Stable sign code used by canonical wire and persistence encodings.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum ExactNumberSign {
    Negative = 0,
    Zero = 1,
    Positive = 2,
}

impl TryFrom<u8> for ExactNumberSign {
    type Error = ExactNumberError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Negative),
            1 => Ok(Self::Zero),
            2 => Ok(Self::Positive),
            _ => Err(ExactNumberError::new(format!(
                "unknown exact Number sign code {value}"
            ))),
        }
    }
}

/// Exact rounding rules exposed by the Number algebra.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExactRoundingRule {
    NearestEven,
    NearestAwayFromZero,
    TowardZero,
    TowardPositive,
    TowardNegative,
    AwayFromZero,
}

impl ExactRoundingRule {
    pub const ALL: [Self; 6] = [
        Self::NearestEven,
        Self::NearestAwayFromZero,
        Self::TowardZero,
        Self::TowardPositive,
        Self::TowardNegative,
        Self::AwayFromZero,
    ];

    pub const fn as_tag(self) -> &'static str {
        match self {
            Self::NearestEven => "NearestEven",
            Self::NearestAwayFromZero => "NearestAwayFromZero",
            Self::TowardZero => "TowardZero",
            Self::TowardPositive => "TowardPositive",
            Self::TowardNegative => "TowardNegative",
            Self::AwayFromZero => "AwayFromZero",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "NearestEven" => Self::NearestEven,
            "NearestAwayFromZero" => Self::NearestAwayFromZero,
            "TowardZero" => Self::TowardZero,
            "TowardPositive" => Self::TowardPositive,
            "TowardNegative" => Self::TowardNegative,
            "AwayFromZero" => Self::AwayFromZero,
            _ => return None,
        })
    }
}

/// Stable reason carried by `InvalidNumber[reason, position]`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExactNumberParseReason {
    Empty,
    Whitespace,
    LeadingPlus,
    InvalidDigit,
    InvalidSyntax,
    InvalidExponent,
    ZeroDenominator,
    InvalidRadix,
    ResourceLimit,
}

impl ExactNumberParseReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Whitespace => "whitespace",
            Self::LeadingPlus => "leading_plus",
            Self::InvalidDigit => "invalid_digit",
            Self::InvalidSyntax => "invalid_syntax",
            Self::InvalidExponent => "invalid_exponent",
            Self::ZeroDenominator => "zero_denominator",
            Self::InvalidRadix => "invalid_radix",
            Self::ResourceLimit => "resource_limit",
        }
    }
}

/// One-based, deterministic failure for strict source or runtime Number text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactNumberParseError {
    reason: ExactNumberParseReason,
    position: usize,
    message: String,
}

impl ExactNumberParseError {
    fn new(reason: ExactNumberParseReason, position: usize, message: impl Into<String>) -> Self {
        Self {
            reason,
            position: position.max(1),
            message: message.into(),
        }
    }

    pub const fn reason(&self) -> ExactNumberParseReason {
        self.reason
    }

    pub const fn position(&self) -> usize {
        self.position
    }
}

impl fmt::Display for ExactNumberParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at position {}: {}",
            self.reason.as_str(),
            self.position,
            self.message
        )
    }
}

impl Error for ExactNumberParseError {}

/// Deterministic Number construction, domain, or resource failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactNumberError {
    message: String,
}

impl ExactNumberError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ExactNumberError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for ExactNumberError {}

impl From<ExactNumberParseError> for ExactNumberError {
    fn from(error: ExactNumberParseError) -> Self {
        Self::new(error.to_string())
    }
}

/// Boon's one canonical arbitrary-precision exact rational Number.
///
/// The denominator is always positive, the two components are coprime, and
/// zero has the sole representation `0 / 1`.
#[derive(Clone, Debug)]
pub struct ExactNumber {
    numerator: BigInt,
    denominator: BigUint,
}

impl ExactNumber {
    pub fn zero() -> Self {
        Self {
            numerator: BigInt::zero(),
            denominator: BigUint::one(),
        }
    }

    pub fn one() -> Self {
        Self {
            numerator: BigInt::one(),
            denominator: BigUint::one(),
        }
    }

    pub fn from_i64(value: i64) -> Self {
        Self {
            numerator: BigInt::from(value),
            denominator: BigUint::one(),
        }
    }

    pub fn from_u64(value: u64) -> Self {
        Self {
            numerator: BigInt::from(value),
            denominator: BigUint::one(),
        }
    }

    pub fn from_usize(value: usize) -> Self {
        Self {
            numerator: BigInt::from(value),
            denominator: BigUint::one(),
        }
    }

    /// Parses the complete input as one exact Number.
    ///
    /// `radix = None` accepts exact decimal, exponent, and canonical fraction
    /// notation. An explicit radix accepts one whole Number and the matching
    /// `0b`, `0o`, or `0x` prefix when present.
    pub fn parse_strict(text: &str, radix: Option<u32>) -> Result<Self, ExactNumberParseError> {
        Self::parse_strict_with_profile(text, radix, ExactNumberSemanticProfileV1::default())
    }

    pub fn parse_strict_with_profile(
        text: &str,
        radix: Option<u32>,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberParseError> {
        match radix {
            Some(radix) => parse_radix_number(text, radix, profile),
            None => {
                validate_default_number_syntax(text, profile)?;
                Self::parse_with_profile(text, profile).map_err(|error| {
                    let message = error.to_string();
                    let reason = if message.contains("denominator must be positive") {
                        ExactNumberParseReason::ZeroDenominator
                    } else if message.contains("budget")
                        || message.contains("out of range")
                        || message.contains("exceeds")
                    {
                        ExactNumberParseReason::ResourceLimit
                    } else {
                        ExactNumberParseReason::InvalidSyntax
                    };
                    ExactNumberParseError::new(reason, 1, message)
                })
            }
        }
    }

    /// Converts one finite binary64 value at an explicit external-data
    /// boundary into the exact rational denoted by its IEEE-754 bits.
    ///
    /// Source literals and Boon arithmetic never use this conversion.
    pub fn from_f64_boundary_exact(value: f64) -> Result<Self, ExactNumberError> {
        if !value.is_finite() {
            return Err(ExactNumberError::new(
                "external binary64 Number boundary requires a finite value",
            ));
        }
        if value == 0.0 {
            return Ok(Self::zero());
        }
        let bits = value.to_bits();
        let negative = bits >> 63 != 0;
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1_u64 << 52) - 1);
        let (significand, exponent) = if exponent_bits == 0 {
            (fraction, -1074)
        } else {
            ((1_u64 << 52) | fraction, exponent_bits - 1023 - 52)
        };
        let mut numerator = BigUint::from(significand);
        let mut denominator = BigUint::one();
        if exponent >= 0 {
            numerator <<= exponent as usize;
        } else {
            denominator <<= exponent.unsigned_abs() as usize;
        }
        Self::from_ratio(
            BigInt::from_biguint(if negative { Sign::Minus } else { Sign::Plus }, numerator),
            denominator,
        )
    }

    pub fn from_ratio(numerator: BigInt, denominator: BigUint) -> Result<Self, ExactNumberError> {
        Self::from_ratio_with_profile(
            numerator,
            denominator,
            ExactNumberSemanticProfileV1::default(),
        )
    }

    pub fn from_ratio_with_profile(
        numerator: BigInt,
        denominator: BigUint,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberError> {
        if denominator.is_zero() {
            return Err(ExactNumberError::new(
                "exact Number denominator must be positive",
            ));
        }
        if numerator.is_zero() {
            return Ok(Self::zero());
        }
        let magnitude = numerator.magnitude().gcd(&denominator);
        let numerator = numerator / BigInt::from(magnitude.clone());
        let denominator = denominator / magnitude;
        let value = Self {
            numerator,
            denominator,
        };
        value.check_profile(profile)?;
        Ok(value)
    }

    /// Reconstructs a value only from the minimal normalized encoding.
    pub fn from_canonical_bytes(
        sign: ExactNumberSign,
        numerator_magnitude: &[u8],
        denominator: &[u8],
    ) -> Result<Self, ExactNumberError> {
        Self::from_canonical_bytes_with_profile(
            sign,
            numerator_magnitude,
            denominator,
            ExactNumberSemanticProfileV1::default(),
        )
    }

    pub fn from_canonical_bytes_with_profile(
        sign: ExactNumberSign,
        numerator_magnitude: &[u8],
        denominator: &[u8],
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberError> {
        if numerator_magnitude.first() == Some(&0) {
            return Err(ExactNumberError::new(
                "exact Number numerator magnitude is not minimal",
            ));
        }
        if denominator.is_empty() || denominator.first() == Some(&0) {
            return Err(ExactNumberError::new(
                "exact Number denominator encoding must be minimal and nonempty",
            ));
        }
        let denominator = BigUint::from_bytes_be(denominator);
        if denominator.is_zero() {
            return Err(ExactNumberError::new(
                "exact Number denominator must be positive",
            ));
        }
        let magnitude = BigUint::from_bytes_be(numerator_magnitude);
        if sign == ExactNumberSign::Zero {
            if !numerator_magnitude.is_empty() || denominator != BigUint::one() {
                return Err(ExactNumberError::new(
                    "exact Number zero must be encoded only as 0 / 1",
                ));
            }
            return Ok(Self::zero());
        }
        if numerator_magnitude.is_empty() || magnitude.is_zero() {
            return Err(ExactNumberError::new(
                "nonzero exact Number sign requires a numerator magnitude",
            ));
        }
        let numerator = BigInt::from_biguint(
            match sign {
                ExactNumberSign::Negative => Sign::Minus,
                ExactNumberSign::Positive => Sign::Plus,
                ExactNumberSign::Zero => unreachable!(),
            },
            magnitude,
        );
        if numerator.magnitude().gcd(&denominator) != BigUint::one() {
            return Err(ExactNumberError::new(
                "exact Number numerator and denominator are not normalized",
            ));
        }
        let value = Self {
            numerator,
            denominator,
        };
        value.check_profile(profile)?;
        Ok(value)
    }

    pub fn sign(&self) -> ExactNumberSign {
        match self.numerator.sign() {
            Sign::Minus => ExactNumberSign::Negative,
            Sign::NoSign => ExactNumberSign::Zero,
            Sign::Plus => ExactNumberSign::Positive,
        }
    }

    pub fn numerator_magnitude_bytes(&self) -> Vec<u8> {
        if self.numerator.is_zero() {
            Vec::new()
        } else {
            self.numerator.magnitude().to_bytes_be()
        }
    }

    pub fn denominator_bytes(&self) -> Vec<u8> {
        self.denominator.to_bytes_be()
    }

    pub fn component_bits(&self) -> (u64, u64) {
        (self.numerator.magnitude().bits(), self.denominator.bits())
    }

    pub fn canonical_storage_bytes(&self) -> usize {
        1 + self.numerator_magnitude_bytes().len() + self.denominator_bytes().len()
    }

    /// Returns a prefix-free byte sequence whose lexicographic order is the
    /// exact rational order.
    ///
    /// The payload is derived from a run-length encoded Stern-Brocot path.
    /// Tokens are limited to `0..=2`, so callers can safely prefix or invert
    /// the sequence when composing directed ordered keys.
    pub fn canonical_order_bytes(&self) -> Vec<u8> {
        match self.sign() {
            ExactNumberSign::Negative => {
                let mut bytes = vec![0];
                let mut magnitude = encode_positive_order_path(
                    self.numerator.magnitude().clone(),
                    self.denominator.clone(),
                );
                for byte in &mut magnitude {
                    *byte = 2 - *byte;
                }
                bytes.extend(magnitude);
                bytes
            }
            ExactNumberSign::Zero => vec![1],
            ExactNumberSign::Positive => {
                let mut bytes = vec![2];
                bytes.extend(encode_positive_order_path(
                    self.numerator.magnitude().clone(),
                    self.denominator.clone(),
                ));
                bytes
            }
        }
    }

    pub fn is_zero(&self) -> bool {
        self.numerator.is_zero()
    }

    pub fn is_positive(&self) -> bool {
        self.numerator.is_positive()
    }

    pub fn is_negative(&self) -> bool {
        self.numerator.is_negative()
    }

    pub fn is_whole(&self) -> bool {
        self.denominator.is_one()
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self, ExactNumberError> {
        self.checked_add_with_profile(other, ExactNumberSemanticProfileV1::default())
    }

    pub fn checked_add_with_profile(
        &self,
        other: &Self,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberError> {
        self.check_binary_work(other, profile, "addition")?;
        let numerator = &self.numerator * BigInt::from(other.denominator.clone())
            + &other.numerator * BigInt::from(self.denominator.clone());
        let denominator = &self.denominator * &other.denominator;
        Self::from_ratio_with_profile(numerator, denominator, profile)
    }

    pub fn checked_sub(&self, other: &Self) -> Result<Self, ExactNumberError> {
        self.checked_sub_with_profile(other, ExactNumberSemanticProfileV1::default())
    }

    pub fn checked_sub_with_profile(
        &self,
        other: &Self,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberError> {
        self.check_binary_work(other, profile, "subtraction")?;
        let numerator = &self.numerator * BigInt::from(other.denominator.clone())
            - &other.numerator * BigInt::from(self.denominator.clone());
        let denominator = &self.denominator * &other.denominator;
        Self::from_ratio_with_profile(numerator, denominator, profile)
    }

    pub fn checked_mul(&self, other: &Self) -> Result<Self, ExactNumberError> {
        self.checked_mul_with_profile(other, ExactNumberSemanticProfileV1::default())
    }

    pub fn checked_mul_with_profile(
        &self,
        other: &Self,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberError> {
        self.check_binary_work(other, profile, "multiplication")?;
        let left_cancel = self.numerator.magnitude().gcd(&other.denominator);
        let right_cancel = other.numerator.magnitude().gcd(&self.denominator);
        let left_numerator = &self.numerator / BigInt::from(left_cancel.clone());
        let right_numerator = &other.numerator / BigInt::from(right_cancel.clone());
        let left_denominator = &self.denominator / right_cancel;
        let right_denominator = &other.denominator / left_cancel;
        Self::from_ratio_with_profile(
            left_numerator * right_numerator,
            left_denominator * right_denominator,
            profile,
        )
    }

    pub fn checked_div(&self, other: &Self) -> Result<Self, ExactNumberError> {
        self.checked_div_with_profile(other, ExactNumberSemanticProfileV1::default())
    }

    pub fn checked_div_with_profile(
        &self,
        other: &Self,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberError> {
        if other.is_zero() {
            return Err(ExactNumberError::new("exact Number division by zero"));
        }
        self.check_binary_work(other, profile, "division")?;
        let sign = if self.numerator.sign() == other.numerator.sign() {
            Sign::Plus
        } else {
            Sign::Minus
        };
        let numerator = BigInt::from_biguint(
            if self.is_zero() { Sign::NoSign } else { sign },
            self.numerator.magnitude() * &other.denominator,
        );
        let denominator = &self.denominator * other.numerator.magnitude();
        Self::from_ratio_with_profile(numerator, denominator, profile)
    }

    /// Euclidean remainder over whole Numbers.
    pub fn checked_rem(&self, other: &Self) -> Result<Self, ExactNumberError> {
        if !self.is_whole() || !other.is_whole() {
            return Err(ExactNumberError::new(
                "exact Number remainder requires whole operands",
            ));
        }
        if other.is_zero() {
            return Err(ExactNumberError::new("exact Number remainder by zero"));
        }
        Ok(Self {
            numerator: self.numerator.mod_floor(&other.numerator.abs()),
            denominator: BigUint::one(),
        })
    }

    pub fn round_to(
        &self,
        quantum: &Self,
        rule: ExactRoundingRule,
    ) -> Result<Self, ExactNumberError> {
        self.round_to_with_profile(quantum, rule, ExactNumberSemanticProfileV1::default())
    }

    pub fn round_to_with_profile(
        &self,
        quantum: &Self,
        rule: ExactRoundingRule,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberError> {
        if !quantum.is_positive() {
            return Err(ExactNumberError::new(
                "exact Number rounding quantum must be strictly positive",
            ));
        }
        let scaled = self.checked_div_with_profile(quantum, profile)?;
        let denominator = BigInt::from(scaled.denominator.clone());
        let (floor, remainder) = scaled.numerator.div_mod_floor(&denominator);
        let exact = remainder.is_zero();
        let ceil = if exact { floor.clone() } else { &floor + 1 };
        let rounded = match rule {
            ExactRoundingRule::TowardNegative => floor,
            ExactRoundingRule::TowardPositive => ceil,
            ExactRoundingRule::TowardZero => {
                if scaled.is_negative() && !exact {
                    ceil
                } else {
                    floor
                }
            }
            ExactRoundingRule::AwayFromZero => {
                if scaled.is_negative() {
                    floor
                } else {
                    ceil
                }
            }
            ExactRoundingRule::NearestEven | ExactRoundingRule::NearestAwayFromZero => {
                match (&remainder * BigInt::from(2_u8)).cmp(&denominator) {
                    Ordering::Less => floor,
                    Ordering::Greater => ceil,
                    Ordering::Equal => match rule {
                        ExactRoundingRule::NearestEven => {
                            if floor.is_even() {
                                floor
                            } else {
                                ceil
                            }
                        }
                        ExactRoundingRule::NearestAwayFromZero => {
                            if scaled.is_negative() {
                                floor
                            } else {
                                ceil
                            }
                        }
                        _ => unreachable!(),
                    },
                }
            }
        };
        quantum.checked_mul_with_profile(
            &Self {
                numerator: rounded,
                denominator: BigUint::one(),
            },
            profile,
        )
    }

    pub fn floor(&self) -> Self {
        Self {
            numerator: self
                .numerator
                .div_floor(&BigInt::from(self.denominator.clone())),
            denominator: BigUint::one(),
        }
    }

    pub fn ceil(&self) -> Self {
        Self {
            numerator: self
                .numerator
                .div_ceil(&BigInt::from(self.denominator.clone())),
            denominator: BigUint::one(),
        }
    }

    pub fn truncate(&self) -> Self {
        Self {
            numerator: &self.numerator / BigInt::from(self.denominator.clone()),
            denominator: BigUint::one(),
        }
    }

    pub fn abs(&self) -> Self {
        Self {
            numerator: self.numerator.abs(),
            denominator: self.denominator.clone(),
        }
    }

    pub fn to_i64_exact(&self) -> Result<i64, ExactNumberError> {
        if !self.is_whole() {
            return Err(ExactNumberError::new(format!(
                "number `{self}` is not whole"
            )));
        }
        self.numerator.to_i64().ok_or_else(|| {
            ExactNumberError::new(format!(
                "number `{self}` does not fit signed 64-bit storage"
            ))
        })
    }

    pub fn to_u64_exact(&self) -> Result<u64, ExactNumberError> {
        if !self.is_whole() {
            return Err(ExactNumberError::new(format!(
                "number `{self}` is not whole"
            )));
        }
        self.numerator.to_u64().ok_or_else(|| {
            ExactNumberError::new(format!(
                "number `{self}` does not fit unsigned 64-bit storage"
            ))
        })
    }

    pub fn to_usize_exact(&self) -> Result<usize, ExactNumberError> {
        if !self.is_whole() {
            return Err(ExactNumberError::new(format!(
                "number `{self}` is not whole"
            )));
        }
        self.numerator.to_usize().ok_or_else(|| {
            ExactNumberError::new(format!(
                "number `{self}` is not a non-negative platform index"
            ))
        })
    }

    /// Explicit host/render boundary conversion. Boon arithmetic never calls
    /// this method.
    pub fn to_f64_host_rounded(&self) -> Result<f64, ExactNumberError> {
        let numerator = self.numerator.to_f64().ok_or_else(|| {
            ExactNumberError::new("exact Number numerator exceeds the host f64 boundary")
        })?;
        let denominator = self.denominator.to_f64().ok_or_else(|| {
            ExactNumberError::new("exact Number denominator exceeds the host f64 boundary")
        })?;
        let value = numerator / denominator;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ExactNumberError::new(
                "exact Number is outside the finite host f64 boundary",
            ))
        }
    }

    pub fn to_canonical_text(&self) -> Result<String, ExactNumberError> {
        self.to_canonical_text_with_profile(ExactNumberSemanticProfileV1::default())
    }

    pub fn to_canonical_text_with_profile(
        &self,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<String, ExactNumberError> {
        if self.is_whole() {
            return check_formatted_digits(self.numerator.to_string(), profile);
        }
        let mut denominator = self.denominator.clone();
        let two = BigUint::from(2_u8);
        let five = BigUint::from(5_u8);
        let mut twos = 0_usize;
        let mut fives = 0_usize;
        while (&denominator % &two).is_zero() {
            denominator /= &two;
            twos += 1;
        }
        while (&denominator % &five).is_zero() {
            denominator /= &five;
            fives += 1;
        }
        if !denominator.is_one() {
            return check_formatted_digits(
                format!("{}/{}", self.numerator, self.denominator),
                profile,
            );
        }
        let scale = twos.max(fives);
        let mut scaled = self.numerator.magnitude().clone();
        if scale > twos {
            scaled *= BigUint::from(2_u8).pow(
                u32::try_from(scale - twos)
                    .map_err(|_| ExactNumberError::new("decimal scale exceeds u32"))?,
            );
        }
        if scale > fives {
            scaled *= BigUint::from(5_u8).pow(
                u32::try_from(scale - fives)
                    .map_err(|_| ExactNumberError::new("decimal scale exceeds u32"))?,
            );
        }
        let mut digits = scaled.to_string();
        if digits.len() <= scale {
            let zeros = scale + 1 - digits.len();
            digits = format!("{}{}", "0".repeat(zeros), digits);
        }
        let point = digits.len() - scale;
        digits.insert(point, '.');
        let text = if self.is_negative() {
            format!("-{digits}")
        } else {
            digits
        };
        check_formatted_digits(text, profile)
    }

    pub(crate) fn integer_to_str_radix(
        &self,
        radix: u32,
    ) -> Result<(bool, String), ExactNumberError> {
        if !self.is_whole() {
            return Err(ExactNumberError::new(
                "radix formatting requires a whole exact Number",
            ));
        }
        Ok((
            self.is_negative(),
            self.numerator.magnitude().to_str_radix(radix),
        ))
    }

    fn parse_with_profile(
        text: &str,
        profile: ExactNumberSemanticProfileV1,
    ) -> Result<Self, ExactNumberError> {
        if text.is_empty() || text.trim() != text {
            return Err(ExactNumberError::new(
                "exact Number text must be nonempty and contain no surrounding whitespace",
            ));
        }
        let digit_count = text.bytes().filter(u8::is_ascii_digit).count();
        if digit_count == 0 || digit_count > profile.max_parsed_digits {
            return Err(ExactNumberError::new(format!(
                "exact Number parsed digit budget exceeded: {digit_count} > {}",
                profile.max_parsed_digits
            )));
        }
        if let Some((left, right)) = split_once_unique(text, '/')? {
            if left.contains(['.', 'e', 'E']) || right.contains(['.', 'e', 'E', '+', '-']) {
                return Err(ExactNumberError::new(
                    "canonical fraction text requires signed integer numerator and positive integer denominator",
                ));
            }
            let numerator = parse_signed_integer(left)?;
            let denominator = parse_unsigned_integer(right)?;
            return Self::from_ratio_with_profile(numerator, denominator, profile);
        }
        parse_decimal(text, profile)
    }

    fn check_profile(&self, profile: ExactNumberSemanticProfileV1) -> Result<(), ExactNumberError> {
        let (numerator_bits, denominator_bits) = self.component_bits();
        if numerator_bits > profile.max_numerator_bits {
            return Err(ExactNumberError::new(format!(
                "exact Number numerator bit budget exceeded: {numerator_bits} > {}",
                profile.max_numerator_bits
            )));
        }
        if denominator_bits > profile.max_denominator_bits {
            return Err(ExactNumberError::new(format!(
                "exact Number denominator bit budget exceeded: {denominator_bits} > {}",
                profile.max_denominator_bits
            )));
        }
        Ok(())
    }

    fn check_binary_work(
        &self,
        other: &Self,
        profile: ExactNumberSemanticProfileV1,
        operation: &str,
    ) -> Result<(), ExactNumberError> {
        let (left_numerator, left_denominator) = self.component_bits();
        let (right_numerator, right_denominator) = other.component_bits();
        let work = left_numerator
            .saturating_add(left_denominator)
            .saturating_mul(right_numerator.saturating_add(right_denominator));
        if work > profile.max_arithmetic_bit_work {
            return Err(ExactNumberError::new(format!(
                "exact Number {operation} work budget exceeded: {work} > {}",
                profile.max_arithmetic_bit_work
            )));
        }
        Ok(())
    }
}

fn parse_failure(
    reason: ExactNumberParseReason,
    position: usize,
    message: impl Into<String>,
) -> ExactNumberParseError {
    ExactNumberParseError::new(reason, position, message)
}

fn validate_digit_budget(
    digit_count: usize,
    position: usize,
    profile: ExactNumberSemanticProfileV1,
) -> Result<(), ExactNumberParseError> {
    if digit_count == 0 {
        return Err(parse_failure(
            ExactNumberParseReason::Empty,
            position,
            "Number text contains no digits",
        ));
    }
    if digit_count > profile.max_parsed_digits {
        return Err(parse_failure(
            ExactNumberParseReason::ResourceLimit,
            position,
            format!(
                "parsed digit budget exceeded: {digit_count} > {}",
                profile.max_parsed_digits
            ),
        ));
    }
    Ok(())
}

fn validate_decimal_digits(
    text: &str,
    start_position: usize,
) -> Result<usize, ExactNumberParseError> {
    let mut count = 0;
    for (offset, character) in text.chars().enumerate() {
        if !character.is_ascii_digit() {
            return Err(parse_failure(
                ExactNumberParseReason::InvalidDigit,
                start_position + offset,
                format!("`{character}` is not a decimal digit"),
            ));
        }
        count += 1;
    }
    Ok(count)
}

fn validate_default_number_syntax(
    text: &str,
    profile: ExactNumberSemanticProfileV1,
) -> Result<(), ExactNumberParseError> {
    if text.is_empty() {
        return Err(parse_failure(
            ExactNumberParseReason::Empty,
            1,
            "Number text is empty",
        ));
    }
    if let Some((offset, _)) = text
        .chars()
        .enumerate()
        .find(|(_, character)| character.is_whitespace())
    {
        return Err(parse_failure(
            ExactNumberParseReason::Whitespace,
            offset + 1,
            "strict Number text cannot contain whitespace",
        ));
    }
    if text.starts_with('+') {
        return Err(parse_failure(
            ExactNumberParseReason::LeadingPlus,
            1,
            "strict Number text does not accept a leading plus sign",
        ));
    }

    let slash_positions = text
        .char_indices()
        .filter_map(|(index, character)| (character == '/').then_some(index))
        .collect::<Vec<_>>();
    if slash_positions.len() > 1 {
        let position = text[..slash_positions[1]].chars().count() + 1;
        return Err(parse_failure(
            ExactNumberParseReason::InvalidSyntax,
            position,
            "fraction notation contains more than one slash",
        ));
    }
    if let Some(slash) = slash_positions.first().copied() {
        let numerator = &text[..slash];
        let denominator = &text[slash + 1..];
        let numerator_digits = numerator.strip_prefix('-').unwrap_or(numerator);
        if numerator_digits.is_empty() {
            return Err(parse_failure(
                ExactNumberParseReason::Empty,
                1,
                "fraction numerator is empty",
            ));
        }
        let numerator_count = validate_decimal_digits(
            numerator_digits,
            usize::from(numerator.starts_with('-')) + 1,
        )?;
        if denominator.is_empty() {
            return Err(parse_failure(
                ExactNumberParseReason::Empty,
                text.chars().count() + 1,
                "fraction denominator is empty",
            ));
        }
        let denominator_position = text[..=slash].chars().count() + 1;
        let denominator_count = validate_decimal_digits(denominator, denominator_position)?;
        validate_digit_budget(
            numerator_count.saturating_add(denominator_count),
            denominator_position,
            profile,
        )?;
        if denominator.bytes().all(|byte| byte == b'0') {
            return Err(parse_failure(
                ExactNumberParseReason::ZeroDenominator,
                denominator_position,
                "fraction denominator must be positive",
            ));
        }
        return Ok(());
    }

    let unsigned = text.strip_prefix('-').unwrap_or(text);
    if unsigned.is_empty() {
        return Err(parse_failure(
            ExactNumberParseReason::Empty,
            2,
            "Number text contains no digits",
        ));
    }
    let sign_offset = usize::from(text.starts_with('-'));
    let mut seen_decimal = false;
    let mut seen_exponent = false;
    let mut mantissa_digits = 0usize;
    let mut exponent_digits = 0usize;
    let mut exponent_sign_allowed = false;
    for (offset, character) in unsigned.chars().enumerate() {
        let position = sign_offset + offset + 1;
        if character.is_ascii_digit() {
            if seen_exponent {
                exponent_digits += 1;
            } else {
                mantissa_digits += 1;
            }
            exponent_sign_allowed = false;
            continue;
        }
        match character {
            '.' if !seen_decimal && !seen_exponent => {
                seen_decimal = true;
            }
            '.' => {
                return Err(parse_failure(
                    ExactNumberParseReason::InvalidSyntax,
                    position,
                    "Number text repeats or misplaces the decimal point",
                ));
            }
            'e' | 'E' if !seen_exponent && mantissa_digits > 0 => {
                seen_exponent = true;
                exponent_sign_allowed = true;
            }
            'e' | 'E' => {
                return Err(parse_failure(
                    ExactNumberParseReason::InvalidExponent,
                    position,
                    "Number text repeats or misplaces the exponent marker",
                ));
            }
            '+' | '-' if exponent_sign_allowed => {
                exponent_sign_allowed = false;
            }
            '+' | '-' => {
                return Err(parse_failure(
                    ExactNumberParseReason::InvalidExponent,
                    position,
                    "sign is valid only at the start of an exponent",
                ));
            }
            _ => {
                return Err(parse_failure(
                    ExactNumberParseReason::InvalidDigit,
                    position,
                    format!("`{character}` is not valid Number syntax"),
                ));
            }
        }
    }
    if mantissa_digits == 0 {
        return Err(parse_failure(
            ExactNumberParseReason::Empty,
            sign_offset + 1,
            "Number mantissa contains no digits",
        ));
    }
    if seen_exponent && exponent_digits == 0 {
        return Err(parse_failure(
            ExactNumberParseReason::InvalidExponent,
            text.chars().count() + 1,
            "Number exponent contains no digits",
        ));
    }
    validate_digit_budget(
        mantissa_digits.saturating_add(exponent_digits),
        text.chars().count(),
        profile,
    )
}

fn parse_radix_number(
    text: &str,
    radix: u32,
    profile: ExactNumberSemanticProfileV1,
) -> Result<ExactNumber, ExactNumberParseError> {
    if !(2..=36).contains(&radix) {
        return Err(parse_failure(
            ExactNumberParseReason::InvalidRadix,
            1,
            format!("radix {radix} is outside 2..=36"),
        ));
    }
    if text.is_empty() {
        return Err(parse_failure(
            ExactNumberParseReason::Empty,
            1,
            "Number text is empty",
        ));
    }
    if let Some((offset, _)) = text
        .chars()
        .enumerate()
        .find(|(_, character)| character.is_whitespace())
    {
        return Err(parse_failure(
            ExactNumberParseReason::Whitespace,
            offset + 1,
            "strict Number text cannot contain whitespace",
        ));
    }
    if text.starts_with('+') {
        return Err(parse_failure(
            ExactNumberParseReason::LeadingPlus,
            1,
            "strict Number text does not accept a leading plus sign",
        ));
    }
    let negative = text.starts_with('-');
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let declared_prefix_radix = if unsigned.starts_with("0b") || unsigned.starts_with("0B") {
        Some(2)
    } else if unsigned.starts_with("0o") || unsigned.starts_with("0O") {
        Some(8)
    } else if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
        Some(16)
    } else {
        None
    };
    if let Some(declared) = declared_prefix_radix
        && declared != radix
    {
        return Err(parse_failure(
            ExactNumberParseReason::InvalidRadix,
            usize::from(negative) + 2,
            format!("base-{declared} prefix does not match requested radix {radix}"),
        ));
    }
    let prefix = usize::from(declared_prefix_radix.is_some()) * 2;
    let digits = &unsigned[prefix..];
    if digits.is_empty() {
        return Err(parse_failure(
            ExactNumberParseReason::Empty,
            text.chars().count() + 1,
            "radix Number contains no digits",
        ));
    }
    for (offset, character) in digits.chars().enumerate() {
        if character.to_digit(radix).is_none() {
            return Err(parse_failure(
                ExactNumberParseReason::InvalidDigit,
                usize::from(negative) + prefix + offset + 1,
                format!("`{character}` is not a base-{radix} digit"),
            ));
        }
    }
    validate_digit_budget(digits.chars().count(), text.chars().count(), profile)?;
    let magnitude = BigUint::parse_bytes(digits.as_bytes(), radix).ok_or_else(|| {
        parse_failure(
            ExactNumberParseReason::InvalidDigit,
            usize::from(negative) + prefix + 1,
            format!("Number is not valid base-{radix} text"),
        )
    })?;
    let numerator = BigInt::from_biguint(
        if magnitude.is_zero() {
            Sign::NoSign
        } else if negative {
            Sign::Minus
        } else {
            Sign::Plus
        },
        magnitude,
    );
    ExactNumber::from_ratio_with_profile(numerator, BigUint::one(), profile)
        .map_err(|error| parse_failure(ExactNumberParseReason::ResourceLimit, 1, error.to_string()))
}

fn split_once_unique(
    text: &str,
    separator: char,
) -> Result<Option<(&str, &str)>, ExactNumberError> {
    let Some(index) = text.find(separator) else {
        return Ok(None);
    };
    if text[index + separator.len_utf8()..].contains(separator) {
        return Err(ExactNumberError::new(format!(
            "exact Number text repeats `{separator}`"
        )));
    }
    Ok(Some((
        &text[..index],
        &text[index + separator.len_utf8()..],
    )))
}

fn parse_signed_integer(text: &str) -> Result<BigInt, ExactNumberError> {
    if text.is_empty() || text == "-" || text == "+" {
        return Err(ExactNumberError::new("exact Number integer is empty"));
    }
    if text.starts_with('+') {
        return Err(ExactNumberError::new(
            "exact Number text does not accept a leading plus sign",
        ));
    }
    BigInt::parse_bytes(text.as_bytes(), 10)
        .ok_or_else(|| ExactNumberError::new(format!("`{text}` is not an exact integer")))
}

fn parse_unsigned_integer(text: &str) -> Result<BigUint, ExactNumberError> {
    if text.is_empty() {
        return Err(ExactNumberError::new("exact Number denominator is empty"));
    }
    BigUint::parse_bytes(text.as_bytes(), 10)
        .ok_or_else(|| ExactNumberError::new(format!("`{text}` is not a positive integer")))
}

fn parse_decimal(
    text: &str,
    profile: ExactNumberSemanticProfileV1,
) -> Result<ExactNumber, ExactNumberError> {
    let (mantissa, exponent_text) = match text.find(['e', 'E']) {
        Some(index) => {
            if text[index + 1..].contains(['e', 'E']) {
                return Err(ExactNumberError::new(
                    "exact Number text repeats the exponent marker",
                ));
            }
            (&text[..index], Some(&text[index + 1..]))
        }
        None => (text, None),
    };
    let exponent = match exponent_text {
        Some(value) if !value.is_empty() => value
            .parse::<i32>()
            .map_err(|_| ExactNumberError::new("exact Number exponent is out of range"))?,
        Some(_) => {
            return Err(ExactNumberError::new("exact Number exponent is empty"));
        }
        None => 0,
    };
    let negative = mantissa.starts_with('-');
    let unsigned = if negative { &mantissa[1..] } else { mantissa };
    if unsigned.starts_with('+') || mantissa.starts_with('+') {
        return Err(ExactNumberError::new(
            "exact Number text does not accept a leading plus sign",
        ));
    }
    let (whole, fraction) = match split_once_unique(unsigned, '.')? {
        Some(parts) => parts,
        None => (unsigned, ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return Err(ExactNumberError::new("exact Number mantissa is empty"));
    }
    if !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ExactNumberError::new(format!(
            "`{text}` is not an exact decimal Number"
        )));
    }
    let digits = format!("{whole}{fraction}");
    let magnitude = BigUint::parse_bytes(digits.as_bytes(), 10)
        .ok_or_else(|| ExactNumberError::new(format!("`{text}` is not an exact decimal Number")))?;
    let scale = i64::try_from(fraction.len())
        .map_err(|_| ExactNumberError::new("exact Number decimal scale is out of range"))?
        - i64::from(exponent);
    let pow = u32::try_from(scale.unsigned_abs())
        .map_err(|_| ExactNumberError::new("exact Number decimal scale is out of range"))?;
    if usize::try_from(pow).unwrap_or(usize::MAX) > profile.max_parsed_digits {
        return Err(ExactNumberError::new(
            "exact Number decimal scale exceeds the parsed digit budget",
        ));
    }
    let power = BigUint::from(10_u8).pow(pow);
    let (magnitude, denominator) = if scale >= 0 {
        (magnitude, power)
    } else {
        (magnitude * power, BigUint::one())
    };
    let numerator = BigInt::from_biguint(
        if magnitude.is_zero() {
            Sign::NoSign
        } else if negative {
            Sign::Minus
        } else {
            Sign::Plus
        },
        magnitude,
    );
    ExactNumber::from_ratio_with_profile(numerator, denominator, profile)
}

fn check_formatted_digits(
    text: String,
    profile: ExactNumberSemanticProfileV1,
) -> Result<String, ExactNumberError> {
    let digit_count = text.bytes().filter(u8::is_ascii_digit).count();
    if digit_count > profile.max_formatted_digits {
        return Err(ExactNumberError::new(format!(
            "exact Number formatted digit budget exceeded: {digit_count} > {}",
            profile.max_formatted_digits
        )));
    }
    Ok(text)
}

fn encode_positive_order_path(mut numerator: BigUint, mut denominator: BigUint) -> Vec<u8> {
    debug_assert!(!numerator.is_zero());
    debug_assert!(!denominator.is_zero());
    let mut output = Vec::new();
    if numerator == denominator {
        output.push(1);
        return output;
    }
    let mut right = numerator > denominator;
    output.push(if right { 2 } else { 0 });
    loop {
        let run = if right {
            let run = (&numerator - BigUint::one()) / &denominator;
            numerator -= &run * &denominator;
            run
        } else {
            let run = (&denominator - BigUint::one()) / &numerator;
            denominator -= &run * &numerator;
            run
        };
        encode_positive_integer_order(&mut output, &run, !right);
        if numerator == denominator {
            output.push(1);
            break;
        }
        output.push(if right { 0 } else { 2 });
        right = !right;
    }
    output
}

fn encode_positive_integer_order(output: &mut Vec<u8>, value: &BigUint, descending: bool) {
    debug_assert!(!value.is_zero());
    let encode_token = |bit: bool| {
        let token = if bit { 2 } else { 0 };
        if descending { 2 - token } else { token }
    };
    let bits = value.bits();
    output.extend(std::iter::repeat_n(encode_token(true), bits as usize));
    output.push(encode_token(false));
    for position in (0..bits.saturating_sub(1)).rev() {
        output.push(encode_token(value.bit(position)));
    }
}

impl FromStr for ExactNumber {
    type Err = ExactNumberError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_strict(value, None).map_err(Into::into)
    }
}

impl fmt::Display for ExactNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_canonical_text() {
            Ok(text) => formatter.write_str(&text),
            Err(_) => write!(formatter, "{}/{}", self.numerator, self.denominator),
        }
    }
}

impl PartialEq for ExactNumber {
    fn eq(&self, other: &Self) -> bool {
        self.numerator == other.numerator && self.denominator == other.denominator
    }
}

impl Eq for ExactNumber {}

impl PartialOrd for ExactNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExactNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.numerator * BigInt::from(other.denominator.clone()))
            .cmp(&(&other.numerator * BigInt::from(self.denominator.clone())))
    }
}

impl Hash for ExactNumber {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.sign().hash(state);
        self.numerator.magnitude().to_bytes_be().hash(state);
        self.denominator.to_bytes_be().hash(state);
    }
}

impl Serialize for ExactNumber {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(3)?;
        tuple.serialize_element(&(self.sign() as u8))?;
        tuple.serialize_element(&self.numerator_magnitude_bytes())?;
        tuple.serialize_element(&self.denominator_bytes())?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for ExactNumber {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ExactNumberVisitor;

        impl<'de> Visitor<'de> for ExactNumberVisitor {
            type Value = ExactNumber;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a canonical exact Number tuple")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let sign = sequence
                    .next_element::<u8>()?
                    .ok_or_else(|| A::Error::custom("missing exact Number sign"))?;
                let numerator = sequence
                    .next_element::<Vec<u8>>()?
                    .ok_or_else(|| A::Error::custom("missing exact Number numerator"))?;
                let denominator = sequence
                    .next_element::<Vec<u8>>()?
                    .ok_or_else(|| A::Error::custom("missing exact Number denominator"))?;
                if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(A::Error::custom("exact Number tuple has trailing fields"));
                }
                let sign = ExactNumberSign::try_from(sign).map_err(A::Error::custom)?;
                ExactNumber::from_canonical_bytes(sign, &numerator, &denominator)
                    .map_err(A::Error::custom)
            }
        }

        deserializer.deserialize_tuple(3, ExactNumberVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;

    fn number(text: &str) -> ExactNumber {
        text.parse().unwrap()
    }

    #[test]
    fn decimals_and_fractions_share_one_canonical_value() {
        assert_eq!(number("1"), number("1.00"));
        assert_eq!(
            number("0.1").checked_add(&number("0.2")).unwrap(),
            number("0.3")
        );
        assert_eq!(
            number("1/3")
                .checked_mul(&ExactNumber::from_i64(3))
                .unwrap(),
            ExactNumber::one()
        );
        assert_eq!(number("-0.0"), ExactNumber::zero());
        assert_eq!(number("1.25e2"), ExactNumber::from_i64(125));
        assert_eq!(number("2/4"), number("0.5"));
    }

    #[test]
    fn strict_parsing_reports_stable_one_based_failures_and_radix_values() {
        let failure = |text: &str| ExactNumber::parse_strict(text, None).unwrap_err();
        assert_eq!(failure("").reason(), ExactNumberParseReason::Empty);
        assert_eq!(failure("").position(), 1);
        assert_eq!(failure(" 1").reason(), ExactNumberParseReason::Whitespace);
        assert_eq!(failure(" 1").position(), 1);
        assert_eq!(
            failure("12x").reason(),
            ExactNumberParseReason::InvalidDigit
        );
        assert_eq!(failure("12x").position(), 3);
        assert_eq!(
            failure("1e").reason(),
            ExactNumberParseReason::InvalidExponent
        );
        assert_eq!(failure("1e").position(), 3);
        assert_eq!(
            failure("1/0").reason(),
            ExactNumberParseReason::ZeroDenominator
        );
        assert_eq!(failure("1/0").position(), 3);

        assert_eq!(
            ExactNumber::parse_strict("0xff", Some(16)).unwrap(),
            ExactNumber::from_i64(255)
        );
        assert_eq!(
            ExactNumber::parse_strict("-101", Some(2)).unwrap(),
            ExactNumber::from_i64(-5)
        );
        let invalid = ExactNumber::parse_strict("102", Some(2)).unwrap_err();
        assert_eq!(invalid.reason(), ExactNumberParseReason::InvalidDigit);
        assert_eq!(invalid.position(), 3);
        let mismatched_prefix = ExactNumber::parse_strict("0b10", Some(16)).unwrap_err();
        assert_eq!(
            mismatched_prefix.reason(),
            ExactNumberParseReason::InvalidRadix
        );
        assert_eq!(mismatched_prefix.position(), 2);
    }

    #[test]
    fn explicit_binary64_boundary_preserves_the_input_bits_exactly() {
        let value = ExactNumber::from_f64_boundary_exact(0.1).unwrap();
        assert_eq!(value, number("3602879701896397/36028797018963968"));
        assert_eq!(
            value.to_f64_host_rounded().unwrap().to_bits(),
            0.1_f64.to_bits()
        );
        assert!(ExactNumber::from_f64_boundary_exact(f64::NAN).is_err());
    }

    #[test]
    fn canonical_encoding_rejects_alternates() {
        assert!(ExactNumber::from_canonical_bytes(ExactNumberSign::Zero, &[], &[1]).is_ok());
        assert!(ExactNumber::from_canonical_bytes(ExactNumberSign::Positive, &[1], &[2]).is_ok());
        assert!(ExactNumber::from_canonical_bytes(ExactNumberSign::Positive, &[2], &[4]).is_err());
        assert!(ExactNumber::from_canonical_bytes(ExactNumberSign::Zero, &[0], &[1]).is_err());
        assert!(
            ExactNumber::from_canonical_bytes(ExactNumberSign::Positive, &[1], &[0, 2]).is_err()
        );
    }

    #[test]
    fn canonical_text_terminates_or_uses_reduced_fraction() {
        assert_eq!(number("10/8").to_canonical_text().unwrap(), "1.25");
        assert_eq!(number("-1/8").to_canonical_text().unwrap(), "-0.125");
        assert_eq!(number("2/6").to_canonical_text().unwrap(), "1/3");
    }

    #[test]
    fn ordering_and_hash_follow_normalized_rationals() {
        let hash = |value: &ExactNumber| {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash(&number("1.0")), hash(&number("2/2")));
        assert!(number("-1/3") < number("0"));
        assert!(number("3/2") > ExactNumber::one());
    }

    #[test]
    fn canonical_order_bytes_are_prefix_free_and_sort_exactly() {
        let values = [
            "-100000000000000000000000000000000000000",
            "-3",
            "-3/2",
            "-2/3",
            "-1/2",
            "0",
            "1/100000000000000000000000000000000000000",
            "1/3",
            "1/2",
            "2/3",
            "1",
            "3/2",
            "2",
            "3",
            "100000000000000000000000000000000000000",
        ]
        .map(number);
        for pair in values.windows(2) {
            assert!(pair[0] < pair[1]);
            let left = pair[0].canonical_order_bytes();
            let right = pair[1].canonical_order_bytes();
            assert!(left < right);
            assert!(!left.starts_with(&right));
            assert!(!right.starts_with(&left));
        }
    }

    #[test]
    fn exact_rounding_covers_ties_and_direction() {
        let quantum = number("1");
        for (value, rule, expected) in [
            ("5/2", ExactRoundingRule::NearestEven, "2"),
            ("7/2", ExactRoundingRule::NearestEven, "4"),
            ("-5/2", ExactRoundingRule::NearestEven, "-2"),
            ("-7/2", ExactRoundingRule::NearestEven, "-4"),
            ("5/2", ExactRoundingRule::NearestAwayFromZero, "3"),
            ("-5/2", ExactRoundingRule::NearestAwayFromZero, "-3"),
            ("7/3", ExactRoundingRule::TowardZero, "2"),
            ("-7/3", ExactRoundingRule::TowardZero, "-2"),
            ("7/3", ExactRoundingRule::TowardPositive, "3"),
            ("-7/3", ExactRoundingRule::TowardPositive, "-2"),
            ("7/3", ExactRoundingRule::TowardNegative, "2"),
            ("-7/3", ExactRoundingRule::TowardNegative, "-3"),
            ("7/3", ExactRoundingRule::AwayFromZero, "3"),
            ("-7/3", ExactRoundingRule::AwayFromZero, "-3"),
        ] {
            assert_eq!(
                number(value).round_to(&quantum, rule).unwrap(),
                number(expected),
                "{value} using {}",
                rule.as_tag()
            );
        }
        assert_eq!(
            number("10/3")
                .round_to(&number("0.01"), ExactRoundingRule::NearestEven)
                .unwrap(),
            number("3.33")
        );
        assert!(
            number("1")
                .round_to(&ExactNumber::zero(), ExactRoundingRule::TowardZero)
                .is_err()
        );
        assert!(
            number("1")
                .round_to(&number("-1"), ExactRoundingRule::TowardZero)
                .is_err()
        );
    }

    #[test]
    fn exact_rounding_rules_use_the_public_boon_tags() {
        for rule in ExactRoundingRule::ALL {
            assert_eq!(ExactRoundingRule::from_tag(rule.as_tag()), Some(rule));
        }
        assert_eq!(ExactRoundingRule::from_tag("Nearest"), None);
    }

    #[test]
    fn whole_conversions_and_euclidean_remainder_are_checked() {
        assert_eq!(number("42").to_i64_exact().unwrap(), 42);
        assert!(number("42.5").to_i64_exact().is_err());
        assert!(number("-1").to_usize_exact().is_err());
        assert_eq!(number("-5").checked_rem(&number("3")).unwrap(), number("1"));
        assert!(number("1/2").checked_rem(&number("2")).is_err());
    }
}
