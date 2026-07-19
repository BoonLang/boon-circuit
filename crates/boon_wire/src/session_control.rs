use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use minicbor::data::Type;
use minicbor::{Decoder, Encoder};
use std::error::Error;
use std::fmt;
use std::str;
use zeroize::{ZeroizeOnDrop, Zeroizing};

pub const SESSION_CONTROL_PROTOCOL_VERSION: u16 = 3;
pub const SESSION_CONTROL_MAX_FRAME_BYTES: usize = 128;
pub const SESSION_ID_BYTES: usize = 32;
pub const RESUME_TOKEN_BYTES: usize = 32;
pub const RESUME_LOOKUP_KEY_BYTES: usize = 43;

const CLIENT_HELLO_KIND: u8 = 0;
const SERVER_OFFER_KIND: u8 = 1;
const CLIENT_COMMIT_KIND: u8 = 2;
const SERVER_READY_KIND: u8 = 3;
const CLIENT_REVOKE_KIND: u8 = 4;
const SERVER_REJECT_KIND: u8 = 5;
const SERVER_REVOKED_KIND: u8 = 6;

const CLIENT_HELLO_FIELDS: u64 = 7;
const SERVER_OFFER_FIELDS: u64 = 6;
const CLIENT_COMMIT_FIELDS: u64 = 5;
const SERVER_READY_FIELDS: u64 = 5;
const CLIENT_REVOKE_FIELDS: u64 = 2;
const SERVER_REJECT_FIELDS: u64 = 2;
const SERVER_REVOKED_FIELDS: u64 = 2;

/// A host-private identifier for one durable Session.
///
/// The bytes are intentionally opaque to application code and diagnostics.
/// This type has no `Debug`, `Display`, or serialization implementation.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionId([u8; SESSION_ID_BYTES]);

impl SessionId {
    pub fn generate() -> Result<Self, SessionIdGenerationError> {
        let mut bytes = [0; SESSION_ID_BYTES];
        getrandom::fill(&mut bytes).map_err(|_| SessionIdGenerationError::RandomUnavailable)?;
        Ok(Self::from_bytes(bytes))
    }

    pub const fn from_bytes(bytes: [u8; SESSION_ID_BYTES]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; SESSION_ID_BYTES] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; SESSION_ID_BYTES] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIdGenerationError {
    RandomUnavailable,
}

impl fmt::Display for SessionIdGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operating-system randomness is unavailable")
    }
}

impl Error for SessionIdGenerationError {}

/// A move-only bearer secret used to resume one Session.
///
/// The bytes are private and zeroized on drop. This type intentionally has no
/// formatting, cloning, copying, or serialization implementation.
pub struct ResumeToken {
    bytes: Zeroizing<[u8; RESUME_TOKEN_BYTES]>,
}

impl ResumeToken {
    pub fn generate() -> Result<Self, ResumeTokenGenerationError> {
        let mut token = Self::from_bytes([0; RESUME_TOKEN_BYTES]);
        getrandom::fill(token.bytes.as_mut())
            .map_err(|_| ResumeTokenGenerationError::RandomUnavailable)?;
        Ok(token)
    }

    pub fn from_bytes(bytes: [u8; RESUME_TOKEN_BYTES]) -> Self {
        Self {
            bytes: Zeroizing::new(bytes),
        }
    }

    pub fn as_bytes(&self) -> &[u8; RESUME_TOKEN_BYTES] {
        &self.bytes
    }

    pub fn into_bytes(self) -> [u8; RESUME_TOKEN_BYTES] {
        *self.bytes
    }

    pub fn to_lookup_key(&self) -> ResumeLookupKey {
        let mut encoded = Zeroizing::new([0; RESUME_LOOKUP_KEY_BYTES]);
        let written = URL_SAFE_NO_PAD
            .encode_slice(self.as_bytes(), encoded.as_mut())
            .expect("43 bytes always hold an unpadded base64url token");
        assert_eq!(written, RESUME_LOOKUP_KEY_BYTES);
        ResumeLookupKey { bytes: encoded }
    }
}

impl ZeroizeOnDrop for ResumeToken {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeTokenGenerationError {
    RandomUnavailable,
}

impl fmt::Display for ResumeTokenGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operating-system randomness is unavailable")
    }
}

impl Error for ResumeTokenGenerationError {}

/// A canonical unpadded base64url storage key for a [`ResumeToken`].
///
/// The storage bytes are private and zeroized on drop. Like `ResumeToken`, this
/// type deliberately has no formatting, cloning, copying, or serialization
/// implementation.
pub struct ResumeLookupKey {
    bytes: Zeroizing<[u8; RESUME_LOOKUP_KEY_BYTES]>,
}

impl ResumeLookupKey {
    pub fn from_storage_str(storage: &str) -> Result<Self, ResumeLookupKeyError> {
        if storage.len() != RESUME_LOOKUP_KEY_BYTES {
            return Err(ResumeLookupKeyError::InvalidEncoding);
        }
        let token = decode_lookup_key(storage.as_bytes())?;
        let canonical = token.to_lookup_key();
        if canonical.as_storage_bytes().as_slice() != storage.as_bytes() {
            return Err(ResumeLookupKeyError::InvalidEncoding);
        }
        Ok(canonical)
    }

    pub fn as_storage_bytes(&self) -> &[u8; RESUME_LOOKUP_KEY_BYTES] {
        &self.bytes
    }

    pub fn as_storage_str(&self) -> &str {
        str::from_utf8(self.as_storage_bytes())
            .expect("a validated resume lookup key is ASCII base64url")
    }

    pub fn into_storage_bytes(self) -> [u8; RESUME_LOOKUP_KEY_BYTES] {
        *self.bytes
    }

    pub fn into_resume_token(self) -> ResumeToken {
        decode_lookup_key(self.as_storage_bytes())
            .expect("a ResumeLookupKey always contains canonical base64url")
    }

    pub fn to_resume_token(&self) -> ResumeToken {
        decode_lookup_key(self.as_storage_bytes())
            .expect("a ResumeLookupKey always contains canonical base64url")
    }
}

impl ZeroizeOnDrop for ResumeLookupKey {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeLookupKeyError {
    InvalidEncoding,
}

impl fmt::Display for ResumeLookupKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("resume lookup key is not canonical base64url")
    }
}

impl Error for ResumeLookupKeyError {}

fn decode_lookup_key(storage: &[u8]) -> Result<ResumeToken, ResumeLookupKeyError> {
    if storage.len() != RESUME_LOOKUP_KEY_BYTES {
        return Err(ResumeLookupKeyError::InvalidEncoding);
    }
    let mut decoded = ResumeToken::from_bytes([0; RESUME_TOKEN_BYTES]);
    let written = URL_SAFE_NO_PAD
        .decode_slice(storage, decoded.bytes.as_mut())
        .map_err(|_| ResumeLookupKeyError::InvalidEncoding)?;
    if written != RESUME_TOKEN_BYTES {
        return Err(ResumeLookupKeyError::InvalidEncoding);
    }
    Ok(decoded)
}

/// Opens or resumes a Session for one exact compiled graph revision.
pub struct ClientHello {
    graph_id: [u8; 32],
    graph_revision: u64,
    schema_hash: [u8; 32],
    resume_token: Option<ResumeToken>,
    applied_server_through: u64,
}

impl ClientHello {
    pub fn new(
        graph_id: [u8; 32],
        graph_revision: u64,
        schema_hash: [u8; 32],
        resume_token: Option<ResumeToken>,
        applied_server_through: u64,
    ) -> Self {
        Self {
            graph_id,
            graph_revision,
            schema_hash,
            resume_token,
            applied_server_through,
        }
    }

    pub fn graph_id(&self) -> &[u8; 32] {
        &self.graph_id
    }

    pub fn graph_revision(&self) -> u64 {
        self.graph_revision
    }

    pub fn schema_hash(&self) -> &[u8; 32] {
        &self.schema_hash
    }

    pub fn resume_token(&self) -> Option<&ResumeToken> {
        self.resume_token.as_ref()
    }

    pub fn applied_server_through(&self) -> u64 {
        self.applied_server_through
    }

    pub fn into_parts(self) -> ([u8; 32], u64, [u8; 32], Option<ResumeToken>, u64) {
        (
            self.graph_id,
            self.graph_revision,
            self.schema_hash,
            self.resume_token,
            self.applied_server_through,
        )
    }
}

/// Offers the freshly rotated resume token and next transport generation.
pub struct ServerOffer {
    resume_token: ResumeToken,
    session_id: SessionId,
    next_generation: u64,
    applied_client_through: u64,
}

impl ServerOffer {
    pub fn new(
        resume_token: ResumeToken,
        session_id: SessionId,
        next_generation: u64,
        applied_client_through: u64,
    ) -> Self {
        Self {
            resume_token,
            session_id,
            next_generation,
            applied_client_through,
        }
    }

    pub fn resume_token(&self) -> &ResumeToken {
        &self.resume_token
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn next_generation(&self) -> u64 {
        self.next_generation
    }

    pub fn applied_client_through(&self) -> u64 {
        self.applied_client_through
    }

    pub fn into_parts(self) -> (ResumeToken, SessionId, u64, u64) {
        (
            self.resume_token,
            self.session_id,
            self.next_generation,
            self.applied_client_through,
        )
    }
}

/// Acknowledges the pending server offer on the current connection.
pub struct ClientCommit {
    session_id: SessionId,
    generation: u64,
    applied_server_through: u64,
}

impl ClientCommit {
    pub const fn new(session_id: SessionId, generation: u64, applied_server_through: u64) -> Self {
        Self {
            session_id,
            generation,
            applied_server_through,
        }
    }

    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn applied_server_through(&self) -> u64 {
        self.applied_server_through
    }
}

/// Confirms the committed Session identity, generation, and client cursor.
pub struct ServerReady {
    session_id: SessionId,
    generation: u64,
    applied_client_through: u64,
}

impl ServerReady {
    pub const fn new(session_id: SessionId, generation: u64, applied_client_through: u64) -> Self {
        Self {
            session_id,
            generation,
            applied_client_through,
        }
    }

    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn applied_client_through(&self) -> u64 {
        self.applied_client_through
    }
}

/// Revokes resume authority associated with the current connection.
pub struct ClientRevoke {
    _private: (),
}

impl ClientRevoke {
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ClientRevoke {
    fn default() -> Self {
        Self::new()
    }
}

/// Confirms that resume authority for the current connection is revoked.
pub struct ServerRevoked {
    _private: (),
}

impl ServerRevoked {
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ServerRevoked {
    fn default() -> Self {
        Self::new()
    }
}

/// Rejects the pending handshake without returning sensitive diagnostics.
pub struct ServerReject {
    _private: (),
}

impl ServerReject {
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ServerReject {
    fn default() -> Self {
        Self::new()
    }
}

/// One raw Client/Session handshake or control frame.
///
/// The envelope intentionally has no `Debug` or `Display` implementation.
pub enum SessionControlFrame {
    ClientHello(ClientHello),
    ServerOffer(ServerOffer),
    ClientCommit(ClientCommit),
    ServerReady(ServerReady),
    ClientRevoke(ClientRevoke),
    ServerReject(ServerReject),
    ServerRevoked(ServerRevoked),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionControlField {
    GraphId,
    SchemaHash,
    ResumeToken,
    SessionId,
}

impl SessionControlField {
    fn label(self) -> &'static str {
        match self {
            Self::GraphId => "graph ID",
            Self::SchemaHash => "schema hash",
            Self::ResumeToken => "resume token",
            Self::SessionId => "Session ID",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionControlFrameError {
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
        field: SessionControlField,
        actual: usize,
    },
    TrailingBytes(usize),
    NonCanonicalFrame,
    MalformedCbor,
    CborEncode,
}

impl fmt::Display for SessionControlFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "session control frame is {actual} bytes, limit is {maximum}"
                )
            }
            Self::UnsupportedProtocolVersion(version) => {
                write!(
                    formatter,
                    "unsupported session control protocol version {version}"
                )
            }
            Self::IndefiniteFrame => formatter
                .write_str("session control frame must use definite CBOR arrays and byte strings"),
            Self::WrongFieldCount { actual, expected } => {
                write!(
                    formatter,
                    "session control frame has {actual} fields, expected {expected}"
                )
            }
            Self::UnknownMessageKind(kind) => {
                write!(formatter, "unknown session control message kind {kind}")
            }
            Self::InvalidFieldWidth { field, actual } => {
                write!(
                    formatter,
                    "session control {} has {actual} bytes, expected 32",
                    field.label()
                )
            }
            Self::TrailingBytes(count) => {
                write!(
                    formatter,
                    "session control frame has {count} trailing bytes"
                )
            }
            Self::NonCanonicalFrame => {
                formatter.write_str("session control frame is not canonical positional CBOR")
            }
            Self::MalformedCbor => formatter.write_str("session control frame is malformed"),
            Self::CborEncode => formatter.write_str("session control CBOR encode failed"),
        }
    }
}

impl Error for SessionControlFrameError {}

pub fn encode_session_control_frame(
    frame: &SessionControlFrame,
) -> Result<Vec<u8>, SessionControlFrameError> {
    let mut bytes = Vec::with_capacity(SESSION_CONTROL_MAX_FRAME_BYTES);
    let mut encoder = Encoder::new(&mut bytes);

    match frame {
        SessionControlFrame::ClientHello(hello) => {
            encode_header(&mut encoder, CLIENT_HELLO_FIELDS, CLIENT_HELLO_KIND)?;
            encoder
                .bytes(hello.graph_id())
                .and_then(|encoder| encoder.u64(hello.graph_revision()))
                .and_then(|encoder| encoder.bytes(hello.schema_hash()))
                .map_err(|_| SessionControlFrameError::CborEncode)?;
            match hello.resume_token() {
                Some(token) => encoder
                    .bytes(token.as_bytes())
                    .map_err(|_| SessionControlFrameError::CborEncode)?,
                None => encoder
                    .null()
                    .map_err(|_| SessionControlFrameError::CborEncode)?,
            };
            encoder
                .u64(hello.applied_server_through())
                .map_err(|_| SessionControlFrameError::CborEncode)?;
        }
        SessionControlFrame::ServerOffer(offer) => {
            encode_header(&mut encoder, SERVER_OFFER_FIELDS, SERVER_OFFER_KIND)?;
            encoder
                .bytes(offer.resume_token().as_bytes())
                .and_then(|encoder| encoder.bytes(offer.session_id().as_bytes()))
                .and_then(|encoder| encoder.u64(offer.next_generation()))
                .and_then(|encoder| encoder.u64(offer.applied_client_through()))
                .map_err(|_| SessionControlFrameError::CborEncode)?;
        }
        SessionControlFrame::ClientCommit(commit) => {
            encode_header(&mut encoder, CLIENT_COMMIT_FIELDS, CLIENT_COMMIT_KIND)?;
            encoder
                .bytes(commit.session_id().as_bytes())
                .and_then(|encoder| encoder.u64(commit.generation()))
                .and_then(|encoder| encoder.u64(commit.applied_server_through()))
                .map_err(|_| SessionControlFrameError::CborEncode)?;
        }
        SessionControlFrame::ServerReady(ready) => {
            encode_header(&mut encoder, SERVER_READY_FIELDS, SERVER_READY_KIND)?;
            encoder
                .bytes(ready.session_id().as_bytes())
                .and_then(|encoder| encoder.u64(ready.generation()))
                .and_then(|encoder| encoder.u64(ready.applied_client_through()))
                .map_err(|_| SessionControlFrameError::CborEncode)?;
        }
        SessionControlFrame::ClientRevoke(_) => {
            encode_header(&mut encoder, CLIENT_REVOKE_FIELDS, CLIENT_REVOKE_KIND)?;
        }
        SessionControlFrame::ServerReject(_) => {
            encode_header(&mut encoder, SERVER_REJECT_FIELDS, SERVER_REJECT_KIND)?;
        }
        SessionControlFrame::ServerRevoked(_) => {
            encode_header(&mut encoder, SERVER_REVOKED_FIELDS, SERVER_REVOKED_KIND)?;
        }
    }

    check_frame_size(bytes.len())?;
    Ok(bytes)
}

pub fn decode_session_control_frame(
    bytes: &[u8],
) -> Result<SessionControlFrame, SessionControlFrameError> {
    check_frame_size(bytes.len())?;
    let mut decoder = Decoder::new(bytes);
    let field_count = decoder
        .array()
        .map_err(|_| SessionControlFrameError::MalformedCbor)?
        .ok_or(SessionControlFrameError::IndefiniteFrame)?;
    if field_count < 2 {
        return Err(SessionControlFrameError::WrongFieldCount {
            actual: field_count,
            expected: 2,
        });
    }

    let version = decoder
        .u16()
        .map_err(|_| SessionControlFrameError::MalformedCbor)?;
    if version != SESSION_CONTROL_PROTOCOL_VERSION {
        return Err(SessionControlFrameError::UnsupportedProtocolVersion(
            version,
        ));
    }
    let kind = decoder
        .u8()
        .map_err(|_| SessionControlFrameError::MalformedCbor)?;
    let expected_fields = fields_for_kind(kind)?;
    if field_count != expected_fields {
        return Err(SessionControlFrameError::WrongFieldCount {
            actual: field_count,
            expected: expected_fields,
        });
    }

    let frame = match kind {
        CLIENT_HELLO_KIND => {
            let graph_id = decode_fixed_bytes(&mut decoder, SessionControlField::GraphId)?;
            let graph_revision = decode_u64(&mut decoder)?;
            let schema_hash = decode_fixed_bytes(&mut decoder, SessionControlField::SchemaHash)?;
            let resume_token = decode_optional_token(&mut decoder)?;
            let applied_server_through = decode_u64(&mut decoder)?;
            SessionControlFrame::ClientHello(ClientHello::new(
                graph_id,
                graph_revision,
                schema_hash,
                resume_token,
                applied_server_through,
            ))
        }
        SERVER_OFFER_KIND => SessionControlFrame::ServerOffer(ServerOffer::new(
            decode_token(&mut decoder)?,
            decode_session_id(&mut decoder)?,
            decode_u64(&mut decoder)?,
            decode_u64(&mut decoder)?,
        )),
        CLIENT_COMMIT_KIND => SessionControlFrame::ClientCommit(ClientCommit::new(
            decode_session_id(&mut decoder)?,
            decode_u64(&mut decoder)?,
            decode_u64(&mut decoder)?,
        )),
        SERVER_READY_KIND => SessionControlFrame::ServerReady(ServerReady::new(
            decode_session_id(&mut decoder)?,
            decode_u64(&mut decoder)?,
            decode_u64(&mut decoder)?,
        )),
        CLIENT_REVOKE_KIND => SessionControlFrame::ClientRevoke(ClientRevoke::new()),
        SERVER_REJECT_KIND => SessionControlFrame::ServerReject(ServerReject::new()),
        SERVER_REVOKED_KIND => SessionControlFrame::ServerRevoked(ServerRevoked::new()),
        _ => unreachable!("message kind was validated"),
    };

    let position = decoder.position();
    if position != bytes.len() {
        return Err(SessionControlFrameError::TrailingBytes(
            bytes.len() - position,
        ));
    }
    if encode_session_control_frame(&frame)? != bytes {
        return Err(SessionControlFrameError::NonCanonicalFrame);
    }
    Ok(frame)
}

fn encode_header(
    encoder: &mut Encoder<&mut Vec<u8>>,
    fields: u64,
    kind: u8,
) -> Result<(), SessionControlFrameError> {
    encoder
        .array(fields)
        .and_then(|encoder| encoder.u16(SESSION_CONTROL_PROTOCOL_VERSION))
        .and_then(|encoder| encoder.u8(kind))
        .map_err(|_| SessionControlFrameError::CborEncode)?;
    Ok(())
}

fn fields_for_kind(kind: u8) -> Result<u64, SessionControlFrameError> {
    match kind {
        CLIENT_HELLO_KIND => Ok(CLIENT_HELLO_FIELDS),
        SERVER_OFFER_KIND => Ok(SERVER_OFFER_FIELDS),
        CLIENT_COMMIT_KIND => Ok(CLIENT_COMMIT_FIELDS),
        SERVER_READY_KIND => Ok(SERVER_READY_FIELDS),
        CLIENT_REVOKE_KIND => Ok(CLIENT_REVOKE_FIELDS),
        SERVER_REJECT_KIND => Ok(SERVER_REJECT_FIELDS),
        SERVER_REVOKED_KIND => Ok(SERVER_REVOKED_FIELDS),
        _ => Err(SessionControlFrameError::UnknownMessageKind(kind)),
    }
}

fn decode_u64(decoder: &mut Decoder<'_>) -> Result<u64, SessionControlFrameError> {
    decoder
        .u64()
        .map_err(|_| SessionControlFrameError::MalformedCbor)
}

fn decode_optional_token(
    decoder: &mut Decoder<'_>,
) -> Result<Option<ResumeToken>, SessionControlFrameError> {
    match decoder
        .datatype()
        .map_err(|_| SessionControlFrameError::MalformedCbor)?
    {
        Type::Null => {
            decoder
                .null()
                .map_err(|_| SessionControlFrameError::MalformedCbor)?;
            Ok(None)
        }
        Type::Bytes => decode_token(decoder).map(Some),
        Type::BytesIndef => Err(SessionControlFrameError::IndefiniteFrame),
        _ => Err(SessionControlFrameError::MalformedCbor),
    }
}

fn decode_token(decoder: &mut Decoder<'_>) -> Result<ResumeToken, SessionControlFrameError> {
    decode_fixed_bytes(decoder, SessionControlField::ResumeToken).map(ResumeToken::from_bytes)
}

fn decode_session_id(decoder: &mut Decoder<'_>) -> Result<SessionId, SessionControlFrameError> {
    decode_fixed_bytes(decoder, SessionControlField::SessionId).map(SessionId::from_bytes)
}

fn decode_fixed_bytes(
    decoder: &mut Decoder<'_>,
    field: SessionControlField,
) -> Result<[u8; 32], SessionControlFrameError> {
    if decoder
        .datatype()
        .map_err(|_| SessionControlFrameError::MalformedCbor)?
        == Type::BytesIndef
    {
        return Err(SessionControlFrameError::IndefiniteFrame);
    }
    let bytes = decoder
        .bytes()
        .map_err(|_| SessionControlFrameError::MalformedCbor)?;
    bytes
        .try_into()
        .map_err(|_| SessionControlFrameError::InvalidFieldWidth {
            field,
            actual: bytes.len(),
        })
}

fn check_frame_size(actual: usize) -> Result<(), SessionControlFrameError> {
    if actual > SESSION_CONTROL_MAX_FRAME_BYTES {
        return Err(SessionControlFrameError::FrameTooLarge {
            actual,
            maximum: SESSION_CONTROL_MAX_FRAME_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use static_assertions::{assert_impl_all, assert_not_impl_any};
    use std::fmt::{Debug, Display};

    assert_impl_all!(SessionId: Clone, Copy, Eq, Send, Sync);
    assert_impl_all!(ResumeToken: Send, Sync, ZeroizeOnDrop);
    assert_impl_all!(ResumeLookupKey: Send, Sync, ZeroizeOnDrop);
    assert_not_impl_any!(SessionId: Debug, Display, serde::Serialize);
    assert_not_impl_any!(ResumeToken: Clone, Copy, Debug, Display, serde::Serialize);
    assert_not_impl_any!(ResumeLookupKey: Clone, Copy, Debug, Display, serde::Serialize);
    assert_not_impl_any!(ClientHello: Debug, Display, serde::Serialize);
    assert_not_impl_any!(ServerOffer: Debug, Display, serde::Serialize);
    assert_not_impl_any!(ClientCommit: Debug, Display, serde::Serialize);
    assert_not_impl_any!(ServerReady: Debug, Display, serde::Serialize);
    assert_not_impl_any!(ClientRevoke: Debug, Display, serde::Serialize);
    assert_not_impl_any!(ServerReject: Debug, Display, serde::Serialize);
    assert_not_impl_any!(ServerRevoked: Debug, Display, serde::Serialize);
    assert_not_impl_any!(SessionControlFrame: Clone, Copy, Debug, Display, serde::Serialize);

    #[test]
    fn session_ids_are_exact_width_and_host_generated() {
        let bytes = std::array::from_fn(|index| index as u8);
        let id = SessionId::from_bytes(bytes);
        assert_eq!(id.as_bytes(), &bytes);
        assert_eq!(id.into_bytes(), bytes);

        let generated = SessionId::generate().unwrap();
        assert_eq!(generated.as_bytes().len(), SESSION_ID_BYTES);
    }

    #[test]
    fn resume_secrets_have_exact_canonical_storage_conversion() {
        let token = ResumeToken::from_bytes(std::array::from_fn(|index| index as u8));
        let lookup = token.to_lookup_key();
        assert_eq!(
            lookup.as_storage_str(),
            "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
        );
        assert_eq!(lookup.as_storage_str().len(), RESUME_LOOKUP_KEY_BYTES);

        let parsed = ResumeLookupKey::from_storage_str(lookup.as_storage_str()).unwrap();
        let restored = parsed.into_resume_token();
        assert_eq!(restored.as_bytes(), token.as_bytes());
        assert!(std::mem::needs_drop::<ResumeToken>());
        assert!(std::mem::needs_drop::<ResumeLookupKey>());
    }

    #[test]
    fn storage_conversion_rejects_noncanonical_base64url() {
        assert!(ResumeLookupKey::from_storage_str("short").is_err());
        assert!(
            ResumeLookupKey::from_storage_str("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .is_err()
        );
        assert!(
            ResumeLookupKey::from_storage_str("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh_")
                .is_err()
        );
        assert_eq!(
            ResumeLookupKeyError::InvalidEncoding.to_string(),
            "resume lookup key is not canonical base64url"
        );
    }

    #[test]
    fn token_generation_uses_the_exact_width() {
        let token = ResumeToken::generate().unwrap();
        assert_eq!(token.as_bytes().len(), RESUME_TOKEN_BYTES);
    }

    #[test]
    fn every_control_message_matches_golden_cbor() {
        let hello = SessionControlFrame::ClientHello(ClientHello::new(
            [0x11; 32],
            7,
            [0x22; 32],
            Some(ResumeToken::from_bytes([0x33; 32])),
            9,
        ));
        let mut hello_golden = vec![0x87, 0x03, CLIENT_HELLO_KIND, 0x58, 0x20];
        hello_golden.extend_from_slice(&[0x11; 32]);
        hello_golden.push(0x07);
        hello_golden.extend_from_slice(&[0x58, 0x20]);
        hello_golden.extend_from_slice(&[0x22; 32]);
        hello_golden.extend_from_slice(&[0x58, 0x20]);
        hello_golden.extend_from_slice(&[0x33; 32]);
        hello_golden.push(0x09);
        assert_eq!(encode_session_control_frame(&hello).unwrap(), hello_golden);

        let cases = [
            (
                SessionControlFrame::ServerOffer(ServerOffer::new(
                    ResumeToken::from_bytes([0x44; 32]),
                    SessionId::from_bytes([0x55; SESSION_ID_BYTES]),
                    11,
                    13,
                )),
                {
                    let mut golden = vec![0x86, 0x03, SERVER_OFFER_KIND, 0x58, 0x20];
                    golden.extend_from_slice(&[0x44; 32]);
                    golden.extend_from_slice(&[0x58, 0x20]);
                    golden.extend_from_slice(&[0x55; SESSION_ID_BYTES]);
                    golden.extend_from_slice(&[0x0b, 0x0d]);
                    golden
                },
            ),
            (
                SessionControlFrame::ClientCommit(ClientCommit::new(
                    SessionId::from_bytes([0x66; SESSION_ID_BYTES]),
                    17,
                    19,
                )),
                {
                    let mut golden = vec![0x85, 0x03, CLIENT_COMMIT_KIND, 0x58, 0x20];
                    golden.extend_from_slice(&[0x66; SESSION_ID_BYTES]);
                    golden.extend_from_slice(&[0x11, 0x13]);
                    golden
                },
            ),
            (
                SessionControlFrame::ServerReady(ServerReady::new(
                    SessionId::from_bytes([0x77; SESSION_ID_BYTES]),
                    23,
                    29,
                )),
                {
                    let mut golden = vec![0x85, 0x03, SERVER_READY_KIND, 0x58, 0x20];
                    golden.extend_from_slice(&[0x77; SESSION_ID_BYTES]);
                    golden.extend_from_slice(&[0x17, 0x18, 0x1d]);
                    golden
                },
            ),
            (
                SessionControlFrame::ClientRevoke(ClientRevoke::new()),
                vec![0x82, 0x03, CLIENT_REVOKE_KIND],
            ),
            (
                SessionControlFrame::ServerReject(ServerReject::new()),
                vec![0x82, 0x03, SERVER_REJECT_KIND],
            ),
            (
                SessionControlFrame::ServerRevoked(ServerRevoked::new()),
                vec![0x82, 0x03, SERVER_REVOKED_KIND],
            ),
        ];
        for (frame, golden) in cases {
            assert_eq!(encode_session_control_frame(&frame).unwrap(), golden);
        }
    }

    #[test]
    fn hello_without_resume_token_round_trips_the_server_cursor() {
        let frame =
            SessionControlFrame::ClientHello(ClientHello::new([1; 32], 1, [2; 32], None, 37));
        let bytes = encode_session_control_frame(&frame).unwrap();
        assert!(bytes.len() <= SESSION_CONTROL_MAX_FRAME_BYTES);
        let decoded = decode_session_control_frame(&bytes).unwrap();
        let SessionControlFrame::ClientHello(hello) = decoded else {
            panic!("decoded wrong control message kind");
        };
        assert_eq!(hello.graph_id(), &[1; 32]);
        assert_eq!(hello.graph_revision(), 1);
        assert_eq!(hello.schema_hash(), &[2; 32]);
        assert!(hello.resume_token().is_none());
        assert_eq!(hello.applied_server_through(), 37);
    }

    #[test]
    fn largest_legal_control_frame_stays_strictly_bounded() {
        let frame = SessionControlFrame::ClientHello(ClientHello::new(
            [0xff; 32],
            u64::MAX,
            [0xfe; 32],
            Some(ResumeToken::from_bytes([0xfd; 32])),
            u64::MAX,
        ));
        let bytes = encode_session_control_frame(&frame).unwrap();
        assert_eq!(bytes.len(), 123);
        assert!(bytes.len() <= SESSION_CONTROL_MAX_FRAME_BYTES);
    }

    #[test]
    fn offer_commit_and_ready_round_trip_all_replay_context() {
        let offer = SessionControlFrame::ServerOffer(ServerOffer::new(
            ResumeToken::from_bytes([0xa5; 32]),
            SessionId::from_bytes([0xb6; SESSION_ID_BYTES]),
            0x0102_0304_0506_0708,
            41,
        ));
        let offer =
            decode_session_control_frame(&encode_session_control_frame(&offer).unwrap()).unwrap();
        let SessionControlFrame::ServerOffer(offer) = offer else {
            panic!("decoded wrong control message kind");
        };
        let (token, session_id, next_generation, applied_client_through) = offer.into_parts();
        assert_eq!(token.as_bytes(), &[0xa5; 32]);
        assert!(session_id == SessionId::from_bytes([0xb6; SESSION_ID_BYTES]));
        assert_eq!(next_generation, 0x0102_0304_0506_0708);
        assert_eq!(applied_client_through, 41);

        let commit = SessionControlFrame::ClientCommit(ClientCommit::new(session_id, 43, 47));
        let commit =
            decode_session_control_frame(&encode_session_control_frame(&commit).unwrap()).unwrap();
        let SessionControlFrame::ClientCommit(commit) = commit else {
            panic!("decoded wrong control message kind");
        };
        assert!(commit.session_id() == session_id);
        assert_eq!(commit.generation(), 43);
        assert_eq!(commit.applied_server_through(), 47);

        let ready = SessionControlFrame::ServerReady(ServerReady::new(session_id, 43, 53));
        let ready =
            decode_session_control_frame(&encode_session_control_frame(&ready).unwrap()).unwrap();
        let SessionControlFrame::ServerReady(ready) = ready else {
            panic!("decoded wrong control message kind");
        };
        assert!(ready.session_id() == session_id);
        assert_eq!(ready.generation(), 43);
        assert_eq!(ready.applied_client_through(), 53);
    }

    #[test]
    fn decoder_rejects_v2_without_compatibility_decoding() {
        assert!(matches!(
            decode_session_control_frame(&[0x82, 0x02, CLIENT_COMMIT_KIND]),
            Err(SessionControlFrameError::UnsupportedProtocolVersion(2))
        ));
    }

    #[test]
    fn decoder_rejects_oversize_indefinite_trailing_and_noncanonical_frames() {
        assert!(matches!(
            decode_session_control_frame(&[0; SESSION_CONTROL_MAX_FRAME_BYTES + 1]),
            Err(SessionControlFrameError::FrameTooLarge { .. })
        ));
        assert!(matches!(
            decode_session_control_frame(&[0x9f, 0x03, CLIENT_COMMIT_KIND, 0xff]),
            Err(SessionControlFrameError::IndefiniteFrame)
        ));

        let mut indefinite_token = vec![0x86, 0x03, SERVER_OFFER_KIND, 0x5f, 0x58, 0x20];
        indefinite_token.extend_from_slice(&[7; 32]);
        indefinite_token.push(0xff);
        indefinite_token.extend_from_slice(&[0x58, 0x20]);
        indefinite_token.extend_from_slice(&[8; SESSION_ID_BYTES]);
        indefinite_token.extend_from_slice(&[1, 0]);
        assert!(matches!(
            decode_session_control_frame(&indefinite_token),
            Err(SessionControlFrameError::IndefiniteFrame)
        ));

        let canonical = encode_session_control_frame(&SessionControlFrame::ClientCommit(
            ClientCommit::new(SessionId::from_bytes([9; SESSION_ID_BYTES]), 1, 0),
        ))
        .unwrap();
        let mut trailing = canonical.clone();
        trailing.push(0);
        assert!(matches!(
            decode_session_control_frame(&trailing),
            Err(SessionControlFrameError::TrailingBytes(1))
        ));

        let mut noncanonical_array = vec![0x98, CLIENT_COMMIT_FIELDS as u8];
        noncanonical_array.extend_from_slice(&canonical[1..]);
        assert!(matches!(
            decode_session_control_frame(&noncanonical_array),
            Err(SessionControlFrameError::NonCanonicalFrame)
        ));

        let mut noncanonical_version = vec![0x85, 0x18, 0x03];
        noncanonical_version.extend_from_slice(&canonical[2..]);
        assert!(matches!(
            decode_session_control_frame(&noncanonical_version),
            Err(SessionControlFrameError::NonCanonicalFrame)
        ));
    }

    #[test]
    fn decoder_rejects_kind_count_width_and_order_changes() {
        assert!(matches!(
            decode_session_control_frame(&[0x82, 0x03, 0x07]),
            Err(SessionControlFrameError::UnknownMessageKind(0x07))
        ));
        assert!(matches!(
            decode_session_control_frame(&[0x84, 0x03, CLIENT_COMMIT_KIND, 0, 0]),
            Err(SessionControlFrameError::WrongFieldCount {
                actual: 4,
                expected: CLIENT_COMMIT_FIELDS
            })
        ));

        let mut narrow_graph = vec![0x87, 0x03, CLIENT_HELLO_KIND, 0x58, 0x1f];
        narrow_graph.extend_from_slice(&[1; 31]);
        narrow_graph.push(0);
        narrow_graph.extend_from_slice(&[0x58, 0x20]);
        narrow_graph.extend_from_slice(&[2; 32]);
        narrow_graph.extend_from_slice(&[0xf6, 0]);
        assert!(matches!(
            decode_session_control_frame(&narrow_graph),
            Err(SessionControlFrameError::InvalidFieldWidth {
                field: SessionControlField::GraphId,
                actual: 31
            })
        ));

        let mut narrow_session = vec![0x86, 0x03, SERVER_OFFER_KIND, 0x58, 0x20];
        narrow_session.extend_from_slice(&[3; 32]);
        narrow_session.extend_from_slice(&[0x58, 0x1f]);
        narrow_session.extend_from_slice(&[4; SESSION_ID_BYTES - 1]);
        narrow_session.extend_from_slice(&[1, 0]);
        assert!(matches!(
            decode_session_control_frame(&narrow_session),
            Err(SessionControlFrameError::InvalidFieldWidth {
                field: SessionControlField::SessionId,
                actual: 31
            })
        ));

        let mut narrow_token = vec![0x86, 0x03, SERVER_OFFER_KIND, 0x58, 0x1f];
        narrow_token.extend_from_slice(&[5; 31]);
        narrow_token.extend_from_slice(&[0x58, 0x20]);
        narrow_token.extend_from_slice(&[6; SESSION_ID_BYTES]);
        narrow_token.extend_from_slice(&[1, 0]);
        assert!(matches!(
            decode_session_control_frame(&narrow_token),
            Err(SessionControlFrameError::InvalidFieldWidth {
                field: SessionControlField::ResumeToken,
                actual: 31
            })
        ));

        let mut wrong_order = vec![0x87, 0x03, CLIENT_HELLO_KIND, 0x58, 0x20];
        wrong_order.extend_from_slice(&[1; 32]);
        wrong_order.extend_from_slice(&[0x58, 0x20]);
        wrong_order.extend_from_slice(&[2; 32]);
        wrong_order.push(0);
        wrong_order.push(0xf6);
        wrong_order.push(0);
        assert!(matches!(
            decode_session_control_frame(&wrong_order),
            Err(SessionControlFrameError::MalformedCbor)
        ));
    }

    #[test]
    fn errors_never_render_secret_identity_or_cursor_values() {
        let token_text = "super_secret_resume_token_material";
        let mut malformed_token = vec![0x86, 0x03, SERVER_OFFER_KIND, 0x58, 0x22];
        malformed_token.extend_from_slice(token_text.as_bytes());
        let error = match decode_session_control_frame(&malformed_token) {
            Err(error) => error,
            Ok(_) => panic!("malformed token was accepted"),
        };
        let display = error.to_string();
        let debug = format!("{error:?}");
        assert!(!display.contains(token_text));
        assert!(!debug.contains(token_text));

        let session_text = "private-session-identity-material!";
        let mut malformed_session = vec![0x86, 0x03, SERVER_OFFER_KIND, 0x58, 0x20];
        malformed_session.extend_from_slice(&[7; 32]);
        malformed_session.push(0x58);
        malformed_session.push(session_text.len() as u8);
        malformed_session.extend_from_slice(session_text.as_bytes());
        let error = match decode_session_control_frame(&malformed_session) {
            Err(error) => error,
            Ok(_) => panic!("malformed Session ID was accepted"),
        };
        assert!(!error.to_string().contains(session_text));
        assert!(!format!("{error:?}").contains(session_text));

        let lookup_error = ResumeLookupKey::from_storage_str(token_text).err().unwrap();
        assert!(!lookup_error.to_string().contains(token_text));
        assert!(!format!("{lookup_error:?}").contains(token_text));
    }
}
