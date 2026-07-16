use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{
    CONNECTION, CONTENT_ENCODING, CONTENT_LANGUAGE, CONTENT_LENGTH, CONTENT_LOCATION, CONTENT_TYPE,
    HeaderMap, HeaderName, HeaderValue, LOCATION, TRANSFER_ENCODING,
};
use reqwest::{Method, StatusCode, Url};
use tokio::sync::Semaphore;

use crate::capability::{ClientConfig, HttpLimits, RedirectPolicy};
use crate::dns::{GuardedResolver, ResolverFailureKind, resolver_failure_kind};
use crate::{
    CancellationToken, ConfigError, EndpointCapability, EndpointName, ExecuteError, Header,
    HttpMethod, HttpRequest, HttpResponse, LimitKind, RequestTimeouts, RequestViolation,
    TimeoutKind,
};

const MAX_CONNECT_TIMEOUT_POOLS: usize = 8;

#[derive(Clone)]
pub struct HttpClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    resolver: Arc<GuardedResolver>,
    transport_settings: TransportSettings,
    transports: Mutex<TransportCache>,
    endpoints: BTreeMap<EndpointName, EndpointCapability>,
    limits: HttpLimits,
    redirects: RedirectPolicy,
    overall_timeout: std::time::Duration,
    connect_timeout: std::time::Duration,
    in_flight: Semaphore,
}

struct TransportSettings {
    response_header_limit: u32,
    pool_idle_timeout: Duration,
    pool_max_idle_per_host: usize,
}

struct TransportCache {
    clients: BTreeMap<Duration, reqwest::Client>,
    recency: VecDeque<Duration>,
}

impl TransportCache {
    fn new(connect_timeout: Duration, client: reqwest::Client) -> Self {
        Self {
            clients: BTreeMap::from([(connect_timeout, client)]),
            recency: VecDeque::from([connect_timeout]),
        }
    }

    fn get(&mut self, connect_timeout: Duration) -> Option<reqwest::Client> {
        let client = self.clients.get(&connect_timeout).cloned()?;
        self.touch(connect_timeout);
        Some(client)
    }

    fn insert(&mut self, connect_timeout: Duration, client: reqwest::Client) {
        while self.clients.len() >= MAX_CONNECT_TIMEOUT_POOLS {
            let Some(oldest) = self.recency.pop_front() else {
                break;
            };
            self.clients.remove(&oldest);
        }
        self.clients.insert(connect_timeout, client);
        self.touch(connect_timeout);
    }

    fn touch(&mut self, connect_timeout: Duration) {
        if let Some(index) = self
            .recency
            .iter()
            .position(|candidate| *candidate == connect_timeout)
        {
            self.recency.remove(index);
        }
        self.recency.push_back(connect_timeout);
    }
}

impl HttpClient {
    pub fn new(config: ClientConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        let resolver = Arc::new(GuardedResolver::new(
            &config.endpoints,
            config.timeouts.connect,
        ));
        let response_header_limit = u32::try_from(config.limits.max_response_header_bytes)
            .map_err(|_| ConfigError::InvalidLimit)?;
        let transport_settings = TransportSettings {
            response_header_limit,
            pool_idle_timeout: config.pool.idle_timeout,
            pool_max_idle_per_host: config.pool.max_idle_per_host,
        };
        let transport = build_transport(
            Arc::clone(&resolver),
            &transport_settings,
            config.timeouts.connect,
        )?;
        let endpoints = config
            .endpoints
            .into_iter()
            .map(|endpoint| (endpoint.name().clone(), endpoint))
            .collect();

        Ok(Self {
            inner: Arc::new(ClientInner {
                resolver,
                transport_settings,
                transports: Mutex::new(TransportCache::new(config.timeouts.connect, transport)),
                endpoints,
                limits: config.limits,
                redirects: config.redirects,
                overall_timeout: config.timeouts.overall,
                connect_timeout: config.timeouts.connect,
                in_flight: Semaphore::new(config.max_in_flight),
            }),
        })
    }

    pub async fn execute(
        &self,
        request: HttpRequest,
        cancellation: &CancellationToken,
    ) -> Result<HttpResponse, ExecuteError> {
        self.execute_with_timeouts(
            request,
            cancellation,
            RequestTimeouts {
                connect: self.inner.connect_timeout,
                overall: self.inner.overall_timeout,
            },
        )
        .await
    }

    pub async fn execute_with_timeouts(
        &self,
        request: HttpRequest,
        cancellation: &CancellationToken,
        timeouts: RequestTimeouts,
    ) -> Result<HttpResponse, ExecuteError> {
        self.validate_request_timeouts(timeouts)?;
        if cancellation.is_cancelled() {
            return Err(ExecuteError::Cancelled);
        }

        let execution = self.execute_inner(request, timeouts.connect);
        let deadline = tokio::time::sleep(timeouts.overall);
        tokio::pin!(execution);
        tokio::pin!(deadline);

        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(ExecuteError::Cancelled),
            result = &mut execution => result,
            _ = &mut deadline => Err(ExecuteError::OverallTimeout),
        }
    }

    fn validate_request_timeouts(&self, timeouts: RequestTimeouts) -> Result<(), ExecuteError> {
        if timeouts.connect.is_zero()
            || timeouts.overall.is_zero()
            || timeouts.connect > timeouts.overall
        {
            return Err(ExecuteError::InvalidTimeouts);
        }
        for (kind, requested, limit) in [
            (
                TimeoutKind::Connect,
                timeouts.connect,
                self.inner.connect_timeout,
            ),
            (
                TimeoutKind::Overall,
                timeouts.overall,
                self.inner.overall_timeout,
            ),
        ] {
            if requested > limit {
                return Err(ExecuteError::TimeoutLimitExceeded {
                    kind,
                    limit_ms: u64::try_from(limit.as_millis()).unwrap_or(u64::MAX),
                });
            }
        }
        Ok(())
    }

    async fn execute_inner(
        &self,
        request: HttpRequest,
        connect_timeout: Duration,
    ) -> Result<HttpResponse, ExecuteError> {
        let _permit = self
            .inner
            .in_flight
            .acquire()
            .await
            .expect("HTTP client semaphore is never closed");
        let endpoint = self
            .inner
            .endpoints
            .get(&request.endpoint)
            .cloned()
            .ok_or_else(|| ExecuteError::UnknownEndpoint {
                endpoint: request.endpoint.clone(),
            })?;
        let transport = self.transport_for_connect_timeout(connect_timeout, endpoint.name())?;
        let mut prepared = self.prepare_request(request)?;
        let mut current_url = endpoint.build_url(&prepared.path_segments, &prepared.query)?;
        self.check_url_limit(&current_url)?;
        let mut current_endpoint = endpoint;
        let mut redirects_followed = 0;
        let mut visited = BTreeSet::from([current_url.as_str().to_owned()]);

        loop {
            let response = self
                .send_once(&transport, &current_url, &prepared, &current_endpoint)
                .await?;
            let status = response.status();
            let response_headers =
                collect_response_headers(response.headers(), &self.inner.limits)?;

            if is_redirect_status(status) {
                let max_hops = match self.inner.redirects {
                    RedirectPolicy::Deny => return Err(ExecuteError::RedirectDenied),
                    RedirectPolicy::Follow { max_hops } => max_hops,
                };
                if redirects_followed >= max_hops {
                    return Err(ExecuteError::RedirectLimitExceeded { limit: max_hops });
                }
                let mut next_url = redirect_target(&current_url, &response_headers)?;
                next_url.set_fragment(None);
                self.check_url_limit(&next_url)?;
                let next_endpoint = self
                    .inner
                    .endpoints
                    .values()
                    .find(|endpoint| endpoint.matches_url(&next_url))
                    .cloned()
                    .ok_or(ExecuteError::RedirectDestinationNotAllowed)?;
                if !visited.insert(next_url.as_str().to_owned()) {
                    return Err(ExecuteError::InvalidRedirect);
                }

                let trust_boundary_changed = current_endpoint.name() != next_endpoint.name();
                if trust_boundary_changed && !prepared.body.is_empty() {
                    return Err(ExecuteError::RedirectCredentialLeakPrevented);
                }
                apply_redirect_method(status, &mut prepared);
                if trust_boundary_changed {
                    // Unknown custom headers can carry credentials, so crossing
                    // a named capability drops every caller-supplied header.
                    prepared.headers.clear();
                }

                current_url = next_url;
                current_endpoint = next_endpoint;
                redirects_followed += 1;
                continue;
            }

            let body = collect_response_body(
                response,
                self.inner.limits.max_response_body_bytes,
                current_endpoint.name(),
            )
            .await?;
            return Ok(HttpResponse {
                status: status.as_u16(),
                headers: response_headers,
                body,
                final_endpoint: current_endpoint.name().clone(),
                redirects_followed,
            });
        }
    }

    fn prepare_request(&self, request: HttpRequest) -> Result<PreparedRequest, ExecuteError> {
        let limits = &self.inner.limits;
        if request.body.len() > limits.max_request_body_bytes {
            return Err(ExecuteError::LimitExceeded {
                kind: LimitKind::RequestBodyBytes,
                limit: limits.max_request_body_bytes,
            });
        }
        if matches!(request.method, HttpMethod::Get | HttpMethod::Head) && !request.body.is_empty()
        {
            return Err(ExecuteError::InvalidRequest(
                RequestViolation::BodyNotAllowedForMethod,
            ));
        }
        if request.headers.len() > limits.max_request_headers {
            return Err(ExecuteError::LimitExceeded {
                kind: LimitKind::RequestHeaderCount,
                limit: limits.max_request_headers,
            });
        }

        let mut header_bytes = 0usize;
        let mut headers = HeaderMap::new();
        for header in request.headers {
            header_bytes = header_bytes
                .checked_add(header.name.len())
                .and_then(|bytes| bytes.checked_add(header.value.len()))
                .ok_or(ExecuteError::LimitExceeded {
                    kind: LimitKind::RequestHeaderBytes,
                    limit: limits.max_request_header_bytes,
                })?;
            if header_bytes > limits.max_request_header_bytes {
                return Err(ExecuteError::LimitExceeded {
                    kind: LimitKind::RequestHeaderBytes,
                    limit: limits.max_request_header_bytes,
                });
            }
            let name = HeaderName::from_bytes(header.name.as_bytes())
                .map_err(|_| ExecuteError::InvalidRequest(RequestViolation::InvalidHeaderName))?;
            if forbidden_request_header(&name) {
                return Err(ExecuteError::InvalidRequest(
                    RequestViolation::ForbiddenHeader,
                ));
            }
            let value = HeaderValue::from_bytes(&header.value)
                .map_err(|_| ExecuteError::InvalidRequest(RequestViolation::InvalidHeaderValue))?;
            headers.append(name, value);
        }

        Ok(PreparedRequest {
            method: request.method.into(),
            path_segments: request.path_segments,
            query: request.query,
            headers,
            body: request.body,
        })
    }

    async fn send_once(
        &self,
        transport: &reqwest::Client,
        url: &Url,
        request: &PreparedRequest,
        endpoint: &EndpointCapability,
    ) -> Result<reqwest::Response, ExecuteError> {
        transport
            .request(request.method.clone(), url.clone())
            .headers(request.headers.clone())
            .body(request.body.clone())
            .send()
            .await
            .map_err(|error| map_transport_error(error, endpoint.name()))
    }

    fn transport_for_connect_timeout(
        &self,
        connect_timeout: Duration,
        endpoint: &EndpointName,
    ) -> Result<reqwest::Client, ExecuteError> {
        let mut transports = self
            .inner
            .transports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(transport) = transports.get(connect_timeout) {
            return Ok(transport);
        }
        let transport = build_transport(
            Arc::clone(&self.inner.resolver),
            &self.inner.transport_settings,
            connect_timeout,
        )
        .map_err(|_| ExecuteError::TransportFailed {
            endpoint: endpoint.clone(),
        })?;
        transports.insert(connect_timeout, transport.clone());
        Ok(transport)
    }

    fn check_url_limit(&self, url: &Url) -> Result<(), ExecuteError> {
        if url.as_str().len() > self.inner.limits.max_url_bytes {
            return Err(ExecuteError::LimitExceeded {
                kind: LimitKind::UrlBytes,
                limit: self.inner.limits.max_url_bytes,
            });
        }
        Ok(())
    }
}

fn build_transport(
    resolver: Arc<GuardedResolver>,
    settings: &TransportSettings,
    connect_timeout: Duration,
) -> Result<reqwest::Client, ConfigError> {
    reqwest::Client::builder()
        .tls_backend_rustls()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .no_proxy()
        .connect_timeout(connect_timeout)
        .pool_idle_timeout(settings.pool_idle_timeout)
        .pool_max_idle_per_host(settings.pool_max_idle_per_host)
        .http2_max_header_list_size(settings.response_header_limit)
        .dns_resolver(resolver)
        .build()
        .map_err(|_| ConfigError::ClientBuildFailed)
}

struct PreparedRequest {
    method: Method,
    path_segments: Vec<String>,
    query: Vec<crate::QueryParameter>,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl From<HttpMethod> for Method {
    fn from(method: HttpMethod) -> Self {
        match method {
            HttpMethod::Get => Self::GET,
            HttpMethod::Head => Self::HEAD,
            HttpMethod::Post => Self::POST,
            HttpMethod::Put => Self::PUT,
            HttpMethod::Patch => Self::PATCH,
            HttpMethod::Delete => Self::DELETE,
            HttpMethod::Options => Self::OPTIONS,
        }
    }
}

fn forbidden_request_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "upgrade"
    )
}

fn is_redirect_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

fn redirect_target(current_url: &Url, headers: &[Header]) -> Result<Url, ExecuteError> {
    let mut locations = headers
        .iter()
        .filter(|header| header.name == LOCATION.as_str());
    let location = locations.next().ok_or(ExecuteError::InvalidRedirect)?;
    if locations.next().is_some() {
        return Err(ExecuteError::InvalidRedirect);
    }
    let location =
        std::str::from_utf8(&location.value).map_err(|_| ExecuteError::InvalidRedirect)?;
    current_url
        .join(location)
        .map_err(|_| ExecuteError::InvalidRedirect)
}

fn apply_redirect_method(status: StatusCode, request: &mut PreparedRequest) {
    let rewrite_to_get = (status == StatusCode::SEE_OTHER
        && !matches!(request.method, Method::GET | Method::HEAD))
        || (matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND)
            && request.method == Method::POST);
    if !rewrite_to_get {
        return;
    }

    request.method = Method::GET;
    request.body.clear();
    for header in [
        CONTENT_LENGTH,
        CONTENT_TYPE,
        CONTENT_ENCODING,
        CONTENT_LANGUAGE,
        CONTENT_LOCATION,
        TRANSFER_ENCODING,
        CONNECTION,
    ] {
        request.headers.remove(header);
    }
}

fn collect_response_headers(
    headers: &HeaderMap,
    limits: &HttpLimits,
) -> Result<Vec<Header>, ExecuteError> {
    let mut result = Vec::new();
    let mut bytes = 0usize;
    for name in headers.keys() {
        for value in headers.get_all(name) {
            if result.len() >= limits.max_response_headers {
                return Err(ExecuteError::LimitExceeded {
                    kind: LimitKind::ResponseHeaderCount,
                    limit: limits.max_response_headers,
                });
            }
            bytes = bytes
                .checked_add(name.as_str().len())
                .and_then(|count| count.checked_add(value.as_bytes().len()))
                .ok_or(ExecuteError::LimitExceeded {
                    kind: LimitKind::ResponseHeaderBytes,
                    limit: limits.max_response_header_bytes,
                })?;
            if bytes > limits.max_response_header_bytes {
                return Err(ExecuteError::LimitExceeded {
                    kind: LimitKind::ResponseHeaderBytes,
                    limit: limits.max_response_header_bytes,
                });
            }
            result.push(Header::new(
                name.as_str().to_owned(),
                value.as_bytes().to_vec(),
            ));
        }
    }
    result.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.value.cmp(&right.value))
    });
    Ok(result)
}

async fn collect_response_body(
    mut response: reqwest::Response,
    limit: usize,
    endpoint: &EndpointName,
) -> Result<Vec<u8>, ExecuteError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(ExecuteError::LimitExceeded {
            kind: LimitKind::ResponseBodyBytes,
            limit,
        });
    }

    let initial_capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0)
        .min(limit);
    let mut body = Vec::with_capacity(initial_capacity);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| map_transport_error(error, endpoint))?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(ExecuteError::LimitExceeded {
                kind: LimitKind::ResponseBodyBytes,
                limit,
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn map_transport_error(error: reqwest::Error, endpoint: &EndpointName) -> ExecuteError {
    match resolver_failure_kind(&error) {
        Some(ResolverFailureKind::LookupFailed) => ExecuteError::DnsResolutionFailed {
            endpoint: endpoint.clone(),
        },
        Some(ResolverFailureKind::AddressPolicyDenied) => ExecuteError::AddressPolicyDenied {
            endpoint: endpoint.clone(),
        },
        None if error.is_timeout() => ExecuteError::ConnectTimeout {
            endpoint: endpoint.clone(),
        },
        None if error.is_connect() => ExecuteError::ConnectFailed {
            endpoint: endpoint.clone(),
        },
        None => ExecuteError::TransportFailed {
            endpoint: endpoint.clone(),
        },
    }
}
