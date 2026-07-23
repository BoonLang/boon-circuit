use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};

pub const KEY_CODEC_VERSION: u8 = 1;
pub const MAX_KEY_COMPONENTS: usize = 8;

const NUMBER_MARKER: u8 = 0x11;
const TEXT_MARKER: u8 = 0x22;
const BOOL_MARKER: u8 = 0x33;
const CLOSED_TAG_MARKER: u8 = 0x44;
const SIGN_BIT: u64 = 1 << 63;

/// A finite IEEE-754 binary64 value with canonical zero representation.
#[derive(Clone, Copy)]
pub struct FiniteNumber(u64);

impl FiniteNumber {
    pub fn new(value: f64) -> Result<Self, KeyError> {
        if !value.is_finite() {
            return Err(KeyError::NonFiniteNumber);
        }
        let bits = if value == 0.0 { 0 } else { value.to_bits() };
        Ok(Self(bits))
    }

    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }

    pub fn to_bits(self) -> u64 {
        self.0
    }
}

impl TryFrom<f64> for FiniteNumber {
    type Error = KeyError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Debug for FiniteNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(formatter)
    }
}

impl PartialEq for FiniteNumber {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for FiniteNumber {}

impl PartialOrd for FiniteNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FiniteNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        self.get().total_cmp(&other.get())
    }
}

impl Hash for FiniteNumber {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Compiler-assigned identity of a closed fieldless tag type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TagTypeId([u8; 16]);

impl TagTypeId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn from_u128(value: u128) -> Self {
        Self(value.to_be_bytes())
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// An ordinal variant of one statically known closed fieldless tag type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClosedTag {
    type_id: TagTypeId,
    ordinal: u32,
}

impl ClosedTag {
    pub const fn new(type_id: TagTypeId, ordinal: u32) -> Self {
        Self { type_id, ordinal }
    }

    pub const fn type_id(self) -> TagTypeId {
        self.type_id
    }

    pub const fn ordinal(self) -> u32 {
        self.ordinal
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StructuralValue {
    Number(FiniteNumber),
    Text(String),
    Bool(bool),
    ClosedTag(ClosedTag),
}

impl StructuralValue {
    pub fn number(value: f64) -> Result<Self, KeyError> {
        FiniteNumber::new(value).map(Self::Number)
    }

    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub const fn kind(&self) -> KeyKind {
        match self {
            Self::Number(_) => KeyKind::Number,
            Self::Text(_) => KeyKind::Text,
            Self::Bool(_) => KeyKind::Bool,
            Self::ClosedTag(value) => KeyKind::ClosedTag(value.type_id()),
        }
    }

    /// Deterministic semantic payload retained by a structural key. This does
    /// not claim allocator, enum, `String`, or `Vec` bookkeeping bytes.
    pub fn payload_bytes(&self) -> u64 {
        match self {
            Self::Number(_) => 8,
            Self::Text(value) => usize_to_u64(value.len()),
            Self::Bool(_) => 1,
            Self::ClosedTag(_) => 20,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum KeyKind {
    Number,
    Text,
    Bool,
    ClosedTag(TagTypeId),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Direction {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeyComponent {
    kind: KeyKind,
    direction: Direction,
}

impl KeyComponent {
    pub const fn new(kind: KeyKind, direction: Direction) -> Self {
        Self { kind, direction }
    }

    pub const fn kind(self) -> KeyKind {
        self.kind
    }

    pub const fn direction(self) -> Direction {
        self.direction
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StructuralKey(Vec<StructuralValue>);

impl StructuralKey {
    pub fn new(parts: Vec<StructuralValue>) -> Result<Self, KeyError> {
        if parts.len() > MAX_KEY_COMPONENTS {
            return Err(KeyError::InvalidKeyArity {
                actual: parts.len(),
                maximum: MAX_KEY_COMPONENTS,
            });
        }
        Ok(Self(parts))
    }

    pub fn parts(&self) -> &[StructuralValue] {
        &self.0
    }

    pub fn payload_bytes(&self) -> u64 {
        self.0.iter().fold(0_u64, |total, part| {
            total.saturating_add(part.payload_bytes())
        })
    }
}

fn usize_to_u64(value: usize) -> u64 {
    value.try_into().unwrap_or(u64::MAX)
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EncodedKey(Vec<u8>);

impl EncodedKey {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KeySchema(Vec<KeyComponent>);

impl KeySchema {
    pub fn new(components: Vec<KeyComponent>) -> Result<Self, KeyError> {
        if components.len() > MAX_KEY_COMPONENTS {
            return Err(KeyError::InvalidSchemaArity {
                actual: components.len(),
                maximum: MAX_KEY_COMPONENTS,
            });
        }
        Ok(Self(components))
    }

    pub fn components(&self) -> &[KeyComponent] {
        &self.0
    }

    pub fn encode(&self, key: &StructuralKey) -> Result<EncodedKey, KeyError> {
        self.validate_key(key)?;
        let mut output = Vec::with_capacity(1 + key.parts().len() * 10);
        output.push(KEY_CODEC_VERSION);
        for (specification, value) in self.0.iter().zip(key.parts()) {
            encode_component(&mut output, *specification, value, true);
        }
        Ok(EncodedKey(output))
    }

    pub fn compare(
        &self,
        left: &StructuralKey,
        right: &StructuralKey,
    ) -> Result<Ordering, KeyError> {
        Ok(self.encode(left)?.cmp(&self.encode(right)?))
    }

    pub(crate) fn encode_text_prefix(
        &self,
        leading: &[StructuralValue],
        prefix: &str,
    ) -> Result<Vec<u8>, KeyError> {
        if leading.len() >= self.0.len() {
            return Err(KeyError::PrefixHasNoTarget {
                leading: leading.len(),
                components: self.0.len(),
            });
        }
        let mut output = self.encode_structural_prefix(leading)?;
        output.reserve(prefix.len());
        let target = self.0[leading.len()];
        if target.kind != KeyKind::Text {
            return Err(KeyError::PrefixTargetIsNotText {
                component: leading.len(),
                actual: target.kind,
            });
        }
        encode_component(
            &mut output,
            target,
            &StructuralValue::Text(prefix.to_owned()),
            false,
        );
        Ok(output)
    }

    pub(crate) fn encode_structural_prefix(
        &self,
        leading: &[StructuralValue],
    ) -> Result<Vec<u8>, KeyError> {
        if leading.len() > self.0.len() {
            return Err(KeyError::PrefixHasNoTarget {
                leading: leading.len(),
                components: self.0.len(),
            });
        }
        let mut output = Vec::with_capacity(1 + leading.len() * 10);
        output.push(KEY_CODEC_VERSION);
        for (position, value) in leading.iter().enumerate() {
            let specification = self.0[position];
            self.validate_part(position, specification.kind, value)?;
            encode_component(&mut output, specification, value, true);
        }
        Ok(output)
    }

    pub(crate) fn encode_component_prefix(
        &self,
        leading: &[StructuralValue],
        value: &StructuralValue,
    ) -> Result<Vec<u8>, KeyError> {
        if leading.len() >= self.0.len() {
            return Err(KeyError::PrefixHasNoTarget {
                leading: leading.len(),
                components: self.0.len(),
            });
        }
        let mut output = self.encode_structural_prefix(leading)?;
        let specification = self.0[leading.len()];
        self.validate_part(leading.len(), specification.kind, value)?;
        encode_component(&mut output, specification, value, true);
        Ok(output)
    }

    fn validate_key(&self, key: &StructuralKey) -> Result<(), KeyError> {
        if key.parts().len() != self.0.len() {
            return Err(KeyError::WrongKeyArity {
                expected: self.0.len(),
                actual: key.parts().len(),
            });
        }
        for (position, (specification, value)) in self.0.iter().zip(key.parts()).enumerate() {
            self.validate_part(position, specification.kind, value)?;
        }
        Ok(())
    }

    fn validate_part(
        &self,
        component: usize,
        expected: KeyKind,
        value: &StructuralValue,
    ) -> Result<(), KeyError> {
        let actual = value.kind();
        if expected != actual {
            return Err(KeyError::WrongKeyKind {
                component,
                expected,
                actual,
            });
        }
        Ok(())
    }
}

fn encode_component(
    output: &mut Vec<u8>,
    specification: KeyComponent,
    value: &StructuralValue,
    terminate_text: bool,
) {
    let start = output.len();
    match value {
        StructuralValue::Number(value) => {
            output.push(NUMBER_MARKER);
            let bits = value.to_bits();
            let ordered = if bits & SIGN_BIT == 0 {
                bits ^ SIGN_BIT
            } else {
                !bits
            };
            output.extend_from_slice(&ordered.to_be_bytes());
        }
        StructuralValue::Text(value) => {
            output.push(TEXT_MARKER);
            encode_text_bytes(output, value.as_bytes());
            if terminate_text {
                output.extend_from_slice(&[0, 0]);
            }
        }
        StructuralValue::Bool(value) => {
            output.push(BOOL_MARKER);
            output.push(u8::from(*value));
        }
        StructuralValue::ClosedTag(value) => {
            output.push(CLOSED_TAG_MARKER);
            output.extend_from_slice(value.type_id().as_bytes());
            output.extend_from_slice(&value.ordinal().to_be_bytes());
        }
    }
    if specification.direction == Direction::Desc {
        for byte in &mut output[start..] {
            *byte = !*byte;
        }
    }
}

fn encode_text_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    for byte in bytes {
        if *byte == 0 {
            output.extend_from_slice(&[0, u8::MAX]);
        } else {
            output.push(*byte);
        }
    }
}

pub(crate) fn lexicographic_successor(mut prefix: Vec<u8>) -> Option<Vec<u8>> {
    for position in (0..prefix.len()).rev() {
        if prefix[position] != u8::MAX {
            prefix[position] += 1;
            prefix.truncate(position + 1);
            return Some(prefix);
        }
    }
    None
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyError {
    NonFiniteNumber,
    InvalidSchemaArity {
        actual: usize,
        maximum: usize,
    },
    InvalidKeyArity {
        actual: usize,
        maximum: usize,
    },
    WrongKeyArity {
        expected: usize,
        actual: usize,
    },
    WrongKeyKind {
        component: usize,
        expected: KeyKind,
        actual: KeyKind,
    },
    PrefixHasNoTarget {
        leading: usize,
        components: usize,
    },
    PrefixTargetIsNotText {
        component: usize,
        actual: KeyKind,
    },
}

impl fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteNumber => formatter.write_str("number key must be finite"),
            Self::InvalidSchemaArity { actual, maximum } => write!(
                formatter,
                "key schema has {actual} components; expected 0..={maximum}"
            ),
            Self::InvalidKeyArity { actual, maximum } => write!(
                formatter,
                "structural key has {actual} components; expected 0..={maximum}"
            ),
            Self::WrongKeyArity { expected, actual } => write!(
                formatter,
                "structural key has {actual} components; schema requires {expected}"
            ),
            Self::WrongKeyKind {
                component,
                expected,
                actual,
            } => write!(
                formatter,
                "key component {component} has kind {actual:?}; expected {expected:?}"
            ),
            Self::PrefixHasNoTarget {
                leading,
                components,
            } => write!(
                formatter,
                "prefix has {leading} leading components but schema has only {components} components"
            ),
            Self::PrefixTargetIsNotText { component, actual } => write!(
                formatter,
                "prefix target component {component} is {actual:?}, not Text"
            ),
        }
    }
}

impl Error for KeyError {}
