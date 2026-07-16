use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EndpointName(String);

impl EndpointName {
    pub fn new(name: impl Into<String>) -> Result<Self, crate::ConfigError> {
        let name = name.into();
        let valid = !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if !valid {
            return Err(crate::ConfigError::InvalidEndpointName);
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EndpointName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

#[derive(Clone, Eq, PartialEq)]
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

impl fmt::Debug for Header {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Header")
            .field("name", &self.name)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct QueryParameter {
    pub name: String,
    pub value: String,
}

impl fmt::Debug for QueryParameter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QueryParameter")
            .field("name", &self.name)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}

impl QueryParameter {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct HttpRequest {
    pub endpoint: EndpointName,
    pub method: HttpMethod,
    pub path_segments: Vec<String>,
    pub query: Vec<QueryParameter>,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestTimeouts {
    pub connect: Duration,
    pub overall: Duration,
}

impl HttpRequest {
    pub fn new(endpoint: EndpointName, method: HttpMethod) -> Self {
        Self {
            endpoint,
            method,
            path_segments: Vec::new(),
            query: Vec::new(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpRequest")
            .field("endpoint", &self.endpoint)
            .field("method", &self.method)
            .field("path_segments", &self.path_segments)
            .field("query", &self.query)
            .field("headers", &self.headers)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    /// Lowercase names, sorted by `(name, value)` for deterministic comparison.
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    pub final_endpoint: EndpointName,
    pub redirects_followed: u8,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body_bytes", &self.body.len())
            .field("final_endpoint", &self.final_endpoint)
            .field("redirects_followed", &self.redirects_followed)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct CancellationToken {
    inner: Arc<CancellationState>,
}

#[derive(Debug)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancellationState {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub(crate) async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitKind {
    RequestBodyBytes,
    RequestHeaderCount,
    RequestHeaderBytes,
    ResponseBodyBytes,
    ResponseHeaderCount,
    ResponseHeaderBytes,
    UrlBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeoutKind {
    Connect,
    Overall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestViolation {
    InvalidPathSegment,
    InvalidHeaderName,
    InvalidHeaderValue,
    ForbiddenHeader,
    BodyNotAllowedForMethod,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecuteError {
    Cancelled,
    OverallTimeout,
    InvalidTimeouts,
    TimeoutLimitExceeded { kind: TimeoutKind, limit_ms: u64 },
    UnknownEndpoint { endpoint: EndpointName },
    InvalidRequest(RequestViolation),
    LimitExceeded { kind: LimitKind, limit: usize },
    RedirectDenied,
    RedirectLimitExceeded { limit: u8 },
    InvalidRedirect,
    RedirectDestinationNotAllowed,
    RedirectCredentialLeakPrevented,
    DnsResolutionFailed { endpoint: EndpointName },
    AddressPolicyDenied { endpoint: EndpointName },
    ConnectTimeout { endpoint: EndpointName },
    ConnectFailed { endpoint: EndpointName },
    TransportFailed { endpoint: EndpointName },
}

impl fmt::Display for ExecuteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ExecuteError {}
