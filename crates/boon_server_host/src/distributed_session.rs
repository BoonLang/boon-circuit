use crate::{CookieMetadata, Header, PeerAddress, RequestScheme, WebSocketClose};
pub use boon_wire::DISTRIBUTED_SESSION_TRANSPORT_PATH;
use std::fmt::{self, Debug, Formatter};
use std::time::Instant;

/// Host-private identity for one live distributed-Session transport connection.
///
/// Programs can retain and compare handles received by [`crate::ServerProgram`]
/// callbacks, then return them in [`DistributedSessionAction`] values. There is
/// deliberately no public constructor, raw-value accessor, or identifying
/// `Debug` representation.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DistributedSessionConnectionId(u64);

impl DistributedSessionConnectionId {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

impl Debug for DistributedSessionConnectionId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("DistributedSessionConnectionId(..)")
    }
}

/// Request metadata for a connection admitted on the reserved transport path.
#[derive(Clone, Debug)]
pub struct DistributedSessionOpen {
    pub headers: Vec<Header>,
    pub cookies: Vec<CookieMetadata>,
    pub peer: PeerAddress,
    pub scheme: RequestScheme,
    pub deadline: Instant,
}

/// A lifecycle event from the dedicated distributed-Session transport lane.
#[derive(Clone, Debug)]
pub enum DistributedSessionEvent {
    Open(DistributedSessionOpen),
    Binary(Vec<u8>),
    Close(Option<WebSocketClose>),
}

/// A transport action targeting any currently live distributed connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DistributedSessionAction {
    Send {
        connection: DistributedSessionConnectionId,
        bytes: Vec<u8>,
    },
    Close {
        connection: DistributedSessionConnectionId,
        close: WebSocketClose,
    },
}

impl DistributedSessionAction {
    pub fn send(connection: DistributedSessionConnectionId, bytes: impl Into<Vec<u8>>) -> Self {
        Self::Send {
            connection,
            bytes: bytes.into(),
        }
    }

    pub fn close(connection: DistributedSessionConnectionId, close: WebSocketClose) -> Self {
        Self::Close { connection, close }
    }
}
