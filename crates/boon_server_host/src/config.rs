use axum::http::header::HeaderName;
use ipnet::IpNet;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlowClientPolicy {
    Close { code: u16, reason: String },
}

impl Default for SlowClientPolicy {
    fn default() -> Self {
        Self::Close {
            code: 1008,
            reason: "outbound backpressure".to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServerLimits {
    pub owner_queue_capacity: usize,
    pub max_connections: usize,
    pub max_http_body_bytes: usize,
    pub max_http_response_body_bytes: usize,
    pub max_request_headers: usize,
    pub max_request_header_bytes: usize,
    pub max_response_headers: usize,
    pub max_response_header_bytes: usize,
    pub max_path_segments: usize,
    pub max_path_segment_bytes: usize,
    pub max_query_pairs: usize,
    pub max_query_bytes: usize,
    pub max_cookies: usize,
    pub max_cookie_bytes: usize,
    pub max_websocket_message_bytes: usize,
    pub websocket_write_queue_messages: usize,
    pub websocket_write_queue_bytes: usize,
    pub max_rooms_per_connection: usize,
    pub max_room_name_bytes: usize,
    pub max_actions_per_event: usize,
    pub max_close_reason_bytes: usize,
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            owner_queue_capacity: 256,
            max_connections: 1_024,
            max_http_body_bytes: 1024 * 1024,
            max_http_response_body_bytes: 4 * 1024 * 1024,
            max_request_headers: 64,
            max_request_header_bytes: 32 * 1024,
            max_response_headers: 64,
            max_response_header_bytes: 32 * 1024,
            max_path_segments: 64,
            max_path_segment_bytes: 4 * 1024,
            max_query_pairs: 128,
            max_query_bytes: 32 * 1024,
            max_cookies: 64,
            max_cookie_bytes: 16 * 1024,
            max_websocket_message_bytes: 1024 * 1024,
            websocket_write_queue_messages: 64,
            websocket_write_queue_bytes: 4 * 1024 * 1024,
            max_rooms_per_connection: 128,
            max_room_name_bytes: 512,
            max_actions_per_event: 256,
            max_close_reason_bytes: 123,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServerTimeouts {
    pub program_call: Duration,
    pub graceful_shutdown: Duration,
    pub websocket_ping_interval: Option<Duration>,
    pub websocket_pong_timeout: Duration,
}

impl Default for ServerTimeouts {
    fn default() -> Self {
        Self {
            program_call: Duration::from_secs(10),
            graceful_shutdown: Duration::from_secs(30),
            websocket_ping_interval: Some(Duration::from_secs(30)),
            websocket_pong_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ServerConfig {
    pub limits: ServerLimits,
    pub timeouts: ServerTimeouts,
    pub request_header_allowlist: BTreeSet<String>,
    pub trusted_proxy: TrustedProxyPolicy,
    pub origin_policy: OriginPolicy,
    pub slow_client_policy: SlowClientPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedProxyPolicy {
    trusted_cidrs: Vec<IpNet>,
    max_forwarded_hops: usize,
}

impl Default for TrustedProxyPolicy {
    fn default() -> Self {
        Self {
            trusted_cidrs: Vec::new(),
            max_forwarded_hops: 16,
        }
    }
}

impl TrustedProxyPolicy {
    pub fn from_cidrs(
        cidrs: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, ConfigError> {
        let trusted_cidrs = cidrs
            .into_iter()
            .map(|cidr| {
                cidr.as_ref().parse::<IpNet>().map_err(|_| {
                    ConfigError(format!("invalid trusted proxy CIDR `{}`", cidr.as_ref()))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if trusted_cidrs.is_empty() {
            return Err(ConfigError(
                "trusted proxy policy requires at least one CIDR".to_owned(),
            ));
        }
        Ok(Self {
            trusted_cidrs,
            ..Self::default()
        })
    }

    pub fn with_max_forwarded_hops(mut self, maximum: usize) -> Self {
        self.max_forwarded_hops = maximum;
        self
    }

    pub fn trusts(&self, address: std::net::IpAddr) -> bool {
        self.trusted_cidrs
            .iter()
            .any(|network| network.contains(&address))
    }

    pub const fn max_forwarded_hops(&self) -> usize {
        self.max_forwarded_hops
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.max_forwarded_hops == 0 {
            return Err(ConfigError(
                "max_forwarded_hops must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OriginPolicy {
    pub allowed_origins: BTreeSet<String>,
    pub require_websocket_origin: bool,
    pub enforce_http_origin: bool,
}

impl OriginPolicy {
    pub fn exact(origins: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allowed_origins: origins.into_iter().map(Into::into).collect(),
            require_websocket_origin: true,
            enforce_http_origin: true,
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        for origin in &self.allowed_origins {
            if !valid_origin(origin) {
                return Err(ConfigError(format!(
                    "invalid exact allowed origin `{origin}`"
                )));
            }
        }
        if (self.require_websocket_origin || self.enforce_http_origin)
            && self.allowed_origins.is_empty()
        {
            return Err(ConfigError(
                "enforced origin policy requires at least one allowed origin".to_owned(),
            ));
        }
        Ok(())
    }
}

impl ServerConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let limits = &self.limits;
        for (name, value) in [
            ("owner_queue_capacity", limits.owner_queue_capacity),
            ("max_connections", limits.max_connections),
            ("max_http_body_bytes", limits.max_http_body_bytes),
            (
                "max_http_response_body_bytes",
                limits.max_http_response_body_bytes,
            ),
            ("max_request_headers", limits.max_request_headers),
            ("max_request_header_bytes", limits.max_request_header_bytes),
            ("max_response_headers", limits.max_response_headers),
            (
                "max_response_header_bytes",
                limits.max_response_header_bytes,
            ),
            ("max_path_segments", limits.max_path_segments),
            ("max_path_segment_bytes", limits.max_path_segment_bytes),
            ("max_query_pairs", limits.max_query_pairs),
            ("max_query_bytes", limits.max_query_bytes),
            ("max_cookies", limits.max_cookies),
            ("max_cookie_bytes", limits.max_cookie_bytes),
            (
                "max_websocket_message_bytes",
                limits.max_websocket_message_bytes,
            ),
            (
                "websocket_write_queue_messages",
                limits.websocket_write_queue_messages,
            ),
            (
                "websocket_write_queue_bytes",
                limits.websocket_write_queue_bytes,
            ),
            ("max_rooms_per_connection", limits.max_rooms_per_connection),
            ("max_room_name_bytes", limits.max_room_name_bytes),
            ("max_actions_per_event", limits.max_actions_per_event),
            ("max_close_reason_bytes", limits.max_close_reason_bytes),
        ] {
            if value == 0 {
                return Err(ConfigError(format!("{name} must be greater than zero")));
            }
        }
        self.trusted_proxy.validate()?;
        self.origin_policy.validate()?;
        if limits.websocket_write_queue_bytes < limits.max_websocket_message_bytes {
            return Err(ConfigError(
                "websocket_write_queue_bytes must hold at least one maximum-sized message"
                    .to_owned(),
            ));
        }
        if limits.max_close_reason_bytes > 123 {
            return Err(ConfigError(
                "max_close_reason_bytes cannot exceed the WebSocket control-frame limit of 123"
                    .to_owned(),
            ));
        }
        if self.timeouts.program_call.is_zero() {
            return Err(ConfigError(
                "program_call timeout must be greater than zero".to_owned(),
            ));
        }
        if self.timeouts.graceful_shutdown.is_zero() {
            return Err(ConfigError(
                "graceful_shutdown timeout must be greater than zero".to_owned(),
            ));
        }
        if self.timeouts.websocket_pong_timeout.is_zero() {
            return Err(ConfigError(
                "websocket_pong_timeout must be greater than zero".to_owned(),
            ));
        }
        if self
            .timeouts
            .websocket_ping_interval
            .is_some_and(|duration| duration.is_zero())
        {
            return Err(ConfigError(
                "websocket_ping_interval must be greater than zero when enabled".to_owned(),
            ));
        }
        for header in &self.request_header_allowlist {
            let parsed = HeaderName::from_bytes(header.as_bytes()).map_err(|_| {
                ConfigError(format!("invalid request header allowlist entry: {header}"))
            })?;
            if parsed.as_str() != header {
                return Err(ConfigError(format!(
                    "request header allowlist entries must be lowercase: {header}"
                )));
            }
        }
        match &self.slow_client_policy {
            SlowClientPolicy::Close { code, reason } => {
                if !valid_application_close_code(*code) {
                    return Err(ConfigError(format!(
                        "slow-client close code {code} is not a valid application close code"
                    )));
                }
                if reason.len() > limits.max_close_reason_bytes {
                    return Err(ConfigError(
                        "slow-client close reason exceeds max_close_reason_bytes".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn valid_origin(origin: &str) -> bool {
    let authority = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"));
    authority.is_some_and(|authority| {
        !authority.is_empty()
            && !authority.contains(['/', '?', '#', '\\', '@'])
            && !authority.chars().any(char::is_whitespace)
    })
}

pub(crate) fn valid_application_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999) && !matches!(code, 1004..=1006 | 1015)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError(pub(crate) String);

impl Display for ConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        ServerConfig::default().validate().unwrap();
    }

    #[test]
    fn rejects_unbounded_or_impossible_limits() {
        let mut config = ServerConfig::default();
        config.limits.owner_queue_capacity = 0;
        assert_eq!(
            config.validate().unwrap_err().to_string(),
            "owner_queue_capacity must be greater than zero"
        );

        let mut config = ServerConfig::default();
        config.limits.websocket_write_queue_bytes = config.limits.max_websocket_message_bytes - 1;
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("at least one maximum-sized message")
        );
    }
}
