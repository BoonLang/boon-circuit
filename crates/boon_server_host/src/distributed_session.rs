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
#[derive(Clone)]
pub struct DistributedSessionOpen {
    pub headers: Vec<Header>,
    pub cookies: Vec<CookieMetadata>,
    pub peer: PeerAddress,
    pub scheme: RequestScheme,
    pub deadline: Instant,
}

impl Debug for DistributedSessionOpen {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DistributedSessionOpen")
            .field("headers_len", &self.headers.len())
            .field("cookies_len", &self.cookies.len())
            .finish_non_exhaustive()
    }
}

/// A lifecycle event from the dedicated distributed-Session transport lane.
#[derive(Clone)]
pub enum DistributedSessionEvent {
    Open(DistributedSessionOpen),
    Binary(Vec<u8>),
    Close(Option<WebSocketClose>),
}

impl Debug for DistributedSessionEvent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(open) => formatter.debug_tuple("Open").field(open).finish(),
            Self::Binary(bytes) => formatter
                .debug_struct("Binary")
                .field("bytes_len", &bytes.len())
                .finish(),
            Self::Close(Some(_)) => formatter.write_str("Close(Some(..))"),
            Self::Close(None) => formatter.write_str("Close(None)"),
        }
    }
}

/// A transport action targeting any currently live distributed connection.
#[derive(Clone, Eq, PartialEq)]
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

impl Debug for DistributedSessionAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Send { connection, bytes } => formatter
                .debug_struct("Send")
                .field("connection", connection)
                .field("bytes_len", &bytes.len())
                .finish(),
            Self::Close { connection, .. } => formatter
                .debug_struct("Close")
                .field("connection", connection)
                .finish_non_exhaustive(),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    const HEADER_NAME_SENTINEL: &str = "header-name-secret-sentinel";
    const HEADER_VALUE_SENTINEL: &[u8] = &[222, 173, 190, 239, 0, 31];
    const COOKIE_NAME_SENTINEL: &str = "cookie-name-secret-sentinel";
    const COOKIE_VALUE_SENTINEL: &str = "cookie-value-secret-sentinel";
    const BINARY_SENTINEL: &[u8] = &[19, 37, 251, 0, 128, 7];
    const CLOSE_SENTINEL: &str = "close-reason-secret-sentinel";
    const CONNECTION_SENTINEL: u64 = 8_675_309;

    fn assert_absent(diagnostic: &str) {
        for secret in [
            HEADER_NAME_SENTINEL,
            "[222, 173, 190, 239, 0, 31]",
            COOKIE_NAME_SENTINEL,
            COOKIE_VALUE_SENTINEL,
            "[19, 37, 251, 0, 128, 7]",
            CLOSE_SENTINEL,
            "8675309",
            "203.0.113.79",
            "43127",
        ] {
            assert!(
                !diagnostic.contains(secret),
                "diagnostic leaked sentinel `{secret}`: {diagnostic}"
            );
        }
    }

    #[test]
    fn distributed_session_diagnostics_redact_transport_secrets() {
        let open = DistributedSessionOpen {
            headers: vec![Header::new(HEADER_NAME_SENTINEL, HEADER_VALUE_SENTINEL)],
            cookies: vec![CookieMetadata {
                name: COOKIE_NAME_SENTINEL.to_owned(),
                value: COOKIE_VALUE_SENTINEL.to_owned(),
            }],
            peer: PeerAddress::Known(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(203, 0, 113, 79)),
                43_127,
            )),
            scheme: RequestScheme::Https,
            deadline: Instant::now() + Duration::from_secs(30),
        };
        let connection = DistributedSessionConnectionId(CONNECTION_SENTINEL);
        let diagnostics = [
            format!("{open:?}"),
            format!("{:?}", DistributedSessionEvent::Open(open)),
            format!(
                "{:?}",
                DistributedSessionEvent::Binary(BINARY_SENTINEL.to_vec())
            ),
            format!(
                "{:?}",
                DistributedSessionEvent::Close(Some(WebSocketClose::new(4_001, CLOSE_SENTINEL)))
            ),
            format!(
                "{:?}",
                DistributedSessionAction::send(connection, BINARY_SENTINEL)
            ),
            format!(
                "{:?}",
                DistributedSessionAction::close(
                    connection,
                    WebSocketClose::new(4_001, CLOSE_SENTINEL)
                )
            ),
        ];

        for diagnostic in &diagnostics {
            assert_absent(diagnostic);
        }
        assert!(diagnostics[0].contains("headers_len: 1"));
        assert!(diagnostics[0].contains("cookies_len: 1"));
        assert!(diagnostics[2].contains(&format!("bytes_len: {}", BINARY_SENTINEL.len())));
        assert!(diagnostics[3].contains("Close(Some(..))"));
        assert!(diagnostics[4].contains("DistributedSessionConnectionId(..)"));
    }
}
