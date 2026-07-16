//! Capability-bounded outbound HTTP for native Boon hosts.
//!
//! Callers select a preconfigured endpoint by name and provide path segments;
//! they never provide an absolute request destination. Redirects, DNS results,
//! response data, concurrency, and time are all checked against explicit
//! bounds. The public values intentionally contain no `reqwest` types so they
//! can later be adapted to transient Boon effects without coupling the runtime
//! to the transport implementation.

mod capability;
mod client;
mod dns;
mod types;

pub use capability::{
    ClientConfig, ConfigError, EndpointCapability, HttpLimits, LocalHttpTestPermit, PoolConfig,
    RedirectPolicy, Timeouts,
};
pub use client::HttpClient;
pub use types::{
    CancellationToken, EndpointName, ExecuteError, Header, HttpMethod, HttpRequest, HttpResponse,
    LimitKind, QueryParameter, RequestTimeouts, RequestViolation, TimeoutKind,
};
