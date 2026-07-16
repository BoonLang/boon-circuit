use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use percent_encoding::percent_decode_str;
use reqwest::Url;

use crate::{EndpointName, ExecuteError, QueryParameter, RequestViolation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AddressPolicy {
    PublicInternet,
    LoopbackTestOnly,
}

impl AddressPolicy {
    pub(crate) fn permits(self, address: IpAddr) -> bool {
        match self {
            Self::PublicInternet => is_public_internet_address(address),
            Self::LoopbackTestOnly => address.is_loopback(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[must_use = "local HTTP must be explicitly acknowledged as test-only"]
pub struct LocalHttpTestPermit {
    _private: (),
}

impl LocalHttpTestPermit {
    /// Explicitly permits an HTTP capability bound to a loopback socket.
    ///
    /// Normal endpoint construction only accepts HTTPS. This escape hatch is
    /// intentionally named for test infrastructure and cannot authorize a
    /// non-loopback address.
    pub fn explicitly_enable_for_tests() -> Self {
        Self { _private: () }
    }
}

#[derive(Clone, Debug)]
pub struct EndpointCapability {
    name: EndpointName,
    base_url: Url,
    address_policy: AddressPolicy,
}

impl EndpointCapability {
    /// Creates a production endpoint capability. `base_url` must use HTTPS and
    /// its path becomes the capability's path prefix.
    pub fn https(name: EndpointName, base_url: impl AsRef<str>) -> Result<Self, ConfigError> {
        let mut base_url =
            Url::parse(base_url.as_ref()).map_err(|_| ConfigError::InvalidEndpointUrl)?;
        if base_url.scheme() != "https" {
            return Err(ConfigError::HttpsRequired);
        }
        validate_base_url(&base_url)?;
        normalize_path_prefix(&mut base_url)?;

        if let Some(address) = literal_host_address(&base_url)
            && !AddressPolicy::PublicInternet.permits(address)
        {
            return Err(ConfigError::AddressNotAllowed);
        }

        Ok(Self {
            name,
            base_url,
            address_policy: AddressPolicy::PublicInternet,
        })
    }

    pub fn local_http_for_tests(
        name: EndpointName,
        address: SocketAddr,
        path_prefix: impl AsRef<str>,
        _permit: LocalHttpTestPermit,
    ) -> Result<Self, ConfigError> {
        if !address.ip().is_loopback() {
            return Err(ConfigError::AddressNotAllowed);
        }
        if address.port() == 0 {
            return Err(ConfigError::InvalidEndpointPort);
        }
        let path_prefix = path_prefix.as_ref();
        if !path_prefix.starts_with('/') || path_prefix.contains('?') || path_prefix.contains('#') {
            return Err(ConfigError::InvalidPathPrefix);
        }

        let mut base_url = Url::parse(&format!("http://{address}/"))
            .map_err(|_| ConfigError::InvalidEndpointUrl)?;
        base_url.set_path(path_prefix);
        normalize_path_prefix(&mut base_url)?;

        Ok(Self {
            name,
            base_url,
            address_policy: AddressPolicy::LoopbackTestOnly,
        })
    }

    pub fn name(&self) -> &EndpointName {
        &self.name
    }

    pub fn scheme(&self) -> &str {
        self.base_url.scheme()
    }

    pub fn host(&self) -> &str {
        self.base_url
            .host_str()
            .expect("validated endpoint capability has a host")
    }

    pub fn port(&self) -> u16 {
        self.base_url
            .port_or_known_default()
            .expect("validated HTTP endpoint capability has a port")
    }

    pub fn path_prefix(&self) -> &str {
        self.base_url.path()
    }

    pub(crate) fn address_policy(&self) -> AddressPolicy {
        self.address_policy
    }

    pub(crate) fn same_origin(&self, other: &Self) -> bool {
        self.scheme() == other.scheme()
            && self.host() == other.host()
            && self.port() == other.port()
    }

    pub(crate) fn matches_url(&self, url: &Url) -> bool {
        url.username().is_empty()
            && url.password().is_none()
            && url.scheme() == self.scheme()
            && url.host_str() == Some(self.host())
            && url.port_or_known_default() == Some(self.port())
            && valid_normalized_path(url.path())
            && self.path_allows(url.path())
            && literal_host_address(url).is_none_or(|address| self.address_policy.permits(address))
    }

    pub(crate) fn build_url(
        &self,
        path_segments: &[String],
        query: &[QueryParameter],
    ) -> Result<Url, ExecuteError> {
        let mut url = self.base_url.clone();
        if !path_segments.is_empty() {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| ExecuteError::InvalidRequest(RequestViolation::InvalidPathSegment))?;
            segments.pop_if_empty();
            for segment in path_segments {
                if !valid_request_path_segment(segment) {
                    return Err(ExecuteError::InvalidRequest(
                        RequestViolation::InvalidPathSegment,
                    ));
                }
                segments.push(segment);
            }
        }
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for parameter in query {
                pairs.append_pair(&parameter.name, &parameter.value);
            }
        }
        Ok(url)
    }

    fn path_allows(&self, path: &str) -> bool {
        let prefix = self.path_prefix();
        if prefix == "/" {
            return path.starts_with('/');
        }
        let prefix_root = prefix.trim_end_matches('/');
        path == prefix_root || path.starts_with(prefix)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedirectPolicy {
    Deny,
    Follow { max_hops: u8 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpLimits {
    pub max_request_body_bytes: usize,
    pub max_response_body_bytes: usize,
    pub max_request_headers: usize,
    pub max_response_headers: usize,
    pub max_request_header_bytes: usize,
    pub max_response_header_bytes: usize,
    pub max_url_bytes: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_request_body_bytes: 1024 * 1024,
            max_response_body_bytes: 4 * 1024 * 1024,
            max_request_headers: 32,
            max_response_headers: 64,
            max_request_header_bytes: 16 * 1024,
            max_response_header_bytes: 64 * 1024,
            max_url_bytes: 8 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Timeouts {
    pub connect: Duration,
    pub overall: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            overall: Duration::from_secs(15),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolConfig {
    pub idle_timeout: Duration,
    pub max_idle_per_host: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(60),
            max_idle_per_host: 8,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub endpoints: Vec<EndpointCapability>,
    pub limits: HttpLimits,
    pub timeouts: Timeouts,
    pub redirects: RedirectPolicy,
    pub pool: PoolConfig,
    pub max_in_flight: usize,
}

impl ClientConfig {
    pub fn new(endpoints: Vec<EndpointCapability>) -> Self {
        Self {
            endpoints,
            limits: HttpLimits::default(),
            timeouts: Timeouts::default(),
            redirects: RedirectPolicy::Deny,
            pool: PoolConfig::default(),
            max_in_flight: 32,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        validate_nonzero_limits(&self.limits)?;
        if self.timeouts.connect.is_zero() || self.timeouts.overall.is_zero() {
            return Err(ConfigError::InvalidTimeout);
        }
        if self.timeouts.connect > self.timeouts.overall {
            return Err(ConfigError::ConnectTimeoutExceedsOverall);
        }
        if self.pool.idle_timeout.is_zero() || self.pool.max_idle_per_host == 0 {
            return Err(ConfigError::InvalidPoolConfig);
        }
        if self.max_in_flight == 0 {
            return Err(ConfigError::InvalidConcurrencyLimit);
        }
        if let RedirectPolicy::Follow { max_hops } = self.redirects
            && !(1..=20).contains(&max_hops)
        {
            return Err(ConfigError::InvalidRedirectLimit);
        }

        let mut by_name = BTreeMap::new();
        let mut host_policies = BTreeMap::new();
        for endpoint in &self.endpoints {
            if by_name.insert(endpoint.name().clone(), endpoint).is_some() {
                return Err(ConfigError::DuplicateEndpointName);
            }
            if let Some(policy) = host_policies.insert(endpoint.host(), endpoint.address_policy())
                && policy != endpoint.address_policy()
            {
                return Err(ConfigError::ConflictingHostAddressPolicy);
            }
        }

        for (index, left) in self.endpoints.iter().enumerate() {
            for right in &self.endpoints[index + 1..] {
                if left.same_origin(right)
                    && (left.path_allows(right.path_prefix().trim_end_matches('/'))
                        || right.path_allows(left.path_prefix().trim_end_matches('/')))
                {
                    return Err(ConfigError::OverlappingEndpointCapabilities);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
    InvalidEndpointName,
    InvalidEndpointUrl,
    HttpsRequired,
    EndpointHostMissing,
    InvalidEndpointPort,
    EndpointCredentialsNotAllowed,
    EndpointQueryNotAllowed,
    EndpointFragmentNotAllowed,
    InvalidPathPrefix,
    AddressNotAllowed,
    DuplicateEndpointName,
    ConflictingHostAddressPolicy,
    OverlappingEndpointCapabilities,
    InvalidLimit,
    InvalidTimeout,
    ConnectTimeoutExceedsOverall,
    InvalidPoolConfig,
    InvalidConcurrencyLimit,
    InvalidRedirectLimit,
    ClientBuildFailed,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ConfigError {}

fn validate_base_url(url: &Url) -> Result<(), ConfigError> {
    if url.host_str().is_none() {
        return Err(ConfigError::EndpointHostMissing);
    }
    if url.port_or_known_default().is_none() || url.port() == Some(0) {
        return Err(ConfigError::InvalidEndpointPort);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ConfigError::EndpointCredentialsNotAllowed);
    }
    if url.query().is_some() {
        return Err(ConfigError::EndpointQueryNotAllowed);
    }
    if url.fragment().is_some() {
        return Err(ConfigError::EndpointFragmentNotAllowed);
    }
    if !valid_normalized_path(url.path()) {
        return Err(ConfigError::InvalidPathPrefix);
    }
    Ok(())
}

fn normalize_path_prefix(url: &mut Url) -> Result<(), ConfigError> {
    if !valid_normalized_path(url.path()) {
        return Err(ConfigError::InvalidPathPrefix);
    }
    if !url.path().ends_with('/') {
        let mut path = url.path().to_owned();
        path.push('/');
        url.set_path(&path);
    }
    Ok(())
}

pub(crate) fn valid_normalized_path(path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    path.split('/').all(|segment| {
        let Ok(decoded) = percent_decode_str(segment).decode_utf8() else {
            return false;
        };
        decoded != "."
            && decoded != ".."
            && !decoded
                .chars()
                .any(|character| matches!(character, '/' | '\\' | '%') || character.is_control())
    })
}

fn valid_request_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '%') || character.is_control())
}

fn validate_nonzero_limits(limits: &HttpLimits) -> Result<(), ConfigError> {
    let values = [
        limits.max_request_body_bytes,
        limits.max_response_body_bytes,
        limits.max_request_headers,
        limits.max_response_headers,
        limits.max_request_header_bytes,
        limits.max_response_header_bytes,
        limits.max_url_bytes,
    ];
    if values.contains(&0) || u32::try_from(limits.max_response_header_bytes).is_err() {
        return Err(ConfigError::InvalidLimit);
    }
    Ok(())
}

fn literal_host_address(url: &Url) -> Option<IpAddr> {
    url.host_str()?.parse().ok()
}

fn is_public_internet_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => !BLOCKED_IPV4
            .iter()
            .any(|&(network, prefix)| ipv4_in_network(address, network, prefix)),
        IpAddr::V6(address) => {
            ipv6_in_network(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3)
                && !BLOCKED_IPV6
                    .iter()
                    .any(|&(network, prefix)| ipv6_in_network(address, network, prefix))
        }
    }
}

const BLOCKED_IPV4: &[(Ipv4Addr, u8)] = &[
    (Ipv4Addr::new(0, 0, 0, 0), 8),
    (Ipv4Addr::new(10, 0, 0, 0), 8),
    (Ipv4Addr::new(100, 64, 0, 0), 10),
    (Ipv4Addr::new(127, 0, 0, 0), 8),
    (Ipv4Addr::new(169, 254, 0, 0), 16),
    (Ipv4Addr::new(172, 16, 0, 0), 12),
    (Ipv4Addr::new(192, 0, 0, 0), 24),
    (Ipv4Addr::new(192, 0, 2, 0), 24),
    (Ipv4Addr::new(192, 88, 99, 0), 24),
    (Ipv4Addr::new(192, 168, 0, 0), 16),
    (Ipv4Addr::new(198, 18, 0, 0), 15),
    (Ipv4Addr::new(198, 51, 100, 0), 24),
    (Ipv4Addr::new(203, 0, 113, 0), 24),
    (Ipv4Addr::new(224, 0, 0, 0), 4),
    (Ipv4Addr::new(240, 0, 0, 0), 4),
];

const BLOCKED_IPV6: &[(Ipv6Addr, u8)] = &[
    (Ipv6Addr::UNSPECIFIED, 96),
    (Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0, 0), 96),
    (Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0), 96),
    (Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0), 48),
    (Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 0), 64),
    (Ipv6Addr::new(0x100, 0, 0, 1, 0, 0, 0, 0), 64),
    (Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23),
    (Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16),
    (Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20),
    (Ipv6Addr::new(0x5f00, 0, 0, 0, 0, 0, 0, 0), 16),
    (Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7),
    (Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10),
    (Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0), 10),
    (Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8),
];

fn ipv4_in_network(address: Ipv4Addr, network: Ipv4Addr, prefix: u8) -> bool {
    let mask = u32::MAX.checked_shl(u32::from(32 - prefix)).unwrap_or(0);
    u32::from(address) & mask == u32::from(network) & mask
}

fn ipv6_in_network(address: Ipv6Addr, network: Ipv6Addr, prefix: u8) -> bool {
    let mask = u128::MAX.checked_shl(u32::from(128 - prefix)).unwrap_or(0);
    u128::from(address) & mask == u128::from(network) & mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_address_policy_rejects_ssrf_ranges() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.31.0.1",
            "192.168.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "100:0:0:1::1",
            "2001:db8::1",
            "4000::1",
            "5f00::1",
            "fc00::1",
            "fe80::1",
            "ff00::1",
        ] {
            let address = address.parse().unwrap();
            assert!(!AddressPolicy::PublicInternet.permits(address), "{address}");
        }
        for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111", "2620:fe::fe"] {
            let address = address.parse().unwrap();
            assert!(AddressPolicy::PublicInternet.permits(address), "{address}");
        }
    }

    #[test]
    fn production_constructor_requires_https_and_public_literal() {
        let name = EndpointName::new("api").unwrap();
        assert!(matches!(
            EndpointCapability::https(name.clone(), "http://example.com/api"),
            Err(ConfigError::HttpsRequired)
        ));
        assert!(matches!(
            EndpointCapability::https(name, "https://169.254.169.254/latest"),
            Err(ConfigError::AddressNotAllowed)
        ));
    }

    #[test]
    fn encoded_path_separators_are_rejected() {
        assert!(!valid_normalized_path("/api/a%2Fb"));
        assert!(!valid_normalized_path("/api/a%255cb"));
        assert!(valid_normalized_path("/api/a%20b"));
    }
}
