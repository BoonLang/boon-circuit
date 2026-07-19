use boon_wire::{
    ClientCommit, ClientHello, ClientRevoke, ClientSessionFrameLimits, ResumeLookupKey,
    SESSION_CONTROL_MAX_FRAME_BYTES, SessionControlFrame, SessionControlFrameError, SessionId,
    decode_session_control_frame, encode_session_control_frame,
};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{self, Display, Formatter, Write as _};

const STORAGE_KEY_DOMAIN: &[u8] = b"boon.distributed-session-storage.v2\0";
const STORAGE_KEY_PREFIX: &str = "boon.session.v2.";
const TOKEN_JOURNAL_PREFIX: &str = "2:";
const TOKEN_JOURNAL_EMPTY: &str = "-";
const MAX_PACKAGE_ID_BYTES: usize = 256;
pub const DISTRIBUTED_SESSION_STORAGE_KEY_BYTES: usize = STORAGE_KEY_PREFIX.len() + 64;
pub const DISTRIBUTED_SESSION_TOKEN_JOURNAL_MAX_BYTES: usize = TOKEN_JOURNAL_PREFIX.len()
    + boon_wire::RESUME_LOOKUP_KEY_BYTES
    + 1
    + boon_wire::RESUME_LOOKUP_KEY_BYTES;

pub type DistributedSessionHandshakeResult<T> = Result<T, DistributedSessionHandshakeError>;

/// Exact public identity used by one browser-owned distributed Session.
///
/// The storage key is a fixed-size digest of this identity. It never contains
/// resume authority or transport-local identifiers.
pub struct DistributedSessionIdentity {
    graph_id: [u8; 32],
    graph_revision: u64,
    schema_hash: [u8; 32],
    storage_key: String,
}

impl DistributedSessionIdentity {
    pub fn new(
        package_id: &str,
        graph_id: [u8; 32],
        graph_revision: u64,
        schema_hash: [u8; 32],
    ) -> DistributedSessionHandshakeResult<Self> {
        validate_package_id(package_id)?;
        if graph_revision == 0 {
            return Err(DistributedSessionHandshakeError::InvalidIdentity(
                "graph revision must be non-zero",
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(STORAGE_KEY_DOMAIN);
        hasher.update((package_id.len() as u16).to_be_bytes());
        hasher.update(package_id.as_bytes());
        hasher.update(graph_id);
        hasher.update(graph_revision.to_be_bytes());
        hasher.update(schema_hash);
        let digest = hasher.finalize();

        let mut storage_key = String::with_capacity(DISTRIBUTED_SESSION_STORAGE_KEY_BYTES);
        storage_key.push_str(STORAGE_KEY_PREFIX);
        for byte in digest {
            write!(&mut storage_key, "{byte:02x}")
                .expect("writing a digest to a String cannot fail");
        }
        debug_assert_eq!(storage_key.len(), DISTRIBUTED_SESSION_STORAGE_KEY_BYTES);

        Ok(Self {
            graph_id,
            graph_revision,
            schema_hash,
            storage_key,
        })
    }

    pub fn storage_key(&self) -> &str {
        &self.storage_key
    }
}

fn validate_package_id(package_id: &str) -> DistributedSessionHandshakeResult<()> {
    if package_id.is_empty()
        || package_id.len() > MAX_PACKAGE_ID_BYTES
        || package_id.trim() != package_id
        || !package_id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
    {
        return Err(DistributedSessionHandshakeError::InvalidIdentity(
            "package ID must be a bounded canonical identifier",
        ));
    }
    Ok(())
}

/// The only persistence operations permitted for browser Session resumption.
///
/// Implementations must bind all three operations to the same session-scoped
/// storage object. `write` must atomically replace the value before returning.
/// A platform error must be returned rather than substituted with another
/// storage mechanism.
pub trait DistributedSessionJournalStore {
    fn read(&mut self, key: &str) -> Result<Option<String>, DistributedSessionStorageError>;
    fn write(&mut self, key: &str, value: &str) -> Result<(), DistributedSessionStorageError>;
    fn remove(&mut self, key: &str) -> Result<(), DistributedSessionStorageError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributedSessionStorageError {
    operation: &'static str,
    message: String,
}

impl DistributedSessionStorageError {
    pub fn platform(operation: &'static str, message: impl Into<String>) -> Self {
        Self {
            operation,
            message: message.into(),
        }
    }

    pub fn operation(&self) -> &'static str {
        self.operation
    }
}

impl Display for DistributedSessionStorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "distributed Session storage {} failed: {}",
            self.operation, self.message
        )
    }
}

impl Error for DistributedSessionStorageError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributedSessionHandshakePhase {
    AwaitingOffer,
    AwaitingReady,
    Current,
    AwaitingRevoke,
    Rejected,
    Revoked,
    Failed,
}

#[derive(Debug)]
pub enum DistributedSessionHandshakeError {
    InvalidIdentity(&'static str),
    InvalidControlFrame(SessionControlFrameError),
    Storage(DistributedSessionStorageError),
    UnexpectedControlFrame(DistributedSessionHandshakePhase),
    Closed(DistributedSessionHandshakePhase),
}

impl Display for DistributedSessionHandshakeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(reason) => {
                write!(formatter, "invalid distributed Session identity: {reason}")
            }
            Self::InvalidControlFrame(error) => Display::fmt(error, formatter),
            Self::Storage(error) => Display::fmt(error, formatter),
            Self::UnexpectedControlFrame(phase) => {
                write!(
                    formatter,
                    "unexpected Session control frame during {phase:?}"
                )
            }
            Self::Closed(phase) => {
                write!(
                    formatter,
                    "distributed Session handshake is already {phase:?}"
                )
            }
        }
    }
}

impl Error for DistributedSessionHandshakeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidControlFrame(error) => Some(error),
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

impl From<SessionControlFrameError> for DistributedSessionHandshakeError {
    fn from(error: SessionControlFrameError) -> Self {
        Self::InvalidControlFrame(error)
    }
}

impl From<DistributedSessionStorageError> for DistributedSessionHandshakeError {
    fn from(error: DistributedSessionStorageError) -> Self {
        Self::Storage(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResumeAttempt {
    Pending,
    Current,
    Fresh,
}

enum RejectProgress {
    Retry(ResumeAttempt),
    Terminal,
}

struct ResumeTokenJournal {
    current: Option<ResumeLookupKey>,
    pending: Option<ResumeLookupKey>,
}

impl ResumeTokenJournal {
    fn empty() -> Self {
        Self {
            current: None,
            pending: None,
        }
    }

    fn parse(storage: &str) -> Result<Self, ()> {
        if storage.len() > DISTRIBUTED_SESSION_TOKEN_JOURNAL_MAX_BYTES
            || !storage.starts_with(TOKEN_JOURNAL_PREFIX)
        {
            return Err(());
        }
        let fields = &storage[TOKEN_JOURNAL_PREFIX.len()..];
        let Some((current, pending)) = fields.split_once(':') else {
            return Err(());
        };
        if pending.contains(':') {
            return Err(());
        }
        let journal = Self {
            current: parse_journal_key(current)?,
            pending: parse_journal_key(pending)?,
        };
        if journal.current.is_none() && journal.pending.is_none() {
            return Err(());
        }
        let canonical = journal.encode();
        if canonical.as_str() != storage {
            return Err(());
        }
        Ok(journal)
    }

    fn initial_attempt(&self) -> ResumeAttempt {
        if self.pending.is_some() {
            ResumeAttempt::Pending
        } else if self.current.is_some() {
            ResumeAttempt::Current
        } else {
            ResumeAttempt::Fresh
        }
    }

    fn resume_token(&self, attempt: ResumeAttempt) -> Option<boon_wire::ResumeToken> {
        match attempt {
            ResumeAttempt::Pending => self.pending.as_ref(),
            ResumeAttempt::Current => self.current.as_ref(),
            ResumeAttempt::Fresh => None,
        }
        .map(ResumeLookupKey::to_resume_token)
    }

    fn install_offer(&mut self, attempt: ResumeAttempt, offered: ResumeLookupKey) {
        match attempt {
            ResumeAttempt::Pending => {
                self.current = self.pending.take();
            }
            ResumeAttempt::Current => {
                self.pending = None;
            }
            ResumeAttempt::Fresh => {
                self.current = None;
                self.pending = None;
            }
        }
        self.pending = Some(offered);
    }

    fn reject(&mut self, attempt: ResumeAttempt) -> RejectProgress {
        match attempt {
            ResumeAttempt::Pending => {
                self.pending = None;
                if self.current.is_some() {
                    RejectProgress::Retry(ResumeAttempt::Current)
                } else {
                    RejectProgress::Retry(ResumeAttempt::Fresh)
                }
            }
            ResumeAttempt::Current => {
                self.current = None;
                self.pending = None;
                RejectProgress::Retry(ResumeAttempt::Fresh)
            }
            ResumeAttempt::Fresh => {
                self.current = None;
                self.pending = None;
                RejectProgress::Terminal
            }
        }
    }

    fn collapse_ready(&mut self) {
        self.current = Some(
            self.pending
                .take()
                .expect("AwaitingReady always owns a pending resume key"),
        );
    }

    fn encode(&self) -> EncodedResumeTokenJournal {
        debug_assert!(self.current.is_some() || self.pending.is_some());
        let mut encoded = EncodedResumeTokenJournal::new();
        encoded.push(TOKEN_JOURNAL_PREFIX.as_bytes());
        encoded.push(
            self.current
                .as_ref()
                .map_or(TOKEN_JOURNAL_EMPTY.as_bytes(), |key| {
                    key.as_storage_bytes().as_slice()
                }),
        );
        encoded.push(b":");
        encoded.push(
            self.pending
                .as_ref()
                .map_or(TOKEN_JOURNAL_EMPTY.as_bytes(), |key| {
                    key.as_storage_bytes().as_slice()
                }),
        );
        encoded
    }
}

fn parse_journal_key(field: &str) -> Result<Option<ResumeLookupKey>, ()> {
    if field == TOKEN_JOURNAL_EMPTY {
        Ok(None)
    } else {
        ResumeLookupKey::from_storage_str(field)
            .map(Some)
            .map_err(|_| ())
    }
}

struct EncodedResumeTokenJournal {
    bytes: [u8; DISTRIBUTED_SESSION_TOKEN_JOURNAL_MAX_BYTES],
    len: usize,
}

impl EncodedResumeTokenJournal {
    fn new() -> Self {
        Self {
            bytes: [0; DISTRIBUTED_SESSION_TOKEN_JOURNAL_MAX_BYTES],
            len: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let end = self
            .len
            .checked_add(bytes.len())
            .expect("resume journal length is bounded");
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len])
            .expect("resume journal contains only canonical ASCII")
    }
}

impl Drop for EncodedResumeTokenJournal {
    fn drop(&mut self) {
        self.bytes.fill(0);
        self.len = 0;
    }
}

enum HandshakeState {
    AwaitingOffer {
        attempt: ResumeAttempt,
        journal: ResumeTokenJournal,
    },
    AwaitingReady {
        attempt: ResumeAttempt,
        journal: ResumeTokenJournal,
        session_id: SessionId,
        generation: u64,
        applied_client_through: u64,
    },
    Current {
        session_id: SessionId,
        generation: u64,
        applied_client_through: u64,
    },
    AwaitingRevoke {
        session_id: SessionId,
        generation: u64,
        applied_client_through: u64,
    },
    Rejected,
    Revoked,
    Failed,
}

impl HandshakeState {
    fn phase(&self) -> DistributedSessionHandshakePhase {
        match self {
            Self::AwaitingOffer { .. } => DistributedSessionHandshakePhase::AwaitingOffer,
            Self::AwaitingReady { .. } => DistributedSessionHandshakePhase::AwaitingReady,
            Self::Current { .. } => DistributedSessionHandshakePhase::Current,
            Self::AwaitingRevoke { .. } => DistributedSessionHandshakePhase::AwaitingRevoke,
            Self::Rejected => DistributedSessionHandshakePhase::Rejected,
            Self::Revoked => DistributedSessionHandshakePhase::Revoked,
            Self::Failed => DistributedSessionHandshakePhase::Failed,
        }
    }
}

/// One phase transition produced by an accepted server control frame.
///
/// This type deliberately has no `Debug` implementation because a client
/// frame can contain host-only transport material.
pub enum DistributedSessionHandshakeStep {
    SendClientFrame(Vec<u8>),
    Current,
    Rejected,
    Revoked,
}

/// Browser-side owner of the canonical distributed Session handshake.
///
/// The controller journals every offered key before emitting `ClientCommit`.
/// A restart therefore tries the uncertain pending key before the last current
/// key and finally makes one bounded fresh attempt.
pub struct DistributedSessionHandshake<S> {
    storage: S,
    identity: DistributedSessionIdentity,
    state: HandshakeState,
    applied_server_through: u64,
}

impl<S: DistributedSessionJournalStore> DistributedSessionHandshake<S> {
    pub fn start(
        identity: DistributedSessionIdentity,
        mut storage: S,
        applied_server_through: u64,
    ) -> DistributedSessionHandshakeResult<(Self, Vec<u8>)> {
        let journal = match storage.read(identity.storage_key())? {
            Some(stored) => {
                let parsed = ResumeTokenJournal::parse(&stored);
                let mut secret_bytes = stored.into_bytes();
                secret_bytes.fill(0);
                match parsed {
                    Ok(journal) => journal,
                    Err(()) => {
                        storage.remove(identity.storage_key())?;
                        ResumeTokenJournal::empty()
                    }
                }
            }
            None => ResumeTokenJournal::empty(),
        };
        let attempt = journal.initial_attempt();
        let client_hello =
            encode_client_hello(&identity, &journal, attempt, applied_server_through)?;
        Ok((
            Self {
                storage,
                identity,
                state: HandshakeState::AwaitingOffer { attempt, journal },
                applied_server_through,
            },
            client_hello,
        ))
    }

    fn restart(
        self,
        applied_server_through: u64,
    ) -> DistributedSessionHandshakeResult<(Self, Vec<u8>)> {
        let Self {
            storage, identity, ..
        } = self;
        Self::start(identity, storage, applied_server_through)
    }

    pub fn phase(&self) -> DistributedSessionHandshakePhase {
        self.state.phase()
    }

    pub fn storage_key(&self) -> &str {
        self.identity.storage_key()
    }

    /// Returns the current transport generation only after token rotation was
    /// durably committed to session storage.
    pub fn generation(&self) -> Option<u64> {
        match &self.state {
            HandshakeState::Current { generation, .. }
            | HandshakeState::AwaitingRevoke { generation, .. } => Some(*generation),
            _ => None,
        }
    }

    pub fn binding(&self) -> Option<(SessionId, u64, u64)> {
        match &self.state {
            HandshakeState::Current {
                session_id,
                generation,
                applied_client_through,
            }
            | HandshakeState::AwaitingRevoke {
                session_id,
                generation,
                applied_client_through,
            } => Some((*session_id, *generation, *applied_client_through)),
            _ => None,
        }
    }

    pub fn accept_server_frame(
        &mut self,
        bytes: &[u8],
    ) -> DistributedSessionHandshakeResult<DistributedSessionHandshakeStep> {
        let frame = match decode_session_control_frame(bytes) {
            Ok(frame) => frame,
            Err(error) => {
                self.state = HandshakeState::Failed;
                return Err(error.into());
            }
        };
        let state = std::mem::replace(&mut self.state, HandshakeState::Failed);
        match (state, frame) {
            (
                HandshakeState::AwaitingOffer {
                    attempt,
                    mut journal,
                },
                SessionControlFrame::ServerOffer(offer),
            ) => {
                let (token, session_id, generation, applied_client_through) = offer.into_parts();
                journal.install_offer(attempt, token.to_lookup_key());
                let frame = encode_session_control_frame(&SessionControlFrame::ClientCommit(
                    ClientCommit::new(session_id, generation, self.applied_server_through),
                ))?;
                self.write_journal(&journal)?;
                self.state = HandshakeState::AwaitingReady {
                    attempt,
                    journal,
                    session_id,
                    generation,
                    applied_client_through,
                };
                Ok(DistributedSessionHandshakeStep::SendClientFrame(frame))
            }
            (
                HandshakeState::AwaitingReady {
                    attempt: _,
                    mut journal,
                    session_id,
                    generation,
                    applied_client_through,
                },
                SessionControlFrame::ServerReady(ready),
            ) => {
                if ready.session_id() != session_id
                    || ready.generation() != generation
                    || ready.applied_client_through() != applied_client_through
                {
                    self.state = HandshakeState::Failed;
                    return Err(DistributedSessionHandshakeError::UnexpectedControlFrame(
                        DistributedSessionHandshakePhase::AwaitingReady,
                    ));
                }
                journal.collapse_ready();
                self.write_journal(&journal)?;
                self.state = HandshakeState::Current {
                    session_id,
                    generation,
                    applied_client_through,
                };
                Ok(DistributedSessionHandshakeStep::Current)
            }
            (
                HandshakeState::AwaitingOffer {
                    attempt,
                    mut journal,
                }
                | HandshakeState::AwaitingReady {
                    attempt,
                    mut journal,
                    session_id: _,
                    generation: _,
                    applied_client_through: _,
                },
                SessionControlFrame::ServerReject(_),
            ) => self.retry_after_reject(attempt, &mut journal),
            (HandshakeState::AwaitingRevoke { .. }, SessionControlFrame::ServerRevoked(_)) => {
                self.storage.remove(self.identity.storage_key())?;
                self.state = HandshakeState::Revoked;
                Ok(DistributedSessionHandshakeStep::Revoked)
            }
            (state, _) => {
                let phase = state.phase();
                self.state = HandshakeState::Failed;
                Err(DistributedSessionHandshakeError::UnexpectedControlFrame(
                    phase,
                ))
            }
        }
    }

    fn retry_after_reject(
        &mut self,
        attempt: ResumeAttempt,
        journal: &mut ResumeTokenJournal,
    ) -> DistributedSessionHandshakeResult<DistributedSessionHandshakeStep> {
        match journal.reject(attempt) {
            RejectProgress::Retry(next_attempt @ ResumeAttempt::Current) => {
                let frame = encode_client_hello(
                    &self.identity,
                    journal,
                    next_attempt,
                    self.applied_server_through,
                )?;
                self.write_journal(journal)?;
                self.state = HandshakeState::AwaitingOffer {
                    attempt: next_attempt,
                    journal: std::mem::replace(journal, ResumeTokenJournal::empty()),
                };
                Ok(DistributedSessionHandshakeStep::SendClientFrame(frame))
            }
            RejectProgress::Retry(next_attempt @ ResumeAttempt::Fresh) => {
                let frame = encode_client_hello(
                    &self.identity,
                    journal,
                    next_attempt,
                    self.applied_server_through,
                )?;
                self.storage.remove(self.identity.storage_key())?;
                self.state = HandshakeState::AwaitingOffer {
                    attempt: next_attempt,
                    journal: std::mem::replace(journal, ResumeTokenJournal::empty()),
                };
                Ok(DistributedSessionHandshakeStep::SendClientFrame(frame))
            }
            RejectProgress::Retry(ResumeAttempt::Pending) => {
                unreachable!("a rejected token cannot retry an older pending token")
            }
            RejectProgress::Terminal => {
                self.storage.remove(self.identity.storage_key())?;
                self.state = HandshakeState::Rejected;
                Ok(DistributedSessionHandshakeStep::Rejected)
            }
        }
    }

    fn write_journal(
        &mut self,
        journal: &ResumeTokenJournal,
    ) -> DistributedSessionHandshakeResult<()> {
        let encoded = journal.encode();
        self.storage
            .write(self.identity.storage_key(), encoded.as_str())?;
        Ok(())
    }

    /// Requests revocation but preserves the journal until the server confirms it.
    pub fn revoke(&mut self) -> DistributedSessionHandshakeResult<Vec<u8>> {
        let state = std::mem::replace(&mut self.state, HandshakeState::Failed);
        let HandshakeState::Current {
            session_id,
            generation,
            applied_client_through,
        } = state
        else {
            let phase = state.phase();
            self.state = state;
            return Err(DistributedSessionHandshakeError::Closed(phase));
        };
        let frame =
            encode_session_control_frame(&SessionControlFrame::ClientRevoke(ClientRevoke::new()))?;
        self.state = HandshakeState::AwaitingRevoke {
            session_id,
            generation,
            applied_client_through,
        };
        Ok(frame)
    }
}

fn encode_client_hello(
    identity: &DistributedSessionIdentity,
    journal: &ResumeTokenJournal,
    attempt: ResumeAttempt,
    applied_server_through: u64,
) -> DistributedSessionHandshakeResult<Vec<u8>> {
    Ok(encode_session_control_frame(
        &SessionControlFrame::ClientHello(ClientHello::new(
            identity.graph_id,
            identity.graph_revision,
            identity.schema_hash,
            journal.resume_token(attempt),
            applied_server_through,
        )),
    )?)
}

const DEFAULT_DISTRIBUTED_SESSION_QUEUE_MESSAGES: usize = 64;
const DEFAULT_DISTRIBUTED_SESSION_QUEUE_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_DISTRIBUTED_SESSION_FRAME_BYTES: usize = 256 * 1024;

/// Browser transport limits applied in addition to the runtime's typed queues.
///
/// `max_frame_bytes` must cover the complete Client/Session wire contract. The
/// separate message and aggregate-byte limits bound frames waiting on browser
/// admission and frames waiting to enter the runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistributedSessionSocketLimits {
    pub max_frame_bytes: usize,
    pub max_inbound_messages: usize,
    pub max_inbound_bytes: usize,
    pub max_outbound_messages: usize,
    pub max_outbound_bytes: usize,
}

impl Default for DistributedSessionSocketLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: DEFAULT_DISTRIBUTED_SESSION_FRAME_BYTES,
            max_inbound_messages: DEFAULT_DISTRIBUTED_SESSION_QUEUE_MESSAGES,
            max_inbound_bytes: DEFAULT_DISTRIBUTED_SESSION_QUEUE_BYTES,
            max_outbound_messages: DEFAULT_DISTRIBUTED_SESSION_QUEUE_MESSAGES,
            max_outbound_bytes: DEFAULT_DISTRIBUTED_SESSION_QUEUE_BYTES,
        }
    }
}

impl DistributedSessionSocketLimits {
    fn validate(self) -> Result<Self, DistributedSessionSocketError> {
        if self.max_inbound_messages == 0 {
            return Err(DistributedSessionSocketError::InvalidLimits(
                "max_inbound_messages must be non-zero",
            ));
        }
        if self.max_outbound_messages == 0 {
            return Err(DistributedSessionSocketError::InvalidLimits(
                "max_outbound_messages must be non-zero",
            ));
        }
        let required_frame_bytes = ClientSessionFrameLimits::default()
            .max_frame_bytes
            .max(SESSION_CONTROL_MAX_FRAME_BYTES);
        if self.max_frame_bytes < required_frame_bytes {
            return Err(DistributedSessionSocketError::InvalidLimits(
                "max_frame_bytes is smaller than the Client/Session wire contract",
            ));
        }
        if self.max_inbound_bytes < self.max_frame_bytes {
            return Err(DistributedSessionSocketError::InvalidLimits(
                "max_inbound_bytes must admit at least one maximum-size frame",
            ));
        }
        if self.max_outbound_bytes < self.max_frame_bytes {
            return Err(DistributedSessionSocketError::InvalidLimits(
                "max_outbound_bytes must admit at least one maximum-size frame",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributedSessionSocketDirection {
    Inbound,
    Outbound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributedSessionSocketPhase {
    Idle,
    Connecting,
    AwaitingOffer,
    AwaitingReady,
    Current,
    AwaitingRevoke,
    ReconnectRequired,
    Rejected,
    Revoked,
    Closed,
    Failed,
}

#[derive(Debug)]
pub enum DistributedSessionSocketError {
    InvalidLimits(&'static str),
    Handshake(DistributedSessionHandshakeError),
    Runtime(boon_runtime::DistributedRuntimeError),
    FrameTooLarge {
        direction: DistributedSessionSocketDirection,
        actual: usize,
        maximum: usize,
    },
    QueueFull {
        direction: DistributedSessionSocketDirection,
        max_messages: usize,
        max_bytes: usize,
    },
    UnexpectedSocketEvent(DistributedSessionSocketPhase),
    TextFrame,
    InvalidOutboundLease,
    SocketEpochExhausted,
    OutboundLeaseExhausted,
}

impl Display for DistributedSessionSocketError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits(reason) => {
                write!(
                    formatter,
                    "invalid distributed Session socket limits: {reason}"
                )
            }
            Self::Handshake(error) => Display::fmt(error, formatter),
            Self::Runtime(error) => Display::fmt(error, formatter),
            Self::FrameTooLarge {
                direction,
                actual,
                maximum,
            } => write!(
                formatter,
                "distributed Session {direction:?} frame has {actual} bytes; maximum is {maximum}"
            ),
            Self::QueueFull {
                direction,
                max_messages,
                max_bytes,
            } => write!(
                formatter,
                "distributed Session {direction:?} queue reached {max_messages} messages or {max_bytes} bytes"
            ),
            Self::UnexpectedSocketEvent(phase) => {
                write!(
                    formatter,
                    "unexpected socket event while Session is {phase:?}"
                )
            }
            Self::TextFrame => {
                formatter.write_str("distributed Session WebSocket requires binary frames")
            }
            Self::InvalidOutboundLease => {
                formatter.write_str("distributed Session outbound lease is not current")
            }
            Self::SocketEpochExhausted => {
                formatter.write_str("distributed Session socket epoch is exhausted")
            }
            Self::OutboundLeaseExhausted => {
                formatter.write_str("distributed Session outbound lease ID is exhausted")
            }
        }
    }
}

impl Error for DistributedSessionSocketError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Handshake(error) => Some(error),
            Self::Runtime(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DistributedSessionHandshakeError> for DistributedSessionSocketError {
    fn from(error: DistributedSessionHandshakeError) -> Self {
        Self::Handshake(error)
    }
}

impl From<boon_runtime::DistributedRuntimeError> for DistributedSessionSocketError {
    fn from(error: boon_runtime::DistributedRuntimeError) -> Self {
        Self::Runtime(error)
    }
}

/// Narrow transport-facing surface of [`boon_runtime::DistributedClientRuntime`].
///
/// The trait keeps browser-independent lifecycle tests deterministic without a
/// WebAssembly runtime. Product hosts use the concrete implementation below.
pub trait DistributedSessionClientRuntime {
    fn bind(
        &mut self,
        session_id: SessionId,
        generation: u64,
        applied_client_through: u64,
    ) -> Result<boon_runtime::DistributedClientUpdate, boon_runtime::DistributedRuntimeError>;

    fn mark_current(
        &mut self,
    ) -> Result<boon_runtime::DistributedClientUpdate, boon_runtime::DistributedRuntimeError>;

    fn mark_stale(
        &mut self,
    ) -> Result<boon_runtime::DistributedClientUpdate, boon_runtime::DistributedRuntimeError>;

    fn accept_session_frame(
        &mut self,
        bytes: &[u8],
    ) -> Result<boon_runtime::DistributedClientUpdate, boon_runtime::DistributedRuntimeError>;

    fn next_session_frame(
        &mut self,
    ) -> Result<Option<Vec<u8>>, boon_runtime::DistributedRuntimeError>;

    fn acknowledge_session_frame(&mut self) -> bool;

    fn pending_session_frames(&self) -> usize;

    fn applied_server_through(&self) -> u64;
}

impl DistributedSessionClientRuntime for boon_runtime::DistributedClientRuntime {
    fn bind(
        &mut self,
        session_id: SessionId,
        generation: u64,
        applied_client_through: u64,
    ) -> Result<boon_runtime::DistributedClientUpdate, boon_runtime::DistributedRuntimeError> {
        boon_runtime::DistributedClientRuntime::bind(
            self,
            session_id,
            generation,
            applied_client_through,
        )
    }

    fn mark_current(
        &mut self,
    ) -> Result<boon_runtime::DistributedClientUpdate, boon_runtime::DistributedRuntimeError> {
        boon_runtime::DistributedClientRuntime::mark_current(self)
    }

    fn mark_stale(
        &mut self,
    ) -> Result<boon_runtime::DistributedClientUpdate, boon_runtime::DistributedRuntimeError> {
        boon_runtime::DistributedClientRuntime::mark_stale(self)
    }

    fn accept_session_frame(
        &mut self,
        bytes: &[u8],
    ) -> Result<boon_runtime::DistributedClientUpdate, boon_runtime::DistributedRuntimeError> {
        boon_runtime::DistributedClientRuntime::accept_session_frame(self, bytes)
    }

    fn next_session_frame(
        &mut self,
    ) -> Result<Option<Vec<u8>>, boon_runtime::DistributedRuntimeError> {
        boon_runtime::DistributedClientRuntime::next_session_frame(self)
    }

    fn acknowledge_session_frame(&mut self) -> bool {
        boon_runtime::DistributedClientRuntime::acknowledge_session_frame(self)
    }

    fn pending_session_frames(&self) -> usize {
        boon_runtime::DistributedClientRuntime::pending_session_frames(self)
    }

    fn applied_server_through(&self) -> u64 {
        boon_runtime::DistributedClientRuntime::applied_server_through(self)
    }
}

struct QueuedBinaryFrame {
    socket_epoch: u64,
    bytes: Vec<u8>,
}

struct BoundedInboundFrames {
    frames: VecDeque<QueuedBinaryFrame>,
    bytes: usize,
    max_frame_bytes: usize,
    max_messages: usize,
    max_bytes: usize,
}

impl BoundedInboundFrames {
    fn new(limits: DistributedSessionSocketLimits) -> Self {
        Self {
            frames: VecDeque::new(),
            bytes: 0,
            max_frame_bytes: limits.max_frame_bytes,
            max_messages: limits.max_inbound_messages,
            max_bytes: limits.max_inbound_bytes,
        }
    }

    fn push(
        &mut self,
        socket_epoch: u64,
        bytes: Vec<u8>,
    ) -> Result<(), DistributedSessionSocketError> {
        if bytes.len() > self.max_frame_bytes {
            return Err(DistributedSessionSocketError::FrameTooLarge {
                direction: DistributedSessionSocketDirection::Inbound,
                actual: bytes.len(),
                maximum: self.max_frame_bytes,
            });
        }
        let next_bytes = self.bytes.checked_add(bytes.len());
        if self.frames.len() >= self.max_messages
            || next_bytes.is_none_or(|next| next > self.max_bytes)
        {
            return Err(DistributedSessionSocketError::QueueFull {
                direction: DistributedSessionSocketDirection::Inbound,
                max_messages: self.max_messages,
                max_bytes: self.max_bytes,
            });
        }
        self.bytes = next_bytes.expect("checked inbound queue length");
        self.frames.push_back(QueuedBinaryFrame {
            socket_epoch,
            bytes,
        });
        Ok(())
    }

    fn front(&self) -> Option<&QueuedBinaryFrame> {
        self.frames.front()
    }

    fn pop_front(&mut self) -> bool {
        let Some(frame) = self.frames.pop_front() else {
            return false;
        };
        self.bytes = self
            .bytes
            .checked_sub(frame.bytes.len())
            .expect("inbound queue byte accounting is exact");
        true
    }

    fn clear(&mut self) {
        self.frames.clear();
        self.bytes = 0;
    }

    fn len(&self) -> usize {
        self.frames.len()
    }

    fn byte_len(&self) -> usize {
        self.bytes
    }
}

struct QueuedOutboundFrame {
    lease_id: u64,
    socket_epoch: u64,
    bytes: Vec<u8>,
    runtime_frame: bool,
}

struct BoundedOutboundFrames {
    frames: VecDeque<QueuedOutboundFrame>,
    bytes: usize,
    max_frame_bytes: usize,
    max_messages: usize,
    max_bytes: usize,
    next_lease_id: u64,
}

impl BoundedOutboundFrames {
    fn new(limits: DistributedSessionSocketLimits) -> Self {
        Self {
            frames: VecDeque::new(),
            bytes: 0,
            max_frame_bytes: limits.max_frame_bytes,
            max_messages: limits.max_outbound_messages,
            max_bytes: limits.max_outbound_bytes,
            next_lease_id: 0,
        }
    }

    fn can_admit(&self, bytes: usize) -> bool {
        bytes <= self.max_frame_bytes
            && self.frames.len() < self.max_messages
            && self
                .bytes
                .checked_add(bytes)
                .is_some_and(|next| next <= self.max_bytes)
    }

    fn can_admit_maximum_frame(&self) -> bool {
        self.can_admit(self.max_frame_bytes)
    }

    fn push(
        &mut self,
        socket_epoch: u64,
        bytes: Vec<u8>,
        runtime_frame: bool,
    ) -> Result<(), DistributedSessionSocketError> {
        if bytes.len() > self.max_frame_bytes {
            return Err(DistributedSessionSocketError::FrameTooLarge {
                direction: DistributedSessionSocketDirection::Outbound,
                actual: bytes.len(),
                maximum: self.max_frame_bytes,
            });
        }
        if !self.can_admit(bytes.len()) {
            return Err(DistributedSessionSocketError::QueueFull {
                direction: DistributedSessionSocketDirection::Outbound,
                max_messages: self.max_messages,
                max_bytes: self.max_bytes,
            });
        }
        let lease_id = self
            .next_lease_id
            .checked_add(1)
            .ok_or(DistributedSessionSocketError::OutboundLeaseExhausted)?;
        self.next_lease_id = lease_id;
        self.bytes += bytes.len();
        self.frames.push_back(QueuedOutboundFrame {
            lease_id,
            socket_epoch,
            bytes,
            runtime_frame,
        });
        Ok(())
    }

    fn front(&self) -> Option<&QueuedOutboundFrame> {
        self.frames.front()
    }

    fn acknowledge(
        &mut self,
        socket_epoch: u64,
        lease_id: u64,
    ) -> Result<(), DistributedSessionSocketError> {
        let Some(front) = self.frames.front() else {
            return Err(DistributedSessionSocketError::InvalidOutboundLease);
        };
        if front.socket_epoch != socket_epoch || front.lease_id != lease_id {
            return Err(DistributedSessionSocketError::InvalidOutboundLease);
        }
        let frame = self
            .frames
            .pop_front()
            .expect("outbound queue front was present");
        self.bytes = self
            .bytes
            .checked_sub(frame.bytes.len())
            .expect("outbound queue byte accounting is exact");
        Ok(())
    }

    fn validate_acknowledgement(
        &self,
        socket_epoch: u64,
        lease_id: u64,
    ) -> Result<bool, DistributedSessionSocketError> {
        let Some(front) = self.frames.front() else {
            return Err(DistributedSessionSocketError::InvalidOutboundLease);
        };
        if front.socket_epoch != socket_epoch || front.lease_id != lease_id {
            return Err(DistributedSessionSocketError::InvalidOutboundLease);
        }
        Ok(front.runtime_frame)
    }

    fn clear(&mut self) {
        self.frames.clear();
        self.bytes = 0;
    }

    fn len(&self) -> usize {
        self.frames.len()
    }

    fn byte_len(&self) -> usize {
        self.bytes
    }

    fn runtime_frame_count(&self) -> usize {
        self.frames
            .iter()
            .filter(|frame| frame.runtime_frame)
            .count()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributedSessionSocketAdmission {
    Accepted,
    IgnoredStaleSocket,
}

#[derive(Debug, Default)]
pub struct DistributedSessionSocketPoll {
    pub runtime_updates: Vec<boon_runtime::DistributedClientUpdate>,
}

#[derive(Debug)]
pub struct DistributedSessionSocketDisconnect {
    pub admission: DistributedSessionSocketAdmission,
    pub runtime_update: Option<boon_runtime::DistributedClientUpdate>,
}

/// Borrowed front frame of the browser outbound queue.
///
/// Dropping this value does not remove the frame. Call
/// [`DistributedSessionSocketOwner::acknowledge_outbound`] only after the
/// browser accepted the bytes through `WebSocket.send`.
pub struct DistributedSessionOutboundLease<'a> {
    socket_epoch: u64,
    lease_id: u64,
    bytes: &'a [u8],
}

impl DistributedSessionOutboundLease<'_> {
    pub fn socket_epoch(&self) -> u64 {
        self.socket_epoch
    }

    pub fn lease_id(&self) -> u64 {
        self.lease_id
    }

    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
}

enum DistributedSessionSocketLifecycle {
    Idle,
    Connecting { socket_epoch: u64 },
    Open { socket_epoch: u64 },
    ReconnectRequired,
    Rejected,
    Revoked,
    Closed,
    Failed,
}

/// Platform-neutral owner of one browser distributed-Session connection.
///
/// Every browser callback carries the socket epoch returned by
/// [`Self::begin_connect`]. Events from replaced sockets are ignored before
/// they can reach the handshake or runtime. Inbound frames and outbound frames
/// awaiting browser admission are separately bounded.
pub struct DistributedSessionSocketOwner<S, R = boon_runtime::DistributedClientRuntime> {
    handshake: Option<DistributedSessionHandshake<S>>,
    pending_hello: Option<Vec<u8>>,
    restart_handshake: bool,
    runtime: R,
    runtime_generation: Option<u64>,
    lifecycle: DistributedSessionSocketLifecycle,
    last_socket_epoch: u64,
    limits: DistributedSessionSocketLimits,
    inbound: BoundedInboundFrames,
    outbound: BoundedOutboundFrames,
}

impl<S, R> DistributedSessionSocketOwner<S, R>
where
    S: DistributedSessionJournalStore,
    R: DistributedSessionClientRuntime,
{
    pub fn new(
        identity: DistributedSessionIdentity,
        storage: S,
        runtime: R,
        limits: DistributedSessionSocketLimits,
    ) -> Result<Self, DistributedSessionSocketError> {
        let limits = limits.validate()?;
        let applied_server_through = runtime.applied_server_through();
        let (handshake, hello) =
            DistributedSessionHandshake::start(identity, storage, applied_server_through)?;
        Ok(Self {
            handshake: Some(handshake),
            pending_hello: Some(hello),
            restart_handshake: false,
            runtime,
            runtime_generation: None,
            lifecycle: DistributedSessionSocketLifecycle::Idle,
            last_socket_epoch: 0,
            limits,
            inbound: BoundedInboundFrames::new(limits),
            outbound: BoundedOutboundFrames::new(limits),
        })
    }

    pub fn phase(&self) -> DistributedSessionSocketPhase {
        match self.lifecycle {
            DistributedSessionSocketLifecycle::Idle => DistributedSessionSocketPhase::Idle,
            DistributedSessionSocketLifecycle::Connecting { .. } => {
                DistributedSessionSocketPhase::Connecting
            }
            DistributedSessionSocketLifecycle::Open { .. } => self
                .handshake
                .as_ref()
                .map(|handshake| match handshake.phase() {
                    DistributedSessionHandshakePhase::AwaitingOffer => {
                        DistributedSessionSocketPhase::AwaitingOffer
                    }
                    DistributedSessionHandshakePhase::AwaitingReady => {
                        DistributedSessionSocketPhase::AwaitingReady
                    }
                    DistributedSessionHandshakePhase::Current => {
                        DistributedSessionSocketPhase::Current
                    }
                    DistributedSessionHandshakePhase::AwaitingRevoke => {
                        DistributedSessionSocketPhase::AwaitingRevoke
                    }
                    DistributedSessionHandshakePhase::Rejected => {
                        DistributedSessionSocketPhase::Rejected
                    }
                    DistributedSessionHandshakePhase::Revoked => {
                        DistributedSessionSocketPhase::Revoked
                    }
                    DistributedSessionHandshakePhase::Failed => {
                        DistributedSessionSocketPhase::Failed
                    }
                })
                .unwrap_or(DistributedSessionSocketPhase::Failed),
            DistributedSessionSocketLifecycle::ReconnectRequired => {
                DistributedSessionSocketPhase::ReconnectRequired
            }
            DistributedSessionSocketLifecycle::Rejected => DistributedSessionSocketPhase::Rejected,
            DistributedSessionSocketLifecycle::Revoked => DistributedSessionSocketPhase::Revoked,
            DistributedSessionSocketLifecycle::Closed => DistributedSessionSocketPhase::Closed,
            DistributedSessionSocketLifecycle::Failed => DistributedSessionSocketPhase::Failed,
        }
    }

    pub fn begin_connect(&mut self) -> Result<u64, DistributedSessionSocketError> {
        if !matches!(
            self.lifecycle,
            DistributedSessionSocketLifecycle::Idle
                | DistributedSessionSocketLifecycle::ReconnectRequired
        ) {
            return Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                self.phase(),
            ));
        }
        if self.restart_handshake {
            let handshake = self.handshake.take().ok_or(
                DistributedSessionSocketError::UnexpectedSocketEvent(
                    DistributedSessionSocketPhase::Failed,
                ),
            )?;
            let (handshake, hello) = match handshake.restart(self.runtime.applied_server_through())
            {
                Ok(restarted) => restarted,
                Err(error) => {
                    self.lifecycle = DistributedSessionSocketLifecycle::Failed;
                    return Err(error.into());
                }
            };
            self.handshake = Some(handshake);
            self.pending_hello = Some(hello);
            self.restart_handshake = false;
        }
        let socket_epoch = self
            .last_socket_epoch
            .checked_add(1)
            .ok_or(DistributedSessionSocketError::SocketEpochExhausted)?;
        self.last_socket_epoch = socket_epoch;
        self.lifecycle = DistributedSessionSocketLifecycle::Connecting { socket_epoch };
        Ok(socket_epoch)
    }

    pub fn socket_opened(
        &mut self,
        socket_epoch: u64,
    ) -> Result<DistributedSessionSocketAdmission, DistributedSessionSocketError> {
        let DistributedSessionSocketLifecycle::Connecting {
            socket_epoch: active_epoch,
        } = self.lifecycle
        else {
            return if self.is_stale_socket(socket_epoch) {
                Ok(DistributedSessionSocketAdmission::IgnoredStaleSocket)
            } else {
                Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                    self.phase(),
                ))
            };
        };
        if socket_epoch != active_epoch {
            return Ok(DistributedSessionSocketAdmission::IgnoredStaleSocket);
        }
        let hello = self.pending_hello.take().ok_or(
            DistributedSessionSocketError::UnexpectedSocketEvent(self.phase()),
        )?;
        self.outbound.push(socket_epoch, hello, false)?;
        self.lifecycle = DistributedSessionSocketLifecycle::Open { socket_epoch };
        Ok(DistributedSessionSocketAdmission::Accepted)
    }

    pub fn socket_connect_failed(
        &mut self,
        socket_epoch: u64,
    ) -> Result<DistributedSessionSocketDisconnect, DistributedSessionSocketError> {
        let DistributedSessionSocketLifecycle::Connecting {
            socket_epoch: active_epoch,
        } = self.lifecycle
        else {
            return if self.is_stale_socket(socket_epoch) {
                Ok(DistributedSessionSocketDisconnect {
                    admission: DistributedSessionSocketAdmission::IgnoredStaleSocket,
                    runtime_update: None,
                })
            } else {
                Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                    self.phase(),
                ))
            };
        };
        if socket_epoch != active_epoch {
            return Ok(DistributedSessionSocketDisconnect {
                admission: DistributedSessionSocketAdmission::IgnoredStaleSocket,
                runtime_update: None,
            });
        }
        self.lifecycle = DistributedSessionSocketLifecycle::ReconnectRequired;
        Ok(DistributedSessionSocketDisconnect {
            admission: DistributedSessionSocketAdmission::Accepted,
            runtime_update: None,
        })
    }

    pub fn push_inbound_binary(
        &mut self,
        socket_epoch: u64,
        bytes: Vec<u8>,
    ) -> Result<DistributedSessionSocketAdmission, DistributedSessionSocketError> {
        if !self.is_open_socket(socket_epoch) {
            return if self.is_stale_socket(socket_epoch) {
                Ok(DistributedSessionSocketAdmission::IgnoredStaleSocket)
            } else {
                Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                    self.phase(),
                ))
            };
        }
        self.inbound.push(socket_epoch, bytes)?;
        Ok(DistributedSessionSocketAdmission::Accepted)
    }

    pub fn reject_text_frame(
        &self,
        socket_epoch: u64,
    ) -> Result<DistributedSessionSocketAdmission, DistributedSessionSocketError> {
        if self.is_open_socket(socket_epoch) {
            Err(DistributedSessionSocketError::TextFrame)
        } else if self.is_stale_socket(socket_epoch) {
            Ok(DistributedSessionSocketAdmission::IgnoredStaleSocket)
        } else {
            Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                self.phase(),
            ))
        }
    }

    /// Processes at most one queued binary frame.
    ///
    /// `Ok(None)` means either the inbound queue is empty or processing is
    /// applying backpressure until an outbound control frame is acknowledged.
    pub fn poll_inbound(
        &mut self,
    ) -> Result<Option<DistributedSessionSocketPoll>, DistributedSessionSocketError> {
        let Some(frame) = self.inbound.front() else {
            return Ok(None);
        };
        let socket_epoch = frame.socket_epoch;
        if !self.is_open_socket(socket_epoch) {
            self.inbound.pop_front();
            return Ok(Some(DistributedSessionSocketPoll::default()));
        }

        let phase = self
            .handshake
            .as_ref()
            .map(DistributedSessionHandshake::phase)
            .ok_or(DistributedSessionSocketError::UnexpectedSocketEvent(
                DistributedSessionSocketPhase::Failed,
            ))?;
        let mut runtime_updates = Vec::new();
        match phase {
            DistributedSessionHandshakePhase::AwaitingOffer
            | DistributedSessionHandshakePhase::AwaitingReady
            | DistributedSessionHandshakePhase::AwaitingRevoke => {
                if !self.outbound.can_admit(SESSION_CONTROL_MAX_FRAME_BYTES) {
                    return Ok(None);
                }
                let step = self
                    .handshake
                    .as_mut()
                    .expect("open socket owns a handshake")
                    .accept_server_frame(&frame.bytes)?;
                match step {
                    DistributedSessionHandshakeStep::SendClientFrame(bytes) => {
                        self.outbound.push(socket_epoch, bytes, false)?;
                    }
                    DistributedSessionHandshakeStep::Current => {
                        let (session_id, generation, applied_client_through) = self
                            .handshake
                            .as_ref()
                            .and_then(DistributedSessionHandshake::binding)
                            .ok_or(DistributedSessionSocketError::UnexpectedSocketEvent(
                                DistributedSessionSocketPhase::Failed,
                            ))?;
                        runtime_updates.push(self.runtime.bind(
                            session_id,
                            generation,
                            applied_client_through,
                        )?);
                        self.runtime_generation = Some(generation);
                        runtime_updates.push(self.runtime.mark_current()?);
                    }
                    DistributedSessionHandshakeStep::Rejected => {
                        self.lifecycle = DistributedSessionSocketLifecycle::Rejected;
                    }
                    DistributedSessionHandshakeStep::Revoked => {
                        if self.runtime_generation.is_some() {
                            runtime_updates.push(self.runtime.mark_stale()?);
                            self.runtime_generation = None;
                        }
                        self.lifecycle = DistributedSessionSocketLifecycle::Revoked;
                    }
                }
            }
            DistributedSessionHandshakePhase::Current => {
                runtime_updates.push(self.runtime.accept_session_frame(&frame.bytes)?);
            }
            DistributedSessionHandshakePhase::Rejected
            | DistributedSessionHandshakePhase::Revoked
            | DistributedSessionHandshakePhase::Failed => {
                return Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                    self.phase(),
                ));
            }
        }
        self.inbound.pop_front();
        if matches!(
            self.lifecycle,
            DistributedSessionSocketLifecycle::Rejected
                | DistributedSessionSocketLifecycle::Revoked
        ) {
            self.inbound.clear();
            self.outbound.clear();
        }
        Ok(Some(DistributedSessionSocketPoll { runtime_updates }))
    }

    pub fn revoke(&mut self) -> Result<(), DistributedSessionSocketError> {
        let socket_epoch = self.open_socket_epoch().ok_or(
            DistributedSessionSocketError::UnexpectedSocketEvent(self.phase()),
        )?;
        if !self.outbound.can_admit(SESSION_CONTROL_MAX_FRAME_BYTES) {
            return Err(DistributedSessionSocketError::QueueFull {
                direction: DistributedSessionSocketDirection::Outbound,
                max_messages: self.limits.max_outbound_messages,
                max_bytes: self.limits.max_outbound_bytes,
            });
        }
        let bytes = self
            .handshake
            .as_mut()
            .ok_or(DistributedSessionSocketError::UnexpectedSocketEvent(
                DistributedSessionSocketPhase::Failed,
            ))?
            .revoke()?;
        self.outbound.push(socket_epoch, bytes, false)?;
        Ok(())
    }

    /// Leases the exact front frame without removing it from browser ownership.
    ///
    /// If the control queue is empty while the runtime is current, one runtime
    /// frame is moved into the bounded browser queue. The moved bytes remain at
    /// the front across failed sends and are returned unchanged on the next call.
    pub fn lease_outbound(
        &mut self,
        socket_epoch: u64,
    ) -> Result<Option<DistributedSessionOutboundLease<'_>>, DistributedSessionSocketError> {
        if !self.is_open_socket(socket_epoch) {
            return if self.is_stale_socket(socket_epoch) {
                Ok(None)
            } else {
                Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                    self.phase(),
                ))
            };
        }
        if self.outbound.front().is_none()
            && self.runtime_generation.is_some()
            && self.runtime.pending_session_frames() != 0
            && self.outbound.can_admit_maximum_frame()
            && let Some(bytes) = self.runtime.next_session_frame()?
        {
            self.outbound.push(socket_epoch, bytes, true)?;
        }
        Ok(self
            .outbound
            .front()
            .map(|frame| DistributedSessionOutboundLease {
                socket_epoch: frame.socket_epoch,
                lease_id: frame.lease_id,
                bytes: &frame.bytes,
            }))
    }

    pub fn acknowledge_outbound(
        &mut self,
        socket_epoch: u64,
        lease_id: u64,
    ) -> Result<DistributedSessionSocketAdmission, DistributedSessionSocketError> {
        if !self.is_open_socket(socket_epoch) {
            return if self.is_stale_socket(socket_epoch) {
                Ok(DistributedSessionSocketAdmission::IgnoredStaleSocket)
            } else {
                Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                    self.phase(),
                ))
            };
        }
        let runtime_frame = self
            .outbound
            .validate_acknowledgement(socket_epoch, lease_id)?;
        if runtime_frame && !self.runtime.acknowledge_session_frame() {
            return Err(DistributedSessionSocketError::InvalidOutboundLease);
        }
        self.outbound.acknowledge(socket_epoch, lease_id)?;
        Ok(DistributedSessionSocketAdmission::Accepted)
    }

    pub fn socket_disconnected(
        &mut self,
        socket_epoch: u64,
    ) -> Result<DistributedSessionSocketDisconnect, DistributedSessionSocketError> {
        self.finish_socket(
            socket_epoch,
            DistributedSessionSocketLifecycle::ReconnectRequired,
        )
    }

    pub fn abort_socket(
        &mut self,
        socket_epoch: u64,
    ) -> Result<DistributedSessionSocketDisconnect, DistributedSessionSocketError> {
        self.finish_socket(socket_epoch, DistributedSessionSocketLifecycle::Failed)
    }

    pub fn close(
        &mut self,
        socket_epoch: u64,
    ) -> Result<DistributedSessionSocketDisconnect, DistributedSessionSocketError> {
        self.finish_socket(socket_epoch, DistributedSessionSocketLifecycle::Closed)
    }

    pub fn active_socket_epoch(&self) -> Option<u64> {
        match self.lifecycle {
            DistributedSessionSocketLifecycle::Connecting { socket_epoch }
            | DistributedSessionSocketLifecycle::Open { socket_epoch } => Some(socket_epoch),
            _ => None,
        }
    }

    pub fn runtime_generation(&self) -> Option<u64> {
        self.runtime_generation
    }

    pub fn pending_inbound_frames(&self) -> usize {
        self.inbound.len()
    }

    pub fn pending_inbound_bytes(&self) -> usize {
        self.inbound.byte_len()
    }

    pub fn pending_outbound_frames(&self) -> usize {
        let runtime_frames_in_browser_queue = self.outbound.runtime_frame_count();
        let runtime_frames_not_yet_leased = self
            .runtime
            .pending_session_frames()
            .checked_sub(runtime_frames_in_browser_queue)
            .expect("a browser-owned runtime frame remains leased by the runtime");
        self.outbound
            .len()
            .checked_add(runtime_frames_not_yet_leased)
            .expect("bounded outbound frame counts cannot overflow")
    }

    pub fn pending_browser_outbound_frames(&self) -> usize {
        self.outbound.len()
    }

    pub fn pending_browser_outbound_bytes(&self) -> usize {
        self.outbound.byte_len()
    }

    pub fn runtime(&self) -> &R {
        &self.runtime
    }

    fn finish_socket(
        &mut self,
        socket_epoch: u64,
        next_lifecycle: DistributedSessionSocketLifecycle,
    ) -> Result<DistributedSessionSocketDisconnect, DistributedSessionSocketError> {
        let Some(active_epoch) = self.active_socket_epoch() else {
            return if self.is_stale_socket(socket_epoch) {
                Ok(DistributedSessionSocketDisconnect {
                    admission: DistributedSessionSocketAdmission::IgnoredStaleSocket,
                    runtime_update: None,
                })
            } else {
                Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                    self.phase(),
                ))
            };
        };
        if active_epoch != socket_epoch {
            return Ok(DistributedSessionSocketDisconnect {
                admission: DistributedSessionSocketAdmission::IgnoredStaleSocket,
                runtime_update: None,
            });
        }
        let was_open = matches!(
            self.lifecycle,
            DistributedSessionSocketLifecycle::Open { .. }
        );
        self.inbound.clear();
        self.outbound.clear();
        let runtime_update = if self.runtime_generation.is_some() {
            match self.runtime.mark_stale() {
                Ok(update) => {
                    self.runtime_generation = None;
                    Some(update)
                }
                Err(error) => {
                    self.lifecycle = DistributedSessionSocketLifecycle::Failed;
                    return Err(error.into());
                }
            }
        } else {
            None
        };
        if was_open {
            self.restart_handshake = true;
            self.pending_hello = None;
        }
        self.lifecycle = next_lifecycle;
        Ok(DistributedSessionSocketDisconnect {
            admission: DistributedSessionSocketAdmission::Accepted,
            runtime_update,
        })
    }

    fn is_open_socket(&self, socket_epoch: u64) -> bool {
        matches!(
            self.lifecycle,
            DistributedSessionSocketLifecycle::Open {
                socket_epoch: active_epoch
            } if active_epoch == socket_epoch
        )
    }

    fn open_socket_epoch(&self) -> Option<u64> {
        match self.lifecycle {
            DistributedSessionSocketLifecycle::Open { socket_epoch } => Some(socket_epoch),
            _ => None,
        }
    }

    fn is_stale_socket(&self, socket_epoch: u64) -> bool {
        if socket_epoch == 0 {
            return false;
        }
        self.active_socket_epoch()
            .map_or(socket_epoch <= self.last_socket_epoch, |active_epoch| {
                socket_epoch < active_epoch
            })
    }
}

impl<S> DistributedSessionSocketOwner<S, boon_runtime::DistributedClientRuntime>
where
    S: DistributedSessionJournalStore,
{
    pub fn dispatch(
        &mut self,
        path: &str,
        payload: boon_runtime::SourcePayload,
    ) -> Result<boon_runtime::DistributedClientUpdate, DistributedSessionSocketError> {
        self.require_current()?;
        Ok(self.runtime.dispatch(path, payload)?)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        outcome: boon_runtime::Value,
    ) -> Result<boon_runtime::DistributedClientUpdate, DistributedSessionSocketError> {
        self.require_current()?;
        Ok(self.runtime.complete_transient_effect(call_id, outcome)?)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        result_sequence: u64,
        outcome: boon_runtime::Value,
    ) -> Result<boon_runtime::DistributedClientUpdate, DistributedSessionSocketError> {
        self.require_current()?;
        Ok(self
            .runtime
            .deliver_transient_effect_result(call_id, result_sequence, outcome)?)
    }

    pub fn cancel_all_transient_effects(
        &mut self,
    ) -> Result<boon_runtime::DistributedClientUpdate, DistributedSessionSocketError> {
        Ok(self.runtime.cancel_all_transient_effects()?)
    }

    pub fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<boon_runtime::Value, DistributedSessionSocketError> {
        Ok(self.runtime.root_value_current(name)?)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.runtime.pending_transient_effect_count()
    }

    pub fn document_frame(&self) -> Option<&boon_runtime::DocumentFrame> {
        self.runtime.document_frame()
    }

    fn require_current(&self) -> Result<(), DistributedSessionSocketError> {
        if self.phase() != DistributedSessionSocketPhase::Current {
            return Err(DistributedSessionSocketError::UnexpectedSocketEvent(
                self.phase(),
            ));
        }
        Ok(())
    }
}
