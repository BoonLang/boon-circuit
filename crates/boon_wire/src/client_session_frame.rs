use crate::session_control::SessionId;
use crate::{Limits, Value, decode_with_limits, encode_with_limits};
use minicbor::data::Type;
use minicbor::{Decoder, Encoder};
use std::fmt;

pub const CLIENT_SESSION_PROTOCOL_VERSION: u16 = 2;

const DATA_KIND: u8 = 0;
const ACK_KIND: u8 = 1;
const RESYNC_KIND: u8 = 2;

const DATA_FIELDS: u64 = 12;
const ACK_FIELDS: u64 = 5;
const RESYNC_FIELDS: u64 = 5;

/// One raw data-plane frame crossing the Client/Session boundary.
///
/// Session identity and transport generation remain explicit positional wire
/// fields while staying host-private. This type intentionally has no `Debug`,
/// `Display`, or serialization implementation so payloads and hidden context
/// cannot be logged wholesale.
#[derive(Clone, Eq, PartialEq)]
pub enum ClientSessionFrame {
    Data {
        graph_hash: [u8; 32],
        graph_revision: u64,
        schema_hash: [u8; 32],
        session_id: SessionId,
        generation: u64,
        operation_sequence: u64,
        ack_through: u64,
        edge_id: [u8; 32],
        semantic_revision: u64,
        payload: Value,
    },
    Ack {
        session_id: SessionId,
        generation: u64,
        ack_through: u64,
    },
    Resync {
        session_id: SessionId,
        generation: u64,
        expected_next: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientSessionFrameLimits {
    pub max_frame_bytes: usize,
    pub value: Limits,
}

impl Default for ClientSessionFrameLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 256 * 1024,
            value: Limits {
                max_total_bytes: 240 * 1024,
                max_depth: 32,
                max_nodes: 16 * 1024,
                max_collection_length: 4 * 1024,
                max_text_bytes: 64 * 1024,
                max_byte_string_bytes: 128 * 1024,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientSessionFrameField {
    GraphHash,
    SchemaHash,
    SessionId,
    EdgeId,
}

impl ClientSessionFrameField {
    fn label(self) -> &'static str {
        match self {
            Self::GraphHash => "graph hash",
            Self::SchemaHash => "schema hash",
            Self::SessionId => "Session ID",
            Self::EdgeId => "edge ID",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientSessionFrameError {
    FrameTooLarge {
        actual: usize,
        maximum: usize,
    },
    UnsupportedProtocolVersion(u16),
    IndefiniteFrame,
    WrongFieldCount {
        actual: u64,
        expected: u64,
    },
    UnknownMessageKind(u8),
    InvalidFieldWidth {
        field: ClientSessionFrameField,
        actual: usize,
    },
    TrailingBytes(usize),
    NonCanonicalFrame,
    CborEncode,
    CborDecode,
    InvalidPayload,
}

impl fmt::Display for ClientSessionFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "client/session frame is {actual} bytes, limit is {maximum}"
                )
            }
            Self::UnsupportedProtocolVersion(version) => {
                write!(
                    formatter,
                    "unsupported client/session protocol version {version}"
                )
            }
            Self::IndefiniteFrame => formatter
                .write_str("client/session frame must use definite CBOR arrays and byte strings"),
            Self::WrongFieldCount { actual, expected } => {
                write!(
                    formatter,
                    "client/session frame has {actual} fields, expected {expected}"
                )
            }
            Self::UnknownMessageKind(kind) => {
                write!(formatter, "unknown client/session message kind {kind}")
            }
            Self::InvalidFieldWidth { field, actual } => {
                write!(
                    formatter,
                    "client/session {} has {actual} bytes, expected 32",
                    field.label()
                )
            }
            Self::TrailingBytes(count) => {
                write!(formatter, "client/session frame has {count} trailing bytes")
            }
            Self::NonCanonicalFrame => {
                formatter.write_str("client/session frame is not canonical positional CBOR")
            }
            Self::CborEncode => formatter.write_str("client/session CBOR encode failed"),
            Self::CborDecode => formatter.write_str("client/session CBOR decode failed"),
            Self::InvalidPayload => formatter.write_str("client/session payload is invalid"),
        }
    }
}

impl std::error::Error for ClientSessionFrameError {}

pub fn encode_client_session_frame(
    frame: &ClientSessionFrame,
    limits: ClientSessionFrameLimits,
) -> Result<Vec<u8>, ClientSessionFrameError> {
    let mut bytes = Vec::with_capacity(128);
    let mut encoder = Encoder::new(&mut bytes);

    match frame {
        ClientSessionFrame::Data {
            graph_hash,
            graph_revision,
            schema_hash,
            session_id,
            generation,
            operation_sequence,
            ack_through,
            edge_id,
            semantic_revision,
            payload,
        } => {
            let payload = encode_with_limits(payload, limits.value)
                .map_err(|_| ClientSessionFrameError::InvalidPayload)?;
            encode_header(&mut encoder, DATA_FIELDS, DATA_KIND)?;
            encoder
                .bytes(graph_hash)
                .and_then(|encoder| encoder.u64(*graph_revision))
                .and_then(|encoder| encoder.bytes(schema_hash))
                .and_then(|encoder| encoder.bytes(session_id.as_bytes()))
                .and_then(|encoder| encoder.u64(*generation))
                .and_then(|encoder| encoder.u64(*operation_sequence))
                .and_then(|encoder| encoder.u64(*ack_through))
                .and_then(|encoder| encoder.bytes(edge_id))
                .and_then(|encoder| encoder.u64(*semantic_revision))
                .and_then(|encoder| encoder.bytes(&payload))
                .map_err(|_| ClientSessionFrameError::CborEncode)?;
        }
        ClientSessionFrame::Ack {
            session_id,
            generation,
            ack_through,
        } => {
            encode_header(&mut encoder, ACK_FIELDS, ACK_KIND)?;
            encoder
                .bytes(session_id.as_bytes())
                .and_then(|encoder| encoder.u64(*generation))
                .and_then(|encoder| encoder.u64(*ack_through))
                .map_err(|_| ClientSessionFrameError::CborEncode)?;
        }
        ClientSessionFrame::Resync {
            session_id,
            generation,
            expected_next,
        } => {
            encode_header(&mut encoder, RESYNC_FIELDS, RESYNC_KIND)?;
            encoder
                .bytes(session_id.as_bytes())
                .and_then(|encoder| encoder.u64(*generation))
                .and_then(|encoder| encoder.u64(*expected_next))
                .map_err(|_| ClientSessionFrameError::CborEncode)?;
        }
    }

    check_frame_size(bytes.len(), limits.max_frame_bytes)?;
    Ok(bytes)
}

pub fn decode_client_session_frame(
    bytes: &[u8],
    limits: ClientSessionFrameLimits,
) -> Result<ClientSessionFrame, ClientSessionFrameError> {
    check_frame_size(bytes.len(), limits.max_frame_bytes)?;
    let mut decoder = Decoder::new(bytes);
    let field_count = decoder
        .array()
        .map_err(|_| ClientSessionFrameError::CborDecode)?
        .ok_or(ClientSessionFrameError::IndefiniteFrame)?;
    if field_count < 2 {
        return Err(ClientSessionFrameError::WrongFieldCount {
            actual: field_count,
            expected: 2,
        });
    }

    let version = decoder
        .u16()
        .map_err(|_| ClientSessionFrameError::CborDecode)?;
    if version != CLIENT_SESSION_PROTOCOL_VERSION {
        return Err(ClientSessionFrameError::UnsupportedProtocolVersion(version));
    }
    let kind = decoder
        .u8()
        .map_err(|_| ClientSessionFrameError::CborDecode)?;
    let expected_fields = fields_for_kind(kind)?;
    if field_count != expected_fields {
        return Err(ClientSessionFrameError::WrongFieldCount {
            actual: field_count,
            expected: expected_fields,
        });
    }

    let frame = match kind {
        DATA_KIND => {
            let graph_hash = decode_fixed_bytes(&mut decoder, ClientSessionFrameField::GraphHash)?;
            let graph_revision = decode_u64(&mut decoder)?;
            let schema_hash =
                decode_fixed_bytes(&mut decoder, ClientSessionFrameField::SchemaHash)?;
            let session_id = decode_session_id(&mut decoder)?;
            let generation = decode_u64(&mut decoder)?;
            let operation_sequence = decode_u64(&mut decoder)?;
            let ack_through = decode_u64(&mut decoder)?;
            let edge_id = decode_fixed_bytes(&mut decoder, ClientSessionFrameField::EdgeId)?;
            let semantic_revision = decode_u64(&mut decoder)?;
            let payload_bytes = decode_definite_bytes(&mut decoder)?;
            let payload = decode_with_limits(payload_bytes, limits.value)
                .map_err(|_| ClientSessionFrameError::InvalidPayload)?;
            ClientSessionFrame::Data {
                graph_hash,
                graph_revision,
                schema_hash,
                session_id,
                generation,
                operation_sequence,
                ack_through,
                edge_id,
                semantic_revision,
                payload,
            }
        }
        ACK_KIND => ClientSessionFrame::Ack {
            session_id: decode_session_id(&mut decoder)?,
            generation: decode_u64(&mut decoder)?,
            ack_through: decode_u64(&mut decoder)?,
        },
        RESYNC_KIND => ClientSessionFrame::Resync {
            session_id: decode_session_id(&mut decoder)?,
            generation: decode_u64(&mut decoder)?,
            expected_next: decode_u64(&mut decoder)?,
        },
        _ => unreachable!("message kind was validated"),
    };

    let position = decoder.position();
    if position != bytes.len() {
        return Err(ClientSessionFrameError::TrailingBytes(
            bytes.len() - position,
        ));
    }
    if encode_client_session_frame(&frame, limits)? != bytes {
        return Err(ClientSessionFrameError::NonCanonicalFrame);
    }
    Ok(frame)
}

fn encode_header(
    encoder: &mut Encoder<&mut Vec<u8>>,
    fields: u64,
    kind: u8,
) -> Result<(), ClientSessionFrameError> {
    encoder
        .array(fields)
        .and_then(|encoder| encoder.u16(CLIENT_SESSION_PROTOCOL_VERSION))
        .and_then(|encoder| encoder.u8(kind))
        .map_err(|_| ClientSessionFrameError::CborEncode)?;
    Ok(())
}

fn fields_for_kind(kind: u8) -> Result<u64, ClientSessionFrameError> {
    match kind {
        DATA_KIND => Ok(DATA_FIELDS),
        ACK_KIND => Ok(ACK_FIELDS),
        RESYNC_KIND => Ok(RESYNC_FIELDS),
        _ => Err(ClientSessionFrameError::UnknownMessageKind(kind)),
    }
}

fn decode_u64(decoder: &mut Decoder<'_>) -> Result<u64, ClientSessionFrameError> {
    decoder
        .u64()
        .map_err(|_| ClientSessionFrameError::CborDecode)
}

fn decode_session_id(decoder: &mut Decoder<'_>) -> Result<SessionId, ClientSessionFrameError> {
    decode_fixed_bytes(decoder, ClientSessionFrameField::SessionId).map(SessionId::from_bytes)
}

fn decode_fixed_bytes(
    decoder: &mut Decoder<'_>,
    field: ClientSessionFrameField,
) -> Result<[u8; 32], ClientSessionFrameError> {
    let bytes = decode_definite_bytes(decoder)?;
    bytes
        .try_into()
        .map_err(|_| ClientSessionFrameError::InvalidFieldWidth {
            field,
            actual: bytes.len(),
        })
}

fn decode_definite_bytes<'bytes>(
    decoder: &mut Decoder<'bytes>,
) -> Result<&'bytes [u8], ClientSessionFrameError> {
    if decoder
        .datatype()
        .map_err(|_| ClientSessionFrameError::CborDecode)?
        == Type::BytesIndef
    {
        return Err(ClientSessionFrameError::IndefiniteFrame);
    }
    decoder
        .bytes()
        .map_err(|_| ClientSessionFrameError::CborDecode)
}

fn check_frame_size(actual: usize, maximum: usize) -> Result<(), ClientSessionFrameError> {
    if actual > maximum {
        return Err(ClientSessionFrameError::FrameTooLarge { actual, maximum });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode;
    use crate::session_control::SESSION_ID_BYTES;
    use boon_data::Bytes;
    use static_assertions::assert_not_impl_any;
    use std::collections::BTreeMap;
    use std::fmt::{Debug, Display};

    assert_not_impl_any!(ClientSessionFrame: Debug, Display, serde::Serialize);

    fn data_frame() -> ClientSessionFrame {
        ClientSessionFrame::Data {
            graph_hash: [1; 32],
            graph_revision: 7,
            schema_hash: [2; 32],
            session_id: SessionId::from_bytes([3; SESSION_ID_BYTES]),
            generation: 11,
            operation_sequence: 13,
            ack_through: 9,
            edge_id: [4; 32],
            semantic_revision: 17,
            payload: Value::Record(BTreeMap::from([
                (
                    "bytes".to_owned(),
                    Value::Bytes(Bytes::from_static(b"chunk")),
                ),
                ("text".to_owned(), Value::Text("accepted".to_owned())),
            ])),
        }
    }

    #[test]
    fn every_positional_variant_round_trips_canonically() {
        let limits = ClientSessionFrameLimits::default();
        let frames = [
            data_frame(),
            ClientSessionFrame::Ack {
                session_id: SessionId::from_bytes([5; SESSION_ID_BYTES]),
                generation: 19,
                ack_through: 23,
            },
            ClientSessionFrame::Resync {
                session_id: SessionId::from_bytes([6; SESSION_ID_BYTES]),
                generation: 29,
                expected_next: 31,
            },
        ];

        for expected in frames {
            let encoded = encode_client_session_frame(&expected, limits).unwrap();
            let decoded = decode_client_session_frame(&encoded, limits).unwrap();
            assert!(decoded == expected);
        }
    }

    #[test]
    fn every_positional_variant_matches_golden_bytes() {
        let limits = ClientSessionFrameLimits::default();
        let data = ClientSessionFrame::Data {
            graph_hash: [1; 32],
            graph_revision: 7,
            schema_hash: [2; 32],
            session_id: SessionId::from_bytes([3; SESSION_ID_BYTES]),
            generation: 11,
            operation_sequence: 13,
            ack_through: 9,
            edge_id: [4; 32],
            semantic_revision: 17,
            payload: Value::Null,
        };
        let mut data_golden = vec![0x8c, 0x02, DATA_KIND, 0x58, 0x20];
        data_golden.extend_from_slice(&[1; 32]);
        data_golden.push(0x07);
        data_golden.extend_from_slice(&[0x58, 0x20]);
        data_golden.extend_from_slice(&[2; 32]);
        data_golden.extend_from_slice(&[0x58, 0x20]);
        data_golden.extend_from_slice(&[3; SESSION_ID_BYTES]);
        data_golden.extend_from_slice(&[0x0b, 0x0d, 0x09]);
        data_golden.extend_from_slice(&[0x58, 0x20]);
        data_golden.extend_from_slice(&[4; 32]);
        data_golden.push(0x11);
        data_golden.extend_from_slice(&[0x45, b'B', b'W', b'V', 0x01, 0x00]);
        assert_eq!(
            encode_client_session_frame(&data, limits).unwrap(),
            data_golden
        );

        let ack = ClientSessionFrame::Ack {
            session_id: SessionId::from_bytes([5; SESSION_ID_BYTES]),
            generation: 19,
            ack_through: 23,
        };
        let mut ack_golden = vec![0x85, 0x02, ACK_KIND, 0x58, 0x20];
        ack_golden.extend_from_slice(&[5; SESSION_ID_BYTES]);
        ack_golden.extend_from_slice(&[0x13, 0x17]);
        assert_eq!(
            encode_client_session_frame(&ack, limits).unwrap(),
            ack_golden
        );

        let resync = ClientSessionFrame::Resync {
            session_id: SessionId::from_bytes([6; SESSION_ID_BYTES]),
            generation: 29,
            expected_next: 31,
        };
        let mut resync_golden = vec![0x85, 0x02, RESYNC_KIND, 0x58, 0x20];
        resync_golden.extend_from_slice(&[6; SESSION_ID_BYTES]);
        resync_golden.extend_from_slice(&[0x18, 0x1d, 0x18, 0x1f]);
        assert_eq!(
            encode_client_session_frame(&resync, limits).unwrap(),
            resync_golden
        );
    }

    #[test]
    fn decoder_rejects_v1_without_compatibility_decoding() {
        let mut old_v1 = vec![0x88, 0x01, 0x58, 0x20];
        old_v1.extend_from_slice(&[1; 32]);
        old_v1.extend_from_slice(&[0x58, 0x20]);
        old_v1.extend_from_slice(&[2; 32]);
        old_v1.extend_from_slice(&[0x58, 0x20]);
        old_v1.extend_from_slice(&[3; 32]);
        old_v1.extend_from_slice(&[0x07, 0x0b, 0x0d]);
        old_v1.extend_from_slice(&[0x45, b'B', b'W', b'V', 0x01, 0x00]);

        assert!(matches!(
            decode_client_session_frame(&old_v1, ClientSessionFrameLimits::default()),
            Err(ClientSessionFrameError::UnsupportedProtocolVersion(1))
        ));
    }

    #[test]
    fn decoder_rejects_oversize_indefinite_trailing_and_noncanonical_frames() {
        let limits = ClientSessionFrameLimits::default();
        let encoded = encode_client_session_frame(
            &ClientSessionFrame::Ack {
                session_id: SessionId::from_bytes([7; SESSION_ID_BYTES]),
                generation: 1,
                ack_through: 0,
            },
            limits,
        )
        .unwrap();

        let tiny = ClientSessionFrameLimits {
            max_frame_bytes: encoded.len() - 1,
            ..limits
        };
        assert!(matches!(
            decode_client_session_frame(&encoded, tiny),
            Err(ClientSessionFrameError::FrameTooLarge { .. })
        ));
        assert!(matches!(
            decode_client_session_frame(&[0x9f, 0x02, ACK_KIND, 0xff], limits),
            Err(ClientSessionFrameError::IndefiniteFrame)
        ));

        let mut indefinite_id = vec![0x85, 0x02, ACK_KIND, 0x5f, 0x58, 0x20];
        indefinite_id.extend_from_slice(&[7; SESSION_ID_BYTES]);
        indefinite_id.extend_from_slice(&[0xff, 0x01, 0x00]);
        assert!(matches!(
            decode_client_session_frame(&indefinite_id, limits),
            Err(ClientSessionFrameError::IndefiniteFrame)
        ));

        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(matches!(
            decode_client_session_frame(&trailing, limits),
            Err(ClientSessionFrameError::TrailingBytes(1))
        ));

        let mut noncanonical_array = vec![0x98, ACK_FIELDS as u8];
        noncanonical_array.extend_from_slice(&encoded[1..]);
        assert!(matches!(
            decode_client_session_frame(&noncanonical_array, limits),
            Err(ClientSessionFrameError::NonCanonicalFrame)
        ));

        let mut noncanonical_version = vec![0x85, 0x18, 0x02];
        noncanonical_version.extend_from_slice(&encoded[2..]);
        assert!(matches!(
            decode_client_session_frame(&noncanonical_version, limits),
            Err(ClientSessionFrameError::NonCanonicalFrame)
        ));
    }

    #[test]
    fn decoder_rejects_unknown_kind_wrong_count_and_field_widths() {
        let limits = ClientSessionFrameLimits::default();
        assert!(matches!(
            decode_client_session_frame(&[0x82, 0x02, 0x03], limits),
            Err(ClientSessionFrameError::UnknownMessageKind(3))
        ));
        assert!(matches!(
            decode_client_session_frame(&[0x84, 0x02, ACK_KIND, 0, 0], limits),
            Err(ClientSessionFrameError::WrongFieldCount {
                actual: 4,
                expected: ACK_FIELDS
            })
        ));

        let mut narrow_session = Vec::new();
        Encoder::new(&mut narrow_session)
            .array(ACK_FIELDS)
            .and_then(|encoder| encoder.u16(CLIENT_SESSION_PROTOCOL_VERSION))
            .and_then(|encoder| encoder.u8(ACK_KIND))
            .and_then(|encoder| encoder.bytes(&[1; SESSION_ID_BYTES - 1]))
            .and_then(|encoder| encoder.u64(1))
            .and_then(|encoder| encoder.u64(0))
            .unwrap();
        assert!(matches!(
            decode_client_session_frame(&narrow_session, limits),
            Err(ClientSessionFrameError::InvalidFieldWidth {
                field: ClientSessionFrameField::SessionId,
                actual: 31
            })
        ));

        let mut narrow_graph = Vec::new();
        Encoder::new(&mut narrow_graph)
            .array(DATA_FIELDS)
            .and_then(|encoder| encoder.u16(CLIENT_SESSION_PROTOCOL_VERSION))
            .and_then(|encoder| encoder.u8(DATA_KIND))
            .and_then(|encoder| encoder.bytes(&[1; 31]))
            .and_then(|encoder| encoder.u64(1))
            .and_then(|encoder| encoder.bytes(&[2; 32]))
            .and_then(|encoder| encoder.bytes(&[3; SESSION_ID_BYTES]))
            .and_then(|encoder| encoder.u64(1))
            .and_then(|encoder| encoder.u64(1))
            .and_then(|encoder| encoder.u64(0))
            .and_then(|encoder| encoder.bytes(&[4; 32]))
            .and_then(|encoder| encoder.u64(1))
            .and_then(|encoder| encoder.bytes(&encode(&Value::Null).unwrap()))
            .unwrap();
        assert!(matches!(
            decode_client_session_frame(&narrow_graph, limits),
            Err(ClientSessionFrameError::InvalidFieldWidth {
                field: ClientSessionFrameField::GraphHash,
                actual: 31
            })
        ));
    }

    #[test]
    fn payload_errors_do_not_render_payload_cursors_or_identifiers() {
        let limits = ClientSessionFrameLimits::default();
        let graph_hash = [0x91; 32];
        let schema_hash = [0x92; 32];
        let session_id = [0x93; SESSION_ID_BYTES];
        let edge_id = [0x94; 32];
        let generation = 0xf1e2_d3c4_b5a6_9788;
        let operation_sequence = 0xe1d2_c3b4_a596_8778;
        let ack_through = 0xd1c2_b3a4_9586_7768;
        let secret_payload = b"payload-must-not-appear";
        let mut bytes = Vec::new();
        Encoder::new(&mut bytes)
            .array(DATA_FIELDS)
            .and_then(|encoder| encoder.u16(CLIENT_SESSION_PROTOCOL_VERSION))
            .and_then(|encoder| encoder.u8(DATA_KIND))
            .and_then(|encoder| encoder.bytes(&graph_hash))
            .and_then(|encoder| encoder.u64(7))
            .and_then(|encoder| encoder.bytes(&schema_hash))
            .and_then(|encoder| encoder.bytes(&session_id))
            .and_then(|encoder| encoder.u64(generation))
            .and_then(|encoder| encoder.u64(operation_sequence))
            .and_then(|encoder| encoder.u64(ack_through))
            .and_then(|encoder| encoder.bytes(&edge_id))
            .and_then(|encoder| encoder.u64(17))
            .and_then(|encoder| encoder.bytes(secret_payload))
            .unwrap();
        let error = match decode_client_session_frame(&bytes, limits) {
            Err(error) => error,
            Ok(_) => panic!("invalid payload was accepted"),
        };
        assert!(matches!(error, ClientSessionFrameError::InvalidPayload));

        let display = error.to_string();
        let debug = format!("{error:?}");
        let forbidden = [
            String::from_utf8_lossy(secret_payload).into_owned(),
            generation.to_string(),
            operation_sequence.to_string(),
            ack_through.to_string(),
            format!("{graph_hash:?}"),
            format!("{schema_hash:?}"),
            format!("{session_id:?}"),
            format!("{edge_id:?}"),
        ];
        for forbidden in forbidden {
            assert!(!display.contains(&forbidden));
            assert!(!debug.contains(&forbidden));
        }
    }
}
