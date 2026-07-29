use crate::{ExactNumber, ExactNumberError};
use bytes::Bytes;
use num_bigint::{BigInt, BigUint, Sign};
use num_traits::{One, Signed, Zero};
use serde::de::{Error as _, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::error::Error;
use std::fmt;

/// Deterministic semantic ceiling for one fixed-width BITS value.
///
/// This is a resource bound, not a machine-word width. Backends may impose a
/// smaller target eligibility bound, but may never silently truncate a value
/// accepted by the language.
pub const MAX_BITS_WIDTH: u32 = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BitsDirection {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BitsInterpretation {
    Unsigned,
    TwosComplement,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BitsByteOrder {
    BigEndian,
    LittleEndian,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BitsArithmeticFailure {
    Underflow,
    Overflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitsError {
    message: String,
}

impl BitsError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BitsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for BitsError {}

impl From<ExactNumberError> for BitsError {
    fn from(error: ExactNumberError) -> Self {
        Self::new(error.to_string())
    }
}

/// Boon's canonical immutable fixed-width raw bit sequence.
///
/// Bytes are big-endian. Their length is exactly `ceil(width / 8)`, and any
/// unused high bits in the first byte are zero. Width is part of the value's
/// static type and is also retained here so persistence and wire boundaries can
/// validate canonical bytes without relying on external schema context.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Bits {
    width: u32,
    bytes: Bytes,
}

impl Bits {
    pub fn zero(width: u32) -> Result<Self, BitsError> {
        validate_width(width)?;
        Ok(Self {
            width,
            bytes: vec![0; byte_len(width)?].into(),
        })
    }

    pub fn from_canonical_bytes(width: u32, bytes: impl Into<Bytes>) -> Result<Self, BitsError> {
        validate_width(width)?;
        let bytes = bytes.into();
        let expected = byte_len(width)?;
        if bytes.len() != expected {
            return Err(BitsError::new(format!(
                "BITS[{width}] requires exactly {expected} canonical byte(s), found {}",
                bytes.len()
            )));
        }
        let unused = (8 - width % 8) % 8;
        if unused != 0 && bytes.first().is_some_and(|byte| byte >> (8 - unused) != 0) {
            return Err(BitsError::new(format!(
                "BITS[{width}] has nonzero unused high bits"
            )));
        }
        Ok(Self { width, bytes })
    }

    pub fn from_biguint(width: u32, value: BigUint) -> Result<Self, BitsError> {
        validate_width(width)?;
        if value.bits() > u64::from(width) {
            return Err(BitsError::new(format!(
                "value requires {} bits and does not fit BITS[{width}]",
                value.bits()
            )));
        }
        let expected = byte_len(width)?;
        let encoded = value.to_bytes_be();
        let mut bytes = vec![0; expected];
        let start = expected
            .checked_sub(encoded.len())
            .ok_or_else(|| BitsError::new("BITS canonical byte length underflowed"))?;
        bytes[start..].copy_from_slice(&encoded);
        Self::from_canonical_bytes(width, bytes)
    }

    pub fn parse_encoded(width: u32, radix: u32, digits: &str) -> Result<Self, BitsError> {
        validate_width(width)?;
        if !(2..=36).contains(&radix) {
            return Err(BitsError::new(format!(
                "BITS literal radix must be between 2 and 36, found {radix}"
            )));
        }
        if digits.is_empty() {
            return Err(BitsError::new("BITS literal must include digits after `u`"));
        }
        let normalized = digits.chars().filter(|ch| *ch != '_').collect::<String>();
        if normalized.is_empty() {
            return Err(BitsError::new(
                "BITS literal must include at least one non-underscore digit",
            ));
        }
        if !normalized.chars().all(|ch| ch.is_digit(radix)) {
            return Err(BitsError::new(format!(
                "BITS literal `{radix}u{digits}` contains digits outside radix {radix}"
            )));
        }
        let value = BigUint::parse_bytes(normalized.as_bytes(), radix).ok_or_else(|| {
            BitsError::new(format!(
                "BITS literal `{radix}u{digits}` could not be decoded"
            ))
        })?;
        Self::from_biguint(width, value)
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }

    pub fn to_biguint(&self) -> BigUint {
        BigUint::from_bytes_be(&self.bytes)
    }

    pub fn to_radix_digits(&self, radix: u32) -> Result<String, BitsError> {
        if !(2..=36).contains(&radix) {
            return Err(BitsError::new(format!(
                "BITS formatting radix must be between 2 and 36, found {radix}"
            )));
        }
        Ok(self.to_biguint().to_str_radix(radix))
    }

    pub fn bit(
        &self,
        one_based_position: usize,
        direction: BitsDirection,
    ) -> Result<bool, BitsError> {
        let bit = self.bit_index(one_based_position, direction)?;
        Ok(self.to_biguint().bit(u64::from(bit)))
    }

    pub fn with_bit(
        &self,
        one_based_position: usize,
        direction: BitsDirection,
        value: bool,
    ) -> Result<Self, BitsError> {
        let bit = self.bit_index(one_based_position, direction)?;
        let mut raw = self.to_biguint();
        raw.set_bit(u64::from(bit), value);
        Self::from_biguint(self.width, raw)
    }

    pub fn slice(&self, one_based_from: usize, count: u32) -> Result<Self, BitsError> {
        if count == 0 {
            return Err(BitsError::new("Bits/slice count must be positive"));
        }
        let from = checked_position(self.width, one_based_from)?;
        let end = from
            .checked_add(count)
            .ok_or_else(|| BitsError::new("Bits/slice range overflowed"))?;
        if end > self.width {
            return Err(BitsError::new(format!(
                "Bits/slice range from {one_based_from} with count {count} exceeds width {}",
                self.width
            )));
        }
        let shift = self.width - end;
        let mask = bit_mask(count);
        Self::from_biguint(count, (self.to_biguint() >> shift) & mask)
    }

    pub fn with_slice(&self, one_based_from: usize, value: &Self) -> Result<Self, BitsError> {
        let from = checked_position(self.width, one_based_from)?;
        let end = from
            .checked_add(value.width)
            .ok_or_else(|| BitsError::new("Bits/set_slice range overflowed"))?;
        if end > self.width {
            return Err(BitsError::new(format!(
                "Bits/set_slice range from {one_based_from} with width {} exceeds width {}",
                value.width, self.width
            )));
        }
        let shift = self.width - end;
        let range_mask = bit_mask(value.width) << shift;
        let full_mask = bit_mask(self.width);
        let raw = (self.to_biguint() & (&full_mask ^ &range_mask)) | (value.to_biguint() << shift);
        Self::from_biguint(self.width, raw)
    }

    pub fn concat(&self, right: &Self) -> Result<Self, BitsError> {
        let width = self
            .width
            .checked_add(right.width)
            .ok_or_else(|| BitsError::new("Bits/concat width overflowed"))?;
        validate_width(width)?;
        Self::from_biguint(
            width,
            (self.to_biguint() << right.width) | right.to_biguint(),
        )
    }

    pub fn bit_and(&self, other: &Self) -> Result<Self, BitsError> {
        self.same_width(other, "Bits/and")?;
        Self::from_biguint(self.width, self.to_biguint() & other.to_biguint())
    }

    pub fn bit_or(&self, other: &Self) -> Result<Self, BitsError> {
        self.same_width(other, "Bits/or")?;
        Self::from_biguint(self.width, self.to_biguint() | other.to_biguint())
    }

    pub fn bit_xor(&self, other: &Self) -> Result<Self, BitsError> {
        self.same_width(other, "Bits/xor")?;
        Self::from_biguint(self.width, self.to_biguint() ^ other.to_biguint())
    }

    pub fn bit_not(&self) -> Result<Self, BitsError> {
        Self::from_biguint(self.width, self.to_biguint() ^ bit_mask(self.width))
    }

    pub fn logical_shift_left(&self, amount: usize) -> Result<Self, BitsError> {
        if amount >= self.width as usize {
            return Self::zero(self.width);
        }
        Self::from_biguint(
            self.width,
            (self.to_biguint() << amount) & bit_mask(self.width),
        )
    }

    pub fn logical_shift_right(&self, amount: usize) -> Result<Self, BitsError> {
        if amount >= self.width as usize {
            return Self::zero(self.width);
        }
        Self::from_biguint(self.width, self.to_biguint() >> amount)
    }

    pub fn arithmetic_shift_right(&self, amount: usize) -> Result<Self, BitsError> {
        if amount == 0 {
            return Ok(self.clone());
        }
        let signed = self.to_signed_bigint();
        let shifted = if amount >= self.width as usize {
            if signed.is_negative() {
                BigInt::from(-1)
            } else {
                BigInt::zero()
            }
        } else {
            signed >> amount
        };
        Self::from_bigint_twos_complement(self.width, shifted)
    }

    pub fn rotate_left(&self, amount: usize) -> Result<Self, BitsError> {
        let amount = amount % self.width as usize;
        if amount == 0 {
            return Ok(self.clone());
        }
        let raw = self.to_biguint();
        let width = self.width as usize;
        Self::from_biguint(
            self.width,
            ((&raw << amount) | (raw >> (width - amount))) & bit_mask(self.width),
        )
    }

    pub fn rotate_right(&self, amount: usize) -> Result<Self, BitsError> {
        let amount = amount % self.width as usize;
        if amount == 0 {
            return Ok(self.clone());
        }
        let raw = self.to_biguint();
        let width = self.width as usize;
        Self::from_biguint(
            self.width,
            ((&raw >> amount) | (raw << (width - amount))) & bit_mask(self.width),
        )
    }

    pub fn zero_extend(&self, new_width: u32) -> Result<Self, BitsError> {
        if new_width < self.width {
            return Err(BitsError::new(format!(
                "Bits/zero_extend width {new_width} is smaller than {}",
                self.width
            )));
        }
        Self::from_biguint(new_width, self.to_biguint())
    }

    pub fn sign_extend(&self, new_width: u32) -> Result<Self, BitsError> {
        if new_width < self.width {
            return Err(BitsError::new(format!(
                "Bits/sign_extend width {new_width} is smaller than {}",
                self.width
            )));
        }
        Self::from_bigint_twos_complement(new_width, self.to_signed_bigint())
    }

    pub fn truncate(&self, new_width: u32) -> Result<Self, BitsError> {
        if new_width == 0 || new_width > self.width {
            return Err(BitsError::new(format!(
                "Bits/truncate width must be between 1 and {}, found {new_width}",
                self.width
            )));
        }
        Self::from_biguint(new_width, self.to_biguint() & bit_mask(new_width))
    }

    pub fn compare(
        &self,
        other: &Self,
        interpretation: BitsInterpretation,
    ) -> Result<Ordering, BitsError> {
        self.same_width(other, "Bits/compare")?;
        Ok(match interpretation {
            BitsInterpretation::Unsigned => self.to_biguint().cmp(&other.to_biguint()),
            BitsInterpretation::TwosComplement => {
                self.to_signed_bigint().cmp(&other.to_signed_bigint())
            }
        })
    }

    pub fn add_or_wrap(&self, other: &Self) -> Result<Self, BitsError> {
        self.same_width(other, "Bits/add_or_wrap")?;
        Self::from_biguint(
            self.width,
            (self.to_biguint() + other.to_biguint()) & bit_mask(self.width),
        )
    }

    pub fn add_widening(
        &self,
        other: &Self,
        interpretation: BitsInterpretation,
    ) -> Result<Self, BitsError> {
        self.same_width(other, "Bits/add_widening")?;
        let width = self
            .width
            .checked_add(1)
            .ok_or_else(|| BitsError::new("Bits/add_widening width overflowed"))?;
        validate_width(width)?;
        match interpretation {
            BitsInterpretation::Unsigned => {
                Self::from_biguint(width, self.to_biguint() + other.to_biguint())
            }
            BitsInterpretation::TwosComplement => Self::from_bigint_twos_complement(
                width,
                self.to_signed_bigint() + other.to_signed_bigint(),
            ),
        }
    }

    pub fn try_add(
        &self,
        other: &Self,
        interpretation: BitsInterpretation,
    ) -> Result<Result<Self, BitsArithmeticFailure>, BitsError> {
        self.same_width(other, "Bits/try_add")?;
        match interpretation {
            BitsInterpretation::Unsigned => {
                let value = self.to_biguint() + other.to_biguint();
                if value.bits() > u64::from(self.width) {
                    Ok(Err(BitsArithmeticFailure::Overflow))
                } else {
                    Ok(Ok(Self::from_biguint(self.width, value)?))
                }
            }
            BitsInterpretation::TwosComplement => {
                let value = self.to_signed_bigint() + other.to_signed_bigint();
                if !signed_fits(self.width, &value) {
                    Ok(Err(BitsArithmeticFailure::Overflow))
                } else {
                    Ok(Ok(Self::from_bigint_twos_complement(self.width, value)?))
                }
            }
        }
    }

    pub fn subtract_or_wrap(&self, other: &Self) -> Result<Self, BitsError> {
        self.same_width(other, "Bits/subtract_or_wrap")?;
        let modulus = BigInt::one() << self.width;
        let difference = BigInt::from(self.to_biguint()) - BigInt::from(other.to_biguint());
        let wrapped = ((difference % &modulus) + &modulus) % &modulus;
        let (_, bytes) = wrapped.to_bytes_be();
        Self::from_biguint(self.width, BigUint::from_bytes_be(&bytes))
    }

    pub fn try_subtract(
        &self,
        other: &Self,
        interpretation: BitsInterpretation,
    ) -> Result<Result<Self, BitsArithmeticFailure>, BitsError> {
        self.same_width(other, "Bits/try_subtract")?;
        match interpretation {
            BitsInterpretation::Unsigned => {
                let left = self.to_biguint();
                let right = other.to_biguint();
                if left < right {
                    Ok(Err(BitsArithmeticFailure::Underflow))
                } else {
                    Ok(Ok(Self::from_biguint(self.width, left - right)?))
                }
            }
            BitsInterpretation::TwosComplement => {
                let value = self.to_signed_bigint() - other.to_signed_bigint();
                if !signed_fits(self.width, &value) {
                    Ok(Err(BitsArithmeticFailure::Overflow))
                } else {
                    Ok(Ok(Self::from_bigint_twos_complement(self.width, value)?))
                }
            }
        }
    }

    pub fn from_number(
        width: u32,
        value: &ExactNumber,
        interpretation: BitsInterpretation,
    ) -> Result<Self, BitsError> {
        if !value.is_whole() {
            return Err(BitsError::new("Number/to_bits requires a whole Number"));
        }
        let integer = value.to_bigint_exact()?;
        match interpretation {
            BitsInterpretation::Unsigned => {
                let Some(value) = integer.to_biguint() else {
                    return Err(BitsError::new(
                        "Number/to_bits Unsigned rejects negative values",
                    ));
                };
                Self::from_biguint(width, value)
            }
            BitsInterpretation::TwosComplement => {
                if !signed_fits(width, &integer) {
                    return Err(BitsError::new(format!(
                        "Number does not fit signed BITS[{width}]"
                    )));
                }
                Self::from_bigint_twos_complement(width, integer)
            }
        }
    }

    pub fn to_number(&self, interpretation: BitsInterpretation) -> Result<ExactNumber, BitsError> {
        let integer = match interpretation {
            BitsInterpretation::Unsigned => BigInt::from(self.to_biguint()),
            BitsInterpretation::TwosComplement => self.to_signed_bigint(),
        };
        Ok(ExactNumber::from_ratio(integer, BigUint::one())?)
    }

    pub fn to_bytes(&self, byte_order: BitsByteOrder) -> Result<Bytes, BitsError> {
        if !self.width.is_multiple_of(8) {
            return Err(BitsError::new(format!(
                "BITS[{}] is not byte-aligned; pad it explicitly before converting to BYTES",
                self.width
            )));
        }
        match byte_order {
            BitsByteOrder::BigEndian => Ok(self.bytes.clone()),
            BitsByteOrder::LittleEndian => {
                let mut bytes = self.bytes.to_vec();
                bytes.reverse();
                Ok(bytes.into())
            }
        }
    }

    pub fn from_bytes(
        width: u32,
        bytes: impl Into<Bytes>,
        byte_order: BitsByteOrder,
    ) -> Result<Self, BitsError> {
        validate_width(width)?;
        if !width.is_multiple_of(8) {
            return Err(BitsError::new(format!(
                "BITS[{width}] is not byte-aligned; pad the BYTES explicitly before converting"
            )));
        }
        let mut bytes = bytes.into();
        if byte_order == BitsByteOrder::LittleEndian {
            let mut reversed = bytes.to_vec();
            reversed.reverse();
            bytes = reversed.into();
        }
        Self::from_canonical_bytes(width, bytes)
    }

    fn bit_index(
        &self,
        one_based_position: usize,
        direction: BitsDirection,
    ) -> Result<u32, BitsError> {
        let zero_based = checked_position(self.width, one_based_position)?;
        Ok(match direction {
            BitsDirection::Left => self.width - zero_based - 1,
            BitsDirection::Right => zero_based,
        })
    }

    fn same_width(&self, other: &Self, operation: &str) -> Result<(), BitsError> {
        if self.width == other.width {
            Ok(())
        } else {
            Err(BitsError::new(format!(
                "{operation} requires equal widths, found BITS[{}] and BITS[{}]",
                self.width, other.width
            )))
        }
    }

    fn to_signed_bigint(&self) -> BigInt {
        let unsigned = BigInt::from(self.to_biguint());
        if self
            .bit(1, BitsDirection::Left)
            .expect("canonical BITS always has a first bit")
        {
            unsigned - (BigInt::one() << self.width)
        } else {
            unsigned
        }
    }

    fn from_bigint_twos_complement(width: u32, value: BigInt) -> Result<Self, BitsError> {
        validate_width(width)?;
        if !signed_fits(width, &value) {
            return Err(BitsError::new(format!(
                "signed value does not fit BITS[{width}]"
            )));
        }
        let encoded = if value.is_negative() {
            value + (BigInt::one() << width)
        } else {
            value
        };
        let (sign, bytes) = encoded.to_bytes_be();
        debug_assert!(matches!(sign, Sign::NoSign | Sign::Plus));
        Self::from_biguint(width, BigUint::from_bytes_be(&bytes))
    }
}

impl Ord for Bits {
    fn cmp(&self, other: &Self) -> Ordering {
        for position in 1..=self.width.min(other.width) as usize {
            match self
                .bit(position, BitsDirection::Left)
                .expect("canonical BITS position")
                .cmp(
                    &other
                        .bit(position, BitsDirection::Left)
                        .expect("canonical BITS position"),
                ) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        }
        self.width.cmp(&other.width)
    }
}

impl PartialOrd for Bits {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Bits {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = self.to_biguint().to_str_radix(2);
        let width = self.width as usize;
        write!(formatter, "BITS[{}] {{ 2u", self.width)?;
        for _ in digits.len()..width {
            formatter.write_str("0")?;
        }
        formatter.write_str(&digits)?;
        formatter.write_str(" }")
    }
}

impl Serialize for Bits {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(2)?;
        tuple.serialize_element(&self.width)?;
        tuple.serialize_element(self.bytes.as_ref())?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for Bits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BitsVisitor;

        impl<'de> Visitor<'de> for BitsVisitor {
            type Value = Bits;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("canonical [width, big-endian bytes] BITS tuple")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let width = sequence
                    .next_element::<u32>()?
                    .ok_or_else(|| A::Error::custom("BITS tuple is missing width"))?;
                let bytes = sequence
                    .next_element::<Vec<u8>>()?
                    .ok_or_else(|| A::Error::custom("BITS tuple is missing bytes"))?;
                if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(A::Error::custom("BITS tuple has trailing elements"));
                }
                Bits::from_canonical_bytes(width, bytes).map_err(A::Error::custom)
            }
        }

        deserializer.deserialize_tuple(2, BitsVisitor)
    }
}

fn validate_width(width: u32) -> Result<(), BitsError> {
    if width == 0 {
        return Err(BitsError::new("BITS width must be positive"));
    }
    if width > MAX_BITS_WIDTH {
        return Err(BitsError::new(format!(
            "BITS width {width} exceeds deterministic semantic limit {MAX_BITS_WIDTH}"
        )));
    }
    Ok(())
}

fn byte_len(width: u32) -> Result<usize, BitsError> {
    usize::try_from(width.div_ceil(8))
        .map_err(|_| BitsError::new("BITS byte length does not fit this target"))
}

fn checked_position(width: u32, one_based_position: usize) -> Result<u32, BitsError> {
    if one_based_position == 0 {
        return Err(BitsError::new("BITS positions are one-based"));
    }
    let zero_based = u32::try_from(one_based_position - 1)
        .map_err(|_| BitsError::new("BITS position does not fit the semantic width domain"))?;
    if zero_based >= width {
        return Err(BitsError::new(format!(
            "BITS position {one_based_position} exceeds width {width}"
        )));
    }
    Ok(zero_based)
}

fn bit_mask(width: u32) -> BigUint {
    (BigUint::one() << width) - BigUint::one()
}

fn signed_fits(width: u32, value: &BigInt) -> bool {
    if width == 0 {
        return false;
    }
    let bound = BigInt::one() << (width - 1);
    value >= &-&bound && value < &bound
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(width: u32, radix: u32, digits: &str) -> Bits {
        Bits::parse_encoded(width, radix, digits).unwrap()
    }

    #[test]
    fn literals_are_width_checked_and_canonical() {
        assert_eq!(bits(8, 2, "1010_0011").bytes().as_ref(), &[0b1010_0011]);
        assert_eq!(bits(12, 16, "a3").bytes().as_ref(), &[0x00, 0xa3]);
        assert!(Bits::parse_encoded(0, 2, "0").is_err());
        assert!(Bits::parse_encoded(7, 2, "1000_0000").is_err());
        assert!(Bits::from_canonical_bytes(7, vec![0x80]).is_err());
    }

    #[test]
    fn positions_slices_and_concat_follow_display_order() {
        let value = bits(8, 2, "1010_0011");
        assert!(value.bit(1, BitsDirection::Left).unwrap());
        assert!(value.bit(1, BitsDirection::Right).unwrap());
        assert!(!value.bit(3, BitsDirection::Right).unwrap());
        assert_eq!(value.slice(2, 3).unwrap(), bits(3, 2, "010"));
        assert_eq!(
            value.with_slice(2, &bits(3, 2, "111")).unwrap(),
            bits(8, 2, "1111_0011")
        );
        assert_eq!(
            bits(4, 2, "1010").concat(&bits(4, 2, "0011")).unwrap(),
            value
        );
        assert!(value.bit(0, BitsDirection::Left).is_err());
        assert!(value.slice(7, 3).is_err());
    }

    #[test]
    fn shifts_rotates_and_bitwise_operations_are_total() {
        let value = bits(8, 2, "1000_0011");
        assert_eq!(
            value.logical_shift_left(1).unwrap(),
            bits(8, 2, "0000_0110")
        );
        assert_eq!(
            value.logical_shift_right(2).unwrap(),
            bits(8, 2, "0010_0000")
        );
        assert_eq!(
            value.arithmetic_shift_right(2).unwrap(),
            bits(8, 2, "1110_0000")
        );
        assert_eq!(
            value.logical_shift_left(8).unwrap(),
            bits(8, 2, "0000_0000")
        );
        assert_eq!(value.rotate_left(1).unwrap(), bits(8, 2, "0000_0111"));
        assert_eq!(value.rotate_right(1).unwrap(), bits(8, 2, "1100_0001"));
        assert_eq!(value.bit_not().unwrap(), bits(8, 2, "0111_1100"));
    }

    #[test]
    fn arithmetic_names_make_width_and_overflow_explicit() {
        let max = bits(4, 2, "1111");
        let one = bits(4, 2, "0001");
        assert_eq!(max.add_or_wrap(&one).unwrap(), bits(4, 2, "0000"));
        assert_eq!(
            max.add_widening(&one, BitsInterpretation::Unsigned)
                .unwrap(),
            bits(5, 2, "1_0000")
        );
        assert_eq!(
            max.try_add(&one, BitsInterpretation::Unsigned).unwrap(),
            Err(BitsArithmeticFailure::Overflow)
        );
        assert_eq!(
            one.try_subtract(&max, BitsInterpretation::Unsigned)
                .unwrap(),
            Err(BitsArithmeticFailure::Underflow)
        );

        let signed_max = bits(4, 2, "0111");
        assert_eq!(
            signed_max
                .try_add(&one, BitsInterpretation::TwosComplement)
                .unwrap(),
            Err(BitsArithmeticFailure::Overflow)
        );
        assert_eq!(
            max.to_number(BitsInterpretation::TwosComplement).unwrap(),
            ExactNumber::from_i64(-1)
        );
    }

    #[test]
    fn serde_rejects_noncanonical_representation() {
        let value = bits(9, 16, "101");
        let encoded = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<Bits>(&encoded).unwrap(), value);
        assert!(serde_json::from_str::<Bits>("[9,[255,1]]").is_err());
    }

    #[test]
    fn byte_conversion_is_exact_aligned_and_ordered() {
        let value = bits(16, 16, "1234");
        assert_eq!(
            value.to_bytes(BitsByteOrder::BigEndian).unwrap().as_ref(),
            &[0x12, 0x34]
        );
        assert_eq!(
            value
                .to_bytes(BitsByteOrder::LittleEndian)
                .unwrap()
                .as_ref(),
            &[0x34, 0x12]
        );
        assert_eq!(
            Bits::from_bytes(16, [0x34, 0x12].as_slice(), BitsByteOrder::LittleEndian).unwrap(),
            value
        );
        assert!(
            bits(7, 2, "1010101")
                .to_bytes(BitsByteOrder::BigEndian)
                .is_err()
        );
        assert!(Bits::from_bytes(16, [0x12].as_slice(), BitsByteOrder::BigEndian).is_err());
    }

    #[test]
    fn exhaustive_small_width_semantics_match_integer_references() {
        fn raw_bits(width: u32, raw: u16) -> Bits {
            Bits::from_biguint(width, BigUint::from(raw)).unwrap()
        }

        fn assert_raw(actual: Bits, expected: u16) {
            assert_eq!(actual.to_biguint(), BigUint::from(expected));
        }

        fn signed_value(raw: u16, width: u32) -> i32 {
            let sign = 1_u16 << (width - 1);
            if raw & sign == 0 {
                i32::from(raw)
            } else {
                i32::from(raw) - (1_i32 << width)
            }
        }

        fn signed_encoding(value: i32, width: u32) -> u16 {
            if value < 0 {
                ((1_i32 << width) + value) as u16
            } else {
                value as u16
            }
        }

        for width in 1..=8_u32 {
            let modulus = 1_u16 << width;
            let mask = modulus - 1;
            let signed_min = -(1_i32 << (width - 1));
            let signed_max = (1_i32 << (width - 1)) - 1;

            for raw in 0..=mask {
                let value = raw_bits(width, raw);
                for position in 1..=width as usize {
                    assert_eq!(
                        value.bit(position, BitsDirection::Left).unwrap(),
                        raw & (1_u16 << (width as usize - position)) != 0
                    );
                    assert_eq!(
                        value.bit(position, BitsDirection::Right).unwrap(),
                        raw & (1_u16 << (position - 1)) != 0
                    );
                }

                assert_raw(value.bit_not().unwrap(), raw ^ mask);
                for amount in 0..=(width as usize * 2 + 1) {
                    let logical_left = if amount >= width as usize {
                        0
                    } else {
                        (raw << amount) & mask
                    };
                    let logical_right = if amount >= width as usize {
                        0
                    } else {
                        raw >> amount
                    };
                    let arithmetic_right = if amount >= width as usize {
                        if signed_value(raw, width) < 0 {
                            mask
                        } else {
                            0
                        }
                    } else {
                        signed_encoding(signed_value(raw, width) >> amount, width)
                    };
                    let rotation = amount % width as usize;
                    let rotate_left = if rotation == 0 {
                        raw
                    } else {
                        ((raw << rotation) | (raw >> (width as usize - rotation))) & mask
                    };
                    let rotate_right = if rotation == 0 {
                        raw
                    } else {
                        ((raw >> rotation) | (raw << (width as usize - rotation))) & mask
                    };

                    assert_raw(value.logical_shift_left(amount).unwrap(), logical_left);
                    assert_raw(value.logical_shift_right(amount).unwrap(), logical_right);
                    assert_raw(
                        value.arithmetic_shift_right(amount).unwrap(),
                        arithmetic_right,
                    );
                    assert_raw(value.rotate_left(amount).unwrap(), rotate_left);
                    assert_raw(value.rotate_right(amount).unwrap(), rotate_right);
                }

                assert_raw(value.zero_extend(width + 2).unwrap(), raw);
                assert_raw(
                    value.sign_extend(width + 2).unwrap(),
                    signed_encoding(signed_value(raw, width), width + 2),
                );
                for truncated_width in 1..=width {
                    assert_raw(
                        value.truncate(truncated_width).unwrap(),
                        raw & ((1_u16 << truncated_width) - 1),
                    );
                }

                for other_raw in 0..=mask {
                    let other = raw_bits(width, other_raw);
                    let signed_left = signed_value(raw, width);
                    let signed_right = signed_value(other_raw, width);

                    assert_raw(value.bit_and(&other).unwrap(), raw & other_raw);
                    assert_raw(value.bit_or(&other).unwrap(), raw | other_raw);
                    assert_raw(value.bit_xor(&other).unwrap(), raw ^ other_raw);
                    assert_raw(
                        value.add_or_wrap(&other).unwrap(),
                        raw.wrapping_add(other_raw) & mask,
                    );
                    assert_raw(
                        value.subtract_or_wrap(&other).unwrap(),
                        raw.wrapping_sub(other_raw) & mask,
                    );
                    assert_eq!(
                        value.compare(&other, BitsInterpretation::Unsigned).unwrap(),
                        raw.cmp(&other_raw)
                    );
                    assert_eq!(
                        value
                            .compare(&other, BitsInterpretation::TwosComplement)
                            .unwrap(),
                        signed_left.cmp(&signed_right)
                    );
                    assert_raw(
                        value
                            .add_widening(&other, BitsInterpretation::Unsigned)
                            .unwrap(),
                        raw + other_raw,
                    );
                    assert_raw(
                        value
                            .add_widening(&other, BitsInterpretation::TwosComplement)
                            .unwrap(),
                        signed_encoding(signed_left + signed_right, width + 1),
                    );

                    let unsigned_sum = u32::from(raw) + u32::from(other_raw);
                    match value.try_add(&other, BitsInterpretation::Unsigned).unwrap() {
                        Ok(actual) => {
                            assert!(unsigned_sum < u32::from(modulus));
                            assert_raw(actual, unsigned_sum as u16);
                        }
                        Err(failure) => {
                            assert_eq!(failure, BitsArithmeticFailure::Overflow);
                            assert!(unsigned_sum >= u32::from(modulus));
                        }
                    }

                    match value
                        .try_subtract(&other, BitsInterpretation::Unsigned)
                        .unwrap()
                    {
                        Ok(actual) => {
                            assert!(raw >= other_raw);
                            assert_raw(actual, raw - other_raw);
                        }
                        Err(failure) => {
                            assert_eq!(failure, BitsArithmeticFailure::Underflow);
                            assert!(raw < other_raw);
                        }
                    }

                    let signed_sum = signed_left + signed_right;
                    match value
                        .try_add(&other, BitsInterpretation::TwosComplement)
                        .unwrap()
                    {
                        Ok(actual) => {
                            assert!((signed_min..=signed_max).contains(&signed_sum));
                            assert_raw(actual, signed_encoding(signed_sum, width));
                        }
                        Err(failure) => {
                            assert_eq!(failure, BitsArithmeticFailure::Overflow);
                            assert!(!(signed_min..=signed_max).contains(&signed_sum));
                        }
                    }

                    let signed_difference = signed_left - signed_right;
                    match value
                        .try_subtract(&other, BitsInterpretation::TwosComplement)
                        .unwrap()
                    {
                        Ok(actual) => {
                            assert!((signed_min..=signed_max).contains(&signed_difference));
                            assert_raw(actual, signed_encoding(signed_difference, width));
                        }
                        Err(failure) => {
                            assert_eq!(failure, BitsArithmeticFailure::Overflow);
                            assert!(!(signed_min..=signed_max).contains(&signed_difference));
                        }
                    }
                }
            }
        }
    }
}
