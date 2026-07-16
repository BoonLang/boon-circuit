use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Header {
    pub name: String,
    pub value: Vec<u8>,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeerAddress {
    Known(SocketAddr),
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestScheme {
    Http,
    Https,
}

impl RequestScheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CookieMetadata {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path_segments: Vec<String>,
    pub query: BTreeMap<String, Vec<String>>,
    pub headers: Vec<Header>,
    pub cookies: Vec<CookieMetadata>,
    pub body: Vec<u8>,
    pub peer: PeerAddress,
    pub scheme: RequestScheme,
    pub deadline: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct WebSocketOpen {
    pub path_segments: Vec<String>,
    pub query: BTreeMap<String, Vec<String>>,
    pub headers: Vec<Header>,
    pub cookies: Vec<CookieMetadata>,
    pub peer: PeerAddress,
    pub scheme: RequestScheme,
    pub deadline: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSocketClose {
    pub code: u16,
    pub reason: String,
}

impl WebSocketClose {
    pub fn new(code: u16, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebSocketTransportError {
    MessageTooLarge,
    InvalidMessage,
    Io,
    ProgramTimeout,
    AdmissionOverloaded,
}

#[derive(Clone, Debug)]
pub enum WebSocketEvent {
    Open(WebSocketOpen),
    Text(String),
    Binary(Vec<u8>),
    Close(Option<WebSocketClose>),
    TransportError(WebSocketTransportError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebSocketFrame {
    Text(String),
    Binary(Vec<u8>),
}

impl WebSocketFrame {
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Binary(bytes) => bytes.len(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebSocketAction {
    Accept,
    Reject(HttpResponse),
    Reply(WebSocketFrame),
    Send(WebSocketFrame),
    JoinRoom {
        room: String,
    },
    LeaveRoom {
        room: String,
    },
    Broadcast {
        room: String,
        frame: WebSocketFrame,
        include_current: bool,
    },
    RequestResync {
        frame: WebSocketFrame,
    },
    Close(WebSocketClose),
}
