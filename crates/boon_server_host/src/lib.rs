//! Bounded native HTTP and WebSocket transport for serialized server programs.
//!
//! Request and socket correlation stay private to this crate. A program can
//! only act on the HTTP request or WebSocket connection for its current call.

mod config;
mod host;
mod program;
mod types;

pub use config::{
    ConfigError, OriginPolicy, ServerConfig, ServerLimits, ServerTimeouts, SlowClientPolicy,
    TrustedProxyPolicy,
};
pub use host::{RunningServer, ServerError, ShutdownError, bind};
pub use program::{CallCancellation, CancellationReason, ServerProgram};
pub use types::{
    CookieMetadata, Header, HttpRequest, HttpResponse, PeerAddress, RequestScheme, WebSocketAction,
    WebSocketClose, WebSocketEvent, WebSocketFrame, WebSocketOpen, WebSocketTransportError,
};
