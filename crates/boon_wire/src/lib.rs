//! Canonical bounded binary encoding for [`boon_data::Value`].
//!
//! This format is for process and network boundaries. In-process Boon graphs
//! should continue to pass `Value` directly without serialization.
//!
//! # Version 1 format
//!
//! Every message starts with `BWV` followed by the one-byte format version.
//! Values use a one-byte tag. Lengths and collection counts use minimal unsigned
//! LEB128; numbers use eight little-endian IEEE-754 bytes. Text is UTF-8. Lists
//! preserve item order, while record, variant-field, and error-field keys must be
//! strictly increasing in Rust `String` order. Variant tags and error codes use
//! the same length-prefixed text representation as record keys.

#![forbid(unsafe_code)]

use boon_data::{FiniteReal, Value};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::str;

mod client_session_frame;
mod session_control;

pub use client_session_frame::{
    CLIENT_SESSION_PROTOCOL_VERSION, ClientSessionDataOperation, ClientSessionFrame,
    ClientSessionFrameError, ClientSessionFrameField, ClientSessionFrameLimits,
    decode_client_session_frame, encode_client_session_frame,
};
pub use session_control::{
    ClientCommit, ClientHello, ClientRevoke, RESUME_LOOKUP_KEY_BYTES, RESUME_TOKEN_BYTES,
    ResumeLookupKey, ResumeLookupKeyError, ResumeToken, ResumeTokenGenerationError,
    SESSION_CONTROL_MAX_FRAME_BYTES, SESSION_CONTROL_PROTOCOL_VERSION, SESSION_ID_BYTES,
    ServerOffer, ServerReady, ServerReject, ServerRevoked, SessionControlField,
    SessionControlFrame, SessionControlFrameError, SessionId, SessionIdGenerationError,
    decode_session_control_frame, encode_session_control_frame,
};

const MAGIC: [u8; 3] = *b"BWV";
pub const FORMAT_VERSION: u8 = 1;
pub const HEADER: [u8; 4] = [MAGIC[0], MAGIC[1], MAGIC[2], FORMAT_VERSION];

/// The protocol-owned same-origin WebSocket path for Client/Session traffic.
/// Both browser and server adapters consume this constant so the route cannot
/// silently drift between the two transport endpoints.
pub const DISTRIBUTED_SESSION_TRANSPORT_PATH: &str = "/_boon/distributed-session";

const TAG_NULL: u8 = 0;
const TAG_FALSE: u8 = 1;
const TAG_TRUE: u8 = 2;
const TAG_NUMBER: u8 = 3;
const TAG_TEXT: u8 = 4;
const TAG_BYTES: u8 = 5;
const TAG_LIST: u8 = 6;
const TAG_RECORD: u8 = 7;
const TAG_VARIANT: u8 = 8;
const TAG_ERROR: u8 = 9;

/// Resource limits applied to both encoding and decoding.
///
/// `max_depth` and `max_nodes` count only recursive `Value` nodes. Record keys,
/// variant tags, and error codes are bounded as text but are not separate nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_total_bytes: usize,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_collection_length: usize,
    pub max_text_bytes: usize,
    pub max_byte_string_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_total_bytes: 16 * 1024 * 1024,
            max_depth: 64,
            max_nodes: 1_000_000,
            max_collection_length: 1_000_000,
            max_text_bytes: 1024 * 1024,
            max_byte_string_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitKind {
    TotalBytes,
    Depth,
    Nodes,
    CollectionLength,
    TextBytes,
    ByteStringBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WireError {
    InvalidMagic,
    UnsupportedVersion(u8),
    UnexpectedEnd,
    TrailingBytes(usize),
    UnknownTag(u8),
    InvalidUtf8,
    NonCanonicalVarint,
    VarintOverflow,
    LengthOverflow,
    NonFiniteNumber,
    NonCanonicalNumber,
    NonCanonicalMapOrder,
    LimitExceeded {
        kind: LimitKind,
        actual: usize,
        maximum: usize,
    },
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => formatter.write_str("invalid Boon wire magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported Boon wire version {version}")
            }
            Self::UnexpectedEnd => formatter.write_str("unexpected end of Boon wire value"),
            Self::TrailingBytes(count) => {
                write!(formatter, "Boon wire value has {count} trailing byte(s)")
            }
            Self::UnknownTag(tag) => write!(formatter, "unknown Boon wire value tag {tag}"),
            Self::InvalidUtf8 => formatter.write_str("Boon wire text is not valid UTF-8"),
            Self::NonCanonicalVarint => {
                formatter.write_str("Boon wire varint is not minimally encoded")
            }
            Self::VarintOverflow => formatter.write_str("Boon wire varint overflows u64"),
            Self::LengthOverflow => {
                formatter.write_str("Boon wire length does not fit this platform")
            }
            Self::NonFiniteNumber => formatter.write_str("Boon wire number is not finite"),
            Self::NonCanonicalNumber => {
                formatter.write_str("Boon wire number is not canonically encoded")
            }
            Self::NonCanonicalMapOrder => {
                formatter.write_str("Boon wire map keys are not strictly increasing")
            }
            Self::LimitExceeded {
                kind,
                actual,
                maximum,
            } => write!(
                formatter,
                "Boon wire {kind:?} limit exceeded: {actual} > {maximum}"
            ),
        }
    }
}

impl Error for WireError {}

/// Encodes one value using the current format and default resource limits.
pub fn encode(value: &Value) -> Result<Vec<u8>, WireError> {
    encode_with_limits(value, Limits::default())
}

/// Encodes one value using the current format and explicit resource limits.
pub fn encode_with_limits(value: &Value, limits: Limits) -> Result<Vec<u8>, WireError> {
    let mut encoder = Encoder {
        bytes: Vec::new(),
        limits,
        nodes: 0,
    };
    encoder.write(&HEADER)?;
    encoder.value(value, 1)?;
    Ok(encoder.bytes)
}

/// Decodes exactly one value using default resource limits.
pub fn decode(bytes: &[u8]) -> Result<Value, WireError> {
    decode_with_limits(bytes, Limits::default())
}

/// Decodes exactly one value using explicit resource limits.
pub fn decode_with_limits(bytes: &[u8], limits: Limits) -> Result<Value, WireError> {
    check_limit(LimitKind::TotalBytes, bytes.len(), limits.max_total_bytes)?;
    if bytes.len() < HEADER.len() {
        return Err(WireError::UnexpectedEnd);
    }
    if bytes[..MAGIC.len()] != MAGIC {
        return Err(WireError::InvalidMagic);
    }
    if bytes[MAGIC.len()] != FORMAT_VERSION {
        return Err(WireError::UnsupportedVersion(bytes[MAGIC.len()]));
    }

    let mut decoder = Decoder {
        bytes,
        position: HEADER.len(),
        limits,
        nodes: 0,
    };
    let value = decoder.value(1)?;
    if decoder.position != bytes.len() {
        return Err(WireError::TrailingBytes(bytes.len() - decoder.position));
    }
    Ok(value)
}

fn check_limit(kind: LimitKind, actual: usize, maximum: usize) -> Result<(), WireError> {
    if actual > maximum {
        return Err(WireError::LimitExceeded {
            kind,
            actual,
            maximum,
        });
    }
    Ok(())
}

struct Encoder {
    bytes: Vec<u8>,
    limits: Limits,
    nodes: usize,
}

impl Encoder {
    fn value(&mut self, value: &Value, depth: usize) -> Result<(), WireError> {
        check_limit(LimitKind::Depth, depth, self.limits.max_depth)?;
        self.nodes = self.nodes.checked_add(1).ok_or(WireError::LengthOverflow)?;
        check_limit(LimitKind::Nodes, self.nodes, self.limits.max_nodes)?;

        match value {
            Value::Null => self.byte(TAG_NULL),
            Value::Bool(false) => self.byte(TAG_FALSE),
            Value::Bool(true) => self.byte(TAG_TRUE),
            Value::Number(number) => {
                self.byte(TAG_NUMBER)?;
                self.write(&number.get().to_bits().to_le_bytes())
            }
            Value::Text(text) => {
                self.byte(TAG_TEXT)?;
                self.text(text)
            }
            Value::Bytes(bytes) => {
                self.byte(TAG_BYTES)?;
                check_limit(
                    LimitKind::ByteStringBytes,
                    bytes.len(),
                    self.limits.max_byte_string_bytes,
                )?;
                self.length(bytes.len())?;
                self.write(bytes)
            }
            Value::List(values) => {
                self.byte(TAG_LIST)?;
                self.collection_len(values.len())?;
                for value in values {
                    self.value(value, depth + 1)?;
                }
                Ok(())
            }
            Value::Record(fields) => {
                self.byte(TAG_RECORD)?;
                self.fields(fields, depth)
            }
            Value::Variant { tag, fields } => {
                self.byte(TAG_VARIANT)?;
                self.text(tag)?;
                self.fields(fields, depth)
            }
            Value::Error { code, fields } => {
                self.byte(TAG_ERROR)?;
                self.text(code)?;
                self.fields(fields, depth)
            }
        }
    }

    fn fields(
        &mut self,
        fields: &BTreeMap<String, Value>,
        parent_depth: usize,
    ) -> Result<(), WireError> {
        self.collection_len(fields.len())?;
        for (key, value) in fields {
            self.text(key)?;
            self.value(value, parent_depth + 1)?;
        }
        Ok(())
    }

    fn text(&mut self, value: &str) -> Result<(), WireError> {
        check_limit(
            LimitKind::TextBytes,
            value.len(),
            self.limits.max_text_bytes,
        )?;
        self.length(value.len())?;
        self.write(value.as_bytes())
    }

    fn collection_len(&mut self, length: usize) -> Result<(), WireError> {
        check_limit(
            LimitKind::CollectionLength,
            length,
            self.limits.max_collection_length,
        )?;
        self.length(length)
    }

    fn length(&mut self, length: usize) -> Result<(), WireError> {
        let value = u64::try_from(length).map_err(|_| WireError::LengthOverflow)?;
        let mut encoded = [0u8; 10];
        let mut remaining = value;
        let mut count = 0;
        loop {
            let mut byte = (remaining & 0x7f) as u8;
            remaining >>= 7;
            if remaining != 0 {
                byte |= 0x80;
            }
            encoded[count] = byte;
            count += 1;
            if remaining == 0 {
                break;
            }
        }
        self.write(&encoded[..count])
    }

    fn byte(&mut self, byte: u8) -> Result<(), WireError> {
        self.write(&[byte])
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        let new_len = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or(WireError::LengthOverflow)?;
        check_limit(LimitKind::TotalBytes, new_len, self.limits.max_total_bytes)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    position: usize,
    limits: Limits,
    nodes: usize,
}

impl Decoder<'_> {
    fn value(&mut self, depth: usize) -> Result<Value, WireError> {
        check_limit(LimitKind::Depth, depth, self.limits.max_depth)?;
        self.nodes = self.nodes.checked_add(1).ok_or(WireError::LengthOverflow)?;
        check_limit(LimitKind::Nodes, self.nodes, self.limits.max_nodes)?;

        match self.byte()? {
            TAG_NULL => Ok(Value::Null),
            TAG_FALSE => Ok(Value::Bool(false)),
            TAG_TRUE => Ok(Value::Bool(true)),
            TAG_NUMBER => self.number().map(Value::Number),
            TAG_TEXT => self.text().map(Value::Text),
            TAG_BYTES => self.byte_string().map(|bytes| Value::Bytes(bytes.into())),
            TAG_LIST => {
                let count = self.collection_len(1)?;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.value(depth + 1)?);
                }
                Ok(Value::List(values))
            }
            TAG_RECORD => self.fields(depth).map(Value::Record),
            TAG_VARIANT => {
                let tag = self.text()?;
                let fields = self.fields(depth)?;
                Ok(Value::Variant { tag, fields })
            }
            TAG_ERROR => {
                let code = self.text()?;
                let fields = self.fields(depth)?;
                Ok(Value::Error { code, fields })
            }
            tag => Err(WireError::UnknownTag(tag)),
        }
    }

    fn fields(&mut self, parent_depth: usize) -> Result<BTreeMap<String, Value>, WireError> {
        let count = self.collection_len(2)?;
        let mut fields = BTreeMap::new();
        for _ in 0..count {
            let key = self.text()?;
            if fields
                .last_key_value()
                .is_some_and(|(previous, _)| previous >= &key)
            {
                return Err(WireError::NonCanonicalMapOrder);
            }
            let value = self.value(parent_depth + 1)?;
            fields.insert(key, value);
        }
        Ok(fields)
    }

    fn number(&mut self) -> Result<FiniteReal, WireError> {
        let encoded: [u8; 8] = self
            .read(8)?
            .try_into()
            .map_err(|_| WireError::UnexpectedEnd)?;
        let value = f64::from_bits(u64::from_le_bytes(encoded));
        if !value.is_finite() {
            return Err(WireError::NonFiniteNumber);
        }
        if value.to_bits() == (-0.0f64).to_bits() {
            return Err(WireError::NonCanonicalNumber);
        }
        FiniteReal::new(value).map_err(|_| WireError::NonFiniteNumber)
    }

    fn text(&mut self) -> Result<String, WireError> {
        let length = self.length()?;
        check_limit(LimitKind::TextBytes, length, self.limits.max_text_bytes)?;
        let bytes = self.read(length)?;
        str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| WireError::InvalidUtf8)
    }

    fn byte_string(&mut self) -> Result<Vec<u8>, WireError> {
        let length = self.length()?;
        check_limit(
            LimitKind::ByteStringBytes,
            length,
            self.limits.max_byte_string_bytes,
        )?;
        Ok(self.read(length)?.to_vec())
    }

    fn collection_len(&mut self, minimum_item_bytes: usize) -> Result<usize, WireError> {
        let count = self.length()?;
        check_limit(
            LimitKind::CollectionLength,
            count,
            self.limits.max_collection_length,
        )?;
        if count > self.remaining() / minimum_item_bytes {
            return Err(WireError::UnexpectedEnd);
        }
        Ok(count)
    }

    fn length(&mut self) -> Result<usize, WireError> {
        let mut value = 0u64;
        for index in 0..10 {
            let byte = self.byte()?;
            let payload = u64::from(byte & 0x7f);
            if index == 9 && payload > 1 {
                return Err(WireError::VarintOverflow);
            }
            value |= payload << (index * 7);
            if byte & 0x80 == 0 {
                if index > 0 && payload == 0 {
                    return Err(WireError::NonCanonicalVarint);
                }
                return usize::try_from(value).map_err(|_| WireError::LengthOverflow);
            }
        }
        Err(WireError::VarintOverflow)
    }

    fn byte(&mut self) -> Result<u8, WireError> {
        let byte = self
            .bytes
            .get(self.position)
            .copied()
            .ok_or(WireError::UnexpectedEnd)?;
        self.position += 1;
        Ok(byte)
    }

    fn read(&mut self, length: usize) -> Result<&[u8], WireError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(WireError::LengthOverflow)?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or(WireError::UnexpectedEnd)?;
        self.position = end;
        Ok(bytes)
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_shapes() -> Value {
        Value::Record(BTreeMap::from([
            ("bytes".into(), Value::Bytes(vec![0, 127, 128, 255].into())),
            (
                "error".into(),
                Value::Error {
                    code: "Unavailable".into(),
                    fields: BTreeMap::from([("retry".into(), Value::Bool(false))]),
                },
            ),
            (
                "list".into(),
                Value::List(vec![
                    Value::Null,
                    Value::Number(FiniteReal::new(-12.5).unwrap()),
                    Value::Text("Boon \u{2603}".into()),
                ]),
            ),
            (
                "variant".into(),
                Value::Variant {
                    tag: "Ready".into(),
                    fields: BTreeMap::from([("enabled".into(), Value::Bool(true))]),
                },
            ),
        ]))
    }

    #[test]
    fn round_trips_every_value_shape() {
        let value = all_shapes();
        let encoded = encode(&value).unwrap();
        assert_eq!(decode(&encoded).unwrap(), value);
    }

    #[test]
    fn encoding_is_deterministic_and_matches_golden_bytes() {
        let first = Value::Record(BTreeMap::from([
            (
                "z".into(),
                Value::Variant {
                    tag: "Ok".into(),
                    fields: BTreeMap::from([(
                        "n".into(),
                        Value::Number(FiniteReal::new(1.5).unwrap()),
                    )]),
                },
            ),
            ("a".into(), Value::Bool(true)),
        ]));
        let mut reordered = BTreeMap::new();
        reordered.insert("a".into(), Value::Bool(true));
        reordered.insert(
            "z".into(),
            Value::Variant {
                tag: "Ok".into(),
                fields: BTreeMap::from([(
                    "n".into(),
                    Value::Number(FiniteReal::new(1.5).unwrap()),
                )]),
            },
        );
        let second = Value::Record(reordered);

        let golden = vec![
            b'B', b'W', b'V', 1, 7, 2, 1, b'a', 2, 1, b'z', 8, 2, b'O', b'k', 1, 1, b'n', 3, 0, 0,
            0, 0, 0, 0, 0xf8, 0x3f,
        ];
        assert_eq!(encode(&first).unwrap(), golden);
        assert_eq!(encode(&second).unwrap(), golden);
        assert_eq!(decode(&golden).unwrap(), first);
    }

    #[test]
    fn rejects_malformed_headers_tags_and_trailing_bytes() {
        assert_eq!(decode(b"bad\x01\x00"), Err(WireError::InvalidMagic));
        assert_eq!(
            decode(b"BWV\x02\x00"),
            Err(WireError::UnsupportedVersion(2))
        );
        assert_eq!(decode(b"BWV\x01\xff"), Err(WireError::UnknownTag(0xff)));
        assert_eq!(decode(b"BWV\x01\x00\x00"), Err(WireError::TrailingBytes(1)));
    }

    #[test]
    fn rejects_invalid_text_varints_and_numbers() {
        assert_eq!(decode(b"BWV\x01\x04\x01\xff"), Err(WireError::InvalidUtf8));
        assert_eq!(
            decode(b"BWV\x01\x04\x80\x00"),
            Err(WireError::NonCanonicalVarint)
        );

        let mut overflowing = b"BWV\x01\x04".to_vec();
        overflowing.extend_from_slice(&[0xff; 9]);
        overflowing.push(0x02);
        assert_eq!(decode(&overflowing), Err(WireError::VarintOverflow));

        let mut infinity = b"BWV\x01\x03".to_vec();
        infinity.extend_from_slice(&f64::INFINITY.to_bits().to_le_bytes());
        assert_eq!(decode(&infinity), Err(WireError::NonFiniteNumber));

        let mut negative_zero = b"BWV\x01\x03".to_vec();
        negative_zero.extend_from_slice(&(-0.0f64).to_bits().to_le_bytes());
        assert_eq!(decode(&negative_zero), Err(WireError::NonCanonicalNumber));
    }

    #[test]
    fn rejects_noncanonical_map_key_order_and_duplicates() {
        let out_of_order = b"BWV\x01\x07\x02\x01b\x00\x01a\x00";
        assert_eq!(decode(out_of_order), Err(WireError::NonCanonicalMapOrder));

        let duplicate = b"BWV\x01\x07\x02\x01a\x00\x01a\x00";
        assert_eq!(decode(duplicate), Err(WireError::NonCanonicalMapOrder));
    }

    #[test]
    fn enforces_every_decode_limit() {
        let value = Value::List(vec![
            Value::Text("abcd".into()),
            Value::Bytes(vec![1, 2, 3, 4].into()),
            Value::List(vec![Value::Null]),
        ]);
        let encoded = encode(&value).unwrap();

        let cases = [
            (
                Limits {
                    max_total_bytes: encoded.len() - 1,
                    ..Limits::default()
                },
                LimitKind::TotalBytes,
            ),
            (
                Limits {
                    max_depth: 2,
                    ..Limits::default()
                },
                LimitKind::Depth,
            ),
            (
                Limits {
                    max_nodes: 4,
                    ..Limits::default()
                },
                LimitKind::Nodes,
            ),
            (
                Limits {
                    max_collection_length: 2,
                    ..Limits::default()
                },
                LimitKind::CollectionLength,
            ),
            (
                Limits {
                    max_text_bytes: 3,
                    ..Limits::default()
                },
                LimitKind::TextBytes,
            ),
            (
                Limits {
                    max_byte_string_bytes: 3,
                    ..Limits::default()
                },
                LimitKind::ByteStringBytes,
            ),
        ];

        for (limits, expected_kind) in cases {
            let Err(WireError::LimitExceeded { kind, .. }) = decode_with_limits(&encoded, limits)
            else {
                panic!("expected {expected_kind:?} limit failure");
            };
            assert_eq!(kind, expected_kind);
        }
    }

    #[test]
    fn encoding_uses_the_same_limits() {
        let limits = Limits {
            max_nodes: 1,
            ..Limits::default()
        };
        assert!(matches!(
            encode_with_limits(&Value::List(vec![Value::Null]), limits),
            Err(WireError::LimitExceeded {
                kind: LimitKind::Nodes,
                ..
            })
        ));
    }
}
