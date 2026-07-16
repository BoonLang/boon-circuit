use crate::WebHostError;
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const DEFAULT_MAX_HEADER_COUNT: usize = 64;
pub const DEFAULT_MAX_HEADER_BYTES: usize = 16 * 1024;
pub const DEFAULT_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_MAX_URL_BYTES: usize = 8 * 1024;
pub const DEFAULT_MAX_SOCKET_MESSAGE_BYTES: usize = 256 * 1024;
pub const DEFAULT_MAX_SOCKET_QUEUE_MESSAGES: usize = 64;
pub const DEFAULT_MAX_SOCKET_QUEUE_BYTES: usize = 1024 * 1024;
pub const MAX_BROWSER_TIMEOUT_MS: u32 = 300_000;

pub type WebHostResult<T> = Result<T, WebHostError>;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FetchMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl FetchMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HeaderValue {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserFetchOrigin {
    SameOrigin,
    Https {
        host: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
    },
}

impl BrowserFetchOrigin {
    fn validate(&self) -> WebHostResult<()> {
        let Self::Https { host, port } = self else {
            return Ok(());
        };
        if host.is_empty()
            || host.len() > 253
            || host.starts_with('.')
            || host.ends_with('.')
            || host.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return Err(invalid(
                "fetch HTTPS host",
                "must be a canonical ASCII DNS name",
            ));
        }
        if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
            return Err(invalid(
                "fetch HTTPS host",
                "localhost is not a production external endpoint",
            ));
        }
        if host.parse::<std::net::IpAddr>().is_ok() {
            return Err(invalid(
                "fetch HTTPS host",
                "literal IP addresses are not accepted by browser endpoint capabilities",
            ));
        }
        if port.is_some_and(|port| port == 0) {
            return Err(invalid("fetch HTTPS port", "must be non-zero"));
        }
        Ok(())
    }

    pub fn request_url(&self, path_and_query: &str) -> String {
        match self {
            Self::SameOrigin => path_and_query.to_owned(),
            Self::Https { host, port } => match port {
                Some(port) => format!("https://{host}:{port}{path_and_query}"),
                None => format!("https://{host}{path_and_query}"),
            },
        }
    }

    pub fn resolved_origin(&self, same_origin: &str) -> WebHostResult<String> {
        match self {
            Self::SameOrigin => {
                validate_http_origin(same_origin)?;
                Ok(same_origin.trim_end_matches('/').to_owned())
            }
            Self::Https { host, port } => Ok(match port {
                Some(port) => format!("https://{host}:{port}"),
                None => format!("https://{host}"),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserFetchCapability {
    pub name: String,
    pub origin: BrowserFetchOrigin,
    pub path_prefix: String,
    pub methods: BTreeSet<FetchMethod>,
    pub request_headers: BTreeSet<String>,
    pub max_url_bytes: usize,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub max_header_count: usize,
    pub max_header_bytes: usize,
    pub timeout_ms: u32,
}

impl BrowserFetchCapability {
    pub fn same_origin_api(name: impl Into<String>, path_prefix: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            origin: BrowserFetchOrigin::SameOrigin,
            path_prefix: path_prefix.into(),
            methods: [FetchMethod::Get, FetchMethod::Post].into_iter().collect(),
            request_headers: ["accept", "content-type"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            max_url_bytes: DEFAULT_MAX_URL_BYTES,
            max_request_bytes: DEFAULT_MAX_BODY_BYTES,
            max_response_bytes: DEFAULT_MAX_BODY_BYTES,
            max_header_count: DEFAULT_MAX_HEADER_COUNT,
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            timeout_ms: 15_000,
        }
    }

    pub fn https_endpoint(
        name: impl Into<String>,
        host: impl Into<String>,
        path_prefix: impl Into<String>,
    ) -> Self {
        let mut capability = Self::same_origin_api(name, path_prefix);
        capability.origin = BrowserFetchOrigin::Https {
            host: host.into(),
            port: None,
        };
        capability
    }

    fn validate(&self) -> WebHostResult<()> {
        validate_capability_name(&self.name)?;
        self.origin.validate()?;
        validate_relative_path(&self.path_prefix, "fetch capability path_prefix")?;
        if self.path_prefix.contains('?') {
            return Err(invalid(
                "fetch capability path_prefix",
                "query strings are not allowed in a path prefix",
            ));
        }
        if self.methods.is_empty() {
            return Err(invalid(
                "fetch capability methods",
                "at least one method is required",
            ));
        }
        validate_nonzero_limit("fetch max_request_bytes", self.max_request_bytes)?;
        validate_nonzero_limit("fetch max_url_bytes", self.max_url_bytes)?;
        validate_nonzero_limit("fetch max_response_bytes", self.max_response_bytes)?;
        validate_nonzero_limit("fetch max_header_count", self.max_header_count)?;
        validate_nonzero_limit("fetch max_header_bytes", self.max_header_bytes)?;
        if self.timeout_ms == 0 || self.timeout_ms > MAX_BROWSER_TIMEOUT_MS {
            return Err(invalid(
                "fetch timeout_ms",
                format!("must be within 1..={MAX_BROWSER_TIMEOUT_MS}"),
            ));
        }
        for header in &self.request_headers {
            validate_header_name(header)?;
            if forbidden_browser_header(header) {
                return Err(WebHostError::CapabilityDenied {
                    capability: self.name.clone(),
                    reason: format!("browser-controlled header {header} cannot be granted"),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserFetchRequest {
    pub request_id: u64,
    pub capability: String,
    pub method: FetchMethod,
    pub path_and_query: String,
    pub headers: Vec<HeaderValue>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedFetchRequest {
    pub request: BrowserFetchRequest,
    pub origin: BrowserFetchOrigin,
    pub max_response_bytes: usize,
    pub max_response_header_count: usize,
    pub max_response_header_bytes: usize,
    pub timeout_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserFetchResponse {
    pub request_id: u64,
    pub status: u16,
    pub headers: Vec<HeaderValue>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct BrowserFetchCapabilities {
    entries: BTreeMap<String, BrowserFetchCapability>,
}

impl BrowserFetchCapabilities {
    pub fn new(
        capabilities: impl IntoIterator<Item = BrowserFetchCapability>,
    ) -> WebHostResult<Self> {
        let mut entries = BTreeMap::new();
        for mut capability in capabilities {
            capability.request_headers = capability
                .request_headers
                .into_iter()
                .map(|header| header.to_ascii_lowercase())
                .collect();
            capability.validate()?;
            let name = capability.name.clone();
            if entries.insert(name.clone(), capability).is_some() {
                return Err(invalid(
                    "fetch capability",
                    format!("duplicate capability {name}"),
                ));
            }
        }
        Ok(Self { entries })
    }

    pub fn validate_request(
        &self,
        request: BrowserFetchRequest,
    ) -> WebHostResult<ValidatedFetchRequest> {
        let capability = self.entries.get(&request.capability).ok_or_else(|| {
            WebHostError::CapabilityDenied {
                capability: request.capability.clone(),
                reason: "capability is not declared".to_owned(),
            }
        })?;
        if !capability.methods.contains(&request.method) {
            return Err(WebHostError::CapabilityDenied {
                capability: capability.name.clone(),
                reason: format!("method {} is not allowed", request.method.as_str()),
            });
        }
        validate_relative_path(&request.path_and_query, "fetch path_and_query")?;
        if request.path_and_query.len() > capability.max_url_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "fetch URL".to_owned(),
                limit: capability.max_url_bytes,
            });
        }
        if request.path_and_query.contains('#') {
            return Err(invalid(
                "fetch path_and_query",
                "fragments are not sent in HTTP requests",
            ));
        }
        if !path_has_prefix(&request.path_and_query, &capability.path_prefix) {
            return Err(WebHostError::CapabilityDenied {
                capability: capability.name.clone(),
                reason: format!(
                    "path {} is outside {}",
                    request.path_and_query, capability.path_prefix
                ),
            });
        }
        if request.body.len() > capability.max_request_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "fetch request body".to_owned(),
                limit: capability.max_request_bytes,
            });
        }
        if !request.body.is_empty()
            && matches!(request.method, FetchMethod::Get | FetchMethod::Head)
        {
            return Err(WebHostError::InvalidInput {
                field: "fetch request body".to_owned(),
                reason: format!("{} requests cannot carry a body", request.method.as_str()),
            });
        }
        validate_headers(
            &request.headers,
            capability.max_header_count,
            capability.max_header_bytes,
            Some(&capability.request_headers),
            &capability.name,
        )?;
        Ok(ValidatedFetchRequest {
            request,
            origin: capability.origin.clone(),
            max_response_bytes: capability.max_response_bytes,
            max_response_header_count: capability.max_header_count,
            max_response_header_bytes: capability.max_header_bytes,
            timeout_ms: capability.timeout_ms,
        })
    }

    pub fn capability(&self, name: &str) -> Option<&BrowserFetchCapability> {
        self.entries.get(name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserWebSocketCapability {
    pub name: String,
    pub path_prefix: String,
    pub protocols: BTreeSet<String>,
    pub max_url_bytes: usize,
    pub max_message_bytes: usize,
    pub max_queue_messages: usize,
    pub max_queue_bytes: usize,
}

impl BrowserWebSocketCapability {
    pub fn same_origin(name: impl Into<String>, path_prefix: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path_prefix: path_prefix.into(),
            protocols: BTreeSet::new(),
            max_url_bytes: DEFAULT_MAX_URL_BYTES,
            max_message_bytes: DEFAULT_MAX_SOCKET_MESSAGE_BYTES,
            max_queue_messages: DEFAULT_MAX_SOCKET_QUEUE_MESSAGES,
            max_queue_bytes: DEFAULT_MAX_SOCKET_QUEUE_BYTES,
        }
    }

    fn validate(&self) -> WebHostResult<()> {
        validate_capability_name(&self.name)?;
        validate_relative_path(&self.path_prefix, "websocket capability path_prefix")?;
        if self.path_prefix.contains('?') {
            return Err(invalid(
                "websocket capability path_prefix",
                "query strings are not allowed in a path prefix",
            ));
        }
        validate_nonzero_limit("websocket max_message_bytes", self.max_message_bytes)?;
        validate_nonzero_limit("websocket max_url_bytes", self.max_url_bytes)?;
        validate_nonzero_limit("websocket max_queue_messages", self.max_queue_messages)?;
        validate_nonzero_limit("websocket max_queue_bytes", self.max_queue_bytes)?;
        for protocol in &self.protocols {
            if protocol.is_empty()
                || protocol.len() > 128
                || !protocol
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && byte != b',' && byte != b' ')
            {
                return Err(invalid("websocket protocol", "contains invalid bytes"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserWebSocketRequest {
    pub connection_id: u64,
    pub capability: String,
    pub path_and_query: String,
    pub protocols: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedWebSocketRequest {
    pub request: BrowserWebSocketRequest,
    pub max_message_bytes: usize,
    pub max_queue_messages: usize,
    pub max_queue_bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct BrowserWebSocketCapabilities {
    entries: BTreeMap<String, BrowserWebSocketCapability>,
}

impl BrowserWebSocketCapabilities {
    pub fn new(
        capabilities: impl IntoIterator<Item = BrowserWebSocketCapability>,
    ) -> WebHostResult<Self> {
        let mut entries = BTreeMap::new();
        for capability in capabilities {
            capability.validate()?;
            let name = capability.name.clone();
            if entries.insert(name.clone(), capability).is_some() {
                return Err(invalid(
                    "websocket capability",
                    format!("duplicate capability {name}"),
                ));
            }
        }
        Ok(Self { entries })
    }

    pub fn validate_request(
        &self,
        request: BrowserWebSocketRequest,
    ) -> WebHostResult<ValidatedWebSocketRequest> {
        let capability = self.entries.get(&request.capability).ok_or_else(|| {
            WebHostError::CapabilityDenied {
                capability: request.capability.clone(),
                reason: "capability is not declared".to_owned(),
            }
        })?;
        validate_relative_path(&request.path_and_query, "websocket path_and_query")?;
        if request.path_and_query.len() > capability.max_url_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "WebSocket URL".to_owned(),
                limit: capability.max_url_bytes,
            });
        }
        if request.path_and_query.contains('#') {
            return Err(invalid(
                "websocket path_and_query",
                "fragments are not part of a WebSocket request target",
            ));
        }
        if !path_has_prefix(&request.path_and_query, &capability.path_prefix) {
            return Err(WebHostError::CapabilityDenied {
                capability: capability.name.clone(),
                reason: format!(
                    "path {} is outside {}",
                    request.path_and_query, capability.path_prefix
                ),
            });
        }
        if request.protocols.len() > 16 {
            return Err(WebHostError::LimitExceeded {
                resource: "websocket protocols".to_owned(),
                limit: 16,
            });
        }
        let mut requested_protocols = BTreeSet::new();
        for protocol in &request.protocols {
            if !requested_protocols.insert(protocol) {
                return Err(invalid(
                    "websocket protocols",
                    format!("duplicate protocol {protocol}"),
                ));
            }
            if !capability.protocols.contains(protocol) {
                return Err(WebHostError::CapabilityDenied {
                    capability: capability.name.clone(),
                    reason: format!("protocol {protocol} is not allowed"),
                });
            }
        }
        Ok(ValidatedWebSocketRequest {
            request,
            max_message_bytes: capability.max_message_bytes,
            max_queue_messages: capability.max_queue_messages,
            max_queue_bytes: capability.max_queue_bytes,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SocketFrame {
    Text { text: String },
    Binary { bytes: Vec<u8> },
}

impl SocketFrame {
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Text { text } => text.len(),
            Self::Binary { bytes } => bytes.len(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BoundedSocketQueue {
    queue: VecDeque<SocketFrame>,
    bytes: usize,
    max_message_bytes: usize,
    max_messages: usize,
    max_bytes: usize,
}

impl BoundedSocketQueue {
    pub fn new(
        max_message_bytes: usize,
        max_messages: usize,
        max_bytes: usize,
    ) -> WebHostResult<Self> {
        validate_nonzero_limit("socket max_message_bytes", max_message_bytes)?;
        validate_nonzero_limit("socket max_messages", max_messages)?;
        validate_nonzero_limit("socket max_bytes", max_bytes)?;
        Ok(Self {
            queue: VecDeque::new(),
            bytes: 0,
            max_message_bytes,
            max_messages,
            max_bytes,
        })
    }

    pub fn push(&mut self, frame: SocketFrame) -> WebHostResult<()> {
        let bytes = frame.byte_len();
        if bytes > self.max_message_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "websocket message".to_owned(),
                limit: self.max_message_bytes,
            });
        }
        if self.queue.len() >= self.max_messages
            || self
                .bytes
                .checked_add(bytes)
                .is_none_or(|total| total > self.max_bytes)
        {
            return Err(WebHostError::QueueOverflow {
                queue: "websocket messages".to_owned(),
                capacity: self.max_messages,
            });
        }
        self.bytes += bytes;
        self.queue.push_back(frame);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<SocketFrame> {
        let frame = self.queue.pop_front()?;
        self.bytes = self.bytes.saturating_sub(frame.byte_len());
        Some(frame)
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn byte_len(&self) -> usize {
        self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserHistoryCapability {
    pub path_prefix: String,
    pub max_url_bytes: usize,
    pub max_state_bytes: usize,
}

impl BrowserHistoryCapability {
    pub fn validate_entry(&self, entry: &BrowserHistoryEntry) -> WebHostResult<()> {
        validate_relative_path(&self.path_prefix, "history capability path_prefix")?;
        validate_relative_path(&entry.path_query_fragment, "history URL")?;
        if entry.path_query_fragment.len() > self.max_url_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "history URL".to_owned(),
                limit: self.max_url_bytes,
            });
        }
        if entry.state.len() > self.max_state_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "history state".to_owned(),
                limit: self.max_state_bytes,
            });
        }
        if !path_has_prefix(&entry.path_query_fragment, &self.path_prefix) {
            return Err(WebHostError::CapabilityDenied {
                capability: "url_history".to_owned(),
                reason: format!(
                    "path {} is outside {}",
                    entry.path_query_fragment, self.path_prefix
                ),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserHistoryEntry {
    pub path_query_fragment: String,
    pub state: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserHistoryMutation {
    Push,
    Replace,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserClipboardCapability {
    pub max_text_bytes: usize,
    pub require_user_activation: bool,
}

impl BrowserClipboardCapability {
    pub fn validate_text(&self, text: &str, user_activated: bool) -> WebHostResult<()> {
        if self.require_user_activation && !user_activated {
            return Err(WebHostError::CapabilityDenied {
                capability: "clipboard".to_owned(),
                reason: "a current user activation is required".to_owned(),
            });
        }
        if text.len() > self.max_text_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "clipboard text".to_owned(),
                limit: self.max_text_bytes,
            });
        }
        Ok(())
    }
}

pub(crate) fn validate_capability_name(name: &str) -> WebHostResult<()> {
    if name.is_empty()
        || name.len() > 96
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid(
            "capability name",
            "must contain 1..=96 ASCII letters, digits, '.', '_' or '-'",
        ));
    }
    Ok(())
}

fn validate_relative_path(path_and_query: &str, field: &str) -> WebHostResult<()> {
    if !path_and_query.starts_with('/')
        || path_and_query.starts_with("//")
        || path_and_query.contains("\\")
        || path_and_query.chars().any(char::is_control)
    {
        return Err(invalid(
            field,
            "must be a same-origin absolute path beginning with one '/'",
        ));
    }
    let path = path_and_query
        .split(['?', '#'])
        .next()
        .unwrap_or(path_and_query);
    for encoded in path.split('/') {
        let decoded = percent_decode_str(encoded)
            .decode_utf8()
            .map_err(|_| invalid(field, "contains invalid percent-encoded UTF-8"))?;
        if decoded == "."
            || decoded == ".."
            || decoded.contains('/')
            || decoded.contains('\\')
            || decoded.chars().any(char::is_control)
        {
            return Err(invalid(field, "contains a path traversal segment"));
        }
    }
    Ok(())
}

fn path_has_prefix(path_and_query: &str, prefix: &str) -> bool {
    let path = path_and_query
        .split(['?', '#'])
        .next()
        .unwrap_or(path_and_query);
    if prefix == "/" {
        return true;
    }
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| prefix.ends_with('/') || suffix.starts_with('/'))
}

fn validate_headers(
    headers: &[HeaderValue],
    max_count: usize,
    max_bytes: usize,
    allowlist: Option<&BTreeSet<String>>,
    capability: &str,
) -> WebHostResult<()> {
    if headers.len() > max_count {
        return Err(WebHostError::LimitExceeded {
            resource: "HTTP header count".to_owned(),
            limit: max_count,
        });
    }
    let mut bytes = 0usize;
    for header in headers {
        validate_header_name(&header.name)?;
        if header
            .value
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\0'))
        {
            return Err(invalid("HTTP header value", "contains a control delimiter"));
        }
        let normalized = header.name.to_ascii_lowercase();
        if forbidden_browser_header(&normalized) {
            return Err(WebHostError::CapabilityDenied {
                capability: capability.to_owned(),
                reason: format!("browser-controlled header {} cannot be set", header.name),
            });
        }
        if allowlist.is_some_and(|allowlist| !allowlist.contains(&normalized)) {
            return Err(WebHostError::CapabilityDenied {
                capability: capability.to_owned(),
                reason: format!("header {} is not allowlisted", header.name),
            });
        }
        bytes = bytes
            .checked_add(header.name.len() + header.value.len())
            .ok_or_else(|| WebHostError::LimitExceeded {
                resource: "HTTP header bytes".to_owned(),
                limit: max_bytes,
            })?;
    }
    if bytes > max_bytes {
        return Err(WebHostError::LimitExceeded {
            resource: "HTTP header bytes".to_owned(),
            limit: max_bytes,
        });
    }
    Ok(())
}

fn validate_header_name(name: &str) -> WebHostResult<()> {
    if name.is_empty()
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
    {
        return Err(invalid("HTTP header name", "contains invalid bytes"));
    }
    Ok(())
}

fn forbidden_browser_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "accept-charset"
            | "accept-encoding"
            | "access-control-request-headers"
            | "access-control-request-method"
            | "connection"
            | "content-length"
            | "cookie"
            | "date"
            | "dnt"
            | "expect"
            | "host"
            | "keep-alive"
            | "origin"
            | "permissions-policy"
            | "referer"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "via"
    ) || name.to_ascii_lowercase().starts_with("proxy-")
        || name.to_ascii_lowercase().starts_with("sec-")
}

fn validate_nonzero_limit(field: &str, value: usize) -> WebHostResult<()> {
    if value == 0 {
        return Err(invalid(field, "must be non-zero"));
    }
    Ok(())
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> WebHostError {
    WebHostError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    }
}

fn validate_http_origin(origin: &str) -> WebHostResult<()> {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .ok_or_else(|| invalid("browser same origin", "must use HTTP or HTTPS"))?;
    if rest.is_empty() || rest.contains(['/', '?', '#', '\\']) || rest.chars().any(char::is_control)
    {
        return Err(invalid(
            "browser same origin",
            "must contain only a scheme and authority",
        ));
    }
    Ok(())
}
